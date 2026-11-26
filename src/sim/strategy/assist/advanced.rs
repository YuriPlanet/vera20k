//! Conservative specialist micro and local base emergencies.
//!
//! These requests use the same tracked orders as the rest of AutoPlay. Native
//! combat, passenger-weapon selection and unit deployment own the mechanics.
//! This module decides *when* to ask, never writes game state directly.

use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::Observation;
use crate::sim::control::Goal;

use super::AssistBrain;
use super::orders::{issue_tracked, ordered_this_frame};
use super::squads::Role;
use super::tracked::TrackedKind;
use super::squared_distance;

const BASE_ALERT_RADIUS: i32 = 17;
const AIR_ALERT_RADIUS: i32 = 32;
const DEFENDER_REACH: i32 = 33;

/// Air threats enter the alarm perimeter before reaching the yard: a slow
/// bomber must be engaged on approach, not only after its first bomb lands.
fn alarm_radius(airborne: bool) -> i32 {
    if airborne { AIR_ALERT_RADIUS } else { BASE_ALERT_RADIUS }
}
const CARRIER_BOARD_RANGE: i32 = 4;
const CARRIER_RETRY: u64 = 90;
const SIEGE_DEPLOY_RANGE: i32 = 12;
const SIEGE_DANGER_RANGE: i32 = 4;
const SIEGE_LOCK_FRAMES: u64 = 300;

/// Whether the unit is a live airborne threat, regardless of whether a
/// mod listed it in VehicleTypes or AircraftTypes. A deployed helicopter is
/// on the ground; a balloon-hoverer is always aloft.
fn airborne(ty: &crate::rules::object_type::ObjectType, deployed: bool) -> bool {
    ty.category == ObjectCategory::Aircraft || ty.balloon_hover
        || (ty.jumpjet && !deployed)
}

/// Native Iron Curtain/Force Shield is a timer, not an unusually thick armor.
/// Shooting such a unit wastes the defense window. Mobile units directly in
/// its path retreat to a reachable safe cell and retry after the timer ends.
/// Only enemy states already exposed in the player's current vision are read.
pub(super) fn avoid_invulnerable_assault(
    brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox,
) {
    let dangers: Vec<_> = obs.visible_enemy_entities()
        .filter(|enemy| obs.visible_enemy_invulnerable(enemy.entity_id))
        .filter(|enemy| obs.entity_type(rules,enemy.entity_id)
            .is_some_and(|ty| ty.category != ObjectCategory::Building))
        .filter(|enemy| brain.census.base_cell.is_some_and(|home|
            squared_distance(home, enemy.cell) <= 36 * 36)
            || brain.census.units.iter().any(|unit|
                unit.traits.combat && squared_distance(unit.cell,enemy.cell) <= 12 * 12))
        .collect();
    let mut issued = 0;
    for enemy in dangers {
        for unit in &brain.census.units {
            if issued >= 8 { return; }
            if !unit.controllable() || !unit.traits.combat || unit.in_service()
                || ordered_this_frame(out,unit.entity_id)
                || squared_distance(unit.cell,enemy.cell) > 12 * 12
                || super::orders::committed_garrison_entry(
                    &brain.tracked,rules,obs,unit.entity_id)
            { continue; }
            let Some(ty) = obs.entity_type(rules,unit.entity_id) else { continue; };
            // Offset along eight rays *away* from the invulnerable tank.
            // Do not retreat toward a second gun or a cell the unit cannot reach.
            // A retreat must actually leave the engagement perimeter. At
            // 12 cells, a soldier already touching the shielded tank could
            // only travel to distance 12-14, failing the >14 safety test and
            // receiving no move at all. Search farther rings instead; the
            // native reachability and known-gun safety gates still apply.
            let offsets = [(-20,0),(20,0),(0,-20),(0,20),
                (-16,-16),(-16,16),(16,-16),(16,16),
                (-24,0),(24,0),(0,-24),(0,24)];
            let safe = offsets.iter().filter_map(|(dx,dy)| {
                let x = u16::try_from(i32::from(unit.cell.0)+dx).ok()?;
                let y = u16::try_from(i32::from(unit.cell.1)+dy).ok()?;
                let cell=(x,y);
                (obs.is_cell_in_playfield(x,y)
                    && obs.can_reach(ty.movement_zone,unit.cell,cell)
                    && squared_distance(cell,enemy.cell) > 14 * 14
                    && !super::tactics::unsafe_post(rules,obs,unit.entity_id,cell))
                    .then_some(cell)
            }).min_by_key(|cell| (squared_distance(unit.cell,*cell),*cell));
            if let Some(safe)=safe
                && issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,
                    unit.entity_id,TrackedKind::Avoid,None,Some(safe),
                    Goal::Move { cell: safe })
            {
                issued+=1;
            }
        }
    }
}

