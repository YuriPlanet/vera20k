//! Production Rules reader regressions for installed Ares FactoryOwners.
//! Reader/constructor/phase expectations currently rest on instructions
//! 1003CB1D,1003DEC0,1003E973,1003FAE0,1003C310,1001BA90, not executed
//! FactoryOwners list goldens. This does not claim full production/save parity.
//! Shared Country Find below separately consumes the existing executed
//! ares_country_veterancy parent_find controls; qualification suffix execution
//! is retained in tools/rules_oracle/ares_factory_owners.{json,md}.

use super::*;
use crate::rules::ruleset::{CountryIdx, RuleSet};
#[cfg(feature = "ares")]
use crate::util::save_codec;

#[test]
fn shared_ares_country_find_matches_native_controls_and_rules_projection() {
    let native =
        &crate::test_fixture::json("tools/rules_oracle/ares_country_veterancy.json")["parent_find"];
    let ids: Vec<_> = native["registry_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap())
        .collect();
    let mut text = String::from("[Countries]\n");
    for (index, id) in ids.iter().enumerate() {
        text.push_str(&format!("{index}={id}\n"));
    }
    text.push_str("[PARENT]\nName=PARENT_ALIAS\n");
    let rules = RuleSet::from_ini(&IniFile::from_str(&text)).unwrap();
    for row in native["rows"].as_array().unwrap() {
        let token = row["token"].as_str().unwrap();
        let expected = row["returned_registry_index"]
            .as_u64()
            .map(|index| CountryIdx(u16::try_from(index).unwrap()));
        assert_eq!(
            ares_country_index_of_name(token, ids.iter().copied()),
            expected,
            "{row}"
        );
        // The production allocator excludes sentinel IDs and duplicate case
        // variants. Compare the same native returned identity in that registry.
        let expected = row["returned_id"]
            .as_str()
            .and_then(|id| rules.country_index(id));
        assert_eq!(rules.ares_find_country(token), expected, "{row}");
    }
    #[cfg(feature = "ares")]
    {
        let mut reader = RulesPassProcessor::default();
        for id in ids {
            reader.find_or_construct(RulesTypeFamily::Country, id);
        }
        let building = reader.find_or_construct(RulesTypeFamily::Building, "CONTROL");
        reader.process_techno_family(
            RulesTypeFamily::Building,
            &IniFile::from_str("[CONTROL]\nFactoryOwners=NoNe,<NONE>,dup,parent\n"),
            &IniFile::empty(),
        );
        assert_eq!(
            reader.families[&RulesTypeFamily::Building][building]
                .factory_owners
                .as_ref()
                .unwrap()
                .allowed,
            vec![CountryIdx(3), CountryIdx(0)],
            "FactoryOwners uses the same Find even with registered sentinel IDs"
        );
        let child = reader.find_or_construct(RulesTypeFamily::Country, "CHILD");
        reader.family_mut(RulesTypeFamily::Country)[child].country_parent = "none".to_owned();
        reader.family_mut(RulesTypeFamily::Country)[1]
            .country_veterancy
            .ares
            .buildings = vec!["CONTROL".to_owned()];
        reader.process_ares_country(child, &IniFile::empty(), false);
        let state = &reader.families[&RulesTypeFamily::Country][child]
            .country_veterancy
            .ares;
        assert!(state.parent_initialized);
        assert!(
            state.buildings.is_empty(),
            "Country inheritance also rejects the registered sentinel"
        );
    }
}

#[test]
fn all_techno_families_project_factory_fields_from_process_without_a_second_reader() {
    for family in [
        "BuildingTypes",
        "VehicleTypes",
        "AircraftTypes",
        "InfantryTypes",
    ] {
        let registries = if family == "BuildingTypes" {
            "[BuildingTypes]\n0=CONTROL\n1=SOURCE\n".to_owned()
        } else {
            format!("[BuildingTypes]\n0=SOURCE\n[{family}]\n0=CONTROL\n")
        };
        let ini = IniFile::from_str(&format!(
            "[Countries]\n0=CountryA\n1=CountryB\n\
             [CountryA]\nName=Alias\nParentCountry=CountryB\n\
             {registries}[SOURCE]\nFactory.ExplicitOnly=yes\n[CONTROL]\n\
             BuiltAt=SOURCE,SOURCE,unknown\nFactory.ExplicitOnly=yes\n\
             FactoryOwners=countrya,COUNTRYA,Alias,unknown,CountryB\n\
             FactoryOwners.Forbidden=CountryB\n\
             FactoryOwners.HaveAllPlans=yes\nFactoryOwners.Permanent=no\n\
             FactoryOwners.HasAllPlans=yes\n"
        ));
        let direct = crate::rules::object_type::ObjectType::from_ini_section(
            "CONTROL",
            ini.section("CONTROL").unwrap(),
            ObjectCategory::Building,
        );
        assert!(
            direct.factory_owners().is_empty(),
            "raw constructor has no Country registry"
        );
        assert!(!direct.factory_owners_permanent());
        assert!(!direct.factory_has_all_plans());
        assert!(direct.built_at().is_empty());
        assert!(!direct.factory_explicit_only());

        let rules = RuleSet::from_ini(&ini).unwrap();
        let object = rules.object("CONTROL").unwrap();
        let allowed = if crate::rules::ARES_ENABLED {
            vec![CountryIdx(0), CountryIdx(0), CountryIdx(1)]
        } else {
            vec![]
        };
        assert_eq!(object.factory_owners(), allowed, "{family}");
        assert_eq!(
            object.forbidden_factory_owners(),
            if crate::rules::ARES_ENABLED {
                vec![CountryIdx(1)]
            } else {
                vec![]
            },
            "{family}"
        );
        assert!(!object.factory_owners_permanent(), "later alias wins");
        assert_eq!(object.factory_has_all_plans(), crate::rules::ARES_ENABLED);
        let source = rules
            .type_array_index(ObjectCategory::Building, "SOURCE")
            .unwrap();
        assert_eq!(rules.ares_find_building("sOuRcE"), Some(source));
        assert_eq!(rules.ares_find_building("none"), None);
        assert_eq!(rules.ares_find_building(" SOURCE"), None);
        assert_eq!(
            object.built_at(),
            if crate::rules::ARES_ENABLED {
                vec![source, source]
            } else {
                vec![]
            }
        );
        assert_eq!(
            object.factory_explicit_only(),
            crate::rules::ARES_ENABLED && family == "BuildingTypes"
        );
        assert_eq!(
            rules.object("SOURCE").unwrap().factory_explicit_only(),
            crate::rules::ARES_ENABLED
        );
    }
}

#[cfg(feature = "ares")]
#[test]
fn factory_lists_and_flags_retain_across_layers_and_replace_only_on_nonempty_reads() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[Countries]\n0=A\n1=B\n[BuildingTypes]\n0=CONTROL\n\
         [CONTROL]\nFactoryOwners=A,A\nFactoryOwners.Forbidden=B\n\
         BuiltAt=CONTROL,CONTROL\nFactory.ExplicitOnly=yes\n\
         FactoryOwners.Permanent=yes\nFactoryOwners.HasAllPlans=yes\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[CONTROL]\nFactoryOwners=\nFactoryOwners.Forbidden=unknown\n\
         BuiltAt=\nFactory.ExplicitOnly=bad\n\
         FactoryOwners.HaveAllPlans=no\nFactoryOwners.Permanent=bad\n\
         FactoryOwners.HasAllPlans=bad\n",
        ),
    );
    let rules = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
    let object = rules.object("CONTROL").unwrap();
    assert_eq!(object.factory_owners(), &[CountryIdx(0), CountryIdx(0)]);
    assert!(object.forbidden_factory_owners().is_empty());
    assert!(!object.factory_owners_permanent());
    assert!(object.factory_has_all_plans());
    assert_eq!(object.built_at(), &[0, 0]);
    assert!(object.factory_explicit_only());
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[CONTROL]\nFactoryOwners=,,,\nFactoryOwners.Forbidden=B\n\
         BuiltAt=,,,\nFactory.ExplicitOnly=no\n\
         FactoryOwners.Permanent=yes\nFactoryOwners.HasAllPlans=no\n",
        ),
    );
    let rules = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
    let object = rules.object("CONTROL").unwrap();
    assert!(object.factory_owners().is_empty());
    assert_eq!(object.forbidden_factory_owners(), &[CountryIdx(1)]);
    assert!(object.factory_owners_permanent());
    assert!(!object.factory_has_all_plans());
    assert!(object.built_at().is_empty());
    assert!(!object.factory_explicit_only());
}

