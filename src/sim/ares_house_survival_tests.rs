//! Production reader/lifecycle regressions; executable Ares controls live with
//! the existing house_defeat_gate oracle. These tests do not certify whole-mod
//! or downstream Blowup_All parity.

use super::HouseTracking;
use crate::rules::ARES_ENABLED;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::Simulation;
use crate::util::save_codec;

fn native_corpus() -> &'static serde_json::Value {
    crate::test_fixture::json("tools/spatial_oracle/ares_house_survival.json")
}

#[test]
fn keep_alive_physical_reader_matches_executed_nullable_histories() {
    let corpus = native_corpus();
    let history = &corpus["reader_histories"][0];
    for family in [
        "BuildingTypes",
        "VehicleTypes",
        "InfantryTypes",
        "AircraftTypes",
    ] {
        let mut layers =
            RulesLayerStack::new(IniFile::from_str(&format!("[{family}]\n0=CONTROL\n")));
        for step in history["steps"].as_array().unwrap() {
            layers.push(
                RulesLayerKind::Scenario,
                IniFile::from_str(step["physical_ini"].as_str().unwrap()),
            );
            let rules = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
            let expected =
                (ARES_ENABLED && step["after"]["is_set"] == 1).then(|| step["after"]["value"] == 1);
            assert_eq!(
                rules.object("CONTROL").unwrap().keep_alive(),
                expected,
                "{family}: {}",
                step["file"]
            );
        }
    }
}

#[cfg(feature = "ares")]
#[test]
fn keep_alive_tracking_matches_executed_defaults_overrides_and_wrapping() {
    use crate::map::entities::EntityCategory;
    for row in native_corpus()["tracking_histories"].as_array().unwrap() {
        let input = &row["input"];
        let building = input["class"] == "building";
        let family = if building {
            "BuildingTypes"
        } else {
            "VehicleTypes"
        };
        let mut text = format!("[{family}]\n0=CONTROL\n[CONTROL]\nStrength=100\n");
        for (flag, key) in [
            ("insignificant", "Insignificant"),
            ("dont_score", "DontScore"),
        ] {
            if input[flag] == true {
                text.push_str(&format!("{key}=yes\n"));
            }
        }
        if let Some(value) = input["keep_alive"].as_bool() {
            text.push_str(&format!("KeepAlive={}\n", if value { "yes" } else { "no" }));
        }
        if input["unit_like"] == true {
            text.push_str(
                "UndeploysInto=PAYLOAD\n[VehicleTypes]\n0=PAYLOAD\n[PAYLOAD]\nStrength=100\n",
            );
        }
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(&text),
            &IniFile::from_str(if input["unit_like"] == true {
                "[CONTROL]\nFoundation=1x1\n"
            } else {
                "[CONTROL]\nFoundation=2x2\n"
            }),
        )
        .unwrap();
        let mut sim = Simulation::with_seed(3);
        let owner = sim.interner.intern("Owner");
        sim.houses
            .register(HouseState::new(owner, 0, None, true, 0, 10));
        let id = sim
            .spawn_object_limbo_at_height("CONTROL", "Owner", 10, 10, 0, 0, &rules)
            .unwrap();
        let mut entity = sim.substrate.entities.get(id).unwrap().clone();
        let mut tracking = HouseTracking {
            keep_alive: input["start"].as_i64().unwrap_or(0) as i32,
            ..Default::default()
        };
        for step in row["steps"].as_array().unwrap() {
            match step["operation"].as_str().unwrap() {
                "gain" => tracking.add_tracking(&mut entity),
                "loss" => tracking.remove_tracking(&mut entity),
                operation => panic!("unknown native operation {operation}"),
            }
            let expected = &step["after"];
            let int = |key: &str| expected[key].as_i64().unwrap() as i32;
            assert_eq!(
                (
                    tracking.keep_alive,
                    tracking.buildings(),
                    tracking.vehicles(),
                    tracking.owned_count(EntityCategory::Unit, entity.type_ref()),
                    tracking.owned_count(EntityCategory::Structure, entity.type_ref())
                ),
                (
                    int("keep_alive"),
                    int("buildings"),
                    int("vehicles"),
                    int("unit_type"),
                    int("building_type")
                ),
                "{}: {}",
                input["name"],
                step["operation"],
            );
        }
    }
}

