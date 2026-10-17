//! Selection/Attack voice fixed-catalog binding and reached passes.
//! Reader525430 / TechnoReadINI712B1D..712BF1; empty constructors710D74..710DBD.
//! Original1cdd1180 controls: input_oracle/selection_navigation.{py,json,meta.json}.
//! Attack list/four slots/Rules+230: spatial_oracle/attack_order_voice.

use std::sync::Arc;

use super::NativeRulesProcessOwner;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::sound_ini::SoundRegistry;
use crate::test_fixture::ini_sections;
use serde_json::{Value, json};

fn attack_voice_native() -> &'static Value {
    let native = crate::test_fixture::json("tools/spatial_oracle/attack_order_voice.json");
    assert_eq!(
        native["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    native
}

fn order_voice_catalog(corpus: &Value) -> Arc<SoundRegistry> {
    Arc::new(SoundRegistry::from_ini(&ini_sections(
        &corpus["readers"]["sound"]["selected"],
    )))
}

/// Native fixture-local Voc IDs map to the independently read fixed names.
fn order_voice_name(corpus: &'static Value, index: i64) -> Option<&'static str> {
    if index == -1 {
        return None;
    }
    Some(
        corpus["readers"]["sound"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["fixture_index"] == index)
            .expect("native fixed-catalog sound identity")["name"]
            .as_str()
            .unwrap(),
    )
}

fn assert_attack_voice_fields(rules: &RuleSet, expected: &Value, label: &str) {
    let object = rules.object("MTNK").unwrap();
    let names: Vec<String> = expected["voice_attack"]
        .as_array()
        .unwrap()
        .iter()
        .map(|index| {
            order_voice_name(attack_voice_native(), index.as_i64().unwrap())
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        object.voice_attack(),
        names.as_slice(),
        "{label}: VoiceAttack"
    );
    for (slot, (weapon, elite)) in [(0, false), (0, true), (1, false), (1, true)]
        .into_iter()
        .enumerate()
    {
        let native = order_voice_name(
            attack_voice_native(),
            expected["weapon_voice_slots"][slot].as_i64().unwrap(),
        );
        assert_eq!(
            object.voice_attack_override(weapon, elite),
            native,
            "{label}: scalar slot{slot}"
        );
        if weapon != 0 {
            // VoiceAttack's TEST index selects secondary for every nonzero
            // value; this query must not impose GetWeapon's index bounds.
            for other in [-1, 2, i32::MAX] {
                assert_eq!(
                    object.voice_attack_override(other, elite),
                    native,
                    "{label}"
                );
            }
        }
    }
}

fn bound_voice_rules(
    layers: &crate::rules::native_processing::RulesLayerStack,
    sounds: &SoundRegistry,
) -> RuleSet {
    let processed = layers.process().unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    rules.bind_type_sound_references(processed.ini(), sounds);
    rules
}

/// Original712C5B..712CC5/713679..713775 retained list/four scalar outputs.
#[test]
fn attack_voice_constructor_physical_layers_and_retained_reads_match_original() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};

    let native = &attack_voice_native()["readers"];
    let sounds = order_voice_catalog(attack_voice_native());
    let mut layers = RulesLayerStack::new(IniFile::from_str("[VehicleTypes]\n0=MTNK\n"));
    let constructor = bound_voice_rules(&layers, &sounds);
    assert_attack_voice_fields(&constructor, &native["constructor"], "constructor");
    assert_eq!(
        constructor.general.voice_ifv_repair(),
        order_voice_name(
            attack_voice_native(),
            native["voice_ifv_repair"]["constructor"].as_i64().unwrap()
        )
    );
    for row in native["layers"].as_array().unwrap() {
        if row["absent"] == true {
            continue;
        }
        let label = row["file"].as_str().unwrap();
        let kind = match label {
            "RULESMD.INI" => RulesLayerKind::RulesMd,
            "LANGRULE.INI" => RulesLayerKind::LangRule,
            "MPBattleMD.ini" => RulesLayerKind::GameMode,
            "XMP03T4.MAP" => RulesLayerKind::Scenario,
            _ => panic!("unexpected native Rules source {label}"),
        };
        layers.push(kind, ini_sections(&row["selected"]));
        let rules = bound_voice_rules(&layers, &sounds);
        assert_attack_voice_fields(&rules, &row["after"], label);
        assert_eq!(
            rules.general.voice_ifv_repair(),
            order_voice_name(
                attack_voice_native(),
                row["voice_ifv_repair"].as_i64().unwrap()
            ),
            "{label}"
        );
    }
    let controls = native["retained_controls"].as_array().unwrap();
    assert_eq!(controls.len(), 9);
    for row in controls {
        let label = row["name"].as_str().unwrap();
        assert_attack_voice_fields(&bound_voice_rules(&layers, &sounds), &row["before"], label);
        layers.push(
            RulesLayerKind::Scenario,
            ini_sections(&row["input"]["sections"]),
        );
        assert_attack_voice_fields(&bound_voice_rules(&layers, &sounds), &row["after"], label);
    }
}

/// Rules+230 ctor665976=-1; original AudioVisual669D63..669DA7 controls.
#[test]
fn attack_voice_ifv_repair_preserves_original_current_id_read_history() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};

    let readers = &attack_voice_native()["readers"];
    let sounds = order_voice_catalog(attack_voice_native());
    let mut layers = RulesLayerStack::new(ini_sections(&readers["layers"][0]["selected"]));
    for row in readers["voice_ifv_repair"]["controls"].as_array().unwrap() {
        let label = row["name"].as_str().unwrap();
        assert_eq!(
            bound_voice_rules(&layers, &sounds)
                .general
                .voice_ifv_repair(),
            order_voice_name(attack_voice_native(), row["before"].as_i64().unwrap()),
            "{label}: before"
        );
        layers.push(RulesLayerKind::Scenario, ini_sections(&row["sections"]));
        assert_eq!(
            bound_voice_rules(&layers, &sounds)
                .general
                .voice_ifv_repair(),
            order_voice_name(attack_voice_native(), row["after"].as_i64().unwrap()),
            "{label}: after"
        );
    }
}

