//! LAN/WOL startup rule inputs through the canonical Rules/INI readers.
//! Original Rules665650 +17E3 and ReadAI673CFD..673D1D establish the
//! CompEasyBonus default, retained ReadBool and exact [AI] key. Physical
//! RULESMD, optional LANGRULE, mode and map are scenario passes, not AIMD.

use super::{GeneralRules, RuleSet};
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};

#[test]
fn comp_easy_bonus_uses_constructor_true_and_exact_ai_key_without_general_gate() {
    assert!(GeneralRules::default().comp_easy_bonus);
    for text in [
        "",
        "[General]\nCompEasyBonus=no\n",
        "[AI]\ncompeasybonus=no\n",
        "[ai]\nCompEasyBonus=no\n",
        "[AI]\nCompEasyBonus=\n",
        "[AI]\nCompEasyBonus=junk\n",
    ] {
        assert!(
            RuleSet::from_ini(&IniFile::from_str(text))
                .unwrap()
                .general
                .comp_easy_bonus,
            "{text:?}"
        );
    }
    for (raw, expected) in [
        ("no", false),
        ("Yes", true),
        ("0", false),
        ("2", true),
        ("Nope", false),
    ] {
        let ini = IniFile::from_str(&format!("[AI]\nCompEasyBonus={raw}\n"));
        assert_eq!(
            RuleSet::from_ini(&ini).unwrap().general.comp_easy_bonus,
            expected,
            "{raw}"
        );
    }
}

#[test]
fn comp_easy_bonus_retains_prior_valid_value_across_rules_language_mode_and_map() {
    let mut layers = RulesLayerStack::new(IniFile::from_str("[AI]\nCompEasyBonus=no\n"));
    for (kind, text, expected) in [
        (RulesLayerKind::LangRule, "[AI]\nCompEasyBonus=Yes\n", true),
        (RulesLayerKind::GameMode, "[AI]\nCompEasyBonus=no\n", false),
        (RulesLayerKind::Scenario, "[AI]\nCompEasyBonus=2\n", false),
        (
            RulesLayerKind::Scenario,
            "[AI]\nCompEasyBonus=junk\n",
            false,
        ),
        (RulesLayerKind::Scenario, "[AI]\nCompEasyBonus=\n", false),
        (
            RulesLayerKind::Scenario,
            "[General]\nCompEasyBonus=yes\n",
            false,
        ),
        (RulesLayerKind::Scenario, "[AI]\nCompEasyBonus=1\n", true),
    ] {
        layers.push(kind, IniFile::from_str(text));
        let processed = layers.process().unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(
            rules.general.comp_easy_bonus, expected,
            "{kind:?}: {text:?}"
        );
    }
}

#[test]
fn retail_comp_easy_bonus_is_off_for_battle_and_free_for_all() {
    let Some(root) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    assert!(!RuleSet::from_ini(&root).unwrap().general.comp_easy_bonus);
    // Production fixtures apply RULESMD, optional LANGRULE, selected mode,
    // then the physical map through the retained native Rules owner.
    for mode in [1, 2] {
        let Some(loaded) =
            crate::rules::retail_ini_fixture::retail_mode_rules_for_map(mode, "Hills.mmx")
        else {
            return;
        };
        assert!(!loaded.rules.general.comp_easy_bonus, "mode={mode}");
    }
}

#[test]
fn comp_easy_bonus_survives_native_type_reset_and_saved_registry_without_new_authored_key() {
    use crate::util::save_codec;
    let processed = RulesLayerStack::new(IniFile::from_str("[AI]\nCompEasyBonus=no\n"))
        .process()
        .unwrap();
    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    let registry = trace.into_registry_state_discarding_events();
    let registry: crate::rules::native_processing::NativeRulesRegistryState =
        save_codec::deserialize(&save_codec::serialize(&registry).unwrap()).unwrap();
    let next = RulesLayerStack::new(IniFile::from_str("[General]\nName=Next scene\n"))
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), registry.destructive_reset())
        .unwrap();
    let retained = RuleSet::from_processed_rules(&next).unwrap();
    let cold = RuleSet::from_ini(&IniFile::from_str("[General]\nName=Next scene\n")).unwrap();
    assert!(!retained.general.comp_easy_bonus);
    assert!(cold.general.comp_easy_bonus);
    assert_eq!(retained.source_ini_hash(), cold.source_ini_hash());
    assert_ne!(
        retained.simulation_config_hash(),
        cold.simulation_config_hash()
    );
}

#[test]
fn country_multiplay_is_separate_from_passive_and_retains_native_bool_history() {
    use crate::rules::native_processing::NativeRulesRegistryState;
    use crate::util::save_codec;
    let ini = IniFile::from_str;
    let mut layers = RulesLayerStack::new(ini(
        "[Countries]\n0=Fixture\n1=ConstructorOnly\n[Fixture]\nMultiplayPassive=yes\n",
    ));
    let rules = RuleSet::from_rules_layers(&layers).unwrap();
    assert!(!rules.country_multiplay("Fixture"));
    assert!(rules.country_multiplay_passive("Fixture"));
    assert!(!rules.country_multiplay("ConstructorOnly"));
    assert!(!rules.country_multiplay("Missing"));
    for (kind, text, expected) in [
        (RulesLayerKind::LangRule, "[Fixture]\nMultiplay=yes\n", true),
        (RulesLayerKind::GameMode, "[Fixture]\nMultiplay=2\n", true),
        (RulesLayerKind::Scenario, "[Fixture]\nMultiplay=\n", true),
        (RulesLayerKind::Scenario, "[Fixture]\nmultiplay=no\n", true),
        (RulesLayerKind::Scenario, "[fixture]\nMultiplay=no\n", true),
        (RulesLayerKind::Scenario, "[Fixture]\nMultiplay=no\n", false),
        (RulesLayerKind::Scenario, "[Fixture]\nMultiplay=yes\n", true),
    ] {
        layers.push(kind, ini(text));
        assert_eq!(
            RuleSet::from_rules_layers(&layers)
                .unwrap()
                .country_multiplay("fixture"),
            expected,
            "{text}"
        );
    }
    let (_, trace) = layers
        .process()
        .unwrap()
        .into_ini_and_native_type_construction_trace();
    let state = trace.into_registry_state_discarding_events();
    let state: NativeRulesRegistryState =
        save_codec::deserialize(&save_codec::serialize(&state).unwrap()).unwrap();
    let continued = RulesLayerStack::new(ini("[Countries]\n0=Fixture\n"))
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
        .unwrap();
    assert!(
        RuleSet::from_processed_rules(&continued)
            .unwrap()
            .country_multiplay("Fixture")
    );
    let (_, trace) = continued.into_ini_and_native_type_construction_trace();
    let reset = RulesLayerStack::new(ini("[Countries]\n0=Fixture\n"))
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            trace
                .into_registry_state_discarding_events()
                .destructive_reset(),
        )
        .unwrap();
    assert!(
        !RuleSet::from_processed_rules(&reset)
            .unwrap()
            .country_multiplay("Fixture")
    );
}

#[test]
fn retail_playable_countries_use_multiplay_flag_and_ui_name_source() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    for country in crate::skirmish_launch::LaunchCountry::ALL {
        let id = rules.country_name(country.index()).unwrap();
        assert!(rules.country_multiplay(id), "{id}");
        assert!(rules.country_display_name_sources(id).0.is_some(), "{id}");
    }
    assert!(!rules.country_multiplay("Neutral"));
    assert!(!rules.country_multiplay("Special"));
}
