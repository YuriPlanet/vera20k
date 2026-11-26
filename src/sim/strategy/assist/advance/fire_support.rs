//! Behind-the-screen fire support for live rule-defined long-range vehicles.
//!
//! An artillery vehicle is not an extra short-range tank. Its native `Attack`
//! mission stops in the weapon's range; `AttackMove` to a base cell drives it
//! into the defenders instead. This pass never selects hidden enemy entities,
//! and it only commits a gun when a real friendly screen stands in front of it.

use std::collections::BTreeSet;

use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::{EnemyEntity, Observation};

use super::super::AssistBrain;
use super::super::census::{Slot, Unit};
use super::super::squads::Role;
use super::{cell_distance, weapon_range_cells};

const MIN_REAR_REACH: i32 = 7;
const MAX_SUPPORT_ORDERS: usize = 6;

/// Support capability comes from current live weapons, never a fixed unit ID
/// or the hypothetical sum of every IFV Gunner slot.
pub(in crate::sim::strategy::assist) fn support_reach(
    rules: &RuleSet, obs: &Observation<'_>, unit: &Unit,
) -> i32 {
    if unit.slot != Slot::Vehicle || !unit.traits.combat || unit.traits.suicide
        || unit.traits.irreplaceable || unit.traits.carries || unit.traits.harvester
        || unit.traits.yard_maker || !unit.controllable() || unit.in_service()
    { return 0; }
    let Some(ty) = obs.entity_type(rules, unit.entity_id) else { return 0; };
    if ty.gunner {
        obs.active_gunner_weapon(rules, unit.entity_id)
            .filter(|weapon| !weapon.never_use && weapon.damage > 0
                && !weapon.suicide)
            .map_or(0, super::super::siege_counter::max_range_cells)
    } else {
        super::weapon_of(rules, ty, obs.is_deployed(unit.entity_id))
            .map_or(0, super::super::siege_counter::max_range_cells)
    }
}

/// A *different* fighting unit must be physically ahead of the gun, close
/// enough to intercept a counterattack rather than counted across the map.
fn screens(gun_enemy: i32, friend_enemy: i32, friend_gun: i32) -> bool {
    // A gun which has already crept just ahead of its screen still needs
    // recovery to a rear post. Requiring the screen to be two cells farther
    // forward before recognizing it abandons those guns to AttackMove.
    friend_enemy <= gun_enemy + 2 && friend_enemy <= 22 && friend_gun <= 16
}

fn forward_screen<'a>(
    gun: &Unit, objective: (u16, u16), frontliners: &'a [&Unit],
) -> Option<&'a Unit> {
    let gun_distance = cell_distance(gun.cell, objective);
    frontliners.iter().copied().filter(|friend| {
        friend.entity_id != gun.entity_id && friend.health_permille >= 400
            && screens(gun_distance, cell_distance(friend.cell, objective),
                cell_distance(friend.cell, gun.cell))
    }).min_by_key(|friend| {
        (cell_distance(friend.cell, objective), friend.entity_id)
    })
}

/// A post on the *friendly* side of the leading screen. The caller checks
/// walkability, local threat geometry and true movement-zone connectivity.
fn behind(front: (u16,u16), objective: (u16,u16), depth: i32, shift: i32)
    -> Option<(u16, u16)> {
    let mut dx = (i32::from(front.0) - i32::from(objective.0)).signum();
    let dy = (i32::from(front.1) - i32::from(objective.1)).signum();
    if dx == 0 && dy == 0 { dx = 1; }
    let side = (-dy, dx);
    Some((
        u16::try_from(i32::from(front.0) + dx * depth + side.0 * shift).ok()?,
        u16::try_from(i32::from(front.1) + dy * depth + side.1 * shift).ok()?,
    ))
}

