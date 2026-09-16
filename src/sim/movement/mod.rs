//! Unit movement system — moves entities along A* paths each tick.
//!
//! The movement system reads MovementTarget fields and advances entities
//! toward their destination using lepton-based sub-cell movement.
//! Each tick, `sub_x`/`sub_y` advance along the direction vector at
//! `speed` leptons per second. Cell transitions occur when sub_x/sub_y
//! cross the cell boundary (0 or 256 leptons).
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
//! 192=west on screen (iso -x,+y). Facing is updated whenever the entity starts
//! moving toward a new cell.
//!
//! ## Sub-modules
//! - `movement_commands` — A* pathfinding and MovementTarget attachment
//! - `movement_tick` — per-tick ground movement state machine (the main loop)
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/entity_store, sim/game_entity, sim/pathfinding.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

#[cfg(test)]
use std::collections::BTreeMap;

use crate::map::entities::EntityCategory;
#[cfg(test)]
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::sim::cell_rect::PlayfieldBounds;
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::InternedId;
#[cfg(test)]
use crate::sim::lifecycle_request::LifecycleRequest;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::cell_entry::WallArmTables;
#[cfg(test)]
use crate::sim::pathfinding::terrain_cost::TerrainCostGrid;
#[cfg(test)]
use crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig;
use crate::sim::pathfinding::zone_map::ZoneGrid;
#[cfg(test)]
use crate::sim::rng::SimRng;
#[cfg(test)]
use crate::util::fixed_math::SIM_ZERO;
use crate::util::fixed_math::{SIM_ONE, SimFixed, facing_from_delta_int};

// --- Internal submodules ---
pub(crate) mod at_coord;
mod cell_arrival;
mod drive_locomotion;
pub(crate) mod ground_pose;
pub(crate) mod infantry_entry;
pub(crate) mod locomotor_owner;
pub(crate) mod locomotor_ready;
mod movement_blocked;
pub(crate) mod movement_bridge;
mod movement_commands;
mod movement_occupancy;
mod movement_path;
mod movement_step;
pub(crate) mod movement_tick;
mod navcom;
pub(crate) use navcom::set_walk_destination_coord;
mod path_markers;
pub(crate) mod ready_producer;
pub(crate) mod slope_transition;
mod track_head;
mod track_host;
pub(crate) mod track_process;
pub(crate) mod walk_head;
mod walk_host;
mod walk_path;

// --- Movement-related modules (public API) ---
pub mod air_movement;
pub mod bump_crush;
pub mod drive_track;
pub mod drop_pod_movement;
pub mod facing_class;
pub mod group_destination;
pub mod homing_movement;
pub mod hover;
pub mod jumpjet_flight;
pub mod jumpjet_movement;
pub mod locomotion;
pub mod locomotor;
pub mod parachute_descent;
pub mod rocket_movement;
pub mod scatter;
pub mod teleport_movement;
pub mod tube_movement;
pub mod tunnel_movement;
pub mod turret;

pub use facing_class::FacingClass;

// The drive-locomotor "Process" presence marker, consumed read-only by the
// per-object AI shell (sim/world/techno_ai.rs, Slice S1) to observe that the
// locomotor would process AFTER mission dispatch. Behavior-neutral re-export.
#[cfg(test)]
pub(crate) use drive_locomotion::owner_current_speed_from_fraction;
// NOT test-gated: `techno_common_pre`'s DisguiseWhenStill check
// (sim/world/techno_ai.rs) consumes this in every build; a 2026-08-14
// warning-cleanup gate on it broke release-only compilation.
pub(crate) use drive_locomotion::drive_locomotor_is_moving;
#[cfg(test)]
pub(crate) use drive_locomotion::{DriveProcessOutcome, process_drive_locomotion_shell};

