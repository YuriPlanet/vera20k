use super::*;
use crate::rules::ruleset::RuleSet;
use crate::util::save_codec;

#[test]
fn attack_cursor_rule_retains_layers_reset_save_and_effective_hash() {
    let fresh = RuleSet::from_ini(&IniFile::empty()).unwrap();
    assert!(!fresh.general.attack_cursor_on_disguise);
    let mut layers =
        RulesLayerStack::new(IniFile::from_str("[General]\nAttackCursorOnDisguise=yes\n"));
    layers.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[General]\nAttackCursorOnDisguise=?\n"),
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[General]\nattackcursorondisguise=no\n"),
    );
    let processed = layers.process().unwrap();
    assert!(
        RuleSet::from_processed_rules(&processed)
            .unwrap()
            .general
            .attack_cursor_on_disguise
    );
    let restored: ProcessedRulesLayers =
        save_codec::deserialize(&save_codec::serialize(&processed).unwrap()).unwrap();
    let (_, trace) = restored.into_ini_and_native_type_construction_trace();
    let state = trace
        .into_registry_state_discarding_events()
        .destructive_reset();
    let next = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
        .unwrap();
    let retained = RuleSet::from_processed_rules(&next).unwrap();
    assert!(retained.general.attack_cursor_on_disguise);
    assert_eq!(fresh.source_ini_hash(), retained.source_ini_hash());
    assert_ne!(
        fresh.simulation_config_hash(),
        retained.simulation_config_hash()
    );
}
