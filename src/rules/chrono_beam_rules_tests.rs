//! ChronoBeamColor's production AudioVisual reader and retained Rules lifetime.
//! Native constructor/reader scope: procedural_drawing_oracle/chrono_beam.md.
//! These authored cases test reader placement and state handoff; they are not
//! native arithmetic goldens or malformed-stack parity claims.

use super::*;
use crate::rules::ruleset::RuleSet;

#[test]
fn chrono_beam_color_reads_only_the_exact_audio_visual_key_over_black() {
    for (text, expected) in [
        ("", [0; 3]),
        ("[General]\nChronoBeamColor=7,11,19\n", [0; 3]),
        ("[audiovisual]\nChronoBeamColor=7,11,19\n", [0; 3]),
        ("[AudioVisual]\nchronobeamcolor=7,11,19\n", [0; 3]),
        ("[AudioVisual]\nChronoBeamColor=\n", [0; 3]),
        ("[AudioVisual]\nChronoBeamColor=7,11,19\n", [7, 11, 19]),
    ] {
        let rules = RuleSet::from_ini(&IniFile::from_str(text)).unwrap();
        assert_eq!(rules.general.chrono_beam_color, expected, "{text:?}");
    }
}

#[test]
fn chrono_beam_color_obeys_rules_language_mode_and_map_read_order() {
    let mut stack = RulesLayerStack::new(IniFile::from_str(
        "[AudioVisual]\nChronoBeamColor=7,11,19\n",
    ));
    stack.push(
        RulesLayerKind::LangRule,
        IniFile::from_str("[AudioVisual]\nChronoBeamColor=23,29,31\n"),
    );
    assert_eq!(
        RuleSet::from_rules_layers(&stack)
            .unwrap()
            .general
            .chrono_beam_color,
        [23, 29, 31]
    );
    stack.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[AudioVisual]\nChronoBeamColor=37,41,43\n"),
    );
    stack.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[AudioVisual]\nChronoBeamColor=\n"),
    );
    let rules = RuleSet::from_rules_layers(&stack).unwrap();
    assert_eq!(rules.general.chrono_beam_color, [37, 41, 43]);

    stack.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[AudioVisual]\nChronoBeamColor=47,53,59\n"),
    );
    let rules = RuleSet::from_rules_layers(&stack).unwrap();
    assert_eq!(rules.general.chrono_beam_color, [47, 53, 59]);

    stack.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[AudioVisual]\nChronoBeamColor=0,0,0\n"),
    );
    let rules = RuleSet::from_rules_layers(&stack).unwrap();
    assert_eq!(rules.general.chrono_beam_color, [0; 3]);
}

#[test]
fn chrono_beam_color_retains_cold_reads_through_handoff_and_type_reset() {
    let root = IniFile::from_str("[AudioVisual]\nChronoBeamColor=7,11,19\n");
    let language = IniFile::from_str("[AudioVisual]\nChronoBeamColor=23,29,31\n");
    let cold = process_native_rules_cold_start(
        NativeRulesRegistryState::default(),
        &root,
        &IniFile::empty(),
        Some(&language),
    )
    .unwrap();
    let mut state = cold.into_registry_state_discarding_events();
    for (reset, text) in [
        (false, ""),
        (false, "[AudioVisual]\nChronoBeamColor=\n"),
        (true, "[AudioVisual]\nchronobeamcolor=37,41,43\n"),
        (false, "[General]\nChronoBeamColor=47,53,59\n"),
    ] {
        if reset {
            state = state.destructive_reset();
        }
        let processed = RulesLayerStack::new(IniFile::from_str(text))
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
            .unwrap();
        // SavedScenarioResources serializes this same move-only authority.
        // Restore it rather than re-reading the current (missing/empty) text.
        let bytes = bincode::serialize(&processed).unwrap();
        let processed: ProcessedRulesLayers = bincode::deserialize(&bytes).unwrap();
        assert_eq!(processed.chrono_beam_color(), [23, 29, 31]);
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(rules.general.chrono_beam_color, [23, 29, 31]);
        let (_, receipt) = processed.into_ini_and_native_type_construction_trace();
        state = receipt.into_registry_state_discarding_events();
    }
}

#[test]
fn chrono_beam_color_changes_configuration_identity_after_its_source_is_absent() {
    let mut identities = Vec::new();
    for color in ["7,11,19", "23,29,31"] {
        let cold = process_native_rules_cold_start(
            NativeRulesRegistryState::default(),
            &IniFile::from_str(&format!("[AudioVisual]\nChronoBeamColor={color}\n")),
            &IniFile::empty(),
            None,
        )
        .unwrap();
        let processed = RulesLayerStack::new(IniFile::empty())
            .process_with_fixed_art_and_registry_state(
                &IniFile::empty(),
                cold.into_registry_state_discarding_events(),
            )
            .unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        identities.push((rules.source_ini_hash(), rules.simulation_config_hash()));
    }
    assert_eq!(identities[0].0, identities[1].0);
    assert_ne!(identities[0].1, identities[1].1);
}

#[test]
fn physical_chrono_beam_color_uses_the_production_rulesmd_reader() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    assert_eq!(rules.general.chrono_beam_color, [128, 200, 255]);
}

#[test]
fn original_constructor_and_current_color_controls_match_the_rules_owner() {
    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/chrono_beam.json");
    let rgb = |v: &serde_json::Value| -> [u8; 3] {
        std::array::from_fn(|i| v[i].as_u64().unwrap().try_into().unwrap())
    };
    let mut state = NativeRulesRegistryState::default();
    let mut current = RuleSet::from_ini(&IniFile::empty())
        .unwrap()
        .general
        .chrono_beam_color;
    for row in native["color_controls"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        if name == "constructor" {
            assert_eq!(current, rgb(&row["rgb"]), "original Rules665650 default");
            continue;
        }
        assert_eq!(
            current,
            rgb(&row["current_default"]),
            "{name} native default"
        );
        let processed = RulesLayerStack::new(crate::test_fixture::ini_sections(&row["sections"]))
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
            .unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        current = rules.general.chrono_beam_color;
        assert_eq!(current, rgb(&row["rgb"]), "{name} original474B50 result");
        let (_, receipt) = processed.into_ini_and_native_type_construction_trace();
        state = receipt.into_registry_state_discarding_events();
    }
}
