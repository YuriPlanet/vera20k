//! `Grinding.*` reads (`Ext/BuildingType/Body.cpp:200-207`).
use super::PhobosGrindingSettings;
use crate::rules::ini_parser::IniFile;

fn read(text: &str) -> PhobosGrindingSettings {
    PhobosGrindingSettings::from_section(IniFile::from_str(text).section_or_empty("GRINDER"))
}

#[test]
fn grinding_defaults_match_the_constructor() {
    let set = read("[GRINDER]\nGrinding=yes\n");
    assert_eq!(set, PhobosGrindingSettings::default());
    assert!(!set.allow_allies);
    assert!(set.allow_owner);
    assert!(set.play_die_sound);
    assert!(set.allow_types.is_empty() && set.disallow_types.is_empty());
    assert_eq!(set.weapon_required_credits, 0);
    assert_eq!(set.sound, None);
    assert_eq!(set.weapon, None);
}

#[test]
fn grinding_reads_its_gates_types_weapon_and_credits() {
    let set = read(
        "[GRINDER]\nGrinding=yes\nGrinding.AllowAllies=yes\n\
         Grinding.AllowOwner=no\nGrinding.AllowTypes=MTNK,HTNK\n\
         Grinding.DisallowTypes=,E1\nGrinding.PlayDieSound=no\n\
         Grinding.Weapon=GRINDWPN\nGrinding.Weapon.RequiredCredits=250\n",
    );
    assert!(set.allow_allies);
    assert!(!set.allow_owner);
    assert_eq!(set.allow_types, ["MTNK", "HTNK"]);
    assert_eq!(set.disallow_types, ["E1"]);
    assert!(!set.play_die_sound);
    assert_eq!(set.weapon.as_deref(), Some("GRINDWPN"));
    assert_eq!(set.weapon_required_credits, 250);
}

/// A value of `ValueableVector` that names no type stays in the parsed list;
/// the consumer resolves names against the ruleset and treats the list as
/// empty when none of them exist.
#[test]
fn grinding_keeps_unresolved_type_names_for_the_consumer() {
    let set = read("[GRINDER]\nGrinding.AllowTypes=NOT_A_TYPE\n");
    assert_eq!(set.allow_types, ["NOT_A_TYPE"]);
}
