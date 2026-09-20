//! Refinery docking visual sequence — approach, link, unload, depart.
//!
//! Drives the sub-state machine (`RefineryDockPhase`) when the miner is in
//! `MinerState::Dock`. Mirrors the four-state FSM used by the original
//! game's harvester deploy mission (cases 0/1/3/4): approach the queue,
//! link onto the pad, deposit bales, then hand back to harvest scheduling.
//!
//! `refinery_pad_cell` is a thin wrapper over
//! [`crate::sim::docking::pad_geometry::pad_cell_for`] — that helper owns
//! the single building-center-relative lepton→cell conversion shared with
//! aircraft pad descent.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/miner, sim/miner_dock, sim/components,
//!   sim/movement, sim/docking/pad_geometry, rules/.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::BaleDepositEvent;
use crate::sim::miner::{MinerConfig, MinerKind, MinerState, RefineryDockPhase, ResourceType};
use crate::sim::mission::MissionType;
use crate::sim::movement;
use crate::sim::movement::facing_class::FacingClass;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;

use super::miner_dock::{self, ContactAdmission};
use super::miner_system::{MinerSnapshot, effective_purifier_count};
use crate::sim::economy::apply_income_mult;
use crate::sim::house_state::{house_state_for_owner_mut, income_ppm_for_owner};
use crate::sim::production::{credits_entry_for_owner, foundation_dimensions};

/// Maximum diamond-ring radius for the post-unload exit-cell spiral search.
/// gamemd's `FootClass::Find_Nearby_Passable_Cell` derives its cap from
/// `Speed + SightRange` (capped at 32). A miner-class unit lands at ~14.
/// 16 covers the same footprint with a small safety margin and still
/// terminates quickly when the area around the refinery is fully blocked.
pub(super) const EXIT_SEARCH_MAX_RADIUS: i32 = 16;

/// Target body facing on the refinery dock pad — East (0x40 in 8-bit,
/// 0x4000 in 16-bit). The miner's back faces the refinery (which sits west
/// of the dock pad), aligning the dump animation with the open-bay voxel.
/// Verified from gamemd `UnitClass::Receive_Radio` case 0x16 at 0x737430:
/// reads PrimaryFacing rate-timer, calls locomotor `Do_Turn(0x4000)` to
/// rotate toward East. Applies to both HARV and CMIN (no Teleporter gate).
const DOCK_FACING_EAST: u8 = 0x40;
const DOCK_FACING_EAST_DIR: u16 = (DOCK_FACING_EAST as u16) << 8;
const ENTER_RETRY_BASE_FRAMES: u8 = 14;
const ENTER_RETRY_JITTER_MAX_FRAMES: u32 = 2;
/// Keyless-`[Harvest]` fallback for the approach re-HELLO cadence (U5); the
/// stock `Rate=.016` resolves to 14 from the table, so this is only reached
/// when a mod strips the section.
const APPROACH_HELLO_BASE_FRAMES: u8 = 14;
const MISSION_DEPLOY_FACING_WAIT_FRAMES: u8 = 5;
const MISSION_DEPLOY_UNLOAD_BASE_FRAMES: u8 = 14;
const MISSION_DEPLOY_UNLOAD_JITTER_MAX_FRAMES: u32 = 2;

/// Helper: record a dock phase transition to the snapshot's debug buffer.
fn record_dock_phase(snap: &mut MinerSnapshot, old: RefineryDockPhase, new: RefineryDockPhase) {
    snap.debug_dock_events
        .push((format!("{:?}", old), format!("{:?}", new)));
}

fn facing8_to_dir16(facing: u8) -> u16 {
    (facing as u16) << 8
}

fn dock_pivot_accepts(dir: u16) -> bool {
    ((((dir as u32) >> 7) + 1) & 0x1FE) == 0x80
}

fn dock_pivot_rot_byte(sim: &Simulation, rules: &RuleSet, snap: &MinerSnapshot) -> u8 {
    rules
        .object_case_insensitive(sim.interner.resolve(snap.type_id))
        .map(|obj| obj.turret_rot.clamp(0, 0xFF) as u8)
        .unwrap_or(10)
}

/// Base cadence (frames) for a mission's miner-path timer, sourced from the
/// parsed `[<Mission>] Rate` (ftol frames). Falls back to `fallback` only for a
/// keyless mission (U5 guard); clamps into the `u8` timer-seed domain. The stock
/// `[Enter]/[Unload]/[Harvest] Rate=.016` all resolve to 14, so wiring the table
/// leaves the stock cadence byte-identical.
pub(super) fn mission_base_frames(rules: &RuleSet, mission: MissionType, fallback: u8) -> u8 {
    let frames = rules.mission_control.rate_frames(mission);
    if frames == 0 {
        fallback
    } else {
        frames.min(u8::MAX as u32) as u8
    }
}

pub(super) fn schedule_enter_retry(
    sim: &mut Simulation,
    rules: &RuleSet,
    snap: &mut MinerSnapshot,
) {
    // gamemd computes the base `ftol(Rate*900)` FIRST, then draws RandomRanged(0,2),
    // then adds — the base lookup consumes no RNG, so the stream order is preserved.
    let base = mission_base_frames(rules, MissionType::Enter, ENTER_RETRY_BASE_FRAMES);
    let jitter = sim
        .miner_jitter_rng()
        .next_range_u32_inclusive(0, ENTER_RETRY_JITTER_MAX_FRAMES) as u8;
    let duration = u32::from(base.saturating_add(jitter));
    snap.miner
        .dock_enter_retry
        .arm(sim.session.binary_frame, duration);
}

/// Arm the approach re-HELLO gate for one Harvest-mission cadence window
/// (`ftol([Harvest] Rate*900) + RandomRanged(0,2)`), mirroring the harvest
/// epilogue. The jitter is the same `Scen->Random` `RandomRanged(0,2)` used by
/// every mission-cadence epilogue.
fn schedule_approach_hello(sim: &mut Simulation, rules: &RuleSet, snap: &mut MinerSnapshot) {
    let base = mission_base_frames(rules, MissionType::Harvest, APPROACH_HELLO_BASE_FRAMES);
    let jitter = sim
        .miner_jitter_rng()
        .next_range_u32_inclusive(0, ENTER_RETRY_JITTER_MAX_FRAMES) as u8;
    let duration = u32::from(base.saturating_add(jitter));
    snap.miner
        .approach_hello_timer
        .arm(sim.session.binary_frame, duration);
}

fn enter_retry_due(sim: &Simulation, snap: &MinerSnapshot) -> bool {
    snap.miner.dock_enter_retry.due(sim.session.binary_frame)
}

fn clear_enter_retry(snap: &mut MinerSnapshot) {
    snap.miner.dock_enter_retry.clear();
}

fn schedule_mission_deploy_delay(snap: &mut MinerSnapshot, frame: u32, duration: u8) {
    snap.miner
        .mission_deploy_timer
        .arm(frame, u32::from(duration));
}

fn mission_deploy_due(sim: &Simulation, snap: &MinerSnapshot) -> bool {
    snap.miner
        .mission_deploy_timer
        .due(sim.session.binary_frame)
}