#[test]
fn attack_voice_orphan_body_does_not_precede_type_allocation() {
    let native = attack_voice_native();
    let sounds = order_voice_catalog(attack_voice_native());
    let root = ini_sections(&native["readers"]["retained_controls"][0]["input"]["sections"]);
    let mut owner =
        NativeRulesProcessOwner::from_cold_start_sources(root, None, IniFile::empty(), sounds)
            .unwrap();
    let mode = IniFile::from_str("[VehicleTypes]\n0=MTNK\n");
    let rules = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(Some(&mode)),
            &IniFile::empty(),
        )
        .unwrap()
        .into_parts()
        .0;
    assert_attack_voice_fields(&rules, &native["readers"]["constructor"], "late allocation");
}

#[test]
fn move_voice_reader_matches_original_constructor_and_retained_passes() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};

    let native = crate::test_fixture::json("tools/spatial_oracle/move_order_voice.json");
    assert_eq!(native["schema"], 1);
    assert_eq!(
        native["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    let sounds = order_voice_catalog(native);
    let assert_fields = |rules: &RuleSet, expected: &Value, label: &str| {
        let object = rules.object("MTNK").unwrap();
        let names: Vec<_> = expected["voice_move"]
            .as_array()
            .unwrap()
            .iter()
            .map(|index| {
                order_voice_name(native, index.as_i64().unwrap())
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(object.voice_move(), names, "{label}: full Move vector");
        for (key, actual) in [
            ("voice_enter", object.voice_enter()),
            ("voice_capture", object.voice_capture()),
            ("voice_harvest", object.voice_harvest()),
        ] {
            assert_eq!(
                actual,
                order_voice_name(native, expected[key].as_i64().unwrap()),
                "{label}: {key}"
            );
        }
    };
    let mut layers = RulesLayerStack::new(IniFile::from_str("[VehicleTypes]\n0=MTNK\n"));
    assert_fields(
        &bound_voice_rules(&layers, &sounds),
        &native["readers"]["constructor"],
        "original constructor",
    );
    for row in native["readers"]["layers"].as_array().unwrap() {
        if row["absent"] == true {
            continue;
        }
        layers.push(RulesLayerKind::Scenario, ini_sections(&row["selected"]));
        assert_fields(
            &bound_voice_rules(&layers, &sounds),
            &row["after"],
            row["file"].as_str().unwrap(),
        );
    }
    let controls = native["readers"]["retained_controls"].as_array().unwrap();
    assert_eq!(controls.len(), 17, "all original reader histories");
    for row in controls {
        let label = row["name"].as_str().unwrap();
        assert_fields(&bound_voice_rules(&layers, &sounds), &row["before"], label);
        layers.push(
            RulesLayerKind::Scenario,
            ini_sections(&row["input"]["sections"]),
        );
        assert_fields(&bound_voice_rules(&layers, &sounds), &row["after"], label);
    }
}

#[test]
fn move_voice_orphan_body_does_not_precede_type_allocation() {
    let native = crate::test_fixture::json("tools/spatial_oracle/move_order_voice.json");
    let valid = &native["readers"]["retained_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "mixed_case_duplicates_and_scalars")
        .unwrap()["input"]["sections"];
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        ini_sections(valid),
        None,
        IniFile::empty(),
        order_voice_catalog(native),
    )
    .unwrap();
    let mode = IniFile::from_str("[VehicleTypes]\n0=MTNK\n");
    let rules = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(Some(&mode)),
            &IniFile::empty(),
        )
        .unwrap()
        .into_parts()
        .0;
    let object = rules.object("MTNK").unwrap();
    assert!(object.voice_move().is_empty());
    assert_eq!(object.voice_enter(), None);
    assert_eq!(object.voice_capture(), None);
    assert_eq!(object.voice_harvest(), None);
}

#[test]
fn order_voice_resolved_catalog_changes_effective_config_for_list_and_each_scalar() {
    use crate::rules::native_processing::RulesLayerStack;

    let sounds = order_voice_catalog(attack_voice_native());
    let empty = SoundRegistry::from_ini(&IniFile::empty());
    for key in [
        "VoiceMove",
        "VoiceEnter",
        "VoiceCapture",
        "VoiceHarvest",
        "VoiceAttack",
        "VoicePrimaryWeaponAttack",
        "VoicePrimaryEliteWeaponAttack",
        "VoiceSecondaryWeaponAttack",
        "VoiceSecondaryEliteWeaponAttack",
        "VoiceIFVRepair",
    ] {
        let section = if key == "VoiceIFVRepair" {
            "AudioVisual"
        } else {
            "MTNK"
        };
        let layers = RulesLayerStack::new(IniFile::from_str(&format!(
            "[VehicleTypes]\n0=MTNK\n[{section}]\n{key}=GenAllVehicleAttackCommand\n"
        )));
        let resolved = bound_voice_rules(&layers, &sounds);
        let unresolved = bound_voice_rules(&layers, &empty);
        assert_eq!(
            resolved.source_ini_hash(),
            unresolved.source_ini_hash(),
            "{key}"
        );
        assert_ne!(
            resolved.simulation_config_hash(),
            unresolved.simulation_config_hash(),
            "{key}: effective sound binding, not only source text"
        );
    }
}

#[test]
fn attack_voice_stock_battle_uses_original_fixed_sound_bindings() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let native = &attack_voice_native()["readers"];
    assert_attack_voice_fields(&retail.rules, &native["result"], "physical Battle");
    assert_eq!(
        retail.rules.general.voice_ifv_repair(),
        order_voice_name(
            attack_voice_native(),
            native["voice_ifv_repair"]["result"].as_i64().unwrap()
        )
    );
}

fn catalog() -> Arc<SoundRegistry> {
    Arc::new(SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=First\n1=Second\n2=Third\n[NotRegistered]\nSounds=sample\n",
    )))
}

