//! Foot4DA806..4DAB3C comparisons from original SQD execution.
//! Native Process is a declared no-movement boundary. Component tests compose
//! the existing counter and sound owners; the retail test below runs the real
//! production frame and Ship Process, without claiming native movement parity.

use super::{MoveSoundState, SimSoundEvent, Simulation, TickLane};
use crate::map::entities::EntityCategory;
use crate::rules::move_sound_tests::{assert_binding, fixture_rules, native};
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::rng::SimRng;
use crate::sim::snapshot::GameSnapshot;
use crate::util::fixed_math::SimFixed;
use serde_json::{Value, json};

fn actor_state(sim: &Simulation, id: u64) -> Value {
    let actor = sim.substrate.entities.get(id).unwrap();
    json!({
        "body_counter": actor.body_frame_counter,
        "active_raw": u8::from(actor.move_sound.is_active()),
        "countdown": actor.move_sound.countdown(),
    })
}

fn expected_state(value: &Value) -> Value {
    json!({"body_counter": value["body_counter"],
        "active_raw": value["active_raw"], "countdown": value["countdown"]})
}

fn assert_rng(sim: &Simulation, expected: &Value, context: &str) {
    for (name, rng) in [
        ("main", &sim.main_rng),
        ("scenario", &sim.scenario_rng),
        ("mapgen", &sim.mapgen_rng),
    ] {
        assert_eq!(
            rng.native_state_hex(),
            expected["rng"][name].as_str().unwrap(),
            "{context}: complete {name} continuation"
        );
    }
}

fn scene(input: &Value, before: &Value, rules: &RuleSet) -> (Simulation, u64) {
    let seed = input["seed"].as_u64().unwrap_or(31);
    let mut sim = Simulation::with_seed(seed);
    sim.main_rng = SimRng::new(seed);
    sim.scenario_rng = SimRng::new(seed);
    sim.mapgen_rng = SimRng::new(seed);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 0, 10));
    sim.session.house_order.push(owner);
    let id = sim.allocate_stable_id();
    let xyz: Vec<_> = before["xyz"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_i64().unwrap() as i32)
        .collect();
    let mut actor = GameEntity::test_default(
        id,
        "SQD",
        "Americans",
        (xyz[0] / 256) as u16,
        (xyz[1] / 256) as u16,
    );
    actor.owner = owner;
    actor.type_ref = sim.interner.intern("SQD");
    actor.category = EntityCategory::Unit;
    actor.position.sub_x = SimFixed::from_num(xyz[0] % 256);
    actor.position.sub_y = SimFixed::from_num(xyz[1] % 256);
    actor.position.z = (xyz[2] / 104) as u8;
    actor.position.exact_z_leptons = Some(xyz[2]);
    actor.lifecycle.in_limbo = input["limbo"].as_u64().unwrap_or(0) != 0;
    actor.lifecycle.object_alive = input["alive"].as_u64().unwrap_or(1) != 0;
    actor.lifecycle.cell_marked = false;
    actor.locomotor = Some(LocomotorState::from_object_type(
        rules.object("SQD").unwrap(),
        0,
    ));
    actor.body_frame_counter = before["body_counter"].as_u64().unwrap() as u32;
    actor.move_sound = MoveSoundState::from_raw_for_test(
        before["active_raw"].as_u64().unwrap() != 0,
        before["countdown"].as_i64().unwrap() as i32,
    );
    actor.set_falling_down_for_test(input["falling"].as_u64().unwrap_or(0) != 0);
    actor.crashing = input["crashing"].as_u64().unwrap_or(0) != 0;
    sim.substrate.entities.insert(actor);
    assert_eq!(actor_state(&sim, id), expected_state(before));
    assert_rng(&sim, before, "supplied seeded entry");
    (sim, id)
}

fn native_effects(step: &Value, coordinates: bool) -> Vec<Value> {
    step["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| match event["kind"].as_str().unwrap() {
            "hard_stop" | "release" | "decay_stop" => Some(json!([event["kind"]])),
            "playback" if coordinates => Some(json!(["playback", event["name"], event["xyz"]])),
            "playback" => Some(json!(["playback", event["name"]])),
            _ => None,
        })
        .collect()
}

