//! Blocked movement handling — repath attempts when a mover's next cell is occupied or impassable.
//!
//! Called from movement_tick when terrain, cliff, or occupancy checks fail.
//! Manages the blocked_delay timer and path_stuck_counter to prevent thrashing.

use std::collections::BTreeSet;

use crate::rules::locomotor_type::MovementZone;
use crate::sim::components::MovementTarget;
use crate::sim::debug_event_log::DebugEventKind;
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::pathfinding::LayeredEntityBlockMap;
use crate::sim::pathfinding::terrain_cost::TerrainCostGrid;
use crate::sim::rng::SimRng;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

use super::movement_path::{supports_layered_bridge_pathing, try_repath_after_block};
use super::path_markers::BridgeMarkerContext;
use super::{MovementConfig, MovementTickStats, PathfindingContext};

/// Shared logic for handling a blocked movement tick.
///
/// Implements the original engine's two-timer system:
/// - `movement_delay` guards against calling Find_Path too often (PathDelay=)
/// - `blocked_delay` waits for friendlies to clear before escalating (BlockagePathDelay=)
/// - `path_stuck_counter` limits total retries before giving up (init=10)
///
/// `skip_grace_period` distinguishes gamemd's code-7 (terrain / impassable
/// / hard-block) path from code-2 (moving friendly). Code-7 has no grace
/// timer in the original — the unit stops and repaths immediately at
/// urgency=2. Code-2 spends `BlockagePathDelay` ticks at urgency=1 before
/// escalating.
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_blocked_tick(
    path_replay: &mut crate::sim::components::FootPathQueue,
    target: &mut MovementTarget,
    path_runtime: &mut crate::sim::components::FootPathRuntime,
    facing: &mut u8,
    body_facing: Option<super::FacingClass>,
    locomotor: &Option<LocomotorState>,
    drive_locomotion: &mut Option<crate::sim::components::DriveLocomotionRuntime>,
    ship_locomotion: &mut Option<crate::sim::components::ShipLocomotionRuntime>,
    entity_id: u64,
    current_pos: (u16, u16),
    active_layer: MovementLayer,
    on_bridge: bool,
    stats: &mut MovementTickStats,
    finished_entities: &mut Vec<u64>,
    aborted_for_stuck: &mut bool,
    ctx: PathfindingContext<'_>,
    entity_cost_grid: Option<&TerrainCostGrid>,
    entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    too_big_to_fit_under_bridge: bool,
    mcfg: MovementConfig,
    rng: &mut SimRng,
    sim_tick: u64,
    path_stuck_init: u32,
    mover_is_crusher: bool,
    is_infantry: bool,
    allow_zone_hierarchy: bool,
    skip_grace_period: bool,
    close_enough_abort: bool,
    marker_context: Option<BridgeMarkerContext<'_>>,
    occupancy: &crate::sim::occupancy::OccupancyGrid,
) -> Vec<(u32, DebugEventKind)> {
    let walk = locomotor
        .as_ref()
        .is_some_and(|l| l.kind == crate::rules::locomotor_type::LocomotorKind::Walk);
    let mut deferred_events: Vec<(u32, DebugEventKind)> = Vec::new();
    stats.blocked_attempts = stats.blocked_attempts.saturating_add(1);
    let next_cell = target.path.get(target.next_index).copied();
    let goal = target
        .final_goal
        .unwrap_or_else(|| target.path.last().copied().unwrap_or(current_pos));

    if !path_runtime.path_blocked {
        path_runtime.path_blocked = true;
        path_runtime.start_blocked(
            mcfg.binary_frame,
            if skip_grace_period {
                0
            } else {
                mcfg.blockage_path_delay_ticks
            },
            walk,
        );
        if let Some((nx, ny)) = next_cell {
            deferred_events.push((
                sim_tick as u32,
                DebugEventKind::Blocked {
                    by_entity: None,
                    cell: (nx, ny),
                },
            ));
        }
    } else if skip_grace_period {
        // Terrain/impassable block reached while a code-2 grace timer is
        // still running from a prior entity block. gamemd code-7 path has
        // no grace — reset so urgency=2 fires this tick.
        path_runtime.start_blocked(mcfg.binary_frame, 0, walk);
    }

    // The `CloseEnough` give-up radius is not consulted by every block code.
    // All five `Rules+0x1718` compares in the Drive movement body sit outside
    // the code-2 dispatch, so a mover blocked by a moving friendly never
    // abandons its approach on that ground — it just repaths. Callers on the
    // code-2 arm pass `false`. (The remaining arms keep VERA's single shared
    // site; whether each of them maps onto one of the five native compares is
    // UNCHECKED and recorded separately.)
    if close_enough_abort && mcfg.close_enough > SIM_ZERO {
        // Native compares a genuine 3-D Euclidean lepton distance —
        // `CoordStruct::Distance3D` @ `0x0041C380`, `Sqrt_Approx(z² + y² + x²)`
        // then `Math::ftol` — against `Rules+0x1718` (`CloseEnough`, string
        // `0x0083BD84`). `Process_Movement` compares it at `0x004B297F`,
        // `0x004B2C5C`, `0x004B3141`, `0x004B37BE` and `0x004B42DB`.
        //
        // A Manhattan sum is never smaller than the Euclidean one, and the
        // predicate is `dist < CloseEnough` → stop, so the old form made the
        // abort fire *less* often: units kept pushing to the exact goal where
        // retail already declared arrival. With stock `CloseEnough = 576` the
        // only cell offsets that change answer are Δ(2,1) and Δ(1,2) —
        // Manhattan 768 (no abort) against Euclidean 572 (abort).
        //
        // Two VERA-internal residuals remain, gamemd equivalent UNCHECKED.
        // Z is dropped, because VERA's goal here is a cell pair with no height —
        // it matters on bridge approaches. And the distance is measured between
        // cell indices × 256 rather than between actual coordinates, so VERA is
        // blind to sub-cell offsets, which is the larger of the two errors.
        //
        // Unpinned: `close_enough_abort` has two references in the whole
        // tree, the path-test harness sets `close_enough` to zero which
        // disables this branch, and no fixture drives a diagonal approach at
        // the give-up radius. One at Δ(2,1) with `close_enough = 576` would
        // pin the entire behaviour delta.
        let dx = (goal.0 as i64 - current_pos.0 as i64).abs() * 256;
        let dy = (goal.1 as i64 - current_pos.1 as i64).abs() * 256;
        let dist = SimFixed::from_num(crate::util::fixed_math::isqrt_i64(dx * dx + dy * dy));
        if dist < mcfg.close_enough {
            log::info!(
                "CLOSE_ENOUGH entity={} pos=({},{}) goal=({},{}) dist={} - stopping",
                entity_id,
                current_pos.0,
                current_pos.1,
                goal.0,
                goal.1,
                dist,
            );
            finished_entities.push(entity_id);
            *aborted_for_stuck = true;
            return deferred_events;
        }
    }

    if !path_runtime
        .movement_timer
        .expired(mcfg.binary_frame as i32)
    {
        return deferred_events;
    }

    // Repath every tick while movement_delay == 0. Urgency escalates once the
    // blocked_delay (BlockagePathDelay) timer has expired:
    //   urgency=1 while blocked_delay > 0 → 4x traffic penalty
    //   urgency=2 once blocked_delay == 0 → 1000x route-around
    // Matches gamemd.exe DriveLocomotionClass::Process_Movement (LAB_004b3607).
    stats.repath_attempts = stats.repath_attempts.saturating_add(1);
    let urgency: u8 = if !path_runtime.blocked_timer.expired(mcfg.binary_frame as i32) {
        1
    } else {
        2
    };
    let layered_pathing_for_repath = locomotor
        .as_ref()
        .zip(ctx.path_grid)
        .is_some_and(|(loco, pg)| supports_layered_bridge_pathing(loco, pg, on_bridge));
    let repath_mz: Option<MovementZone> = locomotor.as_ref().map(|l| l.movement_zone);
    let marker_search = marker_context.map(|context| {
        context.build(
            occupancy,
            entity_id,
            current_pos,
            *facing,
            body_facing,
            on_bridge,
            urgency,
        )
    });
    // Walk code2 retains its +6B7 latch and +668 grace timer across successful
    // FindPath (0x75B979..0x75B9F9). Actual walking clears the latch at 0x75BFCD.
    // The shared path installer retains its existing reset for other movers.
    let walk_blocked_state = locomotor
        .as_ref()
        .filter(|l| l.kind == crate::rules::locomotor_type::LocomotorKind::Walk)
        .map(|_| (path_runtime.path_blocked, path_runtime.blocked_timer));
    let repath_ok = try_repath_after_block(
        target,
        path_runtime,
        walk,
        facing,
        current_pos,
        active_layer,
        layered_pathing_for_repath,
        ctx,
        entity_cost_grid,
        entity_blocks,
        rng,
        repath_mz,
        too_big_to_fit_under_bridge,
        mcfg,
        entity_block_map,
        urgency,
        mover_is_crusher,
        is_infantry,
        allow_zone_hierarchy,
        marker_search.as_ref(),
    );
    if repath_ok {
        if let Some((path_blocked, blocked_delay)) = walk_blocked_state {
            path_runtime.path_blocked = path_blocked;
            path_runtime.blocked_timer = blocked_delay;
        }
        match locomotor.as_ref().map(|locomotor| locomotor.kind) {
            Some(crate::rules::locomotor_type::LocomotorKind::Drive) => {
                if drive_locomotion.is_some() {
                    super::path_markers::install_path_replay(
                        path_replay,
                        current_pos,
                        &target.path,
                        target.next_index,
                    );
                }
            }
            Some(crate::rules::locomotor_type::LocomotorKind::Ship) => {
                if ship_locomotion.is_some() {
                    super::path_markers::install_path_replay(
                        path_replay,
                        current_pos,
                        &target.path,
                        target.next_index,
                    );
                }
            }
            _ => {}
        }
        stats.repath_successes = stats.repath_successes.saturating_add(1);
        if is_infantry
            && !locomotor
                .as_ref()
                .is_some_and(|l| l.kind == crate::rules::locomotor_type::LocomotorKind::Walk)
        {
            path_runtime.path_blocked = false;
        }
        path_runtime.retries_left = path_stuck_init;
        deferred_events.push((
            sim_tick as u32,
            DebugEventKind::Repath {
                reason: format!(
                    "blocked repath succeeded (urgency={} effective={})",
                    urgency,
                    marker_search
                        .as_ref()
                        .map_or(urgency, |search| search.effective_urgency)
                ),
                new_path_len: target.path.len(),
            },
        ));
    } else if urgency >= 2 {
        // Only escalated (urgency=2) repath failures count toward give-up.
        // gamemd.exe decrements path_stuck_counter in a separate "no valid
        // next cell" branch, not on every code-2 repath miss — so we don't
        // decrement during the blocked_delay grace period (urgency=1).
        path_runtime.retries_left = path_runtime.retries_left.saturating_sub(1);
        if path_runtime.retries_left == 0 {
            log::warn!(
                "STUCK ABORT entity={} pos=({},{}) - path_stuck_counter exhausted",
                entity_id,
                current_pos.0,
                current_pos.1,
            );
            deferred_events.push((
                sim_tick as u32,
                DebugEventKind::StuckAbort { blocked_ticks: 0 },
            ));
            stats.stuck_recoveries = stats.stuck_recoveries.saturating_add(1);
            finished_entities.push(entity_id);
            *aborted_for_stuck = true;
        }
        // gamemd does not restart the grace timer *here*, on a failed
        // route-around — this arm writes nothing.
        //
        // It does restart it in the code-2 dispatch, on every pass and not just
        // on the `path_blocked` 0 -> 1 transition: the original's store of the
        // wait sits straight-line after its blocker-scatter call with no branch
        // between them. An earlier revision of this comment claimed the
        // transition was the only writer, and the code was gated to match; that
        // left the timer at zero forever once it first expired, so the blocker
        // scatter — and its scenario-stream draw — fired every tick instead of
        // once per span. See
        // `movement_tests::code_two_post_scatter_wait_rearms_on_every_pass_while_the_block_holds`.
        //
        // So a boxed-in unit does not sit at urgency 2 continuously; it
        // escalates to 2 once per span, then drops back to 1 when the wait
        // re-arms.
    } else {
        // urgency=1 grace-period failure: set a short movement_delay to
        // rate-limit A* calls while the blocked_delay counter keeps ticking.
        path_runtime.start_movement(mcfg.binary_frame, mcfg.path_delay_ticks, walk);
    }
    // Walk restarts PathDelay after every actual FindPath attempt, including
    // success and urgency-2 failure (0x75B98C..0x75B9B1). Drive does not.
    if locomotor
        .as_ref()
        .is_some_and(|l| l.kind == crate::rules::locomotor_type::LocomotorKind::Walk)
    {
        path_runtime.start_movement(mcfg.binary_frame, mcfg.path_delay_ticks, walk);
    }
    deferred_events
}

