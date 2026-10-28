//! Installed Ares1003EC3C..7C,1002AC23..35 and1003DB30.
use crate::rules::{
    ini_parser::IniFile,
    native_processing::{NativeRulesRegistryState, RulesLayerKind, RulesLayerStack},
    object_type::ObjectCategory,
    ruleset::RuleSet,
};

fn key<'a>(rules: &'a RuleSet, name: &'a str) -> &'a str {
    rules.type_selection_key(ObjectCategory::Infantry, name)
}

#[test]
fn type_selection_group_readers_retain_layers_and_saved_registry() {
    let root = IniFile::from_str(
        "[InfantryTypes]\n0=ALPHA\n1=BETA\n2=RESET\n3=LONG\n\
         [General]\nTypeSelectUseDeploy=no\n\
         [ALPHA]\nGroupAs=COMMON\n[BETA]\nGroupAs=common\n\
         [RESET]\nGroupAs=old\n[LONG]\nGroupAs=ABCDEFGHIJKLMNOPQRSTUVWXYZabcdeTAIL\n",
    );
    let default_rules = RuleSet::from_ini(&IniFile::empty()).unwrap();
    assert!(default_rules.type_select_uses_deploy());
    let mut layers = RulesLayerStack::new(root);
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[General]\nTypeSelectUseDeploy=invalid\n\
             [ALPHA]\nGroupAs=\n[RESET]\nGroupAs=<NoNe>\n",
        ),
    );
    let processed = layers.process().unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    #[cfg(feature = "ares")]
    {
        assert!(!rules.type_select_uses_deploy());
        assert_eq!(key(&rules, "ALPHA"), "COMMON");
        assert_eq!(key(&rules, "BETA"), "common");
        assert_eq!(key(&rules, "LONG"), "ABCDEFGHIJKLMNOPQRSTUVWXYZabcde");
    }
    #[cfg(not(feature = "ares"))]
    {
        assert!(rules.type_select_uses_deploy());
        assert_eq!(key(&rules, "ALPHA"), "ALPHA");
        assert_eq!(key(&rules, "LONG"), "LONG");
    }
    assert_eq!(key(&rules, "RESET"), "RESET");
    let saved = serde_json::to_vec(processed.native_registry_state()).unwrap();
    let restored: NativeRulesRegistryState = serde_json::from_slice(&saved).unwrap();
    let next = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), restored)
        .unwrap();
    let next = RuleSet::from_processed_rules(&next).unwrap();
    assert_eq!(key(&next, "ALPHA"), key(&rules, "ALPHA"));
    assert_eq!(
        next.type_select_uses_deploy(),
        rules.type_select_uses_deploy()
    );
}
