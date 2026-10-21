//! Original Scenario.Side / cached Vox / dequeue history. The native controls
//! stop before PlayFile and do not establish hardware or time-gate parity.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::{snapshot::GameSnapshot, world::Simulation};
use serde_json::{Value, json};

fn native() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/eva_selection.json",
    ))
    .unwrap()
}

fn registry_from(physical: &Value) -> EvaRegistry {
    let mut text = String::new();
    for (section, fields) in physical.as_object().unwrap() {
        text.push_str(&format!("[{section}]\n"));
        for (key, value) in fields.as_object().unwrap() {
            text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
    }
    EvaRegistry::from_ini(&IniFile::from_str(&text))
}

fn node_image(node: &VoxNode) -> Value {
    json!({"event": node.event, "priority": node.priority as i32,
        "type": node.eva_type as i32, "sequence": node.seq})
}

fn canonical_state(mut state: Value) -> Value {
    fn node(value: &mut Value) {
        if let Some(name) = value["event"].as_str() {
            value["event"] = Value::String(name.to_ascii_uppercase());
        }
    }
    node(&mut state["pending"]);
    for name in ["critical", "interrupts"] {
        for entry in state[name].as_array_mut().unwrap() {
            node(entry);
        }
    }
    for list in state["queues"].as_object_mut().unwrap().values_mut() {
        for entry in list.as_array_mut().unwrap() {
            node(entry);
        }
    }
    state
}

fn image(queue: &VoxQueue, sim: &Simulation, registry: &EvaRegistry, probe: &VoxNode) -> Value {
    json!({
        "scenario_side": sim.session().scenario_side(),
        "selected_voice": queue.selected_voice(),
        "lookup_filename": queue.sample_for(probe, registry).unwrap_or(""),
        "entry_state": queue.entry_states.get(&probe.event).copied().unwrap_or(2),
        "pending": queue.pending.as_ref().map(node_image),
        "critical": queue.critical_list.iter().map(node_image).collect::<Vec<_>>(),
        "interrupts": queue.interrupt_list.iter().map(node_image).collect::<Vec<_>>(),
        "queues": (0..4).map(|index| (index.to_string(),
            queue.priority_lists[index].iter().map(node_image).collect::<Vec<_>>()))
            .collect::<std::collections::BTreeMap<_, _>>(),
        "sequence": queue.seq_counter,
        "pause_depth": queue.pause_depth,
        "suspend_depth": queue.suspend_depth,
    })
}

#[test]
fn eva_selection_matches_original_cache_queue_and_restored_side_history() {
    let corpus = native();
    let registry = registry_from(&corpus["synthetic_ini"]);
    let name = corpus["synthetic_ini"]["DialogList"]["0"].as_str().unwrap();
    let entry = registry.entry(name).unwrap();
    let probe = VoxNode {
        event: entry.name.to_ascii_uppercase(),
        eva_type: entry.eva_type,
        priority: entry.priority,
        seq: 0,
    };
    let mut sim = Simulation::with_seed(31);
    let rng = sim.rng_state();
    let mut queue = VoxQueue::new();
    assert_eq!(
        image(&queue, &sim, &registry, &probe),
        canonical_state(corpus["cold"].clone())
    );
    for step in corpus["history"].as_array().unwrap() {
        let label = step["label"].as_str().unwrap();
        assert_eq!(
            image(&queue, &sim, &registry, &probe),
            canonical_state(step["before"].clone()),
            "before {label}"
        );
        let operation = &step["operation"];
        match operation["kind"].as_str().unwrap() {
            "bind" => queue.set_side(
                operation["side"].as_i64().unwrap() as i32,
                &crate::rules::ruleset::RuleSet::from_ini(&IniFile::empty()).unwrap(),
            ),
            "queue" => {
                let entry = registry
                    .entry(operation["event"].as_str().unwrap())
                    .unwrap();
                queue.queue_voice(
                    VoxRequest {
                        event: &entry.name,
                        eva_type: entry.eva_type,
                        priority: entry.priority,
                    },
                    None,
                    None,
                );
            }
            "dequeue_admitted" => {
                let node = queue.take_next(1, false).unwrap();
                assert_eq!(
                    format!("{}.WAV", queue.sample_for(&node, &registry).unwrap_or("")),
                    step["dequeue_filename"].as_str().unwrap(),
                    "{label}"
                );
            }
            "reset" => {
                queue.reset_all();
            }
            "scenario_side_input" => sim
                .session_mut_for_test()
                .set_scenario_side_for_test(operation["side"].as_i64().unwrap() as i32),
            "scenario_default_store" => sim.session_mut_for_test().set_scenario_side_for_test(0),
            "bind_restored" => {
                let row = &corpus["scenario_round_trips"]
                    [operation["round_trip_index"].as_u64().unwrap() as usize];
                let mut saved = Simulation::new();
                saved
                    .session_mut_for_test()
                    .set_scenario_side_for_test(row["input_side"].as_i64().unwrap() as i32);
                let bytes = GameSnapshot::save(&saved, 0, 0, "eva-selection", 0);
                saved.session_mut_for_test().set_scenario_side_for_test(
                    row["overwritten_before_load"].as_i64().unwrap() as i32,
                );
                let loaded = GameSnapshot::load(&bytes).unwrap().sim;
                assert_eq!(
                    loaded.session().scenario_side(),
                    row["restored_side"].as_i64().unwrap() as i32
                );
                queue.set_side(
                    loaded.session().scenario_side(),
                    &crate::rules::ruleset::RuleSet::from_ini(&IniFile::empty()).unwrap(),
                );
            }
            other => panic!("unhandled native operation {other}"),
        }
        assert_eq!(
            image(&queue, &sim, &registry, &probe),
            canonical_state(step["after"].clone()),
            "after {label}"
        );
        assert_eq!(sim.rng_state(), rng, "{label}");
        assert_eq!(step["rng_unchanged"], true);
        assert!(step["rng_entries"].as_array().unwrap().is_empty());
    }
}

#[test]
fn eva_selection_distinguishes_pending_preemption_from_queue_cleanup() {
    let corpus = native();
    for row in corpus["pending_eviction_controls"].as_array().unwrap() {
        let registry = registry_from(&row["synthetic_ini"]);
        let mut queue = VoxQueue::new();
        queue.set_side(
            row["before"]["selected_voice"].as_i64().unwrap() as i32,
            &crate::rules::ruleset::RuleSet::from_ini(&IniFile::empty()).unwrap(),
        );
        for request in row["queue_inputs"].as_array().unwrap() {
            assert_eq!(request["type_override"], -1);
            assert_eq!(request["priority_override"], -1);
            let entry = registry.entry(request["event"].as_str().unwrap()).unwrap();
            queue.queue_voice(
                VoxRequest {
                    event: &entry.name,
                    eva_type: entry.eva_type,
                    priority: entry.priority,
                },
                None,
                None,
            );
        }
        let image = |queue: &VoxQueue| {
            let entries = row["before"]["entries"].as_object().unwrap().keys().map(|name| {
                let entry = registry.entry(name).unwrap();
                (name.clone(), json!({"type": entry.eva_type as i32,
                    "priority": entry.priority as i32,
                    "state": queue.entry_states.get(&name.to_ascii_uppercase()).copied().unwrap_or(2)}))
            }).collect::<std::collections::BTreeMap<_,_>>();
            json!({"entries": entries, "selected_voice": queue.selected_voice(),
                "pending": queue.pending.as_ref().map(node_image),
                "critical": queue.critical_list.iter().map(node_image).collect::<Vec<_>>(),
                "interrupts": queue.interrupt_list.iter().map(node_image).collect::<Vec<_>>(),
                "queues": (0..4).map(|index| (index.to_string(),
                    queue.priority_lists[index].iter().map(node_image).collect::<Vec<_>>()))
                    .collect::<std::collections::BTreeMap<_,_>>(),
                "sequence": queue.seq_counter, "pause_depth": queue.pause_depth,
                "suspend_depth": queue.suspend_depth})
        };
        assert_eq!(
            image(&queue),
            canonical_state(row["before"].clone()),
            "before {}",
            row["label"]
        );
        match row["operation"].as_str().unwrap() {
            "dequeue_admitted" => {
                let node = queue.take_next(1, false).unwrap();
                assert_eq!(
                    node.event,
                    row["selected_event"].as_str().unwrap().to_ascii_uppercase()
                );
                assert_eq!(
                    format!("{}.WAV", queue.sample_for(&node, &registry).unwrap_or("")),
                    row["dequeue_filename"].as_str().unwrap()
                );
            }
            "clear_all_queues" => queue.clear_all_queues(),
            "reset" => {
                queue.reset_all();
            }
            other => panic!("unhandled native cleanup {other}"),
        }
        assert_eq!(
            image(&queue),
            canonical_state(row["after"].clone()),
            "after {}",
            row["label"]
        );
        assert_eq!(row["rng_unchanged"], true);
        assert!(row["rng_entries"].as_array().unwrap().is_empty());
    }
}
