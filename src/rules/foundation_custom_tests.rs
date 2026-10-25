//! Synthetic production rules, compared with the two executed Ares controls.

use super::Foundation;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::RulesLayerStack;
use crate::rules::ruleset::RuleSet;
use serde_json::Value;

pub(crate) fn native_rows() -> Vec<Value> {
    let packet: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/ares_foundation.json",
    ))
    .unwrap();
    packet["controls"].as_array().unwrap().clone()
}

fn base_ini() -> IniFile {
    IniFile::from_str(
        "[Clear]\nBuildable=yes\n[Countries]\n0=Americans\n[Americans]\nSide=GDI\n\
        [BuildingTypes]\n0=CUSTOM_TEST\n[CUSTOM_TEST]\nImage=CUSTOM_ART\n\
        Owner=Americans\nStrength=500\nTechLevel=1\nFoundation=4x4\n",
    )
}

pub(crate) fn fixed_art(row: &Value) -> IniFile {
    let mut text = String::from("[CUSTOM_ART]\nHeight=3\n");
    for (key, value) in row["authored"].as_object().unwrap() {
        text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
    }
    IniFile::from_str(&text)
}

pub(crate) fn rules(row: &Value) -> RuleSet {
    let art = fixed_art(row);
    let processed = RulesLayerStack::new(base_ini())
        .process_with_fixed_art(&art)
        .unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules
}

fn terminated(row: &Value) -> Value {
    let values = row.as_array().unwrap();
    Value::Array(values[..values.len() - 1].to_vec())
}

#[test]
fn custom_foundations_match_native_reader_geometry_and_retention() {
    use crate::sim::docking::building_dock::{foundation_exit_list, foundation_exit_pair};
    for row in native_rows() {
        let art = fixed_art(&row);
        let processed = RulesLayerStack::new(base_ini())
            .process_with_fixed_art(&art)
            .unwrap();
        let rules = rules(&row);
        let object = rules.object("CUSTOM_TEST").unwrap();
        let foundation = &object.foundation;
        let expected = &row["installed"];
        assert_eq!(
            u64::from(foundation.selector()),
            expected["selector"].as_u64().unwrap()
        );
        assert_eq!(
            serde_json::json!(foundation.dimensions()),
            serde_json::json!([expected["width"], expected["height"]])
        );
        assert_eq!(
            serde_json::json!(foundation.cell_offsets()),
            terminated(&expected["cells"])
        );
        assert_eq!(
            serde_json::json!(foundation_exit_list(foundation)),
            terminated(&expected["outline"])
        );
        let bounds = foundation.cell_bounds();
        assert_eq!(
            serde_json::json!([bounds.0, bounds.1, bounds.2, bounds.3]),
            row["getters"]["occupied_bounds"]
        );
        for bib in [false, true] {
            for include in [false, true] {
                let key = format!("height_bib{}_include{}", u8::from(bib), u8::from(include));
                assert_eq!(
                    i64::from(foundation.height_with_bib(include, bib)),
                    row["getters"][key]["result"].as_i64().unwrap()
                );
            }
        }
        // The rectangle's actual index10 was captured by native GetOutline.
        // The sparse reader still executes all ten slots (recorded key-format
        // calls); absent slots and the final slot hold its captured sentinel.
        let native_outline = expected["outline"].as_array().unwrap();
        let pair10 = native_outline
            .get(10)
            .unwrap_or_else(|| native_outline.last().unwrap());
        assert_eq!(
            serde_json::json!(foundation_exit_pair(foundation, 10)),
            *pair10
        );
        let outline_reads = row["numeric_format_service_calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| call["format"] == "FoundationOutline.%d")
            .count();
        assert_eq!(
            foundation.custom_outline().unwrap().len(),
            outline_reads + 1
        );
        assert!(row["code_unchanged"].as_bool().unwrap());
        assert_eq!(row["rng_states_before"], row["rng_states_after"]);

        let registry = processed
            .into_ini_and_native_type_construction_trace()
            .1
            .into_registry_state_discarding_events();
        let changed_art = IniFile::from_str(
            "[CUSTOM_ART]\nFoundation=Custom\n\
            Foundation.X=1\nFoundation.Y=1\nFoundation.0=0,0\n",
        );
        let later = RulesLayerStack::new(base_ini())
            .process_with_fixed_art_and_registry_state(&changed_art, registry)
            .unwrap();
        let retained = RuleSet::from_processed_rules(&later).unwrap();
        assert_eq!(
            retained.object("CUSTOM_TEST").unwrap().foundation,
            *foundation
        );
        assert_eq!(row["reload_reader_calls"], 0);
    }
}

#[test]
fn fixed_art_custom_geometry_affects_configuration_hash_and_rejects_unsupported_sizes() {
    let row = native_rows().remove(0);
    let original = rules(&row);
    let mut changed = row.clone();
    changed["authored"]["FoundationOutline.0"] = Value::String("9,9".to_owned());
    let alternate = rules(&changed);
    assert_eq!(original.source_ini_hash(), alternate.source_ini_hash());
    assert_ne!(
        original.simulation_config_hash(),
        alternate.simulation_config_hash()
    );
    let art =
        IniFile::from_str("[CUSTOM_ART]\nFoundation=Custom\nFoundation.X=65536\nFoundation.Y=1\n");
    let processed = RulesLayerStack::new(base_ini())
        .process_with_fixed_art(&art)
        .unwrap();
    assert!(RuleSet::from_processed_rules(&processed).is_err());
    assert_eq!(
        Foundation::from("Custom").dimensions(),
        (1, 1),
        "the stock token reader cannot construct an extension without its ART suffix"
    );
}

#[test]
fn custom_outline_backing_and_cursor_match_native_direct_consumers() {
    use crate::sim::docking::building_dock::foundation_exit_pair;
    let packet: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/ares_foundation.json",
    ))
    .unwrap();
    let row = &packet["outline_backing"];
    let rules = rules(row);
    let foundation = &rules.object("CUSTOM_TEST").unwrap().foundation;
    assert_eq!(
        serde_json::json!(foundation.custom_outline().unwrap()),
        row["outline_backing"]
    );
    assert_eq!(
        serde_json::json!(foundation_exit_pair(foundation, 10)),
        row["index10"]
    );
    let bounds = foundation.cell_bounds();
    assert_eq!(
        serde_json::json!([
            (i32::from(bounds.2) - i32::from(bounds.0) + 1).max(1),
            (i32::from(bounds.3) - i32::from(bounds.1) + 1).max(1),
        ]),
        row["cursor_bounds"]
    );
    let cursor = (
        row["cursor_input"][0].as_u64().unwrap() as u16,
        row["cursor_input"][1].as_u64().unwrap() as u16,
    );
    let origin = foundation.placement_origin_at_cursor(cursor);
    assert_eq!(serde_json::json!(origin), row["marker_origin"]);
    assert_eq!(
        serde_json::json!([
            origin.0.wrapping_sub(cursor.0) as i16,
            origin.1.wrapping_sub(cursor.1) as i16,
        ]),
        row["cursor_offset"]
    );
    assert_eq!(row["marker_bool"], 1);
    assert!(row["code_unchanged"].as_bool().unwrap());
    assert_eq!(row["rng_states_before"], row["rng_states_after"]);
}
