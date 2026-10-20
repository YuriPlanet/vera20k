//! Damage-sound catalog binding through the actual retained rules passes.
//! These reader regressions do not establish receiver arithmetic parity.
//! Original gamemd:
//! type712EB7..712F07 and Rules66AC98..66ACDD use ReadString528A10 (128 bytes)
//! then VocFindIndex7514D0, retaining previous valid IDs on failed resolution.

use crate::rules::ini_parser::IniFile;
use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
use crate::rules::sound_ini::SoundRegistry;
use std::sync::Arc;

#[test]
fn damage_sound_global_reference_retention_matches_whole_native_reader_passes() {
    let Some((root, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let Some(sound_ini) = crate::rules::retail_ini_fixture::retail_ini("soundmd.ini") else {
        return;
    };
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_damage_sound_joined.json",
    ))
    .unwrap();
    assert_eq!(corpus["selected_profile"], "all");
    let passes = corpus["threshold_controls"]["global_later_passes"]
        .as_array()
        .unwrap();
    assert_eq!(passes.len(), 3, "all executed later reader controls");
    let name = |index: &serde_json::Value| -> Option<&str> {
        let index = index.as_i64().unwrap();
        if index == -1 {
            None
        } else {
            Some(
                corpus["selected_sounds"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|sound| sound["fixture_index"].as_i64().unwrap() == index)
                    .unwrap()["name"]
                    .as_str()
                    .unwrap(),
            )
        }
    };
    let sounds = Arc::new(SoundRegistry::from_ini(&sound_ini));
    let mut previous = name(&passes[0]["before"]);
    for end in 0..=passes.len() {
        // Original full6691E0 performs each later AV read in place. Exercise
        // the same exact values as ordered LANG/mode/map production passes;
        // no public/raw GeneralRules writer or partial rebinding is used.
        let layers: Vec<_> = passes[..end]
            .iter()
            .map(|pass| {
                IniFile::from_str(&format!(
                    "[AudioVisual]\nBuildingDamageSound={}\n",
                    pass["raw"].as_str().unwrap()
                ))
            })
            .collect();
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            root.clone(),
            layers.first().cloned(),
            art.clone(),
            Arc::clone(&sounds),
            Default::default(),
        )
        .unwrap();
        let rules = owner
            .load_scenario(
                NativeScenarioRulesPrefix::NonCampaign(layers.get(1)),
                layers.get(2).unwrap_or(&IniFile::empty()),
            )
            .unwrap()
            .into_parts()
            .0;
        if end == 0 {
            assert_eq!(rules.general.building_damage_sound(), previous);
        } else {
            let native = &passes[end - 1];
            let label = native["label"].as_str().unwrap();
            assert_eq!(previous, name(&native["before"]), "{label}: prior binding");
            assert_ne!(native["returned"].as_u64().unwrap(), 0, "{label}");
            let expected = name(&native["after"]);
            assert_eq!(
                rules.general.building_damage_sound(),
                expected,
                "{label}: retained production AV reader"
            );
            previous = expected;
        }
    }
}
