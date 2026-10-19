//! Execute106 controls and the shared current-type state used downstream.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::rng::SimRng;
use serde_json::{Value, json};

fn corpus() -> &'static Value {
    let data = crate::test_fixture::json("tools/spatial_oracle/trigger_object_tech_level.json");
    assert_eq!(
        data["native_sha256"],
        "3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600"
    );
    data
}

fn number(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn category(value: &Value) -> ObjectCategory {
    match value.as_str().unwrap() {
        "Vehicle" => ObjectCategory::Vehicle,
        "Infantry" => ObjectCategory::Infantry,
        "Aircraft" => ObjectCategory::Aircraft,
        "Building" => ObjectCategory::Building,
        other => panic!("unrecognized original fixture category {other}"),
    }
}

fn registry(category: ObjectCategory) -> &'static str {
    match category {
        ObjectCategory::Vehicle => "VehicleTypes",
        ObjectCategory::Infantry => "InfantryTypes",
        ObjectCategory::Aircraft => "AircraftTypes",
        ObjectCategory::Building => "BuildingTypes",
    }
}

fn rules_for_types(types: &[Value]) -> RuleSet {
    let mut layers = RulesLayerStack::new(IniFile::empty());
    // One first allocation per pass retains the native control's global
    // constructor order, including lazy/cross-category equal IDs.
    for ty in types {
        let id = ty["id"].as_str().unwrap();
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(&format!(
                "[{}]\n0={id}\n[{id}]\nTechLevel={}\n",
                registry(category(&ty["category"])),
                number(&ty["tech_level"])
            )),
        );
    }
    RuleSet::from_rules_layers(&layers).unwrap()
}

fn rng_hash(hex: &str) -> String {
    let bytes: Vec<_> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    crate::util::sha256::sha256_hex(&bytes)
}

fn rng_hashes(sim: &Simulation) -> Value {
    json!({
        "scenario": rng_hash(&sim.scenario_rng.native_state_hex()),
        "main": rng_hash(&sim.main_rng.native_state_hex()),
        "mapgen": rng_hash(&sim.mapgen_rng.native_state_hex()),
    })
}

fn apply(sim: &mut Simulation, rules: &RuleSet, raw: &str) -> bool {
    let entry = crate::map::actions::parse_actions(&IniFile::from_str(&format!(
        "[Actions]\nTEST=1,{raw}\n"
    )))
    .remove("TEST")
    .unwrap()
    .entries()[0]
        .clone();
    let action = RuntimeDefinitions::default().resolve_action(entry, None, None);
    sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: None,
            incoming_house: None,
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        Some(rules),
        None,
        FrameEffects::default(),
    )
}