/// Highest priority local response, without pulling the remote attack squad
/// away from its objective. Defenders choose a visible target that actually
/// threatens a yard, mine or harvester; they do not shoot neutral property.
pub(super) fn protect_base(brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox) {
    let mut sites: Vec<(u16, u16)> = brain.census.yard_cells.iter()
        .chain(brain.census.refinery_cells.iter()).copied().collect();
    // A foreign-side mobile yard is 3,000 credits' worth of future technology.
    // Treat it as a temporary protected base asset on the entire trip home,
    // including when it was found outside any existing base perimeter.
    sites.extend(brain.census.units.iter().filter_map(|unit| {
        if !unit.traits.yard_maker || !unit.controllable() { return None; }
        let ty = obs.object_type(rules,unit.type_id)?;
        let yard = rules.object(ty.deploys_into.as_deref()?)?;
        (brain.census.yards == 0 || super::build::foreign_yard(rules,obs,yard)).then_some(unit.cell)
    }));
    let miners: Vec<_> = brain.census.units.iter().filter(|u| u.traits.harvester).map(|u| u.cell).collect();
    if sites.is_empty() && miners.is_empty() { return; }
    let visible_buildings: std::collections::BTreeSet<_> = obs.visible_enemy_entities()
        .filter(|e| obs.object_type(rules,e.type_id).is_some_and(|ty| ty.category == ObjectCategory::Building))
        .map(|e| e.entity_id).collect();
    let threats: Vec<_> = obs.visible_enemy_entities()
        .filter(|enemy| obs.entity_type(rules, enemy.entity_id)
            .is_some_and(|ty| (ty.category != ObjectCategory::Building
                && super::convergence::mobile_threat(rules,obs,enemy))
                || ((0..8).any(|slot| ty.weapon_at(slot).is_some())
                    && obs.attack_target(enemy.entity_id).is_some())))
        .filter(|enemy| sites.iter().any(|site| {
            let air = obs.entity_type(rules, enemy.entity_id).is_some_and(|ty|
                airborne(ty,obs.is_deployed(enemy.entity_id)));
            // A mind-controller creeping toward the yard is an emergency
            // before it enters ordinary tank weapon range; once it has stolen
            // a defender, the stolen tank is a threat of its own.
            let psychic = obs.entity_type(rules, enemy.entity_id).is_some_and(|ty|
                super::tactics::mind_control_reach(rules, ty).is_some());
            let urgent = obs.entity_type(rules, enemy.entity_id).is_some_and(|ty|
                super::tactics::emergency_threat_score(rules,ty) >= 900);
            let radius = if psychic || urgent { alarm_radius(air).max(28) }
                else { alarm_radius(air) };
            squared_distance(*site, enemy.cell) <= radius * radius
        }) || brain.census.units.iter().any(|miner| miner.traits.harvester
                && squared_distance(miner.cell, enemy.cell) <= 8 * 8))
        .collect();
    if threats.is_empty() { return; }
    let defenders = brain.squads.squad(Role::Defend)
        .map(|s| s.members.clone()).unwrap_or_default();
    let attackers = brain.squads.squad(Role::Attack)
        .map(|s| s.members.clone()).unwrap_or_default();
    let mut issued = 0;
    for defender in &brain.census.units {
        if issued >= 5 { break; }
        // Base emergencies can also borrow an idle *local* mobile not assigned
        // to the attacking fist. This is especially important against air:
        // the Defense role may contain only anti-ground units.
        let local = sites.iter().any(|site|
            squared_distance(*site, defender.cell) <= 17 * 17)
            || miners.iter().any(|site| squared_distance(*site,defender.cell) <= 12*12);
        let local_reserve = !attackers.contains(&defender.entity_id)
            && local && obs.attack_target(defender.entity_id).is_none();
        // A soldier assigned to Attack but still standing *inside* the base
        // cannot watch a harvester or yard get destroyed. A real nearby raid
        // temporarily outranks its stale push role; never recall the remote
        // attacking fist or overwrite an ongoing firefight.
        let emergency_loan = attackers.contains(&defender.entity_id)
            && local && obs.attack_target(defender.entity_id).is_none()
            && threats.iter().any(|enemy| miners.iter().any(|site| squared_distance(*site,enemy.cell) <= 10*10)
                || sites.iter().any(|site| {
                let air = obs.entity_type(rules, enemy.entity_id).is_some_and(|ty|
                    airborne(ty,obs.is_deployed(enemy.entity_id)));
                squared_distance(*site, enemy.cell) <= alarm_radius(air).pow(2)
            }));
        // Anti-air must interrupt even an already fighting base garrison when
        // a bomber approaches; ground-only fighters are not diverted to sky.
        let air_emergency = local && threats.iter().any(|enemy| {
            obs.entity_type(rules,enemy.entity_id).is_some_and(|ty|
                airborne(ty,obs.is_deployed(enemy.entity_id)))
                && obs.can_engage(rules, defender.entity_id, enemy.entity_id)
                && obs.can_hurt(rules, defender.entity_id, enemy.entity_id)
        });
        let critical_emergency = local && threats.iter().any(|enemy| {
            !obs.visible_enemy_invulnerable(enemy.entity_id)
                && obs.entity_type(rules,enemy.entity_id).is_some_and(|ty|
                    super::tactics::emergency_threat_score(rules,ty) >= 900)
                && obs.can_engage(rules, defender.entity_id, enemy.entity_id)
                && obs.can_hurt(rules, defender.entity_id, enemy.entity_id)
        });
        let local_contact = local && brain.census.main_available(defender) && obs.attack_target(defender.entity_id).is_some_and(|id|
            visible_buildings.contains(&id))
            && threats.iter().any(|e| super::convergence::mobile_threat(rules,obs,e)
                && obs.engage(rules,defender.entity_id,e.entity_id) == crate::sim::control::observation::Engage::Ready
                && obs.can_hurt(rules,defender.entity_id,e.entity_id));
        if !(defenders.contains(&defender.entity_id) || local_reserve
            || emergency_loan || air_emergency || critical_emergency || local_contact)
            || !defender.controllable()
            || brain.cover_detail.values().any(|(id,_,_)|*id==defender.entity_id)
            || brain.mine_clearance.get(&defender.entity_id).is_some_and(|(_,end)|obs.tick()<*end)
            || defender.in_service() || !defender.traits.combat
            || brain.safety.reserved(rules,obs,defender)
            || ordered_this_frame(out, defender.entity_id)
            || (brain.tracked.held(obs.tick(), defender.entity_id)
                && !emergency_loan && !air_emergency && !critical_emergency && !local_contact
                && !(local && obs.attack_target(defender.entity_id).is_none()))
        { continue; }
        let target = threats.iter()
            .filter(|enemy| squared_distance(defender.cell, enemy.cell)
                <= DEFENDER_REACH * DEFENDER_REACH)
            .filter(|enemy| !local_contact || (super::convergence::mobile_threat(rules,obs,enemy)
                && obs.engage(rules,defender.entity_id,enemy.entity_id) == crate::sim::control::observation::Engage::Ready))
            .filter(|enemy| !obs.visible_enemy_invulnerable(enemy.entity_id)
                && !protect_controlled_friend(brain,rules,obs,defender.entity_id,enemy.entity_id)
                && obs.can_engage(rules, defender.entity_id, enemy.entity_id)
                && obs.can_hurt(rules, defender.entity_id, enemy.entity_id))
            .filter(|enemy|{
                let Some(theirs)=obs.object_type(rules,enemy.type_id) else{return false;};
                let id=defender.entity_id;
                let ready=obs.engage(rules,id,enemy.entity_id)
                    ==crate::sim::control::observation::Engage::Ready;
                if let Some(control)=super::tactics::actor_control_reach(rules,obs,enemy.entity_id) {
                    if !super::tactics::psychic_immune(rules,obs,id) {
                        return ready && super::advance::cell_distance(defender.cell,enemy.cell)>control+1;
                    }
                }
                if super::tactics::anti_infantry_standoff(rules,theirs) {
                    return ready || obs.object_type(rules,defender.type_id).is_some_and(|ours|
                        super::tactics::personnel_resistant(rules,theirs,ours))
                        && super::orders::reachable(rules,obs,id,enemy.cell)
                        && super::tactics::lethal_approach(rules,obs,id,defender.cell,enemy.cell).is_none();
                }
                true
            })
            .filter(|enemy|{
                // The distant mining district must not recruit a lone tank
                // through a dangerous or disconnected route. The bounded
                // clearance detail above owns coordinated pursuit.
                let remote_mine=brain.mine_incidents.contains_key(&enemy.entity_id)
                    && sites.iter().all(|site|squared_distance(*site,enemy.cell)>18*18);
                !remote_mine || obs.engage(rules,defender.entity_id,enemy.entity_id)
                    ==crate::sim::control::observation::Engage::Ready
            })
            .min_by_key(|enemy| {
                // Attacks directly aimed at income assets take precedence.
                let attacks_asset = obs.attack_target(enemy.entity_id).is_some_and(|id|
                    brain.census.units.iter().any(|unit|
                        unit.entity_id == id && unit.traits.harvester)
                    || obs.own_entities().any(|own|
                        own.entity_id == id && sites.contains(&own.cell)));
                let parasite_weapon = obs.entity_type(rules, enemy.entity_id).is_some_and(|ty| {
                    (0..8).filter_map(|slot| ty.weapon_at(slot))
                        .filter_map(|name| rules.weapon(name))
                        .filter_map(|weapon| weapon.warhead.as_deref())
                        .filter_map(|name| rules.warhead(name))
                        .any(|warhead| warhead.parasite)
                });
                let (airborne, controller, urgency) = obs.entity_type(rules, enemy.entity_id)
                    .map(|ty| (airborne(ty,obs.is_deployed(enemy.entity_id)),
                        super::tactics::mind_control_reach(rules, ty).is_some(),
                        super::tactics::emergency_threat_score(rules,ty)))
                    .unwrap_or((false, false, 0));
                (-urgency, !attacks_asset, !airborne, !controller, !parasite_weapon,
                    sites.iter().chain(miners.iter()).map(|site| squared_distance(*site,enemy.cell))
                        .min().unwrap_or(i32::MAX),enemy.entity_id)
            });
        let Some(target) = target else { continue; };
        if issue_tracked(&mut brain.tracked, brain.trace, out, rules, obs,
            defender.entity_id, TrackedKind::Engage, Some(target.entity_id), Some(target.cell),
            Goal::Attack { target: target.entity_id }) { issued += 1; }
    }
}

/// A recently controlled friendly is still a recoverable asset. Prefer an
/// observed controller and a feasible, actual damaging weapon. No attempt
/// is recorded for an unseen or unreachable controller.
const LIBERATION_WINDOW:u64=210;
const LIBERATION_RETRY:u64=450;

fn open_liberation_window(windows:&mut std::collections::BTreeMap<u64,(u64,u64)>,
    victim:u64,controller:u64,now:u64) {
    windows.entry(victim).or_insert((controller,now.saturating_add(LIBERATION_WINDOW)));
}

