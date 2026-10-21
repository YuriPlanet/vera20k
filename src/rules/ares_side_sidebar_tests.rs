//! Selected Ares SideExt controls through the retained production Rules owner.
//! Native coverage is initializer/reader-prefix execution, not complete startup.

use super::*;
use crate::rules::ruleset::RuleSet;
#[cfg(feature = "ares")]
use crate::rules::ruleset::SideIdx;

fn advance(processed: ProcessedRulesLayers, pass: IniFile) -> ProcessedRulesLayers {
    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    RulesLayerStack::new(pass)
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            trace.into_registry_state_discarding_events(),
        )
        .unwrap()
}

#[cfg(feature = "ares")]
fn native() -> &'static serde_json::Value {
    crate::test_fixture::json("tools/rules_oracle/ares_side_sidebar.json")
}

#[cfg(feature = "ares")]
fn registry_ini(order: &serde_json::Value) -> IniFile {
    let mut sides = IniSection::new("Sides".into());
    for id in order.as_array().unwrap() {
        sides.set(id.as_str().unwrap(), "");
    }
    IniFile::from_sections_for_test([sides])
}

#[cfg(feature = "ares")]
fn selected_pass(values: &serde_json::Value) -> IniFile {
    let mut section = IniSection::new("FourthSynthetic".into());
    for (key, value) in values.as_object().unwrap() {
        section.set(key, value.as_str().unwrap());
    }
    IniFile::from_sections_for_test([section])
}

#[cfg(feature = "ares")]
fn assert_selected(rules: &RuleSet, id: &str, expected: &serde_json::Value) {
    let index = rules.side_index(id).unwrap();
    assert_eq!(
        rules.side_sidebar_mix_file_index(index),
        Some(expected["mix_file_index"].as_i64().unwrap() as i32),
        "{id}: MIX index"
    );
    assert_eq!(
        rules.side_sidebar_yuri_file_names(index),
        expected["yuri_file_names"].as_bool(),
        "{id}: Yuri filenames"
    );
}

#[cfg(feature = "ares")]
#[test]
fn ares_side_sidebar_defaults_and_retained_layers_match_native_controls() {
    let initial = &native()["initial"];
    let processed = RulesLayerStack::new(registry_ini(&initial["fixture_side_order"]))
        .process()
        .unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    for row in initial["rows"].as_array().unwrap() {
        assert_selected(&rules, row["name"].as_str().unwrap(), &row["state"]);
    }
    assert_eq!(
        rules
            .allocated_sides()
            .map(|(_, id)| id)
            .collect::<Vec<_>>(),
        initial["fixture_side_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap())
            .collect::<Vec<_>>()
    );
    assert_eq!(rules.side_sidebar_mix_file_index(SideIdx(u8::MAX)), None);

    let retained = &native()["retained"];
    let mut processed = RulesLayerStack::new(registry_ini(&retained["fixture_side_order"]))
        .process()
        .unwrap();
    for row in retained["rows"].as_array().unwrap() {
        assert_selected(
            &RuleSet::from_processed_rules(&processed).unwrap(),
            "FourthSynthetic",
            &row["before"],
        );
        processed = advance(processed, selected_pass(&row["supplied_ascii_reads"]));
        assert_selected(
            &RuleSet::from_processed_rules(&processed).unwrap(),
            "FourthSynthetic",
            &row["after"],
        );
        // Restore the actual retained owner, rather than reconstructing from
        // the latest projected INI or rerunning constructor/default logic.
        let bytes = crate::util::save_codec::serialize(&processed).unwrap();
        let restored: ProcessedRulesLayers = crate::util::save_codec::deserialize(&bytes).unwrap();
        assert_eq!(
            RuleSet::from_processed_rules(&processed)
                .unwrap()
                .simulation_config_hash(),
            RuleSet::from_processed_rules(&restored)
                .unwrap()
                .simulation_config_hash()
        );
        processed = restored;
    }
}

#[cfg(feature = "ares")]
#[test]
fn ares_side_sidebar_waits_for_full_type_pass_and_preserves_failed_process_state() {
    let root = IniFile::from_str(
        "[Countries]\n0=COUNTRY\n[COUNTRY]\nSide=FourthSynthetic\n\
         [FourthSynthetic]\nSidebar.MixFileIndex=4\nSidebar.YuriFileNames=yes\n",
    );
    // The Country-only prepass may allocate a Side but never executes the
    // ReadTypeData679A10 hook. SideExt ctor1002D9F0 leaves +88=0/+8C=false.
    let prepass =
        process_native_noncampaign_rules_prepass(NativeRulesRegistryState::default(), &root);
    let state = prepass.registry_state().families[&RulesTypeFamily::Side][0]
        .side_sidebar
        .as_ref()
        .unwrap();
    assert_eq!(
        (state.phase, state.mix_file_index, state.yuri_file_names),
        (
            native()["constructor"]["state"]["phase"].as_i64().unwrap() as i32,
            native()["constructor"]["state"]["mix_file_index"]
                .as_i64()
                .unwrap() as i32,
            native()["constructor"]["state"]["yuri_file_names"]
                .as_bool()
                .unwrap(),
        )
    );
    let processed = RulesLayerStack::new(root.clone())
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            prepass.into_registry_state_discarding_events(),
        )
        .unwrap();
    let configured = &native()["retained"]["rows"][1]["after"];
    assert_selected(
        &RuleSet::from_processed_rules(&processed).unwrap(),
        "FourthSynthetic",
        configured,
    );

    // Without that prepass the Country reader allocates after the Side hook.
    let late = RulesLayerStack::new(root).process().unwrap();
    let late_rules = RuleSet::from_processed_rules(&late).unwrap();
    let side = late_rules.side_index("FourthSynthetic").unwrap();
    assert_eq!(late_rules.side_sidebar_mix_file_index(side), Some(0));
    let late = advance(late, IniFile::empty());
    assert_selected(
        &RuleSet::from_processed_rules(&late).unwrap(),
        "FourthSynthetic",
        &native()["initial"]["rows"][0]["state"],
    );

    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    let failure = RulesLayerStack::new(IniFile::from_str("[Sides]\nBroken=<random>\n"))
        .process_with_fixed_art_and_registry_state_recovering(
            &IniFile::empty(),
            trace.into_registry_state_discarding_events(),
            RulesProcessContext::Ordinary,
        )
        .unwrap_err();
    let (_, failed) = failure.into_parts();
    let processed = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            failed.into_registry_state_discarding_events(),
        )
        .unwrap();
    assert_selected(
        &RuleSet::from_processed_rules(&processed).unwrap(),
        "FourthSynthetic",
        configured,
    );
    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    let reset = RulesLayerStack::new(IniFile::from_str(
        "[Sides]\nFourthSynthetic=UnknownCountry\n",
    ))
    .process_with_fixed_art_and_registry_state(
        &IniFile::empty(),
        trace
            .into_registry_state_discarding_events()
            .destructive_reset(),
    )
    .unwrap();
    assert_selected(
        &RuleSet::from_processed_rules(&reset).unwrap(),
        "FourthSynthetic",
        &native()["initial"]["rows"][0]["state"],
    );
    assert!(
        RuleSet::from_processed_rules(&reset)
            .unwrap()
            .side_index("Broken")
            .is_none()
    );
}

