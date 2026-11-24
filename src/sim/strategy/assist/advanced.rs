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

/// Highest priority local response, without pulling the remote attack squad
/// away from its objective. Defenders choose a visible target that actually
/// threatens a yard, mine or harvester; they do not shoot neutral property.
pub(super) fn protect_base(brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox) {
    let sites: Vec<(u16, u16)> = brain.census.yard_cells.iter()
        .chain(brain.census.refinery_cells.iter()).copied().collect();
    if sites.is_empty() { return; }
    let threats: Vec<_> = obs.visible_enemy_entities()
        .filter(|enemy| obs.entity_type(rules, enemy.entity_id)
            .is_some_and(|ty| ty.category != ObjectCategory::Building
                || ((0..8).any(|slot| ty.weapon_at(slot).is_some())
                    && obs.attack_target(enemy.entity_id).is_some())))
        .filter(|enemy| sites.iter().any(|site| {
            let air = obs.entity_type(rules, enemy.entity_id).is_some_and(|ty|
                ty.category == ObjectCategory::Aircraft || (ty.jumpjet && !obs.is_deployed(enemy.entity_id)));
            // A mind-controller creeping toward the yard is an emergency
            // before it enters ordinary tank weapon range; once it has stolen
            // a defender, the stolen tank is a threat of its own.
            let psychic = obs.entity_type(rules, enemy.entity_id).is_some_and(|ty|
                super::tactics::mind_control_reach(rules, ty).is_some());
            let radius = if psychic { alarm_radius(air).max(24) }
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
            squared_distance(*site, defender.cell) <= 17 * 17);
        let local_reserve = !attackers.contains(&defender.entity_id)
            && local && obs.attack_target(defender.entity_id).is_none();
        // A soldier assigned to Attack but still standing *inside* the base
        // cannot watch a harvester or yard get destroyed. A real nearby raid
        // temporarily outranks its stale push role; never recall the remote
        // attacking fist or overwrite an ongoing firefight.
        let emergency_loan = attackers.contains(&defender.entity_id)
            && local && obs.attack_target(defender.entity_id).is_none()
            && threats.iter().any(|enemy| sites.iter().any(|site| {
                let air = obs.entity_type(rules, enemy.entity_id).is_some_and(|ty|
                    ty.category == ObjectCategory::Aircraft || ty.jumpjet);
                squared_distance(*site, enemy.cell) <= alarm_radius(air).pow(2)
            }));
        // Anti-air must interrupt even an already fighting base garrison when
        // a bomber approaches; ground-only fighters are not diverted to sky.
        let air_emergency = local && threats.iter().any(|enemy| {
            obs.entity_type(rules,enemy.entity_id).is_some_and(|ty|
                ty.category == ObjectCategory::Aircraft || ty.jumpjet)
                && obs.can_engage(rules, defender.entity_id, enemy.entity_id)
                && obs.can_hurt(rules, defender.entity_id, enemy.entity_id)
        });
        if !(defenders.contains(&defender.entity_id) || local_reserve
            || emergency_loan || air_emergency)
            || !defender.controllable()
            || defender.in_service() || !defender.traits.combat
            || ordered_this_frame(out, defender.entity_id)
            || (brain.tracked.held(obs.tick(), defender.entity_id)
                && !emergency_loan && !air_emergency
                && !(local && obs.attack_target(defender.entity_id).is_none()))
        { continue; }
        let target = threats.iter()
            .filter(|enemy| squared_distance(defender.cell, enemy.cell)
                <= DEFENDER_REACH * DEFENDER_REACH)
            .filter(|enemy| obs.can_engage(rules, defender.entity_id, enemy.entity_id)
                && obs.can_hurt(rules, defender.entity_id, enemy.entity_id))
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
                let (airborne, controller) = obs.entity_type(rules, enemy.entity_id)
                    .map(|ty| (ty.category == ObjectCategory::Aircraft || ty.jumpjet,
                        super::tactics::mind_control_reach(rules, ty).is_some()))
                    .unwrap_or((false, false));
                (!airborne, !controller, !parasite_weapon, !attacks_asset, sites.iter()
                    .map(|site| squared_distance(*site, enemy.cell)).min().unwrap_or(i32::MAX),
                    enemy.entity_id)
            });
        let Some(target) = target else { continue; };
        if issue_tracked(&mut brain.tracked, brain.trace, out, rules, obs,
            defender.entity_id, TrackedKind::Engage, Some(target.entity_id), Some(target.cell),
            Goal::Attack { target: target.entity_id }) { issued += 1; }
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
        if !defenders.contains(&unit.entity_id) || !unit.controllable()
            || !unit.traits.combat || unit.in_service()
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
pub(super) fn secure_captured_assets(
    brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox,
) {
    if brain.director.recovering_income() || brain.director.under_attack_now() { return; }
    let reserves = brain.squads.squad(Role::Defend)
        .map(|s| s.members.clone()).unwrap_or_default();
    if reserves.len() < 5 { return; }
    let mut assets: Vec<_> = brain.captured_assets.keys().filter_map(|id| {
        let building = obs.own_entities().find(|e| e.entity_id == *id)?;
        let ty = obs.entity_type(rules, *id)?;
        (ty.category == ObjectCategory::Building && guarded_prize(rules, ty))
            .then_some((*id, building.cell, high_value_prize(rules, ty)))
    }).collect();
    assets.sort_by_key(|(id, _, priority)| (!*priority, *id));
    let live = &brain.census.units;
    brain.asset_guards.retain(|guard, asset| {
        assets.iter().any(|(id,_,_)| *id == *asset)
            && reserves.contains(guard)
            && live.iter().any(|u| u.entity_id == *guard && u.controllable()
                && u.traits.combat && !u.in_service())
    });
    for (id, cell, priority) in assets {
        let desired: usize = if priority { 2 } else { 1 };
        let assigned = brain.asset_guards.values().filter(|value| **value == id).count();
        // An existing local combat unit already provides a shield; do not
        // waste several reserve guards marching to the same captured site.
        let local = live.iter().filter(|u| u.traits.combat && u.controllable()
            && brain.asset_guards.get(&u.entity_id) != Some(&id)
            && squared_distance(u.cell, cell) <= 7 * 7).count();
        if assigned + local >= desired || brain.asset_guards.len() >= 3
            || reserves.len().saturating_sub(brain.asset_guards.len()
                + brain.outpost_guards.len()) <= 4 { continue; }
        let candidate = live.iter()
            .filter(|u| reserves.contains(&u.entity_id) && u.controllable()
                && u.traits.combat && !u.in_service()
                && !brain.outpost_guards.contains_key(&u.entity_id)
                && !brain.asset_guards.contains_key(&u.entity_id)
                && !ordered_this_frame(out, u.entity_id)
                && !brain.tracked.held(obs.tick(), u.entity_id)
                && obs.attack_target(u.entity_id).is_none())
            .min_by_key(|u| (squared_distance(u.cell, cell), u.entity_id));
        let Some(unit) = candidate else { continue; };
        let Some(ty) = obs.entity_type(rules, unit.entity_id) else { continue; };
        // Stand around the foundation. A lab is not a civilian garrison and
        // cannot accept an Enter command merely because it is ours.
        let offsets = [(3,0),(-3,0),(0,3),(0,-3),(2,2),(2,-2),(-2,2),(-2,-2)];
        let post = offsets.into_iter().filter_map(|(dx,dy)| {
            let (Ok(x),Ok(y)) = (u16::try_from(i32::from(cell.0)+dx),
                u16::try_from(i32::from(cell.1)+dy)) else { return None; };
            let post = (x,y);
            (obs.is_cell_in_playfield(x,y) && obs.is_ground_walkable(x,y)
                && (!obs.has_zone_grid()
                    || obs.can_reach(ty.movement_zone, unit.cell, post))
                && !obs.visible_enemy_entities().any(|enemy|
                    squared_distance(enemy.cell, post) <= 8 * 8)).then_some(post)
        }).min_by_key(|post| squared_distance(unit.cell, *post));
        let Some(post) = post else { continue; };
        if issue_tracked(&mut brain.tracked, brain.trace, out, rules, obs,
            unit.entity_id, TrackedKind::Guard, None, Some(post),
            Goal::GuardAt { cell: post }) {
            brain.asset_guards.insert(unit.entity_id, id);
            log::debug!("auto play: assign defender {} to captured asset {id} at {post:?}", unit.entity_id);
            // Keep order budgets small; further sites receive the next frame.
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

/// A garrison is valuable cover, so do not empty it just because a scout
/// passes by. If an owned occupied building is being attacked by a vehicle
/// and nobody nearby can help, a controlled evacuation lets its passengers
/// use their normal weapons or escape. Neutral garrisons do not grant ownership
/// of the building, so the normal command gate may refuse them.
pub(super) fn relieve_garrison(brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox) {
    if !brain.director.under_attack_now() { return; }
    for site in obs.own_entities() {
        if !site.orderable || obs.passengers(site.entity_id) == 0
            || obs.owned_by_hand(site.entity_id)
            || obs.tick() < brain.garrison_release_after.get(&site.entity_id)
                .copied().unwrap_or(0) { continue; }
        if obs.entity_type(rules, site.entity_id)
            .is_none_or(|ty| ty.category != ObjectCategory::Building) { continue; }
        let attacked = obs.visible_enemy_entities().any(|enemy|
            obs.attack_target(enemy.entity_id) == Some(site.entity_id)
            && squared_distance(site.cell, enemy.cell) <= 10 * 10);
        if !attacked { continue; }
        // Do not discard the benefit of the firing ports if mobile defenders
        // are already answering this threat.
        let close_mobiles = brain.census.units.iter()
            .filter(|unit| unit.traits.combat && unit.controllable()
                && squared_distance(site.cell, unit.cell) <= 8 * 8).count();
        if close_mobiles >= 2 { continue; }
        if issue_tracked(&mut brain.tracked, brain.trace, out, rules, obs,
            site.entity_id, TrackedKind::Enter, None, None, Goal::Unload) {
            brain.garrison_release_after.insert(site.entity_id, obs.tick() + 450);
            break;
        }
    }
}

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
        repair_ifv_loaded(rules, obs, &brain.census, unit.entity_id)) { return; }
    let has_empty_chassis = brain.census.units.iter().any(|unit| {
        obs.entity_type(rules, unit.entity_id).is_some_and(|ty| ty.gunner)
            && obs.passengers(unit.entity_id) == 0
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
    if reserves.len() < 2 { return; }
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
            && prepare_repair && home
            && brain.census.units.iter().filter(|unit| {
                obs.entity_type(rules, unit.entity_id).is_some_and(|kind| kind.gunner)
                    && brain.census.base_cell.is_some_and(|base|
                        squared_distance(base, unit.cell) <= 18 * 18)
            }).filter(|unit| obs.passengers(unit.entity_id) == 0)
              .map(|unit| unit.entity_id).min() == Some(carrier.entity_id);
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
        if selected_repair_carrier || brain.repair_ifv_pending.contains_key(&carrier.entity_id) {
            continue;
        }
        let soldiers = brain.census.units.iter()
            .filter(|unit| unit.slot == super::census::Slot::Infantry
                && reserves.contains(&unit.entity_id) && unit.controllable()
                && unit.traits.combat && !unit.traits.engineer && !unit.traits.spy
                && !unit.in_service() && !ordered_this_frame(out, unit.entity_id)
                && !brain.tracked.held(obs.tick(), unit.entity_id)
                && obs.can_enter_transport(rules, unit.entity_id, carrier.entity_id)
                && squared_distance(carrier.cell, unit.cell)
                    <= CARRIER_BOARD_RANGE * CARRIER_BOARD_RANGE);
        let soldier = soldiers.min_by_key(|unit| {
            let mode = obs.entity_type(rules, unit.entity_id)
                .map_or(0, |kind| kind.ifv_mode);
            let range = obs.entity_type(rules, unit.entity_id)
                .and_then(|kind| kind.weapon_at(0))
                .and_then(|name| rules.weapon(name))
                .map_or(0, |weapon| weapon.range_leptons);
            let mode_count = modes.get(&mode).copied().unwrap_or(0);
            (if ty.gunner { mode_count } else { 0 },
                if ty.open_topped { -range } else { 0 },
                squared_distance(unit.cell, carrier.cell), unit.entity_id)
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
pub(super) fn repair_ifvs(
    brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox,
) {
    if brain.director.under_attack_now() { return; }
    brain.repair_ifv_after.retain(|_, until| *until > obs.tick());
    for carrier in &brain.census.units {
        if !carrier.controllable() || carrier.in_service()
            || ordered_this_frame(out, carrier.entity_id)
            || brain.repair_ifv_after.contains_key(&carrier.entity_id)
            || obs.attack_target(carrier.entity_id).is_some() { continue; }
        let Some(ty) = obs.entity_type(rules, carrier.entity_id) else { continue; };
        if ty.category != ObjectCategory::Vehicle || !ty.gunner { continue; }
        let has_repair_gunner = brain.census.units.iter().any(|passenger| {
            obs.inside_transport(passenger.entity_id) == Some(carrier.entity_id)
                && obs.entity_type(rules, passenger.entity_id)
                    .is_some_and(|kind| gunner_repairs(rules, ty, kind.ifv_mode))
        });
        if !has_repair_gunner { continue; }
        let wounded = brain.census.units.iter()
            .filter(|unit| unit.entity_id != carrier.entity_id
                && unit.health_permille < 850 && unit.health_permille > 0
                && obs.entity_type(rules, unit.entity_id)
                    .is_some_and(|kind| kind.category == ObjectCategory::Vehicle)
                && !unit.in_service())
            .min_by_key(|unit| (squared_distance(unit.cell, carrier.cell), unit.entity_id));
        let Some(wounded) = wounded else { continue; };
        if squared_distance(carrier.cell, wounded.cell) > 20 * 20 { continue; }
        let near = squared_distance(carrier.cell, wounded.cell) <= 2 * 2;
        let (goal, post) = if near {
            (Goal::Guard, None)
        } else {
            let post = [(1,0),(-1,0),(0,1),(0,-1),(1,1),(-1,1),(1,-1),(-1,-1)]
                .into_iter().filter_map(|(dx,dy)| {
                    let (Ok(x),Ok(y)) = (u16::try_from(i32::from(wounded.cell.0)+dx),
                        u16::try_from(i32::from(wounded.cell.1)+dy)) else { return None; };
                    (obs.is_cell_in_playfield(x,y) && obs.is_ground_walkable(x,y)
                        && obs.can_reach(ty.movement_zone, carrier.cell, (x,y))
                        && !obs.visible_enemy_entities().any(|enemy|
                            squared_distance(enemy.cell,(x,y)) <= 6*6)).then_some((x,y))
                }).min_by_key(|cell| squared_distance(carrier.cell,*cell));
            let Some(post) = post else { continue; };
            (Goal::GuardAt { cell: post }, Some(post))
        };
        if issue_tracked(&mut brain.tracked, brain.trace, out, rules, obs,
            carrier.entity_id, TrackedKind::Guard, None, post, goal) {
            brain.repair_ifv_after.insert(carrier.entity_id, obs.tick().saturating_add(240));
            break;
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
