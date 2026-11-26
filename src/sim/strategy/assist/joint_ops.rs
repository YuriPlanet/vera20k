//! V38 joint operations: a small, committed screen and real rear fire support
//! make a *visible* high-value neutral prize safe for the existing engineer
//! Capture/escort pipeline. The remaining Attack fist, the Defend squad,
//! Harass and Scout continue doing their own jobs.
//!
//! There is exactly one operation per house, with durable actor membership and
//! a fixed objective. Only native Move/Attack/AttackMove/Guard goals are sent;
//! never create a building, teleport a soldier or attack an object in fog.
//! In particular a submitted Capture is not an ownership receipt. Every order
//! is checked by native admission and L2 feedback as usual.

use std::collections::{BTreeMap, BTreeSet};

use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::{EnemyEntity, Observation};

use super::AssistBrain;
use super::asset_ops::CoalitionStatus;
use super::census::{Slot, Unit};
use super::orders;
use super::squads::Role;
use super::tracked::TrackedKind;
use super::{advance, squared_distance, tactics};

const PLAN_INTERVAL: u64 = 90;
// The ordinary campaign requires this many attackers to pass its own size
// gate; detaching below it would "preserve" a fist that can never attack.
const MAIN_FIST_RESERVE: usize = super::squads::ARMY_TO_ATTACK;
const MIN_FRONT: usize = 3;
const MAX_FRONT: usize = 4;
const MAX_REAR: usize = 2;
const ASSEMBLE_LIMIT: u64 = 840;
const CLEAR_LIMIT: u64 = 1_200;
const SCREEN_LIMIT: u64 = 1_050;
const OPERATION_LIMIT: u64 = 3_000;
const WITHDRAW_TIME: u64 = 180;
const RETRY_DELAY: u64 = 1_350;
const ORDERS_PER_FRAME: usize = 4;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Stage {
    Assemble,
    Clear,
    Screen,
    Withdraw,
}

#[derive(Debug)]
struct Operation {
    prize: u64,
    cell: (u16,u16),
    rally: (u16,u16),
    started: u64,
    changed: u64,
    stage: Stage,
    front: BTreeSet<u64>,
    rear: BTreeSet<u64>,
    /// Only orders issued *by this director*. On release we must not erase
    /// a newer dodge, escort or player-authorized order on the same actor.
    own_orders: BTreeMap<u64, (TrackedKind, Goal)>,
}

impl Operation {
    fn contains(&self, actor: u64) -> bool {
        self.front.contains(&actor) || self.rear.contains(&actor)
    }

    fn move_to(&mut self, stage: Stage, now: u64) {
        if self.stage != stage {
            log::info!("auto play: joint operation {} {:?} -> {:?} at tick {}",
                self.prize, self.stage, stage, now);
            self.stage = stage;
            self.changed = now;
        }
    }
}

/// A dedicated director, not a new native TeamClass or an all-map enemy AI.
#[derive(Debug, Default)]
pub(super) struct Director {
    active: Option<Operation>,
    retry_after: BTreeMap<u64,u64>,
    next_plan: u64,
    completed: u32,
    withdrawn: u32,
}

impl Director {
    pub(super) fn reserves(&self, actor: u64) -> bool {
        self.active.as_ref().is_some_and(|op| op.contains(actor))
    }

    pub(super) fn prize(&self) -> Option<u64> {
        self.active.as_ref().and_then(|op|
            (op.stage != Stage::Withdraw).then_some(op.prize))
    }

