//! Retained Country owner against executed Ares component controls. The native
//! fixture declares cached INI/prior registry state; production layer, save and
//! reset checks below separately exercise the connected Rules owner.

use super::*;
use crate::rules::ruleset::RuleSet;
use crate::util::save_codec;
use serde_json::Value;

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/rules_oracle/ares_country_veterancy.json")
}

fn names(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().to_owned())
        .collect()
}

fn pass(id: &str, values: &Value) -> IniFile {
    let mut section = IniSection::new(id.to_owned());
    if let Some(values) = values.as_object() {
        for (key, value) in values {
            if let Some(value) = value.as_str() {
                section.set(key, value);
            }
        }
    }
    let mut ini = IniFile::empty();
    ini.replace_first_section(section);
    ini
}

fn set_original_lists(state: &mut CountryVeterancyState, value: &Value) {
    state.infantry = names(&value["VeteranInfantry"]);
    state.units = names(&value["VeteranUnits"]);
    state.aircraft = names(&value["VeteranAircraft"]);
}

fn assert_original_lists(state: &CountryVeterancyState, expected: &Value, label: &str) {
    for (key, actual) in [
        ("VeteranInfantry", &state.infantry),
        ("VeteranUnits", &state.units),
        ("VeteranAircraft", &state.aircraft),
    ] {
        assert_eq!(*actual, names(&expected[key]), "{label}: {key}");
    }
}

#[test]
fn ares_country_list_reads_match_native_find_only_controls() {
    for row in corpus()["country_lists"].as_array().unwrap() {
        let initial = &row["initial"];
        let mut reader = RulesPassProcessor::default();
        let country = initial["country_id"].as_str().unwrap();
        let index = reader.find_or_construct(RulesTypeFamily::Country, country);
        set_original_lists(
            &mut reader.family_mut(RulesTypeFamily::Country)[index].country_veterancy,
            &initial["veteran_lists"],
        );
        for (key, family) in [
            ("VeteranInfantry", RulesTypeFamily::Infantry),
            ("VeteranUnits", RulesTypeFamily::Vehicle),
            ("VeteranAircraft", RulesTypeFamily::Aircraft),
        ] {
            for id in names(&initial["registered_type_ids"][key]) {
                reader.find_or_construct(family, &id);
            }
        }
        let ini = pass(country, &row["values"]);
        reader.process_house_family(&ini, false);
        let label = row["name"].as_str().unwrap();
        assert_original_lists(
            &reader.families[&RulesTypeFamily::Country][index].country_veterancy,
            &row["first"],
            label,
        );
        if row["after_explicit_reread"].is_object() {
            for (key, family) in [
                ("VeteranInfantry", RulesTypeFamily::Infantry),
                ("VeteranUnits", RulesTypeFamily::Vehicle),
                ("VeteranAircraft", RulesTypeFamily::Aircraft),
            ] {
                for id in names(&initial["unregistered_type_ids"][key]) {
                    reader.find_or_construct(family, &id);
                }
            }
            assert_original_lists(
                &reader.families[&RulesTypeFamily::Country][index].country_veterancy,
                &row["after_later_registration_without_read"],
                label,
            );
            reader.process_house_family(&ini, false);
            assert_original_lists(
                &reader.families[&RulesTypeFamily::Country][index].country_veterancy,
                &row["after_explicit_reread"],
                label,
            );
        }
    }
}

fn seed_parent(reader: &mut RulesPassProcessor, initial: &Value) -> usize {
    let id = initial["parent_id"].as_str().unwrap();
    let index = reader.find_or_construct(RulesTypeFamily::Country, id);
    let parent = &mut reader.family_mut(RulesTypeFamily::Country)[index];
    parent.country_parent = id.to_owned();
    parent.body.set("Name", "PARENT_ALIAS");
    parent.country_veterancy.ares.phase = 2;
    parent.country_veterancy.ares.buildings = names(&initial["parent_veteran_buildings"]);
    set_original_lists(
        &mut parent.country_veterancy,
        &initial["parent_original_country_vectors"],
    );
    index
}

