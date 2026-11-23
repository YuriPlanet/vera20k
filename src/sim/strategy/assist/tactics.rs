//! Visible threat geometry for expensive front-line decisions. Mind control
//! is not ordinary damage: a captured tank joins the enemy and starts shooting
//! us. Only current sight and live rule-defined warheads enter this analysis.

use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::observation::Observation;

use super::threats::distance;

/// The longest weapon range which may mind-control its victim, in cells.
/// `None` means the type has no such weapon, not that it is safe to shoot.
pub(super) fn mind_control_reach(rules: &RuleSet, ty: &ObjectType) -> Option<i32> {
    (0..ty.weapon_record_count()).filter_map(|slot| {
        let weapon = rules.weapon(ty.weapon_at(slot)?)?;
        let head = rules.warhead(weapon.warhead.as_deref()?)?;
        if !head.mind_control || weapon.never_use {
            return None;
        }
        let per_cell = crate::util::lepton::LEPTONS_PER_CELL_I32;
        Some((weapon.range_leptons.max(0) + per_cell - 1) / per_cell)
    }).max()
}

/// A *currently visible* mind-controller whose reach covers a proposed cell.
/// If two cover it, the closer one takes priority; ID is the tie breaker.
pub(super) fn controller_covering(
    rules: &RuleSet, obs: &Observation<'_>, cell: (u16,u16),
) -> Option<(u64, (u16,u16), i32)> {
    obs.visible_enemy_entities().filter_map(|enemy| {
        let ty = obs.entity_type(rules, enemy.entity_id)?;
        let reach = mind_control_reach(rules, ty)?;
        // One cell of grace for moving targets and turning the turret.
        (distance(cell, enemy.cell) <= reach + 1)
            .then_some((enemy.entity_id, enemy.cell, reach))
    }).min_by_key(|(id, origin, _)| (distance(*origin, cell), *id))
}

/// Walk a short distance away from the controller instead of charging into
/// the same fire again. All candidates use native movement-zone connectivity.
pub(super) fn evade_controller(
    rules: &RuleSet, obs: &Observation<'_>, actor: u64,
    at: (u16,u16), threat: (u16,u16),
) -> Option<(u16,u16)> {
    let zone = obs.entity_type(rules, actor)?.movement_zone;
    let dx = (i32::from(at.0) - i32::from(threat.0)).signum();
    let dy = (i32::from(at.1) - i32::from(threat.1)).signum();
    if dx == 0 && dy == 0 { return None; }
    for step in [3, 2, 4] {
        let (Ok(x), Ok(y)) = (u16::try_from(i32::from(at.0) + dx * step),
            u16::try_from(i32::from(at.1) + dy * step)) else { continue; };
        let cell = (x,y);
        if obs.is_ground_walkable(x,y) && obs.can_reach(zone,at,cell)
            && controller_covering(rules,obs,cell).is_none()
        {
            return Some(cell);
        }
    }
    None
}