#[test]
fn voice_select_keeps_the_full_ordered_resolved_list_across_reached_passes() {
    let ini = IniFile::from_str;
    let root = ini(
        "[InfantryTypes]\n0=KEEP\n1=REPLACE\n2=CLEAR\n3=DELIMITERS\n4=DEFAULT\n\
         [KEEP]\nVoiceSelect=First,Unknown,Second,First\n\
         [REPLACE]\nVoiceSelect=First\n\
         [CLEAR]\nVoiceSelect=First\n\
         [DELIMITERS]\nVoiceSelect=First\n\
         [DEFAULT]\nVoiceSelectWrong=First\n\
         [LATE]\nVoiceSelect=First\n",
    );
    let lang = ini("[KEEP]\nVoiceSelect=\nvoiceselect=Third\n\
         [REPLACE]\nVoiceSelect=Second\n\
         [LATE]\nVoiceSelect=Second\n");
    let mode = ini("[InfantryTypes]\n5=LATE\n\
         [KEEP]\nVoiceSelect= \n\
         [REPLACE]\nVoiceSelect=Third,sEcOnD,Third\n\
         [CLEAR]\nVoiceSelect=Unknown,NotRegistered\n\
         [DELIMITERS]\nVoiceSelect=,,,\n");
    let map = ini("[KEEP]\nvoiceselect=Third\n\
         [REPLACE]\nVoiceSelect=\n\
         [CLEAR]\nVoiceSelect=\n\
         [DELIMITERS]\nVoiceSelect=\n");
    let sounds = catalog();
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        Some(lang),
        ini(""),
        Arc::clone(&sounds),
    )
    .unwrap();
    let (rules, _, _, _) = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(Some(&mode)),
            &map,
        )
        .unwrap()
        .into_parts();
    assert!(Arc::ptr_eq(owner.fixed_sounds(), &sounds));
    assert_eq!(
        rules.object("KEEP").unwrap().voice_select,
        ["First", "Second", "First"]
    );
    assert_eq!(
        rules.object("REPLACE").unwrap().voice_select,
        ["Third", "Second", "Third"]
    );
    for name in ["CLEAR", "DELIMITERS", "DEFAULT", "LATE"] {
        assert!(
            rules.object(name).unwrap().voice_select.is_empty(),
            "{name}"
        );
    }
}

