//! Original constructor/reader/layer outputs through the production owner.
use super::*;
use crate::rules::{
    ini_parser::IniFile,
    native_processing::{NativeRulesRegistryState, RulesLayerStack},
    ruleset::RuleSet,
};
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text("tools/voxel_oracle/recoil.json")).unwrap()
}

fn ini(sections: &Value) -> IniFile {
    let mut result = IniFile::empty();
    for (name, keys) in sections.as_object().unwrap() {
        let mut section = IniSection::new(name.clone());
        for (key, value) in keys.as_object().unwrap() {
            section.set(key, value.as_str().unwrap());
        }
        result.replace_first_section(section);
    }
    result
}

fn assert_config(config: RecoilConfig, expected: &Value) {
    assert_eq!(config.enabled(), expected["enabled"].as_bool().unwrap());
    for (control, name) in config.controls().into_iter().zip(["turret", "barrel"]) {
        let expected: Vec<_> = expected[name]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap() as i32)
            .collect();
        assert_eq!(control.parameters().as_slice(), expected);
    }
}

#[test]
fn recoil_constructor_and_reader_match_original() {
    let native = corpus();
    assert_config(RecoilConfig::default(), &native["constructor"]);
    for row in native["readers"].as_array().unwrap() {
        assert_config(
            RecoilConfig::from_ini_section(ini(&row["sections"]).section_or_empty("GTGCAN")),
            &row["after"],
        );
    }
}

#[test]
fn recoil_reached_sections_reset_barrel_through_retained_registry() {
    let native = corpus();
    let mut state = NativeRulesRegistryState::default();
    for (index, row) in native["layers"].as_array().unwrap().iter().enumerate() {
        let mut pass = ini(&row["sections"]);
        if index == 0 {
            pass.merge(&IniFile::from_str("[BuildingTypes]\n0=GTGCAN\n"));
        }
        let processed = RulesLayerStack::new(pass)
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
            .unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_config(rules.object("GTGCAN").unwrap().recoil, &row["after"]);
        let (_, trace) = processed.into_ini_and_native_type_construction_trace();
        state = trace.into_registry_state_discarding_events();
    }
}

#[test]
fn recoil_retail_reader_and_scenario_layers_match_original() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let native = corpus();
    for row in native["physical"].as_array().unwrap() {
        assert_config(
            retail
                .rules
                .object(row["name"].as_str().unwrap())
                .unwrap()
                .recoil,
            &row["config"],
        );
    }
}

#[test]
fn recoil_retained_configuration_participates_in_restore_fingerprint() {
    let mut rules = Vec::new();
    for earlier in [
        "[BuildingTypes]\n0=GTGCAN\n[GTGCAN]\nTurretRecoil=yes\nBarrelTravel=8\n",
        "[BuildingTypes]\n0=GTGCAN\n",
    ] {
        let processed = RulesLayerStack::new(IniFile::from_str(earlier))
            .process()
            .unwrap();
        let (_, trace) = processed.into_ini_and_native_type_construction_trace();
        let processed = RulesLayerStack::new(IniFile::empty())
            .process_with_fixed_art_and_registry_state(
                &IniFile::empty(),
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
        rules.push(RuleSet::from_processed_rules(&processed).unwrap());
    }
    assert_eq!(rules[0].source_ini_hash(), rules[1].source_ini_hash());
    assert_ne!(
        rules[0].simulation_config_hash(),
        rules[1].simulation_config_hash()
    );
}
