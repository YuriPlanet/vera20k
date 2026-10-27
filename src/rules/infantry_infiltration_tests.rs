use super::{ObjectCategory, ObjectType};
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;

#[test]
fn thief_capture_flags_retain_native_reads_and_legacy_save_projection() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[InfantryTypes]\n0=THIEF\n[THIEF]\nThief=yes\nVehicleThief=yes\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[THIEF]\nThief=unknown\nVehicleThief=unknown\n"),
    );
    let retained = RuleSet::from_rules_layers(&layers).unwrap();
    let ty = retained.object("THIEF").unwrap();
    assert!(ty.thief && ty.vehicle_thief);
    assert!(!ty.infiltrate, "neither theft byte forces Infiltrate");

    let flat = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=THIEF\n[THIEF]\nThief=unknown\nVehicleThief=unknown\n",
    ))
    .unwrap();
    assert!(!flat.object("THIEF").unwrap().thief);
    assert_ne!(
        retained.simulation_config_hash(),
        flat.simulation_config_hash()
    );

    // Persisted current-default bytes survive rebind; old processing states
    // without them keep the raw projection used before these fields existed.
    let ini = IniFile::from_str("[THIEF]\nThief=yes\nVehicleThief=yes\n");
    let raw = ini.section("THIEF").unwrap();
    let mut state = super::InfantryInfiltration::default();
    state.apply_pass(raw);
    let state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    let invalid_ini = IniFile::from_str("[THIEF]\nThief=unknown\nVehicleThief=unknown\n");
    let mut ty = ObjectType::from_ini_section(
        "THIEF",
        invalid_ini.section("THIEF").unwrap(),
        ObjectCategory::Infantry,
    );
    ty.project_infantry_infiltration(state);
    assert!(ty.thief && ty.vehicle_thief);
    let legacy =
        serde_json::from_str(r#"{"infiltrate":false,"c4":false,"engineer":false,"agent":false}"#)
            .unwrap();
    let mut ty = ObjectType::from_ini_section("THIEF", raw, ObjectCategory::Infantry);
    ty.project_infantry_infiltration(legacy);
    assert!(ty.thief && ty.vehicle_thief);
}

/// Infantry ReadINI52466E..52469A runs after the explicit Infiltrate read.
#[test]
fn infantry_roles_force_infiltrate_even_after_explicit_no() {
    let infantry = |body: &str| {
        let ini = IniFile::from_str(&format!("[InfantryTypes]\n0=TEST\n[TEST]\n{body}\n"));
        RuleSet::from_ini(&ini)
            .unwrap()
            .object("TEST")
            .unwrap()
            .infiltrate
    };
    for role in ["C4", "Engineer", "Agent"] {
        let ini = IniFile::from_str(&format!("[TEST]\nInfiltrate=no\n{role}=yes\n"));
        let section = ini.section("TEST").unwrap();
        assert!(
            infantry(&format!("Infiltrate=no\n{role}=yes")),
            "{role} overrides Infiltrate=no"
        );
        for category in [
            ObjectCategory::Vehicle,
            ObjectCategory::Building,
            ObjectCategory::Aircraft,
        ] {
            assert!(!ObjectType::from_ini_section("TEST", section, category).infiltrate);
        }
    }
    for (flags, expected) in [
        ("Strength=100", false),
        ("Infiltrate=yes", true),
        ("c4=yes\nengineer=yes\nagent=yes", false),
    ] {
        assert_eq!(infantry(flags), expected, "{flags}");
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