pub(super) fn attempt_liberation(brain:&mut AssistBrain,rules:&RuleSet,
    obs:&Observation<'_>,out:&mut Outbox) {
    use crate::sim::control::observation::Engage;
    use super::advance::cell_distance;
    let now=obs.tick();
    if now%15!=0{return;}
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    let pairs:Vec<_>=visible.iter().filter_map(|victim|
        obs.visible_liberation_target(victim.entity_id).map(|controller|(victim.entity_id,controller)))
        .collect();
    let expired:Vec<_>=brain.liberation_window.iter().filter(|(victim,(controller,end))|
        now>=*end || !pairs.contains(&(**victim,*controller)))
        .map(|(id,_)|*id).collect();
    for victim in expired {
        brain.liberation_window.remove(&victim);
        if obs.owns_object(victim) {
            log::info!("auto play: v62_liberation_recovered tick={now} victim={victim}");
        }
        brain.liberation_retry.insert(victim,now.saturating_add(LIBERATION_RETRY));
    }
    brain.liberation_retry.retain(|_,end|now<*end);
    if pairs.is_empty(){return;}
    let mut used=std::collections::BTreeSet::new();
    let mut checked=std::collections::BTreeSet::new();
    let mut issued=0;
    for (_,target) in pairs.iter().copied() {
        if issued>=4 || !checked.insert(target){continue;}
        let Some(enemy)=visible.iter().find(|e|e.entity_id==target) else {continue;};
        let Some(ty)=obs.object_type(rules,enemy.type_id) else {continue;};
        if obs.visible_enemy_invulnerable(target){continue;}
        let victims:Vec<_>=pairs.iter().filter_map(|(victim,controller)|
            (*controller==target).then_some(*victim)).collect();
        if victims.iter().all(|victim|brain.liberation_retry.contains_key(victim)){continue;}
        let mut candidates=Vec::new();
        for u in brain.census.units.iter().filter(|u|brain.census.available(u)
            && u.health_permille>=700 && !u.traits.irreplaceable
            && !u.traits.scout && !used.contains(&u.entity_id)
            && !brain.tracked.held(now,u.entity_id)
            && brain.tracked.tasks.get(u.entity_id).is_none()
            && !brain.cover_detail.values().any(|(id,_,_)|*id==u.entity_id)
            && !brain.joint_ops.reserves(u.entity_id)
            && !brain.census.factory_handoffs.contains_key(&u.entity_id)
            && !super::advance::siege_member(brain,now,u.entity_id)
            && !brain.mine_clearance.contains_key(&u.entity_id)
            && !obs.out_of_ammo(u.entity_id)
            && cell_distance(u.cell,enemy.cell)<=24
            && obs.can_hurt(rules,u.entity_id,target)
            && super::orders::admits(rules,obs,u.entity_id,&Goal::Attack{target})) {
            let id=u.entity_id;
            let Some(ours)=obs.object_type(rules,u.type_id) else {continue;};
            let immune=ours.immune_to_psionics;
            let psychic=super::tactics::actor_control_reach(rules,obs,target).unwrap_or(0);
            let range=super::tactics::actual_threat_reach(rules,obs,id,target);
            let enemy_range=super::tactics::actual_threat_reach(rules,obs,target,id);
            let ready=obs.engage(rules,id,target)==Engage::Ready;
            // Do not feed another victim to the same controller.
            if !immune && psychic>0 && cell_distance(u.cell,enemy.cell)<=psychic+1 {continue;}
            if obs.attack_target(id).is_some_and(|old|old!=target && !victims.contains(&old)
                && obs.engage(rules,id,old)==Engage::Ready
                && obs.attack_target(old)==Some(id)) {continue;}
            let guns=super::safety::envelopes(rules,obs,id,&visible,true);
            if ready {
                if !super::safety::firing_here(rules,obs,u,target) {continue;}
                candidates.push((!ready,cell_distance(u.cell,enemy.cell),id,u.cell,Goal::Attack{target}));
                continue;
            }
            if immune && enemy_range==0 && !obs.is_deployed(id)
                && super::orders::reachable(rules,obs,id,enemy.cell)
                && super::safety::safe_leg(&guns,u.cell,enemy.cell,false) {
                candidates.push((true,cell_distance(u.cell,enemy.cell),id,enemy.cell,Goal::Attack{target}));
                continue;
            }
            if obs.is_deployed(id) || u.traits.aircraft || range<=enemy_range.max(if immune{0}else{psychic})+2 {
                continue;
            }
            // Select an actual walkable firing position, not a Move into
            // the controller's cell. Recheck safety and range every sample.
            let r=(range-1).clamp(2,24);
            let mut post=None;
            for (dx,dy) in [(r,0),(-r,0),(0,r),(0,-r),
                (r*2/3,r*2/3),(-r*2/3,r*2/3),(r*2/3,-r*2/3),(-r*2/3,-r*2/3)] {
                let (Ok(x),Ok(y))=(u16::try_from(i32::from(enemy.cell.0)+dx),
                    u16::try_from(i32::from(enemy.cell.1)+dy)) else {continue;};
                let cell=(x,y);
                if !obs.is_cell_in_playfield(x,y) || !obs.is_ground_walkable(x,y)
                    || !super::orders::reachable(rules,obs,id,cell)
                    || !super::safety::safe_leg(&guns,u.cell,cell,false)
                    || (!immune && cell_distance(cell,enemy.cell)<=psychic+1) {continue;}
                let score=(cell_distance(u.cell,cell),cell);
                if post.is_none_or(|old|score<old){post=Some(score);}
            }
            if let Some((distance,cell))=post {
                candidates.push((true,distance,id,cell,Goal::Move{cell}));
            }
        }
        candidates.sort_by_key(|(approach,distance,id,_,_)|(*approach,*distance,*id));
        let mut attempted=false;
        for (_,_,actor,cell,goal) in candidates.into_iter().take(2) {
            if issued>=4{break;}
            let ready_native=matches!(goal,Goal::Attack{target:t} if obs.attack_target(actor)==Some(t));
            let moving_native=matches!(goal,Goal::Move{cell:c} if obs.destination(actor)==Some(c));
            let requested=if ordered_this_frame(out,actor){false}
                else if ready_native || moving_native {true}
                else {
                    let kind=if matches!(goal,Goal::Attack{..}){TrackedKind::Engage}else{TrackedKind::Move};
                    issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,
                        actor,kind,matches!(goal,Goal::Attack{..}).then_some(target),Some(cell),goal)
                };
            if requested {
                attempted=true;used.insert(actor);issued+=1;
                log::debug!("auto play: v62_liberation_attempt tick={now} actor={actor} controller={target} goal={goal:?}");
            }
        }
        if attempted {
            for victim in victims {
                if !brain.liberation_retry.contains_key(&victim) {
                    if !brain.liberation_window.contains_key(&victim) {
                        log::info!("auto play: v62_liberation_window tick={now} victim={victim} controller={target}");
                    }
                    open_liberation_window(&mut brain.liberation_window,victim,target,now);
                }
            }
        }
    }
}

/// Temporary victim protection applies only while a live, visible controller
/// relationship remains. An immediate self-defence shot is still permitted.
pub(super) fn protect_controlled_friend(brain:&AssistBrain,rules:&RuleSet,obs:&Observation<'_>,
    actor:u64,victim:u64)->bool {
    let Some((controller,end))=brain.liberation_window.get(&victim) else{return false;};
    obs.tick()<*end && obs.visible_liberation_target(victim)==Some(*controller)
        && !(obs.attack_target(victim)==Some(actor)
            && obs.engage(rules,victim,actor)==crate::sim::control::observation::Engage::Ready)
}

/// Reconcile after ordinary attack planners. This only blocks explicit
/// assistant attacks, not native automatic fire or already-launched shots.
pub(super) fn review_liberation(brain:&mut AssistBrain,rules:&RuleSet,
    obs:&Observation<'_>,out:&mut Outbox) {
    if brain.liberation_window.is_empty(){return;}
    let mut blocked=std::collections::BTreeSet::new();
    out.orders.retain(|order|{
        let Goal::Attack{target}=order.goal else{return true;};
        if obs.owned_by_hand(order.actor) || !protect_controlled_friend(brain,rules,obs,order.actor,target)
        {return true;}
        blocked.insert(order.actor);false
    });
    for u in &brain.census.units {
        if !u.controllable() || u.in_service() || !u.traits.combat {continue;}
        let id=u.entity_id;
        if obs.attack_target(id).is_some_and(|victim|protect_controlled_friend(brain,rules,obs,id,victim))
            && !out.orders.iter().any(|o|o.actor==id) {
            blocked.insert(id);
            out.orders.push(crate::sim::control::intent::Order{actor:id,goal:Goal::Stop});
        }
    }
    for id in blocked {
        brain.tracked.void(id,TrackedKind::Attack);
        brain.tracked.void(id,TrackedKind::Engage);
        brain.tracked.forget_primary(id);
    }
}

