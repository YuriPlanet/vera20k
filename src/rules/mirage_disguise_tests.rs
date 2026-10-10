//! Original Mirage constructors and selected ordered Rules readers.
//!
//! The corpus executes TechnoType710AF0, Rules665650, the four ReadBool
//! calls at714404..71446C and the General list/ReadInt block671D3E..671D92.
//! These tests compare those field boundaries through the production Rules
//! processor; they do not claim the complete type reader or disguise lifecycle.

use super::ini_parser::IniFile;
use super::native_processing::{
    NativeTypeConstructorFamily, ProcessedRulesLayers, RulesLayerKind, RulesLayerStack,
};
use super::ruleset::RuleSet;
use serde_json::{Value, json};

fn corpus() -> Value {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/mirage_disguise.json",
    ))
    .expect("original Mirage disguise corpus");
    assert_eq!(native["schema_version"], 1);
    assert_eq!(
        native["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    native
}

pub(super) fn cached_sections(sections: &Value) -> IniFile {
    let mut ini = IniFile::empty();
    for (name, entries) in sections.as_object().unwrap() {
        let section = ini.projection_section_mut(name);
        for (key, value) in entries.as_object().unwrap() {
            // The oracle supplies cached lexical values. Preserve explicit
            // empty entries and exact key spelling at that same boundary.
            section.set(key, value.as_str().unwrap());
        }
    }
    ini
}

fn constructor_layers() -> RulesLayerStack {
    // The native fixture constructs this TechnoType before its selected
    // scalar reads. No TerrainType is preallocated by the reader fixture.
    RulesLayerStack::new(IniFile::from_str("[VehicleTypes]\n0=MGTK\n"))
}

fn append_native_layer(layers: &mut RulesLayerStack, source: &Value) {
    if source["absent"] == true {
        assert_eq!(source["file"], "LANGRULE.INI");
        return;
    }
    let kind = match source["file"].as_str().unwrap() {
        "RULESMD.INI" => RulesLayerKind::RulesMd,
        "LANGRULE.INI" => RulesLayerKind::LangRule,
        "MPBattleMD.ini" => RulesLayerKind::GameMode,
        "XMP03T4.MAP" => RulesLayerKind::Scenario,
        other => panic!("unexpected native Mirage source {other}"),
    };
    layers.push(kind, cached_sections(&source["sections"]));
}

fn selected_layers(native: &Value) -> RulesLayerStack {
    let mut layers = constructor_layers();
    for source in native["retail"]["layers"].as_array().unwrap() {
        append_native_layer(&mut layers, source);
    }
    layers
}

fn assert_flags(rules: &RuleSet, expected: &Value, context: &str) {
    let mgtk = rules.object("MGTK").expect("registered MGTK");
    assert_eq!(
        json!({
            "CanDisguise": u8::from(mgtk.can_disguise),
            "PermaDisguise": u8::from(mgtk.perma_disguise),
            "DetectDisguise": u8::from(mgtk.detect_disguise),
            "DisguiseWhenStill": u8::from(mgtk.disguise_when_still),
        }),
        *expected,
        "{context}"
    );
}

fn assert_general(rules: &RuleSet, expected: &Value, context: &str) {
    assert_eq!(
        json!(rules.general.default_mirage_disguises),
        expected["disguises"],
        "{context}: retained resolved TerrainType slots"
    );
    assert_eq!(
        json!(rules.general.infantry_blink_disguise_time),
        expected["infantry_blink_disguise_time"],
        "{context}: signed reveal duration"
    );
}

fn assert_reader_state(rules: &RuleSet, expected: &Value, context: &str) {
    assert_flags(rules, &expected["type"], context);
    assert_general(rules, &expected["general"], context);
}

fn observe_native_allocations(expected: &Value, allocations: &mut Vec<(String, String)>) {
    let pointers = expected["pointers"].as_array().unwrap();
    let names = expected["disguises"].as_array().unwrap();
    assert_eq!(pointers.len(), names.len(), "native pointer/name receipt");
    for (pointer, name) in pointers.iter().zip(names) {
        let pointer = pointer.as_str().unwrap();
        let name = name.as_str().unwrap();
        if let Some((_, stored_name)) = allocations.iter().find(|(seen, _)| seen == pointer) {
            assert_eq!(stored_name, name, "native retained TerrainType identity");
        } else {
            allocations.push((pointer.to_owned(), name.to_owned()));
        }
    }
}

fn assert_general_pass(
    layers: &RulesLayerStack,
    expected: &Value,
    count: &Value,
    allocations: &mut Vec<(String, String)>,
    context: &str,
) {
    let processed = layers.process().expect("production ordered Rules passes");
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    assert_general(&rules, expected, context);
    assert_eq!(
        json!(processed.default_mirage_disguises()),
        expected["disguises"],
        "{context}: native list owner"
    );
    observe_native_allocations(expected, allocations);
    assert_terrain_identities(&processed, &rules, allocations, count, context);
}

fn assert_terrain_identities(
    processed: &ProcessedRulesLayers,
    rules: &RuleSet,
    allocations: &[(String, String)],
    count: &Value,
    context: &str,
) {
    // In this bounded reader fixture, every Terrain allocation is first
    // observed as a list slot. Native pointer equality records retained
    // identity without equating native addresses with Rust addresses.
    let expected_names: Vec<_> = allocations.iter().map(|(_, name)| name.as_str()).collect();
    let registry = processed
        .ini()
        .section("TerrainTypes")
        .unwrap()
        .registry_ids();
    assert_eq!(
        json!(registry.len()),
        *count,
        "{context}: native registry count"
    );
    assert_eq!(
        registry, expected_names,
        "{context}: allocation order and spelling"
    );
    let constructed: Vec<_> = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .filter(|event| event.family() == NativeTypeConstructorFamily::TerrainType)
        .map(|event| event.native_stored_id())
        .collect();
    assert_eq!(
        constructed, expected_names,
        "{context}: constructor identities"
    );
    assert_eq!(
        rules.terrain_object_types.len(),
        registry.len(),
        "{context}"
    );
    for name in &rules.general.default_mirage_disguises {
        let terrain = rules
            .terrain_object_type_case_insensitive(name)
            .expect("Mirage slot resolves to its allocated TerrainType");
        assert_eq!(terrain.name, *name, "{context}: stored TerrainType name");
    }
}

#[test]
fn disguise_fields_match_original_constructor_defaults() {
    let native = corpus();
    let rules = RuleSet::from_rules_layers(&constructor_layers()).unwrap();
    assert_reader_state(&rules, &native["retail"]["constructor"], "constructors");
}

#[test]
fn disguise_fields_match_original_ordered_retail_reader_layers() {
    let native = corpus();
    let mut layers = constructor_layers();
    for source in native["retail"]["layers"].as_array().unwrap() {
        if source["absent"] == true {
            append_native_layer(&mut layers, source);
            continue;
        }
        let context = source["file"].as_str().unwrap();
        let before = RuleSet::from_rules_layers(&layers).unwrap();
        assert_reader_state(&before, &source["before"], &format!("{context} before"));
        append_native_layer(&mut layers, source);
        let after = RuleSet::from_rules_layers(&layers).unwrap();
        assert_reader_state(&after, &source["after"], context);
    }
}

#[test]
fn disguise_flags_match_original_exact_keys_and_current_value_defaults() {
    let native = corpus();
    let mut layers = selected_layers(&native);
    let controls = native["retail"]["flag_controls"].as_array().unwrap();
    assert!(!controls.is_empty(), "native boolean reader controls");
    for row in controls {
        let context = row["name"].as_str().unwrap();
        let before = RuleSet::from_rules_layers(&layers).unwrap();
        assert_flags(&before, &row["before"], &format!("{context} before"));
        layers.push(RulesLayerKind::Scenario, cached_sections(&row["sections"]));
        let after = RuleSet::from_rules_layers(&layers).unwrap();
        assert_flags(&after, &row["after"], context);
    }
}

#[test]
fn retained_disguise_flags_participate_in_restore_fingerprint() {
    let native = corpus();
    let mut layers = selected_layers(&native);
    let mut continuations = Vec::new();
    for row in native["retail"]["flag_controls"].as_array().unwrap() {
        layers.push(RulesLayerKind::Scenario, cached_sections(&row["sections"]));
        let context = row["name"].as_str().unwrap();
        if !matches!(context, "all_yes" | "all_no") {
            continue;
        }
        let (_, trace) = layers
            .process()
            .unwrap()
            .into_ini_and_native_type_construction_trace();
        let processed = RulesLayerStack::new(IniFile::empty())
            .process_with_fixed_art_and_registry_state(
                &IniFile::empty(),
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_flags(&rules, &row["after"], context);
        continuations.push(rules);
    }
    let [enabled, disabled] = continuations.as_slice() else {
        panic!("native all_yes and all_no flag controls must both be present");
    };
    assert_eq!(enabled.source_ini_hash(), disabled.source_ini_hash());
    assert_ne!(
        enabled.simulation_config_hash(),
        disabled.simulation_config_hash(),
        "identical current inputs with different retained Mirage flags must not restore together"
    );
}

#[test]
fn mirage_list_identity_and_signed_delay_match_original_layer_controls() {
    let native = corpus();
    let mut layers = selected_layers(&native);
    let mut allocations = Vec::new();
    for source in native["retail"]["layers"].as_array().unwrap() {
        if source["absent"] != true {
            observe_native_allocations(&source["after"]["general"], &mut allocations);
        }
    }
    let controls = native["retail"]["general_controls"].as_array().unwrap();
    assert!(!controls.is_empty(), "native General reader controls");
    for row in controls {
        let context = row["name"].as_str().unwrap();
        assert_general_pass(
            &layers,
            &row["before"],
            &row["terrain_count_before"],
            &mut allocations,
            &format!("{context} before"),
        );
        layers.push(RulesLayerKind::Scenario, cached_sections(&row["sections"]));
        assert_general_pass(
            &layers,
            &row["after"],
            &row["terrain_count_after"],
            &mut allocations,
            context,
        );
    }
}

#[test]
fn physical_retail_mgtk_disguise_rules_match_original_root_reader() {
    let Some(raw) = super::retail_ini_fixture::retail_ini_bytes("rulesmd.ini") else {
        return;
    };
    let Some(art) = super::retail_ini_fixture::retail_ini("artmd.ini") else {
        return;
    };
    let native = corpus();
    let root = &native["retail"]["layers"][0];
    assert_eq!(root["file"], "RULESMD.INI");
    assert_eq!(crate::util::sha256::sha256_hex(&raw), root["sha256"]);
    let processed = RulesLayerStack::new(IniFile::from_bytes(&raw).unwrap())
        .process_with_fixed_art(&art)
        .expect("physical retail rules and fixed ART");
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    assert_reader_state(&rules, &root["after"], "physical RULESMD.INI");
}

#[test]
fn physical_retail_mgtk_disguise_rules_match_original_battle_scenario() {
    let Some(retail) = super::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let native = corpus();
    let scenario = native["retail"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["file"] == "XMP03T4.MAP")
        .unwrap();
    assert_reader_state(&retail.rules, &scenario["after"], "physical Battle/Anytown");
}