    fn finish(&mut self, brain: &mut AssistBrain, op: Operation,
        out: &mut Outbox, success: bool, now: u64,
    ) {
        // A cap on repeated costly expeditions, but never a permanent
        // blacklist. Success may still be followed by a new strategy target.
        self.retry_after.insert(op.prize, now.saturating_add(RETRY_DELAY));
        if success { self.completed = self.completed.saturating_add(1); }
        else { self.withdrawn = self.withdrawn.saturating_add(1); }
        for (actor, (kind, goal)) in op.own_orders {
            if brain.tracked.primary_goal(actor) != Some(goal) { continue; }
            brain.tracked.forget_primary(actor);
            brain.tracked.void(actor, kind);
            // A native AttackMove survives forgetting the tracker. On an
            // aborted action, stop the *old* march before regular defence
            // or the next campaign may choose a new purpose for this actor.
            if !success && !out.ordered_this_frame(actor) {
                let _ = out.want(actor, 0, Goal::Stop);
            }
        }
        log::info!("auto play: joint operation {} ends success={} completed={} withdrawn={}",
            op.prize, success, self.completed, self.withdrawn);
    }

    /// Only currently visible fighters can be assigned. No captured MCV,
    /// specialist, hero, suicide unit, harvester, transport or player scope
    /// ever becomes an expendable front-line body in this operation.
    fn candidate(brain: &AssistBrain, obs: &Observation<'_>, unit: &Unit,
        members: &BTreeSet<u64>,
    ) -> bool {
        members.contains(&unit.entity_id)
            && matches!(unit.slot, Slot::Infantry | Slot::Vehicle)
            && brain.census.available(unit) && unit.traits.armed
            && !unit.traits.aircraft && !unit.traits.suicide
            && !unit.traits.irreplaceable && !unit.traits.yard_maker
            && !unit.traits.harvester && !unit.traits.carries
            && !obs.is_deployed(unit.entity_id)
            && unit.health_permille >= 650 && unit.points > 0
            && !brain.crate_claims.contains_key(&unit.entity_id)
            && !brain.crate_returns.contains_key(&unit.entity_id)
            && !brain.specialist_shuttles.contains_key(&unit.entity_id)
            && !brain.specialist_shuttles.values().any(|job|
                job.passenger == unit.entity_id)
            && !brain.asset_ops.has_muster(unit.entity_id)
            && !brain.escorts.values().any(|escort| escort.guard == unit.entity_id)
            && !brain.tracked.tasks.get(unit.entity_id).is_some()
    }

    fn create(brain: &AssistBrain, rules: &RuleSet,
        obs: &Observation<'_>, retry: &BTreeMap<u64,u64>,
    ) -> Option<Operation> {
        if brain.director.under_attack_now() || brain.director.recovering_under_fire()
            || brain.mobilization.active() || brain.campaign.economy_resting()
            || brain.siege_charge.is_some()
            || brain.blackout.is_some()
        { return None; }
        let attack = brain.squads.squad(Role::Attack)?;
        // Never impair the committed main fist. A wealthy house with a large
        // stockpile may run a limited side operation; a small house may not.
        let main_available = super::readiness::attack_members(brain);
        if main_available < MAIN_FIST_RESERVE + MIN_FRONT { return None; }
        let home = brain.census.base_cell?;
        let (prize, cell) = brain.asset_ops.coalition_target(rules,obs,home,retry)?;
        let mut eligible: Vec<Unit> = brain.census.units.iter().copied()
            .filter(|unit| Self::candidate(brain, obs, unit, &attack.members))
            .collect();
        if eligible.len() < MAIN_FIST_RESERVE + MIN_FRONT { return None; }
        // Far-away reserve bodies must not make the search quadratic on
        // twenty-thousand-object / 120-house scenarios. Sort once, evaluate
        // weapons and zone queries for only the nearest eligible detachment.
        eligible.sort_by_key(|unit|
            (squared_distance(unit.cell, cell), unit.entity_id));
        eligible.truncate(96);
        let front: Vec<Unit> = eligible.iter().copied().filter(|unit|
            advance::fire_support::support_reach(rules, obs, unit) < 7).collect();
        if front.len() < MIN_FRONT { return None; }
        let post = choose_rally(rules, obs, home, cell, &front)?;
        eligible.retain(|unit| orders::reachable(rules, obs, unit.entity_id, post));
        let mut screen: Vec<Unit> = eligible.iter().copied().filter(|unit|
            advance::fire_support::support_reach(rules, obs, unit) < 7).collect();
        let mut guns: Vec<Unit> = eligible.iter().copied().filter(|unit|
            advance::fire_support::support_reach(rules, obs, unit) >= 7).collect();
        screen.sort_by_key(|unit| (squared_distance(unit.cell, post), unit.entity_id));
        guns.sort_by_key(|unit| (squared_distance(unit.cell, post), unit.entity_id));
        if screen.len() < MIN_FRONT { return None; }
        let assigned_front: BTreeSet<_> = screen.iter().take(MAX_FRONT)
            .map(|unit| unit.entity_id).collect();
        let assigned_rear: BTreeSet<_> = guns.iter().take(MAX_REAR)
            .map(|unit| unit.entity_id).collect();
        if !reserve_permits(main_available, assigned_front.len() + assigned_rear.len()) {
            return None;
        }
        log::info!("auto play: joint operation assembles prize={} cell={:?} rally={:?} front={} guns={}",
            prize, cell, post, assigned_front.len(), assigned_rear.len());
        Some(Operation {
            prize, cell, rally: post, started: obs.tick(), changed: obs.tick(),
            stage: Stage::Assemble, front: assigned_front, rear: assigned_rear,
            own_orders: BTreeMap::new(),
        })
    }