/// Defensive fire is not merely more hit points: short-ranged infantry can
/// be destroyed before they take a single shot. An existing standoff unit
/// removes this gate; otherwise a rules-legal ranged vehicle must be
/// obtainable, or there is no artillery strategy to wait for.
pub(super) fn need_siege(
    rules: &RuleSet, obs: &Observation<'_>,
    target: (u16,u16), fighters: &[&super::census::Unit],
) -> Option<i32> {
    let (defensive_reach, defense, defender_id) = obs.visible_enemy_entities()
        .filter_map(|enemy| {
            let ty = obs.entity_type(rules,enemy.entity_id)?;
            if !ty.is_base_defense
                || !obs.visible_enemy_building_operational(rules, enemy.entity_id)
                || distance(enemy.cell,target) > 14
                // Don't park the entire army at home because a gun exists
                // on the far side of the map. Wait for artillery only when
                // the main force is actually approaching its defended line.
                || !fighters.iter().any(|unit| distance(unit.cell,enemy.cell) <= 18
                    && obs.can_hurt(rules,enemy.entity_id,unit.entity_id))
            { return None; }
            let reach = super::advance::weapon_range_cells(rules,ty,false);
            (reach >= 4).then_some((reach,ty,enemy.entity_id))
        })
        .max_by_key(|(reach,ty,id)| (*reach,ty.strength,*id))?;
    if fighters.iter().any(|unit| {
        obs.entity_type(rules,unit.entity_id).is_some_and(|ty| {
            super::advance::weapon_range_cells(rules,ty,obs.is_deployed(unit.entity_id))
                > defensive_reach + 1
                && obs.can_hurt(rules,unit.entity_id,defender_id)
        })
    }) { return None; }
    // Do not deadlock a faction whose current rules cannot build any standoff
    // answer. It must keep the old v10 assault behaviour instead.
    let buildable = super::build::class_types(rules,obs,
        crate::rules::object_type::ObjectCategory::Vehicle)
        .any(|(ty,_)| !ty.naval && super::orders::is_combat_unit(rules,ty)
            && (0..ty.weapon_record_count()).any(|slot| {
                let Some(weapon) = ty.weapon_at(slot).and_then(|name| rules.weapon(name))
                else { return false; };
                if weapon.never_use { return false; }
                let Some(head) = weapon.warhead.as_deref().and_then(|name| rules.warhead(name))
                else { return false; };
                super::advance::weapon_reach_cells(weapon) > defensive_reach + 1
                    && f64::from_bits(head.interaction(defense.armor_id()).multiplier().bits()) > 0.0
            }));
    buildable.then_some(defensive_reach)
}


/// A valuable, already damaged fighter can be one volley from disappearing
/// before the ordinary 30% health retreat triggers. Keep the retreat *owner*
/// in `orders::managed`: this only answers whether the danger is imminent.
/// Both the attacker and its weapon must be currently visible and rule-legal.
fn early_extraction(health_permille: u16, tier: usize, single_volley_lethal: bool) -> bool {
    let caution_line = if tier >= 2 { 600 } else { 450 };
    health_permille <= caution_line && (single_volley_lethal || health_permille <= 400)
}

pub(super) fn exposed_casualty(
    rules: &RuleSet, obs: &Observation<'_>, unit: &super::census::Unit,
) -> bool {
    use super::census::Slot;
    if !unit.traits.combat || !unit.controllable() || unit.in_service()
        || !matches!(unit.slot, Slot::Vehicle | Slot::Infantry)
        || obs.is_deployed(unit.entity_id) || unit.health_permille > 600
    { return false; }
    let Some(victim) = obs.entity_type(rules, unit.entity_id) else { return false; };
    let health = obs.health(unit.entity_id).unwrap_or(0);
    if health <= 0 { return false; }
    obs.visible_enemy_entities().any(|enemy| {
        if !obs.can_hurt(rules, enemy.entity_id, unit.entity_id) { return false; }
        let Some(attacker) = obs.entity_type(rules, enemy.entity_id) else { return false; };
        let reach = effective_reach_against(rules, obs, enemy.entity_id, unit.entity_id);
        if reach <= 0 || distance(enemy.cell, unit.cell) > reach + 1 { return false; }
        let lethal = super::avoidance::kills_outright(rules, attacker, victim, health);
        // A gun that is firing at somebody else should not empty our entire
        // front just because it happens to cover the area. A lethal volley is
        // the exception: when it changes target there will be no second turn.
        let targeting = obs.attack_target(enemy.entity_id) == Some(unit.entity_id);
        (targeting || lethal) && early_extraction(unit.health_permille, unit.tier, lethal)
    })
}

