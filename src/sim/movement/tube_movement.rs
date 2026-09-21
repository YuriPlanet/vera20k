//! Native TubeClass movement for explicit map tubes.
//!
//! gamemd: `UnitClass::TubeMovement` 0x007359F0, which reads the cell's tube
//! through `CellClass::GetTubeAtCell` 0x00484F20 and, on exit, drives
//! `FacingClass::UpdateFacing` from the record's +0x2C direction field.
//!
//! Automatic tunnel/low-bridge records have a zero-length path. They are
//! predicate and zone metadata only: the retail producer divides by path
//! length, so a zero-step shell must never become active movement state.

use std::sync::OnceLock;

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::retail_trig::TrigTable;
use crate::map::tube_facts::{TubeFact, TubeId, TubeSource};
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{DriveCoord, DriveLocomotionRuntime, MovementTarget, Position};
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::StringInterner;
use crate::sim::movement::bump_crush;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{
    CellListInsertion, CellOccupationGrid, OccupancyGrid, RawCellOccupationGrid,
    VEHICLE_OCCUPATION_BIT, infantry_raw_occupation_mask,
};
use crate::sim::pathfinding::PathGrid;
use crate::sim::rng::SimRng;
use crate::sim::world::EnterOrderCounter;
use crate::util::fixed_math::{SIM_ONE, SIM_ZERO};
use crate::util::lepton::{self, CELL_CENTER_LEPTON};
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53, sqrt_approx_f32};

/// FootClass-owned active TubeMovement payload.
///
/// `Some` also means the mover is absent from CellClass object lists and raw
/// occupation. Immutable path bytes remain in the map-owned [`TubeFact`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct LowBridgeTubeMovementState {
    pub tube_id: TubeId,
    pub cursor: u8,
    pub target: DriveCoord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TubeBeginError {
    ZeroLengthTube,
    UnsupportedCategory,
    MissingTerrain,
}

/// Identify the native direction-8 path shape represented by Rust's
/// non-adjacent path node. This is a producer admission check only; state and
/// substrate mutation happen atomically in [`begin_path_tube_step`].
pub fn pending_path_tube_id(
    target: &MovementTarget,
    position: &Position,
    current_layer: MovementLayer,
    terrain: Option<&ResolvedTerrainGrid>,
) -> Option<TubeId> {
    if current_layer != MovementLayer::Ground
        || target.bypass_grid
        || target.next_index >= target.path.len()
    {
        return None;
    }
    let current = (position.rx, position.ry);
    let next = target.path[target.next_index];
    let dx = i32::from(next.0) - i32::from(current.0);
    let dy = i32::from(next.1) - i32::from(current.1);
    if (dx.abs() <= 1 && dy.abs() <= 1) || (dx == 0 && dy == 0) {
        return None;
    }
    let terrain = terrain?;
    let tube_id = terrain.cell(current.0, current.1)?.tube_index?;
    if tube_id.0 > i8::MAX as u16 {
        return None;
    }
    let tube = terrain.tube(tube_id)?;
    (tube.source == TubeSource::ExplicitMap && tube.path_len() > 0 && tube.exit == next)
        .then_some(tube_id)
}

/// Begin a verified explicit tube and perform native Mark(REMOVE) substrate
/// teardown. The pending non-adjacent node is consumed immediately, preserving
/// the route tail for the post-tube object turn.
#[allow(clippy::too_many_arguments)]
pub(crate) fn begin_path_tube_step(
    foot_occupation_enabled: &mut bool,
    path_replay: &mut crate::sim::components::FootPathQueue,
    entity_id: u64,
    category: EntityCategory,
    position: &mut Position,
    drive_locomotion: &mut Option<DriveLocomotionRuntime>,
    low_bridge_tube_state: &mut Option<LowBridgeTubeMovementState>,
    target: &mut MovementTarget,
    cell_marked: &mut bool,
    tube_id: TubeId,
    terrain: &ResolvedTerrainGrid,
    occupancy: &mut OccupancyGrid,
    cell_occupation: &mut CellOccupationGrid,
    raw_cell_occupation: &mut RawCellOccupationGrid,
) -> Result<(), TubeBeginError> {
    if !matches!(category, EntityCategory::Unit | EntityCategory::Infantry) {
        return Err(TubeBeginError::UnsupportedCategory);
    }
    if category == EntityCategory::Unit
        && drive_locomotion
            .as_ref()
            .is_some_and(|drive| drive.track.turn_index != -1)
    {
        return Err(TubeBeginError::MissingTerrain);
    }
    let tube = terrain
        .tube(tube_id)
        .ok_or(TubeBeginError::MissingTerrain)?;
    let state = initial_state(category, position, tube_id, tube, terrain)?;

    detach_for_tube(
        foot_occupation_enabled,
        entity_id,
        category,
        position,
        drive_locomotion,
        cell_marked,
        occupancy,
        cell_occupation,
        raw_cell_occupation,
    );
    if category == EntityCategory::Unit
        && let Some(drive) = drive_locomotion.as_mut()
    {
        // Original4B1352/1357/135A stores the signed exit center in Head_To
        // with Z=0. Destination, cursor, short selection and residual survive.
        // The queue shift4B1362..136E does not rewrite Foot+558's reference.
        drive.head_to = Some(DriveCoord {
            x: i32::from(tube.exit.0 as i16)
                .wrapping_mul(256)
                .wrapping_add(128),
            y: i32::from(tube.exit.1 as i16)
                .wrapping_mul(256)
                .wrapping_add(128),
            z: 0,
        });
        super::path_markers::consume_path_replay(path_replay, 1);
        drive.track_valid = true; // Original4B1480.
        drive.track.turn_index = -1; // Original4B1484; cursor is untouched.
    }
    *low_bridge_tube_state = Some(state);
    target.next_index = target.next_index.saturating_add(1).min(target.path.len());
    Ok(())
}