#[cfg(feature = "ares")]
#[test]
fn factory_country_binding_uses_current_registry_and_retained_extension_admission() {
    let mut reader = RulesPassProcessor::default();
    reader.find_or_construct(RulesTypeFamily::Country, "EARLY");
    let index = reader.find_or_construct(RulesTypeFamily::Building, "CONTROL");
    assert_eq!(
        reader.families[&RulesTypeFamily::Building][index].factory_owners,
        Some(AresFactoryOwners::default())
    );
    let pass = IniFile::from_str("[CONTROL]\nFactoryOwners=LATE\nBuiltAt=LATE_FACTORY\n");
    reader.process_techno_family(RulesTypeFamily::Building, &pass, &IniFile::empty());
    let late = reader.find_or_construct(RulesTypeFamily::Country, "LATE");
    let late_factory = reader.find_or_construct(RulesTypeFamily::Building, "LATE_FACTORY");
    reader.process_techno_family(
        RulesTypeFamily::Building,
        &IniFile::from_str("[CONTROL]\nStrength=100\n"),
        &IniFile::empty(),
    );
    assert!(
        reader.families[&RulesTypeFamily::Building][index]
            .factory_owners
            .as_ref()
            .unwrap()
            .allowed
            .is_empty(),
        "later registration without a list read cannot backfill the missing reference"
    );
    assert!(
        reader.families[&RulesTypeFamily::Building][index]
            .factory_owners
            .as_ref()
            .unwrap()
            .built_at
            .is_empty()
    );
    reader.process_techno_family(RulesTypeFamily::Building, &pass, &IniFile::empty());
    assert_eq!(
        reader.families[&RulesTypeFamily::Building][index]
            .factory_owners
            .as_ref()
            .unwrap()
            .built_at,
        vec![i32::try_from(late_factory).unwrap()]
    );
    assert_eq!(
        reader.families[&RulesTypeFamily::Building][index]
            .factory_owners
            .as_ref()
            .unwrap()
            .allowed,
        vec![CountryIdx(u16::try_from(late).unwrap())]
    );

    for phase in [-1, 0, 1, 2, 3] {
        let state = reader.family_mut(RulesTypeFamily::Building)[index]
            .factory_owners
            .as_mut()
            .unwrap();
        state.phase = phase;
        state.allowed = vec![CountryIdx(1)];
        state.built_at = vec![1];
        reader.process_techno_family(
            RulesTypeFamily::Building,
            &IniFile::from_str("[CONTROL]\nFactoryOwners=EARLY\nBuiltAt=CONTROL\n"),
            &IniFile::empty(),
        );
        let state = reader.families[&RulesTypeFamily::Building][index]
            .factory_owners
            .as_ref()
            .unwrap();
        assert_eq!(
            state.allowed,
            if (0..=2).contains(&phase) {
                vec![CountryIdx(0)]
            } else {
                vec![CountryIdx(1)]
            }
        );
        assert_eq!(
            state.phase,
            if (0..=2).contains(&phase) { 2 } else { phase }
        );
        assert_eq!(
            state.built_at,
            if (0..=2).contains(&phase) {
                vec![0]
            } else {
                vec![1]
            }
        );
    }
    reader.family_mut(RulesTypeFamily::Building)[index]
        .factory_owners
        .as_mut()
        .unwrap()
        .phase = 2;
    let mut section = IniSection::new("CONTROL".to_owned());
    section.set("FactoryOwners", &format!("{}EARLY", ",".repeat(2047)));
    section.set("BuiltAt", &format!("{}CONTROL", ",".repeat(2047)));
    reader.process_techno_family(
        RulesTypeFamily::Building,
        &IniFile::from_sections_for_test([section]),
        &IniFile::empty(),
    );
    assert!(
        reader.families[&RulesTypeFamily::Building][index]
            .factory_owners
            .as_ref()
            .unwrap()
            .built_at
            .is_empty()
    );
    assert!(
        reader.families[&RulesTypeFamily::Building][index]
            .factory_owners
            .as_ref()
            .unwrap()
            .allowed
            .is_empty(),
        "ReadString2048 excludes a registered token beyond its 2047-byte payload"
    );
}

