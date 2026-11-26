//! A small cross-unit mission: one base defender protects one engineer on a
//! committed capture walk. It is not a new command or a replacement for the
//! native fighting AI. The engineer keeps its Capture primary, while the
//! escort follows at a distance using an ordinary attack-move click.
//!
//! Ownership is deliberately conservative: only spare members of the Defend
//! squad can be borrowed, never the offensive fist, a scouting dog, an
//! aircraft or a player-controlled unit. A base raid releases all escorts.
//! A failed/unaccepted Capture never creates an escort on the same frame.
//! Combat micro and emergency dodge outrank the escort's movement. Once the
//! engineer, the target or either unit's permission disappears, release the
//! assignment and the escort's resumable goal together.

use std::collections::BTreeSet;

use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::Observation;

use super::AssistBrain;
use super::census::Unit;
use super::orders;
use super::squads::Role;
use super::tracked::TrackedKind;
use super::squared_distance;

/// Leave three defenders at home even while the base is quiet. The other
/// members stay available for the army's existing raid response.
const BASE_RESERVE: usize = 3;
const MAX_ESCORTS: usize = 2;
/// The board has a small per-frame unit-command budget. Escort movement can
/// consume at most one slot, leaving room for defence, dodge and the fist.
const ESCORT_ORDERS_PER_FRAME: usize = 1;
const RECRUIT_DISTANCE: i32 = 14;
const LEASH_DISTANCE: i32 = 20;
const FOLLOW_DISTANCE: i32 = 4;
const TARGET_ARRIVAL_DISTANCE: i32 = 3;

fn escort_capacity(defenders: usize, home_raid: bool) -> usize {
    if home_raid { 0 } else {
        defenders.saturating_sub(BASE_RESERVE).min(MAX_ESCORTS)
    }
}

/// A cross-unit commitment, keyed by the engineer rather than by the guard.
/// The post is recorded only when a real order entered the outbox; it allows
/// release to clear *our* primary without destroying a later command's goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Escort {
    pub(super) guard: u64,
    pub(super) target: u64,
    pub(super) post: Option<(u16, u16)>,
}

/// A bodyguard should follow the engineer, not the target building. Place the
/// rally post behind the engineer's direction of travel (two cells behind),
/// not on the occupied Capture destination. Adjacent alternatives cope with
/// a wall, another unit or a map edge without random tie-breaking.
fn follow_post(
    engineer: (u16, u16),
    target: (u16, u16),
    mut available: impl FnMut((u16, u16)) -> bool,
) -> Option<(u16, u16)> {
    let vx = (i32::from(target.0) - i32::from(engineer.0)).signum();
    let vy = (i32::from(target.1) - i32::from(engineer.1)).signum();
    if vx == 0 && vy == 0 {
        return None;
    }
    let centre = (
        i32::from(engineer.0) - 2 * vx,
        i32::from(engineer.1) - 2 * vy,
    );
    const NEAR: &[(i32, i32)] = &[
        (0, 0), (0, -1), (0, 1), (-1, 0), (1, 0),
        (-1, -1), (1, -1), (-1, 1), (1, 1),
    ];
    NEAR.iter().find_map(|(dx, dy)| {
        let cell = (
            u16::try_from(centre.0 + *dx).ok()?,
            u16::try_from(centre.1 + *dy).ok()?,
        );
        available(cell).then_some(cell)
    })
}

/// Pick the closest free defender with the stable id as a tie-breaker. The
/// predicate checks native orderability and live Tracker ownership outside
/// this pure selection kernel; it never consumes an attacking squad member.
fn select_guard(
    units: &[Unit],
    defenders: &BTreeSet<u64>,
    reserved: &BTreeSet<u64>,
    engineer: (u16, u16),
    available: impl Fn(u64) -> bool,
) -> Option<u64> {
    units.iter()
        .filter(|unit| defenders.contains(&unit.entity_id))
        .filter(|unit| !reserved.contains(&unit.entity_id))
        .filter(|unit| unit.controllable() && !unit.in_service())
        .filter(|unit| unit.traits.combat && unit.traits.armed)
        .filter(|unit| !unit.traits.aircraft && !unit.traits.irreplaceable)
        .filter(|unit| squared_distance(unit.cell, engineer)
            <= RECRUIT_DISTANCE * RECRUIT_DISTANCE)
        .filter(|unit| available(unit.entity_id))
        .min_by_key(|unit| (squared_distance(unit.cell, engineer), unit.entity_id))
        .map(|unit| unit.entity_id)
}

