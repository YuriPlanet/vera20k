//! Move command issuing — A* pathfinding and MovementTarget attachment.
//!
//! Entry points for issuing move commands to entities. These are called from
//! `world_commands.rs`, `miner_system.rs`, and `production_queue.rs` — not
//! from the per-tick movement loop.
//!
//! ## Dependency rules
//! - Internal to sim/movement — called via re-exports in mod.rs.

use std::collections::BTreeSet;

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::GeneralRules;
use crate::sim::components::MovementTarget;
use crate::sim::entity_store::EntityStore;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::terrain_cost::TerrainCostGrid;
use crate::sim::pathfinding::zone_map::ZoneGrid;
use crate::sim::pathfinding::{BlockerNeighborCounts, LayeredEntityBlockMap};
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

use super::movement_path::{
    find_move_path, merge_path_blocks, resolve_reachable_move_goal, resolve_requested_move_goal,
    supports_layered_bridge_pathing,
};
use super::{PathfindingContext, facing_from_delta};
use crate::rules::locomotor_type::MovementZone;
use crate::sim::components::OrderIntent;
use crate::sim::game_entity::GameEntity;

use super::teleport_movement;

/// Check if an entity can accept a new movement destination.
///
/// Prevents destination changes during special states: dying, deploying,
/// undeploying, falling, and unloading passengers.
pub(crate) fn can_accept_destination(entity: &GameEntity) -> bool {
    if entity.dying {
        return false;
    }
    if entity.building_up.is_some() || entity.building_down.is_some() {
        return false;
    }
    if matches!(entity.order_intent, Some(OrderIntent::Unloading)) {
        return false;
    }
    true
}

/// Clear owner navigation and queued endpoint state through the native-shaped
/// null-destination path.
pub fn clear_navigation_for_entity(entity: &mut GameEntity) {
    super::navcom::set_destination_internal_null(entity);
    entity.navigation.nav_queue.clear();
}

/// A committed head comes from the active locomotor after world callbacks;
/// the physical path and raw occupation metadata cannot reconstruct it.
fn committed_movement_head(entity: &GameEntity) -> Option<(u16, u16)> {
    let head = if entity.locomotor.as_ref()?.kind == LocomotorKind::Walk {
        entity.locomotor.as_ref()?.step_head()?
    } else {
        super::track_head::committed_track_head(entity)?
    };
    Some(((head.x / 256) as u16, (head.y / 256) as u16))
}

/// Clear a destination while preserving only an already committed Drive/Ship
/// segment. Shared by Stop and the MCV EventClass deploy handoff.
pub fn stop_navigation_at_committed_head(e: &mut GameEntity) {
    let committed_head = committed_path_head(e);
    clear_navigation_for_entity(e);
    // Chain selection consumes the remaining native direction queue, which is
    // independent of the physical A* cursor. Stop must retire that abandoned
    // suffix as well as truncate MovementTarget below. Keep the committed
    // retained selector/head and replay reference until the segment finishes.
    super::path_markers::exhaust_path_replay(&mut e.navigation.path_replay);
    retain_path_to_head(e, committed_head);
}

fn committed_path_head(e: &GameEntity) -> Option<((u16, u16), super::locomotor::MovementLayer)> {
    committed_movement_head(e).map(|head_cell| {
        let layer = e
            .movement_target
            .as_ref()
            .and_then(|target| {
                target
                    .path
                    .iter()
                    .position(|&cell| cell == head_cell)
                    .map(|index| target.layer_at(index))
            })
            .unwrap_or_else(|| e.movement_layer_or_ground());
        (head_cell, layer)
    })
}

/// Keep only physical movement already accepted by the locomotor. The caller
/// owns destination/queue writes: explicit Attack's null setter does not clear
/// NavQueue, whereas the pre-existing Stop command path does.
pub(crate) fn retain_committed_movement(e: &mut GameEntity) {
    let head = committed_path_head(e);
    retain_path_to_head(e, head);
}