#[cfg(feature = "ares")]
#[test]
fn factory_state_save_restore_hash_validation_and_reset_preserve_one_owner() {
    let initial = RulesLayerStack::new(IniFile::from_str(
        "[Countries]\n0=A\n1=B\n[BuildingTypes]\n0=CONTROL\n[CONTROL]\n\
         FactoryOwners=A,A,B\nFactoryOwners.Forbidden=B\n\
         BuiltAt=CONTROL,CONTROL\nFactory.ExplicitOnly=yes\n\
         FactoryOwners.Permanent=yes\nFactoryOwners.HasAllPlans=yes\n",
    ))
    .process()
    .unwrap();
    let bytes = save_codec::serialize(&initial).unwrap();
    let restored: ProcessedRulesLayers = save_codec::deserialize(&bytes).unwrap();
    let rules = RuleSet::from_processed_rules(&restored).unwrap();
    assert_eq!(
        rules.object("CONTROL").unwrap().factory_owners(),
        &[CountryIdx(0), CountryIdx(0), CountryIdx(1)]
    );
    assert_eq!(rules.object("CONTROL").unwrap().built_at(), &[0, 0]);
    assert!(rules.object("CONTROL").unwrap().factory_explicit_only());
    let mut altered = serde_json::to_value(&restored).unwrap();
    let state = &mut altered["native_type_construction_trace"]["registry_state"]["families"]["Building"]
        [0]["factory_owners"];
    state["phase"] = 3.into();
    let altered: ProcessedRulesLayers = serde_json::from_value(altered).unwrap();
    let other = RuleSet::from_processed_rules(&altered).unwrap();
    assert_ne!(
        rules.simulation_config_hash(),
        other.simulation_config_hash(),
        "phase changes future read admission"
    );
    let mut bad = serde_json::to_value(&restored).unwrap();
    bad["native_type_construction_trace"]["registry_state"]["families"]["Building"][0]["factory_owners"]
        ["allowed"] = serde_json::json!([2]);
    let bad: ProcessedRulesLayers = serde_json::from_value(bad).unwrap();
    assert!(RuleSet::from_processed_rules(&bad).is_err());
    let mut bad = serde_json::to_value(&restored).unwrap();
    bad["native_type_construction_trace"]["registry_state"]["families"]["Building"][0]["factory_owners"]
        ["built_at"] = serde_json::json!([-1]);
    let bad: ProcessedRulesLayers = serde_json::from_value(bad).unwrap();
    assert!(RuleSet::from_processed_rules(&bad).is_err());

    let (_, trace) = restored.into_ini_and_native_type_construction_trace();
    let reset = RulesLayerStack::new(IniFile::from_str(
        "[Countries]\n0=A\n[BuildingTypes]\n0=CONTROL\n[CONTROL]\nStrength=100\n",
    ))
    .process_with_fixed_art_and_registry_state(
        &IniFile::empty(),
        trace
            .into_registry_state_discarding_events()
            .destructive_reset(),
    )
    .unwrap();
    let reset = RuleSet::from_processed_rules(&reset).unwrap();
    let object = reset.object("CONTROL").unwrap();
    assert!(object.factory_owners().is_empty());
    assert!(object.forbidden_factory_owners().is_empty());
    assert!(!object.factory_owners_permanent());
    assert!(!object.factory_has_all_plans());
    assert!(object.built_at().is_empty());
    assert!(!object.factory_explicit_only());
}

