//! Original scalar voice readers, fixed catalog and reached Rules passes.
//! Source: input_oracle/unit_voice_playback.py's --slave-miner-pack-voice
//! mode and input_oracle/slave_miner_pack_voice.{json,meta.json}.

use super::ini_parser::IniFile;
use super::native_processing::{RulesLayerKind, RulesLayerStack};
use super::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
use super::ruleset::RuleSet;
use super::sound_ini::SoundRegistry;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) fn native() -> Value {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/slave_miner_pack_voice.json",
    ))
    .unwrap();
    assert_eq!(corpus["schema_version"], 1);
    corpus
}

fn section(name: &str, keys: &Value) -> String {
    let mut text = format!("[{name}]\n");
    for (key, value) in keys.as_object().unwrap() {
        text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
    }
    text
}

fn sounds(corpus: &Value) -> SoundRegistry {
    let mut text = String::new();
    for (name, keys) in corpus["sound_registry"]["selected"].as_object().unwrap() {
        text.push_str(&section(name, keys));
    }
    SoundRegistry::from_ini(&IniFile::from_str(&text))
}

fn project(layers: &RulesLayerStack, sounds: &SoundRegistry) -> RuleSet {
    let processed = layers.process().unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    rules.bind_type_sound_references(processed.ini(), sounds);
    rules
}

fn assert_binding(rules: &RuleSet, expected: &Value, context: &str) {
    let object = rules.object("YAREFN").unwrap();
    for (key, value) in [
        ("voice_deploy_name", json!(object.voice_deploy())),
        ("voice_undeploy_name", json!(object.voice_undeploy())),
        ("voice_harvest_name", json!(object.voice_harvest())),
        ("resource_gatherer", json!(object.resource_gatherer)),
        ("resource_destination", json!(object.resource_destination)),
    ] {
        assert_eq!(value, expected[key], "{context}: {key}");
    }
}

pub(crate) fn control_rules(corpus: &Value, input: &Value) -> RuleSet {
    let mut text = String::from("[BuildingTypes]\n0=YAREFN\n[YAREFN]\nStrength=2000\n");
    for (field, key) in [
        ("voice_deploy", "VoiceDeploy"),
        ("voice_harvest", "VoiceHarvest"),
    ] {
        if let Some(name) = input[field].as_str() {
            text.push_str(&format!("{key}={name}\n"));
        }
    }
    for (field, key) in [
        ("resource_gatherer", "ResourceGatherer"),
        ("resource_destination", "ResourceDestination"),
    ] {
        text.push_str(&format!(
            "{key}={}\n",
            if input[field].as_bool().unwrap() {
                "yes"
            } else {
                "no"
            }
        ));
    }
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::from_str(&text),
        None,
        IniFile::from_str(""),
        Arc::new(sounds(corpus)),
    )
    .unwrap();
    owner
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(None),
            &IniFile::from_str(""),
        )
        .unwrap()
        .into_parts()
        .0
}

#[test]
fn scalar_voice_defaults_and_retained_reached_passes_match_original_readers() {
    let corpus = native();
    let sounds = sounds(&corpus);
    // The registered type's construction is an explicit native fixture prior.
    let mut root = IniFile::from_str("[BuildingTypes]\n0=YAREFN\n");
    let mut layers = RulesLayerStack::new(root.clone());
    assert_binding(
        &project(&layers, &sounds),
        &corpus["type_constructor"],
        "constructor",
    );
    for row in corpus["retail_layers"].as_array().unwrap() {
        if row["absent"] == true {
            continue;
        }
        let pass = IniFile::from_str(&section("YAREFN", &row["keys"]));
        match row["file"].as_str().unwrap() {
            "RULESMD.INI" => {
                root.merge(&pass);
                layers = RulesLayerStack::new(root.clone());
            }
            "LANGRULE.INI" => layers.push(RulesLayerKind::LangRule, pass),
            "MPBattleMD.ini" => layers.push(RulesLayerKind::GameMode, pass),
            "XMP03T4.MAP" => layers.push(RulesLayerKind::Scenario, pass),
            other => panic!("unexpected layer {other}"),
        }
        assert_binding(
            &project(&layers, &sounds),
            &row["after"],
            row["file"].as_str().unwrap(),
        );
    }
    let controls = corpus["reader_controls"].as_array().unwrap();
    assert_eq!(controls.len(), 12);
    for row in controls {
        let context = row["name"].as_str().unwrap();
        assert_binding(&project(&layers, &sounds), &row["before"], context);
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(&section("YAREFN", &row["keys"])),
        );
        assert_binding(&project(&layers, &sounds), &row["after"], context);
    }
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn stock_pack_voice_slots_match_physical_anytown_battle_production_rules() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let corpus = native();
    assert_binding(
        &retail.rules,
        &corpus["retail_layers"][3]["after"],
        "physical production rules",
    );
    let object = retail.rules.object("YAREFN").unwrap();
    assert_eq!(object.deploy_sound.as_deref(), Some("SlaveMinerUndeploy"));
    let gi = retail.rules.object("E1").unwrap();
    assert_eq!((gi.voice_deploy(), gi.voice_undeploy()), (None, None));
}