/// `MinimumRange` is a real native weapon restriction (V3 missiles have
/// one). A rear post too near the objective can be safe but unable to fire.
fn minimum_fire_distance(rules: &RuleSet, obs: &Observation<'_>, unit: &Unit) -> i32 {
    let Some(ty) = obs.entity_type(rules, unit.entity_id) else { return 0; };
    let weapon = if ty.gunner {
        obs.active_gunner_weapon(rules, unit.entity_id)
    } else {
        super::weapon_of(rules, ty, obs.is_deployed(unit.entity_id))
    };
    weapon.map_or(0, |weapon| {
        let n = weapon.minimum_range_leptons.max(0);
        let cell = crate::util::lepton::LEPTONS_PER_CELL_I32;
        n.div_euclid(cell) + i32::from(n.rem_euclid(cell) != 0)
    })
}

fn support_post(
    rules: &RuleSet, obs: &Observation<'_>, unit: &Unit,
    screen: &Unit, objective: (u16,u16), reach: i32,
) -> Option<(u16,u16)> {
    // If the screen is already further away than the gun's effective reach,
    // the artillery should *follow*, not camp far behind the map's front.
    let minimum = minimum_fire_distance(rules, obs, unit);
    let screen_distance = cell_distance(screen.cell, objective);
    for depth in [4, 6, 2, 8] {
        for shift in [0, -2, 2, -4, 4] {
            let Some(post) = behind(screen.cell, objective, depth, shift) else { continue; };
            let distance = cell_distance(post, objective);
            // Far from the target, follow the screen across the map; don't
            // wait until the enemy comes into gun range before beginning to
            // move. Near the objective, choose an actual firing post instead
            // of camping a few cells beyond the weapon's maximum reach.
            if (screen_distance <= reach + 3 && distance > reach + 5)
                || distance <= minimum
                || !obs.is_ground_walkable(post.0, post.1)
                || !super::super::orders::reachable(rules, obs, unit.entity_id, post)
                || super::super::tactics::unsafe_post(rules, obs, unit.entity_id, post)
                || super::super::tactics::lethal_approach(
                    rules, obs, unit.entity_id, unit.cell, post).is_some()
            { continue; }
            return Some(post);
        }
    }
    None
}

fn pick_visible_target(
    rules: &RuleSet, obs: &Observation<'_>, unit: &Unit,
    screen: &Unit, reach: i32, objective: (u16,u16),
    visible: &[EnemyEntity],
) -> Option<EnemyEntity> {
    let minimum = minimum_fire_distance(rules, obs, unit);
    visible.iter().copied().filter(|enemy| {
        cell_distance(unit.cell, enemy.cell) > minimum
            && cell_distance(enemy.cell, objective) <= 16
            && cell_distance(enemy.cell, screen.cell) <= 22
            && cell_distance(unit.cell, enemy.cell) <= reach + 5
            && obs.can_hurt(rules, unit.entity_id, enemy.entity_id)
            && super::super::tactics::actual_threat_reach(
                rules, obs, unit.entity_id, enemy.entity_id) >= MIN_REAR_REACH
            && super::super::orders::admits(rules, obs,
                unit.entity_id, &Goal::Attack { target: enemy.entity_id })
            && (cell_distance(unit.cell, enemy.cell) <= reach
                || super::super::tactics::lethal_approach(
                    rules, obs, unit.entity_id, unit.cell, enemy.cell).is_none())
    }).min_by_key(|enemy| {
        let ty = obs.entity_type(rules, enemy.entity_id);
        let threat = ty.is_some_and(|ty| ty.is_base_defense)
            && obs.visible_enemy_building_operational(rules, enemy.entity_id);
        let pressing_screen = obs.attack_target(enemy.entity_id) == Some(screen.entity_id);
        let infrastructure = ty.is_some_and(|ty| ty.category == ObjectCategory::Building);
        (!pressing_screen, !threat, !infrastructure,
            cell_distance(unit.cell, enemy.cell), enemy.entity_id)
    })
}

/// Keep supported artillery out of the ordinary frontal AttackMove loop even
/// when an existing native Attack needs no new order. Returned IDs are *only*
/// those with a visible/frontline-backed artillery role this frame.
pub(in crate::sim::strategy::assist) fn push(
    brain: &mut AssistBrain, rules: &RuleSet, obs: &Observation<'_>,
    out: &mut Outbox, fighters: &[&Unit], objective: (u16,u16),
) -> BTreeSet<u64> {
    push_impl(brain,rules,obs,out,fighters,objective,false)
}

