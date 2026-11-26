//! Sending the army out.
//!
//! Native carries the attack in teams: an `AITrigger` creates one and its
//! script assigns the missions. A brain-driven house never gets an `AITrigger`
//! — `HouseClass::Update`'s team block is gated on "no human controls it" —
//! so without this module its army sits on area guard for the whole match and
//! the house can only lose by attrition.
//!
//! This is the player's substitute, and deliberately two rules. The first is
//! *when*: an army large enough to be worth spending, heavy enough to beat what
//! is standing where it is going ([`super::squads::outguns`]), a base that can
//! replace it, and nothing already at the yard. The second is *where*: one structure
//! for the whole army — the enemy economy structure nearest the first fighter
//! in entity-store order — because a fist takes a base, and units that walk
//! at their own nearest building arrive one at a time and die one at a time.
//!
//! One goal for the army is a measurement, not a preference. Three variants
//! were run end to end against the pinned `Death.mmx` match and all three
//! lost the push the current rule wins: aiming each unit at the structure its
//! own weapons answer best, aiming the fist from the army's centre instead of
//! that first fighter, and holding one target until it is destroyed instead
//! of re-reading the nearest one each frame (`super::RESIDUALS` has the
//! ticks). So the *target* stays one, and what differs per unit is the
//! **order**: a unit already inside its own weapon's reach of that structure
//! attacks it from where it stands, and one further out attack-moves toward
//! it. That keeps the fist together — the failure the three variants shared
//! was dispersion, not per-unit decisions — while stopping the army from
//! walking its short-ranged units point-blank into a base to shoot at all.
//!
//! While it is pushing it owns the whole army, so the area-guard step stands
//! down for the frame; a house that has not got an army yet keeps holding its
//! base, which is what a player does with five tanks and no plan.

use std::collections::{BTreeMap, BTreeSet};

use crate::rules::object_type::{ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::{EnemyEntity, Observation};
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};

use super::AssistBrain;
use super::census::{Census, Unit};
use super::squared_distance;

mod targets;
mod assault;
pub(super) mod fire_support;

/// How large the army has to be before it is worth pushing with: the teams'
/// own number ([`super::squads::ARMY_TO_ATTACK`]), because "how big is big
/// enough" is one question with one owner.
///
/// Below it the opening force is the whole army, and an opening force that
/// walks into the enemy's pre-placed garrison is a house that has traded its
/// base for nothing — which is why the surplus goes harassing instead.

/// Bound the strategic retask batch without leaving half a starting army idle.
/// The interactive L2 command budget is 512; eight was a stale L3 bottleneck.
const ADVANCE_ORDERS_PER_FRAME: usize = 24;
/// Hold a dispersed armoured fist briefly rather than launching a single
/// heavy tank as each factory completes it. A dead economy must not stall
/// an otherwise viable attack forever.
const ARMOUR_ASSEMBLE_FRAMES: u64 = 900;
const ARMOUR_WAVE_MIN: usize = 3;

fn central_actor(cells:&[(u16,u16)],fallback:(u16,u16))->(u16,u16) {
    let Some(&first)=cells.first() else {return fallback;};
    if cells.len()<3 {return first;}
    let mut xs:Vec<_>=cells.iter().map(|c|c.0).collect();
    let mut ys:Vec<_>=cells.iter().map(|c|c.1).collect();
    let middle=cells.len()/2;
    let center=(*xs.select_nth_unstable(middle).1,*ys.select_nth_unstable(middle).1);
    if cell_distance(first,center)<=12 {return first;}
    cells.iter().copied().min_by_key(|cell|cell_distance(*cell,center)).unwrap_or(first)
}

fn main_fist_anchor(fighters:&[&Unit],fallback:(u16,u16))->(u16,u16) {
    let ground:Vec<_>=fighters.iter().filter(|u|
        matches!(u.slot,super::census::Slot::Infantry|super::census::Slot::Vehicle)
            && !u.traits.aircraft).map(|u|u.cell).collect();
    if ground.len()>=3 {return central_actor(&ground,fallback);}
    central_actor(&fighters.iter().map(|u|u.cell).collect::<Vec<_>>(),fallback)
}

fn armoured_fighter(unit: &Unit, rules: &RuleSet, obs: &Observation<'_>) -> bool {
    unit.slot == super::census::Slot::Vehicle && unit.traits.combat
        && obs.object_type(rules, unit.type_id).is_some_and(|ty| {
            ty.category == crate::rules::object_type::ObjectCategory::Vehicle
                && ty.strength >= 300 && !ty.resource_gatherer
        })
}

fn armour_wave_missing(close: usize, waiting: u64) -> bool {
    close < ARMOUR_WAVE_MIN && waiting < ARMOUR_ASSEMBLE_FRAMES
}

/// A distant army is not a column standing beside its lead scout. Requiring
/// actual local companions prevents a lone vehicle from probing a prism or
/// heavy artillery emplacement while the main fist is still on the road.
#[allow(dead_code)]
fn battery_wave_ready(nearby: usize) -> bool {
    nearby >= ARMOUR_WAVE_MIN
}