/// Effective range for the weapons that can *actually* engage this target.
/// Checking just a unit's primary weapon loses its secondary AA missile,
/// which is exactly what makes anti-air look harmless to rocket soldiers.
fn effective_reach_against(
    rules: &RuleSet, obs: &Observation<'_>, attacker: u64, victim: u64,
) -> i32 {
    use crate::rules::object_type::ObjectCategory;
    let Some(gun) = obs.entity_type(rules, attacker) else { return 0; };
    // An unpowered defense does not have a real weapon envelope; continue
    // treating its *building* as a hostile strategic target, not live fire.
    if gun.category == ObjectCategory::Building
        && !obs.visible_enemy_building_operational(rules, attacker)
    { return 0; }
    let Some(target) = obs.entity_type(rules, victim) else { return 0; };
    let air = target.category == ObjectCategory::Aircraft
        || (target.jumpjet && !obs.is_deployed(victim));
    (0..gun.weapon_record_count()).filter_map(|slot| {
        let weapon = rules.weapon(gun.weapon_at(slot)?)?;
        if weapon.never_use { return None; }
        let viable = if air {
            rules.weapon_projectile(weapon).is_some_and(|projectile| projectile.aa)
        } else {
            weapon.warhead.as_deref()
                .and_then(|name| rules.warhead(name))
                .is_some_and(|warhead| f64::from_bits(
                    warhead.interaction(target.armor_id()).multiplier().bits()) > 0.0)
        };
        viable.then_some(super::advance::weapon_reach_cells(weapon))
    }).max().unwrap_or(0)
}

pub(super) fn actual_threat_reach(
    rules: &RuleSet, obs: &Observation<'_>, attacker: u64, victim: u64,
) -> i32 {
    effective_reach_against(rules, obs, attacker, victim)
}

/// A short-ranged unit must not march through a visible weapon envelope
/// that it cannot answer. This includes AA against jumpjet infantry, snipers
/// against infantry and fixed artillery against short-range tanks. A whole
/// army may have adequate points while one such unit has no useful trade.
/// Combat strength is a group property, not a reason to kite every healthy
/// armored unit from a single weak pillbox. Never apply this to fragile air or
/// infantry, and never override the psychic controller emergency.
pub(super) fn overwhelming_armor_commit(
    armored: bool, health_permille: u16, ours: i32, theirs: i32,
) -> bool {
    armored && health_permille >= 650 && theirs > 0
        && ours >= theirs.saturating_mul(3)
}

fn loses_range_trade(own_reach: i32, hostile_reach: i32, can_answer: bool) -> bool {
    hostile_reach > 0 && (!can_answer || own_reach <= hostile_reach + 1)
}

pub(super) fn lethal_approach(
    rules: &RuleSet, obs: &Observation<'_>, actor: u64,
    from: (u16, u16), destination: (u16, u16),
) -> Option<((u16,u16), i32)> {
    use crate::rules::object_type::ObjectCategory;
    let actor_ty = obs.entity_type(rules, actor)?;
    let fragile = matches!(actor_ty.category,
        ObjectCategory::Infantry | ObjectCategory::Aircraft) || actor_ty.jumpjet;
    let own_reach = super::advance::weapon_range_cells(
        rules, actor_ty, obs.is_deployed(actor));
    obs.visible_enemy_entities().filter_map(|enemy| {
        let hostile = obs.entity_type(rules, enemy.entity_id)?;
        // A tank vs tank exchange is decided by micro, not by the siege gate.
        // Only fixed guns, or a long-ranged anti-personnel / anti-air weapon,
        // should stop a normal marching soldier before reaching the line.
        if !hostile.is_base_defense && !fragile { return None; }
        let reach = effective_reach_against(rules, obs, enemy.entity_id, actor);
        if reach <= 0 { return None; }
        if reach < 4 || (!hostile.is_base_defense && reach < 6) { return None; }
        if !loses_range_trade(own_reach, reach,
            obs.can_hurt(rules, actor, enemy.entity_id)) { return None; }
        let in_lane = distance(enemy.cell, destination) <= reach + 1
            || distance(enemy.cell, from) <= reach + 1
            || super::orders::path_crosses(&[enemy.cell], from, destination, reach);
        in_lane.then_some((enemy.cell, reach))
    }).max_by_key(|(cell, reach)| (*reach, std::cmp::Reverse(distance(from,*cell))))
}

