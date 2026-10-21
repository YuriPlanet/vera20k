//! Bounded native catalog/filename controls from Ares10063660/100639C0/10063920.
//! The corpus executes original ReadString and secure copy; it does not certify
//! installed hooks, physical startup, queue timing, saves or audible playback.

use super::*;
use serde_json::Value;

fn controls() -> &'static [Value] {
    crate::test_fixture::json("tools/spatial_oracle/ares_eva.json")["read_bind"]["controls"]
        .as_array()
        .unwrap()
}

fn control(kind: &str) -> &'static Value {
    controls().iter().find(|row| row["kind"] == kind).unwrap()
}

fn registry() -> EvaRegistry {
    EvaRegistry::from_ini(&crate::test_fixture::ini_sections(
        &control("registration")["input"],
    ))
}

#[test]
fn fixed_catalog_uses_feature_specific_registration_and_voice_lookup() {
    let registry = registry();
    for (tag, index) in [("aLLied", 0), ("rUSsian", 1), ("yURI", 2)] {
        assert_eq!(registry.voice_index_for_tag(tag), Some(index));
    }
    for tag in ["", "unknown", " VoiceA", "VoiceA ", " Allied"] {
        assert_eq!(registry.voice_index_for_tag(tag), None, "{tag:?}");
    }
    assert_eq!(registry.len(), 2, "duplicate event retains its first slot");
    assert_eq!(registry.entry_by_index(0).unwrap().name, "EVA_TEST");
    assert_eq!(registry.index_of("eva_test"), Some(0));

    #[cfg(feature = "ares")]
    {
        let expected = control("registration");
        let names: Vec<_> = registry.entries.iter().map(|entry| &entry.name).collect();
        assert_eq!(
            serde_json::to_value(names).unwrap(),
            expected["event_names"]
        );
        assert_eq!(
            serde_json::to_value(&registry.extra_tags).unwrap(),
            expected["tags"],
            "stock names skipped; duplicate tags retain first spelling and order",
        );
        for (ordinal, tag) in expected["tags"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                registry.voice_index_for_tag(&tag.as_str().unwrap().to_ascii_lowercase()),
                Some(ordinal as i32 + 3),
            );
        }
        assert_eq!(registry.voice_index_for_tag(&"X".repeat(205)), None);
        assert_eq!(registry.index_of(&"N".repeat(45)), None);
    }
    #[cfg(not(feature = "ares"))]
    {
        assert_eq!(registry.voice_index_for_tag("VoiceA"), None);
        assert_eq!(registry.voice_index_for_tag("VoiceB"), None);
        assert_eq!(registry.entry_by_index(1).unwrap().name, "N".repeat(45));
        assert_eq!(registry.index_of(&"N".repeat(45)), Some(1));
    }
}

#[test]
fn filenames_match_native_columns_and_keep_stock_fallback_without_ares() {
    let registry = registry();
    let entry = registry.entry("EVA_TEST").unwrap();
    for row in controls().iter().filter(|row| row["kind"] == "filename") {
        let voice = i32::try_from(row["voice_index"].as_i64().unwrap()).unwrap();
        let expected = if crate::rules::ARES_ENABLED || (0..=2).contains(&voice) {
            row["filename"].as_str().unwrap()
        } else {
            "STOCK_Y"
        };
        assert_eq!(
            entry.column_by_voice(voice).unwrap_or(""),
            expected,
            "{row}"
        );
    }
    // This registered tag has no per-event sample; an empty extra slot stays empty.
    #[cfg(feature = "ares")]
    assert_eq!(entry.column_by_voice(5), None);
    assert_eq!(entry.column(EvaSide::Yuri), Some("STOCK_Y"));
}

#[cfg(feature = "ares")]
#[test]
fn extra_columns_match_native_read_and_missing_reread_clears_previous_sample() {
    let mut registry = registry();
    let samples = |entry: &EvaEntry| {
        entry
            .extra_columns
            .iter()
            .map(|sample| sample.as_deref().unwrap_or(""))
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        serde_json::to_value(samples(&registry.entries[0])).unwrap(),
        control("extra_columns")["samples"],
    );
    let row = control("extra_columns_reread");
    let ini = crate::test_fixture::ini_sections(&row["input"]);
    registry.entries[0].read_extra_columns(ini.section("EVA_TEST"), &registry.extra_tags);
    assert_eq!(
        serde_json::to_value(samples(&registry.entries[0])).unwrap(),
        row["samples"],
    );
    // Exercise the same replacement boundary for an explicitly empty key and
    // missing section, without adding a second catalog reader or mutable API.
    let ini = IniFile::from_str("[EVA_TEST]\nVoiceB=\n");
    registry.entries[0].read_extra_columns(ini.section("EVA_TEST"), &registry.extra_tags);
    assert_eq!(registry.entries[0].column_by_voice(4), None);
    registry.entries[0].read_extra_columns(None, &registry.extra_tags);
    assert!(
        registry.entries[0]
            .extra_columns
            .iter()
            .all(Option::is_none)
    );
}

#[cfg(feature = "ares")]
#[test]
fn registered_none_spellings_are_resolved_before_side_none_handling() {
    for row in controls()
        .iter()
        .filter(|row| row["kind"] == "none_tag_precedence")
    {
        let mut ini = String::from("[EVATypes]\n");
        for (index, tag) in row["tags"].as_array().unwrap().iter().enumerate() {
            ini.push_str(&format!("{index}={}\n", tag.as_str().unwrap()));
        }
        let registry = EvaRegistry::from_ini(&IniFile::from_str(&ini));
        let expected = row["registered"]
            .as_bool()
            .unwrap()
            .then(|| i32::try_from(row["resolved"].as_i64().unwrap()).unwrap());
        assert_eq!(
            registry.voice_index_for_tag(row["input"].as_str().unwrap()),
            expected,
            "{row}",
        );
    }
}
