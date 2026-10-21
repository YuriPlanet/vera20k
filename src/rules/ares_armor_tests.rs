//! Native inputs/results are consumed at the fixture boundary; the production
//! RulesLayerStack owns all registrations, binding, layering and projections.

use crate::rules::armor::{ArmorId, armor_index};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;

#[cfg(feature = "ares")]
use super::*;
#[cfg(feature = "ares")]
use crate::rules::native_processing::ProcessedRulesLayers;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
#[cfg(feature = "ares")]
use crate::util::save_codec;
#[cfg(feature = "ares")]
use serde_json::Value;

#[cfg(feature = "ares")]
fn native() -> &'static Value {
    crate::test_fixture::json("tools/rules_oracle/ares_armor.json")
}

#[cfg(feature = "ares")]
fn row(name: &str) -> &'static Value {
    native()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("native armor control {name}"))
}

#[cfg(feature = "ares")]
fn assert_versus(actual: Versus, expected: &Value) {
    assert_eq!(
        actual.multiplier.bits(),
        u64::from_str_radix(expected["multiplier_bits"].as_str().unwrap(), 16).unwrap()
    );
    assert_eq!(actual.force_fire, expected["force_fire"].as_bool().unwrap());
    assert_eq!(actual.retaliate, expected["retaliate"].as_bool().unwrap());
    assert_eq!(
        actual.passive_acquire,
        expected["passive_acquire"].as_bool().unwrap()
    );
}

#[cfg(feature = "ares")]
fn native_pass(control: &Value) -> IniFile {
    let mut ini = IniFile::empty();
    for event in control["events"].as_array().unwrap() {
        if event["service"] != "supplied_ReadString" && event["service"] != "supplied_ReadBool" {
            continue;
        }
        let Some(value) = event["value"].as_str() else {
            continue;
        };
        let name = event["section"].as_str().unwrap();
        let mut section = ini
            .section(name)
            .cloned()
            .unwrap_or_else(|| IniSection::new(name.into()));
        section.set(event["key"].as_str().unwrap(), value);
        ini.replace_first_section(section);
    }
    ini
}

#[cfg(feature = "ares")]
fn inherited_layers() -> RulesLayerStack {
    let mut root = native_pass(row("warhead_inline_inheritance"));
    root.merge(&IniFile::from_str(
        "[Warheads]\n0=TestWH\n[VehicleTypes]\n0=TARGET\n[TARGET]\nArmor=paper\n",
    ));
    RulesLayerStack::new(root)
}

#[cfg(feature = "ares")]
fn compare_wh(processed: &ProcessedRulesLayers, expected: &Value) {
    let rules = RuleSet::from_processed_rules(processed).unwrap();
    let catalog = processed.ares_armors().unwrap();
    let state = rules.warhead("TestWH").unwrap();
    for (name, expected) in expected.as_object().unwrap() {
        let armor = catalog.find(name).unwrap();
        let actual = state.ares_verses_hash_state().unwrap().entries[armor.index()];
        assert_versus(actual, expected);
        assert_eq!(state.interaction(armor).multiplier(), actual.multiplier);
    }
}

#[cfg(feature = "ares")]
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
fn armor_domain_preserves_case_insensitive_native_lookup_without_implicit_trim() {
    assert_eq!(armor_index("NoNe"), ArmorId::NONE);
    assert_eq!(armor_index("WOOD"), ArmorId::WOOD);
    assert_eq!(armor_index("unknown"), ArmorId::NONE);
    assert_eq!(armor_index(" heavy "), ArmorId::NONE);
}

#[cfg(feature = "ares")]
#[test]
fn token_numbers_and_independent_flags_match_executed_ares_parser() {
    let mut count = 0;
    for control in native()["cases"].as_array().unwrap() {
        if !control["name"].as_str().unwrap().starts_with("parse_") {
            continue;
        }
        let token = control["fixture"]["token"].as_str().unwrap();
        assert_versus(Versus::parse(token), &control["result"]);
        count += 1;
    }
    assert_eq!(count, 19);
}

