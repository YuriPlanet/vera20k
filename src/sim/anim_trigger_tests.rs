//! Executed TAction41/Anim421EA0..725C70 controls, not rendered-output proof.
//! The corpus supplies physical ART/SHP inputs and post-reset ordinal order;
//! production rules processing, asset bounds and the shared Anim owner consume
//! them here. Release retail map loading remains a separate integration check.

use super::*;
use crate::map::waypoints::Waypoint;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{NativeRulesRegistryState, process_native_rules_cold_start};
use crate::rules::retail_ini_fixture::retail_rules_and_art;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::FrameEffects;
use serde_json::{Value, json};
use std::collections::HashMap;

fn corpus() -> Vec<Value> {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_action_values.json",
    ))
    .expect("original action corpus")
}

fn history_row(rows: &[Value]) -> &Value {
    rows.iter()
        .find(|row| row["case_kind"] == "anim_history")
        .expect("original full Anim histories")
}

#[test]
fn retail_cold_and_post_reset_animation_ordinals_match_original_registries() {
    let Some((retail, art)) = retail_rules_and_art() else {
        return;
    };
    let rows = corpus();
    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules().expect("retail rules");
    let cold =
        process_native_rules_cold_start(NativeRulesRegistryState::default(), &retail, &art, None)
            .unwrap();
    for row in rows.iter().filter(|row| row["case_kind"] == "anim_catalog") {
        let expected = row["catalog"]["names"].as_array().unwrap();
        let actual: Vec<&str> = if row["catalog"]["phase"] == "cold" {
            cold.registry_state()
                .anim_type_art_read_states()
                .take(expected.len())
                .map(|(name, _)| name)
                .collect()
        } else {
            rules
                .anim_type_art_read_states()
                .iter()
                .take(expected.len())
                .map(|(name, _)| name.as_str())
                .collect()
        };
        assert_eq!(
            json!(actual),
            json!(expected),
            "{}",
            row["catalog"]["phase"]
        );
        if row["catalog"]["phase"] == "post_reset_process" {
            for (index, expected) in expected.iter().enumerate() {
                assert_eq!(
                    rules.anim_type_name_by_index(index as i32),
                    expected.as_str()
                );
            }
        }
    }
    assert_eq!(rules.anim_type_name_by_index(-1), None);
    assert_eq!(rules.anim_type_name_by_index(i32::MAX), None);
}

fn apply_art_control(art: &mut IniFile, row: &Value, key: &str, name: &str) {
    let Some(keys) = row["input"][key].as_object() else {
        return;
    };
    let entries: String = keys
        .iter()
        .map(|(key, value)| format!("{key}={}\n", value.as_str().unwrap()))
        .collect();
    art.merge(&IniFile::from_str(&format!("[{name}]\n{entries}")));
}

fn rules_for_history(retail: &IniFile, physical_art: &IniFile, row: &Value) -> RuleSet {
    let mut art = physical_art.clone();
    apply_art_control(
        &mut art,
        row,
        "art_overrides",
        row["art"]["name"].as_str().unwrap(),
    );
    if !row["next_art"].is_null() {
        apply_art_control(
            &mut art,
            row,
            "next_art_overrides",
            row["next_art"]["name"].as_str().unwrap(),
        );
    }
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(retail, &art).unwrap();
    let mut registry = crate::rules::retail_ini_fixture::art_registry(&art);
    for native in [&row["art"], &row["next_art"]]
        .into_iter()
        .filter(|v| !v.is_null())
    {
        let name = native["name"].as_str().unwrap();
        // Original427D00 read the real SHP's signed header count. This binds
        // that observed input through the production ART bounds owner.
        registry.bind_anim_frame_count_for_test(
            name,
            native["raw_shp_frame_count"].as_i64().unwrap() as i32,
        );
        let config = registry.anim_runtime_config(name).unwrap();
        assert_eq!(
            json!(config.rate_logic_frames),
            native["rate"],
            "{name} Rate"
        );
        assert_eq!(
            json!(config.loop_count),
            native["loop_count"],
            "{name} LoopCount"
        );
        assert_eq!(json!(config.loop_end), native["loop_end"], "{name} LoopEnd");
        assert_eq!(json!(config.end), native["end"], "{name} End");
        assert_eq!(json!(config.z_adjust), native["z_adjust"], "{name} ZAdjust");
        assert_eq!(
            json!(config.normalized),
            native["normalized"],
            "{name} Normalized"
        );
    }
    rules.install_art_data(registry);
    rules
}

