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
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::lifecycle_request::LifecycleRequest;
use crate::sim::occupancy::{
    BUILDING_OCCUPATION_BIT, CellListInsertion, cell_list_layer_for_entity, entity_occupancy_cells,
};
use crate::sim::passenger::PassengerRole;
use crate::sim::projectile::ProjectileTarget;
use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;
use crate::util::lepton::LEPTONS_PER_LEVEL;

use super::Simulation;
use super::display_layers::DisplayLayer;
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
/// and it skips the Techno `+0x500` / `+0x218` / CaptureManager block opened
/// at `0x00707AE7`. After that body returns, Foot's override independently
/// clears an exact ArchiveTarget match on BOTH controls (`4D99F1..4D99FC`).
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
    registry: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
    requested_facing: Option<u8>,
}

impl<'a> UninitContext<'a> {
    /// A caller's rules and overlay table, with no terrain override.
    pub(crate) const fn new(
        rules: Option<&'a RuleSet>,
        registry: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Self {
        Self {
            terrain: None,
            rules,
            registry,
            requested_facing: None,
        }
    }

    pub(crate) const fn with_rules(rules: &'a RuleSet) -> Self {
        Self {
            terrain: None,
            rules: Some(rules),
            registry: None,
            requested_facing: None,
        }
    }

    /// A nested receiver with an admitted RuleSet must retain the enclosing
    /// borrowed map and registry while supplying every type-reading lifecycle
    /// writer. SellBuilding's nested Foot Unlimbo is one such receiver.
    pub(crate) const fn requiring_rules<'b>(self, rules: &'b RuleSet) -> UninitContext<'b>
    where
        'a: 'b,
    {
        UninitContext {
            terrain: self.terrain,
            rules: Some(rules),
            registry: self.registry,
            requested_facing: self.requested_facing,
        }
    }

    /// Techno Unlimbo's direction for this synchronous reveal. The caller's
    /// raw Z stays in RevealPosition, through the existing type clamp before
    /// Mark; successful, alive Techno6F6DAA snaps the body before idle.
    pub(crate) const fn with_unlimbo_facing(self, requested_facing: Option<u8>) -> Self {
        Self {
            requested_facing,
            ..self
        }
    }

    /// The match's OverlayTypeClass table, for receivers that classify a
    /// cell's overlay (an ejected occupant's Scatter entry test).
    pub(crate) const fn with_registry(
        self,
        registry: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Self {
        Self { registry, ..self }
    }

    pub(crate) const fn registry(
        self,
    ) -> Option<&'a crate::rules::overlay_types::OverlayTypeRegistry> {
        self.registry
    }

    pub(crate) const fn terrain(
        self,
    ) -> Option<&'a crate::map::resolved_terrain::ResolvedTerrainGrid> {
        self.terrain
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
    /// Object5F4F1B accepted Foot admission: either the one class +1AC
    /// receiver returned zero or the caller's A8E7AC scope skipped it.
    /// Query-local height/list outputs never establish the object's pose.
    UnitEntryAdmitted,
}

