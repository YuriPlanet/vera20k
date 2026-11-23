use super::ini_parser::IniFile;
use super::native_processing::{NativeRulesRegistryState, RulesLayerKind, RulesLayerStack};
use super::ruleset::{GeneralRules, RuleSet};
use crate::util::save_codec;

/// RulesCtor66776F and ReadGeneral66EB03..66EB1C: false at construction,
/// current-default exact-key reads, and Rules lifetime across Type resets.
#[test]
fn shrouded_subterannean_moves_retains_layer_history_through_save_and_type_reset() {
    assert!(!GeneralRules::default().allow_shrouded_subterannean_moves());
    let Some(retail) = super::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    assert!(
        RuleSet::from_ini(&retail)
            .unwrap()
            .general
            .allow_shrouded_subterannean_moves()
    );

    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[General]\nAllowShroudedSubteranneanMoves=yes\n",
    ));
    layers.push(
        RulesLayerKind::LangRule,
        IniFile::from_str("[General]\nAllowShroudedSubteranneanMoves=no\n"),
    );
    assert!(
        !RuleSet::from_rules_layers(&layers)
            .unwrap()
            .general
            .allow_shrouded_subterannean_moves()
    );
    layers.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[General]\nAllowShroudedSubteranneanMoves=yes\n"),
    );
    let invalid = IniFile::from_str(
        "[General]\nAllowShroudedSubteranneanMoves=junk\n\
         allowshroudedsubteranneanmoves=no\n\
         [AudioVisual]\nAllowShroudedSubteranneanMoves=no\n",
    );
    layers.push(RulesLayerKind::Scenario, invalid.clone());
    let processed = layers.process().unwrap();
    assert!(
        RuleSet::from_processed_rules(&processed)
            .unwrap()
            .general
            .allow_shrouded_subterannean_moves()
    );
    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    let registry = trace.into_registry_state_discarding_events();
    let restored: NativeRulesRegistryState =
        save_codec::deserialize(&save_codec::serialize(&registry).unwrap()).unwrap();
    let next = RulesLayerStack::new(invalid.clone())
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), restored.destructive_reset())
        .unwrap();
    let retained = RuleSet::from_processed_rules(&next).unwrap();
    let cold = RuleSet::from_ini(&invalid).unwrap();
    assert!(retained.general.allow_shrouded_subterannean_moves());
    assert!(!cold.general.allow_shrouded_subterannean_moves());
    assert_eq!(retained.source_ini_hash(), cold.source_ini_hash());
    assert_ne!(
        retained.simulation_config_hash(),
        cold.simulation_config_hash()
    );
}
