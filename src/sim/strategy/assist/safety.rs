//! Safety belongs to an actor, not to the task that happened to discover danger.
//!
//! This is the last L4 admission check, after the existing per-frame rank
//! arbiter. It uses only currently observed threats and native missions. A
//! rejected task must not be replayed by the mission-resume layer. It does not
//! grant movement, repair, or damage; all resulting goals still go through L2.
use std::collections::{BTreeMap, BTreeSet};

use crate::rules::locomotor_type::MovementZone;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::{Goal, brain::Outbox, intent::Order};
use crate::sim::control::observation::{EnemyEntity, Engage, Observation};
use crate::sim::mission::MissionType;
use crate::sim::command::Command;

use super::{AssistBrain, census::{Slot, Unit}, doctrine, orders, squads::Role,
    tactics, threats::distance, tracked::TrackedKind};

type Cell = (u16, u16);
const CAUTION_FRAMES: u64 = 450;
const RETRY_FRAMES: u64 = 30;
const ABORT_FRAMES: u64 = 90;

#[derive(Debug, Default)]
pub(super) struct Director {
    wounded: BTreeSet<u64>,
    caution_until: BTreeMap<u64, u64>,
    // A frame-local recovery decision. None means preserve the safe native
    // movement/service/position, not that another task may acquire this unit.
    plans: BTreeMap<u64, Option<Goal>>,
    requested: BTreeMap<u64, (Goal, u64)>,
    repair_visits: BTreeMap<u64,(u64,Cell,u64)>,
    repair_retry_after: BTreeMap<u64,u64>,
    rear_posts: BTreeMap<u64,(Cell,u64)>,
    field_progress: BTreeMap<u64,(u16,u64)>,
    field_retry_after: BTreeMap<u64,u64>,
    orphan_progress: BTreeMap<u64,(Cell,Option<Cell>,u64)>,
    return_jobs: BTreeMap<u64,(Cell,Cell,u64)>,
    return_retry_after: BTreeMap<u64,u64>,
    abandoned: BTreeSet<u64>,
    blocked_at: BTreeMap<u64, u64>,
    announced: bool,
}

impl Director {
    /// A persistent casualty is not a reinforcement merely because the
    /// old retreat lease expired. Predict hard-threshold entries before
    /// the recovery pass; the actual pass owns the injury lifecycle.
    pub(super) fn reserved(&self, rules: &RuleSet, obs: &Observation<'_>, unit: &Unit) -> bool {
        if self.wounded.contains(&unit.entity_id) || self.return_jobs.contains_key(&unit.entity_id)
            || self.abandoned.contains(&unit.entity_id) { return true; }
        (unit.health_permille <= ranked_entry_floor(unit.traits.irreplaceable,obs.own_elite(unit.entity_id))
            || obs.has_parasite(unit.entity_id))
            && ground_recovery_unit(rules, obs, unit)
    }

    pub(super) fn field_retry(&self,actor:u64,now:u64)->bool {
        self.field_retry_after.get(&actor).is_some_and(|end|now<*end)
    }

    pub(super) fn abandon(&mut self,actor:u64) {self.abandoned.insert(actor);}

    pub(super) fn mark_detour(&mut self, actor: u64, tick: u64) {
        self.caution_until.insert(actor,tick.saturating_add(CAUTION_FRAMES));
    }

    pub(super) fn forget(&mut self, actor: u64) {
        self.wounded.remove(&actor);
        self.caution_until.remove(&actor);
        self.plans.remove(&actor);
        self.requested.remove(&actor);
        self.repair_visits.remove(&actor);
        self.repair_retry_after.remove(&actor);
        self.rear_posts.remove(&actor);
        self.field_progress.remove(&actor);
        self.field_retry_after.remove(&actor);
        self.orphan_progress.remove(&actor);
        self.return_jobs.remove(&actor);
        self.return_retry_after.remove(&actor);
        self.abandoned.remove(&actor);
        self.blocked_at.remove(&actor);
    }
}

