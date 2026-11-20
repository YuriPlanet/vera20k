//! Phobos `Detonate.*` through the production superweapon launch tail
//! (`Ext/SWType/FireSuperWeapon.cpp:267-303`).

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::superweapon::chronosphere_tests::{charge_super, click, world_with};
use crate::sim::world::Simulation;

const OWNER: &str = "Americans";
const ROOT: &str = "RootSpecial";
const TARGET: (u16, u16) = (30, 30);

const BASE: &str = "[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n\
[BuildingTypes]\n0=PROVIDER\n\
[E1]\nStrength=100\nSpeed=4\nArmor=none\n\
[PROVIDER]\nStrength=1000\nTurret=no\nSight=0\nSuperWeapon=RootSpecial\n\
Foundation=1x1\nPower=100\n\
[WeaponTypes]\n0=GUN\n[GUN]\nDamage=25\nRange=5\nROF=20\nWarhead=WH\n\
[Warheads]\n0=WH\n[WH]\nCellSpread=1\n\
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
[SuperWeaponTypes]\n0=RootSpecial\n\
[RootSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n";

fn rules_with(extra: &str) -> RuleSet {
    let mut ini = IniFile::from_str(BASE);
    ini.merge(&IniFile::from_str(extra));
    RuleSet::from_ini_with_fixed_art_for_test(
        &ini,
        &IniFile::from_str("[PROVIDER]\nFoundation=1x1\n"),
    )
    .expect("rules with the Detonate keys parse")
}

fn world(rules: RuleSet) -> (RuleSet, Simulation, InternedId) {
    world_with(rules, 64, &[])
}

fn spawn_unit(sim: &mut Simulation, rules: &RuleSet, cell: (u16, u16)) -> u64 {
    sim.spawn_object("E1", "Russians", cell.0, cell.1, 0, rules)
        .expect("E1 spawns")
}

fn health(sim: &Simulation, id: u64) -> i32 {
    sim.substrate
        .entities
        .get(id)
        .map_or(0, |entity| entity.health.current())
}

/// `Detonate.Warhead` with the damage override: the full-detonation bullet
/// lands on the target cell only.
#[test]
fn detonate_warhead_hits_the_target_cell() {
    let rules = rules_with("[RootSpecial]\nDetonate.Warhead=WH\nDetonate.Damage=40\n");
    let detonate = rules
        .phobos_super_weapon_detonate(ROOT)
        .expect("the reader kept the detonation");
    assert_eq!(detonate.warhead(), Some("WH"));
    assert_eq!(detonate.damage(), Some(40));
    assert!(!detonate.is_empty());
    let (rules, mut sim, owner) = world(rules);
    charge_super(&mut sim, owner, ROOT);
    let hit = spawn_unit(&mut sim, &rules, TARGET);
    let far = spawn_unit(&mut sim, &rules, (33, 30));
    let sw_type = sim.interner.get(ROOT).expect("interned root");
    assert!(
        super::is_charged(&sim, owner, sw_type),
        "the super is charged"
    );
    assert!(
        sim.fire_super_weapon(
            &rules,
            owner,
            sw_type,
            TARGET,
            None,
            crate::sim::world::FrameEffects::default(),
            false,
        ),
        "the launch was admitted"
    );
    assert_eq!(health(&sim, hit), 60, "the warhead detonated at the cell");
    assert_eq!(
        health(&sim, far),
        100,
        "the 1-cell spread leaves the neighbour"
    );
}

/// `Detonate.Warhead.Full=no` (`MapClass::DamageArea`, `:301`): the direct
/// area pass still damages the impact cell.
#[test]
fn detonate_warhead_full_no_uses_the_area_pass() {
    let rules = rules_with(
        "[RootSpecial]\nDetonate.Warhead=WH\nDetonate.Damage=40\nDetonate.Warhead.Full=no\n",
    );
    let (rules, mut sim, owner) = world(rules);
    charge_super(&mut sim, owner, ROOT);
    let hit = spawn_unit(&mut sim, &rules, TARGET);
    let far = spawn_unit(&mut sim, &rules, (33, 30));
    click(&mut sim, &rules, owner, ROOT, TARGET);
    assert_eq!(health(&sim, hit), 60, "DamageArea hit the cell");
    assert_eq!(health(&sim, far), 100, "the spread stops at one cell");
}

/// `Detonate.Weapon` uses the weapon's damage when `Detonate.Damage` is
/// unset, and the override when set.
#[test]
fn detonate_weapon_uses_and_overrides_its_damage() {
    let rules = rules_with("[RootSpecial]\nDetonate.Weapon=GUN\n");
    let weapon = rules.weapon("GUN").expect("the weapon resolves");
    assert_eq!(weapon.warhead.as_deref(), Some("WH"));
    let (rules, mut sim, owner) = world(rules);
    charge_super(&mut sim, owner, ROOT);
    let hit = spawn_unit(&mut sim, &rules, TARGET);
    click(&mut sim, &rules, owner, ROOT, TARGET);
    assert_eq!(health(&sim, hit), 75, "the weapon's 25 damage applied");

    let rules = rules_with("[RootSpecial]\nDetonate.Weapon=GUN\nDetonate.Damage=40\n");
    let (rules, mut sim, owner) = world(rules);
    charge_super(&mut sim, owner, ROOT);
    let hit = spawn_unit(&mut sim, &rules, TARGET);
    click(&mut sim, &rules, owner, ROOT, TARGET);
    assert_eq!(health(&sim, hit), 60, "the override replaced the damage");
}

/// `Detonate.AtFirer` with no launch building detonates at
/// `CoordStruct::Empty`, i.e. cell (0,0), not at the target cell.
#[test]
fn detonate_at_firer_without_a_building_uses_the_empty_coordinate() {
    let rules = rules_with(
        "[RootSpecial]\nDetonate.Warhead=WH\nDetonate.Damage=40\nDetonate.AtFirer=yes\n",
    );
    let (rules, mut sim, owner) = world(rules);
    charge_super(&mut sim, owner, ROOT);
    let origin = spawn_unit(&mut sim, &rules, (0, 0));
    let target = spawn_unit(&mut sim, &rules, TARGET);
    click(&mut sim, &rules, owner, ROOT, TARGET);
    assert_eq!(health(&sim, origin), 60, "Empty falls on cell (0,0)");
    assert_eq!(health(&sim, target), 100, "the target cell was not hit");
}

/// Phobos `SuperWeapons=` names a building as the detonation firer; the key
/// is read without the Ares capability too.
#[test]
fn a_superweapons_list_names_the_detonation_firer() {
    let rules = rules_with(
        "[RootSpecial]
Detonate.Warhead=WH
         [PROVIDER]
SuperWeapon=none
SuperWeapons=RootSpecial
",
    );
    let provider = rules.object("PROVIDER").expect("provider type");
    assert!(
        super::type_super_weapons(provider).any(|name| name.eq_ignore_ascii_case(ROOT)),
        "the additional list names the detonation firer"
    );
}