fn initial_state(
    category: EntityCategory,
    position: &mut Position,
    tube_id: TubeId,
    tube: &TubeFact,
    terrain: &ResolvedTerrainGrid,
) -> Result<LowBridgeTubeMovementState, TubeBeginError> {
    let path_len = tube.path_len();
    if path_len == 0 {
        return Err(TubeBeginError::ZeroLengthTube);
    }
    let raw_step = tube.path_steps[0];
    let (dx, dy) = direction_delta(raw_step);
    let current_x = position_world_x(position);
    let current_y = position_world_y(position);
    let current_ground =
        ground_height_at(terrain, current_x, current_y).ok_or(TubeBeginError::MissingTerrain)?;
    let exit_ground =
        ground_height_at_cell_center(terrain, tube.exit).ok_or(TubeBeginError::MissingTerrain)?;
    let z_step = exit_ground.wrapping_sub(current_ground) / path_len as i32;
    let (target_x, target_y) = if category == EntityCategory::Infantry {
        (
            current_x.wrapping_add(dx.wrapping_mul(256)),
            current_y.wrapping_add(dy.wrapping_mul(256)),
        )
    } else {
        let next_x = i32::from(tube.entry.0).wrapping_add(dx);
        let next_y = i32::from(tube.entry.1).wrapping_add(dy);
        (
            next_x.wrapping_mul(256).wrapping_add(128),
            next_y.wrapping_mul(256).wrapping_add(128),
        )
    };
    position.exact_z_leptons.get_or_insert(current_ground);
    Ok(LowBridgeTubeMovementState {
        tube_id,
        cursor: 0,
        target: DriveCoord {
            x: target_x,
            y: target_y,
            z: current_ground.wrapping_add(z_step),
        },
    })
}

fn detach_for_tube(
    foot_occupation_enabled: &mut bool,
    entity_id: u64,
    category: EntityCategory,
    position: &Position,
    drive_locomotion: &mut Option<DriveLocomotionRuntime>,
    cell_marked: &mut bool,
    occupancy: &mut OccupancyGrid,
    cell_occupation: &mut CellOccupationGrid,
    raw_cell_occupation: &mut RawCellOccupationGrid,
) {
    let rx = position.rx;
    let ry = position.ry;
    occupancy.remove_on_layer(rx, ry, entity_id, MovementLayer::Ground);
    match category {
        EntityCategory::Unit => {
            if let Some(drive) = drive_locomotion.as_mut() {
                crate::sim::occupancy::clear_drive_head_to_occupation_for_remove(
                    drive,
                    cell_occupation,
                    entity_id,
                );
                *foot_occupation_enabled = false;
            }
            cell_occupation.clear_vehicle_on_layer(rx, ry, entity_id, MovementLayer::Ground);
            raw_cell_occupation.clear_ground(rx, ry, VEHICLE_OCCUPATION_BIT);
        }
        EntityCategory::Infantry => raw_cell_occupation.clear_ground(
            rx,
            ry,
            infantry_raw_occupation_mask(position.sub_x, position.sub_y),
        ),
        _ => {}
    }
    *cell_marked = false;
}