fn clear_mission_deploy_delay(snap: &mut MinerSnapshot) {
    snap.miner.mission_deploy_timer.clear();
}

fn clear_unload_timer_cluster(snap: &mut MinerSnapshot) {
    snap.miner.unload_accumulator = 0;
    snap.miner.unload_timer_fired = false;
    snap.miner.unload_cluster_timer.clear();
    snap.miner.unload_cluster_scratch = 0;
    snap.miner.unload_cluster_repeat = 0;
}

fn clear_unload_cluster(snap: &mut MinerSnapshot) {
    snap.miner.unload_active = false;
    clear_unload_timer_cluster(snap);
}

fn tick_unload_accumulator(sim: &Simulation, snap: &mut MinerSnapshot) {
    // An unarmed timer means the cluster is inactive (was `start_frame == None`).
    if !snap.miner.unload_cluster_timer.is_armed() {
        snap.miner.unload_timer_fired = false;
        return;
    }
    if snap.miner.unload_cluster_repeat == 0 {
        snap.miner.unload_timer_fired = false;
        return;
    }
    if !snap
        .miner
        .unload_cluster_timer
        .due(sim.session.binary_frame)
    {
        snap.miner.unload_timer_fired = false;
        return;
    }

    snap.miner.unload_accumulator = snap
        .miner
        .unload_accumulator
        .saturating_add(snap.miner.unload_accumulator_step);
    snap.miner.unload_timer_fired = true;
    snap.miner
        .unload_cluster_timer
        .arm(sim.session.binary_frame, snap.miner.unload_cluster_repeat);
    snap.miner.unload_cluster_scratch = 0;
}

// ---------------------------------------------------------------------------
// Cell computation helpers
// ---------------------------------------------------------------------------

/// Queue cell — where the miner waits outside the refinery (pathfindable).
///
/// Uses art.ini `QueueingCell=` when available (merged into ObjectType),
/// otherwise falls back to geometric approximation from foundation dimensions.
pub(super) fn refinery_queue_cell(
    rx: u16,
    ry: u16,
    width: u16,
    height: u16,
    queueing_cell: Option<(u16, u16)>,
) -> (u16, u16) {
    if let Some((qx, qy)) = queueing_cell {
        (rx + qx, ry + qy)
    } else {
        (rx + width, ry + height / 2)
    }
}

/// CAN_DOCK queue target sent by `BuildingClass::Receive_Radio` case 0x0E.
///
/// Verified in gamemd: this path hardcodes building anchor + (3, 1) and does
/// not read art.ini `QueueingCell=`.
pub(super) fn refinery_can_dock_queue_cell(rx: u16, ry: u16) -> (u16, u16) {
    (rx.saturating_add(3), ry.saturating_add(1))
}

/// Pad cell — on the refinery platform inside the building footprint.
///
/// When art.ini declares a `DockingOffset0` (passed through as `docking_offset`),
/// delegates to [`crate::sim::docking::pad_geometry::pad_cell_for`] for the
/// shared building-center-relative lepton→cell conversion. Otherwise falls back
/// to the stock refinery pad opened by the live building object-list scan:
/// `(+3 cells, +1 cell)` from the NW corner.
pub(super) fn refinery_pad_cell(
    rx: u16,
    ry: u16,
    width: u16,
    height: u16,
    docking_offset: Option<(i32, i32, i32)>,
) -> (u16, u16) {
    if let Some((dx, dy, dz)) = docking_offset {
        let pad = crate::rules::object_type::DockPad {
            lepton_offset: (dx, dy, dz),
        };
        crate::sim::docking::pad_geometry::pad_cell_for((rx, ry), (width, height), &pad)
    } else {
        let _ = (width, height);
        (rx.saturating_add(3), ry.saturating_add(1))
    }
}

/// Conditional reciprocal-link exit cell.
///
/// Stock zero-link refinery unload completion does not use this helper:
/// `UnitClass::Mission_Deploy_Building` state 4 queues/continues Harvest
/// without installing a new passable-cell destination. This remains for
/// conditional reciprocal-link/interrupt modelling and for legacy tests
/// that pin the old helper's geometry.
///
/// `find_nearby_passable_cell_with_index` provides a fallback when
/// the queue cell is blocked (e.g., another miner already waiting
/// there): ring 1+ picks an adjacent cell, typically still east of
/// the foundation. Falls back to the art.ini `QueueingCell`
/// (or the geometric default from [`refinery_queue_cell`]) when no
/// passable cell exists within [`EXIT_SEARCH_MAX_RADIUS`] or no path
/// grid is available.
///
#[cfg(test)]
pub(super) fn refinery_exit_cell(
    rx: u16,
    ry: u16,
    width: u16,
    height: u16,
    queueing_cell: Option<(u16, u16)>,
    path_grid: Option<&PathGrid>,
    occupancy: Option<&OccupancyGrid>,
    tick: u64,
) -> (u16, u16) {
    let queue = refinery_queue_cell(rx, ry, width, height, queueing_cell);

    if let Some(grid) = path_grid {
        if let Some(cell) = find_nearby_passable_cell_with_index(
            queue.0 as i32,
            queue.1 as i32,
            grid,
            occupancy,
            EXIT_SEARCH_MAX_RADIUS,
            tick,
        ) {
            return cell;
        }
    }

    queue
}

/// Whether `(x, y)` is in-bounds, passable on the ground layer, and not
/// occupied by any other ground-layer entity.
///
/// Bridge layer is intentionally ignored: refinery exit cells must drop the
/// miner on land. `occupancy` may be `None` when only path passability is
/// known (used by direct unit tests of the spiral algorithm).
fn is_exit_cell_passable(
    x: i32,
    y: i32,
    grid: &PathGrid,
    occupancy: Option<&OccupancyGrid>,
) -> bool {
    if x < 0 || y < 0 || x >= grid.width() as i32 || y >= grid.height() as i32 {
        return false;
    }
    let cx = x as u16;
    let cy = y as u16;
    if !grid.is_walkable(cx, cy) {
        return false;
    }
    if let Some(occ) = occupancy {
        if !occ.is_empty_on_layer(cx, cy, MovementLayer::Ground) {
            return false;
        }
    }
    true
}

