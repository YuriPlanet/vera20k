//! Recovery of interrupted strategic orders.
//!
//! Tactical passes run before this pass. They may recompute Dodge, Kite,
//! Engage, Avoid or Breach from the *current* battlefield. Only if none still
//! owns the actor does this module try the saved primary objective.
//! Never replay an old tactical coordinate: the shell or enemy has moved.

use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::Observation;

use super::AssistBrain;
use super::tracked::{PausedMission, TrackedKind, primary_kind};

/// Make room for native motion to settle without abandoning a useful order.
/// A truly stalled old detour is handed back to the strategic planner.
const STALE_OVERRIDE_FRAMES: u64 = 240;
/// Recovery is lower priority than fresh tactical decisions and must not use
/// all of L2's command budget when many units finish dodging together.
const RESUMES_PER_FRAME: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Recovery {
    Wait,
    AlreadyRunning,
    Resume,
    Replan,
}

/// A stale obstacle shot must not be replayed while the obstruction still
/// stands. The world owns whether the unit is still walking or shooting.
fn settle(
    now: u64,
    paused: PausedMission,
    current_cell: (u16, u16),
    destination: Option<(u16, u16)>,
    attack_target: Option<u64>,
    primary: Goal,
    still_blocked: bool,
) -> Recovery {
    let elapsed = now.saturating_sub(paused.at);
    let target_cell = match primary {
        Goal::Move { cell } | Goal::AttackMove { cell }
        | Goal::GuardAt { cell } | Goal::Harvest { cell } => Some(cell),
        _ => None,
    };
    if attack_target.is_some_and(|target| {
        matches!(primary, Goal::Attack { target: wanted } if wanted == target)
    }) {
        return Recovery::AlreadyRunning;
    }
    if let Goal::Move { cell } | Goal::AttackMove { cell } = primary {
        if current_cell == cell && attack_target.is_none() {
            // Already at the strategic waypoint: replaying AttackMove here
            // would restart native acquisition rather than make progress.
            return Recovery::AlreadyRunning;
        }
    }
    if destination.is_some_and(|cell| Some(cell) == target_cell) {
        return Recovery::AlreadyRunning;
    }
    if attack_target.is_some() {
        return if elapsed > STALE_OVERRIDE_FRAMES {
            Recovery::Replan
        } else {
            Recovery::Wait
        };
    }
    if paused.kind == TrackedKind::Breach && still_blocked {
        return if elapsed > STALE_OVERRIDE_FRAMES {
            Recovery::Replan
        } else {
            Recovery::Wait
        };
    }
    if let Some(destination) = destination {
        let arrived_at_tactical_cell = paused.cell == Some(current_cell);
        if destination == current_cell || arrived_at_tactical_cell {
            return Recovery::Resume;
        }
        return if elapsed > STALE_OVERRIDE_FRAMES {
            Recovery::Replan
        } else {
            Recovery::Wait
        };
    }
    Recovery::Resume
}

/// Only a still-authorized, sensible strategic goal may be restored. A
/// vanished enemy, an occupied capture target or a depleted known ore cell is
/// reconsidered by the usual planning passes, not retried from stale memory.
fn valid_goal(
    brain: &AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    actor: u64,
    goal: Goal,
) -> bool {
    use super::orders;
    match goal {
        Goal::Attack { target } => obs.health(target).is_some()
            && orders::admits(rules, obs, actor, &goal),
        Goal::Capture { target } => obs.health(target).is_some()
            && !brain.tracked.claimed_by_other(
                obs.tick(), TrackedKind::Capture, target, actor,
            )
            && orders::admits(rules, obs, actor, &goal),
        Goal::Enter { target } => obs.health(target).is_some()
            && orders::admits(rules, obs, actor, &goal),
        Goal::Move { cell } | Goal::AttackMove { cell }
        | Goal::GuardAt { cell } => orders::on_map(cell, obs.map_extent())
            && orders::reachable(rules, obs, actor, cell),
        Goal::Harvest { cell } => brain.ore_known.contains_key(&cell)
            && orders::reachable(rules, obs, actor, cell)
            && (!obs.is_cell_visible(cell.0, cell.1)
                || obs.ore_cells(rules).contains(&cell)),
        Goal::Guard => true,
        _ => false,
    }
}

