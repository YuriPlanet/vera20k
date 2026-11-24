//! `WeaponTypeExt::LoadFromINIFile`'s five strafing reads
//! (`Ext/WeaponType/Body.cpp:108-112`) and their `[General]` fallbacks
//! (`Ext/Rules/Body.cpp:264-266`).

use super::PhobosStrafingRules;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::phobos_globals::PhobosGlobals;
use crate::rules::ruleset::RuleSet;

fn read(ini: &str, section: &str) -> PhobosStrafingRules {
    let file = IniFile::from_str(ini);
    PhobosStrafingRules::from_section(file.section_or_empty(section))
}

#[test]
fn defaults_are_unset_and_shots_reads_five() {
    let set = PhobosStrafingRules::default();
    assert!(set.is_default());
    assert_eq!(set.shots(), 5, "Strafing_Shots.Get(5)");
    let globals = PhobosGlobals::default();
    assert!(!set.simulate_burst(&globals));
    assert!(!set.use_ammo_per_shot(&globals));
    assert!(!set.target_cell(&globals));
    assert_eq!(set.end_delay, None);
}

#[test]
fn reads_the_five_keys_and_the_clearing_tokens_reset_them() {
    let set = read(
        "[W]\nStrafing.Shots=7\nStrafing.SimulateBurst=yes\nStrafing.UseAmmoPerShot=yes\n\
         Strafing.TargetCell=yes\nStrafing.EndDelay=33\n",
        "W",
    );
    assert_eq!(set.shots, Some(7));
    assert_eq!(set.simulate_burst, Some(true));
    assert_eq!(set.use_ammo_per_shot, Some(true));
    assert_eq!(set.target_cell, Some(true));
    assert_eq!(set.end_delay, Some(33));
    assert!(!set.is_default());

    for value in ["<none>", "none", "<default>"] {
        let ini = format!(
            "[W]\nStrafing.Shots={value}\nStrafing.SimulateBurst={value}\n\
             Strafing.UseAmmoPerShot={value}\nStrafing.TargetCell={value}\n\
             Strafing.EndDelay={value}\n"
        );
        assert!(read(&ini, "W").is_default(), "[{value}]");
    }
}

/// The globals are `[General]` `Valueable<bool>`s (constructor false); a
/// weapon whose boolean key is unset falls back to them, and a later rules
/// pass that names the key overrides the fallback.
#[test]
fn unset_weapon_booleans_fall_back_to_the_general_globals() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[General]\nStrafing.SimulateBurst=yes\nStrafing.UseAmmoPerShot=yes\n\
         Strafing.TargetCell=yes\n\
         [VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=Gun\n\
         [Gun]\nDamage=1\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[Gun]\nStrafing.TargetCell=no\n"),
    );
    let rules = RuleSet::from_rules_layers(&layers).unwrap();
    assert!(rules.phobos_globals.strafing_simulate_burst);
    assert!(rules.phobos_globals.strafing_use_ammo_per_shot);
    assert!(rules.phobos_globals.strafing_target_cell);
    let gun = rules.weapon("Gun").unwrap();
    let globals = &rules.phobos_globals;
    assert!(gun.phobos_strafing().simulate_burst(globals));
    assert!(gun.phobos_strafing().use_ammo_per_shot(globals));
    assert!(
        !gun.phobos_strafing().target_cell(globals),
        "the scenario's no is a value, not a fallback"
    );
    assert_eq!(gun.phobos_strafing().shots(), 5);
}