    pub(super) fn run(brain: &mut AssistBrain, rules: &RuleSet,
        obs: &Observation<'_>, out: &mut Outbox,
    ) {
        let now = obs.tick();
        let mut director = std::mem::take(&mut brain.joint_ops);
        director.retry_after.retain(|_, until| *until > now);
        if director.active.is_none() && now >= director.next_plan {
            director.next_plan = now.saturating_add(PLAN_INTERVAL);
            director.active = Self::create(brain, rules, obs, &director.retry_after);
        }
        let Some(mut op) = director.active.take() else {
            brain.joint_ops = director;
            return;
        };
        // An operation is one bounded strategic task, not a per-frame enemy
        // scanner. An actual home raid must still preempt it immediately.
        if now % 15 != 0 && !brain.director.under_attack_now() && !brain.director.recovering_under_fire()
            && !brain.mobilization.active() && !brain.campaign.economy_resting() {
            director.active = Some(op);
            brain.joint_ops = director;
            return;
        }
        // Squad ownership can change at its own 300-frame regroup interval.
        // An actor reassigned to home defence must not remain a borrowed
        // operation fighter simply because its entity ID is still alive.
        let current_attack: BTreeSet<u64> = brain.squads.squad(Role::Attack)
            .map(|squad| squad.members.clone()).unwrap_or_default();
        let reassigned: Vec<u64> = op.front.union(&op.rear)
            .filter(|id| !current_attack.contains(*id)).copied().collect();
        for id in reassigned {
            if let Some((kind, goal)) = op.own_orders.remove(&id)
                && brain.tracked.primary_goal(id) == Some(goal)
            {
                brain.tracked.void(id,kind);
                brain.tracked.forget_primary(id);
                if !out.ordered_this_frame(id) { let _ = out.want(id,0,Goal::Stop); }
            }
            op.front.remove(&id);
            op.rear.remove(&id);
        }
        let status = brain.asset_ops.coalition_status(obs, op.prize);
        // Neither new fog information nor a unit death is inferred here.
        // Our own roster and an actually owned building are lawful inputs.
        let roster: Vec<Unit> = brain.census.units.iter().copied()
            .filter(|unit| op.contains(unit.entity_id) && brain.census.available(unit)
                && unit.health_permille >= 350)
            .collect();
        let remaining_front = roster.iter().filter(|unit|
            op.front.contains(&unit.entity_id)).count();
        let revoked = brain.director.under_attack_now()
            || brain.director.recovering_under_fire()
            || brain.mobilization.active() || brain.campaign.economy_resting()
            || !obs.forces_enabled()
            || now.saturating_sub(op.started) > OPERATION_LIMIT
            || remaining_front < MIN_FRONT
            || matches!(status, CoalitionStatus::Unseen | CoalitionStatus::Unavailable)
            || brain.siege_charge.is_some()
            || brain.blackout.is_some();
        if status == CoalitionStatus::Owned {
            director.finish(brain, op, out, true, now);
            brain.joint_ops = director;
            return;
        }
        if brain.director.under_attack_now() {
            // A real base alarm is an immediate break of the detachment,
            // not 180 more frames of borrowed defenders ignoring home.
            director.finish(brain, op, out, false, now);
            brain.joint_ops = director;
            return;
        }
        if revoked { op.move_to(Stage::Withdraw, now); }
        if op.stage == Stage::Assemble {
            let elapsed = now.saturating_sub(op.changed);
            let fronts_here = roster.iter().filter(|unit|
                op.front.contains(&unit.entity_id)
                    && advance::cell_distance(unit.cell, op.rally) <= 9).count();
            let guns_here = op.rear.is_empty() || roster.iter().any(|unit|
                op.rear.contains(&unit.entity_id)
                    && advance::cell_distance(unit.cell, op.rally) <= 16);
            if fronts_here >= MIN_FRONT && guns_here {
                op.move_to(Stage::Clear, now);
            } else if elapsed >= ASSEMBLE_LIMIT {
                op.move_to(Stage::Withdraw, now);
            }
        }
        // Only currently *visible* and operational defences count. Unarmed
        // enemy buildings are not enemies to destroy before Capture.
        let threats: Vec<EnemyEntity> = obs.visible_enemy_entities()
            .filter(|enemy| advance::cell_distance(enemy.cell, op.cell) <= 16)
            .filter(|enemy| obs.object_type(rules, enemy.type_id).is_some_and(|ty|
                super::convergence::mobile_threat(rules,obs,enemy)
                    || super::siege_counter::dangerous_battery(rules, ty)
                        && obs.visible_enemy_building_operational(
                        rules, enemy.entity_id)))
            .take(128).collect();
        if op.stage == Stage::Clear {
            let elapsed = now.saturating_sub(op.changed);
            if threats.is_empty() && matches!(status,
                CoalitionStatus::Open | CoalitionStatus::Capturing)
            { op.move_to(Stage::Screen, now); }
            else if elapsed >= CLEAR_LIMIT {
                op.move_to(Stage::Withdraw, now);
            } else {
                let theirs = brain.census.enemy_points(rules, obs, op.cell, 16);
                let ours: i32 = roster.iter().map(|unit| unit.points).sum();
                if !viable_local_fight(ours, theirs, threats.len()) {
                    op.move_to(Stage::Withdraw, now);
                }
            }
        }
        if op.stage == Stage::Screen {
            let elapsed = now.saturating_sub(op.changed);
            if status == CoalitionStatus::Threatened && !threats.is_empty() {
                op.move_to(Stage::Clear, now);
            } else if elapsed >= SCREEN_LIMIT {
                // This window gives production and the existing Capture
                // pipeline time to send an engineer; it cannot hold the fist
                // hostage forever if L2 repeatedly refuses the capture.
                op.move_to(Stage::Withdraw, now);
            }
        }
        if op.stage == Stage::Withdraw {
            Self::withdraw(brain, rules, obs, out, &mut op);
            if now.saturating_sub(op.changed) >= WITHDRAW_TIME {
                director.finish(brain, op, out, false, now);
            } else {
                director.active = Some(op);
            }
        } else {
            Self::command(brain, rules, obs, out, &mut op, &roster, &threats);
            director.active = Some(op);
        }
        brain.joint_ops = director;
    }

