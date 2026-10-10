//! Original TechnoType MoveSound525430/713459 and cadence712222 readers.
//! The shared fixture projects the executed physical sections through the
//! production Rules pass owner; it never computes a native expected list.

use super::ini_parser::IniFile;
use super::native_processing::{RulesLayerKind, RulesLayerStack};
use super::process_owner::NativeRulesProcessOwner;
use super::ruleset::RuleSet;
use super::sound_ini::SoundRegistry;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) fn native() -> Value {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fv_cell_attack/foot_move_sound.json",
    ))
    .unwrap();
    assert_eq!(corpus["schema_version"], 1);
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

fn catalog(corpus: &Value) -> SoundRegistry {
    SoundRegistry::from_ini(&sections_ini(
        &corpus["retail"]["sound_registry"]["selected"],
    ))
}

fn layer_kind(name: &str) -> RulesLayerKind {
    match name {
        "LANGRULE.INI" => RulesLayerKind::LangRule,
        "MPBattleMD.ini" => RulesLayerKind::GameMode,
        "Hills.map" => RulesLayerKind::Scenario,
        other => panic!("unexpected native MoveSound layer {other}"),
    }
}

fn layers(corpus: &Value) -> RulesLayerStack {
    let sources = corpus["retail"]["type_layers"].as_array().unwrap();
    assert_eq!(sources[0]["file"], "RULESMD.INI");
    // The native fixture constructs this registered type before its selected
    // reader blocks. The registry declaration is that explicit fixture input.
    let mut root = IniFile::from_str("[VehicleTypes]\n0=SQD\n");
    root.merge(&sections_ini(&sources[0]["sections"]));
    let mut layers = RulesLayerStack::new(root);
    for source in &sources[1..] {
        if source["absent"].as_bool().unwrap_or(false) {
            continue;
        }
        layers.push(
            layer_kind(source["file"].as_str().unwrap()),
            sections_ini(&source["sections"]),
        );
    }
    layers
}

fn project(layers: &RulesLayerStack, sounds: &SoundRegistry) -> RuleSet {
    // The oracle executes selected Rules readers and supplies no ART read in
    // this slice. The separate retail integration test uses the full source owner.
    let processed = layers.process().unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    rules.bind_type_sound_references(processed.ini(), sounds);
    rules
}

pub(crate) fn fixture_rules(corpus: &Value, input: &Value) -> RuleSet {
    let mut layers = layers(corpus);
    let mut overrides = String::from("[SQD]\n");
    for (field, key) in [
        ("move_sound", "MoveSound"),
        ("walk_rate", "WalkRate"),
        ("idle_rate", "IdleRate"),
    ] {
        if let Some(value) = input.get(field) {
            let value = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            overrides.push_str(&format!("{key}={value}\n"));
        }
    }
    layers.push(RulesLayerKind::Scenario, IniFile::from_str(&overrides));
    project(&layers, &catalog(corpus))
}

pub(crate) fn assert_binding(rules: &RuleSet, expected: &Value, context: &str) {
    let object = rules.object("SQD").unwrap();
    assert_eq!(
        json!(object.move_sound),
        expected["names"],
        "{context}: list"
    );
    assert_eq!(
        json!(object.walk_rate),
        expected["walk_rate"],
        "{context}: WalkRate"
    );
    assert_eq!(
        json!(object.idle_rate),
        expected["idle_rate"],
        "{context}: IdleRate"
    );
}

#[test]
fn move_sound_and_body_rates_begin_with_original_constructor_defaults() {
    let corpus = native();
    let rules = RuleSet::from_ini(&IniFile::from_str("[VehicleTypes]\n0=SQD\n")).unwrap();
    let object = rules.object("SQD").unwrap();
    let original = &corpus["retail"]["constructor"];
    assert_eq!(
        json!(object.move_sound.len()),
        original["sound_vector_count"]
    );
    assert_eq!(json!(object.walk_rate), original["walk_rate"]);
    assert_eq!(json!(object.idle_rate), original["idle_rate"]);
}

#[test]
fn move_sound_selected_physical_layers_and_reader_history_match_original() {
    let corpus = native();
    let sounds = catalog(&corpus);
    let sources = corpus["retail"]["type_layers"].as_array().unwrap();
    let mut stack = RulesLayerStack::new(IniFile::from_str("[VehicleTypes]\n0=SQD\n"));
    for source in sources {
        if source["absent"].as_bool().unwrap_or(false) {
            assert_eq!(source["file"], "LANGRULE.INI");
            continue;
        }
        stack.push(
            if source["file"] == "RULESMD.INI" {
                RulesLayerKind::RulesMd
            } else {
                layer_kind(source["file"].as_str().unwrap())
            },
            sections_ini(&source["sections"]),
        );
        assert_binding(
            &project(&stack, &sounds),
            &source["after"],
            source["file"].as_str().unwrap(),
        );
    }
    let histories = corpus["reader_histories"].as_array().unwrap();
    for (index, row) in histories.iter().enumerate() {
        assert_binding(
            &project(&stack, &sounds),
            &row["before"],
            &format!("reader {index} before"),
        );
        let text = row["raw"].as_str().map_or_else(
            || "[SQD]\n".to_owned(),
            |raw| format!("[SQD]\nMoveSound={raw}\n"),
        );
        stack.push(RulesLayerKind::Scenario, IniFile::from_str(&text));
        assert_binding(
            &project(&stack, &sounds),
            &row["after"],
            &format!("reader {index} after"),
        );
    }
    assert_eq!(
        histories.len(),
        12,
        "every executed reader control is consumed"
    );
}

