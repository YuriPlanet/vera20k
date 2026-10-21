//! Retained Country GUI and parent ordering against executed component controls.
use super::*;
use crate::rules::country_loading::AresCountryLoading;
use crate::util::save_codec;
use serde_json::{Value, json};

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/rules_oracle/ares_country_loading.json")
}

fn loading_from(value: &Value) -> AresCountryLoading {
    serde_json::from_value(json!({
        "file": value["file"], "palette": value["palette"],
        "name": value["name"], "special": value["special"], "brief": value["brief"],
        "color": value["color"].as_u64().unwrap() as u32 as i32,
    }))
    .unwrap()
}

fn assert_loading(loading: &AresCountryLoading, expected: &Value, label: &str) {
    assert_eq!(
        loading.files(),
        (
            expected["file"].as_str().unwrap(),
            expected["palette"].as_str().unwrap()
        ),
        "{label}"
    );
    assert_eq!(
        loading.labels(),
        [
            expected["name"].as_str().unwrap(),
            expected["special"].as_str().unwrap(),
            expected["brief"].as_str().unwrap()
        ],
        "{label}"
    );
    assert_eq!(
        loading.color_index(),
        expected["color"].as_u64().unwrap() as u32 as i32,
        "{label}"
    );
}

#[test]
fn ares_country_loading_defaults_match_native_literal_ids_not_registry_ordinals() {
    let mut reader = RulesPassProcessor::default();
    // Unknown ID occupies index0 and Arabs index7: neither uses stock ordinal.
    let unknown = reader.find_or_construct(RulesTypeFamily::Country, "EXTRA");
    assert_eq!(unknown, 0);
    for control in corpus()["initializers"].as_array().unwrap() {
        let id = control["country_id"].as_str().unwrap();
        let index = reader.find_or_construct(RulesTypeFamily::Country, id);
        let country = &reader.families[&RulesTypeFamily::Country][index];
        assert_loading(
            country.country_veterancy.loading(),
            &control["after_id_defaults"],
            id,
        );
        assert_eq!(country.country_veterancy.ares.phase, 0);
    }
}

#[test]
fn ares_country_loading_retained_primary_parent_and_color_match_native_histories() {
    for history in corpus()["controls"].as_array().unwrap() {
        let label = history["name"].as_str().unwrap();
        let mut reader = RulesPassProcessor::default();
        reader.colors = history["color_registry"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| (id.as_str().unwrap().to_string(), "0,0,0".to_string()))
            .collect();
        let child = reader.find_or_construct(RulesTypeFamily::Country, "CHILD_A");
        let parent = reader.find_or_construct(RulesTypeFamily::Country, "PARENT_A");
        let prior = &history["declared_initial"];
        let member = &mut reader.family_mut(RulesTypeFamily::Country)[child];
        member.country_side_index = history["country_side"].as_i64().unwrap() as i32;
        member.country_parent = history["parent_country"].as_str().unwrap().into();
        member.country_veterancy.ares = AresCountryState {
            phase: prior["phase"].as_u64().unwrap() as u32 as i32,
            parent_initialized: prior["parent_initialized"] != 0,
            loading: loading_from(prior),
            ..Default::default()
        };
        reader.family_mut(RulesTypeFamily::Country)[parent]
            .country_veterancy
            .ares
            .loading = loading_from(&history["declared_parent"]);
        for row in history["rows"].as_array().unwrap() {
            let mut section = IniSection::new("CHILD_A".into());
            for (key, value) in row["input"]["values"].as_object().unwrap() {
                if let Some(value) = value.as_str() {
                    section.set(key, value);
                }
            }
            let mut pass = IniFile::empty();
            pass.replace_first_section(section);
            reader.process_ares_country(child, &pass, row["input"]["primary"].as_bool().unwrap());
            let state = &reader.families[&RulesTypeFamily::Country][child]
                .country_veterancy
                .ares;
            assert_loading(&state.loading, &row["after"], label);
            assert_eq!(
                state.phase,
                row["after"]["phase"].as_u64().unwrap() as u32 as i32,
                "{label}"
            );
            assert_eq!(
                state.parent_initialized,
                row["after"]["parent_initialized"] != 0,
                "{label}"
            );
        }
    }
}

#[test]
fn ares_country_loading_production_passes_and_save_keep_gui_without_sim_hash_changes() {
    let root = IniFile::from_str(
        "[Countries]\n0=PARENT\n1=CHILD\n[Colors]\nFirst=0,0,128\nSecond=120,255,255\n[PARENT]\nFile.LoadScreen=parent.shp\nLoadScreenText.Name=Name:Parent\n[CHILD]\nParentCountry=PARENT\nFile.LoadScreen=child.shp\nLoadScreenText.Name=Name:Child\n",
    );
    let map = IniFile::from_str(
        "[CHILD]\nFile.LoadScreen=ignored-map.shp\nLoadScreenText.Name=Ignored\nLoadScreenText.Color=Second\n",
    );
    let mut layers = RulesLayerStack::new(root.clone());
    layers.push(RulesLayerKind::Scenario, map);
    let processed = layers.process().unwrap();
    let state = processed.native_registry_state();
    let child = state.families[&RulesTypeFamily::Country][1]
        .country_veterancy
        .loading();
    assert_eq!(child.files().0, "parent.shp"); // First primary GUI precedes inheritance.
    assert_eq!(child.labels()[0], "Name:Parent");
    assert_eq!(child.color_index(), 2); // Map's common color read still applies.
    let before_hash = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        state.families[&RulesTypeFamily::Country][1]
            .country_veterancy
            .hash(&mut h);
        h.finish()
    };
    let bytes = save_codec::serialize(state).unwrap();
    let restored: NativeRulesRegistryState = save_codec::deserialize(&bytes).unwrap();
    let mut reader = RulesPassProcessor::with_registry_state(restored);
    reader.process_house_family(&root, true);
    let country = &reader.families[&RulesTypeFamily::Country][1].country_veterancy;
    assert_eq!(country.loading().files().0, "child.shp"); // Consumed parent flag survives save.
    assert_eq!(country.loading().labels()[0], "Name:Child");
    let mut h = std::collections::hash_map::DefaultHasher::new();
    country.hash(&mut h);
    assert_eq!(
        h.finish(),
        before_hash,
        "GUI changes cannot affect simulation CRC"
    );
}

#[test]
fn ares_country_loading_filename_format_matches_native_sprintf() {
    for control in corpus()["filename_formats"].as_array().unwrap() {
        let mut ini = IniFile::empty();
        let mut section = IniSection::new("CONTROL".into());
        section.set("File.LoadScreen", control["template"].as_str().unwrap());
        ini.replace_first_section(section);
        let mut loading = AresCountryLoading::for_country_id("CONTROL");
        loading.read_primary(ini.section("CONTROL").unwrap());
        assert_eq!(
            loading
                .background_file(control["width"] == "640")
                .as_deref(),
            control["output"].as_str()
        );
    }
}