/// Guard/engineer permissions and the base's defensive priority are part of
/// the assignment's lifetime, not only its creation. In particular a fifth-
/// key scope flip, a reassigned squad, or a raid must release the guard before
/// recovery has a chance to replay its old escort post.
fn still_assigned(
    engineer: Option<&Unit>,
    guard: Option<&Unit>,
    defenders: &BTreeSet<u64>,
    home_raid: bool,
    target_still_capturable: bool,
    guard_deployed: bool,
) -> bool {
    !home_raid && target_still_capturable && !guard_deployed
        && engineer.is_some_and(|unit| unit.traits.engineer && unit.controllable())
        && guard.is_some_and(|unit| {
            defenders.contains(&unit.entity_id)
                && unit.controllable() && !unit.in_service()
        })
        && engineer.zip(guard).is_some_and(|(engineer, guard)| {
            squared_distance(engineer.cell, guard.cell)
                <= LEASH_DISTANCE * LEASH_DISTANCE
        })
}

/// A living, authorized Capture commitment; looking up a hidden hostile's
/// coordinates is not allowed, even if an old entity id still exists.
fn active_target(
    brain: &AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    engineer: u64,
) -> Option<(u64, (u16, u16))> {
    let Goal::Capture { target } = brain.tracked.primary_goal(engineer)? else {
        return None;
    };
    // Ask L2's legal click first: the full object lookup below is only
    // admissible for a target this house may currently act on.
    if !orders::admits(rules, obs, engineer, &Goal::Capture { target }) {
        return None;
    }
    let (cell, _) = obs.object_cell_and_type(target)?;
    if obs.health(target).is_none() {
        return None;
    }
    Some((target, cell))
}