#[cfg(feature = "ares")]
#[test]
fn short_game_gate_matches_executed_full_vector_and_nonzero_controls() {
    let corpus = native_corpus();
    for row in corpus["survival_cases"].as_array().unwrap() {
        let input = &row["input"];
        if input["short_game"] == false || input["passive"] == true {
            continue;
        }
        let mut sim = Simulation::new();
        let ids: Vec<_> = (0..32)
            .map(|index| sim.interner.intern(&format!("TYPE{index}")))
            .collect();
        let mut tracking = HouseTracking {
            keep_alive: input["keep_alive_count"].as_i64().unwrap_or(0) as i32,
            buildings: input["buildings"].as_i64().unwrap_or(0) as i32,
            ..Default::default()
        };
        let capacity = input["capacity"].as_u64().unwrap_or(16) as usize;
        for (index, count) in input["counts"].as_object().unwrap() {
            let index: usize = index.parse().unwrap();
            if index < capacity {
                tracking
                    .unit_types
                    .insert(ids[index], count.as_i64().unwrap() as i32);
            }
        }
        let base: Vec<_> = input["base_indices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|index| Some(ids[index.as_u64().unwrap() as usize]))
            .collect();
        assert_eq!(
            tracking.game_alive(true, base, None, |_| panic!(
                "short game must not inspect Foot arrays"
            )),
            row["events"].as_array().unwrap().is_empty(),
            "{}",
            input["name"]
        );
    }
}

#[test]
fn keep_alive_retains_nullable_input_through_rules_registry_save() {
    let resume = |setting: Option<&str>| {
        let row = setting.map_or_else(String::new, |value| format!("KeepAlive={value}\n"));
        let initial = RulesLayerStack::new(IniFile::from_str(&format!(
            "[VehicleTypes]\n0=CONTROL\n[CONTROL]\n{row}",
        )))
        .process()
        .unwrap();
        let (_, trace) = initial.into_ini_and_native_type_construction_trace();
        let registry = trace.into_registry_state_discarding_events();
        let registry = save_codec::deserialize(&save_codec::serialize(&registry).unwrap()).unwrap();
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=CONTROL\n[CONTROL]\nKeepAlive=<none>\n",
        ));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[CONTROL]\nName=Retained\n"),
        );
        let resumed = layers
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), registry)
            .unwrap();
        RuleSet::from_processed_rules(&resumed).unwrap()
    };
    let absent = resume(None);
    let yes = resume(Some("yes"));
    let no = resume(Some("none"));
    assert_eq!(absent.object("CONTROL").unwrap().keep_alive(), None);
    assert_eq!(
        yes.object("CONTROL").unwrap().keep_alive(),
        ARES_ENABLED.then_some(true)
    );
    assert_eq!(
        no.object("CONTROL").unwrap().keep_alive(),
        ARES_ENABLED.then_some(false)
    );
    assert_eq!(
        yes.simulation_config_hash() != no.simulation_config_hash(),
        ARES_ENABLED,
        "identical current layers retain the earlier effective input",
    );
    assert_eq!(
        absent.simulation_config_hash() != no.simulation_config_hash(),
        ARES_ENABLED,
        "unset and explicit false are distinct type state",
    );
}