fn simulation_for_history(rules: &RuleSet, row: &Value) -> Simulation {
    let mut sim = Simulation::with_seed(0);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    let x = row["input"]["cell"][0].as_i64().unwrap() as u16;
    let y = row["input"]["cell"][1].as_i64().unwrap() as u16;
    let waypoint = row["read"]["waypoint"].as_u64().unwrap() as u32;
    sim.session.install_waypoints(&HashMap::from([(
        waypoint,
        Waypoint {
            index: waypoint,
            rx: x,
            ry: y,
        },
    )]));
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    let cell = terrain.native_cell_identity((x as i16, y as i16));
    terrain.write_native_cell_flags(cell, row["input"]["flags"].as_u64().unwrap() as u32);
    sim.session.set_binary_frame_for_test(1000);
    // The original fixture supplies Scenario+214=1000. Reserve the same
    // prefix through the existing cursor; stable IDs remain independent.
    for _ in 0..1000 {
        sim.next_native_runtime_id();
    }
    sim
}

pub(in crate::sim) fn assert_state(sim: &Simulation, id: AnimId, expected: &Value, context: &str) {
    let anim = sim.anim(id).unwrap();
    let retired = sim.substrate.pending_delete.contains_object(&id);
    let runtime = &anim.runtime;
    let actual = json!({
        "location": [anim.world_coord.x, anim.world_coord.y, anim.world_coord.z],
        "is_bouncing": u8::from(anim.bounce.is_some()),
        "native_id": anim.native_unique_id,
        "alive": u8::from(!retired),
        "limbo": u8::from(retired),
        "draw_flags": anim.draw_flags,
        "z_adjust": anim.z_adjust,
        "runtime": {
            "current_frame": runtime.current_frame,
            "frame_step": runtime.frame_step,
            "delay_remaining": runtime.delay_remaining,
            "rate_reload": runtime.rate_reload,
            "frame_timer": [runtime.frame_timer.start_frame(), runtime.frame_timer.duration()],
            "loop_remaining": runtime.loop_remaining,
            "first_ai_guard": u8::from(runtime.first_ai_guard),
            "constructor_reverse": u8::from(runtime.constructor_reverse),
            "inactive": u8::from(runtime.inactive),
            "paused": u8::from(runtime.paused),
        },
        "no_effects": anim.no_effects(),
        "type": sim.interner.resolve(anim.type_id),
        "pending_delete_count": sim.substrate.pending_delete.len(),
        "anim_count": sim.anims().count(),
    });
    assert_eq!(&actual, expected, "{context}");
    assert_eq!(anim.in_logic_vector, !retired, "{context} Logic membership");
    assert_eq!(
        sim.display_layers().layer_of(id).is_some(),
        !retired,
        "{context} Display retirement"
    );
}

fn assert_sound_calls(sim: &Simulation, calls: &Value, context: &str) {
    let expected: Vec<(String, String)> = calls
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|call| match call["call"].as_str().unwrap() {
            "play_sound" => Some((
                "start".into(),
                call["sound"].as_str().unwrap().to_ascii_uppercase(),
            )),
            "stop_handle" if call["handle_present"] == true => Some(("stop".into(), String::new())),
            "release_handle" => Some(("release".into(), String::new())),
            _ => None,
        })
        .collect();
    let actual: Vec<(String, String)> = sim
        .sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::AnimationStarted { sound_id, .. } => {
                Some(("start".into(), sound_id.to_ascii_uppercase()))
            }
            SimSoundEvent::AnimationStopped {
                stop_sound_id: None,
                ..
            } => Some(("stop".into(), String::new())),
            SimSoundEvent::AnimationStopped {
                stop_sound_id: Some(sound),
                ..
            } => Some((
                "start".into(),
                sim.interner.resolve(*sound).to_ascii_uppercase(),
            )),
            SimSoundEvent::ObjectSoundReleased { .. } => Some(("release".into(), String::new())),
            _ => None,
        })
        .collect();
    assert_eq!(actual, expected, "{context} ordered sound effects");
}