// Re-export command functions so callers can use `movement::issue_move_command` etc.
pub use movement_commands::{
    DestinationTiming, clear_navigation_for_entity, issue_direct_move, issue_move_command,
    set_destination_for_teleporter_entity, stop_navigation_at_committed_head,
};
pub(crate) use movement_commands::{
    issue_move_command_with_destination, issue_move_command_with_layered,
    prepare_walk_cell_destination, retain_committed_movement,
};
#[cfg(test)]
pub(crate) use movement_path::{
    path_search_used_zone_grid_marker, reset_path_search_used_zone_grid_marker,
};
pub(crate) use movement_tick::sync_formation_speeds_after_live_pass;
// Legacy batch tick used by focused movement fixtures.
#[cfg(test)]
pub(crate) use movement_tick::tick_movement_with_grids;

/// Install the active-YR `DriveLocomotion::Force_Track` state for a flat-ground
/// unit. The caller supplies head offsets from the unit's current cell origin;
/// and the caller's raw Z (native Force_Track4B0C40 copies the full XYZ).
/// The stored head is an exact absolute lepton coordinate.
pub(crate) fn install_forced_drive_track(
    entity: &mut crate::sim::game_entity::GameEntity,
    cell_occupation: &mut crate::sim::occupancy::CellOccupationGrid,
    mut forced: drive_track::ForcedDriveTrackState,
    head_z_leptons: i32,
) -> bool {
    if entity.occupancy_list_layer() != Some(locomotor::MovementLayer::Ground) {
        return false;
    }

    let absolute_x = i32::from(entity.position.rx) * 256 + forced.track.head_offset_x;
    let absolute_y = i32::from(entity.position.ry) * 256 + forced.track.head_offset_y;
    let target_rx = absolute_x.div_euclid(256);
    let target_ry = absolute_y.div_euclid(256);
    let (Ok(target_rx), Ok(target_ry)) = (u16::try_from(target_rx), u16::try_from(target_ry))
    else {
        return false;
    };

    let head = crate::sim::components::DriveCoord {
        x: absolute_x,
        y: absolute_y,
        z: head_z_leptons,
    };
    let footprint = crate::sim::components::DriveOccupationFootprint {
        rx: target_rx,
        ry: target_ry,
        layer: locomotor::MovementLayer::Ground,
    };

    // Captured before the Drive borrow. The early return above already
    // established the mover is on the ground plane.
    let current_cell = (entity.position.rx, entity.position.ry);
    let current_layer = locomotor::MovementLayer::Ground;
    let entity_stable_id = entity.stable_id();

    let drive = entity
        .drive_locomotion
        .get_or_insert_with(crate::sim::components::DriveLocomotionRuntime::default);
    // Force_Track preserves DriveLocomotion's integer movement residual. The
    // detached forced cursor mirrors that canonical owner field for snapshots.
    forced.track.residual = drive.track.residual;
    drive.pending_track_occupation = false;
    drive.destination = Some(head);
    drive.head_to = Some(head);
    drive
        .track
        .select_forced(i32::from(forced.turn_track_index));
    drive.track_valid = true;
    drive.target_speed_fraction = SIM_ONE;
    // OPEN Process-host timing: native Force_Track4B0D52 changes the class
    // target only; the subsequent ProcessTrack owns the applied-speed write.
    entity.foot_speed.applied_fraction = SIM_ONE;
    entity.foot_speed.cached_current_speed =
        drive_locomotion::owner_current_speed_from_fraction(forced.speed, SIM_ONE);
    // Force_Track directly installs the new head mark. Its active retail callers
    // enter with no old head — but nothing in this function's signature enforces
    // that, and a caller that reached a mid-curve mover would otherwise strand
    // both of that curve's claims: a head cell and a forward handoff cell that
    // nothing occupies and every later mover is refused entry to.
    // `Apply_Track_Occupation_Mode` releases the pair together on mode 0, so
    // release them here before installing the replacement.
    crate::sim::occupancy::drop_drive_handoff_occupation(
        &mut entity.foot_occupation_enabled,
        drive,
        cell_occupation,
        entity_stable_id,
        current_cell,
        current_layer,
    );
    crate::sim::occupancy::clear_drive_head_to_occupation_for_replacement(
        &mut entity.foot_occupation_enabled,
        drive,
        cell_occupation,
        entity_stable_id,
        current_cell,
        current_layer,
    );
    cell_occupation.mark_vehicle_on_layer(
        footprint.rx,
        footprint.ry,
        entity_stable_id,
        footprint.layer,
    );
    drive.occupation_head_to = Some(footprint);

    entity.drive_track = None;
    entity.forced_drive_track = Some(forced);
    entity.facing_target = None;
    true
}

