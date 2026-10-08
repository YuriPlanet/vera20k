//! FootClass-style navigation destination helpers.
//!
//! These helpers model the owner `NavCom` lifecycle separately from
//! `MovementTarget`, which remains the active path execution adapter.

use super::track_process::TrackFamily;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

/// Pixels Walk Move_To lifts a bridge destination (`0x0075AD4D MOV ECX,0x3C`).
const WALK_BRIDGE_LIFT_PIXELS: i32 = 60;

/// Drive Stop_Moving 0x4AFE00 and Ship 0x69F510 clamp the class target
/// fraction to the float 0.3 at 0x7E6240 / 0x7F1308 (identical bodies).
const TRACK_STOP_TARGET_FRACTION: SimFixed = SimFixed::lit("0.3");

/// Accepted Walk75ACB0 destination store. This conversion is independent of
/// HeadTo/OnBridge's416: a bridge cell lifts the destination by
/// `native_pixel_height_leptons(60)` (`0x0075AD52 CALL 0x006D2120`), 414.
/// See walk_head_occupation.json destination rows, including actual Cell+4C.
pub(crate) fn set_walk_destination_coord(
    entity: &mut GameEntity,
    coord: DriveCoord,
    terrain: Option<&ResolvedTerrainGrid>,
) {
    // Walk75ACBD/75ACD0/75ACE3 checks EMP and both owner warp bytes.
    // The teleport owner represents the latter; the native EMP timer still
    // lacks its production writer. There is deliberately no power gate here.
    if super::locomotor_owner::owner_is_warping(entity) {
        return;
    }
    let Some(loco) = entity
        .locomotor
        .as_mut()
        .filter(|l| l.kind == LocomotorKind::Walk)
    else {
        return;
    };
    let mut coord = coord;
    if coord != (DriveCoord { x: 0, y: 0, z: 0 }) {
        if let Some(terrain) = terrain {
            let cell =
                terrain.native_cell_identity(((coord.x / 256) as i16, (coord.y / 256) as i16));
            if terrain.native_cell_flags(cell) & 0x100 != 0 {
                coord.z = coord.z.wrapping_add(
                    crate::util::lepton::native_pixel_height_leptons(WALK_BRIDGE_LIFT_PIXELS),
                );
            }
        }
    }
    loco.set_walk_destination(Some(coord));
}

fn is_drive_locomotor(entity: &GameEntity) -> bool {
    entity
        .locomotor
        .as_ref()
        .is_some_and(|loco| matches!(loco.kind, LocomotorKind::Drive))
}

fn is_ship_locomotor(entity: &GameEntity) -> bool {
    entity
        .locomotor
        .as_ref()
        .is_some_and(|loco| matches!(loco.kind, LocomotorKind::Ship))
}

pub(crate) fn target_cell_coord(
    rx: u16,
    ry: u16,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
) -> DriveCoord {
    if let Some(cells) = cells {
        let identity = cells.lookup((rx as i16, ry as i16));
        let (x, y, z) = crate::sim::cell_kernel::native_cell_own_coords(identity, cells)
            .unwrap_or_else(|| {
                // Preserve this caller's existing unsupported-slope zero-Z
                // adapter. The receiver identity/XY still come from the same
                // Cell and the shared center kernel.
                let (x, y) = cells.coord(identity);
                let point = crate::sim::cell_kernel::cell_center(
                    crate::sim::cell_kernel::CellCoordinate {
                        x: i32::from(x),
                        y: i32::from(y),
                    },
                    0,
                );
                (i64::from(point.x), i64::from(point.y), 0)
            });
        return DriveCoord {
            x: x as i32,
            y: y as i32,
            z: z as i32,
        };
    }
    // Existing no-map diagnostic adapter; ordinary receivers use the retained
    // Cell identity and one shared native point above.
    let point = crate::sim::cell_kernel::cell_center(
        crate::sim::cell_kernel::CellCoordinate {
            x: i32::from(rx as i16),
            y: i32::from(ry as i16),
        },
        0,
    );
    DriveCoord {
        x: point.x,
        y: point.y,
        z: point.z,
    }
}