/// A mine's attacker remains a local clearance objective after the miner
/// flees. Select only visible live threats, not a hidden enemy location.
/// The existing base guard remains responsible for its own emergency.
pub(super) fn clear_mine_raids(brain:&mut AssistBrain,rules:&RuleSet,
    obs:&Observation<'_>,out:&mut Outbox) {
    use crate::sim::control::observation::Engage;
    use super::advance::cell_distance;
    let now=obs.tick();
    if now%15!=0 {return;}
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    let miners:Vec<_>=brain.census.units.iter().filter(|u|u.traits.harvester
        || obs.object_type(rules,u.type_id).is_some_and(|ty|
            ty.category==ObjectCategory::Building && ty.resource_destination && ty.enslaves.is_some()))
        .copied().collect();
    for e in &visible {
        if let Some(miner)=miners.iter().find(|m|obs.can_hurt(rules,e.entity_id,m.entity_id)
            && (obs.attack_target(e.entity_id)==Some(m.entity_id)
                || (cell_distance(e.cell,m.cell)<=10
                    && obs.engage(rules,e.entity_id,m.entity_id)==Engage::Ready))) {
            brain.mine_incidents.insert(e.entity_id,(miner.cell,now.saturating_add(600)));
        }
    }
    // Keep a sighted incident only while the threat remains in the mining
    // district. If it disappears, the old ID is not an attack instruction.
    brain.mine_incidents.retain(|target,(cell,end)|now<*end && visible.iter().any(|e|
        e.entity_id==*target && cell_distance(e.cell,*cell)<=22));
    let live=&brain.census.units;
    brain.mine_clearance_retry.retain(|_,end|now<*end);
    let expired:Vec<_>=brain.mine_clearance.iter().filter(|(_, (target,end))|
        now>=*end || !brain.mine_incidents.contains_key(target))
        .map(|(actor,(target,_))|(*actor,*target)).collect();
    for (actor,target) in expired {
        brain.mine_clearance.remove(&actor);
        brain.mine_clearance_retry.insert(actor,now.saturating_add(240));
        if live.iter().any(|u|u.entity_id==actor && u.controllable() && !u.in_service())
            && obs.attack_target(actor)==Some(target)
            && obs.engage(rules,actor,target)!=Engage::Ready {
            brain.tracked.cancel_for_safety(actor);
            brain.safety.abandon(actor);
            if !out.ordered_this_frame(actor){let _=out.want(actor,0,Goal::Stop);}
        }
    }
    brain.mine_clearance.retain(|actor,(target,end)|now<*end
        && brain.mine_incidents.contains_key(target)
        && live.iter().any(|u|u.entity_id==*actor && u.controllable()
            && !u.in_service() && u.health_permille>=650
            && !brain.safety.reserved(rules,obs,u)));
    if brain.mine_incidents.is_empty(){return;}
    let defenders=brain.squads.squad(Role::Defend)
        .map(|s|s.members.clone()).unwrap_or_default();
    let home=brain.census.base_cell;
    let home_guard_count=live.iter().filter(|u|defenders.contains(&u.entity_id)
        && brain.census.available(u) && home.is_some_and(|h|cell_distance(h,u.cell)<=18)).count();
    let mut issued=0;
    let mut spent_home=0;
    let mut incidents:Vec<_>=brain.mine_incidents.iter().filter_map(|(id,(cell,_))|
        visible.iter().find(|e|e.entity_id==*id).copied().map(|e|(e,*cell))).collect();
    incidents.sort_by_key(|(e,_)|(!obs.attack_target(e.entity_id)
        .is_some_and(|target|miners.iter().any(|m|m.entity_id==target)),e.entity_id));
    for (enemy,field) in incidents {
        if issued>=4 {break;}
        let target=enemy.entity_id;
        let Some(ty)=obs.object_type(rules,enemy.type_id) else {continue;};
        if obs.visible_enemy_invulnerable(target) || ty.category==ObjectCategory::Building
            || super::tactics::mind_control_reach(rules,ty).is_some() {continue;}
        // Actual heavy fixed positions and psychic controllers remain the
        // siege/counter owner's job, never a speculative miner-rescue charge.
        let other_threats:Vec<_>=visible.iter().copied().filter(|e|e.entity_id!=target).collect();
        let mut candidates:Vec<_>=live.iter().copied().filter(|u|{
            let id=u.entity_id;
            let old=brain.mine_clearance.get(&id).is_some_and(|(t,end)|*t==target && now<*end);
            let at_home=home.is_some_and(|h|cell_distance(h,u.cell)<=18);
            let near=cell_distance(u.cell,enemy.cell)<=48;
            (old || brain.census.available(u)) && u.controllable() && !u.in_service()
                && !brain.mine_clearance_retry.contains_key(&id)
                && !out.ordered_this_frame(id)
                && matches!(u.slot,super::census::Slot::Infantry|super::census::Slot::Vehicle)
                && !u.traits.aircraft && !u.traits.irreplaceable && !u.traits.scout
                && u.health_permille>=700 && near
                && !brain.safety.reserved(rules,obs,u)
                && !brain.tracked.held(now,id)
                && brain.tracked.tasks.get(id).is_none()
                && !brain.cover_detail.values().any(|(actor,_,_)|*actor==id)
                && !brain.joint_ops.reserves(id)
                && !super::advance::siege_member(brain,now,id)
                && !brain.asset_guards.contains_key(&id)
                && !brain.outpost_guards.contains_key(&id)
                && !brain.census.factory_handoffs.contains_key(&id)
                && !obs.is_deployed(id)
                && !obs.owned_by_hand(id)
                && !obs.attack_target(id).is_some_and(|t|t!=target
                    && obs.engage(rules,id,t)==Engage::Ready)
                && (old || !at_home || !defenders.contains(&id)
                    || (!brain.director.under_attack_now() && home_guard_count>2))
                && obs.can_hurt(rules,id,target)
                && super::orders::admits(rules,obs,id,&Goal::Attack{target})
                && (obs.attack_target(id)==Some(target)
                    || brain.tracked.admits(now,id,TrackedKind::Engage,Some(target),Some(enemy.cell)))
                && super::safety::safe_leg(&super::safety::envelopes(rules,obs,id,&other_threats,true),
                    u.cell,enemy.cell,false)
                && (obs.engage(rules,id,target)==Engage::Ready
                    || super::orders::reachable(rules,obs,id,enemy.cell))
        }).collect();
        candidates.sort_by_key(|u|(
            !brain.mine_clearance.get(&u.entity_id).is_some_and(|(t,_)|*t==target),
            obs.engage(rules,u.entity_id,target)!=Engage::Ready,
            cell_distance(u.cell,enemy.cell),std::cmp::Reverse(u.health_permille),u.entity_id));
        // Cohort local to the nearest responder; do not count a tank across
        // the map or an unqualified weapon as support. A lone ready shot
        // can defend itself but cannot launch a losing pursuit.
        let Some(first)=candidates.first().copied() else {continue;};
        candidates.retain(|u|cell_distance(u.cell,first.cell)<=12);
        let mut home_slots=home_guard_count.saturating_sub(2+spent_home);
        let group:Vec<_>=candidates.into_iter().filter(|u|{
            let loan=defenders.contains(&u.entity_id)
                && home.is_some_and(|h|cell_distance(h,u.cell)<=18)
                && !brain.mine_clearance.contains_key(&u.entity_id);
            if !loan{return true;}
            if home_slots==0{return false;}
            home_slots-=1;true
        }).take((4usize.saturating_sub(issued)).max(1)).collect();
        let strength=group.iter().fold(0i64,|sum,u|sum+i64::from(u.points.max(0)));
        let hp=i64::from(obs.health(target).unwrap_or(ty.strength).max(1));
        let supported=group.len()>=2 && strength>=hp.saturating_mul(2);
        for u in group {
            if issued>=4 {break;}
            let id=u.entity_id;
            let ready=obs.engage(rules,id,target)==Engage::Ready;
            let loses=super::tactics::unfavourable_vehicle_duel(rules,obs,id,target);
            if (!supported && (!ready || loses))
                || (obs.own_elite(id) && !ready && !supported) {continue;}
            let home_loan=defenders.contains(&id) && home.is_some_and(|h|cell_distance(h,u.cell)<=18)
                && !brain.mine_clearance.contains_key(&id);
            if home_loan && home_guard_count.saturating_sub(spent_home)<=2 {continue;}
            if obs.attack_target(id)==Some(target) && (ready || supported) {
                brain.mine_clearance.entry(id).or_insert((target,now.saturating_add(600)));
                brain.census.unavailable.insert(id);
                if home_loan {spent_home+=1;}
                continue;
            }
            if ordered_this_frame(out,id) {continue;}
            if issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,
                id,TrackedKind::Engage,Some(target),Some(enemy.cell),Goal::Attack{target}) {
                brain.mine_clearance.entry(id).or_insert((target,now.saturating_add(600)));
                brain.census.unavailable.insert(id);
                if home_loan {spent_home+=1;}
                issued+=1;
                log::info!("auto play: v62_mine_clearance tick={now} actor={id} target={target} field={field:?} supported={supported}");
            }
        }
    }
}

/// Role::Defend alone does not bring a soldier home. In long offensive
/// matches the original Guard order could be issued at the front and the
/// soldier remained there for the rest of the game, despite occupying a
/// permanent reserve slot. Repatriate idle, *uncommitted* guards to reachable
/// cells next to the main base, without stealing active firefights or escorts.
pub(super) fn station_home_reserve(
    brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox,
) {
    if brain.director.under_attack_now() { return; }
    let Some(home) = brain.census.base_cell else { return; };
    let defenders = brain.squads.squad(Role::Defend)
        .map(|s| s.members.clone()).unwrap_or_default();
    let offsets = [(5,0),(-5,0),(0,5),(0,-5),(4,4),(4,-4),(-4,4),(-4,-4)];
    let mut issued = 0usize;
    for unit in &brain.census.units {
        if issued >= 4 { break; }
        if !defenders.contains(&unit.entity_id) || !brain.census.available(unit)
            || brain.outpost_guards.contains_key(&unit.entity_id)
            || brain.asset_guards.contains_key(&unit.entity_id)
            || squared_distance(home, unit.cell) <= 12 * 12
            || ordered_this_frame(out, unit.entity_id)
            || obs.attack_target(unit.entity_id).is_some()
            || brain.tracked.held(obs.tick(), unit.entity_id)
            || super::orders::committed_garrison_entry(
                &brain.tracked, rules, obs, unit.entity_id)
        { continue; }
        let Some(ty) = obs.entity_type(rules, unit.entity_id) else { continue; };
        let post = offsets.iter().filter_map(|(dx,dy)| {
            let (Ok(x),Ok(y)) = (
                u16::try_from(i32::from(home.0) + dx),
                u16::try_from(i32::from(home.1) + dy),
            ) else { return None; };
            let cell = (x,y);
            (obs.is_cell_in_playfield(x,y) && obs.is_ground_walkable(x,y)
                && obs.can_reach(ty.movement_zone, unit.cell, cell)
                && !obs.visible_enemy_entities().any(|enemy|
                    squared_distance(enemy.cell, cell) <= 8*8))
                .then_some(cell)
        }).min_by_key(|cell| squared_distance(unit.cell,*cell));
        let Some(post) = post else { continue; };
        if issue_tracked(&mut brain.tracked, brain.trace, out, rules, obs,
            unit.entity_id, TrackedKind::Move, None, Some(post),
            Goal::Move { cell: post }) {
            issued += 1;
        }
    }
}

/// A mining outpost is a military position, not just a newly unfolded
/// construction yard. Keep two local defenders without weakening the primary
/// base by borrowing more than the existing Defend reserve can spare.
/// GuardAt is a native move-and-defend order. It never takes Attack members,
/// and dangerous front-line micro may still pre-empt its low Guard priority.
pub(super) fn secure_outpost(
    brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox,
) {
    if brain.director.under_attack_now() || brain.director.recovering_income() { return; }
    let Some(yard) = super::frontier::remote_yard(&brain.census, &brain.ore_known)
    else { brain.outpost_guards.clear(); return; };
    let reserves = brain.squads.squad(Role::Defend)
        .map(|s| s.members.clone()).unwrap_or_default();
    if reserves.len() < 4 { return; }
    let units = &brain.census.units;
    let eligible = |id: u64| reserves.contains(&id)
        && units.iter().any(|u| u.entity_id == id
            && u.controllable() && u.traits.combat && !u.in_service());
    brain.outpost_guards.retain(|id, post| {
        *post == yard && eligible(*id)
    });
    let local: usize = units.iter().filter(|unit| unit.controllable()
        && unit.traits.combat && reserves.contains(&unit.entity_id)
        && squared_distance(unit.cell, yard) <= 9*9).count();
    let travelling = brain.outpost_guards.keys().filter(|id| {
        units.iter().any(|unit| unit.entity_id == **id
            && squared_distance(unit.cell, yard) > 9*9)
    }).count();
    if local + travelling >= 2 { return; }
    // Reserve at least three other defenders at the original base. Count a
    // travelling guard as assigned, not as an already-arrived local defender.
    if reserves.len().saturating_sub(brain.outpost_guards.len()
        + brain.asset_guards.len()) <= 4 { return; }
    let candidate = units.iter()
        .filter(|unit| eligible(unit.entity_id)
            && !brain.outpost_guards.contains_key(&unit.entity_id)
            && squared_distance(unit.cell, yard) > 10*10
            && !ordered_this_frame(out, unit.entity_id)
            && !brain.tracked.held(obs.tick(), unit.entity_id))
        .min_by_key(|unit| (squared_distance(unit.cell, yard), unit.entity_id));
    let Some(unit) = candidate else { return; };
    let Some(ty) = obs.entity_type(rules, unit.entity_id) else { return; };
    let offsets = [(5,0),(-5,0),(0,5),(0,-5),(4,4),(4,-4),(-4,4),(-4,-4)];
    let post = offsets.into_iter().filter_map(|(dx,dy)| {
        let (Ok(x), Ok(y)) = (u16::try_from(i32::from(yard.0)+dx),
            u16::try_from(i32::from(yard.1)+dy)) else { return None; };
        let cell = (x,y);
        (obs.is_cell_in_playfield(x,y) && obs.is_ground_walkable(x,y)
            && (!obs.has_zone_grid() || obs.can_reach(ty.movement_zone, unit.cell, cell))
            && !obs.visible_enemy_entities().any(|enemy|
                squared_distance(enemy.cell,cell) <= 10*10)).then_some(cell)
    }).next();
    let Some(post) = post else { return; };
    if issue_tracked(&mut brain.tracked, brain.trace, out, rules, obs,
        unit.entity_id, TrackedKind::Guard, None, Some(post),
        Goal::GuardAt { cell: post }) {
        brain.outpost_guards.insert(unit.entity_id, yard);
    }
}

