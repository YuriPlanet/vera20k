//! Locomotor movement and its shared Foot/coordinate owners.
//!
//! Native-frame visits execute the retained locomotor state: Drive/Ship use
//! their track hosts and Walk pays a polar step toward its accepted head.
//! Other locomotors run their own Process hosts. Coordinate and occupation
//! transactions publish the resulting whole-cell crossings.
//!
//! ## Coordinate update
//! Movement advances `rx`/`ry`/`sub_x`/`sub_y` only. Screen position is not
//! stored and not written here — `render::locomotor_visual::screen_position`
//! derives it from these leptons on read, which is what gives smooth sub-cell
//! movement without render interpolation.
//!
//! ## Facing
//! RA2 uses a 0-255 screen-relative DirStruct byte: 0=north on screen (iso -x,-y),
//! 64=east on screen (iso +x,-y), 128=south on screen (iso +x,+y),
//! 192=west on screen (iso -x,+y). The full 16-bit heading is the body
//! FacingClass (`GameEntity::body_facing`, `+0x388`), each Techno's only copy;
//! byte readers take its high byte.
//!
//! ## Sub-modules
//! - `movement_commands` — the class setters' shared Move_To tails; no order
//!   searches a route
//! - `foot_path` — the shared `FootClass::Find_Path` (0x4D3920) owner that a
//!   Walk or Drive/Ship no-queue Process request runs at Simulation level
//! - `walk_path` / `track_path` — the Walk and Drive/Ship continuations after
//!   Find_Path and the class receivers they reach
//! - `track_continuation` — the Drive/Ship Process continuing into
//!   Process_Movement and Process_Track(1) after a track ends in the same call
//! - `movement_tick` — per-tick ground movement state machine (the main loop)
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/entity_store, sim/game_entity, sim/pathfinding.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::map::entities::EntityCategory;
#[cfg(any(test, feature = "test-support"))]
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::sim::cell_rect::PlayfieldBounds;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::entity_store::EntityStore;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::lifecycle_request::LifecycleRequest;
use crate::sim::pathfinding::PathGrid;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig;
use crate::sim::pathfinding::zone_map::ZoneGrid;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::rng::SimRng;
#[cfg(any(test, feature = "test-support"))]
use crate::util::fixed_math::facing_from_delta_int;

// --- Internal submodules ---
pub(crate) mod at_coord;
mod building_coordinate;
pub(crate) use building_coordinate::exit_coordinate as building_exit_coordinate;
pub(crate) use building_coordinate::{factory_exit_track_coordinate, war_factory_exit_cell};
mod cell_contact;
mod drive_locomotion;
mod foot_approach;
mod foot_coordinate;
pub(crate) mod foot_mark;
mod foot_path;
pub(crate) use drive_locomotion::drive_do_turn;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use foot_path::FindPathResult;
pub(crate) use foot_path::FootPathOutcome;
pub(crate) use foot_path::coord_cell;
pub(crate) use track_fresh::{BlockingObject, ProcessMovementArgs};
mod foot_range_stop;
mod foot_speed;
pub mod ground_pose;
pub(crate) mod infantry_action;
pub(crate) mod infantry_entry;
mod infantry_scatter;
pub(crate) mod locomotor_owner;
pub(crate) mod locomotor_ready;
mod locomotor_turn;
pub(crate) use locomotor_turn::locomotor_do_turn;
pub mod motion_query;
pub(crate) mod movement_bridge;
mod movement_commands;
mod movement_path;
mod movement_step;
pub(crate) mod movement_tick;
mod nav_list;
pub(crate) mod navcom;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use navcom::set_walk_destination_coord;
pub(crate) use navcom::{building_dock_cell, building_dock_coordinate, nav_target_coordinate};
#[cfg(test)]
pub(crate) mod fresh_oracle_seam;
pub(crate) mod path_batch;
mod path_markers;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use path_markers::fixture_path_replay;
mod per_cell;
pub(crate) use per_cell::PerCellReason;
pub(crate) mod ready_producer;
mod scatter;
#[cfg(test)]
pub(crate) use infantry_scatter::answered_process;
pub(crate) use scatter::{DamageRetaliation, ScatterFlags};
mod scatter_cell;
pub mod slope_transition;
mod track_continuation;
mod track_fresh;
mod track_fresh_dispatch;
pub(crate) mod track_head;
mod track_host;
mod track_path;
pub(crate) use track_path::SetterForceReassign;
pub mod track_process;
mod track_speed;
pub(crate) mod track_turn;
mod walk_admission;
pub(crate) mod walk_head;
mod walk_host;
mod walk_path;
mod walk_step;

// --- Movement-related modules (public API) ---
pub mod air_movement;
pub mod bump_crush;
pub mod drive_track;
pub mod facing_class;
pub mod fly_height;
pub mod group_destination;
pub mod hover;
pub mod jumpjet_movement;
pub mod locomotion;
pub mod locomotor;
pub mod parachute_descent;
pub mod rocket_movement;
mod teleport_chrono;
pub mod teleport_movement;
pub mod tube_movement;
pub mod turret;

pub use facing_class::FacingClass;

#[cfg(any(test, feature = "test-support"))]
pub use drive_locomotion::{DriveLocomotionRuntime, ShipLocomotionRuntime};
#[cfg(any(test, feature = "test-support"))]
pub(crate) use foot_speed::owner_current_speed_from_fraction;
pub use foot_speed::owner_speed_bonus;
pub use foot_speed::{SpeedRules, owner_current_speed};