// ---------------------------------------------------------------------------
// Constants — shared across movement submodules via `super::`
// ---------------------------------------------------------------------------

/// Initial path retry counter before giving up (`Foot+0x64C`, init 10).
///
/// The mechanism is real and this constant is right: `Process_Movement` reads
/// `[ECX+0x64C]` at `0x004B2DC8`, and on `> 0` decrements and stores it back
/// (`0x004B2DD2`/`0x004B2DD3`) before continuing; on `<= 0` it clears the drive
/// coord, calls `FootClass::Stop_Moving` plus vtable `+0x480`/`+0x484`, and
/// plays the blocked voice at `Foot+0x68A`. So it *is* decremented and it *does*
/// end the move at zero.
///
/// Recorded difference: **what decrements it.** Native decrements it on every
/// pass through the generic blocked label `LAB_004B3282` — reached from
/// `Is_Cell_In_Playfield == 0`, from code 3, from `code != 6` and from two
/// code-6 sub-failures — and the same label is where the literal 10 is stored.
/// VERA decrements per failed repath instead. The escalation clock is a
/// separate record: `Foot+0x668` = frame, `Foot+0x66C`, and `Foot+0x670` =
/// `Rules+0x1768`, stored only on the `Foot+0x6B7 == 0` transition of the code-2
/// arm — which contains no `Scatter_Objects` call at all. Trigger: a unit
/// blocked long enough to exhaust the counter. Player effect: the give-up point
/// arrives after a different number of ticks than retail's. Frequency: every
/// traffic jam, many times a minute once a base has armour queuing. Downstream
/// risk: the two clocks are separate fields and must stay separate.
const PATH_STUCK_INIT: u32 = 10;
/// Minimum height level difference to trigger Rust's defensive cliff detection.
///
/// **VERA-internal, gamemd equivalent UNCHECKED** — "abs(current_z / HeightStep
/// - cell.height) >= 3 levels" carries no address and no verified owner.
const CLIFF_HEIGHT_THRESHOLD: u16 = 3;
/// Infantry wobble phase increment per second (radians/sec).
/// One full cycle (2π) per ~2.5 seconds ≈ 2.5 rad/s. Matches slow
/// infantry walk cadence in the original game.
const INFANTRY_WOBBLE_RATE: f32 = 2.5;
/// Minimum speed as a fraction of max speed during normal braking.
/// Original engine: 0.3 (30% of max speed).
const MIN_BRAKE_FRACTION: SimFixed = SimFixed::lit("0.3");

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
    /// Map-global tables the wall arm reads, carried once per pass (ledger I9b).
    ///
    /// The search calls the Foot `+0x1AC` slot per neighbour and prices the
    /// returned class through `AStar_compute_edge_cost @ 0x00429830`; a wall
    /// answers 4 or 5, which expand at 60x and 20x rather than blocking. `None`
    /// keeps the pre-I9b search, where a wall is simply impassable.
    ///
    /// This carries the *tables*, not a built classifier: the classifier is
    /// per mover and is constructed at the search boundary from these plus the
    /// mover's own facts. Holding a per-mover `&dyn` on a per-pass `Copy` struct
    /// was a granularity mismatch.
    pub wall_tables: Option<WallArmTables<'a>>,
}