/// Shared destination-setter adjustment (Drive4AFD40 / Ship69F450). Each call
/// receives raw caller XYZ, including already elevated Infantry GetCoords.
fn adjusted_destination(
    mut coord: DriveCoord,
    terrain: Option<&ResolvedTerrainGrid>,
) -> Option<DriveCoord> {
    if coord == (DriveCoord { x: 0, y: 0, z: 0 }) {
        return None;
    }
    if let Some(terrain) = terrain {
        let rx = (coord.x / 256) as i16;
        let ry = (coord.y / 256) as i16;
        let cell = terrain.native_cell_identity((rx, ry));
        let structural = terrain.native_cell_flags(cell) & 0x100 != 0;
        if structural {
            coord.z = coord
                .z
                .wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
        }
    }
    Some(coord)
}

/// Native4B05D0..0638 compares the raw target XYZ before calling SetDestination.
/// The committed head is independent and remains owned by movement acceptance.
pub(super) fn refresh_drive_destination_coord(
    entity: &mut GameEntity,
    coord: DriveCoord,
    terrain: Option<&ResolvedTerrainGrid>,
) -> bool {
    let Some(loco) = entity
        .locomotor
        .as_ref()
        .filter(|l| l.has_track_state(TrackFamily::Drive))
    else {
        return false;
    };
    let requested = (coord != (DriveCoord { x: 0, y: 0, z: 0 })).then_some(coord);
    if loco.track_destination(TrackFamily::Drive) == requested {
        return false;
    }
    drive_set_destination(entity, coord, terrain)
}

/// The cell of `BuildingClass::GetDockCoord @ 0x00447B20` for `building`, as
/// `MapClass::operator[](COORD) @ 0x00565730` looks it up: the DOCKING
/// receiver's forced-MOVE_HERE test (`0x0043C91B..0x0043C93A`) and
/// Per_Cell_Process's dock-cell test (`0x0073A3B1..0x0073A437`). For a
/// `Refinery=` type the coordinate is the foundation centre plus 128 leptons
/// east, which for the stock 4x3 foundation is the cell NW+(2,1), beside the
/// pad NW+(3,1) (tools/spatial_oracle/refinery_dock, `nav_on_dock_coord`).
pub(crate) fn building_dock_cell(
    entities: &EntityStore,
    building_id: u64,
    requester: Option<u64>,
    terrain: Option<&ResolvedTerrainGrid>,
    rules: &crate::rules::ruleset::RuleSet,
    interner: &crate::sim::intern::StringInterner,
) -> Option<(u16, u16)> {
    let coord =
        building_dock_coordinate(entities, building_id, requester, terrain, rules, interner)?;
    let cell = |value: i32| u16::try_from(value / 256).ok();
    Some((cell(coord.x)?, cell(coord.y)?))
}

/// Live GetDockCoord447B20 output shared by cell projection and Building
/// service distance447E00. This remains the one retail-offset coordinate owner.
pub(crate) fn building_dock_coordinate(
    entities: &EntityStore,
    building_id: u64,
    requester: Option<u64>,
    terrain: Option<&ResolvedTerrainGrid>,
    rules: &crate::rules::ruleset::RuleSet,
    interner: &crate::sim::intern::StringInterner,
) -> Option<DriveCoord> {
    let building = entities.get(building_id)?;
    let object = rules.object(interner.resolve(building.type_ref()))?;
    super::building_coordinate::dock_coordinate(
        super::ground_pose::object_location(building, terrain),
        super::ground_pose::object_get_coords(building, terrain),
        object,
        &building.radio_contacts,
        requester,
        || {
            let id = requester.ok_or("Bunker dock coordinate needs a requester")?;
            let entity = entities
                .get(id)
                .ok_or_else(|| format!("Dock requester {id} disappeared"))?;
            Ok(super::ground_pose::object_get_coords(entity, terrain))
        },
    )
    .ok()
}