#[test]
fn voice_select_readstring128_and_untrimmed_tokens_precede_sound_lookup() {
    let sounds = catalog();
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::from_str(&format!(
            "[VehicleTypes]\n0=SPACES\n1=BOUNDARY\n\
             [SPACES]\nVoiceSelect=First, Second,Second ,sEcOnD,,First\n\
             [BOUNDARY]\nVoiceSelect={},First,SecondX\n",
            "x".repeat(114),
        )),
        None,
        IniFile::from_str(""),
        sounds,
    )
    .unwrap();
    let (rules, _, _, _) = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
            &IniFile::from_str(""),
        )
        .unwrap()
        .into_parts();
    assert_eq!(
        rules.object("SPACES").unwrap().voice_select,
        ["First", "Second", "First"]
    );
    // ReadString's 127 payload bytes retain Second but cut its final X;
    // the long unresolved token is discarded only after tokenization.
    assert_eq!(
        rules.object("BOUNDARY").unwrap().voice_select,
        ["First", "Second"]
    );
}

/// Feedback uses the same native ReadSoundList owner as Select, with a
/// separate empty constructor vector and exact-case key. These expected
/// lists are original712D99..712E03 executions, not a second parser.
#[test]
fn voice_feedback_reader_matches_original_constructor_and_reached_passes() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;
    use serde_json::{Value, json};

    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/unit_voice_playback.json",
    ))
    .unwrap();
    let mut catalog_ini = String::from("[SoundList]\n");
    for (index, name) in corpus["retail"]["sound_sections"]["SoundList"]
        .as_object()
        .unwrap()
    {
        catalog_ini.push_str(&format!("{index}={}\n", name.as_str().unwrap()));
    }
    let sounds = SoundRegistry::from_ini(&IniFile::from_str(&catalog_ini));
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=FEEDBACK_READER_CONTROL\n",
    ));
    let project = |layers: &RulesLayerStack| {
        let processed = layers.process().unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.bind_type_sound_references(processed.ini(), &sounds);
        rules
    };
    assert_eq!(
        json!(
            project(&layers)
                .object("FEEDBACK_READER_CONTROL")
                .unwrap()
                .voice_feedback
        ),
        corpus["retail"]["feedback_constructor"]["names"],
    );
    let rows = corpus["retail"]["feedback_reader_controls"]
        .as_array()
        .unwrap();
    assert_eq!(rows.len(), 8, "every executed read in the retained history");
    for row in rows {
        let context = row["name"].as_str().unwrap();
        assert_eq!(
            json!(
                project(&layers)
                    .object("FEEDBACK_READER_CONTROL")
                    .unwrap()
                    .voice_feedback
            ),
            row["before"]["names"],
            "{context}: prior vector",
        );
        let mut pass = String::from("[FEEDBACK_READER_CONTROL]\n");
        if let (Some(key), Some(value)) = (row["key"].as_str(), row["raw"].as_str()) {
            pass.push_str(&format!("{key}={value}\n"));
        }
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(&pass));
        assert_eq!(
            json!(
                project(&layers)
                    .object("FEEDBACK_READER_CONTROL")
                    .unwrap()
                    .voice_feedback
            ),
            row["after"]["names"],
            "{context}: resolved ordered vector",
        );
    }
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn voice_feedback_matches_the_physical_battle_reader_and_sound_catalog_identity() {
    use serde_json::{Value, json};
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/unit_voice_playback.json",
    ))
    .unwrap();
    let last = corpus["retail"]["layers"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_eq!(
        json!(retail.rules.object("E1").unwrap().voice_feedback),
        last["voice_feedback"]["names"],
    );
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    for name in ["RULESMD.INI", "SOUNDMD.INI"] {
        let original = corpus["physical"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["name"] == name)
            .unwrap();
        let bytes = assets.load_file_from_mix(name).unwrap().bytes;
        assert_eq!(
            crate::util::sha256::sha256_hex(&bytes),
            original["sha256"].as_str().unwrap(),
            "{name}"
        );
    }
}