/// A captured lab or public asset changes the front line. Its engineer is
/// consumed on arrival, so a new owner cannot wait for an engineer to defend
/// it. Assign a *bounded*, persistent escort out of the reserve instead of
/// repeatedly raiding the enemy base for a prize we cannot hold.
/// GuardAt is an ordinary player order; the native world owns combat.
fn asset_guard_capacity(defenders:usize,assets:usize)->usize {
    defenders.saturating_sub(3).min(8).min(assets.saturating_mul(2))
}

#[cfg(all(test, tests_sim_p_s))]
#[test]
fn v65_asset_guards_never_consume_the_last_three_defenders() {
    assert_eq!(asset_guard_capacity(3,10),0);
    assert_eq!(asset_guard_capacity(7,10),4);
    assert_eq!(asset_guard_capacity(20,10),8);
    assert_eq!(asset_guard_capacity(20,1),2);
}

pub(super) fn secure_captured_assets(
    brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox,
) {
    let now=obs.tick();
    if now%15!=0 {return;}
    let reserves=brain.squads.squad(Role::Defend)
        .map(|s|s.members.clone()).unwrap_or_default();
    // Every actually held public asset is worth considering, including a
    // recaptured oil rig after a save/load. Do not depend on a receipt list.
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    let mut assets:Vec<_>=obs.own_entities().filter_map(|building|{
        let ty=obs.object_type(rules,building.type_id)?;
        if ty.category!=ObjectCategory::Building
            || !(super::orders::strategic_public_asset(ty)
                || (brain.captured_assets.contains_key(&building.entity_id) && guarded_prize(rules,ty)))
        {return None;}
        let cell=building.cell;
        let threatened=visible.iter().any(|e|super::advance::cell_distance(cell,e.cell)<=18
            && obs.object_type(rules,e.type_id).is_some_and(|t|
                t.engineer || (t.category!=ObjectCategory::Building
                    && super::orders::is_combat_unit(rules,t))));
        let remote=brain.census.base_cell.is_some_and(|home|super::advance::cell_distance(home,cell)>12);
        let priority=if ty.produce_cash_amount>0 {
            4+(i64::from(ty.produce_cash_amount)*900/i64::from(ty.produce_cash_delay.max(1)))
                .clamp(0,4000) as i32/1000
        }else if ty.hospital || ty.unit_repair {3}
        else if high_value_prize(rules,ty){4}else{2};
        (remote || threatened).then_some((building.entity_id,cell,threatened,priority))
    }).collect();
    assets.sort_by_key(|(id,_,threatened,value)|(!*threatened,std::cmp::Reverse(*value),*id));
    let live=&brain.census.units;
    let emergency=brain.director.under_attack_now() || brain.director.recovering_income()
        || brain.director.recovering_under_fire();
    brain.asset_guards.retain(|guard,asset|!emergency && assets.iter().any(|(id,_,_,_)|id==asset)
        && reserves.contains(guard) && live.iter().any(|u|u.entity_id==*guard
            && u.controllable() && !u.in_service() && u.health_permille>=600));
    brain.asset_guard_posts.retain(|id,_|brain.asset_guards.contains_key(id));
    brain.asset_guard_retry.retain(|_,end|now<*end);
    if emergency || reserves.len()<3 {return;}
    let capacity=asset_guard_capacity(reserves.len(),assets.len());
    let assigned:Vec<_>=brain.asset_guards.iter().map(|(actor,asset)|(*actor,*asset)).collect();
    for (actor,asset) in assigned {
        let Some(u)=live.iter().find(|u|u.entity_id==actor) else {continue;};
        let Some((post,last,progress))=brain.asset_guard_posts.get(&actor).copied()
            else {brain.asset_guards.remove(&actor);continue;};
        let guns=super::safety::envelopes(rules,obs,actor,&visible,true);
        let near=squared_distance(post,u.cell)<=4*4;
        let fighting=obs.attack_target(actor).is_some_and(|target|
            obs.engage(rules,actor,target)==crate::sim::control::observation::Engage::Ready);
        let safe=super::orders::reachable(rules,obs,actor,post)
            && super::safety::safe_leg(&guns,u.cell,post,false);
        if (!safe && !fighting) || (!near && !fighting && last==u.cell
            && now.saturating_sub(progress)>=450) {
            brain.asset_guards.remove(&actor);
            brain.asset_guard_posts.remove(&actor);
            brain.asset_guard_retry.insert(asset,now.saturating_add(450));
            brain.tracked.cancel_for_safety(actor);
            brain.safety.abandon(actor);
            continue;
        }
        if last!=u.cell || near || fighting {
            brain.asset_guard_posts.insert(actor,(post,u.cell,now));
        }
        if !near && !fighting && obs.destination(actor)!=Some(post)
            && !out.ordered_this_frame(actor) {
            let _=issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,
                actor,TrackedKind::Guard,None,Some(post),Goal::GuardAt{cell:post});
        }
    }
    let home_available=reserves.iter().filter(|id|!brain.asset_guards.contains_key(*id)
        && !brain.outpost_guards.contains_key(*id)
        && live.iter().any(|u|u.entity_id==**id && brain.census.available(u)
            && brain.census.base_cell.is_some_and(|home|
                super::advance::cell_distance(home,u.cell)<=20))).count();
    for (id,cell,threatened,priority) in assets {
        // A local mobile body counts only when it can actually cover the
        // site. Aircraft passing overhead or a remote appointment do not.
        let desired=if threatened && priority>=3 {2}else{1};
        let assigned=brain.asset_guards.values().filter(|asset|**asset==id).count();
        let local=live.iter().filter(|u|u.traits.combat && u.controllable()
            && !u.in_service() && u.health_permille>=600
            && matches!(u.slot,super::census::Slot::Infantry|super::census::Slot::Vehicle)
            && !u.traits.aircraft
            && !brain.asset_guards.contains_key(&u.entity_id)
            && reserves.contains(&u.entity_id)
            && squared_distance(u.cell,cell)<=6*6
            && obs.attack_target(u.entity_id).is_none()
            && obs.destination(u.entity_id).is_none()).count();
        if assigned>=desired || brain.asset_guards.len()>=capacity
            || brain.asset_guard_retry.contains_key(&id) {continue;}
        let mut best=None;
        for unit in live.iter().filter(|u|reserves.contains(&u.entity_id)
            && brain.census.available(u) && u.health_permille>=650
            && !brain.outpost_guards.contains_key(&u.entity_id)
            && !brain.asset_guards.contains_key(&u.entity_id)
            && !brain.census.mobile_aa.contains(&u.entity_id)
            && !out.ordered_this_frame(u.entity_id) && !brain.tracked.held(now,u.entity_id)
            && obs.attack_target(u.entity_id).is_none()
            && matches!(u.slot,super::census::Slot::Infantry|super::census::Slot::Vehicle)
            && !u.traits.aircraft) {
            let at_home=brain.census.base_cell.is_some_and(|home|
                super::advance::cell_distance(home,unit.cell)<=20);
            if home_available<=3 && at_home {continue;}
            let guns=super::safety::envelopes(rules,obs,unit.entity_id,&visible,true);
            for (dx,dy) in [(3,0),(-3,0),(0,3),(0,-3),(4,4),(-4,4),(4,-4),(-4,-4)] {
                let (Ok(x),Ok(y))=(u16::try_from(i32::from(cell.0)+dx),
                    u16::try_from(i32::from(cell.1)+dy)) else {continue;};
                let post=(x,y);
                if !obs.is_cell_in_playfield(x,y) || !obs.is_ground_walkable(x,y)
                    || !super::orders::reachable(rules,obs,unit.entity_id,post)
                    || !super::safety::safe_leg(&guns,unit.cell,post,false)
                {continue;}
                let crowd=brain.asset_guard_posts.values().filter(|(at,_,_)|
                    squared_distance(*at,post)<=3*3).count() as i32;
                let near_asset=squared_distance(unit.cell,cell)<=6*6;
                let score=(squared_distance(unit.cell,post).saturating_add(crowd*100)
                    .saturating_add(if local>0 && !near_asset {1000}else{0}),
                    unit.entity_id,post);
                if best.is_none_or(|old|score<old){best=Some(score);}
            }
        }
        let Some((_,actor,post))=best else {continue;};
        if issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,
            actor,TrackedKind::Guard,None,Some(post),Goal::GuardAt{cell:post}) {
            brain.asset_guards.insert(actor,id);
            if let Some(unit)=live.iter().find(|u|u.entity_id==actor) {
                brain.asset_guard_posts.insert(actor,(post,unit.cell,now));
            }
            log::info!("auto play: v61_asset_guard tick={now} actor={actor} target={id} post={post:?}");
            break;
        }
    }
}

