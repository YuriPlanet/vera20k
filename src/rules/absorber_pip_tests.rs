//! Building pip-tail fields through the shared production rules-pass owner.
use super::ObjectType;
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;
use serde_json::{Value, json};

fn fields(object: &ObjectType) -> Value {
    json!({
        "pip_scale": object.pip_scale as i32,
        "pips_draw_for_all": object.pips_draw_for_all(),
        "passengers": object.passengers,
        "unit_absorb": object.unit_absorb,
        "infantry_absorb": object.infantry_absorb,
        "can_be_occupied": object.can_be_occupied,
        "show_occupant_pips": object.show_occupant_pips,
        "max_number_occupants": object.max_number_occupants,
        "gunner": object.gunner,
    })
}

fn assert_fields(object: &ObjectType, expected: &Value, context: &str) {
    for (key, actual) in fields(object).as_object().unwrap() {
        assert_eq!(actual, &expected[key], "{context}: {key}");
    }
}

#[test]
fn original_absorber_reader_defaults_survive_admitted_and_absent_passes() {
    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/absorber_pips.json");
    let controls = &native["reader_controls"];
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=AbsorberProbe\n[AbsorberProbe]\n",
    ));
    assert_fields(
        RuleSet::from_rules_layers(&layers)
            .unwrap()
            .object("AbsorberProbe")
            .unwrap(),
        &controls["constructor"],
        "original constructor",
    );
    // Original710D22/71412B..145 and45E0DE/460988..9D6 use the
    // current value as default. Existing IniSection readers fold these passes.
    for row in controls["controls"].as_array().unwrap() {
        // These are native cache inputs, including a stored-empty value that
        // physical INI parsing discards. Preserve the recorded cache prior
        // through the existing store owner before production layer processing.
        let pass = IniFile::from_sections_for_test(
            row["input"]["sections"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(name, entries)| {
                    let mut section = IniSection::new(name.clone());
                    for (key, value) in entries.as_object().unwrap() {
                        section.set(key, value.as_str().unwrap());
                    }
                    section
                }),
        );
        assert_eq!(
            pass.section("AbsorberProbe").is_some(),
            row["admitted"].as_bool().unwrap()
        );
        layers.push(RulesLayerKind::Scenario, pass);
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_fields(
            rules.object("AbsorberProbe").unwrap(),
            &row["after"],
            row["name"].as_str().unwrap(),
        );
    }
}

#[test]
fn physical_bio_reactor_fields_match_original_layered_readers() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/absorber_pips.json");
    let reactor = retail.rules.object("YAPOWR").unwrap();
    assert_fields(
        reactor,
        &native["readers"]["result"],
        "physical Battle reactor",
    );
    assert_eq!(
        reactor.absorbs_passengers(),
        native["raster_controls"][0]["actor_results"][0]["is_absorb_allowed"]
            .as_bool()
            .unwrap()
    );
    let last_layer = native["readers"]["layers"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    for name in ["INIT", "YENGINEER"] {
        let object = retail.rules.object(name).unwrap();
        let expected = &last_layer["types"][name]["after"];
        assert_eq!(
            object.pip() as u64,
            expected["pip"].as_u64().unwrap(),
            "{name}"
        );
        assert_eq!(
            object.occupy_pip() as u64,
            expected["occupy_pip"].as_u64().unwrap(),
            "{name}"
        );
    }
}
