//! Retained Side EVA field controls from Ares1002D9F0/1002E190/1002B450,
//! tag reader1002EC00 and scalar save/load1002D516/1002D2EE. The native corpus
//! covers component reads and one framed scalar, not an entire Side savefile.

#![cfg(feature = "ares")]

use super::*;
use crate::rules::ares_side_extension::AresSideExtension;
use crate::util::save_codec;
use serde_json::Value;

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/ares_eva.json")
}

fn controls(kind: &str) -> impl Iterator<Item = &'static Value> + '_ {
    corpus()["read_bind"]["controls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(move |row| row["kind"] == kind)
}

fn integer(row: &Value, field: &str) -> i32 {
    i32::try_from(row[field].as_i64().unwrap()).unwrap()
}

fn read(state: &mut AresSideState, id: &str, pass: &IniFile, eva: &EvaRegistry) {
    state.read_pass(id, 0, pass, eva, &SideRegisteredTypes::default(), |_| None);
}

fn catalog() -> EvaRegistry {
    EvaRegistry::from_ini(&crate::test_fixture::ini_sections(
        &controls("registration").next().unwrap()["input"],
    ))
}

#[test]
fn side_constructor_defaults_and_retained_phases_match_native_controls() {
    let eva = catalog();
    for row in controls("side_default") {
        let mut state = AresSideState::default();
        assert_eq!(
            state.fields.eva_voice_index,
            integer(row, "constructor_voice_index")
        );
        assert_eq!(state.phase, 0);
        read(
            &mut state,
            row["id"].as_str().unwrap(),
            &IniFile::empty(),
            &eva,
        );
        assert_eq!(
            state.fields.eva_voice_index,
            integer(row, "voice_index"),
            "{row}"
        );
        assert_eq!(state.phase, 2);
    }
    for row in controls("side_phase_dispatch") {
        let initial = AresSideState {
            phase: integer(row, "initial_phase"),
            fields: AresSideExtension {
                eva_voice_index: integer(row, "initial_voice_index"),
                ..Default::default()
            },
        };
        let mut state = initial.clone();
        // The native phase control records the unrelated full reader as a
        // sink, so an empty pass compares its initializer/retention outcome.
        read(&mut state, "SyntheticSide", &IniFile::empty(), &eva);
        assert_eq!(state.phase, integer(row, "final_phase"), "{row}");
        assert_eq!(
            state.fields.eva_voice_index,
            integer(row, "final_voice_index"),
            "{row}"
        );

        // Also make admission observable through the real production tag reader.
        let mut state = initial;
        read(
            &mut state,
            "SyntheticSide",
            &IniFile::from_str("[SyntheticSide]\nEVA.Tag=VoiceA\n"),
            &eva,
        );
        assert_eq!(
            state.fields.eva_voice_index,
            if row["reader_visits"].as_array().unwrap().is_empty() {
                integer(row, "initial_voice_index")
            } else {
                3
            },
            "{row}",
        );
    }
}

#[test]
fn side_tag_history_retains_missing_empty_unknown_and_resolves_none_after_lookup() {
    let eva = catalog();
    let mut state = AresSideState::default();
    read(&mut state, "SyntheticSide", &IniFile::empty(), &eva);
    for row in controls("side_read_and_bind") {
        assert_eq!(state.fields.eva_voice_index, integer(row, "prior"), "{row}");
        let pass = match row["input"].as_str() {
            Some(tag) => IniFile::from_str(&format!("[SyntheticSide]\nEVA.Tag={tag}\n")),
            None => IniFile::empty(),
        };
        let sidebar = (
            state.fields.sidebar_mix_file_index,
            state.fields.sidebar_yuri_file_names,
        );
        read(&mut state, "SyntheticSide", &pass, &eva);
        assert_eq!(
            state.fields.eva_voice_index,
            integer(row, "resolved"),
            "{row}"
        );
        assert_eq!(state.phase, 2);
        assert_eq!(
            (
                state.fields.sidebar_mix_file_index,
                state.fields.sidebar_yuri_file_names
            ),
            sidebar
        );
    }
    for row in controls("none_tag_precedence") {
        let mut text = String::from("[EVATypes]\n");
        for (index, tag) in row["tags"].as_array().unwrap().iter().enumerate() {
            text.push_str(&format!("{index}={}\n", tag.as_str().unwrap()));
        }
        let eva = EvaRegistry::from_ini(&IniFile::from_str(&text));
        let mut state = AresSideState {
            phase: 2,
            fields: AresSideExtension {
                eva_voice_index: integer(row, "prior"),
                ..Default::default()
            },
        };
        let pass = IniFile::from_str(&format!(
            "[SyntheticSide]\nEVA.Tag={}\n",
            row["input"].as_str().unwrap(),
        ));
        read(&mut state, "SyntheticSide", &pass, &eva);
        assert_eq!(
            state.fields.eva_voice_index,
            integer(row, "resolved"),
            "{row}"
        );
    }
}

#[test]
fn retained_side_save_roundtrip_preserves_native_signed_eva_scalars_without_catalog_clamp() {
    // VERA's enclosing serde format is its own save format. Only this retained
    // signed field's value and four payload bytes are compared with native;
    // the native extension framing and entire Side/OLE record are excluded.
    let empty_catalog = EvaRegistry::default();
    for row in corpus()["scalar_roundtrips"].as_array().unwrap() {
        let mut state = AresSideState::default();
        read(&mut state, "ThirdSide", &IniFile::empty(), &empty_catalog);
        state.fields.eva_voice_index = integer(row, "input");
        let payload = save_codec::serialize(&state.fields.eva_voice_index).unwrap();
        let payload_hex: String = payload.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(payload_hex, row["payload_hex"].as_str().unwrap(), "{row}");

        let bytes = save_codec::serialize(&state).unwrap();
        let sidebar = (
            state.fields.sidebar_mix_file_index,
            state.fields.sidebar_yuri_file_names,
        );
        state.fields.eva_voice_index = integer(row, "overwrite_before_load");
        assert_eq!(state.fields.eva_voice_index, 99);
        state = save_codec::deserialize(&bytes).unwrap();
        assert_eq!(
            state.fields.eva_voice_index,
            integer(row, "restored"),
            "{row}"
        );
        assert_eq!(state.phase, 2);
        assert_eq!(
            (
                state.fields.sidebar_mix_file_index,
                state.fields.sidebar_yuri_file_names
            ),
            sidebar
        );
        // A later read without a recognized tag retains the serialized index,
        // including extra index4 with an empty current catalog and sentinel-1.
        read(
            &mut state,
            "ThirdSide",
            &IniFile::from_str("[ThirdSide]\nEVA.Tag=unknown\n"),
            &empty_catalog,
        );
        assert_eq!(
            state.fields.eva_voice_index,
            integer(row, "restored"),
            "{row}"
        );
    }
}