/// Native pointer-comparison gates use the receiver identity. The stored tag
/// is retained; it does not make a different entity receiver. Valid Cells
/// retain the existing coordinate identity; dummy/aliased Cell pointer
/// retention is outside this ordinary object-reference slice.
pub(crate) fn nav_targets_same_receiver(left: Option<NavTargetRef>, right: NavTargetRef) -> bool {
    match (left, right) {
        (
            Some(NavTargetRef::Cell {
                rx: left_x,
                ry: left_y,
            }),
            NavTargetRef::Cell { rx, ry },
        ) => (left_x, left_y) == (rx, ry),
        (
            Some(
                NavTargetRef::Entity { id: left }
                | NavTargetRef::Object { id: left }
                | NavTargetRef::Building { id: left },
            ),
            NavTargetRef::Entity { id: right }
            | NavTargetRef::Object { id: right }
            | NavTargetRef::Building { id: right },
        ) => left == right,
        _ => false,
    }
}

/// Resolve the live receiver behind a non-null NavCom. The Rust reference tag
/// does not change the native virtual receiver, and a dangling ID is not NULL.
pub(crate) fn nav_target_coordinate(
    target: NavTargetRef,
    requester: Option<u64>,
    entities: &EntityStore,
    terrain: Option<&ResolvedTerrainGrid>,
    rules: Option<(
        &crate::rules::ruleset::RuleSet,
        &crate::sim::intern::StringInterner,
    )>,
) -> Result<DriveCoord, String> {
    let id = match target {
        NavTargetRef::Cell { rx, ry } => {
            return Ok(target_cell_coord(
                rx,
                ry,
                terrain
                    .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                    .as_ref(),
            ));
        }
        NavTargetRef::Entity { id }
        | NavTargetRef::Object { id }
        | NavTargetRef::Building { id } => id,
    };
    let entity = entities
        .get(id)
        .ok_or_else(|| format!("NavCom coordinate target {id} disappeared"))?;
    if entity.category == crate::map::entities::EntityCategory::Structure {
        let (rules, interner) =
            rules.ok_or_else(|| format!("Building {id} navigation requires type data"))?;
        let object = rules
            .object(interner.resolve(entity.type_ref()))
            .ok_or_else(|| format!("Building {id} navigation type disappeared"))?;
        return super::building_coordinate::navigation_coordinate(
            super::ground_pose::object_location(entity, terrain),
            super::ground_pose::object_get_coords(entity, terrain),
            object,
            &entity.radio_contacts,
            requester,
            || {
                let id = requester.expect("Bunker only reads a non-null requester");
                let entity = entities
                    .get(id)
                    .ok_or_else(|| format!("Navigation requester {id} disappeared"))?;
                Ok(super::ground_pose::object_get_coords(entity, terrain))
            },
        );
    }
    super::foot_coordinate::navigation_coordinate(entity, terrain)
}

/// Cell callers of the shared non-null destination owner. Object callers
/// capture their receiver's +4C through `nav_target_coordinate` instead.
pub(crate) fn set_destination_internal_cell(
    entity: &mut GameEntity,
    target: (u16, u16),
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    binary_frame: u32,
) {
    let coord = target_cell_coord(
        target.0,
        target.1,
        resolved_terrain
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
            .as_ref(),
    );
    set_destination_internal_coord(
        entity,
        NavTargetRef::cell(target.0, target.1),
        coord,
        resolved_terrain,
        binary_frame,
    );
}

/// Foot4D9510/4D9628 publishes the reference and dispatches its captured +4C
/// coordinate. Cell and object orders reach the same active locomotor owner;
/// `binary_frame` is Hover Move_To's paralysis clock.
pub(crate) fn set_destination_internal_coord(
    entity: &mut GameEntity,
    target: NavTargetRef,
    coord: DriveCoord,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    binary_frame: u32,
) {
    publish_nav_com(entity, target);

    if is_drive_locomotor(entity) {
        drive_set_destination(entity, coord, resolved_terrain);
    } else if is_ship_locomotor(entity) {
        ship_set_destination(entity, coord, resolved_terrain);
    } else if super::hover::hover_move_to(entity, coord, resolved_terrain, binary_frame) {
    } else {
        set_walk_destination_coord(entity, coord, resolved_terrain);
    }
}