/// Execute one active Unit/Infantry TubeMovement object turn. Returns `true`
/// whenever the entity was active at entry, including a successful final which
/// cleared the state; the caller must therefore suppress ordinary movement for
/// the remainder of this tick.
#[allow(clippy::too_many_arguments)]
pub(crate) fn tick_active_tube_object(
    entities: &mut EntityStore,
    entity_id: u64,
    terrain: &ResolvedTerrainGrid,
    path_grid: Option<&PathGrid>,
    occupancy: &mut OccupancyGrid,
    cell_occupation: &mut CellOccupationGrid,
    raw_cell_occupation: &mut RawCellOccupationGrid,
    next_occupancy_enter_order: &mut EnterOrderCounter,
    rules: Option<&RuleSet>,
    interner: &StringInterner,
    rng: &mut SimRng,
    native_frame: u32,
) -> bool {
    let Some(entity) = entities.get(entity_id) else {
        return false;
    };
    let Some(mut state) = entity.low_bridge_tube_state else {
        return false;
    };
    let Some(tube) = terrain.tube(state.tube_id) else {
        return true;
    };
    let category = entity.category;
    let speed = rules
        .and_then(|rules| rules.object(interner.resolve(entity.type_ref())))
        .map_or(0, |object| native_type_speed(object.speed));
    let budget = if category == EntityCategory::Unit {
        speed.wrapping_mul(3) / 2
    } else {
        speed
    };
    let z_step = live_z_step(terrain, tube).unwrap_or(0);

    if usize::from(state.cursor) >= tube.path_len() {
        return finalize_tube_object(
            entities,
            entity_id,
            state,
            terrain,
            path_grid,
            occupancy,
            cell_occupation,
            raw_cell_occupation,
            next_occupancy_enter_order,
            rules,
            interner,
            rng,
            native_frame,
        );
    }

    let current = entity_world_coord(entity, terrain).unwrap_or(state.target);
    let distance = native_distance_leptons(current, state.target);
    if distance > budget {
        let Some(trig) = active_tube_trig() else {
            return true;
        };
        let raw_step = tube.path_steps[usize::from(state.cursor)];
        let next = advance_amount(current, state.target, budget, raw_step, z_step, trig);
        if let Some(entity) = entities.get_mut(entity_id) {
            set_entity_world_coord(entity, next);
        }
        return true;
    }

    state.cursor = state.cursor.saturating_add(1);
    if let Some(entity) = entities.get_mut(entity_id) {
        set_entity_world_coord(entity, state.target);
        entity.low_bridge_tube_state = Some(state);
    }
    if usize::from(state.cursor) >= tube.path_len() {
        return finalize_tube_object(
            entities,
            entity_id,
            state,
            terrain,
            path_grid,
            occupancy,
            cell_occupation,
            raw_cell_occupation,
            next_occupancy_enter_order,
            rules,
            interner,
            rng,
            native_frame,
        );
    }

    let next_raw = tube.path_steps[usize::from(state.cursor)];
    let (dx, dy) = direction_delta(next_raw);
    let old_target = state.target;
    state.target.x = state.target.x.wrapping_add(dx.wrapping_mul(256));
    state.target.y = state.target.y.wrapping_add(dy.wrapping_mul(256));
    state.target.z = state.target.z.wrapping_add(z_step);
    let leftover = budget.wrapping_sub(distance);
    let Some(trig) = active_tube_trig() else {
        return true;
    };
    let next = advance_amount(old_target, state.target, leftover, next_raw, z_step, trig);
    if let Some(entity) = entities.get_mut(entity_id) {
        set_entity_world_coord(entity, next);
        entity.low_bridge_tube_state = Some(state);
    }
    true
}

fn active_tube_trig() -> Option<&'static TrigTable> {
    let trig = crate::map::retail_trig::global();
    if trig.is_none() {
        static WARNED: OnceLock<()> = OnceLock::new();
        if WARNED.set(()).is_ok() {
            log::warn!("explicit TubeMovement requires the verified retail sine table");
        }
    }
    trig
}

