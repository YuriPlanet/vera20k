//! Original GuardSound66AE1C and VoiceSpecialAttack712CC5 reader results.
//! Expected lists and defaults come from the executed Area Guard corpus;
//! the fixture supplies its selected physical sections to the production
//! layer processor and fixed SOUNDMD resolver.

use super::ini_parser::IniFile;
use super::native_processing::{RulesLayerKind, RulesLayerStack};
use super::process_owner::NativeRulesProcessOwner;
use super::ruleset::RuleSet;
use super::sound_ini::SoundRegistry;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) fn native() -> Value {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/area_guard.json",
    ))
    .expect("checked original Area Guard corpus");
    assert_eq!(
        corpus["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    corpus
}

fn sections_ini(sections: &Value) -> IniFile {
    let mut text = String::new();
    for (name, entries) in sections.as_object().unwrap() {
        text.push_str(&format!("[{name}]\n"));
        for (key, value) in entries.as_object().unwrap() {
            text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
    }
    IniFile::from_str(&text)
}

pub(crate) fn catalog(corpus: &Value) -> SoundRegistry {
    SoundRegistry::from_ini(&sections_ini(
        &corpus["retail"]["sound_registry"]["selected"],
    ))
}

pub(crate) fn voice_overrides(case: &Value) -> IniFile {
    let mut text = String::new();
    if let Some(lists) = case["voice_lists"].as_object() {
        for (name, list) in lists {
            text.push_str(&format!(
                "[{name}]\nVoiceSpecialAttack={}\n",
                list.as_str().unwrap()
            ));
        }
    }
    IniFile::from_str(&text)
}

pub(crate) fn fixture_rules(corpus: &Value, case: &Value) -> RuleSet {
    let mut layers = physical_layers(corpus);
    layers.push(RulesLayerKind::Scenario, voice_overrides(case));
    project(&layers, &catalog(corpus))
}

fn project(layers: &RulesLayerStack, sounds: &SoundRegistry) -> RuleSet {
    let processed = layers.process().unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    rules.bind_type_sound_references(processed.ini(), sounds);
    rules
}

fn initial_layers() -> RulesLayerStack {
    // The oracle constructs both registered receivers before these selected
    // reader blocks. This declaration supplies that explicit fixture input.
    RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n[InfantryTypes]\n0=E1\n",
    ))
}

fn append_physical_layer(layers: &mut RulesLayerStack, source: &Value) {
    if source["absent"].as_bool().unwrap_or(false) {
        assert_eq!(source["file"], "LANGRULE.INI");
        return;
    }
    let kind = match source["file"].as_str().unwrap() {
        "RULESMD.INI" => RulesLayerKind::RulesMd,
        "LANGRULE.INI" => RulesLayerKind::LangRule,
        "MPBattleMD.ini" => RulesLayerKind::GameMode,
        "Hills.map" => RulesLayerKind::Scenario,
        other => panic!("unexpected Area Guard source {other}"),
    };
    layers.push(kind, sections_ini(&source["sections"]));
}

fn physical_layers(corpus: &Value) -> RulesLayerStack {
    let mut layers = initial_layers();
    for source in corpus["retail"]["layers"].as_array().unwrap() {
        append_physical_layer(&mut layers, source);
    }
    layers
}

fn assert_voices(rules: &RuleSet, type_name: &str, expected: &Value, context: &str) {
    let actual = &rules.object(type_name).unwrap().voice_special_attack;
    assert_eq!(json!(actual.len()), expected["count"], "{context}");
    assert_eq!(json!(actual), expected["names"], "{context}");
}

#[test]
fn guard_sound_and_default_voice_begin_with_executed_constructor_defaults() {
    let corpus = native();
    let rules = project(&initial_layers(), &catalog(&corpus));
    let original = &corpus["retail"]["constructor"];
    assert_eq!(original["guard"], -1);
    assert_eq!(rules.general.guard_sound, None);
    for name in ["MTNK", "E1"] {
        assert_voices(&rules, name, &original["voices"][name], "constructor");
    }
}

#[test]
fn guard_and_voice_readers_retain_native_values_through_every_physical_layer() {
    let corpus = native();
    let sounds = catalog(&corpus);
    let mut layers = initial_layers();
    for source in corpus["retail"]["layers"].as_array().unwrap() {
        append_physical_layer(&mut layers, source);
        if source["absent"].as_bool().unwrap_or(false) {
            continue;
        }
        let rules = project(&layers, &sounds);
        let context = source["file"].as_str().unwrap();
        assert_eq!(
            json!(rules.general.guard_sound),
            source["guard"]["name"],
            "{context}"
        );
        for name in ["MTNK", "E1"] {
            assert_voices(&rules, name, &source["voices"][name], context);
        }
    }
}

fn reader_control_ini(section: &str, default_key: &str, row: &Value) -> IniFile {
    let key = row["key"].as_str().unwrap_or(default_key);
    let text = row["value"].as_str().map_or_else(
        || format!("[{section}]\n"),
        |value| format!("[{section}]\n{key}={value}\n"),
    );
    IniFile::from_str(&text)
}

#[test]
fn default_voice_reader_matches_native_order_duplicates_retention_and_capacity() {
    let corpus = native();
    let sounds = catalog(&corpus);
    let mut layers = physical_layers(&corpus);
    let controls = corpus["retail"]["voice_controls"].as_array().unwrap();
    assert_eq!(controls.len(), 11, "all original list-reader controls");
    for row in controls {
        let context = row["name"].as_str().unwrap();
        assert_voices(&project(&layers, &sounds), "E1", &row["before"], context);
        layers.push(
            RulesLayerKind::Scenario,
            reader_control_ini("E1", "VoiceSpecialAttack", row),
        );
        assert_voices(&project(&layers, &sounds), "E1", &row["after"], context);
    }
}