/// Foot4D94C7/4D9510: NavComAux cleared and the reference published, before
/// the locomotor dispatch. The Foot+0x6AC skip (4D9607) stops here, and a
/// Teleport owner's Move_To is dispatched by the Unit setter.
pub(super) fn publish_nav_com(entity: &mut GameEntity, target: NavTargetRef) {
    entity.navigation.nav_com_aux = None;
    entity.navigation.nav_com = Some(target);
    entity.navigation.pending_arrival_clear = false;
}

/// Foot4D94C7/4D9510 NULL reference publication, before locomotor Stop.
pub(super) fn publish_null_nav_com(entity: &mut GameEntity) {
    entity.navigation.nav_com_aux = None;
    entity.navigation.nav_com = None;
    entity.navigation.pending_arrival_clear = false;
}

/// FootClass::Stop_Moving-equivalent owner clear (`0x004DF0D0`): zeroes only
/// the owner destination pair (NavCom and its auxiliary slot), nothing else.
pub(crate) fn foot_stop_moving(entity: &mut GameEntity) {
    entity.navigation.nav_com_aux = None;
    entity.navigation.nav_com = None;
}

fn drive_set_destination(
    entity: &mut GameEntity,
    destination: DriveCoord,
    terrain: Option<&ResolvedTerrainGrid>,
) -> bool {
    // Drive4AFD40 checks owner+270/+271 before any destination or map read.
    // Track MoveTo refusal leaves Foot's accepted NavCom/timer writes intact.
    // EMP/Unit+6D8 and Foot+6A0 producers remain separate unported gates.
    // Original whole-call comparisons: tools/spatial_oracle/track_destination.
    if super::locomotor_owner::owner_is_warping(entity) {
        return false;
    }
    let destination = adjusted_destination(destination, terrain);
    let Some(loco) = entity.locomotor.as_mut() else {
        return false;
    };
    loco.ensure_installed_track_state();
    // Native4AFD40 writes destination only. Accepted movement owns Head_To.
    loco.store_track_destination(TrackFamily::Drive, destination)
}

/// ILocomotion +0x44 Move_To of the active Drive/Ship instance without the
/// Foot setter (Drive 0x4AFD40 / Ship 0x69F450): the outer Process's NavCom
/// reissue (0x4B09A3 / 0x6A006C) calls it directly. Returns false when the
/// instance refused (warp) or is not Drive/Ship.
pub(super) fn track_move_to(
    entity: &mut GameEntity,
    coord: DriveCoord,
    terrain: Option<&ResolvedTerrainGrid>,
) -> bool {
    if is_drive_locomotor(entity) {
        drive_set_destination(entity, coord, terrain)
    } else if is_ship_locomotor(entity) {
        if super::locomotor_owner::owner_is_warping(entity) {
            return false;
        }
        ship_set_destination(entity, coord, terrain);
        true
    } else {
        false
    }
}

/// ILocomotion +0x48 Stop_Moving of the active Drive, Ship or Hover
/// instance, the only call of Foot's failed-path receiver 0x4D55C0 (Unit
/// +0x500). It clears the locomotor destination; NavCom and the committed
/// head are untouched.
pub(crate) fn track_stop_moving(entity: &mut GameEntity) -> bool {
    if is_drive_locomotor(entity) {
        drive_stop_moving(entity);
    } else if is_ship_locomotor(entity) {
        ship_stop_moving(entity);
    } else {
        return super::hover::hover_stop_moving(entity);
    }
    true
}

