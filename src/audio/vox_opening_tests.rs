//! Executed original QueueVoice752480 histories. Queue structure and native
//! sequence are compared after every call; supplied pause/current controls
//! exclude hardware completion, entry-state diagnostics and opening timing.

use super::*;
use crate::rules::sound_ini::EvaRegistry;
use serde_json::{Value, json};

fn corpus() -> Vec<Value> {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_action_values.json",
    ))
    .unwrap()
}

fn node_image(node: &VoxNode) -> Value {
    json!({
        "event": node.event,
        "priority": node.priority as i32,
        "type": node.eva_type as i32,
        "sequence": node.seq,
    })
}

fn native_node_image(node: &Value) -> Value {
    if node.is_null() {
        return Value::Null;
    }
    json!({
        "event": node["entry"]["name"].as_str().unwrap().to_ascii_uppercase(),
        "priority": node["priority"],
        "type": node["type"],
        "sequence": node["sequence"],
    })
}

fn queue_image(queue: &VoxQueue) -> Value {
    json!({
        "critical": queue.critical_list.iter().map(node_image).collect::<Vec<_>>(),
        "interrupts": queue.interrupt_list.iter().map(node_image).collect::<Vec<_>>(),
        "pending": queue.pending.as_ref().map(node_image),
        "queues": queue.priority_lists.iter().map(|list| list.iter().map(node_image).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "current": queue.current.as_ref().map(|node| &node.event),
        "sequence": queue.seq_counter,
        "pause": queue.pause_depth,
        "suspend": queue.suspend_depth,
        "gap": queue.gap_ms,
    })
}

fn native_queue_image(state: &Value) -> Value {
    let nodes = |field: &Value| {
        field
            .as_array()
            .unwrap()
            .iter()
            .map(native_node_image)
            .collect::<Vec<_>>()
    };
    json!({
        "critical": nodes(&state["critical"]),
        "interrupts": nodes(&state["interrupts"]),
        "pending": native_node_image(&state["pending"]),
        "queues": (0..4).map(|index| nodes(&state["queues"][index.to_string()])).collect::<Vec<_>>(),
        "current": state["current"]["name"].as_str().map(str::to_ascii_uppercase),
        "sequence": state["sequence"],
        "pause": state["pause_flag"],
        "suspend": state["suspend"],
        "gap": state["wait"][0].as_u64().unwrap() | (state["wait"][1].as_u64().unwrap() << 32),
    })
}

fn request_node(entry: &crate::rules::sound_ini::EvaEntry) -> VoxNode {
    VoxNode {
        event: entry.name.to_ascii_uppercase(),
        eva_type: entry.eva_type,
        priority: entry.priority,
        seq: 0,
    }
}

fn dispatch(queue: &mut VoxQueue, registry: &EvaRegistry, step: &Value) -> QueueEffect {
    let call = step["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|call| call["call"] == "QueueVoice")
        .unwrap();
    assert_eq!(call["type_override"], 2);
    assert_eq!(call["priority_override"], -1);
    // This fixture seeds the process cache image, not Scenario.Side or
    // a SetSide call (the Ares binder has separate native controls).
    queue.selected_voice = step["before"]["side"].as_i64().unwrap() as i32;
    let index = call["index"].as_i64().unwrap() as i32;
    let effect = registry
        .entry_by_index(index)
        .map_or_else(QueueEffect::default, |entry| {
            queue.queue_voice(
                VoxRequest {
                    event: &entry.name,
                    eva_type: entry.eva_type,
                    priority: entry.priority,
                },
                Some(EvaType::Interrupt),
                None,
            )
        });
    let reached = |address: &str| {
        step["calls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|call| call["call"] == address)
    };
    assert_eq!(effect.advance_queue, reached("00752760"));
    assert_eq!(effect.stop_stream, reached("00407F40"));
    if effect.advance_queue {
        // Original PlayNextQueued's supplied pause flag is1 throughout.
        assert!(queue.take_next(1, false).is_none());
    }
    effect
}

#[test]
fn retail_trigger_eva_requests_match_original_queue_admission_and_history() {
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("evamd.ini") else {
        return;
    };
    let registry = EvaRegistry::from_ini(&ini);
    let rows = corpus();
    let mut controls = 0;
    for row in rows
        .iter()
        .filter(|row| row["case_kind"] == "eva_read_dispatch")
    {
        let mut queue = VoxQueue::new();
        queue.set_paused(true);
        // The original registry/queue persists its insertion sequence even
        // though these independent controls clear every queued node.
        queue.seq_counter = row["before"]["sequence"].as_u64().unwrap() as u32;
        assert_eq!(queue_image(&queue), native_queue_image(&row["before"]));
        dispatch(&mut queue, &registry, row);
        assert_eq!(
            queue_image(&queue),
            native_queue_image(&row["after"]),
            "{}",
            row["raw"]
        );
        controls += 1;
    }
    assert_eq!(controls, 15);

    let history = rows
        .iter()
        .find(|row| row["case_kind"] == "eva_history")
        .unwrap();
    let steps = history["history"].as_array().unwrap();
    let mut queue = VoxQueue::new();
    queue.set_paused(true);
    queue.seq_counter = steps[0]["before"]["sequence"].as_u64().unwrap() as u32;
    for step in steps {
        if step.get("raw").is_none() {
            queue.clear_all_queues();
        } else {
            let before = &step["before"];
            // The original history supplies a current pointer before the
            // same-current control; it does not start or complete hardware.
            if queue.current.as_ref().map(|node| node.event.as_str())
                != before["current"]["name"]
                    .as_str()
                    .map(str::to_ascii_uppercase)
                    .as_deref()
            {
                let name = before["current"]["name"].as_str().unwrap();
                queue.current = Some(request_node(registry.entry(name).unwrap()));
            }
            if before["suspend"] == 1 && queue.suspend_depth == 0 {
                queue.suspend();
            }
            assert_eq!(
                queue_image(&queue),
                native_queue_image(before),
                "before {}",
                step["label"]
            );
            dispatch(&mut queue, &registry, step);
        }
        assert_eq!(
            queue_image(&queue),
            native_queue_image(&step["after"]),
            "after {}",
            step["label"]
        );
    }
    assert_eq!(steps.len(), 8);
}