#[allow(clippy::too_many_arguments)]
fn finalize_tube_object(
    entities: &mut EntityStore,
    entity_id: u64,
    state: LowBridgeTubeMovementState,
    terrain: &ResolvedTerrainGrid,
    path_grid: Option<&PathGrid>,
    occupancy: &mut OccupancyGrid,
    cell_occupation: &mut CellOccupationGrid,
    raw_cell_occupation: &mut RawCellOccupationGrid,
    next_occupancy_enter_order: &mut EnterOrderCounter,
    rules: Option<&RuleSet>,
    interner: &StringInterner,
    rng: &mut SimRng,
    native_frame: u32,
) -> bool {
    let Some(tube) = terrain.tube(state.tube_id) else {
        return true;
    };
    let Some(entity) = entities.get(entity_id) else {
        return true;
    };
    let category = entity.category;
    let reached_cell = (entity.position.rx, entity.position.ry);

    let infantry_subcell = if category == EntityCategory::Infantry {
        let terrain_blocked =
            path_grid.is_some_and(|grid| !grid.is_walkable(reached_cell.0, reached_cell.1));
        let passable = !terrain_blocked
            && bump_crush::cell_passable_for_infantry(
                occupancy.get(reached_cell.0, reached_cell.1),
                MovementLayer::Ground,
            );
        if !passable {
            scatter_exit_blockers(
                entities,
                entity_id,
                reached_cell,
                path_grid,
                Some(terrain),
                occupancy,
                rules,
                interner,
                rng,
                crate::sim::movement::DestinationTiming::from_rules(native_frame, rules),
            );
            stop_blocked_mover(entities, entity_id);
            return true;
        }
        let (sub_x, sub_y) = entities
            .get(entity_id)
            .map(|entity| (entity.position.sub_x, entity.position.sub_y))
            .unwrap_or((CELL_CENTER_LEPTON, CELL_CENTER_LEPTON));
        bump_crush::allocate_sub_cell_with_preference(
            occupancy.get(reached_cell.0, reached_cell.1),
            MovementLayer::Ground,
            None,
            sub_x,
            sub_y,
            rng,
        )
    } else {
        None
    };

    if category == EntityCategory::Unit {
        let blockers: Vec<u64> = occupancy
            .get(reached_cell.0, reached_cell.1)
            .map(|cell| {
                cell.iter_layer(MovementLayer::Ground)
                    .map(|occupant| occupant.entity_id)
                    .collect()
            })
            .unwrap_or_default();
        if !blockers.is_empty() {
            scatter_exit_blockers(
                entities,
                entity_id,
                reached_cell,
                path_grid,
                Some(terrain),
                occupancy,
                rules,
                interner,
                rng,
                crate::sim::movement::DestinationTiming::from_rules(native_frame, rules),
            );
            stop_blocked_mover(entities, entity_id);
            return true;
        }
    }

    if category == EntityCategory::Infantry && infantry_subcell.is_none() {
        stop_blocked_mover(entities, entity_id);
        return true;
    }

    if let Some(entity) = entities.get_mut(entity_id) {
        if category == EntityCategory::Infantry {
            let sub_cell = infantry_subcell.expect("checked infantry exit sub-cell");
            entity.position.rx = reached_cell.0;
            entity.position.ry = reached_cell.1;
            entity.sub_cell = Some(sub_cell);
            (entity.position.sub_x, entity.position.sub_y) =
                lepton::subcell_lepton_offset(Some(sub_cell));
            let x = position_world_x(&entity.position);
            let y = position_world_y(&entity.position);
            if ground_height_at(terrain, x, y).is_some() {
                entity.position.z = terrain
                    .cell(reached_cell.0, reached_cell.1)
                    .map_or(entity.position.z, |cell| cell.level);
                entity.position.exact_z_leptons = None;
            }
        } else {
            let object = rules.and_then(|r| r.object(interner.resolve(entity.type_ref())));
            let speed = super::foot_speed::adjusted_speed(
                entity,
                object,
                rules.map_or(1.0, |r| r.general.veteran_speed),
            );
            let owner_current_speed =
                super::foot_speed::owner_current_speed_from_fraction(speed, SIM_ONE);
            entity.position.rx = tube.exit.0;
            entity.position.ry = tube.exit.1;
            entity.position.sub_x = CELL_CENTER_LEPTON;
            entity.position.sub_y = CELL_CENTER_LEPTON;
            entity.position.exact_z_leptons = Some(state.target.z);
            if let Some(cell) = terrain.cell(tube.exit.0, tube.exit.1) {
                entity.position.z = cell.level;
            }
            if let Some(drive) = entity.drive_locomotion.as_mut() {
                drive.turn.first_movement_allowed = true;
                drive.target_speed_fraction = SIM_ONE;
            }
            // Unit73604F writes the live Foot owner even if PerCell replaced
            // Drive. Exact post-callback timing remains part of the Process host.
            entity.foot_speed.applied_fraction = SIM_ONE;
            entity.foot_speed.cached_current_speed = owner_current_speed;
        }
        entity.low_bridge_tube_state = None;
    }
    put_after_tube(
        entities,
        entity_id,
        occupancy,
        cell_occupation,
        raw_cell_occupation,
        next_occupancy_enter_order,
    );

    if category == EntityCategory::Unit {
        update_unit_final_facing(entities, entity_id, terrain, native_frame);
    }
    true
}

fn put_after_tube(
    entities: &mut EntityStore,
    entity_id: u64,
    occupancy: &mut OccupancyGrid,
    cell_occupation: &mut CellOccupationGrid,
    raw_cell_occupation: &mut RawCellOccupationGrid,
    next_occupancy_enter_order: &mut EnterOrderCounter,
) {
    let Some(entity) = entities.get_mut(entity_id) else {
        return;
    };
    let rx = entity.position.rx;
    let ry = entity.position.ry;
    occupancy.add(
        rx,
        ry,
        entity_id,
        MovementLayer::Ground,
        (entity.category == EntityCategory::Infantry)
            .then_some(entity.sub_cell)
            .flatten(),
        CellListInsertion::from_category(entity.category),
    );
    match entity.category {
        EntityCategory::Unit => {
            raw_cell_occupation.mark_ground(rx, ry, VEHICLE_OCCUPATION_BIT);
            cell_occupation.mark_vehicle_on_layer(rx, ry, entity_id, MovementLayer::Ground);
            entity.foot_occupation_enabled = true;
        }
        EntityCategory::Infantry => {
            let mask = infantry_raw_occupation_mask(entity.position.sub_x, entity.position.sub_y);
            raw_cell_occupation.mark_ground(rx, ry, mask);
            // Native Infantry final explicitly calls the same raw mark again.
            raw_cell_occupation.mark_ground(rx, ry, mask);
        }
        _ => {}
    }
    entity.occupancy_enter_order = next_occupancy_enter_order.next();
    entity.lifecycle.cell_marked = true;
}