fn selection_voice_native() -> Value {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/selection_navigation.json",
    ))
    .unwrap();
    let meta: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/selection_navigation.meta.json",
    ))
    .unwrap();
    assert_eq!(corpus["schema"], 1);
    assert_eq!(
        meta["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    let readers = &corpus["enslaved_voice_type_histories"];
    assert_eq!(readers["reader_entry"], "0x712b1d");
    assert_eq!(readers["enslaved_reader_entry"], "0x712b87");
    assert_eq!(readers["reader_stop"], "0x712bf1");
    readers.clone()
}

fn selection_voice_catalog(native: &Value) -> Arc<SoundRegistry> {
    Arc::new(SoundRegistry::from_ini(&ini_sections(
        &native["sound_registry"]["selected"],
    )))
}

fn assert_selection_voice_bindings(
    rules: &RuleSet,
    type_id: &str,
    normal: &Value,
    enslaved: &Value,
    label: &str,
) {
    let object = rules.object(type_id).unwrap();
    for (key, actual, expected) in [
        ("VoiceSelect", &object.voice_select, normal),
        (
            "VoiceSelectEnslaved",
            &object.voice_select_enslaved,
            enslaved,
        ),
    ] {
        assert_eq!(
            json!(actual.len()),
            expected["count"],
            "{label}: {key} count"
        );
        assert_eq!(json!(actual), expected["names"], "{label}: {key} slots");
        assert_eq!(
            expected["ids"].as_array().unwrap().len(),
            actual.len(),
            "{label}: each executed native sound index retains its named slot"
        );
    }
}

#[test]
fn enslaved_selection_voice_constructor_vectors_match_original_type_constructors() {
    let native = selection_voice_native();
    let root = IniFile::from_str(
        "[InfantryTypes]\n0=E1\n1=SLAV\n[VehicleTypes]\n0=MTNK\n1=VOICE_CONTROL\n",
    );
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        None,
        IniFile::empty(),
        selection_voice_catalog(&native),
    )
    .unwrap();
    let rules = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
            &IniFile::empty(),
        )
        .unwrap()
        .into_parts()
        .0;
    let constructors = native["constructors"].as_array().unwrap();
    assert_eq!(constructors.len(), 4);
    for row in constructors {
        let type_id = row["type_id"].as_str().unwrap();
        assert_selection_voice_bindings(
            &rules,
            type_id,
            &row["normal"],
            &row["enslaved"],
            &format!(
                "{type_id}: original{} supplied poisoned storage",
                row["entry"]
            ),
        );
    }
}

