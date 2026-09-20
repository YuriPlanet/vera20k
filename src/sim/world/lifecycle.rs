//! Ordered ObjectClass-style lifecycle authority.
//!
//! Owns the independent Reveal, Conceal/Limbo, UnInit, LogicVector membership,
//! and pending-delete transitions.  Upper-layer work is emitted as ordered data;
//! this module never depends on render, UI, sidebar, audio, or net.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::{CellRect, resolve_reservation_real_cell, scan_cell_rect};
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::intern::InternedId;
use crate::sim::lifecycle_request::LifecycleRequest;
use crate::sim::map::bridge_topology::BRIDGE_DECK_HEIGHT_LEPTONS;
use crate::sim::occupancy::{
    BUILDING_OCCUPATION_BIT, CellListInsertion, OBJECT_OCCUPATION_BIT, VEHICLE_OCCUPATION_BIT,
    air_spatial_bucket_index, air_spatial_tracks_entity, cell_list_layer_for_entity,
    entity_occupancy_cells, infantry_raw_occupation_mask,
};
use crate::sim::passenger::PassengerRole;
use crate::sim::projectile::ProjectileTarget;
use crate::util::fixed_math::SimFixed;
use crate::util::lepton::{LEPTONS_PER_LEVEL, ground_height_leptons};

use super::Simulation;
use super::substrate::ObjectKind;

/// The control value `DispatchPointerExpiredCleanup @ 0x007258D0` forwards to
/// every listener's `PointerExpired` slot (`vt+0x28`), i.e. the third argument
/// of `TechnoClass::PointerExpired @ 0x007077C0`.
///
/// Two production callers, and they disagree on the value:
/// * `ObjectClass__UnInit @ 0x005F65F0` dispatches with **1** at `0x005F6616` —
///   the object is going away, so every reference is dropped unconditionally.
/// * `ObjectClass::Detach_All(bool)` (vtable `+0xDC`, `0x005F5280`; the
///   `FootClass` override is `0x004D9720`) forwards its own argument, and both
///   cloak callers pass **0**: `TechnoClass::StartCloaking @ 0x00703770` and
///   the `CloakState 1 -> 2` arm of `TechnoClass::CloakingTick @ 0x006FB740`.
///
/// A control of 0 changes three things inside the receiver body: it runs the
/// `allowClear` sensor test (`0x00707994 CALL 0x004870D0`,
/// `CellClass::SensorCountForHouse`), it exempts a receiver whose own house
/// owns the expiring object from the Target clear (`0x007079B7..0x007079CB`),
/// and it skips the `+0x500` / `+0x218` / CaptureManager block opened at
/// `0x00707AE7`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PointerExpiryControl {
    /// `Detach_All(false)` — the expiring object survives.
    DetachAll,
    /// `ObjectClass::UnInit` — the expiring object is being removed.
    Uninit,
}

/// Borrowed map authority carried through one synchronous receiver lifecycle
/// tree, including nested survivor Reveal as well as UnInit. Ordinary entry
/// points use the Simulation-owned terrain; combat uses this context while
/// that same terrain is staged outside `Simulation`.
/// Native clear/repair routines always query the global MapClass: Unit clear
/// `0x00744210` (RemoveContent `0x0047EA90`, vt+0xF4), Aircraft clear
/// `0x005F6120` (vtable `0x007E22A4`), Building reservation clear `0x004561F0`,
/// and Bullet expiry `0x004684E0`. Source: active gamemd.exe bodies and vtables.
/// An absent resident field during receiver execution is not an empty map.
#[derive(Clone, Copy, Default)]
pub(crate) struct UninitContext<'a> {
    terrain: Option<&'a crate::map::resolved_terrain::ResolvedTerrainGrid>,
    rules: Option<&'a RuleSet>,
    bridge_state: Option<&'a crate::sim::bridge_state::BridgeRuntimeState>,
}

impl<'a> UninitContext<'a> {
    pub(crate) const fn with_terrain(
        terrain: Option<&'a crate::map::resolved_terrain::ResolvedTerrainGrid>,
    ) -> Self {
        Self {
            terrain,
            rules: None,
            bridge_state: None,
        }
    }

    pub(crate) fn with_terrain_and_rules(
        terrain: Option<&'a crate::map::resolved_terrain::ResolvedTerrainGrid>,
        rules: &'a RuleSet,
    ) -> Self {
        let mut context = Self::with_terrain(terrain);
        context.rules = Some(rules);
        context
    }

    pub(crate) const fn with_rules(rules: &'a RuleSet) -> Self {
        Self {
            terrain: None,
            rules: Some(rules),
            bridge_state: None,
        }
    }

    pub(crate) const fn terrain(
        self,
    ) -> Option<&'a crate::map::resolved_terrain::ResolvedTerrainGrid> {
        self.terrain
    }

    pub(crate) const fn with_bridge_state(
        mut self,
        bridge_state: Option<&'a crate::sim::bridge_state::BridgeRuntimeState>,
    ) -> Self {
        self.bridge_state = bridge_state;
        self
    }

    pub(crate) const fn rules(self) -> Option<&'a RuleSet> {
        self.rules
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlacementEvidence {
    RejectedEarly,
    MarkFailed,
    MarkSucceeded,
    /// BuildingClass map-reader upgrade construction reaches virtual Unlimbo
    /// at the host coordinate, but the distinct upgrade object attaches to its
    /// parent rather than competing for the parent's footprint. Native:
    /// `BuildingClass::ReadFromINI @ 0x0044F820`, call at `0x0044FDB9`.
    AttachedUpgrade,
    /// Run the modeled `ObjectClass::Reveal @ 0x005F4EC0` playfield admission
    /// and Mark(PUT) transaction. Production Unit Unlimbo uses this after its
    /// class-specific exact-zero CanEnter admission; fixed/runtime constructors
    /// use it so a later placement rejection can retain the spent constructor.
    EvaluateMark,
    /// The concrete `UnitClass::Can_Enter_Cell @ 0x0073F0A0` call made by
    /// `ObjectClass::Unlimbo @ 0x005F4EC0` returned exact zero for the named
    /// CellClass occupation plane. Carrying the selected plane prevents a
    /// caller that already proved the native predicate (notably naval factory
    /// delivery) from evaluating it twice while still letting the common
    /// reveal boundary establish OnBridge and the deck Z before Mark(PUT).
    UnitCanEnterExactZero {
        layer: crate::sim::movement::locomotor::MovementLayer,
    },
}

pub(super) fn building_base_reservation_rect(
    rx: u16,
    ry: u16,
    foundation: &str,
    spacing: i32,
) -> CellRect {
    let (width, height) = crate::rules::foundation::foundation_dimensions(foundation);
    CellRect::new(
        native_base_reservation_start(rx, spacing),
        native_base_reservation_start(ry, spacing),
        i32::from(width).wrapping_add(spacing.wrapping_mul(2)),
        i32::from(height).wrapping_add(spacing.wrapping_mul(2)),
    )
}

fn native_base_reservation_start(anchor: u16, spacing: i32) -> i32 {
    i32::from(i32::from(anchor).wrapping_sub(spacing) as i16)
}

fn packed_reservation_coord(x: i32, y: i32) -> u32 {
    u32::from(x as i16 as u16) | (u32::from(y as i16 as u16) << 16)
}

fn base_reservation_perimeter_rect(rect: CellRect) -> CellRect {
    CellRect::new(
        rect.x.wrapping_sub(1),
        rect.y.wrapping_sub(1),
        rect.width.wrapping_add(3),
        rect.height.wrapping_add(3),
    )
}

/// The "no valid cell here" sentinel `BulletClass::PointerExpired` compares its
/// truncated target cell against, at `0x0046856E` and `0x0046857C`. Both
/// `DAT_0089DDF0`/`DAT_0089DDF2` words read zero in the image, and the only
/// writer in the program — the four-instruction routine at `0x00466270` — zeroes
/// them, so the sentinel is the cell (0, 0) rather than a general off-map test.
pub(crate) const NULL_TARGET_CELL_SENTINEL: (u16, u16) = (0, 0);

/// Cell selected after the represented ObjectClass virtual `GetCoords` result
/// is truncated from world leptons. BuildingClass shifts its stored NW anchor
/// to the geometric foundation center before that truncation.
///
/// The `None` arm is unreachable at retail map sizes rather than by
/// construction. `position.rx`/`ry` are `u16` and the in-cell offsets stay in
/// `0..256`, but the Structure arm adds `(width - 1) * 128` leptons before
/// truncating, so a multi-cell structure at the very top of the `u16` cell range
/// would overflow and return `None`. No retail map approaches that bound.
/// Native's own guard against a bad coordinate is the (0, 0) sentinel, checked
/// at the callback instead, so this arm is not the sentinel's analogue.
fn object_get_coords_cell(entity: &crate::sim::game_entity::GameEntity) -> Option<(u16, u16)> {
    let mut world_x = i32::from(entity.position.rx)
        .wrapping_mul(crate::sim::cell_kernel::LEPTONS_PER_CELL)
        .wrapping_add(entity.position.sub_x.to_num::<i32>());
    let mut world_y = i32::from(entity.position.ry)
        .wrapping_mul(crate::sim::cell_kernel::LEPTONS_PER_CELL)
        .wrapping_add(entity.position.sub_y.to_num::<i32>());
    if entity.category == EntityCategory::Structure {
        let (width, height) = crate::rules::foundation::foundation_dimensions(&entity.foundation);
        world_x = world_x.wrapping_add(i32::from(width.saturating_sub(1)).wrapping_mul(128));
        world_y = world_y.wrapping_add(i32::from(height.saturating_sub(1)).wrapping_mul(128));
    }
    Some((
        u16::try_from(crate::sim::cell_kernel::world_to_cell_trunc(world_x)).ok()?,
        u16::try_from(crate::sim::cell_kernel::world_to_cell_trunc(world_y)).ok()?,
    ))
}

