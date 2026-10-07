//! Retained SelectAnim inputs from original constructors/readers. The selector
//! itself has a separate execution corpus; these tests own only rules handoff.
use super::*;
use crate::rules::ruleset::RuleSet;
use serde_json::Value;

fn authored_ini(sections: &Value) -> IniFile {
    let mut text = String::new();
    for (section, keys) in sections.as_object().unwrap() {
        text.push_str(&format!("[{section}]\n"));
        for (key, value) in keys.as_object().unwrap() {
            text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
    }
    IniFile::from_str(&text)
}

fn names(values: &Value) -> Vec<String> {
    values
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn select_anim_inputs_match_native_retained_passes() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/select_anim_inputs.json",
    ))
    .unwrap();
    let rows = native["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 12);
    let mut state = NativeRulesRegistryState::default();
    let mut empty_pass_hashes = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let name = row["name"].as_str().unwrap();
        let mut pass = authored_ini(&row["authored_sections"]);
        if index == 0 {
            // Oracle preconstructs WH; declare it at the production discovery
            // boundary. No body is authored in this first constructor control.
            pass.merge(&IniFile::from_str("[Warheads]\n0=WH\n"));
        }
        let processed = RulesLayerStack::new(pass)
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
            .unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        let warhead = rules.warhead("WH").unwrap();
        let expected = &row["after"];
        assert_eq!(
            warhead.conventional,
            expected["conventional"].as_bool().unwrap(),
            "{name}"
        );
        assert_eq!(
            warhead.em_effect,
            expected["em_effect"].as_bool().unwrap(),
            "{name}"
        );
        assert_eq!(warhead.anim_list, names(&expected["anim_list"]), "{name}");
        assert_eq!(
            rules.general.lightning_warhead,
            expected["lightning_warhead"].as_str().unwrap_or(""),
            "{name}"
        );
        assert_eq!(
            rules.general.weather_con_bolt_explosion,
            expected["weather_con_bolt_explosion"]
                .as_str()
                .unwrap_or(""),
            "{name}"
        );
        assert_eq!(
            rules.general.weapon_nullify_anim,
            expected["weapon_nullify_anim"].as_str().unwrap_or(""),
            "{name}"
        );
        assert_eq!(
            rules.combat_damage.splash_list,
            names(&expected["splash_list"]),
            "{name}"
        );
        if matches!(name, "missing_retains" | "missing_after_clear") {
            empty_pass_hashes.push((rules.source_ini_hash(), rules.simulation_config_hash()));
        }
        let (_, receipt) = processed.into_ini_and_native_type_construction_trace();
        state = receipt.into_registry_state_discarding_events();
    }
    assert_eq!(empty_pass_hashes[0].0, empty_pass_hashes[1].0);
    assert_ne!(
        empty_pass_hashes[0].1, empty_pass_hashes[1].1,
        "empty current INIs with distinct retained selector inputs must reject restore"
    );
}

#[test]
fn type_reset_clears_retired_references_then_rebuilds_native_physical_bindings() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/select_anim_reset.json",
    ))
    .unwrap();
    let selected = &native["physical_reread"];
    let pass = authored_ini(&selected["cached_reread"]);
    let processed = RulesLayerStack::new(pass.clone())
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            NativeRulesRegistryState::default(),
        )
        .unwrap();
    let (_, receipt) = processed.into_ini_and_native_type_construction_trace();
    let reset = receipt
        .into_registry_state_discarding_events()
        .destructive_reset();
    let cleared = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), reset)
        .unwrap();
    let cleared_rules = RuleSet::from_processed_rules(&cleared).unwrap();
    // Native clears LightningWarhead but leaves freed Anim pointers. Empty
    // Rust references are an explicit safety policy, not a stale-pointer golden.
    assert!(cleared_rules.general.lightning_warhead.is_empty());
    assert!(cleared_rules.general.weather_con_bolt_explosion.is_empty());
    assert!(cleared_rules.general.weapon_nullify_anim.is_empty());
    assert!(cleared_rules.combat_damage.splash_list.is_empty());
    assert!(cleared_rules.warhead("IonWH").is_none());
    let (_, receipt) = cleared.into_ini_and_native_type_construction_trace();
    let rebuilt = RulesLayerStack::new(pass)
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            receipt.into_registry_state_discarding_events(),
        )
        .unwrap();
    let rebuilt = RuleSet::from_processed_rules(&rebuilt).unwrap();
    let expected = &selected["after_named"];
    assert_eq!(
        rebuilt.general.lightning_warhead,
        expected["lightning_warhead"].as_str().unwrap()
    );
    assert_eq!(
        rebuilt.general.weather_con_bolt_explosion,
        expected["weather_con_bolt_explosion"].as_str().unwrap()
    );
    assert_eq!(
        rebuilt.general.weapon_nullify_anim,
        expected["weapon_nullify_anim"].as_str().unwrap()
    );
    assert_eq!(
        rebuilt.combat_damage.splash_list,
        names(&expected["splash_list"])
    );
    let wh = rebuilt.warhead(&rebuilt.general.lightning_warhead).unwrap();
    assert_eq!(wh.conventional, expected["conventional"].as_bool().unwrap());
    assert_eq!(wh.em_effect, expected["em_effect"].as_bool().unwrap());
    assert_eq!(wh.anim_list, names(&expected["anim_list"]));
}