fn high_value_prize(rules: &RuleSet, ty: &crate::rules::object_type::ObjectType) -> bool {
    ty.build_const_eligible() || ty.construction_yard
        || rules.build_tech_types.iter().any(|name| *name == ty.id)
        || ty.super_weapon.is_some() || ty.super_weapon2.is_some()
}

fn guarded_prize(rules: &RuleSet, ty: &crate::rules::object_type::ObjectType) -> bool {
    high_value_prize(rules, ty) || super::orders::strategic_public_asset(ty)
        || ty.factory_type().is_some()
}

// Garrison release is owned by the native damage/occupancy engine.
// A shortage of mobile defenders must not itself cause Unload.

/// Reconfigure one safe, GI-loaded gunner IFV as a mechanic carrier.
/// Without this step a base that already filled all its IFVs with riflemen
/// could never create the requested engineer IFV, regardless of how many
/// engineers were produced. `Unload` is a normal player order; the native
/// passenger implementation chooses where the exiting soldier can stand.
pub(super) fn free_repair_ifv_seat(
    brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox,
) {
    if brain.director.under_attack_now() || brain.director.recovering_income() { return; }
    let Some(base) = brain.census.base_cell else { return; };
    let available_engineers = brain.census.units.iter().filter(|unit|
        unit.controllable() && unit.traits.engineer && !unit.in_service()
            && obs.inside_transport(unit.entity_id).is_none()).count();
    if available_engineers < 2 { return; }
    let wounded = brain.census.units.iter().any(|unit|
        unit.traits.combat && unit.health_permille < 850
            && obs.entity_type(rules,unit.entity_id).is_some_and(|ty|
                ty.category == ObjectCategory::Vehicle));
    let tanks = brain.census.units.iter().filter(|unit| unit.traits.combat
        && obs.entity_type(rules,unit.entity_id).is_some_and(|ty|
            ty.category == ObjectCategory::Vehicle)).count();
    if !wounded && (tanks < 5 || obs.credits() < 3_000) { return; }
    if brain.census.units.iter().any(|unit|
        repair_ifv_loaded(rules, obs, &brain.census, unit.entity_id))
        || brain.repair_ifv_pending.iter().any(|(id,until)|obs.tick()<*until && obs.owns_object(*id))
    { return; }
    let aa_stock = super::composition::boarding_air_stock(brain,rules,obs);
    let has_empty_chassis = brain.census.units.iter().any(|unit| {
        obs.entity_type(rules, unit.entity_id).is_some_and(|ty| ty.gunner)
            && obs.passengers(unit.entity_id) == 0
            && !super::composition::preserve_air_carrier(rules,obs,unit,aa_stock)
            && squared_distance(base, unit.cell) <= 18 * 18
    });
    if has_empty_chassis { return; }
    let Some(carrier) = brain.census.units.iter().filter(|unit|
        unit.controllable() && !unit.in_service()
            && !ordered_this_frame(out,unit.entity_id)
            && !brain.tracked.held(obs.tick(),unit.entity_id)
            && obs.attack_target(unit.entity_id).is_none()
            && squared_distance(base,unit.cell) <= 18 * 18
            && obs.passengers(unit.entity_id) > 0
            && obs.entity_type(rules,unit.entity_id).is_some_and(|ty| ty.gunner)
            && !super::composition::preserve_air_carrier(rules,obs,unit,aa_stock)
            && !brain.repair_ifv_pending.contains_key(&unit.entity_id)
            && !obs.visible_enemy_entities().any(|enemy|
                squared_distance(enemy.cell,unit.cell) <= 9*9))
        .min_by_key(|unit|unit.entity_id) else { return; };
    if issue_tracked(&mut brain.tracked, brain.trace, out, rules, obs,
        carrier.entity_id, TrackedKind::Enter, None, None, Goal::Unload) {
        brain.repair_ifv_pending.insert(carrier.entity_id,
            obs.tick().saturating_add(450));
        log::info!("auto play: unload IFV {} to create mechanic seat",carrier.entity_id);
    }
}