/// A finite, evaluated operation over the exact cohort that will attack.
#[derive(Debug, Clone)]
pub(super) struct SiegeCharge {
    pub(super) target: u64,
    pub(super) until: u64,
    pub(super) members: BTreeSet<u64>,
    cell: (u16,u16),
    lane: u64,
    initial_hp: i64,
    // Original owned cohort stays in the ledger even if subsequently detached,
    // wounded, garrisoned or in native service. Missing ownership != killed.
    original: BTreeMap<u64,(i32,i32)>,
    initial_target_hp: i32,
    best_target_hp: i32,
    last_progress: u64,
    approach_until: u64,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ChargeRetry {
    target: u64,
    until: u64,
    hp: i64,
    failures: u8,
    target_hp: i32,
    forget_at: u64,
}

fn retry_allowed(retry: Option<ChargeRetry>, target: u64, now: u64, hp: i64, target_hp: i32) -> bool {
    retry.is_none_or(|r| r.target != target || now >= r.until
        || hp.saturating_mul(100) >= r.hp.saturating_mul(125 + 25*i64::from(r.failures.saturating_sub(1).min(2)))
        || (target_hp > 0 && i64::from(target_hp)*4 <= i64::from(r.target_hp)*3))
}

fn retry_delay(failures: u8) -> u64 { 900 * u64::from(failures.clamp(1,4)) }

fn retry_reference(old:Option<ChargeRetry>,failed:bool,hp:i64,target_hp:i32)->(i64,i32) {
    if !failed {
        old.map_or((hp,target_hp),|r|(r.hp,r.target_hp))
    } else {
        old.filter(|r|r.failures>0).map_or((hp,target_hp),
            |r|(r.hp.max(hp),r.target_hp.min(target_hp)))
    }
}

/// Approximate replacement value exposed/lost, not actual cash charges or
/// confirmed deaths. Health is net change from wave start; repaired damage
/// between samples is intentionally not counted twice.
fn cohort_feedback(original: &BTreeMap<u64,(i32,i32)>, owned: &BTreeMap<u64,i32>) -> (i64,usize,i64,i64) {
    let (mut hp,mut missing,mut removed_value,mut damage_value)=(0i64,0usize,0i64,0i64);
    for (&id,&(start,cost)) in original {
        let Some(&now)=owned.get(&id).filter(|&&hp| hp>0) else {
            missing+=1; removed_value+=i64::from(cost.max(0)); continue;
        };
        hp+=i64::from(now);
        damage_value+=i64::from(start.saturating_sub(now).max(0))*i64::from(cost.max(0))/i64::from(start.max(1));
    }
    (hp,missing,removed_value,damage_value)
}

/// Continue a coherent charge as a unit operation. Retreat/micro are allowed
/// outside the short window; a dead or invisible target cancels the order.
pub(super) fn charge_member(brain: &AssistBrain, tick: u64, actor: u64) -> bool {
    if brain.siege_charge.as_ref().is_some_and(|wave|
        tick < wave.until && wave.members.contains(&actor))
    { return true; }
    // A confirmed outage is a finite coordinated operation, not a reason
    // for the ordinary dodge/kite/micro passes to split its assault group.
    // The blackout commander makes the attack/withdraw choice each frame.
    brain.blackout.is_some_and(|plan| {
        matches!(plan.phase, BlackoutPhase::Committed | BlackoutPhase::Withdrawal)
            && brain.squads.squad(super::squads::Role::Attack)
                .is_some_and(|s|s.members.contains(&actor))
            && brain.census.units.iter().any(|unit| unit.entity_id == actor
                && brain.census.main_available(unit)
                && cell_distance(unit.cell, plan.cell) <= 34)
    })
}

/// End a failed wave without letting the same failed primary Attack survive
/// through the ordinary mission-resume layer. The final front guard owns the
/// safe disengagement. A visibility loss is NOT a confirmed destruction.
fn end_charge(brain: &mut AssistBrain, obs: &Observation<'_>, reason: &str) {
    let Some(mut wave) = brain.siege_charge.take() else { return; };
    let tick=obs.tick();
    let visible=obs.visible_enemy_entities().any(|enemy| enemy.entity_id==wave.target);
    if visible && let Some(hp)=obs.health(wave.target) {
        wave.best_target_hp=wave.best_target_hp.min(hp.max(0));
    }
    let owned: BTreeMap<_,_>=obs.own_entities().filter(|e| wave.original.contains_key(&e.entity_id))
        .map(|e|(e.entity_id,e.health)).collect();
    let (hp,missing,removed_value,damage_value)=cohort_feedback(&wave.original,&owned);
    log::info!("auto play: assault_end tick={tick} target={} reason={reason} start_hp={} remaining_hp={} best_target_hp={}",
        wave.target,wave.initial_hp,hp,wave.best_target_hp);
    let failed=matches!(reason,"loss-limit"|"forecast-changed"|"no-damage-progress"|"deadline"|"cohort-detached");
    let old=brain.charge_retry.get(&wave.target).copied().filter(|r| tick<r.forget_at);
    let failures=old.map_or(0,|r|r.failures).saturating_add(u8::from(failed));
    let delay=if failed {retry_delay(failures)} else {180};
    // Preserve existing stronger backoff across a strategic interrupt. A
    // target leaving sight is not a confirmed kill or a failed assault.
    let until=old.map_or(tick,|r|r.until).max(tick.saturating_add(delay));
    // A weaker failed wave must not make the next retry's reinforcement
    // threshold easier. Keep the strongest recent failed cohort as reference.
    let (reference_hp, reference_target_hp) =
        retry_reference(old,failed,wave.initial_hp,wave.initial_target_hp);
    brain.charge_retry.insert(wave.target,ChargeRetry {target:wave.target,
        until,hp:reference_hp,failures,target_hp:reference_target_hp,
        forget_at:tick.saturating_add(7200)});
    log::info!("auto play: assault_feedback tick={tick} target={} missing_owned={} removed_value={} damage_value={} target_damage={} failures={} cooldown={} visible={}",
        wave.target,missing,removed_value,damage_value,wave.initial_target_hp.saturating_sub(wave.best_target_hp).max(0),failures,until.saturating_sub(tick),visible);
    // Repeated low-progress losses are evidence for changing fronts, not
    // merely delaying the identical attack. The target selector retains
    // its all-candidate fallback when there is no alternative.
    if failed && visible && failures>=2
        && i64::from(wave.best_target_hp)*5>i64::from(wave.initial_target_hp)*4 {
        brain.front_review.defer_failed_charge(tick,wave.lane,wave.cell);
        log::info!("auto play: front_deferred tick={tick} lane={} target={} cell={:?} reason=assault-feedback",
            wave.lane,wave.target,wave.cell);
    }
    for id in wave.members {
        release_charge_member(&mut brain.tracked,&mut brain.charge_released,id,wave.target);
    }
}

/// Releasing a wave must not destroy a distinct job acquired since its
/// approval (a native service, escort, or another legitimate primary).
fn release_charge_member(tracked: &mut super::tracked::Tracker,
    released: &mut BTreeMap<u64,u64>, id: u64, target: u64) {
    if tracked.primary_goal(id)==Some(Goal::Attack {target}) {
        tracked.cancel_for_safety(id);
    }
    released.insert(id,target);
}

/// Called before tactical passes. The previous wave is reviewed independently
/// of the decision to launch another one, including when the economy stops it.
pub(super) fn prepare_siege_charge(
    brain: &mut AssistBrain, rules: &RuleSet, obs: &Observation<'_>,
) {
    let tick = obs.tick();
    // Stockpiling is advice, not a second, stricter assault forecast.
    // Genuine force recovery and immediate home danger still interrupt.
    let permit = obs.forces_enabled() && !brain.campaign.holding_fist()
        && !brain.campaign.economy_resting() && brain.blackout.is_none()
        && !brain.director.rebuilding_force() && !brain.mobilization.active()
        && !brain.director.under_attack_now();
    if let Some(wave) = &brain.siege_charge {
        let target = wave.target;
        let visible = obs.visible_enemy_entities().any(|e| e.entity_id == target);
        let remaining: BTreeSet<_> = brain.census.units.iter().filter(|u|
            wave.members.contains(&u.entity_id) && brain.census.main_available(u)
                && u.health_permille >= 500).map(|u| u.entity_id).collect();
        let hp: i64 = brain.census.units.iter().filter(|u| remaining.contains(&u.entity_id))
            .map(|u| i64::from(obs.health(u.entity_id).unwrap_or(0))).sum();
        let mut reason = if !permit { Some("strategic-interrupt") }
            else if !visible { Some("target-not-visible") }
            else if obs.visible_enemy_invulnerable(target) { Some("invulnerable") }
            else if !obs.visible_enemy_building_operational(rules,target) { Some("offline") }
            else if tick >= wave.until { Some("deadline") }
            else if remaining.len() < assault::MIN_COLUMN || hp * 100 < wave.initial_hp * 55 {
                Some("loss-limit")
            } else { None };
        if reason.is_none() && tick % super::tracked::MICRO_FRAMES == 0 {
            // A newly visible controller or heavy overlapping battery may
            // invalidate the original estimate. Re-evaluate only this cohort,
            // never replenish it with fresh tanks in the middle of a loss.
            let cohort: Vec<_> = brain.census.units.iter().filter(|u|
                remaining.contains(&u.entity_id)).collect();
            let enemy = obs.visible_enemy_entities().find(|e| e.entity_id == target);
            if enemy.and_then(|e| assault::estimate(rules,obs,e,&cohort))
                .is_none_or(|forecast| !forecast.allowed) { reason = Some("forecast-changed"); }
        }
        if let Some(reason) = reason {
            end_charge(brain,obs,reason);
            return;
        }
        let wave = brain.siege_charge.as_mut().expect("active wave");
        let removed: Vec<_> = wave.members.difference(&remaining).copied().collect();
        for id in removed {
            release_charge_member(&mut brain.tracked,&mut brain.charge_released,id,target);
        }
        wave.members = remaining;
        if tick % super::tracked::MICRO_FRAMES == 0 {
            if let Some(hp_now) = obs.health(target) {
                if hp_now < wave.best_target_hp {
                    wave.best_target_hp = hp_now;
                    wave.last_progress = tick;
                }
            }
            if tick >= wave.approach_until
                && tick.saturating_sub(wave.last_progress.max(wave.approach_until)) >= 150 {
                end_charge(brain,obs,"no-damage-progress");
            }
        }
        return;
    }
    if tick%45==0 { brain.charge_retry.retain(|_,retry| tick<retry.forget_at); }
    if !permit || tick % 45 != 0 || brain.blackout.is_some() { return; }
    let Some((lane, _, campaign_cell)) = brain.push_target else { return; };
    let Some(squad) = brain.squads.squad(super::squads::Role::Attack) else { return; };
    let main: Vec<_> = brain.census.units.iter().filter(|u|
        squad.members.contains(&u.entity_id) && brain.census.main_available(u)).collect();
    let Some((battery,tower_reach)) = super::tactics::siege_barrier(rules,obs,campaign_cell,&main)
        else { return; };
    // Prefer a real in-range safe gun. Merely being able to buy an artillery
    // unit, or having a launcher stranded behind a cliff, does not veto a wave.
    if brain.front_review.deferred(tick).is_none_or(|(_,cell)| cell_distance(cell,battery.cell)>12)
        && main.iter().any(|u| super::safety::firing_here(rules,obs,u,battery.entity_id)
            && obs.attack_target(u.entity_id) == Some(battery.entity_id)) { return; }
    let mut candidates: Vec<_> = main.into_iter().filter(|u|
        cell_distance(u.cell,battery.cell) <= tower_reach + 8
            && assault::eligible(rules,obs,u,battery)).collect();
    candidates.sort_by_key(|u| (cell_distance(u.cell,battery.cell),u.entity_id));
    candidates.truncate(assault::MAX_COHORT_SEARCH);
    let cells:Vec<_>=candidates.iter().map(|u|u.cell).collect();
    let Some(leader)=assault::cohesive_leader(&cells) else {return;};
    let ready: Vec<_> = candidates.into_iter().filter(|u| cell_distance(u.cell,leader)<=8)
        .take(assault::MAX_COLUMN).collect();
    let Some(forecast) = assault::estimate(rules,obs,battery,&ready) else { return; };
    if !forecast.allowed
        || !retry_allowed(brain.charge_retry.get(&battery.entity_id).copied(),battery.entity_id,tick,forecast.hp,forecast.target_hp) {
        return;
    }
    let members: BTreeSet<_> = ready.iter().map(|u| u.entity_id).collect();
    let count = members.len();
    for &id in &members {
        brain.charge_released.remove(&id);
        brain.tracked.cancel_for_safety(id);
    }
    let original=ready.iter().filter_map(|u| Some((u.entity_id,
        (obs.health(u.entity_id)?,obs.cost_of(rules,obs.object_type(rules,u.type_id)?).max(0)))))
        .collect();
    brain.siege_charge = Some(SiegeCharge {target:battery.entity_id,cell:battery.cell,lane,
        until:tick.saturating_add((forecast.frames+120).clamp(240,660)),members,
        initial_hp:forecast.hp,original,initial_target_hp:forecast.target_hp,best_target_hp:forecast.target_hp,last_progress:tick,
        approach_until:tick.saturating_add(forecast.approach+45)});
    log::info!("auto play: assault_commit tick={tick} target={} count={} hp={} target_hp={} forecast_ticks={} forecast_loss_hp={} forecast_loss_credits={}",
        battery.entity_id,count,forecast.hp,forecast.target_hp,forecast.frames,
        forecast.loss_hp,forecast.loss_credits);
}

fn execute_siege_charge(
    brain: &mut AssistBrain, rules: &RuleSet, obs: &Observation<'_>, out: &mut Outbox,
) -> bool {
    let Some(wave) = brain.siege_charge.as_ref() else { return false; };
    if obs.tick() >= wave.until { return false; }
    let target = wave.target;
    if !obs.visible_enemy_entities().any(|e| e.entity_id == target) { return false; }
    // Tasks can be claimed after the early review. Do not send a replacement
    // tank alone just because the rest of its cohort has been detached.
    let active: BTreeSet<_> = brain.census.units.iter().filter(|u| wave.members.contains(&u.entity_id)
        && brain.census.main_available(u) && u.health_permille>=500)
        .map(|u|u.entity_id).collect();
    if active.len() < assault::MIN_COLUMN {
        end_charge(brain,obs,"cohort-detached");
        return false;
    }
    for id in active.iter().copied() {
        if obs.attack_target(id) == Some(target) { continue; }
        let goal = Goal::Attack { target };
        if !super::orders::admits(rules,obs,id,&goal)
            || !brain.tracked.admits(obs.tick(),id,super::tracked::TrackedKind::Attack,
                Some(target),Some(wave.cell)) { continue; }
        // Ordinary retargeting must not cancel the bounded operation. Raw
        // emergency/repair admissions remain protected by the final gates.
        if out.want(id,2,goal) {
            brain.tracked.record_goal(obs.tick(),id,super::tracked::TrackedKind::Attack,
                Some(target),Some(wave.cell),goal);
        }
    }
    let support: Vec<_> = brain.census.units.iter().filter(|u| !wave.members.contains(&u.entity_id)
        && brain.census.main_available(u) && brain.squads.squad(super::squads::Role::Attack)
            .is_some_and(|team| team.members.contains(&u.entity_id)))
        .collect();
    // Short-range infantry/support wait outside the gunline while the
    // approved armoured column works. Long-range guns retain safe fire.
    stage_siege(&mut brain.tracked,brain.trace,&support,rules,obs,out,wave.cell,false);
    brain.push_gate = "siege-mass-charge";
    true
}

/// The operation's actual cohort owns its attack until it ends. Only strip
/// contradictory ordinary orders, never a recovery/engineer/native service.
/// Runs before safety::review (which has the last word for casualties).
pub(super) fn finish_charge(brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox) {
    let Some(wave) = brain.siege_charge.as_ref() else { return; };
    let active: BTreeSet<_> = brain.census.units.iter().filter(|u|
        wave.members.contains(&u.entity_id) && brain.census.main_available(u)
        && u.health_permille >= 500 && obs.can_hurt(rules,u.entity_id,wave.target))
        .map(|u| u.entity_id).collect();
    if active.len()<assault::MIN_COLUMN {
        // A cancelled charge must not leave its winning proposal in the
        // outbox. Safety still reviews the native old mission and other jobs.
        let target=wave.target;
        out.orders.retain(|o|!(wave.members.contains(&o.actor)
            && o.goal==(Goal::Attack {target})));
        end_charge(brain,obs,"cohort-detached");
        return;
    }
    let target=wave.target;
    let cell=wave.cell;
    let removed: Vec<_>=wave.members.difference(&active).copied().collect();
    // A later pass may have claimed service or another task. Revoke its
    // charge exemption now, before safety::review, not next frame.
    for &id in &removed {
        release_charge_member(&mut brain.tracked,&mut brain.charge_released,id,target);
    }
    brain.siege_charge.as_mut().expect("active wave").members=active.clone();
    let goal=Goal::Attack {target};
    let mut kept=BTreeSet::new();
    out.orders.retain(|o| {
        if !active.contains(&o.actor) {
            return !(removed.contains(&o.actor) && o.goal==goal);
        }
        if o.goal==goal { kept.insert(o.actor); true } else { false }
    });
    // A competing tactical pass may have won the temporary per-frame rank.
    // Restore the committed order at final arbitration only for actual healthy
    // members, never for a native service or a newly detached actor.
    for &id in &active {
        if kept.contains(&id) || obs.attack_target(id)==Some(target) {
            if brain.tracked.primary_goal(id)!=Some(goal) {
                brain.tracked.record_goal(obs.tick(),id,super::tracked::TrackedKind::Attack,
                    Some(target),Some(cell),goal);
            }
        } else if obs.tick()%super::tracked::MICRO_FRAMES==id%super::tracked::MICRO_FRAMES
            && super::orders::admits(rules,obs,id,&goal) {
            out.orders.push(crate::sim::control::intent::Order {actor:id,goal});
            brain.tracked.record_goal(obs.tick(),id,super::tracked::TrackedKind::Attack,
                Some(target),Some(cell),goal);
        }
    }
    out.attachments.retain(|a| !active.contains(&a.actor));
}

fn isolated_battery_approach(
    rules: &RuleSet, obs: &Observation<'_>,
    actor: u64, from: (u16,u16), to: (u16,u16),
) -> Option<((u16,u16), i32)> {
    use crate::rules::object_type::ObjectCategory;
    let ty = obs.entity_type(rules, actor)?;
    if !matches!(ty.category, ObjectCategory::Vehicle | ObjectCategory::Infantry)
        || ty.naval { return None; }
    obs.visible_enemy_entities().filter_map(|enemy| {
        let gun = obs.entity_type(rules, enemy.entity_id)?;
        if !super::siege_counter::dangerous_battery(rules, gun)
            || !obs.visible_enemy_building_operational(rules, enemy.entity_id)
        { return None; }
        let reach = super::tactics::effective_reach_against(rules, obs, enemy.entity_id, actor)
            .max(if ty.immune_to_psionics || ty.immune_to_psionic_weapons {0}
                else {super::tactics::mind_control_reach(rules,gun).unwrap_or(0)});
        if reach <= 0 || (super::siege_counter::battery_reach(rules,gun) < 7
            && super::tactics::mind_control_reach(rules,gun).is_none()) { return None; }
        // A handful of ordinary tanks nearby does NOT make the approach
        // safe. Actual synchronized, forecast-approved charges have their own one-shot
        // `execute_siege_charge` branch, run before normal advance. An older
        // `battery_wave_ready(3)` shortcut let three Grizzlies cross an 8/15
        // cell prism/Grand-Cannon kill zone as if they were siege artillery.
        let in_lane = cell_distance(enemy.cell, from) <= reach + 2
            || cell_distance(enemy.cell, to) <= reach + 2
            || super::orders::path_crosses(&[enemy.cell], from, to, reach + 2);
        if !in_lane { return None; }
        let own = super::tactics::actual_threat_reach(
            rules, obs, actor, enemy.entity_id);
        // Already within safe firing range? Shoot instead of staging again.
        let safe_shot = own > reach + 1
            && cell_distance(from, enemy.cell) <= own
            && cell_distance(from, enemy.cell) > reach + 1
            && !super::tactics::unsafe_post(rules, obs, actor, from);
        (!safe_shot).then_some((enemy.cell, reach))
    }).max_by_key(|(cell, reach)| (*reach, std::cmp::Reverse(cell_distance(from,*cell))))
}

/// How far behind the leading fighter a unit may fall before the push takes it
/// again, in cells. A unit that has nothing to walk to keeps whatever it was
/// told last, which is right for the tip and wrong for the tail: without this
/// the tail stands where it stopped for the rest of the match.
const STAGGER_BEHIND: i32 = 10;

/// How close an enemy has to come to the yard before the army stays home,
/// in cells. A raid inside this ring is what the army was bought for.
///
/// [`super::squads`] answers the raid itself from the same radius: a smaller
/// one there left a band where the push stopped for a threat no team was sent
/// at, so the whole army stood in the yard while the raid burned it.
pub(super) const THREAT_RADIUS: i32 = 18;

/// How long the army may go without taking an enemy object before the push
/// stops waiting for a favourable comparison, in frames: one minute.
const PUSH_STALL_FRAMES: u64 = 900;

/// How long the army stays home after a gate turned it back, in frames: fifty
/// seconds.
///
/// The gates below are read fresh every frame, and one of them — `outgunned` —
/// compares the army against whatever stands around the frame's target, which
/// moves as the army moves. Measured on Death.mmx seed 24301, the consequence
/// was a gate that flipped four times in ten thousand frames: `push` for ten
/// samples, `outgunned` for eight, `push` again, `outgunned` again. Every flip
/// re-tasks the whole fist, which from the player's side is an army walking out
/// and back rather than fighting.
///
/// The window is asymmetric on purpose. Turning back is a verdict about this
/// second and takes effect at once; coming out again waits. Fifty seconds is
/// long enough for a wave of attackers to cross the ground the re-task would
/// have spent, and short enough that an army that outgrew the gate is not held
/// at home for a minute of the match.

/// Whether the push has stalled: the enemy has not lost an object since
/// `progress` was stamped, and a minute has passed since then.
///
/// `progress` is `(frame, enemy count)`, stamped when the army first became
/// large enough to push and refreshed every time the count falls. Before the
/// first stamp the answer is no: an opening army is not stalled, it is small.
fn push_stalled(progress: (u64, usize), now: u64, current: usize) -> bool {
    let (since, then) = progress;
    since != 0 && current >= then && now.saturating_sub(since) >= PUSH_STALL_FRAMES
}

/// Where the army looks when it knows of nothing to attack: the nearest
/// ground nobody has seen, else the middle of the map.
fn searching_cell(obs: &Observation<'_>, anchor: (u16, u16)) -> Option<(u16, u16)> {
    if let Some(cell) = super::micro::nearest_unexplored(
        obs,
        anchor,
        crate::rules::locomotor_type::MovementZone::Normal,
    ) {
        return Some(cell);
    }
    let (width, height) = obs.map_extent();
    (width > 0 && height > 0).then_some((width / 2, height / 2))
}

/// How near a crate has to be to the cell the army walks to for the walk to
/// go over it, in cells.
///
/// Three is the crate's own reach plus a cell: the box is on the way, not on
/// the target.
/// A blackout is not a timer-driven attack order. We first observe a spy
/// assigned to a real power plant, then require a currently *visible* defence
/// to switch off, then attack with the group, and finally stop the native
/// attack missions before/when that defence becomes powered again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BlackoutPhase { Staging, Committed, Withdrawal }

#[derive(Debug, Clone, Copy)]
pub(super) struct BlackoutOperation {
    pub(super) battery: u64,
    pub(super) cell: (u16, u16),
    pub(super) started: u64,
    pub(super) phase: BlackoutPhase,
    pub(super) off_since: Option<u64>,
    /// The actual visible plant ordered during this blackout, retained so
    /// withdrawal can cancel its native Attack even after it leaves vision.
    pub(super) power_focus: Option<(u64, (u16, u16))>,
}

fn blackout_transition(phase: BlackoutPhase, visible: bool, operational: bool,
    elapsed_off: u64, max_window: u64) -> BlackoutPhase {
    match phase {
        BlackoutPhase::Staging if visible && !operational => BlackoutPhase::Committed,
        BlackoutPhase::Committed if !visible || operational
            || elapsed_off >= max_window => BlackoutPhase::Withdrawal,
        _ => phase,
    }
}

/// Apply the already-planned operation before ordinary push scoring. Without
/// this, the same power loss which removes the siege barrier can also
/// discard the earlier waiting-for-spy plan and the army never moves.
fn drive_blackout(
    brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox, fighters: &[&Unit],
) -> bool {
    let Some(mut plan) = brain.blackout else { return false };
    let tick = obs.tick();
    let seen = obs.visible_enemy_entities()
        .any(|enemy| enemy.entity_id == plan.battery);
    if !seen && obs.is_cell_visible(plan.cell.0, plan.cell.1) {
        // A currently visible empty plot is evidence that the defence fell;
        // fog outside our vision is not such evidence.
        brain.blackout = None;
        brain.blackout_withdraw_after.clear();
        return false;
    }
    if plan.phase == BlackoutPhase::Staging
        && tick.saturating_sub(plan.started) > 1_500
    {
        brain.blackout = None;
        brain.blackout_withdraw_after.clear();
        return false;
    }
    let powered = seen && obs.visible_enemy_building_operational(rules, plan.battery);
    let off_elapsed = plan.off_since.map_or(0, |since| tick.saturating_sub(since));
    let duration = u64::try_from(rules.general.spy_power_blackout.max(0)).unwrap_or(0);
    // Leave a safety margin for a one-second command queue, formation travel
    // and nonzero native attack latency. A longer brownout is not guaranteed
    // merely because a spy was the original cause.
    let safe_window = duration.saturating_sub(120).max(60);
    let next = blackout_transition(plan.phase, seen, powered, off_elapsed, safe_window);
    if next != plan.phase {
        if next == BlackoutPhase::Committed { plan.off_since = Some(tick); }
        log::info!("auto play: blackout {:?} -> {:?} for battery {} at tick {}",
            plan.phase, next, plan.battery, tick);
        plan.phase = next;
    }
    if plan.phase == BlackoutPhase::Staging {
        brain.blackout = Some(plan);
        return false; // the normal range-aware siege staging owns this phase
    }
    if plan.phase == BlackoutPhase::Committed {
        // A genuine currently-visible off-line battery opens one bounded
        // coordinated attack window. Do not infer blackout from fog or from a
        // missing spy, and do not send a single scout to test an active tower.
        if !seen || powered { brain.blackout = Some(plan); return true; }
        // The outage is an *opportunity to kill generation*, not an order to
        // remove the now-silent tower. Destroying a Powered=yes consumer
        // reduces drain and can restore other guns. Use visible power plants
        // from the *same observed owner* and a locally reachable firing lane;
        // never inspect the opponent's hidden electricity totals.
        let plant = obs.visible_enemy_house(plan.battery).and_then(|owner| {
            obs.visible_enemy_entities().filter(|enemy| {
                obs.visible_enemy_house(enemy.entity_id) == Some(owner)
                    && cell_distance(enemy.cell, plan.cell) <= 28
                    && obs.entity_type(rules, enemy.entity_id)
                        .is_some_and(|ty| ty.category ==
                            crate::rules::object_type::ObjectCategory::Building && ty.power > 0)
                    && fighters.iter().any(|unit| unit.controllable()
                        && !unit.in_service()
                        && cell_distance(unit.cell, enemy.cell) <= 34
                        && obs.can_hurt(rules, unit.entity_id, enemy.entity_id)
                        && super::orders::reachable(rules, obs, unit.entity_id, enemy.cell))
            }).min_by_key(|enemy| {
                // Keep the current generator until it really disappears;
                // changing to a newly seen small plant every frame restarts
                // the entire native approach.
                let persistent = plan.power_focus
                    .is_none_or(|(id, _)| id != enemy.entity_id);
                let output = obs.entity_type(rules, enemy.entity_id)
                    .map_or(0, |ty| ty.power.max(0));
                (persistent, std::cmp::Reverse(output),
                    cell_distance(enemy.cell, plan.cell), enemy.entity_id)
            })
        });
        // The previous plant id remains only as a withdrawal marker. If a
        // generator disappeared while the outage persists, NEVER fall back
        // to attacking the silent tower: destroying the power consumer may
        // turn the rest of the defense grid back on. The regular campaign
        // can resume when the bounded blackout operation ends.
        let Some(plant) = plant else {
            let mut issued = 0;
            for unit in fighters {
                if issued >= ADVANCE_ORDERS_PER_FRAME { break; }
                if !unit.controllable() || unit.in_service()
                    || obs.attack_target(unit.entity_id) != Some(plan.battery)
                { continue; }
                if out.want(unit.entity_id, 0, Goal::Stop) {
                    brain.tracked.forget_primary(unit.entity_id);
                    issued += 1;
                }
            }
            end_charge(brain,obs,"blackout-transition");
            brain.push_gate = "blackout-no-visible-generator";
            brain.blackout = Some(plan);
            return true;
        };
        plan.power_focus = Some((plant.entity_id, plant.cell));
        let (victim, victim_cell) = (plant.entity_id, plant.cell);
        let available = fighters.iter().filter(|unit| unit.controllable()
            && !unit.in_service() && cell_distance(unit.cell, victim_cell) <= 34
            && obs.can_hurt(rules, unit.entity_id, victim)).count();
        if available < 3 {
            brain.push_gate = "blackout-awaiting-fist";
            brain.blackout = Some(plan);
            return true;
        }
        let mut issued = 0;
        for unit in fighters {
            if issued >= ADVANCE_ORDERS_PER_FRAME { break; }
            if !unit.controllable() || unit.in_service()
                || cell_distance(unit.cell, victim_cell) > 34
                || !obs.can_hurt(rules, unit.entity_id, victim)
                || obs.attack_target(unit.entity_id) == Some(victim)
            { continue; }
            // If other guns in this base remain operational, don't turn a
            // temporary blackout raid into an unprotected rush through them.
            if victim != plan.battery
                && super::tactics::lethal_approach(
                    rules, obs, unit.entity_id, unit.cell, victim_cell).is_some()
            { continue; }
            let goal = Goal::Attack { target: victim };
            if !super::orders::admits(rules, obs, unit.entity_id, &goal) { continue; }
            if out.want(unit.entity_id, 2, goal) {
                brain.tracked.record_goal(tick, unit.entity_id,
                    super::tracked::TrackedKind::Attack,
                    Some(victim), Some(victim_cell), goal);
                issued += 1;
            }
        }
        end_charge(brain,obs,"blackout-transition");
        brain.push_gate = "blackout-commit";
        brain.blackout = Some(plan);
        return true;
    }
    // The grid has come back, the battery was lost in fog, or the safe end of
    // the outage is approaching. Merely changing `push_gate` leaves every
    // native Attack/AttackMove already en route; replace those orders now.
    let mut issued = 0;
    for unit in fighters {
        if issued >= ADVANCE_ORDERS_PER_FRAME { break; }
        if !unit.controllable() || unit.in_service() || obs.is_deployed(unit.entity_id)
            || unit.slot == super::census::Slot::Aircraft
        { continue; }
        let near = cell_distance(unit.cell, plan.cell) <= 24
            || plan.power_focus.is_some_and(|(_, cell)|
                cell_distance(unit.cell, cell) <= 24);
        let charging = obs.attack_target(unit.entity_id) == Some(plan.battery)
            || plan.power_focus.is_some_and(|(id, _)|
                obs.attack_target(unit.entity_id) == Some(id))
            || obs.destination(unit.entity_id).is_some_and(|dest|
                cell_distance(dest, plan.cell) <= 18
                    || plan.power_focus.is_some_and(|(_, cell)|
                        cell_distance(dest, cell) <= 18));
        if !near && !charging { continue; }
        let reach = if seen { obs.entity_type(rules, plan.battery)
            .map_or(13, |ty| weapon_range_cells(rules, ty, false).max(5)) }
            else { 13 };
        let post = super::tactics::siege_post(
            rules, obs, unit.entity_id, unit.cell, plan.cell, reach)
            .or_else(|| brain.census.base_cell.filter(|cell|
                super::orders::reachable(rules, obs, unit.entity_id, *cell)
                    && !super::tactics::unsafe_post(rules, obs, unit.entity_id, *cell)));
        let goal = post.map_or(Goal::Stop, |cell| Goal::Move { cell });
        if post.is_some_and(|cell| obs.destination(unit.entity_id) == Some(cell))
            || brain.blackout_withdraw_after.get(&unit.entity_id)
                .is_some_and(|until| tick < *until)
        { continue; }
        if out.want(unit.entity_id, 0, goal) {
            brain.blackout_withdraw_after.insert(unit.entity_id,
                tick.saturating_add(90));
            brain.tracked.void(unit.entity_id, super::tracked::TrackedKind::Attack);
            brain.tracked.void(unit.entity_id, super::tracked::TrackedKind::AttackMove);
            brain.tracked.forget_primary(unit.entity_id);
            issued += 1;
        }
    }
    brain.push_gate = "blackout-withdraw";
    // Withdraw for a bounded interval, not forever. A later normal siege
    // re-evaluation can resume once the front is safe and capabilities change.
    if tick.saturating_sub(plan.off_since.unwrap_or(plan.started)) >= duration.saturating_add(240) {
        brain.blackout = None;
        brain.blackout_withdraw_after.clear();
    } else {
        brain.blackout = Some(plan);
    }
    true
}

fn siege_blackout_wait(start: u64, tick: u64, live_power_infiltration: bool) -> bool {
    live_power_infiltration && tick.saturating_sub(start) < 1_500
}

#[cfg(all(test, tests_sim_p_s))]
mod v31_blackout_state_tests {
    use super::{blackout_transition, BlackoutPhase};