/// Refresh a short actor lease on every review. The previous review's lease
/// survives regrouping, so promoting a raider does not erase its caution.
pub(super) fn remember_roles(brain: &mut AssistBrain, tick: u64) {
    let live: BTreeSet<_> = brain.census.units.iter()
        .filter(|u| !u.hand && !u.traits.spawned && matches!(u.slot, Slot::Infantry | Slot::Vehicle))
        .map(|u| u.entity_id).collect();
    brain.safety.wounded.retain(|id| live.contains(id));
    brain.safety.requested.retain(|id, _| live.contains(id));
    brain.safety.blocked_at.retain(|id, _| live.contains(id));
    brain.safety.caution_until.retain(|id, until| live.contains(id) && tick < *until);
    let raiders = brain.squads.all().iter()
        .filter(|s| matches!(s.role, Role::Harass | Role::Scout))
        .flat_map(|s| s.members.iter().copied());
    for actor in raiders.chain(brain.census.units.iter().filter(|u|
        u.traits.scout || brain.avoid_retreat_until.get(&u.entity_id).is_some_and(|end| tick < *end)
            || brain.crate_claims.contains_key(&u.entity_id)
            || brain.crate_returns.contains_key(&u.entity_id))
        .map(|u| u.entity_id)) {
        if live.contains(&actor) {
            brain.safety.caution_until.insert(actor, tick.saturating_add(CAUTION_FRAMES));
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Envelope { pub(super) at: Cell, pub(super) reach: i32 }

pub(super) fn envelopes(rules: &RuleSet, obs: &Observation<'_>, actor: u64,
    visible: &[EnemyEntity], all_weapons: bool) -> Vec<Envelope> {
    let Some(victim) = obs.entity_type(rules, actor) else { return Vec::new(); };
    visible.iter().filter_map(|enemy| {
        let ty = obs.object_type(rules, enemy.type_id)?;
        let control = if victim.immune_to_psionics {
            0
        } else { tactics::actor_control_reach(rules,obs,enemy.entity_id).unwrap_or(0) };
        if !all_weapons && ty.category != ObjectCategory::Building && control <= 0 {
            return None;
        }
        if ty.category == ObjectCategory::Building
            && !obs.visible_enemy_building_operational(rules, enemy.entity_id) { return None; }
        let reach = tactics::effective_reach_against(rules, obs, enemy.entity_id, actor)
            .max(control);
        // Preserve the avoidance pass's one-cell-outside corners for healthy
        // raiders. Fragile specialists/casualties use an extra cell.
        let margin=if all_weapons && victim.engineer && ty.category!=ObjectCategory::Building {3}
            else if all_weapons {1} else {0};
        (reach > 0).then_some(Envelope { at: enemy.cell, reach: reach.saturating_add(margin) })
    }).collect()
}

fn covered(guns: &[Envelope], cell: Cell) -> bool {
    guns.iter().any(|g| distance(g.at, cell) <= g.reach)
}

/// A segment may leave a box it already starts in, but it must move outward
/// along a dominant axis. It must not cross its center or enter a second box.
pub(super) fn safe_leg(guns: &[Envelope], from: Cell, to: Cell, escape: bool) -> bool {
    guns.iter().all(|g| {
        if !tactics::path_enters_control_box(from, to, g.at, g.reach) { return true; }
        let x = i32::from(from.0) - i32::from(g.at.0);
        let y = i32::from(from.1) - i32::from(g.at.1);
        let dx = i32::from(to.0) - i32::from(from.0);
        let dy = i32::from(to.1) - i32::from(from.1);
        let outward = (x == 0 && y == 0 && (dx != 0 || dy != 0))
            || (x.abs() >= y.abs() && x * dx > 0)
            || (y.abs() >= x.abs() && y * dy > 0);
        escape && distance(g.at, from) <= g.reach && outward
            && distance(g.at, to) > distance(g.at, from)
    })
}

/// Shared admission for new engineer missions, including the endpoint itself.
/// Ordinary escorts do not make a lethal tower envelope safe for an engineer.
pub(super) fn specialist_approach_safe(rules: &RuleSet, obs: &Observation<'_>,
    actor: u64, target: u64) -> bool {
    let Some(from) = obs.observed_object_cell(actor) else { return false; };
    let Some(to) = obs.observed_object_cell(target) else { return false; };
    let Some(ty)=obs.entity_type(rules,actor) else {return false;};
    if obs.is_campaign() {
        if !orders::campaign_building_approachable(rules,obs,ty.movement_zone,from,target) {return false;}
    } else if !orders::building_approachable(obs,ty.movement_zone,from,to) {return false;}
    let visible: Vec<_> = obs.visible_enemy_entities().collect();
    safe_leg(&envelopes(rules, obs, actor, &visible, true), from, to, false)
        && super::site_risk::capture_direct_safe(rules,obs,target,to,&visible)
}

/// A rifleman is exposed until it actually enters the building. The
/// building's firing ports do not make a dangerous approach safe.
pub(super) fn garrison_approach_safe(rules:&RuleSet,obs:&Observation<'_>,actor:u64,target:u64)->bool {
    let Some(from)=obs.observed_object_cell(actor) else {return false;};
    let Some(to)=obs.observed_object_cell(target) else {return false;};
    if !obs.can_garrison(rules,actor,target) {return false;}
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    safe_leg(&envelopes(rules,obs,actor,&visible,true),from,to,false)
}

fn ground_actor(rules: &RuleSet, obs: &Observation<'_>, unit: &Unit) -> bool {
    matches!(unit.slot, Slot::Infantry | Slot::Vehicle)
        && obs.object_type(rules, unit.type_id).is_some_and(|ty|
            ty.movement_zone != MovementZone::Fly && !ty.balloon_hover && !ty.jumpjet)
}

pub(super) fn ground_recovery_unit(rules: &RuleSet, obs: &Observation<'_>, unit: &Unit) -> bool {
    ground_actor(rules, obs, unit) && unit.traits.combat
        && doctrine::actor_duty(rules,obs,unit.entity_id).is_some_and(|duty| !matches!(
            duty, doctrine::Duty::Disposable | doctrine::Duty::BombPlanter))
}

fn recovery_floor(hero: bool) -> u16 { if hero { 850 } else { 600 } }
fn entry_floor(hero: bool) -> u16 { if hero { 750 } else { 300 } }

fn ranked_entry_floor(hero:bool,elite:bool)->u16 {
    if hero {entry_floor(true)}else if elite {550}else{entry_floor(false)}
}
fn ranked_recovery_floor(hero:bool,elite:bool)->u16 {
    if hero {recovery_floor(true)}else if elite {700}else{recovery_floor(false)}
}

fn in_rear(brain:&AssistBrain,cell:Cell)->bool {
    brain.census.base_cell.is_some_and(|home|distance(home,cell)<=16)
        || brain.census.refinery_cells.iter().any(|home|distance(*home,cell)<=12)
}

fn voluntary_repair_due(unit:&Unit,rear:bool)->bool {
    unit.slot==Slot::Vehicle && unit.health_permille<=(if rear {850}else{650})
        && !unit.traits.harvester && !unit.traits.yard_maker
}

fn available_field_healer(brain:&AssistBrain,rules:&RuleSet,obs:&Observation<'_>,
    unit:&Unit,visible:&[EnemyEntity])->bool {
    // Only an observed nearby compatible healer in a safe work area.
    brain.census.units.iter().any(|medic|medic.controllable() && !medic.in_service()
        && medic.health_permille>=600 && distance(medic.cell,unit.cell)<=24
        && super::advanced::field_healer(rules,obs,medic.entity_id)
        && super::advanced::field_heal_compatible(rules,obs,medic.entity_id,unit.entity_id)
        && orders::reachable(rules,obs,medic.entity_id,unit.cell)
        && safe_leg(&envelopes(rules,obs,medic.entity_id,visible,true),
            medic.cell,unit.cell,false))
}

fn casualty_post(brain:&AssistBrain,rules:&RuleSet,obs:&Observation<'_>,unit:&Unit,
    guns:&[Envelope],now:u64)->Option<Cell> {
    let id=unit.entity_id;
    let valid=|p:Cell|valid_cell(rules,obs,unit,p) && !covered(guns,p)
        && safe_leg(guns,unit.cell,p,true)
        && brain.census.refinery_cells.iter().all(|at|distance(*at,p)>4)
        && brain.census.repair_sites.iter().all(|(at,_)|distance(*at,p)>4);
    if let Some((post,since))=brain.safety.rear_posts.get(&id).copied() {
        // Do not make a safe patient orbit the base on every sample.
        let crowded=brain.census.units.iter().filter(|u|u.entity_id!=id
            && matches!(u.slot,Slot::Infantry|Slot::Vehicle)
            && distance(u.cell,post)<=1).take(3).count()>=3;
        if valid(post) && (!crowded || now.saturating_sub(since)<180) {return Some(post);}
    }
    let mut best=None;
    for anchor in brain.census.yard_cells.iter().copied()
        .chain(brain.census.refinery_cells.iter().copied()).chain(brain.census.base_cell) {
        for r in [5i32,8,11,14] {
            for (dx,dy) in [(r,0),(-r,0),(0,r),(0,-r),(r,r),(-r,r),(r,-r),(-r,-r),
                (r,r/2),(-r,r/2),(r,-r/2),(-r,-r/2)] {
                let (Ok(x),Ok(y))=(u16::try_from(i32::from(anchor.0)+dx),
                    u16::try_from(i32::from(anchor.1)+dy)) else{continue;};
                let post=(x,y);
                if !valid(post){continue;}
                let crowded=brain.census.units.iter().filter(|u|u.entity_id!=id
                    && distance(u.cell,post)<=2).count();
                let reserved=brain.safety.rear_posts.iter().filter(|(other,(p,_))|
                    **other!=id && distance(*p,post)<=3).count();
                let score=(crowded+reserved*4,distance(unit.cell,post),r,post);
                if best.is_none_or(|old|score<old){best=Some(score);}
            }
        }
    }
    best.map(|(_,_,_,p)|p)
}

/// Actual repair visit, not merely a depot in the global census. Each
/// building is reserved once; unsafe or unreachable bays do not qualify.
fn repair_depot(brain:&AssistBrain,rules:&RuleSet,obs:&Observation<'_>,unit:&Unit,
    guns:&[Envelope],claims:&BTreeMap<u64,u64>)->Option<u64> {
    if unit.slot!=Slot::Vehicle || obs.credits()<=0 {return None;}
    let id=unit.entity_id;
    let ty=obs.object_type(rules,unit.type_id)?;
    brain.census.repair_sites.iter().filter(|(cell,bay)|
        obs.can_visit_own_repair_bay(rules,id,*bay)
        && claims.get(bay).is_none_or(|actor|*actor==id)
        && orders::campaign_building_approachable(rules,obs,ty.movement_zone,unit.cell,*bay)
        && safe_leg(guns,unit.cell,*cell,true) && !covered(guns,*cell))
        .min_by_key(|(cell,bay)|(brain.safety.repair_visits.get(&id)
            .is_none_or(|(old,_,_)|old!=bay),distance(*cell,unit.cell),*bay)).map(|(_,bay)|*bay)
}

fn valid_cell(rules: &RuleSet, obs: &Observation<'_>, unit: &Unit, cell: Cell) -> bool {
    obs.is_cell_in_playfield(cell.0, cell.1)
        && (!obs.has_ground_grid() || obs.is_ground_walkable(cell.0, cell.1))
        && orders::reachable(rules, obs, unit.entity_id, cell)
}

fn within_firing_annulus(cell: Cell, at: Cell, inner: i32, outer: i32) -> bool {
    let dx = i64::from(cell.0)-i64::from(at.0);
    let dy = i64::from(cell.1)-i64::from(at.1);
    let d2 = dx*dx+dy*dy;
    outer >= inner && d2 >= i64::from(inner)*i64::from(inner)
        && d2 <= i64::from(outer)*i64::from(outer)
}

/// A conservative firing annulus for a concrete visible building. Use the
/// actual legal damaging weapon and its min/max range (not a guessed type
/// name), a one-cell margin at maximum reach, and all visible fixed/control
/// envelopes. This is a geometric admission, not a native path proof.
pub(super) fn firing_post(rules: &RuleSet, obs: &Observation<'_>, unit: &Unit,
    target: u64, at: Cell, search: bool) -> Option<Cell> {
    if !ground_actor(rules,obs,unit) || !obs.can_hurt(rules,unit.entity_id,target) { return None; }
    let ty = obs.entity_type(rules,unit.entity_id)?;
    let victim = obs.entity_type(rules,target)?;
    if victim.category != ObjectCategory::Building || ty.gunner || obs.is_deployed(unit.entity_id) { return None; }
    let per_cell = crate::util::lepton::LEPTONS_PER_CELL_I32;
    let weapon = (0..ty.weapon_record_count()).filter_map(|slot| rules.weapon(ty.weapon_at(slot)?))
        .filter(|w| !w.never_use && !w.suicide && w.damage > 0
            && w.minimum_range_leptons < w.range_leptons
            && rules.weapon_projectile(w).is_some_and(|p| p.ag)
            && super::siege_counter::effective_ground_warhead(rules,ty,w,victim))
        .max_by_key(|w| w.range_leptons)?;
    let outer = (weapon.range_leptons.div_euclid(per_cell)-1).clamp(0,32);
    let inner = weapon.minimum_range_leptons.max(0).div_euclid(per_cell)+1;
    if outer < inner { return None; }
    let visible: Vec<_> = obs.visible_enemy_entities().collect();
    let guns = envelopes(rules,obs,unit.entity_id,&visible,false);
    let valid = |cell: Cell| {
        within_firing_annulus(cell,at,inner,outer)
            && !covered(&guns,cell)
            && safe_leg(&guns,unit.cell,cell,true)
            && valid_cell(rules,obs,unit,cell)
    };
    // A live destination is a lease, not a reason to keep resetting Move.
    if let Some(cell) = obs.destination(unit.entity_id).filter(|cell| valid(*cell)) { return Some(cell); }
    if !search { return None; }
    let mut best = None;
    // Sample a fixed number of points on the outside four rings. Include
    // axial and diagonal-offset points so one blocked lane isn't fatal.
    for r in (inner.max(outer-3)..=outer).rev() {
        for (dx,dy) in [(r,0),(-r,0),(0,r),(0,-r),
            (r*3/4,r/2),(r*3/4,-r/2),(-r*3/4,r/2),(-r*3/4,-r/2),
            (r/2,r*3/4),(-r/2,r*3/4),(r/2,-r*3/4),(-r/2,-r*3/4)] {
            let (Ok(x),Ok(y)) = (u16::try_from(i32::from(at.0)+dx),
                u16::try_from(i32::from(at.1)+dy)) else { continue; };
            let cell = (x,y);
            if cell == unit.cell || !valid(cell) { continue; }
            let score = (distance(unit.cell,cell),std::cmp::Reverse(distance(cell,at)),cell);
            if best.is_none_or(|old| score < old) { best=Some(score); }
        }
    }
    best.map(|(_,_,cell)| cell)
}

/// A native in-range attack does not need to chase. Reject positions inside
/// another currently observed fixed weapon or psychic-control envelope.
pub(super) fn firing_here(rules: &RuleSet, obs: &Observation<'_>, unit: &Unit, target: u64) -> bool {
    if obs.engage(rules,unit.entity_id,target) != Engage::Ready
        || !obs.can_hurt(rules,unit.entity_id,target) { return false; }
    let visible: Vec<_> = obs.visible_enemy_entities().collect();
    !covered(&envelopes(rules,obs,unit.entity_id,&visible,false),unit.cell)
}

fn shooting(rules: &RuleSet, obs: &Observation<'_>, actor: u64) -> bool {
    obs.attack_target(actor).is_some_and(|target| obs.observed_object_cell(target).is_some()
        && obs.is_hostile(target) && obs.engage(rules, actor, target) == Engage::Ready)
}

/// Short outward steps also work while L2 protects a live firing mission
/// against long movement orders. Prefer the rear, never a second gun box.
fn retreat_step(rules: &RuleSet, obs: &Observation<'_>, unit: &Unit,
    guns: &[Envelope], rear: Cell, short: bool) -> Option<Cell> {
    let max = if short { 2 } else { 8 };
    let mut best = None;
    for ring in [2i32, 4, 6, 8] {
        if ring > max { break; }
        for (dx, dy) in [(-ring,0),(ring,0),(0,-ring),(0,ring),
            (-ring,-ring),(-ring,ring),(ring,-ring),(ring,ring)] {
            let (Ok(x), Ok(y)) = (u16::try_from(i32::from(unit.cell.0)+dx),
                u16::try_from(i32::from(unit.cell.1)+dy)) else { continue; };
            let cell = (x,y);
            if !safe_leg(guns, unit.cell, cell, true) || !valid_cell(rules, obs, unit, cell) { continue; }
            let score = (covered(guns, cell), distance(cell,rear), ring, cell);
            if best.is_none_or(|old| score < old) { best = Some(score); }
        }
    }
    best.map(|(_,_,_,cell)| cell)
}

/// Secondary escape for overlapping batteries: leaving every envelope in
/// one step can be geometrically impossible. Never enter a new envelope, and
/// require the exposure count/depth to decrease strictly. Only front review
/// uses this fallback; specialists and wounded-service routes remain strict.
fn exposure(guns:&[Envelope],cell:Cell)->(usize,i64) {
    guns.iter().fold((0,0),|(count,depth),gun| {
        let d=distance(gun.at,cell);
        if d<=gun.reach {(count+1,depth+i64::from(gun.reach-d+1))} else {(count,depth)}
    })
}
fn escape_reduces(guns:&[Envelope],from:Cell,to:Cell)->bool {
    exposure(guns,to)<exposure(guns,from)
        && guns.iter().all(|gun|distance(gun.at,from)<=gun.reach
            || !tactics::path_enters_control_box(from,to,gun.at,gun.reach))
}
fn front_escape_step(rules:&RuleSet,obs:&Observation<'_>,unit:&Unit,guns:&[Envelope],rear:Cell,short:bool)->Option<Cell> {
    let mut best=None;
    for ring in [2i32,4,6,8] {
        if short && ring>2 {break;}
        for (dx,dy) in [(-ring,0),(ring,0),(0,-ring),(0,ring),(-ring,-ring),(-ring,ring),(ring,-ring),(ring,ring)] {
            let (Ok(x),Ok(y))=(u16::try_from(i32::from(unit.cell.0)+dx),u16::try_from(i32::from(unit.cell.1)+dy)) else {continue;};
            let cell=(x,y);
            if !escape_reduces(guns,unit.cell,cell)||!valid_cell(rules,obs,unit,cell) {continue;}
            let score=(exposure(guns,cell),distance(cell,rear),ring,cell);
            if best.is_none_or(|old|score<old) {best=Some(score);}
        }
    }
    best.map(|(_,_,_,cell)|cell)
}

pub(super) fn safe_attack_leg(rules:&RuleSet,obs:&Observation<'_>,unit:&Unit,
    cell:Cell,visible:&[EnemyEntity])->bool {
    safe_leg(&envelopes(rules,obs,unit.entity_id,visible,true),unit.cell,cell,false)
}

fn native_goal(obs: &Observation<'_>, unit: &Unit) -> Option<Goal> {
    let id = unit.entity_id;
    match unit.mission.known() {
        Some(MissionType::Capture) => obs.own_destination_object(id).map(|target| Goal::Capture {target}),
        Some(MissionType::Enter) => obs.own_destination_object(id).map(|target| Goal::Enter {target}),
        Some(MissionType::Attack) => obs.attack_target(id).map(|target| Goal::Attack {target})
            .or_else(|| obs.destination(id).map(|cell| Goal::AttackMove {cell})),
        Some(MissionType::AttackMove) => obs.destination(id)
            .map(|cell| Goal::AttackMove {cell})
            .or_else(|| obs.attack_target(id).map(|target| Goal::Attack {target})),
        Some(MissionType::Move) => obs.destination(id)
            .map(|cell| Goal::Move {cell})
            .or_else(|| obs.own_destination_object(id).map(|target| Goal::MoveToObject {target}))
            .or_else(|| obs.attack_target(id).map(|target| Goal::Attack {target})),
        _ => obs.attack_target(id).map(|target| Goal::Attack {target}),
    }
}

fn unsafe_goal(rules: &RuleSet, obs: &Observation<'_>, unit: &Unit,
    guns: &[Envelope], goal: Goal) -> bool {
    let id = unit.entity_id;
    let to = match goal {
        Goal::Stop => return false,
        Goal::Move {cell} => return !safe_leg(guns, unit.cell, cell, true),
        Goal::AttackMove {cell} | Goal::GuardAt {cell} | Goal::AttackCell {cell} => Some(cell),
        Goal::Attack {target} => {
            let Some(cell) = obs.observed_object_cell(target) else { return true; };
            if !obs.is_hostile(target) { return true; }
            // A safe V3/Prism shot need not enter the target's own gun box.
            Some(if obs.engage(rules, id, target) == Engage::Ready { unit.cell } else { cell })
        }
        Goal::Capture {target} => {
            let Some(cell) = obs.observed_object_cell(target) else { return true; };
            if unit.traits.engineer && !obs.can_take_building(rules, id, target) { return true; }
            Some(cell)
        }
        Goal::Enter {target} | Goal::MoveToObject {target} => obs.observed_object_cell(target),
        Goal::Guard | Goal::Scatter | Goal::Deploy => Some(unit.cell),
        _ => return false,
    };
    to.is_none_or(|to| !safe_leg(guns, unit.cell, to, false))
}

fn clear_task(brain: &mut AssistBrain, actor: u64) {
    brain.tracked.cancel_for_safety(actor);
    brain.crate_claims.remove(&actor);
    brain.crate_returns.remove(&actor);
    // Do not discard a carrier's existing ferry ticket just because the
    // carrier is wounded: its passenger may already be aboard. The native
    // cargo and the ferry's existing lease/return logic remain authoritative.
    brain.specialist_shuttles.retain(|_, task| task.passenger != actor);
}

/// Re-evaluate injuries before other force orders. Arrival at home, a won
/// exchange or an absent repair bay is never evidence that health recovered.
pub(super) fn recover(brain: &mut AssistBrain, rules: &RuleSet, obs: &Observation<'_>,
    tick: u64, out: &mut Outbox) {
    brain.safety.plans.clear();
    if !brain.safety.announced {
        log::info!("auto play: safety_continuity tick={tick}");
        brain.safety.announced = true;
    }
    let units: Vec<_> = brain.census.units.iter().filter(|u| !u.hand
        && (brain.safety.wounded.contains(&u.entity_id)
            || u.health_permille <= ranked_entry_floor(u.traits.irreplaceable,obs.own_elite(u.entity_id)).max(650)
            || (u.health_permille<=850 && in_rear(brain,u.cell))
            || (u.traits.combat && obs.has_parasite(u.entity_id)))
        && ground_recovery_unit(rules, obs, u)).copied().collect();
    let live: BTreeSet<_> = units.iter().map(|u| u.entity_id).collect();
    brain.safety.wounded.retain(|id| live.contains(id));
    // Existing native docking visits get first refusal on their bay. Reserve
    // an otherwise empty bay once per frame, not one request per casualty.
    brain.safety.repair_visits.retain(|id,_|live.contains(id));
    brain.safety.rear_posts.retain(|id,_|live.contains(id) && brain.safety.wounded.contains(id));
    brain.safety.field_progress.retain(|id,_|live.contains(id));
    brain.safety.field_retry_after.retain(|id,end|live.contains(id) && tick<*end);
    brain.safety.repair_retry_after.retain(|id,end|live.contains(id) && tick<*end);
    let mut bay_claims = BTreeMap::new();
    for unit in &units {
        if unit.mission.known()==Some(MissionType::Enter)
            && let Some(bay)=obs.own_destination_object(unit.entity_id)
            && obs.own_repair_bay_available(rules,unit.entity_id,bay) {
            bay_claims.entry(bay).or_insert(unit.entity_id);
        }
    }
    for (&id,&(bay,_,progress)) in &brain.safety.repair_visits {
        if tick.saturating_sub(progress)<450 && obs.can_visit_own_repair_bay(rules,id,bay) {
            bay_claims.entry(bay).or_insert(id);
        }
    }
    let mut visible = None;
    for unit in &units {
        let id = unit.entity_id;
        let parasite = obs.has_parasite(id);
        let already = brain.safety.wounded.contains(&id);
        let servicing = obs.own_repair_service(rules,id);
        let enemies=visible.get_or_insert_with(||obs.visible_enemy_entities().collect::<Vec<_>>());
        let guns=envelopes(rules,obs,id,enemies,true);
        let mut depot=repair_depot(brain,rules,obs,unit,&guns,&bay_claims);
        let mut failed_visit=false;
        if let Some((bay,last,progress))=brain.safety.repair_visits.get(&id).copied() {
            if servicing {
                depot=Some(bay);
            } else if depot!=Some(bay) || (last==unit.cell && tick.saturating_sub(progress)>=450) {
                // Busy/destroyed/blocked depot: stop renewing the failed
                // native trip. Return to a safe rear area instead.
                brain.safety.repair_visits.remove(&id);
                brain.safety.repair_retry_after.insert(id,tick.saturating_add(600));
                failed_visit=true;
                depot=None;
            } else if last!=unit.cell {
                brain.safety.repair_visits.insert(id,(bay,unit.cell,tick));
            }
        }
        let may_repair=!brain.safety.repair_retry_after.contains_key(&id);
        if !may_repair && !servicing {depot=None;}
        let rearward=in_rear(brain,unit.cell);
        let mut field_heal=rearward && unit.health_permille<950 && !brain.safety.field_retry(id,tick)
            && available_field_healer(brain,rules,obs,unit,enemies);
        if field_heal {
            let sample=brain.safety.field_progress.entry(id).or_insert((unit.health_permille,tick));
            if unit.health_permille>sample.0 {
                *sample=(unit.health_permille,tick);
            }else if tick.saturating_sub(sample.1)>=600 {
                field_heal=false;
                brain.safety.field_progress.remove(&id);
                brain.safety.field_retry_after.insert(id,tick.saturating_add(600));
                log::debug!("auto play: v63_field_treatment_stalled tick={tick} actor={id}");
            }
        }else{brain.safety.field_progress.remove(&id);}
        let repairing=servicing || brain.safety.repair_visits.contains_key(&id)
            || (depot.is_some() && voluntary_repair_due(unit,rearward)) || field_heal;
        let release_floor=if repairing {950}else{
            ranked_recovery_floor(unit.traits.irreplaceable,obs.own_elite(id))};
        if already && unit.health_permille >= release_floor
            && !parasite && !servicing && !failed_visit {
            brain.safety.wounded.remove(&id);
            brain.safety.requested.remove(&id);
            brain.safety.repair_visits.remove(&id);
            brain.safety.rear_posts.remove(&id);
            log::info!("auto play: casualty_recovered tick={tick} actor={id} hp={}", unit.health_permille);
            continue;
        }
        if !already {
            if !unit.controllable() || (unit.in_service() && !servicing)
                || brain.census.factory_handoffs.contains_key(&id)
                || brain.specialist_shuttles.contains_key(&id)
                || brain.specialist_shuttles.values().any(|trip|trip.passenger==id)
                || brain.safety.return_jobs.contains_key(&id) { continue; }
            if !(unit.traits.combat && (unit.health_permille <= ranked_entry_floor(unit.traits.irreplaceable,obs.own_elite(id))
                || tactics::exposed_casualty(rules, obs, unit)))
                && !(parasite && depot.is_some())
                && !(voluntary_repair_due(unit,rearward) && may_repair && depot.is_some())
                && !(rearward && unit.health_permille<=850 && field_heal)
                { continue; }
            brain.safety.wounded.insert(id);
            clear_task(brain,id);
            log::info!("auto play: casualty_enter tick={tick} actor={id} hp={}", unit.health_permille);
        }
        brain.tracked.hold(id, tick.saturating_add(1));
        if !unit.controllable() || servicing {
            brain.safety.plans.insert(id,None);
            continue;
        }
        // Do not leave a wounded deployed infantryman on the front merely
        // because its locomotor cannot follow an ordinary Move in that pose.
        if unit.slot == Slot::Infantry && unit.traits.deployer && obs.is_deployed(id) {
            if brain.safety.requested.get(&id).is_none_or(|(g,at)|
                *g != Goal::Deploy || tick.saturating_sub(*at) >= 60) {
                out.command(Command::ToggleInfantryDeploy {entity_id:id});
                brain.safety.requested.insert(id,(Goal::Deploy,tick));
            }
            brain.safety.plans.insert(id,None);
            continue;
        }
        let guns=&guns;
        let rear=if depot.is_none() {
            if let Some(post)=casualty_post(brain,rules,obs,unit,guns,tick) {
                if brain.safety.rear_posts.get(&id).is_none_or(|(old,_)|*old!=post) {
                    brain.safety.rear_posts.insert(id,(post,tick));
                    log::debug!("auto play: v63_rear_post tick={tick} actor={id} post={post:?} heal={field_heal}");
                }
                post
            }else{brain.census.rally.or(brain.census.base_cell).unwrap_or(unit.cell)}
        }else{
            brain.safety.rear_posts.remove(&id);
            brain.census.rally.or(brain.census.base_cell).unwrap_or(unit.cell)
        };
        let firing = shooting(rules, obs, id);
        let old = native_goal(obs, unit);
        let unsafe_now = covered(guns,unit.cell);
        let desired = if let Some(target) = depot {
            bay_claims.insert(target,id);
            if !brain.safety.repair_visits.contains_key(&id) {
                brain.safety.repair_visits.insert(id,(target,unit.cell,tick));
                log::info!("auto play: v61_repair_visit tick={tick} actor={id} bay={target} hp={}",unit.health_permille);
            }
            Some(Goal::Enter {target})
        } else if distance(rear,unit.cell)>2 && valid_cell(rules,obs,unit,rear)
            && safe_leg(guns,unit.cell,rear,true) && !covered(guns,rear) && !firing {
            Some(Goal::Move {cell:rear})
        } else if let Some(Goal::Move {cell}) = old.filter(|g| matches!(g,Goal::Move {cell}
            if *cell != unit.cell && safe_leg(guns,unit.cell,*cell,true)
                && (unsafe_now || distance(*cell,rear)<distance(unit.cell,rear)))) {
            Some(Goal::Move {cell})
        } else if unsafe_now || distance(unit.cell,rear)>4 {
            retreat_step(rules,obs,unit,guns,rear,firing)
                .filter(|cell| unsafe_now || distance(*cell,rear)<distance(unit.cell,rear))
                .map(|cell| Goal::Move {cell}).or(Some(Goal::Stop))
        } else if firing && !unsafe_now {
            // Keep a safe in-range shot, not a new pursuit. Native targeting
            // is re-read every frame; if the target leaves range, stop it.
            match old {
                Some(Goal::Attack {..}) => old,
                Some(_) => Some(Goal::Stop),
                None => None,
            }
        } else if old.is_some() || matches!(unit.mission.known(), Some(MissionType::Capture | MissionType::Attack | MissionType::AttackMove | MissionType::Enter)) {
            Some(Goal::Stop)
        } else if unit.traits.combat && !unsafe_now && distance(unit.cell,rear)<=4
            && tick % RETRY_FRAMES == id % RETRY_FRAMES {
            // A casualty without a depot is not useless. A rear-area gun
            // can fire now, without receiving permission to pursue later.
            enemies.iter().filter(|enemy| distance(unit.cell,enemy.cell)<=24
                && obs.engage(rules,id,enemy.entity_id)==Engage::Ready
                && orders::admits(rules,obs,id,&Goal::Attack {target:enemy.entity_id}))
                .min_by_key(|enemy| (distance(unit.cell,enemy.cell),enemy.entity_id))
                .map(|enemy| Goal::Attack {target:enemy.entity_id})
        } else { None };
        let desired = match (desired,old) {
            (Some(Goal::Move {cell}),Some(Goal::AttackMove {cell:old_cell})) if cell == old_cell => Some(Goal::Stop),
            _ => desired,
        };
        let request = desired.filter(|goal| Some(*goal) != old)
            .filter(|goal| brain.safety.requested.get(&id).is_none_or(|(last,at)|
                last != goal || tick.saturating_sub(*at)>=RETRY_FRAMES));
        if let Some(goal) = request {
            brain.safety.requested.insert(id,(goal,tick));
            out.want(id,0,goal);
        }
        brain.safety.plans.insert(id,request);
    }
}

/// A taskless/stalled unit should not remain outside friendly territory
/// indefinitely. This is a return journey, not a replacement for normal
/// scouting, assault staging, local defence, native service or specialist work.
pub(super) fn recall_orphans(brain:&mut AssistBrain,rules:&RuleSet,
    obs:&Observation<'_>,out:&mut Outbox) {
    let now=obs.tick();
    if now%15!=0 || brain.census.base_cell.is_none() {return;}
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    let mut anchors:Vec<Cell>=brain.census.yard_cells.iter().chain(brain.census.refinery_cells.iter())
        .copied().collect();
    if let Some(home)=brain.census.base_cell {anchors.push(home);}
    anchors.sort_unstable();anchors.dedup();
    let mut posts:Vec<Cell>=brain.census.rally.into_iter().collect();
    for &(x,y) in &anchors {
        for r in [3i32,5,8] {
            for (dx,dy) in [(r,0),(-r,0),(0,r),(0,-r),(r,r),(-r,r),(r,-r),(-r,-r)] {
                if let (Ok(x),Ok(y))=(u16::try_from(i32::from(x)+dx),u16::try_from(i32::from(y)+dy)) {
                    if obs.is_cell_in_playfield(x,y) && obs.is_ground_walkable(x,y) {
                        posts.push((x,y));
                    }
                }
            }
        }
    }
    posts.sort_unstable();posts.dedup();
    let units:Vec<_>=brain.census.units.iter().copied().filter(|u|
        u.controllable() && ground_actor(rules,obs,u)
        && !u.traits.yard_maker && !u.traits.harvester && !u.traits.spawned
        && !brain.census.factory_handoffs.contains_key(&u.entity_id)
        && !obs.is_deployed(u.entity_id)
        && obs.inside_transport(u.entity_id).is_none()).collect();
    let live:BTreeSet<_>=units.iter().map(|u|u.entity_id).collect();
    brain.safety.orphan_progress.retain(|id,_|live.contains(id));
    brain.safety.return_jobs.retain(|id,_|live.contains(id));
    brain.safety.abandoned.retain(|id|live.contains(id));
    brain.safety.return_retry_after.retain(|id,end|live.contains(id) && now<*end);
    let mut issued=0;
    for unit in units {
        let id=unit.entity_id;
        let service=obs.own_repair_service(rules,id) || brain.safety.wounded.contains(&id);
        let occupied=brain.tracked.tasks.get(id).is_some()
            || brain.crate_claims.contains_key(&id) || brain.crate_returns.contains_key(&id)
            || brain.asset_guards.contains_key(&id) || brain.outpost_guards.contains_key(&id)
            || brain.cover_detail.values().any(|(actor,_,_)|*actor==id)
            || brain.mine_clearance.get(&id).is_some_and(|(_,end)|now<*end)
            || brain.recon.sweeping(id)
            || brain.recon.destination(id).is_some()
            || brain.joint_ops.reserves(id) || brain.asset_ops.has_muster(id)
            || brain.specialist_shuttles.contains_key(&id)
            || brain.specialist_shuttles.values().any(|t|t.passenger==id)
            || super::advance::siege_member(brain,now,id)
            || brain.charge_rotating.contains_key(&id)
            || brain.repair_ifv_pending.get(&id).is_some_and(|end|now<*end)
            || super::advanced::repair_ifv_loaded(rules,obs,&brain.census,id);
        if service || occupied {
            brain.safety.return_jobs.remove(&id);
            brain.safety.orphan_progress.remove(&id);
            brain.safety.abandoned.remove(&id);
            continue;
        }
        let home=anchors.iter().any(|a|distance(*a,unit.cell)<=12);
        if home {
            brain.safety.return_jobs.remove(&id);
            brain.safety.orphan_progress.remove(&id);
            if brain.safety.abandoned.remove(&id) {
                brain.tracked.cancel_for_safety(id);
                out.orders.retain(|o|o.actor!=id);
                out.orders.push(Order{actor:id,goal:Goal::Stop});
            }
            continue;
        }
        let actual=obs.attack_target(id).filter(|target|
            visible.iter().any(|e|e.entity_id==*target) && obs.can_hurt(rules,id,*target));
        let firing=actual.is_some_and(|target|obs.engage(rules,id,target)==Engage::Ready);
        let dest=obs.destination(id);
        let intent=out.orders.iter().find(|o|o.actor==id).map(|o|o.goal);
        let proposed=match intent {
            Some(Goal::Move{cell}|Goal::AttackMove{cell})=>Some(cell),
            Some(Goal::Attack{target})=>obs.observed_object_cell(target),
            _=>None,
        };
        let new_goal=proposed.is_some_and(|cell|Some(cell)!=dest
            && orders::reachable(rules,obs,id,cell));
        let state=brain.safety.orphan_progress.entry(id).or_insert((unit.cell,dest,now));
        // Mere repeated reissues to the same cell are not progress.
        if state.0!=unit.cell {
            *state=(unit.cell,dest,now);
        } else {
            state.1=dest;
        }
        let stationary=now.saturating_sub(state.2);
        let stalled=stationary>=if dest.is_some(){450}else{240};
        let returning=brain.safety.return_jobs.get(&id).copied();
        let abandoned=brain.safety.abandoned.contains(&id);
        // A coherent main force which is currently holding a front need
        // not march home solely because it has no individual target.
        let formation=brain.squads.squad(Role::Attack).is_some_and(|team|
            team.members.contains(&id) && brain.push_target.is_some_and(|(_,_,front)|
                distance(front,unit.cell)<=20)
                && brain.census.units.iter().any(|v|v.entity_id!=id
                    && team.members.contains(&v.entity_id) && brain.census.available(v)
                    && distance(v.cell,unit.cell)<=8));
        if returning.is_none() && !abandoned
            && (unit.in_service() || firing || (new_goal && stationary<900) || formation || !stalled)
        {continue;}
        if returning.is_none() && brain.safety.return_retry_after.contains_key(&id) {
            brain.safety.abandoned.remove(&id);
            continue;
        }
        let guns=envelopes(rules,obs,id,&visible,true);
        let safe=|cell:Cell| valid_cell(rules,obs,&unit,cell)
            && safe_leg(&guns,unit.cell,cell,true) && !covered(&guns,cell);
        if let Some((post,last,progress))=returning {
            if distance(unit.cell,post)<=4 {
                brain.safety.return_jobs.remove(&id);
                brain.safety.abandoned.remove(&id);
                brain.safety.orphan_progress.remove(&id);
                continue;
            }
            if !safe(post) || (last==unit.cell && now.saturating_sub(progress)>=450) {
                brain.safety.return_jobs.remove(&id);
                brain.safety.abandoned.remove(&id);
                brain.safety.return_retry_after.insert(id,now.saturating_add(240));
                continue;
            }
            if last!=unit.cell {brain.safety.return_jobs.insert(id,(post,unit.cell,now));}
            brain.tracked.hold(id,now.saturating_add(30));
            brain.avoid_retreat_until.insert(id,now.saturating_add(30));
            if dest==Some(post) && intent.is_none() {continue;}
            if issued>=4 {continue;}
            if orders::admits(rules,obs,id,&Goal::Move{cell:post}) {
                out.orders.retain(|o|o.actor!=id);
                out.orders.push(Order{actor:id,goal:Goal::Move{cell:post}});
                issued+=1;
            }
            continue;
        }
        // Do not replace an active, useful native service/docking mission
        // even if its destination is represented by an object instead of a cell.
        if unit.in_service() && !abandoned {continue;}
        if issued>=4 {continue;}
        if let Some(post)=posts.iter().copied().filter(|p|safe(*p))
            .min_by_key(|p|(distance(unit.cell,*p),*p)) {
            let goal=Goal::Move{cell:post};
            if !orders::admits(rules,obs,id,&goal){continue;}
            brain.tracked.cancel_for_safety(id);
            brain.tracked.hold(id,now.saturating_add(30));
            brain.avoid_retreat_until.insert(id,now.saturating_add(30));
            brain.safety.abandoned.remove(&id);
            brain.safety.return_jobs.insert(id,(post,unit.cell,now));
            out.orders.retain(|o|o.actor!=id);
            out.orders.push(Order{actor:id,goal});
            issued+=1;
            log::info!("auto play: v61_return_home tick={now} actor={id} post={post:?} failed_job={abandoned}");
        } else {
            // No reachable safe home on this island. Don't manufacture a
            // cross-water destination or keep a known-dead specialist job.
            brain.safety.abandoned.remove(&id);
            brain.safety.return_retry_after.insert(id,now.saturating_add(240));
            if abandoned {
                out.orders.retain(|o|o.actor!=id);
                out.orders.push(Order{actor:id,goal:Goal::Stop});
            }
        }
    }
}

/// Ordinary front-line actors do not acquire a raider lease merely by staging.
/// Outside an approved operation, however, they must not independently pursue
/// a target into a live heavy gunline. Keep actual local defensive shots and
/// safe standoff fire. This is not an extra ownership/retreat state.
fn front_bad(rules: &RuleSet, obs: &Observation<'_>, unit: &Unit,
    guns: &[Envelope], goal: Goal) -> bool {
    match goal {
        Goal::Attack {target} => {
            if obs.observed_object_cell(target).is_some() && obs.is_hostile(target)
                && obs.engage(rules,unit.entity_id,target)==Engage::Ready
                && obs.entity_type(rules,target).is_some_and(|ty|
                    ty.category != ObjectCategory::Building) { return false; }
            unsafe_goal(rules,obs,unit,guns,goal)
        }
        Goal::AttackMove {..} | Goal::Move {..} | Goal::GuardAt {..}
            | Goal::AttackCell {..} => unsafe_goal(rules,obs,unit,guns,goal),
        _ => false,
    }
}

/// An ordinary healthy tank already in firing range, with two other
/// locally ready armoured guns, is a real combat contact rather than a
/// new lone approach. No exemption follows it outside the current range.
pub(super) fn ready_battery_contact<'u>(
    rules:&RuleSet,obs:&Observation<'_>,unit:&Unit,allies:impl Iterator<Item=&'u Unit>,
    goal:Goal,
)->bool {
    let Goal::Attack {target}=goal else {return false;};
    if unit.slot!=Slot::Vehicle || !unit.traits.combat || !unit.controllable()
        || unit.in_service() || unit.health_permille<700 || unit.traits.irreplaceable
        || unit.traits.carries || unit.traits.yard_maker || unit.traits.harvester
        || unit.traits.suicide || !obs.is_hostile(target)
        || obs.visible_enemy_invulnerable(target)
        || !obs.can_hurt(rules,unit.entity_id,target)
        || !obs.entity_type(rules,target).is_some_and(|ty|
            super::siege_counter::dangerous_battery(rules,ty))
        || !obs.visible_enemy_building_operational(rules,target)
        || obs.engage(rules,unit.entity_id,target)!=Engage::Ready
        // Three ready tanks inside a Prism/Grand Cannon envelope are still
        // exposed tanks. Only a forecast-approved siege wave can accept that
        // trade. A normal safe standoff shot is still allowed.
        || !firing_here(rules,obs,unit,target)
    {return false;}
    let supported=allies.filter(|other|other.entity_id!=unit.entity_id
        && other.slot==Slot::Vehicle && other.controllable() && !other.in_service()
        && other.traits.combat && other.health_permille>=700
        && !other.traits.irreplaceable && !other.traits.carries
        && !other.traits.yard_maker && !other.traits.harvester && !other.traits.suicide
        && distance(other.cell,unit.cell)<=8
        && obs.engage(rules,other.entity_id,target)==Engage::Ready
        && obs.can_hurt(rules,other.entity_id,target)).take(2).count()>=2;
    if !supported {return false;}
    let Some(victim)=obs.entity_type(rules,unit.entity_id) else {return false;};
    // Include mobile controllers too; only scan for them after a real
    // supported contact was found.
    victim.immune_to_psionics || victim.immune_to_psionic_weapons
        || !obs.visible_enemy_entities().any(|e|obs.object_type(rules,e.type_id)
            .and_then(|ty|tactics::mind_control_reach(rules,ty))
            .is_some_and(|reach|distance(e.cell,unit.cell)<=reach+1))
}

pub(super) fn supported_ready_fire(brain:&AssistBrain,rules:&RuleSet,obs:&Observation<'_>,
    unit:&Unit,goal:Goal)->bool {
    !brain.director.recovering_under_fire() && !brain.mobilization.active()
        && !brain.campaign.holding_fist() && brain.census.main_available(unit)
        && !brain.charge_released.contains_key(&unit.entity_id)
        && ready_battery_contact(rules,obs,unit,brain.census.units.iter()
            .filter(|other|brain.census.main_available(other)),goal)
}

fn review_front(brain: &mut AssistBrain, rules: &RuleSet, obs: &Observation<'_>,
    proposed: &BTreeMap<u64,Goal>, replacements: &mut BTreeMap<u64,Option<Goal>>) {
    let tick = obs.tick();
    let batteries: Vec<_> = obs.visible_enemy_entities().filter(|e|
        obs.object_type(rules,e.type_id).is_some_and(|ty|
            (super::siege_counter::dangerous_battery(rules,ty)
                && (super::siege_counter::battery_reach(rules,ty)>=4
                    || tactics::mind_control_reach(rules,ty).is_some())
                && obs.visible_enemy_building_operational(rules,e.entity_id))
            || tactics::actor_control_reach(rules,obs,e.entity_id).is_some()
            || tactics::anti_infantry_standoff(rules,ty))).collect();
    brain.charge_released.retain(|id,target| brain.census.units.iter().any(|u|
        u.entity_id==*id && !u.hand) && (obs.attack_target(*id)==Some(*target)
        || proposed.get(id)==Some(&Goal::Attack {target:*target})));
    if batteries.is_empty() && brain.charge_released.is_empty() { return; }
    let units: Vec<_> = brain.census.units.iter().filter(|u| u.controllable()
        && ground_recovery_unit(rules,obs,u) && !u.traits.engineer
        && !replacements.contains_key(&u.entity_id)
        && (brain.census.available(u) || brain.census.borrowed_defenders.contains(&u.entity_id)
            || brain.charge_released.contains_key(&u.entity_id))
        && !u.in_service() && !obs.own_repair_service(rules,u.entity_id)
        && !super::advance::siege_member(brain,tick,u.entity_id)).copied().collect();
    let mut by_type = BTreeMap::new();
    for unit in &units {
        let id=unit.entity_id;
        let next=proposed.get(&id).copied();
        let native=native_goal(obs,unit);
        let gunner = obs.object_type(rules,unit.type_id).is_some_and(|ty|ty.gunner);
        let guns=by_type.entry((unit.type_id,obs.is_deployed(id),obs.own_elite(id),
            gunner.then_some(id))).or_insert_with(|| {
            let relevant: Vec<_> = batteries.iter().copied().filter(|e|
                obs.object_type(rules,e.type_id).is_some_and(|ty|
                    super::siege_counter::battery_reach(rules,ty)>=7
                    || tactics::mind_control_reach(rules,ty).is_some()
                    || tactics::effective_reach_against(rules,obs,e.entity_id,id)
                        > tactics::effective_reach_against(rules,obs,id,e.entity_id)))
                .collect();
            let mut guns=envelopes(rules,obs,id,&relevant,false);
            if unit.slot==Slot::Infantry {
                for e in &batteries {
                    if obs.object_type(rules,e.type_id).is_some_and(|ty|
                        tactics::anti_infantry_standoff(rules,ty)) {
                        let hostile=tactics::actual_threat_reach(rules,obs,e.entity_id,id);
                        let own=tactics::actual_threat_reach(rules,obs,id,e.entity_id);
                        if hostile>own+1 && obs.can_hurt(rules,e.entity_id,id) {
                            guns.push(Envelope{at:e.cell,reach:hostile+1});
                        }
                    }
                }
            }
            guns
        });
        let released=brain.charge_released.get(&id).copied();
        let bad=|g: Goal| {
            let ended=matches!(g,Goal::Attack {target} if Some(target)==released);
            ended || (!guns.is_empty() && front_bad(rules,obs,unit,guns,g)
                && !matches!(g,Goal::Move {cell} if covered(guns,unit.cell)
                    && escape_reduces(guns,unit.cell,cell) && valid_cell(rules,obs,unit,cell))
                && !supported_ready_fire(brain,rules,obs,unit,g))
        };
        let next_bad=next.is_some_and(|g| bad(g));
        let exposed=covered(guns,unit.cell);
        // A stopped tank under an operational battery is not safely staged.
        // Keep a real local mobile defensive shot while healthy, or a legal
        // outward move. Otherwise escape even if no new Attack was proposed.
        let mobile_shot=|g:Goal| matches!(g,Goal::Attack {target}
            if unit.health_permille>=700 && obs.is_hostile(target)
                && obs.observed_object_cell(target).is_some()
                && obs.entity_type(rules,target).is_some_and(|ty|ty.category!=ObjectCategory::Building)
                && obs.engage(rules,id,target)==Engage::Ready && obs.can_hurt(rules,id,target));
        let leaving=|g:Goal| matches!(g,Goal::Move {cell} if cell!=unit.cell
            && (safe_leg(guns,unit.cell,cell,true)||escape_reduces(guns,unit.cell,cell))
            && valid_cell(rules,obs,unit,cell));
        let native_bad=native.is_some_and(|g| bad(g))
            || (exposed && !native.is_some_and(|g| mobile_shot(g)||leaving(g)));
        if !next_bad && !native_bad { continue; }
        // A safe explicit replacement already supersedes the old pursuit;
        // Stop by itself does not get a tank out of a cannon envelope.
        if native_bad && !next_bad && next.is_some_and(|g| if exposed {
            mobile_shot(g)||leaving(g)
        } else { matches!(g,Goal::Move {..}|Goal::Attack {..}|Goal::AttackMove {..}|Goal::Stop) }) { continue; }
        if brain.census.main_available(unit) { brain.tracked.cancel_for_safety(id); }
        else { brain.tracked.forget_primary(id); }
        let replacement=if !native_bad { None } else if covered(guns,unit.cell) {
            retreat_step(rules,obs,unit,guns,brain.census.rally.or(brain.census.base_cell)
                .unwrap_or(unit.cell),shooting(rules,obs,id))
                .or_else(|| front_escape_step(rules,obs,unit,guns,
                    brain.census.rally.or(brain.census.base_cell).unwrap_or(unit.cell),
                    shooting(rules,obs,id)))
                .map(|cell| Goal::Move {cell}).or(Some(Goal::Stop))
        } else { Some(Goal::Stop) };
        if brain.safety.blocked_at.get(&id).is_none_or(|at| tick.saturating_sub(*at)>=ABORT_FRAMES) {
            brain.safety.blocked_at.insert(id,tick);
            log::info!("auto play: front_guard_veto tick={tick} actor={id} infantry={} released={} proposed={next:?} native={native:?}",unit.slot==Slot::Infantry,released.is_some());
        }
        let replacement=replacement.filter(|g| Some(*g)!=native
            && brain.safety.requested.get(&id).is_none_or(|(last,at)|
                last!=g || tick.saturating_sub(*at)>=RETRY_FRAMES));
        if let Some(goal)=replacement { brain.safety.requested.insert(id,(goal,tick)); }
        replacements.insert(id,replacement);
    }
}

/// A focus-rotation lease withdraws only the chosen actor. The native repair
/// owner and the player's controls retain priority. Continue a real outward
/// destination instead of rewriting it every frame; never cross a new gun.
fn review_rotations(brain:&mut AssistBrain,rules:&RuleSet,obs:&Observation<'_>,
    replacements:&mut BTreeMap<u64,Option<Goal>>) {
    if brain.charge_rotating.is_empty() {return;}
    let tick=obs.tick();
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    let rotating:Vec<_>=brain.census.units.iter().filter(|u|u.controllable() && !u.in_service()
        && !obs.own_repair_service(rules,u.entity_id)
        && brain.charge_rotating.get(&u.entity_id).is_some_and(|(_,until)|tick<*until))
        .copied().collect();
    for unit in &rotating {
        let id=unit.entity_id;
        if replacements.contains_key(&id) || brain.tracked.tasks.get(id).is_some() {continue;}
        brain.tracked.forget_primary(id);
        let guns=envelopes(rules,obs,id,&visible,false);
        let native=native_goal(obs,unit);
        let exposed=covered(&guns,unit.cell);
        let leaving=matches!(native,Some(Goal::Move {cell}) if cell!=unit.cell
            && valid_cell(rules,obs,unit,cell)
            && (safe_leg(&guns,unit.cell,cell,true)||escape_reduces(&guns,unit.cell,cell)));
        let rear=brain.census.rally.or(brain.census.base_cell).unwrap_or(unit.cell);
        let goal=if leaving {None}
            else if exposed {
                retreat_step(rules,obs,unit,&guns,rear,shooting(rules,obs,id))
                    .or_else(||front_escape_step(rules,obs,unit,&guns,rear,shooting(rules,obs,id)))
                    .map(|cell|Goal::Move {cell}).or(Some(Goal::Stop))
            } else if matches!(native,Some(Goal::Attack {..}|Goal::AttackMove {..})) {Some(Goal::Stop)}
            else {None};
        let goal=goal.filter(|g|Some(*g)!=native && brain.safety.requested.get(&id)
            .is_none_or(|(last,at)|last!=g||tick.saturating_sub(*at)>=RETRY_FRAMES));
        if let Some(g)=goal {
            brain.safety.requested.insert(id,(g,tick));
            log::info!("auto play: v56_rotation_order tick={tick} actor={id} goal={g:?}");
        }
        // Never let a same-frame ordinary pursuit undo the withdrawal. This
        // does not alter the other vehicles' existing attack orders.
        replacements.insert(id,goal);
    }
}

/// Check both new winners and native missions that have been allowed to run
/// without a new order. This closes the Attack and Capture handoff holes.
fn isolated_overmatched(own:i32,enemy:i32,attackers:usize,allies:usize)->bool {
    allies==0 && attackers>=2 && i64::from(enemy)*2>=i64::from(own.max(1))*3
}

fn unreachable_attack(rules:&RuleSet,obs:&Observation<'_>,u:&Unit,goal:Goal)->bool {
    match goal {
        Goal::AttackMove{cell}=>!super::orders::reachable(rules,obs,u.entity_id,cell),
        Goal::Attack{target}=>{
            let Some(cell)=obs.observed_object_cell(target) else {return false;};
            if !obs.is_hostile(target) || obs.engage(rules,u.entity_id,target)==Engage::Ready {return false;}
            if obs.entity_type(rules,target).is_some_and(|ty|ty.category==ObjectCategory::Building) {
                !obs.object_type(rules,u.type_id).is_some_and(|ty|
                    if obs.is_campaign() {
                        super::orders::campaign_building_approachable(rules,obs,ty.movement_zone,u.cell,target)
                    } else {super::orders::building_approachable(obs,ty.movement_zone,u.cell,cell)})
            } else {!super::orders::reachable(rules,obs,u.entity_id,cell)}
        }
        _=>false,
    }
}

/// Local force continuity: a remote army cannot protect a lone vehicle.
fn review_isolated(brain:&mut AssistBrain,rules:&RuleSet,obs:&Observation<'_>,
    proposed:&BTreeMap<u64,Goal>,replacements:&mut BTreeMap<u64,Option<Goal>>) {
    let tick=obs.tick();
    // Keep the final check bounded; unreachable new orders are checked every
    // frame, mobile threat comparisons on the ordinary short cadence.
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    let units:Vec<_>=brain.census.units.iter().filter(|u|
        u.controllable() && u.traits.combat && !u.traits.engineer
        && !u.in_service() && ground_recovery_unit(rules,obs,u)
        && !replacements.contains_key(&u.entity_id)
        && !super::advance::siege_member(brain,tick,u.entity_id)
        && !obs.own_repair_service(rules,u.entity_id)
        && !super::orders::committed_garrison_entry(&brain.tracked,rules,obs,u.entity_id)
    ).cloned().collect();
    for unit in &units {
        let id=unit.entity_id;
        let next=proposed.get(&id).copied();
        let native=native_goal(obs,unit);
        let attack=|g:Goal|matches!(g,Goal::Attack{..}|Goal::AttackMove{..});
        let next_bad=next.is_some_and(|g|unreachable_attack(rules,obs,unit,g));
        let native_bad=native.is_some_and(|g|unreachable_attack(rules,obs,unit,g));
        // Do not cancel an already valid retreat or a different service.
        if native_bad && next.is_some_and(|g|!unreachable_attack(rules,obs,unit,g)) {continue;}
        let mut overmatched=false;
        if tick%15==id%15 && next.or(native).is_some_and(attack) {
            let threats:Vec<_>=visible.iter().filter(|e|
                obs.object_type(rules,e.type_id).is_some_and(|ty|ty.category!=ObjectCategory::Building)
                && obs.can_hurt(rules,e.entity_id,id)
                && {let reach=tactics::actual_threat_reach(rules,obs,e.entity_id,id);
                    reach>0 && distance(e.cell,unit.cell)<=reach+2}
            ).collect();
            if threats.len()>=2 {
                let allies=units.iter().filter(|ally|ally.entity_id!=id && ally.health_permille>=500
                    && distance(ally.cell,unit.cell)<=8
                    && super::orders::reachable(rules,obs,ally.entity_id,unit.cell)
                    && threats.iter().any(|e|obs.can_hurt(rules,ally.entity_id,e.entity_id))).count();
                let hostile=threats.iter().filter_map(|e|obs.object_type(rules,e.type_id))
                    .map(|ty|ty.strength.max(1)).fold(0i32,i32::saturating_add);
                let own=obs.object_type(rules,unit.type_id).map_or(1,|ty|ty.strength.max(1));
                overmatched=isolated_overmatched(own,hostile,threats.len(),allies);
            }
        }
        if !next_bad && !native_bad && !overmatched {continue;}
        clear_task(brain,id);
        brain.tracked.hold(id,tick.saturating_add(90));
        brain.avoid_retreat_until.insert(id,tick.saturating_add(90));
        let guns=envelopes(rules,obs,id,&visible,true);
        let rear=brain.census.rally.or(brain.census.base_cell).unwrap_or(unit.cell);
        let replacement=if overmatched {
            retreat_step(rules,obs,unit,&guns,rear,shooting(rules,obs,id))
                .or_else(||front_escape_step(rules,obs,unit,&guns,rear,shooting(rules,obs,id)))
                .map(|cell|Goal::Move{cell}).or(Some(Goal::Stop))
        } else if native_bad {Some(Goal::Stop)} else {None};
        if brain.safety.blocked_at.get(&id).is_none_or(|at|tick.saturating_sub(*at)>=90) {
            brain.safety.blocked_at.insert(id,tick);
            log::info!("auto play: v57_local_veto tick={tick} actor={id} unreachable={} isolated={overmatched}",next_bad||native_bad);
        }
        replacements.insert(id,replacement.filter(|g|Some(*g)!=native
            && brain.safety.requested.get(&id).is_none_or(|(last,at)|last!=g||tick.saturating_sub(*at)>=RETRY_FRAMES)));
        if let Some(Some(goal))=replacements.get(&id) {brain.safety.requested.insert(id,(*goal,tick));}
    }
}

pub(super) fn review(brain: &mut AssistBrain, rules: &RuleSet, obs: &Observation<'_>, out: &mut Outbox) {
    let tick = obs.tick();
    remember_roles(brain,tick);
    let plans = std::mem::take(&mut brain.safety.plans);
    out.attachments.retain(|a| !plans.contains_key(&a.actor));
    let proposed: BTreeMap<_,_> = out.orders.iter().map(|o| (o.actor,o.goal)).collect();
    let mut replacements = plans;
    review_rotations(brain,rules,obs,&mut replacements);
    let units: Vec<_> = brain.census.units.iter().filter(|u| u.controllable()
        && ground_actor(rules,obs,u) && !replacements.contains_key(&u.entity_id)
        && (u.traits.engineer || brain.safety.caution_until.contains_key(&u.entity_id)
            || (u.traits.occupier && (u.mission.known()==Some(MissionType::Capture)
                || matches!(proposed.get(&u.entity_id),Some(Goal::Capture {..}))))))
        .cloned().collect();
    if !units.is_empty() {
        let visible: Vec<_> = obs.visible_enemy_entities().collect();
        let mut by_type = BTreeMap::new();
        for unit in &units {
            let id = unit.entity_id;
            if !unit.traits.engineer && super::advance::siege_member(brain,tick,id) { continue; }
            if obs.own_repair_service(rules,id) { continue; }
            let next = proposed.get(&id).copied();
            let garrison_entry=unit.traits.occupier && !unit.traits.engineer
                && (unit.mission.known()==Some(MissionType::Capture)
                    || matches!(next,Some(Goal::Capture {..})));
            let all_weapons=unit.traits.engineer || garrison_entry;
            let guns = by_type.entry((unit.type_id,all_weapons))
                .or_insert_with(|| envelopes(rules,obs,id,&visible,all_weapons));
            let native = native_goal(obs,unit).or_else(|| {
                if unit.mission.known() == Some(MissionType::Capture) {
                    brain.tracked.primary_goal(id).filter(|g| matches!(g, Goal::Capture {..}))
                } else { None }
            });
            let unsafe_order = |g: Goal| unsafe_goal(rules,obs,unit,guns,g)
                || (unit.traits.engineer && matches!(g,Goal::Capture {target}
                    if !super::site_risk::capture_site_safe(&brain.census,rules,obs,target,&visible)
                        || !orders::building_approachable(obs,
                            obs.object_type(rules,unit.type_id).map_or(MovementZone::Infantry,|ty|ty.movement_zone),
                            unit.cell,obs.observed_object_cell(target).unwrap_or(unit.cell))))
                || (garrison_entry && matches!(g,Goal::Capture {target}
                    if !obs.can_garrison(rules,id,target)));
            let next_bad = next.is_some_and(|g| unsafe_order(g));
            let native_bad = native.is_some_and(|g| unsafe_order(g))
                || (matches!(unit.mission.known(), Some(MissionType::AttackMove | MissionType::Guard | MissionType::AreaGuard))
                    && obs.attack_target(id).is_some_and(|target| unsafe_goal(rules,obs,unit,guns,Goal::Attack {target})))
                || ((unit.traits.engineer || garrison_entry)
                    && unit.mission.known()==Some(MissionType::Capture) && native.is_none());
            let exposed = covered(guns,unit.cell);
            // A new safe movement out of fire supersedes a bad old order.
            let safe_exit = next.is_some_and(|g| matches!(g,Goal::Move {cell}
                if cell != unit.cell && safe_leg(guns,unit.cell,cell,true) && valid_cell(rules,obs,unit,cell)
                    && (!shooting(rules,obs,id) || distance(unit.cell,cell)<=2)));
            let native_exit = native.is_some_and(|g| matches!(g,Goal::Move {cell}
                if cell != unit.cell && safe_leg(guns,unit.cell,cell,true))) && !native_bad;
            // A safe, explicit replacement is allowed to cancel an unsafe
            // native pursuit. Do not veto the very retarget that saves it.
            let overrides = !exposed && next.is_some_and(|g| !next_bad && match g {
                Goal::Move {cell} | Goal::AttackMove {cell} => !shooting(rules,obs,id) || distance(unit.cell,cell)<=2,
                Goal::Attack {..} | Goal::Capture {..} | Goal::Enter {..} | Goal::Stop => true,
                _ => false,
            });
            if safe_exit || overrides || (!next_bad && !native_bad && (!exposed || native_exit)) { continue; }
            let bad_target = next.filter(|_| next_bad).or(native.filter(|_| native_bad));
            if let Some(Goal::Capture {target}) = bad_target {
                brain.specialist_risk_after.insert((id,target),tick.saturating_add(240));
                if unit.traits.engineer {brain.asset_ops.aborted(tick,id,target);}
            }
            clear_task(brain,id);
            brain.tracked.hold(id,tick.saturating_add(ABORT_FRAMES));
            if !unit.traits.engineer {
                brain.avoid_retreat_until.insert(id,tick.saturating_add(ABORT_FRAMES));
            }
            let rear = brain.census.rally.or(brain.census.base_cell).unwrap_or(unit.cell);
            let replacement = if native_exit { None } else if exposed {
                retreat_step(rules,obs,unit,guns,rear,shooting(rules,obs,id))
                    .map(|cell| Goal::Move {cell}).or(Some(Goal::Stop))
            } else if native_bad { Some(Goal::Stop) }
            else { None }; // a safe existing detour is still valid
            let replacement = replacement.filter(|g| Some(*g)!=native);
            let report = brain.safety.blocked_at.get(&id).is_none_or(|at| tick.saturating_sub(*at)>=ABORT_FRAMES);
            if report {
                if unit.traits.engineer && super::trace_enabled("AUTOPLAY_V56_TRACE") {
                    let reason=match bad_target {
                        Some(Goal::Capture {target})=>match obs.observed_object_cell(target) {
                            None=>"target-not-observed",
                            Some(_) if !obs.can_take_building(rules,id,target)=>"native-capture-ineligible",
                            Some(cell) if !safe_leg(guns,unit.cell,cell,false)=>"route-envelope",
                            Some(_) if !super::site_risk::capture_site_safe(&brain.census,rules,obs,target,&visible)=>"site-pressure",
                            _=>"other-capture",
                        },
                        _=>"other-goal-or-exposed",
                    };
                    log::info!("auto play: v56_engineer_veto tick={tick} actor={id} reason={reason} target={bad_target:?}");
                }
                brain.safety.blocked_at.insert(id,tick);
                log::info!("auto play: safety_veto tick={tick} actor={id} engineer={} proposed={next:?} native={native:?} replacement={replacement:?}", unit.traits.engineer);
            }
            // Avoid re-sending an unaccepted stop/escape every frame. This
            // is a request cooldown, never permission to accept an unsafe goal.
            let replacement = replacement.filter(|goal| brain.safety.requested.get(&id)
                .is_none_or(|(last,at)| last!=goal || tick.saturating_sub(*at)>=RETRY_FRAMES));
            if let Some(goal) = replacement { brain.safety.requested.insert(id,(goal,tick)); }
            replacements.insert(id,replacement);
        }
    }
    review_front(brain,rules,obs,&proposed,&mut replacements);
    review_isolated(brain,rules,obs,&proposed,&mut replacements);
    if replacements.is_empty() { return; }
    out.attachments.retain(|a| !replacements.contains_key(&a.actor));
    // The ordinary deploy pass runs before us. Do not leave a contradictory
    // raw toggle on a healthy actor whose new movement was safety-vetoed.
    // Recovery's own deliberate undeploy toggle remains intact.
    out.commands.retain(|c| !matches!(c,Command::ToggleInfantryDeploy {entity_id}
        if replacements.contains_key(entity_id) && !brain.safety.wounded.contains(entity_id)));
    // One order per actor. Urgent safe replacements go first, with stable
    // actor-id ordering; the remaining winners keep their original order.
    out.orders.retain(|o| !replacements.contains_key(&o.actor));
    let mut urgent: Vec<_> = replacements.into_iter().filter_map(|(actor, goal)|
        goal.map(|goal| Order {actor,goal})).collect();
    urgent.append(&mut out.orders);
    out.orders = urgent;
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    #[test]
    fn v62_elites_withdraw_earlier_without_becoming_hero_only_units() {
        assert_eq!(ranked_entry_floor(false,false),300);
        assert_eq!(ranked_entry_floor(false,true),550);
        assert_eq!(ranked_recovery_floor(false,true),700);
        assert!(ranked_entry_floor(true,false)>ranked_entry_floor(false,true));
    }
    #[test]
    fn isolated_guard_requires_real_local_superiority() {
        assert!(isolated_overmatched(400,800,2,0));
        assert!(!isolated_overmatched(400,800,2,1));
        assert!(!isolated_overmatched(400,800,1,0));
        assert!(!isolated_overmatched(400,500,2,0));
    }
    #[test]
    fn firing_annulus_accounts_for_minimum_range_and_diagonal_distance() {
        assert!(!within_firing_annulus((20,20),(20,20),6,12));
        assert!(within_firing_annulus((10,20),(20,20),6,12));
        assert!(!within_firing_annulus((10,10),(20,20),6,12));
        assert!(!within_firing_annulus((10,20),(20,20),13,12));
    }
    #[test]
    fn gun_box_includes_capture_endpoint_and_crossing() {
        let guns = [Envelope {at:(20,20),reach:6}];
        assert!(!safe_leg(&guns,(4,20),(20,20),false));
        assert!(!safe_leg(&guns,(4,20),(36,20),false));
        assert!(safe_leg(&guns,(4,12),(36,12),false));
    }
    #[test]
    fn escape_is_outward_and_cannot_enter_another_box() {
        let a = Envelope {at:(20,20),reach:6};
        assert!(safe_leg(&[a],(23,20),(25,20),true));
        assert!(!safe_leg(&[a],(23,20),(10,20),true));
        assert!(!safe_leg(&[a,Envelope {at:(30,20),reach:5}],(23,20),(25,20),true));
    }
    // Flat synthetic fixture; these regressions do not require retail assets.
    fn scene() -> (crate::sim::world::Simulation, RuleSet) {
        let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
            "[VehicleTypes]\n0=TANK\n1=ARTY\n[InfantryTypes]\n0=ENG\n1=RIFLE\n[BuildingTypes]\n0=TOWER\n1=PRIZE\n2=DEPOT\n\
             [TANK]\nStrength=100\nSpeed=5\nPrimary=Gun\nArmor=none\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             [ARTY]\nStrength=100\nSpeed=5\nPrimary=LongGun\nArmor=none\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             [ENG]\nStrength=100\nSpeed=5\nEngineer=yes\nArmor=none\n\
             Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             [RIFLE]\nStrength=100\nSpeed=5\nPrimary=Gun\nArmor=none\n\
             Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             [Gun]\nDamage=20\nROF=20\nRange=4\nWarhead=SA\nProjectile=Bullet\n\
             [LongGun]\nDamage=20\nROF=20\nRange=12\nWarhead=SA\nProjectile=Bullet\n\
             [TowerGun]\nDamage=100\nROF=20\nRange=6\nWarhead=SA\nProjectile=Bullet\n\
             [Bullet]\nAG=yes\nAA=no\nInviso=yes\n\
             [SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [TOWER]\nStrength=1000\nFoundation=1x1\nPrimary=TowerGun\nIsBaseDefense=yes\nPowered=no\n\
             [PRIZE]\nStrength=500\nFoundation=1x1\nCapturable=yes\n\
             [DEPOT]\nStrength=1000\nFoundation=3x3\nUnitRepair=yes\nNumberImpassableRows=1\n",
        )).expect("safety fixture rules");
        let mut sim = crate::sim::arena_fixture::rival_arena_known(&rules,0xA436);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        (sim,rules)
    }

    #[test]
    fn supported_ready_fire_does_not_legalize_exposed_tank_group() {
        let (mut sim,rules)=scene();
        let owner=sim.interner.intern("Americans");
        let tank=sim.spawn_object("TANK","Americans",16,20,255,&rules).unwrap();
        let tower=sim.spawn_object("TOWER","Russians",19,20,255,&rules).unwrap();
        let mut brain=AssistBrain::new(0xA436);
        {
            let obs=Observation::new(&sim,owner);
            brain.rebuild_census(&rules,&obs);
            let unit=brain.census.units.iter().find(|u|u.entity_id==tank).unwrap();
            assert!(!supported_ready_fire(&brain,&rules,&obs,unit,Goal::Attack {target:tower}));
        }
        sim.spawn_object("TANK","Americans",16,19,255,&rules).unwrap();
        sim.spawn_object("TANK","Americans",17,21,255,&rules).unwrap();
        {
            let obs=Observation::new(&sim,owner);
            brain.rebuild_census(&rules,&obs);
            let unit=brain.census.units.iter().find(|u|u.entity_id==tank).unwrap();
            assert_eq!(obs.engage(&rules,tank,tower),Engage::Ready);
            assert!(!supported_ready_fire(&brain,&rules,&obs,unit,Goal::Attack {target:tower}),
                "three ready short-range tanks must not override a stronger battery");
            assert!(!supported_ready_fire(&brain,&rules,&obs,unit,Goal::AttackMove {cell:(19,20)}));
        }
        sim.substrate.entities.get_mut(tank).unwrap().health.set_for_test(40);
        let obs=Observation::new(&sim,owner);
        brain.rebuild_census(&rules,&obs);
        let unit=brain.census.units.iter().find(|u|u.entity_id==tank).unwrap();
        assert!(!supported_ready_fire(&brain,&rules,&obs,unit,Goal::Attack {target:tower}));
    }

    #[test]
    fn safe_standoff_contact_remains_legal() {
        let (mut sim,rules)=scene();
        let owner=sim.interner.intern("Americans");
        let arty=sim.spawn_object("ARTY","Americans",10,20,255,&rules).unwrap();
        sim.spawn_object("ARTY","Americans",10,19,255,&rules).unwrap();
        sim.spawn_object("ARTY","Americans",11,21,255,&rules).unwrap();
        let tower=sim.spawn_object("TOWER","Russians",19,20,255,&rules).unwrap();
        let obs=Observation::new(&sim,owner);
        let mut brain=AssistBrain::new(0xA436);
        brain.rebuild_census(&rules,&obs);
        let unit=brain.census.units.iter().find(|u|u.entity_id==arty).unwrap();
        assert_eq!(obs.engage(&rules,arty,tower),Engage::Ready);
        assert!(firing_here(&rules,&obs,unit,tower));
        assert!(supported_ready_fire(&brain,&rules,&obs,unit,Goal::Attack {target:tower}));
    }
    #[test]
    fn overlapping_escape_must_reduce_exposure_without_new_battery() {
        let guns=[Envelope {at:(20,20),reach:6},Envelope {at:(30,20),reach:6}];
        assert!(escape_reduces(&guns,(25,20),(25,27)));
        assert!(!escape_reduces(&guns,(25,20),(25,20)));
        let more=[guns[0],guns[1],Envelope {at:(25,29),reach:4}];
        assert!(!escape_reduces(&more,(25,20),(25,27)));
    }

    #[test]
    fn retarget_cannot_drop_a_raiders_caution() {
        let (mut sim,rules)=scene();
        let owner=sim.interner.intern("Americans");
        let tank=sim.spawn_object("TANK","Americans",8,20,255,&rules).unwrap();
        let target=sim.spawn_object("PRIZE","Russians",22,20,255,&rules).unwrap();
        let tower=sim.spawn_object("TOWER","Russians",19,20,255,&rules).unwrap();
        assert!(sim.apply_command("Americans",&Goal::Move {cell:(8,10)}.command(tank),Some(&rules)));
        let obs=Observation::new(&sim,owner);
        assert!(tactics::effective_reach_against(&rules,&obs,tower,tank)>0);
        let mut brain=AssistBrain::new(0xA436);
        brain.rebuild_census(&rules,&obs);
        brain.tracked.record_goal(0,tank,TrackedKind::Avoid,None,Some((8,10)),Goal::Move {cell:(8,10)});
        // A real prior danger retreat retains its lease after a retask. A
        // generic assault staging Move alone must not create this lease.
        remember_roles(&mut brain,0);
        assert!(!brain.safety.caution_until.contains_key(&tank));
        brain.safety.mark_detour(tank,0);
        remember_roles(&mut brain,0);
        brain.tracked.cancel_for_safety(tank); // previous detour/task is gone
        let mut out=Outbox::default();
        out.want(tank,6,Goal::Attack {target});
        out.resolve();
        review(&mut brain,&rules,&obs,&mut out);
        assert!(!out.orders.iter().any(|o| o.actor==tank && matches!(o.goal,Goal::Attack {..})));
        assert_eq!(brain.tracked.primary_goal(tank),None);
        assert!(!out.orders.iter().any(|o| o.actor==tank && o.goal==Goal::Stop),
            "rejecting a retarget must not cancel a safe existing detour");
    }

    #[test]
    fn engineer_new_capture_is_rejected_at_tower_endpoint() {
        let (mut sim,rules)=scene();
        let owner=sim.interner.intern("Americans");
        let engineer=sim.spawn_object("ENG","Americans",8,20,255,&rules).unwrap();
        let target=sim.spawn_object("PRIZE","Russians",22,20,255,&rules).unwrap();
        sim.spawn_object("TOWER","Russians",21,18,255,&rules).unwrap();
        let obs=Observation::new(&sim,owner);
        assert!(!specialist_approach_safe(&rules,&obs,engineer,target));
        let mut brain=AssistBrain::new(0xA436);
        brain.rebuild_census(&rules,&obs);
        let mut out=Outbox::default();
        // A direct order from a different pass still faces the final gate.
        out.want(engineer,0,Goal::Capture {target});
        out.resolve();
        review(&mut brain,&rules,&obs,&mut out);
        assert!(!out.orders.iter().any(|o| o.actor==engineer && matches!(o.goal,Goal::Capture {..})));
    }

    #[test]
    fn ongoing_native_capture_is_stopped_when_new_tower_covers_it() {
        let (mut sim,rules)=scene();
        let owner=sim.interner.intern("Americans");
        let engineer=sim.spawn_object("ENG","Americans",8,20,255,&rules).unwrap();
        let target=sim.spawn_object("PRIZE","Russians",22,20,255,&rules).unwrap();
        assert!(sim.apply_command("Americans",&Goal::Capture {target}.command(engineer),Some(&rules)));
        sim.spawn_object("TOWER","Russians",21,18,255,&rules).unwrap();
        let obs=Observation::new(&sim,owner);
        let mut brain=AssistBrain::new(0xA436);
        brain.rebuild_census(&rules,&obs);
        let unit=brain.census.units.iter().find(|u|u.entity_id==engineer).unwrap();
        assert_eq!(native_goal(&obs,unit),Some(Goal::Capture {target}));
        let mut out=Outbox::default();
        review(&mut brain,&rules,&obs,&mut out); // no new proposed order
        assert!(out.orders.iter().any(|o| o.actor==engineer && o.goal==Goal::Stop));
    }

    #[test]
    fn no_bay_casualty_cannot_be_retasked_until_health_returns() {
        let (mut sim,rules)=scene();
        let owner=sim.interner.intern("Americans");
        let arty=sim.spawn_object("ARTY","Americans",8,20,255,&rules).unwrap();
        let target=sim.spawn_object("PRIZE","Russians",26,20,255,&rules).unwrap();
        sim.substrate.entities.get_mut(arty).unwrap().health.set_for_test(20);
        let mut brain=AssistBrain::new(0xA436);
        {
            let obs=Observation::new(&sim,owner);
            brain.rebuild_census(&rules,&obs);
            brain.census.rally=Some((8,20));
            let mut out=Outbox::default();
            recover(&mut brain,&rules,&obs,0,&mut out);
            out.want(arty,6,Goal::Attack {target});
            out.resolve();
            review(&mut brain,&rules,&obs,&mut out);
            assert!(brain.tracked.held(0,arty));
            assert!(!out.orders.iter().any(|o| o.actor==arty));
        }
        // Partial healing above the entry floor is not sufficient.
        sim.substrate.entities.get_mut(arty).unwrap().health.set_for_test(45);
        {
            let obs=Observation::new(&sim,owner);
            brain.rebuild_census(&rules,&obs);
            let mut out=Outbox::default();
            recover(&mut brain,&rules,&obs,1,&mut out);
            assert!(brain.safety.wounded.contains(&arty));
        }
        // Once a legal bay appears, the wounded actor should seek it.
        let depot=sim.spawn_object("DEPOT","Americans",13,20,255,&rules).unwrap();
        {
            let obs=Observation::new(&sim,owner);
            brain.rebuild_census(&rules,&obs);
            let mut out=Outbox::default();
            recover(&mut brain,&rules,&obs,2,&mut out);
            assert!(out.orders.iter().any(|o| o.actor==arty && o.goal==(Goal::Enter {target:depot})));
        }
        sim.substrate.entities.get_mut(arty).unwrap().health.set_for_test(70);
        let obs=Observation::new(&sim,owner);
        brain.rebuild_census(&rules,&obs);
        let mut out=Outbox::default();
        recover(&mut brain,&rules,&obs,3,&mut out);
        assert!(!brain.safety.wounded.contains(&arty));
        assert!(!brain.tracked.held(3,arty));
    }

    #[test]
    fn safe_standoff_shot_is_not_a_charge_into_the_target() {
        let (mut sim,rules)=scene();
        let owner=sim.interner.intern("Americans");
        let arty=sim.spawn_object("ARTY","Americans",8,20,255,&rules).unwrap();
        let tower=sim.spawn_object("TOWER","Russians",18,20,255,&rules).unwrap();
        let obs=Observation::new(&sim,owner);
        let mut brain=AssistBrain::new(0xA436);
        brain.rebuild_census(&rules,&obs);
        let unit=brain.census.units.iter().find(|u|u.entity_id==arty).unwrap();
        let visible:Vec<_>=obs.visible_enemy_entities().collect();
        let guns=envelopes(&rules,&obs,arty,&visible,false);
        assert_eq!(obs.engage(&rules,arty,tower),Engage::Ready);
        assert!(!unsafe_goal(&rules,&obs,unit,&guns,Goal::Attack {target:tower}));
    }

    #[test]
    fn unapproved_rifle_pursuit_is_vetoed_but_safe_artillery_is_preserved() {
        let (mut sim,rules)=scene();
        let owner=sim.interner.intern("Americans");
        let rifle=sim.spawn_object("RIFLE","Americans",8,19,255,&rules).unwrap();
        let arty=sim.spawn_object("ARTY","Americans",8,20,255,&rules).unwrap();
        let tower=sim.spawn_object("TOWER","Russians",18,20,255,&rules).unwrap();
        let obs=Observation::new(&sim,owner);
        let mut brain=AssistBrain::new(0xA436);
        brain.rebuild_census(&rules,&obs);
        let mut out=Outbox::default();
        out.want(rifle,6,Goal::Attack {target:tower});
        out.want(arty,6,Goal::Attack {target:tower});
        out.resolve();
        review(&mut brain,&rules,&obs,&mut out);
        assert!(!out.orders.iter().any(|o| o.actor==rifle && matches!(o.goal,Goal::Attack {..})));
        assert!(out.orders.iter().any(|o| o.actor==arty && o.goal==(Goal::Attack {target:tower})));
    }

    #[test]
    fn health_hysteresis_is_not_the_entry_threshold() {
        assert!(recovery_floor(false)>entry_floor(false));
        assert!(recovery_floor(true)>entry_floor(true));
    }
}