#[cfg(test)]
mod native_walk_timer_tests {
    use super::*;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::occupancy::OccupancyGrid;
    use crate::sim::pathfinding::PathGrid;

    // Native fixture comparisons use the retained frame-anchored timer owner.
    fn remaining(frame: i64, start: i64, duration: i64) -> u16 {
        u16::try_from(if start == -1 {
            duration
        } else {
            (duration - (frame - start)).max(0)
        })
        .unwrap()
    }

    #[test]
    fn walk_code_two_repath_timing_matches_native_vectors() {
        // Original instructions 0x75B8A0..0x75B979 / 0x75C1F1. Recheck:
        // python -m tools.infantry_scatter_oracle --check
        // Coverage: grace initialization/preservation, movement-delay gate,
        // and urgency selection. Native FindPath and final timer restart are
        // outside the oracle; the open-grid success/restart checks below are
        // Rust integration regressions, not native pathfinding parity.
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../../../tools/infantry_scatter_oracle.json"))
                .unwrap();
        assert_eq!(data["source"], "unicorn/gamemd.exe");
        let cases = data["walk_code2_timers"].as_array().unwrap();
        assert_eq!(cases.len(), 30);
        let grid = PathGrid::new(20, 20);
        let occupancy = OccupancyGrid::new();
        let locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
        let mut observed = [0usize; 3]; // wait, urgency1, urgency2

        for case in cases {
            let n = |key: &str| case[key].as_i64().unwrap();
            let b = |key: &str| case[key].as_bool().unwrap();
            let frame = n("frame");
            let prior_movement = remaining(frame, n("movement_start"), n("movement_duration"));
            let mut target = MovementTarget {
                path: vec![(8, 12), (9, 12), (10, 12)],
                path_layers: vec![MovementLayer::Ground; 3],
                next_index: 1,
                final_goal: Some((10, 12)),
                ..Default::default()
            };
            let mut path_runtime = crate::sim::components::FootPathRuntime {
                path_blocked: b("already_blocked"),
                blocked_timer: crate::sim::timer::CdTimer::from_raw(
                    n("grace_start") as i32,
                    n("grace_duration") as i32,
                ),
                movement_timer: crate::sim::timer::CdTimer::from_raw(
                    n("movement_start") as i32,
                    n("movement_duration") as i32,
                ),
                retries_left: 10,
            };
            let mut facing = 64;
            let mut stats = MovementTickStats::default();
            let mut finished = Vec::new();
            let mut aborted = false;
            let events = handle_blocked_tick(
                &mut Default::default(),
                &mut target,
                &mut path_runtime,
                &mut facing,
                None,
                &locomotor,
                &mut None,
                &mut None,
                1,
                (8, 12),
                MovementLayer::Ground,
                false,
                &mut stats,
                &mut finished,
                &mut aborted,
                PathfindingContext {
                    wall_cost: None,
                    path_grid: Some(&grid),
                    zone_grid: None,
                    resolved_terrain: None,
                    playfield_bounds: None,
                    blocker_neighbor_counts: None,
                },
                None,
                None,
                None,
                false,
                MovementConfig {
                    binary_frame: frame as u32,
                    close_enough: SIM_ZERO,
                    path_delay_ticks: 3,
                    blockage_path_delay_ticks: n("configured_grace") as u16,
                },
                &mut SimRng::new(7),
                frame as u64,
                10,
                false,
                true,
                true,
                false,
                false,
                None,
                &occupancy,
            );
            assert_eq!(stats.repath_attempts, u32::from(b("repath")), "{case}");
            assert_eq!(path_runtime.path_blocked, b("out_blocked"), "{case}");
            assert_eq!(
                path_runtime.blocked_timer.remaining(frame as i32) as u16,
                remaining(frame, n("out_grace_start"), n("out_grace_duration")),
                "{case}"
            );
            assert!(!aborted && finished.is_empty(), "{case}");
            assert_eq!(target.final_goal, Some((10, 12)), "{case}");
            if b("repath") {
                let urgency = n("urgency");
                observed[urgency as usize] += 1;
                assert_eq!(case["goal"], serde_json::json!([10, 12]));
                assert_eq!(stats.repath_successes, 1, "{case}");
                assert!(
                    events.iter().any(|(_, event)| matches!(event,
                        DebugEventKind::Repath { reason, .. }
                        if reason.contains(&format!("urgency={urgency} effective={urgency}"))
                    )),
                    "{case}"
                );
                assert_eq!(
                    path_runtime.movement_timer.remaining(frame as i32) as u16,
                    3,
                    "{case}"
                );
            } else {
                observed[0] += 1;
                assert_eq!(
                    path_runtime.movement_timer.remaining(frame as i32) as u16,
                    prior_movement,
                    "{case}"
                );
                assert!(
                    !events
                        .iter()
                        .any(|(_, event)| matches!(event, DebugEventKind::Repath { .. })),
                    "{case}"
                );
            }
        }
        assert!(observed.iter().all(|&count| count > 0), "{observed:?}");
    }
}