#[test]
fn retail_trigger_anim_and_retained_no_effects_next_match_original_until_cleanup() {
    let Some((retail, art)) = retail_rules_and_art() else {
        return;
    };
    let rows = corpus();
    let histories = history_row(&rows)["histories"].as_array().unwrap();
    assert_eq!(histories.len(), 3);
    for row in histories {
        let rules = rules_for_history(&retail, &art, row);
        let mut sim = simulation_for_history(&rules, row);
        let before_rng = sim.scenario_rng.logical_state();
        assert_eq!(row["raw_draw_count"], 0);
        assert_eq!(row["rng_before"], row["rng_after"]);
        assert!(sim.trigger_play_anim_at_waypoint(
            &rules,
            row["read"]["value"].as_i64().unwrap() as i32,
            row["read"]["waypoint"].as_i64().unwrap() as i32,
        ));
        let id = *sim.anims().next().unwrap().0;
        let label = row["input"]["name"].as_str().unwrap();
        assert_state(&sim, id, &row["initial"], label);
        assert_sound_calls(&sim, &row["constructor_calls"], label);
        assert_eq!(
            sim.anim_display_layer(id, Some(&rules)),
            Some(crate::sim::world::display_layers::DisplayLayer::TOP)
        );
        sim.sound_events.clear();
        // Compare the continuation after real snapshot serialization as well
        // as the live constructor. The retained byte must suppress later
        // Next Start and UnInit effects, while frames/loops still advance.
        let saved = GameSnapshot::save(&sim, 0, 0, "trigger-anim", 0);
        let mut restored = GameSnapshot::load(&saved).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_state(&restored, id, &row["initial"], label);
        for visit in row["history"].as_array().unwrap() {
            let frame = visit["frame"].as_u64().unwrap() as u32;
            for current in [&mut sim, &mut restored] {
                current.session.set_binary_frame_for_test(frame);
                current.sound_events.clear();
                current.visit_anim(id, &rules, None, FrameEffects::default());
                assert_state(current, id, &visit["state"], label);
                assert_sound_calls(current, &visit["calls"], label);
                assert_eq!(
                    current.scenario_rng.logical_state(),
                    before_rng,
                    "{label} RNG"
                );
            }
        }
        for current in [&mut sim, &mut restored] {
            assert!(current.anim(id).unwrap().completed());
            current.process_pending_delete();
            assert_eq!(json!(current.anims().count()), row["cleanup"]["anim_count"]);
            assert_eq!(
                json!(current.substrate.pending_delete.len()),
                row["cleanup"]["pending_delete_count"]
            );
            assert!(current.anim(id).is_none());
            assert!(!current.live_object_order_snapshot().contains(&id));
            assert_eq!(current.display_layers().layer_of(id), None);
        }
    }
}

#[test]
fn constructor_zero_z_adjust_uses_type_and_nonzero_overrides_match_original() {
    let Some((retail, art)) = retail_rules_and_art() else {
        return;
    };
    let rows = corpus();
    let native = history_row(&rows);
    let row = &native["histories"][0];
    let rules = rules_for_history(&retail, &art, row);
    let mut sim = simulation_for_history(&rules, row);
    let type_id = sim.interner.intern(row["art"]["name"].as_str().unwrap());
    for control in native["constructor_controls"].as_array().unwrap() {
        let id = sim
            .spawn_anim_at_world(
                &rules,
                AnimClassSpawnDescriptor {
                    draw_flags: 0x600,
                    z_adjust: control["input_z_adjust"].as_i64().unwrap() as i32,
                    ..AnimClassSpawnDescriptor::new(
                        type_id,
                        10,
                        10,
                        SimFixed::from_num(128),
                        SimFixed::from_num(128),
                        0,
                    )
                },
                AnimWorldCoord {
                    x: 2688,
                    y: 2688,
                    z: 0,
                },
            )
            .unwrap();
        let anim = sim.anim(id).unwrap();
        assert_eq!(json!(anim.z_adjust), control["state"]["z_adjust"]);
        assert_eq!(json!(anim.no_effects()), control["state"]["no_effects"]);
        sim.destroy_anim(id, &rules);
        sim.process_pending_delete();
    }
}