fn retain_path_to_head(
    e: &mut GameEntity,
    committed_head: Option<((u16, u16), super::locomotor::MovementLayer)>,
) {
    let current_cell = (e.position.rx, e.position.ry);
    let current_layer = e.movement_layer_or_ground();
    let walk_head = e.locomotor.as_ref().and_then(|l| {
        (l.kind == LocomotorKind::Walk)
            .then(|| l.step_head())
            .flatten()
    });
    let committed_walk = walk_head.is_some();
    // Stop clears the owner destination immediately, but an
    // already committed Drive/Ship curve keeps only the
    // current-to-head step. Removing every trailing A* entry
    // prevents chaining or segment repath toward the abandoned
    // owner goal.
    if let (Some((head_cell, head_layer)), Some(target)) =
        (committed_head, e.movement_target.as_mut())
    {
        if current_cell == head_cell {
            target.path = vec![head_cell];
            target.path_layers = vec![head_layer];
            // Walk retirement is subcell-head completion, not cell equality.
            target.next_index = usize::from(!committed_walk);
            target.move_dir_x = SIM_ZERO;
            target.move_dir_y = SIM_ZERO;
            target.move_dir_len = SIM_ZERO;
        } else {
            target.path = vec![current_cell, head_cell];
            target.path_layers = vec![current_layer, head_layer];
            target.next_index = 1;
            let (dir_x, dir_y, dir_len) = crate::util::lepton::cell_delta_to_lepton_dir(
                i32::from(head_cell.0) - i32::from(current_cell.0),
                i32::from(head_cell.1) - i32::from(current_cell.1),
            );
            target.move_dir_x = dir_x;
            target.move_dir_y = dir_y;
            target.move_dir_len = dir_len;
        }
        target.final_goal = Some(head_cell);
        if let Some(head) = walk_head {
            // Walk75BD29 samples current XYZ against the retained subcell
            // head. A command must not redirect it to the cell center.
            let current = super::ground_pose::position_world_coord(&e.position);
            let dx = SimFixed::from_num(head.x.wrapping_sub(current.x));
            let dy = SimFixed::from_num(head.y.wrapping_sub(current.y));
            target.move_dir_x = dx;
            target.move_dir_y = dy;
            target.move_dir_len = crate::util::fixed_math::fixed_distance(dx, dy);
        }
    } else {
        e.movement_target = None;
    }
}

/// The caller's native frame and configured Foot blocked timer duration.
/// Passing this context keeps accepted destination timers anchored even when
/// Scatter and the ordinary object turn both process one actor in a frame.
#[derive(Debug, Clone, Copy)]
pub struct DestinationTiming {
    pub binary_frame: u32,
    pub blockage_path_delay_ticks: i32,
}

impl DestinationTiming {
    pub const fn new(binary_frame: u32, blockage_path_delay_ticks: i32) -> Self {
        Self {
            binary_frame,
            blockage_path_delay_ticks,
        }
    }

    pub fn from_rules(binary_frame: u32, rules: Option<&crate::rules::ruleset::RuleSet>) -> Self {
        Self::new(
            binary_frame,
            rules.map_or(60, |r| r.general.blockage_path_delay_ticks),
        )
    }

    /// Set_Destination_Internal 0x004D94B0 tail 0x4D96C2..0x4D9707: +6B7 = 0,
    /// +668 = (frame, Rules+1768), +640 = (frame, 0) for every accepted
    /// setter. The setter never writes +64C; the no-head Process FindPath
    /// success continuation 0x75B2E2 owns that reset.
    pub(crate) fn accept(self, entity: &mut crate::sim::game_entity::GameEntity) {
        let path = &mut entity.navigation.path_runtime;
        path.start_movement(self.binary_frame, 0);
        path.start_blocked(self.binary_frame, self.blockage_path_delay_ticks);
        path.path_blocked = false;
    }
}

/// Issue a move command and attach its MovementTarget execution request.
///
/// Walk, Drive and Ship destinations defer the path search to their first
/// Process (the class setters never search). Other locomotors retain their
/// command-time path adapter.
///
/// `speed` is the movement speed in cells per second (from rules.ini Speed= value).
pub fn issue_move_command(
    entities: &mut EntityStore,
    grid: &PathGrid,
    entity_id: u64,
    target: (u16, u16),
    speed: SimFixed,
    queue: bool,
    terrain_costs: Option<&TerrainCostGrid>,
    entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    timing: crate::sim::movement::DestinationTiming,
) -> bool {
    issue_move_command_with_layered(
        entities,
        grid,
        entity_id,
        target,
        speed,
        queue,
        terrain_costs,
        entity_blocks,
        None, // resolved_terrain — per-tick repath has it
        None, // zone_grid — basic entrypoint has no Simulation context
        entity_block_map,
        None,
        None,
        None,
        timing,
    )
}