#[test]
fn ares_country_extension_histories_match_native_parent_and_retention() {
    let corpus = corpus();
    for row in corpus["extension_histories"].as_array().unwrap() {
        let initial = &row["initial"];
        let mut reader = RulesPassProcessor::default();
        for id in names(&initial["registered_building_ids"]) {
            reader.find_or_construct(RulesTypeFamily::Building, &id);
        }
        let country = initial["country_id"].as_str().unwrap();
        let index = reader.find_or_construct(RulesTypeFamily::Country, country);
        let child = &mut reader.family_mut(RulesTypeFamily::Country)[index];
        child.country_parent = initial["parent_country"].as_str().unwrap().to_owned();
        set_original_lists(
            &mut child.country_veterancy,
            &initial["original_country_vectors"],
        );
        child.country_veterancy.ares = AresCountryState {
            phase: initial["extension_phase"].as_i64().unwrap() as i32,
            parent_initialized: initial["parent_initialized"].as_bool().unwrap(),
            buildings: names(&initial["veteran_buildings"]),
            ..Default::default()
        };
        let parent_id = initial["parent_id"].as_str().unwrap();
        if names(&initial["registered_country_ids"])
            .iter()
            .any(|id| id == parent_id)
        {
            seed_parent(&mut reader, initial);
        }
        for step in row["steps"].as_array().unwrap() {
            let input = &step["input"];
            if input["register_parent"].as_bool().unwrap_or(false) {
                seed_parent(&mut reader, initial);
            }
            if let Some(name) = input["parent_name"].as_str() {
                reader.family_mut(RulesTypeFamily::Country)[index].country_parent = name.to_owned();
            }
            if input["register_late"].as_bool().unwrap_or(false) {
                for id in names(&initial["unregistered_building_ids"]) {
                    reader.find_or_construct(RulesTypeFamily::Building, &id);
                }
            }
            if let Some(value) = input["parent_value"].as_str() {
                let parent = reader
                    .find_existing(RulesTypeFamily::Country, parent_id)
                    .unwrap();
                reader.family_mut(RulesTypeFamily::Country)[parent]
                    .country_veterancy
                    .ares
                    .buildings = vec![value.to_owned()];
            }
            if input["read_parent_values"].is_object() {
                let parent = reader
                    .find_existing(RulesTypeFamily::Country, parent_id)
                    .unwrap();
                reader.process_ares_country(
                    parent,
                    &pass(parent_id, &input["read_parent_values"]),
                    false,
                );
            }
            if initial["base_reader_mode"] == "missing_section_suffix" {
                reader.process_house_family(&IniFile::empty(), false);
            } else {
                reader.process_ares_country(index, &pass(country, &input["values"]), false);
            }
            let actual = &reader.families[&RulesTypeFamily::Country][index].country_veterancy;
            let label = format!("{} step{}", row["name"], step["step"]);
            assert_eq!(
                actual.ares.buildings,
                names(&step["veteran_buildings"]),
                "{label}"
            );
            assert_eq!(
                actual.ares.parent_initialized,
                step["parent_initialized"].as_u64().unwrap() != 0,
                "{label}"
            );
            assert_eq!(
                actual.ares.phase as u32,
                step["extension_init_state"].as_u64().unwrap() as u32,
                "{label}"
            );
            if step["original_country_vectors"].is_object() {
                assert_original_lists(actual, &step["original_country_vectors"], &label);
            }
        }
    }
}