/// A rally post is usable only outside all visible fixed guns and psychic
/// coverage. The decision is shared by artillery staging and reinforcement
/// assembly, so neither can send its men to the danger the other avoids.
pub(super) fn unsafe_post(
    rules: &RuleSet, obs: &Observation<'_>, actor: u64, post: (u16,u16),
) -> bool {
    controller_covering(rules,obs,post).is_some() || obs.visible_enemy_entities().any(|enemy| {
        let Some(ty) = obs.entity_type(rules, enemy.entity_id) else { return false; };
        effective_reach_against(rules, obs, enemy.entity_id, actor) > 0
            && (ty.is_base_defense
                || obs.entity_type(rules, actor).is_some_and(|ours| {
                    ours.jumpjet || matches!(ours.category,
                        crate::rules::object_type::ObjectCategory::Infantry
                        | crate::rules::object_type::ObjectCategory::Aircraft)
                }))
            && distance(enemy.cell,post) <= effective_reach_against(
                rules,obs,enemy.entity_id,actor) + 1
    })
}

/// Find a legal standoff cell on the *friendly* side of a visible gun. This
/// does not choose an attack target: the target/mission belongs to `advance`.
/// Every candidate is checked against all currently visible defensive guns,
/// not just the first one, so a second pillbox cannot make the staging post
/// more dangerous than the road we are leaving.
fn standoff_lane(
    from: (u16,u16), gun: (u16,u16), radius: i32, shift: i32,
) -> Option<(u16,u16)> {
    let mut dx = (i32::from(from.0) - i32::from(gun.0)).signum();
    let dy = (i32::from(from.1) - i32::from(gun.1)).signum();
    if dx == 0 && dy == 0 { dx = 1; }
    let lateral = (-dy,dx);
    let (Ok(x),Ok(y)) = (
        u16::try_from(i32::from(gun.0) + dx * radius + lateral.0 * shift),
        u16::try_from(i32::from(gun.1) + dy * radius + lateral.1 * shift),
    ) else { return None; };
    Some((x,y))
}

pub(super) fn siege_post(
    rules: &RuleSet, obs: &Observation<'_>, actor: u64,
    from: (u16,u16), gun: (u16,u16), reach: i32,
) -> Option<(u16,u16)> {
    let zone = obs.entity_type(rules, actor)?.movement_zone;
    // Give adjacent units distinct lanes; one shared post would turn the
    // staging line into a traffic jam. Actor IDs break ties deterministically.
    let lane = (actor % 5) as i32 - 2;
    for radius in [reach + 3, reach + 5, reach + 7] {
        for shift in [lane * 2, 0, -lane * 2] {
            let Some((x,y)) = standoff_lane(from,gun,radius,shift) else { continue; };
            let post = (x,y);
            if !obs.is_ground_walkable(x,y) || !obs.can_reach(zone,from,post) {
                continue;
            }
            if !unsafe_post(rules,obs,actor,post) { return Some(post); }
        }
    }
    None
}

/// The army has enough strength in total, but not at the point of contact.
/// This is a case for one *bounded* reinforcement wave, not a reason to raise
/// the all-map attack threshold (which would hold victories at the home base).
fn support_gap(front: i32, rear: i32, enemy: i32, front_count: usize, rear_count: usize) -> bool {
    enemy > 0 && front_count >= 2 && rear_count >= 3
        && front.saturating_mul(1_000) < enemy.saturating_mul(1_200)
        && front.saturating_add(rear).saturating_mul(1_000)
            >= enemy.saturating_mul(1_250)
}