#[test]
fn enslaved_selection_voice_readers_match_original_retained_passes_and_fixed_lookup() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};

    let native = selection_voice_native();
    let root = IniFile::from_str("[VehicleTypes]\n0=VOICE_CONTROL\n");
    let sounds = selection_voice_catalog(&native);
    let owner = NativeRulesProcessOwner::from_cold_start_sources(
        root.clone(),
        None,
        IniFile::empty(),
        Arc::clone(&sounds),
    )
    .unwrap();
    let mut layers = RulesLayerStack::new(root);
    let project = |layers: &RulesLayerStack| {
        let processed = layers.process().unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        owner.bind_type_sound_references(&mut rules, &processed);
        rules
    };
    let rows = native["authored"].as_array().unwrap();
    assert_eq!(rows.len(), 15, "all executed retained reader controls");
    for row in rows {
        let label = row["name"].as_str().unwrap();
        assert_selection_voice_bindings(
            &project(&layers),
            "VOICE_CONTROL",
            &row["normal_before"],
            &row["before"],
            &format!("{label}: before"),
        );
        layers.push(RulesLayerKind::Scenario, ini_sections(&row["sections"]));
        assert_selection_voice_bindings(
            &project(&layers),
            "VOICE_CONTROL",
            &row["normal_after"],
            &row["after"],
            &format!("{label}: after"),
        );
    }
    assert!(Arc::ptr_eq(owner.fixed_sounds(), &sounds));
}

#[test]
fn enslaved_selection_voice_reaches_registered_types_and_retains_process_catalog() {
    let native = selection_voice_native();
    let rows = native["authored"].as_array().unwrap();
    let row = |name: &str| rows.iter().find(|row| row["name"] == name).unwrap();
    let initial = row("both_registered");
    let duplicates = row("duplicates_case");
    let constructor = native["constructors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["type_id"] == "VOICE_CONTROL")
        .unwrap();
    let raw = initial["sections"]["VOICE_CONTROL"]["VoiceSelectEnslaved"]
        .as_str()
        .unwrap();
    let mut root = IniFile::from_str(&format!(
        "[VehicleTypes]\n0=VOICE_CONTROL\n[LATE]\nVoiceSelectEnslaved={raw}\n"
    ));
    root.merge(&ini_sections(&initial["sections"]));
    let lang = ini_sections(&row("empty_retains")["sections"]);
    // Scenario Rules cannot replace the startup-selected SOUNDMD registry.
    let mode = IniFile::from_str("[VehicleTypes]\n1=LATE\n[SoundList]\n0=AbsentEnslavedVoice\n");
    let map = ini_sections(&row("wrong_key_case_retains")["sections"]);
    let sounds = selection_voice_catalog(&native);
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        Some(lang),
        IniFile::empty(),
        Arc::clone(&sounds),
    )
    .unwrap();
    let first = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(Some(&mode)),
            &map,
        )
        .unwrap()
        .into_parts()
        .0;
    assert_selection_voice_bindings(
        &first,
        "VOICE_CONTROL",
        &initial["normal_after"],
        &initial["after"],
        "root/LANG/mode/map reached reads",
    );
    assert_selection_voice_bindings(
        &first,
        "LATE",
        &constructor["normal"],
        &constructor["enslaved"],
        "unregistered earlier section is not inherited",
    );
    let late_map = IniFile::from_str(&format!(
        "[LATE]\nVoiceSelectEnslaved={}\n",
        duplicates["raw"].as_str().unwrap()
    ));
    let second = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(Some(&mode)),
            &late_map,
        )
        .unwrap()
        .into_parts()
        .0;
    assert_selection_voice_bindings(
        &second,
        "LATE",
        &constructor["normal"],
        &duplicates["after"],
        "registered type receives the reached map vector",
    );
    assert!(Arc::ptr_eq(owner.fixed_sounds(), &sounds));
}