pub(super) fn building_base_reservation_repair_rect(
    rx: u16,
    ry: u16,
    foundation_width: u16,
    foundation_height: u16,
    spacing: i32,
) -> CellRect {
    let five_times_spacing = spacing.wrapping_mul(5);
    let primary_x = native_base_reservation_start(rx, spacing);
    let primary_y = native_base_reservation_start(ry, spacing);
    CellRect::new(
        primary_x.wrapping_sub(spacing),
        primary_y.wrapping_sub(spacing),
        i32::from(foundation_width).wrapping_add(five_times_spacing),
        i32::from(foundation_height).wrapping_add(five_times_spacing),
    )
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RevealPosition {
    /// Isometric map-cell coordinate, not a screen axis.
    pub rx: u16,
    /// Isometric map-cell coordinate, not a screen axis.
    pub ry: u16,
    /// Current Rust height level, not pixels or leptons.
    pub z: u8,
    /// Lepton offset inside the cell (256 leptons per cell).
    pub sub_x: SimFixed,
    /// Lepton offset inside the cell (256 leptons per cell).
    pub sub_y: SimFixed,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RevealRequest {
    pub position: RevealPosition,
    pub placement: PlacementEvidence,
    /// Caller-supplied result of the still-blocked native type/mode gate.
    pub logic_eligible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevealFailure {
    MissingObject,
    RejectedEarly,
    MarkFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevealOutcome {
    Revealed { logic_registered: bool },
    AlreadyRevealed,
    Failed(RevealFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConcealOutcome {
    Concealed,
    AlreadyConcealed,
    MissingOrDead,
}

/// Release-visible lifecycle handoffs.  Consumers may be temporarily no-op,
/// but the stream preserves the verified native relative ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecycleOutput {
    RevealDisplay { stable_id: u64 },
    DisplayRemove { stable_id: u64 },
    DetachAttachedAnims { stable_id: u64 },
    StopVoc { stable_id: u64 },
    DirtyTacticalRect { stable_id: u64 },
    ClearDrawnState { stable_id: u64 },
    ClearRedraw { stable_id: u64 },
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecycleTestEvent {
    RevealLimboCleared,
    RevealCoordinatesCommitted,
    MarkPut,
    RawOccupationListLinked,
    HiddenOccupationEntered,
    BaseReservationMarked,
    RawOccupationMarked,
    CellMarked,
    BuildConstAppended {
        stable_id: u64,
    },
    BasePlanFilled {
        stable_id: u64,
    },
    RevealDisplayBoundary,
    LogicAppended,
    LogicMembershipSet,
    ConcealDeselected,
    ConcealDestroyNotifyBoundary {
        stable_id: u64,
        object_alive: bool,
        cell_marked: bool,
        resolvable: bool,
    },
    ConcealAlreadyLimboReturn {
        stable_id: u64,
        object_alive: bool,
        resolvable: bool,
    },
    BaseReservationCleared,
    RawOccupationListUnlinked,
    HiddenOccupationExited,
    RawOccupationCleared,
    ConcealUnmarked,
    ConcealDisplayBoundary,
    ConcealAnimBoundary,
    ConcealVocBoundary,
    ConcealLogicRemoved,
    ConcealDirtyTacticalRectBoundary,
    ConcealClearDrawnStateBoundary,
    ConcealLimboSet,
    ConcealClearRedrawBoundary,
    BreakSlot {
        slot: usize,
        target: Option<u64>,
    },
    BreakSenderCleared {
        target: u64,
    },
    BreakReceiverClassEffect {
        target: u64,
    },
    BreakReceiverCleared {
        target: u64,
    },
    UninitClassPre {
        stable_id: u64,
    },
    UninitRemovalNotifyBoundary {
        stable_id: u64,
        object_alive: bool,
        cell_marked: bool,
        resolvable: bool,
    },
    UninitRemovalListenerVisited {
        expired_id: u64,
        listener_id: u64,
        target_alive: bool,
        target_in_limbo: bool,
    },
    ProjectilePointerExpiredVisited {
        expired_id: u64,
        projectile_id: u64,
        expired_resolvable: bool,
        projectile_resolvable: bool,
        source_id: u64,
        target: ProjectileTarget,
    },
    WaveDamageReceiverSelected {
        wave_id: u64,
        target_id: u64,
        scenario_rng_state: u64,
    },
    /// The FireAt transaction has committed every receiver/effect owned by
    /// this shot. Wave registration happens at this boundary, but the new
    /// Logic tail must not dispatch until all later pre-existing callbacks.
    CombatFireEffectsCommitted {
        attacker_id: u64,
        scenario_rng_state: u64,
    },
    UninitAliveCleared {
        stable_id: u64,
    },
    PostMortemKillBookkeeping {
        stable_id: u64,
    },
    PostMortemRadioBreakCompleted {
        stable_id: u64,
    },
    PostMortemDeselected {
        stable_id: u64,
    },
    PostMortemDestroyNotifyBoundary {
        stable_id: u64,
    },
    /// One visited listener of the live-detach targeting sweep, in the order
    /// the sweep visited it. Recorded for every listener that was pointed at
    /// the detaching object, so a test can pin the descending walk.
    DetachTargetingSweepVisited {
        detach_id: u64,
        listener_id: u64,
        restored: bool,
        target_cleared: bool,
    },
    PendingDeleteQueued {
        stable_id: u64,
    },
    BinaryFrameCommitted,
    PendingDeleteDrainStarted,
    FinalizedCommon {
        stable_id: u64,
    },
}

impl Simulation {
    #[cfg(test)]
    pub(crate) fn trace_lifecycle_for_test(&mut self, event: LifecycleTestEvent) {
        self.lifecycle_test_events.push(event);
    }

    #[cfg(test)]
    pub(crate) fn lifecycle_test_events_for_test(&self) -> &[LifecycleTestEvent] {
        &self.lifecycle_test_events
    }

    #[cfg(test)]
    pub(crate) fn clear_lifecycle_test_events_for_test(&mut self) {
        self.lifecycle_test_events.clear();
    }

    /// Adapt the current level-based placement API to the native input Coord.Z.
    /// UnitType 0x747EB0 / InfantryType 0x5247D0 clamp that input to the exact
    /// ground surface before Object Unlimbo 0x5F4EC0 commits XYZ and Mark(PUT).
    /// They do not add a bridge offset. Authored bridge placement supplies its
    /// deck coordinate before that clamp; translate our coarse deck request here.
    /// This coarse API cannot recover all raw authored inputs (notably Unit
    /// input zero on negative terrain); the report records that caller residual.
    /// See docs/research/RAMP_UNIT_HEIGHT_GHIDRA_REPORT.md.
    fn grounded_reveal_z(
        &self,
        stable_id: u64,
        position: RevealPosition,
        context: UninitContext<'_>,
    ) -> Option<i32> {
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::movement::ground_pose::ground_surface_z_at;
        use crate::sim::movement::locomotor::MovementLayer;

        let entity = self.substrate.entities.get(stable_id)?;
        if !matches!(
            entity.category,
            EntityCategory::Unit | EntityCategory::Infantry
        ) || !entity.locomotor.as_ref().is_some_and(|loco| {
            matches!(
                loco.kind,
                LocomotorKind::Drive | LocomotorKind::Walk | LocomotorKind::Ship
            ) && loco.layer != MovementLayer::Air
        }) || entity.parachute_state.is_some()
            || entity.low_bridge_tube_state.is_some()
            || entity.tunnel_state.is_some()
            || entity.rocket_state.is_some()
            || entity.drop_pod_state.is_some()
        {
            // These owners still carry their own altitude/coordinate state.
            // In particular, attaching a parachute precedes ordinary Reveal.
            return None;
        }
        let terrain = context.terrain().or(self.resolved_terrain.as_ref())?;
        let xy = [
            i32::from(position.rx)
                .wrapping_mul(256)
                .wrapping_add(position.sub_x.to_num::<i32>()),
            i32::from(position.ry)
                .wrapping_mul(256)
                .wrapping_add(position.sub_y.to_num::<i32>()),
        ];
        let ground_z = ground_surface_z_at(xy, false, Some(terrain), None)?;
        let level = terrain
            .native_fixed_cell_index((xy[0] / 256) as i16, (xy[1] / 256) as i16)
            .map_or_else(
                || terrain.shared_cell_dummy().snapshot().level,
                |index| terrain.cells()[index].level as i8,
            );
        let input_z = if entity.on_bridge && i32::from(position.z as i8) == i32::from(level) + 4 {
            // VERA input adapter: retain the ramp remainder that the existing
            // coarse bridge-level API cannot carry. Native clamp remains max.
            ground_z.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS)
        } else {
            i32::from(position.z as i8).wrapping_mul(LEPTONS_PER_LEVEL as i32)
        };
        Some(input_z.max(ground_z))
    }

    fn current_reveal_position(&self, stable_id: u64) -> Option<RevealPosition> {
        self.substrate
            .entities
            .get(stable_id)
            .map(|entity| RevealPosition {
                rx: entity.position.rx,
                ry: entity.position.ry,
                z: entity.position.z,
                sub_x: entity.position.sub_x,
                sub_y: entity.position.sub_y,
            })
    }

    fn raw_occupation_cell_facts(
        &self,
        position: RevealPosition,
        context: UninitContext<'_>,
    ) -> (i32, i32, bool) {
        let Some(terrain_cell) = context
            .terrain()
            .or(self.resolved_terrain.as_ref())
            .and_then(|terrain| terrain.cell(position.rx, position.ry))
        else {
            return (0, 0, false);
        };
        let ground_level = i32::from(terrain_cell.level as i8);
        let world_x = i32::from(position.rx)
            .wrapping_mul(crate::sim::cell_kernel::LEPTONS_PER_CELL)
            .wrapping_add(position.sub_x.to_num::<i32>());
        let world_y = i32::from(position.ry)
            .wrapping_mul(crate::sim::cell_kernel::LEPTONS_PER_CELL)
            .wrapping_add(position.sub_y.to_num::<i32>());
        let ground_z = ground_height_leptons(
            terrain_cell.level,
            terrain_cell.slope_type,
            world_x,
            world_y,
        )
        .unwrap_or_else(|_| {
            i32::from(terrain_cell.level as i8).wrapping_mul(LEPTONS_PER_LEVEL as i32)
        });
        let live_structural_bridge = terrain_cell.bridge_facts.has_structural_bridge()
            && context
                .bridge_state
                .or(self.bridge_state.as_ref())
                .is_some_and(|state| state.is_bridge_walkable(position.rx, position.ry));
        (ground_level, ground_z, live_structural_bridge)
    }

    /// gamemd-derived: active YR `ObjectClass__Mark_Put @ 0x005F60A0` and
    /// `ObjectClass__Mark_Remove @ 0x005F6120` compare the signed absolute
    /// Object coordinate Z with exact ground Z plus the 416-lepton deck height.
    fn raw_occupation_reaches_deck(
        position: RevealPosition,
        exact_z_leptons: Option<i32>,
        ground_level: i32,
        ground_z: i32,
    ) -> bool {
        exact_z_leptons.map_or_else(
            || i32::from(position.z as i8) >= ground_level.wrapping_add(4),
            |object_z| object_z >= ground_z.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS),
        )
    }

    fn mark_common_raw_occupation(
        &mut self,
        stable_id: u64,
        category: EntityCategory,
        cells: &[(u16, u16)],
        position: RevealPosition,
        exact_z_leptons: Option<i32>,
        context: UninitContext<'_>,
    ) -> bool {
        match category {
            EntityCategory::Unit => {
                let (ground_level, ground_z, live_structural_bridge) =
                    self.raw_occupation_cell_facts(position, context);
                if Self::raw_occupation_reaches_deck(
                    position,
                    exact_z_leptons,
                    ground_level,
                    ground_z,
                ) && live_structural_bridge
                {
                    self.substrate.raw_cell_occupation.mark_deck(
                        position.rx,
                        position.ry,
                        VEHICLE_OCCUPATION_BIT,
                    );
                } else {
                    self.substrate.raw_cell_occupation.mark_ground(
                        position.rx,
                        position.ry,
                        VEHICLE_OCCUPATION_BIT,
                    );
                }
                true
            }
            EntityCategory::Infantry => {
                let Some(owner) = self.substrate.entities.get(stable_id).map(|e| e.owner()) else {
                    return false;
                };
                let (ground_level, ground_z, live_structural_bridge) =
                    self.raw_occupation_cell_facts(position, context);
                let mask = infantry_raw_occupation_mask(position.sub_x, position.sub_y);
                // Native: `InfantryClass::MarkCellOccupancy` @ `0x005217C0`
                // selects the deck only at/above the bridge plane and only
                // while `CellClass+0x140 & 0x100` holds. (`0x00743FC0` is
                // inside `UnitClass`, not `InfantryClass`.)
                if Self::raw_occupation_reaches_deck(
                    position,
                    exact_z_leptons,
                    ground_level,
                    ground_z,
                ) && live_structural_bridge
                {
                    self.substrate.raw_cell_occupation.mark_deck_infantry(
                        position.rx,
                        position.ry,
                        mask,
                        owner,
                    );
                } else {
                    self.substrate.raw_cell_occupation.mark_ground_infantry(
                        position.rx,
                        position.ry,
                        mask,
                        owner,
                    );
                }
                true
            }
            EntityCategory::Structure => {
                for &(rx, ry) in cells {
                    self.substrate
                        .raw_cell_occupation
                        .mark_ground(rx, ry, BUILDING_OCCUPATION_BIT);
                }
                !cells.is_empty()
            }
            EntityCategory::Aircraft => {
                let (ground_level, ground_z, live_structural_bridge) =
                    self.raw_occupation_cell_facts(position, context);
                if Self::raw_occupation_reaches_deck(
                    position,
                    exact_z_leptons,
                    ground_level,
                    ground_z,
                ) && live_structural_bridge
                {
                    self.substrate.raw_cell_occupation.mark_deck(
                        position.rx,
                        position.ry,
                        OBJECT_OCCUPATION_BIT,
                    );
                } else {
                    self.substrate.raw_cell_occupation.mark_ground(
                        position.rx,
                        position.ry,
                        OBJECT_OCCUPATION_BIT,
                    );
                }
                true
            }
        }
    }

    fn clear_common_raw_occupation(
        &mut self,
        _stable_id: u64,
        category: EntityCategory,
        cells: &[(u16, u16)],
        position: RevealPosition,
        exact_z_leptons: Option<i32>,
        context: UninitContext<'_>,
    ) -> bool {
        match category {
            EntityCategory::Unit => {
                let (ground_level, ground_z, _) = self.raw_occupation_cell_facts(position, context);
                if Self::raw_occupation_reaches_deck(
                    position,
                    exact_z_leptons,
                    ground_level,
                    ground_z,
                ) {
                    self.substrate.raw_cell_occupation.clear_deck(
                        position.rx,
                        position.ry,
                        VEHICLE_OCCUPATION_BIT,
                    );
                } else {
                    self.substrate.raw_cell_occupation.clear_ground(
                        position.rx,
                        position.ry,
                        VEHICLE_OCCUPATION_BIT,
                    );
                }
                true
            }
            EntityCategory::Structure => {
                for &(rx, ry) in cells {
                    self.substrate.raw_cell_occupation.clear_ground(
                        rx,
                        ry,
                        BUILDING_OCCUPATION_BIT,
                    );
                }
                !cells.is_empty()
            }
            EntityCategory::Aircraft => {
                let (ground_level, ground_z, live_structural_bridge) =
                    self.raw_occupation_cell_facts(position, context);
                if Self::raw_occupation_reaches_deck(
                    position,
                    exact_z_leptons,
                    ground_level,
                    ground_z,
                ) && live_structural_bridge
                {
                    self.substrate.raw_cell_occupation.clear_deck(
                        position.rx,
                        position.ry,
                        OBJECT_OCCUPATION_BIT,
                    );
                } else {
                    self.substrate.raw_cell_occupation.clear_ground(
                        position.rx,
                        position.ry,
                        OBJECT_OCCUPATION_BIT,
                    );
                }
                true
            }
            EntityCategory::Infantry => {
                let (ground_level, ground_z, _) = self.raw_occupation_cell_facts(position, context);
                let mask = infantry_raw_occupation_mask(position.sub_x, position.sub_y);
                // Native: InfantryClass::Unmark (+0x744170) picks its plane from
                // height alone, retaining the proven mark/unmark bridge-bit asymmetry.
                if Self::raw_occupation_reaches_deck(
                    position,
                    exact_z_leptons,
                    ground_level,
                    ground_z,
                ) {
                    self.substrate.raw_cell_occupation.clear_deck_infantry(
                        position.rx,
                        position.ry,
                        mask,
                    );
                } else {
                    self.substrate.raw_cell_occupation.clear_ground_infantry(
                        position.rx,
                        position.ry,
                        mask,
                    );
                }
                true
            }
        }
    }

    /// Compatibility convenience for already-admitted current-position callers.
    /// It still executes the complete result-bearing Reveal transaction.
    pub(crate) fn reveal(&mut self, stable_id: u64) -> RevealOutcome {
        if self.substrate.anims.contains_key(stable_id) {
            let registered = self.reveal_anim(stable_id);
            return RevealOutcome::Revealed {
                logic_registered: registered,
            };
        }
        if self.substrate.particle_systems.contains_key(stable_id) {
            let registered = self.reveal_particle_system(stable_id);
            return RevealOutcome::Revealed {
                logic_registered: registered,
            };
        }
        let Some(position) = self.current_reveal_position(stable_id) else {
            return RevealOutcome::Failed(RevealFailure::MissingObject);
        };
        self.try_reveal_entity(
            stable_id,
            RevealRequest {
                position,
                placement: PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            },
        )
    }

    /// ObjectClass::Reveal: clear limbo for the attempt, commit coordinates,
    /// Mark(PUT), expose display, then append eligible LogicClass membership.
    pub(crate) fn try_reveal_entity(
        &mut self,
        stable_id: u64,
        request: RevealRequest,
    ) -> RevealOutcome {
        self.try_reveal_entity_with_context(stable_id, request, UninitContext::default())
    }

    /// A nested receiver Reveal uses the same map authority as its enclosing
    /// destruction transaction. SellBuilding @ 0x00458060 still reaches the
    /// global MapClass membership writer in Techno Unlimbo @ 0x006F6CC0..6CFE.
    /// Source: active gamemd.exe call and instruction bodies.
    pub(crate) fn try_reveal_entity_with_context(
        &mut self,
        stable_id: u64,
        request: RevealRequest,
        context: UninitContext<'_>,
    ) -> RevealOutcome {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return RevealOutcome::Failed(RevealFailure::MissingObject);
        };
        if !entity.lifecycle.in_limbo {
            return RevealOutcome::AlreadyRevealed;
        }
        if entity.lifecycle.cell_marked || request.placement == PlacementEvidence::RejectedEarly {
            return RevealOutcome::Failed(RevealFailure::RejectedEarly);
        }
        if request.placement == PlacementEvidence::AttachedUpgrade
            && entity.structure_upgrade_link.is_none()
        {
            return RevealOutcome::Failed(RevealFailure::RejectedEarly);
        }
        if request.placement == PlacementEvidence::EvaluateMark
            && !self.reveal_position_is_in_playfield(request.position, context)
        {
            return RevealOutcome::Failed(RevealFailure::RejectedEarly);
        }

        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.lifecycle.in_limbo = false;
        }
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::RevealLimboCleared);

        let exact_z = self.grounded_reveal_z(stable_id, request.position, context);
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.position.rx = request.position.rx;
            entity.position.ry = request.position.ry;
            entity.position.z = request.position.z;
            entity.position.exact_z_leptons = exact_z;
            entity.position.sub_x = request.position.sub_x;
            entity.position.sub_y = request.position.sub_y;
        }
        // `TechnoClass::Unlimbo @ 0x006F6CFE` establishes the canonical
        // TechnoClass+0x3D5 byte from mode-one MapClass membership. Headless
        // fixtures have no MapClass authority, so they retain the constructor
        // default and their consumers explicitly leave the byte unenforced.
        self.establish_entity_playfield_membership_on_unlimbo(stable_id, context);
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::RevealCoordinatesCommitted);

        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::MarkPut);
        if request.placement == PlacementEvidence::MarkFailed {
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.lifecycle.in_limbo = true;
            }
            return RevealOutcome::Failed(RevealFailure::MarkFailed);
        }

        let attached_upgrade = request.placement == PlacementEvidence::AttachedUpgrade;
        if !attached_upgrade {
            if !self.mark_entity_put(stable_id, context) {
                if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                    entity.lifecycle.in_limbo = true;
                }
                return RevealOutcome::Failed(RevealFailure::MarkFailed);
            }
            if let Some(entity) = self.substrate.entities.get_mut(stable_id)
                && entity.spotlight_capable
                && entity.category == crate::map::entities::EntityCategory::Structure
                && entity.building_light.is_none()
            {
                // `BuildingClass::Unlimbo @ 0x00441187` constructs after placement succeeds.
                entity.building_light = Some(crate::sim::game_entity::BuildingLightRuntime {
                    behavior: 1,
                    target_id: None,
                });
            }
        }
        // TechnoUnlimbo6F6E65..AD runs this second mode-one query only
        // after successful Object Mark and the +90 alive gate. A failed Mark
        // must retain history. This precedes Foot4D722F's owner observation.
        // The dead Techno arm still returns success to Foot4D7184, so only
        // this writer is gated: ForceSlope, owner discovery and Sight follow.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.lifecycle.object_alive)
            && self.entity_playfield_membership_mode_one(
                stable_id,
                context.terrain().or(self.resolved_terrain.as_ref()),
            ) == Some(false)
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            entity.discovery.discovered_by_current_house = false;
        }
        // FootClass::Unlimbo @ 0x004D7170 dispatches active Drive/Ship
        // Force_Slope at 0x004D71A9 only after TechnoClass placement succeeds.
        // This precedes display/Logic exposure and must not run on either
        // failed placement path above.
        let reveal_slope = self.substrate.entities.get(stable_id).and_then(|entity| {
            context
                .terrain()
                .or(self.resolved_terrain.as_ref())?
                .cell(entity.position.rx, entity.position.ry)
                .map(|cell| cell.slope_type)
        });
        if let Some(sampled_slope) = reveal_slope
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            crate::sim::movement::slope_transition::snap_after_successful_unlimbo(
                entity,
                sampled_slope,
                self.session.binary_frame,
            );
        }
        // ObjectUnlimbo5F4FB4 Mark/CellPUT has already run. Foot4D722F calls
        // +198(owner) next; only afterward Infantry51E0EF clears +41B for
        // exactly Sight=0. Never move these producers before the Mark call.
        self.record_foot_owner_discovery(stable_id);
        if let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && entity.category == EntityCategory::Infantry
            && entity.sight_is_zero
        {
            entity.discovery.discovered_by_current_house = false;
        }
        if !self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.lifecycle.object_alive)
        {
            return RevealOutcome::Revealed {
                logic_registered: false,
            };
        }
        if !attached_upgrade {
            self.append_live_build_const(stable_id);
            self.refresh_waypoint_edge_from_committed_structure(stable_id);
            self.mark_building_base_reservation_with_arg(stable_id, false, context);
            self.fill_base_plan_from_successful_building_unlimbo(stable_id);
        }
        self.lifecycle_outputs
            .push(LifecycleOutput::RevealDisplay { stable_id });
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::RevealDisplayBoundary);

        let logic_registered = if request.logic_eligible {
            self.register_logic_object(stable_id)
        } else {
            false
        };
        RevealOutcome::Revealed { logic_registered }
    }

    /// The owner-receiver portion of Foot4D722F -> Techno6F4960. Constructor
    /// classification makes an admitted current-owner entry +41A=true; another
    /// owner writes the aggregate +41C. This is not the first foreign-current-
    /// viewer CellPUT arm: that still requires its Tag4/House1F4 continuation.
    /// Missing current-house/House state belongs to the admitted headless
    /// substrate, not an invented native House0 or actor-owner fallback.
    fn record_foot_owner_discovery(&mut self, stable_id: u64) {
        let Some(current_house) = self.session.current_house else {
            return;
        };
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        if entity.category == EntityCategory::Structure {
            return;
        }
        let current = entity.owner() == current_house;
        let history = entity.discovery;
        if (current && history.discovered_by_current_house)
            || (!current && history.discovered_by_other_house)
        {
            return;
        }
        if current && !history.owned_by_current_house {
            // The required first-current-viewer receiver is intentionally not
            // asserted delivered from an unrepresented classification producer.
            // Initial Jumpjet continuation must not bypass that open boundary.
            return;
        }
        let Some(house) = self.houses.get(&entity.owner()) else {
            return;
        };
        let controlled = house.is_controlled_by_human(self.session.game_mode_nonzero);
        let entity = self
            .substrate
            .entities
            .get_mut(stable_id)
            .expect("selected Foot");
        if !current {
            // 6F4990 precedes base discovery and the mission inquiry.
            entity.discovery.discovered_by_other_house = true;
        }
        if !controlled
            && entity.mission.effective().known()
                == Some(crate::rules::mission_data::MissionType::Ambush)
        {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                entity,
                crate::sim::mission::MissionId::from_known(
                    crate::rules::mission_data::MissionType::Hunt,
                ),
            );
        }
        if current {
            entity.discovery.discovered_by_current_house = true;
            // +5778/+5779 invalidate building-derived power/radar/SpySat.
            // This Foot-only observation changes no building input; existing
            // normal consumer evaluation is retained, not duplicate dirty bytes.
        }
    }

    /// `BuildingClass::Unlimbo @ 0x00440580` calls
    /// `FUN_0042F260 @ 0x0042F260` at `0x0044159D..0x004415B3` for
    /// successful non-human BasePlan satisfaction.
    fn fill_base_plan_from_successful_building_unlimbo(&mut self, stable_id: u64) {
        let Some((owner, type_index, packed_cell, has_undeploy_target)) =
            self.substrate.entities.get(stable_id).and_then(|entity| {
                (entity.category == EntityCategory::Structure && entity.base_plan_type_index >= 0)
                    .then_some((
                        entity.owner(),
                        entity.base_plan_type_index,
                        crate::sim::base_plan::pack_base_plan_cell(
                            i32::from(entity.position.rx),
                            i32::from(entity.position.ry),
                        ),
                        entity.base_plan_has_undeploy_target,
                    ))
            })
        else {
            return;
        };
        let game_mode_nonzero = self.session.game_mode_nonzero;
        let Some(house) = self.houses.get_mut(&owner) else {
            return;
        };
        if house.is_controlled_by_human(game_mode_nonzero) {
            return;
        }
        let _filled = house
            .base_plan
            .fill_successful_building(type_index, packed_cell, has_undeploy_target)
            .is_some();
        #[cfg(test)]
        if _filled {
            self.trace_lifecycle_for_test(LifecycleTestEvent::BasePlanFilled { stable_id });
        }
    }

    /// `BuildingClass__Limbo @ 0x00445880` calls
    /// `FUN_0050A490 @ 0x0050A490` for BasePlan invalidation before the common
    /// Techno/Object concealment path. VERA has no map-editor runtime; every
    /// call to this lifecycle seam is therefore outside editor mode.
    fn invalidate_base_plan_from_building_limbo(&mut self, stable_id: u64) {
        let Some((owner, type_index, packed_cell, is_base_defense, in_limbo)) =
            self.substrate.entities.get(stable_id).and_then(|entity| {
                (entity.category == EntityCategory::Structure && entity.base_plan_type_index >= 0)
                    .then_some((
                        entity.owner(),
                        entity.base_plan_type_index,
                        crate::sim::base_plan::pack_base_plan_cell(
                            i32::from(entity.position.rx),
                            i32::from(entity.position.ry),
                        ),
                        entity.base_plan_is_defense,
                        entity.lifecycle.in_limbo,
                    ))
            })
        else {
            return;
        };
        if in_limbo {
            return;
        }
        if let Some(house) = self.houses.get_mut(&owner) {
            house.base_plan.invalidate_limbo_building(
                type_index,
                packed_cell,
                is_base_defense,
                self.session.game_mode_nonzero,
            );
        }
    }

    /// Refresh the owning house's navigation edge from a live, map-committed
    /// structure. Launch base-center authority remains the assigned start cell.
    pub(crate) fn refresh_waypoint_edge_from_committed_structure(&mut self, stable_id: u64) {
        let Some(bounds) = self.playfield_bounds else {
            return;
        };
        let Some((owner, anchor)) = self.substrate.entities.get(stable_id).and_then(|entity| {
            (entity.category == EntityCategory::Structure
                && entity.determines_waypoint_edge
                && entity.lifecycle.object_alive
                && !entity.lifecycle.in_limbo
                && entity.lifecycle.cell_marked)
                .then_some((entity.owner(), (entity.position.rx, entity.position.ry)))
        }) else {
            return;
        };
        let edge = crate::sim::house_state::determine_waypoint_edge(anchor, bounds);
        if let Some(house) = self.houses.get_mut(&owner) {
            house.waypoint_edge = edge;
        }
    }

    fn mark_entity_put(&mut self, stable_id: u64, context: UninitContext<'_>) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(stable_id) else {
            return false;
        };
        if entity.lifecycle.cell_marked {
            return false;
        }
        // Object5F58F7 publishes +74 before Foot4D37A6 queries virtual +78.
        // Jumpjet high-flying5F6B90 reads this intermediate value.
        entity.lifecycle.cell_marked = true;
        let cells = entity_occupancy_cells(entity);
        let layer = cell_list_layer_for_entity(entity);
        let sub_cell = if entity.category == EntityCategory::Infantry {
            entity.sub_cell
        } else {
            None
        };
        let insertion = CellListInsertion::from_category(entity.category);
        let category = entity.category;
        let foundation = entity.foundation.clone();
        let hidden_profile = entity.building_hidden_occupancy;
        let current_cell = (entity.position.rx, entity.position.ry);
        let raw_position = RevealPosition {
            rx: entity.position.rx,
            ry: entity.position.ry,
            z: entity.position.z,
            sub_x: entity.position.sub_x,
            sub_y: entity.position.sub_y,
        };
        let exact_z_leptons = entity.position.exact_z_leptons;
        let inside_transport = entity.passenger_role.is_inside_transport();
        let air_spatial_bucket =
            (!inside_transport && air_spatial_tracks_entity(entity)).then(|| {
                air_spatial_bucket_index(
                    entity.position.rx,
                    entity.position.ry,
                    self.session.map_width,
                    self.session.map_height,
                )
            });
        let order = self.substrate.next_occupancy_enter_order.next();

        if category == EntityCategory::Structure {
            let (width, height) = crate::rules::foundation::foundation_dimensions(&foundation);
            let mut intersections = Vec::with_capacity(usize::from(width) * usize::from(height));
            for dy in 0..height {
                for dx in 0..width {
                    let Some(rx) = current_cell.0.checked_add(dx) else {
                        continue;
                    };
                    let Some(ry) = current_cell.1.checked_add(dy) else {
                        continue;
                    };
                    intersections.push((rx, ry));
                }
            }
            if let Some(smudge_grid) = self.smudge_grid.as_mut() {
                smudge_grid.clear_intersecting_footprints(&intersections);
            }
            self.flush_smudge_dirty();
        }

        if !inside_transport {
            if let Some(layer) = layer {
                for &(rx, ry) in &cells {
                    self.substrate
                        .occupancy
                        .add(rx, ry, stable_id, layer, sub_cell, insertion);
                }
                if matches!(
                    category,
                    EntityCategory::Unit
                        | EntityCategory::Infantry
                        | EntityCategory::Structure
                        | EntityCategory::Aircraft
                ) {
                    #[cfg(test)]
                    self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationListLinked);
                    if category == EntityCategory::Structure
                        && layer == crate::sim::movement::locomotor::MovementLayer::Ground
                        && hidden_profile.is_some_and(|profile| {
                            self.substrate.hidden_occupation.enter_building(
                                current_cell,
                                &foundation,
                                profile,
                                Some((self.session.map_width, self.session.map_height)),
                            )
                        })
                    {
                        #[cfg(test)]
                        self.trace_lifecycle_for_test(LifecycleTestEvent::HiddenOccupationEntered);
                    }
                    if self.mark_common_raw_occupation(
                        stable_id,
                        category,
                        &cells,
                        raw_position,
                        exact_z_leptons,
                        context,
                    ) {
                        #[cfg(test)]
                        self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationMarked);
                    }
                }
                if category == EntityCategory::Unit {
                    self.substrate.cell_occupation.mark_vehicle_on_layer(
                        current_cell.0,
                        current_cell.1,
                        stable_id,
                        layer,
                    );
                }
            }
        }
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.occupancy_enter_order = order;
            match air_spatial_bucket {
                Some(bucket) if entity.air_spatial_bucket != Some(bucket) => {
                    entity.air_spatial_bucket = Some(bucket);
                    entity.air_spatial_enter_order = order;
                }
                Some(_) => {}
                None => {
                    entity.air_spatial_bucket = None;
                    entity.air_spatial_enter_order = 0;
                }
            }
            entity.lifecycle.cell_marked = true;
            entity.foot_occupation_enabled = true;
        }
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::CellMarked);
        self.substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.lifecycle.cell_marked)
    }

    /// Shared mode-one playfield gate from active
    /// `ObjectClass::Reveal @ 0x005F4EC0` before Mark(PUT). A normal constructed
    /// scenario always has MapClass bounds; unbounded synthetic fixtures retain
    /// their historical permissive behavior.
    fn reveal_position_is_in_playfield(
        &self,
        position: RevealPosition,
        context: UninitContext<'_>,
    ) -> bool {
        let Some(bounds) = self.playfield_bounds else {
            return true;
        };
        crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            (i32::from(position.rx), i32::from(position.ry)),
            Some(bounds),
            context.terrain().or(self.resolved_terrain.as_ref()),
        )
    }

    pub(crate) fn base_reservation_house_index(&self, owner: InternedId) -> Option<i32> {
        // Active YR Full_Init constructs the complete HouseClass array before
        // Terrain/Techno map sections reveal objects. Scenario loading preserves
        // that order, so Reveal writes the final house-index bit immediately.
        if self.session.house_order.is_empty() {
            return None;
        }
        let index = self
            .session
            .house_order
            .iter()
            .position(|registered| *registered == owner)
            .and_then(|index| i32::try_from(index).ok());
        debug_assert!(
            index.is_some(),
            "base-reservation owner must be present in ScenarioSession.house_order"
        );
        index
    }

    fn mark_building_base_reservation_with_arg(
        &mut self,
        stable_id: u64,
        repair_only: bool,
        context: UninitContext<'_>,
    ) -> bool {
        let Some((owner, rect)) = self.base_reservation_writer(stable_id) else {
            return false;
        };
        let Some(house_index) = self.base_reservation_house_index(owner) else {
            return false;
        };
        self.substrate.base_reservations.reserve_rect(
            context.terrain().or(self.resolved_terrain.as_ref()),
            rect,
            house_index,
        );
        if let Some(house) = self.houses.get_mut(&owner) {
            house
                .base_reservation
                .update_bounds(rect.x, rect.y, rect.width, rect.height);
        } else {
            debug_assert!(false, "base-reservation owner must have HouseState");
        }
        if !repair_only {
            self.update_base_reservation_perimeter_after_mark(owner, house_index, rect, context);
        }
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::BaseReservationMarked);
        true
    }

    fn update_base_reservation_perimeter_after_mark(
        &mut self,
        owner: InternedId,
        house_index: i32,
        rect: CellRect,
        context: UninitContext<'_>,
    ) {
        scan_cell_rect(base_reservation_perimeter_rect(rect), |x, y| {
            let neighbor_mask = self
                .substrate
                .base_reservations
                .house_reservation_neighbor_mask(
                    context.terrain().or(self.resolved_terrain.as_ref()),
                    x,
                    y,
                    house_index,
                );
            let packed = packed_reservation_coord(x, y);
            if let Some(house) = self.houses.get_mut(&owner) {
                match neighbor_mask {
                    0x0000_00ff => house.base_reservation.remove_perimeter_cell(packed),
                    1..=0x0000_00fe => house
                        .base_reservation
                        .append_perimeter_cell_if_absent(packed),
                    0 | u32::MAX => {}
                    _ => unreachable!("neighbor mask is eight bits or the absent-center sentinel"),
                }
            }
            true
        });
    }

    fn update_base_reservation_perimeter_after_clear(
        &mut self,
        owner: InternedId,
        house_index: i32,
        rect: CellRect,
        context: UninitContext<'_>,
    ) {
        scan_cell_rect(base_reservation_perimeter_rect(rect), |x, y| {
            let neighbor_mask = self
                .substrate
                .base_reservations
                .house_reservation_neighbor_mask(
                    context.terrain().or(self.resolved_terrain.as_ref()),
                    x,
                    y,
                    house_index,
                );
            let packed = packed_reservation_coord(x, y);
            match neighbor_mask {
                0x0000_00ff => {
                    if let Some(house) = self.houses.get_mut(&owner) {
                        house.base_reservation.remove_perimeter_cell(packed);
                    }
                }
                1..=0x0000_00fe => {
                    if let Some(house) = self.houses.get_mut(&owner) {
                        house
                            .base_reservation
                            .append_perimeter_cell_if_absent(packed);
                    }
                }
                0 | u32::MAX => {
                    if let Some(house) = self.houses.get_mut(&owner) {
                        house.base_reservation.remove_perimeter_cell(packed);
                    }
                    // Native resolves the center again before the extra clear.
                    self.substrate.base_reservations.clear(
                        context.terrain().or(self.resolved_terrain.as_ref()),
                        x,
                        y,
                        house_index,
                    );
                }
                _ => unreachable!("neighbor mask is eight bits or the absent-center sentinel"),
            }
            true
        });
    }

    fn base_reservation_writer(&self, stable_id: u64) -> Option<(InternedId, CellRect)> {
        let entity = self.substrate.entities.get(stable_id)?;
        let spacing = entity.base_reservation_spacing?;
        (entity.category == EntityCategory::Structure
            && entity.lifecycle.object_alive
            && !entity.lifecycle.in_limbo
            && entity.lifecycle.cell_marked
            && cell_list_layer_for_entity(entity)
                == Some(crate::sim::movement::locomotor::MovementLayer::Ground))
        .then(|| {
            (
                entity.owner(),
                building_base_reservation_rect(
                    entity.position.rx,
                    entity.position.ry,
                    &entity.foundation,
                    spacing,
                ),
            )
        })
    }

    fn clear_building_base_reservation_and_repair(
        &mut self,
        stable_id: u64,
        context: UninitContext<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        let Some(spacing) = entity.base_reservation_spacing else {
            return false;
        };
        if entity.category != EntityCategory::Structure
            || !entity.lifecycle.cell_marked
            || cell_list_layer_for_entity(entity)
                != Some(crate::sim::movement::locomotor::MovementLayer::Ground)
        {
            return false;
        }
        let owner = entity.owner();
        let rect = building_base_reservation_rect(
            entity.position.rx,
            entity.position.ry,
            &entity.foundation,
            spacing,
        );
        let (foundation_width, foundation_height) =
            crate::rules::foundation::foundation_dimensions(&entity.foundation);
        let repair_rect = building_base_reservation_repair_rect(
            entity.position.rx,
            entity.position.ry,
            foundation_width,
            foundation_height,
            spacing,
        );
        let Some(house_index) = self.base_reservation_house_index(owner) else {
            return false;
        };

        self.substrate.base_reservations.clear_rect(
            context.terrain().or(self.resolved_terrain.as_ref()),
            rect,
            house_index,
        );
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::BaseReservationCleared);

        // The native repair scan happens while this building is still linked in
        // the ground list. Each lookup selects only the first Building and calls
        // its repair-only writer immediately; identities are not deduplicated.
        scan_cell_rect(repair_rect, |x, y| {
            let neighbor_id = resolve_reservation_real_cell(
                context.terrain().or(self.resolved_terrain.as_ref()),
                x,
                y,
            )
            .and_then(|(rx, ry)| {
                self.substrate.occupancy.first_building_on_layer(
                    rx,
                    ry,
                    crate::sim::movement::locomotor::MovementLayer::Ground,
                )
            });
            if let Some(neighbor_id) = neighbor_id
                && neighbor_id != stable_id
            {
                self.mark_building_base_reservation_with_arg(neighbor_id, true, context);
            }
            true
        });
        self.update_base_reservation_perimeter_after_clear(owner, house_index, rect, context);
        true
    }

    fn unmark_entity_remove(&mut self, stable_id: u64, context: UninitContext<'_>) -> bool {
        self.unmark_entity_remove_impl(stable_id, true, context)
    }

    fn unmark_entity_remove_impl(
        &mut self,
        stable_id: u64,
        clear_air_spatial: bool,
        context: UninitContext<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(stable_id) else {
            return false;
        };
        if !entity.lifecycle.cell_marked {
            return false;
        }
        // REMOVE5F5913 clears +74 before the same Foot +78 query.
        entity.lifecycle.cell_marked = false;
        let cells = entity_occupancy_cells(entity);
        let layer = cell_list_layer_for_entity(entity);
        let category = entity.category;
        let foundation = entity.foundation.clone();
        let hidden_profile = entity.building_hidden_occupancy;
        let current_cell = (entity.position.rx, entity.position.ry);
        let raw_position = RevealPosition {
            rx: entity.position.rx,
            ry: entity.position.ry,
            z: entity.position.z,
            sub_x: entity.position.sub_x,
            sub_y: entity.position.sub_y,
        };
        let exact_z_leptons = entity.position.exact_z_leptons;
        let inside_transport = entity.passenger_role.is_inside_transport();
        if category == EntityCategory::Unit {
            let (entities, occupation) = (
                &mut self.substrate.entities,
                &mut self.substrate.cell_occupation,
            );
            if let Some(drive) = entities
                .get_mut(stable_id)
                .and_then(|entity| entity.drive_locomotion.as_mut())
            {
                crate::sim::occupancy::clear_drive_head_to_occupation_for_remove(
                    drive, occupation, stable_id,
                );
            }
        }
        if let Some(layer) = layer {
            for &(rx, ry) in &cells {
                self.substrate
                    .occupancy
                    .remove_on_layer(rx, ry, stable_id, layer);
            }
            if !inside_transport
                && matches!(
                    category,
                    EntityCategory::Unit
                        | EntityCategory::Infantry
                        | EntityCategory::Structure
                        | EntityCategory::Aircraft
                )
            {
                #[cfg(test)]
                self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationListUnlinked);
                if category == EntityCategory::Structure
                    && layer == crate::sim::movement::locomotor::MovementLayer::Ground
                    && hidden_profile.is_some_and(|profile| {
                        self.substrate.hidden_occupation.exit_building(
                            current_cell,
                            &foundation,
                            profile,
                            Some((self.session.map_width, self.session.map_height)),
                        )
                    })
                {
                    #[cfg(test)]
                    self.trace_lifecycle_for_test(LifecycleTestEvent::HiddenOccupationExited);
                }
                if self.clear_common_raw_occupation(
                    stable_id,
                    category,
                    &cells,
                    raw_position,
                    exact_z_leptons,
                    context,
                ) {
                    #[cfg(test)]
                    self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationCleared);
                }
            }
            if category == EntityCategory::Unit {
                self.substrate.cell_occupation.clear_vehicle_on_layer(
                    current_cell.0,
                    current_cell.1,
                    stable_id,
                    layer,
                );
            }
        }
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.lifecycle.cell_marked = false;
            if clear_air_spatial {
                entity.air_spatial_bucket = None;
                entity.air_spatial_enter_order = 0;
            }
            entity.foot_occupation_enabled = false;
        }
        true
    }

    /// Mirror the native air-vector move producer: retain vector position while
    /// the object stays in one bucket, otherwise remove from the old vector and
    /// append to the destination vector's tail.
    fn sync_air_spatial_membership(&mut self, stable_id: u64) {
        let desired_bucket = self.substrate.entities.get(stable_id).and_then(|entity| {
            (entity.lifecycle.object_alive
                && !entity.lifecycle.in_limbo
                && entity.lifecycle.cell_marked
                && !entity.passenger_role.is_inside_transport()
                && air_spatial_tracks_entity(entity))
            .then(|| {
                air_spatial_bucket_index(
                    entity.position.rx,
                    entity.position.ry,
                    self.session.map_width,
                    self.session.map_height,
                )
            })
        });
        let current_bucket = self
            .substrate
            .entities
            .get(stable_id)
            .and_then(|entity| entity.air_spatial_bucket);
        if current_bucket == desired_bucket {
            return;
        }
        let enter_order = desired_bucket
            .map(|_| self.substrate.next_occupancy_enter_order.next())
            .unwrap_or(0);
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.air_spatial_bucket = desired_bucket;
            entity.air_spatial_enter_order = enter_order;
        }
    }

    /// Own the represented bridge DropIn's complete cell-list relayer.
    /// Native ObjectClass::DropIn 0x005F4160 removes membership while OnBridge
    /// is set, then clears it and re-enters through the multi-cell hooks.
    /// The fresh entry order also owns reconstruction after snapshot restore.
    ///
    /// This preserves the existing Rust ground snap and locomotor reset.
    /// Native falling/unchanged-Z, concrete raw occupation callbacks, and
    /// hidden-building occupation entry remain DRIFT. Add/RemoveContent skip Infantry raw callbacks;
    /// full Mark/Unmark would also discard reservations and building smudges.
    pub(super) fn drop_in_bridge_member(&mut self, stable_id: u64) {
        use crate::sim::movement::locomotor::{GroundMovePhase, MovementLayer};
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let cells = entity_occupancy_cells(entity);
        let current_cell = (entity.position.rx, entity.position.ry);
        let sub_cell = entity.sub_cell;
        let insertion = CellListInsertion::from_category(entity.category);
        let ground_level = self
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(current_cell.0, current_cell.1))
            .map_or(0, |cell| cell.level);
        for &(rx, ry) in &cells {
            self.substrate
                .occupancy
                .remove_on_layer(rx, ry, stable_id, MovementLayer::Bridge);
        }
        let order = self.substrate.next_occupancy_enter_order.next();
        let entity = self
            .substrate
            .entities
            .get_mut(stable_id)
            .expect("member remains represented");
        entity.bridge_occupancy = None;
        entity.on_bridge = false;
        entity.position.z = ground_level;
        entity.position.exact_z_leptons = None;
        entity.movement_target = None;
        entity.occupancy_enter_order = order;
        if let Some(loco) = entity.locomotor.as_mut() {
            loco.layer = MovementLayer::Ground;
            loco.phase = GroundMovePhase::Idle;
        }
        // Rebuild only the derived vehicle projection: serialized head-to,
        // handoff and current-cleared facts retain their existing owners.
        self.substrate.cell_occupation.reconcile_entity(entity);
        for &(rx, ry) in &cells {
            self.substrate.occupancy.add(
                rx,
                ry,
                stable_id,
                MovementLayer::Ground,
                sub_cell,
                insertion,
            );
        }
    }

    /// Test/fixture helper retained at the transaction boundary.  It is
    /// idempotent and updates the authoritative `cell_marked` fact.
    pub(crate) fn add_entity_occupancy(&mut self, stable_id: u64) {
        let _ = self.mark_entity_put(stable_id, UninitContext::default());
    }

    /// Existing movement and fixture boundary; common lifecycle code calls the
    /// private unmark transaction instead.
    pub(crate) fn remove_entity_occupancy(&mut self, stable_id: u64) {
        self.unmark_entity_remove(stable_id, UninitContext::default());
    }

    /// Materialize the legacy split representation before its first Fly
    /// producer visit. Exact Object coordinates are already physical Z and
    /// cannot be reconstructed from a stale altitude cache at the wrapper tail.
    /// Native4CDD1A reads current owner height after horizontal SetCoords;
    /// conditional SetHeight calls (4CDE9D/4CDFB6) modify that value. The
    /// equal-height alive branch4CDECA..4CE145 performs no height write.
    fn materialize_legacy_fly_coordinate(&mut self, stable_id: u64) {
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::movement::ground_pose::{ground_surface_z_at, position_world_xy};
        use crate::sim::movement::locomotor::MovementLayer;

        let Some(entity) = self.substrate.entities.get_mut(stable_id) else {
            return;
        };
        let Some(locomotor) = entity.locomotor.as_ref() else {
            return;
        };
        if entity.position.exact_z_leptons.is_some()
            || locomotor.layer != MovementLayer::Air
            || locomotor.kind != LocomotorKind::Fly
        {
            return;
        }
        // Shared ground owner includes the live canonical Dummy's level and
        // slope. Only a mapless fixture uses the constructor's ground zero.
        let surface = ground_surface_z_at(
            position_world_xy(&entity.position),
            entity.on_bridge,
            self.resolved_terrain.as_ref(),
            None,
        );
        let surface = match (surface, self.resolved_terrain.is_some()) {
            (Some(surface), _) => surface,
            (None, false) => {
                if entity.on_bridge {
                    BRIDGE_DECK_HEIGHT_LEPTONS
                } else {
                    0
                }
            }
            (None, true) => return,
        };
        entity.position.exact_z_leptons =
            Some(surface.wrapping_add(locomotor.altitude.to_num::<i32>()));
    }

    /// Run one production air-process visit with the active Fly
    /// remove-before/process/add-after cell-list transaction around it.
    pub(crate) fn tick_air_movement_with_cell_lists_one(
        &mut self,
        stable_id: u64,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> crate::sim::movement::air_movement::AirMovementTickStats {
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::movement::locomotor::MovementLayer;

        let transact_fly = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                entity.category == EntityCategory::Aircraft
                    && entity.lifecycle.object_alive
                    && !entity.lifecycle.in_limbo
                    && entity.locomotor.as_ref().is_some_and(|locomotor| {
                        locomotor.kind == LocomotorKind::Fly
                            && locomotor.layer == MovementLayer::Air
                    })
            });
        if transact_fly {
            self.unmark_entity_remove_impl(stable_id, false, UninitContext::default());
        }

        self.materialize_legacy_fly_coordinate(stable_id);

        // A cruising Jumpjet runs the native Update/State3 body instead of the
        // air adapter (`world::jumpjet_cruise`).
        let stats = match self.tick_jumpjet_cruise_one(stable_id, rules) {
            Some(stats) => stats,
            None => crate::sim::movement::air_movement::tick_air_movement(
                &mut self.substrate.entities,
                &[stable_id],
                self.session.tick,
            ),
        };

        if transact_fly
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| entity.lifecycle.object_alive && !entity.lifecycle.in_limbo)
        {
            self.add_entity_occupancy(stable_id);
        }
        self.sync_air_spatial_membership(stable_id);
        stats
    }

    /// Object-kind classification for the LogicVector dispatch (F13). Probes
    /// the stores in the exact pre-consolidation order (anims → particle
    /// systems → terrain → projectiles → waves → entities) and returns `None`
    /// when the id is represented nowhere. Object IDs are unique across
    /// stores, so first-match equals only-match.
    pub(crate) fn classify_object(&self, stable_id: u64) -> Option<ObjectKind> {
        if self.substrate.anims.contains_key(stable_id) {
            Some(ObjectKind::Anim)
        } else if self.substrate.voxel_anims.contains_key(stable_id) {
            Some(ObjectKind::VoxelAnim)
        } else if self.substrate.particle_systems.contains_key(stable_id) {
            Some(ObjectKind::ParticleSystem)
        } else if self.production.terrain_objects.contains_key(&stable_id) {
            Some(ObjectKind::Terrain)
        } else if self.projectiles.get(stable_id).is_some() {
            Some(ObjectKind::Projectile)
        } else if self.waves.get(stable_id).is_some() {
            Some(ObjectKind::Wave)
        } else if self.substrate.entities.contains(stable_id) {
            Some(ObjectKind::Entity)
        } else {
            None
        }
    }

    /// Read the per-object `in_logic_vector` membership flag for a classified
    /// object. The flag lives on the object in its own store; this is the
    /// single dispatch the registration/removal contract reads through.
    fn logic_membership_flag(&self, stable_id: u64, kind: ObjectKind) -> bool {
        match kind {
            ObjectKind::Anim => self
                .substrate
                .anims
                .get(stable_id)
                .is_some_and(|anim| anim.in_logic_vector),
            ObjectKind::VoxelAnim => self
                .substrate
                .voxel_anims
                .get(stable_id)
                .is_some_and(|debris| debris.in_logic_vector),
            ObjectKind::ParticleSystem => self
                .substrate
                .particle_systems
                .get(stable_id)
                .is_some_and(|system| system.in_logic_vector),
            ObjectKind::Terrain => self
                .production
                .terrain_objects
                .get(&stable_id)
                .is_some_and(|terrain| terrain.in_logic_vector),
            ObjectKind::Projectile => self
                .projectiles
                .get(stable_id)
                .is_some_and(|projectile| projectile.in_logic_vector),
            ObjectKind::Wave => self
                .waves
                .get(stable_id)
                .is_some_and(|wave| wave.in_logic_vector),
            ObjectKind::Entity => self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| entity.in_logic_vector),
        }
    }

    /// Write the per-object `in_logic_vector` membership flag for a classified
    /// object. The single dispatch the registration/removal contract repairs
    /// the flag through.
    fn set_logic_membership_flag(&mut self, stable_id: u64, kind: ObjectKind, member: bool) {
        match kind {
            ObjectKind::Anim => {
                if let Some(anim) = self.substrate.anims.get_mut(stable_id) {
                    anim.in_logic_vector = member;
                }
            }
            ObjectKind::VoxelAnim => {
                if let Some(debris) = self.substrate.voxel_anims.get_mut(stable_id) {
                    debris.in_logic_vector = member;
                }
            }
            ObjectKind::ParticleSystem => {
                if let Some(system) = self.substrate.particle_systems.get_mut(stable_id) {
                    system.in_logic_vector = member;
                }
            }
            ObjectKind::Terrain => {
                if let Some(terrain) = self.production.terrain_objects.get_mut(&stable_id) {
                    terrain.in_logic_vector = member;
                }
            }
            ObjectKind::Projectile => {
                if let Some(projectile) = self.projectiles.get_mut(stable_id) {
                    projectile.in_logic_vector = member;
                }
            }
            ObjectKind::Wave => {
                if let Some(wave) = self.waves.get_mut(stable_id) {
                    wave.in_logic_vector = member;
                }
            }
            ObjectKind::Entity => {
                if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                    entity.in_logic_vector = member;
                }
            }
        }
    }

    /// gamemd-derived: active YR `LogicClass__RegisterObject @ 0x0055BAA0`
    /// gates on the object's membership flag, appends at the live tail, then
    /// sets the flag only after insertion succeeds.
    fn register_logic_object(&mut self, stable_id: u64) -> bool {
        let kind = self.classify_object(stable_id);
        if let Some(kind) = kind {
            if self.logic_membership_flag(stable_id, kind) {
                return true;
            }
        }
        let Some(kind) = kind else {
            return false;
        };
        if self.substrate.logic.try_push(stable_id).is_err() {
            return false;
        }
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::LogicAppended);
        self.set_logic_membership_flag(stable_id, kind, true);
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::LogicMembershipSet);
        true
    }

    /// gamemd-derived: active YR `LogicClass__UnregisterObject @ 0x0055BAE0`
    /// gates removal on the object's membership flag, performs the first-match
    /// stable erase, then repairs that flag.
    fn unregister_logic_object(&mut self, stable_id: u64) -> bool {
        let Some(kind) = self.classify_object(stable_id) else {
            return false;
        };
        if !self.logic_membership_flag(stable_id, kind) {
            return false;
        }
        let _ = self.substrate.logic.remove_first(stable_id);
        self.set_logic_membership_flag(stable_id, kind, false);
        true
    }

    /// Test-only access to the exact LogicVector helper ordering. Returns the
    /// dispatch outcome so order/gate tests can assert it directly.
    #[cfg(test)]
    pub(crate) fn register_live_object(&mut self, stable_id: u64) -> bool {
        self.register_logic_object(stable_id)
    }

    /// Test-only access to the exact LogicVector helper ordering. Returns the
    /// dispatch outcome so order/gate tests can assert it directly.
    #[cfg(test)]
    pub(crate) fn unregister_live_object(&mut self, stable_id: u64) -> bool {
        self.unregister_logic_object(stable_id)
    }

    pub(crate) fn reveal_anim(&mut self, stable_id: u64) -> bool {
        if !self
            .substrate
            .anims
            .get(stable_id)
            .is_some_and(|anim| !anim.runtime.inactive)
        {
            return false;
        }
        self.register_logic_object(stable_id)
    }

    pub(crate) fn conceal_anim(&mut self, stable_id: u64) -> bool {
        self.unregister_logic_object(stable_id)
    }

    /// `ObjectClass::Unlimbo` inside `VoxelAnimClass::Constructor @ 0x007493B0`
    /// reveals the piece, which is what puts it in the LogicClass vector.
    pub(crate) fn reveal_voxel_anim(&mut self, stable_id: u64) -> bool {
        if !self.substrate.voxel_anims.contains_key(stable_id) {
            return false;
        }
        self.register_logic_object(stable_id)
    }

    pub(crate) fn reveal_particle_system(&mut self, stable_id: u64) -> bool {
        if !self.substrate.particle_systems.contains_key(stable_id) {
            return false;
        }
        self.register_logic_object(stable_id)
    }

    pub(crate) fn conceal_particle_system(&mut self, stable_id: u64) -> bool {
        self.unregister_logic_object(stable_id)
    }

    pub(crate) fn register_terrain_object(&mut self, stable_id: u64) -> bool {
        self.production
            .terrain_objects
            .get(&stable_id)
            .is_some_and(|terrain| terrain.is_live())
            && self.register_logic_object(stable_id)
    }

    pub(crate) fn register_projectile(&mut self, stable_id: u64) -> bool {
        self.projectiles.get(stable_id).is_some() && self.register_logic_object(stable_id)
    }

    pub(crate) fn register_wave(&mut self, stable_id: u64) -> bool {
        self.waves.get(stable_id).is_some() && self.register_logic_object(stable_id)
    }

    pub(crate) fn unregister_non_entity_object(&mut self, stable_id: u64) -> bool {
        self.unregister_logic_object(stable_id)
    }

    /// Terminal Terrain/Bullet/Wave objects leave Logic immediately but retain
    /// their physical store identity until the common late delete drain.
    ///
    /// gamemd-derived: active YR `DrainDeferredFinalizationQueue @ 0x00725C70`
    /// performs scalar destruction/freeing only after the frame commit.
    pub(crate) fn retire_non_entity_object(&mut self, stable_id: u64) -> bool {
        let represented = matches!(
            self.classify_object(stable_id),
            Some(ObjectKind::Terrain | ObjectKind::Projectile | ObjectKind::Wave)
        );
        if !represented {
            return false;
        }
        let _ = self.unregister_logic_object(stable_id);
        self.substrate.pending_delete.push(stable_id);
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::PendingDeleteQueued { stable_id });
        true
    }

    /// Open-topped cargo entry hides the passenger but then directly restores
    /// its active membership. This is deliberately not Reveal: the passenger
    /// remains limbo/unmarked while its AI stays in the live object order.
    pub(crate) fn register_open_topped_passenger(&mut self, stable_id: u64) -> bool {
        if !self.substrate.entities.contains(stable_id) {
            return false;
        }
        self.register_logic_object(stable_id)
    }

    /// Compatibility dispatch which keeps AnimClass logic-only and routes every
    /// GameEntity through the complete common Object Conceal transaction.
    #[cfg(test)]
    pub(crate) fn conceal(&mut self, stable_id: u64) -> ConcealOutcome {
        if self.substrate.anims.contains_key(stable_id) {
            return if self.conceal_anim(stable_id) {
                ConcealOutcome::Concealed
            } else {
                ConcealOutcome::AlreadyConcealed
            };
        }
        if self.substrate.particle_systems.contains_key(stable_id) {
            return if self.conceal_particle_system(stable_id) {
                ConcealOutcome::Concealed
            } else {
                ConcealOutcome::AlreadyConcealed
            };
        }
        self.object_conceal(stable_id)
    }

    /// ObjectClass::Conceal represented order. Conceal does not mutate Alive.
    #[cfg(test)]
    pub(crate) fn object_conceal(&mut self, stable_id: u64) -> ConcealOutcome {
        self.object_conceal_with_context(stable_id, UninitContext::default())
    }

    fn object_conceal_with_context(
        &mut self,
        stable_id: u64,
        context: UninitContext<'_>,
    ) -> ConcealOutcome {
        let Some((in_limbo, object_alive)) = self
            .substrate
            .entities
            .get(stable_id)
            .map(|entity| (entity.lifecycle.in_limbo, entity.lifecycle.object_alive))
        else {
            return ConcealOutcome::MissingOrDead;
        };
        #[cfg(not(test))]
        let _ = object_alive;
        // gamemd-derived: `ObjectClass::Conceal @ 0x005F4D30` tests InLimbo
        // at `0x005F4D45` and returns before Destroy on the already-limbo path.
        if in_limbo {
            #[cfg(test)]
            self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealAlreadyLimboReturn {
                stable_id,
                object_alive,
                resolvable: self.substrate.entities.contains(stable_id),
            });
            return ConcealOutcome::AlreadyConcealed;
        }

        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.selected = false;
        }
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealDeselected);

        // gamemd-derived: active YR `ObjectClass::Conceal @ 0x005F4D30`
        // enters `ObjectClass::Destroy(1) @ 0x005F5280` after deselection and
        // before `Mark(REMOVE)`, so the expiry broadcast observes the target
        // alive, resolvable, and still cell-marked.
        #[cfg(test)]
        {
            let (object_alive, cell_marked) = self
                .substrate
                .entities
                .get(stable_id)
                .map(|entity| (entity.lifecycle.object_alive, entity.lifecycle.cell_marked))
                .unwrap_or((false, false));
            self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealDestroyNotifyBoundary {
                stable_id,
                object_alive,
                cell_marked,
                resolvable: self.substrate.entities.contains(stable_id),
            });
        }
        self.notify_pointer_expired(stable_id, context);

        if self.unmark_entity_remove(stable_id, context) {
            #[cfg(test)]
            self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealUnmarked);
        }

        self.lifecycle_outputs
            .push(LifecycleOutput::DisplayRemove { stable_id });
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealDisplayBoundary);
        self.lifecycle_outputs
            .push(LifecycleOutput::DetachAttachedAnims { stable_id });
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealAnimBoundary);
        self.lifecycle_outputs
            .push(LifecycleOutput::StopVoc { stable_id });
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealVocBoundary);

        if self.unregister_logic_object(stable_id) {
            #[cfg(test)]
            self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealLogicRemoved);
        }

        let dirty_rect_eligible = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.dirty_rect_eligible);
        if dirty_rect_eligible {
            self.lifecycle_outputs
                .push(LifecycleOutput::DirtyTacticalRect { stable_id });
            #[cfg(test)]
            self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealDirtyTacticalRectBoundary);
        }
        self.lifecycle_outputs
            .push(LifecycleOutput::ClearDrawnState { stable_id });
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealClearDrawnStateBoundary);

        // ObjectConceal5F4E98 invokes Techno6F4A40 here, before +8E Limbo.
        // Human ownership preserves +41B; +41A/+41C are never cleared here.
        let owner_controlled_by_human = self
            .substrate
            .entities
            .get(stable_id)
            .and_then(|entity| self.houses.get(&entity.owner()))
            .map(|house| house.is_controlled_by_human(self.session.game_mode_nonzero));
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            if owner_controlled_by_human == Some(false) {
                entity.discovery.discovered_by_current_house = false;
            }
            entity.lifecycle.in_limbo = true;
            // `BuildingClass::Limbo` destroys its owned BuildingLight before
            // the remaining building-count/base-node teardown.
            entity.building_light = None;
        }
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealLimboSet);
        self.lifecycle_outputs
            .push(LifecycleOutput::ClearRedraw { stable_id });
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealClearRedrawBoundary);
        ConcealOutcome::Concealed
    }

    /// TechnoClass Limbo sends synchronous BREAK to every contact before the
    /// common Object Conceal transaction.
    pub(crate) fn techno_limbo(&mut self, stable_id: u64) -> ConcealOutcome {
        self.techno_limbo_with_context(stable_id, UninitContext::default())
    }

    pub(crate) fn techno_limbo_with_rules(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
    ) -> ConcealOutcome {
        self.techno_limbo_with_context(stable_id, UninitContext::with_rules(rules))
    }

    fn techno_limbo_with_context(
        &mut self,
        stable_id: u64,
        context: UninitContext<'_>,
    ) -> ConcealOutcome {
        if !self.substrate.entities.contains(stable_id) {
            return ConcealOutcome::MissingOrDead;
        }
        self.release_track_occupation_before_foot_limbo(stable_id);
        self.release_walk_occupation_before_foot_limbo(stable_id);
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| !entity.lifecycle.in_limbo)
        {
            //6F6B16 ordinary sight release precedes TypeCD1 gap removal at
            //6F6B6A; both precede Object Limbo. Repeated Limbo emits neither.
            self.fog.release_entity_sight(stable_id);
            self.remove_building_gap_before_limbo(stable_id);
        }
        // BuildingClass owns this pass before the common TechnoClass Limbo can
        // clear committed type/cell facts or broadcast another expiry callback.
        self.invalidate_base_plan_from_building_limbo(stable_id);
        // FootClass::Limbo @ 0x004DB260 and BuildingClass::Limbo @
        // 0x00445880 remove the exact deposited footprint before conceal.
        if let Some(rules) = context.rules() {
            self.remove_sensor_before_limbo_with_rules(stable_id, rules);
        } else {
            self.remove_sensor_before_limbo(stable_id);
        }
        // Dead and InLimbo are independent native state. TechnoClass::Limbo
        // still reaches ObjectClass::Conceal for a stored dead object; the
        // latter's InLimbo branch alone decides whether Conceal is a no-op.
        self.clear_building_base_reservation_and_repair(stable_id, context);
        crate::sim::radio::broadcast_break(self, stable_id);
        self.object_conceal_with_context(stable_id, context)
    }

    /// Existing Rust owner-count mutation with an explicit exactly-once guard.
    pub(crate) fn release_owned_count_once(&mut self, stable_id: u64) {
        let Some((owner, category, already_released, destroyed, killed_by, award, dont_score)) =
            self.substrate.entities.get(stable_id).map(|entity| {
                (
                    entity.owner(),
                    entity.category,
                    entity.owned_count_released,
                    entity.health.current == 0,
                    entity.killed_by,
                    entity.kill_award_points,
                    entity.dont_score,
                )
            })
        else {
            return;
        };
        if already_released {
            return;
        }
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.owned_count_released = true;
        }
        let owner_name = self.interner.resolve(owner).to_string();
        self.decrement_owned_count(&owner_name, category);
        if destroyed && !dont_score {
            self.record_match_kill_and_loss(owner, category, killed_by, award);
        }
    }

    /// Score-screen bookkeeping for one destroyed object: a loss for its owner, a
    /// kill for the house credited with destroying it, and that house's score
    /// award.
    ///
    /// This runs at the single owned-count release point rather than in the
    /// damage loop so it fires exactly once per object, but it does NOT
    /// re-derive the killer here — `killed_by` was captured at the instant of
    /// destruction, which is where gamemd records it.
    ///
    /// A `DontScore=` victim never reaches this recorder at all — its loss is
    /// suppressed alongside its kill and points, matching the single early return
    /// gamemd takes before any of the three.
    ///
    /// The kill is counted regardless of how the killer relates to the victim:
    /// gamemd increments the killing house's kill table for allied and
    /// self-inflicted destruction too, and suppresses only the *points*. (It also
    /// has a victim-type suppression flag with no VERA equivalent yet —
    /// UNCHECKED, not modelled.) Sold or otherwise despawned objects reach this
    /// helper with non-zero health and the caller filters them out.
    fn record_match_kill_and_loss(
        &mut self,
        owner: InternedId,
        category: EntityCategory,
        killed_by: Option<InternedId>,
        award: i32,
    ) {
        let structure = category == EntityCategory::Structure;
        if let Some(house) = self.houses.get_mut(&owner) {
            if structure {
                house.stats.buildings_lost = house.stats.buildings_lost.saturating_add(1);
            } else {
                house.stats.units_lost = house.stats.units_lost.saturating_add(1);
            }
        }
        let Some(killer) = killed_by else {
            return;
        };
        // Destroying an ally's object (or one's own) still counts as a kill but
        // is worth no score. `Record_The_Kill @ 0x00702D40` computes the award
        // once — behind `HouseClass::IsAlly @ 0x004F9A90`, asked BY THE KILLER —
        // and feeds the same value to the score add at 0x0070300F and to the
        // veterancy accumulator, so this test must be the one-way one and must
        // match `combat::award_kill_experience`.
        let friendly = crate::map::houses::is_allied_with(
            &self.house_alliances,
            self.interner.resolve(killer),
            self.interner.resolve(owner),
        );
        if let Some(house) = self.houses.get_mut(&killer) {
            if structure {
                house.stats.buildings_killed = house.stats.buildings_killed.saturating_add(1);
            } else {
                house.stats.units_killed = house.stats.units_killed.saturating_add(1);
            }
            if !friendly {
                house.stats.score_points = house.stats.score_points.saturating_add(award);
            }
        }
    }

    /// ObjectClass's exact-zero callback transaction for an eligible
    /// `CausesDelayKill` building. Active gamemd runs the routed kill callback
    /// and virtual Destroy/reference notification before TechnoClass arms the
    /// timer and restores Alive/Health=1. This deliberately does not call
    /// UnInit, Limbo, release owned counts, or enqueue physical deletion.
    pub(crate) fn postmortem_exact_zero_callbacks(
        &mut self,
        stable_id: u64,
        killer_owner: Option<InternedId>,
        rules: &crate::rules::ruleset::RuleSet,
        context: UninitContext<'_>,
    ) {
        let Some((owner, category, dont_score, type_ref, veterancy)) =
            self.substrate.entities.get(stable_id).map(|target| {
                (
                    target.owner(),
                    target.category,
                    target.dont_score,
                    target.type_ref(),
                    target.veterancy,
                )
            })
        else {
            return;
        };

        debug_assert_eq!(
            self.substrate
                .entities
                .get(stable_id)
                .map(|target| target.health.current),
            Some(0),
            "PostMortem Object callbacks run at exact zero"
        );
        if !dont_score {
            let award = crate::sim::combat::score_award_for_victim(
                rules.object(self.interner.resolve(type_ref)),
                veterancy,
            );
            self.record_match_kill_and_loss(owner, category, killer_owner, award);
        }
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::PostMortemKillBookkeeping { stable_id });

        // BuildingClass::Destroy broadcasts BREAK before entering the common
        // ObjectClass::Destroy body. The per-building native factory pointer has
        // no Rust representation; eligible stock barrels have none.
        crate::sim::radio::broadcast_break(self, stable_id);
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::PostMortemRadioBreakCompleted {
            stable_id,
        });

        // ObjectClass::Destroy(1) unconditionally deselects before broadcasting
        // pointer expiry. It leaves Logic/occupancy/liveness intact.
        if let Some(target) = self.substrate.entities.get_mut(stable_id) {
            target.selected = false;
        }
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::PostMortemDeselected { stable_id });

        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::PostMortemDestroyNotifyBoundary {
            stable_id,
        });
        self.notify_pointer_expired(stable_id, context);
    }

    pub(crate) fn apply_lifecycle_request(&mut self, request: LifecycleRequest) {
        match request {
            LifecycleRequest::Uninit {
                stable_id,
                reason: _,
            } => self.uninit(stable_id),
        }
    }

    pub(crate) fn apply_lifecycle_request_with_rules(
        &mut self,
        request: LifecycleRequest,
        rules: &RuleSet,
    ) {
        match request {
            LifecycleRequest::Uninit {
                stable_id,
                reason: _,
            } => self.uninit_with_rules(stable_id, rules),
        }
    }

    fn run_represented_uninit_pre_hook(&mut self, stable_id: u64) {
        self.clear_all_building_anim_slots(stable_id);
        self.clear_building_damage_fire_slots(stable_id);
        self.release_owned_count_once(stable_id);
        crate::sim::docking::bunker_link::break_links_on_despawn(self, stable_id);
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::UninitClassPre { stable_id });
    }

    fn uninit_carried_passengers(&mut self, carrier_id: u64, context: UninitContext<'_>) {
        let passenger_ids = self
            .substrate
            .entities
            .get_mut(carrier_id)
            .and_then(|carrier| carrier.passenger_role.cargo_mut())
            .map_or_else(Vec::new, |cargo| cargo.take_for_uninit());

        for passenger_id in passenger_ids {
            debug_assert_ne!(
                passenger_id, carrier_id,
                "transport {carrier_id} contains itself"
            );
            if passenger_id == carrier_id {
                continue;
            }
            if let Some(passenger) = self.substrate.entities.get_mut(passenger_id) {
                if matches!(
                    passenger.passenger_role,
                    PassengerRole::Inside { transport_id } if transport_id == carrier_id
                ) {
                    passenger.passenger_role = PassengerRole::None;
                }
                passenger.health.current = 0;
            }
            self.uninit_with_context(passenger_id, context);
        }
    }

    /// TechnoClass fatal-receiver passenger rung. The native death helper
    /// enters only after carried objects have completed their own authoritative
    /// UnInit transactions; the carrier itself remains represented for its
    /// DeathWeapon and category-specific UnInit that follows.
    pub(crate) fn purge_carried_passengers_for_fatal(
        &mut self,
        carrier_id: u64,
        context: UninitContext<'_>,
    ) {
        self.uninit_carried_passengers(carrier_id, context);
    }

    fn nav_ref_targets_expired(target: &NavTargetRef, expired_id: u64) -> bool {
        matches!(
            target,
            NavTargetRef::Entity { id }
                | NavTargetRef::Object { id }
                | NavTargetRef::Building { id }
                if *id == expired_id
        )
    }

    /// Represented entries in global ObjectClass construction order. Stable
    /// IDs are monotonic and never reused, so merging the separate Rust stores
    /// by ID reproduces the native registration order without walking holes
    /// left by already-finalized objects.
    ///
    /// gamemd-derived: active YR `ObjectClass` construction/destruction at
    /// `0x005F3900` / `0x005F3B80` maintains the listener roster in object
    /// construction order.
    fn removal_listener_order(&self) -> Vec<u64> {
        let mut listeners = self.substrate.entities.keys_sorted();
        listeners.extend(self.substrate.anims.iter().map(|(&stable_id, _)| stable_id));
        listeners.extend(
            self.substrate
                .particle_systems
                .iter()
                .map(|(&stable_id, _)| stable_id),
        );
        listeners.extend(self.projectiles.iter().map(|(&stable_id, _)| stable_id));
        listeners.extend(self.waves.iter().map(|(&stable_id, _)| stable_id));
        listeners.sort_unstable();
        debug_assert!(
            listeners.windows(2).all(|pair| pair[0] != pair[1]),
            "object stable ID exists in more than one represented store"
        );
        listeners
    }

    /// The detach-time targeting sweep: release every object currently shooting
    /// at `detach_id`, which is leaving play *while still alive*.
    ///
    /// This is not the pointer-expiry broadcast. The detaching object survives —
    /// it is being sold, changing owner, teleporting, or being detached by area
    /// damage — so nothing else nulls the references pointed at it, and this is
    /// the only route by which a live-but-detached target releases its
    /// attackers. In an ordinary skirmish it fires tens of times per match:
    /// every building sale, every engineer capture, every mind-control or
    /// Psychic Beacon owner change, every Chrono Legionnaire or Chronosphere
    /// teleport.
    ///
    /// Three clauses are NOT copies of the pointer-expiry sweep and are
    /// reproduced verbatim because each is observable:
    ///
    /// 1. **Descending stable-ID iteration.** The native walk runs the global
    ///    techno vector from its last entry down to its first. When two objects
    ///    share the detaching target, the higher ID restores first, so its
    ///    restored mission dispatches first and draws from the scenario RNG
    ///    first. An ascending walk would reorder the global draw sequence.
    /// 2. **Restore runs first, with no suspended-mission pre-check.** The
    ///    expiry sweep asks whether a mission is suspended and clears the target
    ///    before restoring; this one restores unconditionally and clears after.
    /// 3. **The target clear is conditional on the Restore not having replaced
    ///    the target.** A successful Restore re-installs the archived target, and
    ///    the null-out then does not run.
    ///
    /// **ONLY ONE OF SIX NATIVE ENTRY POINTS IS WIRED.** The sweep is reached
    /// from six distinct functions in the original — area damage (six call
    /// sites in one function), building sale, an unnamed pair, the building
    /// occupy-map placement, the owner change, and the teleport locomotor's
    /// state machine. VERA calls it only from the owner change. The earlier
    /// "1 of 3 Restore sites" framing counted CALL sites through the Restore
    /// vtable slot, which is a different and smaller set than the sweep's own
    /// callers, and read more complete than it was. Consequence: an attacker
    /// stays latched onto a target that is being sold or chrono-teleported
    /// until the target is actually removed and the pointer-expiry broadcast
    /// catches it. Frequency: every building sale and every Chrono
    /// Legionnaire / Chronosphere use — tens of times a match — though the
    /// window is short and the blast radius today is limited to objects that
    /// went through a blocked-step Override.
    ///
    /// RESIDUALS, recorded rather than guessed:
    /// - The native suppression clause skips the whole block for a listener
    ///   whose manager sub-object points at the detaching object while that
    ///   object is still alive and active — a mind-control / capture link
    ///   holding its live victim as its target. It exists precisely so that the
    ///   owner change *performed by* mind control does not make the controller
    ///   drop its own new victim. VERA models mind control as a per-entity flag
    ///   with no controller-to-victim link, so the clause cannot be evaluated
    ///   and is omitted. Consequence: once VERA gives mind control a controller
    ///   link, a controller will lose its target here where the original keeps
    ///   it — every control event. Today the arm is unreachable because no
    ///   controller link exists to hold the victim as a target.
    /// - The aircraft-Patrol arm, which clears two patrol-cursor fields on an
    ///   aircraft whose committed mission is Patrol. Neither field is
    ///   represented; the arm is a no-op for every ground object.
    /// - A second native table swept after the techno vector, nulling two
    ///   pointer slots that match the detaching object. Its element class is
    ///   UNKNOWN, so it is not modelled.
    pub(crate) fn stop_all_targeting_on_detach(&mut self, detach_id: u64) {
        let mut listeners = self.substrate.entities.keys_sorted();
        listeners.reverse();

        for listener_id in listeners {
            if !self.listener_targets(listener_id, detach_id) {
                continue;
            }

            let restored = self
                .mission_restore_on_target_detach(listener_id)
                .expect("detach sweep listener was resolved immediately before the Restore");

            let target_cleared = self.listener_targets(listener_id, detach_id);
            if target_cleared {
                self.set_archive_target_represented(listener_id, None)
                    .expect("detach sweep listener remains present for the target clear");
            }

            let _ = restored;
            #[cfg(test)]
            self.trace_lifecycle_for_test(LifecycleTestEvent::DetachTargetingSweepVisited {
                detach_id,
                listener_id,
                restored,
                target_cleared,
            });
        }
    }

    /// Whether `listener_id` currently holds `detach_id` as its shoot-at target.
    ///
    /// Cell targets never match: the native comparison is against an object
    /// pointer, so an object overridden onto a wall cell is invisible to both
    /// detach sweeps.
    fn listener_targets(&self, listener_id: u64, detach_id: u64) -> bool {
        self.substrate
            .entities
            .get(listener_id)
            .is_some_and(|listener| {
                matches!(
                    listener.attack_target.as_ref().map(|target| target.target),
                    Some(TargetKind::Entity(id)) if id == detach_id
                )
            })
    }

    /// Infantry PerCell repair519D17..519D36 calls each registered Infantry
    /// +28(hut,false) in descending construction order. The hut survives;
    /// this is not the global ObjectClass removal broadcast. Actual Infantry
    /// +28 is51AA10 -> Foot4D9960 -> Techno7077C0. Its extra +6C0 clear
    /// compares an InfantryType pointer with the hut and cannot match.
    pub(crate) fn expire_infantry_bridge_hut_targets(&mut self, hut_id: u64) {
        // Infantry ctor517B34 appends to its type registry. Represented
        // PointerExpired receivers do not construct/delete registry entries;
        // stable construction IDs therefore preserve its descending cursor.
        let mut listeners = self.substrate.entities.keys_sorted();
        listeners.reverse();
        for listener_id in listeners {
            if !self
                .substrate
                .entities
                .get(listener_id)
                .is_some_and(|e| e.category == EntityCategory::Infantry)
            {
                continue;
            }
            let Some(hut) = self.substrate.entities.get(hut_id) else {
                return;
            };
            let facts = (
                object_get_coords_cell(hut),
                hut.lifecycle.object_alive,
                hut.health.current,
                hut.mission.current().known() == Some(crate::sim::mission::MissionType::Selling),
                hut.owner(),
            );
            self.notify_entity_pointer_expired(
                listener_id,
                hut_id,
                facts.0,
                false,
                facts.1,
                facts.2,
                facts.3,
                Some(facts.4),
                PointerExpiryControl::DetachAll,
            );
            // Techno707B24 forwards this manager independently of control.
            crate::sim::spawn_manager::notify_pointer_expired(self, listener_id, hut_id);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn notify_entity_pointer_expired(
        &mut self,
        listener_id: u64,
        expired_id: u64,
        // The expiring object's `ObjectClass::GetCoords` cell — the single
        // derivation `BulletClass::PointerExpired @ 0x004684E0` uses for both
        // the entity-hosted and store-hosted missile arms. It is also the cell
        // `TechnoClass::PointerExpired` hands to `SensorCountForHouse` when it
        // computes `allowClear` on the `Detach_All` control.
        expired_get_coords_cell: Option<(u16, u16)>,
        expired_is_high_flying: bool,
        expired_object_alive: bool,
        expired_health: i32,
        expired_is_selling: bool,
        expired_owner: Option<InternedId>,
        control: PointerExpiryControl,
    ) {
        let Some(listener) = self.substrate.entities.get(listener_id) else {
            return;
        };
        let current_target_matches = listener.attack_target.as_ref().is_some_and(
            |target| matches!(target.target, TargetKind::Entity(id) if id == expired_id),
        );
        let listener_owner = listener.owner();
        let passive_scan_remaining = listener
            .passive_scan_timer
            .remaining(self.session.binary_frame);
        let mission_is_suspended =
            listener.mission.suspended() != crate::sim::mission::MissionId::NONE;

        // `TechnoClass::PointerExpired @ 0x007077C0`, the `allowClear` local
        // (`[ESP+0x24]`): it starts true and is cancelled only on the
        // `Detach_All` control, when the expiring object is a Techno and the
        // RECEIVER's own house already holds a sensor count on that object's
        // cell — `0x00707994 CALL 0x004870D0`. The Techno half of that guard is
        // `AbstractFlags +0x14` bit 0, ORed in for every object by
        // `TechnoClass__Constructor @ 0x006F3228` and therefore true of every
        // `GameEntity`, so it needs no test here. `allowClear` gates both the
        // Target (`+0x2B4`) clear at `0x007079AD` and the `+0x2B8` clear at
        // `0x00707A7A`.
        let allow_clear = match control {
            PointerExpiryControl::Uninit => true,
            PointerExpiryControl::DetachAll => !expired_get_coords_cell
                .is_some_and(|(rx, ry)| self.fog.has_sensor_for_house(listener_owner, rx, ry)),
        };
        // `0x007079B7..0x007079CB`: on the same control the Target clear is
        // additionally skipped when the expiring object's `GetOwner` (vslot
        // `+0x3C`) equals the receiver's `+0x21C`, so a diving submarine never
        // drops its own house's shooters.
        let same_owner_exempt =
            control == PointerExpiryControl::DetachAll && expired_owner == Some(listener_owner);
        let clears_current_target = current_target_matches && allow_clear && !same_owner_exempt;

        // `0x007079D1..0x00707A0D`: the re-arm sits INSIDE the guarded arm,
        // ahead of `Assign_Target(NULL)`, so a receiver that keeps its target
        // spends no Scenario draw. An already-expired timer (`elapsed >=
        // duration`) skips the block entirely, which `remaining()`'s clamp to 0
        // reproduces.
        let passive_scan_delay = (clears_current_target && passive_scan_remaining > 10)
            .then(|| self.scenario_rng.next_range_u32_inclusive(4, 8));
        if let Some(listener) = self.substrate.entities.get_mut(listener_id) {
            if let Some(delay) = passive_scan_delay {
                listener
                    .passive_scan_timer
                    .arm(self.session.binary_frame, delay);
            }

            // `RadioClass::PointerExpired @ 0x0065AAC0` nulls matching sparse
            // slots in place, but ONLY on a nonzero control:
            //
            // ```
            // 0065aac1 MOV EBX,[ESP+0xc]      ; EBX = the control argument
            // 0065aae6 MOV EDX,[EAX + ECX*4]  ; slot
            // 0065aaec CMP EDX,EDI            ; slot == expired ?
            // 0065aaee JNZ 0065aafa
            // 0065aaf0 TEST BL,BL             ; control
            // 0065aaf2 JZ   0065aafa          ; control 0 skips the clear
            // 0065aaf4 MOV dword ptr [EAX],0x0
            // ```
            //
            // So `Detach_All(false)` — the dive — leaves radio contacts intact,
            // and only UnInit breaks them. A diving submarine therefore keeps
            // its naval-yard repair link and its transport link.
            if control == PointerExpiryControl::Uninit {
                listener.clear_live_contact_with(expired_id);
            }

            // TechnoClass removes an expiring passenger from its CargoClass before
            // clearing its target/archive/manager reference family.
            if let PassengerRole::Transport { cargo } = &mut listener.passenger_role {
                let _ = cargo.disembark(expired_id);
            }
        }

        if clears_current_target {
            self.set_archive_target_represented(listener_id, None)
                .expect("expiry listener remains present");
            if mission_is_suspended {
                self.mission_restore_after_target_expiry(listener_id)
                    .expect("represented expiry restore remains available");
            }
        }

        let Some(listener) = self.substrate.entities.get_mut(listener_id) else {
            return;
        };
        // `+0x2B8` is cleared on an exact match under `allowClear` alone — the
        // same-owner exemption applies only to `+0x2B4`.
        if allow_clear
            && matches!(
                listener.suspended_attack_target,
                Some(TargetKind::Entity(id)) if id == expired_id
            )
        {
            listener.suspended_attack_target = None;
        }

        // FootClass clears SuspendedNavCom first, then its current/aux target,
        // and removes every matching queue entry. Cell targets are unaffected.
        //
        // `FootClass::PointerExpired @ 0x004D9960` carries its OWN copy of the
        // `allowClear` Boolean, computed by a SECOND `SensorCountForHouse`
        // call — `0x004D9A57 CALL 0x004870D0`, at the expiring object's
        // `GetCoords` cell (`0x004D9A3D` vslot `+0x48`) for the RECEIVER's house
        // (`param_1[0x87] + 0x30`) — under the same control-0 / nonnull /
        // `+0x14` bit-0 guard as the Techno body. It gates the `+0x5A0`/`+0x5A4`
        // PAIR clear below; the `+0x5A8` clear above it is unguarded. The two
        // Booleans are equal in value because both read the receiver's house at
        // the same cell, so the single `allow_clear` local models both.
        if listener
            .navigation
            .suspended_nav_com
            .as_ref()
            .is_some_and(|target| Self::nav_ref_targets_expired(target, expired_id))
        {
            listener.navigation.suspended_nav_com = None;
        }
        let current_nav_matches = listener
            .navigation
            .nav_com
            .as_ref()
            .is_some_and(|target| Self::nav_ref_targets_expired(target, expired_id));
        let retain_capture_nav = current_nav_matches
            && listener.category == EntityCategory::Infantry
            && listener.occupier
            && listener.mission.current().known()
                == Some(crate::sim::mission::MissionType::Capture)
            && expired_object_alive
            && expired_health > 0
            && !expired_is_selling;
        //4D9A0F gates the pair on current NavCom identity;4D9ABD then
        //clears both fields, irrespective of the auxiliary pointer's value.
        if current_nav_matches && !retain_capture_nav && allow_clear {
            listener.navigation.nav_com_aux = None;
            listener.navigation.nav_com = None;
        }
        listener
            .navigation
            .nav_queue
            .retain(|target| !Self::nav_ref_targets_expired(target, expired_id));

        if listener.capture_target == Some(expired_id) {
            listener.capture_target = None;
        }
        if listener
            .c4_plant
            .as_ref()
            .is_some_and(|plant| plant.target_building_id == expired_id)
        {
            listener.c4_plant = None;
        }

        if listener
            .dock_state
            .as_ref()
            .is_some_and(|dock| dock.dock_building_id == expired_id)
        {
            listener.dock_state = None;
        }
        if let Some(ammo) = listener.aircraft_ammo.as_mut() {
            if ammo.target_airfield == Some(expired_id) {
                ammo.target_airfield = None;
                ammo.target_pad = None;
            }
        }
        if let Some(miner) = listener.miner.as_mut() {
            if miner.home_refinery == Some(expired_id) {
                miner.home_refinery = None;
            }
            if miner.reserved_refinery == Some(expired_id) {
                miner.reserved_refinery = None;
                miner.dock_queued = false;
            }
        }

        let clear_passenger_role = match &listener.passenger_role {
            PassengerRole::Transport { .. } => false,
            PassengerRole::Boarding {
                target_transport_id,
                ..
            } => *target_transport_id == expired_id,
            PassengerRole::Inside { transport_id } => *transport_id == expired_id,
            PassengerRole::None => false,
        };
        if clear_passenger_role {
            listener.passenger_role = PassengerRole::None;
        }

        if let Some(homing) = listener.homing_state.as_mut() {
            homing.expire_object_target(
                expired_id,
                expired_get_coords_cell,
                expired_is_high_flying,
            );
        }

        if let Some(pending) = listener.pending_c4_detonation.as_mut()
            && pending.source_entity_id == Some(expired_id)
        {
            pending.source_entity_id = None;
        }

        // Deliberately retain last_attacker_id. Native retaliation reads the
        // dying object through the deferred-delete window; it is not one of
        // the proactively-cleared target roles above.
    }

    /// ObjectClass::Detach_From_All_Lists represented listener broadcast.
    ///
    /// The callback pass runs while the target remains alive, unconcealed,
    /// cell-marked, and resolvable. The represented callbacks below do not add
    /// or erase listener objects, so the native live-vector cursor and this
    /// monotonic construction-order walk have the same observable result.
    ///
    /// gamemd-derived: active YR `DispatchPointerExpiredCleanup @ 0x007258D0`
    /// is called directly by `ObjectClass__UnInit @ 0x005F65F0` and again by
    /// `ObjectClass::Destroy @ 0x005F5280` inside the virtual Conceal path.
    /// Every listener uses the live MapClass authority, including when a receiver
    /// has temporarily lent that grid through UninitContext. The remaining arms
    /// read the object's liveness, health and mission from the same world.
    fn notify_pointer_expired(&mut self, expired_id: u64, context: UninitContext<'_>) {
        if !self.substrate.entities.contains(expired_id) {
            return;
        }

        // gamemd-derived: the House pointer-expiry handler reached by this
        // synchronous broadcast stable-removes a BuildConst Building while it
        // is still alive, marked, and resolvable. Keeping the House mutation
        // at this callback boundary also covers direct UnInit/destruction;
        // Conceal's already-limbo return never reaches it.
        self.remove_build_const_from_owner(expired_id);
        self.remove_house_base_membership(expired_id);
        // `TechnoClass::PointerExpired @ 0x0070785F..0x0070792A` (and the
        // death arm of `ReceiveDamage @ 0x0070206A`): both halves of a drain
        // link drop when either object expires.
        crate::sim::credit_income::clear_drain_links_on_expiry(self, expired_id);
        self.broadcast_pointer_expired(expired_id, PointerExpiryControl::Uninit, context);
    }

    /// `ObjectClass::Detach_All(false)` — the vtable `+0xDC` call
    /// `TechnoClass::StartCloaking @ 0x00703770` makes before it writes any
    /// cloak state, and again from the `CloakState 1 -> 2` completion arm of
    /// `TechnoClass::CloakingTick @ 0x006FB740` (`0x006FBA98`). Both reach
    /// `DispatchPointerExpiredCleanup @ 0x007258D0` with control **0**, which
    /// runs the same `TechnoClass::PointerExpired` body as the UnInit path over
    /// the same registered-object roster. The detaching object survives, so no
    /// House-side BuildConst removal happens here.
    ///
    /// **The non-Techno listeners on that roster are control-INSENSITIVE**, read
    /// this session and no longer a residual:
    ///
    /// * `WaveClass::PointerExpired @ 0x0075F610` (vtable `0x007F6BF4 + 0x28`)
    ///   and `ParticleSystemClass::PointerExpired @ 0x0062FE90` branch on the
    ///   control only inside the inherited `ObjectClass` call.
    /// * `AnimClass`'s `+0x28` override is `0x00425150` (vtable
    ///   `0x007E3354 + 0x28`; its only xref is that slot). It too forwards the
    ///   control to `ObjectClass::PointerExpired` and takes no further branch on
    ///   it.
    /// * `BulletClass::PointerExpired @ 0x004684E0` branches on the control only
    ///   for its trailing global-vector erase; the `+0x10C` target repair that
    ///   substitutes `Get_CellClass` at the expired object's last cell — the arm
    ///   VERA models in `HomingState::expire_object_target` — is unguarded.
    /// * `ObjectClass::PointerExpired @ 0x005F5230` itself gates only the
    ///   `+0x30` chain relink on a nonzero control; VERA models no `+0x30`
    ///   chain, so it is correct by omission.
    ///
    /// So a torpedo already in flight at a diving submarine falling to the sub's
    /// last cell instead of tracking it is native behavior, not an approximation.
    pub(crate) fn detach_all_pointer_expired(&mut self, expired_id: u64) {
        if !self.substrate.entities.contains(expired_id) {
            return;
        }
        self.broadcast_pointer_expired(
            expired_id,
            PointerExpiryControl::DetachAll,
            UninitContext::default(),
        );
    }

    fn broadcast_pointer_expired(
        &mut self,
        expired_id: u64,
        control: PointerExpiryControl,
        context: UninitContext<'_>,
    ) {
        let Some((
            expired_target_cell,
            expired_is_high_flying,
            expired_object_alive,
            expired_health,
            expired_is_selling,
            expired_owner,
        )) = self.substrate.entities.get(expired_id).map(|expired| {
            let high_flying = expired.locomotor.as_ref().is_some_and(|locomotor| {
                // High-flying objects expire to null; lower objects preserve
                // GetHeight() >= 2 * LevelHeight (2 * 104 leptons).
                locomotor.is_airborne() && locomotor.altitude >= SimFixed::from_num(2 * 104)
            });
            (
                object_get_coords_cell(expired),
                high_flying,
                expired.lifecycle.object_alive,
                expired.health.current,
                expired.mission.current().known()
                    == Some(crate::sim::mission::MissionType::Selling),
                Some(expired.owner()),
            )
        })
        else {
            return;
        };

        // `BulletClass::PointerExpired @ 0x004684E0` performs the packed
        // `MapClass::Get_CellClass @ 0x005657A0` lookup only for a matching
        // target. Its result pointer is stored at Bullet+0x10C: an allocated
        // slot therefore remains a stable Cell target, while a miss stores the
        // one shared dummy at `0x00ABDC50`. Later `BulletClass::AI @ 0x004666E0`
        // dispatches that live pointer and observes its most recent coord stamp.

        for listener_id in self.removal_listener_order() {
            let is_entity = self.substrate.entities.contains(listener_id);
            let is_anim = self.substrate.anims.contains_key(listener_id);
            let is_particle = self.substrate.particle_systems.contains_key(listener_id);
            let is_projectile = self.projectiles.get(listener_id).is_some();
            let is_wave = self.waves.get(listener_id).is_some();
            if !is_entity && !is_anim && !is_particle && !is_projectile && !is_wave {
                continue;
            }

            #[cfg(test)]
            if control == PointerExpiryControl::Uninit {
                let (target_alive, target_in_limbo) = self
                    .substrate
                    .entities
                    .get(expired_id)
                    .map(|target| (target.lifecycle.object_alive, target.lifecycle.in_limbo))
                    .unwrap_or((false, true));
                self.trace_lifecycle_for_test(LifecycleTestEvent::UninitRemovalListenerVisited {
                    expired_id,
                    listener_id,
                    target_alive,
                    target_in_limbo,
                });
            }

            if is_entity {
                self.notify_entity_pointer_expired(
                    listener_id,
                    expired_id,
                    expired_target_cell,
                    expired_is_high_flying,
                    expired_object_alive,
                    expired_health,
                    expired_is_selling,
                    expired_owner,
                    control,
                );
                // `TechnoClass::PointerExpired` forwards to the listener's
                // SpawnManager: `0x00707B24 CALL 0x006B7C60`, gated only on
                // `+0x2D0 != 0`. This is the only mechanism that drops a
                // destroyed wing target, so without it a Carrier keeps sending
                // its Hornets at a corpse. The forward sits OUTSIDE the control
                // test, so it runs on a cloak dive as well as on UnInit.
                crate::sim::spawn_manager::notify_pointer_expired(self, listener_id, expired_id);
                // The CaptureManager forward — `0x00707B14 CALL 0x00471F90` —
                // sits inside the `if (control != 0)` block opened at
                // `0x00707AE7`, so a dive leaves mind-control links alone while
                // a death drops them.
                if control == PointerExpiryControl::Uninit
                    && let Some(manager) = self
                        .substrate
                        .entities
                        .get_mut(listener_id)
                        .and_then(|entity| entity.capture_manager.as_mut())
                {
                    manager.pointer_expired(expired_id);
                }
            } else if is_anim {
                self.expire_anim_owner_reference(listener_id, expired_id);
            } else if is_particle {
                let system = self
                    .substrate
                    .particle_systems
                    .get_mut(listener_id)
                    .expect("particle listener disappeared during expiry callback");
                if system.owner_entity == Some(expired_id) {
                    system.owner_entity = None;
                }
                if system.attached_entity == Some(expired_id) {
                    system.attached_entity = None;
                    // vtable `+0xF8`, the same mark the lifetime and
                    // spawn-cutoff paths set.
                    system.done_spawning = true;
                }
            } else if is_projectile {
                let target_matches = self.projectiles.get(listener_id).is_some_and(|projectile| {
                    projectile.target == ProjectileTarget::Entity(expired_id)
                });
                let projectile_replacement_target = if !target_matches
                    || expired_is_high_flying
                    || expired_target_cell.is_none()
                    || expired_target_cell == Some(NULL_TARGET_CELL_SENTINEL)
                {
                    ProjectileTarget::None
                } else {
                    let (rx, ry) = expired_target_cell.expect("checked target cell");
                    match context.terrain().or(self.resolved_terrain.as_ref()) {
                        Some(terrain) => match crate::sim::cell_rect::get_cellclass_fallback(
                            Some(terrain),
                            i32::from(rx),
                            i32::from(ry),
                        ) {
                            crate::sim::cell_rect::CellRef::Real(_) => {
                                ProjectileTarget::Cell { rx, ry }
                            }
                            crate::sim::cell_rect::CellRef::Dummy { .. } => {
                                ProjectileTarget::DummyCell
                            }
                        },
                        // Terrainless synthetic fixtures keep their historical
                        // stable-cell fallback. Production is terrain-backed.
                        None => ProjectileTarget::Cell { rx, ry },
                    }
                };
                let present = self.projectiles.pointer_expired(
                    listener_id,
                    expired_id,
                    projectile_replacement_target,
                );
                debug_assert!(present);
                #[cfg(test)]
                if let Some((source_id, target)) = self
                    .projectiles
                    .get(listener_id)
                    .map(|projectile| (projectile.source_id, projectile.target))
                {
                    self.trace_lifecycle_for_test(
                        LifecycleTestEvent::ProjectilePointerExpiredVisited {
                            expired_id,
                            projectile_id: listener_id,
                            expired_resolvable: self.substrate.entities.contains(expired_id),
                            projectile_resolvable: true,
                            source_id,
                            target,
                        },
                    );
                }
            } else if is_wave {
                let (owner_cleared, _) = self
                    .waves
                    .pointer_expired(listener_id, expired_id)
                    .expect("Wave listener disappeared during expiry callback");
                if owner_cleared && self.active_wave_links.get(&expired_id) == Some(&listener_id) {
                    // TechnoClass keeps the Wave link through the dying/deferred
                    // interval. Once the exact owner pointer expires, retaining
                    // this Rust projection would serialize a link whose Wave
                    // owner is now null.
                    self.active_wave_links.remove(&expired_id);
                }
            }
        }
    }

    /// ObjectClass::UnInit represented ordering.  Physical removal is deferred.
    pub(crate) fn uninit(&mut self, stable_id: u64) {
        self.uninit_with_context(stable_id, UninitContext::default());
    }

    pub(crate) fn uninit_with_rules(&mut self, stable_id: u64, rules: &RuleSet) {
        self.uninit_with_context(stable_id, UninitContext::with_rules(rules));
    }

    pub(crate) fn uninit_with_context(&mut self, stable_id: u64, context: UninitContext<'_>) {
        let Some(entity) = self.substrate.entities.get_mut(stable_id) else {
            return;
        };
        // Consume deferred/retained Infantry lifetime before pointer-expiry
        // callbacks, including an UnInit that overtakes receiver delivery.
        entity.infantry_terminal = None;

        self.run_represented_uninit_pre_hook(stable_id);
        self.uninit_carried_passengers(stable_id, context);
        // `SpawnManagerClass::PointerExpired`, owner arm: `Kill_All_Spawns()`
        // then `ClearAllTargets()`. Docked/reloading children and any missile
        // still in its post-launch window die with the parent; aircraft already
        // out are released. The target clear is the second, separate call —
        // `Kill_All_Spawns` alone never touches the targets.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.spawn_manager.is_some())
        {
            crate::sim::spawn_manager::kill_all_spawns_with_context(self, stable_id, context);
            crate::sim::spawn_manager::clear_all_spawn_targets(self, stable_id);
        }

        #[cfg(test)]
        {
            let (object_alive, cell_marked) = self
                .substrate
                .entities
                .get(stable_id)
                .map(|entity| (entity.lifecycle.object_alive, entity.lifecycle.cell_marked))
                .unwrap_or((false, false));
            self.trace_lifecycle_for_test(LifecycleTestEvent::UninitRemovalNotifyBoundary {
                stable_id,
                object_alive,
                cell_marked,
                resolvable: self.substrate.entities.contains(stable_id),
            });
        }
        self.notify_pointer_expired(stable_id, context);

        let _ = self.techno_limbo_with_context(stable_id, context);
        self.substrate.entities.note_dying_transition();
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.lifecycle.object_alive = false;
            entity.dying = true;
        }
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::UninitAliveCleared { stable_id });

        // Native append has no duplicate suppression.  The drain collapses all
        // occurrences when this dead object becomes the selected ready entry.
        self.substrate.pending_delete.push(stable_id);
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::PendingDeleteQueued { stable_id });
    }

    #[cfg(test)]
    pub(crate) fn despawn_entity(&mut self, stable_id: u64) {
        self.uninit(stable_id);
    }

    /// Particle systems stay resolvable until the ordinary common late drain.
    /// Their owned particles have already emptied before this transition.
    pub(crate) fn retire_particle_system(&mut self, stable_id: u64) {
        let ready = self
            .substrate
            .particle_systems
            .get(stable_id)
            .is_some_and(|system| system.done_spawning && system.particles.is_empty());
        if !ready {
            return;
        }

        self.conceal_particle_system(stable_id);
        self.substrate.pending_delete.push(stable_id);
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::PendingDeleteQueued { stable_id });
    }

    fn pending_object_is_ready(&self, stable_id: u64) -> bool {
        if let Some(entity) = self.substrate.entities.get(stable_id) {
            return !entity.lifecycle.object_alive;
        }
        if let Some(anim) = self.substrate.anims.get(stable_id) {
            return anim.runtime.inactive;
        }
        if let Some(system) = self.substrate.particle_systems.get(stable_id) {
            return system.done_spawning && system.particles.is_empty();
        }
        if let Some(terrain) = self.production.terrain_objects.get(&stable_id) {
            return !terrain.is_live() && !terrain.in_logic_vector;
        }
        if let Some(projectile) = self.projectiles.get(stable_id) {
            return !projectile.in_logic_vector;
        }
        if let Some(wave) = self.waves.get(stable_id) {
            return !wave.in_logic_vector;
        }
        true
    }

    fn finalize_and_remove_common(&mut self, stable_id: u64) {
        self.release_house_base_tracking(stable_id);
        self.destroy_building_light(stable_id);
        if self.substrate.anims.contains_key(stable_id) {
            self.conceal_anim(stable_id);
            self.detach_anim_from_owner(stable_id);
            self.clear_building_anim_reference(stable_id);
        }
        // A Jumpjet destroyed while hovering never reaches State 4's release,
        // so its cell AltObject slot (`CellClass+0xE0`) is dropped here rather
        // than leaving the cell permanently claimed against later hoverers.
        self.substrate.air_slots.release_owner(stable_id);
        let entity = self.substrate.entities.remove(stable_id);
        let anim = self.substrate.anims.remove(stable_id);
        let particle_system = self.substrate.particle_systems.finalize_remove(stable_id);
        let terrain = self.production.terrain_objects.remove(&stable_id);
        if let Some(terrain) = terrain.as_ref() {
            let cell = terrain.cell();
            if self.production.terrain_object_cells.get(&cell) == Some(&stable_id) {
                self.production.terrain_object_cells.remove(&cell);
                self.production.terrain_spawners.remove(&cell);
                self.production.terrain_occupation_bits.remove(&cell);
                self.production
                    .tiberium_spawning_terrain_cells
                    .remove(&cell);
            }
        }
        let projectile = self.projectiles.remove(stable_id);
        let wave = self.waves.remove(stable_id);
        if let Some(wave) = wave.as_ref()
            && let Some(owner_id) = wave.owner_id
            && self.active_wave_links.get(&owner_id) == Some(&stable_id)
        {
            self.active_wave_links.remove(&owner_id);
        }
        if let Some(system) = particle_system.as_ref()
            && let Some(owner_id) = system.owner_entity
            && let Some(owner) = self.substrate.entities.get_mut(owner_id)
            && owner.damage_smoke_system_id == Some(stable_id)
        {
            // Native pointer expiry clears TechnoClass +0x310 only when the
            // marked system physically leaves object storage. Keeping the
            // identity through retirement prevents same-frame duplicates.
            owner.damage_smoke_system_id = None;
        }
        debug_assert!(
            usize::from(entity.is_some())
                + usize::from(anim.is_some())
                + usize::from(particle_system.is_some())
                + usize::from(terrain.is_some())
                + usize::from(projectile.is_some())
                + usize::from(wave.is_some())
                <= 1,
            "object id {stable_id} was removed from multiple stores"
        );
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::FinalizedCommon { stable_id });
    }

    fn finalize_multiplayer_feedback_anim(&mut self, stable_id: u64) {
        self.detach_anim_from_owner(stable_id);
        self.substrate.multiplayer_feedback_anims.remove(stable_id);
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::FinalizedCommon { stable_id });
    }

    /// Native-shaped pending-delete drain: preserve alive entries, collapse all
    /// duplicate ready IDs, and finalize each selected object exactly once.
    ///
    /// gamemd-derived: active YR `DrainDeferredFinalizationQueue @ 0x00725C70`
    /// is reached from `Main_Tick` at `0x0055DE9F` after the frame commit; it
    /// preserves non-ready entries, collapses selected duplicates, and finalizes
    /// each selected ready object once.
    pub(crate) fn process_pending_delete(&mut self) {
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::PendingDeleteDrainStarted);
        let mut index = 0;
        while index < self.substrate.pending_delete.len() {
            let stable_id = self.substrate.pending_delete[index];
            if !self.pending_object_is_ready(stable_id) {
                index += 1;
                continue;
            }
            self.substrate
                .pending_delete
                .retain(|&queued| queued != stable_id);
            self.finalize_and_remove_common(stable_id);
        }

        while let Some(&stable_id) = self.substrate.multiplayer_feedback_pending_delete.first() {
            self.substrate
                .multiplayer_feedback_pending_delete
                .retain(|&queued| queued != stable_id);
            self.finalize_multiplayer_feedback_anim(stable_id);
        }
    }

    /// Test compatibility only.  Production has one ordinary tail drain.
    #[cfg(test)]
    pub(crate) fn flush_pending_delete(&mut self) {
        self.process_pending_delete();
    }
}