    fn order(brain: &mut AssistBrain, rules: &RuleSet, obs: &Observation<'_>,
        out: &mut Outbox, op: &mut Operation, unit: u64,
        kind: TrackedKind, target: Option<u64>, cell: Option<(u16,u16)>,
        goal: Goal,
    ) -> bool {
        if out.ordered_this_frame(unit) { return false; }
        if orders::issue_tracked(&mut brain.tracked, brain.trace, out,
            rules, obs, unit, kind, target, cell, goal)
        {
            op.own_orders.insert(unit, (kind, goal));
            true
        } else { false }
    }

    fn command(brain: &mut AssistBrain, rules: &RuleSet,
        obs: &Observation<'_>, out: &mut Outbox, op: &mut Operation,
        roster: &[Unit], threats: &[EnemyEntity],
    ) {
        let mut orders_left = ORDERS_PER_FRAME;
        // Long-range guns never get the same frontal AttackMove as a tank.
        // Reuse V35's native-weapon-aware posts, Gunner mode and MinimumRange.
        if matches!(op.stage, Stage::Clear | Stage::Screen) && !op.rear.is_empty() {
            let members: Vec<&Unit> = roster.iter().filter(|unit|
                op.front.contains(&unit.entity_id) || op.rear.contains(&unit.entity_id))
                .collect();
            let before: BTreeSet<u64> = out.orders.iter().map(|order| order.actor).collect();
            let _ = advance::fire_support::push_coordinated(brain, rules, obs, out,
                &members, op.cell);
            for id in &op.rear {
                if before.contains(id) || !out.ordered_this_frame(*id) { continue; }
                if let (Some(kind), Some(goal)) = (brain.tracked.live_kind(obs.tick(), *id),
                    brain.tracked.primary_goal(*id))
                {
                    op.own_orders.insert(*id, (kind, goal));
                }
            }
        }
        // Materialize member references before mutating the operation's
        // order ledger; keeping a filter closure borrowing `op.front` alive
        // would overlap the mutable operation borrow in `Self::order`.
        let front: Vec<&Unit> = roster.iter().filter(|unit|
            op.front.contains(&unit.entity_id)).collect();
        for unit in front {
            if orders_left == 0 { break; }
            if out.ordered_this_frame(unit.entity_id)
                || brain.tracked.pending_resume(obs.tick(), unit.entity_id).is_some()
            { continue; }
            let desired = match op.stage {
                Stage::Assemble => Some((TrackedKind::AttackMove, None,
                    Some(op.rally), Goal::AttackMove { cell: op.rally })),
                Stage::Clear => {
                    let reach = obs.entity_type(rules,unit.entity_id)
                        .map_or(0, |ty| advance::weapon_range_cells(rules,ty,false));
                    let reachable_threat = threats.iter().filter(|enemy|
                        // An escort is not a sacrificial tank. Live towers
                        // which outrange it are assigned to the counter
                        // production/rear-gun commander, not this frontliner.
                        !super::siege_counter::avoid_solo_tower_attack(
                            rules,obs,unit.entity_id,enemy.entity_id)
                        && obs.can_hurt(rules, unit.entity_id, enemy.entity_id)
                        && orders::admits(rules, obs, unit.entity_id,
                            &Goal::Attack { target: enemy.entity_id })
                        && advance::cell_distance(unit.cell,enemy.cell) <= reach + 2
                    ).min_by_key(|enemy| (
                        !obs.entity_type(rules, enemy.entity_id)
                            .is_some_and(|ty| super::siege_counter::dangerous_battery(rules, ty)),
                        advance::cell_distance(enemy.cell,unit.cell), enemy.entity_id,
                    ));
                    if let Some(enemy) = reachable_threat {
                        Some((TrackedKind::Attack, Some(enemy.entity_id),
                            Some(enemy.cell), Goal::Attack { target: enemy.entity_id }))
                    } else {
                        let post = safe_approach(rules, obs, unit, op.rally, op.cell)
                            .unwrap_or(op.rally);
                        (advance::cell_distance(unit.cell, post) > 3).then_some((
                            TrackedKind::AttackMove, None, Some(post),
                            Goal::AttackMove { cell: post }))
                    }
                }
                Stage::Screen => {
                    let post = safe_approach(rules, obs, unit, op.rally, op.cell)
                        .unwrap_or(op.rally);
                    if advance::cell_distance(unit.cell,post) <= 3 {
                        Some((TrackedKind::Guard, None, None, Goal::Guard))
                    } else {
                        Some((TrackedKind::AttackMove, None, Some(post),
                            Goal::AttackMove { cell: post }))
                    }
                }
                Stage::Withdraw => None,
            };
            let Some((kind,target,cell,goal)) = desired else { continue };
            if (kind == TrackedKind::AttackMove &&
                obs.destination(unit.entity_id) == cell)
                || (kind == TrackedKind::Attack &&
                    obs.attack_target(unit.entity_id) == target) { continue; }
            if Self::order(brain,rules,obs,out,op,unit.entity_id,
                kind,target,cell,goal)
            { orders_left -= 1; }
        }
        // The second echelon has a rendezvous too. Without this leg guns
        // remained at the factory while the three screening tanks waited for
        // them forever, so an otherwise ready operation timed out at Rally.
        if op.stage == Stage::Assemble {
            let rear: Vec<&Unit> = roster.iter().filter(|unit|
                op.rear.contains(&unit.entity_id)).collect();
            for gun in rear {
                if orders_left == 0 { break; }
                if out.ordered_this_frame(gun.entity_id)
                    || obs.destination(gun.entity_id) == Some(op.rally)
                    || advance::cell_distance(gun.cell, op.rally) <= 10
                    || !orders::reachable(rules, obs, gun.entity_id, op.rally)
                    || tactics::unsafe_post(rules, obs, gun.entity_id, op.rally)
                    || tactics::lethal_approach(rules, obs,
                        gun.entity_id, gun.cell, op.rally).is_some()
                { continue; }
                if Self::order(brain,rules,obs,out,op,gun.entity_id,
                    TrackedKind::Move, None, Some(op.rally),
                    Goal::Move { cell: op.rally })
                { orders_left -= 1; }
            }
        }
    }