#[cfg(feature = "ares")]
#[test]
fn explicit_factory_uses_separate_building_phase_and_current_stock_repair_guard() {
    let mut reader = RulesPassProcessor::default();
    reader.find_or_construct(RulesTypeFamily::Country, "A");
    let index = reader.find_or_construct(RulesTypeFamily::Building, "CONTROL");
    for (body, expected) in [
        (
            "Factory=AircraftType\nUnitRepair=yes\nFactory.ExplicitOnly=yes\n",
            false,
        ),
        ("UnitRepair=no\nFactory.ExplicitOnly=yes\n", true),
        // Missing Factory retains AircraftType; the common-reader skip keeps true.
        ("UnitRepair=yes\nFactory.ExplicitOnly=no\n", true),
        // Missing UnitRepair retains true, but the updated Factory no longer skips.
        ("Factory=UnitType\nFactory.ExplicitOnly=no\n", false),
    ] {
        reader.process_techno_family(
            RulesTypeFamily::Building,
            &IniFile::from_str(&format!("[CONTROL]\n{body}")),
            &IniFile::empty(),
        );
        let state = reader.families[&RulesTypeFamily::Building][index]
            .explicit_factory
            .as_ref()
            .unwrap();
        assert_eq!(state.explicit_only, expected, "{body}");
        assert_eq!(state.phase, 2);
    }
    for phase in [-1, 0, 1, 2, 3] {
        reader.family_mut(RulesTypeFamily::Building)[index]
            .factory_owners
            .as_mut()
            .unwrap()
            .phase = 3;
        let state = reader.family_mut(RulesTypeFamily::Building)[index]
            .explicit_factory
            .as_mut()
            .unwrap();
        state.phase = phase;
        state.explicit_only = false;
        reader.process_techno_family(
            RulesTypeFamily::Building,
            &IniFile::from_str("[CONTROL]\nFactory.ExplicitOnly=yes\nFactoryOwners=A\n"),
            &IniFile::empty(),
        );
        let member = &reader.families[&RulesTypeFamily::Building][index];
        let state = member.explicit_factory.as_ref().unwrap();
        assert_eq!(state.explicit_only, (0..=2).contains(&phase));
        assert_eq!(
            state.phase,
            if (0..=2).contains(&phase) { 2 } else { phase }
        );
        assert!(
            member.factory_owners.as_ref().unwrap().allowed.is_empty(),
            "Techno phase is independently skipped"
        );
    }
}