/// Diamond-ring spiral that collects ALL passable cells in the first
/// non-empty ring, mirroring gamemd's `FootClass::Find_Nearby_Passable_Cell`
/// (0x56DC20) candidate-collection block. The original engine then picks
/// from the collected pool via `g_CurrentFrameCounter % count` — caller
/// supplies the equivalent index.
///
/// Why this matters: a deterministic "return first walkable" picks the
/// same cell every time, which is visually fine when the anchor itself is
/// passable, but produces "miner always exits at the same spot" drift when
/// a conditional release anchor is blocked and several ring-1 candidates
/// exist.
/// The modulo selection over the ring's candidates spreads exits across
/// the available cells the way gamemd does.
///
/// Returns the chosen cell, or `None` if no ring within `max_radius`
/// produces a passable cell. The selection is `candidates[index % count]`,
/// matching gamemd's modulo-based pick.
pub(crate) fn find_nearby_passable_cell_with_index(
    ox: i32,
    oy: i32,
    grid: &PathGrid,
    occupancy: Option<&OccupancyGrid>,
    max_radius: i32,
    index: u64,
) -> Option<(u16, u16)> {
    // Ring 0: the anchor itself. If passable, it's the sole candidate.
    if is_exit_cell_passable(ox, oy, grid, occupancy) {
        return Some((ox as u16, oy as u16));
    }
    let mut candidates: Vec<(u16, u16)> = Vec::with_capacity(24);
    for r in 1..=max_radius {
        candidates.clear();
        // Segment 1: top + bottom rows.
        for delta in -r..=r {
            if is_exit_cell_passable(ox + delta, oy - r, grid, occupancy) {
                candidates.push(((ox + delta) as u16, (oy - r) as u16));
            }
            if is_exit_cell_passable(ox + delta, oy + r, grid, occupancy) {
                candidates.push(((ox + delta) as u16, (oy + r) as u16));
            }
        }
        // Segment 2: left + right columns (corners already covered by segment 1).
        for delta in (1 - r)..=(r - 1) {
            if is_exit_cell_passable(ox - r, oy + delta, grid, occupancy) {
                candidates.push(((ox - r) as u16, (oy + delta) as u16));
            }
            if is_exit_cell_passable(ox + r, oy + delta, grid, occupancy) {
                candidates.push(((ox + r) as u16, (oy + delta) as u16));
            }
        }
        if !candidates.is_empty() {
            // gamemd's `local_60[g_CurrentFrameCounter % count]` selection.
            let pick = (index as usize) % candidates.len();
            return Some(candidates[pick]);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Refinery lookup helpers
// ---------------------------------------------------------------------------

/// Resolve a refinery entity's foundation and compute stock dock cells.
/// Returns `(wait_queue, accepted_cell, pad, dock_capacity)` or `None` if the
/// refinery is gone.
fn resolve_refinery_cells(
    sim: &Simulation,
    rules: &RuleSet,
    ref_sid: u64,
) -> Option<((u16, u16), (u16, u16), (u16, u16), usize)> {
    let entity = sim.substrate.entities.get(ref_sid)?;
    if entity.dying || entity.health.current == 0 {
        return None;
    }
    let obj = sim.object_type(entity.type_ref(), rules);
    let (w, h) = obj
        .map(|o| foundation_dimensions(&o.foundation))
        .unwrap_or((1, 1));
    let qc = obj.and_then(|o| o.queueing_cell);
    let dock_off = obj.and_then(|o| o.pads.first().map(|p| p.lepton_offset));
    let dock_capacity = obj.map(|o| o.dock_contact_capacity() as usize).unwrap_or(1);
    let rx = entity.position.rx;
    let ry = entity.position.ry;
    let wait_queue = refinery_queue_cell(rx, ry, w, h, qc);
    Some((
        wait_queue,
        refinery_can_dock_queue_cell(rx, ry),
        refinery_pad_cell(rx, ry, w, h, dock_off),
        dock_capacity,
    ))
}

/// Look up the UnloadingClass for a miner type from rules.ini.
fn unloading_class(rules: &RuleSet, type_id: &str) -> Option<String> {
    rules
        .object_case_insensitive(type_id)
        .and_then(|obj| obj.unloading_class.clone())
}

fn mission_deploy_unload_building(sim: &Simulation, miner_id: u64) -> Option<u64> {
    let miner = sim.substrate.entities.get(miner_id)?;
    if miner.position.rx == 0 {
        return None;
    }
    let lookup_rx = miner.position.rx - 1;
    let lookup_ry = miner.position.ry;
    let layer = miner
        .occupancy_list_layer()
        .unwrap_or(MovementLayer::Ground);
    sim.substrate
        .occupancy
        .get(lookup_rx, lookup_ry)?
        .iter_layer(layer)
        .find_map(|occupant| {
            let entity = sim.substrate.entities.get(occupant.entity_id)?;
            if entity.category == EntityCategory::Structure
                && !entity.dying
                && entity.health.current > 0
            {
                Some(entity.stable_id())
            } else {
                None
            }
        })
}

fn dock_abort_state(snap: &MinerSnapshot) -> MinerState {
    if snap.miner.forced_return && !snap.miner.cargo.is_empty() {
        MinerState::ForcedReturn
    } else if snap.miner.is_full() {
        MinerState::ReturnToRefinery
    } else {
        MinerState::SearchOre
    }
}

fn dock_abort_state_from_miner(miner: &super::Miner) -> MinerState {
    if miner.forced_return && !miner.cargo.is_empty() {
        MinerState::ForcedReturn
    } else if miner.is_full() {
        MinerState::ReturnToRefinery
    } else {
        MinerState::SearchOre
    }
}

/// VERA's current refinery-loss adapter: release contacts/reservations and
/// reset the miner cursor/timers, preserving cargo and locomotor state.
/// Returns the number of miners whose adapter state was cleared.
///
/// Native Sell44AAA4 and ReceiveDamage4424A2 call release4593A0 only through
/// the reciprocal bunker link (+2E4), whose producer is gated by Bunker at
/// 44B797..44B7A3. Refinery contacts are not that link and cannot
/// authorize Force_Track(0x47) or SetSpeedFraction(1). Native sale's radio0x17
/// receiver737A98 and death's later contact-loss dispatch73DEE0 have distinct
/// mission/scatter timing. This eager shared reset remains an unfinished
/// VERA adapter; it does not implement either complete native sequence.
pub(crate) fn interrupt_refinery_docked_miners(sim: &mut Simulation, ref_sid: u64) -> usize {
    // The refinery's own contact slots name every miner it admitted, so no
    // world scan is needed. Ascending id keeps the former visiting order.
    let mut candidates: Vec<u64> = sim
        .substrate
        .entities
        .get(ref_sid)
        .map(|refinery| refinery.radio_contacts.iter_live().collect())
        .unwrap_or_default();
    candidates.sort_unstable();
    candidates.retain(|&entity_id| {
        sim.substrate.entities.get(entity_id).is_some_and(|entity| {
            entity.miner_state() == Some(MinerState::Dock)
                && entity
                    .miner
                    .as_ref()
                    .is_some_and(|miner| miner.reserved_refinery == Some(ref_sid))
        })
    });

    let mut interrupted = 0;
    for entity_id in candidates {
        miner_dock::break_contact(sim, entity_id, ref_sid);
        let Some(entity) = sim.substrate.entities.get_mut(entity_id) else {
            continue;
        };
        let Some(miner) = entity.miner.as_mut() else {
            continue;
        };
        let next_state = dock_abort_state_from_miner(miner);
        miner.reserved_refinery = None;
        miner.dock_queued = false;
        miner.dock_phase = RefineryDockPhase::Approach;
        miner.dock_pivot_facing = None;
        miner.dock_enter_retry.clear();
        miner.mission_deploy_timer.clear();
        miner.unload_active = false;
        miner.unload_accumulator = 0;
        miner.unload_timer_fired = false;
        miner.unload_cluster_timer.clear();
        miner.unload_cluster_scratch = 0;
        miner.unload_cluster_repeat = 0;
        miner.exit_cell = None;
        if miner.is_full() {
            miner.target_ore_cell = None;
        }
        entity.mission.set_handler_state(next_state.cursor());

        entity.display_type_override = None;
        entity.movement_target = None;
        interrupted += 1;
    }
    interrupted
}

fn abort_invalid_refinery(sim: &mut Simulation, snap: &mut MinerSnapshot, ref_sid: Option<u64>) {
    if let Some(ref_sid) = ref_sid {
        miner_dock::break_contact(sim, snap.entity_id, ref_sid);
    }

    sim.cancel_drive_track(snap.entity_id);
    if let Some(entity) = sim.substrate.entities.get_mut(snap.entity_id) {
        entity.display_type_override = None;
        entity.facing_target = None;
        entity.movement_target = None;
    }

    snap.miner.reserved_refinery = None;
    snap.miner.dock_queued = false;
    snap.miner.dock_phase = RefineryDockPhase::Approach;
    snap.miner.dock_pivot_facing = None;
    clear_enter_retry(snap);
    clear_mission_deploy_delay(snap);
    clear_unload_cluster(snap);
    snap.miner.exit_cell = None;
    if snap.miner.is_full() {
        snap.miner.target_ore_cell = None;
    }
    snap.state = dock_abort_state(snap);
}

fn abort_missing_unload_building(sim: &mut Simulation, snap: &mut MinerSnapshot, ref_sid: u64) {
    miner_dock::break_contact(sim, snap.entity_id, ref_sid);

    sim.cancel_drive_track(snap.entity_id);
    if let Some(entity) = sim.substrate.entities.get_mut(snap.entity_id) {
        entity.facing_target = None;
        entity.movement_target = None;
        snap.rx = entity.position.rx;
        snap.ry = entity.position.ry;
    }

    snap.miner.reserved_refinery = None;
    snap.miner.dock_queued = false;
    snap.miner.dock_phase = RefineryDockPhase::Approach;
    snap.miner.dock_pivot_facing = None;
    clear_enter_retry(snap);
    clear_mission_deploy_delay(snap);
    clear_unload_timer_cluster(snap);
    snap.miner.exit_cell = None;
    if snap.miner.is_full() {
        snap.miner.target_ore_cell = None;
    }
    snap.state = dock_abort_state(snap);
}

/// Contact-gone abandonment of an unload, `Mission_Unload @ 0x0073DEEB..0x0073DF55`.
///
/// Native side effects, in order:
/// - `vtable+0x484 (0,1)` = `UnitClass::Enter_Idle_Mode @ 0x00738970`. With the
///   CURRENT mission still Unload (`+0xAC == 0x10`) its final
///   `Assign_Mission` is skipped (`0x00738D0x` gate excludes 0x19/0xB/0x10/9),
///   and the harvester branch bails on `In_Radio_Contact` anyway; the
///   `FootClass` base (`0x004D82B0`) only drops target/destination. No mission
///   is assigned here — NOT a Harvest resume.
/// - `+0x6D1 = 0`: the unload-active latch (and with it the UnloadingClass
///   image) is cleared.
/// - locomotor `Is_Moving` → `vtable+0x500` (`0x004D55C0` → locomotor
///   `Stop_Moving`).
/// - `Is_Ready_To_Commence` → `Commence`: only an already QUEUED mission is
///   promoted (fieldwise no-op on an empty queue).
/// - `return 1`: the unit stays in Unload and re-dispatches next frame, repeating
///   this until something queues a mission for it.
///
/// Rust keeps the miner parked in `Pivoting` with the latch cleared, re-checking
/// every frame; a promoted queued mission leaves the dock sequence through the
/// same zeroed handler cursor the state-4 exit uses, and releases the miner's
/// live contact and radio-bus slot exactly as that exit does.
///
/// Native runs the `In_Radio_Contact` check at `0x0073DEE7` BEFORE the `+0xBC`
/// state dispatch, so state 3 (dumping) abandons the unload on contact loss
/// as well; `phase_unloading` asks on each of its due dispatches too. The
/// trigger is a player retask: `miner_dock::break_for_retask` BREAKs the
/// contact of a miner in an unload phase and leaves the phase alone so that
/// this path, not a phase reset, ends the unload.
///
/// Residual: `Departing` does not re-check. It BREAKs the contact itself on
/// its first dispatch, so a retask there only repeats that BREAK.
fn abort_unload_contact_lost(sim: &mut Simulation, snap: &mut MinerSnapshot, ref_sid: u64) {
    sim.cancel_drive_track(snap.entity_id);
    if let Some(entity) = sim.substrate.entities.get_mut(snap.entity_id) {
        // Enter_Idle_Mode base: drop target/destination; +0x500: Stop_Moving.
        entity.facing_target = None;
        entity.movement_target = None;
        // +0x6D1 = 0 drops the UnloadingClass image with the latch.
        entity.display_type_override = None;
    }
    snap.miner.dock_pivot_facing = None;
    clear_unload_cluster(snap);

    let now = sim.session.binary_frame;
    let commenced = matches!(sim.mission_commence_exact(snap.entity_id, now), Ok(true));
    if commenced {
        // Commence zeroed the handler cursor (== SearchOre); the dock
        // handshake goes with it, as on the state-4 exit: BREAK both ends.
        miner_dock::break_contact(sim, snap.entity_id, ref_sid);
        snap.miner.reserved_refinery = None;
        snap.miner.dock_queued = false;
        snap.miner.dock_phase = RefineryDockPhase::Approach;
        clear_enter_retry(snap);
        clear_mission_deploy_delay(snap);
        snap.miner.exit_cell = None;
        if snap.miner.is_full() {
            snap.miner.target_ore_cell = None;
        }
        snap.state = MinerState::SearchOre;
        return;
    }
    // `return 1`: re-dispatch next frame, still in the Unload-equivalent.
    schedule_mission_deploy_delay(snap, now, 1);
    snap.miner.dock_phase = RefineryDockPhase::Pivoting;
}

// ---------------------------------------------------------------------------
// Main dock sequence handler
// ---------------------------------------------------------------------------

/// Process one tick of the refinery docking sequence for a single miner.
pub(super) fn handle_dock_sequence(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
) {
    let phase_before = snap.miner.dock_phase;

    let Some(ref_sid) = snap.miner.reserved_refinery else {
        abort_invalid_refinery(sim, snap, None);
        if phase_before != snap.miner.dock_phase {
            record_dock_phase(snap, phase_before, snap.miner.dock_phase);
        }
        return;
    };

    // Only before admission: a miner the refinery already admitted keeps its
    // handshake across an owner change (a mind-controlled miner still unloads
    // into the refinery owner's account).
    if !miner_dock::has_contact(sim, ref_sid, snap.entity_id)
        && !miner_dock::same_house(sim, ref_sid, snap.entity_id)
    {
        abort_invalid_refinery(sim, snap, Some(ref_sid));
        if phase_before != snap.miner.dock_phase {
            record_dock_phase(snap, phase_before, snap.miner.dock_phase);
        }
        return;
    }

    match snap.miner.dock_phase {
        RefineryDockPhase::Approach => {
            let Some((wait_queue, _accepted_cell, _pad, dock_capacity)) =
                resolve_refinery_cells(sim, rules, ref_sid)
            else {
                abort_invalid_refinery(sim, snap, Some(ref_sid));
                if phase_before != snap.miner.dock_phase {
                    record_dock_phase(snap, phase_before, snap.miner.dock_phase);
                }
                return;
            };
            phase_approach(
                sim,
                rules,
                path_grid,
                overlay_registry,
                snap,
                wait_queue,
                ref_sid,
                dock_capacity,
            );
        }
        RefineryDockPhase::MissionEnter => {
            let Some((wait_queue, accepted_cell, _pad, dock_capacity)) =
                resolve_refinery_cells(sim, rules, ref_sid)
            else {
                abort_invalid_refinery(sim, snap, Some(ref_sid));
                if phase_before != snap.miner.dock_phase {
                    record_dock_phase(snap, phase_before, snap.miner.dock_phase);
                }
                return;
            };
            phase_mission_enter(
                sim,
                rules,
                path_grid,
                overlay_registry,
                snap,
                wait_queue,
                accepted_cell,
                ref_sid,
                dock_capacity,
            );
        }
        RefineryDockPhase::AwaitingAcceptedCell => {
            phase_awaiting_accepted_cell(sim, snap);
        }
        RefineryDockPhase::FaceSync => {
            if resolve_refinery_cells(sim, rules, ref_sid).is_none() {
                abort_invalid_refinery(sim, snap, Some(ref_sid));
                if phase_before != snap.miner.dock_phase {
                    record_dock_phase(snap, phase_before, snap.miner.dock_phase);
                }
                return;
            }
            phase_face_sync(sim, rules, snap, ref_sid);
        }
        RefineryDockPhase::MissionQueued => {
            phase_mission_queued(snap);
        }
        RefineryDockPhase::Pivoting => {
            if resolve_refinery_cells(sim, rules, ref_sid).is_none() {
                abort_invalid_refinery(sim, snap, Some(ref_sid));
                if phase_before != snap.miner.dock_phase {
                    record_dock_phase(snap, phase_before, snap.miner.dock_phase);
                }
                return;
            }
            phase_pivoting(sim, rules, config, snap, ref_sid);
        }
        RefineryDockPhase::Unloading => {
            phase_unloading(sim, rules, config, snap, ref_sid);
        }
        RefineryDockPhase::DepositCooldown => {
            phase_deposit_cooldown(snap);
        }
        RefineryDockPhase::Departing => {
            phase_departing(sim, rules, snap, ref_sid);
        }
    }

    tick_unload_accumulator(sim, snap);

    if phase_before != snap.miner.dock_phase {
        record_dock_phase(snap, phase_before, snap.miner.dock_phase);
    }
}

// ---------------------------------------------------------------------------
// Phase handlers
// ---------------------------------------------------------------------------

fn phase_approach(
    sim: &mut Simulation,
    rules: &RuleSet,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
    wait_queue: (u16, u16),
    ref_sid: u64,
    dock_capacity: usize,
) {
    // Mission_Harvest state 2 sends only HELLO(0x02), one dispatch per Harvest
    // mission cadence (~14-16f) — not every sim tick. Gate the re-HELLO on the
    // per-miner harvest-cadence timer; the drive toward the queue cell stays
    // ungated (ObjectClass::AI runs every tick before the mission timer).
    if !snap
        .miner
        .approach_hello_timer
        .due(sim.session.binary_frame)
    {
        if !is_adjacent_or_at((snap.rx, snap.ry), wait_queue) {
            if let Some(grid) = path_grid {
                issue_move_if_idle(
                    sim,
                    rules,
                    grid,
                    snap.entity_id,
                    wait_queue,
                    snap.speed,
                    overlay_registry,
                );
            }
        }
        return;
    }

    // On ROGER it queues Mission_Enter for the next dispatch instead of jumping
    // straight to the accepted-cell move or unload pivot.
    let admission = miner_dock::hello(sim, snap.entity_id, ref_sid, dock_capacity);

    snap.miner.dock_queued = admission != ContactAdmission::Accepted;
    if admission == ContactAdmission::Accepted {
        // G5: the accepted HELLO queues Mission_Enter with the mission-epilogue
        // cadence; the first CAN_DOCK waits ~14-16f (arm), it does not collapse
        // to an always-due next-tick dispatch.
        schedule_enter_retry(sim, rules, snap);
        snap.miner.dock_phase = RefineryDockPhase::MissionEnter;
        return;
    }

    // Not accepted: re-arm the Harvest re-HELLO cadence (one HELLO per due
    // window) and keep heading toward QueueingCell.
    schedule_approach_hello(sim, rules, snap);
    if !is_adjacent_or_at((snap.rx, snap.ry), wait_queue) {
        if let Some(grid) = path_grid {
            issue_move_if_idle(
                sim,
                rules,
                grid,
                snap.entity_id,
                wait_queue,
                snap.speed,
                overlay_registry,
            );
        }
    }
}

fn phase_mission_enter(
    sim: &mut Simulation,
    rules: &RuleSet,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
    wait_queue: (u16, u16),
    accepted_cell: (u16, u16),
    ref_sid: u64,
    dock_capacity: usize,
) {
    if !enter_retry_due(sim, snap) {
        return;
    }

    let admission = miner_dock::hello(sim, snap.entity_id, ref_sid, dock_capacity);
    let already_entered = miner_dock::has_entered(sim, ref_sid, snap.entity_id);
    if admission != ContactAdmission::Accepted && !already_entered {
        snap.miner.dock_queued = true;
        if !is_adjacent_or_at((snap.rx, snap.ry), wait_queue) {
            if let Some(grid) = path_grid {
                issue_move_if_idle(
                    sim,
                    rules,
                    grid,
                    snap.entity_id,
                    wait_queue,
                    snap.speed,
                    overlay_registry,
                );
            }
        }
        schedule_enter_retry(sim, rules, snap);
        return;
    }

    // Past the early return above the miner holds the contact or has already
    // entered, so the 0x18 handshake may start as soon as it stands on the
    // accepted cell.
    snap.miner.dock_queued = false;

    let moving = sim
        .substrate
        .entities
        .get(snap.entity_id)
        .is_some_and(|e| e.movement_target.is_some());

    if (snap.rx, snap.ry) == accepted_cell && !moving {
        miner_dock::enter_dock(sim, snap.entity_id, ref_sid);
        sync_dock_facing(sim, rules, snap);
        snap.miner.dock_phase = RefineryDockPhase::FaceSync;
        schedule_enter_retry(sim, rules, snap);
        return;
    }

    // A war miner now reaches Mission_Enter from up to `HarvesterTooFarDistance`
    // (5 cells) out — the state-2 HELLO handoff at `0x0073EE51` — not from
    // adjacency. Native Mission_Enter drives to the CAN_DOCK cell through the
    // ordinary pathfinder; VERA's direct move below is a straight line that
    // ignores the grid, which is only safe from a neighbouring cell. Path to
    // the `QueueingCell` first (it sits beside the pad on stock refineries)
    // and take the direct step from there. VERA-internal shape, gamemd
    // equivalent UNCHECKED beyond "the pathfinder gets it there". Chrono keeps
    // its existing shape: its inbound leg is the teleport locomotor's.
    //
    // Residual (VERA-internal): the hop assumes the `QueueingCell` is
    // adjacent to the accepted CAN_DOCK cell, which holds for every stock
    // refinery (art `QueueingCell=4,1` beside the pad). A modded refinery
    // whose `QueueingCell` is NOT adjacent to the pad loops here: the miner
    // reaches the queue cell, is still not adjacent to `accepted_cell`, and
    // is routed back to the queue cell on every enter retry, never taking
    // the direct step. Trigger: modded art only; stock play never hits it.
    // Effect: that miner never docks. Fix belongs with the native
    // pathfinder-driven Mission_Enter drive, not here.
    if !moving
        && snap.miner.kind == MinerKind::War
        && !is_adjacent_or_at((snap.rx, snap.ry), accepted_cell)
    {
        if let Some(grid) = path_grid {
            issue_move_if_idle(
                sim,
                rules,
                grid,
                snap.entity_id,
                wait_queue,
                snap.speed,
                overlay_registry,
            );
        }
        schedule_enter_retry(sim, rules, snap);
        return;
    }

    if !moving {
        // Building 0x0E sends 0x12 with anchor+(3,1). The accepted cell is
        // inside the refinery footprint for stock GAREFN/NAREFN, so use the
        // direct move path already used for refinery pad entry.
        let timing =
            movement::DestinationTiming::from_rules(sim.session.binary_frame, rules.into());
        if movement::issue_direct_move(
            &mut sim.substrate.entities,
            snap.entity_id,
            accepted_cell,
            snap.speed,
            timing,
        ) {
            if let Some(target) = sim
                .substrate
                .entities
                .get_mut(snap.entity_id)
                .and_then(|entity| entity.movement_target.as_mut())
            {
                target.bypass_grid = true;
            }
        }
    }
    schedule_enter_retry(sim, rules, snap);
    snap.miner.dock_phase = RefineryDockPhase::AwaitingAcceptedCell;
}

fn phase_awaiting_accepted_cell(sim: &mut Simulation, snap: &mut MinerSnapshot) {
    let moving = sim
        .substrate
        .entities
        .get(snap.entity_id)
        .is_some_and(|e| e.movement_target.is_some());
    if moving {
        return;
    }

    if let Some(entity) = sim.substrate.entities.get(snap.entity_id) {
        snap.rx = entity.position.rx;
        snap.ry = entity.position.ry;
    }

    // Movement completion is not enough to start the dock pivot. The next
    // Mission_Enter/CAN_DOCK pass must receive 0x12 as already-there before
    // the building emits the 0x18/0x16 entered/pivot handshake.
    snap.miner.dock_phase = RefineryDockPhase::MissionEnter;
}

fn phase_face_sync(sim: &mut Simulation, rules: &RuleSet, snap: &mut MinerSnapshot, ref_sid: u64) {
    let arrived = sim
        .substrate
        .entities
        .get(snap.entity_id)
        .is_some_and(|e| e.movement_target.is_none());
    if !arrived {
        return;
    }

    if let Some(entity) = sim.substrate.entities.get(snap.entity_id) {
        snap.rx = entity.position.rx;
        snap.ry = entity.position.ry;
    }

    let accepted = sync_dock_facing(sim, rules, snap);
    if !enter_retry_due(sim, snap) {
        return;
    }

    // L20: EnterDock(0x18) is one-per-0x0E-pass — send it only on a due Enter
    // dispatch, not every arrived tick (which spammed ~14-16 duplicates per
    // wait window). The per-tick facing interpolation (sync_dock_facing)
    // stays ungated above; the miner already carries the entered flag from the
    // Mission_Enter handshake that moved it into this phase.
    miner_dock::enter_dock(sim, snap.entity_id, ref_sid);

    if accepted {
        // L9: the accepted 0x16->0x15 handoff is still a Mission_Enter dispatch
        // in gamemd and draws one RandomRanged(0,2) (Scen->Random) in its
        // epilogue before it hands off to the queued mission. Draw it here to
        // keep the RNG stream aligned; the value is consumed by the epilogue
        // (this handoff clears the retry rather than re-arming, so it only
        // advances the stream by one).
        let _ = sim
            .miner_jitter_rng()
            .next_range_u32_inclusive(0, ENTER_RETRY_JITTER_MAX_FRAMES);
        clear_enter_retry(snap);
        snap.miner.dock_phase = RefineryDockPhase::MissionQueued;
    } else {
        schedule_enter_retry(sim, rules, snap);
    }
}

fn phase_mission_queued(snap: &mut MinerSnapshot) {
    snap.miner.dock_phase = RefineryDockPhase::Pivoting;
}

fn sync_dock_facing(sim: &mut Simulation, rules: &RuleSet, snap: &mut MinerSnapshot) -> bool {
    let rot = dock_pivot_rot_byte(sim, rules, snap);
    let binary_frame = sim.session.binary_frame;
    let Some(entity) = sim.substrate.entities.get(snap.entity_id) else {
        return false;
    };

    if snap.miner.dock_pivot_facing.is_none() {
        let mut pivot = FacingClass::new(facing8_to_dir16(entity.facing), rot);
        pivot.set(DOCK_FACING_EAST_DIR, binary_frame);
        snap.miner.dock_pivot_facing = Some(pivot);
    }

    let pivot = snap
        .miner
        .dock_pivot_facing
        .as_mut()
        .expect("pivot timer initialized");
    pivot.set_rot(rot);
    pivot.set(DOCK_FACING_EAST_DIR, binary_frame);

    let current_dir = pivot.current(binary_frame);
    dock_pivot_accepts(current_dir)
}

fn start_unload_deploy(sim: &mut Simulation, rules: &RuleSet, snap: &mut MinerSnapshot) {
    if let Some(entity) = sim.substrate.entities.get_mut(snap.entity_id) {
        if let Some(uc) = unloading_class(rules, sim.interner.resolve(snap.type_id)) {
            entity.display_type_override = Some(sim.interner.intern(&uc));
        }
        entity.facing_target = None;
    }

    snap.miner.unload_active = true;
    snap.miner.unload_accumulator = 0;
    snap.miner.unload_timer_fired = false;
    snap.miner
        .unload_cluster_timer
        .arm(sim.session.binary_frame, 1);
    snap.miner.unload_cluster_scratch = 0;
    snap.miner.unload_cluster_repeat = 1;
    snap.miner.dock_phase = RefineryDockPhase::Unloading;
}

/// Smooth in-place rotation toward DOCK_FACING_EAST (0x40). gamemd starts
/// this via radio 0x16 by calling the locomotor's `Do_Turn(0x4000)`, then
/// accepts deploy once the 16-bit PrimaryFacing timer enters the same
/// quantized East-facing window.
fn phase_pivoting(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    snap: &mut MinerSnapshot,
    ref_sid: u64,
) {
    if !mission_deploy_due(sim, snap) {
        return;
    }

    // `0x0073DEE0`: every harvester-branch Unload dispatch first asks
    // `RadioClass::In_Radio_Contact` (0x0065AE30, any non-null entry in the
    // `+0xE4`×`+0xE8` contact array; mislabeled `PathType__Has_Valid_Steps`)
    // BEFORE the facing gate. Contacts gone → the unload is abandoned.
    if !miner_dock::has_contact(sim, ref_sid, snap.entity_id) {
        abort_unload_contact_lost(sim, snap, ref_sid);
        return;
    }

    if sync_dock_facing(sim, rules, snap) {
        // Mission 0x10 has reached its facing gate. Radio 0x15 only queued
        // that mission; unload-active effects begin here.
        snap.miner.dock_pivot_facing = None;
        start_unload_deploy(sim, rules, snap);
        let base = mission_base_frames(
            rules,
            MissionType::Unload,
            MISSION_DEPLOY_UNLOAD_BASE_FRAMES,
        );
        let jitter = sim
            .miner_jitter_rng()
            .next_range_u32_inclusive(0, MISSION_DEPLOY_UNLOAD_JITTER_MAX_FRAMES)
            as u8;
        schedule_mission_deploy_delay(snap, sim.session.binary_frame, base.saturating_add(jitter));
    } else {
        let _ = config;
        let _ = ref_sid;
        schedule_mission_deploy_delay(
            snap,
            sim.session.binary_frame,
            MISSION_DEPLOY_FACING_WAIT_FRAMES,
        );
        snap.miner.dock_phase = RefineryDockPhase::Pivoting;
    }
}

fn phase_unloading(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    snap: &mut MinerSnapshot,
    ref_sid: u64,
) {
    if !snap.miner.unload_active && !snap.miner.unload_cluster_timer.is_armed() {
        // Compatibility for old saves and tests that entered Unloading before
        // the byte-field cluster existed. Seed the accumulator at the threshold
        // so the next gate check passes immediately.
        snap.miner.unload_active = true;
        snap.miner.unload_accumulator = i32::from(config.unload_tick_interval);
        snap.miner
            .unload_cluster_timer
            .arm(sim.session.binary_frame, 1);
        snap.miner.unload_cluster_repeat = 1;
    }

    if !mission_deploy_due(sim, snap) {
        return;
    }

    // The same `In_Radio_Contact` gate as `phase_pivoting`: it precedes the
    // state dispatch, so the dump state abandons on contact loss too.
    if !miner_dock::has_contact(sim, ref_sid, snap.entity_id) {
        abort_unload_contact_lost(sim, snap, ref_sid);
        return;
    }

    // gamemd gate: drain when the whole-frame accumulator reaches the dump
    // threshold (`HarvesterDumpRate × 900 <= accumulator`, ceil pre-stored).
    if snap.miner.unload_accumulator < i32::from(config.unload_tick_interval) {
        return;
    }

    let Some(unload_building_id) = mission_deploy_unload_building(sim, snap.entity_id) else {
        abort_missing_unload_building(sim, snap, ref_sid);
        return;
    };
    // Original73E37E..73E3BA precedes FindFirstNonEmptySlot. Constructors
    // therefore consume identity/RNG and append Logic entries before credits.
    crate::sim::world::building_anim::begin_refinery_unload_gate(sim, rules, unload_building_id);

    // Drain one resource-type "slot" per threshold crossing — all bales
    // of the same type drop in one atomic step. The 14.4-tick interval
    // is the latency between SLOT drains, not between bale credits.
    //
    // Slot order is fixed: Ore first, then Gems, so mixed cargo drains
    // ore first.
    const SLOT_ORDER: [ResourceType; 2] = [ResourceType::Ore, ResourceType::Gem];
    let next_slot = SLOT_ORDER
        .iter()
        .copied()
        .find(|t| snap.miner.cargo.iter().any(|b| b.resource_type == *t));

    if let Some(slot_type) = next_slot {
        let mut slot_value: i32 = 0;
        let mut slot_bales: i32 = 0; // P7: bale COUNT (the HarvestedCredits stat is bales×5, not value×5)
        snap.miner.cargo.retain(|b| {
            if b.resource_type == slot_type {
                slot_value = slot_value.saturating_add(i32::from(b.value));
                slot_bales += 1;
                false
            } else {
                true
            }
        });

        // Credits go to the REFINERY OWNER, not the harvester's current
        // controller. gamemd reads the building's owner via vtable+0x3C
        // (`GetOwner` on the BuildingClass instance, not on the harvester).
        // Matters under mind-control: a Yuri unit MC'ing an enemy harvester
        // still credits the original refinery owner — the "steal" doesn't
        // work. The single GetOwner result also keys the purifier-count
        // lookup, so base credits and bonus always share one owner.
        let refinery_owner: String = sim
            .substrate
            .entities
            .get(unload_building_id)
            .map(|b| sim.interner.resolve(b.owner()).to_string())
            .expect("west-cell unload building should exist");

        // P7: per-country IncomeMult folds into the base credits (single truncation,
        // matching gamemd's one ftol per deposit call); 1.0 on stock (identity, hash-neutral).
        // The HarvestedCredits stat accrues bales×5 (statistics-only; never the wallet).
        let income_ppm = income_ppm_for_owner(&sim.houses, &sim.interner, rules, &refinery_owner);
        let base_credits = apply_income_mult(slot_value, income_ppm);
        if base_credits > 0 {
            {
                let credits = credits_entry_for_owner(sim, &refinery_owner);
                *credits = credits.saturating_add(base_credits);
            }
            if let Some(h) =
                house_state_for_owner_mut(&mut sim.houses, &refinery_owner, &sim.interner)
            {
                h.economy.add_harvested(slot_bales);
            }
        }

        // Purifier bonus applied once per slot drain — the single-truncation credit + the
        // single-truncation HarvestedCredits stat (see the helpers' contracts).
        let purifier_count = effective_purifier_count(sim, rules, &refinery_owner);
        let bonus_ppm = rules.general.purifier_bonus_ppm;
        let bonus_credits = crate::sim::economy::purifier_bonus_credits(
            slot_value,
            purifier_count,
            bonus_ppm,
            income_ppm,
        );
        if bonus_credits > 0 {
            {
                let credits = credits_entry_for_owner(sim, &refinery_owner);
                *credits = credits.saturating_add(bonus_credits);
            }
            let bonus_stat = crate::sim::economy::purifier_bonus_harvested(
                slot_bales,
                purifier_count,
                bonus_ppm,
            );
            if let Some(h) =
                house_state_for_owner_mut(&mut sim.houses, &refinery_owner, &sim.interner)
            {
                h.economy.add_harvested_raw(bonus_stat);
            }
        }

        // One deposit event per due dump gate. Native fires the refinery smoke
        // burst (vtable+0x468, `0x0073E37E`) and the `+0x584 == NULL` SpecialAnim
        // start (`0x0073E384..0x0073E3BA`) BEFORE it looks at the cargo, so the
        // gate that drains a slot and the gate that finds nothing both emit.
        sim.bale_events.push(BaleDepositEvent {
            building_id: unload_building_id,
            tick: sim.session.tick,
            drained: true,
            empty: false,
        });

        snap.miner.unload_accumulator = 0;
        schedule_mission_deploy_delay(snap, sim.session.binary_frame, 1);
        return;
    }

    // Cargo empty at a dump-gate crossing: `FindFirstNonEmptySlot` has
    // returned -1, so stock Mission_Deploy_Building state 3 advances to
    // state 4. Do not seed another dump-gate cooldown here; the due
    // mission-deploy delay and accumulator gate have already fired.
    //
    // This gate still ran the smoke burst and the SpecialAnim-start check
    // (`0x0073E37E..0x0073E3BA`), then `0x0073E4DC..0x0073E534`: slot-8
    // ProductionAnim (stock GAREFN/NAREFN define none → no-op), `+0xBC = 4`,
    // and `ClearAnimSlot(0xA)` while `+0x584` is still alive — the SpecialAnim
    // is cut, not played out.
    crate::sim::world::building_anim::end_refinery_unload_empty(sim, rules, unload_building_id);
    let unload_building = Some(unload_building_id);
    if let Some(building_id) = unload_building {
        sim.bale_events.push(BaleDepositEvent {
            building_id,
            tick: sim.session.tick,
            drained: false,
            empty: true,
        });
    }
    snap.miner.home_refinery = unload_building;
    schedule_mission_deploy_delay(snap, sim.session.binary_frame, 1);
    snap.miner.dock_phase = RefineryDockPhase::Departing;
}

/// Legacy/pass-through phase retained for older save/test states. Stock
/// unload reaches Departing directly from the empty-slot gate in
/// `phase_unloading`.
fn phase_deposit_cooldown(snap: &mut MinerSnapshot) {
    // Legacy pass-through: the deposit-cooldown countdown is retired (it was
    // always 0 in the active path). `phase_departing` owns the state-4 cleanup
    // and clears the unloading sprite override during that handoff.
    snap.miner.dock_phase = RefineryDockPhase::Departing;
}

fn phase_departing(sim: &mut Simulation, rules: &RuleSet, snap: &mut MinerSnapshot, ref_sid: u64) {
    let teleporting = sim
        .substrate
        .entities
        .get(snap.entity_id)
        .is_some_and(|e| e.teleport_state.is_some());
    if teleporting {
        return;
    }

    // Stock CMIN/HARV -> GAREFN/NAREFN completion is the zero-link
    // Mission_Deploy_Building state-4 branch: clear the unload-active
    // bookkeeping and hand directly back to Harvest/SearchOre scheduling.
    // Do not seed ReleaseDockedHarvester effects here: no Force_Track(0x47),
    // no BunkerWallsDownSound, and no cached queue-cell destination.
    miner_dock::break_contact(sim, snap.entity_id, ref_sid);

    sim.cancel_drive_track(snap.entity_id);
    if let Some(entity) = sim.substrate.entities.get_mut(snap.entity_id) {
        entity.display_type_override = None;
        entity.movement_target = None;
        entity.facing_target = None;
        snap.rx = entity.position.rx;
        snap.ry = entity.position.ry;
    }

    snap.miner.reserved_refinery = None;
    snap.miner.dock_queued = false;
    snap.miner.forced_return = false;
    snap.miner.dock_pivot_facing = None;
    clear_enter_retry(snap);
    clear_mission_deploy_delay(snap);
    clear_unload_cluster(snap);
    // The Mission_Deploy state-4 exit returns through the dispatch epilogue:
    // base = ftol([Harvest] Rate × 900) after the Queue(10,0)+Commence below
    // promotes Harvest to current, plus one RandomRanged(0,2) on Scen->Random.
    // The base lookup consumes no RNG, so drawing the jitter here keeps the
    // stream position unchanged. The value flows out through the dispatch
    // timer (`snap.dispatch_delay`), so the resumed ore search waits the full
    // Rate + jitter (~14-16f stock) — the internal harvest-timer arming this
    // replaced paced it by jitter alone.
    let resume_base = mission_base_frames(rules, MissionType::Harvest, APPROACH_HELLO_BASE_FRAMES);
    let resume_jitter = sim
        .miner_jitter_rng()
        .next_range_u32_inclusive(0, ENTER_RETRY_JITTER_MAX_FRAMES);
    snap.dispatch_delay = i32::from(resume_base) + resume_jitter as i32;
    // Clear the pending ore target and stale exit cache. Preserve
    // `last_harvest_cell`; the ghost-cell archive survives the dock cycle
    // so the next `SearchOre` can return to the productive patch saved when
    // this miner became full.
    snap.miner.target_ore_cell = None;
    snap.miner.exit_cell = None;
    snap.miner.dock_phase = RefineryDockPhase::Approach;
    snap.state = MinerState::SearchOre;
    // Native Mission_Deploy state-4 exit queues Harvest and explicitly
    // Commences it (Unit DeployBuilding `0x0073E283`: Queue(10,0), contact
    // work, explicit Commence) — the resumed harvest cycle's mission commit.
    // Commence zeroes the handler state (== SearchOre) and resets the
    // dispatch timer; the host's post-handler epilogue write then installs
    // `dispatch_delay` on top, in the native order.
    let now = sim.session.binary_frame;
    let _ = sim.mission_queue_exact(
        snap.entity_id,
        crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Harvest),
        0,
        now,
        &crate::sim::mission::authority::EntityReadyInputProvider,
    );
    let _ = sim.mission_commence_exact(snap.entity_id, now);
}

// ---------------------------------------------------------------------------
// Utility (re-exported from miner_system for shared use)
// ---------------------------------------------------------------------------

/// True if `pos` is at `target` or cardinally/diagonally adjacent (1 cell away).
fn is_adjacent_or_at(pos: (u16, u16), target: (u16, u16)) -> bool {
    let dx = (pos.0 as i32 - target.0 as i32).unsigned_abs();
    let dy = (pos.1 as i32 - target.1 as i32).unsigned_abs();
    dx <= 1 && dy <= 1
}

/// Issue a move command only if the entity isn't already pathing to this target.
fn issue_move_if_idle(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &PathGrid,
    entity_id: u64,
    target: (u16, u16),
    speed: SimFixed,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
    if target.0 >= grid.width() || target.1 >= grid.height() {
        return;
    }
    let already = sim
        .substrate
        .entities
        .get(entity_id)
        .and_then(|e| e.movement_target.as_ref())
        .and_then(|mt| mt.path.last().copied())
        .is_some_and(|goal| goal == target);
    if !already {
        let blocker_neighbor_counts =
            movement::bump_crush::build_blocker_neighbor_counts_with_overlays(
                &sim.substrate.entities,
                grid.width(),
                grid.height(),
                sim.resolved_terrain.as_ref(),
                sim.overlay_grid.as_ref(),
                overlay_registry,
                &sim.interner,
                Some(rules),
            );
        let mover_is_crusher = sim.substrate.entities.get(entity_id).is_some_and(|e| {
            crate::sim::movement::bump_crush::CrushCapability::of(e).can_crush_units()
        });
        let _ = movement::issue_move_command_with_layered(
            &mut sim.substrate.entities,
            grid,
            entity_id,
            target,
            speed,
            false,
            None,
            None,
            sim.resolved_terrain.as_ref(),
            sim.zone_grid.as_ref(),
            None,
            mover_is_crusher,
            Some(&blocker_neighbor_counts),
            sim.playfield_bounds,
            Some(&mut sim.substrate.cell_occupation),
            crate::sim::movement::DestinationTiming::from_rules(
                sim.session.binary_frame,
                rules.into(),
            ),
        );
    }
}