/// Boarding one nearby reserve soldier into IFV (Gunner) or Battle Fortress
/// (OpenTopped) changes the vehicle's weapon behaviour in the *native* engine.
/// Unlike ordinary APC delivery, combat carriers must never auto-unload at
/// the enemy gate. Restrict boarding to spare defenders to avoid dismantling
/// an assault wave — a known regression from generic carrier logic.
pub(super) fn combat_carriers(brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox) {
    if brain.director.recovering_income() || brain.director.under_attack_now()
        || obs.tick() < brain.combat_board_after { return; }
    brain.repair_ifv_pending.retain(|_, until| *until > obs.tick());
    let reserves = brain.squads.squad(Role::Defend)
        .map(|s| s.members.clone()).unwrap_or_default();
    // Mechanics are a separate service, not two riflemen borrowed from the
    // defend squad. The ordinary combat boarding floor still applies below.
    let attackers = brain.squads.squad(Role::Attack)
        .map(|s| s.members.clone()).unwrap_or_default();
    // Once a mechanic is riding a repair IFV, another vehicle must not spend
    // the next engineer. This specialist remains behind the main tank wave.
    let repair_carrier_exists = brain.census.units.iter().any(|u| {
        obs.entity_type(rules, u.entity_id).is_some_and(|carrier_ty| {
            carrier_ty.gunner && brain.census.units.iter().any(|passenger| {
                obs.inside_transport(passenger.entity_id) == Some(u.entity_id)
                    && obs.entity_type(rules, passenger.entity_id).is_some_and(|kind|
                        gunner_repairs(rules, carrier_ty, kind.ifv_mode))
            })
        })
    });
    let pending_mechanic = brain.repair_ifv_pending.iter()
        .filter(|(id,until)|obs.tick()<**until && obs.owns_object(**id))
        .map(|(&id,_)|id).min();
    let mut aa_stock = None;
    for carrier in &brain.census.units {
        if !carrier.controllable() || carrier.in_service()
            || ordered_this_frame(out, carrier.entity_id) { continue; }
        let Some(ty) = obs.entity_type(rules, carrier.entity_id) else { continue; };
        if ty.category != ObjectCategory::Vehicle || ty.naval
            || !(ty.gunner || ty.open_topped) || ty.passengers <= 0
            || obs.passengers(carrier.entity_id) >= ty.passengers as usize
            || brain.tracked.pending_enterers(obs.tick(), carrier.entity_id)
                .any(|passenger| obs.inside_transport(passenger) != Some(carrier.entity_id))
        { continue; }
        // A gunner carrier's passenger IFVMode is the native weapon selector.
        // Prefer a mode not already represented in our loaded IFVs; a row of
        // identical rifleman IFVs throws away the mechanic the player values.
        // For an open-top fortress, longer-range infantry can fire from cover.
        let mut modes = std::collections::BTreeMap::<i32, usize>::new();
        if ty.gunner {
            for passenger in &brain.census.units {
                let Some(parent) = obs.inside_transport(passenger.entity_id) else { continue; };
                if !obs.entity_type(rules, parent).is_some_and(|kind| kind.gunner) {
                    continue;
                }
                if let Some(kind) = obs.entity_type(rules, passenger.entity_id) {
                    *modes.entry(kind.ifv_mode).or_default() += 1;
                }
            }
        }
        // An Engineer's native IFVMode selects a *negative-damage* repair
        // weapon, rather than a passenger rifle. Only take a spare engineer
        // when there is a damaged friendly vehicle to heal. The sole engineer
        // is reserved for capturing public assets, not consumed as a gunner.
        let stock = *aa_stock.get_or_insert_with(||super::composition::boarding_air_stock(brain,rules,obs));
        let wounded_vehicles = brain.census.units.iter().filter(|u| {
            u.health_permille < 850 && u.traits.combat
                && obs.entity_type(rules, u.entity_id)
                    .is_some_and(|kind| kind.category == ObjectCategory::Vehicle)
        }).count();
        let engineer_count = brain.census.units.iter().filter(|u| {
            u.traits.engineer && u.controllable() && !u.in_service()
        }).count();
        let tank_force = brain.census.units.iter().filter(|u| u.traits.combat
            && obs.entity_type(rules,u.entity_id).is_some_and(|kind|
                kind.category == ObjectCategory::Vehicle)).count();
        let prepare_repair = wounded_vehicles > 0
            || (tank_force >= 5 && obs.credits() >= 3_000);
        let home = brain.census.base_cell.is_some_and(|base|
            squared_distance(base, carrier.cell) <= 18 * 18);
        // Prefer one specialist chassis. Keep its seat free until a second
        // engineer is trained: otherwise normal reserve infantry fills it
        // with riflemen first, and the mechanic never gets a slot.
        let selected_repair_carrier = ty.gunner && !repair_carrier_exists
            && (prepare_repair || pending_mechanic==Some(carrier.entity_id)) && home
            && super::composition::repair_carrier_allowed(brain,rules,obs,carrier,stock)
            && match pending_mechanic {
                Some(id) => id==carrier.entity_id,
                None => brain.census.units.iter().filter(|unit| {
                    unit.controllable() && !unit.in_service()
                        && obs.entity_type(rules, unit.entity_id).is_some_and(|kind| kind.gunner)
                        && super::composition::repair_carrier_allowed(brain,rules,obs,unit,stock)
                        && brain.census.base_cell.is_some_and(|base|
                            squared_distance(base, unit.cell) <= 18 * 18)
                }).filter(|unit| obs.passengers(unit.entity_id) == 0)
                  .map(|unit| unit.entity_id).min() == Some(carrier.entity_id),
            };
        if selected_repair_carrier && engineer_count < 2 { continue; }
        if selected_repair_carrier
            && engineer_count >= 2
            && (!attackers.contains(&carrier.entity_id)
                || brain.census.base_cell.is_some_and(|home|
                    squared_distance(home, carrier.cell) <= 18 * 18))
            && obs.attack_target(carrier.entity_id).is_none() {
            let engineer = brain.census.units.iter()
                .filter(|u| u.traits.engineer && u.controllable() && !u.in_service()
                    && !ordered_this_frame(out, u.entity_id)
                    && !brain.tracked.held(obs.tick(), u.entity_id)
                    && obs.can_enter_transport(rules, u.entity_id, carrier.entity_id)
                    && squared_distance(u.cell, carrier.cell) <= 24 * 24)
                .filter(|u| obs.entity_type(rules, u.entity_id)
                    .is_some_and(|engineer_ty| gunner_repairs(rules, ty, engineer_ty.ifv_mode)))
                .min_by_key(|u| (squared_distance(u.cell, carrier.cell), u.entity_id));
            if let Some(engineer) = engineer {
                let near = squared_distance(engineer.cell, carrier.cell)
                    <= CARRIER_BOARD_RANGE * CARRIER_BOARD_RANGE;
                let issued = if near {
                    issue_tracked(&mut brain.tracked, brain.trace, out, rules, obs,
                        engineer.entity_id, TrackedKind::Enter, Some(carrier.entity_id),
                        Some(carrier.cell), Goal::Enter { target: carrier.entity_id })
                } else if !brain.tracked.held(obs.tick(), carrier.entity_id)
                    && !obs.visible_enemy_entities().any(|enemy|
                        squared_distance(enemy.cell, engineer.cell) <= 8 * 8) {
                    // The IFV is faster than a fragile engineer. Drive the
                    // empty carrier to the protected infantry instead of
                    // asking an engineer to cross the battlefield alone.
                    issue_tracked(&mut brain.tracked, brain.trace, out, rules, obs,
                        carrier.entity_id, TrackedKind::Move, None,
                        Some(engineer.cell), Goal::Move { cell: engineer.cell })
                } else { false };
                if issued {
                    brain.combat_board_after = obs.tick().saturating_add(CARRIER_RETRY);
                    brain.repair_ifv_pending.insert(carrier.entity_id,
                        obs.tick().saturating_add(300));
                    log::info!("auto play: engineer {} repair IFV rendezvous {} near={near}",
                        engineer.entity_id, carrier.entity_id);
                    break;
                }
                // Reserve the free Gunner seat for the requested engineer;
                // do not fill it with a rifleman in this same frame.
                continue;
            }
        }
        // Never immediately fill a selected mechanic seat with a GI when the
        // engineer is not within reach this frame. The long rendezvous is a
        // mission, not an invitation to change the chassis weapon again.
        if selected_repair_carrier || brain.repair_ifv_pending.contains_key(&carrier.entity_id)
            || reserves.len() < 2
            || (ty.gunner && super::composition::preserve_air_carrier(rules,obs,carrier,stock)) {
            continue;
        }
        // A Battle-Fortress style platform gets no firepower from seats that
        // never fill. Bring the *vehicle* to nearby reserved infantry while
        // safe, then use the native Enter order in range. Never march an
        // unsupported rifleman across an active battle to a front-line hull.
        if ty.open_topped && obs.passengers(carrier.entity_id) < ty.passengers as usize
            && !brain.director.under_attack_now()
            && brain.census.base_cell.is_some_and(|home|
                squared_distance(home, carrier.cell) <= 20 * 20)
            && !brain.tracked.held(obs.tick(), carrier.entity_id)
        {
            let rendezvous = brain.census.units.iter()
                .filter(|inf| inf.slot == super::census::Slot::Infantry
                    && reserves.contains(&inf.entity_id) && inf.controllable()
                    && inf.traits.combat && !inf.in_service()
                    && !ordered_this_frame(out, inf.entity_id)
                    && !brain.tracked.held(obs.tick(), inf.entity_id)
                    && obs.can_enter_transport(rules, inf.entity_id, carrier.entity_id)
                    && squared_distance(inf.cell, carrier.cell) <= 18 * 18
                    && squared_distance(inf.cell, carrier.cell) >
                        CARRIER_BOARD_RANGE * CARRIER_BOARD_RANGE)
                .filter(|inf| !obs.visible_enemy_entities().any(|enemy|
                    squared_distance(inf.cell, enemy.cell) <= 10 * 10))
                .filter(|inf| obs.can_reach(ty.movement_zone, carrier.cell, inf.cell))
                .min_by_key(|inf| (squared_distance(inf.cell, carrier.cell), inf.entity_id));
            if let Some(inf) = rendezvous
                && issue_tracked(&mut brain.tracked, brain.trace, out, rules, obs,
                    carrier.entity_id, TrackedKind::Move, None, Some(inf.cell),
                    Goal::Move { cell: inf.cell })
            {
                brain.combat_board_after = obs.tick().saturating_add(CARRIER_RETRY);
                continue;
            }
        }
        let soldiers = brain.census.units.iter()
            .filter(|unit| unit.slot == super::census::Slot::Infantry
                && reserves.contains(&unit.entity_id) && unit.controllable()
                && unit.traits.combat && !unit.traits.engineer && !unit.traits.spy
                && !unit.in_service() && !ordered_this_frame(out, unit.entity_id)
                && !brain.tracked.held(obs.tick(), unit.entity_id)
                && obs.can_enter_transport(rules, unit.entity_id, carrier.entity_id)
                && squared_distance(carrier.cell, unit.cell)
                    <= CARRIER_BOARD_RANGE * CARRIER_BOARD_RANGE)
            // Empty seats are preferable to loading someone who contributes
            // no safe, real damage. Suicide and negative-damage modes require
            // a specialist task instead of being treated as generic guns.
            .filter(|unit| obs.entity_type(rules, unit.entity_id).is_some_and(|passenger|
                super::combinations::passenger_value(rules, obs, ty, passenger, carrier.cell) > 0));
        let soldier = soldiers.max_by_key(|unit| {
            let passenger = obs.entity_type(rules, unit.entity_id);
            let mode = passenger.map_or(0, |kind| kind.ifv_mode);
            let value = passenger.map_or(0, |kind|
                super::combinations::passenger_value(rules, obs, ty, kind, carrier.cell));
            // Repetition is useful only if it answers current threats; a
            // distinctive weapon mode should win a close tie, not override a
            // clearly superior anti-air/anti-armour configuration.
            let diversity = if ty.gunner {
                i64::try_from(modes.get(&mode).copied().unwrap_or(0))
                    .unwrap_or(i64::MAX).saturating_mul(20)
            } else { 0 };
            (value.saturating_sub(diversity),
                -i64::from(squared_distance(unit.cell, carrier.cell)),
                std::cmp::Reverse(unit.entity_id))
        });
        let Some(soldier) = soldier else { continue; };
        if issue_tracked(&mut brain.tracked, brain.trace, out, rules, obs,
            soldier.entity_id, TrackedKind::Enter, Some(carrier.entity_id), Some(carrier.cell),
            Goal::Enter { target: carrier.entity_id }) {
            brain.combat_board_after = obs.tick().saturating_add(CARRIER_RETRY);
            break;
        }
    }
}

/// Native Gunner mode points to the carrier's weapon slot, not to the
/// Engineer's own primary weapon. Negative damage is the normal repair
/// bullet; this also covers a mod's equivalent mode without a type-name list.
pub(super) fn gunner_repairs(
    rules: &RuleSet, carrier: &crate::rules::object_type::ObjectType, mode: i32,
) -> bool {
    usize::try_from(mode).ok().and_then(|slot| carrier.weapon_at(slot))
        .and_then(|name| rules.weapon(name))
        .is_some_and(|weapon| weapon.damage < 0)
}

/// A loaded engineer IFV is a workshop, not an expendable lone assault car.
pub(super) fn repair_ifv_loaded(
    rules: &RuleSet, obs: &Observation<'_>, census: &super::census::Census,
    carrier: u64,
) -> bool {
    let Some(ty) = obs.entity_type(rules, carrier) else { return false; };
    ty.gunner && census.units.iter().any(|passenger| {
        obs.inside_transport(passenger.entity_id) == Some(carrier)
            && obs.entity_type(rules, passenger.entity_id).is_some_and(|kind|
                gunner_repairs(rules, ty, kind.ifv_mode))
    })
}

/// Repair IFVs need to *stop* next to a damaged tank and enter native Guard
/// so the healer's short Guard scan can choose it. Repeated Move orders would
/// continuously reset that scan. No direct health edits are ever made.
pub(super) fn field_healer(rules:&RuleSet,obs:&Observation<'_>,actor:u64)->bool {
    let Some(ty)=obs.entity_type(rules,actor) else{return false;};
    matches!(ty.category,ObjectCategory::Infantry|ObjectCategory::Vehicle)
        && !ty.spawned && ty.speed>0
        && (0..ty.weapon_record_count()).filter_map(|slot|obs.observed_actor_weapon(rules,actor,slot))
            .any(|w|w.damage<0 && !w.never_use)
        && !(0..ty.weapon_record_count()).filter_map(|slot|obs.observed_actor_weapon(rules,actor,slot))
            .any(|w|w.damage>0 && !w.never_use)
}

