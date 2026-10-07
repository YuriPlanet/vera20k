//! Original Infantry constructor and ReadSpeedType results, including retained
//! per-pass defaults. Full type construction is only claimed for the native
//! constructor; the native reader corpus executes the selected field boundary.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/infantry_speed_type.json",
    ))
    .unwrap()
}

fn supplied_cache(sections: &Value) -> IniFile {
    let mut ini = IniFile::empty();
    for (name, entries) in sections.as_object().unwrap() {
        for (key, value) in entries.as_object().unwrap() {
            // Native controls supply raw cached strings. Preserve whitespace
            // and empty entries instead of running the physical-file loader.
            ini.projection_section_mut(name)
                .set(key, value.as_str().unwrap());
        }
    }
    ini
}

#[test]
fn infantry_constructor_and_speed_type_reader_match_original() {
    let native = corpus();
    let cases = native["controls"].as_array().unwrap();
    assert_eq!(cases.len(), 26);
    for case in cases {
        let mut ini = supplied_cache(&case["supplied_sections"]);
        let section = ini.projection_section_mut("ENGINEER");
        let object = ObjectType::from_ini_section("ENGINEER", section, ObjectCategory::Infantry);
        assert_eq!(
            object.speed_type as i64,
            case["after"].as_i64().unwrap(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn speed_type_rules_passes_retain_current_field_and_invalid_value() {
    let native = corpus();
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[InfantryTypes]\n0=ENGINEER\n[ENGINEER]\nStrength=75\n",
    ));
    for pass in native["explicit_layer_controls"].as_array().unwrap() {
        layers.push(
            RulesLayerKind::Scenario,
            supplied_cache(&pass["supplied_sections"]),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(
            rules.object("ENGINEER").unwrap().speed_type as i64,
            pass["after"].as_i64().unwrap(),
            "{}",
            pass["name"]
        );
    }
}

#[test]
fn retail_engineer_keeps_original_foot_constructor_default() {
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let rules = RuleSet::from_ini(&ini).unwrap();
    assert_eq!(
        rules.object("ENGINEER").unwrap().speed_type as i64,
        corpus()["physical"]["final_speed_type"].as_i64().unwrap()
    );
}