    fn withdraw(brain: &mut AssistBrain, rules: &RuleSet,
        obs: &Observation<'_>, out: &mut Outbox, op: &mut Operation,
    ) {
        // On a raid or failed clearance, do not leave a native assault in
        // flight. Only actors with one of our *own* standing intentions are
        // stopped here. Other high-priority micro jobs are untouched.
        let old = op.own_orders.clone();
        for (id, (kind, goal)) in old.into_iter().take(ORDERS_PER_FRAME) {
            if out.ordered_this_frame(id) ||
                brain.tracked.primary_goal(id) != Some(goal) { continue; }
            brain.tracked.void(id, kind);
            brain.tracked.forget_primary(id);
            // An urgent Stop is the reliable way to cancel the *native*
            // attack/march; a new lower-priority Guard may be deferred by
            // the still-live attack's Tracker term.
            if out.want(id, 0, Goal::Stop) {
                op.own_orders.remove(&id);
            }
        }
    }
}

/// Keep an independent viable main fist after borrowing up to six fighters.
fn reserve_permits(total_attack: usize, assigned: usize) -> bool {
    assigned >= MIN_FRONT && assigned <= MAX_FRONT + MAX_REAR
        && total_attack.saturating_sub(assigned) >= MAIN_FIST_RESERVE
}

