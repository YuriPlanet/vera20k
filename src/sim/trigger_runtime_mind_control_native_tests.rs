//! Compare the callback portion of the shared MindControl receiver with the
//! original4690B0 prefix. Native accepted-looking rows stop before471D40;
//! Rust continues through its existing CaptureUnit owner. Ownership, mission,
//! sound and whole-call RNG after that boundary are deliberately not compared.

use super::*;
use serde_json::{Value, json};

fn number(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn native_tag_map(input: &Value) -> String {
    let definitions = input["triggers"].as_array().unwrap();
    let mut headers = String::from("[Triggers]\n");
    let mut events = String::from("[Events]\n");
    let mut actions = String::from("[Actions]\n");
    for (index, definition) in definitions.iter().enumerate() {
        let linked = if index + 1 < definitions.len() {
            format!("TR{}", index + 1)
        } else {
            "<none>".into()
        };
        headers.push_str(&format!(
            "TR{index}=Country0,{linked},Native{index},0,1,1,1,0\n"
        ));
        let source_events = definition["events"].as_array().unwrap();
        events.push_str(&format!("TR{index}={}", source_events.len()));
        for event in source_events {
            events.push_str(&format!(",{},0,{}", number(&event[0]), number(&event[1])));
        }
        events.push('\n');
        let source_actions = definition["actions"].as_array().unwrap();
        actions.push_str(&format!("TR{index}={}", source_actions.len()));
        for action in source_actions {
            let kind = number(&action[0]);
            if action.as_array().unwrap().len() == 3 {
                actions.push_str(&format!(",{kind},2,TR{},0,0,0,0,A", number(&action[2])));
            } else {
                actions.push_str(&format!(",{kind},0,{},0,0,0,0,A", number(&action[1])));
            }
        }
        actions.push('\n');
    }
    format!(
        "{headers}{events}{actions}[Tags]\nTAG={},Native,TR0\n",
        number(&input["repeat"])
    )
}

#[test]
fn mind_control_tag_ingress_matches_original_callbacks_and_live_trigger_state() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/mind_control_tags.json");
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 11);
    for case in cases {
        let input = &case["input"];
        let name = input["name"].as_str().unwrap();
        let prior = &input["probe"];
        let flag = |key: &str| prior[key].as_bool().unwrap();
        let (mut sim, rules, overlay, houses) =
            event_ingress_fixture_with_rules(MIND_CONTROL_TAG_RULES);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session
            .set_current_house_for_test(Some(if flag("human_target") {
                houses[2]
            } else {
                houses[0]
            }));
        let source = sim
            .spawn_object_at_height(
                if flag("manager") {
                    "TAGYURI"
                } else {
                    "TAGUNIT"
                },
                "House0",
                17,
                15,
                0,
                0,
                &rules,
            )
            .unwrap();
        assert_eq!(
            sim.substrate
                .entities
                .get(source)
                .unwrap()
                .capture_manager
                .is_some(),
            flag("manager"),
            "{name}"
        );
        let victim_owner = houses[if flag("same_owner") { 0 } else { 2 }];
        let victim = spawn_event_ingress_unit(&mut sim, &rules, victim_owner, (15, 15));
        sim.session.set_binary_frame_for_test(123);
        install_event_ingress_map(&mut sim, &rules, &native_tag_map(input));
        sim.attach_object_trigger_tag(victim, Some("TAG"));
        let tag = sim.trigger_runtime.attachments[&TagAttachment::Object(victim)];
        if !flag("attached") {
            sim.attach_object_trigger_tag(victim, None);
        }
        // Match the declared native detonation priors, not a death/flight
        // replay. Native reads ObjectAlive independently of positive HP.
        sim.substrate
            .entities
            .get_mut(victim)
            .unwrap()
            .lifecycle
            .set_object_alive_for_test(flag("alive"));
        let detonation = mind_control_tag_projectile(
            &mut sim,
            if flag("firer") {
                source
            } else {
                crate::sim::combat::RAD_NO_ATTACKER
            },
            victim,
        );
        if !flag("target") {
            sim.projectiles
                .get_mut(detonation.projectile_id)
                .unwrap()
                .set_target_for_test(ProjectileTarget::None);
        }
        if let Some(expected) = case["boundary"]["target_was_human"].as_bool() {
            assert_eq!(
                sim.house_is_human_player(victim_owner),
                expected,
                "{name}: supplied pre-callback House"
            );
        }
        sim.trigger_runtime.dispatch_trace.clear();
        sim.trigger_runtime.action_trace.clear();
        sim.mind_control_detonation(detonation, &rules, Some(&overlay), FrameEffects::default());

        let expected_dispatch: Vec<_> = case["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| call["kind"] == "tag")
            .map(|call| {
                assert_eq!(call["object"], "target", "{name}");
                assert_eq!(call["source"], "firer", "{name}");
                assert_eq!(call["cell"], json!([0, 0]), "{name}");
                assert_eq!(call["force"], 0, "{name}");
                (tag, number(&call["event"]))
            })
            .collect();
        // Rust continues through CaptureUnit, whose ownership callbacks
        // raise events 48/29 after the native prefix's stopping boundary.
        assert!(
            sim.trigger_runtime
                .dispatch_trace
                .starts_with(&expected_dispatch),
            "{name}: attacked callbacks precede capture callbacks"
        );
        assert!(
            sim.trigger_runtime.dispatch_trace[expected_dispatch.len()..]
                .iter()
                .all(|(_, kind)| matches!(kind, 48 | 29)),
            "{name}: only capture ownership callbacks follow the measured prefix"
        );
        // This graph contains only non-latching events 6/44. The later
        // 48/29 cannot spring its actions or change its live Trigger state.
        let expected_actions: Vec<_> = case["tag_state"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|action| (number(&action[0]), number(&action[1])))
            .collect();
        assert_eq!(
            sim.trigger_runtime.action_trace, expected_actions,
            "{name}: action order"
        );
        assert_eq!(
            json!(sim.trigger_runtime.globals_set),
            case["tag_state"]["globals"],
            "{name}: globals"
        );
        assert_eq!(
            json!(sim.trigger_runtime.locals_set),
            case["tag_state"]["locals"],
            "{name}: locals"
        );

        let live = sim.trigger_runtime.tag(tag).unwrap();
        let triggers: Vec<_> = live
            .triggers
            .iter()
            .map(|&id| {
                let trigger = sim.trigger_runtime.trigger(id).unwrap();
                json!({"type": trigger.type_index, "enabled": i32::from(trigger.enabled),
                "pending": i32::from(trigger.pending), "completed": trigger.completed,
                "start": trigger.timer.start_frame(), "duration": trigger.timer.duration()})
            })
            .collect();
        assert_eq!(
            json!({"refs": live.refs, "pending": i32::from(live.pending), "triggers": triggers}),
            case["tag_state"]["tags"][0],
            "{name}: live Tag/Trigger state"
        );
        assert_eq!(
            sim.substrate.pending_delete.len(),
            case["tag_state"]["pending_count"].as_u64().unwrap() as usize,
            "{name}: deferred removal"
        );
        // Only the two actual native refusal rows establish capture outcome.
        // The ordinary rows' endpoint is merely the incoming CaptureUnit call.
        if flag("execute_rejection") {
            let returned = case["calls"]
                .as_array()
                .unwrap()
                .iter()
                .find(|call| call["kind"] == "capture_return")
                .unwrap();
            assert_eq!(returned["al"], 0, "{name}: native refusal");
            assert_eq!(
                sim.substrate
                    .entities
                    .get(victim)
                    .unwrap()
                    .mind_control
                    .controller(),
                None,
                "{name}"
            );
        }
    }
}