/// Gamemd-shaped Set_Destination bridge for Teleporter units.
///
/// `LocomotorState.kind` remains the active locomotor. If the target cell is a
/// building cell, a Teleport-primary unit activates Drive piggyback and receives
/// a normal ground movement target. If the target cell is empty and active
/// Teleport is available, Teleport receives Head_To_Coord and starts the warp.
#[allow(clippy::too_many_arguments)]
pub fn set_destination_for_teleporter_entity(
    entities: &mut EntityStore,
    grid: Option<&PathGrid>,
    entity_id: u64,
    target: (u16, u16),
    speed: SimFixed,
    queue: bool,
    terrain_costs: Option<&TerrainCostGrid>,
    entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    zone_grid: Option<&ZoneGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    rules: &GeneralRules,
    is_harvester: bool,
    is_teleporter: bool,
    destination_has_building: bool,
    playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
    binary_frame: u32,
) -> bool {
    let Some(entity) = entities.get(entity_id) else {
        return false;
    };
    if !can_accept_destination(entity) {
        return false;
    }
    let has_teleport_locomotor = entity.locomotor.as_ref().is_some_and(|loco| {
        loco.effective_kind() == LocomotorKind::Teleport
            || loco.active_kind() == LocomotorKind::Teleport
    });
    if !is_teleporter || !has_teleport_locomotor {
        let Some(grid) = grid else {
            return false;
        };
        return issue_move_command_with_layered(
            entities,
            grid,
            entity_id,
            target,
            speed,
            queue,
            terrain_costs,
            entity_blocks,
            resolved_terrain,
            zone_grid,
            entity_block_map,
            None,
            playfield_bounds,
            None,
            crate::sim::movement::DestinationTiming::new(
                binary_frame,
                rules.blockage_path_delay_ticks,
            ),
        );
    }

    if destination_has_building {
        let Some(grid) = grid else {
            return false;
        };
        if let Some(entity) = entities.get_mut(entity_id) {
            super::locomotor_owner::begin_drive_for_teleporter(entity, binary_frame);
        }
        return issue_move_command_with_layered(
            entities,
            grid,
            entity_id,
            target,
            speed,
            queue,
            terrain_costs,
            entity_blocks,
            resolved_terrain,
            zone_grid,
            entity_block_map,
            None,
            playfield_bounds,
            None,
            crate::sim::movement::DestinationTiming::new(
                binary_frame,
                rules.blockage_path_delay_ticks,
            ),
        );
    }

    if let Some(entity) = entities.get_mut(entity_id) {
        let should_restore = entity.locomotor.as_ref().is_some_and(|loco| {
            loco.effective_kind() == LocomotorKind::Teleport
                && loco.active_kind() != LocomotorKind::Teleport
        });
        // `TechnoClass::Set_Destination` @ `0x00741970` unwinds through the
        // same gated protocol `FootClass::AI` uses — `Is_Ok_To_End` (`+0x14`)
        // first, transfer only when it returns true — at `0x00742587` and
        // `0x00742681`. Its third END, `0x00742A7C`, and the war-factory-exit
        // fragment at `0x0044E014` are gated on `Is_Piggybacking` (`+0x1C`)
        // alone, so "no ungated END" would be too strong; every native END is
        // nevertheless part of a *swap*. Here the gated form is the right one:
        // a Chrono Miner still driving keeps Drive installed and the per-tick
        // restore picks it up on the frame the drive actually stops.
        if should_restore {
            super::locomotor_owner::try_restore_primary(entity);
        }
    }

    teleport_movement::issue_active_teleport_head_to_coord(
        entities,
        entity_id,
        target,
        rules,
        is_harvester,
    )
}

/// Issue a direct move to a single cell without A* pathfinding.
///
/// Used for scripted movement into/out of building footprints where the target
/// cell is not pathfindable (e.g. refinery pad inside the foundation). Creates
/// a 2-cell `MovementTarget` `[start, target]` with a Euclidean direction
/// vector that handles multi-cell deltas correctly. Each step bypasses A*;
/// callers that also need to bypass `path_grid` walkability (e.g. foundation
/// traversal) should set `bypass_grid = true` on the resulting `MovementTarget`.
///
/// Returns `true` if the entity was found and the move was issued.
pub fn issue_direct_move(
    entities: &mut EntityStore,
    entity_id: u64,
    target: (u16, u16),
    speed: SimFixed,
    timing: crate::sim::movement::DestinationTiming,
) -> bool {
    let Some(entity) = entities.get(entity_id) else {
        return false;
    };
    if !can_accept_destination(entity) {
        return false;
    }
    let start = (entity.position.rx, entity.position.ry);
    if start == target {
        timing.accept(entities.get_mut(entity_id).expect("accepted mover"));
        return true;
    }
    let current_layer = entity.movement_layer_or_ground();

    let dx = target.0 as i32 - start.0 as i32;
    let dy = target.1 as i32 - start.1 as i32;
    let new_facing = facing_from_delta(dx, dy);
    // Compute direction vector with EUCLIDEAN length so multi-cell deltas
    // (e.g. pad→exit_cell may be (-2, +1)) advance at the correct speed.
    // `cell_delta_to_lepton_dir` only handles unit deltas — for multi-cell
    // deltas its length is wrong, causing the dual-axis crossing check in
    // movement_step to never satisfy.
    let dir_x: SimFixed = SimFixed::from_num(dx * 256);
    let dir_y: SimFixed = SimFixed::from_num(dy * 256);
    let dir_len: SimFixed = crate::util::fixed_math::fixed_distance(dir_x, dir_y);

    let movement = MovementTarget {
        path: vec![start, target],
        path_layers: vec![current_layer, current_layer],
        next_index: 1,
        speed,
        current_speed: speed,
        move_dir_x: dir_x,
        move_dir_y: dir_y,
        move_dir_len: dir_len,
        ignore_terrain_cost: true,
        adapter_route: true,
        ..Default::default()
    };

    if let Some(entity_mut) = entities.get_mut(entity_id) {
        // Direct callers share Foot4D96C2's accepted destination tail.
        timing.accept(entity_mut);
        entity_mut.movement_target = Some(movement);
        let has_rot = entity_mut.locomotor.as_ref().is_some_and(|l| l.rot > 0);
        if entity_mut.category != EntityCategory::Infantry && has_rot {
            entity_mut.facing_target = Some(new_facing);
        } else {
            entity_mut.facing = new_facing;
        }
    }
    true
}