#[test]
fn keep_alive_limbo_capture_reload_and_destruction_use_the_tracking_owner() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=BEACON\n[BEACON]\nStrength=100\n\
         Insignificant=yes\nKeepAlive=yes\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(3);
    let first = sim.interner.intern("First");
    let second = sim.interner.intern("Second");
    for (index, owner) in [first, second].into_iter().enumerate() {
        sim.houses
            .register(HouseState::new(owner, index as i32, None, true, 0, 10));
    }
    let beacon = sim
        .spawn_object_limbo_at_height("BEACON", "First", 10, 10, 0, 0, &rules)
        .unwrap();
    let survives = |tracking: &HouseTracking| tracking.game_alive(true, [], None, |_| false);
    assert_eq!(survives(sim.houses[&first].tracking()), ARES_ENABLED);
    assert_eq!(sim.houses[&first].tracking().units_for_test(), 0);
    #[cfg(feature = "ares")]
    assert_eq!(
        sim.houses[&first].tracking().keep_alive as i64,
        native_corpus()["capture"]["before"]["old"]["keep_alive"]
            .as_i64()
            .unwrap()
    );
    sim.change_owner(beacon, second);
    assert!(!survives(sim.houses[&first].tracking()));
    assert_eq!(survives(sim.houses[&second].tracking()), ARES_ENABLED);
    #[cfg(feature = "ares")]
    {
        let capture = &native_corpus()["capture"]["after"];
        assert_eq!(
            sim.houses[&first].tracking().keep_alive as i64,
            capture["old"]["keep_alive"].as_i64().unwrap()
        );
        assert_eq!(
            sim.houses[&second].tracking().keep_alive as i64,
            capture["new"]["keep_alive"].as_i64().unwrap()
        );
    }

    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let before = sim.state_hash();
    let saved = GameSnapshot::save(&sim, 0, 0, "keep-alive", 0);
    let mut restored = GameSnapshot::load(&saved).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.state_hash(), before);
    assert_eq!(survives(restored.houses[&second].tracking()), ARES_ENABLED);
    for candidate in [&mut sim, &mut restored] {
        candidate.uninit_with_rules(beacon, &rules);
        candidate.flush_pending_delete();
        assert!(!survives(candidate.houses[&second].tracking()));
    }
    assert_eq!(sim.state_hash(), restored.state_hash());
}

#[cfg(feature = "ares")]
#[test]
fn keep_alive_counter_and_future_removal_input_are_hashed_independently() {
    use crate::sim::state_hasher::StateHasher;
    use std::hash::Hasher;

    let hash = |tracking: &HouseTracking| {
        let mut hasher = StateHasher::new();
        tracking.hash_defeat_counters(&mut hasher);
        hasher.finish()
    };
    let mut tracking = HouseTracking::default();
    let empty = hash(&tracking);
    // The executed stream's event-history prior is never rebuilt from objects.
    let corpus = native_corpus();
    let stream = &corpus["stream_cases"][1];
    tracking.keep_alive = stream["input"]["keep_alive_count"].as_i64().unwrap() as i32;
    let negative = hash(&tracking);
    assert_ne!(negative, empty);
    let restored: HouseTracking =
        save_codec::deserialize(&save_codec::serialize(&tracking).unwrap()).unwrap();
    assert_eq!(
        restored.keep_alive as i64,
        stream["restored"]["keep_alive_count"].as_i64().unwrap()
    );
    assert_eq!(hash(&restored), negative);
    assert!(restored.game_alive(true, [], None, |_| panic!("KeepAlive precedes scans")));

    let input_hash = |keep_alive| {
        let mut hasher = StateHasher::new();
        super::TrackingFacts {
            keep_alive,
            ..Default::default()
        }
        .hash_retained_inputs(&mut hasher);
        hasher.finish()
    };
    assert_ne!(input_hash(true), input_hash(false));
}

#[cfg(feature = "ares")]
#[test]
fn keep_alive_precedes_normal_class_total_and_presence_queries() {
    let corpus = native_corpus();
    let row = corpus["survival_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "normal_keep_alive_first")
        .unwrap();
    let mut tracking = HouseTracking {
        keep_alive: row["input"]["keep_alive_count"].as_i64().unwrap() as i32,
        ..Default::default()
    };
    tracking.set_active_units_for_test(row["input"]["active_units"].as_i64().unwrap() as i32);
    assert_eq!(
        tracking.game_alive(false, [], None, |_| panic!(
            "KeepAlive must precede object scans"
        )),
        row["events"].as_array().unwrap().is_empty()
    );
}
