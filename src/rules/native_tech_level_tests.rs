//! TechnoType identity, current-default reads and generation retirement.
//!
//! The native lifecycle controls execute real AircraftType construction,
//! TAction106, Process and Reset on gamemd3e81a617..08600. The tests below
//! compare their bounded scalar/identity history, not whole Full_Init parity.

use super::*;
use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
use crate::rules::ruleset::RuleSet;
use serde_json::Value;

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/trigger_object_tech_level.json")
}

fn number(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn stage<'a>(native: &'a Value, name: &str) -> &'a Value {
    native["lifecycle"]["stages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stage| stage["name"] == name)
        .unwrap()
}

fn assert_stage(registry: &NativeRulesRegistryState, expected: &Value) {
    registry.validate_techno_type_order().unwrap();
    let actual: Vec<_> = registry.techno_type_states().collect();
    let types = expected["types"].as_array().unwrap();
    assert_eq!(
        actual.len(),
        expected["global_count"].as_u64().unwrap() as usize
    );
    assert_eq!(
        registry.family_len(NativeTypeConstructorFamily::AircraftType),
        expected["aircraft_count"].as_u64().unwrap() as usize
    );
    assert_eq!(actual.len(), types.len());
    for ((category, id, tech_level), original) in actual.into_iter().zip(types) {
        assert_eq!(category, ObjectCategory::Aircraft);
        assert_eq!(id, original["id"].as_str().unwrap());
        assert_eq!(tech_level, number(&original["tech_level"]));
    }
}

fn registry(processed: ProcessedRulesLayers) -> NativeRulesRegistryState {
    processed
        .into_ini_and_native_type_construction_trace()
        .1
        .into_registry_state_discarding_events()
}

fn ordered_prefix(sections: &Value) -> IniFile {
    IniFile::from_sections_for_test(sections.as_object().unwrap().iter().map(|(id, values)| {
        let mut section = IniSection::new(id.clone());
        for (key, value) in values.as_object().unwrap() {
            section.set(key, value.as_str().unwrap());
        }
        section
    }))
}

#[test]
fn native_tech_level_lifecycle_retains_prefix_values_then_retires_the_generation() {
    let native = corpus();
    let seed = IniFile::from_str(
        "[AircraftTypes]\n0=ORCA\n1=BEAG\n[ORCA]\nName=ORCA\n[BEAG]\nName=BEAG\n",
    );
    let layers = RulesLayerStack::new(seed);
    let processed = layers.process().unwrap();
    let mut state = registry(processed);
    assert_stage(&state, stage(native, "constructors"));

    // The Simulation executes106; this boundary accepts its retired values.
    // Its native action outputs are the prior for the following Process.
    state
        .consume_outgoing_type_tech_levels(
            stage(native, "actions")["types"]
                .as_array()
                .unwrap()
                .iter()
                .map(|ty| {
                    (
                        ObjectCategory::Aircraft,
                        ty["id"].as_str().unwrap().to_owned(),
                        number(&ty["tech_level"]),
                    )
                }),
        )
        .unwrap();
    assert_stage(&state, stage(native, "actions"));

    for pass in native["lifecycle"]["input"]["prefix_passes"]
        .as_array()
        .unwrap()
    {
        let prefix = RulesLayerStack::new(ordered_prefix(&pass["sections"]));
        let processed = prefix
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
            .unwrap();
        let projected = RuleSet::from_processed_rules(&processed).unwrap();
        state = registry(processed);
        assert_stage(&state, stage(native, pass["name"].as_str().unwrap()));
        for (_, id, value) in state.techno_type_states() {
            assert_eq!(
                projected.native_techno_type_exact(id).unwrap().tech_level,
                value
            );
        }
    }

    // Save/load preserves the native allocation order and current fields;
    // it does not rerun constructors or a Rules pass.
    let bytes = bincode::serialize(&state).unwrap();
    let restored: NativeRulesRegistryState = bincode::deserialize(&bytes).unwrap();
    assert_stage(&restored, stage(native, "prefix_present_key"));
    let reset = restored.destructive_reset();
    assert_stage(&reset, stage(native, "destructive_reset"));
    let fresh = layers
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), reset)
        .unwrap();
    assert_stage(
        fresh.native_type_construction_trace().registry_state(),
        stage(native, "new_constructors"),
    );
}

#[test]
fn native_techno_lookup_uses_exact_case_and_global_allocation_chronology() {
    let first = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=SHARED\n[SHARED]\nTechLevel=12\n",
    ))
    .process()
    .unwrap();
    let tail = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=SHARED\n[SHARED]\nName=Shared\n",
    ));
    let processed = tail
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), registry(first))
        .unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    assert_eq!(
        rules.native_techno_type_exact("SHARED").unwrap().category,
        ObjectCategory::Building,
        "later-layer allocation follows Vehicle even though Process registers Buildings first"
    );
    assert!(rules.native_techno_type_exact("shared").is_none());
    assert!(rules.native_techno_type_exact("MISSING").is_none());
    assert!(
        rules.object("shared").is_some(),
        "broad lookup retains its old contract"
    );

    // A lazy Building reference is allocated during the later Vehicle reader,
    // after the explicit Infantry registry. It must win over that Infantry.
    let lazy = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=VEH\n[InfantryTypes]\n0=SHARED\n\
         [VEH]\nDeploysInto=SHARED\n[SHARED]\nTechLevel=7\n",
    ))
    .process()
    .unwrap();
    let rules = RuleSet::from_processed_rules(&lazy).unwrap();
    assert_eq!(
        rules.native_techno_type_exact("SHARED").unwrap().category,
        ObjectCategory::Building,
        "lazy allocation is recorded at its existing find-or-allocate owner"
    );
}