pub(crate) fn issue_move_command_with_layered(
    entities: &mut EntityStore,
    grid: &PathGrid,
    entity_id: u64,
    target: (u16, u16),
    speed: SimFixed,
    queue: bool,
    terrain_costs: Option<&TerrainCostGrid>,
    entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    zone_grid: Option<&ZoneGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    blocker_neighbor_counts: Option<&BlockerNeighborCounts>,
    playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
    cell_occupation: Option<&mut crate::sim::occupancy::CellOccupationGrid>,
    timing: crate::sim::movement::DestinationTiming,
) -> bool {
    issue_move_command_with_destination(
        entities,
        grid,
        entity_id,
        target,
        speed,
        queue,
        terrain_costs,
        entity_blocks,
        resolved_terrain,
        zone_grid,
        entity_block_map,
        blocker_neighbor_counts,
        playfield_bounds,
        cell_occupation,
        None,
        timing,
    )
}

/// An object order supplies its captured coordinate independently of the A*
/// approach endpoint. Foot4D9510 / Walk75ACB0 own this accepted destination.
pub(crate) fn issue_move_command_with_destination(
    entities: &mut EntityStore,
    grid: &PathGrid,
    entity_id: u64,
    target: (u16, u16),
    speed: SimFixed,
    queue: bool,
    terrain_costs: Option<&TerrainCostGrid>,
    entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    zone_grid: Option<&ZoneGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    blocker_neighbor_counts: Option<&BlockerNeighborCounts>,
    playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
    cell_occupation: Option<&mut crate::sim::occupancy::CellOccupationGrid>,
    object_destination: Option<(
        crate::sim::components::NavTargetRef,
        crate::sim::components::DriveCoord,
    )>,
    timing: crate::sim::movement::DestinationTiming,
) -> bool {
    // Read the entity's current position and locomotor state.
    let Some(entity) = entities.get(entity_id) else {
        log::warn!("issue_move_command: entity {} not found", entity_id);
        return false;
    };
    if !can_accept_destination(entity) {
        return false;
    }
    if entity.locomotor.as_ref().is_some_and(|loco| {
        matches!(
            loco.active_kind(),
            LocomotorKind::Drive | LocomotorKind::Ship
        )
    }) {
        prepare_track_destination(
            entities.get_mut(entity_id).expect("resolved mover"),
            target,
            object_destination,
            speed,
            resolved_terrain,
            timing,
        );
        return true;
    }
    // The original engine dispatches its cell-entry predicate by object class,
    // so terrain-object occupation is read at sub-cell granularity for infantry
    // and whole-cell for everything else, and reads the crusher flags from the
    // mover's own type. urgency=0: an initial move command.
    let path_facts = super::MoverPathFacts::from_entity_without_wall_arm(entity, 0);
    // `AStar @ 0x0042CAD6` uses hierarchy only for a mover whose stored
    // TechnoClass+0x3D5 byte is true. Authority is explicit: resolved terrain
    // and MapClass bounds have independent lifetimes in headless fixtures and
    // during staged startup, so neither can stand in for the other.
    let allow_zone_hierarchy = playfield_bounds.is_none() || entity.in_playfield;
    let locomotor_kind = entity.locomotor.as_ref().map(|locomotor| locomotor.kind);
    // A new destination never rewinds a curve already in flight.
    // `TechnoClass::Set_Destination` @ `0x00741970` only records the target —
    // NavCom in `FootClass::Set_Destination_Internal` @ `0x004D94B0`, the
    // coordinate in Drive `Head_To_Coord` @ `0x004AFD40` — and never touches
    // the Drive track cursor, so the new path takes effect at the curve's next
    // node. Keep its retained selector, cursor and head; anchor the new path
    // at that committed head cell.
    let current_cell = (entity.position.rx, entity.position.ry);
    let committed_walk = locomotor_kind == Some(LocomotorKind::Walk)
        && entity
            .locomotor
            .as_ref()
            .is_some_and(|l| l.step_head().is_some());
    let in_flight_curve_head = committed_movement_head(entity);
    let keep_in_flight_curve = in_flight_curve_head.is_some();
    let (start_rx, start_ry) = in_flight_curve_head.unwrap_or(current_cell);
    let current_layer = match in_flight_curve_head {
        // The layer the body will be on at the curve head — from the accepted
        // path while it still lists that node, else the current layer.
        Some(head) => entity
            .movement_target
            .as_ref()
            .and_then(|target| {
                target
                    .path
                    .iter()
                    .position(|&cell| cell == head)
                    .map(|index| target.layer_at(index))
            })
            .unwrap_or_else(|| entity.movement_layer_or_ground()),
        None => entity.movement_layer_or_ground(),
    };
    // Derive movement_zone from the entity's locomotor — no parameter needed.
    let movement_zone: Option<MovementZone> = entity.locomotor.as_ref().map(|l| l.movement_zone);
    let speed_type = entity.locomotor.as_ref().map(|l| l.speed_type);
    let too_big_to_fit_under_bridge = entity.too_big_to_fit_under_bridge;
    let layered_pathing = entity
        .locomotor
        .as_ref()
        .is_some_and(|loco| supports_layered_bridge_pathing(loco, grid, entity.on_bridge));
    if locomotor_kind == Some(LocomotorKind::Walk)
        && (!queue
            || entities
                .get(entity_id)
                .is_some_and(|e| e.movement_target.is_none()))
    {
        // The ordinary Infantry setter51AA40 -> Foot4D94B0 -> Walk75ACB0
        // accepts before FindPath. Process75AFC5 reads the live route later;
        // see tools/spatial_oracle/walk_first_path.json. In particular an
        // occupied corridor cannot refuse an otherwise accepted destination.
        //4C747C installs the already encoded destination. The ordinary
        // click resolver4DE1D0 runs before event production, never here:
        // topology may have changed while a synchronized order was queued.
        let effective_target = target;
        let entity = entities.get_mut(entity_id).expect("resolved mover");
        if let Some((reference, coord)) = object_destination {
            super::navcom::set_destination_internal_coord(
                entity,
                reference,
                coord,
                resolved_terrain,
            );
        } else {
            super::navcom::set_destination_internal_cell(
                entity,
                effective_target,
                resolved_terrain,
            );
        }
        entity.navigation.path_replay.clear_live_head();
        timing.accept(entity);
        prepare_destination_execution(entity, effective_target, speed);
        return true;
    }
    // Retain the existing non-Walk recovery policy. Infantry51AA40 ->
    // Foot4D94B0 -> Walk75ACB0 has no PowerOn; a powered-down Walk still
    // accepts its destination, with power unchanged (scatter_destination).
    if locomotor_kind != Some(LocomotorKind::Walk)
        && let Some(loco) = entities
            .get_mut(entity_id)
            .and_then(|e| e.locomotor.as_mut())
    {
        loco.power_on();
    }
    let mut merged_entity_blocks = merge_path_blocks(
        entity_blocks,
        resolved_terrain,
        movement_zone,
        too_big_to_fit_under_bridge,
    );
    if let Some(occupation) = cell_occupation.as_deref() {
        merged_entity_blocks.extend(occupation.occupied_cells_ignoring(
            crate::sim::movement::locomotor::MovementLayer::Ground,
            entity_id,
        ));
    }
    let merged_entity_blocks_ref =
        (!merged_entity_blocks.is_empty()).then_some(&merged_entity_blocks);
    let Some(effective_target) = resolve_requested_move_goal(
        grid,
        target,
        merged_entity_blocks_ref,
        movement_zone,
        resolved_terrain,
        10,
    ) else {
        log::warn!(
            "No walkable cell near ({},{}) - cannot issue move",
            target.0,
            target.1,
        );
        return false;
    };
    if effective_target != target {
        log::info!(
            "Move: goal ({},{}) blocked, redirecting to ({},{})",
            target.0,
            target.1,
            effective_target.0,
            effective_target.1,
        );
    }

    if queue {
        // Remaining adapter families can append to their prepared path.
        // Ordinary Drive/Ship requests have already published the destination.
        let entity_mut = entities.get_mut(entity_id);
        if let Some(entity_mut) = entity_mut {
            if let Some(ref mut movement) = entity_mut.movement_target {
                let append_start = movement
                    .path
                    .last()
                    .copied()
                    .unwrap_or((start_rx, start_ry));
                let append_layer = movement
                    .path_layers
                    .last()
                    .copied()
                    .unwrap_or(current_layer);
                let zone_mz = movement_zone.unwrap_or(MovementZone::Normal);
                let Some((appended, appended_layers)) = find_move_path(
                    PathfindingContext {
                        wall_tables: None,
                        path_grid: Some(grid),
                        zone_grid,
                        resolved_terrain,
                        playfield_bounds,
                        blocker_neighbor_counts,
                    },
                    layered_pathing,
                    append_start,
                    append_layer,
                    effective_target,
                    terrain_costs,
                    // Pass the merged entity_blocks set to both layered slots so
                    // the layered A* sees building footprints regardless of which
                    // layer it expands. Mirrors the try_repath_after_block fix.
                    merged_entity_blocks_ref,
                    merged_entity_blocks_ref,
                    merged_entity_blocks_ref,
                    zone_mz,
                    movement_zone,
                    too_big_to_fit_under_bridge,
                    entity_block_map,
                    path_facts,
                    allow_zone_hierarchy,
                ) else {
                    return false;
                };
                if appended.len() >= 2 {
                    movement.path.extend_from_slice(&appended[1..]);
                    movement
                        .path_layers
                        .extend_from_slice(&appended_layers[1..]);
                    movement.speed = speed;
                    entity_mut
                        .navigation
                        .path_runtime
                        .start_blocked(timing.binary_frame, 0);
                    entity_mut.navigation.path_runtime.path_blocked = false;
                    debug_assert_eq!(
                        movement.path.len(),
                        movement.path_layers.len(),
                        "path/path_layers desync after queue append"
                    );
                }
                return true;
            }
        }
    }
    let zone_mz = movement_zone.unwrap_or(MovementZone::Normal);
    let ctx = PathfindingContext {
        wall_tables: None,
        path_grid: Some(grid),
        zone_grid,
        resolved_terrain,
        playfield_bounds,
        blocker_neighbor_counts,
    };
    let search = |goal: (u16, u16)| {
        find_move_path(
            ctx,
            layered_pathing,
            (start_rx, start_ry),
            current_layer,
            goal,
            terrain_costs,
            // Pass the merged entity_blocks set to both layered slots so the
            // layered A* sees building footprints regardless of which layer
            // it expands. Mirrors the try_repath_after_block fix.
            merged_entity_blocks_ref,
            merged_entity_blocks_ref,
            merged_entity_blocks_ref,
            zone_mz,
            movement_zone,
            too_big_to_fit_under_bridge,
            entity_block_map,
            path_facts,
            allow_zone_hierarchy,
        )
    };
    // Order-time reachability recovery. Gamemd runs `Can_Reach_Zone` before it
    // installs the mission and, when it fails, retargets to the nearest cell in
    // the mover's OWN zone rather than dropping the order — the unit drives to
    // the near bank. VERA evaluates it only after the search has already failed:
    // the reduced per-row zone map alone under-reports reachability relative to
    // the hierarchy-backed layered search (a high-bridge route the search finds
    // can read as cross-zone), so gating ahead of the search would refuse
    // destinations gamemd accepts. The recovered set is the same; only the
    // evaluation order differs.
    let mut effective_target = effective_target;
    let mut found = search(effective_target);
    if found.is_none()
        && let Some(st) = speed_type
    {
        match resolve_reachable_move_goal(
            grid,
            zone_grid,
            resolved_terrain,
            (start_rx, start_ry),
            current_layer,
            effective_target,
            zone_mz,
            st,
        ) {
            Some(near_bank) if near_bank != effective_target => {
                log::info!(
                    "Move: ({},{}) is out of the mover's zone, retargeting to ({},{})",
                    effective_target.0,
                    effective_target.1,
                    near_bank.0,
                    near_bank.1,
                );
                effective_target = near_bank;
                found = search(effective_target);
            }
            _ => {}
        }
    }
    let Some((path, path_layers)) = found else {
        let eb_count = merged_entity_blocks_ref.map_or(0, |s| s.len());
        log::warn!(
            "No path from ({},{}) to ({},{}) [entity_blocks={}, start_walkable={}, goal_walkable={}]",
            start_rx,
            start_ry,
            effective_target.0,
            effective_target.1,
            eb_count,
            grid.is_walkable(start_rx, start_ry),
            grid.is_walkable(effective_target.0, effective_target.1),
        );
        return false;
    };

    // Log path with walkability check for each cell — helps diagnose paths
    // that go through blocked cells (indicates PathGrid mismatch).
    let path_desc: String = path
        .iter()
        .map(|&(px, py)| {
            let w = grid.is_walkable(px, py);
            if w {
                format!("({},{})", px, py)
            } else {
                format!("({},{})!BLOCKED", px, py)
            }
        })
        .collect::<Vec<_>>()
        .join("→");
    log::info!(
        "Path: grid={}x{} entity_blocks={} {}",
        grid.width(),
        grid.height(),
        merged_entity_blocks_ref.map_or(0, |s| s.len()),
        path_desc,
    );

    // Compute initial facing toward the first movement cell (path[1], since path[0] = start).
    let mut new_facing: Option<u8> = None;
    if path.len() >= 2 {
        let next: (u16, u16) = path[1];
        let dx: i32 = next.0 as i32 - start_rx as i32;
        let dy: i32 = next.1 as i32 - start_ry as i32;
        new_facing = Some(facing_from_delta(dx, dy));
    }

    // A kept curve's head cell is a future node the body has not crossed into
    // yet: the queue cursor starts ON it so the coordinate crossing consumes
    // it, exactly as it would have consumed that node under the replaced path.
    let head_not_yet_reached =
        keep_in_flight_curve && (committed_walk || (start_rx, start_ry) != current_cell);
    let first_target_index = if head_not_yet_reached { 0 } else { 1 };

    // Compute initial direction vector toward the first path step.
    // No carry-forward needed — sub_x/sub_y already encode the entity's
    // exact lepton position, so it continues from wherever it is.
    let (dir_x, dir_y, dir_len) = if head_not_yet_reached {
        // The first vector target is the kept curve's head itself — up to two
        // cells out for a two-node curve — so use the Euclidean form (as in
        // `issue_direct_move`) in case the curve is torn down early and the
        // vector step has to cover the multi-cell delta.
        let dx = i32::from(start_rx) - i32::from(current_cell.0);
        let dy = i32::from(start_ry) - i32::from(current_cell.1);
        let dir_x = SimFixed::from_num(dx * 256);
        let dir_y = SimFixed::from_num(dy * 256);
        let dir_len = crate::util::fixed_math::fixed_distance(dir_x, dir_y);
        (dir_x, dir_y, dir_len)
    } else if path.len() >= 2 {
        crate::util::lepton::cell_delta_to_lepton_dir(
            path[1].0 as i32 - path[0].0 as i32,
            path[1].1 as i32 - path[0].1 as i32,
        )
    } else {
        (SIM_ZERO, SIM_ZERO, SIM_ZERO)
    };
    // Attach the MovementTarget and update facing on the entity.
    // All units start at full speed — acceleration/deceleration is disabled.
    let movement: MovementTarget = MovementTarget {
        path,
        path_layers,
        // Index 0 is the path anchor: the current position normally (first
        // target is 1), the kept curve's still-unreached head when re-ordered
        // mid-curve (the head itself is the first queued node).
        next_index: first_target_index,
        speed,
        current_speed: speed,
        move_dir_x: dir_x,
        move_dir_y: dir_y,
        move_dir_len: dir_len,
        final_goal: Some(effective_target),
        ..Default::default()
    };
    debug_assert_eq!(
        movement.path.len(),
        movement.path_layers.len(),
        "path/path_layers desync in initial MovementTarget"
    );
    if let Some(entity_mut) = entities.get_mut(entity_id) {
        let locomotor_kind = entity_mut
            .locomotor
            .as_ref()
            .map(|locomotor| locomotor.kind);
        if let Some((reference, coord)) = object_destination {
            super::navcom::set_destination_internal_coord(
                entity_mut,
                reference,
                coord,
                resolved_terrain,
            );
        } else if locomotor_kind == Some(LocomotorKind::Walk) {
            super::navcom::set_destination_internal_cell(
                entity_mut,
                effective_target,
                resolved_terrain,
            );
        }
        if locomotor_kind == Some(LocomotorKind::Walk) {
            // Infantry51AD11 clears one live Foot path word. The accepted
            // setter preserves its suffix/reference and NavQueue; Walk's first
            // no-head Process requests FindPath75AFC5 later. See
            // tools/spatial_oracle/walk_first_path.json. MovementTarget keeps
            // the existing prepared physical path, without publishing it here.
            entity_mut.navigation.path_replay.clear_live_head();
        }
        if !keep_in_flight_curve && let Some(f) = new_facing {
            let has_rot = entity_mut.locomotor.as_ref().is_some_and(|l| l.rot > 0);
            if entity_mut.category != EntityCategory::Infantry && has_rot {
                entity_mut.facing_target = Some(f);
            } else {
                entity_mut.facing = f;
            }
        }
        // Unit's accepted setter reaches Foot4D96C2..9707 just as Walk's
        // does. Preserve +64C; this is not a Foot constructor.
        timing.accept(entity_mut);
        entity_mut.movement_target = Some(movement);
    }

    true
}

