//! VoiceSelect's production fixed-catalog binding and reached Rules passes.
//! Reader525430 / TechnoReadINI712B1D..712B87; constructor710D74..710D9A.

use std::sync::Arc;

use super::NativeRulesProcessOwner;
use crate::rules::ini_parser::IniFile;
use crate::rules::sound_ini::SoundRegistry;

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
        .load_noncampaign_scenario(Some(&mode), &map)
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
        .load_noncampaign_scenario(None, &IniFile::from_str(""))
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