#[cfg(feature = "ares")]
#[test]
fn can_fire_keeps_the_native_unordered_comparison_when_force_fire_is_false() {
    let control = row("can_fire_nan_false");
    let fixture = &control["fixture"];
    let interaction = ArmorInteraction::ares(
        NativeF64Bits::from_bits(
            u64::from_str_radix(fixture["multiplier_bits"].as_str().unwrap(), 16).unwrap(),
        ),
        fixture["force_fire"].as_bool().unwrap(),
        false,
        false,
    );
    assert_eq!(
        interaction.can_fire(),
        control["accepted"].as_bool().unwrap()
    );
}

#[cfg(feature = "ares")]
#[test]
fn whole_record_inheritance_observes_earlier_overrides_and_later_reads_keep_children() {
    let mut layers = inherited_layers();
    let first = layers.process().unwrap();
    compare_wh(&first, &row("warhead_inline_inheritance")["result"]);
    layers.push(
        RulesLayerKind::Scenario,
        native_pass(row("warhead_reread_retains_custom")),
    );
    compare_wh(
        &layers.process().unwrap(),
        &row("warhead_reread_retains_custom")["result"],
    );
}

#[cfg(feature = "ares")]
#[test]
fn registered_defaults_match_native_indices_and_numeric_fallback_records() {
    let processed = inherited_layers().process().unwrap();
    let catalog = processed.ares_armors().unwrap();
    for name in ["paper", "later", "magic"] {
        let expected = row(&format!("registry_{name}"));
        let id = catalog.find(name).unwrap();
        let definition = &catalog.armors[id.index()];
        assert_eq!(
            definition
                .default_index
                .map(|id| id.index() as i64)
                .unwrap_or(-1),
            expected["default_index"].as_i64().unwrap()
        );
        assert_versus(definition.default_verses, &expected["default_verses"]);
    }
}

#[cfg(feature = "ares")]
#[test]
fn unknown_armor_is_bound_once_until_the_object_explicitly_rereads_it() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=TARGET\n[TerrainTypes]\n0=TREE\n\
         [TARGET]\nArmor=future\n[TREE]\nArmor=future\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[ArmorTypes]\nfuture=steel\n[TARGET]\nStrength=100\n[TREE]\nStrength=100\n",
        ),
    );
    let rules = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
    assert_eq!(rules.object("TARGET").unwrap().armor_id(), ArmorId::NONE);
    assert_eq!(
        rules
            .terrain_object_type_case_insensitive("TREE")
            .unwrap()
            .armor_id(),
        ArmorId::NONE
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[TARGET]\nArmor=FUTURE\n[TREE]\nArmor=FUTURE\n"),
    );
    let processed = layers.process().unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    let armor = processed.ares_armors().unwrap().find("future").unwrap();
    assert_eq!(rules.object("TARGET").unwrap().armor_id(), armor);
    assert_eq!(
        rules
            .terrain_object_type_case_insensitive("TREE")
            .unwrap()
            .armor_id(),
        armor
    );
}

#[cfg(feature = "ares")]
#[test]
fn retained_binding_changes_hash_even_when_current_source_and_catalog_match() {
    let mut unknown_first = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=TARGET\n[TARGET]\nArmor=future\n",
    ));
    unknown_first.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[ArmorTypes]\nfuture=steel\n[TARGET]\nStrength=100\n"),
    );
    let unknown = continue_process(unknown_first.process().unwrap(), IniFile::empty());
    let mut known_first = RulesLayerStack::new(IniFile::from_str(
        "[ArmorTypes]\nfuture=steel\n[VehicleTypes]\n0=TARGET\n[TARGET]\nArmor=future\n",
    ));
    known_first.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[TARGET]\nStrength=100\n"),
    );
    let known = continue_process(known_first.process().unwrap(), IniFile::empty());
    let unknown = RuleSet::from_processed_rules(&unknown).unwrap();
    let known = RuleSet::from_processed_rules(&known).unwrap();
    assert_eq!(unknown.source_ini_hash(), known.source_ini_hash());
    assert_ne!(
        unknown.object("TARGET").unwrap().armor_id(),
        known.object("TARGET").unwrap().armor_id()
    );
    assert_ne!(
        unknown.simulation_config_hash(),
        known.simulation_config_hash()
    );
}