/// Movement timing/threshold config derived from rules.ini [General] section.
/// Separate from `PathfindingContext` because `find_move_path` doesn't need these.
#[derive(Clone, Copy)]
pub(super) struct MovementConfig {
    pub binary_frame: u32,
    pub close_enough: SimFixed,
    pub path_delay_ticks: u16,
    pub blockage_path_delay_ticks: u16,
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
    pub drive_accelerates: bool,
    pub owner: InternedId,
    /// `TechnoClass::Is_Armed @ 0x00701120` (vtable `+0x2AC`). An unarmed mover
    /// leaves the wall arm through the shared epilogue at `0x0073FCD0`.
    ///
    /// Resolved here, per mover, rather than at each path-request site: ledger
    /// row I9c is what happens when a mover fact is derived independently per
    /// caller and the callers without context quietly pass `false`.
    pub is_armed: bool,
    /// Slot-0 warhead `Wall=` (`WarheadTypeClass+0x144`).
    pub warhead_wall: bool,
    /// Slot-0 warhead `Wood=` (`+0x147`), which the arm admits only against an
    /// overlay whose own `Armor` is wood, and only for Units.
    pub warhead_wood: bool,
    pub too_big_to_fit_under_bridge: bool,
    pub on_bridge: bool,
    pub runtime_bridge_transition: movement_bridge::RuntimeBridgeTransitionState,
    pub locomotor: Option<locomotor::LocomotorState>,
    pub rot: i32,
    /// Mover's `MovementTarget.bypass_grid` flag — when true, structure
    /// occupants are skipped during the foundation-cross occupancy check
    /// (harvester dock drive: buildings are not scatter targets).
    pub bypass_grid: bool,
    /// Whether this mover's current mission is one of the five the original
    /// engine lets bypass sub-cell occupancy: Enter (7), Capture (8), Eaten
    /// (9), Area Guard (11), Patrol (25).
    ///
    /// Half of the "priority" placement condition; the other half is the
    /// NavCom sitting in the cell being entered, which can only be tested per
    /// crossing (see [`MoverSnapshot::nav_com_cell`]).
    pub sub_cell_priority_mission: bool,
    /// Cell currently occupied by this mover's NavCom target, when that target
    /// is an **object**.
    ///
    /// Resolved once per tick from the entity store as the target entity's
    /// anchor cell. A bare-cell NavCom yields `None`: the original tests its
    /// destination as an object pointer, so a cell destination never satisfies
    /// the priority condition.
    pub nav_com_cell: Option<(u16, u16)>,
    /// Native AStar hierarchy admission reads the stored TechnoClass+0x3D5
    /// byte. False under live MapClass authority bypasses hierarchy and uses
    /// flat A*; headless fixtures without authority retain hierarchy.
    pub allow_zone_hierarchy: bool,
}