/// Joint-operation guns are a separate, bounded commitment: their screens
/// may act during the main fist's stockpile phase, but no other artillery is
/// released from that stockpile.
pub(in crate::sim::strategy::assist) fn push_coordinated(
    brain: &mut AssistBrain, rules: &RuleSet, obs: &Observation<'_>,
    out: &mut Outbox, fighters: &[&Unit], objective: (u16,u16),
) -> BTreeSet<u64> {
    push_impl(brain,rules,obs,out,fighters,objective,true)
}

fn push_impl(
    brain: &mut AssistBrain, rules: &RuleSet, obs: &Observation<'_>,
    out: &mut Outbox, fighters: &[&Unit], objective: (u16,u16),
    joint_operation: bool,
) -> BTreeSet<u64> {
    let mut held = BTreeSet::new();
    if brain.campaign.holding_fist() && !joint_operation { return held; }
    let visible: Vec<_> = obs.visible_enemy_entities()
        .filter(|enemy| cell_distance(enemy.cell, objective) <= 16)
        .take(80).collect();
    // Compute fire-support capability once per candidate, not in an
    // O(fighters * fighters) rules lookup inside every screen comparison.
    let frontliners: Vec<&Unit> = fighters.iter().copied()
        .filter(|friend| friend.controllable() && !friend.in_service()
            && friend.traits.combat && friend.health_permille >= 400
            && support_reach(rules, obs, friend) < MIN_REAR_REACH)
        .take(192).collect();
    let mut issued = 0;
    let limit = fighters.len().min(192);
    if limit == 0 { return held; }
    // A stable first-six scan would repeatedly update the leading artillery
    // while rear guns never receive their first post on a large map.
    let start = (obs.tick() as usize) % limit;
    for unit in fighters.iter().take(limit).cycle().skip(start).take(limit).copied() {
        let reach = support_reach(rules, obs, unit);
        if reach < MIN_REAR_REACH || !unit.controllable() || unit.in_service()
            || super::super::orders::committed_garrison_entry(
                &brain.tracked, rules, obs, unit.entity_id)
        { continue; }
        let Some(screen) = forward_screen(unit, objective, &frontliners)
        else { continue; };
        held.insert(unit.entity_id);
        if super::super::orders::ordered_this_frame(out, unit.entity_id)
            || issued >= MAX_SUPPORT_ORDERS
        { continue; }
        if let Some(enemy) = pick_visible_target(rules, obs, unit, screen,
            reach, objective, &visible)
        {
            if obs.attack_target(unit.entity_id) == Some(enemy.entity_id) {
                continue;
            }
            if super::super::orders::issue_tracked(
                &mut brain.tracked, brain.trace, out, rules, obs, unit.entity_id,
                super::super::tracked::TrackedKind::Attack,
                Some(enemy.entity_id), Some(enemy.cell),
                Goal::Attack { target: enemy.entity_id },
            ) { issued += 1; }
            continue;
        }
        if obs.is_deployed(unit.entity_id) { continue; }
        let Some(post) = support_post(rules, obs, unit, screen, objective, reach)
        else {
            // No safe firing post is currently reachable. Do not leave a
            // previous AttackMove driving this valuable gun into the base.
            if obs.destination(unit.entity_id).is_some_and(|cell|
                cell_distance(cell, objective) <= 10)
                && super::super::orders::issue_tracked(
                    &mut brain.tracked, brain.trace, out, rules, obs,
                    unit.entity_id, super::super::tracked::TrackedKind::Engage,
                    None, None, Goal::Guard,
                ) { issued += 1; }
            continue;
        };
        if cell_distance(unit.cell, post) <= 3 {
            // An old AttackMove is *still* walking even after our planner
            // stopped issuing it. Cancel only that dangerous old march.
            if obs.destination(unit.entity_id).is_some_and(|cell|
                cell_distance(cell, objective) <= 10)
                && super::super::orders::issue_tracked(
                    &mut brain.tracked, brain.trace, out, rules, obs,
                    unit.entity_id, super::super::tracked::TrackedKind::Engage,
                    None, None, Goal::Guard,
                ) { issued += 1; }
        } else if obs.destination(unit.entity_id) != Some(post)
            && super::super::orders::issue_tracked(
                &mut brain.tracked, brain.trace, out, rules, obs,
                unit.entity_id, super::super::tracked::TrackedKind::Move,
                None, Some(post), Goal::Move { cell: post },
            ) { issued += 1; }
    }
    held
}