/// Ordinary Unit741970 -> Foot4D94B0 -> Drive4AFD40/Ship69F450 accepts
/// before any FindPath, preserves power and a paid head, and clears only the
/// live path word (741E88) unless Enter lacks a radio contact
/// (tools/spatial_oracle/track_path_continuation, Enter rows); the setter's
/// flag-gated NavQueue Clear (7422E8..7422F4) empties the waypoint queue
/// (track_destination NavQueue rows). Neither map availability nor an A* result admits the
/// destination: the first no-queue Process owns the request (Drive 4B28A3,
/// Ship 6A1EF3). Native evidence: track_destination unit rows and
/// track_order_path. Complete class preprocessing (radio building, Teleporter
/// swap, deploy bytes, +1F8 and skip-MoveTo) remains open.
pub(crate) fn prepare_track_destination(
    entity: &mut GameEntity,
    target: (u16, u16),
    object_destination: Option<(
        crate::sim::components::NavTargetRef,
        crate::sim::components::DriveCoord,
    )>,
    speed: SimFixed,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    timing: crate::sim::movement::DestinationTiming,
) {
    clear_destination_path_head(entity);
    entity.navigation.nav_queue.clear();
    if let Some((reference, coord)) = object_destination {
        super::navcom::set_destination_internal_coord(entity, reference, coord, resolved_terrain);
    } else {
        super::navcom::set_destination_internal_cell(entity, target, resolved_terrain);
    }
    timing.accept(entity);
    prepare_destination_execution(entity, target, speed);
}