#[cfg(feature = "ares")]
#[test]
fn retained_global_controls_match_native_constructor_and_eight_read_history_controls() {
    let native = &crate::test_fixture::json("tools/rules_oracle/ares_factory_owners.json")["global_controls"];
    let mut state = AresGlobalControls::default();
    assert_eq!(
        i64::from(state.phase),
        native["initial"]["phase"].as_i64().unwrap()
    );
    assert_eq!(
        state.allow_bypass_build_limit.map(u8::from).as_slice(),
        native["initial"]["allow_bypass_by_difficulty"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_u64().unwrap() as u8)
            .collect::<Vec<_>>()
    );
    state.phase = 2; // Native controls explicitly bypass unrelated first-phase initialization.
    for row in native["rows"].as_array().unwrap() {
        let before: Vec<_> = row["before"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_u64().unwrap() != 0)
            .collect();
        assert_eq!(state.allow_bypass_build_limit.as_slice(), before, "{row}");
        let pass = match row["supplied_ascii"].as_str() {
            Some(value) => IniFile::from_str(&format!(
                "[GlobalControls]\nAllowBypassBuildLimit={value}\n"
            )),
            None => IniFile::empty(),
        };
        state.read_pass(&pass);
        let after: Vec<_> = row["after"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_u64().unwrap() != 0)
            .collect();
        assert_eq!(state.allow_bypass_build_limit.as_slice(), after, "{row}");
        assert_eq!(i64::from(state.phase), row["phase_after"].as_i64().unwrap());
    }
    for phase in [-1, 0, 1, 2, 3] {
        let mut state = AresGlobalControls {
            phase,
            allow_bypass_build_limit: [true; 3],
        };
        state.read_pass(&IniFile::from_str(
            "[GlobalControls]\nAllowBypassBuildLimit=no,bad,yes\n",
        ));
        assert_eq!(
            state.allow_bypass_build_limit,
            if (0..=2).contains(&phase) {
                [true, true, false]
            } else {
                [true; 3]
            }
        );
        assert_eq!(
            state.phase,
            if (0..=2).contains(&phase) { 2 } else { phase }
        );
    }
}