#[test]
fn native_techno_lookup_does_not_alias_unrepresentable_truncated_duplicates() {
    let long = "ABCDEFGHIJKLMNOPQRSTUVWX_suffix";
    let stored = abstract_type_stored_id(long);
    let processed = RulesLayerStack::new(IniFile::from_str(&format!(
        "[InfantryTypes]\n0={stored}\n[AircraftTypes]\n0={long}\n1={long}\n\
         [{stored}]\nTechLevel=4\n"
    )))
    .process()
    .unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    // Within this first Process the Infantry allocation occurs after both
    // ambiguous Aircraft slots, so the native winner is representable.
    assert_eq!(
        rules.native_techno_type_exact(&stored).unwrap().category,
        ObjectCategory::Infantry
    );
    let tail = RulesLayerStack::new(IniFile::from_str(&format!("[AircraftTypes]\n2={long}\n")))
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), registry(processed))
        .unwrap();
    let rules = RuleSet::from_processed_rules(&tail).unwrap();
    assert!(
        rules.object(&stored).is_some(),
        "legacy projection still parses"
    );
    assert!(
        rules.native_techno_type_exact(&stored).is_none(),
        "the last native match is ambiguous; do not fall back to the earlier Infantry"
    );
    let mut state = registry(tail);
    assert!(
        state
            .consume_outgoing_type_tech_levels([(ObjectCategory::Aircraft, stored, 1,)])
            .is_err(),
        "retirement cannot transfer a runtime value onto ambiguous native stored IDs"
    );
}

#[test]
fn saved_global_techno_order_rejects_bad_slots_missing_members_and_duplicates() {
    for corruption in [0, 1, 2, 3] {
        let mut processed =
            RulesLayerStack::new(IniFile::from_str("[AircraftTypes]\n0=ORCA\n1=BEAG\n"))
                .process()
                .unwrap();
        let order = &mut processed
            .native_type_construction_trace
            .registry_state
            .techno_type_order;
        match corruption {
            0 => order[0].1 = 10,
            1 => {
                order.pop();
            }
            2 => order[1] = order[0],
            _ => order[0].0 = RulesTypeFamily::Animation,
        }
        assert!(processed.validate_saved_state().is_err());
        assert!(RuleSet::from_processed_rules(&processed).is_err());
    }
}

#[test]
fn outgoing_tech_level_transfer_is_exact_and_atomic() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[AircraftTypes]\n0=ORCA\n1=BEAG\n[ORCA]\nTechLevel=3\n[BEAG]\nTechLevel=11\n",
    ))
    .process()
    .unwrap();
    let mut state = registry(processed);
    for (category, bad_id) in [
        (ObjectCategory::Aircraft, "orca"),
        (ObjectCategory::Aircraft, "MISSING"),
        (ObjectCategory::Vehicle, "BEAG"),
    ] {
        let before: Vec<_> = state
            .techno_type_states()
            .map(|(category, id, value)| (category, id.to_owned(), value))
            .collect();
        assert!(
            state
                .consume_outgoing_type_tech_levels([
                    (ObjectCategory::Aircraft, "ORCA".to_owned(), 99),
                    (category, bad_id.to_owned(), 1),
                ])
                .is_err()
        );
        assert_eq!(
            state
                .techno_type_states()
                .map(|(category, id, value)| (category, id.to_owned(), value))
                .collect::<Vec<_>>(),
            before,
            "an invalid later identity must not partially transfer the first value"
        );
    }
}

