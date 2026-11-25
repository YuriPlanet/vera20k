//! Native passenger/weapon interactions interpreted from the active rules.
//!
//! `Gunner=yes` changes the carrier's active weapon to the *passenger's*
//! `IFVMode` slot; `OpenTopped=yes` instead lets each passenger fire its own
//! `OpenTransportWeapon` (or its native primary/secondary choice). These are
//! different interactions: never add all IFV weapon records together. A mod
//! can replace any unit or weapon name without changing these decisions.

use crate::rules::object_type::{ObjectType, ObjectCategory};
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::control::observation::Observation;

use super::squared_distance;

/// The single Gunner slot the native passenger sets on the chassis.
pub(super) fn gunner_weapon<'r>(
    rules: &'r RuleSet, carrier: &ObjectType, passenger: &ObjectType,
) -> Option<&'r WeaponType> {
    if !carrier.gunner { return None; }
    let slot = usize::try_from(passenger.ifv_mode).ok()?;
    carrier.weapon_at(slot).and_then(|name| rules.weapon(name))
}

/// A bounded estimate of useful *shots*, not the unit's entire army value.
/// Repair and suicidal weapons have their own specialists, not generic
/// long-lived combat boarding orders. Special warhead effects are not DPS.
fn weapon_score(rules: &RuleSet, weapon: &WeaponType,
    target: Option<&ObjectType>,
) -> i64 {
    if weapon.never_use || weapon.suicide || weapon.damage <= 0 { return 0; }
    let Some(projectile) = rules.weapon_projectile(weapon) else { return 0; };
    let Some(head) = weapon.warhead.as_deref().and_then(|id| rules.warhead(id))
    else { return 0; };
    let Some(target) = target else {
        // When the enemy is hidden, rank only broadly useful real weapons.
        // Reserve large strategic values for known targets, not guesses.
        return i64::from(weapon.damage.max(0).min(200))
            .saturating_mul(i64::from(weapon.burst.clamp(1, 8)))
            .saturating_mul(i64::from((weapon.range_leptons / 256).clamp(1, 18)))
            / i64::from(weapon.rof.clamp(15, 300));
    };
    let air = target.category == ObjectCategory::Aircraft || target.balloon_hover
        || target.jumpjet;
    if air && !projectile.aa || !air && !projectile.ag { return 0; }
    let verses = f64::from_bits(head.interaction(target.armor_id()).multiplier().bits());
    if verses <= 0.0 { return 0; }
    let damage = (f64::from(weapon.damage).min(5_000.0) * verses)
        .clamp(0.0, 10_000.0) as i64;
    let rate = damage.saturating_mul(i64::from(weapon.burst.clamp(1, 8)))
        .saturating_mul(90) / i64::from(weapon.rof.clamp(15, 300));
    // A valid but short-ranged gun has less value in a mobile firing port.
    rate.saturating_add(i64::from((weapon.range_leptons / 256).clamp(0, 24)) * 3)
}

/// Expected incremental value of filling a single seat. The native engine
/// still decides which weapon fires and whether the passenger may enter;
/// this only chooses which *legal* spare passenger deserves a scarce seat.
pub(super) fn passenger_value(
    rules: &RuleSet, obs: &Observation<'_>,
    carrier: &ObjectType, passenger: &ObjectType, at: (u16, u16),
) -> i64 {
    if carrier.gunner {
        let Some(weapon) = gunner_weapon(rules, carrier, passenger) else { return 0; };
        return score_on_visible_front(rules, obs, weapon, at);
    }
    if !carrier.open_topped { return 0; }
    let indices: Vec<usize> = if passenger.open_transport_weapon >= 0 {
        usize::try_from(passenger.open_transport_weapon).ok().into_iter().collect()
    } else {
        (0..2).collect()
    };
    indices.into_iter().filter_map(|slot| {
        let weapon = rules.weapon(passenger.weapon_at(slot)?)?;
        Some(score_on_visible_front(rules, obs, weapon, at))
    }).max().unwrap_or(0)
}

fn score_on_visible_front(
    rules: &RuleSet, obs: &Observation<'_>, weapon: &WeaponType,
    at: (u16, u16),
) -> i64 {
    let mut value = 0i64;
    let mut seen = 0usize;
    for enemy in obs.visible_enemy_entities() {
        if squared_distance(at, enemy.cell) > 32 * 32 { continue; }
        let Some(target) = obs.entity_type(rules, enemy.entity_id) else { continue; };
        value = value.max(weapon_score(rules, weapon, Some(target)));
        seen += 1;
        if seen >= 12 { break; }
    }
    if seen == 0 { weapon_score(rules, weapon, None) }
    else { value }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    #[test]
    fn gunner_uses_passengers_mode_not_every_weapon_on_the_chassis() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        let carrier = rules.object("FV").expect("IFV");
        let engineer = rules.object("ENGINEER").expect("engineer");
        let infantry = rules.object("E1").expect("GI");
        assert!(gunner_weapon(&rules, carrier, engineer)
            .is_some_and(|w| w.damage < 0));
        assert!(gunner_weapon(&rules, carrier, infantry)
            .is_some_and(|w| w.damage > 0));
        let fortress = rules.object("BFRT").expect("battle fortress");
        assert!(fortress.open_topped);
        assert!(gunner_weapon(&rules, fortress, engineer).is_none());
        // Two cross-side combinations from the *rule fields*, never ID
        // branches in production code. The returned IFV slot is the exact
        // weapon the native passenger will select on this carrier.
        let tesla = rules.object("SHK").expect("tesla trooper");
        assert_eq!(tesla.ifv_mode, 6);
        assert!(gunner_weapon(&rules, carrier, tesla).is_some_and(|w|
            w.damage > 0 && w.range_leptons >= 6 * 256));
        let terrorist = rules.object("TERROR").expect("terrorist");
        assert_eq!(terrorist.ifv_mode, 11);
        assert!(gunner_weapon(&rules, carrier, terrorist).is_some_and(|w|
            w.suicide && w.damage > 0));
    }

    #[test]
    fn suicide_and_repair_are_not_generic_frontline_damage() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        let ifv = rules.object("FV").expect("IFV");
        let engineer = rules.object("ENGINEER").expect("engineer");
        let repair = gunner_weapon(&rules, ifv, engineer).expect("repair gun");
        assert_eq!(weapon_score(&rules, repair, None), 0);
        if let Some(terrorist) = rules.object("TERROR") {
            if let Some(weapon) = gunner_weapon(&rules, ifv, terrorist) {
                if weapon.suicide { assert_eq!(weapon_score(&rules, weapon, None), 0); }
            }
        }
    }
}
