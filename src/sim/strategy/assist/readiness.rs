//! Project the actual ownership/availability of the living army.
//!
//! No independent task state, no new command, no enemy truth. The raw census
//! remains the historical HP series for loss detection. This view is rebuilt
//! before strategic decisions and after the existing specialist owners run.
use std::collections::BTreeSet;

use crate::rules::ruleset::RuleSet;
use crate::sim::control::observation::Observation;
use super::{AssistBrain, census::Slot, squads::Role};

pub(super) fn refresh(brain: &mut AssistBrain, rules: &RuleSet, obs: &Observation<'_>) {
    let now = obs.tick();
    let mut unavailable = BTreeSet::new();
    unavailable.extend(brain.crate_claims.keys().copied());
    unavailable.extend(brain.crate_returns.keys().copied());
    unavailable.extend(brain.specialist_shuttles.keys().copied());
    unavailable.extend(brain.specialist_shuttles.values().map(|trip| trip.passenger));
    let mut borrowed: BTreeSet<_> = brain.escorts.values().map(|escort| escort.guard).collect();
    borrowed.extend(brain.asset_guards.keys().copied());
    borrowed.extend(brain.outpost_guards.keys().copied());
    unavailable.extend(brain.tracked.tasks.entries().map(|(actor, _)| actor));
    unavailable.extend(brain.repair_ifv_pending.iter().filter(|(_, until)| now < **until)
        .map(|(id, _)| *id));
    let mut detached = BTreeSet::new();
    for unit in brain.census.combat_units().filter(|unit| unit.controllable()) {
        let id = unit.entity_id;
        if brain.joint_ops.reserves(id) { detached.insert(id); }
        if brain.asset_ops.has_muster(id) { borrowed.insert(id); }
        if brain.safety.reserved(rules, obs, unit) || brain.tracked.held(now, id)
            || brain.avoid_retreat_until.get(&id).is_some_and(|until| now < *until)
            || (unit.slot == Slot::Aircraft && obs.out_of_ammo(id))
            || (unit.slot == Slot::Vehicle && obs.active_gunner_weapon(rules, id)
                .is_some_and(|weapon| weapon.damage < 0))
            || (unit.slot == Slot::Vehicle && obs.own_repair_service(rules, id))
        { unavailable.insert(id); }
    }
    // A dead, human-controlled or recovering guard must not keep an old
    // lease alive solely because the specialist table has not reconciled.
    let borrowed_defenders: BTreeSet<_> = brain.census.combat_units()
        .filter(|u| u.controllable() && !u.in_service() && u.points > 0
            && !unavailable.contains(&u.entity_id) && borrowed.contains(&u.entity_id))
        .map(|u| u.entity_id).collect();
    unavailable.extend(borrowed.iter().copied());
    brain.census.unavailable = unavailable;
    brain.census.detached = detached;
    brain.census.borrowed_defenders = borrowed_defenders;
}

pub(super) fn attack_members(brain: &AssistBrain) -> usize {
    brain.squads.squad(Role::Attack).map_or(0, |team| brain.census.units.iter()
        .filter(|unit| team.members.contains(&unit.entity_id)
            && brain.census.main_available(unit)).count())
}

/// The same log cadence as the historical census. A missing field in an
/// older run is not evidence of a zero strength deficit.
pub(super) fn report(brain: &AssistBrain, tick: u64) {
    if tick % 600 != 0 { return; }
    log::info!("auto play: readiness tick={} roster={} available={} raw_hp={} available_hp={} attack={}",
        tick, brain.census.fighters, brain.census.available_fighters(),
        brain.census.army_points(), brain.census.available_points(), attack_members(brain));
}