fn viable_local_fight(ours: i32, theirs: i32, threats: usize) -> bool {
    threats == 0 || ours >= 1_200
        && ours.saturating_mul(1_000) >= theirs.max(1).saturating_mul(1_150)
}

/// Choose an exterior post on the home-facing side of the target. A post
/// across the cliff may look geometric but is invalid unless the ground
/// zone and visible weapon safety of an actual frontline unit permit it.
fn choose_rally(rules: &RuleSet, obs: &Observation<'_>,
    home: (u16,u16), target: (u16,u16), front: &[Unit],
) -> Option<(u16,u16)> {
    let mut dx = (i32::from(home.0) - i32::from(target.0)).signum();
    let dy = (i32::from(home.1) - i32::from(target.1)).signum();
    if dx == 0 && dy == 0 { dx = 1; }
    let side = (-dy, dx);
    for r in [18,22,26,14] {
        for offset in [0,4,-4,8,-8] {
            let (Ok(x), Ok(y)) = (
                u16::try_from(i32::from(target.0) + dx*r + side.0*offset),
                u16::try_from(i32::from(target.1) + dy*r + side.1*offset),
            ) else { continue; };
            let cell = (x,y);
            if !obs.is_cell_in_playfield(x,y) || !obs.is_ground_walkable(x,y) {
                continue;
            }
            let reachable = front.iter().take(32).filter(|unit|
                orders::reachable(rules,obs,unit.entity_id,cell)
                    && !tactics::unsafe_post(rules,obs,unit.entity_id,cell)
                    && tactics::lethal_approach(rules,obs,unit.entity_id,
                        unit.cell,cell).is_none()).count();
            if reachable >= MIN_FRONT { return Some(cell); }
        }
    }
    None
}

