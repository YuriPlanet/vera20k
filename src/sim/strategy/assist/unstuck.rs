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
    previous_destination: Option<(u16,u16)>,
    destination_flips: u8,
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
            destination: dest, previous_destination: None, destination_flips: 0,
            last_cell: at, before_last: None,
            best_distance: distance, last_progress_at: now,
            bounce: 0, cooldown_until: 0,
        });
        if sample.destination != dest {
            // Two nearby waypoints alternating while the unit stands in the
            // same corner is not a fresh mission on every tick. Keep the
            // original no-progress clock and count successive A/B/A flips.
            let close = cell_distance(sample.destination, dest) <= 4
                && cell_distance(sample.last_cell, at) <= 3;
            if !close {
                *sample = Motion {
                    destination: dest, previous_destination: None,
                    destination_flips: 0, last_cell: at, before_last: None,
                    best_distance: distance, last_progress_at: now,
                    bounce: 0, cooldown_until: now.saturating_add(30),
                };
                return false;
            }
            let returned = sample.previous_destination == Some(dest);
            sample.destination_flips = if returned {
                sample.destination_flips.saturating_add(1)
            } else { 0 };
            sample.previous_destination = Some(sample.destination);
            sample.destination = dest;
            sample.last_cell = at;
            sample.best_distance = distance;
            if now < sample.cooldown_until { return false; }
            if sample.destination_flips < 6
                && now.saturating_sub(sample.last_progress_at) < NO_PROGRESS_FRAMES
            { return false; }
            sample.last_progress_at = now;
            sample.destination_flips = 0;
            sample.cooldown_until = now.saturating_add(ESCAPE_COOLDOWN_FRAMES);
            return true;
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
            sample.destination_flips = 0;
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
/// Bounded local escape planner. The old two-cell perpendicular shuffle
/// chose the *first* traversable square, then discarded it if dangerous;
/// another safe square was never considered. Evaluate several waypoints in
/// eight directions and score them only if the native connectivity graph can
/// reach them and today's *visible* guns cannot cover them.
fn step_around(
    rules: &RuleSet, obs: &Observation<'_>, zone: MovementZone,
    from: (u16,u16), to: (u16,u16), id: u64,
) -> Option<(u16,u16)> {
    let dx = i32::from(to.0) - i32::from(from.0);
    let dy = i32::from(to.1) - i32::from(from.1);
    let offsets = [(1,0),(-1,0),(0,1),(0,-1),
        (1,1),(-1,1),(1,-1),(-1,-1)];
    let mut best: Option<((i32,i32,i32,u16,u16),(u16,u16))> = None;
    for distance in [3, 6, 9] {
        for (sx,sy) in offsets {
            // Do not just drive forward again into the same blocked approach.
            // A perpendicular side turn or a backward offset is a detour;
            // progress toward the original goal is still a useful tie-break.
            let dot = dx * sx + dy * sy;
            let cross = (dx * sy - dy * sx).abs();
            if dot > 0 && cross < (dx.abs()+dy.abs()).max(1)/3 { continue; }
            let (Ok(x), Ok(y)) = (
                u16::try_from(i32::from(from.0) + sx*distance),
                u16::try_from(i32::from(from.1) + sy*distance),
            ) else { continue; };
            let cell = (x,y);
            if !obs.is_cell_in_playfield(x,y) || !obs.is_ground_walkable(x,y)
                || !obs.can_reach(zone,from,cell)
                || super::tactics::unsafe_post(rules,obs,id,cell)
                || obs.visible_enemy_entities().any(|enemy|
                    super::squared_distance(enemy.cell,cell) <= 6*6)
            { continue; }
            let key = (if cross > 0 {0} else {1},
                cell_distance(cell,to), distance, x, y);
            if best.is_none_or(|(current,_)|key<current) { best=Some((key,cell)); }
        }
    }
    best.map(|(_,cell)|cell)
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
        if !unit.controllable() || !(unit.traits.combat || unit.traits.yard_maker)
            || unit.in_service() || obs.is_deployed(id)
            || brain.avoid_retreat_until.get(&id).is_some_and(|until| now < *until)
            || obs.attack_target(id).is_some()
            || super::orders::committed_garrison_entry(&brain.tracked, rules, obs, id)
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
        // A six-way lateral shuffle is not an escape from a prism/tesla gun.
        // If a long walk has made no progress *inside* a visible weapon band,
        // abandon that goal for a safe rally and cool down the attack orders.
        // A standalone retreat still uses the normal L2 gate, not a sim write.
        if super::tactics::unsafe_post(rules, obs, id, unit.cell) {
            let safe = brain.census.rally.or(brain.census.base_cell)
                .filter(|cell| *cell != unit.cell
                    && super::orders::reachable(rules, obs, id, *cell)
                    && !super::tactics::unsafe_post(rules, obs, id, *cell));
            let goal = safe.map_or(Goal::Stop, |cell| Goal::Move { cell });
            if out.want(id, 0, goal) {
                brain.tracked.forget_primary(id);
                brain.avoid_retreat_until.insert(id, now.saturating_add(450));
                issued += 1;
                log::debug!("auto play: stalled {} inside hostile guns; reset {:?}", id, goal);
            }
            continue;
        }
        let Some(step) = step_around(rules, obs, ty.movement_zone, unit.cell, dest, id) else {
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

#[cfg(all(test, tests_sim_p_s))]
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
    fn alternating_nearby_waypoints_do_not_reset_stall_forever() {
        let mut recovery = Recovery::default();
        let mut escaped = false;
        for i in 0..18 {
            let destination = if i % 2 == 0 { (20, 3) } else { (21, 3) };
            escaped |= recovery.observe(i * 6, 19, (4, 3), destination);
        }
        assert!(escaped, "changing between nearby goals must not mask a stuck unit");
    }

    #[test]
    fn new_target_resets_stall_history() {
        let mut recovery = Recovery::default();
        assert!(!recovery.observe(10, 4, (5,5), (25,5)));
        assert!(!recovery.observe(170, 4, (5,5), (15,15)));
        assert!(!recovery.observe(190, 4, (5,5), (15,15)));
    }
}
