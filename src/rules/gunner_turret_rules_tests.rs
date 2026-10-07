//! Native reader outputs through the typed reader and retained production owner.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{NativeRulesRegistryState, RulesLayerStack};
use crate::rules::object_type::ObjectType;
use crate::rules::process_owner::NativeRulesProcessOwner;
use crate::rules::ruleset::RuleSet;
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/ifv_turret_switching.json",
    ))
    .unwrap()
}

fn cached_ini(sections: &Value) -> IniFile {
    let mut ini = IniFile::empty();
    for (name, keys) in sections.as_object().unwrap() {
        let mut section = IniSection::new(name.clone());
        for (key, value) in keys.as_object().unwrap() {
            section.set(key, value.as_str().unwrap());
        }
        ini.replace_first_section(section);
    }
    ini
}

fn assert_config(config: &GunnerTurrets, native: &Value, context: &str) {
    assert_eq!(
        config.is_charge_turret(),
        native["is_charge_turret"].as_bool().unwrap(),
        "charge flag: {context}"
    );
    let table = (0..WEAPON_SLOT_COUNT)
        .map(|weapon| config.turret_for_weapon(weapon as i32))
        .collect::<Vec<_>>();
    let expected = native["turrets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_i64().unwrap() as i32)
        .collect::<Vec<_>>();
    assert_eq!(table, expected, "mapping table: {context}");
}

fn assert_object(object: &ObjectType, native: &Value, context: &str) {
    assert_config(&object.gunner_turrets, native, context);
    assert_eq!(
        object.ifv_mode,
        native["ifv_mode"].as_i64().unwrap() as i32,
        "signed IFVMode: {context}"
    );
}

#[test]
fn native_gunner_reader_controls_match() {
    let native = corpus();
    assert_config(
        &GunnerTurrets::default(),
        &native["readers"]["constructor"],
        "constructor",
    );
    for row in native["readers"]["rows"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        let ini = cached_ini(&row["sections"]);
        let sections = ini.section_names();
        let id = sections.first().copied().unwrap_or("FV");
        let category = if id == "E1" {
            ObjectCategory::Infantry
        } else {
            ObjectCategory::Vehicle
        };
        let object = ObjectType::from_ini_section(id, ini.section_or_empty(id), category);
        assert_object(&object, &row["after"], name);
    }
}

#[test]
fn native_layer_history_survives_process_registry_handoff() {
    let native = corpus();
    let mut state = NativeRulesRegistryState::default();
    for (index, row) in native["readers"]["layer_history"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let mut pass = cached_ini(&row["sections"]);
        if index == 0 {
            pass.merge(&IniFile::from_str("[VehicleTypes]\n0=FV\n"));
        }
        let processed = RulesLayerStack::new(pass)
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
            .unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_object(
            rules.object("FV").unwrap(),
            &row["after"],
            row["name"].as_str().unwrap(),
        );
        let (_, trace) = processed.into_ini_and_native_type_construction_trace();
        state = trace.into_registry_state_discarding_events();
    }
}

#[test]
fn production_root_langrule_mode_map_order_matches_native_reader_history() {
    let native = corpus();
    let history = native["readers"]["layer_history"].as_array().unwrap();
    let mut root = cached_ini(&history[0]["sections"]);
    root.merge(&IniFile::from_str("[VehicleTypes]\n0=FV\n"));
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        Some(cached_ini(&history[1]["sections"])),
        IniFile::empty(),
        std::sync::Arc::default(),
    )
    .unwrap();
    for index in [3, 4] {
        let (rules, _, _, _) = owner
            .load_noncampaign_scenario(
                Some(&cached_ini(&history[2]["sections"])),
                &cached_ini(&history[index]["sections"]),
            )
            .unwrap()
            .into_parts();
        assert_object(
            rules.object("FV").unwrap(),
            &history[index]["after"],
            history[index]["name"].as_str().unwrap(),
        );
    }
}

#[test]
fn identical_current_sources_hash_retained_gunner_configuration() {
    let native = corpus();
    let mut states = Vec::new();
    for earlier in [
        "[VehicleTypes]\n0=FV\n[FV]\nNormalTurretWeapon=0\nNormalTurretIndex=3\n",
        "[VehicleTypes]\n0=FV\n",
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
        states.push(RuleSet::from_processed_rules(&processed).unwrap());
    }
    assert_config(
        &states[0].object("FV").unwrap().gunner_turrets,
        &native["readers"]["layer_history"][0]["after"],
        "retained authored table",
    );
    assert_config(
        &states[1].object("FV").unwrap().gunner_turrets,
        &native["readers"]["constructor"],
        "retained constructor table",
    );
    assert_eq!(states[0].source_ini_hash(), states[1].source_ini_hash());
    assert_ne!(
        states[0].simulation_config_hash(),
        states[1].simulation_config_hash(),
        "restore must reject different retained mappings for identical current sources"
    );
}

#[test]
fn retail_root_ifv_and_passengers_match_original_readers() {
    let Some((root, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        None,
        art,
        std::sync::Arc::default(),
    )
    .unwrap();
    let (rules, _, _, _) = owner
        .load_noncampaign_scenario(None, &IniFile::empty())
        .unwrap()
        .into_parts();
    let native = corpus();
    let root = native["physical"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["file"] == "rulesmd.ini")
        .unwrap();
    assert_object(
        rules.object("FV").unwrap(),
        &root["after"],
        "retail root FV",
    );
    for passenger in native["physical"]["infantry"].as_array().unwrap() {
        let id = passenger["type"].as_str().unwrap();
        assert_object(rules.object(id).unwrap(), &passenger["after"], id);
    }
}

// Hills runs through production source selection here. The native component
// corpus establishes the stock FV baseline, not the complete Hills Rules pass.
#[test]
fn retail_battle_hills_preserves_native_ifv_baseline() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let native = corpus();
    assert_object(
        retail.rules.object("FV").unwrap(),
        &native["physical"]["layers"][0]["after"],
        "production Battle/Hills against the native retail FV baseline",
    );
}