#[cfg(feature = "ares")]
#[test]
fn armor_id_above_255_reaches_the_same_warhead_record_after_saved_roundtrip() {
    let mut text = String::from("[ArmorTypes]\n");
    for index in 0..260 {
        text.push_str(&format!("custom{index}=steel\n"));
    }
    text.push_str(
        "[VehicleTypes]\n0=TARGET\n[TARGET]\nArmor=custom259\n[Warheads]\n0=TestWH\n\
                   [TestWH]\nVersus.custom259=25%\n",
    );
    let processed = RulesLayerStack::new(IniFile::from_str(&text))
        .process()
        .unwrap();
    let bytes = save_codec::serialize(&processed).unwrap();
    let restored: ProcessedRulesLayers = save_codec::deserialize(&bytes).unwrap();
    let rules = RuleSet::from_processed_rules(&restored).unwrap();
    let armor = rules.object("TARGET").unwrap().armor_id();
    assert!(armor.index() > usize::from(u8::MAX));
    assert_eq!(
        rules
            .warhead("TestWH")
            .unwrap()
            .interaction(armor)
            .multiplier()
            .bits(),
        u64::from_str_radix(
            row("warhead_inline_inheritance")["result"]["steel"]["multiplier_bits"]
                .as_str()
                .unwrap(),
            16,
        )
        .unwrap()
    );
    assert_eq!(
        rules.simulation_config_hash(),
        RuleSet::from_processed_rules(&processed)
            .unwrap()
            .simulation_config_hash()
    );
}

#[cfg(feature = "ares")]
#[test]
fn saved_handoff_retains_extension_history_but_scenario_reset_rebuilds_it() {
    let mut layers = inherited_layers();
    layers.push(
        RulesLayerKind::Scenario,
        native_pass(row("warhead_reread_retains_custom")),
    );
    let processed = layers.process().unwrap();
    let original_hash = RuleSet::from_processed_rules(&processed)
        .unwrap()
        .simulation_config_hash();
    let bytes = save_codec::serialize(&processed).unwrap();
    let restored: ProcessedRulesLayers = save_codec::deserialize(&bytes).unwrap();
    assert_eq!(
        RuleSet::from_processed_rules(&restored)
            .unwrap()
            .simulation_config_hash(),
        original_hash
    );
    let continued = continue_process(restored, IniFile::empty());
    compare_wh(&continued, &row("warhead_reread_retains_custom")["result"]);
    let (_, trace) = continued.into_ini_and_native_type_construction_trace();
    let reset = RulesLayerStack::new(IniFile::from_str("[Warheads]\n0=TestWH\n"))
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            trace
                .into_registry_state_discarding_events()
                .destructive_reset(),
        )
        .unwrap();
    assert_eq!(reset.ares_armors().unwrap().len(), 11);
    let rules = RuleSet::from_processed_rules(&reset).unwrap();
    assert_eq!(
        rules
            .warhead("TestWH")
            .unwrap()
            .interaction(ArmorId::NONE)
            .multiplier(),
        NativeF64Bits::ONE
    );
}

#[cfg(feature = "ares")]
#[test]
fn effective_extension_records_change_hash_for_identical_current_source() {
    let first = continue_process(inherited_layers().process().unwrap(), IniFile::empty());
    let mut later = inherited_layers();
    later.push(
        RulesLayerKind::Scenario,
        native_pass(row("warhead_reread_retains_custom")),
    );
    let second = continue_process(later.process().unwrap(), IniFile::empty());
    let first = RuleSet::from_processed_rules(&first).unwrap();
    let second = RuleSet::from_processed_rules(&second).unwrap();
    assert_eq!(first.source_ini_hash(), second.source_ini_hash());
    assert_ne!(
        first.simulation_config_hash(),
        second.simulation_config_hash()
    );
}