#[allow(clippy::too_many_arguments)]
fn scatter_exit_blockers(
    entities: &mut EntityStore,
    mover_id: u64,
    cell: (u16, u16),
    path_grid: Option<&PathGrid>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    occupancy: &OccupancyGrid,
    rules: Option<&RuleSet>,
    interner: &StringInterner,
    rng: &mut SimRng,
    timing: crate::sim::movement::DestinationTiming,
) {
    let blockers: Vec<u64> = occupancy
        .get(cell.0, cell.1)
        .map(|cell| {
            cell.iter_layer(MovementLayer::Ground)
                .map(|occupant| occupant.entity_id)
                .collect()
        })
        .unwrap_or_default();
    for blocker_id in blockers {
        if blocker_id == mover_id {
            continue;
        }
        let Some(blocker) = entities.get(blocker_id) else {
            continue;
        };
        if !matches!(
            blocker.category,
            EntityCategory::Unit | EntityCategory::Infantry
        ) || blocker.locomotor.is_none()
            || locomotor_is_moving(blocker)
        {
            continue;
        }
        bump_crush::scatter_blocker(
            entities,
            blocker_id,
            path_grid,
            resolved_terrain,
            occupancy,
            MovementLayer::Ground,
            rng,
            rules,
            interner,
            timing,
        );
    }
}

fn locomotor_is_moving(entity: &GameEntity) -> bool {
    if entity.locomotor.as_ref().is_some_and(|loco| {
        loco.active_kind() == crate::rules::locomotor_type::LocomotorKind::Drive
    }) {
        super::drive_locomotion::drive_locomotor_is_moving(entity)
    } else {
        entity.movement_target.is_some()
            || crate::sim::movement::track_head::committed_track_head(entity).is_some()
    }
}

fn stop_blocked_mover(entities: &mut EntityStore, entity_id: u64) {
    if let Some(entity) = entities.get_mut(entity_id) {
        if let Some(target) = entity.movement_target.as_mut() {
            target.current_speed = SIM_ZERO;
        }
        if let Some(drive) = entity.drive_locomotion.as_mut() {
            drive.target_speed_fraction = SIM_ZERO;
        }
        // Unit735F6A / Infantry51B8FC apply zero on the live Foot owner.
        entity.foot_speed.applied_fraction = SIM_ZERO;
        entity.foot_speed.cached_current_speed = 0;
    }
}

fn update_unit_final_facing(
    entities: &mut EntityStore,
    entity_id: u64,
    terrain: &ResolvedTerrainGrid,
    native_frame: u32,
) {
    let Some(entity) = entities.get_mut(entity_id) else {
        return;
    };
    let Some(tube) = terrain
        .cell(entity.position.rx, entity.position.ry)
        .and_then(|cell| cell.tube_index)
        .and_then(|tube_id| terrain.tube(tube_id))
    else {
        return;
    };
    let q32 = tube.direction.wrapping_shl(13).wrapping_sub(0x6001) & 0xffff_e000_u32 as i32;
    let q16 = q32 as u16;
    entity.facing = (q16 >> 8) as u8;
    entity.facing_target = None;
    if let Some(body) = entity.body_facing.as_mut() {
        body.snap(q16, native_frame);
    }
}

fn native_type_speed(raw_speed: i32) -> i32 {
    raw_speed
        .max(0)
        .wrapping_mul(256)
        .wrapping_div(100)
        .min(255)
}

fn live_z_step(terrain: &ResolvedTerrainGrid, tube: &TubeFact) -> Option<i32> {
    let count = i32::try_from(tube.path_len()).ok()?;
    if count == 0 {
        return None;
    }
    let entry = ground_height_at_cell_center(terrain, tube.entry)?;
    let exit = ground_height_at_cell_center(terrain, tube.exit)?;
    Some(exit.wrapping_sub(entry) / count)
}

fn ground_height_at_cell_center(terrain: &ResolvedTerrainGrid, cell: (u16, u16)) -> Option<i32> {
    ground_height_at(
        terrain,
        i32::from(cell.0).wrapping_mul(256).wrapping_add(128),
        i32::from(cell.1).wrapping_mul(256).wrapping_add(128),
    )
}

fn ground_height_at(terrain: &ResolvedTerrainGrid, x: i32, y: i32) -> Option<i32> {
    let rx = u16::try_from(x.div_euclid(256)).ok()?;
    let ry = u16::try_from(y.div_euclid(256)).ok()?;
    let cell = terrain.cell(rx, ry)?;
    lepton::ground_height_leptons(cell.level, cell.slope_type, x, y).ok()
}

fn entity_world_coord(entity: &GameEntity, terrain: &ResolvedTerrainGrid) -> Option<DriveCoord> {
    let x = position_world_x(&entity.position);
    let y = position_world_y(&entity.position);
    Some(DriveCoord {
        x,
        y,
        z: entity
            .position
            .exact_z_leptons
            .or_else(|| ground_height_at(terrain, x, y))?,
    })
}

