//! Original VoiceSelect708EB0 and QueueVoice708D90 compared through the
//! existing Simulation Main owner, including terrain-prefix continuation.

use super::Simulation;
use crate::sim::rng::trace_draws;

fn sound_name(id: i64) -> String {
    if id == -1 {
        String::new()
    } else {
        format!("Sound{id}")
    }
}

#[test]
fn normal_voice_requests_match_native_rejections_draws_and_complete_main_state() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/selection_navigation.json",
    ))
    .unwrap();
    let mut histories = 0;
    let mut calls = 0;
    let mut separate_receivers = 0;
    for history in native["voice_histories"].as_array().unwrap() {
        if history["slave"] == true || history["robot_offline"] == true {
            separate_receivers += 1;
            continue;
        }
        let label = history["id"].as_str().unwrap();
        let voices: Vec<_> = history["voice_list"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| sound_name(id.as_i64().unwrap()))
            .collect();
        let mut sim = Simulation::with_seed(history["seed"].as_u64().unwrap());
        if let Some(prefix) = history["main_prefix"]["draws"].as_array() {
            assert_eq!(
                sim.main_rng.native_state_hex(),
                history["main_prefix"]["before_state_hex"].as_str().unwrap(),
                "{label}"
            );
            let (_, mut main) = sim.terrain_load_draws();
            for expected in prefix {
                assert_eq!(u64::from(main.next_u32()), expected.as_u64().unwrap());
            }
        }
        assert_eq!(
            sim.main_rng.native_state_hex(),
            history["rng_before_hex"]["main"].as_str().unwrap(),
            "{label}"
        );
        let scenario_before = sim.scenario_rng.logical_state();
        let mapgen_before = sim.mapgen_rng.logical_state();
        let hash_before = sim.state_hash();
        let mut queued = sound_name(history["initial_queued_voice"].as_i64().unwrap());
        for step in history["steps"].as_array().unwrap() {
            let main_before = sim.main_rng.logical_state();
            let (request, draws) = trace_draws(|| {
                sim.voice_request_from_list(
                    &voices,
                    history["voice_enabled"].as_bool().unwrap_or(true),
                    history["owner"].as_str().unwrap_or("local") == "local",
                )
            });
            if let Some(sound_id) = request {
                assert_eq!(
                    sound_id,
                    sound_name(step["voice_requests"][0]["sound_id"].as_i64().unwrap()),
                    "{label}"
                );
                queued = sound_id.to_string();
            }
            assert_eq!(
                queued,
                sound_name(step["queued_voices"]["1"].as_i64().unwrap()),
                "{label}"
            );
            assert_eq!(
                draws
                    .iter()
                    .map(|draw| draw["value"].clone())
                    .collect::<Vec<_>>(),
                *step["main_draws"].as_array().unwrap(),
                "{label}"
            );
            assert_eq!(
                main_before == sim.main_rng.logical_state(),
                step["rng_unchanged"]["main"].as_bool().unwrap(),
                "{label}"
            );
            assert_eq!(sim.scenario_rng.logical_state(), scenario_before, "{label}");
            assert_eq!(sim.mapgen_rng.logical_state(), mapgen_before, "{label}");
            assert_eq!(sim.state_hash(), hash_before, "{label}");
            assert_eq!(step["rng_unchanged"]["scenario"], true, "{label}");
            assert_eq!(step["rng_unchanged"]["mapgen"], true, "{label}");
            calls += 1;
        }
        assert_eq!(
            sim.main_rng.native_state_hex(),
            history["rng_after_hex"]["main"].as_str().unwrap(),
            "{label}"
        );
        for expected in history["main_next_four"].as_array().unwrap() {
            assert_eq!(
                u64::from(sim.main_rng.next_u32()),
                expected.as_u64().unwrap(),
                "{label}"
            );
        }
        histories += 1;
    }
    assert_eq!((histories, calls, separate_receivers), (25, 96, 4));
}