#[test]
fn ares_country_constructor_and_extension_stream_keep_retained_state() {
    let native = corpus();
    let default = AresCountryState::default();
    assert_eq!(
        default.phase,
        native["constructor"]["init_state"].as_i64().unwrap() as i32
    );
    assert_eq!(
        default.parent_initialized,
        native["constructor"]["parent_initialized"]
            .as_u64()
            .unwrap()
            != 0
    );
    assert!(default.buildings.is_empty());
    // Native extension_stream preserves list order/duplicates and the separate
    // one-shot byte. Rust's save codec owns its own framing and IDs, not native
    // pointer tokens or OLE swizzle layout.
    for row in native["extension_stream"].as_array().unwrap() {
        // Bijective labels stand in for the supplied native pointer tokens;
        // this checks retained order/duplicates, not native pointer framing.
        let labels = |values: &Value| {
            values
                .as_array()
                .unwrap()
                .iter()
                .map(|token| format!("{:08X}", token.as_u64().unwrap()))
                .collect::<Vec<_>>()
        };
        let state = AresCountryState {
            phase: 2,
            parent_initialized: row["input_flag"].as_u64().unwrap() != 0,
            buildings: labels(&row["pointer_tokens"]),
            ..Default::default()
        };
        let restored: AresCountryState =
            save_codec::deserialize(&save_codec::serialize(&state).unwrap()).unwrap();
        assert_eq!(restored.buildings, labels(&row["loaded_pointer_tokens"]));
        assert_eq!(
            restored.parent_initialized,
            row["loaded_parent_initialized"].as_u64().unwrap() != 0
        );
    }
}

fn continue_process(processed: ProcessedRulesLayers, pass: IniFile) -> ProcessedRulesLayers {
    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    RulesLayerStack::new(pass)
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            trace.into_registry_state_discarding_events(),
        )
        .unwrap()
}

#[test]
fn ares_country_veterancy_reaches_layers_save_hash_validation_and_reset() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[Countries]\n0=PARENT\n1=CHILD\n\
         [PARENT]\nParentCountry=PARENT\nVeteranBuildings=KNOWN\n\
         [CHILD]\nParentCountry=PARENT\n\
         [BuildingTypes]\n0=KNOWN\n1=PRIOR\n[KNOWN]\nStrength=100\n[PRIOR]\nStrength=100\n",
    ))
    .process()
    .unwrap();
    let bytes = save_codec::serialize(&processed).unwrap();
    let restored: ProcessedRulesLayers = save_codec::deserialize(&bytes).unwrap();
    let continued = continue_process(
        restored,
        IniFile::from_str("[PARENT]\nVeteranBuildings=PRIOR\n"),
    );
    let rules = RuleSet::from_processed_rules(&continued).unwrap();
    assert!(rules.country_starts_veteran("CHILD", rules.object("KNOWN").unwrap()));
    assert!(!rules.country_starts_veteran("CHILD", rules.object("PRIOR").unwrap()));
    assert!(rules.country_starts_veteran("PARENT", rules.object("PRIOR").unwrap()));

    let mut altered = serde_json::to_value(&continued).unwrap();
    altered["native_type_construction_trace"]["registry_state"]["families"]["Country"][1]["country_veterancy"]
        ["ares"]["parent_initialized"] = false.into();
    let altered: ProcessedRulesLayers = serde_json::from_value(altered).unwrap();
    let other = RuleSet::from_processed_rules(&altered).unwrap();
    assert_ne!(
        rules.simulation_config_hash(),
        other.simulation_config_hash(),
        "the first-attempt flag affects future reads even when current lists match"
    );
    let retried = continue_process(altered, IniFile::empty());
    let retried = RuleSet::from_processed_rules(&retried).unwrap();
    assert!(retried.country_starts_veteran("CHILD", retried.object("PRIOR").unwrap()));

    let mut bad = serde_json::to_value(&continued).unwrap();
    bad["native_type_construction_trace"]["registry_state"]["families"]["Country"][1]["country_veterancy"]
        ["ares"]["buildings"] = serde_json::json!(["UNREGISTERED"]);
    let bad: ProcessedRulesLayers = serde_json::from_value(bad).unwrap();
    assert!(RuleSet::from_processed_rules(&bad).is_err());
    let (_, trace) = continued.into_ini_and_native_type_construction_trace();
    let reset = RulesLayerStack::new(IniFile::from_str(
        "[Countries]\n0=CHILD\n[BuildingTypes]\n0=KNOWN\n[KNOWN]\nStrength=100\n",
    ))
    .process_with_fixed_art_and_registry_state(
        &IniFile::empty(),
        trace
            .into_registry_state_discarding_events()
            .destructive_reset(),
    )
    .unwrap();
    let reset = RuleSet::from_processed_rules(&reset).unwrap();
    assert!(!reset.country_starts_veteran("CHILD", reset.object("KNOWN").unwrap()));
}