/// Accepted Cell destination -> Walk75ACB0, without searching or advancing
/// Process. The class caller owns preceding admission; this accepted Cell
/// path preserves Enter-without-contact's skipped path-head write.
/// No PathGrid is needed until the next ordinary Process (or the immediate
/// Process specifically required by NULL-source Scatter51D478).
pub(crate) fn prepare_walk_cell_destination(
    entities: &mut EntityStore,
    entity_id: u64,
    target: (u16, u16),
    speed: SimFixed,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    timing: crate::sim::movement::DestinationTiming,
) -> bool {
    let Some(entity) = entities.get_mut(entity_id) else {
        return false;
    };
    if !entity
        .locomotor
        .as_ref()
        .is_some_and(|l| l.kind == LocomotorKind::Walk)
    {
        return false;
    }
    clear_destination_path_head(entity);
    super::navcom::set_destination_internal_cell(entity, target, resolved_terrain);
    timing.accept(entity);
    prepare_destination_execution(entity, target, speed);
    true
}

/// The setters' one-word PathHead clear, which Enter (current or queued)
/// without a radio contact skips: Infantry 51AC25..51AD17, Unit
/// 741C4F..741C78 -> 741E88 (both the NULL and the non-NULL destination).
/// NavQueue, suffix and reference survive.
pub(super) fn clear_destination_path_head(entity: &mut GameEntity) {
    if (entity.mission.effective().raw() != 7 && entity.mission.queued().raw() != 7)
        || !entity.radio_contacts.is_empty()
    {
        entity.navigation.path_replay.clear_live_head();
    }
}

