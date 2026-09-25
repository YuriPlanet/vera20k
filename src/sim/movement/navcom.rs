//! FootClass-style navigation destination helpers.
//!
//! These helpers model the owner `NavCom` lifecycle separately from
//! `MovementTarget`, which remains the active path execution adapter.

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::{
    DriveCoord, DriveLocomotionRuntime, NavTargetRef, ShipLocomotionRuntime,
};
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::MissionType;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

/// Drive Stop_Moving 0x4AFE00 and Ship 0x69F510 clamp the class target
/// fraction to the float 0.3 at 0x7E6240 / 0x7F1308 (identical bodies).
const TRACK_STOP_TARGET_FRACTION: SimFixed = SimFixed::lit("0.3");

/// Accepted Walk75ACB0 destination store. This conversion is independent of
/// HeadTo/OnBridge's416: original6D1830,6D18C0,6D1BF0 initialize the scale for
/// 6D2120(60), whose result is414 under captured startup FPCW0E7F and027F.
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
                coord.z = coord.z.wrapping_add(414);
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
    resolved_terrain: Option<&ResolvedTerrainGrid>,
) -> DriveCoord {
    // Cell486840 sign-extends the returned object's own CellStruct, including
    // aliased fixed-grid lookups and the shared dummy's stamped coordinates.
    let identity =
        resolved_terrain.map(|terrain| terrain.native_cell_identity((rx as i16, ry as i16)));
    let cell = resolved_terrain
        .zip(identity)
        .map_or((rx as i16, ry as i16), |(terrain, identity)| {
            terrain.native_cell_coord(identity)
        });
    let mut coord = DriveCoord {
        x: i32::from(cell.0) * 256 + 128,
        y: i32::from(cell.1) * 256 + 128,
        z: 0,
    };
    if let Some((terrain, identity)) = resolved_terrain.zip(identity) {
        // 486840 -> 47B3A0 retains the SAME Cell receiver for height. A new
        // world-XY lookup changes the cell for negative/aliased coordinates.
        let (level, slope) = match identity {
            crate::map::cell_index::NativeCellIdentity::Real(index) => {
                let cell = &terrain.cells()[index];
                (cell.level, cell.slope_type)
            }
            crate::map::cell_index::NativeCellIdentity::Dummy => {
                let dummy = terrain.shared_cell_dummy().snapshot();
                (dummy.level as u8, dummy.slope_type)
            }
        };
        coord.z =
            crate::util::lepton::ground_height_leptons(level, slope, coord.x, coord.y).unwrap_or(0);
    }
    coord
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
                .wrapping_add(crate::util::lepton::BRIDGE_HEIGHT_DELTA_LEPTONS as i32);
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
    let Some(drive) = entity.drive_locomotion.as_ref() else {
        return false;
    };
    let requested = (coord != (DriveCoord { x: 0, y: 0, z: 0 })).then_some(coord);
    if drive.destination == requested {
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
    rules: &crate::rules::ruleset::RuleSet,
    interner: &crate::sim::intern::StringInterner,
) -> Option<(u16, u16)> {
    let building = entities.get(building_id)?;
    let object = rules.object(interner.resolve(building.type_ref()))?;
    let coord = super::building_coordinate::dock_coordinate(
        super::ground_pose::position_world_coord(&building.position),
        super::ground_pose::object_center_coord(building, object),
        object,
        &building.radio_contacts,
        requester,
        || {
            let id = requester.ok_or("Bunker dock coordinate needs a requester")?;
            let entity = entities
                .get(id)
                .ok_or_else(|| format!("Dock requester {id} disappeared"))?;
            Ok(rules
                .object(interner.resolve(entity.type_ref()))
                .map_or_else(
                    || {
                        super::ground_pose::object_center_coord_with_foundation(
                            entity,
                            &entity.foundation,
                        )
                    },
                    |object| super::ground_pose::object_center_coord(entity, object),
                ))
        },
    )
    .ok()?;
    let cell = |value: i32| u16::try_from(value / 256).ok();
    Some((cell(coord.x)?, cell(coord.y)?))
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
        NavTargetRef::Cell { rx, ry } => return Ok(target_cell_coord(rx, ry, terrain)),
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
            super::ground_pose::position_world_coord(&entity.position),
            super::ground_pose::object_center_coord(entity, object),
            object,
            &entity.radio_contacts,
            requester,
            || {
                let id = requester.expect("Bunker only reads a non-null requester");
                let entity = entities
                    .get(id)
                    .ok_or_else(|| format!("Navigation requester {id} disappeared"))?;
                Ok(rules
                    .object(interner.resolve(entity.type_ref()))
                    .map_or_else(
                        || {
                            super::ground_pose::object_center_coord_with_foundation(
                                entity,
                                &entity.foundation,
                            )
                        },
                        |object| super::ground_pose::object_center_coord(entity, object),
                    ))
            },
        );
    }
    super::foot_coordinate::navigation_coordinate(entity, terrain)
}

