//! Original Object5F5390 controls at explicitly supplied Tag/virtual sinks.
//! These compare the shared body, not complete Tag actions or a death world.

use super::*;

fn damage_corpus() -> Value {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_damage_events.json",
    ))
    .unwrap();
    assert_eq!(
        native["native_sha256"],
        "3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600"
    );
    assert_eq!(native["receiver_rng"]["unchanged"], true);
    assert_eq!(
        native["receiver_rng"]["full_seed31"],
        native["receiver_rng"]["full_after"]
    );
    native
}

fn live(entity: &GameEntity, strength: &Cell<i32>, tag: &Cell<bool>) -> Value {
    json!({
        "health": entity.health.current(),
        "alive": u8::from(entity.lifecycle.object_alive()),
        "strength": strength.get(),
        "attached_tag": tag.get(),
    })
}

fn virtual_slot(callback: HealthCallback) -> &'static str {
    match callback {
        HealthCallback::Changed => "148",
        HealthCallback::Kill => "0E0",
        HealthCallback::Destroy => "0DC",
        HealthCallback::Tag { .. } => unreachable!(),
    }
}

// Native pointers and Rust fixture identities share presence/target roles;
// their numeric addresses are not a gameplay value this scalar host exposes.
fn normalized_virtuals(output: &Value) -> Value {
    let mut calls = output["callbacks"].clone();
    for call in calls.as_array_mut().unwrap() {
        if call["slot"] == "0E0" {
            call["argument"] = json!(i64::from(call["argument"].as_u64().unwrap() != 0));
        }
    }
    calls
}

fn normalized_tags(output: &Value) -> Value {
    json!(
        output["tag_calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|call| json!({
                "kind": call["kind"], "source": call["source"].as_u64().unwrap() != 0,
                "health": call["health"], "alive": call["alive"],
            }))
            .collect::<Vec<_>>()
    )
}

#[test]
fn techno_post_object_anger_delta_matches_original_executed_fragment() {
    let native = damage_corpus();
    let rows = native["techno_anger_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    for row in rows {
        let input = &row["input"];
        //Original701DFD..701E68 stops at the actual House504790 entry.
        //Its PC53/chop intermediate is significant (51/100*700 gives356).
        //The House node update itself retains its separate existing owner.
        let actual = super::super::super::receiver_anger_delta(
            input["damage_packet"].as_i64().unwrap() as i32,
            input["strength"].as_i64().unwrap() as i32,
            input["cost"].as_i64().unwrap() as i32,
        );
        assert_eq!(
            actual,
            row["output"]["delta"].as_i64().unwrap() as i32,
            "{}",
            row["name"]
        );
    }
}