/// How much armed enemy strength is *presently visible* at the front. Historic
/// sightings guide scouting, but they must not create fictitious gun pressure.
fn visible_front_points(
    rules: &RuleSet, obs: &Observation<'_>, at: (u16,u16),
) -> i32 {
    obs.visible_enemy_entities()
        .filter(|enemy| distance(enemy.cell,at) <= 18)
        .filter_map(|enemy| {
            let ty = obs.entity_type(rules,enemy.entity_id)?;
            if ty.is_base_defense
                && !obs.visible_enemy_building_operational(rules, enemy.entity_id)
            { return None; }
            let health = obs.health(enemy.entity_id)?.max(0);
            let fraction = i64::from(health).saturating_mul(1_000)
                / i64::from(ty.strength.max(1));
            let fraction = u16::try_from(fraction.clamp(0,1_000)).unwrap_or(0);
            Some(super::census::fight_points(ty,fraction))
        })
        .fold(0i32, |total, points| total.saturating_add(points))
}

pub(super) fn waiting_for_support(
    rules: &RuleSet, obs: &Observation<'_>, target: (u16,u16),
    fighters: &[&super::census::Unit],
) -> bool {
    let enemy = visible_front_points(rules,obs,target);
    let mut front = 0i32;
    let mut rear = 0i32;
    let mut front_count = 0usize;
    let mut rear_count = 0usize;
    for fighter in fighters {
        if distance(fighter.cell,target) <= 18 {
            front = front.saturating_add(fighter.points);
            front_count += 1;
        } else {
            rear = rear.saturating_add(fighter.points);
            rear_count += 1;
        }
    }
    support_gap(front,rear,enemy,front_count,rear_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damaged_elites_leave_before_an_imminent_second_volley() {
        assert!(early_extraction(550,2,true));
        assert!(!early_extraction(700,2,true));
        assert!(!early_extraction(550,0,true));
        assert!(early_extraction(390,0,false));
    }

    #[test]
    fn staging_lanes_spread_tanks_and_keep_edge_cells_valid() {
        let leading = standoff_lane((20,40),(40,40),9,0);
        let adjacent = standoff_lane((20,40),(40,40),9,3);
        assert_eq!(leading,Some((31,40)));
        assert_eq!(adjacent,Some((31,37)));
        assert_eq!(standoff_lane((0,0),(1,0),8,0),None);
    }

    #[test]
    fn reinforcement_gate_requires_an_actual_winnable_wave() {
        assert!(support_gap(200,900,700,2,6));
        assert!(!support_gap(200,900,0,2,6));
        assert!(!support_gap(200,200,700,2,6));
        assert!(!support_gap(900,300,700,5,3));
        assert!(!support_gap(200,900,700,2,1));
    }

    #[test]
    fn retail_psychic_units_are_rule_identified() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        // Modded rules can carry the same trait under another object name.
        if let Some(yuri) = rules.object("YURI") {
            assert!(mind_control_reach(&rules, yuri).is_some());
        }
        if let Some(grizzly) = rules.object("MTNK") {
            assert!(mind_control_reach(&rules, grizzly).is_none());
        }
    }
}

#[cfg(test)]
mod v14_counter_tests {
    use super::*;

    #[test]
    fn a_short_range_soldier_cannot_trade_into_a_long_range_gun() {
        assert!(loses_range_trade(5, 12, true));
        assert!(loses_range_trade(12, 5, false));
        assert!(!loses_range_trade(12, 5, true));
        assert!(!loses_range_trade(0, 0, false));
    }
}

#[cfg(test)]
mod v18_commit_tests {
    use super::*;

    #[test]
    fn only_healthy_overwhelming_armor_may_ignore_a_minor_pillbox() {
        assert!(overwhelming_armor_commit(true, 900, 900, 300));
        assert!(!overwhelming_armor_commit(false, 900, 900, 300));
        assert!(!overwhelming_armor_commit(true, 500, 900, 300));
        assert!(!overwhelming_armor_commit(true, 900, 899, 300));
        assert!(!overwhelming_armor_commit(true, 900, 900, 0));
    }
}