#[test]
fn move_sound_retained_cadence_and_names_match_the_full_retail_battle_reader() {
    let Some(retail) = super::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let corpus = native();
    let last = corpus["retail"]["type_layers"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_binding(
        &retail.rules,
        &last["after"],
        "physical Hills/Battle production owner",
    );
    let Some((_, assets)) = super::retail_ini_fixture::retail_assets() else {
        return;
    };
    for (name, hash) in [
        ("RULESMD.INI", &corpus["retail"]["type_layers"][0]["sha256"]),
        ("SOUNDMD.INI", &corpus["retail"]["sound_registry"]["sha256"]),
    ] {
        let loaded = assets.load_file_from_mix(name).unwrap();
        assert_eq!(
            crate::util::sha256::sha256_hex(&loaded.bytes),
            hash.as_str().unwrap(),
            "{name}"
        );
    }
    let audio = super::audio_sources::AudioDefinitions::select(&assets);
    for original in corpus["retail"]["sound_fields"].as_array().unwrap() {
        let sound = audio
            .sounds()
            .get(original["name"].as_str().unwrap())
            .unwrap();
        assert_eq!(json!(sound.sounds), original["samples"]);
        assert_eq!(json!(sound.control), original["control"]);
        assert_eq!(json!(sound.loop_count), original["loop_count"]);
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
fn resolved_sound_lists_participate_in_simulation_identity_without_catalog_ordinals() {
    let corpus = native();
    let raw = corpus["reader_histories"][0]["raw"].as_str().unwrap();
    let root = IniFile::from_str(&format!("[VehicleTypes]\n0=SQD\n[SQD]\nMoveSound={raw}\n"));
    let resolved = authored_rules(&root, catalog(&corpus));
    let unresolved = authored_rules(&root, SoundRegistry::from_ini(&IniFile::empty()));
    assert_eq!(
        json!(resolved.object("SQD").unwrap().move_sound),
        corpus["reader_histories"][0]["after"]["names"]
    );
    assert!(unresolved.object("SQD").unwrap().move_sound.is_empty());
    assert_ne!(
        resolved.simulation_config_hash(),
        unresolved.simulation_config_hash(),
        "fixed-catalog admission changes Main draw behavior with identical Rules text"
    );

    let names = corpus["reader_histories"][0]["after"]["names"]
        .as_array()
        .unwrap();
    let reversed_catalog = IniFile::from_str(&format!(
        "[SoundList]\n0={}\n1={}\n",
        names[1].as_str().unwrap(),
        names[0].as_str().unwrap()
    ));
    let reordered = authored_rules(&root, SoundRegistry::from_ini(&reversed_catalog));
    assert_eq!(
        resolved.object("SQD").unwrap().move_sound,
        reordered.object("SQD").unwrap().move_sound
    );
    assert_eq!(
        resolved.simulation_config_hash(),
        reordered.simulation_config_hash(),
        "unrelated fixed-catalog ordinals do not change resolved type-list semantics"
    );

    let duplicate_raw = corpus["reader_histories"][5]["raw"].as_str().unwrap();
    let duplicate_root = IniFile::from_str(&format!(
        "[VehicleTypes]\n0=SQD\n[SQD]\nMoveSound={duplicate_raw}\n"
    ));
    let duplicated = authored_rules(&duplicate_root, catalog(&corpus));
    assert_eq!(
        json!(duplicated.object("SQD").unwrap().move_sound),
        corpus["reader_histories"][5]["after"]["names"]
    );
    assert_ne!(
        resolved.simulation_config_hash(),
        duplicated.simulation_config_hash(),
        "ordered duplicate slots change the selected sound for a given raw draw"
    );
}

#[test]
fn registered_none_and_reserved_bracketed_none_follow_original_list_lookup() {
    let corpus = native();
    let original = &corpus["registered_null_names"];
    let mut sound_ini = sections_ini(&corpus["retail"]["sound_registry"]["selected"]);
    sound_ini.merge(&sections_ini(&original["authored_sections"]));
    let sounds = SoundRegistry::from_ini(&sound_ini);
    for name in original["catalog"].as_array().unwrap() {
        assert!(sounds.get(name.as_str().unwrap()).is_some());
    }
    let mut stack = layers(&corpus);
    for (index, row) in original["histories"].as_array().unwrap().iter().enumerate() {
        assert_binding(
            &project(&stack, &sounds),
            &row["before"],
            &format!("literal-name {index} before"),
        );
        stack.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(&format!(
                "[SQD]\nMoveSound={}\n",
                row["raw"].as_str().unwrap(),
            )),
        );
        assert_binding(
            &project(&stack, &sounds),
            &row["after"],
            &format!("literal-name {index} after"),
        );
    }
    assert_eq!(original["histories"].as_array().unwrap().len(), 4);
}
