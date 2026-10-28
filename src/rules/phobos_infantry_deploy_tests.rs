//! Phobos `InfantryAutoDeploy` resolution: `[General]` then the InfantryType
//! key, through the production readers.
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;

fn rules(global: &str, techno: &str, section: &str) -> RuleSet {
    let ini = format!(
        "[General]\n{global}[{section}]\n0=TEST\n[TEST]\nStrength=100\n{techno}"
    );
    RuleSet::from_ini(&IniFile::from_str(&ini)).expect("rules read")
}

/// Retail YR leaves both keys unset, which keeps the deploy arm dead; the
/// global and the TechnoType key each turn it on, and the type wins.
#[test]
fn auto_deploy_resolves_the_techno_key_then_the_general_default() {
    for (global, techno, expected) in [
        ("", "", false),
        ("InfantryAutoDeploy=no\n", "", false),
        ("InfantryAutoDeploy=yes\n", "", true),
        ("", "InfantryAutoDeploy=yes\n", true),
        ("InfantryAutoDeploy=no\n", "InfantryAutoDeploy=yes\n", true),
        ("InfantryAutoDeploy=yes\n", "InfantryAutoDeploy=no\n", false),
    ] {
        let rules = rules(global, techno, "InfantryTypes");
        let ty = rules.object("TEST").expect("TEST type");
        assert_eq!(
            rules.infantry_auto_deploy_for(ty),
            expected,
            "global {global:?} techno {techno:?}"
        );
    }
}

/// Phobos holds the key in `InfantryTypeExt`, so a section outside
/// `[InfantryTypes]` carries no value even when it repeats the key.
#[test]
fn only_infantry_sections_carry_the_key() {
    let rules = rules("", "InfantryAutoDeploy=yes\n", "VehicleTypes");
    let ty = rules.object("TEST").expect("TEST type");
    assert!(!rules.infantry_auto_deploy_for(ty));
}