fn set_entity_world_coord(entity: &mut GameEntity, coord: DriveCoord) {
    let rx = coord.x.div_euclid(256);
    let ry = coord.y.div_euclid(256);
    if let (Ok(rx), Ok(ry)) = (u16::try_from(rx), u16::try_from(ry)) {
        entity.position.rx = rx;
        entity.position.ry = ry;
        entity.position.sub_x =
            crate::util::fixed_math::SimFixed::from_num(coord.x.rem_euclid(256));
        entity.position.sub_y =
            crate::util::fixed_math::SimFixed::from_num(coord.y.rem_euclid(256));
        entity.position.exact_z_leptons = Some(coord.z);
    }
}

fn position_world_x(position: &Position) -> i32 {
    i32::from(position.rx)
        .wrapping_mul(256)
        .wrapping_add(position.sub_x.to_num::<i32>())
}

fn position_world_y(position: &Position) -> i32 {
    i32::from(position.ry)
        .wrapping_mul(256)
        .wrapping_add(position.sub_y.to_num::<i32>())
}

fn direction_delta(raw: i32) -> (i32, i32) {
    crate::util::direction::DIRECTION_DELTAS[(raw & 7) as usize]
}

fn native_distance_leptons(current: DriveCoord, target: DriveCoord) -> i32 {
    let dx = X87Chop53::load_i32(target.x.wrapping_sub(current.x));
    let dy = X87Chop53::load_i32(target.y.wrapping_sub(current.y));
    let dz = X87Chop53::load_i32(target.z.wrapping_sub(current.z));
    let squared = X87Chop53::add(
        X87Chop53::add(X87Chop53::mul(dx, dx), X87Chop53::mul(dy, dy)),
        X87Chop53::mul(dz, dz),
    );
    let root_bits = sqrt_approx_f32(squared).expect("tube coordinate distance stays finite");
    let root = X87Chop53::load_f32(root_bits).expect("Sqrt_Approx returns finite output");
    X87Chop53::ftol_i64(root).expect("tube distance fits i32") as i32
}

fn advance_amount(
    base: DriveCoord,
    target: DriveCoord,
    amount: i32,
    raw_step: i32,
    z_step: i32,
    trig: &TrigTable,
) -> DriveCoord {
    let facing = crate::util::direction_tables::facing16_from_delta(
        target.x.wrapping_sub(base.x),
        target.y.wrapping_sub(base.y),
    );
    let angle_scale = f64::from_bits(0xbf19_222d_989f_5e57);
    let angle = f64::from(i32::from(facing as i16).wrapping_sub(0x3fff)) * angle_scale;
    let cosine = trig.cos_radians(angle);
    let sine = trig.sin_radians(angle);
    let amount_x87 = X87Chop53::load_i32(amount);
    let x = ftol_add_f32_product(base.x, cosine, amount_x87);
    let y = ftol_add_f32_product(base.y, -sine, amount_x87);

    let nominal_bits = if raw_step & 1 == 0 {
        256.0_f64.to_bits()
    } else {
        0x4076_a09e_667f_3bcd
    };
    let nominal = X87Chop53::load_f64(NativeF64Bits::from_bits(nominal_bits))
        .expect("nominal tube segment length is finite");
    let ratio = X87Chop53::div(amount_x87, nominal).expect("nominal length is nonzero");
    let z_delta = X87Chop53::mul(ratio, X87Chop53::load_i32(z_step));
    let z = X87Chop53::ftol_i64(X87Chop53::add(X87Chop53::load_i32(base.z), z_delta))
        .expect("tube Z fits i32") as i32;
    DriveCoord { x, y, z }
}

