use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{
    NativeRulesRegistryState, ProcessedRulesLayers, RulesLayerStack,
};
use crate::rules::ruleset::RuleSet;
use crate::util::save_codec;

fn values(weights: ThreatCoefficients) -> [f64; 5] {
    [
        weights.my_effectiveness(),
        weights.target_effectiveness(),
        weights.target_special_threat(),
        weights.target_strength(),
        weights.target_distance(),
    ]
}

fn weights(rules: &RuleSet, id: &str) -> [f64; 5] {
    values(rules.object(id).unwrap().threat_coefficients())
}

fn take_state(processed: ProcessedRulesLayers) -> NativeRulesRegistryState {
    processed
        .into_ini_and_native_type_construction_trace()
        .1
        .into_registry_state_discarding_events()
}

#[test]
fn zero_type_weights_wait_for_an_admitted_section_after_restoring_rules() {
    let Some((mut root, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    root.merge(&IniFile::from_str(
        "[E1]\nMyEffectivenessCoefficient=0\nTargetEffectivenessCoefficient=-0\n\
         TargetSpecialThreatCoefficient=0\nTargetStrengthCoefficient=0\nTargetDistanceCoefficient=0\n",
    ));
    let first = RulesLayerStack::new(root)
        .process_with_fixed_art(&art)
        .unwrap();
    let later = RulesLayerStack::new(IniFile::from_str(
        "[General]\nMyEffectivenessCoefficientDefault=11\nTargetEffectivenessCoefficientDefault=12\n\
         TargetSpecialThreatCoefficientDefault=13\nTargetStrengthCoefficientDefault=14\n\
         TargetDistanceCoefficientDefault=15\n[InfantryTypes]\n900=LATEGI\n[LATEGI]\nName=late GI\n",
    )).process_with_fixed_art_and_registry_state(&art, take_state(first)).unwrap();
    let saved: ProcessedRulesLayers =
        save_codec::deserialize(&save_codec::serialize(&later).unwrap()).unwrap();
    let rules = RuleSet::from_processed_rules(&saved).unwrap();
    assert_eq!(
        rules
            .object("E1")
            .unwrap()
            .threat_coefficients()
            .target_effectiveness()
            .to_bits(),
        (-0.0_f64).to_bits(),
    );
    assert_eq!(
        weights(&rules, "E1"),
        [0.0; 5],
        "a missing section does not replace zero weights"
    );
    assert_eq!(weights(&rules, "LATEGI"), [11.0, 12.0, 13.0, 14.0, 15.0]);
    let admitted = RulesLayerStack::new(IniFile::from_str(
        "[E1]\nName=GI\n[GGI]\nName=Guardian GI\n",
    ))
    .process_with_fixed_art_and_registry_state(&art, take_state(saved))
    .unwrap();
    let rules = RuleSet::from_processed_rules(&admitted).unwrap();
    assert_eq!(weights(&rules, "E1"), [11.0, 12.0, 13.0, 14.0, 15.0]);
    assert_eq!(
        weights(&rules, "GGI"),
        [200.0, -200.0, 200.0, -200.0, -10.0]
    );
}

#[test]
fn type_weight_history_survives_save_hashing_and_a_destructive_type_reset() {
    // Original constructors store zero even when General/type sections are absent.
    let ctor = RuleSet::from_ini(&IniFile::from_str("[InfantryTypes]\n0=UNREAD\n")).unwrap();
    assert_eq!(values(ctor.general.threat_coefficient_defaults()), [0.0; 5]);
    assert_eq!(weights(&ctor, "UNREAD"), [0.0; 5]);
    let history = |initial| {
        let first = RulesLayerStack::new(IniFile::from_str(&format!(
            "[General]\nMyEffectivenessCoefficientDefault={initial}\n[InfantryTypes]\n0=E1\n[E1]\nName=GI\n",
        ))).process().unwrap();
        let state: NativeRulesRegistryState =
            save_codec::deserialize(&save_codec::serialize(&take_state(first)).unwrap()).unwrap();
        RulesLayerStack::new(IniFile::from_str(
            "[General]\nMyEffectivenessCoefficientDefault=900\n[E1]\nName=GI\n",
        ))
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
        .unwrap()
    };
    let first = history(101);
    let second = history(202);
    let a = RuleSet::from_processed_rules(&first).unwrap();
    let b = RuleSet::from_processed_rules(&second).unwrap();
    assert_eq!(a.source_ini_hash(), b.source_ini_hash());
    assert_eq!(weights(&a, "E1"), [101.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(weights(&b, "E1"), [202.0, 0.0, 0.0, 0.0, 0.0]);
    assert_ne!(a.simulation_config_hash(), b.simulation_config_hash());
    let reset = RulesLayerStack::new(IniFile::from_str("[InfantryTypes]\n0=E1\n[E1]\nName=GI\n"))
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            take_state(first).destructive_reset(),
        )
        .unwrap();
    let reset = RuleSet::from_processed_rules(&reset).unwrap();
    assert_eq!(weights(&reset, "E1"), [900.0, 0.0, 0.0, 0.0, 0.0]);
}