#[test]
fn type_tech_level_dispatch_matches_original_identity_signed_values_house_dirty_and_rng() {
    let data = corpus();
    for row in data["dispatch_controls"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        let input = &row["input"];
        let types = input["types"].as_array().unwrap();
        let rules = rules_for_types(types);
        let mut sim = Simulation::with_seed(0);
        for ty in types {
            let object = rules
                .object_in_category(category(&ty["category"]), ty["id"].as_str().unwrap())
                .unwrap();
            sim.supply_object_tech_level_for_test(object, number(&ty["tech_level"]));
        }
        let mut owners = Vec::new();
        for (index, dirty) in input["house_dirty"].as_array().unwrap().iter().enumerate() {
            let owner = sim.interner.intern(&format!("House{index}"));
            let mut house = HouseState::new(owner, 0, None, false, 0, 10);
            if !dirty.as_bool().unwrap() {
                assert!(house.take_tech_recheck());
            }
            sim.houses.insert(owner, house);
            sim.session.house_order.push(owner);
            owners.push(owner);
        }
        let frozen_houses: Vec<_> = owners
            .iter()
            .map(|owner| serde_json::to_value(&sim.houses[owner]).unwrap())
            .collect();
        let seed = row["rng_seed"].as_u64().unwrap();
        sim.scenario_rng = SimRng::new(seed);
        sim.main_rng = SimRng::new(seed).into();
        sim.mapgen_rng = SimRng::new(seed);
        assert_eq!(rng_hashes(&sim), row["rng_before"], "{name}");
        let raw = input["raw_action"].as_str().unwrap();
        let (returned, draws) = crate::sim::rng::trace_draws(|| apply(&mut sim, &rules, raw));
        assert_eq!(
            i32::from(returned),
            number(&row["output"]["returned"]),
            "{name}"
        );
        assert!(draws.is_empty(), "{name}: action106 has no draws");
        assert_eq!(rng_hashes(&sim), row["rng_after"], "{name}");
        for (index, ty) in types.iter().enumerate() {
            let object = rules
                .object_in_category(category(&ty["category"]), ty["id"].as_str().unwrap())
                .unwrap();
            assert_eq!(
                sim.object_tech_level(object),
                number(&row["output"]["tech_levels"][index]),
                "{name}: type{index}"
            );
        }
        for (index, owner) in owners.iter().enumerate() {
            let expected = row["output"]["house_dirty"][index].as_bool().unwrap();
            assert_eq!(sim.houses[owner].tech_recheck_pending(), expected, "{name}");
            let mut prior = frozen_houses[index].clone();
            prior["tech_recheck"] = json!(expected);
            assert_eq!(
                serde_json::to_value(&sim.houses[owner]).unwrap(),
                prior,
                "{name}"
            );
        }
    }
}

#[test]
fn current_type_values_roundtrip_hash_by_value_and_transfer_only_at_retirement() {
    let data = corpus();
    let types = data["lifecycle"]["stages"][0]["types"].as_array().unwrap();
    let rules = rules_for_types(types);
    let actions = data["lifecycle"]["input"]["raw_actions"]
        .as_array()
        .unwrap();
    let mut sim = Simulation::with_seed(0);
    let constructor_hash = sim.state_hash();
    for raw in actions {
        assert!(apply(&mut sim, &rules, raw.as_str().unwrap()));
    }
    let modified_hash = sim.state_hash();
    assert_ne!(constructor_hash, modified_hash);
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "TechLevel", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    restored.validate_type_tech_levels(&rules).unwrap();
    assert_eq!(restored.state_hash(), modified_hash);
    let mut reversed = Simulation::with_seed(0);
    for raw in actions.iter().rev() {
        assert!(apply(&mut reversed, &rules, raw.as_str().unwrap()));
    }
    assert_eq!(
        reversed.state_hash(),
        modified_hash,
        "maps hash values, not write order"
    );
    let changes = restored.retire_object_type_tech_levels();
    let expected = &data["lifecycle"]["stages"][1]["types"];
    assert_eq!(changes.len(), expected.as_array().unwrap().len());
    for (category, id, value) in changes {
        let original = expected
            .as_array()
            .unwrap()
            .iter()
            .find(|ty| ty["id"] == id)
            .unwrap();
        assert_eq!(category, ObjectCategory::Aircraft);
        assert_eq!(value, number(&original["tech_level"]));
    }
    assert_eq!(restored.state_hash(), constructor_hash);
}

#[test]
fn type_tech_level_snapshot_rejects_an_unresolved_constructor_identity() {
    let rules = rules_for_types(&[]);
    let mut sim = Simulation::with_seed(0);
    let object = crate::rules::object_type::ObjectType::from_ini_section(
        "MissingNativeType",
        &crate::rules::ini_parser::IniSection::new("MissingNativeType".to_string()),
        ObjectCategory::Vehicle,
    );
    sim.supply_object_tech_level_for_test(&object, 1);
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "TechLevel", 0);
    let restored = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    assert!(matches!(
        restored.validate_type_tech_levels(&rules),
        Err(crate::sim::snapshot::SnapshotRestoreError::InvalidTypeTechLevels { .. })
    ));
}

#[path = "trigger_runtime_type_tech_level_production_tests.rs"]
mod production_tests;