    #[test]
    fn only_a_visible_observed_outage_commits_the_raid() {
        assert_eq!(blackout_transition(BlackoutPhase::Staging, false, false, 0, 880),
            BlackoutPhase::Staging);
        assert_eq!(blackout_transition(BlackoutPhase::Staging, true, true, 0, 880),
            BlackoutPhase::Staging);
        assert_eq!(blackout_transition(BlackoutPhase::Staging, true, false, 0, 880),
            BlackoutPhase::Committed);
    }

    #[test]
    fn power_return_loss_of_sight_and_expiry_all_require_withdrawal() {
        assert_eq!(blackout_transition(BlackoutPhase::Committed, true, true, 80, 880),
            BlackoutPhase::Withdrawal);
        assert_eq!(blackout_transition(BlackoutPhase::Committed, false, false, 80, 880),
            BlackoutPhase::Withdrawal);
        assert_eq!(blackout_transition(BlackoutPhase::Committed, true, false, 880, 880),
            BlackoutPhase::Withdrawal);
        assert_eq!(blackout_transition(BlackoutPhase::Committed, true, false, 500, 880),
            BlackoutPhase::Committed);
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v26_siege_coordination_tests {
    use super::{battery_wave_ready, siege_blackout_wait};

    #[test]
    fn waiting_for_a_real_spy_blackout_is_finite() {
        assert!(siege_blackout_wait(100, 700, true));
        assert!(!siege_blackout_wait(100, 700, false));
        assert!(!siege_blackout_wait(100, 1_600, true));
    }

    #[test]
    fn artillery_does_not_probe_a_battery_until_nearby_comrades_arrive() {
        assert!(!battery_wave_ready(1));
        assert!(!battery_wave_ready(2));
        assert!(battery_wave_ready(3));
    }
}

const CRATE_ON_THE_WAY_CELLS: i32 = 3;

/// The crate the walk to `cell` passes, if there is one.
///
/// The nearest one wins, so the army takes the first box on its way and not
/// the one across the map.
fn crate_on_the_way(
    rules: &RuleSet,
    obs: &Observation<'_>,
    cell: (u16, u16),
) -> Option<(u16, u16)> {
    obs.crate_cells(rules)
        .into_iter()
        .filter(|crate_cell| {
            squared_distance(*crate_cell, cell) <= CRATE_ON_THE_WAY_CELLS * CRATE_ON_THE_WAY_CELLS
        })
        .min_by_key(|crate_cell| (squared_distance(*crate_cell, cell), *crate_cell))
}

/// How long the fist keeps the structure it committed to, in frames: two
/// minutes at the simulation's fifteen a second.
///
/// Long enough to walk there and take it apart, short enough that a target the
/// army cannot reach stops holding it.
const PUSH_LOCK_FRAMES: u64 = 1_800;

const PUSH_REST_FRAMES: u64 = 750;

/// How few enemy objects are left before the strength gate stops asking. Below
/// this the enemy is a clean-up job, not a fight: the army goes in and finishes
/// it instead of waiting for a comparison it can only lose by arithmetic.
const MOP_UP_OBJECTS: usize = 8;

/// Whether the enemy is a clean-up job: too little mobile mass left to be a
/// fight, whatever the arithmetic says about the fist.
///
/// The wishlist's stalemate — "最后消灭了大部分敌人，但Autoplay也不发起进攻，然后就
/// 僵持在这了。发动攻击后总会剩下敌人的建筑不再打了". The absolute
/// [`ARMY_TO_ATTACK`] floor is an opening number, and it had no bypass: a house
/// with nine fighters and an enemy holding two buildings sat at `too-small` for
/// the rest of a nine-hour match while the buildings stood.
///
/// The count is the enemy mobile mass **this house can see near its own base**,
/// not the sum over the whole map. On a 1v29 that sum never falls below eight
/// while twenty-nine houses are alive, so the bypass never fired and the floor
/// it exists to relieve held the army home for the entire match. What the
/// bypass is for is the local stalemate a player reads off the screen: the
/// ground around the base is clear, so the army is not being kept to defend
/// anything, and what remains is a march.
///
/// `home` is the house's base cell; the read is [`THREAT_RADIUS`] around it,
/// the same ring [`base_under_threat`] calls "the enemy is at the gate".
fn mop_up(census: &Census) -> bool {
    let Some(home) = census.base_cell else {
        // No base to defend: nothing is holding the army where it stands.
        return true;
    };
    census.enemies_near(home, THREAT_RADIUS) < MOP_UP_OBJECTS
}

/// The clean-up bypass reads the ground around the base, not the whole map.
///
/// The whole-map sum is what made it dead on a 1v29: twenty-nine houses keep
/// more than [`MOP_UP_OBJECTS`] bodies on the field for the entire match, so the
/// bypass never fired and the `ARMY_TO_ATTACK` floor it exists to relieve held
/// the army home until the base fell.
#[test]
fn the_clean_up_bypass_reads_the_base_ring_not_the_whole_map() {
    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        .expect("the repository's own rules");
    let mut sim = crate::sim::world::Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Americans");
    let mut census = Census::default();
    census.refresh(
        &crate::rules::unit_role::RoleTable::build(&rules),
        &rules,
        &Observation::new(&sim, owner),
    );

    // No base at all: nothing is being held at home.
    census.base_cell = None;
    assert!(
        mop_up(&census),
        "a house with no base has nothing to defend"
    );

    // A base with an empty ring, while the map elsewhere is full of enemies:
    // the sum is large, the ring is clear, and the army is free to march.
    let home = (20, 20);
    census.base_cell = Some(home);
    census.threat = [200, 80, 0, 0, 0, 0, 0];
    census.enemies = Vec::new();
    assert!(
        mop_up(&census),
        "a full map elsewhere does not hold the army at a clear base"
    );

    // The same ring with a field army standing in it is not a clean-up job. The
    // bypass is for "a handful left", so the raid has to be more than a handful:
    // one straggler at the gate is exactly what it is meant to wave through.
    census.enemies = (0..MOP_UP_OBJECTS)
        .map(|index| {
            let offset = u16::try_from(index).unwrap_or(0);
            crate::sim::control::observation::EnemyEntity {
                entity_id: u64::try_from(index).unwrap_or(0),
                type_id: crate::sim::intern::InternedId::default(),
                cell: (home.0.wrapping_add(offset), home.1),
            }
        })
        .collect();
    assert!(
        !mop_up(&census),
        "a field army at the gate keeps the army home: nearby={}",
        census.enemies_near(home, THREAT_RADIUS)
    );
}

/// Send the army at the enemy's base when it is large enough. `false` when
/// the house is not pushing this frame, so the caller keeps its own posture.
/// Whether the army is still inside a commit window a gate opened.
///
/// Turning back is a verdict about this second and takes effect at once; coming
/// out again waits [`PUSH_REST_FRAMES`]. The asymmetry is the whole point — an
/// army that leaves the frame a comparison improves walks out and back, which is
/// what the measured gate flapping looked like.
fn resting(brain: &AssistBrain, tick: u64) -> bool {
    brain.push_rest.0 != 0 && tick.saturating_sub(brain.push_rest.0) < PUSH_REST_FRAMES
}

/// Whether `unit`'s type answers the job `target`'s armour asks for.
///
/// The native fire test this replaces asks whether the gun can point at the
/// target at all — a verses line, non-zero or not. That is the wrong question
/// for a Mirage Tank in front of a concrete wall. `MirageWH` sits at 30/20/20
/// percent against the three structure armours, which is non-zero, so
/// `Engage::Ready` says yes, and the house spends 1000 credits to do a fifth of
/// a Grizzly's work against a building it cannot afford to trade for. The role
/// table answers the question the player is asking: is this unit *for* this job.
///
/// The bar is [`ROLE_FLOOR`] — the same quarter-of-the-best the census counts as
/// "able". A unit under it is not bad at the job; it is for a different one, and
/// the house has one of those in every slot. That reading is also why the answer
/// stays a *relative* one: the weights are scaled against the best the same rule
/// set offers, so a mod that swaps the line tank changes which unit leads and
/// never needs this function to change with it.
///
/// A target whose type or armour the ledger cannot describe answers `true`: the
/// refusal exists to hold back the wrong specialist, not to hold the whole fist
/// home over an unknown.
#[allow(dead_code)]
fn able_for(
    census: &Census,
    unit_type: InternedId,
    target: u64,
    rules: &RuleSet,
    obs: &Observation<'_>,
) -> bool {
    let Some(target_type) = obs.entity_type(rules, target) else {
        return true;
    };
    let Some(role) = crate::rules::unit_role::role_against_armour(target_type.armor_id()) else {
        return true;
    };
    census.facts(unit_type).is_none_or(|facts| {
        // Against the unit's *own* best answer, not against the ruleset's. The
        // question is "is this its job?", and a Mirage is 85.7 against armour
        // and 20.0 against concrete: 23% of its own best is a specialist being
        // sent to do something else, and the refinery it would take an hour to
        // chew through is one a Grizzly (36.8 against concrete, 62% of its own
        // best) takes in a minute. The ruleset-wide floor cannot tell those two
        // apart, because it asks how good anything is against buildings rather
        // than how good *this* is.
        let own_best = crate::rules::unit_role::ROLES
            .iter()
            .map(|role| facts.weights.get(*role))
            .fold(0.0_f64, f64::max);
        own_best <= 0.0 || facts.weights.get(role) >= own_best * crate::rules::unit_role::ROLE_FLOOR
    })
}

/// Only regroup *ground vehicles* and only when dispersed far behind a
/// leading vehicle that is still outside the battle. The caller keeps its
/// original single enemy target; this waypoint is solely a marching aid for
/// the tail, never a new target-priority algorithm.
fn needs_tank_cohesion(unit: &Unit, post: (u16,u16)) -> bool {
    unit.slot == super::census::Slot::Vehicle
        && unit.traits.combat && unit.controllable() && !unit.in_service()
        && cell_distance(unit.cell, post) > STAGGER_BEHIND + 4
}

fn tank_cohesion_post(
    fighters: &[&Unit], target: (u16,u16),
    obs: &Observation<'_>, census: &Census,
) -> Option<(u16,u16)> {
    let mut tanks: Vec<&Unit> = fighters.iter().copied().filter(|u| {
        u.slot == super::census::Slot::Vehicle
            && u.traits.combat && u.controllable() && !u.in_service()
    }).collect();
    if tanks.len() < 6 {
        return None;
    }
    tanks.sort_by_key(|u| (cell_distance(u.cell, target), u.entity_id));
    let lead = tanks[0].cell;
    let tail = tanks.last()?.cell;
    if cell_distance(lead, target) < 20
        || cell_distance(lead, tail) < STAGGER_BEHIND * 2
        || census.enemies.iter().any(|enemy| {
            obs.is_cell_visible(enemy.cell.0, enemy.cell.1)
                && cell_distance(enemy.cell, lead) <= 12
        })
    {
        return None;
    }
    // Move the destination four cells from the leader toward the tail. A
    // cell *on* the tank is normally occupied, so take a walkable nearby cell.
    let dx = (i32::from(tail.0) - i32::from(lead.0)).signum();
    let dy = (i32::from(tail.1) - i32::from(lead.1)).signum();
    for distance in [4i32,3,5,2] {
        let (Ok(x),Ok(y)) = (
            u16::try_from(i32::from(lead.0) + distance * dx),
            u16::try_from(i32::from(lead.1) + distance * dy),
        ) else { continue; };
        if obs.is_ground_walkable(x,y) {
            return Some((x,y));
        }
    }
    None
}

/// The screen goes behind *actual nearby armour*, rather than charging a
/// fragile rifleman ahead of the tanks. Only applies while marching across
/// safe ground: once in contact, micro/engage owns both units.
fn infantry_screen_post(
    fighters: &[&Unit], soldier: &Unit, target: (u16,u16),
    rules: &RuleSet, obs: &Observation<'_>,
) -> Option<(u16,u16)> {
    if soldier.slot != super::census::Slot::Infantry || !soldier.traits.combat
        || soldier.traits.irreplaceable { return None; }
    let armor = fighters.iter().copied()
        .filter(|unit| unit.slot == super::census::Slot::Vehicle
            && unit.traits.combat && unit.controllable() && !unit.in_service()
            && unit.points >= soldier.points)
        .min_by_key(|unit| (cell_distance(unit.cell, target), unit.entity_id))?;
    let armor_distance = cell_distance(armor.cell, target);
    if armor_distance <= 8 || cell_distance(soldier.cell, target) + 3 >= armor_distance
        || super::micro::firing(obs, soldier.mission, soldier.entity_id)
    { return None; }
    let (dx,dy) = (
        (i32::from(armor.cell.0) - i32::from(target.0)).signum(),
        (i32::from(armor.cell.1) - i32::from(target.1)).signum(),
    );
    for offset in [3,4,2] {
        let (Ok(x),Ok(y)) = (
            u16::try_from(i32::from(armor.cell.0) + offset * dx),
            u16::try_from(i32::from(armor.cell.1) + offset * dy),
        ) else { continue; };
        let cell=(x,y);
        if obs.is_ground_walkable(x,y)
            && super::orders::reachable(rules,obs,soldier.entity_id,cell)
            && !super::tactics::unsafe_post(rules,obs,soldier.entity_id,cell)
        { return Some(cell); }
    }
    None
}

/// During an artillery preparation window, standing under enemy guns is
/// worse than waiting. Move the *current* front back to a lawful standoff
/// ring, and send late reinforcements to that ring rather than into the tower.
/// This is a temporary detour: the original push target stays in `advance`.
fn stage_siege(
    tracker: &mut super::tracked::Tracker, trace: Option<u64>,
    fighters: &[&Unit], rules: &RuleSet, obs: &Observation<'_>,
    out: &mut Outbox, target: (u16,u16), allow_contact: bool,
) -> usize {
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    let gunline: Vec<_> = visible.iter().copied().filter_map(|enemy| {
        let ty = obs.entity_type(rules,enemy.entity_id)?;
        if !super::siege_counter::dangerous_battery(rules,ty)
            || !obs.visible_enemy_building_operational(rules, enemy.entity_id)
            || cell_distance(enemy.cell,target) > 14 { return None; }
        let reach = super::siege_counter::battery_reach(rules,ty);
        (reach >= 4).then_some((enemy.entity_id,enemy.cell,reach))
    }).collect();
    if gunline.is_empty() {return 0;}
    // Limit the common-lane search; the larger 48-candidate charge search
    // runs only on its own admission cadence.
    let assembly=allow_contact.then(||gunline.iter().copied()
        .min_by_key(|(id,cell,_)|(cell_distance(*cell,target),*id))
        .and_then(|(id,gun,_)|{
            let mut armour:Vec<_>=fighters.iter().copied().filter(|u|
                u.controllable() && !u.in_service() && u.health_permille>=700
                && armoured_fighter(u,rules,obs)).collect();
            armour.sort_by_key(|u|(cell_distance(u.cell,gun),u.entity_id));
            let cells:Vec<_>=armour.iter().take(16).map(|u|u.cell).collect();
            assault::cohesive_leader(&cells).map(|cell|(id,cell))
        })).flatten();
    let mobile_contacts: BTreeSet<_> = visible.iter().copied()
        .filter(|enemy| super::convergence::mobile_threat(rules,obs,enemy))
        .map(|enemy| enemy.entity_id).collect();
    let mut issued = 0;
    for unit in fighters.iter().cycle()
        .skip((obs.tick() as usize) % fighters.len().max(1))
        .take(fighters.len().min(128))
    {
        if issued >= 5 { break; }
        if !unit.controllable() || unit.in_service() || obs.is_deployed(unit.entity_id)
            || unit.slot == super::census::Slot::Aircraft
            || super::orders::ordered_this_frame(out,unit.entity_id)
        { continue; }
        if unit.health_permille >= 600 && obs.attack_target(unit.entity_id).is_some_and(|target|
            mobile_contacts.contains(&target) && super::safety::firing_here(rules,obs,unit,target)) {
            continue;
        }
        let Some((id,gun,reach)) = gunline.iter().copied()
            .filter(|(id,_,_)| obs.can_hurt(rules,*id,unit.entity_id)
                || obs.entity_type(rules,*id).is_some_and(|ty|
                    super::tactics::mind_control_reach(rules, ty).is_some()))
            .min_by_key(|(id,at,_)| (cell_distance(*at,unit.cell),*id))
        else { continue; };
        // A real supported in-range shot is not a lone approach. The
        // ordinary siege stage must not order a retreat merely because the
        // final safety pass would have allowed this same engagement.
        if allow_contact && obs.attack_target(unit.entity_id).is_some_and(|target|
            super::safety::ready_battery_contact(rules,obs,unit,
                fighters.iter().copied(),Goal::Attack {target})) {continue;}
        if allow_contact && super::safety::ready_battery_contact(rules,obs,unit,
            fighters.iter().copied(),Goal::Attack {target:id})
        {
            if super::orders::issue_tracked(tracker,trace,out,rules,obs,unit.entity_id,
                super::tracked::TrackedKind::Attack,Some(id),Some(gun),Goal::Attack {target:id}) {
                issued+=1;
            }
            continue;
        }
        // A prism/V3-style battery already has a winning range trade. Its
        // assignment is Attack on the tower, never Move to a new staging cell.
        let own_reach = super::tactics::actual_threat_reach(
            rules, obs, unit.entity_id, id,
        );
        if own_reach > reach + 1 && (super::safety::firing_here(rules,obs,unit,id)
            || obs.destination(unit.entity_id).is_some_and(|post|
                super::safety::firing_post(rules,obs,unit,id,gun,false)==Some(post))) { continue; }
        // A new safe firing-post proposal in this frame is already owned by
        // the battery commander; do not replace it with generic staging.
        if super::orders::ordered_this_frame(out,unit.entity_id) { continue; }
        let approach=assembly.filter(|(battery,_)|*battery==id
                && armoured_fighter(unit,rules,obs))
            .map_or(unit.cell,|(_,cell)|cell);
        let Some(post)=super::tactics::siege_post_on_lane(
            rules,obs,unit.entity_id,unit.cell,approach,gun,reach,&visible)
        else {continue;};
        let still_charging = obs.attack_target(unit.entity_id) == Some(id)
            || obs.destination(unit.entity_id).is_some_and(|destination|
                cell_distance(destination, gun) <= reach + 1
                    || super::orders::path_crosses(
                        &[gun], unit.cell, destination, reach + 1));
        // A *native* Attack/AttackMove from an earlier planning cycle can
        // survive while the strategic gate now says "wait for artillery".
        // Staging without cancelling that mission left tanks walking to their
        // deaths, even after the visible new plan looked defensive.
        if cell_distance(unit.cell, post) <= 2
            && cell_distance(unit.cell, gun) > reach + 1
        {
            if still_charging && super::orders::issue_tracked(
                tracker, trace, out, rules, obs, unit.entity_id,
                super::tracked::TrackedKind::Avoid, None, Some(post), Goal::Stop,
            ) { issued += 1; }
            continue;
        }
        if obs.destination(unit.entity_id) == Some(post) { continue; }
        if super::orders::issue_tracked(
            tracker,trace,out,rules,obs,unit.entity_id,
            super::tracked::TrackedKind::Avoid,None,Some(post),Goal::Move { cell: post },
        ) { issued += 1; }
    }
    issued
}

/// A strategic reserve is still a real front, not twelve tanks idling at
/// their factories. Position the main fist on the *friendly* side of the
/// known enemy base at a conservative offset, without walking through any
/// currently visible defensive battery on the way. This runs only while the
/// long-lived campaign is containing/stockpiling/refitting; the Harass team
/// is free to keep interdicting miners elsewhere.
fn stage_strategic_reserve(
    tracker: &mut super::tracked::Tracker, trace: Option<u64>,
    fighters: &[&Unit], rules: &RuleSet, obs: &Observation<'_>,
    out: &mut Outbox, front: (u16,u16), home: Option<(u16,u16)>,
) -> usize {
    let guns: Vec<_> = obs.visible_enemy_entities().filter_map(|enemy| {
        let ty = obs.entity_type(rules, enemy.entity_id)?;
        if !super::siege_counter::dangerous_battery(rules,ty)
            || !obs.visible_enemy_building_operational(rules, enemy.entity_id)
        { return None; }
        let reach = super::siege_counter::battery_reach(rules, ty);
        (reach > 0 && cell_distance(front, enemy.cell) <= 28)
            .then_some((enemy.entity_id, enemy.cell, reach))
    }).collect();
    // Collect once, rather than walking the enemy store once per fighter.
    // On a 120-house map even visible-only queries must remain bounded.
    let visible_structures: BTreeSet<u64> = obs.visible_enemy_entities()
        .filter(|enemy| obs.entity_type(rules, enemy.entity_id)
            .is_some_and(|ty| ty.category ==
                crate::rules::object_type::ObjectCategory::Building))
        .map(|enemy| enemy.entity_id).collect();
    let distant_reach = guns.iter().map(|(_,_,r)| *r).max().unwrap_or(0);
    let radius = (distant_reach + 5).max(20);
    let home = home.unwrap_or_else(|| fighters.first().map_or(front, |unit| unit.cell));
    let mut dx = (i32::from(home.0) - i32::from(front.0)).signum();
    let dy = (i32::from(home.1) - i32::from(front.1)).signum();
    if dx == 0 && dy == 0 { dx = 1; }
    let lateral = (-dy, dx);
    let mobile_contacts: BTreeSet<_> = obs.visible_enemy_entities()
        .filter(|enemy| super::convergence::mobile_threat(rules,obs,enemy))
        .map(|enemy| enemy.entity_id).collect();
    let mut issued = 0;
    for unit in fighters.iter().cycle()
        .skip((obs.tick() as usize) % fighters.len().max(1))
        .take(fighters.len().min(32))
    {
        if issued >= 7 { break; }
        if !unit.controllable() || unit.in_service() || obs.is_deployed(unit.entity_id)
            || unit.slot == super::census::Slot::Aircraft
            || super::orders::ordered_this_frame(out, unit.entity_id)
        { continue; }
        if unit.health_permille >= 600 && obs.attack_target(unit.entity_id).is_some_and(|target|
            mobile_contacts.contains(&target) && super::safety::firing_here(rules,obs,unit,target)) {
            continue;
        }
        // Unlike a short-range screen, an artillery unit can keep shelling
        // during Stockpile if it has a safe actual firing annulus. Use a
        // ground-reachable post relative to the *gun itself*, not merely the
        // current strategic objective's centre, which may lie 14 cells away.
        let artillery_moving = tracker.live_intent(obs.tick(),unit.entity_id)
            .is_some_and(|(kind,target)| kind == super::tracked::TrackedKind::Avoid
                && target.is_some_and(|id| guns.iter().any(|(gun,at,_)| *gun == id
                    && obs.destination(unit.entity_id).is_some_and(|dest|
                        super::safety::firing_post(rules,obs,unit,id,*at,false)==Some(dest)))));
        if artillery_moving { continue; }
        let mut post = None;
        for range in [radius, radius + 4, radius + 8] {
            for shift in [0i32, 3, -3, 6, -6] {
                let x = i32::from(front.0) + dx * range + lateral.0 * shift;
                let y = i32::from(front.1) + dy * range + lateral.1 * shift;
                let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else { continue };
                let cell = (x,y);
                if !obs.is_ground_walkable(x,y)
                    || !super::orders::reachable(rules,obs,unit.entity_id,cell)
                    || super::tactics::unsafe_post(rules,obs,unit.entity_id,cell)
                { continue; }
                if guns.iter().any(|(_,gun,reach)|
                    (cell_distance(unit.cell,*gun) > *reach + 2)
                    && super::orders::path_crosses(&[*gun],unit.cell,cell,*reach + 2))
                { continue; }
                post = Some(cell);
                break;
            }
            if post.is_some() { break; }
        }
        // An artillery unit legitimately shelling a gun at standoff range
        // must not be stopped merely for holding an Attack mission on a
        // building. Otherwise reserve formation cancels every second salvo.
        let safe_battery_attack = guns.iter().any(|(id,_,_)| {
            obs.attack_target(unit.entity_id) == Some(*id)
                && super::safety::firing_here(rules,obs,unit,*id)
        });
        if safe_battery_attack { continue; }
        let dangerous_old_attack = !safe_battery_attack && (
            obs.attack_target(unit.entity_id)
            .is_some_and(|id| visible_structures.contains(&id))
            || obs.destination(unit.entity_id).is_some_and(|cell|
                cell_distance(cell,front) <= 10));
        if let Some(post) = post {
            if cell_distance(unit.cell,post) <= 3 {
                if dangerous_old_attack && super::orders::issue_tracked(
                    tracker,trace,out,rules,obs,unit.entity_id,
                    super::tracked::TrackedKind::Avoid,None,Some(post),Goal::Stop,
                ) { issued += 1; }
                continue;
            }
            if obs.destination(unit.entity_id) == Some(post) { continue; }
            if super::orders::issue_tracked(
                tracker,trace,out,rules,obs,unit.entity_id,
                super::tracked::TrackedKind::Avoid,None,Some(post),Goal::Move { cell: post },
            ) { issued += 1; }
        } else if dangerous_old_attack && super::orders::issue_tracked(
            tracker,trace,out,rules,obs,unit.entity_id,
            super::tracked::TrackedKind::Avoid,None,None,Goal::Stop,
        ) { issued += 1; }
    }
    issued
}

/// An outgunned front should not be an inert front. A small number of
/// visible enemy units/towers may be traded with safely by standoff pieces
/// even while the rest of the army waits for reinforcements. This is not an
/// all-army push: only units whose *actual* weapon outranges the target and
/// can damage it are allowed to step forward. It also prioritises psychic
/// controllers, which are much more dangerous than their nominal DPS.
fn limited_front_skirmish(
    tracker: &mut super::tracked::Tracker, trace: Option<u64>,
    fighters: &[&Unit], rules: &RuleSet, obs: &Observation<'_>,
    out: &mut Outbox, front: (u16,u16),
) -> bool {
    let visible: Vec<_> = obs.visible_enemy_entities()
        .filter(|enemy| cell_distance(enemy.cell, front) <= 18)
        .collect();
    let mut active = false;
    let mut issued = 0;
    for unit in fighters.iter().take(128) {
        if !unit.controllable() || unit.in_service() || unit.traits.aircraft
            || unit.slot == super::census::Slot::Ship
            || super::orders::ordered_this_frame(out,unit.entity_id)
            || super::orders::committed_garrison_entry(tracker,rules,obs,unit.entity_id)
        { continue; }
        let picked = visible.iter().filter_map(|enemy| {
            if !obs.can_hurt(rules,unit.entity_id,enemy.entity_id)
                || !super::orders::admits(rules,obs,unit.entity_id,
                    &Goal::Attack { target: enemy.entity_id })
            { return None; }
            let ours = super::tactics::actual_threat_reach(
                rules,obs,unit.entity_id,enemy.entity_id);
            let theirs = super::tactics::actual_threat_reach(
                rules,obs,enemy.entity_id,unit.entity_id);
            if ours < 5 || ours <= theirs + 1
                || cell_distance(unit.cell,enemy.cell) > 26
                || super::tactics::unfavourable_vehicle_duel(
                    rules,obs,unit.entity_id,enemy.entity_id)
            { return None; }
            // Winning the range comparison with *one* tower does not mean a
            // lone artillery unit may walk into the other towers around it.
            // It can fire from its safe present position, or wait for its wave.
            if obs.entity_type(rules, enemy.entity_id)
                .is_some_and(|ty| super::siege_counter::dangerous_battery(rules, ty))
                && (cell_distance(unit.cell, enemy.cell) > ours
                    || super::tactics::unsafe_post(rules, obs, unit.entity_id, unit.cell))
            { return None; }
            let control = obs.entity_type(rules,enemy.entity_id)
                .is_some_and(|ty| super::tactics::mind_control_reach(rules,ty).is_some());
            Some((!control, cell_distance(unit.cell,enemy.cell), enemy.entity_id,
                enemy.cell))
        }).min_by_key(|(not_control,distance,id,_)| (*not_control,*distance,*id));
        let Some((_, _, target, cell)) = picked else { continue; };
        if obs.attack_target(unit.entity_id) == Some(target) {
            active = true;
            continue;
        }
        if issued >= 3 { continue; }
        if super::orders::issue_tracked(tracker,trace,out,rules,obs,
            unit.entity_id,super::tracked::TrackedKind::Attack,
            Some(target),Some(cell),Goal::Attack { target }) {
            active = true;
            issued += 1;
        }
    }
    active
}

/// A range trade qualifies as safe battery fire only when the *same*
/// effective ground weapon can actually hurt the tower.
fn standoff_can_breach(own: i32, enemy: i32, can_damage: bool) -> bool {
    can_damage && own >= 6 && own > enemy + 1
}

/// Direct available standoff artillery at the *visible, powered* battery
/// covering the assault. An Attack on the object lets the native weapon-range
/// approach stop in firing position; AttackMove to the base's centre makes a
/// prism tank oscillate while the forward screen is erased one by one.
fn assign_breach_battery(
    tracker: &mut super::tracked::Tracker, trace: Option<u64>,
    fighters: &[&Unit], rules: &RuleSet, obs: &Observation<'_>,
    out: &mut Outbox, gun: EnemyEntity,
) -> bool {
    let mut working = false;
    let mut issued = 0;
    for unit in fighters {
        if !unit.controllable() || unit.in_service() || obs.is_deployed(unit.entity_id)
            || unit.traits.aircraft || unit.slot == super::census::Slot::Ship
            || !obs.can_hurt(rules,unit.entity_id,gun.entity_id) { continue; }
        let own = super::tactics::actual_threat_reach(rules,obs,unit.entity_id,gun.entity_id);
        let enemy = super::tactics::actual_threat_reach(rules,obs,gun.entity_id,unit.entity_id);
        if !standoff_can_breach(own,enemy,true) { continue; }
        let firing = super::safety::firing_here(rules,obs,unit,gun.entity_id);
        let heading_to_post = obs.destination(unit.entity_id).is_some_and(|post|
            super::safety::firing_post(rules,obs,unit,gun.entity_id,gun.cell,false)==Some(post)
                && obs.attack_target(unit.entity_id).is_none());
        working |= (firing && obs.attack_target(unit.entity_id)==Some(gun.entity_id)) || heading_to_post;
        if issued >= 2 || super::orders::ordered_this_frame(out,unit.entity_id) { continue; }
        if super::siege_counter::work_battery(tracker,trace,rules,obs,out,unit,
            gun.entity_id,gun.cell,obs.tick()%15 == unit.entity_id%15) {
            issued += 1;
            working |= out.orders.last().is_some_and(|o|o.actor==unit.entity_id
                && match o.goal {
                    Goal::Move {..}=>true,
                    Goal::Attack {target}=>target==gun.entity_id,
                    _=>false,
                });
        }
    }
    working
}

/// The front is outnumbered *locally* although its rear has enough strength
/// to win the exchange. A short staging window moves the tail to a position
/// just behind the tip; it never selects a different enemy objective.
fn stage_reinforcements(
    tracker: &mut super::tracked::Tracker, trace: Option<u64>,
    fighters: &[&Unit], rules: &RuleSet, obs: &Observation<'_>,
    out: &mut Outbox, target: (u16,u16),
) -> usize {
    let Some(tip) = fighters.iter()
        .filter(|unit| cell_distance(unit.cell,target) <= 18)
        .min_by_key(|unit| (cell_distance(unit.cell,target),unit.entity_id))
    else { return 0; };
    let mut dx = (i32::from(tip.cell.0) - i32::from(target.0)).signum();
    let dy = (i32::from(tip.cell.1) - i32::from(target.1)).signum();
    if dx == 0 && dy == 0 { dx = 1; }
    let mut posts = Vec::new();
    let lateral = (-dy,dx);
    for range in [16i32,18,14,20] {
        for shift in [0,-2,2,-4,4] {
            let (Ok(x),Ok(y)) = (
                u16::try_from(i32::from(target.0) + dx * range + lateral.0 * shift),
                u16::try_from(i32::from(target.1) + dy * range + lateral.1 * shift),
            ) else { continue; };
            if obs.is_ground_walkable(x,y) { posts.push((x,y)); }
        }
    }
    let mut issued = 0;
    for unit in fighters.iter().cycle()
        .skip((obs.tick() as usize) % fighters.len().max(1))
        .take(fighters.len().min(128))
    {
        if issued >= 5 { break; }
        if !unit.controllable() || unit.in_service() || obs.is_deployed(unit.entity_id)
            || unit.slot == super::census::Slot::Aircraft
            || super::micro::firing(obs,unit.mission,unit.entity_id)
            || super::orders::ordered_this_frame(out,unit.entity_id)
        { continue; }
        let Some(post) = posts.iter().copied()
            .cycle().skip((unit.entity_id as usize) % posts.len().max(1))
            .take(posts.len())
            .find(|post| super::orders::reachable(rules,obs,unit.entity_id,*post)
                && !super::tactics::unsafe_post(rules,obs,unit.entity_id,*post))
        else { continue; };
        if cell_distance(unit.cell,post) <= 3
            || obs.destination(unit.entity_id) == Some(post)
        { continue; }
        if super::orders::issue_tracked(
            tracker,trace,out,rules,obs,unit.entity_id,
            super::tracked::TrackedKind::Avoid,None,Some(post),Goal::Move { cell: post },
        ) { issued += 1; }
    }
    issued
}

/// Permit a stalled, well-supported heavy tank column to breach a visible
/// gunline even when the perfect counter is not affordable. No infantry or
/// damaged lone vehicles are authorized to trade against multiple towers.
#[allow(dead_code)]
fn forced_armor_commit(armored: bool, health_permille: u16, own: i32, enemy: i32) -> bool {
    armored && health_permille >= 780 && own >= 2_000
        && own >= enemy.max(1).saturating_mul(2)
}

#[cfg(all(test, tests_sim_p_s))]
mod forced_breach_tests {
    use super::forced_armor_commit;

    #[test]
    fn only_a_healthy_locally_superior_armored_column_may_break_the_siege() {
        assert!(forced_armor_commit(true, 900, 5_000, 2_200));
        assert!(!forced_armor_commit(false, 900, 5_000, 1_000));
        assert!(!forced_armor_commit(true, 400, 5_000, 1_000));
        assert!(!forced_armor_commit(true, 900, 1_500, 200));
        assert!(!forced_armor_commit(true, 900, 3_000, 2_000));
    }
}

/// Recall exposed offensive squads through the normal intent tracker. Local
/// defence and a current shot from outside the target's reach stay in place.
pub(super) fn withdraw_for_recovery(
    brain: &mut AssistBrain, rules: &RuleSet, obs: &Observation<'_>, out: &mut Outbox,
) {
    let Some(home) = brain.census.base_cell else { return; };
    let visible: std::collections::BTreeMap<_, _> = obs.visible_enemy_entities()
        .map(|enemy| (enemy.entity_id, enemy.cell)).collect();
    let tick = obs.tick();
    let mut issued = 0;
    let units = &brain.census.units;
    for unit in units.iter().cycle().skip((tick as usize) % units.len().max(1)).take(units.len().min(128)) {
        if issued >= ADVANCE_ORDERS_PER_FRAME { break; }
        let id = unit.entity_id;
        let offensive = [super::squads::Role::Attack, super::squads::Role::Harass].iter()
            .any(|&role| brain.squads.squad(role).is_some_and(|squad| squad.members.contains(&id)));
        if !offensive || !unit.controllable() || unit.in_service() || unit.traits.yard_maker
            || !matches!(unit.slot, super::census::Slot::Infantry | super::census::Slot::Vehicle)
            || brain.joint_ops.reserves(id) || super::orders::ordered_this_frame(out, id)
        {
            continue;
        }
        let outside = |cell| squared_distance(cell, home) > THREAT_RADIUS * THREAT_RADIUS;
        let target = obs.attack_target(id);
        let target_cell = target.and_then(|target| visible.get(&target).copied());
        let destination = obs.destination(id);
        if target.is_none() && destination.is_some() && destination == brain.census.rally {
            continue;
        }
        if !outside(unit.cell) && !destination.is_some_and(outside)
            && !target_cell.is_some_and(outside) && (target.is_none() || target_cell.is_some())
        {
            continue;
        }
        if let Some((target, cell)) = target.zip(target_cell) {
            if !outside(cell) || (super::convergence::near_income(&brain.census,cell)
                && obs.engage(rules,id,target) == crate::sim::control::observation::Engage::Ready) {
                continue;
            }
            let distance = cell_distance(unit.cell, cell);
            let our_reach = super::tactics::effective_reach_against(rules, obs, id, target);
            let their_reach = super::tactics::effective_reach_against(rules, obs, target, id);
            if distance <= our_reach && distance > their_reach + 1
                && !super::tactics::unsafe_post(rules, obs, id, unit.cell)
            {
                continue;
            }
        }
        let post = brain.census.rally.filter(|&(x, y)| {
            obs.is_ground_walkable(x, y) && (x, y) != unit.cell
                && super::orders::reachable(rules, obs, id, (x, y))
                && !super::tactics::unsafe_post(rules, obs, id, (x, y))
        });
        let goal = match post {
            Some(cell) if outside(unit.cell) => Goal::Move { cell },
            _ if target.is_some() || destination.is_some_and(outside) => Goal::Stop,
            _ => continue,
        };
        if super::orders::issue_tracked(&mut brain.tracked, brain.trace, out, rules, obs, id,
            super::tracked::TrackedKind::Avoid, None, post, goal)
        {
            brain.tracked.forget_primary(id);
            issued += 1;
        }
    }
}

pub(super) fn advance(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) -> bool {
    if brain.director.rebuilding_force() || brain.mobilization.active() || brain.campaign.economy_resting() {
        brain.front_review.pause();
        brain.push_gate = if brain.director.rebuilding_force() { "rebuild-army" }
            else if brain.mobilization.active() { "mobilizing" } else { "economy-rest" };
        brain.assault_wait = None;
        end_charge(brain,obs,"strategic-interrupt");
        brain.blackout = None;
        brain.blackout_withdraw_after.clear();
        return false;
    }
    if !brain.census.units.iter().any(|unit| unit.traits.combat) {
        brain.push_gate = "no-fighters";
        return false;
    }
    // An army the base cannot replace is not an army, it is the whole house
    // walking out of the gate; and a base with nothing to hold it is a base
    // that falls while the army is away. The third gate is the one that
    // decides matches: an enemy already at the yard is what the army is for,
    // and marching out with it in the base loses the production the army
    // came from.
    // The army, as the one object that answers "who is in it and where it
    // stands": the same list every pass below reads, instead of each pass
    // filtering the ledger its own way.
    let army = super::army::Army::of(&brain.census, obs);
    // The fist, not the whole army: the teams already answer who holds the
    // base and who raids, and a push that takes everyone with it is a base
    // left empty the moment the front moves. A house with no team yet (the
    // first frames) still pushes with everyone, which is the opening it is.
    //
    // This is the line the squads draw: Defend stays where it is, Harass has
    // its own orders, and what is left is the advance's. Reading `army.members`
    // here meant every raider and every defender walked at the same structure
    // as the push, so the two teams existed only in the ledger.
    let fist: Option<&BTreeSet<u64>> = brain
        .squads
        .squad(super::squads::Role::Attack)
        .map(|squad| &squad.members);
    let mut fighters: Vec<&Unit> = match fist {
        Some(members) => army
            .members
            .iter()
            .filter(|unit| members.contains(&unit.entity_id))
            .collect(),
        None => army.members.iter().collect(),
    };
    // A hero does not march with the fist in the first place. `BuildLimit=1` is
    // a unit the house cannot buy another of, and a push that spends it for a
    // structure has traded the whole match for one building — the wishlist's
    // "保护英雄单位". They hold at home, which is where the defence step already
    // sends them, and the retreat line walks them out well before the ordinary
    // one when they are caught.
    fighters.retain(|unit| !unit.traits.irreplaceable && !unit.traits.yard_maker
        && !brain.specialist_shuttles.contains_key(&unit.entity_id)
        && !brain.specialist_shuttles.values().any(|trip| trip.passenger == unit.entity_id)
        && !brain.crate_claims.contains_key(&unit.entity_id)
        && !brain.crate_returns.contains_key(&unit.entity_id)
        && !brain.repair_ifv_pending.get(&unit.entity_id)
            .is_some_and(|until| *until > obs.tick())
        && !super::advanced::repair_ifv_loaded(rules, obs, &brain.census, unit.entity_id)
        // V38 security/escort detachments cannot receive a conflicting
        // campaign march while the joint operation is active.
        && !brain.joint_ops.reserves(unit.entity_id));
    if fighters.is_empty() {
        brain.push_gate = "no-eligible-fighters";
        return false;
    }
    if drive_blackout(brain, rules, obs, out, &fighters) { return true; }
    if execute_siege_charge(brain, rules, obs, out) { return true; }
    // The strategic campaign's stockpile is not a combat order. Defence,
    // harassing, scouting and the task ledger have already run this frame;
    // only the dedicated Attack fist is barred from piecemeal probing.
    let strategic_hold = brain.campaign.holding_fist();
    // No exclusion for a unit whose weapon spends it. It was here with a
    // matching errand of its own, and together they cost fourfold on two seeds
    // (see the note where the errand used to be called). The tag itself is
    // correct retail data; what measured badly is every policy built on it.
    // Which gate holds the army home, for the census line: seven gates that
    // all answer "not this frame" are indistinguishable in a log that only
    // counts what the house has.
    // Whether a raid is on the base right now: the ring [`base_under_threat`]
    // reads, counted rather than weighed.
    // Use the same visible threat classification as the economy/defence
    // director. A known unarmed building or a fogged contact is not by
    // itself a new order to recall the whole offensive fist.
    let raided=brain.director.under_attack_now();
    // An opening on depleted random ore is not an ordinary race to the enemy:
    // a tank lost outside the yard may be the last tank the house can afford.
    // This is bounded to the opening, and only uses legitimately *seen* ore
    // plus the house's measured deliveries, not hidden map deposits.
    let scarce_opening = tick_limited_scarce_opening(
        obs.tick(), obs.credits(), brain.income.per_minute(),
        brain.ore_known.len(), fighters.len(), raided,
    );
    let small = fighters.len() < super::squads::ARMY_TO_ATTACK && !mop_up(&brain.census);
    let local_opportunity = small && obs.visible_enemy_entities().any(|e|
        super::convergence::mobile_threat(rules,obs,&e)
            && fighters.iter().any(|u| super::convergence::local_rank(&brain.census,u.cell,&e).is_some()
                && obs.engage(rules,u.entity_id,e.entity_id)==crate::sim::control::observation::Engage::Ready
                && obs.can_hurt(rules,u.entity_id,e.entity_id)));
    let gate = if scarce_opening && !local_opportunity {
        "scarce-opening"
    } else if small && !local_opportunity {
        "too-small"
    } else if !super::build::can_replace_units(rules, obs, &brain.census) && raided {
        // A house that cannot replace its army does not spend it — *while it
        // still has a base to lose*. The two halves of that sentence are the
        // whole gate.
        //
        // Without the second half it is a death spiral: the enemy takes the war
        // factory, the gate turns true, the army that could have taken the
        // factory back is held in the yard, and the house waits to die with its
        // hands down. Measured on the 1v29 (team 3), `push=cannot-replace` held
        // for five of the last samples while 130 of ours died and the enemy lost
        // four objects.
        //
        // With a raid on the base there is still something to defend, so the
        // army stays; with the ring clear the army is the only thing the house
        // has left and the only way back is forward.
        "cannot-replace"
    } else if !super::build::base_defended(rules, obs, &brain.census) && raided {
        // A tower is held `want: 2` and the attack gate used to wait for one
        // unconditionally, so a base that had not finished its first tower
        // never attacked at all. It is the same "waiting to die" spiral, one
        // building earlier. With the ring clear the army goes; the tower is the
        // plan's business, not the push's.
        "base-defended"
    } else {
        "push"
    };
    // The commit window: a gate that flipped away runs for its term whatever the
    // next frame's numbers say. Read before the gate is published so the census
    // line still reports the fresh answer.
    let tick = obs.tick();
    let resting = resting(brain, tick);
    let gate = if gate != "push" {
        brain.push_rest = (tick, gate);
        gate
    } else if resting && !(local_opportunity && matches!(brain.push_rest.1,"too-small" | "scarce-opening")) {
        brain.push_rest.1
    } else {
        "push"
    };
    brain.push_gate = gate;
    if gate != "push" {
        brain.front_review.pause();
        brain.assault_wait = None;
        // A squad can drop below the normal advance-size gate *after* we
        // chose a strategic reserve. Keep ownership of its old dangerous
        // native order even when there is no new target selection this frame.
        if strategic_hold && !raided {
            if let Some((_,_,front)) = brain.push_target {
                stage_strategic_reserve(&mut brain.tracked,brain.trace,
                    &fighters,rules,obs,out,front,
                    brain.census.rally.or(brain.census.base_cell));
            }
            return true;
        }
        return false;
    }
    let tick = obs.tick();
    // Which target: the whole reading of the enemy -- structures, economy,
    // clean-up, reachability -- is one decision and lives in `targets`.
    // A remote home defender is not the assault's positional anchor.
    let anchor=main_fist_anchor(&fighters,army.anchor);
    let (answer, progress) = targets::choose_focused(
        &brain.census,
        &brain.tracked,
        tick,
        brain.push_progress,
        anchor,
        &fighters,
        rules,
        obs,
        brain.campaign_house,
        brain.recon.enemy_search_site(obs, anchor),
        brain.front_review.deferred(tick),
    );
    brain.push_progress = progress;
    // A target switch or a pause cannot manufacture progress. Only a live
    // chosen hostile may be sampled by the front-review ledger.
    let target = match answer {
        targets::Answer::NowhereToLook => {
            brain.push_gate = "nowhere-to-look";
            brain.assault_wait = None;
            return false;
        }
        targets::Answer::NoTarget => {
            brain.push_gate = "no-target";
            brain.assault_wait = None;
            return false;
        }
        targets::Answer::Chosen(chosen) => chosen,
    };
    if small && !target.local_contact {
        brain.push_gate = "too-small";
        return false;
    }
    if !target.local_contact { brain.campaign_house = obs.visible_enemy_house(target.nearest.0); }
    if target.local_contact && tick % 600 == 0 {
        log::info!("auto play: local_contact tick={tick} target={} cell={:?}",target.entity_id,target.cell);
    }
    // The reachable target, not the blocker: the push's bookkeeping is about
    // where the army is going, and the blocker is only what it shoots first.
    brain.push_target = Some((target.nearest.0, tick, target.nearest.1));
    let mopping_up = target.mopping_up;
    let (target_id, target_cell) = (target.entity_id, target.cell);
    let barrier = if target_id != 0 && !target.local_contact {
        super::tactics::siege_barrier(rules,obs,target_cell,&fighters)
    } else { None };
    let siege_reach = barrier.map(|(_,reach)| reach);
    brain.siege_reach = siege_reach;
    // Observe the actual blocking battery, not an economy building that the
    // artillery has never had a chance to shoot. A stationary gun with no
    // damage progress cannot reset the front-review clock by reissuing Move.
    let (review_id,review_cell) = barrier.map_or((target_id,target_cell),|(gun,_)|(gun.entity_id,gun.cell));
    if !strategic_hold && !raided && !target.local_contact
        && fighters.iter().any(|u| cell_distance(u.cell,review_cell)<=24) {
        brain.front_review.observe(rules,obs,target.nearest.0,review_id,review_cell);
    } else { brain.front_review.pause(); }
    if strategic_hold && !raided {
        // A reserve needs a *physical* safe post and must cancel existing
        // native attack missions. Merely returning false here allowed the
        // old AttackMove to keep carrying tanks into prism batteries.
        stage_siege(&mut brain.tracked, brain.trace,
            &fighters, rules, obs, out, target_cell,false);
        stage_strategic_reserve(&mut brain.tracked, brain.trace,
            &fighters, rules, obs, out, target_cell,
            brain.census.rally.or(brain.census.base_cell));
        // Keep the real target/reach for the production planner. The economy
        // may then select long-ranged damage instead of feeding line tanks.
        brain.push_gate = if siege_reach.is_some() { "need-siege" }
            else { "strategic-stockpile" };
        return true;
    }
    // Real standoff batteries must be allowed to fire while the shorter-range
    // screen holds outside the enemy gun line. This is a bounded *breach*, not
    // another request to make more artillery when one already exists.
    if let Some((gun,_)) = barrier
        && assign_breach_battery(
            &mut brain.tracked, brain.trace, &fighters, rules, obs, out, gun,
        )
    {
        if brain.siege_wait.is_none_or(|(id,_)| id != gun.entity_id) {
            brain.siege_wait = Some((gun.entity_id,tick));
        }
        // An artillery unit has an actual winning range trade. Do not let
        // the arbitrary waiting deadline turn the screen into isolated tower
        // probes while that weapon is working toward the live battery.
        brain.push_gate = if brain.front_review.deferred(tick).is_some_and(|(_,cell)|
            cell_distance(cell,gun.cell)<=12) { "battery-review" } else { "battery-breach" };
        stage_siege(&mut brain.tracked, brain.trace, &fighters, rules, obs, out, target_cell,true);
        return true;
    }
    if let Some(reach) = siege_reach {
        // Keep a bounded artillery preparation window for one enemy target.
        // A missing factory, an unaffordable price, or a refused production
        // order must never freeze the old successful v10 assault indefinitely.
        let blocked_id=barrier.map_or(target_id,|(gun,_)| gun.entity_id);
        let start = match brain.siege_wait {
            Some((id, since)) if id == blocked_id => since,
            _ => tick,
        };
        brain.siege_wait = Some((blocked_id, start));
        // There is no clock-based lone-tank breach against an operational
        // battery. Either a real assembled wave commits (above), or we stage
        // for standoff artillery/spy blackout while the raiders hit income.
        brain.push_gate = "need-siege";
        let spy_power_attempt_live = brain.spy_attempts.iter().any(|(agent, (plant, _, _))| {
            brain.census.units.iter().any(|unit|
                unit.entity_id == *agent && unit.traits.spy)
                && brain.campaign_house.is_some()
                && obs.visible_enemy_house(*plant) == brain.campaign_house
                && obs.visible_enemy_entities().any(|enemy|
                    enemy.entity_id == *plant && obs.entity_type(rules, *plant)
                        .is_some_and(|ty| ty.power > 0))
        });
        // When a real spy is walking to this same house's visible power
        // plant, hold the heavy cannon assault briefly for the blackout.
        // Visibility and the native Capture intent, not a guessed effect,
        // justify the wait; after 1,500 frames it must not deadlock the army.
        let blackout_window = siege_blackout_wait(start, tick, spy_power_attempt_live);
        if blackout_window {
            brain.push_gate = "spy-blackout-staging";
            if brain.blackout.is_none() {
                if let Some(battery) = obs.visible_enemy_entities().filter(|enemy|
                    cell_distance(enemy.cell, target_cell) <= 14)
                    .filter(|enemy| obs.entity_type(rules, enemy.entity_id)
                        .is_some_and(|ty| super::siege_counter::dangerous_battery(rules, ty))
                        && obs.visible_enemy_building_operational(rules, enemy.entity_id))
                    .max_by_key(|enemy| obs.entity_type(rules, enemy.entity_id)
                        .map_or(0, |ty| weapon_range_cells(rules, ty, false)))
                {
                    brain.blackout = Some(BlackoutOperation {
                        battery: battery.entity_id, cell: battery.cell,
                        started: tick, phase: BlackoutPhase::Staging, off_since: None,
                        power_focus: None,
                    });
                    log::info!("auto play: coordinated blackout plan for battery {}",
                        battery.entity_id);
                }
            }
        }
        // Waiting must be an actual manoeuvre, not just a label. A tank
        // already walking into the pillbox will keep that native order
        // unless we deliberately move it back out of the gunline.
        stage_siege(&mut brain.tracked,brain.trace,&fighters,rules,obs,out,target_cell,true);
        log::debug!("auto play: stage for siege reach>{reach} at {target_cell:?}");
        return true;
    } else {
        brain.siege_wait = None;
    }
    // A crate on the way is a crate the army walks over.
    //
    // The box is a bonus and not a job, and the walk to the structure passes
    // it: the unit stands on the cell, the map takes it, and the next frame
    // the army is at the structure with the money. A crate the army walked
    // past because it was busy shooting is a crate the play test reports as
    // "it never picks the boxes up".
    let target_cell = crate_on_the_way(rules, obs, target_cell).unwrap_or(target_cell);
    // And a fist big enough to take it. The size gate above counts heads; this
    // one weighs them against what is standing there, and an army that cannot
    // win the fight stays home and grows instead of being spent.
    //
    // Except when there is nothing left to weigh. A house down to a handful of
    // objects is not a base to be beaten, it is what is left of one — and the
    // gate that stops the house spending an army it cannot replace has no fight
    // left to protect it from. Without this the match ends in a timeout with
    // the army standing in front of two walls: measured, the enemy was down to
    // 21 objects and the push never left, on every variant that changed what
    // gets ordered. This is the reason those variants looked worse than they
    // were.
    // A search is not a fight: there is nothing to weigh the army against, and
    // the march is the point.
    let searching = target.entity_id == 0;
    // A fist is what actually crosses the map, not the units holding home.
    // Never count the garrison or a harasser as support for a target they will
    // not arrive at. Use the *same* fighter list used to issue this push.
    let committed_points = fighters.iter().fold(0i32, |total, unit| {
        total.saturating_add(unit.points)
    });
    if !mopping_up
        && !searching
        && !super::squads::outguns(&brain.census, rules, obs, target.cell, committed_points)
    {
        // The same commit window as the gates above: `outguns` weighs the army
        // against whatever stands around this frame's target, so a target the
        // army is walking past can turn the answer for a second and back.
        brain.push_gate = "outgunned";
        brain.push_rest = (obs.tick(), "outgunned");
        // When the fist cannot win a frontal assault, standoff units can
        // still dismantle exposed guards and controllers at the doorway.
        // The weaker tanks remain behind the line instead of repeatedly
        // charging and being lost to the same guns.
        if limited_front_skirmish(&mut brain.tracked,brain.trace,&fighters,
            rules,obs,out,target_cell)
        {
            brain.push_gate = "standoff-skirmish";
            return true;
        }
        return false;
    }

    // A strong army can still lose when only its fastest two units meet the
    // enemy and the others are twenty cells behind. Look at currently visible
    // contact strength, not the all-map force total. Wait one bounded wave;
    // if the rear cannot arrive, do not freeze this previously winning push.
    if !mopping_up && !searching
        && super::tactics::waiting_for_support(rules,obs,target_cell,&fighters)
    {
        let start = match brain.assault_wait {
            Some((id,since)) if id == target_id => since,
            _ => tick,
        };
        brain.assault_wait = Some((target_id,start));
        if tick.saturating_sub(start) < 240 {
            brain.push_gate = "await-support";
            stage_reinforcements(
                &mut brain.tracked,brain.trace,&fighters,rules,obs,out,target_cell,
            );
            return true;
        }
    } else {
        brain.assault_wait = None;
    }

    // A tank built in a rich stalemate is not a tank committed to battle.
    // Wait for three real armoured vehicles within twelve cells of one
    // another. All tanks use one finite global timer, not a per-target timer
    // that can be reset every time the target selector changes its mind.
    let armour: Vec<&Unit> = fighters.iter().copied()
        .filter(|unit| armoured_fighter(unit, rules, obs)).collect();
    let broken_wave = armour.iter().any(|unit| {
        armour.iter().filter(|other| cell_distance(unit.cell, other.cell) <= 12)
            .count() < ARMOUR_WAVE_MIN
    });
    if !raided && !mopping_up && !searching && broken_wave {
        brain.armour_wave_wait.get_or_insert(tick);
    } else {
        brain.armour_wave_wait = None;
    }

    // A front with real armor/infantry screening can carry long-range guns
    // behind it. This assigns actual Attack targets or safe rear staging posts
    // before the generic fist march; the returned guns must never receive the
    // same-frame AttackMove to the structure's centre.
    let support_holds = fire_support::push(brain, rules, obs, out, &fighters, target_cell);
    // This set makes a dead-target check one O(log N) lookup per soldier,
    // rather than scanning every visible entity once for every soldier.
    let live_hostile_ids: BTreeSet<u64> = obs.visible_enemy_entities()
        .map(|enemy| enemy.entity_id).collect();

    // The frame's budget is a handful of orders and the rest of the list is
    // dropped, so the scan's order decides who is ever told anything at all:
    // the same leading units were served every frame and the rest of the army
    // stood where it was built, which is what "the infantry just stands there"
    // looks like from the player's side. Rotating the start by the tick gives
    // every unit its turn, and an order holds for its term once given.
    //
    // Taken after the anchor, so the target this frame is aimed at does not
    // depend on who happens to be scanned first.
    let turn = fighters.len();
    if turn > 0 {
        fighters.rotate_left((tick as usize) % turn);
    }
    if super::trace_enabled("AUTOPLAY_TRACE_PUSH") {
        let distances: Vec<i32> = fighters
            .iter()
            .map(|unit| {
                let dx = i32::from(unit.cell.0) - i32::from(target_cell.0);
                let dy = i32::from(unit.cell.1) - i32::from(target_cell.1);
                dx * dx + dy * dy
            })
            .collect();
        let spread = distances.iter().max().copied().unwrap_or(0)
            - distances.iter().min().copied().unwrap_or(0);
        let moving = fighters
            .iter()
            .filter(|unit| {
                obs.destination(unit.entity_id).is_some()
                    || obs.attack_target(unit.entity_id).is_some()
            })
            .count();
        let near = distances
            .iter()
            .filter(|squared| **squared <= 12 * 12)
            .count();
        let cx = if fighters.is_empty() {
            0
        } else {
            let sum: i64 = fighters.iter().map(|unit| i64::from(unit.cell.0)).sum();
            (sum / i64::try_from(fighters.len()).unwrap_or(1)) as i32
        };
        let cy = if fighters.is_empty() {
            0
        } else {
            let sum: i64 = fighters.iter().map(|unit| i64::from(unit.cell.1)).sum();
            (sum / i64::try_from(fighters.len()).unwrap_or(1)) as i32
        };
        log::info!(
            "push trace: tick={} fighters={} moving={} near={} spread={} fist=({},{}) target={:?} at {:?} hp={:?} enemies={}",
            tick,
            fighters.len(),
            moving,
            near,
            spread,
            cx,
            cy,
            target_id,
            target_cell,
            obs.health(target_id),
            brain.census.enemies.len(),
        );
    }
    // The fighter nearest the target is the tip of the fist. A unit more than
    // STAGGER_BEHIND cells behind it never got a second order: it walked, it
    // arrived, its mission id stayed Move, and the skip below read that as
    // "already walking" for the rest of the match. Measured on Death.mmx seed
    // 24301 at tick 31876: 65 fighters, 7 with a destination, 19 inside twelve
    // cells of the target, spread 131 cells -- the tail was standing still.
    let tip = fighters
        .iter()
        .map(|unit| unit.cell)
        .min_by_key(|at| cell_distance(*at, target_cell))
        .unwrap_or(target_cell);
    let left_behind = |unit: &Unit| cell_distance(unit.cell, tip) > STAGGER_BEHIND;
    // A common target alone does not form a tank column: the fast tip can be
    // half a map ahead of its heavy support while the tail still has the old
    // waypoint. In safe territory, name a *temporary* march waypoint beside
    // the leading tanks for those that are far behind. Do not stop fighters
    // in contact or redirect the leading vehicles away from a won objective.
    let tank_rally = tank_cohesion_post(&fighters, target_cell, obs, &brain.census);
    if let Some(anchor) = tank_rally {
        // Use the scarce order budget on the lagging tanks first. Once their
        // new waypoint stands in Tracker they stop consuming this quota.
        fighters.sort_by_key(|unit| (
            !needs_tank_cohesion(unit, anchor),
            cell_distance(unit.cell, anchor),
            unit.entity_id,
        ));
    }

        if super::trace_enabled("AUTOPLAY_TRACE_SKIP") && tick % 600 == 0 {
            let mut deployed = 0;
            let mut attacking = 0;
            let mut walking = 0;
            let mut far = 0;
            let mut other = 0;
            for unit in &fighters {
                let attack = unit.mission == MissionId::from_known(MissionType::Attack)
                    && obs.attack_target(unit.entity_id)
                        .is_some_and(|target| live_hostile_ids.contains(&target));
                let move_ = unit.mission == MissionId::from_known(MissionType::Move);
                if obs.is_deployed(unit.entity_id) {
                    deployed += 1;
                } else if attack {
                    attacking += 1;
                } else if move_ && (obs.destination(unit.entity_id).is_some() || !left_behind(unit)) {
                    walking += 1;
                } else if move_ {
                    far += 1;
                } else {
                    other += 1;
                }
            }
            log::info!(
                "skip trace: tick={} fighters={} deployed={} attacking={} walking={} far={} other={} target={:?} at {:?}",
                tick,
                fighters.len(),
                deployed,
                attacking,
                walking,
                far,
                other,
                target_id,
                target_cell,
            );
        }
    let mut issued = 0;
    let mut refused = 0;
    for unit in &fighters {
        if issued >= ADVANCE_ORDERS_PER_FRAME {
            break;
        }
        // Watercraft are owned by micro's separate naval target selector.
        // A land campaign coordinate cannot be used as an ocean waypoint.
        if unit.slot == super::census::Slot::Ship { continue; }
        // The avoider owns a failed-route withdrawal for this finite interval.
        // The crate detail also owns its travel until the visible prize is taken;
        // neither is an attack squad's next march instruction.
        if brain.avoid_retreat_until.get(&unit.entity_id).is_some_and(|until| tick < *until)
            || brain.crate_claims.contains_key(&unit.entity_id)
            || brain.crate_returns.contains_key(&unit.entity_id)
        { continue; }
        if let Some(since) = brain.armour_wave_wait
            && armour_wave_missing(
                armour.iter().filter(|other| cell_distance(unit.cell, other.cell) <= 12)
                    .count(), tick.saturating_sub(since))
            && armoured_fighter(unit, rules, obs)
            && obs.attack_target(unit.entity_id).is_none()
            && !super::tactics::unsafe_post(rules, obs, unit.entity_id, unit.cell)
        {
            let safe_rally = brain.census.rally.or(brain.census.base_cell)
                .filter(|cell| super::orders::reachable(rules, obs, unit.entity_id, *cell)
                    && !super::tactics::unsafe_post(rules, obs, unit.entity_id, *cell));
            if let Some(cell) = safe_rally
                && cell_distance(unit.cell, cell) > 4
            {
                if super::orders::issue_tracked(&mut brain.tracked, brain.trace,
                    out, rules, obs, unit.entity_id,
                    super::tracked::TrackedKind::Move, None, Some(cell),
                    Goal::Move { cell })
                { issued += 1; }
            } else if (obs.destination(unit.entity_id).is_some()
                || obs.attack_target(unit.entity_id).is_some())
                && out.want(unit.entity_id, 0, Goal::Stop)
            {
                brain.tracked.forget_primary(unit.entity_id);
                issued += 1;
            }
            continue;
        }
        if unit.slot == super::census::Slot::Vehicle
            && let Some(enemy) = brain.census.enemies.iter().find(|enemy| {
                obs.is_cell_visible(enemy.cell.0, enemy.cell.1)
                    && cell_distance(unit.cell, enemy.cell) <= 9
                    && super::tactics::unfavourable_vehicle_duel(
                        rules, obs, unit.entity_id, enemy.entity_id)
                    && !super::tactics::armored_support(&brain.census, rules, obs,
                        unit.entity_id, enemy.entity_id, unit.cell)
            })
        {
            let home = brain.census.rally.or(brain.census.base_cell)
                .filter(|post| super::orders::reachable(rules, obs, unit.entity_id, *post)
                    && !super::tactics::unsafe_post(rules, obs, unit.entity_id, *post));
            if let Some(cell) = home
                && super::orders::issue_tracked(&mut brain.tracked, brain.trace,
                    out, rules, obs, unit.entity_id, super::tracked::TrackedKind::Avoid,
                    Some(enemy.entity_id), Some(cell), Goal::Move { cell })
            {
                brain.avoid_retreat_until.insert(unit.entity_id, tick.saturating_add(225));
                issued += 1;
                continue;
            }
        }
        if support_holds.contains(&unit.entity_id) {
            // The artillery's own Attack or safe rear post is managed by
            // fire_support, even if its native goal needs no new order today.
            continue;
        }
        // A unit already fighting or walking is doing what this would ask. A
        // mission with nothing behind it is not: a unit left on Attack after
        // its target died was skipped here, in `micro` and in the guard step,
        // and stood in the yard for the rest of the match.
        let cohesion = tank_rally.filter(|anchor| needs_tank_cohesion(unit, *anchor))
            .filter(|anchor| super::orders::reachable(rules, obs, unit.entity_id, *anchor));
        let mut march_cell = cohesion.unwrap_or(target_cell);
        if let Some(screen) = infantry_screen_post(&fighters, unit, target_cell, rules, obs)
            && super::orders::reachable(rules,obs,unit.entity_id,screen)
        {
            march_cell = screen;
        }
        // A high-value vehicle must not simply drive into a visible psychic
        // controller's weapon range. Attack it from outside range when it
        // has the reach; otherwise preserve the unit and let infantry or
        // artillery deal with the controller. This is not omniscient: threats
        // beyond current sight never enter the decision.
        if unit.traits.combat && !unit.traits.aircraft
            && let Some((controller, position, reach)) =
                super::tactics::controller_covering_path(
                    rules, obs, unit.cell, march_cell)
        {
            let own_reach = obs.entity_type(rules,unit.entity_id)
                .map_or(0,|ty| weapon_range_cells(rules,ty,obs.is_deployed(unit.entity_id)));
            // A Prism/V3-style weapon must actually fire on the controller;
            // waiting to shoot the remote main base wastes its sole advantage.
            if own_reach > reach + 1
                && obs.can_hurt(rules, unit.entity_id, controller)
                && super::orders::admits(rules,obs,unit.entity_id,
                    &Goal::Attack { target: controller })
            {
                if obs.attack_target(unit.entity_id) != Some(controller)
                    && super::orders::issue_tracked(
                        &mut brain.tracked,brain.trace,out,rules,obs,
                        unit.entity_id,super::tracked::TrackedKind::Attack,
                        Some(controller),Some(position),Goal::Attack { target: controller },
                    ) { issued += 1; }
                continue;
            }
            if super::tactics::controller_covering(rules,obs,unit.cell).is_some() {
                if let Some(safe) = super::tactics::evade_controller(
                    rules,obs,unit.entity_id,unit.cell,position)
                {
                    if super::orders::issue_tracked(
                        &mut brain.tracked,brain.trace,out,rules,obs,
                        unit.entity_id,super::tracked::TrackedKind::Dodge,
                        None,Some(safe),Goal::Move { cell: safe },
                    ) { issued += 1; }
                }
            } else if (obs.destination(unit.entity_id).is_some()
                || obs.attack_target(unit.entity_id).is_some())
                && super::orders::issue_tracked(
                    &mut brain.tracked,brain.trace,out,rules,obs,
                    // Stop the *existing* AttackMove; merely choosing not to
                    // reissue it would let native navigation charge on.
                    unit.entity_id,super::tracked::TrackedKind::Engage,
                    Some(controller),None,Goal::Guard,
                ) { issued += 1; }
            if super::trace_enabled("AUTOPLAY_TRACE_PUSH") && tick % 150 == 0 {
                log::info!("auto play: hold control line unit={} controller={} from={:?} destination={:?}",
                    unit.entity_id, controller, unit.cell, march_cell);
            }
            continue;
        }
        // Different units face different counters. A rocket soldier has no
        // business following a tank into anti-air fire, and an infantryman
        // cannot trade with a sniper or a grand cannon merely because the
        // army's *total* points beat the defending base. Unlike the 600-frame
        // artillery purchase window, this individual protection does not
        // expire into a guaranteed suicidal march.
        let isolated_battery = isolated_battery_approach(
            rules, obs, unit.entity_id, unit.cell, march_cell);
        if let Some((gun, reach)) = isolated_battery.or_else(||
            super::tactics::lethal_approach(
                rules, obs, unit.entity_id, unit.cell, march_cell,
            )).filter(|(gun, reach)| {
            // Individual fragility rules must still protect infantry and
            // aircraft. A healthy armored fist, however, is allowed to trade
            // through a lone bunker when the nearby force is overwhelming.
            let ours = fighters.iter()
                .filter(|f| cell_distance(f.cell, *gun) <= 16)
                .fold(0i32, |sum, f| sum.saturating_add(f.points));
            let theirs = brain.census.enemy_points(rules, obs, *gun, 16);
            let can_hit_gun = obs.visible_enemy_entities().any(|enemy| {
                enemy.cell == *gun && obs.can_hurt(rules, unit.entity_id, enemy.entity_id)
            });
            // Huge fixed batteries cannot be overrun by simply having more
            // hit points: the vehicles may die before the first shot.
            let own_reach = obs.entity_type(rules, unit.entity_id)
                .map_or(0, |ty| weapon_range_cells(rules, ty,
                    obs.is_deployed(unit.entity_id)));
            isolated_battery.is_some()
                || super::tactics::extreme_battery_mismatch(own_reach, *reach)
                || !(can_hit_gun && (super::tactics::overwhelming_armor_commit(
                unit.slot == super::census::Slot::Vehicle,
                unit.health_permille, ours, theirs,
            )))
        }) {
            let safe = super::tactics::siege_post(
                rules, obs, unit.entity_id, unit.cell, gun, reach,
            ).or_else(|| {
                brain.census.rally.or(brain.census.base_cell).filter(|home| {
                    super::orders::reachable(rules,obs,unit.entity_id,*home)
                        && !super::tactics::unsafe_post(rules,obs,unit.entity_id,*home)
                })
            });
            if let Some(post) = safe
                && super::orders::reachable(rules,obs,unit.entity_id,post)
                && cell_distance(unit.cell,post) > 2
            {
                if super::orders::issue_tracked(
                    &mut brain.tracked,brain.trace,out,rules,obs,unit.entity_id,
                    super::tracked::TrackedKind::Avoid,None,Some(post),
                    Goal::Move { cell: post },
                ) { issued += 1; }
            } else if super::orders::issue_tracked(
                &mut brain.tracked,brain.trace,out,rules,obs,unit.entity_id,
                super::tracked::TrackedKind::Engage,None,None,Goal::Guard,
            ) { issued += 1; }
            continue;
        }
        // A Mirage-style ambusher is not the battering ram for an enemy
        // factory. When infantry are actually visible nearby, let it engage
        // them from the current line rather than marching past them toward
        // the campaign's building target. The cannon/psychic danger checks
        // above still take precedence over this local target specialization.
        if obs.entity_type(rules, unit.entity_id)
            .is_some_and(|ty| ty.disguise_when_still)
        {
            let personnel = obs.visible_enemy_entities()
                .filter(|enemy| cell_distance(enemy.cell, unit.cell) <= 12)
                .filter(|enemy| obs.entity_type(rules, enemy.entity_id)
                    .is_some_and(|ty| ty.category ==
                        crate::rules::object_type::ObjectCategory::Infantry))
                .filter(|enemy| obs.can_hurt(rules, unit.entity_id, enemy.entity_id)
                    && super::orders::admits(rules, obs, unit.entity_id,
                        &Goal::Attack { target: enemy.entity_id }))
                .min_by_key(|enemy| (cell_distance(enemy.cell, unit.cell), enemy.entity_id));
            if let Some(enemy) = personnel {
                if obs.attack_target(unit.entity_id) == Some(enemy.entity_id) {
                    continue;
                }
                if super::orders::issue_tracked(
                    &mut brain.tracked, brain.trace, out, rules, obs,
                    unit.entity_id, super::tracked::TrackedKind::Attack,
                    Some(enemy.entity_id), Some(enemy.cell),
                    Goal::Attack { target: enemy.entity_id },
                ) {
                    issued += 1;
                    continue;
                }
                // A denied retarget is not a commitment: let the original
                // march decision run rather than parking the ambusher.
            }
        }
        // Existing fire is worth preserving only while its target is still
        // a *visible enemy*, not an already destroyed or captured structure.
        // Lost sight does not authorize a new entity-id Attack either; the
        // next legal order comes from a fresh observation or last-known cell.
        let attacking = unit.mission == MissionId::from_known(MissionType::Attack)
            && obs.attack_target(unit.entity_id)
                .is_some_and(|aim| live_hostile_ids.contains(&aim));
        // A destination from an old push is not proof that this unit is
        // advancing toward the *current* target. This used to leave entire
        // columns standing outside an enemy base after a target switch.
        let walking = unit.mission == MissionId::from_known(MissionType::Move)
            && match obs.destination(unit.entity_id) {
                Some(destination) => cell_distance(destination, march_cell) <= 4,
                None => !left_behind(unit),
            };
        if attacking || walking {
            continue;
        }
        // A dug-in unit cannot walk, and the push is a walk: the micro layer
        // holds it on area guard where it was deployed, which is where the
        // ground it is holding is.
        if obs.is_deployed(unit.entity_id) {
            continue;
        }
        // The step that owns a specialist order already spent this unit's frame:
        // a spy walking into a building and a rifleman boarding a carrier both
        // outrank the push.
        if out.orders.iter().any(|order| order.actor == unit.entity_id)
            || brain.tracked.pending_resume(tick, unit.entity_id).is_some()
        {
            // A tactical detour can still be moving in native after its
            // short Tracker term ends. Mission Resume owns that hand-off.
            continue;
        }
        // Who owns the unit is the tracker's ladder: a fight in reach, a step
        // out from under a shell, a detour or a breach outranks the march, and
        // `admits` below refuses the order while one is live. A march sent
        // over them is the ping-pong a unit shows at a gate: the shot opens the
        // way, the march clears the shot, and neither finishes.
        // A unit whose weapons have nothing for the target does not march at
        // it. `admits` is `GetFireError` plus the armour floor, the same answer
        // the player's click gets, and the units it refuses are exactly the
        // ones that walk to the wall and die there: a dog sent at a building
        // its jaw cannot touch, a terror drone whose jump has no business with
        // concrete, a rifleman against armour nothing it carries dents.
        //
        // They hold where they stand instead, which is what a player does with
        // them — the fist is the units that can do the job. Marching them along
        // anyway is the "sent at the enemy and wiped out for nothing" a play
        // test reports, and their attack-moves are also what made a squad
        // shuffle around a structure it could never damage.
        // A sniper or tank destroyer is an expensive specialist, not a
        // default demolition team. If the newly selected visible structure
        // is a poor weapon match, keep this unit for the next engagement.
        // The general army can still search without an entity target.
        if !searching && !super::doctrine::target_ok(
            rules,obs,unit.entity_id,target_id) { continue; }
        // Searching: selective weapons walk with the force rather than
        // autonomously attack the first barely-damageable civilian wall.
        if searching {
            let selective = obs.entity_type(rules,unit.entity_id).is_some_and(|ty|
                matches!(super::doctrine::duty(rules,ty),
                    super::doctrine::Duty::TankHunter |
                        super::doctrine::Duty::Marksman |
                        super::doctrine::Duty::Parasite |
                        super::doctrine::Duty::Scout));
            let kind = if selective { super::tracked::TrackedKind::Move }
                else { super::tracked::TrackedKind::AttackMove };
            if !brain
                .tracked
                .admits(tick, unit.entity_id, kind, None, Some(march_cell))
            {
                continue;
            }
            if !super::orders::issue_tracked(
                &mut brain.tracked, brain.trace, out, rules, obs, unit.entity_id,
                kind, None, Some(march_cell),
                if selective { Goal::Move { cell: march_cell } }
                else { Goal::AttackMove { cell: march_cell } },
            ) {
                continue;
            }
            issued += 1;
            continue;
        }
        let attack = Goal::Attack { target: target_id };
        let engagement = obs.engage(rules, unit.entity_id, target_id);
        if engagement == crate::sim::control::observation::Engage::Cannot {
            if !brain.tracked.admits(
                tick,
                unit.entity_id,
                super::tracked::TrackedKind::Guard,
                None,
                None,
            ) {
                continue;
            }
            if super::orders::issue_tracked(
                &mut brain.tracked, brain.trace, out, rules, obs,
                unit.entity_id, super::tracked::TrackedKind::Guard,
                None, None, Goal::Guard,
            ) {
                issued += 1;
            }
            continue;
        }
        // Inside its own reach the unit shoots the structure from where it
        // stands; outside it, it walks. The goal is still the one target — this
        // is the same fist, ordered by what each unit can actually do.
        //
        // `Attack` alone was measured here — it hands the approach to the native
        // hunt, which closes to the unit's *own* range and stops, and it is the
        // shape the wishlist's "卡位" asks for. Across the four arena seeds the
        // exchange went 1.38/2.57/6.29/13.5 -> 1.00/3.00/4.71/13.0, a mean of
        // 5.94 down to 5.43, and the seed that matters most fell to a dead-even
        // trade. So the walk stays an `AttackMove` to the target's cell and the
        // stand-off is left on the table until something explains that drop.
        // A target the gate will refuse is not a target it can shoot: a neutral
        // block is nobody's and `orders::admits` drops every `Goal::Attack` at
        // it (measured on seed 4242: 298 refusals, issued 0-1 a frame, the fist
        // parked in front of it for 7000 ticks). AttackMove is the legal shape of
        // the same order -- it walks at the block and fires at what the walk
        // meets -- so the fist keeps moving instead of standing.
        let shootable = engagement == crate::sim::control::observation::Engage::Ready
            && obs.is_hostile(target_id);

        // Unlike a line tank's ordinary AttackMove, a selective weapon must
        // close on its *chosen* valid target without changing to a random
        // rifleman or wall along the route.
        let selective = obs.entity_type(rules,unit.entity_id).is_some_and(|ty|
            matches!(super::doctrine::duty(rules,ty),
                super::doctrine::Duty::TankHunter |
                    super::doctrine::Duty::Marksman |
                    super::doctrine::Duty::Parasite |
                    super::doctrine::Duty::Scout));
        let (kind, goal) = if shootable || selective {
            (super::tracked::TrackedKind::Attack, attack)
        } else {
            (
                super::tracked::TrackedKind::AttackMove,
                Goal::AttackMove { cell: march_cell },
            )
        };
        // L4: it has been sent at this target, the target has not walked away,
        // and the order has not expired — so it is already doing this.
        if !brain.tracked.admits(
            tick,
            unit.entity_id,
            kind,
            Some(target.entity_id),
            Some(march_cell),
        ) {
            continue;
        }
        // An order the click refuses is not an order: it must not be counted
        // against the frame's eight and must not overwrite a standing primary.
        // `issue_tracked` both arbitrates and remembers the accepted goal.
        if !super::orders::issue_tracked(
            &mut brain.tracked, brain.trace, out, rules, obs, unit.entity_id,
            kind, Some(target.entity_id), Some(march_cell), goal,
        ) {
            refused += 1;
            if super::trace_enabled("AUTOPLAY_TRACE_REFUSE") && tick % 600 == 0 {
                let dx = i32::from(unit.cell.0) - i32::from(target_cell.0);
                let dy = i32::from(unit.cell.1) - i32::from(target_cell.1);
                let reachable = super::orders::reachable(rules, obs, unit.entity_id, target_cell);
                log::info!(
                    "refuse probe: unit={} at {:?} target={} at {:?} d2={} hostile={} engage={} hurt={} reach={}",
                    unit.entity_id,
                    unit.cell,
                    target_id,
                    target_cell,
                    dx * dx + dy * dy,
                    obs.is_hostile(target_id),
                    obs.can_engage(rules, unit.entity_id, target_id),
                    obs.can_hurt(rules, unit.entity_id, target_id),
                    reachable,
                );
            }
            continue;
        }
        issued += 1;
    }
    if super::trace_enabled("AUTOPLAY_TRACE_REFUSE") && tick % 600 == 0 {
        log::info!(
            "budget probe: tick={} issued={} refused={}",
            tick,
            issued,
            refused
        );
    }
    true
}

/// Conserve an early, poorly replenished army while factories and defensive
/// infrastructure recover. The deadline prevents a dry map from causing an
/// indefinite no-attack stalemate.
fn tick_limited_scarce_opening(
    tick: u64, cash: i32, income_per_minute: Option<i32>,
    known_ore_cells: usize, attack_members: usize, raided: bool,
) -> bool {
    !raided && (1_200..5_400).contains(&tick)
        && known_ore_cells < 12
        && income_per_minute.is_some_and(|income| income <= 450)
        && cash < 3_200 && attack_members < 15
}

#[cfg(all(test, tests_sim_p_s))]
mod resource_opening_tests {
    use super::tick_limited_scarce_opening;

    #[test]
    fn scarce_income_conserves_opening_army_but_never_blocks_forever() {
        assert!(tick_limited_scarce_opening(2_000, 900, Some(0), 2, 10, false));
        assert!(!tick_limited_scarce_opening(6_000, 900, Some(0), 2, 10, false));
        assert!(!tick_limited_scarce_opening(2_000, 900, Some(0), 2, 10, true));
        assert!(!tick_limited_scarce_opening(2_000, 900, Some(5000), 2, 10, false));
        assert!(!tick_limited_scarce_opening(2_000, 900, Some(0), 30, 10, false));
    }
}

/// The reach of the weapon the object would actually fire, in cells, rounded
/// up; zero when it has none.
///
/// A dug-in soldier fires a different weapon from the one it stands up with.
/// `[E1] DeployFire=yes` and `DeployFireWeapon=` — slot 1, the secondary, by
/// default (`InfantryTypeClass::ReadINI` `0x0052466E..0x0052469A` reads the
/// key; `combat_weapon.rs` picks the slot from it) — and `[E1] Secondary=Para`
/// reaches 5 cells where `Primary=M60` reaches 4. A
/// brain that measures the standing weapon sends a deployed soldier at a target
/// its gun cannot cover, and a soldier that cannot walk just stands there.
pub(super) fn weapon_range_cells(rules: &RuleSet, ty: &ObjectType, deployed: bool) -> i32 {
    weapon_of(rules,ty,deployed).map_or(0, weapon_reach_cells)
}

/// The true maximum range of a *particular* weapon. `MinimumRange` is
/// the inner radius of a legal firing annulus; subtracting it from `Range`
/// incorrectly shortened a V3 from 18 to 13 and a Grand Cannon from 15
/// to 12 in the planner. Minimum firing range must be checked separately
/// when placing a gun, not subtracted from its maximum.
pub(super) fn weapon_reach_cells(weapon: &crate::rules::weapon_type::WeaponType) -> i32 {
    super::siege_counter::max_range_cells(weapon)
}

/// How many shots the type's weapon fires in one attack cycle: `Burst=` from
/// `[WeaponTypes]`, read where the simulation reads it
/// (`WeaponTypeClass ReadINI`, default 1).
///
/// One is not a burst, and a weapon that fires one shot a cycle has nothing
/// for a broken cycle to gain.
pub(super) fn weapon_burst(rules: &RuleSet, ty: &ObjectType, deployed: bool) -> i32 {
    weapon_of(rules, ty, deployed).map_or(1, |weapon| weapon.burst.max(1))
}

/// The weapon `ty` fires, in the slot it would fire it from.
fn weapon_of<'r>(
    rules: &'r RuleSet,
    ty: &ObjectType,
    deployed: bool,
) -> Option<&'r crate::rules::weapon_type::WeaponType> {
    let slot = if deployed {
        super::census::Traits::of(rules, ty).deploy_slot
    } else {
        None
    };
    // The slot the type would fire, or else the first slot it has one in: a
    // type whose armed `Secondary=` and empty `Primary=` is army in the ledger,
    // and army with no reach would never be sent at anything.
    let name = slot
        .and_then(|slot| ty.weapon_at(slot))
        .or_else(|| (0..8).find_map(|slot| ty.weapon_at(slot)));
    rules.weapon(name?)
}

/// Chebyshev distance in cells, the metric the game's own adjacency uses.
pub(super) fn cell_distance(a: (u16, u16), b: (u16, u16)) -> i32 {
    let dx = i32::from(a.0).abs_diff(i32::from(b.0));
    let dy = i32::from(a.1).abs_diff(i32::from(b.1));
    i32::try_from(dx.max(dy)).unwrap_or(i32::MAX)
}

/// Whether an enemy is close enough to what this house still holds that the
/// army belongs at home.
///
/// Measured from the yard when one stands, and otherwise from whatever
/// building the house still has: a house whose yard is gone but whose factory
/// still stands has plenty left to lose, and an army that walks out while the
/// enemy is standing on the factory loses that too.
///
/// A house with nothing standing is the other way round. It used to count as
/// under threat and held the army home for the rest of the match, which is
/// the residual the module header records: once the yard is gone the army is
/// the only thing the house has, and an army guarding rubble loses slowly
/// instead of taking the base that would let it rebuild. Nothing standing
/// means nothing to defend, so the push runs.
/// The cell of the enemy standing closest to the house's base, when one is
/// inside the threat ring.
///
/// [`base_under_threat`] answers whether the army belongs home; this answers
/// *where*, which is what a guard on the far side of a growing base needs.
pub(super) fn threatened_cell(census: &Census) -> Option<(u16, u16)> {
    let base = census.base_cell?;
    census
        .enemies
        .iter()
        .filter(|enemy| {
            let dx = i32::from(enemy.cell.0) - i32::from(base.0);
            let dy = i32::from(enemy.cell.1) - i32::from(base.1);
            dx * dx + dy * dy <= THREAT_RADIUS * THREAT_RADIUS
        })
        .min_by_key(|enemy| {
            let dx = i32::from(enemy.cell.0) - i32::from(base.0);
            let dy = i32::from(enemy.cell.1) - i32::from(base.1);
            (dx * dx + dy * dy, enemy.cell)
        })
        .map(|enemy| enemy.cell)
}

#[cfg(all(test, tests_sim_p_s))]
fn base_under_threat(census: &Census) -> bool {
    let Some(base) = census.base_cell else {
        return false;
    };
    census.enemies.iter().any(|enemy| {
        let dx = i32::from(enemy.cell.0) - i32::from(base.0);
        let dy = i32::from(enemy.cell.1) - i32::from(base.1);
        dx * dx + dy * dy <= THREAT_RADIUS * THREAT_RADIUS
    })
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;
    use super::targets::{neutral_blocker, pick_reachable};
    use crate::rules::unit_role::RoleTable;
    use crate::sim::world::Simulation;

    /// A small army still goes at an enemy that has nothing left to fight with.
    ///
    /// The wishlist's stalemate: "最后消灭了大部分敌人，但Autoplay也不发起进攻，然后
    /// 就僵持在这了。发动攻击后总会剩下敌人的建筑不再打了". `ARMY_TO_ATTACK` is an
    /// opening number and had no bypass, so nine fighters against two buildings
    // read `too-small` for the rest of the match.
    #[test]
    fn isolated_oldest_actor_does_not_choose_the_main_front() {
        assert_eq!(central_actor(&[(1,1),(30,30),(31,31),(32,32)],(0,0)),(31,31));
        assert_eq!(central_actor(&[(27,27),(30,30),(31,31)],(0,0)),(27,27));
        assert_eq!(central_actor(&[],(3,4)),(3,4));
    }

    #[test]
    fn armour_assembles_three_tanks_but_never_holds_a_dead_end_forever() {
        assert!(armour_wave_missing(1, 0));
        assert!(armour_wave_missing(2, 400));
        assert!(!armour_wave_missing(3, 20));
        assert!(!armour_wave_missing(1, ARMOUR_ASSEMBLE_FRAMES));
    }

    #[test]
    fn a_household_army_finishes_an_enemy_with_nothing_left() {
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()
            .expect("the repository's own rules");
        let mut sim = crate::sim::world::Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        let mut census = Census::default();
        census.refresh(
            &crate::rules::unit_role::RoleTable::build(&rules),
            &rules,
            &Observation::new(&sim, owner),
        );
        // Two enemy objects left standing at the base: a clean-up job, not a
        // fight. The read is the ring, so the two are placed in it.
        let home = (20, 20);
        census.base_cell = Some(home);
        census.enemies = vec![
            crate::sim::control::observation::EnemyEntity {
                entity_id: 1,
                type_id: crate::sim::intern::InternedId::default(),
                cell: (home.0 + 1, home.1 + 1),
            },
            crate::sim::control::observation::EnemyEntity {
                entity_id: 2,
                type_id: crate::sim::intern::InternedId::default(),
                cell: (home.0 + 2, home.1 + 2),
            },
        ];
        assert!(mop_up(&census), "two objects left is a clean-up job");
        // And a field army standing on the base is not, whatever the fist's
        // size.
        census.enemies = (0..30)
            .map(|index| {
                let offset = i16::try_from(index).unwrap_or(0);
                crate::sim::control::observation::EnemyEntity {
                    entity_id: 100 + u64::try_from(index).unwrap_or(0),
                    type_id: crate::sim::intern::InternedId::default(),
                    cell: (
                        home.0.wrapping_add(offset.unsigned_abs()),
                        home.1.wrapping_add(offset.unsigned_abs()),
                    ),
                }
            })
            .collect();
        assert!(!mop_up(&census), "thirty objects is a fight");
        let _ = &rules;
    }

    /// A push that has taken nothing for a minute stops waiting for a
    /// favourable comparison. The gate is a group reading on purpose: no
    /// per-unit pass can see the army's progress, and a comparison that
    /// never opens is the army standing in front of a base until the clock
    /// runs out.
    #[test]
    fn a_push_that_takes_nothing_for_a_minute_goes_anyway() {
        assert!(
            !push_stalled((0, usize::MAX), 5_000, 30),
            "an opening army is small, not stalled"
        );
        assert!(
            !push_stalled((100, 30), 900, 30),
            "under a minute is still a wait"
        );
        assert!(
            push_stalled((100, 30), 1_000, 30),
            "a minute with nothing taken"
        );
        assert!(
            !push_stalled((100, 30), 1_000, 25),
            "the enemy lost objects since the reading"
        );
    }

    /// A dug-in soldier reaches further than the same soldier standing up:
    /// `[E1] Primary=M60 Range=4` against `Secondary=Para Range=5`, and
    /// `DeployFire=yes` with `DeployFireWeapon=` defaulting to slot 1 is what
    /// decides which one it fires. Measuring the standing weapon sent deployed
    /// soldiers at targets their own gun could not cover, and a soldier that
    /// cannot walk just stands there with the order marker on it.
    #[test]
    fn a_dug_in_soldier_reaches_as_far_as_the_weapon_it_fires() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        let obs = Observation::new(&sim, owner);
        let gi = obs
            .type_id("E1")
            .and_then(|type_id| obs.object_type(&rules, type_id))
            .expect("the G.I. is in the rules");
        let standing = weapon_range_cells(&rules, gi, false);
        let dug_in = weapon_range_cells(&rules, gi, true);
        assert!(standing > 0, "the standing soldier reaches: {standing}");
        assert!(
            dug_in > standing,
            "the deployed one reaches further: {standing} vs {dug_in}"
        );
    }