#[test]
fn rules_hash_and_saved_projection_cover_retained_values_and_global_type_order() {
    let with_current_value = |value| {
        let initial = RulesLayerStack::new(IniFile::from_str(
            "[AircraftTypes]\n0=ORCA\n[ORCA]\nTechLevel=3\n",
        ))
        .process()
        .unwrap();
        let mut state = registry(initial);
        state
            .consume_outgoing_type_tech_levels([(
                ObjectCategory::Aircraft,
                "ORCA".to_owned(),
                value,
            )])
            .unwrap();
        RulesLayerStack::new(IniFile::from_str("[ORCA]\nName=Harrier\n"))
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
            .unwrap()
    };
    let first = with_current_value(11);
    let second = with_current_value(13);
    assert_eq!(first.content_hash(), second.content_hash());
    let first_rules = RuleSet::from_processed_rules(&first).unwrap();
    assert_ne!(
        first_rules.simulation_config_hash(),
        RuleSet::from_processed_rules(&second)
            .unwrap()
            .simulation_config_hash(),
        "runtime handoff changes retained fields independently of the compatibility INI"
    );
    let restored: ProcessedRulesLayers =
        bincode::deserialize(&bincode::serialize(&first).unwrap()).unwrap();
    assert_eq!(
        RuleSet::from_processed_rules(&restored)
            .unwrap()
            .simulation_config_hash(),
        first_rules.simulation_config_hash()
    );

    let with_first_category = |registry_name| {
        let initial =
            RulesLayerStack::new(IniFile::from_str(&format!("[{registry_name}]\n0=SHARED\n")))
                .process()
                .unwrap();
        let processed = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=SHARED\n[BuildingTypes]\n0=SHARED\n[SHARED]\nTechLevel=3\n",
        ))
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), registry(initial))
        .unwrap();
        RuleSet::from_processed_rules(&processed).unwrap()
    };
    let vehicle_first = with_first_category("VehicleTypes");
    let building_first = with_first_category("BuildingTypes");
    assert_eq!(
        vehicle_first.source_ini_hash(),
        building_first.source_ini_hash()
    );
    assert_ne!(
        vehicle_first.simulation_config_hash(),
        building_first.simulation_config_hash(),
        "native106 can distinguish allocation histories with identical current projection text"
    );
    assert_eq!(
        vehicle_first
            .native_techno_type_exact("SHARED")
            .unwrap()
            .category,
        ObjectCategory::Building,
    );
    assert_eq!(
        building_first
            .native_techno_type_exact("SHARED")
            .unwrap()
            .category,
        ObjectCategory::Vehicle,
    );
}

#[test]
fn process_owner_accepts_retired_values_before_its_existing_full_init_reset() {
    let native = corpus();
    let root = IniFile::from_str(
        "[AircraftTypes]\n0=ORCA\n1=BEAG\n[ORCA]\nName=ORCA\n[BEAG]\nName=BEAG\n",
    );
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        None,
        IniFile::empty(),
        std::sync::Arc::default(),
    )
    .unwrap();
    owner
        .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &IniFile::empty())
        .unwrap();
    owner
        .consume_outgoing_type_tech_levels(
            stage(native, "actions")["types"]
                .as_array()
                .unwrap()
                .iter()
                .map(|ty| {
                    (
                        ObjectCategory::Aircraft,
                        ty["id"].as_str().unwrap().to_owned(),
                        number(&ty["tech_level"]),
                    )
                }),
        )
        .unwrap();
    assert!(
        owner.saved_scenario_rules().is_none(),
        "the outgoing scenario has retired"
    );
    let prefix = ordered_prefix(&native["lifecycle"]["input"]["prefix_passes"][0]["sections"]);
    let next = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(Some(&prefix)),
            &IniFile::empty(),
        )
        .unwrap();
    let rules = next.into_parts().0;
    for expected in stage(native, "new_constructors")["types"]
        .as_array()
        .unwrap()
    {
        assert_eq!(
            rules
                .native_techno_type_exact(expected["id"].as_str().unwrap())
                .unwrap()
                .tech_level,
            number(&expected["tech_level"])
        );
    }
}

#[test]
fn retail_tech_levels_read_all_reached_layers_through_the_production_owner() {
    let native = corpus();
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    for source in native["retail"]["sources"].as_array().unwrap() {
        let raw = assets.get_ref(source["file"].as_str().unwrap()).unwrap();
        assert_eq!(crate::util::sha256::sha256_hex(raw), source["sha256"]);
    }
    let sources = crate::rules::retail_sources::RetailRulesSources::select(&assets).unwrap();
    let root_rules = RuleSet::from_ini(&sources.rulesmd.ini).unwrap();
    let mut owner = sources.into_owner(std::sync::Arc::default()).unwrap();
    for route in native["retail"]["routes"].as_array().unwrap() {
        let map_name = route["map"].as_str().unwrap();
        let raw = assets.get_ref(map_name).unwrap();
        let map = crate::map::map_file::MapFile::from_bytes(raw).unwrap();
        let named_name = map_name.replace(".MAP", ".INI");
        let named = assets.resolve_ref(&named_name).map(|_| {
            crate::rules::retail_sources::select_ini(&assets, &named_name)
                .unwrap()
                .ini
        });
        let rules = owner
            .load_scenario(
                NativeScenarioRulesPrefix::Campaign(named.as_ref()),
                &map.ini,
            )
            .unwrap()
            .into_parts()
            .0;
        for row in native["scalar_layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["map"] == map_name)
        {
            let id = row["type"].as_str().unwrap();
            let passes = row["passes"].as_array().unwrap();
            assert_eq!(
                root_rules.native_techno_type_exact(id).unwrap().tech_level,
                number(&passes[0]["value"])
            );
            assert_eq!(
                rules.native_techno_type_exact(id).unwrap().tech_level,
                number(&passes.last().unwrap()["value"]),
                "{map_name}/{id}: physical Rules+optional prefixes+map"
            );
        }
    }
}
