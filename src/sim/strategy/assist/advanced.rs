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
const DEFENDER_REACH: i32 = 22;
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
            .is_some_and(|ty| ty.category != ObjectCategory::Building))
        .filter(|enemy| sites.iter().any(|site|
            squared_distance(*site, enemy.cell) <= BASE_ALERT_RADIUS * BASE_ALERT_RADIUS)
            || brain.census.units.iter().any(|miner| miner.traits.harvester
                && squared_distance(miner.cell, enemy.cell) <= 8 * 8))
        .collect();
    if threats.is_empty() { return; }
    let defenders = brain.squads.squad(Role::Defend)
        .map(|s| s.members.clone()).unwrap_or_default();
    let mut issued = 0;
    for defender in &brain.census.units {
        if issued >= 3 { break; }
        if !defenders.contains(&defender.entity_id) || !defender.controllable()
            || defender.in_service() || !defender.traits.combat
            || ordered_this_frame(out, defender.entity_id)
            || brain.tracked.held(obs.tick(), defender.entity_id) { continue; }
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
                (!parasite_weapon, !attacks_asset, sites.iter()
                    .map(|site| squared_distance(*site, enemy.cell)).min().unwrap_or(i32::MAX),
                    enemy.entity_id)
            });
        let Some(target) = target else { continue; };
        if issue_tracked(&mut brain.tracked, brain.trace, out, rules, obs,
            defender.entity_id, TrackedKind::Engage, Some(target.entity_id), Some(target.cell),
            Goal::Attack { target: target.entity_id }) { issued += 1; }
    }
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

/// Boarding one nearby reserve soldier into IFV (Gunner) or Battle Fortress
/// (OpenTopped) changes the vehicle's weapon behaviour in the *native* engine.
/// Unlike ordinary APC delivery, combat carriers must never auto-unload at
/// the enemy gate. Restrict boarding to spare defenders to avoid dismantling
/// an assault wave — a known regression from generic carrier logic.
pub(super) fn combat_carriers(brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox) {
    if brain.director.recovering_income() || brain.director.under_attack_now()
        || obs.tick() < brain.combat_board_after { return; }
    let reserves = brain.squads.squad(Role::Defend)
        .map(|s| s.members.clone()).unwrap_or_default();
    if reserves.len() < 4 { return; }
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn caps_keep_specialist_micro_from_flooding_the_order_budget() {
        assert!(SIEGE_LOCK_FRAMES > CARRIER_RETRY);
        assert!(BASE_ALERT_RADIUS < DEFENDER_REACH);
        assert!(CARRIER_BOARD_RANGE < DEFENDER_REACH);
    }
}
