//! Fire and move: shoot, step out while the gun reloads, come back when it is
//! loaded, and do it around whatever can actually shoot back.
//!
//! The cycle is one frame's arithmetic, not a state machine:
//!
//! - the gun is **ready** and the target is inside the unit's reach: nothing to
//!   say. The micro pass issues the `Attack` and the unit fires where it stands;
//!   the native mission closes the last cells by itself.
//! - the gun is ready and the target is **outside** the reach: the `Attack` is
//!   issued from here, because a kite that walked itself out of its own range
//!   has to be the one that walks back in — the micro pass only looks at what
//!   is already in reach.
//! - the gun is **reloading**: the unit takes one step away from the enemies
//!   that can reach it *where it stands*, and only from those. That is the
//!   difference between a kite and a rout: a tank that is out of its own range
//!   is not a reason to move, and a building the unit is shooting at is not a
//!   reason either.
//!
//! The step is chosen to leave the nearest reachable enemy as far as possible
//! while staying as close to the unit's own target as the choice allows, so a
//! unit with a tank on its left steps right and keeps the building it was
//! attacking in front of it. Ties break by a fixed direction order, never by
//! hash order.
//!
//! Nothing here remembers the band or the cycle: readiness, distance and the
//! enemies in reach are all read from the world every frame, which is what
//! makes the pass survive a target that dies, a gun that never reloads, and a
//! unit that is re-tasked mid-cycle.

use std::collections::BTreeMap;

use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::Observation;

use super::RetailBrain;
use super::threats::{Threat, ThreatIndex, covers};
use super::tracked::TrackedKind;

/// How far from the unit an enemy is considered at all, in cells. Further than
/// any retail weapon reaches (`[120mm] Range=5.75`, `[V3ROCKET] 18`), and it is
/// what bounds the bucket lookup: an enemy outside this cannot shoot where the
/// unit stands, so it is not a reason to step.
const THREAT_REACH_CELLS: i32 = 24;

/// Side of one threat bucket, in cells. One lookup covers the neighbourhood a
/// weapon reaches across, so a frame costs the enemies around a unit and not
/// the enemies on the map.
const BUCKET_CELLS: u16 = 16;

/// One step: the four straight directions and the four diagonals, in the order
/// ties are broken.
const STEPS: [(i32, i32); 8] = [
    (0, -1),
    (-1, 0),
    (1, 0),
    (0, 1),
    (-1, -1),
    (1, -1),
    (-1, 1),
    (1, 1),
];

/// What a cell has to put up with: how many enemies reach it, and how far the
/// nearest of them is.
///
/// Ordered as `(covered, -nearest)`: fewer reaches first, then more distance
/// from the closest one. A cell inside one tank's six-cell reach is not the
/// same as a cell inside it at five, and a step that cannot leave the reach
/// entirely still gains the half cell that makes the next shot come later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Exposure {
    pub(super) covered: i32,
    /// Distance to the nearest enemy that reaches the cell; `i32::MAX` when
    /// nothing does.
    pub(super) nearest: i32,
}

/// The exposure of `cell` to `threats`.
pub(super) fn exposure(cell: (u16, u16), threats: &[Threat]) -> Exposure {
    let mut covered = 0;
    let mut nearest = i32::MAX;
    for threat in threats {
        if !covers(cell, threat.cell, threat.reach) {
            continue;
        }
        covered += 1;
        nearest = nearest.min(chebyshev(cell, threat.cell));
    }
    Exposure { covered, nearest }
}

impl Exposure {
    /// Ranking key: less covered first, then further from the nearest.
    fn rank(self) -> (i32, i32) {
        (self.covered, -self.nearest)
    }
}