#[cfg(feature = "ares")]
#[test]
fn ares_side_sidebar_retained_values_enter_configuration_identity() {
    let make = |row: usize| {
        let initial =
            RulesLayerStack::new(registry_ini(&native()["retained"]["fixture_side_order"]))
                .process()
                .unwrap();
        let configured = advance(
            initial,
            selected_pass(&native()["retained"]["rows"][row]["supplied_ascii_reads"]),
        );
        let invalid = advance(
            configured,
            IniFile::from_str(
                "[FourthSynthetic]\nSidebar.MixFileIndex=invalid\nSidebar.YuriFileNames=invalid\n",
            ),
        );
        RuleSet::from_processed_rules(&invalid).unwrap()
    };
    let first = make(1);
    let second = make(5);
    assert_eq!(first.source_ini_hash(), second.source_ini_hash());
    assert_ne!(
        first.simulation_config_hash(),
        second.simulation_config_hash()
    );
    assert_selected(
        &first,
        "FourthSynthetic",
        &native()["retained"]["rows"][1]["after"],
    );
    assert_selected(
        &second,
        "FourthSynthetic",
        &native()["retained"]["rows"][5]["after"],
    );
}

#[cfg(not(feature = "ares"))]
#[test]
fn ares_side_sidebar_keys_are_inert_without_the_compiled_extension() {
    let configured = RulesLayerStack::new(IniFile::from_str(
        "[Sides]\nGDI=UnknownCountry\nNod=UnknownCountry\nThirdSide=UnknownCountry\nFourthSynthetic=UnknownCountry\n\
         [FourthSynthetic]\nSidebar.MixFileIndex=4\nSidebar.YuriFileNames=yes\n",
    ))
    .process()
    .unwrap();
    let rules = RuleSet::from_processed_rules(&advance(configured, IniFile::empty())).unwrap();
    assert_eq!(rules.allocated_sides().count(), 4);
    for (index, _) in rules.allocated_sides() {
        assert_eq!(rules.side_sidebar_mix_file_index(index), None);
        assert_eq!(rules.side_sidebar_yuri_file_names(index), None);
    }
}