fn effects(id: u64, events: &[SimSoundEvent], coordinates: bool) -> Vec<Value> {
    events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::AnimationStopped {
                anim_id,
                stop_sound_id,
                ..
            } if *anim_id == id => {
                assert!(stop_sound_id.is_none());
                Some(json!(["hard_stop"]))
            }
            SimSoundEvent::AnimationStarted {
                anim_id,
                sound_id,
                world,
            } if *anim_id == id => Some(if coordinates {
                json!(["playback", sound_id, [world.x, world.y, world.z]])
            } else {
                json!(["playback", sound_id])
            }),
            SimSoundEvent::ObjectSoundReleased { owner } if *owner == id => {
                Some(json!(["release"]))
            }
            SimSoundEvent::ObjectSoundDetached { owner } if *owner == id => {
                Some(json!(["decay_stop"]))
            }
            _ => None,
        })
        .collect()
}

fn visit(sim: &mut Simulation, rules: &RuleSet, id: u64, step: &Value, context: &str) {
    assert_eq!(
        actor_state(sim, id),
        expected_state(&step["before"]),
        "{context}: entry"
    );
    assert_rng(sim, &step["before"], context);
    sim.session.binary_frame = step["frame"].as_u64().unwrap() as u32;
    sim.sound_events.clear();
    let body_before = sim.substrate.entities.get(id).unwrap().body_frame_counter;
    let object = rules.object("SQD").unwrap();
    let (_, draws) = crate::sim::rng::trace_draws(|| {
        crate::sim::animation::tick_unit_body_frame_counter(
            sim.substrate.entities.get_mut(id).unwrap(),
            Some(crate::sim::movement::SpeedRules::new(
                rules,
                &sim.interner,
                &sim.type_handles,
                &sim.houses,
            )),
            crate::sim::animation::ShpVehicleCadence {
                walk_rate: object.walk_rate,
                idle_rate: object.idle_rate,
            },
            object.hover_attack,
            object.deploy_to_land,
            sim.session.binary_frame,
        );
        sim.tick_move_sound_after_process(id, body_before, Some(rules));
    });
    assert_eq!(
        actor_state(sim, id),
        expected_state(&step["after"]),
        "{context}: tail"
    );
    assert_rng(sim, &step["after"], context);
    let native_draws: Vec<_> = step["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "random_return")
        .map(|event| {
            assert_eq!(event["stream"], "main");
            event["raw"].clone()
        })
        .collect();
    assert_eq!(
        draws
            .iter()
            .map(|draw| draw["value"].clone())
            .collect::<Vec<_>>(),
        native_draws,
        "{context}: raw draw count/order"
    );
    let actual = effects(id, &sim.sound_events, true);
    assert_eq!(
        actual.len(),
        sim.sound_events.len(),
        "{context}: all sound effects represented"
    );
    assert_eq!(
        actual,
        native_effects(step, true),
        "{context}: ordered handle effects and location"
    );
}

#[test]
fn native_idle_timelines_keep_latch_countdown_and_main_draw_history() {
    let corpus = native();
    let rows = corpus["timelines"].as_array().unwrap();
    for row in rows {
        let rules = fixture_rules(&corpus, &row["input"]);
        let name = row["name"].as_str().unwrap();
        assert_binding(&rules, &row["binding"], name);
        let steps = row["steps"].as_array().unwrap();
        let (mut sim, id) = scene(&row["input"], &steps[0]["before"], &rules);
        for step in steps {
            visit(
                &mut sim,
                &rules,
                id,
                step,
                &format!("{name}: frame {}", step["frame"]),
            );
        }
    }
    assert_eq!(
        rows.iter()
            .map(|row| row["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "stock_idle_disabled",
            "stock_idle_accepted_boundary",
            "authored_idle_rate5_lapse",
            "ordered_two_sounds",
            "ordered_duplicate_sounds",
            "stock_idle_frame_zero",
        ],
        "all original timelines and the frame-zero addition consumed"
    );
}

#[test]
fn native_sound_tail_controls_preserve_signed_timer_and_counter_wrap() {
    let corpus = native();
    let mut excluded = Vec::new();
    let mut compared = 0;
    for row in corpus["controls"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        if matches!(
            name,
            "moving_without_counter_delta"
                | "fresh_moving_now_only"
                | "pair_marker_type692_false"
                | "pair_marker_type692_true"
        ) {
            // The first two supply callback responses rather than original
            // retained Ship state. The others supply an unrepresented reciprocal
            // link. Neither is silently replaced by an ordinary idle fixture.
            excluded.push(name);
            continue;
        }
        if let Some(answers) = row["input"]["moving_answers"].as_array() {
            assert!(answers.iter().all(|answer| answer == 0), "{name}");
        }
        let rules = fixture_rules(&corpus, &row["input"]);
        assert_binding(&rules, &row["binding"], name);
        let (mut sim, id) = scene(&row["input"], &row["before"], &rules);
        visit(&mut sim, &rules, id, row, name);
        compared += 1;
    }
    assert_eq!(compared, 14);
    assert_eq!(
        excluded,
        [
            "moving_without_counter_delta",
            "fresh_moving_now_only",
            "pair_marker_type692_false",
            "pair_marker_type692_true"
        ]
    );
}

#[test]
fn native_foot_load_reset_runs_through_snapshot_restore_then_the_next_visit() {
    let corpus = native();
    let rules = fixture_rules(&corpus, &json!({}));
    let rows = corpus["load"].as_array().unwrap();
    for row in rows {
        let name = row["name"].as_str().unwrap();
        let (mut sim, id) = scene(&json!({"seed":31}), &row["before"], &rules);
        sim.session.map_name = "MOVE_SOUND_TEST.MAP".to_owned();
        let bytes = GameSnapshot::save_validated(&sim, 17, 18, name, 0);
        let mut loaded = GameSnapshot::load_validated(&bytes, 17, 18, "MOVE_SOUND_TEST.MAP")
            .unwrap()
            .sim;
        assert_eq!(
            actor_state(&loaded, id),
            expected_state(&row["before"]),
            "{name}: raw snapshot fields reach the restore owner"
        );
        loaded.retain_in_scenario_process_state_from(&sim);
        loaded.restore_after_snapshot_load().unwrap();
        loaded.resolve_type_handles(&rules);
        assert_eq!(
            actor_state(&loaded, id),
            expected_state(&row["after"]),
            "{name}: FootLoad resets"
        );
        assert!(
            loaded.sound_events.is_empty(),
            "load does not synthesize MoveSound playback"
        );
        // Native Scenario reload reseeds separately. This bounded raw-object
        // load fixture does not execute that operation; restore the explicitly
        // supplied Scenario boundary before comparing its next Foot visit.
        loaded.scenario_rng = SimRng::from_native_state_hex_for_test(
            row["next_visit"]["before"]["rng"]["scenario"]
                .as_str()
                .unwrap(),
        );
        visit(&mut loaded, &rules, id, &row["next_visit"], name);
    }
    assert_eq!(rows.len(), 2);
}

#[test]
fn ordinary_uninit_then_physical_destruction_detaches_without_a_foot_hard_stop() {
    let corpus = native();
    let rules = fixture_rules(&corpus, &json!({}));
    let handles = corpus["handles"].as_array().unwrap();
    let original_limbo = handles
        .iter()
        .find(|row| row["name"] == "stock_playing_foot_limbo_tail")
        .unwrap();
    let original_destructor = handles
        .iter()
        .find(|row| row["name"] == "stock_playing_foot_destructor_tail")
        .unwrap();
    for active in [false, true] {
        let (mut sim, id) = scene(&json!({"seed":31}), &original_limbo["before"], &rules);
        sim.substrate.entities.get_mut(id).unwrap().move_sound =
            MoveSoundState::from_raw_for_test(active, 3);
        sim.uninit_with_rules(id, &rules);
        assert_eq!(
            effects(id, &sim.sound_events, true),
            native_effects(original_limbo, true)
        );
        assert!(sim.substrate.pending_delete.contains(&id));
        let mut expected = native_effects(original_limbo, true);
        expected.extend(native_effects(original_destructor, true));
        sim.process_pending_delete_with(Some(&rules), None);
        assert_eq!(
            effects(id, &sim.sound_events, true),
            expected,
            "latch {active}: original Limbo then physical Foot release; Object+3C/+50 are different handles"
        );
        assert!(sim.substrate.entities.get(id).is_none());
    }
}

#[test]
fn retail_idle_squid_production_frames_match_native_counter_and_sound_cadence() {
    let Some((retail_dir, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let corpus = native();
    for name in ["stock_idle_disabled", "stock_idle_frame_zero"] {
        let row = corpus["timelines"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap();
        assert_retail_idle_timeline(&retail_dir, row);
    }
}

fn assert_retail_idle_timeline(retail_dir: &std::path::Path, row: &Value) {
    let name = row["name"].as_str().unwrap();
    let steps = row["steps"].as_array().unwrap();
    let mut scene = crate::headless_scenario::load(retail_dir, "Hills.mmx", 31)
        .expect("production retail Hills/Battle load");
    assert_binding(
        &scene.runtime.resources.rules,
        &row["binding"],
        "paid Hills/Battle",
    );
    let runtime = &mut scene.runtime;
    let owner = runtime.simulation.session.current_house.unwrap();
    let owner_name = runtime.simulation.resolve(owner).to_owned();
    let water: Vec<_> = runtime
        .simulation
        .resolved_terrain
        .as_ref()
        .unwrap()
        .cells()
        .iter()
        .filter(|cell| cell.yr_cell_land_type == 2)
        .map(|cell| (cell.rx, cell.ry))
        .collect();
    let id = water
        .into_iter()
        .find_map(|(rx, ry)| {
            runtime.simulation.spawn_object_with_overlay_registry(
                "SQD",
                &owner_name,
                rx,
                ry,
                0,
                &runtime.resources.rules,
                &runtime.resources.overlay_registry,
            )
        })
        .expect("ordinary SQD placement on physical retail water");
    let sim = &mut runtime.simulation;
    let actor = sim.substrate.entities.get(id).unwrap();
    assert_eq!(actor_state(sim, id), expected_state(&steps[0]["before"]));
    assert!(actor.navigation.nav_com.is_none());
    let position = actor.position.clone();
    // The oracle starts from an already-live actor after setup. Align only
    // its declared frame/Main boundary; leave production movement and frame
    // dispatch intact. This is not a whole-map startup RNG comparison.
    sim.session.binary_frame = steps[0]["frame"].as_u64().unwrap() as u32;
    sim.main_rng =
        SimRng::from_native_state_hex_for_test(steps[0]["before"]["rng"]["main"].as_str().unwrap());
    for step in steps {
        let native_frame = step["frame"].as_u64().unwrap() as u32;
        assert_eq!(
            runtime.simulation.session.binary_frame, native_frame,
            "{name}: original entry frame"
        );
        let output = runtime
            .advance_frame(
                &[],
                crate::headless_scenario::SIM_TICK_MS,
                TickLane::Ordinary,
            )
            .expect("paid ordinary production frame");
        assert!(output.tick.frame_committed);
        let sim = &runtime.simulation;
        // SimRuntime's ordinary frame commits binary_frame after object AI.
        // Native slice frame 0 therefore supplies completed boundary 1; the
        // event/state assertions below compare the same pre-commit visit.
        assert_eq!(
            sim.session.binary_frame,
            native_frame.wrapping_add(1),
            "{name}: completed boundary after original frame {native_frame}"
        );
        assert_eq!(
            actor_state(sim, id),
            expected_state(&step["after"]),
            "{name}: paid original frame {native_frame}"
        );
        assert_eq!(
            effects(id, &output.sound_events, false),
            native_effects(step, false),
            "{name}: paid original frame {native_frame}: captured-before-Process counter reaches sound owner"
        );
        assert_eq!(
            sim.main_rng.native_state_hex(),
            step["after"]["rng"]["main"].as_str().unwrap()
        );
        let actor = sim.substrate.entities.get(id).unwrap();
        assert_eq!(actor.position.rx, position.rx);
        assert_eq!(actor.position.ry, position.ry);
        assert_eq!(actor.position.sub_x, position.sub_x);
        assert_eq!(actor.position.sub_y, position.sub_y);
        assert!(actor.navigation.nav_com.is_none());
    }
}