#[cfg(feature = "ares")]
#[test]
fn raw11_stays_independent_and_ares_disables_the_infantry_mask() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Warheads]\n0=TestWH\n[TestWH]\nVerses=50%,50%,50%,50%,0%,50%,0%,50%,50%,50%,50%\n",
    ))
    .unwrap();
    let warhead = rules.warhead("TestWH").unwrap();
    assert_eq!(
        warhead.verses_f64.map(f64::to_bits),
        [1.0_f64.to_bits(); 11]
    );
    assert!(!warhead.uses_medium_wood_target_mask());
    assert!(!warhead.interaction(armor_index("medium")).can_fire());
    assert!(!warhead.interaction(ArmorId::WOOD).can_fire());
}

#[cfg(feature = "ares")]
#[test]
fn malformed_saved_catalog_bound_indices_and_record_vectors_are_rejected() {
    let original = serde_json::to_value(inherited_layers().process().unwrap()).unwrap();
    for defect in [
        "catalog_index",
        "object_index",
        "warhead_vector",
        "missing_extension",
    ] {
        let mut saved = original.clone();
        let registry = &mut saved["native_type_construction_trace"]["registry_state"];
        match defect {
            "catalog_index" => {
                registry["ares_armors"]["armors"][11]["default_index"] = u32::MAX.into()
            }
            "object_index" => registry["families"]["Vehicle"][0]["armor_id"] = u32::MAX.into(),
            "warhead_vector" => {
                registry["families"]["Warhead"][0]["ares_verses"]["entries"] = serde_json::json!([])
            }
            "missing_extension" => registry["families"]["Warhead"][0]["ares_verses"] = Value::Null,
            _ => unreachable!(),
        }
        let restored: ProcessedRulesLayers = serde_json::from_value(saved).unwrap();
        assert!(
            RuleSet::from_processed_rules(&restored).is_err(),
            "{defect}"
        );
    }
}

#[cfg(not(feature = "ares"))]
#[test]
fn disabled_feature_ignores_custom_catalog_and_preserves_native_numeric_gates() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[ArmorTypes]\npaper=steel\n[VehicleTypes]\n0=TARGET\n[TARGET]\nArmor=paper\n\
         [Warheads]\n0=TestWH\n[TestWH]\nVersus.none=0%\n\
         Verses=1%,2%,0%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .unwrap();
    assert_eq!(rules.object("TARGET").unwrap().armor_id(), ArmorId::NONE);
    let wh = rules.warhead("TestWH").unwrap();
    assert!(wh.interaction(ArmorId::NONE).can_retaliate());
    assert!(!wh.interaction(ArmorId::NONE).can_passively_acquire());
    assert!(wh.interaction(armor_index("flak")).can_passively_acquire());
    assert!(!wh.interaction(armor_index("plate")).can_fire());
}

#[cfg(not(feature = "ares"))]
#[test]
fn retail_layered_object_binding_retains_live_armor_and_terrain_constructor_default() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=TARGET\n[TerrainTypes]\n0=TREE\n[TARGET]\n[TREE]\n",
    ));
    for (patch, vehicle, terrain) in [
        (None, ArmorId::NONE, ArmorId::WOOD),
        (
            Some("Armor=heavy"),
            armor_index("heavy"),
            armor_index("heavy"),
        ),
        (
            Some("Strength=100"),
            armor_index("heavy"),
            armor_index("heavy"),
        ),
        (Some("Armor="), armor_index("heavy"), armor_index("heavy")),
        (Some("Armor=unknown"), ArmorId::NONE, ArmorId::NONE),
        (Some("Strength=200"), ArmorId::NONE, ArmorId::NONE),
    ] {
        if let Some(patch) = patch {
            layers.push(
                RulesLayerKind::Scenario,
                IniFile::from_str(&format!("[TARGET]\n{patch}\n[TREE]\n{patch}\n")),
            );
        }
        let rules = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
        assert_eq!(
            rules.object("TARGET").unwrap().armor_id(),
            vehicle,
            "{patch:?}"
        );
        assert_eq!(
            rules
                .terrain_object_type_case_insensitive("TREE")
                .unwrap()
                .armor_id(),
            terrain,
            "{patch:?}"
        );
    }
}