/// Owner non-null destination path for the Phase 1 normal cell-target slice.
pub(crate) fn set_destination_internal_cell(
    entity: &mut GameEntity,
    target: (u16, u16),
    resolved_terrain: Option<&ResolvedTerrainGrid>,
) {
    let coord = target_cell_coord(target.0, target.1, resolved_terrain);
    set_destination_internal_coord(
        entity,
        NavTargetRef::cell(target.0, target.1),
        coord,
        resolved_terrain,
    );
}

/// Foot4D9510/4D9628 publishes the reference and dispatches its captured +4C
/// coordinate. Cell and object orders reach the same active locomotor owner.
pub(crate) fn set_destination_internal_coord(
    entity: &mut GameEntity,
    target: NavTargetRef,
    coord: DriveCoord,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
) {
    publish_nav_com(entity, target);

    if is_drive_locomotor(entity) {
        drive_set_destination(entity, coord, resolved_terrain);
    } else if is_ship_locomotor(entity) {
        ship_set_destination(entity, coord, resolved_terrain);
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

/// Owner null destination path. Clears the owner and active Drive/Ship destination.
pub(super) fn set_destination_internal_null(entity: &mut GameEntity) {
    entity.navigation.nav_com_aux = None;
    entity.navigation.nav_com = None;
    entity.navigation.pending_arrival_clear = false;

    if is_drive_locomotor(entity) {
        drive_stop_moving(entity);
    } else if is_ship_locomotor(entity) {
        ship_stop_moving(entity);
    } else if let Some(loco) = entity.locomotor.as_mut() {
        loco.stop_walk();
    }
}

/// FootClass::Stop_Moving-equivalent owner clear (`0x004DF0D0`): zeroes only
/// the owner destination pair (NavCom and its auxiliary slot), nothing else.
pub(crate) fn foot_stop_moving(entity: &mut GameEntity) {
    entity.navigation.nav_com_aux = None;
    entity.navigation.nav_com = None;
}

/// Return the drive-track runtime to rest: no aim point, no active curve.
fn reset_drive_track_runtime(entity: &mut GameEntity) {
    if let Some(drive) = entity.drive_locomotion.as_mut() {
        drive.head_to = None;
        drive.track_valid = false;
        drive.track.turn_index = -1;
        drive.track.cursor = 0;
    }
}

/// End-of-track owner-navigation resolution for a mover whose path finished
/// this tick. Native contract (drive end-of-track block): when the track ends
/// at the owner destination on a live object, the stop is immediate — the
/// owner destination pair clears the same tick, the path head resets, and,
/// only when the current mission is Move, the arrival advance pops the queued
/// waypoint into a fresh destination. Dying/limbo objects skip the clear
/// entirely (only the ended track's aim point drops). A track that ends away
/// from the owner destination (or a non-cell owner target) keeps the
/// destination; the deferred process-entry pass repaths toward it next tick.
pub(super) fn finish_drive_navigation(
    entity: &mut GameEntity,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
) {
    // Walk's PerCell completion owns its cell/height destination test. Reaching
    // an A* approach endpoint does not authorize Foot SetDestination(NULL).
    if entity
        .locomotor
        .as_ref()
        .is_some_and(|l| l.kind == LocomotorKind::Walk)
    {
        return;
    }
    if is_drive_locomotor(entity) && entity.navigation.nav_com.is_some() {
        if entity.dying {
            // Native liveness gate: no owner clear for a dying object; the
            // ended track still loses its aim point.
            if let Some(drive) = entity.drive_locomotion.as_mut() {
                drive.head_to = None;
            }
            return;
        }
        // Residual: the native arrival match also compares z within twice a
        // global height tolerance (bridge deck vs ground); NavTargetRef::Cell
        // carries no layer, so a same-cell bridge/ground mismatch reads as
        // arrived here.
        let arrived = matches!(
            entity.navigation.nav_com,
            Some(NavTargetRef::Cell { rx, ry })
                if rx == entity.position.rx && ry == entity.position.ry
        );
        if arrived {
            finish_drive_arrival(entity, resolved_terrain);
        } else {
            defer_drive_arrival_clear(entity);
        }
        return;
    }
    if is_ship_locomotor(entity) {
        // Ship's terminal Process_Movement retires the committed +0x3C head
        // before its no-destination/no-path Process tail calls owner
        // SetSpeedFraction(0). Mark the replay queue exhausted first so the
        // ordinary Ship null-destination path observes that same rest state.
        if let Some(ship) = entity.ship_locomotion.as_mut() {
            ship.head_to = None;
            entity.navigation.path_replay.cursor = entity
                .navigation
                .path_replay
                .directions
                .len()
                .min(u16::MAX as usize) as u16;
        }
        set_destination_internal_null(entity);
        entity.navigation.nav_queue.clear();
        return;
    }
    // A soft Stop can clear the owner destination while an already-committed
    // Drive curve is still consuming. Its ordinary terminal Enter still clears
    // the head/valid/selector/cursor tuple even though NavCom is already null.
    if is_drive_locomotor(entity) {
        reset_drive_track_runtime(entity);
    }
    // Non-drive movers (and the remaining Drive owner state) keep the
    // pre-existing immediate cleanup.
    set_destination_internal_null(entity);
    entity.navigation.nav_queue.clear();
}

/// Same-tick arrival at the owner destination: clear the owner destination
/// pair immediately, return the drive runtime to rest, and — only under a
/// current Move mission (the native arrival gate) — advance the queued
/// waypoint into a fresh destination. The path toward the fresh destination
/// is built by the deferred process-entry pass at the top of the next
/// movement tick, matching the native next-process track build.
fn finish_drive_arrival(entity: &mut GameEntity, resolved_terrain: Option<&ResolvedTerrainGrid>) {
    foot_stop_moving(entity);
    entity.navigation.pending_arrival_clear = false;
    reset_drive_track_runtime(entity);
    // VERA-internal rest-state cleanup (speed clamp + drive destination
    // drop) — the same rest state the deferred clear used to reach one tick
    // later; the native drive-runtime equivalent is UNCHECKED.
    drive_stop_moving(entity);
    if entity.mission.effective().known() != Some(MissionType::Move) {
        return;
    }
    let Some(NavTargetRef::Cell { rx, ry }) = entity.navigation.nav_queue.first().copied() else {
        return;
    };
    entity.navigation.nav_queue.remove(0);
    set_destination_internal_cell(entity, (rx, ry), resolved_terrain);
    entity.navigation.pending_arrival_clear = true;
}

/// Track/path execution finished away from the owner destination (or the
/// owner target is not a plain cell): the owner keeps its destination, and
/// the deferred pass at the top of the next movement tick rebuilds a path
/// toward it — the drive locomotor's process-entry fallback. Arrivals AT the
/// owner destination never come through here; they clear immediately via
/// [`finish_drive_arrival`].
pub(super) fn defer_drive_arrival_clear(entity: &mut GameEntity) -> bool {
    if !is_drive_locomotor(entity) || entity.navigation.nav_com.is_none() {
        return false;
    }
    entity.navigation.pending_arrival_clear = true;
    reset_drive_track_runtime(entity);
    true
}

pub(super) fn process_pending_empty_drive_arrivals_in_order(
    entities: &mut EntityStore,
    ids: &[u64],
) {
    for &id in ids {
        let Some(entity) = entities.get_mut(id) else {
            continue;
        };
        if !entity.navigation.pending_arrival_clear {
            continue;
        }
        if entity.movement_target.is_some()
            || crate::sim::movement::track_head::committed_track_head(entity).is_some()
        {
            continue;
        }
        if entity.navigation.nav_queue.is_empty() {
            set_destination_internal_null(entity);
        }
    }
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
    let drive = entity
        .drive_locomotion
        .get_or_insert_with(DriveLocomotionRuntime::default);
    // Native4AFD40 writes destination only. Accepted movement owns Head_To.
    drive.destination = destination;
    true
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

/// ILocomotion +0x48 Stop_Moving of the active Drive/Ship instance, the only
/// call of Foot's failed-path receiver 0x4D55C0 (Unit +0x500). It clears the
/// locomotor destination; NavCom and the committed head are untouched.
pub(crate) fn track_stop_moving(entity: &mut GameEntity) -> bool {
    if is_drive_locomotor(entity) {
        drive_stop_moving(entity);
    } else if is_ship_locomotor(entity) {
        ship_stop_moving(entity);
    } else {
        return false;
    }
    true
}

fn drive_stop_moving(entity: &mut GameEntity) {
    let drive = entity
        .drive_locomotion
        .get_or_insert_with(DriveLocomotionRuntime::default);
    // 0x4AFE00 clamps the class target fraction (+0x50), then clears only
    // the destination; the head (+0x40) may continue to its endpoint. The
    // IsTrain follower cascade has no stock type (no retail IsTrain=yes).
    if drive.target_speed_fraction > TRACK_STOP_TARGET_FRACTION {
        drive.target_speed_fraction = TRACK_STOP_TARGET_FRACTION;
    }
    drive.destination = None;
    // OPEN Process-host timing: native Stop4AFE00 clamps only class target
    // and clears destination. The owner zero belongs to the admitted Process
    // rest tail4B0828, which also tests queue emptiness. Several ordinary
    // arrival returns skip that tail. Preserve the existing adapter timing
    // here until that continuation is wired; this is not Stop parity.
    if drive.head_to.is_none() {
        if entity.foot_speed.applied_fraction > SIM_ZERO {
            entity.foot_speed.applied_fraction = SIM_ZERO;
        }
        entity.foot_speed.cached_current_speed = 0;
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
    let ship = entity
        .ship_locomotion
        .get_or_insert_with(ShipLocomotionRuntime::default);
    // Ship's Move_To slot writes only +0x30. The committed +0x3C head is
    // selected later by Process_Movement from the owner's path.
    ship.destination = destination;
}

fn ship_stop_moving(entity: &mut GameEntity) {
    let ship = entity
        .ship_locomotion
        .get_or_insert_with(ShipLocomotionRuntime::default);
    // Ship Stop_Moving clamps the class-owned target fraction, then clears
    // only +0x30. A committed head may continue to its track endpoint.
    if ship.target_speed_fraction > TRACK_STOP_TARGET_FRACTION {
        ship.target_speed_fraction = TRACK_STOP_TARGET_FRACTION;
    }
    ship.destination = None;

    // OPEN Process-host correction: this preexisting adapter applies the
    // rest speed before the native Process-tail admission. FootStop4DF0D0
    // does NOT clear Foot+5E0; explicit abandonment is a separate owner call.
    if ship.head_to.is_none() {
        if entity.foot_speed.applied_fraction > SIM_ZERO {
            entity.foot_speed.applied_fraction = SIM_ZERO;
        }
        entity.foot_speed.cached_current_speed = 0;
    }
}

impl crate::sim::world::Simulation {
    /// Foot4D94B0 clears NavComAux before its three nonnull admission gates.
    /// Class preprocessing precedes this call; publication, locomotor dispatch
    /// and accepted timers follow it. Linked-lift and retained-particle cleanup
    /// still require their missing native owners and are not implied here.
    pub(crate) fn begin_foot_destination(
        &mut self,
        id: u64,
        nonnull: bool,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let open_transport = crate::sim::passenger::open_topped_transport(
            &self.substrate.entities,
            rules,
            &self.interner,
            entity,
        )
        .is_some();
        let refused = nonnull
            && (entity.foot_locomotor_swap_active
                || open_transport
                || entity.bunker_link.installed_in().is_some());
        self.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .navigation
            .nav_com_aux = None;
        !refused
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
    use crate::util::fixed_math::{SIM_HALF, SIM_ONE};

    #[test]
    fn gsi_13_06_ship_destination_and_stop_stay_on_locomotor_runtime() {
        let mut entity = GameEntity::test_default(1, "DLPH", "Americans", 3, 3);
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Ship));

        set_destination_internal_cell(&mut entity, (4, 3), None);
        let ship = entity.ship_locomotion.as_mut().expect("Ship runtime");
        assert_eq!(ship.destination, Some(DriveCoord::cell(4, 3, 0)));
        assert_eq!(
            ship.head_to, None,
            "Move_To does not invent a committed head"
        );
        ship.target_speed_fraction = SIM_ONE;
        entity.foot_speed.applied_fraction = SIM_HALF;
        entity.foot_speed.cached_current_speed = 10;
        entity.navigation.path_replay.directions = vec![2, 2];
        entity.navigation.path_replay.cursor = 0;

        set_destination_internal_null(&mut entity);
        let ship = entity.ship_locomotion.as_ref().expect("Ship runtime");
        assert_eq!(ship.destination, None);
        assert_eq!(ship.target_speed_fraction, TRACK_STOP_TARGET_FRACTION);
        assert_eq!(entity.navigation.path_replay.cursor, 0);
        assert_eq!(entity.foot_speed.applied_fraction, SIM_ZERO);
        assert_eq!(entity.foot_speed.cached_current_speed, 0);
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
        entity.foot_speed.applied_fraction = SIM_HALF;
        entity.foot_speed.cached_current_speed = 10;
        entity.ship_locomotion = Some(ShipLocomotionRuntime {
            destination: Some(DriveCoord::cell(5, 3, 0)),
            head_to: Some(DriveCoord::cell(4, 3, 0)),
            target_speed_fraction: SIM_ONE,
            track: Default::default(),
            ..Default::default()
        });

        set_destination_internal_null(&mut entity);

        let ship = entity.ship_locomotion.as_ref().expect("Ship runtime");
        assert_eq!(ship.destination, None);
        assert_eq!(ship.head_to, Some(DriveCoord::cell(4, 3, 0)));
        assert_eq!(ship.target_speed_fraction, TRACK_STOP_TARGET_FRACTION);
        assert_eq!(entity.foot_speed.applied_fraction, SIM_HALF);
        assert_eq!(entity.foot_speed.cached_current_speed, 10);

        let ship = entity.ship_locomotion.as_mut().expect("Ship runtime");
        ship.destination = Some(DriveCoord::cell(5, 3, 0));
        ship.target_speed_fraction = SimFixed::lit("0.2");
        set_destination_internal_null(&mut entity);
        assert_eq!(
            entity
                .ship_locomotion
                .as_ref()
                .expect("Ship runtime")
                .target_speed_fraction,
            SimFixed::lit("0.2"),
            "Stop stores min(previous target, 0.3)"
        );
    }

    #[test]
    fn gsi_13_06_ship_final_arrival_retires_head_and_owner_speed() {
        let mut entity = GameEntity::test_default(1, "DLPH", "Americans", 4, 3);
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Ship));
        entity.navigation.nav_com = Some(NavTargetRef::cell(4, 3));
        entity.navigation.path_replay = crate::sim::components::FootPathQueue {
            directions: vec![64],
            cursor: 0,
            ..Default::default()
        };
        entity.foot_speed.applied_fraction = SIM_HALF;
        entity.foot_speed.cached_current_speed = 10;
        entity.ship_locomotion = Some(ShipLocomotionRuntime {
            destination: Some(DriveCoord::cell(4, 3, 0)),
            head_to: Some(DriveCoord::cell(4, 3, 0)),
            target_speed_fraction: SIM_ONE,
            track: Default::default(),
            ..Default::default()
        });

        finish_drive_navigation(&mut entity, None);

        let ship = entity.ship_locomotion.as_ref().expect("Ship runtime");
        assert_eq!(ship.destination, None);
        assert_eq!(ship.head_to, None);
        assert_eq!(entity.navigation.path_replay.cursor, 1);
        assert_eq!(entity.foot_speed.applied_fraction, SIM_ZERO);
        assert_eq!(entity.foot_speed.cached_current_speed, 0);
    }

    fn resting_drive_miner() -> GameEntity {
        let mut entity = GameEntity::test_default(1, "HARV", "Americans", 3, 3);
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        entity.foot_speed.applied_fraction = SIM_ONE;
        entity.drive_locomotion = Some(DriveLocomotionRuntime {
            destination: Some(DriveCoord::cell(3, 3, 0)),
            ..Default::default()
        });
        entity
    }

    /// GSI-06.11 G1: the gamemd Drive `Process` tail drives the applied speed
    /// fraction to exactly 0.0 at rest. There is no 0.3 clamp on this path, so
    /// every `Accelerates=true` departure — the Ore Miner and both MCVs in stock
    /// YR — ramps up from zero rather than launching at 30% speed.
    #[test]
    fn gsi_06_11_drive_rest_speed_fraction_returns_to_zero_not_a_stop_clamp() {
        let mut entity = resting_drive_miner();

        set_destination_internal_null(&mut entity);

        let drive = entity.drive_locomotion.as_ref().expect("drive state");
        assert_eq!(entity.foot_speed.applied_fraction, SIM_ZERO);
        assert_eq!(drive.destination, None);
    }

    /// The reset is gated, not unconditional: gamemd requires the head-to coord
    /// to be empty too, so a mover still committed to a head keeps its fraction.
    #[test]
    fn gsi_06_11_drive_rest_reset_requires_an_empty_head_to() {
        let mut entity = resting_drive_miner();
        entity
            .drive_locomotion
            .as_mut()
            .expect("drive state")
            .head_to = Some(DriveCoord::cell(4, 3, 0));
        entity.foot_speed.applied_fraction = SIM_HALF;

        set_destination_internal_null(&mut entity);

        assert_eq!(entity.foot_speed.applied_fraction, SIM_HALF);
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