#[test]
fn area_guard_default_voice_matches_native_pre_gate_post_gate_and_main_continuation() {
    use crate::map::entities::EntityCategory;
    use crate::rules::area_guard_tests as native;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::house_state::HouseState;
    use crate::sim::rng::SimRng;
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};

    let corpus = native::native();
    let mut covered = BTreeSet::new();
    let mut count = 0;
    for row in corpus["histories"]["rows"].as_array().unwrap() {
        let receipts = row["voice_receipts"].as_array().unwrap();
        let Some(first) = receipts.first() else {
            continue;
        };
        let rules = native::fixture_rules(&corpus, &row["input"]);
        let mut sim = Simulation::new();
        let current_pointer = first["current_house"].as_u64().unwrap();
        let current = sim
            .interner
            .intern(&format!("NativeHouse{current_pointer}"));
        sim.session.current_house = Some(current);
        sim.session.game_mode_nonzero = first["game_mode"].as_u64().unwrap() != 0;
        let initial_actors = row["boundaries"][0]["before"]["actors"]
            .as_object()
            .unwrap();
        let mut ids = BTreeMap::new();
        let mut pending: BTreeMap<String, Option<String>> = BTreeMap::new();
        for (name, actor) in initial_actors {
            let pointer = actor["owner"].as_u64().unwrap();
            let owner = sim.interner.intern(&format!("NativeHouse{pointer}"));
            // These flags are supplied original input state, not a replay of
            // House startup or the separately owned control-transfer lifecycle.
            let source = receipts
                .iter()
                .find(|voice| voice["owner"].as_u64() == Some(pointer));
            let human = source
                .map_or(&actor["human"], |voice| &voice["human"])
                .as_u64()
                .unwrap()
                != 0;
            let control = source
                .map_or(&actor["player_control"], |voice| &voice["player_control"])
                .as_u64()
                .unwrap()
                != 0;
            let mut house = HouseState::new(owner, 0, None, human, 0, 10);
            house.player_control = control;
            sim.houses.insert(owner, house);
            let type_name = actor["type_name"].as_str().unwrap();
            let type_ref = sim.interner.intern(type_name);
            let id = actor["id"].as_u64().unwrap();
            let category = if type_name == "E1" {
                EntityCategory::Infantry
            } else {
                assert_eq!(type_name, "MTNK");
                EntityCategory::Unit
            };
            sim.entities_mut()
                .insert(GameEntity::new_at_frame_zero_for_test(
                    id,
                    0,
                    0,
                    0,
                    0,
                    owner,
                    Health { current: 100 },
                    type_ref,
                    category,
                    0,
                    5,
                    false,
                ));
            ids.insert(name.clone(), id);
            pending.insert(name.clone(), None);
        }
        let state = |receipt: &serde_json::Value, side: &str, stream: &str| {
            let reference = receipt[side][stream].as_str().unwrap();
            row["complete_rng_states"][reference]["bytes"]
                .as_str()
                .unwrap()
        };
        sim.main_rng = SimRng::from_native_state_hex_for_test(state(first, "rng_before", "main"));
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(state(first, "rng_before", "scenario"));
        sim.mapgen_rng =
            SimRng::from_native_state_hex_for_test(state(first, "rng_before", "mapgen"));
        for receipt in receipts {
            let name = receipt["actor"].as_str().unwrap();
            let id = ids[name];
            let type_name = sim
                .interner
                .resolve(sim.entities().get(id).unwrap().type_ref());
            assert_eq!(
                json!(rules.object(type_name).unwrap().voice_special_attack),
                receipt["vector"]["names"]
            );
            assert_eq!(
                pending[name].as_deref(),
                receipt["queued_before"]["name"].as_str()
            );
            for (stream, rng) in [
                ("main", &sim.main_rng),
                ("scenario", &sim.scenario_rng),
                ("mapgen", &sim.mapgen_rng),
            ] {
                assert_eq!(
                    rng.native_state_hex(),
                    state(receipt, "rng_before", stream),
                    "{} before {name} {stream}",
                    row["name"]
                );
            }
            let enabled = receipt["voice_enabled"].as_u64().unwrap() != 0;
            let (request, draws) =
                trace_draws(|| sim.default_order_voice_request(&rules, id, enabled));
            if let Some(sound) = request {
                assert!(
                    receipt["queue_calls"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|call| call["name"] == sound)
                );
                pending.insert(name.to_string(), Some(sound.to_string()));
            }
            assert_eq!(
                pending[name].as_deref(),
                receipt["queued_after"]["name"].as_str(),
                "{}: {name} QueueVoice admission",
                row["name"]
            );
            assert_eq!(
                json!(draws.iter().map(|draw| &draw["value"]).collect::<Vec<_>>()),
                receipt["main_raw"],
                "{}: {name} original raw draws",
                row["name"]
            );
            for (stream, rng) in [
                ("main", &sim.main_rng),
                ("scenario", &sim.scenario_rng),
                ("mapgen", &sim.mapgen_rng),
            ] {
                assert_eq!(
                    rng.native_state_hex(),
                    state(receipt, "rng_after", stream),
                    "{} after {name} {stream}",
                    row["name"]
                );
            }
            count += 1;
        }
        covered.insert(row["name"].as_str().unwrap());
    }
    for name in [
        "stock_mtnk_current_cell",
        "stock_e1_current_cell",
        "voices_disabled",
        "direct_noncontrolled_default_voice",
        "ordered_authored_voice_lists",
        "repeated_voice_pending_overwrite",
    ] {
        assert!(
            covered.contains(name),
            "missing native voice control {name}"
        );
    }
    assert_eq!(
        count, 25,
        "all reached native default-voice calls are compared"
    );
}