#[test]
fn resolved_enslaved_selection_voice_changes_configuration_under_identical_rules() {
    let native = selection_voice_native();
    let initial = native["authored"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "both_registered")
        .unwrap();
    let root = IniFile::from_str(&format!(
        "[VehicleTypes]\n0=VOICE_CONTROL\n[VOICE_CONTROL]\nVoiceSelectEnslaved={}\n",
        initial["sections"]["VOICE_CONTROL"]["VoiceSelectEnslaved"]
            .as_str()
            .unwrap()
    ));
    let load = |sounds| {
        NativeRulesProcessOwner::from_cold_start_sources(
            root.clone(),
            None,
            IniFile::empty(),
            sounds,
        )
        .unwrap()
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
            &IniFile::empty(),
        )
        .unwrap()
        .into_parts()
        .0
    };
    let resolved = load(selection_voice_catalog(&native));
    let unresolved = load(Arc::new(SoundRegistry::from_ini(&IniFile::empty())));
    assert_eq!(
        json!(
            resolved
                .object("VOICE_CONTROL")
                .unwrap()
                .voice_select_enslaved
        ),
        initial["after"]["names"]
    );
    assert!(
        unresolved
            .object("VOICE_CONTROL")
            .unwrap()
            .voice_select_enslaved
            .is_empty()
    );
    for rules in [&resolved, &unresolved] {
        assert!(
            rules
                .object("VOICE_CONTROL")
                .unwrap()
                .voice_select
                .is_empty()
        );
    }
    assert_eq!(resolved.source_ini_hash(), unresolved.source_ini_hash());
    assert_ne!(
        resolved.simulation_config_hash(),
        unresolved.simulation_config_hash(),
        "the newly resolved list changes Main draw admission under identical Rules text"
    );
}

// The test-only retail fixture owns its upward map-loader calls.
#[cfg(test)]
#[test]
#[ignore = "slow; run when changing what it covers"]
fn enslaved_selection_voice_matches_physical_anytown_battle_rules_and_sound_inputs() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let native = selection_voice_native();
    let rows = native["retail"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    for row in rows {
        let type_id = row["type_id"].as_str().unwrap();
        assert_eq!(
            row["layers"].as_array().unwrap().last().unwrap()["file"],
            "XMP03T4.MAP"
        );
        assert_selection_voice_bindings(
            &retail.rules,
            type_id,
            &row["normal"],
            &row["enslaved"],
            "physical AnyTown/Battle production process owner",
        );
    }
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let layers = rows[0]["layers"].as_array().unwrap();
    for source in &layers[..3] {
        let name = source["file"].as_str().unwrap();
        if source["absent"].as_bool().unwrap_or(false) {
            assert_eq!(name, "LANGRULE.INI");
            assert!(
                assets.resolve_ref(name).is_none(),
                "the declared optional source is absent"
            );
        } else {
            let selected = crate::rules::retail_sources::select_ini(&assets, name).unwrap();
            assert_eq!(
                selected.source.source_sha256,
                source["sha256"].as_str().unwrap(),
                "{name}"
            );
        }
    }
    let (root, assets) = crate::rules::retail_ini_fixture::retail_scenario_assets().unwrap();
    let map =
        crate::map::source::load_map_by_name_or_path_with_assets(&root, "XMP03T4.MAP", &assets)
            .unwrap();
    let map_hash = match map.source {
        crate::map::source::LoadedMapSource::Loose { source_sha256, .. }
        | crate::map::source::LoadedMapSource::Mix { source_sha256, .. } => source_sha256,
        other => panic!("unexpected physical native map source {other:?}"),
    };
    assert_eq!(map_hash, layers[3]["sha256"].as_str().unwrap());
    let sound = crate::rules::retail_sources::select_ini(&assets, "SOUNDMD.INI").unwrap();
    assert_eq!(
        sound.source.source_sha256,
        native["sound_registry"]["sha256"].as_str().unwrap()
    );
    let audio = crate::rules::audio_sources::AudioDefinitions::select(&assets);
    for original in native["sound_registry"]["rows"].as_array().unwrap() {
        let sound = audio
            .sounds()
            .get(original["name"].as_str().unwrap())
            .unwrap();
        assert_eq!(json!(sound.sounds), original["samples"]);
    }
}