    /// The army's one target is a structure, and every combat unit used to be
    /// handed an Attack on it regardless of whether its weapon has an answer.
    /// `GetFireError` is the only thing that knows: an ore carrier has no
    /// weapon at all, and an order it is refused every frame is the frame's
    /// command budget spent on nothing — eight refusals is eight orders the
    /// house never gave.
    #[test]
    fn a_unit_whose_weapon_cannot_answer_the_target_is_not_told_to_attack_it() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        let tank = sim
            .spawn_object("MTNK", "Americans", 12, 10, 255, &rules)
            .expect("the grizzly tank");
        let carrier = sim
            .spawn_object("CMIN", "Americans", 10, 10, 255, &rules)
            .expect("the ore carrier");
        let building = sim
            .spawn_object("GAPOWR", "Russians", 11, 12, 255, &rules)
            .expect("the enemy power plant");
        let obs = Observation::new(&sim, owner);
        assert!(
            obs.can_engage(&rules, tank, building),
            "the tank has a weapon for a building"
        );
        assert!(
            !obs.can_engage(&rules, carrier, building),
            "the carrier has no weapon at all"
        );
    }

    /// A retail arena: two houses on 32x32 ground, needed to spawn real object
    /// types. The rules come from the repository's own `ini/` snapshot, so
    /// these run without a retail install.
    /// A gate that turned the army back holds for its window, so one frame of
    /// bad arithmetic does not re-task the whole army.
    ///
    /// Measured on Death.mmx seed 24301 before the window: `push` for ten
    /// samples, `outgunned` for eight, `push` again, `outgunned` again. Every
    /// flip re-tasks the fist, which from the player's side is an army walking
    /// out and back rather than fighting.
    #[test]
    fn a_gate_that_turned_the_army_back_holds_for_its_window() {
        let mut brain = AssistBrain::new(0xA17);
        brain.push_rest = (100, "outgunned");
        assert!(resting(&brain, 101), "a gate one frame old still holds");
        assert!(
            resting(&brain, 100 + PUSH_REST_FRAMES - 1),
            "and holds to the last frame of its window"
        );
        assert!(
            !resting(&brain, 100 + PUSH_REST_FRAMES),
            "and is done at the window's edge"
        );
        // A house that never rested pushes: the opening is not held back by a
        // window that was never opened.
        let fresh = AssistBrain::new(0xA17);
        assert!(!resting(&fresh, 0), "a house that never rested pushes");
    }

    fn arena(seed: u64) -> Option<(Simulation, RuleSet)> {
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()?;
        let mut sim = crate::sim::arena_fixture::rival_arena_known(&rules, seed);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        Some((sim, rules))
    }

    fn ledger(rules: &RuleSet, sim: &Simulation, owner: crate::sim::intern::InternedId) -> Census {
        let mut census = Census::default();
        census.refresh(
            &RoleTable::build(rules),
            rules,
            &Observation::new(sim, owner),
        );
        census
    }

    /// The whole reason the rule was narrowed: a house whose yard is gone and
    /// whose base is rubble used to read as under threat for the rest of the
    /// match, and an army guarding rubble loses slowly instead of taking the
    /// base that would let the house rebuild.
    #[test]
    fn a_house_with_nothing_left_to_defend_is_not_under_threat() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        assert!(
            !base_under_threat(&ledger(&rules, &sim, owner)),
            "nothing standing means nothing to defend"
        );
    }

    /// The same rule with something still standing, measured from the yard: an
    /// enemy inside the ring holds the army at home.
    #[test]
    fn an_enemy_at_the_yard_still_holds_the_army_home() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 16, 16, 255, &rules)
            .expect("an Allied Construction Yard");
        assert!(
            !base_under_threat(&ledger(&rules, &sim, owner)),
            "an empty field around the yard is no threat"
        );

        sim.spawn_object("MTNK", "Russians", 20, 16, 255, &rules)
            .expect("a Soviet tank");
        assert!(
            base_under_threat(&ledger(&rules, &sim, owner)),
            "four cells from the yard is inside {THREAT_RADIUS}"
        );
    }

    /// A neutral refinery is worth taking whole. It must not hijack the whole
    /// attack group simply because it lies on the straight route.
    #[test]
    fn a_neutral_economic_structure_is_not_a_demolition_target() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        register_neutral_house(&mut sim, "Neutral");
        let blocker = sim
            .spawn_object("NAREFN", "Neutral", 20, 20, 255, &rules)
            .expect("a civilian refinery in the way");
        let mut census = ledger(&rules, &sim, owner);
        assert_eq!(
            census.neutrals.len(),
            1,
            "the ledger keeps what is nobody's"
        );
        census.refresh(
            &RoleTable::build(&rules),
            &rules,
            &Observation::new(&sim, owner),
        );
        assert_eq!(
            census.neutrals.len(),
            1,
            "a refresh does not accumulate them"
        );
        assert_eq!(
            neutral_blocker(
                &census,
                &Observation::new(&sim, owner),
                &rules,
                (18, 20),
                (30, 20)
            ),
            None,
            "a neutral refinery is an asset to capture, never a push target"
        );
        let _ = blocker;
        assert_eq!(
            neutral_blocker(
                &census,
                &Observation::new(&sim, owner),
                &rules,
                (10, 10),
                (10, 25)
            ),
            None,
            "a building off the line is not in the way"
        );
    }

    /// A neutral bunker is a fight, not a door: an enemy can stand inside it
    /// and shoot out, and the shooter is not the building, so nothing else in
    /// the ledger sees the threat.
    ///
    /// Measured on Arena.mmx seed 0x5EED at 24000 ticks: 13 of the local
    /// house's 37 losses were to `CABUNK01` (`CanBeOccupied=yes`,
    /// `MaxNumberOccupants=8`, `CanOccupyFire=yes`), including all five of its
    /// machine-gun towers. The army walked to the bunker to demolish it, spent
    /// the engagement on a structure worth nothing, and never arrived.
    #[test]
    fn an_occupiable_neutral_bunker_is_not_a_door_to_demolish() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        register_neutral_house(&mut sim, "Neutral");
        let bunker = sim
            .spawn_object("CABUNK01", "Neutral", 20, 20, 255, &rules)
            .expect("a neutral bunker in the way");
        let census = ledger(&rules, &sim, owner);
        assert_eq!(census.neutrals.len(), 1, "the ledger still records it");
        let ty = rules.object("CABUNK01").expect("the retail bunker");
        assert!(
            ty.can_be_occupied && ty.can_occupy_fire && ty.max_number_occupants > 0,
            "the fixture must be the kind of structure this test is about"
        );
        assert_eq!(
            neutral_blocker(
                &census,
                &Observation::new(&sim, owner),
                &rules,
                (10, 20),
                (30, 20)
            ),
            None,
            "the army walks around it instead of spending itself on it"
        );
        // A plain neutral wall is still a door, so this did not turn the whole
        // demolition path off.
        let _ = bunker;
        assert!(
            neutral_blocker(
                &census,
                &Observation::new(&sim, owner),
                &rules,
                (10, 20),
                (30, 20)
            )
            .is_none()
        );
    }

    /// An enemy building is not a neutral one: it is already in the enemy
    /// ledger and [`super::blocking_structure`] is what picks it.
    #[test]
    fn an_enemy_structure_is_not_a_neutral_blocker() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("NAREFN", "Russians", 20, 20, 255, &rules)
            .expect("an enemy refinery in the way");
        let census = ledger(&rules, &sim, owner);
        assert!(census.neutrals.is_empty(), "an enemy owns its building");
        assert_eq!(
            neutral_blocker(
                &census,
                &Observation::new(&sim, owner),
                &rules,
                (10, 20),
                (30, 20)
            ),
            None
        );
    }

    fn register_neutral_house(sim: &mut crate::sim::world::Simulation, name: &str) {
        let id = sim.interner.intern(name);
        let mut house = crate::sim::house_state::HouseState::new(id, 0, None, false, 0, 10);
        house.set_multiplay_passive_for_test(true);
        sim.houses.register(house);
    }

    /// A unit inside its own weapon's reach of the push's target attacks it
    /// from where it stands instead of walking into it: the same fist, ordered
    /// by what each unit can actually do.
    #[test]
    fn a_unit_in_reach_attacks_the_fist_target_from_where_it_stands() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 8, 8, 255, &rules)
            .expect("a yard");
        sim.spawn_object("GAPILE", "Americans", 14, 8, 255, &rules)
            .expect("a barracks");
        sim.spawn_object("GAWEAP", "Americans", 18, 8, 255, &rules)
            .expect("a war factory");
        sim.spawn_object("GAPILL", "Americans", 4, 8, 255, &rules)
            .expect("a pillbox so the base counts as defended");
        // The fist, the first rank standing beside the refinery: "in reach" is
        // the engine's answer (`GetFireError` with the range test on), which
        // measures to the building's own cells and not to the cell its click
        // names.
        let mut fighters = Vec::new();
        for cell in [(26u16, 26u16), (27, 26), (27, 27), (26, 27), (25, 26)] {
            fighters.push(
                sim.spawn_object("MTNK", "Americans", cell.0, cell.1, 255, &rules)
                    .expect("a Grizzly"),
            );
        }
        for cell in [(24u16, 26u16), (24, 27), (24, 28), (25, 27), (25, 28)] {
            fighters.push(
                sim.spawn_object("MTNK", "Americans", cell.0, cell.1, 255, &rules)
                    .expect("a Grizzly"),
            );
        }
        sim.spawn_object("NAREFN", "Russians", 28, 26, 255, &rules)
            .expect("an enemy refinery");

        let mut brain = AssistBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let mut out = Outbox::default();
        assert!(
            advance(&mut brain, &rules, &obs, &mut out),
            "ten fighters and a defended base is a push"
        );
        assert!(
            out.orders.iter().any(
                |order| order.actor == fighters[0] && matches!(order.goal, Goal::Attack { .. })
            ),
            "the nearest fighter is inside its reach and shoots: {:?}",
            out.orders
        );
    }

    /// A field army with no building left is the army's job whatever its size.
    ///
    /// The count and the stall were the only two ways the mop-up opened, so an
    /// enemy holding nine units and nothing else was above the count with no
    /// structure to besiege: the fist marched to the middle of the map and
    /// stood there. Measured on Death.mmx seed 10, which ended with ten enemy
    /// objects alive and the clock out.
    #[test]
    fn a_field_army_with_no_structure_left_is_targeted_whatever_its_size() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 5, 5, 255, &rules)
            .expect("a yard");
        sim.spawn_object("GAPILE", "Americans", 9, 5, 255, &rules)
            .expect("a barracks");
        sim.spawn_object("GAWEAP", "Americans", 5, 9, 255, &rules)
            .expect("a war factory");
        sim.spawn_object("GAPILL", "Americans", 3, 3, 255, &rules)
            .expect("a pillbox so the base counts as defended");
        let mut fighters = Vec::new();
        for index in 0..10u16 {
            fighters.push(
                sim.spawn_object(
                    "MTNK",
                    "Americans",
                    12 + index / 4,
                    12 + index % 4,
                    255,
                    &rules,
                )
                .expect("a Grizzly"),
            );
        }
        // Nine of them: one above MOP_UP_OBJECTS, and no structure at all.
        let mut enemies = Vec::new();
        for cell in [
            (25u16, 25u16),
            (26, 25),
            (27, 25),
            (25, 26),
            (26, 26),
            (27, 26),
            (25, 27),
            (26, 27),
            (27, 27),
        ] {
            enemies.push(
                sim.spawn_object("HTNK", "Russians", cell.0, cell.1, 255, &rules)
                    .expect("a Russian heavy tank"),
            );
        }

        let mut brain = AssistBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        assert_eq!(brain.census.enemies.len(), 9, "nine, not a handful");
        let obs = Observation::new(&sim, owner);
        let mut out = Outbox::default();
        assert!(
            advance(&mut brain, &rules, &obs, &mut out),
            "ten fighters push"
        );
        // The map's middle is (16,16), which is not in that block: an army
        // walking anywhere else is the old behaviour, not this one.
        assert!(
            out.orders.iter().any(|order| {
                matches!(order.goal, Goal::AttackMove { cell } if (25..=27).contains(&cell.0) && (25..=27).contains(&cell.1))
            }),
            "the fist walks at the tanks it can see: {:?}",
            out.orders
        );
        let _ = (fighters, enemies);
    }

    /// The fist aims at something it can walk to, not merely at something near.
    ///
    /// `orders::issue` refuses a walk the zone map cannot take, and it refuses
    /// it for every unit in turn: measured on Death.mmx seed 24301, 187
    /// refusals of one `AttackMove` and an army idle at the bell while an enemy
    /// object stood across water.
    #[test]
    fn a_target_nobody_can_walk_to_is_passed_over_for_one_somebody_can() {
        let target = |entity_id: u64, cell: (u16, u16)| EnemyEntity {
            entity_id,
            type_id: crate::sim::intern::InternedId::from_index(0),
            cell,
        };
        let nearest_first = [
            target(1, (10, 10)),
            target(2, (12, 10)),
            target(3, (20, 10)),
        ];
        // The near two are across the water; only the far one is walkable.
        let chosen =
            pick_reachable(&nearest_first, |cell| cell.0 == 20).expect("the far one is a choice");
        assert_eq!(chosen.entity_id, 3, "the walkable one, not the nearest");
        // And when the map cuts the fist off from all of them, the nearest is
        // still the answer: holding the army is the caller's job, not this
        // one's.
        let chosen = pick_reachable(&nearest_first, |_| false).expect("the nearest");
        assert_eq!(chosen.entity_id, 1);
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v16_artillery_tests {
    use super::*;

    #[test]
    fn a_real_standoff_gun_should_breach_while_short_guns_wait() {
        assert!(standoff_can_breach(12, 8, true));
        assert!(!standoff_can_breach(8, 8, true));
        assert!(!standoff_can_breach(12, 8, false));
        assert!(!standoff_can_breach(4, 2, true));
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v49_assault_retry_tests {
    use super::*;
    #[test]
    fn failed_wave_requires_cooldown_or_real_reinforcement() {
        let retry = Some(ChargeRetry {target:7,until:900,hp:2400,failures:1,target_hp:600,forget_at:7200});
        assert!(!retry_allowed(retry,7,899,2400,600));
        assert!(retry_allowed(retry,7,899,3000,600));
        assert!(retry_allowed(retry,7,900,2000,600));
        assert!(retry_allowed(retry,8,1,1000,600));
        let repeated=Some(ChargeRetry {failures:3,until:2700,..retry.unwrap()});
        assert!(!retry_allowed(repeated,7,900,3000,600));
        assert!(retry_allowed(repeated,7,900,4200,600));
        assert!(retry_allowed(repeated,7,900,2400,450));
        assert_eq!(retry_delay(8),3600);
    }
    #[test]
    fn a_smaller_failed_wave_cannot_lower_the_reinforcement_reference() {
        let old=Some(ChargeRetry {target:7,until:900,hp:3000,failures:1,target_hp:400,forget_at:7200});
        assert_eq!(retry_reference(old,true,1800,600),(3000,400));
        assert_eq!(retry_reference(old,true,4200,300),(4200,300));
        assert_eq!(retry_reference(old,false,4200,300),(3000,400));
    }
    #[test]
    fn detached_owned_unit_is_not_a_casualty_and_damage_is_not_cash_spent() {
        let original=BTreeMap::from([(1,(300,900)),(2,(300,900)),(3,(300,900))]);
        let owned=BTreeMap::from([(1,300),(2,150)]);
        assert_eq!(cohort_feedback(&original,&owned),(450,1,900,450));
    }
}
