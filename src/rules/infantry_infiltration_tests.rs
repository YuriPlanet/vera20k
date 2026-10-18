use super::{ObjectCategory, ObjectType};
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;

/// Infantry ReadINI52466E..52469A runs after the explicit Infiltrate read.
#[test]
fn infantry_roles_force_infiltrate_even_after_explicit_no() {
    for role in ["C4", "Engineer", "Agent"] {
        let ini = IniFile::from_str(&format!("[TEST]\nInfiltrate=no\n{role}=yes\n"));
        let section = ini.section("TEST").unwrap();
        let infantry = ObjectType::from_ini_section("TEST", section, ObjectCategory::Infantry);
        assert!(infantry.infiltrate, "{role} overrides Infiltrate=no");
        for category in [
            ObjectCategory::Vehicle,
            ObjectCategory::Building,
            ObjectCategory::Aircraft,
        ] {
            assert!(!ObjectType::from_ini_section("TEST", section, category).infiltrate);
        }
    }
    for (flags, expected) in [
        ("", false),
        ("Infiltrate=yes", true),
        ("c4=yes\nengineer=yes\nagent=yes", false),
    ] {
        let ini = IniFile::from_str(&format!("[TEST]\n{flags}\n"));
        assert_eq!(
            ObjectType::from_ini_section(
                "TEST",
                ini.section_or_empty("TEST"),
                ObjectCategory::Infantry
            )
            .infiltrate,
            expected
        );
    }
}

/// An absent key defaults to the last postprocessed byte, not merged INI text.
#[test]
fn infiltration_postread_state_survives_rules_layer_changes() {
    for role in ["C4", "Engineer", "Agent"] {
        let mut layers = RulesLayerStack::new(IniFile::from_str(&format!(
            "[InfantryTypes]\n0=TEST\n[TEST]\nStrength=100\nInfiltrate=no\n{role}=yes\n"
        )));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[TEST]\nC4=no\nEngineer=no\nAgent=no\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        let object = rules.object("TEST").unwrap();
        assert!(!object.c4 && !object.engineer && !object.agent);
        assert!(object.infiltrate, "the {role} pass already set the byte");
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[TEST]\nInfiltrate=no\n"),
        );
        assert!(
            !RuleSet::from_rules_layers(&layers)
                .unwrap()
                .object("TEST")
                .unwrap()
                .infiltrate
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[TEST]\nAgent=yes\nInfiltrate=no\n"),
        );
        assert!(
            RuleSet::from_rules_layers(&layers)
                .unwrap()
                .object("TEST")
                .unwrap()
                .infiltrate
        );
    }
}

#[test]
fn retail_specialist_infantry_are_infiltrators_without_becoming_spies() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    for name in [
        "ENGINEER",
        "SENGINEER",
        "YENGINEER",
        "GHOST",
        "PTROOP",
        "TANY",
        "SPY",
    ] {
        let object = retail.rules.object(name).unwrap();
        assert!(object.infiltrate, "{name}");
        assert_eq!(object.agent, name == "SPY", "{name}");
    }
    assert!(!retail.rules.object("E1").unwrap().infiltrate);
}