impl MoverSnapshot {
    /// The mover's crush authority, as [`bump_crush::CrushCapability::of`]
    /// reads it from the live object.
    pub(super) const fn crush_capability(&self) -> bump_crush::CrushCapability {
        bump_crush::CrushCapability::new(self.regular_crusher, self.omni_crusher)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PendingCrushKill {
    pub victim_id: u64,
    pub crusher_id: u64,
    pub crush_coord: (i32, i32),
}

/// Per-tick movement diagnostics — returned by `tick_movement_with_grids`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MovementTickStats {
    pub movers_total: u32,
    pub moved_steps: u32,
    pub blocked_attempts: u32,
    pub repath_attempts: u32,
    pub repath_successes: u32,
    pub scatter_successes: u32,
    pub crush_kills: u32,
    pub stuck_aborts: u32,
    /// Scatter attempts triggered when infantry are blocked.
    pub scatter_attempts: u32,
    /// Track selections triggered for vehicle turns.
    pub track_selections: u32,
    /// Stuck entities that recovered via repath or scatter.
    pub stuck_recoveries: u32,
    /// Fresh Drive curve selections refused by the cell-entry predicate, on
    /// either arm. Counted so a fixture can show the lane was actually
    /// exercised rather than trivially absent.
    pub selection_admission_refusals: u32,
    /// Elapsed microseconds for the entire tick.
    pub elapsed_us: u64,
}

impl MovementTickStats {
    pub(crate) fn merge(&mut self, other: Self) {
        self.movers_total = self.movers_total.saturating_add(other.movers_total);
        self.moved_steps = self.moved_steps.saturating_add(other.moved_steps);
        self.blocked_attempts = self.blocked_attempts.saturating_add(other.blocked_attempts);
        self.repath_attempts = self.repath_attempts.saturating_add(other.repath_attempts);
        self.repath_successes = self.repath_successes.saturating_add(other.repath_successes);
        self.scatter_successes = self
            .scatter_successes
            .saturating_add(other.scatter_successes);
        self.crush_kills = self.crush_kills.saturating_add(other.crush_kills);
        self.stuck_aborts = self.stuck_aborts.saturating_add(other.stuck_aborts);
        self.scatter_attempts = self.scatter_attempts.saturating_add(other.scatter_attempts);
        self.track_selections = self.track_selections.saturating_add(other.track_selections);
        self.stuck_recoveries = self.stuck_recoveries.saturating_add(other.stuck_recoveries);
        self.selection_admission_refusals = self
            .selection_admission_refusals
            .saturating_add(other.selection_admission_refusals);
        self.elapsed_us = self.elapsed_us.saturating_add(other.elapsed_us);
    }
}

/// Command to move an entity to a target cell (queued for next tick).
#[derive(Debug, Clone)]
pub struct MoveCommand {
    pub entity_id: u64,
    pub target_rx: u16,
    pub target_ry: u16,
    pub queue: bool,
}

// ---------------------------------------------------------------------------
// Public utilities
// ---------------------------------------------------------------------------

/// Compute the active-retail screen-relative facing byte from a coordinate delta.
///
/// Computed directions use the high byte of the native 65,534-scale word;
/// authored quarter-turn values remain distinct.
pub fn facing_from_delta(dx: i32, dy: i32) -> u8 {
    facing_from_delta_int(dx, dy)
}

/// Restore active piggyback locomotors whose owner is no longer moving,
/// teleporting, or deploying.
///
/// This bridge mirrors FootClass::AI's per-tick "ok to end piggyback" check
/// without changing existing movement ownership for non-migrated special flows.
pub fn tick_locomotor_piggyback_restore(entities: &mut EntityStore) -> usize {
    let mut restored = 0usize;
    let keys = entities.keys_sorted();
    for id in keys {
        restored = restored.saturating_add(usize::from(tick_locomotor_piggyback_restore_one(
            entities, id,
        )));
    }
    restored
}

/// Build the `Is_Ok_To_End` inputs for one entity.
///
/// gamemd's END gate reads the ACTIVE locomotor's own `Is_Moving` (ILocomotion
/// slot 4) — `Drive::Is_Ok_To_End` calls it on the object's own ILocomotion,
/// which inspects the Drive locomotor's destination and head-to coordinates
/// against the owner's exact position, never the owner's path queue. Drive now
/// has that predicate; the remaining classes keep the VERA-internal
/// owner-path approximation, gamemd equivalent UNCHECKED.
pub(crate) fn locomotor_end_gate_context(
    entity: &crate::sim::game_entity::GameEntity,
) -> locomotion::piggyback::EndGateContext {
    let active_is_drive = entity.locomotor.as_ref().is_some_and(|loco| {
        loco.active_kind() == crate::rules::locomotor_type::LocomotorKind::Drive
    });
    let owner_moving = if active_is_drive {
        drive_locomotion::drive_locomotor_is_moving(entity) || entity.forced_drive_track.is_some()
    } else {
        entity.movement_target.is_some() || entity.forced_drive_track.is_some()
    };
    locomotion::piggyback::EndGateContext {
        owner_moving,
        owner_teleporting: entity.teleport_state.is_some(),
        owner_deploying: entity.building_up.is_some()
            || entity.building_down.is_some()
            || entity.deploy_state.is_some(),
    }
}

pub(crate) fn tick_locomotor_piggyback_restore_one(entities: &mut EntityStore, id: u64) -> bool {
    let Some(entity) = entities.get_mut(id) else {
        return false;
    };
    locomotor_owner::try_restore_primary(entity)
}

// ---------------------------------------------------------------------------
// Tick entry points (thin wrappers)
// ---------------------------------------------------------------------------

/// Advance all entities with MovementTarget along their paths.
///
/// Called once per admitted native gameplay frame.
/// Entities that reach their destination have MovementTarget removed automatically.
#[cfg(test)]
pub(crate) fn tick_movement(
    entities: &mut EntityStore,
    interner: &mut crate::sim::intern::StringInterner,
    lifecycle_requests: &mut Vec<LifecycleRequest>,
) {
    let empty_costs: BTreeMap<SpeedType, TerrainCostGrid> = BTreeMap::new();
    let empty_alliances: HouseAllianceMap = HouseAllianceMap::new();
    let mut rng: SimRng = SimRng::new(0);
    let mut empty_occupancy = crate::sim::occupancy::OccupancyGrid::new();
    let _ = tick_movement_with_grid(
        entities,
        None,
        &empty_costs,
        &empty_alliances,
        &mut empty_occupancy,
        &mut rng,
        0, // sim_tick not available in test-only wrapper
        interner,
        lifecycle_requests,
    );
}

/// Advance movement and perform deterministic blocked-cell recovery.
///
/// `terrain_costs` is the per-SpeedType land-row map. When provided, repath
/// attempts use `find_path_with_costs`, which reads it as a passability
/// predicate — retail's search does not prefer roads or avoid rough terrain.
#[cfg(test)]
pub(crate) fn tick_movement_with_grid(
    entities: &mut EntityStore,
    path_grid: Option<&PathGrid>,
    terrain_costs: &BTreeMap<SpeedType, TerrainCostGrid>,
    alliances: &HouseAllianceMap,
    occupancy: &mut crate::sim::occupancy::OccupancyGrid,
    rng: &mut SimRng,
    sim_tick: u64,
    interner: &mut crate::sim::intern::StringInterner,
    lifecycle_requests: &mut Vec<LifecycleRequest>,
) -> MovementTickStats {
    let mut sound_events: Vec<crate::sim::world::SimSoundEvent> = Vec::new();
    let mut next_occupancy_enter_order = crate::sim::world::EnterOrderCounter::new();
    let mut cell_occupation = crate::sim::occupancy::CellOccupationGrid::rebuild(entities);
    let mut raw_cell_occupation = crate::sim::occupancy::RawCellOccupationGrid::new();
    tick_movement_with_grids(
        entities,
        None,
        path_grid,
        terrain_costs,
        alliances,
        occupancy,
        &mut cell_occupation,
        &mut raw_cell_occupation,
        &mut next_occupancy_enter_order,
        rng,
        sim_tick,
        sim_tick as u32, // native-frame proxy (test-only wrapper: 1 frame/tick)
        None,            // No zone grid in legacy wrapper
        None,            // No resolved terrain in legacy wrapper
        None,            // No playfield bounds in legacy wrapper
        &TerrainSpeedConfig::default(),
        SIM_ZERO, // No CloseEnough in legacy wrapper
        9,        // Default PathDelay
        60,       // Default BlockagePathDelay
        interner,
        None, // No RuleSet in legacy wrapper — crush sounds suppressed
        &mut sound_events,
        lifecycle_requests,
    )
}

// ---------------------------------------------------------------------------
// Internal helpers — shared across movement submodules
// ---------------------------------------------------------------------------

/// Returns true if the entity has a within-cell destination it hasn't reached yet.
/// Generic sub-cell arrival for locomotors without a retained Drive/Ship head.
/// The locomotor's `subcell_dest` field stores the target lepton coordinates.
///
/// Takes individual fields to avoid borrow conflicts with `entity.movement_target`.
fn walking_to_subcell_dest(
    locomotor: &Option<crate::sim::movement::locomotor::LocomotorState>,
    sub_x: SimFixed,
    sub_y: SimFixed,
) -> bool {
    let Some(loco) = locomotor else {
        return false;
    };
    // Drive/Ship terminate at their retained raw head. CellArrival's legacy
    // center projection must not start a second generic movement afterward.
    if matches!(
        loco.kind,
        crate::rules::locomotor_type::LocomotorKind::Drive
            | crate::rules::locomotor_type::LocomotorKind::Ship
    ) {
        return false;
    }
    let Some((dest_x, dest_y)) = loco.subcell_dest else {
        return false;
    };
    let threshold: SimFixed = SimFixed::from_num(4);
    (dest_x - sub_x).abs() > threshold || (dest_y - sub_y).abs() > threshold
}

#[cfg(test)]
mod ground_pose_tests;
#[cfg(test)]
mod movement_bridge_retail_tests;
#[cfg(test)]
mod movement_tests;
#[cfg(test)]
mod prone_speed_tests;