#[test]
fn global_controls_project_only_reached_build_capability_and_difficulty_indices() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[GlobalControls]\nAllowBypassBuildLimit=no,yes,no\n",
    ))
    .unwrap();
    assert!(!rules.ares_ai_bypasses_build_limit(0));
    assert_eq!(
        rules.ares_ai_bypasses_build_limit(1),
        crate::rules::ARES_ENABLED
    );
    assert!(!rules.ares_ai_bypasses_build_limit(2));
    assert!(!rules.ares_ai_bypasses_build_limit(3));
    assert!(!rules.ares_ai_bypasses_build_limit(usize::MAX));
}

#[cfg(feature = "ares")]
#[test]
fn global_controls_process_save_and_hash_retain_flag_prefix_and_reader_phase() {
    let initial = RulesLayerStack::new(IniFile::from_str(
        "[GlobalControls]\nAllowBypassBuildLimit=yes,yes,yes\n",
    ))
    .process()
    .unwrap();
    let (_, trace) = initial.into_ini_and_native_type_construction_trace();
    let continued = RulesLayerStack::new(IniFile::from_str(
        "[GlobalControls]\nAllowBypassBuildLimit=no,bad,yes\n",
    ))
    .process_with_fixed_art_and_registry_state(
        &IniFile::empty(),
        trace.into_registry_state_discarding_events(),
    )
    .unwrap();
    let bytes = save_codec::serialize(&continued).unwrap();
    let restored: ProcessedRulesLayers = save_codec::deserialize(&bytes).unwrap();
    let rules = RuleSet::from_processed_rules(&restored).unwrap();
    assert!(rules.ares_ai_bypasses_build_limit(0));
    assert!(rules.ares_ai_bypasses_build_limit(1));
    assert!(!rules.ares_ai_bypasses_build_limit(2));
    let mut altered = serde_json::to_value(&restored).unwrap();
    altered["native_type_construction_trace"]["registry_state"]["ares_global_controls"]["phase"] =
        3.into();
    let altered: ProcessedRulesLayers = serde_json::from_value(altered).unwrap();
    assert_ne!(
        rules.simulation_config_hash(),
        RuleSet::from_processed_rules(&altered)
            .unwrap()
            .simulation_config_hash()
    );
    let (_, trace) = altered.into_ini_and_native_type_construction_trace();
    let skipped = RulesLayerStack::new(IniFile::from_str(
        "[GlobalControls]\nAllowBypassBuildLimit=no,no,yes\n",
    ))
    .process_with_fixed_art_and_registry_state(
        &IniFile::empty(),
        trace.into_registry_state_discarding_events(),
    )
    .unwrap();
    assert_eq!(
        skipped.ares_global_controls().allow_bypass_build_limit,
        [true, true, false]
    );
    let (_, trace) = restored.into_ini_and_native_type_construction_trace();
    let type_reset = trace
        .into_registry_state_discarding_events()
        .destructive_reset();
    assert_eq!(
        type_reset.ares_global_controls.allow_bypass_build_limit,
        [true, true, false]
    );
    assert_eq!(type_reset.ares_global_controls.phase, 2);
    let retained = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), type_reset)
        .unwrap();
    assert_eq!(
        retained.ares_global_controls().allow_bypass_build_limit,
        [true, true, false]
    );
    let (_, trace) = retained.into_ini_and_native_type_construction_trace();
    let mut cleared = trace.into_registry_state_discarding_events();
    cleared.clear_scenario_global_controls();
    assert_eq!(
        cleared.ares_global_controls,
        AresGlobalControls::default(),
        "actual ScenarioClear reconstructs RulesExt separately from TypeReset"
    );
}