impl Simulation {
    /// A8E7AC's caller-owned bracket. Native factory444575..444979/444EE6
    /// retains it through Unlimbo, Mark and radio callbacks; nested scopes
    /// increment/decrement the same dword. SpawnSurvivors443141..443288
    /// and passenger escape738030..7381A1 retain it through Scatter and
    /// immediate Walk/cell callbacks. Session GameMode is A8B238.
    pub(crate) fn with_object_placement_scope<R>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.object_placement_scope_depth = self.object_placement_scope_depth.wrapping_add(1);
        let result = operation(self);
        self.object_placement_scope_depth = self.object_placement_scope_depth.wrapping_sub(1);
        result
    }

    pub(crate) fn object_placement_scope_active(&self) -> bool {
        self.object_placement_scope_depth != 0
    }
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
const NULL_TARGET_CELL_SENTINEL: (u16, u16) = (0, 0);

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
    let [world_x, world_y] = crate::sim::movement::ground_pose::object_center_xy(entity);
    Some((
        u16::try_from(crate::util::lepton::lepton_to_cell(world_x)).ok()?,
        u16::try_from(crate::util::lepton::lepton_to_cell(world_y)).ok()?,
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

/// A caller's placement coordinate. Reuse the Location representation so a
/// native CoordStruct can retain its exact Z independently of height levels.
pub(crate) type RevealPosition = crate::sim::components::Position;

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
    /// Logic55AFB0 increments the process FPS sample even on terminal frames.
    LogicVisit,
    /// Logic55B5C3 updates existing lasers before the live object visits.
    LaserUpdate {
        frame: i32,
    },
    /// Copied LaserDraw54FE60 birth; no source/target pointer to detach.
    LaserCreated(crate::sim::combat::laser::LaserBirth),
    /// ObjectUnlimbo5F517A/5F5207 constructs and attaches a presentation trail.
    LineTrailConstructed {
        stable_id: u64,
        style: crate::sim::projectile::ProjectileLineTrail,
    },
    /// ObjectDtor5F3D56 detaches; its fading registry entry remains alive.
    LineTrailDetached {
        stable_id: u64,
    },
    RevealDisplay {
        stable_id: u64,
    },
    DisplayRemove {
        stable_id: u64,
    },
    DetachAttachedAnims {
        stable_id: u64,
    },
    DirtyTacticalRect {
        stable_id: u64,
    },
    ClearDrawnState {
        stable_id: u64,
    },
    ClearRedraw {
        stable_id: u64,
    },
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LifecycleTestEvent {
    RevealLimboCleared,
    RevealCoordinatesCommitted,
    MarkPut,
    UnlimboBodyFacingSnapped,
    UnlimboIdleMode,
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
    /// Native stock constructor/UnInit/scalar-dtor expiry comparison. Captures
    /// the shared dispatch boundary, including readiness and pending order.
    SmudgeExpiryBoundary {
        stable_id: u64,
        native_id: i32,
        native_cursor: u32,
        object_alive: bool,
        in_limbo: bool,
        health: i32,
        location: [i32; 3],
        pending: Vec<u64>,
        generic_objects: Vec<u64>,
        rng: [String; 3],
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
    DestroyRadioBreakCompleted {
        stable_id: u64,
    },
    DestroyDeselected {
        stable_id: u64,
    },
    DestroyNotifyBoundary {
        stable_id: u64,
    },
    BuildingNowDeadRunAway {
        building_id: u64,
        contact_id: u64,
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
    /// What a Fly `Stop_Moving` decided for aircraft `id`.
    FlyStopOrdered {
        id: u64,
        order: super::fly_orders::FlyStopOrder,
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

    /// The Location Z Object Unlimbo 0x5F4EC0 commits (SetLocation at
    /// `0x005F4FA8`), adapted from the level-based placement API. It passes its
    /// input coordinate through the type's virtual +0x6C (`0x005F4F88`):
    /// - BuildingType 0x464A70 keeps the XY and takes the floor there
    ///   (`0x578080`) as the Z, with no deck, whatever the input Z.
    /// - UnitType 0x747EB0 / InfantryType 0x5247D0 clamp that input to the
    ///   exact ground surface, whatever the locomotor, and add no bridge
    ///   offset. Authored bridge placement supplies its deck coordinate before
    ///   that clamp; translate our coarse deck request here. This coarse API
    ///   cannot recover all raw authored inputs (notably Unit input zero on
    ///   negative terrain); the report records that caller residual. See
    ///   docs/research/RAMP_UNIT_HEIGHT_GHIDRA_REPORT.md.
    /// - AircraftType keeps the coordinate (`0x0041CF80`), whose Z
    ///   AircraftClass::Unlimbo 0x414310 set first: a `MissileSpawn=` type
    ///   keeps its input (`0x00414338`), with no floor read; one without
    ///   Techno+3D4 (`0x00414342`) inside the playfield (`0x005785F0`) takes
    ///   the floor (`0x00414361`); any other the floor plus its FlightLevel
    ///   (type virtual +0xBC, `0x00414383`). Native comparison:
    ///   tools/spatial_oracle/aircraft_unlimbo_height.json.
    ///
    /// A falling paradrop keeps its drop's Z, and a `MissileSpawn=` aircraft
    /// the Z its caller staged on its limbo Location (the spawn launch's
    /// coordinate, `spawn_manager::launch_coordinate`). Exact-coordinate input
    /// is retained independently of the legacy coarse level. `None` keeps no exact Z, for:
    /// - a tube owner, whose own state carries the height;
    /// - a `MissileSpawn=` aircraft whose caller staged no exact Z (no
    ///   production caller does);
    /// - an Aircraft revealed without rules, which cannot read the type (the
    ///   reveals that pass none reach no Aircraft in production);
    /// - an object revealed without terrain (headless fixtures), except a
    ///   `MissileSpawn=` aircraft, whose Z reads no floor.
    fn unlimbo_z(
        &self,
        stable_id: u64,
        position: RevealPosition,
        context: UninitContext<'_>,
    ) -> Option<i32> {
        use crate::sim::movement::ground_pose::ground_surface_z_at;

        let xy = [
            i32::from(position.rx)
                .wrapping_mul(256)
                .wrapping_add(position.sub_x.to_num::<i32>()),
            i32::from(position.ry)
                .wrapping_mul(256)
                .wrapping_add(position.sub_y.to_num::<i32>()),
        ];
        let entity = self.substrate.entities.get(stable_id)?;
        if entity.parachute_state.is_some() {
            // Paradrop's Unlimbo coordinate is the drop coordinate, which
            // SetLocation then commits whole (`0x005F5A50`): the falling
            // object keeps the Z its drop gave it.
            return position.exact_z_leptons.or(entity.position.exact_z_leptons);
        }
        if entity.low_bridge_tube_state.is_some() {
            return None;
        }
        let aircraft_type = context
            .rules
            .filter(|_| entity.category == EntityCategory::Aircraft)
            .and_then(|rules| rules.object(self.interner.resolve(entity.type_ref())));
        if aircraft_type.is_some_and(|object| object.missile_spawn) {
            return position.exact_z_leptons.or(entity.position.exact_z_leptons);
        }
        let terrain = context.terrain().or(self.resolved_terrain.as_ref())?;
        let ground_z = ground_surface_z_at(xy, false, Some(terrain), None)?;
        match entity.category {
            EntityCategory::Structure => return Some(ground_z),
            EntityCategory::Aircraft => {
                let rules = context.rules?;
                let object = aircraft_type?;
                return Some(
                    if !entity.is_mission_only()
                        && self.reveal_position_is_in_playfield(position, context)
                    {
                        ground_z
                    } else {
                        ground_z.wrapping_add(object.flight_level(rules.general.flight_level))
                    },
                );
            }
            EntityCategory::Unit | EntityCategory::Infantry => {}
        }
        let level = terrain
            .native_fixed_cell_index((xy[0] / 256) as i16, (xy[1] / 256) as i16)
            .map_or_else(
                || terrain.shared_cell_dummy().snapshot().level,
                |index| terrain.cells()[index].level as i8,
            );
        let input_z = if let Some(input_z) = position.exact_z_leptons {
            input_z
        } else if entity.on_bridge && i32::from(position.z as i8) == i32::from(level) + 4 {
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
            .map(|entity| entity.position)
    }

    /// Compatibility convenience for already-admitted current-position callers.
    /// It still executes the complete result-bearing Reveal transaction.
    #[cfg(test)]
    pub(crate) fn reveal(&mut self, stable_id: u64) -> RevealOutcome {
        if self.substrate.anims.contains_key(stable_id) {
            let registered = self.reveal_anim(stable_id, None);
            return RevealOutcome::Revealed {
                logic_registered: registered,
            };
        }
        if self.substrate.particle_systems.contains_key(stable_id) {
            let registered = self.reveal_particle_system(stable_id, None);
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

    /// [`Self::reveal`] for an entity, with the rules context the type-reading
    /// Unlimbo writers need (Aircraft Techno+3D4 retention, Ground sort keys).
    pub(crate) fn reveal_entity_with_rules(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
    ) -> RevealOutcome {
        let Some(position) = self.current_reveal_position(stable_id) else {
            return RevealOutcome::Failed(RevealFailure::MissingObject);
        };
        self.try_reveal_entity_with_context(
            stable_id,
            RevealRequest {
                position,
                placement: PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            },
            UninitContext::with_rules(rules),
        )
    }

    /// `TechnoClass::Unlimbo`'s barrel elevation writes (`+0x370`,
    /// `0x006F6DC3`, `0x006F6DF5`), aimed by the object's `FireAngle=`, behind
    /// the alive gate that also guards its Added_To_Game (`0x006F6D04`).
    /// [`Self::try_reveal_entity_with_context`] makes them when it has rules;
    /// a caller that reveals without rules makes them after its Reveal.
    pub(crate) fn unlimbo_barrel_elevation(&mut self, stable_id: u64, rules: &RuleSet) {
        let Some(fire_angle) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|entity| entity.lifecycle.object_alive)
            .and_then(|entity| self.object_type(entity.type_ref(), rules))
            .map(|object| object.fire_angle)
        else {
            return;
        };
        let frame = self.session.binary_frame;
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.unlimbo_barrel_elevation(fire_angle, frame);
        }
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
            && !self.object_placement_scope_active()
            && !self.reveal_position_is_in_playfield(request.position, context)
        {
            return RevealOutcome::Failed(RevealFailure::RejectedEarly);
        }

        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.lifecycle.in_limbo = false;
        }
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::RevealLimboCleared);

        // Foot4D9C60 adjusts query-local height/list pointers. OnBridge and
        // the caller's requested XYZ remain independent through Unlimbo.
        let position = request.position;
        let exact_z = self.unlimbo_z(stable_id, position, context);
        // RESIDUAL: `ObjectClass::Unlimbo` sets this Location through
        // SetLocation (vt+0x1B4 at 0x005F4FA8, before its Mark(1) at
        // 0x005F4FB4), which for a Foot also runs the OpenTopped rider tail.
        // VERA copies the parts, keeping no exact Z for a tube owner
        // (`unlimbo_z`), so it cannot go through `foot_set_location`
        // yet. Trigger: a loaded OpenTopped transport revealed away from its
        // riders. Frequency: none known in retail. Effect: the riders keep
        // their old Location.
        let terrain = context.terrain().or(self.resolved_terrain.as_ref());
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.position.rx = position.rx;
            entity.position.ry = position.ry;
            entity.position.z = position.z;
            entity.position.exact_z_leptons = exact_z;
            entity.position.sub_x = position.sub_x;
            entity.position.sub_y = position.sub_y;
            // The committed Location's GetHeight; a parachute keeps the
            // altitude its drop gave it.
            if exact_z.is_some() && entity.parachute_state.is_none() {
                let height =
                    crate::sim::movement::air_movement::current_fly_height(entity, terrain);
                crate::sim::movement::ground_pose::mirror_height(entity, height);
            }
        }
        if let Some(rules) = context.rules {
            self.reposition_building_anim_slots(stable_id, rules);
        }
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
        // Techno6F6CB1 calls Object Unlimbo, and its failed return6F6CB8
        // precedes the +3D5 establishment at6F6CFE. Failed Mark retains the
        // prior byte. Headless fixtures have no MapClass authority and retain
        // the constructor default through this existing writer.
        self.establish_entity_playfield_membership_on_unlimbo(stable_id, context);
        // `TechnoClass::Unlimbo` calls Added_To_Game once placement succeeded
        // (`0x006F6D8F`), behind its alive gate (`0x006F6D04`: a dead Techno
        // returns success before it).
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.lifecycle.object_alive)
        {
            self.update_house_presence(stable_id, true);
        }
        // Techno6F6DAA: only successful placement of an alive Techno snaps
        // the body to this call's direction, before barrel elevation and idle.
        // Its original6F6CA0 body is retained in the shared Unlimbo evidence.
        if let Some(facing) = context.requested_facing
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && entity.lifecycle.object_alive
        {
            entity
                .body_facing
                .snap(u16::from(facing) << 8, self.session.binary_frame);
            #[cfg(test)]
            self.trace_lifecycle_for_test(LifecycleTestEvent::UnlimboBodyFacingSnapped);
        }
        // The barrel elevation writes follow the body snap (`0x006F6DC3`).
        if let Some(rules) = context.rules {
            self.unlimbo_barrel_elevation(stable_id, rules);
        }
        // TechnoClass::Unlimbo 0x006F6E2A..0x006F6E4F: Enter_Idle_Mode(1, 1),
        // Ready_To_Commence and Commence, ahead of its second mode-one query
        // and of Foot4D722F's owner discovery below, which then observes the
        // committed idle mission rather than a queued one.
        if let Some(rules) = context.rules
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| entity.lifecycle.object_alive)
        {
            #[cfg(test)]
            if context.requested_facing.is_some() {
                self.trace_lifecycle_for_test(LifecycleTestEvent::UnlimboIdleMode);
            }
            super::foot_unlimbo_idle_mode(self, stable_id, rules, context.registry());
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
        // Techno6F6ED2/6F6EDE publishes its current threat before the Foot
        // tail. Failed Mark and the dead successful Reveal arm publish none.
        if let Some(rules) = context.rules {
            self.spatial_threat_after_unlimbo(stable_id, rules, context.terrain());
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
        if let Some(entity) = self.substrate.entities.get(stable_id)
            && entity.category != EntityCategory::Structure
        {
            self.record_techno_discovery(stable_id, entity.owner());
        }
        let high_flight = self.foot_neighbors_after_unlimbo(stable_id, context.rules);
        // Foot4D72B2/+54 requires high flight, then Type ConsideredAircraft
        // (+D96) admits AirTrackerAdd4D72DB, whatever the locomotor. Mark
        // itself never adds anything. RESIDUAL: a Reveal without rules
        // (`try_reveal_entity`, `reveal`, spawn
        // `unlimbo`) reads +D96 as its default, Aircraft only, so a
        // high-flying ConsideredAircraft Unit revealed that way waits for the
        // per-tick `sync_air_spatial_membership`.
        if high_flight
            && self.substrate.entities.get(stable_id).is_some_and(|e| {
                context
                    .rules
                    .and_then(|r| r.object(self.interner.resolve(e.type_ref())))
                    .map_or(e.category == EntityCategory::Aircraft, |o| {
                        o.considered_aircraft
                    })
            })
        {
            self.aircraft_tracker_add(stable_id);
        }
        // FootUnlimbo4D72EA..4D72F4 copies Type+2F0 into Foot+530 after
        // neighbors and optional AirTracker, including the dead-Techno success
        // arm. Both early and Mark refusals above must retain the prior value.
        if let Some(rules) = context.rules
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && matches!(
                entity.category,
                EntityCategory::Infantry | EntityCategory::Unit | EntityCategory::Aircraft
            )
            && let Some(object) = rules.object(self.interner.resolve(entity.type_ref()))
        {
            entity
                .navigation
                .retain_threat_avoidance_after_unlimbo(object.threat_avoidance_coefficient);
        }
        // Unit737BBE..737BD2 / Aircraft414403..414417 snap Secondary after
        // Foot success, including dead-Techno success. The Techno body snap
        // above has its own alive gate and precedes idle. Diagnostic callers
        // without a direction retain the existing facing seam.
        if let Some(facing) = context.requested_facing
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && matches!(
                entity.category,
                EntityCategory::Unit | EntityCategory::Aircraft
            )
            && let Some(barrel) = entity.barrel_facing.as_mut()
        {
            barrel.snap(u16::from(facing) << 8, self.session.binary_frame);
        }
        // Unit737BF5..737C75 resets the SAME +F8 StageClass after Foot
        // success. E18/E19 are Unit-section booleans, not General references.
        // The existing owner preserves FC/110; +104 is native stack padding.
        // Original execution/RNG: anytown_damage/unit_unlimbo stage controls.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category == EntityCategory::Unit)
        {
            let visceroid = context
                .rules
                .and_then(|rules| {
                    self.substrate
                        .entities
                        .get(stable_id)
                        .and_then(|entity| rules.object(self.interner.resolve(entity.type_ref())))
                })
                .is_some_and(|object| object.small_visceroid || object.large_visceroid);
            let value = if visceroid {
                self.scenario_rng.next_range_i32_inclusive(0, 29)
            } else {
                0
            };
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.restart_native_stage(
                    value,
                    self.session.binary_frame as i32,
                    i32::from(visceroid),
                );
            }
        }
        // Aircraft4143A8 follows successful Foot Unlimbo, including the dead
        // Techno success arm. Failed placement above must not promote +3D4.
        // RESIDUAL: the class Unlimbo tails also write what VERA does not
        // write here.
        // - Concrete coordinate/height Unlimbo supplies a direction to this
        //   shared lifecycle owner. Low-level diagnostic Reveal has no
        //   direction argument and retains the existing facing seam.
        // - The Aircraft +0x6C9 latch (`0x004143F2..0x004143FC`), set when a
        //   first passenger rides at Unlimbo. Dormant: carriers take their
        //   passengers after Unlimbo, and `superweapon::paradrop` records why
        //   VERA keeps no latch for the paradrop's own writes (`0x0065E7B8`,
        //   `0x0065DCE9`).
        if let Some(rules) = context.rules
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            let type_id = self.interner.resolve(entity.type_ref());
            entity.retain_aircraft_unlimbo_control(rules, type_id);
            if entity.category == EntityCategory::Aircraft
                && let Some(object) = rules.object(type_id)
            {
                let height = crate::sim::movement::air_movement::current_fly_height(
                    entity,
                    context.terrain().or(self.resolved_terrain.as_ref()),
                );
                entity.finish_aircraft_unlimbo(
                    height,
                    object.flight_level(rules.general.flight_level),
                    self.session.binary_frame as i32,
                );
            }
        }
        if let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && entity.category == EntityCategory::Infantry
            && entity.sight_is_zero
        {
            entity.discovery.discovered_by_current_house = false;
        }
        // Then `InfantryClass::Unlimbo` marks the man's sub-cell through his
        // vt+0xF0 (`0x005217C0`) unless he was placed more than a deck above
        // the ground (`0x0051E0F6..0x0051E10E`). His Mark never writes it.
        if let Some((owner, coord)) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|entity| entity.category == EntityCategory::Infantry)
            .map(|entity| {
                (
                    entity.owner(),
                    crate::sim::movement::ground_pose::position_world_coord(&entity.position),
                )
            })
        {
            let terrain = context.terrain().or(self.resolved_terrain.as_ref());
            let ground = crate::sim::movement::ground_pose::ground_surface_z_at(
                [coord.x, coord.y],
                false,
                terrain,
                None,
            )
            .unwrap_or(coord.z);
            if coord.z <= ground.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS) {
                crate::sim::movement::walk_head::raw_at(
                    &mut self.substrate.raw_cell_occupation,
                    owner,
                    coord,
                    true,
                    terrain,
                    self.path_grid.as_deref(),
                );
                #[cfg(test)]
                self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationMarked);
            }
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
            self.mark_ai_repairable_at_unlimbo(stable_id);
            // Building440D07: recompute from the PRE-append House+68. Failed
            // Mark and the earlier dead/attached-upgrade arms never reach it.
            if let Some(rules) = context.rules
                && let Some(owner) = self
                    .substrate
                    .entities
                    .get(stable_id)
                    .filter(|entity| entity.category == EntityCategory::Structure)
                    .map(|entity| entity.owner())
            {
                self.recalculate_house_base_geometry(owner, rules);
            }
            // Building440D13: IncrementFactoryCount right after it.
            self.update_house_tracking(
                stable_id,
                crate::sim::house_tracking::HouseTracking::increment_factory_count,
            );
            self.append_live_build_const(stable_id);
            self.append_house_base_building(stable_id);
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                // Building440D2D, after native House building registration.
                entity.reset_building_health_sample_at_unlimbo();
            }
            if let Some(rules) = context.rules() {
                // Unlimbo44119C clears this object's retained primary before
                //448070 asks the owning house for another live primary.
                self.production.clear_primary_factory(stable_id);
                crate::sim::production::initialize_factory_primary(self, stable_id, rules);
            }
            self.refresh_waypoint_edge_from_committed_structure(stable_id);
            self.mark_building_base_reservation_with_arg(stable_id, false, context);
            self.fill_base_plan_from_successful_building_unlimbo(stable_id);
        }
        self.submit_entity_display(stable_id, context.rules, context.terrain());
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

    /// `BuildingClass::Unlimbo 0x00440B4F..0x00440B7A`, after the Techno
    /// Unlimbo and its alive gate: outside a campaign, a building of a house
    /// no human controls whose house type is not `MultiplayPassive=` becomes
    /// AI-repairable (`+0x6CB`), whatever its map line said.
    fn mark_ai_repairable_at_unlimbo(&mut self, stable_id: u64) {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let game_mode_nonzero = self.session.game_mode_nonzero;
        if entity.category == EntityCategory::Structure
            && game_mode_nonzero
            && self.houses.get(&entity.owner()).is_some_and(|house| {
                !house.is_controlled_by_human(game_mode_nonzero) && !house.multiplay_passive
            })
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            entity.ai_repairable = true;
        }
    }

    /// Shared TechnoDiscovered6F4960, called by CellAddContent47E9DC with
    /// PlayerPtr and FootUnlimbo4D722F with the object's owner. The queried
    /// House50B730 is the VIEWER; dirty flags and the1F4 notification belong
    /// to the object's owner at the time of the callback. The whole native
    /// capture comparison exercises this before the701735 owner swap.
    /// ObjectDiscovered5F5930 accepts a nonnull House without mutation.
    /// Live attached Tag event4 at6F4A18 remains a trigger residual; the
    /// ordinary joined Engineer route carries NULL Tags. Missing House state
    /// is an unavailable headless boundary, not an invented native identity.
    pub(super) fn record_techno_discovery(&mut self, stable_id: u64, viewer: InternedId) {
        let Some(current_house) = self.session.current_house else {
            return;
        };
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let current = viewer == current_house;
        let history = entity.discovery;
        if (current && history.discovered_by_current_house)
            || (!current && history.discovered_by_other_house)
        {
            return;
        }
        let Some(house) = self.houses.get(&viewer) else {
            return;
        };
        let controlled = house.is_controlled_by_human(self.session.game_mode_nonzero);
        let owner = entity.owner();
        let entity = self
            .substrate
            .entities
            .get_mut(stable_id)
            .expect("discovered Techno");
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
            //6F49DE invalidates both House bytes even for a Foot. Another
            //Building's repaired health can therefore become visible before
            //its next own health sample, through this independent writer.
            self.invalidate_house_power(owner, true);
            if !history.owned_by_current_house
                && let Some(house) = self.houses.get_mut(&owner)
            {
                house.notify_discovered_by_current_house();
            }
        }
    }

    /// CellAddContent47E953..47E9DC's discovery gate. In active YR the
    /// IsFogged5865E0 body always returns false. Any noncampaign CellPUT
    /// therefore calls DiscoveredBy(PlayerPtr); campaigns do so only when
    /// the height-projected Cell coordinate is shrouded. Reuse the existing
    ///586360 and Fog authority instead of a presentation visibility cache.
    pub(crate) fn discover_cell_put_object(&mut self, id: u64, cell: (u16, u16)) {
        let Some(viewer) = self.session.current_house else {
            return;
        };
        if !self.session.game_mode_nonzero {
            let Some(terrain) = self.resolved_terrain.as_ref() else {
                return;
            };
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            if !crate::sim::vision::cell_is_shrouded(&self.fog, &cells, viewer, cell) {
                return;
            }
        }
        self.record_techno_discovery(id, viewer);
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

    /// Mark(PUT) through the object's vt+0x124: `FootClass::Mark`
    /// ([`Simulation::foot_mark_put`]) for Infantry, Unit and Aircraft, the
    /// building transaction below for a Structure.
    pub(crate) fn mark_entity_put(&mut self, stable_id: u64, context: UninitContext<'_>) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(stable_id) else {
            return false;
        };
        if entity.category != EntityCategory::Structure {
            return self.foot_mark_put(stable_id, context.rules, context.registry());
        }
        if entity.lifecycle.cell_marked {
            return false;
        }
        entity.lifecycle.cell_marked = true;
        let cells = entity_occupancy_cells(entity);
        let layer = cell_list_layer_for_entity(
            entity,
            context.terrain().or(self.resolved_terrain.as_ref()),
        );
        let insertion = CellListInsertion::from_category(entity.category);
        let foundation = entity.foundation.clone();
        let hidden_profile = entity.building_hidden_occupancy;
        let current_cell = (entity.position.rx, entity.position.ry);
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

        if let Some(layer) = layer {
            for &(rx, ry) in &cells {
                self.substrate
                    .occupancy
                    .add(rx, ry, stable_id, layer, None, insertion);
                self.discover_cell_put_object(stable_id, (rx, ry));
            }
            #[cfg(test)]
            self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationListLinked);
            if layer == crate::sim::movement::locomotor::MovementLayer::Ground
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
            for &(rx, ry) in &cells {
                self.substrate
                    .raw_cell_occupation
                    .mark_ground(rx, ry, BUILDING_OCCUPATION_BIT);
            }
            #[cfg(test)]
            if !cells.is_empty() {
                self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationMarked);
            }
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
            && cell_list_layer_for_entity(entity, self.resolved_terrain.as_ref())
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
            || cell_list_layer_for_entity(
                entity,
                context.terrain().or(self.resolved_terrain.as_ref()),
            ) != Some(crate::sim::movement::locomotor::MovementLayer::Ground)
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

    /// Mark(REMOVE) through the object's vt+0x124: `FootClass::Mark`
    /// ([`Simulation::foot_mark_remove`]) for Infantry, Unit and Aircraft, the
    /// building transaction below for a Structure.
    pub(crate) fn unmark_entity_remove(
        &mut self,
        stable_id: u64,
        context: UninitContext<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(stable_id) else {
            return false;
        };
        if entity.category != EntityCategory::Structure {
            return self.foot_mark_remove(stable_id, context.rules, context.registry());
        }
        if !entity.lifecycle.cell_marked {
            return false;
        }
        entity.lifecycle.cell_marked = false;
        let cells = entity_occupancy_cells(entity);
        let layer = cell_list_layer_for_entity(
            entity,
            context.terrain().or(self.resolved_terrain.as_ref()),
        );
        let foundation = entity.foundation.clone();
        let hidden_profile = entity.building_hidden_occupancy;
        let current_cell = (entity.position.rx, entity.position.ry);
        if let Some(layer) = layer {
            for &(rx, ry) in &cells {
                self.substrate
                    .occupancy
                    .remove_on_layer(rx, ry, stable_id, layer);
            }
            #[cfg(test)]
            self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationListUnlinked);
            if layer == crate::sim::movement::locomotor::MovementLayer::Ground
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
            for &(rx, ry) in &cells {
                self.substrate
                    .raw_cell_occupation
                    .clear_ground(rx, ry, BUILDING_OCCUPATION_BIT);
            }
            #[cfg(test)]
            if !cells.is_empty() {
                self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationCleared);
            }
        }
        true
    }

    /// Own the represented bridge DropIn's complete cell-list relayer.
    /// Native ObjectClass::DropIn 0x005F4160 removes membership while OnBridge
    /// is set, then clears it and re-enters through the multi-cell hooks.
    /// The fresh entry order also owns reconstruction after snapshot restore.
    ///
    /// This preserves the existing Rust ground snap and locomotor reset for an
    /// object that is not falling. DropIn writes no Location, so an object
    /// already falling onto the deck keeps its Z and falls on to the ground
    /// ([`Simulation::advance_fall`]). Native falling/unchanged-Z for the
    /// others, IsABomb (`+0x8F`), concrete raw occupation callbacks, and
    /// hidden-building occupation entry remain DRIFT. Add/RemoveContent skip Infantry raw callbacks;
    /// full Mark/Unmark would also discard reservations and building smudges.
    pub(super) fn drop_in_bridge_member(&mut self, stable_id: u64) {
        use crate::sim::movement::locomotor::MovementLayer;
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let cells = entity_occupancy_cells(entity);
        let current_cell = (entity.position.rx, entity.position.ry);
        let sub_cell = entity.sub_cell;
        let insertion = CellListInsertion::from_category(entity.category);
        let ground_level = self
            .terrain_cell_level(current_cell.0, current_cell.1)
            .unwrap_or(0);
        for &(rx, ry) in &cells {
            self.substrate
                .occupancy
                .remove_on_layer(rx, ry, stable_id, MovementLayer::Bridge);
        }
        let entity = self
            .substrate
            .entities
            .get_mut(stable_id)
            .expect("member remains represented");
        entity.on_bridge = false;
        entity.position.z = ground_level;
        if !entity.is_falling_down() {
            entity.position.exact_z_leptons = None;
        }
        entity.movement_target = None;
        if let Some(loco) = entity.locomotor.as_mut() {
            loco.layer = MovementLayer::Ground;
        }
        // Rebuild only the derived vehicle projection: serialized head-to,
        // handoff and current-cleared facts retain their existing owners.
        self.substrate
            .cell_occupation
            .reconcile_entity(entity, &self.substrate.occupancy);
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

    /// Fixture boundary: puts a fixture object on the map and marks it
    /// (idempotent). Fixtures are constructed in Limbo, which Mark refuses
    /// (`0x005F5854`), so this clears InLimbo first.
    #[cfg(test)]
    pub(crate) fn add_entity_occupancy(&mut self, stable_id: u64) {
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.lifecycle.in_limbo = false;
        }
        let _ = self.mark_entity_put(stable_id, UninitContext::default());
    }

    /// Fixture boundary; common lifecycle code calls the private unmark
    /// transaction instead.
    #[cfg(test)]
    pub(crate) fn remove_entity_occupancy(&mut self, stable_id: u64) {
        self.unmark_entity_remove(stable_id, UninitContext::default());
    }

    /// One air locomotor's Process visit: a Fly's (`0x004CCB40`,
    /// [`Self::fly_process`]) or a Jumpjet's (`world::jumpjet_cruise`),
    /// then the tracker membership of an object that left the map's lists.
    pub(crate) fn tick_air_movement_with_cell_lists_one(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> crate::sim::movement::air_movement::AirMovementTickStats {
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::movement::locomotor::MovementLayer;

        let fly = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                entity.lifecycle.object_alive
                    && !entity.lifecycle.in_limbo
                    && entity.locomotor.as_ref().is_some_and(|locomotor| {
                        locomotor.kind == LocomotorKind::Fly
                            && locomotor.layer == MovementLayer::Air
                    })
            });
        if fly {
            let stats = self.fly_process(stable_id, rules, registry);
            if !stats.impact {
                self.sync_air_spatial_membership(stable_id);
            }
            return stats;
        }
        let jumpjet_layer_before = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|entity| {
                entity
                    .locomotor
                    .as_ref()
                    .is_some_and(|l| l.active_kind() == LocomotorKind::Jumpjet)
            })
            .and_then(|_| self.entity_display_layer(stable_id, rules));
        // A cruising Jumpjet runs the native Update/State3 body
        // (`world::jumpjet_cruise`).
        let stats = match self.tick_jumpjet_cruise_one(stable_id, rules, registry) {
            // State 5's impact notice UnInits the wreck, so `Process`'s layer
            // tail finds it dead (`0x0054B16C`); the object turn commits it.
            Some(stats) if stats.impact => return stats,
            Some(stats) => stats,
            None => crate::sim::movement::air_movement::AirMovementTickStats::default(),
        };
        self.sync_air_spatial_membership(stable_id);
        if let Some(before) = jumpjet_layer_before {
            self.complete_jumpjet_display_process(stable_id, before, rules);
        }
        stats
    }

    /// Fly4CCB40 ->4CD2A0 after4CD600's movement/height Mark pair.
    /// Non-Landable aircraft return before the phase's OWN Mark/Display pair.
    /// Landing runs first, then rechecks takeoff; both resubmit on equal layers.
    /// Return whether the phase performed the Display transaction.
    pub(super) fn complete_fly_phase(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        let admitted = self.substrate.entities.get(id).is_some_and(|entity| {
            entity.lifecycle.object_alive
                && entity.health.current > 0
                && entity
                    .locomotor
                    .as_ref()
                    .is_some_and(|l| l.powered && l.fly_runtime().is_some())
        });
        if !admitted {
            return false;
        }
        let entity = self.substrate.entities.get(id).unwrap();
        let non_landable_level = (entity.category == EntityCategory::Aircraft)
            .then(|| {
                rules.and_then(|r| {
                    r.object(self.interner.resolve(entity.type_ref()))
                        .filter(|object| !object.landable)
                        .map(|object| object.flight_level(r.general.flight_level))
                })
            })
            .flatten();
        if let Some(flight_level) = non_landable_level {
            self.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .locomotor
                .as_mut()
                .unwrap()
                .fly_runtime_mut()
                .unwrap()
                .force_non_landable_flight(flight_level);
            return false;
        }
        if !entity
            .locomotor
            .as_ref()
            .unwrap()
            .fly_runtime()
            .unwrap()
            .has_phase_callback()
        {
            return false;
        }
        let before = self.entity_display_layer(id, rules);
        self.foot_mark_remove(id, rules, registry);
        self.substrate.display.remove(id);
        if self
            .substrate
            .entities
            .get(id)
            .and_then(|e| e.locomotor.as_ref())
            .and_then(|l| l.fly_runtime())
            .is_some_and(|s| s.landing())
        {
            self.apply_fly_landing_callback(id, rules, registry);
        }
        if self
            .substrate
            .entities
            .get(id)
            .and_then(|e| e.locomotor.as_ref())
            .and_then(|l| l.fly_runtime())
            .is_some_and(|s| s.taking_off())
        {
            self.apply_fly_takeoff_callback(id, rules);
        }
        let after = self.entity_display_layer(id, rules);
        if before != after {
            // `0x004CD3B6` asks about the cell under the aircraft (MapAtCoord).
            let coord = crate::sim::movement::ground_pose::position_world_coord(
                &self.substrate.entities.get(id).unwrap().position,
            );
            let below = NavTargetRef::cell((coord.x / 256) as u16, (coord.y / 256) as u16);
            if after == Some(super::display_layers::DisplayLayer::GROUND)
                && !self
                    .substrate
                    .entities
                    .get(id)
                    .and_then(|e| e.locomotor.as_ref())
                    .and_then(|l| l.fly_runtime())
                    .is_some_and(|s| s.taking_off())
                && !self.foot_land_zone_clear(id, below, rules)
            {
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity
                        .locomotor
                        .as_mut()
                        .unwrap()
                        .fly_runtime_mut()
                        .unwrap()
                        .reject_landing_cell();
                    entity.on_bridge = false;
                }
                self.foot_mark_remove(id, rules, registry);
                let height = crate::sim::movement::air_movement::current_fly_height(
                    self.substrate.entities.get(id).unwrap(),
                    self.resolved_terrain.as_ref(),
                );
                self.set_object_height(id, height.wrapping_add(10), rules, registry);
                self.foot_mark_put(id, rules, registry);
            } else {
                self.finish_fly_layer_transition(id, after, rules);
            }
        }
        // Display4A9720 has no limbo gate, so the tail resubmits even an owner
        // the landing retry's C4 receiver just destroyed; store removal expires
        // that registration. Object Mark5F5850 refuses a Limbo owner, so the
        // cell lists never regain it.
        self.submit_entity_display(id, rules, None);
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|e| !e.lifecycle.in_limbo)
        {
            self.foot_mark_put(id, rules, registry);
        }
        true
    }

    /// Admitted callback4CE680. The phase caller owns Mark/Display sequencing;
    /// the callback owns flag clearing and the two existing facing controllers.
    pub(super) fn apply_fly_takeoff_callback(&mut self, id: u64, rules: Option<&RuleSet>) {
        use crate::sim::movement::{air_movement, fly_height::TakeoffFacing, ground_pose};
        let entity = self
            .substrate
            .entities
            .get(id)
            .expect("admitted Fly callback");
        let xy = ground_pose::position_world_xy(&entity.position);
        let mut height = air_movement::current_fly_height(entity, self.resolved_terrain.as_ref());
        //4CE696..4CE6DF: query structure even below416; bridge-normalize only
        // when not already OnBridge and high enough above a structural deck.
        if !entity.on_bridge {
            let bridge = self.resolved_terrain.as_ref().is_some_and(|terrain| {
                let cell =
                    terrain.native_cell_identity(((xy[0] / 256) as i16, (xy[1] / 256) as i16));
                terrain.native_cell_flags(cell) & 0x100 != 0
            });
            if bridge && height >= crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS {
                height = height.wrapping_sub(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
            }
        }
        let landing_base = crate::sim::aircraft::landing_base::landing_base(
            entity,
            &self.substrate.entities,
            rules.map(|r| (r, &self.interner)),
        );
        let entity = self
            .substrate
            .entities
            .get_mut(id)
            .expect("admitted Fly callback");
        air_movement::ensure_fly_secondary_facing(entity);
        let state = entity
            .locomotor
            .as_mut()
            .unwrap()
            .fly_runtime_mut()
            .unwrap();
        let destination = state.destination();
        match state.complete_takeoff(height, landing_base) {
            TakeoffFacing::Unchanged => {}
            TakeoffFacing::SecondaryToPrimaryDestination => {
                let desired = entity.body_facing.destination();
                entity
                    .barrel_facing
                    .as_mut()
                    .unwrap()
                    .set(desired, self.session.binary_frame);
            }
            TakeoffFacing::PrimaryToDestination => {
                // Original also evaluates the zero delta; do not special-case
                // it to the current heading. The shared native table owns it.
                let desired = crate::util::direction_tables::facing16_from_delta(
                    destination.x.wrapping_sub(xy[0]),
                    destination.y.wrapping_sub(xy[1]),
                );
                entity.body_facing.set(desired, self.session.binary_frame);
                entity
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .fly_runtime_mut()
                    .unwrap()
                    .target_speed = crate::util::fixed_math::SIM_ONE;
            }
        }
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
        } else if self
            .smudge_grid
            .as_ref()
            .and_then(|grid| grid.object(stable_id))
            .is_some()
        {
            Some(ObjectKind::Smudge)
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
            ObjectKind::Smudge => false,
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
            ObjectKind::Smudge => assert!(!member, "Smudge {stable_id} never joins Logic"),
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
        if kind == ObjectKind::Smudge {
            return false;
        }
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

    pub(crate) fn reveal_anim(&mut self, stable_id: u64, rules: Option<&RuleSet>) -> bool {
        // A physically retained Destroy/UnInit object has already left Logic
        // and Display. Its independent19B byte can still be zero, so it cannot
        // be resurrected through Reveal before the common deferred drain.
        if self.substrate.pending_delete.contains(&stable_id)
            || !self
                .substrate
                .anims
                .get(stable_id)
                .is_some_and(|anim| !anim.runtime.inactive)
        {
            return false;
        }
        self.mark_anim_display(stable_id, true);
        self.submit_anim_display(stable_id, rules);
        self.register_logic_object(stable_id)
    }

    pub(crate) fn conceal_anim(&mut self, stable_id: u64) -> bool {
        self.substrate.display.remove(stable_id);
        self.mark_anim_display(stable_id, false);
        self.unregister_logic_object(stable_id)
    }

    /// `ObjectClass::Unlimbo` inside `VoxelAnimClass::Constructor @ 0x007493B0`
    /// reveals the piece, which is what puts it in the LogicClass vector.
    pub(crate) fn reveal_voxel_anim(&mut self, stable_id: u64) -> bool {
        if !self.substrate.voxel_anims.contains_key(stable_id) {
            return false;
        }
        // VoxelAnim VT7F6318+78 ->74A960: always Air.
        self.submit_object_display(stable_id, DisplayLayer::AIR, None);
        self.register_logic_object(stable_id)
    }

    pub(crate) fn reveal_particle_system(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
    ) -> bool {
        if !self.substrate.particle_systems.contains_key(stable_id) {
            return false;
        }
        // ParticleSystem VT7EFB9C+78 ->62FE80: always Ground. Rules resolve
        // the existing Building peers' GetYSort adjustments during insertion.
        self.submit_object_display(stable_id, DisplayLayer::GROUND, rules);
        self.register_logic_object(stable_id)
    }

    pub(crate) fn conceal_particle_system(&mut self, stable_id: u64) -> bool {
        self.substrate.display.remove(stable_id);
        self.unregister_logic_object(stable_id)
    }

    pub(crate) fn register_terrain_object(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
    ) -> bool {
        if !self
            .production
            .terrain_objects
            .get(&stable_id)
            .is_some_and(|terrain| terrain.is_live())
        {
            return false;
        }
        // Terrain ctor71BC76 reveals at the retained type-adjusted coordinate;
        // Object5F4260 selects Ground even on elevated cells.
        self.submit_object_display(stable_id, DisplayLayer::GROUND, rules);
        self.register_logic_object(stable_id)
    }

    pub(crate) fn register_projectile(&mut self, stable_id: u64, flat: bool) -> bool {
        if self.projectiles.get(stable_id).is_none() {
            return false;
        }
        // Bullet Fire468B6D ->Submit; GetLayer468B90 reads type+2F7.
        let layer = if flat {
            DisplayLayer::SURFACE
        } else {
            DisplayLayer::AIR
        };
        self.submit_object_display(stable_id, layer, None);
        self.register_logic_object(stable_id)
    }

    pub(crate) fn register_wave(&mut self, stable_id: u64) -> bool {
        if self.waves.get(stable_id).is_none() {
            return false;
        }
        // Wave VT7F6BF4+78 ->75F890: always Air; Submit at75F952.
        self.submit_object_display(stable_id, DisplayLayer::AIR, None);
        self.register_logic_object(stable_id)
    }

    pub(crate) fn unregister_non_entity_object(&mut self, stable_id: u64) -> bool {
        self.substrate.display.remove(stable_id);
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
        if self.classify_object(stable_id) == Some(ObjectKind::Projectile) {
            // `ObjectClass::UnInit @ 0x005F65F0` broadcasts the bullet's
            // expiry before it leaves Logic; an anim riding it ends
            // (`AnimClass::PointerExpired @ 0x00425150`, `+0x17C`).
            self.expire_anim_attached_bullet(stable_id, None);
        }
        let _ = self.unregister_non_entity_object(stable_id);
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
        // enters `ObjectClass::Detach_All(1) @ 0x005F5280` after deselection and
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
        // The Building prelude of this Detach_All(1) abandons the building's
        // own factory (`0x0044EC01..0x0044EC21`).
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category == EntityCategory::Structure)
        {
            crate::sim::production::detach_building_factory(self, context.rules(), stable_id);
        }
        // The Foot prelude (`0x004D9744`) leaves the object's team, before
        // Limbo is set, so the object still takes its idle mode.
        self.leave_team(stable_id, false, context.rules());
        // This Detach_All(1) (`0x005F4D61`) also visits the concealed object
        // itself, so a live spawner's Limbo runs its SpawnManager's owner arm:
        // docked children UnInit with a zero regen timer, and airborne ones
        // turn kamikaze on the wing's target (`0x006B7100`).
        self.notify_pointer_expired(stable_id, context);

        if self.unmark_entity_remove(stable_id, context) {
            #[cfg(test)]
            self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealUnmarked);
        }

        self.substrate.display.remove(stable_id);
        self.lifecycle_outputs
            .push(LifecycleOutput::DisplayRemove { stable_id });
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealDisplayBoundary);
        self.lifecycle_outputs
            .push(LifecycleOutput::DetachAttachedAnims { stable_id });
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealAnimBoundary);
        // RESIDUAL: ObjectConceal5F4D81 stops Object+3C and5F4D89
        // detaches Object+50. Those ambient handles have no Rust owner yet.
        // They are separate from Foot+544/Anim+1A0/Building+6A0; aliasing
        // this boundary to their object ID wrongly cuts off those sounds.
        // Trigger: ObjectType ambient cues; effect: missing conceal teardown
        // once their upstream playback is implemented; no such handle exists
        // for this ordinary SQD chain.
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

        // ObjectConceal5F4E98 invokes vt+0x11C here, before +81 Limbo is set
        // at5F4E9E (the same byte tested by the5F4D45 entry guard).
        self.techno_clear_discovery_on_conceal(stable_id);
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
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

    /// `TechnoClass::ClearDiscoveryOnConceal @ 0x006F4A40` (vt+0x11C): an
    /// object whose house a human does not control
    /// ([`HouseState::is_controlled_by_human`], `0x0050B730`) forgets that
    /// the current house discovered it (`+0x41B`). `+0x41A` and `+0x41C`
    /// stay. Object Conceal calls it (`0x005F4E98`), and so does a paradrop
    /// plane that took a refused passenger back (`0x00415EC1`).
    ///
    /// [`HouseState::is_controlled_by_human`]:
    /// crate::sim::house_state::HouseState::is_controlled_by_human
    pub(crate) fn techno_clear_discovery_on_conceal(&mut self, stable_id: u64) {
        let human = self
            .substrate
            .entities
            .get(stable_id)
            .and_then(|entity| self.houses.get(&entity.owner()))
            .map(|house| house.is_controlled_by_human(self.session.game_mode_nonzero));
        if human == Some(false)
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            entity.discovery.discovered_by_current_house = false;
        }
    }

    /// TechnoClass Limbo sends synchronous BREAK to every contact before the
    /// common Object Conceal transaction.
    #[cfg(test)]
    pub(crate) fn techno_limbo(&mut self, stable_id: u64) -> ConcealOutcome {
        self.techno_limbo_with_context(stable_id, UninitContext::default())
    }

    pub(crate) fn techno_limbo_with_rules(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> ConcealOutcome {
        self.techno_limbo_with_context(stable_id, UninitContext::new(Some(rules), registry))
    }

    fn techno_limbo_with_context(
        &mut self,
        stable_id: u64,
        context: UninitContext<'_>,
    ) -> ConcealOutcome {
        if !self.substrate.entities.contains(stable_id) {
            return ConcealOutcome::MissingOrDead;
        }
        // Building445D8E: Recount on the first Limbo (Building4458CE skips a
        // repeated one), before the base recompute.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| !entity.lifecycle.in_limbo)
        {
            self.update_house_tracking(
                stable_id,
                crate::sim::house_tracking::HouseTracking::recount,
            );
            // 0x004459AE..0x004459CA (infantry) and the unit arm after it:
            // the building's self-heal counts leave its house on the same
            // first Limbo, each clamped at zero.
            if let Some(rules) = context.rules() {
                self.remove_house_self_heal(stable_id, rules);
            }
        }
        // Building445DA6 precedes Techno Limbo445DDA, including its pointer
        // expiry and InLimbo write. Building4458CE skips it on repeated Limbo;
        // UnInit's earlier expiry may already have removed this list entry.
        if let Some(rules) = context.rules()
            && let Some(owner) = self
                .substrate
                .entities
                .get(stable_id)
                .filter(|entity| {
                    entity.category == EntityCategory::Structure && !entity.lifecycle.in_limbo
                })
                .map(|entity| entity.owner())
        {
            self.recalculate_house_base_geometry(owner, rules);
        }
        // `InfantryClass::Limbo @ 0x0051DF10`, before FootClass::Limbo and
        // whether or not the man is already in limbo: its locomotor's
        // Stop_Movement_Animation (ILocomotion `+0xAC`, `0x0051DF30`), the
        // water state's constructor sentinel (`+0x6E8 = 2`), not prone
        // (`+0x6DB`) and Doing Ready (`+0x6C4`, stored without Do_Action).
        // A boarded man leaves his transport with these.
        if let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && entity.category == EntityCategory::Infantry
        {
            if let Some(locomotor) = entity.locomotor.as_mut() {
                locomotor.stop_movement_animation();
            }
            entity.mission_leaf.reset_infantry_water_state();
            if let Some(infantry) = entity.infantry.as_mut() {
                infantry.is_prone = false;
            }
            entity
                .mission_leaf
                .set_infantry_doing_verified(crate::sim::movement::infantry_action::DO_READY)
                .expect("Ready is in the Doing table");
        }
        self.foot_neighbors_before_limbo(stable_id);
        // Foot4DB260 dispatches vt+0x500 on the first Limbo (0x004DB2FB,
        // while A8E7AC is zero) before +9C(0): Infantry 0x0051DAF0, whose
        // Walk Stop clears the moving bytes Lock leaves.
        // RESIDUAL: a Unit's or Aircraft's +0x500 (Foot 0x004D55C0, the
        // locomotor's Stop_Moving) is not run here.
        if let Some(rules) = context.rules()
            && !self.object_placement_scope_active()
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| {
                    entity.category == EntityCategory::Infantry && !entity.lifecycle.in_limbo
                })
            && let Err(cause) = self.infantry_stop_driver(stable_id, rules, context.registry())
        {
            log::debug!("infantry {stable_id} Limbo Stop_Driver: {cause}");
        }
        // 0x004DB324: the active locomotor's +9C(0) on the first Limbo.
        self.locomotor_mark_all_occupation_bits_up(stable_id);
        // The Drive instance retains head-to and handoff projections of that
        // +9C(0) release; they leave with it on the first Limbo.
        if let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && !entity.lifecycle.in_limbo
            && let Some(loco) = entity.locomotor.as_mut()
        {
            crate::sim::occupancy::clear_drive_head_to_occupation_for_remove(
                loco,
                &mut self.substrate.cell_occupation,
                stable_id,
            );
        }
        // FootClass::Limbo (0x004DB260) then Locks the locomotor (+0xB0) on
        // the first Limbo, so a boarded or stored man keeps no Walk
        // destination or head to resume.
        if let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && !entity.lifecycle.in_limbo
            && let Some(locomotor) = entity.locomotor.as_mut()
        {
            locomotor.walk_lock();
        }
        // FootLimbo4DB353 follows the locomotor Lock and precedes the
        // air-tracker/Techno suffix. It detaches any shared Foot sound, with
        // no MoveSound latch gate or write. Unit7440B4, Infantry51DF53 and
        // Aircraft's direct vtable slot all delegate to this one Foot path.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                entity.category != EntityCategory::Structure && !entity.lifecycle.in_limbo
            })
        {
            self.sound_events
                .push(super::SimSoundEvent::ObjectSoundDetached { owner: stable_id });
        }
        self.release_foot_air_tracker_before_limbo(stable_id);
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
            // 6F6BD1: Removed_From_Game, also only on the first Limbo.
            self.update_house_presence(stable_id, false);
            // BuildingClass::Limbo 0x00445946..0x00445988: an opened
            // Helipad's docks leave its house's AirportDocks.
            if let Some(rules) = context.rules()
                && let Some(entity) = self.substrate.entities.get(stable_id)
                && entity.category == EntityCategory::Structure
                && entity.building_actually_placed
                && let Some(object) = self.object_type(entity.type_ref(), rules)
                && object.helipad
            {
                let owner = entity.owner();
                if let Some(house) = self.houses.get_mut(&owner) {
                    house.tracking.limbo_airport_docks(object.number_of_docks);
                }
            }
        }
        //6F6C2A/2F removes the retained contribution and clears+508 before
        //ObjectConceal; its result cannot roll these Techno writes back.
        if let Some(rules) = context.rules() {
            self.spatial_threat_before_limbo(stable_id, rules, context.terrain());
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
        // TechnoClass::Limbo releases the gattling loop and clears the report
        // latch (`0x006F6C6B`, `0x006F6C76`) ahead of its radio pass.
        self.gattling_limbo(stable_id);
        crate::sim::radio::broadcast_break(self, stable_id, None);
        self.object_conceal_with_context(stable_id, context)
    }

    /// Guard UnInit's legacy uncredited-loss fallback. Routed RecordTheKill
    /// callbacks already book their immediate House statistics, including
    /// Temporal's full-health callback. Unrouted destruction sites still book
    /// only their loss here; this does not certify their missing kill callback.
    /// House quantities move through Removed_From_Game and Remove_Tracking.
    pub(crate) fn record_destruction_once(&mut self, stable_id: u64) {
        let Some((owner, category, already_recorded, destroyed, dont_score)) =
            self.substrate.entities.get(stable_id).map(|entity| {
                (
                    entity.owner(),
                    entity.category,
                    entity.destruction_recorded,
                    // A Temporal erase leaves at full health with its kill
                    // already booked (`combat::record_kill_credit`); a
                    // crashing infantryman falls at Health 1 (`0x0051BC57`)
                    // after the kill that booked its loss.
                    entity.health.current == 0 || entity.killed_by.is_some() || entity.crashing,
                    entity.dont_score,
                )
            })
        else {
            return;
        };
        if already_recorded {
            return;
        }
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.destruction_recorded = true;
        }
        if destroyed
            && !dont_score
            && let Some(house) = self.houses.get_mut(&owner)
        {
            house.stats.record_loss(category);
        }
    }

    /// Object5F5765 invokes RecordKill for each actual exact-zero callback.
    /// A retained Health1 hull can reach this again with a different attacker;
    /// stale attribution and the prior UnInit guard must not swallow it.
    /// The receiver then captures this hit's credit, awards experience
    /// (702FF0), and consumes the score record (703003..7031DC) before Destroy.
    pub(crate) fn begin_receiver_kill_record(&mut self, id: u64) {
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            debug_assert_eq!(entity.health.current, 0);
            entity.destruction_recorded = false;
            entity.killed_by = None;
        }
    }

    /// ObjectClass's exact-zero callback transaction for an eligible
    /// `CausesDelayKill` building. Active gamemd runs the routed kill callback
    /// and virtual Destroy/reference notification before TechnoClass arms the
    /// timer and restores Alive/Health=1. This deliberately does not call
    /// UnInit, Limbo, record the destruction, or enqueue physical deletion.
    /// The kill callback ([`crate::sim::combat::record_the_kill`]) has already
    /// booked its House statistics before these Destroy callbacks.
    pub(crate) fn postmortem_exact_zero_callbacks(
        &mut self,
        stable_id: u64,
        _killer_owner: Option<InternedId>,
        context: UninitContext<'_>,
    ) {
        let Some(target) = self.substrate.entities.get(stable_id) else {
            return;
        };

        debug_assert_eq!(
            target.health.current, 0,
            "PostMortem Object callbacks run at exact zero"
        );
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::PostMortemKillBookkeeping { stable_id });
        self.object_destroy_callback(stable_id, context);
    }

    /// `ObjectClass::Detach_All(true)` (vtable `+0xDC`) as the exact-zero
    /// Destroy callback of `ObjectClass::ReceiveDamage` (`0x005F5765..0x005F57AF`):
    /// every killing hit runs it after the kill callback, before TechnoClass's
    /// death arm. The class prelude comes first: `BuildingClass::Detach_All
    /// @ 0x0044EBF0` sends RADIO OVER_OUT to every contact, and
    /// `FootClass::Detach_All @ 0x004D9720` to contact 0. `ObjectClass::
    /// Detach_All @ 0x005F5280` then deselects and announces the expiry
    /// (`0x007258D0`), leaving Logic, occupancy and liveness intact.
    ///
    /// RESIDUALS: the Building prelude also abandons the building's production
    /// at the kill (`0x0044EC01..0x0044EEC8`, `0x004FAA10`), deleting its
    /// object whether or not it is finished. A computer building's own factory
    /// is abandoned here (`production::detach_building_factory`), but VERA
    /// keeps the player's production per house, with no factory pointer on
    /// the building: the next production phase (`revalidate_and_step_factories`)
    /// abandons an unfinished build that lost its factory and a finished one
    /// once no factory of its category remains. Trigger: a house with two
    /// factories of a category loses the one its production is attached to.
    /// Effect: natively the object is refunded and deleted at the kill; VERA
    /// keeps it at the surviving factory. When VERA does abandon it, the
    /// refund's Cost_Of is priced at that later phase, so a FactoryPlant lost
    /// in the same frame changes it (a quarter of a vehicle's price for an
    /// Industrial Plant). The Foot prelude removes the object from its team
    /// (`TeamClass::Remove_Member @ 0x006EA870`, at `0x004D9744`) before its
    /// radio contact.
    pub(crate) fn object_destroy_callback(&mut self, stable_id: u64, context: UninitContext<'_>) {
        let Some(category) = self.substrate.entities.get(stable_id).map(|e| e.category) else {
            return;
        };
        match category {
            EntityCategory::Structure => {
                // The Building prelude opens with the building's own factory
                // (`0x0044EC01..0x0044EC21`).
                crate::sim::production::detach_building_factory(self, context.rules(), stable_id);
                crate::sim::radio::broadcast_break(self, stable_id, None);
            }
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft => {
                // `0x004D9744`: the Foot prelude leaves the object's team
                // first.
                self.leave_team(stable_id, false, context.rules());
                if let Some(contact) = self
                    .substrate
                    .entities
                    .get(stable_id)
                    .and_then(|entity| entity.radio_contacts.slot(0))
                {
                    crate::sim::radio::transmit(
                        self,
                        stable_id,
                        contact,
                        crate::sim::radio::RadioMessage::Break,
                        crate::sim::radio::RadioPayload::default(),
                        None,
                    );
                }
            }
        }
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::DestroyRadioBreakCompleted { stable_id });

        if let Some(target) = self.substrate.entities.get_mut(stable_id) {
            target.selected = false;
        }
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::DestroyDeselected { stable_id });

        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::DestroyNotifyBoundary { stable_id });
        self.notify_pointer_expired(stable_id, context);
    }

    /// The Building NowDead contact loop (`BuildingClass::ReceiveDamage`,
    /// `0x00442511..0x004425F4`) over the contacts the building held before
    /// the hit: one at least 0x100 leptons from the building's `GetCoords`
    /// centre, on a building that is not a `Helipad=`, is sent RUN_AWAY (0x17),
    /// so a War Miner on a destroyed refinery's pad leaves its unload for
    /// Harvest (`radio::receive`), then clears its Unit+0x500 pending entry
    /// (`0x004425AA`). RESIDUAL: the other arm — a nearer contact, or any
    /// contact of a helipad, takes the C4 kill (`vt+0x16C` with `Rules+0xFA8`,
    /// `0x004425B6..0x004425EE`), not wired (units at a building's centre and
    /// aircraft docked on a destroyed helipad survive).
    pub(crate) fn building_now_dead_contacts(
        &mut self,
        building_id: u64,
        contacts: &[u64],
        rules: Option<&RuleSet>,
    ) {
        let Some(rules) = rules else {
            return;
        };
        // `0x00442532`/`0x00442543`: both ends are GetCoords (vt+0x48).
        let Some((centre, helipad)) = self.substrate.entities.get(building_id).and_then(|b| {
            let object = self.object_type(b.type_ref(), rules)?;
            let centre = crate::sim::movement::ground_pose::object_get_coords(
                b,
                self.resolved_terrain.as_ref(),
            );
            Some((
                [
                    i64::from(centre.x),
                    i64::from(centre.y),
                    i64::from(centre.z),
                ],
                object.helipad,
            ))
        }) else {
            return;
        };
        for &contact in contacts {
            let Some(at) = self.substrate.entities.get(contact).map(|c| {
                crate::sim::movement::ground_pose::object_get_coords(
                    c,
                    self.resolved_terrain.as_ref(),
                )
            }) else {
                continue;
            };
            let d = [
                i64::from(at.x) - centre[0],
                i64::from(at.y) - centre[1],
                i64::from(at.z) - centre[2],
            ];
            // `0x00442586`: the truncated length against 0x100.
            let far = d.iter().map(|v| v * v).sum::<i64>() >= 0x100 * 0x100;
            if far && !helipad {
                #[cfg(test)]
                self.trace_lifecycle_for_test(LifecycleTestEvent::BuildingNowDeadRunAway {
                    building_id,
                    contact_id: contact,
                });
                crate::sim::radio::transmit(
                    self,
                    building_id,
                    contact,
                    crate::sim::radio::RadioMessage::RunAway,
                    crate::sim::radio::RadioPayload::default(),
                    Some(rules),
                );
                if let Some(unit) = self.substrate.entities.get_mut(contact) {
                    crate::sim::docking::building_dock::clear_pending_entry(unit);
                }
            }
        }
    }

    /// The Stun of TechnoClass::ReceiveDamage's death arm (`0x00702210`),
    /// after the death sounds and before the debris. Units, Infantry and
    /// Aircraft take `FootClass::Stun @ 0x004D5660`: the class setter's NULL
    /// destination (`0x004D5669`, [`Self::assign_null_destination`]), Path[0]
    /// = -1 (`0x004D5673`) and `Stop_Driver` (vtable `+0x500`), which is
    /// Foot's `0x004D55C0` for a Unit or an Aircraft, the locomotor's
    /// `Stop_Moving` ([`Self::locomotor_stop_moving`]), and Infantry's
    /// `0x0051DAF0` ([`Self::infantry_stop_driver`]). `TechnoClass::Stun @
    /// 0x006FCD40` follows: Assign_Target(NULL), a second NULL destination
    /// (`0x006FCD55`), RADIO OVER_OUT to every contact (`+0x280(3)`),
    /// SpawnManager Kill_All_Spawns `0x006B7100` and ClearAllTargets
    /// `0x006B7BB0`, Detach_All(1), and Deselect.
    ///
    /// Each accepted NULL destination reaches the locomotor's `Stop_Moving`
    /// through Foot's null arm, so a moving Jumpjet stops up to three times.
    /// A Jumpjet Unit's keeps a moving wreck flying to the cell under it and
    /// lifts a descent back into the climb, so `Process`'s crash latch
    /// engages wherever the kill found it. Unit `0x00741970` returns at once
    /// without a NavCom (its `+0x1F8` override down), so the NavCom the
    /// Stop_Driver's re-target writes is what the second NULL destination
    /// clears. A Jumpjet Infantry's placement draws on the Scenario RNG each
    /// time his moving byte is set, and his Stop_Driver's Do_Action at Health
    /// 0 re-enters Stop_Driver once more when it accepts. Native execution:
    /// `tools/spatial_oracle/jumpjet_infantry_crash`.
    ///
    /// Foot+6AD (Magnetron-held) would skip the spawn calls and Detach_All;
    /// VERA never sets it (the IsLocomotor arm is unported). On this path the
    /// spawn calls repeat the Destroy's owner arm and find nothing left to
    /// kill. Detach_All(1) repeats the broadcast the killing hit's Destroy
    /// already made (on a re-entered death arm, `0x0070202E..0x00702035`, that
    /// was an earlier call). The walk is not repeated: every Target write
    /// since then refused the Health-0 object (the setter through
    /// `assign_target_commits`, including the Destroy walk's own Restores; the
    /// scans, orders and legacy retaliation filter Health 0 themselves), so no
    /// listener can hold it again. The Foot prelude's contact-0 OVER_OUT is
    /// covered by the OVER_OUT to every contact. A living object's Stun (a
    /// sinking hull, a retreating aircraft leaving the map) runs the walk.
    /// Native execution of an Aircraft's: `tools/spatial_oracle/fly_stop`'s
    /// Stun rows, replayed by `fly_process_tests::fly_death_stun_matches_native_rows`.
    pub(crate) fn techno_death_stun(&mut self, stable_id: u64, context: UninitContext<'_>) {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let category = entity.category;
        let walking_infantry = category == EntityCategory::Infantry
            && entity.locomotor.as_ref().is_some_and(|locomotor| {
                locomotor.kind == crate::rules::locomotor_type::LocomotorKind::Walk
            });
        if matches!(
            category,
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
        ) {
            self.assign_null_destination(stable_id, context.rules, context.registry);
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.clear_live_path_head();
            }
            if category == EntityCategory::Infantry {
                if let Some(rules) = context.rules
                    && let Err(cause) =
                        self.infantry_stop_driver(stable_id, rules, context.registry)
                {
                    log::debug!("infantry {stable_id} Stun Stop_Driver: {cause}");
                }
            } else if let Err(cause) =
                self.locomotor_stop_moving(stable_id, context.rules, context.registry)
            {
                log::debug!("{stable_id} Stun Stop_Driver: {cause}");
            }
        }
        if walking_infantry {
            let _ = self.assign_target_represented(stable_id, None, context.rules);
        } else if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            crate::sim::mission::concrete_effects::represented_assign_target(entity, None);
        }
        self.assign_null_destination(stable_id, context.rules, context.registry);
        if !self.substrate.entities.contains(stable_id) {
            return;
        }
        crate::sim::radio::broadcast_break(self, stable_id, None);
        crate::sim::spawn_manager::kill_all_spawns_with_context(self, stable_id, context);
        crate::sim::spawn_manager::clear_all_spawn_targets(
            self,
            stable_id,
            context.rules(),
            context.registry(),
        );
        // A living object's Stun cannot use the Health-0 elision above: its
        // Techno6FCD9B Detach_All(1) runs, and self and other pointer-expiry
        // callbacks observe the live object. Unit737E58 repeats Stun after
        // restoring Health 1/+3CD, warp sinking stuns a living hull, and a
        // retreating aircraft leaving the map is stunned alive (Fly 4CD5D7).
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.health.current != 0)
        {
            self.object_destroy_callback(stable_id, context);
        }
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.selected = false;
        }
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
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        match request {
            LifecycleRequest::Uninit {
                stable_id,
                reason: _,
            } => self.uninit_with_context(stable_id, UninitContext::new(Some(rules), registry)),
        }
    }

    fn run_represented_uninit_pre_hook(&mut self, stable_id: u64) {
        // Object UnInit5F6616 expires damage-fire owners before Building's
        // destructor43BDE0 destroys the remaining slot Anims. Do not run the
        // recovery path here: it converts coordinates and stops sounds early.
        // Ordinary21slots have no Anim owner+CC and survive this broadcast;
        // Building43BDC5 clears them only at deferred scalar destruction.
        self.record_destruction_once(stable_id);
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
                    PassengerRole::Inside { transport_id, .. } if transport_id == carrier_id
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

    /// Whether the pointer-expiry forwards an entity listener receives can
    /// change anything for `expired_id`: the listener is the expiring object,
    /// a field [`Self::notify_entity_pointer_expired`] compares names it, or
    /// it holds an object a forward reaches (SpawnManager, CaptureManager,
    /// TemporalClass, its parasite). A superset of each forward's own test, so
    /// passing over a listener without any of these matches visiting it. A
    /// field or forward added to the broadcast must be added here too.
    fn entity_expiry_listener_acts(
        listener: &GameEntity,
        listener_id: u64,
        expired_id: u64,
    ) -> bool {
        let names = |target: Option<TargetKind>| target == Some(TargetKind::Entity(expired_id));
        let nav_names = |target: Option<&NavTargetRef>| {
            target.is_some_and(|target| Self::nav_ref_targets_expired(target, expired_id))
        };
        listener_id == expired_id
            || names(listener.attack_target.as_ref().map(|attack| attack.target))
            || names(listener.suspended_attack_target)
            || names(listener.archive_target())
            || listener.has_live_contact_with(expired_id)
            || match &listener.passenger_role {
                PassengerRole::Transport { cargo } => cargo.passengers.contains(&expired_id),
                PassengerRole::Boarding {
                    target_transport_id,
                    ..
                } => *target_transport_id == expired_id,
                PassengerRole::Inside { transport_id, .. } => *transport_id == expired_id,
                PassengerRole::None => false,
            }
            || nav_names(listener.navigation.suspended_nav_com.as_ref())
            || nav_names(listener.navigation.nav_com.as_ref())
            || listener
                .navigation
                .nav_queue
                .iter()
                .any(|target| Self::nav_ref_targets_expired(target, expired_id))
            || listener
                .c4_plant
                .as_ref()
                .is_some_and(|plant| plant.target_building_id == expired_id)
            || listener.pending_entry() == Some(expired_id)
            || listener.spawn_owner_id == Some(expired_id)
            || listener
                .aircraft_ammo
                .as_ref()
                .is_some_and(|ammo| ammo.dock() == Some(expired_id))
            || listener
                .miner
                .as_ref()
                .is_some_and(|miner| miner.reserved_refinery == Some(expired_id))
            || listener
                .pending_c4_detonation
                .as_ref()
                .is_some_and(|pending| pending.source_entity_id == Some(expired_id))
            || listener.spawn_manager.is_some()
            || listener.capture_manager.is_some()
            || listener.temporal.has_link()
            || listener.parasite_eating_me.is_some()
    }

    /// Each RNG stream's cursor, which every draw moves.
    #[cfg(test)]
    fn rng_cursors(&self) -> [(u8, i32, i32); 3] {
        [&self.scenario_rng, &self.main_rng, &self.mapgen_rng].map(|rng| {
            let view = rng.logical_view();
            (view.disabled, view.index_a, view.index_b)
        })
    }

    /// Represented entries in global ObjectClass construction order, each
    /// with the store it lives in. Stable IDs are monotonic and never reused,
    /// so merging the separate Rust stores by ID reproduces the native
    /// registration order without walking holes left by already-finalized
    /// objects, and an ID's store never changes.
    ///
    /// gamemd-derived: active YR `ObjectClass` construction/destruction at
    /// `0x005F3900` / `0x005F3B80` maintains the listener roster in object
    /// construction order.
    fn removal_listener_order(&self) -> Vec<(u64, ObjectKind)> {
        let mut listeners = Vec::with_capacity(
            self.substrate.entities.len()
                + self.substrate.anims.len()
                + self.substrate.particle_systems.len()
                + self.projectiles.len()
                + self.waves.len()
                + self
                    .smudge_grid
                    .as_ref()
                    .map_or(0, |grid| grid.object_count()),
        );
        listeners.extend(
            self.substrate
                .entities
                .iter_sorted()
                .map(|(stable_id, _)| (stable_id, ObjectKind::Entity)),
        );
        listeners.extend(
            self.substrate
                .anims
                .iter()
                .map(|(&stable_id, _)| (stable_id, ObjectKind::Anim)),
        );
        listeners.extend(
            self.substrate
                .particle_systems
                .iter()
                .map(|(&stable_id, _)| (stable_id, ObjectKind::ParticleSystem)),
        );
        listeners.extend(
            self.projectiles
                .iter()
                .map(|(&stable_id, _)| (stable_id, ObjectKind::Projectile)),
        );
        listeners.extend(
            self.waves
                .iter()
                .map(|(&stable_id, _)| (stable_id, ObjectKind::Wave)),
        );
        if let Some(grid) = &self.smudge_grid {
            listeners.extend(grid.objects().map(|(id, _)| (id, ObjectKind::Smudge)));
        }
        // Each store yields its IDs in order: the run-adaptive stable sort
        // merges those runs instead of sorting from scratch.
        listeners.sort_by_key(|&(stable_id, _)| stable_id);
        debug_assert!(
            listeners.windows(2).all(|pair| pair[0].0 != pair[1].0),
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
    /// The native sweep has callers in area damage, building sale, owner change,
    /// building placement and teleportation. The represented owner-change and
    /// bridge-damage callers share this owner; the other caller chains remain
    /// separate migration work.
    ///
    /// RESIDUALS, recorded rather than guessed:
    /// - The native suppression clause (`0x0070D4DB..0x0070D4FB`) skips the
    ///   whole block for a listener whose `+0x294` manager points at the
    ///   detaching object: the Boris Airstrike link (`AirstrikeClass`,
    ///   `+0x294` written by `0x0041D540..0x0041DCD9`), not mind control
    ///   (`+0x2BC`, whose manager has no such field). VERA has no Airstrike
    ///   manager, so the clause is omitted. A mind-control capture DOES run
    ///   this sweep in full: the firing controller drops its new victim as
    ///   its target.
    /// - The aircraft-Patrol arm, which clears two patrol-cursor fields on an
    ///   aircraft whose committed mission is Patrol. Neither field is
    ///   represented; the arm is a no-op for every ground object.
    /// - The second sweep is TeamClass (0x0070D554..0x0070D57B), clearing
    ///   matching pointers at +0x3C/+0x40. The represented Team owner does not
    ///   yet retain both native pointers. Bridge damage needs that lifecycle
    ///   when those Team states are implemented.
    pub(crate) fn stop_all_targeting_on_detach(
        &mut self,
        detach_id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        self.stop_all_targeting_target(TargetKind::Entity(detach_id), rules, registry);
    }

    /// Apply_area_damage bridge success calls the same 0x0070D4A0 sweep with a
    /// CellClass pointer. This is not CellClass PointerExpired: Restore must
    /// precede the conditional target clear, in descending Techno order.
    pub(crate) fn stop_all_targeting_cell(
        &mut self,
        rx: u16,
        ry: u16,
        rules: Option<&RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        self.stop_all_targeting_target(TargetKind::Cell(rx, ry), rules, registry);
    }

    fn stop_all_targeting_target(
        &mut self,
        target: TargetKind,
        rules: Option<&RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let mut listeners = self.substrate.entities.keys_sorted();
        listeners.reverse();

        for listener_id in listeners {
            if !self.listener_targets(listener_id, target) {
                continue;
            }

            let restored = self
                .mission_restore_represented(listener_id, rules, registry)
                .expect("detach sweep listener was resolved immediately before the Restore");

            let target_cleared = self.listener_targets(listener_id, target);
            if target_cleared {
                self.assign_target_represented(listener_id, None, rules)
                    .expect("detach sweep listener remains present for the target clear");
            }

            let _ = restored;
            #[cfg(test)]
            if let TargetKind::Entity(detach_id) = target {
                self.trace_lifecycle_for_test(LifecycleTestEvent::DetachTargetingSweepVisited {
                    detach_id,
                    listener_id,
                    restored,
                    target_cleared,
                });
            }
        }
    }

    /// Match the retained target identity, preserving Cell/Techno distinction.
    fn listener_targets(&self, listener_id: u64, target: TargetKind) -> bool {
        self.substrate
            .entities
            .get(listener_id)
            .is_some_and(|listener| {
                listener
                    .attack_target
                    .as_ref()
                    .map(|current| current.target)
                    == Some(target)
            })
    }

    /// Infantry PerCell repair519D17..519D36 calls each registered Infantry
    /// +28(hut,false) in descending construction order. The hut survives;
    /// this is not the global ObjectClass removal broadcast. Actual Infantry
    /// +28 is51AA10 -> Foot4D9960 -> Techno7077C0. Its extra +6C0 clear
    /// compares an InfantryType pointer with the hut and cannot match.
    pub(crate) fn expire_infantry_bridge_hut_targets(
        &mut self,
        hut_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
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
                facts.1,
                facts.2,
                facts.3,
                Some(facts.4),
                PointerExpiryControl::DetachAll,
                Some(rules),
                registry,
            );
            // Techno707B24 forwards this manager independently of control.
            crate::sim::spawn_manager::notify_pointer_expired(
                self,
                listener_id,
                hut_id,
                Some(rules),
                registry,
            );
        }
    }

    /// Techno70F770 and the identical PointerExpired7079D1..7A28 arm
    /// shorten the passive targeting timer (+180/+188), never weapon rearm.
    /// The native priority bracket A8E7AC suppresses both the draw and write.
    /// Original controls: infantry_deploy_action.json's reload rows.
    pub(crate) fn shorten_passive_scan_timer(&mut self, id: u64) -> bool {
        let now = self.session.binary_frame;
        if self.object_placement_scope_active()
            || self
                .substrate
                .entities
                .get(id)
                .is_none_or(|entity| entity.passive_scan_timer.remaining(now) <= 10)
        {
            return false;
        }
        let delay = self.scenario_rng.next_range_u32_inclusive(4, 8);
        self.substrate
            .entities
            .get_mut(id)
            .expect("queried passive timer owner remains present")
            .passive_scan_timer
            .arm(now, delay);
        true
    }

    /// The broadcast passes over listeners that
    /// [`Self::entity_expiry_listener_acts`] rejects, so a field compared here
    /// must be tested there too.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn notify_entity_pointer_expired(
        &mut self,
        listener_id: u64,
        expired_id: u64,
        // The expiring object's `ObjectClass::GetCoords` cell, which
        // `TechnoClass::PointerExpired` hands to `SensorCountForHouse` when it
        // computes `allowClear` on the `Detach_All` control.
        expired_get_coords_cell: Option<(u16, u16)>,
        expired_object_alive: bool,
        expired_health: i32,
        expired_is_selling: bool,
        expired_owner: Option<InternedId>,
        control: PointerExpiryControl,
        rules: Option<&RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let Some(listener) = self.substrate.entities.get(listener_id) else {
            return;
        };
        let current_target_matches = listener.attack_target.as_ref().is_some_and(
            |target| matches!(target.target, TargetKind::Entity(id) if id == expired_id),
        );
        let listener_owner = listener.owner();
        let mission_is_suspended =
            listener.mission.suspended() != crate::sim::mission::MissionId::NONE;
        // What the radio and cargo clears below would change. A listener is
        // handed out mutably only when something changes: every hand-out
        // enters the store's touch logs, and most listeners hold no
        // reference to the expiring object.
        let drops_contact =
            control == PointerExpiryControl::Uninit && listener.has_live_contact_with(expired_id);
        let drops_passenger = matches!(
            &listener.passenger_role,
            PassengerRole::Transport { cargo } if cargo.passengers.contains(&expired_id)
        );
        // `0x007077EE..0x007077FD`, after the radio and cargo, whatever the
        // control: a child whose SpawnOwner (`+0x2D4`) expires forgets it.
        let drops_spawn_owner = listener.spawn_owner_id == Some(expired_id);

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
        if clears_current_target {
            self.shorten_passive_scan_timer(listener_id);
        }
        if (drops_contact || drops_passenger || drops_spawn_owner)
            && let Some(listener) = self.substrate.entities.get_mut(listener_id)
        {
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
            if drops_contact {
                listener.clear_live_contact_with(expired_id);
            }

            // TechnoClass removes an expiring passenger from its CargoClass before
            // clearing its target/archive/manager reference family.
            if drops_passenger
                && let PassengerRole::Transport { cargo } = &mut listener.passenger_role
            {
                let _ = cargo.disembark(expired_id);
            }
            if drops_spawn_owner {
                listener.spawn_owner_id = None;
            }
        }

        if clears_current_target {
            self.assign_target_represented(listener_id, None, rules)
                .expect("expiry listener remains present");
            if mission_is_suspended {
                self.mission_restore_represented(listener_id, rules, registry)
                    .expect("represented expiry restore remains available");
            }
        }

        // Decide every clear from a read, then hand the listener out mutably
        // only when one applies (see `drops_contact`).
        let Some(listener) = self.substrate.entities.get(listener_id) else {
            return;
        };
        // `+0x2B8` is cleared on an exact match under `allowClear` alone — the
        // same-owner exemption applies only to `+0x2B4`.
        let clear_suspended_target = allow_clear
            && matches!(
                listener.suspended_attack_target,
                Some(TargetKind::Entity(id)) if id == expired_id
            );
        // Techno707AE7..707B03 clears ArchiveTarget+218 on control1. The
        // later Foot4D99F1..4D99FC clears a still-matching archive on both
        // controls, independent of sensors and same-owner target exemptions.
        // Only Unit/Infantry/Aircraft inherit that additional Foot clear.
        // Native execution: spatial_oracle/foot_archive_expiry.{json,md}.
        let foot_receiver = matches!(
            listener.category,
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
        );
        let clear_archive_target = (control == PointerExpiryControl::Uninit || foot_receiver)
            && listener.archive_target() == Some(TargetKind::Entity(expired_id));

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
        let clear_suspended_nav_com = listener
            .navigation
            .suspended_nav_com
            .as_ref()
            .is_some_and(|target| Self::nav_ref_targets_expired(target, expired_id));
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
        let clear_nav_com = current_nav_matches && !retain_capture_nav && allow_clear;
        let prune_nav_queue = listener
            .navigation
            .nav_queue
            .iter()
            .any(|target| Self::nav_ref_targets_expired(target, expired_id));

        let clear_c4_plant = listener
            .c4_plant
            .as_ref()
            .is_some_and(|plant| plant.target_building_id == expired_id);
        // Techno707AE7..707AF5 clears private pending+500 only on control1.
        // Non-destructive DetachAll(false) retains this independent entry;
        // radio contact expiry owns the admitted visit.
        let clear_dock =
            control == PointerExpiryControl::Uninit && listener.pending_entry() == Some(expired_id);
        // Aircraft PointerExpired clears its dock (`+0x6CC`) for either
        // control (`0x0041B673..0x0041B67F`).
        let clear_airfield = listener
            .aircraft_ammo
            .as_ref()
            .is_some_and(|ammo| ammo.dock() == Some(expired_id));
        let clear_refinery = listener
            .miner
            .as_ref()
            .is_some_and(|miner| miner.reserved_refinery == Some(expired_id));

        let clear_passenger_role = match &listener.passenger_role {
            PassengerRole::Transport { .. } => false,
            PassengerRole::Boarding {
                target_transport_id,
                ..
            } => *target_transport_id == expired_id,
            PassengerRole::Inside { transport_id, .. } => *transport_id == expired_id,
            PassengerRole::None => false,
        };
        let clear_c4_source = listener
            .pending_c4_detonation
            .as_ref()
            .is_some_and(|pending| pending.source_entity_id == Some(expired_id));
        if !(clear_suspended_target
            || clear_archive_target
            || clear_suspended_nav_com
            || clear_nav_com
            || prune_nav_queue
            || clear_c4_plant
            || clear_dock
            || clear_airfield
            || clear_refinery
            || clear_passenger_role
            || clear_c4_source)
        {
            return;
        }

        let listener = self
            .substrate
            .entities
            .get_mut(listener_id)
            .expect("expiry listener read above");
        if clear_suspended_target {
            listener.suspended_attack_target = None;
        }
        if clear_archive_target {
            listener.set_archive_target(None);
        }
        if clear_suspended_nav_com {
            listener.navigation.suspended_nav_com = None;
        }
        if clear_nav_com {
            listener.navigation.nav_com_aux = None;
            listener.navigation.nav_com = None;
        }
        if prune_nav_queue {
            listener
                .navigation
                .nav_queue
                .retain(|target| !Self::nav_ref_targets_expired(target, expired_id));
        }
        if clear_c4_plant {
            listener.c4_plant = None;
        }
        if clear_dock {
            crate::sim::docking::building_dock::expire_reference(listener, expired_id);
        }
        if clear_airfield && let Some(ammo) = listener.aircraft_ammo.as_mut() {
            ammo.set_dock(None);
        }
        if clear_refinery && let Some(miner) = listener.miner.as_mut() {
            miner.reserved_refinery = None;
        }
        if clear_passenger_role {
            listener.passenger_role = PassengerRole::None;
        }
        if clear_c4_source && let Some(pending) = listener.pending_c4_detonation.as_mut() {
            pending.source_entity_id = None;
        }
    }

    /// ObjectClass::Detach_From_All_Lists represented listener broadcast.
    ///
    /// The callback pass runs while the target remains alive, unconcealed,
    /// cell-marked, and resolvable. The represented callbacks below do not add
    /// or erase listener objects, so the native live-vector cursor and this
    /// monotonic construction-order walk have the same observable result.
    ///
    /// gamemd-derived: active YR `DispatchPointerExpiredCleanup @ 0x007258D0`
    /// is called directly by `ObjectClass__UnInit @ 0x005F65F0` and by
    /// `ObjectClass::Detach_All @ 0x005F5280` (the killing hit's Destroy, the
    /// virtual Conceal path and the Stun).
    /// Every listener uses the live MapClass authority, including when a receiver
    /// has temporarily lent that grid through UninitContext. The remaining arms
    /// read the object's liveness, health and mission from the same world.
    fn notify_pointer_expired(&mut self, expired_id: u64, context: UninitContext<'_>) {
        if !self.substrate.entities.contains(expired_id)
            && self
                .smudge_grid
                .as_ref()
                .and_then(|grid| grid.object(expired_id))
                .is_none()
        {
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
    ///   `ProjectileStore::pointer_expired` receives — is unguarded.
    /// * `ObjectClass::PointerExpired @ 0x005F5230` itself gates only the
    ///   `+0x30` chain relink on a nonzero control; VERA models no `+0x30`
    ///   chain, so it is correct by omission.
    ///
    /// So a torpedo already in flight at a diving submarine falling to the sub's
    /// last cell instead of tracking it is native behavior, not an approximation.
    ///
    /// The victim's FootClass parasite forward is control-insensitive too, so a
    /// cloaking host releases its parasite; the rules place the released owner.
    pub(crate) fn detach_all_pointer_expired(
        &mut self,
        expired_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        if !self.substrate.entities.contains(expired_id) {
            return;
        }
        self.broadcast_pointer_expired(
            expired_id,
            PointerExpiryControl::DetachAll,
            UninitContext::new(Some(rules), registry),
        );
    }

    fn broadcast_pointer_expired(
        &mut self,
        expired_id: u64,
        control: PointerExpiryControl,
        context: UninitContext<'_>,
    ) {
        // `DispatchPointerExpiredCleanup @ 0x007258D0` calls
        // `BombListClass::PointerGotInvalid @ 0x00439150` after its listener
        // loop (0x00725961) whatever the control, so a cloaking planter loses
        // its bombs' credit as a removed one does. No listener reads a planter,
        // so running it first changes nothing.
        self.bomb_planter_expired(expired_id);
        // `TeamClass::PointerExpired @ 0x006EAE60`, each team a listener.
        self.team_script_vm
            .pointer_expired(expired_id, control == PointerExpiryControl::Uninit);
        let Some((
            expired_target_cell,
            expired_is_high_flying,
            expired_object_alive,
            expired_health,
            expired_is_selling,
            expired_owner,
        )) = self
            .substrate
            .entities
            .get(expired_id)
            .map(|expired| {
                // High-flying objects expire to null (the target's vt+0x54 at
                // `0x00468562`); lower objects preserve their cell.
                let high_flying = crate::sim::movement::air_movement::is_high_flying(
                    expired,
                    context.terrain().or(self.resolved_terrain.as_ref()),
                    context.rules().map(|rules| (rules, &self.interner)),
                );
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
            .or_else(|| {
                let smudge = self.smudge_grid.as_ref()?.object(expired_id)?;
                let location = smudge.location();
                let cell = u16::try_from(crate::util::lepton::lepton_to_cell(location.x))
                    .ok()
                    .zip(u16::try_from(crate::util::lepton::lepton_to_cell(location.y)).ok());
                Some((
                    cell,
                    false,
                    smudge.object_alive(),
                    smudge.health(),
                    false,
                    None,
                ))
            })
        else {
            return;
        };

        #[cfg(test)]
        if let Some(smudge) = self
            .smudge_grid
            .as_ref()
            .and_then(|grid| grid.object(expired_id))
        {
            let location = smudge.location();
            self.trace_lifecycle_for_test(LifecycleTestEvent::SmudgeExpiryBoundary {
                stable_id: expired_id,
                native_id: smudge.native_unique_id(),
                native_cursor: self.native_unique_ids.as_ref().unwrap().current_raw(),
                object_alive: smudge.object_alive(),
                in_limbo: smudge.in_limbo(),
                health: smudge.health(),
                location: [location.x, location.y, location.z],
                pending: self.substrate.pending_delete.clone(),
                generic_objects: self
                    .removal_listener_order()
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect(),
                rng: [
                    self.scenario_rng.native_state_hex(),
                    self.main_rng.native_state_hex(),
                    self.mapgen_rng.native_state_hex(),
                ],
            });
        }

        // `BulletClass::PointerExpired @ 0x004684E0` performs the packed
        // `MapClass::Get_CellClass @ 0x005657A0` lookup only for a matching
        // target. Its result pointer is stored at Bullet+0x10C: an allocated
        // slot therefore remains a stable Cell target, while a miss stores the
        // one shared dummy at `0x00ABDC50`. Later `BulletClass::AI @ 0x004666E0`
        // dispatches that live pointer and observes its most recent coord stamp.

        for (listener_id, kind) in self.removal_listener_order() {
            // A callback may have removed a later listener; an ID never
            // moves to another store. An entity listener is read once, for
            // its presence and for whether it holds anything this expiry
            // changes.
            let mut entity_acts = false;
            let present = match kind {
                ObjectKind::Entity => {
                    self.substrate
                        .entities
                        .get(listener_id)
                        .is_some_and(|listener| {
                            entity_acts = Self::entity_expiry_listener_acts(
                                listener,
                                listener_id,
                                expired_id,
                            );
                            true
                        })
                }
                ObjectKind::Anim => self.substrate.anims.contains_key(listener_id),
                ObjectKind::ParticleSystem => {
                    self.substrate.particle_systems.contains_key(listener_id)
                }
                ObjectKind::Projectile => self.projectiles.get(listener_id).is_some(),
                ObjectKind::Wave => self.waves.get(listener_id).is_some(),
                ObjectKind::Smudge => self
                    .smudge_grid
                    .as_ref()
                    .and_then(|grid| grid.object(listener_id))
                    .is_some(),
                ObjectKind::VoxelAnim | ObjectKind::Terrain => false,
            };
            if !present {
                continue;
            }

            #[cfg(test)]
            if control == PointerExpiryControl::Uninit {
                let (target_alive, target_in_limbo) = self
                    .substrate
                    .entities
                    .get(expired_id)
                    .map(|target| (target.lifecycle.object_alive, target.lifecycle.in_limbo))
                    .or_else(|| {
                        self.smudge_grid
                            .as_ref()?
                            .object(expired_id)
                            .map(|object| (object.object_alive(), object.in_limbo()))
                    })
                    .unwrap_or((false, true));
                self.trace_lifecycle_for_test(LifecycleTestEvent::UninitRemovalListenerVisited {
                    expired_id,
                    listener_id,
                    target_alive,
                    target_in_limbo,
                });
            }

            if kind == ObjectKind::Entity {
                // Most entity listeners hold nothing this expiry changes, and
                // the game passes over them. Test builds visit them as well,
                // and check below that their forwards handed out no entity and
                // drew no random number.
                if !entity_acts && !cfg!(test) {
                    continue;
                }
                #[cfg(test)]
                let passed_over = (!entity_acts)
                    .then(|| (self.substrate.entities.hand_outs(), self.rng_cursors()));
                self.notify_entity_pointer_expired(
                    listener_id,
                    expired_id,
                    expired_target_cell,
                    expired_object_alive,
                    expired_health,
                    expired_is_selling,
                    expired_owner,
                    control,
                    context.rules(),
                    context.registry(),
                );
                // `TechnoClass::PointerExpired` forwards to the listener's
                // SpawnManager: `0x00707B24 CALL 0x006B7C60`, gated only on
                // `+0x2D0 != 0`. This is the only mechanism that drops a
                // destroyed wing target, so without it a Carrier keeps sending
                // its Hornets at a corpse. The forward sits OUTSIDE the control
                // test, so it runs on a cloak dive as well as on UnInit.
                crate::sim::spawn_manager::notify_pointer_expired(
                    self,
                    listener_id,
                    expired_id,
                    context.rules(),
                    context.registry(),
                );
                // The CaptureManager forward — `0x00707B14 CALL 0x00471F90` —
                // sits inside the `if (control != 0)` block opened at
                // `0x00707AE7`, so a dive leaves mind-control links alone while
                // a death drops them.
                if control == PointerExpiryControl::Uninit
                    && self
                        .substrate
                        .entities
                        .get(listener_id)
                        .is_some_and(|entity| entity.capture_manager.is_some())
                    && let Some(manager) = self
                        .substrate
                        .entities
                        .get_mut(listener_id)
                        .and_then(|entity| entity.capture_manager.as_mut())
                {
                    manager.pointer_expired(expired_id);
                }
                // The TemporalClass forward (`0x00707B34` -> `0x0071AB60`)
                // follows, outside the control test like the SpawnManager's.
                self.temporal_pointer_expired(listener_id, expired_id, context.rules());
                // FootClass::PointerExpired 0x004D998C..0x004D99CD follows the
                // Techno body: the parasite link and its forward.
                self.foot_parasite_pointer_expired(listener_id, expired_id, context.rules());
                #[cfg(test)]
                if let Some(before) = passed_over {
                    assert_eq!(
                        before,
                        (self.substrate.entities.hand_outs(), self.rng_cursors()),
                        "the expiry of {expired_id} changed listener {listener_id}, which \
                         entity_expiry_listener_acts passes over"
                    );
                }
            } else if kind == ObjectKind::Anim {
                self.expire_anim_owner_reference(listener_id, expired_id);
            } else if kind == ObjectKind::ParticleSystem {
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
            } else if kind == ObjectKind::Projectile {
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
            } else if kind == ObjectKind::Wave {
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
        // The kamikaze tracker's Remove (`0x00725972`) follows the listeners
        // and the BombList; a SpawnManager's slot guard reads the membership
        // before it.
        self.kamikaze.remove(expired_id);
    }

    /// ObjectClass::UnInit represented ordering.  Physical removal is deferred.
    pub(crate) fn uninit(&mut self, stable_id: u64) {
        self.uninit_with_context(stable_id, UninitContext::default());
    }

    pub(crate) fn uninit_with_rules(&mut self, stable_id: u64, rules: &RuleSet) {
        self.uninit_with_context(stable_id, UninitContext::with_rules(rules));
    }

    pub(crate) fn uninit_with_context(&mut self, stable_id: u64, context: UninitContext<'_>) {
        if self
            .smudge_grid
            .as_ref()
            .and_then(|grid| grid.object(stable_id))
            .is_some()
        {
            // Whole Smudge6B4A50 -> ObjectUnInit5F65F0: expiry5F661B,
            // then Conceal's Detach_All5F5316 while alive1/limbo0/marked0.
            // SmudgeMark(0) writes no cell data. Only afterwards is this
            // object limbo/dead and appended to the shared pending queue.
            self.notify_pointer_expired(stable_id, context);
            self.notify_pointer_expired(stable_id, context);
            self.smudge_grid.as_mut().unwrap().finish_uninit(stable_id);
            self.substrate.pending_delete.push(stable_id);
            #[cfg(test)]
            self.trace_lifecycle_for_test(LifecycleTestEvent::PendingDeleteQueued { stable_id });
            return;
        }
        let Some(entity) = self.substrate.entities.get_mut(stable_id) else {
            return;
        };
        // Consume deferred/retained Infantry lifetime before pointer-expiry
        // callbacks, including an UnInit that overtakes receiver delivery.
        entity.infantry_terminal = None;
        // `FootClass::UnInit @ 0x004DE5DD` frees a controller's captives
        // first (a crushed or otherwise removed Yuri or Mastermind).
        let foot = matches!(
            entity.category,
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
        );
        if foot
            && entity.capture_manager.is_some()
            && let Some(rules) = context.rules()
        {
            self.free_all_captures(stable_id, rules, context.registry());
        }
        // `0x004DE604`: then the object leaves its team.
        if foot {
            self.leave_team(stable_id, false, context.rules());
        }

        self.run_represented_uninit_pre_hook(stable_id);
        self.uninit_carried_passengers(stable_id, context);

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
        // ObjectClass::UnInit's first statement (`0x005F65F3`) defuses a
        // carried bomb silently: sold, crushed, erased.
        self.bomb_defuse(stable_id);
        self.notify_pointer_expired(stable_id, context);

        let _ = self.techno_limbo_with_context(stable_id, context);
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
        if let Some(smudge) = self
            .smudge_grid
            .as_ref()
            .and_then(|grid| grid.object(stable_id))
        {
            return !smudge.object_alive();
        }
        if let Some(entity) = self.substrate.entities.get(stable_id) {
            return !entity.lifecycle.object_alive;
        }
        if let Some(anim) = self.substrate.anims.get(stable_id) {
            // Every ordinary Anim retirement Conceals before queueing. Owner
            // expiry425196 only sets19B and leaves Logic live until its own AI;
            // that byte is not ObjectUnInit5F6625's readiness state.
            return !anim.in_logic_vector;
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

    /// Shared physical destructor owner, used by deferred retirement and
    /// constructor-complete limbo disposal. Neither caller repeats UnInit or
    /// books a loss here. Rules stay available to synchronous expiry receivers.
    pub(super) fn finalize_and_remove_common(
        &mut self,
        stable_id: u64,
        context: UninitContext<'_>,
    ) {
        if self
            .smudge_grid
            .as_ref()
            .and_then(|grid| grid.object(stable_id))
            .is_some()
        {
            // Scalar Smudge6B4FA0 sends its third removed=true expiry at
            //6B4FC6 before leaving Smudge/Object registries. Place's cells
            // survive this destructor and Building Mark(0).
            self.notify_pointer_expired(stable_id, context);
        }
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category == EntityCategory::Structure)
        {
            // Building destructor43BD27 releases the construction SoundClass
            //(+6A0) before the delegated Techno destructor removes tracking.
            self.sound_events
                .push(super::SimSoundEvent::ObjectSoundReleased { owner: stable_id });
            // Building43BD67 broadcasts removed=true after sound release,
            // even when a limbo object never reached UnInit. The original
            // constructed-cancel controls execute this under Factory's
            // A8E7AC bracket; the ordinary late drain repeats it after UnInit.
            self.notify_pointer_expired(stable_id, context);
            // Building43BDC5 -> ClearAnimSlot451E40(-2) synchronously deletes
            // ordinary21slots, then43BDE0 destroys the distinct eight fire
            // Anims, before Techno tracking removal43BF34. Whole original
            // UnInit/drain control: building_death_anims joined evidence,
            // gamemd SHA1cdd1180e49024fbda8ad568caac2e86e.
            self.clear_all_building_anim_slots(stable_id);
            self.clear_building_damage_fire_slots(stable_id, context.rules());
            // Active-frame Building43BEF5..43BF11 compares Health+6C with
            // the retained AI sample+544 after its slot/fire destructors.
            // A mismatch dirties power only, without publishing a sample or
            // touching radar, before Remove_Tracking43BF34. Original joined
            // building_death_anims: normal fatal0/38 versus direct0/0.
            let unsampled_owner = self.substrate.entities.get(stable_id).and_then(|entity| {
                entity
                    .building_power_health_sample()
                    .filter(|sample| *sample != entity.health.current)
                    .map(|_| entity.owner())
            });
            if let Some(owner) = unsampled_owner {
                self.invalidate_house_power(owner, false);
            }
        }
        // The Techno destructors (`0x0041410B`, `0x0043BF34`, `0x00517E2E`,
        // `0x00735816`) call Remove_Tracking.
        self.update_house_tracking(
            stable_id,
            crate::sim::house_tracking::HouseTracking::remove_tracking,
        );
        // The ObjectClass destructor's defensive Defuse (`0x005F3BA6`).
        self.bomb_defuse(stable_id);
        self.team_script_vm.object_deleted(stable_id);
        self.release_house_base_tracking(stable_id);
        // Selected original Foot destructor4D3632..4D366E clears only its
        // cached+564 Cell if+E0 still holds this Foot. UnInit/Limbo retain it;
        // the block sends no notification and leaves the obsolete+564 intact.
        self.clear_foot_air_slot_at_destruction(stable_id);
        // Foot destructor4D3677 Release406060 follows class Remove_Tracking
        // and Foot's cached-cell cleanup. All Foot classes share it, including
        // constructor-complete limbo disposal. No caller-local hard stop.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category != EntityCategory::Structure)
        {
            self.sound_events
                .push(super::SimSoundEvent::ObjectSoundReleased { owner: stable_id });
        }
        // Foot4D3701 delegates to Techno6F4500 only after its own sound
        // tail. Techno6F4607 destroys the distinct voice handle via405C00,
        // before its attached/deploy Anims6F468B. Keep the disposal marker in
        // the same stream as AI visits and those Anim sound callbacks.
        if self.substrate.entities.contains(stable_id) {
            self.sound_events
                .push(super::SimSoundEvent::UnitVoiceDestroyed { owner: stable_id });
        }
        // Techno destructor6F467F..6F4691 UnInits its retained deploy Anim.
        // Pointer-expiry may already have cleared it through the Anim owner.
        if let Some(anim) = self
            .substrate
            .entities
            .get_mut(stable_id)
            .and_then(|entity| entity.take_deploy_anim())
        {
            self.destroy_anim_with_context(anim, context.rules());
        }
        self.destroy_building_light(stable_id);
        if self.substrate.anims.contains_key(stable_id) {
            self.clear_damage_fire_anim_reference(stable_id);
            self.conceal_anim(stable_id);
            self.release_anim_owner_reference(stable_id);
            self.clear_building_anim_reference(stable_id);
        }
        // Display registration never outlives the object. Fly's phase tail
        // (4CD4DE) resubmits even an owner concealed earlier in the frame.
        self.substrate.display.remove(stable_id);
        let entity = self.substrate.entities.remove(stable_id);
        let anim = self.substrate.anims.remove(stable_id);
        let particle_system = self.substrate.particle_systems.finalize_remove(stable_id);
        let terrain = self.production.terrain_objects.remove(&stable_id);
        if let Some(terrain) = terrain.as_ref() {
            let cell = terrain.cell();
            if self.production.terrain_object_cells.get(&cell) == Some(&stable_id) {
                self.production.terrain_object_cells.remove(&cell);
                self.production.terrain_animations.remove(&cell);
                self.production
                    .tiberium_spawning_terrain_cells
                    .remove(&cell);
            }
        }
        let projectile = self.projectiles.remove(stable_id);
        if projectile.is_some() {
            self.lifecycle_outputs
                .push(LifecycleOutput::LineTrailDetached { stable_id });
        }
        let wave = self.waves.remove(stable_id);
        let smudge = self
            .smudge_grid
            .as_mut()
            .and_then(|grid| grid.finalize_remove(stable_id));
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
                + usize::from(smudge.is_some())
                <= 1,
            "object id {stable_id} was removed from multiple stores"
        );
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::FinalizedCommon { stable_id });
    }

    /// The rules-less drain of test fixtures (see
    /// [`Self::process_pending_delete_with`]).
    #[cfg(test)]
    pub(crate) fn process_pending_delete(&mut self) {
        self.process_pending_delete_with(None, None);
    }

    /// Native-shaped pending-delete drain: preserve alive entries, collapse all
    /// duplicate ready IDs, and finalize each selected object exactly once.
    /// The frame's rules reach the destructors' slave release.
    ///
    /// gamemd-derived: active YR `DrainDeferredFinalizationQueue @ 0x00725C70`
    /// is reached from `Main_Tick` at `0x0055DE9F` after the frame commit; it
    /// preserves non-ready entries, collapses selected duplicates, and finalizes
    /// each selected ready object once.
    pub(crate) fn process_pending_delete_with(
        &mut self,
        rules: Option<&RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
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
            self.release_slave_links_at_destruction(stable_id, rules, registry);
            self.finalize_and_remove_common(stable_id, UninitContext::new(rules, registry));
        }
    }

    /// Test compatibility only.  Production has one ordinary tail drain.
    #[cfg(test)]
    pub(crate) fn flush_pending_delete(&mut self) {
        self.process_pending_delete();
    }
}

#[cfg(test)]
#[path = "foot_archive_expiry_tests.rs"]
mod foot_archive_expiry_tests;

#[cfg(test)]
mod base_plan_lifecycle_tests {

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
            .spawn_object("GACNST", "Computer1", 10, 11, 0, &rules)
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
        sim.spawn_object("GAPOWR", "Human1", 20, 21, 0, &rules)
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
            .spawn_object("GACNST", "Computer1", 10, 11, 0, &rules)
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
                    exact_z_leptons: None,
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
            .spawn_object("GAPOWR", "SkirmishSlot", 40, 41, 0, &rules)
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
            .spawn_object("GAPOWR", "CampaignPlayer", 50, 51, 0, &rules)
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
            .spawn_object("NONEYARD", "Computer1", 10, 11, 0, &rules)
            .expect("none-sentinel Building");
        let angle = sim
            .spawn_object("ANGLEYARD", "Computer1", 12, 13, 0, &rules)
            .expect("angle-sentinel Building");
        let real = sim
            .spawn_object("REALYARD", "Computer1", 14, 15, 0, &rules)
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
            .spawn_object("GAPOWR", "Computer1", 30, 31, 0, &rules)
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