pub(super) fn field_heal_compatible(rules:&RuleSet,obs:&Observation<'_>,healer:u64,target:u64)->bool {
    if healer==target || !obs.owns_object(healer) || !obs.owns_object(target) {return false;}
    let Some(ty)=obs.entity_type(rules,healer) else{return false;};
    let Some(victim)=obs.entity_type(rules,target) else{return false;};
    matches!(victim.category,ObjectCategory::Infantry|ObjectCategory::Vehicle)
        && (0..ty.weapon_record_count()).filter_map(|slot|obs.observed_actor_weapon(rules,healer,slot))
            .any(|w|w.damage<0 && !w.never_use && rules.weapon_projectile(w).is_some_and(|p|p.ag)
                && w.warhead.as_deref().and_then(|name|rules.warhead(name)).is_some_and(|head|
                    f64::from_bits(head.interaction(victim.armor_id()).multiplier().bits())>0.0))
}

pub(super) fn repair_ifvs(
    brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox,
) {
    let now=obs.tick();
    if now%15!=0 {return;}
    brain.repair_ifv_after.retain(|_,until|now<*until);
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    let mut claimed=std::collections::BTreeSet::new();
    for carrier in &brain.census.units {
        let id=carrier.entity_id;
        if !carrier.controllable() || carrier.in_service()
            || brain.safety.reserved(rules,obs,carrier)
            || ordered_this_frame(out,id) || brain.tracked.held(now,id)
            || brain.repair_ifv_after.contains_key(&id)
            || brain.tracked.tasks.get(id).is_some()
            || brain.census.factory_handoffs.contains_key(&id)
            || !field_healer(rules,obs,id) {continue;}
        let Some(ty)=obs.entity_type(rules,id) else{continue;};
        let guns=super::safety::envelopes(rules,obs,id,&visible,true);
        // Repair a safe, stationary patient first, not a healthy vehicle
        // currently marching into battle. The native Guard healer scan
        // still owns the actual shot/repair, never set the patient's HP.
        let wounded=brain.census.units.iter().filter(|u|
            u.controllable() && u.entity_id!=id && !u.in_service()
            && u.health_permille>0 && u.health_permille<950
            && !brain.safety.field_retry(u.entity_id,now)
            && !claimed.contains(&u.entity_id)
            && !obs.own_repair_service(rules,u.entity_id)
            && obs.inside_transport(u.entity_id).is_none()
            && squared_distance(u.cell,carrier.cell)<=28*28
            && field_heal_compatible(rules,obs,id,u.entity_id)
            && (brain.safety.reserved(rules,obs,u)
                || (obs.attack_target(u.entity_id).is_none() && obs.destination(u.entity_id).is_none()))
            && super::safety::safe_leg(&guns,carrier.cell,u.cell,false)
            && !visible.iter().any(|e|squared_distance(e.cell,u.cell)<=6*6
                && obs.can_hurt(rules,e.entity_id,u.entity_id)))
            .min_by_key(|u|(!brain.charge_rotating.contains_key(&u.entity_id),
                !obs.own_elite(u.entity_id),u.health_permille,
                squared_distance(u.cell,carrier.cell),u.entity_id));
        let Some(patient)=wounded else{continue;};
        let near=squared_distance(carrier.cell,patient.cell)<=2*2;
        let (goal,post)=if near {
            (Goal::Guard,None)
        }else{
            let post=[(1,0),(-1,0),(0,1),(0,-1),(1,1),(-1,1),(1,-1),(-1,-1)]
                .into_iter().filter_map(|(dx,dy)|{
                    let (Ok(x),Ok(y))=(u16::try_from(i32::from(patient.cell.0)+dx),
                        u16::try_from(i32::from(patient.cell.1)+dy)) else{return None;};
                    let cell=(x,y);
                    (obs.is_cell_in_playfield(x,y) && obs.is_ground_walkable(x,y)
                        && obs.can_reach(ty.movement_zone,carrier.cell,cell)
                        && super::safety::safe_leg(&guns,carrier.cell,cell,false))
                        .then_some(cell)
                }).min_by_key(|cell|{
                    let occupied=brain.census.units.iter().filter(|u|u.entity_id!=id
                        && squared_distance(u.cell,*cell)<=1).count();
                    (occupied,squared_distance(carrier.cell,*cell),*cell)
                });
            let Some(post)=post else{continue;};
            (Goal::GuardAt{cell:post},Some(post))
        };
        if issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,
            id,TrackedKind::Guard,None,post,goal) {
            brain.repair_ifv_after.insert(id,now.saturating_add(if near {120}else{180}));
            claimed.insert(patient.entity_id);
            log::debug!("auto play: v63_field_healer tick={now} actor={id} patient={} hp={}",
                patient.entity_id,patient.health_permille);
            if claimed.len()>=3 {break;}
        }
    }
}

/// Siege Choppers trade mobility for artillery reach when they find a secure
/// firing position near a *visible* enemy building. Never deploy into nearby
/// enemy fire or on an unknown enemy position. Native deploy owns the anim and
/// weapon swap; this pass only presses the player's D-key equivalent.
pub(super) fn siege_artillery(brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox) {
    for unit in &brain.census.units {
        if !unit.controllable() || unit.in_service()
            || brain.joint_ops.reserves(unit.entity_id)
            || ordered_this_frame(out, unit.entity_id) { continue; }
        let Some(ty) = obs.entity_type(rules, unit.entity_id) else { continue; };
        if !(ty.is_simple_deployer && ty.deploy_to_land) { continue; }
        if brain.siege_toggle_after.get(&unit.entity_id).is_some_and(|until| *until > obs.tick()) {
            continue;
        }
        let danger = obs.visible_enemy_entities().any(|enemy|
            squared_distance(unit.cell, enemy.cell) <= SIEGE_DANGER_RANGE * SIEGE_DANGER_RANGE);
        let objective = obs.visible_enemy_entities().any(|enemy|
            obs.entity_type(rules, enemy.entity_id)
                .is_some_and(|target| target.category == ObjectCategory::Building)
            && squared_distance(unit.cell, enemy.cell) <= SIEGE_DEPLOY_RANGE * SIEGE_DEPLOY_RANGE);
        let deployed = obs.is_deployed(unit.entity_id);
        // Debounce both directions; only undeploy when clearly out of combat.
        let toggle = (!deployed && objective && !danger)
            || (deployed && !objective && !danger);
        if !toggle { continue; }
        if issue_tracked(&mut brain.tracked, brain.trace, out, rules, obs,
            unit.entity_id, TrackedKind::Deploy, None, None, Goal::Deploy) {
            brain.siege_toggle_after.insert(unit.entity_id,
                obs.tick().saturating_add(SIEGE_LOCK_FRAMES));
            break;
        }
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    #[test]
    fn v62_a_rescue_attempt_cannot_indefinitely_extend_victim_protection() {
        let mut windows=std::collections::BTreeMap::new();
        open_liberation_window(&mut windows,9,10,100);
        open_liberation_window(&mut windows,9,10,200);
        assert_eq!(windows.get(&9),Some(&(10,100+LIBERATION_WINDOW)));
    }

    #[test]
    fn aircraft_trigger_home_alert_before_ground_units() {
        assert_eq!(alarm_radius(false), BASE_ALERT_RADIUS);
        assert_eq!(alarm_radius(true), AIR_ALERT_RADIUS);
        assert!(alarm_radius(true) > alarm_radius(false));
    }

    #[test]
    fn retail_gunner_and_open_topped_vehicles_use_native_passenger_modes() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        let ifv = rules.object("FV").expect("retail IFV");
        let fortress = rules.object("BFRT").expect("retail battle fortress");
        let chopper = rules.object("SCHP").expect("retail siege chopper");
        assert!(ifv.gunner && ifv.passengers > 0);
        assert!(fortress.open_topped && fortress.passengers > 0);
        assert!(chopper.is_simple_deployer && chopper.deploy_to_land);
    }

    #[test]
    fn engineer_ifv_mode_selects_a_native_healing_weapon() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        let ifv = rules.object("FV").expect("IFV");
        let engineer = rules.object("ENGINEER").expect("Engineer");
        assert!(gunner_repairs(&rules, ifv, engineer.ifv_mode));
        let rifleman = rules.object("E1").expect("GI");
        assert!(!gunner_repairs(&rules, ifv, rifleman.ifv_mode));
    }

    #[test]
    fn prizes_include_tech_and_income_but_not_any_building() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        assert!(guarded_prize(&rules, rules.object("GATECH").expect("battle lab")));
        assert!(guarded_prize(&rules, rules.object("CAOILD").expect("oil derrick")));
    }

    #[test]
    fn caps_keep_specialist_micro_from_flooding_the_order_budget() {
        assert!(SIEGE_LOCK_FRAMES > CARRIER_RETRY);
        assert!(BASE_ALERT_RADIUS < DEFENDER_REACH);
        assert!(CARRIER_BOARD_RANGE < DEFENDER_REACH);
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v30_airborne_detection_tests {
    use super::*;

    #[test]
    fn balloon_hover_is_anti_air_even_when_declared_as_vehicle() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        let kirov = rules.object("ZEP").expect("kirov");
        let tank = rules.object("APOC").expect("apocalypse tank");
        assert!(airborne(kirov, false));
        assert!(!airborne(tank, false));
        assert!(super::super::tactics::emergency_threat_score(&rules, kirov) >= 900);
        assert!(super::super::tactics::emergency_threat_score(
            &rules, rules.object("TANY").expect("C4 commando")) >= 900);
    }
}
