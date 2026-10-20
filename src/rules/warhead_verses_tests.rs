//! Warhead ReadINI75DDCC uses a literal all-100% default on each admitted
//! section. Expected tables come from the executed original reader controls in
//! tools/rules_oracle/bridge_landing_inputs.{py,json,meta.json}.

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::native_processing::{ProcessedRulesLayers, RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;
use crate::util::save_codec;
use serde_json::Value;

fn native() -> &'static Value {
    crate::test_fixture::json("tools/rules_oracle/bridge_landing_inputs.json")
}

fn discovery() -> IniFile {
    IniFile::from_str("[Warheads]\n0=HE\n")
}

fn control_pass(sections: &Value) -> IniFile {
    let mut text = String::new();
    for (name, keys) in sections.as_object().unwrap() {
        text.push_str(&format!("[{name}]\n"));
        for (key, value) in keys.as_object().unwrap() {
            text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
    }
    IniFile::from_str(&text)
}

fn first_control() -> RulesLayerStack {
    let mut layers = RulesLayerStack::new(discovery());
    layers.push(
        RulesLayerKind::Scenario,
        control_pass(&native()["controls"][0]["input"]),
    );
    layers
}

fn table(rules: &RuleSet) -> [u64; 11] {
    rules.warhead("HE").unwrap().verses_f64.map(f64::to_bits)
}

fn golden(state: &Value) -> [u64; 11] {
    std::array::from_fn(|index| {
        u64::from_str_radix(state["verses_bits"][index].as_str().unwrap(), 16).unwrap()
    })
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
fn native_tables_follow_each_admitted_warhead_layer() {
    let mut layers = RulesLayerStack::new(discovery());
    assert_eq!(
        table(&RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap()),
        golden(&native()["constructor"]["warheads"]["HE"])
    );
    for (index, row) in native()["controls"].as_array().unwrap().iter().enumerate() {
        layers.push(RulesLayerKind::Scenario, control_pass(&row["input"]));
        let rules = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
        assert_eq!(
            table(&rules),
            golden(&row["warheads"]["HE"]),
            "pass {index}"
        );
    }
}

#[test]
fn absent_section_keeps_the_table_but_present_missing_key_resets_it() {
    let before = golden(&native()["controls"][0]["warheads"]["HE"]);
    let reset = golden(&native()["controls"][1]["warheads"]["HE"]);
    for (patch, expected) in [
        ("[Other]\nWall=no\n", before),
        ("[HE]\n; no physical entries\n", before),
        ("[he]\nWall=no\n", before),
        ("[HE]\nWall=no\n", reset),
        ("[HE]\nVerses=\nWall=no\n", reset),
        ("[HE]\nverses=25%\nWall=no\n", reset),
    ] {
        let mut layers = first_control();
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(patch));
        let rules = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
        assert_eq!(table(&rules), expected, "{patch}");
    }
}

#[test]
fn cached_empty_read_preserves_the_table_instead_of_using_the_absent_key_default() {
    // The physical loader drops empty values. A supplied INI cache can still
    // contain one: ReadString528A10 returns zero, so ReadINI75DDCC..75DE5A
    // skips its parse rather than using the missing-key default string.
    let processed = first_control().process().unwrap();
    let mut section = IniSection::new("HE".into());
    section.set("Verses", "");
    section.set("Wall", "no");
    let mut patch = IniFile::empty();
    patch.replace_first_section(section);
    let continued = continue_process(processed, patch);
    assert_eq!(
        table(&RuleSet::from_processed_rules(&continued).unwrap()),
        golden(&native()["controls"][0]["warheads"]["HE"])
    );
}

#[test]
fn table_survives_save_and_ordinary_handoff_but_destructive_reset_uses_constructor() {
    let mut layers = first_control();
    layers.push(
        RulesLayerKind::Scenario,
        control_pass(&native()["controls"][1]["input"]),
    );
    let processed = layers.process().unwrap();
    let expected = golden(&native()["controls"][1]["warheads"]["HE"]);
    let before = RuleSet::from_processed_rules(&processed).unwrap();
    let bytes = save_codec::serialize(&processed).unwrap();
    let restored: ProcessedRulesLayers = save_codec::deserialize(&bytes).unwrap();
    let restored_rules = RuleSet::from_processed_rules(&restored).unwrap();
    assert_eq!(table(&restored_rules), expected);
    assert_eq!(
        before.simulation_config_hash(),
        restored_rules.simulation_config_hash()
    );
    let continued = continue_process(restored, IniFile::empty());
    assert_eq!(
        table(&RuleSet::from_processed_rules(&continued).unwrap()),
        expected
    );

    // Start this reset from the non-default native table, so retaining a
    // pre-reset table cannot accidentally satisfy the constructor assertion.
    let before_reset = first_control().process().unwrap();
    let (_, trace) = before_reset.into_ini_and_native_type_construction_trace();
    let reset = RulesLayerStack::new(discovery())
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            trace
                .into_registry_state_discarding_events()
                .destructive_reset(),
        )
        .unwrap();
    assert_eq!(
        table(&RuleSet::from_processed_rules(&reset).unwrap()),
        golden(&native()["constructor"]["warheads"]["HE"])
    );
}

#[test]
fn retained_table_changes_config_identity_for_the_same_current_source() {
    let first = first_control().process().unwrap();
    let first = continue_process(first, IniFile::empty());
    let first_rules = RuleSet::from_processed_rules(&first).unwrap();

    let mut layers = first_control();
    layers.push(
        RulesLayerKind::Scenario,
        control_pass(&native()["controls"][1]["input"]),
    );
    let second = continue_process(layers.process().unwrap(), IniFile::empty());
    let second_rules = RuleSet::from_processed_rules(&second).unwrap();
    assert_eq!(
        first_rules.source_ini_hash(),
        second_rules.source_ini_hash()
    );
    assert_ne!(table(&first_rules), table(&second_rules));
    assert_ne!(
        first_rules.simulation_config_hash(),
        second_rules.simulation_config_hash()
    );
}

#[test]
fn retail_he_uses_the_native_table_then_the_admitted_map_default() {
    let Some((root, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let mut layers = RulesLayerStack::new(root);
    let processed = layers.process_with_fixed_art(&art).unwrap();
    assert_eq!(
        table(&RuleSet::from_processed_rules(&processed).unwrap()),
        golden(&native()["layers"][0]["warheads"]["HE"])
    );
    layers.push(
        RulesLayerKind::Scenario,
        control_pass(&native()["controls"][1]["input"]),
    );
    let processed = layers.process_with_fixed_art(&art).unwrap();
    assert_eq!(
        table(&RuleSet::from_processed_rules(&processed).unwrap()),
        golden(&native()["controls"][1]["warheads"]["HE"])
    );
}