fn ftol_add_f32_product(base: i32, factor: f32, amount: crate::util::native_x87::X87Value) -> i32 {
    let factor = X87Chop53::load_f32(NativeF32Bits::from_bits(factor.to_bits()))
        .expect("retail trig entry is finite");
    X87Chop53::ftol_i64(X87Chop53::add(
        X87Chop53::load_i32(base),
        X87Chop53::mul(factor, amount),
    ))
    .expect("tube XY fits i32") as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
    use crate::map::tube_facts::TubeSource;
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
    use crate::sim::components::{DriveLocomotionRuntime, Health};
    use crate::sim::game_entity::GameEntity;
    use crate::sim::occupancy::CellListInsertion;

    fn flat_cell(rx: u16, ry: u16, tube_index: Option<TubeId>) -> ResolvedTerrainCell {
        let speed_costs = SpeedCostProfile::default();
        ResolvedTerrainCell {
            rx,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: true,
            tileset_index: None,
            land_type: 0,
            yr_cell_land_type: 0,
            slope_type: 0,
            template_height: 0,
            height_in_pixels: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Clear,
            speed_costs,
            is_water: false,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
            accepts_smudge: true,
            allows_tiberium: false,
            variant: 0,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: false,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: zone_class::GROUND,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: 0,
            base_yr_cell_land_type: 0,
            base_terrain_class: TerrainClass::Clear,
            base_speed_costs: speed_costs,
            build_blocked: false,
            has_bridge_deck: false,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: None,
            bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
            tube_index,
            radar_left: [0; 3],
            radar_right: [0; 3],
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        }
    }

    fn explicit_terrain(path: Vec<i32>) -> ResolvedTerrainGrid {
        let mut cells = Vec::new();
        for x in 0..=3 {
            cells.push(flat_cell(x, 0, (x == 0).then_some(TubeId(0))));
        }
        ResolvedTerrainGrid::from_cells_with_tubes(
            4,
            1,
            cells,
            vec![TubeFact {
                entry: (0, 0),
                exit: (path.len() as u16, 0),
                direction: 2,
                path_steps: path,
                source: TubeSource::ExplicitMap,
            }],
        )
    }

    fn unit(id: u64) -> GameEntity {
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            id,
            0,
            0,
            0,
            0,
            crate::sim::intern::test_intern("Americans"),
            Health { current: 100 },
            crate::sim::intern::test_intern("MTNK"),
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        entity.lifecycle.in_limbo = false;
        entity.lifecycle.cell_marked = true;
        entity.drive_locomotion = Some(DriveLocomotionRuntime::default());
        entity
    }

    #[test]
    fn gsi_04_15_zero_step_auto_shell_is_never_a_path_producer() {
        let terrain = ResolvedTerrainGrid::from_cells_with_tubes(
            1,
            1,
            vec![flat_cell(0, 0, Some(TubeId(0)))],
            vec![TubeFact::auto_low_bridge((0, 0), 2)],
        );
        let mut target = MovementTarget::default();
        target.path = vec![(0, 0), (2, 0)];
        target.next_index = 1;
        let entity = unit(1);
        assert_eq!(
            pending_path_tube_id(
                &target,
                &entity.position,
                MovementLayer::Ground,
                Some(&terrain)
            ),
            None
        );
    }

    /// Supplied post-Tube state exercises the real finalizer's owner writes.
    /// It does not emulate the preceding native PerCell callback or its timing.
    #[test]
    fn tube_finalizer_updates_live_owner_speed_without_drive_payload() {
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::movement::locomotor::LocomotorState;
        use crate::util::fixed_math::SimFixed;
        for (category, blocked) in [
            (EntityCategory::Unit, false),
            (EntityCategory::Unit, true),
            (EntityCategory::Infantry, true),
        ] {
            let terrain = explicit_terrain(vec![2, 2]);
            let mut entity = unit(1);
            entity.category = category;
            entity.drive_locomotion = None;
            entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Teleport));
            entity.lifecycle.cell_marked = false;
            entity.position.rx = 2;
            entity.foot_speed.applied_fraction = SimFixed::lit("0.625");
            entity.foot_speed.cached_current_speed = 6;
            entity.movement_target = Some(MovementTarget {
                speed: SimFixed::from_num(150),
                current_speed: SimFixed::from_num(100),
                ..Default::default()
            });
            let state = LowBridgeTubeMovementState {
                tube_id: TubeId(0),
                cursor: 2,
                target: DriveCoord::cell(2, 0, 0),
            };
            entity.low_bridge_tube_state = Some(state);
            let mut entities = EntityStore::new();
            entities.insert(entity);
            let mut occupancy = OccupancyGrid::new();
            if blocked && category == EntityCategory::Unit {
                let mut blocker = unit(2);
                blocker.position.rx = 2;
                entities.insert(blocker);
                occupancy.add(
                    2,
                    0,
                    2,
                    MovementLayer::Ground,
                    None,
                    CellListInsertion::PrependNonBuilding,
                );
            }
            let grid = PathGrid::test_all_blocked(4, 1);
            assert!(finalize_tube_object(
                &mut entities,
                1,
                state,
                &terrain,
                (category == EntityCategory::Infantry).then_some(&grid),
                &mut occupancy,
                &mut CellOccupationGrid::new(),
                &mut RawCellOccupationGrid::new(),
                &mut EnterOrderCounter::new(),
                None,
                &StringInterner::default(),
                &mut SimRng::new(7),
                21,
            ));
            let owner = entities.get(1).unwrap();
            assert!(owner.drive_locomotion.is_none());
            assert_eq!(
                owner.locomotor.as_ref().unwrap().active_kind(),
                LocomotorKind::Teleport
            );
            assert_eq!(
                owner.foot_speed.applied_fraction,
                if blocked { SIM_ZERO } else { SIM_ONE }
            );
            assert_eq!(
                owner.foot_speed.cached_current_speed,
                if blocked { 0 } else { 10 }
            );
            assert_eq!(owner.low_bridge_tube_state.is_some(), blocked);
            if blocked {
                assert_eq!(
                    owner.movement_target.as_ref().unwrap().current_speed,
                    SIM_ZERO
                );
            } else {
                assert!(owner.lifecycle.cell_marked);
                assert_eq!(occupancy.count_on_layer(2, 0, MovementLayer::Ground), 1);
            }
        }
    }

    #[test]
    fn gsi_04_15_tube_admission_requires_ground_object_list_layer() {
        let terrain = explicit_terrain(vec![2, 2]);
        let entity = unit(1);
        let mut target = MovementTarget {
            path: vec![(0, 0), (2, 0)],
            next_index: 1,
            ..MovementTarget::default()
        };

        assert_eq!(
            pending_path_tube_id(
                &target,
                &entity.position,
                MovementLayer::Bridge,
                Some(&terrain)
            ),
            None
        );
        assert_eq!(
            pending_path_tube_id(
                &target,
                &entity.position,
                MovementLayer::Ground,
                Some(&terrain)
            ),
            Some(TubeId(0))
        );

        target.bypass_grid = true;
        assert_eq!(
            pending_path_tube_id(
                &target,
                &entity.position,
                MovementLayer::Ground,
                Some(&terrain)
            ),
            None
        );
    }

    #[test]
    fn gsi_04_15_begin_detaches_ground_list_owner_mark_and_raw_byte() {
        let terrain = explicit_terrain(vec![2, 2]);
        let mut entity = unit(1);
        let destination = DriveCoord {
            x: 901,
            y: -17,
            z: 731,
        };
        let retained = crate::sim::components::TrackProgress {
            turn_index: -1,
            cursor: -1,
            reversed: true,
            residual: 6,
        };
        let drive = entity.drive_locomotion.as_mut().unwrap();
        drive.destination = Some(destination);
        drive.head_to = Some(DriveCoord {
            x: 43,
            y: 81,
            z: 512,
        });
        drive.track = retained;
        entity.navigation.path_replay = crate::sim::components::FootPathQueue {
            directions: vec![8, 2],
            cursor: 0,
            reference_cell: Some((-3, 7)),
        };
        entity.movement_target = Some(MovementTarget {
            path: vec![(0, 0), (2, 0), (3, 0)],
            next_index: 1,
            ..MovementTarget::default()
        });
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            0,
            0,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let mut cell_occupation = CellOccupationGrid::new();
        cell_occupation.mark_vehicle_on_layer(0, 0, 1, MovementLayer::Ground);
        let mut raw = RawCellOccupationGrid::new();
        raw.mark_ground(0, 0, VEHICLE_OCCUPATION_BIT);

        begin_path_tube_step(
            &mut true,
            &mut entity.navigation.path_replay,
            entity.stable_id,
            entity.category,
            &mut entity.position,
            &mut entity.drive_locomotion,
            &mut entity.low_bridge_tube_state,
            entity.movement_target.as_mut().unwrap(),
            &mut entity.lifecycle.cell_marked,
            TubeId(0),
            &terrain,
            &mut occupancy,
            &mut cell_occupation,
            &mut raw,
        )
        .unwrap();

        assert!(!entity.lifecycle.cell_marked);
        assert_eq!(entity.movement_target.as_ref().unwrap().next_index, 2);
        assert_eq!(occupancy.count_on_layer(0, 0, MovementLayer::Ground), 0);
        assert_eq!(raw.ground_bits(0, 0), 0);
        assert_eq!(cell_occupation.vehicle_bits(0, 0, MovementLayer::Ground), 0);
        assert_eq!(entity.low_bridge_tube_state.unwrap().target.x, 384);
        let drive = entity.drive_locomotion.as_ref().unwrap();
        assert_eq!(drive.destination, Some(destination));
        assert_eq!(
            drive.head_to,
            Some(DriveCoord {
                x: 640,
                y: 128,
                z: 0
            })
        );
        assert!(drive.track_valid);
        assert_eq!(drive.track, retained);
        assert_eq!(entity.navigation.path_replay.cursor, 1);
        assert_eq!(entity.navigation.path_replay.reference_cell, Some((-3, 7)));
    }

    #[test]
    fn gsi_04_15_cardinal_partial_uses_full_3d_distance_but_nominal_z_denominator() {
        let trig = TrigTable::synthetic();
        let current = DriveCoord { x: 0, y: 0, z: 0 };
        let target = DriveCoord {
            x: 256,
            y: 0,
            z: 100,
        };
        assert!(native_distance_leptons(current, target) > 256);
        assert_eq!(
            advance_amount(current, target, 100, 2, 100, &trig),
            DriveCoord {
                x: 100,
                y: 0,
                z: 39
            }
        );
    }

    #[test]
    fn gsi_04_15_type_speed_budget_uses_native_scaled_field() {
        assert_eq!(native_type_speed(-1), 0);
        assert_eq!(native_type_speed(4), 10);
        assert_eq!(native_type_speed(100), 255);
        assert_eq!(native_type_speed(200), 255);
        assert_eq!(native_type_speed(11) * 3 / 2, 42);
    }

    #[test]
    fn gsi_04_15_unit_final_facing_uses_final_cells_raw_tube_direction() {
        let terrain = explicit_terrain(vec![2, 2]);
        let mut entities = EntityStore::new();
        let mut entity = unit(1);
        entity.position.rx = 2;
        entity.low_bridge_tube_state = Some(LowBridgeTubeMovementState {
            tube_id: TubeId(0),
            cursor: 2,
            target: DriveCoord {
                x: 640,
                y: 128,
                z: 0,
            },
        });
        entities.insert(entity);
        update_unit_final_facing(&mut entities, 1, &terrain, 0);
        // Exit has no tube index in this fixture: facing is preserved.
        assert_eq!(entities.get(1).unwrap().facing, 0);
    }
}