#[test]
fn object_threshold_and_first_damage_order_matches_original_eighteen_controls() {
    let native = damage_corpus();
    let rows = native["receiver_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 18);
    for row in rows {
        let input = &row["input"];
        let output = &row["output"];
        let mut entity = GameEntity::test_default(1, "X", "Owner", 2, 2);
        entity
            .health
            .set_for_test(input["current"].as_i64().unwrap() as i32);
        entity
            .lifecycle
            .set_object_alive_for_test(input["alive"] == 1);
        entity.estimated_health =
            crate::sim::estimated_health::EstimatedHealth::from_raw(-987654321);
        let strength = Cell::new(input["strength"].as_i64().unwrap() as i32);
        let attached = input["tag_context"]["tag"].as_bool().unwrap();
        let source = input["tag_context"]["source"].as_bool().unwrap();
        let mut virtuals = Vec::new();
        let mut tags = Vec::new();
        let callback = |entity: &mut GameEntity, callback: HealthCallback| {
            if let HealthCallback::Tag { kind, with_source } = callback {
                if attached {
                    tags.push(json!({"kind":kind, "source":with_source,
                        "health":entity.health.current(), "alive":u8::from(entity.lifecycle.object_alive())}));
                }
            } else {
                observe_virtual(
                    entity,
                    callback,
                    input["callback"].as_str().unwrap(),
                    i64::from(source),
                    &mut virtuals,
                );
            }
        };
        let mut host = LocalHealthHost {
            entity: &mut entity,
            strength: &strength,
            callback,
        };
        let mut packet = input["damage"].as_i64().unwrap() as i32;
        let admitted = packet != 0;
        let (result, draws) = crate::sim::rng::trace_draws(|| {
            commit(
                &mut host,
                &mut packet,
                strength.get(),
                admitted,
                input["kind"] == "building" && input["can_c4"] == false,
                0.25,
                source,
            )
        });
        drop(host);
        assert_eq!(
            entity.health.current(),
            output["actual"].as_i64().unwrap() as i32,
            "{row}"
        );
        assert_eq!(
            entity.estimated_health.get(),
            output["estimated"].as_i64().unwrap() as i32
        );
        assert_eq!(packet, output["damage_packet"].as_i64().unwrap() as i32);
        assert_eq!(
            result as i32,
            output["result"].as_i64().unwrap() as i32,
            "{}",
            row["name"]
        );
        assert_eq!(
            u8::from(entity.lifecycle.object_alive()),
            output["alive"].as_u64().unwrap() as u8
        );
        assert_eq!(json!(tags), normalized_tags(output), "{}", row["name"]);
        assert_eq!(
            json!(virtuals),
            normalized_virtuals(output),
            "{}",
            row["name"]
        );
        assert_eq!(row["observed_rng_detach_entries"], json!([]));
        assert!(draws.is_empty(), "the supplied callback sinks spend no RNG");
    }
}

#[test]
fn object_live_reads_and_callback_returns_match_original_twenty_three_mutations() {
    let native = damage_corpus();
    let rows = native["mutation_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 23);
    for row in rows {
        let input = &row["native_receiver"]["input"];
        let output = &row["native_receiver"]["output"];
        let controlled = &row["controlled_input"];
        let mut entity = GameEntity::test_default(1, "X", "Owner", 2, 2);
        entity
            .health
            .set_for_test(input["current"].as_i64().unwrap() as i32);
        entity
            .lifecycle
            .set_object_alive_for_test(input["alive"] == 1);
        entity.estimated_health =
            crate::sim::estimated_health::EstimatedHealth::from_raw(-987654321);
        let strength = Cell::new(input["strength"].as_i64().unwrap() as i32);
        let attached = Cell::new(input["tag_context"]["tag"].as_bool().unwrap());
        let source = input["tag_context"]["source"].as_bool().unwrap();
        let mode = controlled["callback"].as_str().unwrap();
        let mutation = row["mutations_applied"].as_array().unwrap().first();
        let mut mutated = false;
        let mut virtuals = Vec::new();
        let mut tags = Vec::new();
        let mut returns = Vec::new();
        let callback = |entity: &mut GameEntity, callback: HealthCallback| {
            let identity = if let HealthCallback::Tag { kind, with_source } = callback {
                if !attached.get() {
                    return;
                }
                tags.push(json!({"kind":kind, "source":with_source,
                    "health":entity.health.current(), "alive":u8::from(entity.lifecycle.object_alive())}));
                json!({"family":"tag", "kind":kind, "source":with_source})
            } else {
                observe_virtual(entity, callback, mode, i64::from(source), &mut virtuals);
                json!({"family":"virtual", "slot":virtual_slot(callback)})
            };
            let mut returned = identity.clone();
            returned["live"] = live(entity, &strength, &attached);
            returns.push(returned);
            //Map the declared native return PC to its recorded callee identity.
            //Expected event order never drives the Rust function's dispatch.
            if let Some(mutation) = mutation
                && !mutated
            {
                let at = &mutation["after_return"];
                let matches = identity["family"] == at["family"]
                    && if identity["family"] == "tag" {
                        identity["kind"] == at["kind"]
                            && identity["source"].as_bool().unwrap()
                                == (at["source"].as_u64().unwrap() != 0)
                    } else {
                        identity["slot"] == at["slot"]
                    };
                if matches {
                    let writes = &mutation["requested_writes"];
                    if let Some(hp) = writes["health"].as_i64() {
                        entity.health.set_for_test(hp as i32);
                    }
                    if let Some(value) = writes["alive"].as_bool() {
                        entity.lifecycle.set_object_alive_for_test(value);
                    }
                    if let Some(value) = writes["strength"].as_i64() {
                        strength.set(value as i32);
                    }
                    if let Some(value) = writes["attached_tag"].as_bool() {
                        attached.set(value);
                    }
                    mutated = true;
                }
            }
        };
        let mut host = LocalHealthHost {
            entity: &mut entity,
            strength: &strength,
            callback,
        };
        let mut packet = input["damage"].as_i64().unwrap() as i32;
        let (result, draws) = crate::sim::rng::trace_draws(|| {
            commit(
                &mut host,
                &mut packet,
                strength.get(),
                true,
                false,
                0.25,
                source,
            )
        });
        drop(host);
        let expected_returns = row["ordered_returns"].as_array().unwrap().iter().map(|ret| {
            let mut value = if ret["family"] == "tag" {
                json!({"family":"tag", "kind":ret["kind"], "source":ret["source"].as_u64().unwrap() != 0})
            } else { json!({"family":"virtual", "slot":ret["slot"]}) };
            value["live"] = ret["live"].clone();
            value
        }).collect::<Vec<_>>();
        assert_eq!(json!(returns), json!(expected_returns), "{}", row["name"]);
        assert_eq!(mutated, mutation.is_some(), "{}", row["name"]);
        assert_eq!(
            live(&entity, &strength, &attached),
            row["final_live"],
            "{}",
            row["name"]
        );
        assert_eq!(
            result as i32,
            output["result"].as_i64().unwrap() as i32,
            "{}",
            row["name"]
        );
        assert_eq!(packet, output["damage_packet"].as_i64().unwrap() as i32);
        assert_eq!(
            entity.estimated_health.get(),
            output["estimated"].as_i64().unwrap() as i32
        );
        assert_eq!(json!(tags), normalized_tags(output), "{}", row["name"]);
        assert_eq!(
            json!(virtuals),
            normalized_virtuals(output),
            "{}",
            row["name"]
        );
        assert_eq!(row["rng_detach_calls"], json!([]));
        assert!(draws.is_empty());
    }
}
