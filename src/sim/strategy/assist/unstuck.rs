//! Detect stalled walks and bounded left-right loops without re-issuing every
//! native movement command. A lateral step is a **temporary** Avoid override;
//! the mission layer either resumes a valid primary or asks for a new one.
//! Units in combat, repair, a player's hands or the native harvester cycle are
//! never forced out of their jobs by this recovery pass.

use std::collections::{BTreeMap, BTreeSet};

use crate::rules::locomotor_type::MovementZone;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::Observation;

use super::AssistBrain;
use super::advance::cell_distance;
use super::tracked::TrackedKind;

const NO_PROGRESS_FRAMES: u64 = 180;
const ESCAPE_COOLDOWN_FRAMES: u64 = 300;
const BOUNCE_LIMIT: u8 = 6;
const MAX_ESCAPES_PER_FRAME: usize = 2;

#[derive(Debug, Clone, Copy)]
struct Motion {
    destination: (u16, u16),
    last_cell: (u16, u16),
    before_last: Option<(u16, u16)>,
    best_distance: i32,
    last_progress_at: u64,
    bounce: u8,
    cooldown_until: u64,
}

#[derive(Debug, Default)]
pub(super) struct Recovery {
    motion: BTreeMap<u64, Motion>,
}

impl Recovery {
    /// True only after a lack of net progress, or enough A-B-A reversals to
    /// demonstrate a pathfinding oscillation. Movement *toward* the objective
    /// clears the timer even if the unit is still far from the destination.
    fn observe(&mut self, now: u64, id: u64, at: (u16,u16), dest: (u16,u16)) -> bool {
        let distance = cell_distance(at, dest);
        if distance == 0 {
            self.motion.remove(&id);
            return false;
        }
        let sample = self.motion.entry(id).or_insert(Motion {
            destination: dest, last_cell: at, before_last: None,
            best_distance: distance, last_progress_at: now,
            bounce: 0, cooldown_until: 0,
        });
        if sample.destination != dest {
            *sample = Motion {
                destination: dest, last_cell: at, before_last: None,
                best_distance: distance, last_progress_at: now,
                bounce: 0, cooldown_until: now.saturating_add(30),
            };
            return false;
        }
        if sample.last_cell != at {
            let reversed = sample.before_last == Some(at);
            sample.before_last = Some(sample.last_cell);
            sample.last_cell = at;
            sample.bounce = if reversed { sample.bounce.saturating_add(1) } else { 0 };
        }
        if distance < sample.best_distance {
            sample.best_distance = distance;
            sample.last_progress_at = now;
            sample.bounce = 0;
        }
        if now < sample.cooldown_until {
            return false;
        }
        if sample.bounce < BOUNCE_LIMIT
            && now.saturating_sub(sample.last_progress_at) < NO_PROGRESS_FRAMES
        {
            return false;
        }
        sample.last_progress_at = now;
        sample.bounce = 0;
        sample.cooldown_until = now.saturating_add(ESCAPE_COOLDOWN_FRAMES);
        true
    }

    fn retain(&mut self, live: &BTreeSet<u64>) {
        self.motion.retain(|id, _| live.contains(id));
    }
}

/// A short sideways move, *not* another guess at the blocked distant target.
/// Its native legality and movement-zone connectivity are checked before the
/// usual Outbox order gate. The two perpendicular sides are tried in stable
/// id order so all units do not repeatedly select the same escape cell.
fn step_around(
    obs: &Observation<'_>, zone: MovementZone,
    from: (u16,u16), to: (u16,u16), id: u64,
) -> Option<(u16,u16)> {
    let dx = i32::from(to.0) - i32::from(from.0);
    let dy = i32::from(to.1) - i32::from(from.1);
    let side = if dx.abs() >= dy.abs() { (0, 1) } else { (1, 0) };
    let dirs = if id % 2 == 0 { [1, -1] } else { [-1, 1] };
    for distance in 2..=4 {
        for sign in dirs {
            let x = i32::from(from.0) + side.0 * sign * distance;
            let y = i32::from(from.1) + side.1 * sign * distance;
            let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
                continue;
            };
            let cell = (x, y);
            if obs.is_ground_walkable(x, y) && obs.can_reach(zone, from, cell) {
                return Some(cell);
            }
        }
    }
    None
}

pub(super) fn run(
    brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox,
) {
    let now = obs.tick();
    let mut live = BTreeSet::new();
    let mut issued = 0;
    for unit in &brain.census.units {
        let id = unit.entity_id;
        // Never interrupt anyone deliberately standing still or managed by
        // the player, the refinery or a specialist lifecycle.
        if !unit.controllable() || !unit.traits.combat || unit.in_service()
            || obs.is_deployed(id) || obs.attack_target(id).is_some()
        {
            continue;
        }
        let Some(dest) = obs.destination(id) else {
            continue;
        };
        let Some(ty) = obs.object_type(rules, unit.type_id) else {
            continue;
        };
        if ty.movement_zone == MovementZone::Fly {
            continue;
        }
        live.insert(id);
        if !brain.unstuck.observe(now, id, unit.cell, dest)
            || issued >= MAX_ESCAPES_PER_FRAME
            || out.ordered_this_frame(id)
        {
            continue;
        }
        let Some(step) = step_around(obs, ty.movement_zone, unit.cell, dest, id) else {
            continue;
        };
        // Do not side-step into an observed enemy. Avoidance and combat micro
        // should retain the next decision when an active threat is present.
        if brain.census.enemies.iter().any(|enemy| {
            obs.is_cell_visible(enemy.cell.0, enemy.cell.1)
                && super::squared_distance(enemy.cell, step) <= 6 * 6
        }) {
            continue;
        }
        if super::orders::issue_tracked(
            &mut brain.tracked, brain.trace, out, rules, obs,
            id, TrackedKind::Avoid, None, Some(step), Goal::Move { cell: step },
        ) {
            issued += 1;
            log::debug!("auto play: {id} stalled at {:?} toward {:?}; step aside to {:?}",
                unit.cell, dest, step);
        }
    }
    brain.unstuck.retain(&live);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stationary_navigation_is_recovered_only_after_a_real_stall() {
        let mut recovery = Recovery::default();
        assert!(!recovery.observe(10, 7, (4, 4), (30, 4)));
        assert!(!recovery.observe(189, 7, (4, 4), (30, 4)));
        assert!(recovery.observe(190, 7, (4, 4), (30, 4)));
        assert!(!recovery.observe(191, 7, (4, 4), (30, 4)), "recovery has a cooldown");
    }

    #[test]
    fn progressing_navigation_is_not_called_stuck() {
        let mut recovery = Recovery::default();
        for i in 0..20 {
            assert!(!recovery.observe(i * 25, 9, (u16::try_from(i).unwrap(), 5), (40, 5)));
        }
    }

    #[test]
    fn two_cell_ping_pong_triggers_without_180_frames_waiting() {
        let mut recovery = Recovery::default();
        let mut found = false;
        for i in 0..20 {
            found |= recovery.observe(i * 5, 2, if i % 2 == 0 { (3, 3) } else { (4, 3) }, (20, 3));
        }
        assert!(found, "repeated backtracks do not count as progress");
    }

    #[test]
    fn new_target_resets_stall_history() {
        let mut recovery = Recovery::default();
        assert!(!recovery.observe(10, 4, (5,5), (25,5)));
        assert!(!recovery.observe(170, 4, (5,5), (15,15)));
        assert!(!recovery.observe(190, 4, (5,5), (15,15)));
    }
}