/// Defensive counterpart: hold the rear of the garrison when a visible raid
/// attacks the base. Unlike a push, defense needs no route or remembered
/// target; fire from behind the existing live screen only.
pub(in crate::sim::strategy::assist) fn defend(
    brain: &mut AssistBrain, rules: &RuleSet, obs: &Observation<'_>, out: &mut Outbox,
) {
    let Some(home) = brain.census.base_cell else { return; };
    let visible: Vec<_> = obs.visible_enemy_entities()
        .filter(|enemy| cell_distance(enemy.cell, home) <= 24)
        .take(64).collect();
    if visible.is_empty() { return; }
    let Some(defenders) = brain.squads.squad(Role::Defend) else { return; };
    let friends: Vec<_> = brain.census.units.iter()
        .filter(|unit| defenders.members.contains(&unit.entity_id)
            && unit.controllable() && unit.traits.combat && !unit.in_service())
        .take(96).collect();
    let frontliners: Vec<_> = friends.iter().copied()
        .filter(|friend| friend.health_permille >= 400
            && support_reach(rules, obs, friend) < MIN_REAR_REACH)
        .collect();
    let mut issued = 0;
    for gun in &friends {
        if issued >= MAX_SUPPORT_ORDERS { break; }
        let reach = support_reach(rules, obs, gun);
        if reach < MIN_REAR_REACH || super::super::orders::ordered_this_frame(out, gun.entity_id)
            || super::super::orders::committed_garrison_entry(
                &brain.tracked, rules, obs, gun.entity_id)
        { continue; }
        let Some((screen, enemy)) = visible.iter().filter_map(|enemy| {
            let screen = forward_screen(gun, enemy.cell, &frontliners)?;
            let target = pick_visible_target(rules, obs, gun, screen,
                reach, enemy.cell, &visible)?;
            Some((screen, target))
        }).min_by_key(|(screen, enemy)| {
            (cell_distance(screen.cell, enemy.cell), enemy.entity_id)
        }) else { continue; };
        let _ = screen;
        if obs.attack_target(gun.entity_id) == Some(enemy.entity_id) { continue; }
        if super::super::orders::issue_tracked(
            &mut brain.tracked, brain.trace, out, rules, obs, gun.entity_id,
            super::super::tracked::TrackedKind::Attack,
            Some(enemy.entity_id), Some(enemy.cell),
            Goal::Attack { target: enemy.entity_id },
        ) { issued += 1; }
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::{screens, behind};

    #[test]
    fn a_real_front_line_is_required_not_a_distant_friendly() {
        assert!(screens(16, 8, 9));
        assert!(!screens(16, 8, 25));
        assert!(screens(8, 10, 6)); // artillery can back away to recover
        assert!(!screens(8, 13, 6));
    }

    #[test]
    fn retail_v3_is_rear_support_but_cannot_fire_point_blank() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        let Some(weapon) = rules.weapon("V3Launcher") else { return; };
        assert_eq!(weapon.minimum_range_leptons,
            5 * crate::util::lepton::LEPTONS_PER_CELL_I32);
        assert!(super::super::weapon_reach_cells(weapon) >= super::MIN_REAR_REACH);
    }

    #[test]
    fn artillery_posts_stay_behind_the_front_line() {
        assert_eq!(behind((20, 20), (30, 20), 5, 0), Some((15, 20)));
        assert_eq!(behind((20, 20), (30, 20), 5, 2), Some((15, 18)));
        assert_eq!(behind((1, 1), (10, 10), 5, 0), None);
    }
}