/// A conservative intermediate front post. Not inside the occupied building,
/// not inside a *currently visible* gun's danger zone, and still reachable.
fn safe_approach(rules: &RuleSet, obs: &Observation<'_>, unit: &Unit,
    rally: (u16,u16), prize: (u16,u16),
) -> Option<(u16,u16)> {
    let dx = (i32::from(prize.0) - i32::from(rally.0)).signum();
    let dy = (i32::from(prize.1) - i32::from(rally.1)).signum();
    let side = (-dy, dx);
    for advance in [12,10,8,6,4,2] {
        for offset in [0,-2,2,-4,4] {
            let (Ok(x),Ok(y)) = (
                u16::try_from(i32::from(rally.0) + dx*advance + side.0*offset),
                u16::try_from(i32::from(rally.1) + dy*advance + side.1*offset),
            ) else { continue };
            let post=(x,y);
            if obs.is_cell_in_playfield(x,y) && obs.is_ground_walkable(x,y)
                && orders::reachable(rules,obs,unit.entity_id,post)
                && !tactics::unsafe_post(rules,obs,unit.entity_id,post)
                && tactics::lethal_approach(rules,obs,unit.entity_id,
                    unit.cell,post).is_none()
            { return Some(post); }
        }
    }
    None
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    #[test]
    fn a_side_operation_never_steals_the_main_fist() {
        assert!(!reserve_permits(9,4));
        assert!(!reserve_permits(13,4));
        assert!(reserve_permits(13,3));
        assert!(reserve_permits(15,4));
        assert!(reserve_permits(16,6));
        assert!(!reserve_permits(30,7));
        assert!(!reserve_permits(30,2));
    }

    #[test]
    fn local_superiority_is_required_before_clearance() {
        assert!(viable_local_fight(2_500,2_000,3));
        assert!(!viable_local_fight(1_400,2_500,3));
        assert!(!viable_local_fight(800,100,1));
        assert!(viable_local_fight(0,0,0));
    }

    #[test]
    fn a_goal_is_locked_to_one_prize_until_release() {
        let mut op = Operation {
            prize: 21, cell: (40,40), rally: (22,40), started: 10,
            changed: 10, stage: Stage::Assemble,
            front: [1,2,3].into_iter().collect(),
            rear: [4].into_iter().collect(), own_orders: BTreeMap::new(),
        };
        assert!(op.contains(1) && op.contains(4));
        assert!(!op.contains(10));
        op.move_to(Stage::Clear,20);
        assert_eq!(op.prize,21);
        assert_eq!(op.changed,20);
        // Re-reading the same tactical situation must not indefinitely
        // extend the clearance timeout.
        op.move_to(Stage::Clear,21);
        assert_eq!(op.changed,20);
        op.move_to(Stage::Screen,22);
        assert_eq!(op.prize,21);
    }
}