/// The step away from every threat that beats staying, or `None` when nothing
/// reaches the cell, every neighbour is worse, or the choice is a wash.
///
/// Ranking is `(exposure, distance to the unit's own target)`: first get out of
/// the nearest reachable enemy's reach, then stay as close to what the unit is
/// shooting at as the first rule allows. That is what keeps a unit with a tank
/// on its left stepping right instead of backing into the tank.
pub(super) fn step_out(
    cell: (u16, u16),
    threats: &[Threat],
    target: Option<(u16, u16)>,
) -> Option<(u16, u16)> {
    let here = exposure(cell, threats);
    if here.covered == 0 {
        return None;
    }
    let mut best: Option<((u16, u16), Exposure, i32)> = None;
    for (dx, dy) in STEPS {
        let x = i32::from(cell.0) + dx;
        let y = i32::from(cell.1) + dy;
        if x < 0 || y < 0 {
            continue;
        }
        let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
            continue;
        };
        let exposure = exposure((x, y), threats);
        let reach = target.map_or(0, |target| chebyshev((x, y), target));
        if best.is_none_or(|(_, best_exposure, best_reach)| {
            (exposure.rank(), reach) < (best_exposure.rank(), best_reach)
        }) {
            best = Some(((x, y), exposure, reach));
        }
    }
    best.filter(|(_, exposure, _)| exposure.rank() < here.rank())
        .map(|(cell, _, _)| cell)
}

pub(super) fn chebyshev(a: (u16, u16), b: (u16, u16)) -> i32 {
    super::threats::distance(a, b)
}

/// Whether any weapon `ty` carries has a `Parasite=yes` warhead.
///
/// The flag is the warhead's, not the type's: in the retail rules a terror
/// drone's jaw and a squid's tentacles carry it and nothing else does, and a
/// mod that puts one on a tank is answered without a name list here.
fn carries_parasite(rules: &RuleSet, ty: &crate::rules::object_type::ObjectType) -> bool {
    (0..8)
        .filter_map(|slot| ty.weapon_at(slot))
        .filter_map(|name| rules.weapon(name))
        .filter_map(|weapon| weapon.warhead.as_deref())
        .filter_map(|name| rules.warhead(name))
        .any(|warhead| warhead.parasite)
}

/// The neighbouring cell that puts the most ground between the unit and what
/// would board it, or `None` when no step is further away than standing still.
fn flee_step(
    cell: (u16, u16),
    danger: &[((u16, u16), i32)],
    obs: &Observation<'_>,
) -> Option<(u16, u16)> {
    let clearance = |c: (u16, u16)| {
        danger
            .iter()
            .map(|(other, _)| chebyshev(c, *other))
            .min()
            .unwrap_or(i32::MAX)
    };
    let here = clearance(cell);
    let mut best: Option<(i32, (u16, u16))> = None;
    for (dx, dy) in STEPS {
        let (Ok(x), Ok(y)) = (
            u16::try_from(i32::from(cell.0) + dx),
            u16::try_from(i32::from(cell.1) + dy),
        ) else {
            continue;
        };
        if !obs.is_ground_walkable(x, y) {
            continue;
        }
        let clear = clearance((x, y));
        if clear <= here {
            continue;
        }
        if best.is_none_or(|(best, _)| clear > best) {
            best = Some((clear, (x, y)));
        }
    }
    best.map(|(_, cell)| cell)
}

