//! The process owner binds native sound IDs to fixed SOUNDMD names, retaining
//! valid values across root/LANG/mode/map readers (712FF1/7130A5/6699C8).

use crate::rules::ini_parser::IniFile;
use crate::rules::sound_ini::SoundRegistry;

/// Consume the actual original selected readers, without equating native
/// fixture-relative Voc ordinals to Rust's independently built catalogue.
#[test]
fn original_house_audio_reference_reads_match_production_layers_and_retention() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;
    for (path, provider, keys) in [
        (
            "tools/input_oracle/radar_availability_audio.json",
            "NARADR",
            ["RadarOn", "RadarOff"],
        ),
        (
            "tools/input_oracle/spy_sat_audio.json",
            "GASPYSAT",
            ["SpySatActivationSound", "SpySatDeactivationSound"],
        ),
    ] {
        let native = crate::test_fixture::json(path);
        let sounds = SoundRegistry::from_ini(&crate::test_fixture::ini_sections(
            &native["retail"]["sound_sections"],
        ));
        let project = |layers: &RulesLayerStack| {
            let processed = layers.process().unwrap();
            let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
            rules.bind_type_sound_references(processed.ini(), &sounds);
            rules
        };
        let names = |references: &serde_json::Value| {
            keys.map(|key| {
                let id = references[key].as_i64().unwrap();
                if id == -1 {
                    return None;
                }
                Some(
                    native["retail"]["sounds"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|sound| sound["index"].as_i64() == Some(id))
                        .unwrap()["name"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                )
            })
        };
        let actual = |rules: &RuleSet| {
            if provider == "NARADR" {
                [
                    rules.general.radar_on.clone(),
                    rules.general.radar_off.clone(),
                ]
            } else {
                [
                    rules.general.spy_sat_activation_sound.clone(),
                    rules.general.spy_sat_deactivation_sound.clone(),
                ]
            }
        };
        let assert_provider = |rules: &RuleSet, expected: &serde_json::Value| {
            let object = rules.object(provider).unwrap();
            assert_eq!(object.radar, expected["radar"].as_bool().unwrap());
            assert_eq!(object.spy_sat, expected["spy_sat"].as_bool().unwrap());
            assert_eq!(
                object.strength,
                expected["strength"].as_i64().unwrap() as i32
            );
        };
        let mut layers = RulesLayerStack::new(IniFile::from_str(""));
        assert_eq!(
            actual(&project(&layers)),
            names(&native["retail"]["reference_constructor"])
        );
        let controls = native["retail"]["reader_controls"].as_array().unwrap();
        assert_eq!(controls.len(), 14);
        for row in controls {
            assert_eq!(
                actual(&project(&layers)),
                names(&row["before"]),
                "{} before",
                row["name"]
            );
            layers.push(
                RulesLayerKind::Scenario,
                crate::test_fixture::ini_sections(&row["input"]),
            );
            assert_eq!(
                actual(&project(&layers)),
                names(&row["after"]),
                "{} after",
                row["name"]
            );
        }

        // Native created the provider before these selected field reads. The existing
        // processor receives that same declared registration prior, not an orphan
        // type body implicitly allocated by a later layer.
        let mut layers = RulesLayerStack::new(IniFile::from_str(&format!(
            "[BuildingTypes]\n0={provider}\n"
        )));
        for row in native["retail"]["reference_layers"].as_array().unwrap() {
            if row["absent"] == true {
                assert_eq!(row["file"], "LANGRULE.INI");
                continue;
            }
            assert_eq!(actual(&project(&layers)), names(&row["before"]));
            let kind = match row["file"].as_str().unwrap() {
                "RULESMD.INI" => RulesLayerKind::RulesMd,
                "LANGRULE.INI" => RulesLayerKind::LangRule,
                "MPBattleMD.ini" => RulesLayerKind::GameMode,
                "XMP03T4.MAP" => RulesLayerKind::Scenario,
                other => panic!("unexpected original House audio source {other}"),
            };
            layers.push(kind, crate::test_fixture::ini_sections(&row["input"]));
            let rules = project(&layers);
            assert_eq!(actual(&rules), names(&row["after"]), "{}", row["file"]);
            assert_provider(&rules, &row["provider"]);
        }

        // Also consume the complete physical retail rules through the same
        // production root/LANG/mode/map reader used by the paid scenario.
        if let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        {
            let final_layer = native["retail"]["reference_layers"]
                .as_array()
                .unwrap()
                .iter()
                .rfind(|row| row["absent"] != true)
                .unwrap();
            assert_eq!(actual(&retail.rules), names(&final_layer["after"]));
            assert_provider(&retail.rules, &final_layer["provider"]);
        }
    }
}