/// Install the empty-route scheduling adapter for a Drive/Ship Foot whose
/// locomotor destination was set outside the ordinary setter (the outer
/// Process NavCom reissue), so its next visit reaches Process_Movement.
pub(super) fn schedule_track_process(entity: &mut GameEntity, target: (u16, u16), speed: SimFixed) {
    prepare_destination_execution(entity, target, speed);
}

/// A Drive/Ship route the track terminal spent short of the retained
/// locomotor destination (+34): the adapter keeps scheduling with an empty
/// route, so the next Process_Movement reaches the no-queue arm (Drive
/// 0x4B281C / Ship 0x6A1E75), in the same Process after the track end
/// (`track_continuation`) or on the next visit. With no destination left it
/// retires.
pub(super) fn spend_track_route(entity: &mut GameEntity) {
    let destination = match entity.locomotor.as_ref().map(|loco| loco.active_kind()) {
        Some(LocomotorKind::Drive) => entity.drive_locomotion.as_ref().and_then(|d| d.destination),
        Some(LocomotorKind::Ship) => entity.ship_locomotion.as_ref().and_then(|s| s.destination),
        _ => None,
    };
    if destination.is_none() {
        entity.movement_target = None;
        return;
    }
    if let Some(target) = entity.movement_target.as_mut() {
        target.path.clear();
        target.path_layers.clear();
        target.next_index = 0;
        target.move_dir_x = SIM_ZERO;
        target.move_dir_y = SIM_ZERO;
        target.move_dir_len = SIM_ZERO;
    }
}

/// MovementTarget is only the scheduling adapter. Retain a paid head, never
/// turn or search at order time. The ordinary Process owns the first route
/// and subsequent head selection for Walk, Drive and Ship.
fn prepare_destination_execution(entity: &mut GameEntity, target: (u16, u16), speed: SimFixed) {
    let committed_head = committed_path_head(entity);
    entity.movement_target = Some(MovementTarget {
        speed,
        current_speed: speed,
        final_goal: Some(target),
        ..Default::default()
    });
    if committed_head.is_some() {
        retain_path_to_head(entity, committed_head);
        entity.movement_target.as_mut().unwrap().final_goal = Some(target);
    }
}