/// Shoot, step out, come back.
pub(super) fn run(
    brain: &mut RetailBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let tick = obs.tick();
    let mut index = ThreatIndex::default();
    let mut enemy_cell: BTreeMap<u64, ((u16, u16), i32)> = BTreeMap::new();
    // The enemies whose weapons do not shoot but board: a terror drone's jaw
    // and a squid's tentacles carry a `Parasite=yes` warhead, and the unit they
    // touch is taken over rather than damaged. Collected apart from the firing
    // threats because the answer to one is not the answer to the other.
    let mut parasites: Vec<((u16, u16), i32)> = Vec::new();
    for enemy in &brain.census.enemies {
        let Some(ty) = obs.object_type(rules, enemy.type_id) else {
            continue;
        };
        let reach = super::advance::weapon_range_cells(rules, ty, false);
        enemy_cell.insert(enemy.entity_id, (enemy.cell, reach));
        if reach > 0 {
            index.insert(Threat::gun(enemy.entity_id, enemy.cell, reach));
        }
        if reach > 0 && carries_parasite(rules, ty) {
            parasites.push((enemy.cell, reach));
        }
    }

    for unit in &brain.census.units {
        if !unit.traits.combat || !unit.controllable() {
            continue;
        }
        // A walk-in or a depot visit owns the destination; a kite `Move` over
        // it ends the visit.
        if unit.in_service() {
            continue;
        }
        if obs.is_deployed(unit.entity_id) {
            continue;
        }
        if super::orders::ordered_this_frame(out, unit.entity_id) {
            continue;
        }
        let Some(ty) = obs.object_type(rules, unit.type_id) else {
            continue;
        };
        if ty.category == ObjectCategory::Aircraft {
            continue;
        }
        // A boarding is not a fight. A `Parasiteable=` unit — every soldier and
        // the dog — that walks within a parasite weapon's reach is taken over
        // outright: there is no exchange of fire to win, so the step out comes
        // before every other decision in this pass, loaded gun included.
        if ty.parasiteable {
            let danger: Vec<((u16, u16), i32)> = parasites
                .iter()
                .filter(|(cell, reach)| chebyshev(unit.cell, *cell) <= *reach)
                .copied()
                .collect();
            if !danger.is_empty() {
                if let Some(step) = flee_step(unit.cell, &danger, obs)
                    && brain.tracked.admits(
                        tick,
                        unit.entity_id,
                        TrackedKind::Kite,
                        None,
                        Some(step),
                    )
                {
                    brain
                        .tracked
                        .record(tick, unit.entity_id, TrackedKind::Kite, None, Some(step));
                    super::orders::issue(
                        brain,
                        out,
                        rules,
                        obs,
                        unit.entity_id,
                        Goal::Move { cell: step },
                    );
                }
                continue;
            }
        }
        // A burst weapon mid-cycle is not stepped out of. The step is a Move,
        // and a Move is the assignment that empties the burst — the gun comes
        // back three frames later as if none of it had been fired. The kite
        // pass is a re-position, so it is the one that waits; a single-shot
        // weapon has no cycle to break and is kited as before.
        if super::micro::firing(obs, unit.mission, unit.entity_id)
            && super::advance::weapon_burst(rules, ty, false) > 1
        {
            continue;
        }
        let mine = super::advance::weapon_range_cells(rules, ty, false);
        // What the unit is shooting at: the native mission's own choice. A unit
        // that was never given one does not get an attack from here — the micro
        // pass is where targets are chosen, and a pass that both moves and
        // picks would drag guards off their post.
        let native_target = obs
            .attack_target(unit.entity_id)
            .and_then(|id| enemy_cell.get(&id).map(|(cell, _)| (id, *cell)));
        // A target the index does not hold is nobody's building: a neutral
        // blocker the unit was sent to clear. Stepping out of it is a Move, and
        // a Move ends the attack — and nothing downstream would send the unit
        // back, because only the ledger knows where that building stands. It
        // holds its ground and finishes the demolition.
        if native_target.is_none() && obs.attack_target(unit.entity_id).is_some() {
            continue;
        }
        // The direction half may use whatever is closest when the unit has no
        // target: stepping away from a tank is right even for a unit that was
        // shooting a building.
        let direction_target = native_target.map(|(_, cell)| cell).or_else(|| {
            brain
                .census
                .enemies
                .iter()
                .filter(|enemy| chebyshev(unit.cell, enemy.cell) <= mine.max(1))
                .min_by_key(|enemy| chebyshev(unit.cell, enemy.cell))
                .map(|enemy| enemy.cell)
        });

        let threats: Vec<Threat> = index.near(unit.cell, THREAT_REACH_CELLS);

        if obs.weapon_ready(unit.entity_id) {
            // Loaded. If the target is out of reach, walk back in: the micro
            // pass only orders what is already in reach, so a kite that stepped
            // out would otherwise stand at its own edge and never fire again.
            let Some((target_id, target_cell)) = native_target else {
                continue;
            };
            if chebyshev(unit.cell, target_cell) <= mine {
                continue;
            }
            if !brain.tracked.admits(
                tick,
                unit.entity_id,
                TrackedKind::Kite,
                Some(target_id),
                None,
            ) {
                continue;
            }
            brain.tracked.record(
                tick,
                unit.entity_id,
                TrackedKind::Kite,
                Some(target_id),
                None,
            );
            super::orders::issue(
                brain,
                out,
                rules,
                obs,
                unit.entity_id,
                Goal::Attack { target: target_id },
            );
            continue;
        }

        // Reloading: step out of what can hit this cell, towards the target's
        // side so the next shot is one step back.
        //
        // A retreat from something faster was tried and is not here. The unit
        // moves at `Speed=7` against enemies at `Speed=15`, so the step of one
        // cell cannot open the distance before the gun is loaded. Standing and
        // shooting seemed the better trade. Measured, enemy deaths over own
        // deaths at 24000 ticks: 1.38/2.57/1.86/13.5 without it and
        // 1.17/1.46/8.0/2.35 with it — three of four seeds worse, and seed 42
        // went from 4 own deaths to 20. Reverted; the numbers are the reason.
        let Some(step) = step_out(unit.cell, &threats, direction_target) else {
            continue;
        };
        if !obs.is_ground_walkable(step.0, step.1) {
            continue;
        }
        // The step and the return trip keep separate tracked records — the
        // target of one is a unit id and of the other a cell — so they cannot
        // block each other in the same term.
        if !brain
            .tracked
            .admits(tick, unit.entity_id, TrackedKind::Kite, None, Some(step))
        {
            continue;
        }
        brain
            .tracked
            .record(tick, unit.entity_id, TrackedKind::Kite, None, Some(step));
        super::orders::issue(
            brain,
            out,
            rules,
            obs,
            unit.entity_id,
            Goal::Move { cell: step },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn threat(cell: (u16, u16), reach: i32) -> Threat {
        Threat::gun(0, cell, reach)
    }

    /// A tank on the left of the unit, the building it is shooting in front:
    /// the step goes right, away from the tank and no further from the target
    /// than it has to be.
    #[test]
    fn a_tank_on_the_left_steps_the_unit_right() {
        let threats = [threat((6, 10), 6)];
        let step = step_out((10, 10), &threats, Some((14, 10))).expect("a step");
        assert_eq!(step, (11, 10), "right, not into the tank");
    }

    /// An enemy that cannot reach the cell is not a reason to move: a unit
    /// shooting a building across the map does not back away from nothing.
    #[test]
    fn an_enemy_out_of_its_own_range_is_not_a_threat() {
        let threats = [threat((0, 10), 6)];
        assert_eq!(step_out((10, 10), &threats, Some((14, 10))), None);
    }

    /// One tank reaching, the other far out of its own reach: the step goes
    /// straight away from the one that can shoot, and being inside its reach by
    /// five cells instead of four is the gain that makes the next shot later.
    #[test]
    fn a_step_moves_away_from_the_nearest_reach() {
        let threats = [threat((10, 6), 9), threat((10, 30), 9)];
        let step = step_out((10, 10), &threats, None).expect("a step");
        assert_eq!(step, (10, 11), "down, away from the tank");
        assert!(
            exposure(step, &threats).rank() < exposure((10, 10), &threats).rank(),
            "{step:?} is no better than standing"
        );
    }

    /// Between two tanks at equal distance no single step improves anything —
    /// every step is closer to one of them — so the unit stands and shoots.
    #[test]
    fn a_step_with_no_gain_is_not_taken() {
        let threats = [threat((10, 4), 9), threat((10, 16), 9)];
        assert_eq!(step_out((10, 10), &threats, None), None);
    }

    /// Exposure counts the reachable enemies and weighs the nearest one: a cell
    /// inside two reaches is worse than one inside a single farther reach.
    #[test]
    fn exposure_counts_reaches_and_nearest_distance() {
        let two = [threat((10, 10), 4), threat((11, 10), 4)];
        let one = [threat((10, 8), 4)];
        assert!(exposure((10, 10), &two).covered > exposure((10, 10), &one).covered);
        assert!(
            exposure((10, 4), &one).rank() < exposure((10, 10), &one).rank(),
            "further from the reach is better"
        );
        assert_eq!(exposure((30, 30), &two).covered, 0);
    }

    /// Nothing covering the cell means no order: a pass that moves units
    /// outside every reach spends a command a frame for nothing.
    #[test]
    fn a_safe_cell_asks_for_no_step() {
        assert_eq!(step_out((10, 10), &[threat((20, 20), 6)], None), None);
        assert_eq!(step_out((10, 10), &[], None), None);
    }

    /// Distance is Chebyshev, the same metric the reach is written in.
    #[test]
    fn distance_is_chebyshev() {
        assert_eq!(chebyshev((10, 10), (13, 11)), 3);
        assert_eq!(chebyshev((10, 10), (10, 10)), 0);
    }
}
