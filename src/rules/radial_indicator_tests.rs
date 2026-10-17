//! Production layered readers consume original type/default histories.
use super::ini_parser::IniFile;
use super::native_processing::{RulesLayerKind, RulesLayerStack};
use super::object_type::{ObjectCategory, ObjectType};
use super::ruleset::RuleSet;

fn fields(object: &ObjectType) -> serde_json::Value {
    serde_json::json!({
        "has_radial_indicator":object.has_radial_indicator(),
        "concentric":object.concentric_radial_indicator(),
        "gap_generator":object.gap_generator,
        "gap_radius":object.gap_radius_in_cells,
        "super_gap_radius":object.super_gap_radius_in_cells,
        "sensor_array":object.sensor_array,
        "cloak_generator":object.cloak_generator,
        "cloak_radius":object.cloak_radius_in_cells,
        "psychic_radius":object.psychic_detection_radius,
    })
}

fn assert_fields(object: &ObjectType, expected: &serde_json::Value) {
    for (key, value) in fields(object).as_object().unwrap() {
        assert_eq!(value, &expected[key], "{} {key}", object.id);
    }
}

#[test]
fn constructors_and_each_admitted_radial_reader_history_match_original() {
    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/radial_indicator.json");
    let reader = &native["reader_inputs"];
    for (name, expected) in reader["constructors"].as_object().unwrap() {
        let blank = IniFile::empty();
        let object = ObjectType::from_ini_section(
            name,
            blank.section_or_empty(name),
            ObjectCategory::Building,
        );
        assert_fields(&object, expected);
    }
    let names = reader["constructors"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let registry = format!(
        "[BuildingTypes]\n{}",
        names
            .iter()
            .enumerate()
            .map(|(i, name)| format!("{i}={name}\n"))
            .collect::<String>()
    );
    let mut layers = RulesLayerStack::new(IniFile::from_str(&registry));
    for row in reader["retained_history"].as_array().unwrap() {
        let source = row["sections"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, keys)| {
                format!(
                    "[{name}]\n{}",
                    keys.as_object()
                        .unwrap()
                        .iter()
                        .map(|(key, value)| format!("{key}={}\n", value.as_str().unwrap()))
                        .collect::<String>()
                )
            })
            .collect::<String>();
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(&source));
        let rules = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
        let object = rules.object(row["type"].as_str().unwrap()).unwrap();
        assert_fields(object, &row["after"]);
        for (flag, expected) in row["range_by_268"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                object.building_sensor_range(flag != 0, None),
                expected.as_i64().unwrap() as i32
            );
        }
    }
}

#[test]
fn stock_radial_fields_and_ranges_use_physical_rules_mode_map_and_art() {
    let Some(retail) = super::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/radial_indicator.json");
    for name in ["GAGAP", "NAPSIS"] {
        let object = retail.rules.object(name).unwrap();
        assert_fields(object, &native["reader_inputs"]["final"][name]);
    }
    assert!(
        !retail
            .rules
            .object("YAPSYT")
            .unwrap()
            .has_radial_indicator()
    );
    assert!(
        !retail
            .rules
            .object("GAPILL")
            .unwrap()
            .has_radial_indicator()
    );
}