#[cfg(test)]
mod base_plan_lifecycle_tests {
    use std::collections::BTreeMap;

    use crate::map::entities::EntityCategory;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::object_type::ObjectCategory as RulesObjectCategory;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::base_plan::{BasePlanNode, pack_base_plan_cell};
    use crate::sim::game_entity::{GameEntity, StructureUpgradeLink};
    use crate::sim::house_state::HouseState;
    use crate::sim::scenario_bootstrap::initialize_map_roster_houses;
    use crate::sim::world::{PlacementEvidence, RevealPosition, RevealRequest, Simulation};
    use crate::util::fixed_math::SimFixed;

    fn rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[General]\nMaximumBuildingPlacementFailures=3\n\
             [BuildingTypes]\n0=GAPOWR\n1=GACNST\n\
             [GAPOWR]\nStrength=750\nIsBaseDefense=yes\n\
             [GACNST]\nStrength=1000\nUndeploysInto=AMCV\n",
        ))
        .expect("base-plan lifecycle rules")
    }

    #[test]
    fn gsi_04_05_scenario_plan_precedes_unlimbo_and_human_unlimbo_is_excluded() {
        let rules = rules();
        let scenario = IniFile::from_str(
            "[Houses]\n0=Computer1\n\
             [Computer1]\nPercentBuilt=44\nNodeCount=1\n000=GACNST,10,11\n",
        );
        let roster =
            crate::map::houses::parse_house_roster(&scenario, &rules.color_schemes, Some(&rules));
        let mut sim = Simulation::new();
        initialize_map_roster_houses(&mut sim, &roster, Some(&rules));
        let ai = sim.interner.get("Computer1").unwrap();
        assert_eq!(sim.houses[&ai].base_plan.percent_built, 44);
        assert!(!sim.houses[&ai].base_plan.nodes[0].filled);

        let building = sim
            .spawn_object("GACNST", "Computer1", 10, 11, 0, &rules, &BTreeMap::new())
            .expect("scenario building");
        assert!(sim.houses[&ai].base_plan.nodes[0].filled);
        assert_eq!(sim.houses[&ai].base_plan.nodes[0].retry_count, 0);
        let entity = sim.entities().get(building).unwrap();
        assert_eq!(entity.category, EntityCategory::Structure);
        assert_eq!(entity.base_plan_type_index, 1);
        assert!(!entity.base_plan_is_defense);
        assert!(entity.base_plan_has_undeploy_target);

        let human = sim.interner.intern("Human1");
        let mut human_house = HouseState::new(human, 0, None, true, 0, 10);
        human_house.base_plan.nodes.push(BasePlanNode {
            type_or_control: 0,
            packed_cell: pack_base_plan_cell(20, 21),
            filled: false,
            retry_count: 7,
        });
        sim.houses.insert(human, human_house);
        sim.session.house_order.push(human);
        sim.spawn_object("GAPOWR", "Human1", 20, 21, 0, &rules, &BTreeMap::new())
            .expect("human building");
        assert!(!sim.houses[&human].base_plan.nodes[0].filled);
        assert_eq!(sim.houses[&human].base_plan.nodes[0].retry_count, 7);
    }

    #[test]
    fn gsi_04_05_build_const_append_precedes_base_plan_fill() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nMaximumBuildingPlacementFailures=3\n\
             [AI]\nBuildConst=GACNST\n\
             [BuildingTypes]\n0=GACNST\n\
             [GACNST]\nStrength=1000\nUndeploysInto=AMCV\n",
        ))
        .expect("combined BuildConst/BasePlan rules");
        let scenario = IniFile::from_str(
            "[Houses]\n0=Computer1\n\
             [Computer1]\nNodeCount=1\n000=GACNST,10,11\n",
        );
        let roster =
            crate::map::houses::parse_house_roster(&scenario, &rules.color_schemes, Some(&rules));
        let mut sim = Simulation::new();
        initialize_map_roster_houses(&mut sim, &roster, Some(&rules));
        let owner = sim.interner.get("Computer1").unwrap();

        let building = sim
            .spawn_object("GACNST", "Computer1", 10, 11, 0, &rules, &BTreeMap::new())
            .expect("combined BuildConst/BasePlan Building");

        assert_eq!(sim.houses[&owner].build_const_order, [building]);
        assert!(sim.houses[&owner].base_plan.nodes[0].filled);
        let events = sim.lifecycle_test_events_for_test();
        let build_const = events
            .iter()
            .position(|event| {
                *event
                    == (super::LifecycleTestEvent::BuildConstAppended {
                        stable_id: building,
                    })
            })
            .expect("BuildConst append trace");
        let base_plan = events
            .iter()
            .position(|event| {
                *event
                    == (super::LifecycleTestEvent::BasePlanFilled {
                        stable_id: building,
                    })
            })
            .expect("BasePlan fill trace");
        assert!(build_const < base_plan);
    }

    #[test]
    fn gsi_04_05_attached_upgrade_early_return_does_not_fill_base_plan() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Computer1");
        let type_ref = sim.interner.intern("GAPOWR");
        let mut house = HouseState::new(owner, 0, None, false, 0, 10);
        house.base_plan.nodes.push(BasePlanNode {
            type_or_control: 0,
            packed_cell: pack_base_plan_cell(10, 11),
            filled: false,
            retry_count: 7,
        });
        sim.houses.insert(owner, house);

        let mut upgrade = GameEntity::new_at_frame_zero_for_test(
            1,
            10,
            11,
            0,
            0,
            owner,
            crate::sim::components::Health { current: 100 },
            type_ref,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        upgrade.base_plan_type_index = 0;
        upgrade.structure_upgrade_link = Some(StructureUpgradeLink {
            parent_stable_id: 99,
            slot: 0,
        });
        sim.substrate.entities.insert(upgrade);

        let outcome = sim.try_reveal_entity(
            1,
            RevealRequest {
                position: RevealPosition {
                    rx: 10,
                    ry: 11,
                    z: 0,
                    sub_x: SimFixed::from_num(0),
                    sub_y: SimFixed::from_num(0),
                },
                placement: PlacementEvidence::AttachedUpgrade,
                logic_eligible: true,
            },
        );

        assert!(matches!(outcome, super::RevealOutcome::Revealed { .. }));
        assert!(!sim.houses[&owner].base_plan.nodes[0].filled);
        assert_eq!(sim.houses[&owner].base_plan.nodes[0].retry_count, 7);
        assert!(
            !sim.lifecycle_test_events_for_test().iter().any(|event| {
                *event == super::LifecycleTestEvent::BasePlanFilled { stable_id: 1 }
            })
        );
    }

    #[test]
    fn gsi_04_05_unlimbo_human_control_gate_is_mode_sensitive() {
        fn player_control_only_house(
            sim: &mut Simulation,
            owner_name: &str,
            packed_cell: u32,
        ) -> crate::sim::intern::InternedId {
            let owner = sim.interner.intern(owner_name);
            let mut house = HouseState::new(owner, 0, None, false, 0, 10);
            house.player_control = true;
            house.base_plan.nodes.push(BasePlanNode {
                type_or_control: 0,
                packed_cell,
                filled: false,
                retry_count: 6,
            });
            sim.houses.insert(owner, house);
            sim.session.house_order.push(owner);
            owner
        }

        let rules = rules();
        let mut nonzero = Simulation::new();
        nonzero.session.game_mode_nonzero = true;
        let nonzero_owner =
            player_control_only_house(&mut nonzero, "SkirmishSlot", pack_base_plan_cell(40, 41));
        nonzero
            .spawn_object(
                "GAPOWR",
                "SkirmishSlot",
                40,
                41,
                0,
                &rules,
                &BTreeMap::new(),
            )
            .expect("nonzero-mode Building");
        assert!(nonzero.houses[&nonzero_owner].base_plan.nodes[0].filled);
        assert_eq!(
            nonzero.houses[&nonzero_owner].base_plan.nodes[0].retry_count,
            0
        );

        let mut campaign = Simulation::new();
        let campaign_owner =
            player_control_only_house(&mut campaign, "CampaignPlayer", pack_base_plan_cell(50, 51));
        campaign
            .spawn_object(
                "GAPOWR",
                "CampaignPlayer",
                50,
                51,
                0,
                &rules,
                &BTreeMap::new(),
            )
            .expect("campaign Building");
        assert!(!campaign.houses[&campaign_owner].base_plan.nodes[0].filled);
        assert_eq!(
            campaign.houses[&campaign_owner].base_plan.nodes[0].retry_count,
            6
        );
    }

    #[test]
    fn gsi_04_05_undeploys_into_resolution_controls_base_plan_fallback() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=AMCV\n\
             [BuildingTypes]\n0=NONEYARD\n1=ANGLEYARD\n2=REALYARD\n\
             [AMCV]\nStrength=1000\n\
             [NONEYARD]\nStrength=1000\nUndeploysInto=NoNe\n\
             [ANGLEYARD]\nStrength=1000\nUndeploysInto=<nOnE>\n\
             [REALYARD]\nStrength=1000\nUndeploysInto=amcv\n",
        ))
        .expect("UndeploysInto resolution rules");

        assert!(rules.object("NONEYARD").unwrap().undeploys_into.is_none());
        assert!(rules.object("ANGLEYARD").unwrap().undeploys_into.is_none());
        let resolved = rules
            .object("REALYARD")
            .unwrap()
            .undeploys_into
            .as_deref()
            .expect("real UnitType identity remains resolved");
        assert!(
            rules
                .object_in_category(RulesObjectCategory::Vehicle, resolved)
                .is_some()
        );

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Computer1");
        let mut house = HouseState::new(owner, 0, None, false, 0, 10);
        house.base_plan.nodes = vec![
            BasePlanNode {
                type_or_control: 0,
                packed_cell: pack_base_plan_cell(70, 71),
                filled: false,
                retry_count: 4,
            },
            BasePlanNode {
                type_or_control: 1,
                packed_cell: pack_base_plan_cell(80, 81),
                filled: false,
                retry_count: 5,
            },
            BasePlanNode {
                type_or_control: 2,
                packed_cell: pack_base_plan_cell(90, 91),
                filled: false,
                retry_count: 6,
            },
        ];
        sim.houses.insert(owner, house);
        sim.session.house_order.push(owner);

        let none = sim
            .spawn_object("NONEYARD", "Computer1", 10, 11, 0, &rules, &BTreeMap::new())
            .expect("none-sentinel Building");
        let angle = sim
            .spawn_object(
                "ANGLEYARD",
                "Computer1",
                12,
                13,
                0,
                &rules,
                &BTreeMap::new(),
            )
            .expect("angle-sentinel Building");
        let real = sim
            .spawn_object("REALYARD", "Computer1", 14, 15, 0, &rules, &BTreeMap::new())
            .expect("resolved-undeploy Building");

        assert!(
            !sim.entities()
                .get(none)
                .unwrap()
                .base_plan_has_undeploy_target
        );
        assert!(
            !sim.entities()
                .get(angle)
                .unwrap()
                .base_plan_has_undeploy_target
        );
        assert!(
            sim.entities()
                .get(real)
                .unwrap()
                .base_plan_has_undeploy_target
        );
        let plan = &sim.houses[&owner].base_plan.nodes;
        assert!(!plan[0].filled, "none sentinel must not enable fallback");
        assert_eq!(plan[0].retry_count, 4);
        assert!(!plan[1].filled, "<none> sentinel must not enable fallback");
        assert_eq!(plan[1].retry_count, 5);
        assert!(plan[2].filled, "resolved UnitType enables fallback");
        assert_eq!(plan[2].retry_count, 0);
    }

    #[test]
    fn gsi_04_05_building_limbo_runs_base_plan_invalidation_before_conceal() {
        let rules = rules();
        let mut sim = Simulation::new();
        sim.session.game_mode_nonzero = true;
        let owner = sim.interner.intern("Computer1");
        sim.houses
            .insert(owner, HouseState::new(owner, 0, None, false, 0, 10));
        let building = sim
            .spawn_object("GAPOWR", "Computer1", 30, 31, 0, &rules, &BTreeMap::new())
            .expect("defense building");
        let cell = pack_base_plan_cell(30, 31);
        sim.houses.get_mut(&owner).unwrap().base_plan.nodes = vec![
            BasePlanNode {
                type_or_control: 0,
                packed_cell: cell,
                filled: true,
                retry_count: 8,
            },
            BasePlanNode {
                type_or_control: 1,
                packed_cell: cell,
                filled: false,
                retry_count: -2,
            },
        ];

        assert_eq!(sim.techno_limbo(building), super::ConcealOutcome::Concealed);
        let plan = &sim.houses[&owner].base_plan;
        assert_eq!(plan.nodes[0].type_or_control, -1);
        assert_eq!(plan.nodes[0].packed_cell, 0);
        assert!(plan.nodes[0].filled);
        assert_eq!(plan.nodes[0].retry_count, 8);
        assert_eq!(plan.nodes[1].type_or_control, 1);
        assert_eq!(plan.nodes[1].packed_cell, 0);
        assert!(!plan.nodes[1].filled);
        assert_eq!(plan.nodes[1].retry_count, -2);
        assert!(sim.entities().get(building).unwrap().lifecycle.in_limbo);
    }
}