/// One simultaneous capture and protective movement, with independent actor
/// permissions. Running it after micro gives firing, dodging and kiting first
/// refusal; running it before the ordinary guard pass preserves its post.
pub(super) fn run(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let tick = obs.tick();
    let defenders: BTreeSet<u64> = brain.squads.squad(Role::Defend)
        .map(|squad| squad.members.clone()).unwrap_or_default();
    let home_raid = super::advance::threatened_cell(&brain.census).is_some();

    // Release is evaluated every frame, even if no engineers remain. In
    // particular a fifth-key flip cannot leave a guard assigned to a player.
    // Recompute the home reserve too: after losses or regrouping a previously
    // spare guard may be needed to defend the base again.
    let capacity = escort_capacity(defenders.len(), home_raid);
    let mut kept = 0usize;
    let expired: Vec<u64> = brain.escorts.iter()
        .filter_map(|(engineer_id, mission)| {
            let engineer = brain.census.units.iter()
                .find(|unit| unit.entity_id == *engineer_id);
            let guard = brain.census.units.iter()
                .find(|unit| unit.entity_id == mission.guard);
            let valid = still_assigned(
                engineer, guard, &defenders, home_raid,
                active_target(brain, rules, obs, *engineer_id)
                    .is_some_and(|(target, _)| target == mission.target),
                obs.is_deployed(mission.guard),
            ) && kept < capacity;
            if valid { kept += 1; }
            (!valid).then_some(*engineer_id)
        })
        .collect();
    for engineer in expired {
        if let Some(old) = brain.escorts.remove(&engineer) {
            log::debug!(
                "auto play: escort {} released from engineer {}",
                old.guard, engineer,
            );
            if let Some(post) = old.post
                && brain.tracked.primary_goal(old.guard)
                    == Some(Goal::AttackMove { cell: post })
            {
                brain.tracked.forget_primary(old.guard);
                brain.tracked.void(old.guard, TrackedKind::AttackMove);
            }
        }
    }
    if capacity == 0 {
        return;
    }

    // Reserve only after a Capture survived the previous frame's L2 feedback.
    // The first frame's outbox is merely an ask: its command may be refused.
    let engineers: Vec<(u64, (u16, u16), u64, (u16, u16))> = brain.census.units.iter()
        .filter(|unit| unit.traits.engineer && unit.controllable())
        .filter(|unit| !out.ordered_this_frame(unit.entity_id))
        .filter_map(|unit| {
            let (target, cell) = active_target(brain, rules, obs, unit.entity_id)?;
            if !brain.tracked.confirmed_capture(tick, unit.entity_id, target) {
                return None;
            }
            Some((unit.entity_id, unit.cell, target, cell))
        })
        .collect();

    let mut issued = 0usize;
    for (engineer_id, cell, target, target_cell) in engineers {
        if issued >= ESCORT_ORDERS_PER_FRAME { break; }
        if squared_distance(cell, target_cell)
            <= TARGET_ARRIVAL_DISTANCE * TARGET_ARRIVAL_DISTANCE
        {
            continue;
        }
        if !brain.escorts.contains_key(&engineer_id) {
            if brain.escorts.len() >= capacity {
                continue;
            }
            let reserved: BTreeSet<u64> = brain.escorts.values()
                .map(|mission| mission.guard).collect();
            let chosen = select_guard(
                &brain.census.units, &defenders, &reserved, cell,
                |guard| {
                    !brain.census.unavailable.contains(&guard)
                        && !brain.tracked.held(tick, guard)
                        && !out.ordered_this_frame(guard)
                        && !obs.is_deployed(guard)
                        && obs.attack_target(guard).is_none()
                        && obs.destination(guard).is_none()
                        && matches!(
                            brain.tracked.live_kind(tick, guard),
                            None | Some(TrackedKind::Guard),
                        )
                },
            );
            let Some(guard) = chosen else { continue };
            brain.escorts.insert(engineer_id, Escort {
                guard, target, post: None,
            });
            log::debug!(
                "auto play: escort {} assigned to engineer {} taking {}",
                guard, engineer_id, target,
            );
        }
        let Some(mission) = brain.escorts.get(&engineer_id).copied() else {
            continue;
        };
        let Some(guard) = brain.census.units.iter()
            .find(|unit| unit.entity_id == mission.guard)
        else { continue };
        if squared_distance(guard.cell, cell) <= FOLLOW_DISTANCE * FOLLOW_DISTANCE
            || out.ordered_this_frame(guard.entity_id)
            || obs.attack_target(guard.entity_id).is_some()
        {
            continue;
        }
        let post = follow_post(cell, target_cell, |post| {
            obs.is_ground_walkable(post.0, post.1)
                && orders::reachable(rules, obs, guard.entity_id, post)
        });
        let Some(post) = post else { continue };
        // AttackMove lets the guard fight naturally while approaching the
        // engineer. The combat micro may override it without erasing the
        // engineer's primary or playing an outdated tactical waypoint.
        if orders::issue_tracked(
            &mut brain.tracked, brain.trace, out, rules, obs,
            guard.entity_id, TrackedKind::AttackMove,
            None, Some(post), Goal::AttackMove { cell: post },
        ) {
            if let Some(mission) = brain.escorts.get_mut(&engineer_id) {
                mission.post = Some(post);
            }
            issued += 1;
        }
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;
    use crate::sim::mission::{MissionId, MissionType};
    use super::super::census::{Slot, Traits};

    fn defender(id: u64, cell: (u16, u16)) -> Unit {
        Unit {
            entity_id: id,
            type_id: Default::default(),
            cell,
            health_permille: 1_000,
            points: 100,
            mission: MissionId::from_known(MissionType::Guard),
            orderable: true,
            hand: false,
            slot: Slot::Infantry,
            tier: 0,
            traits: Traits { combat: true, armed: true, ..Default::default() },
        }
    }

    #[test]
    fn a_raid_or_a_shrinking_garrison_reclaims_escorts() {
        assert_eq!(escort_capacity(3, false), 0);
        assert_eq!(escort_capacity(4, false), 1);
        assert_eq!(escort_capacity(5, false), 2);
        assert_eq!(escort_capacity(12, false), MAX_ESCORTS);
        assert_eq!(escort_capacity(12, true), 0);
    }

    #[test]
    fn only_an_available_defender_may_escort_and_ties_are_stable() {
        let mut unavailable = defender(4, (12, 10));
        unavailable.hand = true;
        let units = [
            defender(8, (12, 10)), defender(6, (12, 10)),
            unavailable, defender(3, (11, 10)),
        ];
        let defenders = BTreeSet::from([4, 6, 8]);
        assert_eq!(select_guard(&units, &defenders, &BTreeSet::new(),
            (10, 10), |_| true), Some(6));
        let reserved = BTreeSet::from([6]);
        assert_eq!(select_guard(&units, &defenders, &reserved,
            (10, 10), |_| true), Some(8));
        assert_eq!(select_guard(&units, &defenders, &reserved,
            (10, 10), |_| false), None);
    }

    #[test]
    fn escort_post_stays_behind_capture_route_and_dodges_obstacles() {
        assert_eq!(follow_post((20, 20), (30, 20), |_| true), Some((18, 20)));
        assert_eq!(follow_post((20, 20), (30, 20), |p| p != (18, 20)),
            Some((18, 19)));
        assert_eq!(follow_post((0, 0), (30, 30), |_| true), None,
            "a rally post outside the map cannot be wrapped into u16");
        assert_eq!(follow_post((20, 20), (20, 20), |_| true), None);
    }

    #[test]
    fn an_escort_is_released_on_scope_change_raid_or_loss() {
        let mut engineer = defender(10, (20, 20));
        engineer.traits.engineer = true;
        let mut guard = defender(20, (21, 20));
        let defenders = BTreeSet::from([20]);
        let valid = |engineer: &Unit, guard: &Unit, raid: bool, capturable: bool| {
            still_assigned(Some(engineer), Some(guard), &defenders,
                raid, capturable, false)
        };
        assert!(valid(&engineer, &guard, false, true));
        assert!(!valid(&engineer, &guard, true, true),
            "the base's defence must always take precedence");
        assert!(!valid(&engineer, &guard, false, false),
            "an already captured building needs no escort");
        engineer.hand = true;
        assert!(!valid(&engineer, &guard, false, true),
            "a player-controlled engineer ends its AI group task");
        engineer.hand = false;
        guard.hand = true;
        assert!(!valid(&engineer, &guard, false, true),
            "the fifth key can exclude a guard independently");
        guard.hand = false;
        guard.cell = (60, 20);
        assert!(!valid(&engineer, &guard, false, true),
            "a stranded guard cannot reserve the escort forever");
    }

    #[test]
    fn losing_the_engineers_primary_releases_the_capture_commitment() {
        let mut tracker = super::super::tracked::Tracker::new();
        tracker.record_goal(1, 10, TrackedKind::Capture, Some(80), Some((30, 30)),
            Goal::Capture { target: 80 });
        assert_eq!(tracker.primary_goal(10), Some(Goal::Capture { target: 80 }));
        assert!(!tracker.confirmed_capture(1, 10, 80),
            "L3 has only asked; L2 has not yet accepted or refused");
        assert!(tracker.confirmed_capture(2, 10, 80),
            "the next frame may recruit after L2 had its say");
        tracker.record_goal(5, 10, TrackedKind::Move, None, Some((10, 10)),
            Goal::Move { cell: (10, 10) });
        assert_ne!(tracker.primary_goal(10), Some(Goal::Capture { target: 80 }));
        tracker.forget_primary(10);
        assert_eq!(tracker.primary_goal(10), None);
    }
}