pub use movement_commands::DestinationTiming;
pub use movement_commands::movement_goal_cell;
pub(crate) use movement_commands::{can_accept_destination, prepare_walk_destination};
#[cfg(any(test, feature = "test-support"))]
pub(crate) use movement_path::{
    path_search_used_zone_grid_marker, reset_path_search_used_zone_grid_marker,
};
pub(crate) use navcom::{
    foot_stop_moving, nav_targets_same_receiver, set_destination_internal_cell, target_cell_coord,
    track_stop_moving,
};
// Legacy batch tick used by focused movement fixtures.
#[cfg(any(test, feature = "test-support"))]
pub(crate) use movement_tick::tick_movement_with_grids;

// ---------------------------------------------------------------------------
// Types — shared across movement submodules
// ---------------------------------------------------------------------------

/// Read-only grid/terrain environment for pathfinding and movement decisions.
#[derive(Clone, Copy)]
pub(super) struct PathfindingContext<'a> {
    pub path_grid: Option<&'a PathGrid>,
    pub zone_grid: Option<&'a ZoneGrid>,
    pub resolved_terrain: Option<&'a ResolvedTerrainGrid>,
    pub playfield_bounds: Option<PlayfieldBounds>,
    pub blocker_neighbor_counts: Option<&'a crate::sim::pathfinding::BlockerNeighborCounts>,
}

/// Snapshot of mover properties taken before the inner movement loop.
/// Avoids repeated `entities.get()` calls and survives across the mutable/immutable
/// borrow boundary (lines ~211–920 hold `&mut GameEntity`, lines ~920–1230 release
/// the borrow for `&EntityStore` lookups).
pub(super) struct MoverSnapshot {
    pub category: EntityCategory,
    pub speed_type: Option<SpeedType>,
    pub movement_zone: MovementZone,
    pub omni_crusher: bool,
    pub regular_crusher: bool,
    pub on_bridge: bool,
    pub locomotor: Option<locomotor::LocomotorState>,
    /// Native AStar hierarchy admission reads the stored TechnoClass+0x3D5
    /// byte. False under live MapClass authority bypasses hierarchy and uses
    /// flat A*; headless fixtures without authority retain hierarchy.
    pub allow_zone_hierarchy: bool,
}

/// Per-tick movement diagnostics — returned by `tick_movement_with_grids`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MovementTickStats {
    pub movers_total: u32,
    pub moved_steps: u32,
}

impl MovementTickStats {
    pub(crate) fn merge(&mut self, other: Self) {
        self.movers_total = self.movers_total.saturating_add(other.movers_total);
        self.moved_steps = self.moved_steps.saturating_add(other.moved_steps);
    }
}

// ---------------------------------------------------------------------------
// Public utilities
// ---------------------------------------------------------------------------

/// Compute the active-retail screen-relative facing byte from a coordinate delta.
///
/// Computed directions use the high byte of the native 65,534-scale word;
/// authored quarter-turn values remain distinct.
#[cfg(any(test, feature = "test-support"))]
pub fn facing_from_delta(dx: i32, dy: i32) -> u8 {
    facing_from_delta_int(dx, dy)
}

// ---------------------------------------------------------------------------
// Tick entry points (thin wrappers)
// ---------------------------------------------------------------------------

/// Run one ground movement visit for every entity, in id order.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn tick_movement(
    entities: &mut EntityStore,
    interner: &mut crate::sim::intern::StringInterner,
    lifecycle_requests: &mut Vec<LifecycleRequest>,
) {
    let empty_alliances: HouseAllianceMap = HouseAllianceMap::new();
    let mut rng: SimRng = SimRng::new(0);
    let mut empty_occupancy = crate::sim::occupancy::OccupancyGrid::new();
    let _ = tick_movement_with_grid(
        entities,
        None,
        &empty_alliances,
        &mut empty_occupancy,
        &mut rng,
        0, // sim_tick not available in test-only wrapper
        interner,
        lifecycle_requests,
    );
}

/// Advance movement and perform deterministic blocked-cell recovery.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn tick_movement_with_grid(
    entities: &mut EntityStore,
    path_grid: Option<&PathGrid>,
    alliances: &HouseAllianceMap,
    occupancy: &mut crate::sim::occupancy::OccupancyGrid,
    rng: &mut SimRng,
    sim_tick: u64,
    interner: &mut crate::sim::intern::StringInterner,
    lifecycle_requests: &mut Vec<LifecycleRequest>,
) -> MovementTickStats {
    let mut sound_events: Vec<crate::sim::world::SimSoundEvent> = Vec::new();
    let mut cell_occupation =
        crate::sim::occupancy::CellOccupationGrid::rebuild(entities, occupancy);
    let mut raw_cell_occupation = crate::sim::occupancy::RawCellOccupationGrid::new();
    tick_movement_with_grids(
        entities,
        None,
        path_grid,
        alliances,
        occupancy,
        &mut cell_occupation,
        &mut raw_cell_occupation,
        rng,
        sim_tick,
        sim_tick as u32, // native-frame proxy (test-only wrapper: 1 frame/tick)
        None,            // No zone grid in legacy wrapper
        None,            // No resolved terrain in legacy wrapper
        None,            // No playfield bounds in legacy wrapper
        &TerrainSpeedConfig::default(),
        interner,
        None, // No RuleSet in legacy wrapper — crush sounds suppressed
        &mut sound_events,
        lifecycle_requests,
    )
}

// ---------------------------------------------------------------------------
// Internal helpers — shared across movement submodules
// ---------------------------------------------------------------------------

#[cfg(test)]
mod balloon_hover_tests;
#[cfg(test)]
pub(crate) mod bridge_layer_oracle_tests;
#[cfg(test)]
mod ground_pose_tests;
#[cfg(test)]
mod movement_bridge_retail_tests;
#[cfg(test)]
mod movement_tests;

#[cfg(test)]
mod foot_timer_migration_tests;

#[cfg(test)]
mod drop_as_bomb_oracle_tests;
