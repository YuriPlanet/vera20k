//! Phobos' shield arm of the stock selection hook at `0x6F36DB`
//! (`Ext/Techno/Hooks.Firing.cpp:140-162`): once `TechnoExt::PickWeaponIndex`
//! answers `-1`, an active target shield turns a primary that cannot reach it
//! into a secondary answer, and leaves the stock `0x6F36E3` Verses arm when the
//! primary does reach it.

use super::TargetKind;
use super::combat_weapon;
use crate::rules::ruleset::RuleSet;
use crate::sim::arena_fixture;
use crate::sim::shield::ShieldState;
use crate::sim::world::Simulation;

/// One attacker family, a plain target and a shielded target. `attacker` is the
/// body of the attacker's type section: its `Primary`/`Secondary` and any
/// `NoSecondaryWeaponFallback` keys.
fn shield_rules(attacker: &str) -> RuleSet {
    let ini = arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=ATTACKER\n1=PLAIN\n2=SHIELDED\n\
         [AircraftTypes]\n\
         [ShieldTypes]\n0=Weak\n\
         [Weak]\nStrength=500\nArmor=none\n\
         [ATTACKER]\nStrength=400\nCost=300\nSpeed=6\nArmor=heavy\n{attacker}\
         [PLAIN]\nStrength=400\nCost=300\nSpeed=6\nArmor=heavy\n\
         [SHIELDED]\nStrength=400\nCost=300\nSpeed=6\nArmor=heavy\nShieldType=Weak\n\
         [NoShieldGun]\nDamage=10\nROF=50\nRange=6\nWarhead=NarrowWH\n\
         [ShieldGun]\nDamage=10\nROF=50\nRange=6\nWarhead=AnyWH\n\
         [PiercingGun]\nDamage=10\nROF=50\nRange=6\nWarhead=PierceWH\n\
         [NarrowWH]\nVerses=0%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [AnyWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [PierceWH]\nVerses=0%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         Shield.Penetrate=yes\n"
    ));
    RuleSet::from_ini(&ini).expect("shield selection rules")
}

/// The selected weapon index through the production SelectWeapon owner.
fn selected_index(sim: &Simulation, rules: &RuleSet, attacker: u64, target: u64) -> i32 {
    let entity = sim.entities().get(attacker).expect("attacker entity");
    let object = rules.object("ATTACKER").expect("attacker type");
    combat_weapon::select_weapon_against(
        rules,
        entity,
        object,
        &combat_weapon::attacker_facts(entity, object),
        Some(&TargetKind::Entity(target)),
        sim,
        &sim.interner,
        sim.resolved_terrain.as_ref(),
        Some(&sim.house_alliances),
    )
    .expect("a weapon index")
    .0
}

fn arena(rules: &RuleSet, target_type: &str, seed: u64) -> (Simulation, u64, u64) {
    let mut sim = arena_fixture::rival_arena(rules, seed);
    let attacker = sim
        .spawn_object("ATTACKER", "Americans", 8, 10, 0, rules)
        .expect("attacker spawns");
    let target = sim
        .spawn_object(target_type, "Russians", 12, 10, 0, rules)
        .expect("target spawns");
    (sim, attacker, target)
}

/// An active shield whose armor the primary's warhead ignores selects the
/// secondary, which the stock ladder would never prefer: both warheads pass
/// its Verses arm, so only the shield gate answers here.
#[test]
fn the_shield_arm_takes_the_secondary_the_primary_cannot_reach() {
    let rules = shield_rules("Primary=NoShieldGun\nSecondary=ShieldGun\n");
    let (sim, attacker, target) = arena(&rules, "SHIELDED", 21);
    assert!(
        sim.entities().get(target).unwrap().shield().is_some(),
        "the shielded type carries a live shield"
    );
    assert_eq!(
        selected_index(&sim, &rules, attacker, target),
        1,
        "the shield refuses the primary warhead, so the secondary answers"
    );
}

/// A primary warhead the shield armor does answer keeps the stock Verses
/// answer, which is the primary here because neither Verses lookup is zero.
#[test]
fn a_primary_the_shield_accepts_keeps_the_stock_answer() {
    let rules = shield_rules("Primary=ShieldGun\nSecondary=NoShieldGun\n");
    let (sim, attacker, target) = arena(&rules, "SHIELDED", 22);
    assert_eq!(
        selected_index(&sim, &rules, attacker, target),
        0,
        "the shield admits the primary warhead"
    );
}

/// `Shield.Penetrate` makes `CanBeTargeted` answer true regardless of Verses, so
/// the arm leaves the stock answer in place.
#[test]
fn a_penetrating_primary_keeps_the_stock_answer() {
    let rules = shield_rules("Primary=PiercingGun\nSecondary=ShieldGun\n");
    let (sim, attacker, target) = arena(&rules, "SHIELDED", 23);
    assert_eq!(
        selected_index(&sim, &rules, attacker, target),
        0,
        "a penetrating warhead passes the shield gate"
    );
}

/// An inactive shield never reaches the arm: the same primary that a live
/// shield refuses is selected again.
#[test]
fn an_inactive_shield_keeps_the_stock_answer() {
    let rules = shield_rules("Primary=NoShieldGun\nSecondary=ShieldGun\n");
    let (sim, attacker, target) = arena(&rules, "SHIELDED", 24);
    assert_eq!(selected_index(&sim, &rules, attacker, target), 1);
    let mut sim = sim;
    sim.substrate.entities.get_mut(target).unwrap().seldom.shield =
        Some(ShieldState::new("Weak".to_owned(), 0, 500));
    assert_eq!(
        selected_index(&sim, &rules, attacker, target),
        0,
        "a broken shield skips the arm"
    );
}

/// An unshielded target never reaches the arm either.
#[test]
fn an_unshielded_target_keeps_the_stock_answer() {
    let rules = shield_rules("Primary=NoShieldGun\nSecondary=ShieldGun\n");
    let (sim, attacker, target) = arena(&rules, "PLAIN", 25);
    assert!(sim.entities().get(target).unwrap().shield().is_none());
    assert_eq!(selected_index(&sim, &rules, attacker, target), 0);
}
