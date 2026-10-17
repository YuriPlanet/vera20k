//! Original retained AudioVisual inputs, consumed through their sole process owner.
use super::*;
use crate::rules::native_processing::{
    NativeRulesRegistryState, ProcessedRulesLayers, RulesLayerStack,
    process_native_rules_cold_start,
};
use serde_json::{Value, json};

fn native() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/trigger_map_lighting.json")
}

fn cached_ini(sections: &Value) -> IniFile {
    let mut ini = IniFile::empty();
    for (name, values) in sections.as_object().unwrap() {
        let mut section = crate::rules::ini_parser::IniSection::new(name.clone());
        for (key, value) in values.as_object().unwrap() {
            section.set(key, value.as_str().unwrap());
        }
        ini.replace_first_section(section);
    }
    ini
}

fn state(processed: &ProcessedRulesLayers) -> Value {
    let (rate, step) = processed.ambient_change_inputs();
    let hex = |bits: NativeF64Bits| {
        bits.bits()
            .to_le_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    json!({"rate_f64_le_hex":hex(rate),"step_f64_le_hex":hex(step)})
}

#[test]
fn av_ambient_inputs_follow_native_reads_cold_handoffs_save_and_type_reset() {
    let corpus = &native()["rate_controls"];
    let mut retained = NativeRulesRegistryState::default();
    let first = RulesLayerStack::new(IniFile::empty()).process().unwrap();
    assert_eq!(state(&first), corpus["constructor"]);
    for row in corpus["cases"].as_array().unwrap() {
        let pass = cached_ini(&row["input"]["cache"]);
        retained = process_native_rules_cold_start(retained, &pass, &IniFile::empty(), None)
            .unwrap()
            .into_registry_state_discarding_events();
        // Expose the cold reader's retained state without replaying its text.
        let processed = RulesLayerStack::new(IniFile::empty())
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), retained)
            .unwrap();
        assert_eq!(state(&processed), row["after"], "{}", row["input"]["name"]);
        let saved = bincode::serialize(&processed).unwrap();
        let restored: ProcessedRulesLayers = bincode::deserialize(&saved).unwrap();
        assert_eq!(state(&restored), row["after"]);
        let (_, trace) = restored.into_ini_and_native_type_construction_trace();
        // Rust persistence/reset property; no native arithmetic is generated here.
        let after_reset = RulesLayerStack::new(IniFile::empty())
            .process_with_fixed_art_and_registry_state(
                &IniFile::empty(),
                trace
                    .into_registry_state_discarding_events()
                    .destructive_reset(),
            )
            .unwrap();
        assert_eq!(state(&after_reset), row["after"]);
        let (_, trace) = after_reset.into_ini_and_native_type_construction_trace();
        retained = trace.into_registry_state_discarding_events();
    }
}

#[test]
fn retained_ambient_inputs_change_effective_configuration_identity() {
    let mut identities = Vec::new();
    for step in [".1", ".2"] {
        let first = RulesLayerStack::new(IniFile::from_str(&format!(
            "[AudioVisual]\nAmbientChangeStep={step}\n",
        )))
        .process()
        .unwrap();
        let (_, trace) = first.into_ini_and_native_type_construction_trace();
        let processed = RulesLayerStack::new(IniFile::empty())
            .process_with_fixed_art_and_registry_state(
                &IniFile::empty(),
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        identities.push((rules.source_ini_hash(), rules.simulation_config_hash()));
    }
    assert_eq!(identities[0].0, identities[1].0);
    assert_ne!(identities[0].1, identities[1].1);
}

#[test]
fn malformed_ambient_input_uses_shared_deterministic_parser_policy() {
    // Original malformed float output is uninitialized stack data, not a
    // portable parity golden. Use ini_value's established safe-zero policy.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[AudioVisual]\nAmbientChangeRate=junk\nAmbientChangeStep=junk\n",
    ))
    .unwrap();
    assert!(!rules.general.ambient_change_rate_nonzero);
    assert_eq!(rules.general.ambient_change_interval_frames, 0);
    assert_eq!(rules.general.ambient_change_step, 0);
}