#[test]
fn guard_sound_reader_matches_native_registered_name_and_failed_lookup_retention() {
    let corpus = native();
    let sounds = catalog(&corpus);
    let mut layers = physical_layers(&corpus);
    let controls = corpus["retail"]["guard_controls"].as_array().unwrap();
    assert_eq!(controls.len(), 8, "all original single-reference controls");
    for row in controls {
        let context = row["name"].as_str().unwrap();
        // The native single-reference reader reports the prior numeric ID;
        // compare its fixed-registry name, independently of Rust ordinals.
        let prior_id = row["before"].as_i64().unwrap();
        let prior_name = corpus["retail"]["sound_registry"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|sound| sound["fixture_index"].as_i64() == Some(prior_id))
            .map(|sound| sound["name"].clone())
            .unwrap_or(Value::Null);
        assert_eq!(
            json!(project(&layers, &sounds).general.guard_sound),
            prior_name,
            "{context}"
        );
        layers.push(
            RulesLayerKind::Scenario,
            reader_control_ini("AudioVisual", "GuardSound", row),
        );
        assert_eq!(
            json!(project(&layers, &sounds).general.guard_sound),
            row["after"]["name"],
            "{context}"
        );
    }
}

#[test]
fn area_guard_sound_inputs_match_full_retail_battle_production_reader() {
    let Some(retail) = super::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let corpus = native();
    let last = corpus["retail"]["layers"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_eq!(
        json!(retail.rules.general.guard_sound),
        last["guard"]["name"]
    );
    for name in ["MTNK", "E1"] {
        assert_voices(
            &retail.rules,
            name,
            &last["voices"][name],
            "Hills/Battle production reader",
        );
    }
    let Some((_, assets)) = super::retail_ini_fixture::retail_assets() else {
        return;
    };
    for (name, hash) in [
        ("RULESMD.INI", &corpus["retail"]["layers"][0]["sha256"]),
        ("SOUNDMD.INI", &corpus["retail"]["sound_registry"]["sha256"]),
    ] {
        let bytes = assets.load_file_from_mix(name).unwrap().bytes;
        assert_eq!(
            crate::util::sha256::sha256_hex(&bytes),
            hash.as_str().unwrap(),
            "{name}"
        );
    }
    let audio = super::audio_sources::AudioDefinitions::select(&assets);
    for sound in corpus["retail"]["sound_registry"]["rows"]
        .as_array()
        .unwrap()
    {
        let definition = audio.sounds().get(sound["name"].as_str().unwrap()).unwrap();
        assert_eq!(json!(definition.sounds), sound["samples"]);
    }
}

fn authored_rules(root: &IniFile, sounds: SoundRegistry) -> RuleSet {
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root.clone(),
        None,
        IniFile::empty(),
        Arc::new(sounds),
    )
    .unwrap();
    owner
        .load_noncampaign_scenario(None, &IniFile::empty())
        .unwrap()
        .into_parts()
        .0
}

#[test]
fn resolved_default_voice_list_changes_identity_independently_of_catalog_ordinals() {
    let corpus = native();
    let controls = corpus["retail"]["voice_controls"].as_array().unwrap();
    let list = controls
        .iter()
        .find(|row| row["name"] == "valid_list")
        .unwrap();
    let root = IniFile::from_str(&format!(
        "[InfantryTypes]\n0=E1\n[E1]\nVoiceSpecialAttack={}\n",
        list["value"].as_str().unwrap()
    ));
    let resolved = authored_rules(&root, catalog(&corpus));
    let unresolved = authored_rules(&root, SoundRegistry::from_ini(&IniFile::empty()));
    assert_voices(&resolved, "E1", &list["after"], "resolved list");
    assert!(
        unresolved
            .object("E1")
            .unwrap()
            .voice_special_attack
            .is_empty()
    );
    assert_ne!(
        resolved.simulation_config_hash(),
        unresolved.simulation_config_hash(),
        "fixed-catalog admission changes Main draws with identical Rules text"
    );

    let mut rows: Vec<_> = corpus["retail"]["sound_registry"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row["fixture_index"].as_u64().unwrap()));
    let catalog_text =
        rows.iter()
            .enumerate()
            .fold(String::from("[SoundList]\n"), |mut text, (index, row)| {
                text.push_str(&format!("{index}={}\n", row["name"].as_str().unwrap()));
                text
            });
    let reordered = authored_rules(
        &root,
        SoundRegistry::from_ini(&IniFile::from_str(&catalog_text)),
    );
    assert_eq!(
        resolved.object("E1").unwrap().voice_special_attack,
        reordered.object("E1").unwrap().voice_special_attack
    );
    assert_eq!(
        resolved.simulation_config_hash(),
        reordered.simulation_config_hash(),
        "catalog ordinals are not sound-list semantics"
    );

    let duplicate = controls
        .iter()
        .find(|row| row["name"] == "duplicates_case")
        .unwrap();
    let different_root = IniFile::from_str(&format!(
        "[InfantryTypes]\n0=E1\n[E1]\nVoiceSpecialAttack={}\n",
        duplicate["value"].as_str().unwrap()
    ));
    let different = authored_rules(&different_root, catalog(&corpus));
    assert_voices(
        &different,
        "E1",
        &duplicate["after"],
        "ordered duplicate list",
    );
    assert_ne!(
        resolved.simulation_config_hash(),
        different.simulation_config_hash()
    );
}
