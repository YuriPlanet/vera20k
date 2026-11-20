//! Phobos `Bolt.Duration` / `Bolt.FollowFLH` (`Ext/WeaponType/Body.cpp:101-102`,
//! `Ext/EBolt/Body.cpp:30`, `Ext/EBolt/Hooks.cpp:104-121`): the EBolt
//! lifetime exponent and the firer-tracking override over the stock Unit-only
//! owner link.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::electric_bolt;
use crate::sim::world::LifecycleOutput;

fn bolt_rules(follow: Option<bool>, duration: Option<i32>) -> RuleSet {
    bolt_rules_with(follow, duration, None)
}

fn bolt_rules_with(follow: Option<bool>, duration: Option<i32>, arcs: Option<i32>) -> RuleSet {
    let follow = follow.map_or_else(String::new, |value| {
        format!("Bolt.FollowFLH={}\n", if value { "yes" } else { "no" })
    });
    let duration = duration.map_or_else(String::new, |value| format!("Bolt.Duration={value}\n"));
    let arcs = arcs.map_or_else(String::new, |value| format!("Bolt.Arcs={value}\n"));
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=TESLA\n1=TARGET\n\
         [AircraftTypes]\n[BuildingTypes]\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=Coil\nSight=8\n\
         [TESLA]\nStrength=300\nArmor=heavy\nPrimary=Coil\nSpeed=6\nSight=8\n\
         [TARGET]\nStrength=300\nArmor=heavy\n\
         [Coil]\nDamage=10\nROF=20\nRange=8\nWarhead=WH\nIsElectricBolt=yes\n\
         {follow}{duration}{arcs}\
         [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    )))
    .expect("bolt key rules")
}

fn fired_birth(
    firer_kind: &str,
    follow: Option<bool>,
    duration: Option<i32>,
) -> electric_bolt::ElectricBoltBirth {
    fired_birth_with(firer_kind, follow, duration, None)
}

fn fired_birth_with(
    firer_kind: &str,
    follow: Option<bool>,
    duration: Option<i32>,
    arcs: Option<i32>,
) -> electric_bolt::ElectricBoltBirth {
    let rules = bolt_rules_with(follow, duration, arcs);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x6F_D5D7);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let firer = sim
        .spawn_object(firer_kind, "Americans", 10, 10, 0, &rules)
        .expect("firer");
    let target = sim
        .spawn_object("TARGET", "Russians", 12, 10, 0, &rules)
        .expect("target");
    crate::sim::arena_fixture::discover(&mut sim, target);
    sim.lifecycle_outputs.clear_for_test();
    electric_bolt::fired(
        &mut sim,
        &rules,
        firer,
        TargetKind::Entity(target),
        rules.weapon("Coil").expect("Coil weapon"),
    );
    sim.lifecycle_outputs
        .iter()
        .find_map(|output| match output {
            LifecycleOutput::ElectricBoltCreated(birth) => Some(*birth),
            _ => None,
        })
        .expect("Coil builds a bolt")
}

/// `EBoltExt::CreateEBolt` stores `1 << (clamp(Bolt.Duration, 1, 31) - 1)`;
/// the constructor's 17 reproduces the stock 0x10000.
#[test]
fn bolt_duration_sets_the_lifetime_exponent() {
    assert_eq!(fired_birth("TESLA", None, None).decay, 0x10000);
    assert_eq!(fired_birth("TESLA", None, Some(3)).decay, 4);
    assert_eq!(fired_birth("TESLA", None, Some(1)).decay, 1);
    assert_eq!(fired_birth("TESLA", None, Some(0)).decay, 1);
    assert_eq!(fired_birth("TESLA", None, Some(31)).decay, 1 << 30);
    assert_eq!(fired_birth("TESLA", None, Some(40)).decay, 1 << 30);
}

/// `EBoltExt::CreateEBolt` copies the weapon's `Bolt.Arcs` (constructor 8).
#[test]
fn bolt_arcs_reach_the_birth() {
    assert_eq!(fired_birth("TESLA", None, None).arcs, 8);
    assert_eq!(fired_birth_with("TESLA", None, None, Some(3)).arcs, 3);
}

/// `EBoltExt::_SetOwner` links a bolt when `Bolt.FollowFLH` is set, else only
/// for a Unit firer; a non-Unit link has no Unit `+0x6DC` slot.
#[test]
fn bolt_follow_flh_overrides_the_unit_only_owner_link() {
    let unit = fired_birth("TESLA", None, None);
    let owner = unit.owner.expect("a Unit links its bolt by default");
    assert!(owner.unit_slot);

    assert!(
        fired_birth("TESLA", Some(false), None).owner.is_none(),
        "an authored no stops a Unit's bolt from following"
    );
    assert!(
        fired_birth("E1", None, None).owner.is_none(),
        "a non-Unit does not link by default"
    );
    let infantry = fired_birth("E1", Some(true), None);
    let owner = infantry
        .owner
        .expect("an authored yes links a non-Unit bolt");
    assert!(!owner.unit_slot, "only a Unit owns the +0x6DC slot");
}

/// The Phobos `TechnoClass_InitEBolt` hook skips the stock `+0x6DC` slot test,
/// so a Unit links every bolt, and the native SetOwner InLimbo check
/// (`0x4C2BF2`) never runs, so a Limbo Unit still links.
#[test]
fn phobos_links_every_unit_bolt_and_a_limbo_unit_still_links() {
    let rules = bolt_rules(None, None);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x6F_D5D8);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let firer = sim
        .spawn_object("TESLA", "Americans", 10, 10, 0, &rules)
        .expect("firer");
    let target = sim
        .spawn_object("TARGET", "Russians", 12, 10, 0, &rules)
        .expect("target");
    crate::sim::arena_fixture::discover(&mut sim, target);
    sim.lifecycle_outputs.clear_for_test();
    let weapon = rules.weapon("Coil").expect("Coil weapon");
    for _ in 0..2 {
        electric_bolt::fired(&mut sim, &rules, firer, TargetKind::Entity(target), weapon);
    }
    let owners: Vec<_> = sim
        .lifecycle_outputs
        .iter()
        .filter_map(|output| match output {
            LifecycleOutput::ElectricBoltCreated(birth) => birth.owner,
            _ => None,
        })
        .collect();
    assert_eq!(owners.len(), 2, "both shots build a bolt");
    assert!(
        owners.iter().all(|owner| owner.unit == firer),
        "the hook links every Unit bolt into ElectricBolts"
    );

    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x6F_D5D9);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let firer = sim
        .spawn_object("TESLA", "Americans", 10, 10, 0, &rules)
        .expect("firer");
    let target = sim
        .spawn_object("TARGET", "Russians", 12, 10, 0, &rules)
        .expect("target");
    crate::sim::arena_fixture::discover(&mut sim, target);
    sim.substrate
        .entities
        .get_mut(firer)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(true);
    sim.lifecycle_outputs.clear_for_test();
    electric_bolt::fired(&mut sim, &rules, firer, TargetKind::Entity(target), weapon);
    let birth = sim
        .lifecycle_outputs
        .iter()
        .find_map(|output| match output {
            LifecycleOutput::ElectricBoltCreated(birth) => Some(*birth),
            _ => None,
        });
    assert!(
        birth.is_some_and(|birth| birth.owner.is_some_and(|owner| owner.unit == firer)),
        "a Limbo Unit still links its bolt"
    );
}