fn drive_stop_moving(entity: &mut GameEntity) {
    let Some(loco) = entity.locomotor.as_mut() else {
        return;
    };
    loco.ensure_installed_track_state();
    // 0x4AFE00 clamps the class target fraction (+0x50), then clears only
    // the destination; the head (+0x40) may continue to its endpoint. The
    // IsTrain follower cascade has no stock type (no retail IsTrain=yes).
    if loco
        .track_target_fraction(TrackFamily::Drive)
        .is_some_and(|v| v > TRACK_STOP_TARGET_FRACTION)
    {
        loco.store_track_target_fraction(TrackFamily::Drive, TRACK_STOP_TARGET_FRACTION);
    }
    loco.store_track_destination(TrackFamily::Drive, None);
    // OPEN Process-host timing: native Stop4AFE00 clamps only class target
    // and clears destination. The owner zero belongs to the admitted Process
    // rest tail4B0828, which also tests queue emptiness. Several ordinary
    // arrival returns skip that tail. Preserve the existing adapter timing
    // here until that continuation is wired; this is not Stop parity.
    if loco.track_head(TrackFamily::Drive).is_none() {
        if entity.foot_speed.applied_fraction() > SIM_ZERO {
            entity.foot_speed.set_speed_fraction(SIM_ZERO);
        }
    }
}

fn ship_set_destination(
    entity: &mut GameEntity,
    destination: DriveCoord,
    terrain: Option<&ResolvedTerrainGrid>,
) {
    // Ship69F450 has the same owner warp refusal before its map lookup.
    // Use the existing warp owner, independent of power and locomotor stash.
    if super::locomotor_owner::owner_is_warping(entity) {
        return;
    }
    let destination = adjusted_destination(destination, terrain);
    let Some(loco) = entity.locomotor.as_mut() else {
        return;
    };
    loco.ensure_installed_track_state();
    // Ship's Move_To slot writes only +0x30. The committed +0x3C head is
    // selected later by Process_Movement from the owner's path.
    loco.store_track_destination(TrackFamily::Ship, destination);
}

fn ship_stop_moving(entity: &mut GameEntity) {
    let Some(loco) = entity.locomotor.as_mut() else {
        return;
    };
    loco.ensure_installed_track_state();
    // Ship Stop_Moving clamps the class-owned target fraction, then clears
    // only +0x30. A committed head may continue to its track endpoint.
    if loco
        .track_target_fraction(TrackFamily::Ship)
        .is_some_and(|v| v > TRACK_STOP_TARGET_FRACTION)
    {
        loco.store_track_target_fraction(TrackFamily::Ship, TRACK_STOP_TARGET_FRACTION);
    }
    loco.store_track_destination(TrackFamily::Ship, None);

    // OPEN Process-host correction: this preexisting adapter applies the
    // rest speed before the native Process-tail admission. FootStop4DF0D0
    // does NOT clear Foot+5E0; explicit abandonment is a separate owner call.
    if loco.track_head(TrackFamily::Ship).is_none() {
        if entity.foot_speed.applied_fraction() > SIM_ZERO {
            entity.foot_speed.set_speed_fraction(SIM_ZERO);
        }
    }
}

