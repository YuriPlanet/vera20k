//! Economically useful native grinder orders for already controlled prizes.
//!
//! An enemy unit does not become our property merely because we issued a
//! mind-control Attack. Require a live native controller owned by the house
//! and a real native `Grind` cursor for the exact unit/building pair. Never
//! sell an ordinary captured MCV, harvester or useful healthy combat unit.

use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::Observation;

use super::AssistBrain;
use super::orders::{issue_tracked, ordered_this_frame};
use super::tasks::JobKind;
use super::tracked::TrackedKind;
use super::squared_distance;

const MIN_REFUND: i32 = 80;
const MAX_ROUTE_CELLS: i32 = 44;
const GRIND_RETRY: u64 = 150;

/// An already controlled unit is a resource *option*, not automatically
/// disposable. A near-dead combat prize can be recycled, but a healthy tank
/// is usually worth more on the front than its partial refund. This filter
/// is based on live traits and the native refund, not unit IDs.
fn worth_recycling(controlled: bool, refund: i32, fighter: bool,
    health_permille: u16, valuable_role: bool) -> bool {
    controlled && refund >= MIN_REFUND && !valuable_role
        && (!fighter || health_permille <= 300)
}

pub(super) fn run(brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox) {
    // Do not empty captured forces in the middle of a base defense.
    // A struggling economy can benefit most from a low-risk refund.
    // Emergency base defence, not low income, pauses this worker.
    if brain.director.under_attack_now() || obs.tick() < brain.recycle_after { return; }
    let sites: Vec<_> = obs.own_entities()
        .filter(|site| site.orderable && obs.entity_type(rules,site.entity_id)
            .is_some_and(|ty| ty.category == ObjectCategory::Building && ty.grinding))
        .take(12).collect();
    if sites.is_empty() { return; }
    // Optional, low-frequency native mind-control of one unarmed neutral
    // vehicle. Do not draw the entire civilian map out of the fog, and do
    // not issue ForceAttack: the observed native weapon must be mind control.
    // A newly acquired prize becomes a normal house unit only after the
    // engine reports its new owner/controller on a later frame.
    if obs.tick() % 240 == 0 {
        let possible: Vec<_> = obs.visible_neutral_vehicles(rules).take(24).collect();
        for actor in &brain.census.units {
            if !actor.controllable() || actor.in_service()
                || ordered_this_frame(out, actor.entity_id)
                || brain.tracked.held(obs.tick(), actor.entity_id)
                || obs.attack_target(actor.entity_id).is_some()
                || brain.tracked.tasks.get(actor.entity_id).is_some() { continue; }
            let Some(ty) = obs.entity_type(rules,actor.entity_id) else { continue; };
            let Some(reach) = super::tactics::mind_control_reach(rules,ty)
            else { continue; };
            let candidate=possible.iter().filter(|neutral| {
                let distance=squared_distance(actor.cell,neutral.cell);
                distance <= reach.saturating_mul(reach).min(10 * 10)
                    && !super::tactics::unsafe_post(rules,obs,actor.entity_id,neutral.cell)
                    && !obs.visible_enemy_entities().any(|enemy|
                        squared_distance(enemy.cell,neutral.cell) <= 10 * 10)
                    && obs.can_control_neutral(rules,actor.entity_id,neutral.entity_id)
            }).min_by_key(|neutral| (squared_distance(actor.cell,neutral.cell), neutral.entity_id));
            if let Some(target)=candidate
                && issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,
                    actor.entity_id,TrackedKind::Attack,Some(target.entity_id),
                    Some(target.cell),Goal::Attack { target: target.entity_id })
            {
                brain.recycle_after=obs.tick().saturating_add(240);
                return;
            }
        }
    }
    let mut chosen = None;
    for unit in &brain.census.units {
        if !unit.controllable() || unit.in_service()
            || ordered_this_frame(out,unit.entity_id)
            || brain.tracked.tasks.get(unit.entity_id).is_some()
            || brain.tracked.held(obs.tick(),unit.entity_id)
            || obs.inside_transport(unit.entity_id).is_some()
            || !obs.controlled_by_own_controller(unit.entity_id) { continue; }
        let Some(kind) = obs.entity_type(rules, unit.entity_id) else { continue; };
        if !matches!(kind.category, ObjectCategory::Vehicle | ObjectCategory::Infantry)
            || unit.traits.yard_maker || unit.traits.harvester
            || unit.traits.engineer || unit.traits.spy
            || kind.resource_gatherer || kind.spawned { continue; }
        let refund = obs.grinder_refund(rules, unit.entity_id).unwrap_or(0);
        if !worth_recycling(true,refund,unit.traits.combat,unit.health_permille,
            // Support platforms with seats or special mind-control ability
            // have future operational value even when badly damaged.
            kind.passengers > 0 || super::tactics::mind_control_reach(rules,kind).is_some()) {
            continue;
        }
        // Only one confirmed opportunity at a time. No calls to enemy data
        // outside the legal visible entity projection.
        for site in &sites {
            let dist = squared_distance(unit.cell, site.cell);
            if dist > MAX_ROUTE_CELLS * MAX_ROUTE_CELLS
                || !obs.can_reach(kind.movement_zone,unit.cell,site.cell)
                || !obs.can_enter_grinder(rules,unit.entity_id,site.entity_id)
                || super::tactics::unsafe_post(rules,obs,unit.entity_id,site.cell)
                || obs.visible_enemy_entities().any(|enemy|
                    squared_distance(enemy.cell,unit.cell) <= 9 * 9
                    || squared_distance(enemy.cell,site.cell) <= 9 * 9)
            { continue; }
            let key = (dist, std::cmp::Reverse(refund), unit.entity_id,site.entity_id);
            if chosen.is_none_or(|(best,_,_,_)| key < best) {
                chosen=Some((key,unit.entity_id,unit.cell,*site));
            }
        }
    }
    let Some((_,actor,from,site)) = chosen else { return; };
    let goal=Goal::Grind { target: site.entity_id };
    if issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,
        actor,TrackedKind::Enter,Some(site.entity_id),Some(site.cell),goal)
    {
        brain.tracked.tasks.claim(obs.tick(),actor,JobKind::Recycle,
            Some(site.entity_id),Some(site.cell),from);
        brain.recycle_after=obs.tick().saturating_add(GRIND_RETRY);
        log::info!("auto play: controlled prize {} sent to native grinder {}, refund candidate={}",
            actor,site.entity_id,obs.grinder_refund(rules,actor).unwrap_or(0));
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    #[test]
    fn controlled_grinder_offer_never_consumes_a_healthy_fighting_prize() {
        assert!(worth_recycling(true,400,false,1000,false));
        assert!(worth_recycling(true,400,true,250,false));
        assert!(!worth_recycling(true,400,true,800,false));
        assert!(!worth_recycling(false,400,false,100,false));
        assert!(!worth_recycling(true,100,false,100,false));
        assert!(!worth_recycling(true,400,false,100,true));
    }
}