/// After all tactical passes, restore at most a couple of strategic missions.
///
/// Any new tactical ask already in the Outbox wins. When a restore is accepted
/// by L3 it still requires L2 admission; `restore_requested` preserves the
/// suspended objective if L2 reports a refusal later in this frame.
pub(super) fn resume(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let now = obs.tick();
    let mut candidates: Vec<_> = brain.census.units.iter()
        .filter(|unit| brain.tracked.pending_resume(now, unit.entity_id).is_some())
        .copied()
        .collect();
    if !candidates.is_empty() {
        let len = candidates.len();
        candidates.rotate_left((now as usize) % len);
    }
    let mut sent = 0usize;
    for unit in candidates {
        if sent >= RESUMES_PER_FRAME {
            break;
        }
        let actor = unit.entity_id;
        let Some((goal, paused)) = brain.tracked.pending_resume(now, actor) else {
            continue;
        };
        // A player can change the fifth key between a primary command and
        // its tactical interruption. Never reassert control across that edge.
        if unit.hand || unit.traits.spawned || unit.traits.yard_maker {
            brain.tracked.forget_primary(actor);
            continue;
        }
        if !unit.orderable || unit.in_service() {
            // Boarding, repair, capture and limbo transfer ownership to the
            // native lifecycle. Resurrecting an old Move later would interrupt
            // that work or re-task a unit that is no longer on the map.
            brain.tracked.forget_primary(actor);
            continue;
        }
        if matches!(goal, Goal::Guard | Goal::GuardAt { .. })
            && matches!(unit.mission.known(),
                Some(crate::sim::mission::MissionType::Guard
                    | crate::sim::mission::MissionType::AreaGuard))
        {
            brain.tracked.complete_resume(actor);
            continue;
        }
        if matches!(goal, Goal::Harvest { .. })
            && unit.mission.known() == Some(crate::sim::mission::MissionType::Harvest)
        {
            brain.tracked.complete_resume(actor);
            continue;
        }
        if out.ordered_this_frame(actor) || obs.is_deployed(actor) {
            continue;
        }
        // A newer tactical decision may still be executing in native after
        // its short Tracker term expired. Do not interrupt that travel.
        let blocked = paused.cell.is_some_and(|cell| {
            !obs.is_ground_walkable(cell.0, cell.1)
        });
        match settle(
            now, paused, unit.cell, obs.destination(actor),
            obs.attack_target(actor), goal, blocked,
        ) {
            Recovery::Wait => continue,
            Recovery::AlreadyRunning => {
                brain.tracked.complete_resume(actor);
                continue;
            }
            Recovery::Replan => {
                brain.tracked.forget_primary(actor);
                continue;
            }
            Recovery::Resume => {}
        }
        // A target that was useful before an emergency may no longer exist,
        // or may no longer be visible. Never send a remembered entity-id click
        // into the shroud. The existing planner can choose a new target.
        if !valid_goal(brain, rules, obs, actor, goal) {
            brain.tracked.forget_primary(actor);
            continue;
        }
        let Some(kind) = primary_kind(goal) else {
            brain.tracked.forget_primary(actor);
            continue;
        };
        let (target, cell) = match goal {
            Goal::Attack { target } | Goal::Capture { target }
            | Goal::Enter { target } => (Some(target), None),
            Goal::Move { cell } | Goal::AttackMove { cell }
            | Goal::GuardAt { cell } | Goal::Harvest { cell } => (None, Some(cell)),
            Goal::Guard => (None, None),
            _ => continue,
        };
        if super::orders::issue_tracked(
            &mut brain.tracked, brain.trace, out, rules, obs,
            actor, kind, target, cell, goal,
        ) {
            brain.tracked.restore_requested(now, actor, goal, paused);
            sent += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paused(kind: TrackedKind, at: u64, cell: (u16, u16)) -> PausedMission {
        PausedMission { kind, at, cell: Some(cell), retries: 0 }
    }

    #[test]
    fn a_dodge_move_must_finish_before_the_original_march_is_restored() {
        let original = Goal::AttackMove { cell: (40, 20) };
        let interruption = paused(TrackedKind::Dodge, 10, (12, 10));
        assert_eq!(
            settle(22, interruption, (11, 10), Some((12, 10)), None, original, false),
            Recovery::Wait
        );
        assert_eq!(
            settle(24, interruption, (12, 10), None, None, original, false),
            Recovery::Resume
        );
        assert_eq!(
            settle(30, interruption, (40, 20), None, None, original, false),
            Recovery::AlreadyRunning,
            "an already reached strategic waypoint is never clicked twice"
        );
    }

    #[test]
    fn a_destroyed_or_missing_capture_target_must_be_replanned() {
        let rules = RuleSet::from_ini(
            &crate::sim::arena_fixture::clear_ground_ini(""),
        ).expect("small synthetic rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
        let owner = sim.interner.intern("Americans");
        let obs = Observation::new(&sim, owner);
        let brain = AssistBrain::new(0xA17);
        assert!(!valid_goal(
            &brain, &rules, &obs, 7, Goal::Attack { target: 987_654 },
        ));
        assert!(!valid_goal(
            &brain, &rules, &obs, 7, Goal::Capture { target: 987_654 },
        ));
    }

    #[test]
    fn an_engagement_and_a_blocked_breach_prevent_stale_replay() {
        let original = Goal::AttackMove { cell: (40, 20) };
        let breach = paused(TrackedKind::Breach, 10, (14, 10));
        assert_eq!(
            settle(100, breach, (12, 10), None, None, original, true),
            Recovery::Wait
        );
        assert_eq!(
            settle(251, breach, (12, 10), None, None, original, true),
            Recovery::Replan
        );
        assert_eq!(
            settle(30, paused(TrackedKind::Kite, 10, (11, 11)),
                   (11, 11), None, Some(9), original, false),
            Recovery::Wait
        );
    }
}