impl crate::sim::world::Simulation {
    /// Foot4D94B0 clears NavComAux before its three nonnull admission gates.
    /// Class preprocessing precedes this call; publication, locomotor dispatch
    /// and accepted timers follow it. Linked-lift and retained-particle cleanup
    /// still require their missing native owners and are not implied here.
    pub(crate) fn begin_foot_destination(&mut self, id: u64, nonnull: bool) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let refused = nonnull
            && (entity.foot_locomotor_swap_active
                || entity.passenger_role.in_open_transport()
                || entity.bunker_link.installed_in().is_some());
        self.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .navigation
            .nav_com_aux = None;
        !refused
    }

    /// Foot's `Set_Destination` with a NULL target (`0x004D94B0`), where every
    /// class setter's null arm ends: Unit's at `0x0074314F`, Infantry's at
    /// `0x0051B1D2` and Aircraft's at once (`0x0041AA8B` -> `0x0041ADAC`).
    /// - NavComAux (`0x004D94C7`) and NavCom (`0x004D9510`) clear.
    /// - The linked-lift release (`0x004D9518..0x004D953F`) needs Foot+0x6AD,
    ///   the Magnetron latch, which VERA never raises.
    /// - An Aircraft (What_Am_I 2) whose current (`+0xAC`) or queued
    ///   (`+0xB4`) mission is Attack and which holds a TarCom (`+0x2B4`) skips
    ///   the locomotor Stop (`0x004D9672..0x004D969C`): its Fly flies on.
    /// - Otherwise the active locomotor's `Stop_Moving` runs (`0x004D96B9`,
    ///   [`Self::locomotor_stop_moving`]) and NavCom is cleared again
    ///   (`0x004D96BC`), so a re-target's NavCom does not survive.
    /// - The timer tail (`0x004D96C2..0x004D9707`) follows either way.
    ///
    /// The caller's class prelude runs first; the scheduling adapter is the
    /// caller's too.
    pub(crate) fn foot_null_destination(
        &mut self,
        id: u64,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) {
        use crate::sim::mission::{MissionId, MissionType};
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        publish_null_nav_com(entity);
        let attack = MissionId::from_known(MissionType::Attack);
        // AircraftMission owns the current aircraft dispatch. Its represented
        // Guard -> Attack transition does not update MissionState's current
        // slot yet; use that dispatch owner when present, and the raw slot for
        // receivers without it. Queued missions remain owned by MissionState.
        // Original current/queued Attack gate: track_destination_null_boundary;
        // the real Aircraft/Fly Attack call: aircraft_reengagement.
        let current_attack = entity.aircraft_mission.as_ref().map_or_else(
            || entity.mission.current() == attack,
            crate::sim::aircraft::AircraftMission::is_attacking,
        );
        let attacking_aircraft = entity.category == crate::map::entities::EntityCategory::Aircraft
            && (current_attack || entity.mission.queued() == attack)
            && entity.attack_target.is_some();
        if !attacking_aircraft {
            self.locomotor_stop_moving(id, rules, registry)
                .unwrap_or_else(|cause| panic!("Foot null destination {id}: {cause}"));
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.navigation.nav_com = None;
            }
        }
        let timing = super::DestinationTiming::from_rules(self.session.binary_frame, rules);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            timing.accept(entity);
        }
    }

    /// `Stop_Moving` (ILocomotion `+0x48`) of the owner's active locomotor, as
    /// Foot's null destination calls it (`0x004D96B9`) and as Foot's
    /// `Stop_Driver` is (`0x004D55C0`: the Unit and Aircraft vtable `+0x500`,
    /// and the tail of Infantry's `0x0051DAF0`):
    /// - Drive `0x004AFE00`, Ship `0x0069F510` and Hover `0x00516320`
    ///   ([`track_stop_moving`]);
    /// - Walk `0x0075ADA0` (`Simulation::walk_stop_moving`);
    /// - Jumpjet `0x0054B4D0` (`Simulation::jumpjet_stop_moving`), which
    ///   re-targets a moving owner to the passable cell nearest it;
    /// - Teleport `0x00718230`
    ///   ([`teleport_stop_moving`](super::teleport_movement::teleport_stop_moving));
    /// - Rocket `0x006633C0`, an empty body (`RET 4`).
    ///
    /// RESIDUAL: Fly's (`0x004CCFD0`) is not ported. While Is_Moving
    /// (`0x004CCA90`), it re-targets an Aircraft through its own setter
    /// (vt+0x480): while its mission (vt+0x184) is Attack, to the airfield
    /// `0x0041A160` answers; otherwise to the cell `0x00418E20` picks from
    /// the cell under it, which draws the Scenario RNG (`0x00418F4F`) when
    /// that cell will not do. An empty cell takes `ReceiveDamage` (Rules
    /// `+0xFA8`) instead. VERA drops the Fly's order adapter, the state its
    /// flight reads (`tick_air_movement`). Trigger: a null destination that
    /// Foot's attack gate lets through, or a Stop_Driver, on a moving
    /// aircraft: Stop outside an attack, the death Stun, a team script's or a
    /// Restore's null destination. Effect: the aircraft holds where it is
    /// until its mission orders it on, where native turns toward that
    /// airfield or cell, and no Scenario draw is made. Frequency: every Stop
    /// and kill of an aircraft in flight. Risk: the aircraft's path after
    /// Stop, and the Scenario stream.
    ///
    /// The Jumpjet's needs the map and, for a failed search, the rules; a
    /// world without them leaves the Jumpjet as it was.
    pub(crate) fn locomotor_stop_moving(
        &mut self,
        id: u64,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> Result<(), String> {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return Ok(());
        };
        let Some(kind) = entity.locomotor.as_ref().map(|loco| loco.active_kind()) else {
            return Ok(());
        };
        match kind {
            LocomotorKind::Drive | LocomotorKind::Ship | LocomotorKind::Hover => {
                track_stop_moving(entity);
            }
            LocomotorKind::Walk => self.walk_stop_moving(id, rules)?,
            LocomotorKind::Jumpjet => {
                if !self.jumpjet_stop_moving(id, rules, registry) {
                    log::debug!("Jumpjet {id} Stop_Moving lacks the map or rules");
                }
            }
            LocomotorKind::Teleport => super::teleport_movement::teleport_stop_moving(entity),
            LocomotorKind::Fly => {
                entity.movement_target = None;
            }
            LocomotorKind::Rocket => {}
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "track_destination_tests.rs"]
mod native_destination_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};
    use crate::util::fixed_math::{SIM_HALF, SIM_ONE};

    #[test]
    fn gsi_13_06_ship_destination_and_stop_stay_on_locomotor_runtime() {
        let mut entity = GameEntity::test_default(1, "DLPH", "Americans", 3, 3);
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Ship));

        set_destination_internal_cell(&mut entity, (4, 3), None, 0);
        let loco = entity.locomotor.as_mut().unwrap();
        assert_eq!(
            loco.track_destination(TrackFamily::Ship),
            Some(DriveCoord::cell(4, 3, 0))
        );
        assert_eq!(
            loco.track_head(TrackFamily::Ship),
            None,
            "Move_To does not invent a committed head"
        );
        loco.store_track_target_fraction(TrackFamily::Ship, SIM_ONE);
        entity.foot_speed.set_speed_fraction(SIM_HALF);
        entity.navigation.path_replay.directions = vec![2, 2];
        entity.navigation.path_replay.cursor = 0;

        track_stop_moving(&mut entity);
        let loco = entity.locomotor.as_ref().unwrap();
        assert_eq!(loco.track_destination(TrackFamily::Ship), None);
        assert_eq!(
            loco.track_target_fraction(TrackFamily::Ship),
            Some(TRACK_STOP_TARGET_FRACTION)
        );
        assert_eq!(entity.navigation.path_replay.cursor, 0);
        assert_eq!(entity.foot_speed.applied_fraction(), SIM_ZERO);
    }

    #[test]
    fn gsi_13_06_ship_stop_preserves_committed_head_and_owner_speed() {
        let mut entity = GameEntity::test_default(1, "DLPH", "Americans", 3, 3);
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Ship));
        entity.navigation.nav_com = Some(NavTargetRef::cell(5, 3));
        entity.navigation.path_replay = crate::sim::components::FootPathQueue {
            directions: vec![64, 64],
            cursor: 1,
            ..Default::default()
        };
        entity.foot_speed.set_speed_fraction(SIM_HALF);
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .install_ship_state_for_test(Some(
                ShipLocomotionRuntime::default()
                    .with_destination_for_test(Some(DriveCoord::cell(5, 3, 0)))
                    .with_head_to_for_test(Some(DriveCoord::cell(4, 3, 0)))
                    .with_target_speed_fraction_for_test(SIM_ONE),
            ));

        track_stop_moving(&mut entity);

        let loco = entity.locomotor.as_ref().unwrap();
        assert_eq!(loco.track_destination(TrackFamily::Ship), None);
        assert_eq!(
            loco.track_head(TrackFamily::Ship),
            Some(DriveCoord::cell(4, 3, 0))
        );
        assert_eq!(
            loco.track_target_fraction(TrackFamily::Ship),
            Some(TRACK_STOP_TARGET_FRACTION)
        );
        assert_eq!(entity.foot_speed.applied_fraction(), SIM_HALF);

        let loco = entity.locomotor.as_mut().unwrap();
        loco.store_track_destination(TrackFamily::Ship, Some(DriveCoord::cell(5, 3, 0)));
        loco.store_track_target_fraction(TrackFamily::Ship, SimFixed::lit("0.2"));
        track_stop_moving(&mut entity);
        assert_eq!(
            entity
                .locomotor
                .as_ref()
                .expect("Ship runtime")
                .track_target_fraction(TrackFamily::Ship),
            Some(SimFixed::lit("0.2")),
            "Stop stores min(previous target, 0.3)"
        );
    }

    fn resting_drive_miner() -> GameEntity {
        let mut entity = GameEntity::test_default(1, "HARV", "Americans", 3, 3);
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        entity.foot_speed.set_speed_fraction(SIM_ONE);
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(
                DriveLocomotionRuntime::default()
                    .with_destination_for_test(Some(DriveCoord::cell(3, 3, 0))),
            ));
        entity
    }

    /// GSI-06.11 G1: the gamemd Drive `Process` tail drives the applied speed
    /// fraction to exactly 0.0 at rest. There is no 0.3 clamp on this path, so
    /// every `Accelerates=true` departure — the Ore Miner and both MCVs in stock
    /// YR — ramps up from zero rather than launching at 30% speed.
    #[test]
    fn gsi_06_11_drive_rest_speed_fraction_returns_to_zero_not_a_stop_clamp() {
        let mut entity = resting_drive_miner();

        track_stop_moving(&mut entity);

        let loco = entity.locomotor.as_ref().unwrap();
        assert_eq!(entity.foot_speed.applied_fraction(), SIM_ZERO);
        assert_eq!(loco.track_destination(TrackFamily::Drive), None);
    }

    /// The reset is gated, not unconditional: gamemd requires the head-to coord
    /// to be empty too, so a mover still committed to a head keeps its fraction.
    #[test]
    fn gsi_06_11_drive_rest_reset_requires_an_empty_head_to() {
        let mut entity = resting_drive_miner();
        entity
            .locomotor
            .as_mut()
            .expect("drive state")
            .store_track_head(TrackFamily::Drive, Some(DriveCoord::cell(4, 3, 0)));
        entity.foot_speed.set_speed_fraction(SIM_HALF);

        track_stop_moving(&mut entity);

        assert_eq!(entity.foot_speed.applied_fraction(), SIM_HALF);
    }

    #[test]
    fn resolve_nav_target_drive_coord_tracks_moving_entity() {
        let mut entities = EntityStore::new();
        let mut target = GameEntity::test_default(2, "E1", "Allies", 3, 4);
        target.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
        entities.insert(target);

        let first =
            nav_target_coordinate(NavTargetRef::Entity { id: 2 }, None, &entities, None, None)
                .unwrap();
        entities.get_mut(2).unwrap().position.rx += 1;
        let second =
            nav_target_coordinate(NavTargetRef::Entity { id: 2 }, None, &entities, None, None)
                .unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn nav_target_coordinate_dispatches_cells_and_rejects_dangling_objects() {
        let entities = EntityStore::new();
        assert_eq!(
            nav_target_coordinate(NavTargetRef::cell(12, 34), None, &entities, None, None).unwrap(),
            DriveCoord::cell(12, 34, 0)
        );
        for target in [
            NavTargetRef::Entity { id: 7 },
            NavTargetRef::Object { id: 7 },
            NavTargetRef::Building { id: 7 },
        ] {
            assert!(
                nav_target_coordinate(target, None, &entities, None, None)
                    .unwrap_err()
                    .contains("disappeared")
            );
        }
    }
}
