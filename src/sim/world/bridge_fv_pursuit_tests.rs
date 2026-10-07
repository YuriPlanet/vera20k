//! Production map/command/AI witness for the native ordinary FV Cell approach.
use crate::headless_scenario::{HeadlessScenario, SIM_TICK_MS};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::NavTargetRef;
use crate::sim::world::TickLane;
use crate::sim::world::bridge_test_evidence::load_anytown_concrete;
use crate::util::fixed_math::SimFixed;
use serde_json::{Value, json};

type AmbientAnims = std::collections::BTreeSet<crate::sim::anim_class::AnimId>;

#[path = "bridge_fv_persistence_tests.rs"]
mod persistence;

fn cell(value: &Value) -> (u16, u16) {
    (
        value[0].as_u64().unwrap() as u16,
        value[1].as_u64().unwrap() as u16,
    )
}

fn spawn_fv(scene: &mut HeadlessScenario, xyz: [i32; 3]) -> u64 {
    let runtime = &mut scene.runtime;
    let owner = runtime.simulation.session.current_house.unwrap();
    let owner_name = runtime.simulation.resolve(owner).to_owned();
    let id = runtime
        .simulation
        .spawn_object_with_overlay_registry(
            "FV",
            &owner_name,
            (xyz[0] / 256) as u16,
            (xyz[1] / 256) as u16,
            128,
            &runtime.resources.rules,
            &runtime.resources.overlay_registry,
        )
        .expect("ordinary FV bank placement");
    // Declared native Unlimbo coordinate input within this already admitted
    // Cell. The stock map and production placement/membership remain intact.
    let actor = runtime.simulation.substrate.entities.get_mut(id).unwrap();
    actor.position.sub_x = SimFixed::from_num(xyz[0] % 256);
    actor.position.sub_y = SimFixed::from_num(xyz[1] % 256);
    actor.position.exact_z_leptons = Some(xyz[2]);
    id
}

fn nav(target: Option<NavTargetRef>) -> Value {
    match target {
        None => Value::Null,
        Some(NavTargetRef::Cell { rx, ry }) => json!([rx, ry]),
        other => panic!("expected Cell NavCom, got {other:?}"),
    }
}

fn coordinate(point: Option<crate::sim::components::DriveCoord>) -> Value {
    point.map_or_else(|| json!([0, 0, 0]), |p| json!([p.x, p.y, p.z]))
}

fn rng_sha(rng: &crate::sim::rng::SimRng) -> String {
    let hex = rng.native_state_hex();
    let bytes: Vec<_> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|digits| u8::from_str_radix(std::str::from_utf8(digits).unwrap(), 16).unwrap())
        .collect();
    crate::util::sha256::sha256_hex(&bytes)
}

fn mission_state(actor: &crate::sim::game_entity::GameEntity) -> Value {
    let dispatch = actor.mission.dispatch_timer();
    json!({"mission":actor.mission.current().raw(),
        "queued":actor.mission.queued().raw(),"status":actor.mission.handler_state(),
        "mission_visit_count":actor.mission.ai_counter(),
        "dispatch":[dispatch.start_frame(),dispatch.delay()],
        "rearm":[actor.rearm_timer.start_frame(),actor.rearm_timer.duration()]})
}

fn passive_scan_state(actor: &crate::sim::game_entity::GameEntity) -> Value {
    json!({"timer":[actor.passive_scan_timer.start_frame,actor.passive_scan_timer.duration],
        "last_frame":actor.last_target_scan_frame,
        "acquired":actor.passively_acquired_target})
}

fn paid_diagnostic(
    scene: &HeadlessScenario,
    id: u64,
    ambient_anims: &std::collections::BTreeSet<crate::sim::anim_class::AnimId>,
) -> Value {
    let sim = scene.sim();
    let actor = sim.substrate.entities.get(id).unwrap();
    json!({"frame":sim.session.binary_frame,
        "stable_id":id,
        "game_speed":sim.session.game_options.game_speed,
        "native_id":actor.native_unique_id,
        "next_native_id":sim.native_unique_ids.as_ref().unwrap().current_raw(),
        "mission":mission_state(actor),
        "passive_scan":passive_scan_state(actor),
        "bullets":sim.projectiles.iter().filter(|(_,b)|b.source_id == id)
            .map(|(_,b)|b).collect::<Vec<_>>(),
        "anims":sim.substrate.anims.iter().filter(|(id, _)| !ambient_anims.contains(id))
            .map(|(_, anim)|anim).collect::<Vec<_>>(),
        "anim_types":sim.substrate.anims.iter().filter(|(id, _)| !ambient_anims.contains(id))
            .map(|(_, anim)| {
                let name = sim.resolve(anim.type_id);
                json!({"name":name,"native_id":anim.native_unique_id,
                    "config":scene.runtime.resources.rules.art().anim_runtime_config(name)
                        .map(|c|json!({"raw_shp_frame_count":c.raw_shp_frame_count,
                            "start":c.start,"end":c.end,"loop_start":c.loop_start,
                            "loop_end":c.loop_end,"loop_count":c.loop_count,
                            "rate_logic_frames":c.rate_logic_frames,"normalized":c.normalized}))})
            }).collect::<Vec<_>>(),
        "rng":{"main":sim.main_rng.native_state_hex(),
            "scenario":sim.scenario_rng.native_state_hex(),
            "mapgen":sim.mapgen_rng.native_state_hex()}})
}

fn paid_snapshot(
    scene: &HeadlessScenario,
    id: u64,
    ambient_anims: &std::collections::BTreeSet<crate::sim::anim_class::AnimId>,
) -> Value {
    let sim = scene.sim();
    let actor = sim.substrate.entities.get(id).unwrap();
    let position = crate::sim::movement::ground_pose::position_world_coord(&actor.position);
    let dispatch = actor.mission.dispatch_timer();
    // Constructor storage is lazy until the first Drive Process.
    let constructor_drive = crate::sim::movement::DriveLocomotionRuntime::default();
    let drive = actor
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_drive_runtime())
        .and_then(|r| r.retained())
        .unwrap_or(&constructor_drive);
    let path = &actor.navigation.path_runtime;
    let track_state = json!({
        "movement_timer":[path.movement_timer.start_frame(), path.movement_timer.duration()],
        "blocked_timer":[path.blocked_timer.start_frame(), path.blocked_timer.duration()],
        "residual":drive.track().residual,
        "target_speed_fixed_bits":drive.target_speed_fraction().to_bits(),
    });
    let cell = sim.resolved_terrain.as_ref().unwrap().cell(87, 54).unwrap();
    let bullets: Vec<_> = sim
        .projectiles
        .iter()
        .filter(|(_, b)| b.source_id == id)
        .map(|(_, b)| {
            let guidance = b.guidance.as_ref().expect("physical FV guided missile");
            json!({"native_id":b.native_unique_id,"alive":1,
            "position":[b.position.x,b.position.y,b.position.z],
            "velocity":[b.velocity.x.bits(),b.velocity.y.bits(),b.velocity.z.bits()],
            "arm_timer":[b.arm_timer.start_frame(),b.arm_timer.duration()],
            "course_locked":guidance.course_locked,"course_frames":guidance.course_frames,
            "closing_frames":guidance.closing_frames,
            "closing_accumulator_bits":guidance.closing_accumulator_bits})
        })
        .collect();
    let anims: Vec<_> = sim.substrate.anims.iter()
        .filter(|(id, _)| !ambient_anims.contains(id))
        .map(|(_, a)| json!({"native_id":a.native_unique_id,
            "type_name":sim.resolve(a.type_id),"alive":true,
            "position":[a.world_coord.x,a.world_coord.y,a.world_coord.z],
            "runtime":{"current_frame":a.runtime.current_frame,
                "frame_step":a.runtime.frame_step,"delay_remaining":a.runtime.delay_remaining,
                "rate_reload":a.runtime.rate_reload,
                "frame_timer":[a.runtime.frame_timer.start_frame(),a.runtime.frame_timer.duration()],
                "loop_remaining":a.runtime.loop_remaining,"first_ai_guard":a.runtime.first_ai_guard,
                "constructor_reverse":a.runtime.constructor_reverse,"inactive":a.runtime.inactive,
                "paused":a.runtime.paused}}))
        .collect();
    json!({
        "next_frame":sim.session.binary_frame,
        "game_speed":sim.session.game_options.game_speed,
        "native_id":actor.native_unique_id,
        "next_native_id":sim.native_unique_ids.as_ref().unwrap().current_raw(),
        "mission":mission_state(actor),
        "passive_scan":passive_scan_state(actor),
        "actor": {"position":[position.x,position.y,position.z],
            "health":actor.health.current,"mission":actor.mission.current().raw(),
            "queued":actor.mission.queued().raw(),
            "dispatch":[dispatch.start_frame(),dispatch.delay()],
            "rearm":[actor.rearm_timer.start_frame(),actor.rearm_timer.duration()]},
        "nav_cell":nav(actor.navigation.nav_com),
        "drive_destination":coordinate(actor.locomotor.as_ref().and_then(|l| l.selected_drive_runtime()).and_then(|r| r.retained()).and_then(|d| d.destination())),
        "drive_head":coordinate(actor.locomotor.as_ref().and_then(|l| l.selected_drive_runtime()).and_then(|r| r.retained()).and_then(|d| d.head_to())),
        "track_state":track_state,
        "target_present":actor.attack_target.is_some(),
        "live_bullet_positions":bullets.iter().map(|b|b["position"].clone()).collect::<Vec<_>>(),
        "bullets":bullets,"anims":anims,
        "anim_count":sim.substrate.anims.iter().filter(|(id, _)| !ambient_anims.contains(id)).count(),
        "target_cell":{"land":cell.yr_cell_land_type,"zone_type":cell.zone_type,
            "level":cell.level,"tile":cell.final_tile_index,"subtile":cell.final_sub_tile,
            "overlay":cell.bridge_facts.overlay_id.map_or(-1,i32::from),
            "state":cell.bridge_facts.state_byte},
        "rng_sha256":{"main":rng_sha(&sim.main_rng),"scenario":rng_sha(&sim.scenario_rng),"mapgen":rng_sha(&sim.mapgen_rng)},
    })
}

fn compare_fields(actual: &Value, expected: &Value, path: &str, errors: &mut Vec<String>) {
    if let Some(object) = actual.as_object() {
        for (key, value) in object {
            compare_fields(value, &expected[key], &format!("{path}/{key}"), errors);
        }
    } else if actual != expected {
        errors.push(format!("{path}: Rust {actual}, native {expected}"));
    }
}

fn prepared_paid_scene(stage: &str) -> (HeadlessScenario, AmbientAnims) {
    use crate::sim::world::bridge_test_evidence::{
        damage_anytown_concrete, repair_anytown_concrete,
    };
    let mut scene = load_anytown_concrete();
    let ambient_anims: AmbientAnims = scene
        .sim()
        .substrate
        .anims
        .iter()
        .map(|(id, _)| *id)
        .collect();
    if stage != "healthy" {
        assert!(!damage_anytown_concrete(&mut scene));
    }
    if matches!(stage, "collapsed" | "repaired") {
        assert!(damage_anytown_concrete(&mut scene));
    }
    if stage == "repaired" {
        repair_anytown_concrete(&mut scene);
    }
    // Both native and Rust start a fresh FV continuation on the produced
    // physical map state. Finish prerequisite effects before supplying
    // the declared frame/RNG inputs; this is not whole-Scenario startup.
    for _ in 0..512 {
        if scene
            .sim()
            .substrate
            .anims
            .iter()
            .all(|(id, _)| ambient_anims.contains(id))
            && scene.sim().substrate.pending_delete.is_empty()
        {
            break;
        }
        scene
            .runtime
            .advance_frame(&[], SIM_TICK_MS, TickLane::Ordinary)
            .unwrap();
    }
    assert!(
        scene
            .sim()
            .substrate
            .anims
            .iter()
            .all(|(id, _)| ambient_anims.contains(id)),
        "{stage} prerequisite effects drain"
    );
    assert!(
        scene.sim().substrate.pending_delete.is_empty(),
        "{stage} prerequisite deletions drain"
    );
    (scene, ambient_anims)
}

fn command_paid_fv(
    scene: &mut HeadlessScenario,
    xyz: [i32; 3],
    scenario_seed: u64,
    game_speed: u8,
) -> u64 {
    let sim = &mut scene.runtime.simulation;
    sim.session.binary_frame = 0;
    sim.session.tick = 0;
    // Match the declared original Options constructor input before any
    // measured constructor/AI. Normalized water-impact Anims use this
    // stored speed through native5FB2E0, independently of frame pacing.
    assert!(sim.session.game_options.apply_in_game_speed(game_speed));
    sim.main_rng = crate::sim::rng::SimRng::new(0);
    sim.mapgen_rng = crate::sim::rng::SimRng::new(0);
    sim.scenario_rng = crate::sim::rng::SimRng::new(scenario_seed);
    let id = spawn_fv(scene, xyz);
    // Native states[0] is the original Event receiver at frame1, before
    // its first Logic visit. Apply the same command owner at that boundary;
    // an extra frame0 AI visit would add a Guard cadence draw.
    let runtime = &mut scene.runtime;
    let sim = &mut runtime.simulation;
    sim.session.binary_frame = 1;
    let owner = sim.resolve(sim.session.current_house.unwrap()).to_owned();
    assert!(sim.apply_command_with_overlays(
        &owner,
        &Command::ForceAttackCell {
            attacker_id: id,
            target_rx: 87,
            target_ry: 54,
        },
        Some(&runtime.resources.rules),
        Some(&runtime.resources.overlay_registry),
    ));
    id
}

#[test]
#[ignore = "requires physical Anytown and retail TEMPERATE assets"]
fn retail_fv_paid_pursuit_fire_impacts_and_cleanup_match_native() {
    let input = std::env::var_os("VERA20K_FV_PAID_INPUT").map_or_else(
        || {
            crate::test_fixture::text(
                "tools/spatial_oracle/fv_cell_attack/paid_conditional_v26_vectors.json",
            )
            .to_owned()
        },
        |path| std::fs::read_to_string(path).unwrap(),
    );
    let packet: Value = serde_json::from_str(&input).unwrap();
    let mut errors = Vec::new();
    for case in packet["cases"].as_array().unwrap() {
        let stage = case["stage"].as_str().unwrap();
        let physical_stage = case["physical_stage"].as_str().unwrap_or(stage);
        let (mut scene, ambient_anims) = prepared_paid_scene(physical_stage);
        let id = command_paid_fv(
            &mut scene,
            serde_json::from_value(case["supplied_spawn"]["xyz"].clone()).unwrap(),
            case["boundary"]["scenario_seed"]
                .as_u64()
                .or_else(|| case["rng_initialization"]["scenario_seed"].as_u64())
                .expect("declared constructor Scenario seed"),
            case["options"]["game_speed"].as_u64().unwrap() as u8,
        );
        let mut actual_frames = Vec::new();
        let mut shots = Vec::new();
        let mut diagnostics = Vec::new();
        let mut draw_trace = Vec::new();
        let export_requested = std::env::var_os("VERA20K_FV_PAID_EXPORT").is_some();
        for (index, expected) in case["frames"].as_array().unwrap().iter().enumerate() {
            if index != 0 {
                let frame = scene.sim().session.binary_frame;
                let advance = || {
                    for command in case["followup_commands"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|command| command["frame"] == frame)
                    {
                        assert_eq!(command["kind"], "stop", "declared Event6 input");
                        let runtime = &mut scene.runtime;
                        let sim = &mut runtime.simulation;
                        let owner = sim
                            .resolve(sim.substrate.entities.get(id).unwrap().owner())
                            .to_owned();
                        assert!(sim.apply_command_with_overlays(
                            &owner,
                            &Command::Stop { entity_id: id },
                            Some(&runtime.resources.rules),
                            Some(&runtime.resources.overlay_registry),
                        ));
                    }
                    scene
                        .runtime
                        .advance_frame(&[], SIM_TICK_MS, TickLane::Ordinary)
                        .unwrap()
                };
                let output = if export_requested {
                    let (output, draws) = crate::sim::rng::trace_draws(advance);
                    draw_trace.push(json!({"frame":frame,"draws":draws}));
                    output
                } else {
                    let mut advance = advance;
                    advance()
                };
                // The runtime drains its owned event buffer into this output.
                for fire in output
                    .fire_events
                    .iter()
                    .filter(|fire| fire.attacker_id == id)
                {
                    shots.push(json!({"frame":frame,
                        "position":[fire.fire_coord.x,fire.fire_coord.y,fire.fire_coord.z]}));
                }
            }
            let actual = paid_snapshot(&scene, id, &ambient_anims);
            let mut expected = expected.clone();
            // Keep the native double bits in the corpus. Compare the retained
            // speed request at the production SimFixed precision; timers and
            // residual remain exact native integers. This catches the CRT
            // arrival defect (1.0 versus ~0.3) without demanding x87 storage.
            let native_speed = f64::from_bits(
                expected["track_state"]["target_speed_bits"]
                    .as_u64()
                    .unwrap(),
            );
            assert!(native_speed.is_finite() && (0.0..=1.0).contains(&native_speed));
            expected["track_state"]["target_speed_fixed_bits"] =
                json!(SimFixed::from_num(native_speed).to_bits());
            expected["live_bullet_positions"] = json!(
                expected["bullets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|b| b["alive"] == 1)
                    .map(|b| b["position"].clone())
                    .collect::<Vec<_>>()
            );
            compare_fields(
                &actual,
                &expected,
                &format!("{stage}/frame{index}"),
                &mut errors,
            );
            if export_requested {
                diagnostics.push(paid_diagnostic(&scene, id, &ambient_anims));
            }
            actual_frames.push(actual);
        }
        let expected_shots: Vec<_> = case["shots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|shot| json!({"frame":shot["frame"],"position":shot["position"]}))
            .collect();
        compare_fields(
            &json!(shots),
            &json!(expected_shots),
            &format!("{stage}/shots"),
            &mut errors,
        );
        if let Some(root) = std::env::var_os("VERA20K_FV_PAID_EXPORT") {
            let root = std::path::PathBuf::from(root);
            std::fs::create_dir_all(&root).unwrap();
            serde_json::to_writer_pretty(
                std::fs::File::create(root.join(format!("{stage}.json"))).unwrap(),
                &json!({"frames":actual_frames,"shots":shots,
                    "diagnostics":diagnostics,"draw_trace":draw_trace}),
            )
            .unwrap();
        }
    }
    assert!(
        errors.is_empty(),
        "{} native comparison differences; first twenty:\n{}",
        errors.len(),
        errors
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
#[ignore = "requires physical Anytown and retail TEMPERATE assets"]
fn retail_fv_approach_matches_native_candidates_admission_and_queue() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::combat::AttackTarget;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId};
    let packet: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fv_cell_attack/approach_vectors.json",
    ))
    .unwrap();
    let mut count = 0;
    for group in ["candidates", "admission", "queued"] {
        for row in packet[group].as_array().unwrap() {
            let name = row["input"]["name"].as_str().unwrap();
            let mut scene = load_anytown_concrete();
            scene.runtime.simulation.session.binary_frame = 173;
            scene.runtime.simulation.scenario_rng = crate::sim::rng::SimRng::new(31);
            let xyz = serde_json::from_value(row["source_xyz"].clone()).unwrap();
            let id = spawn_fv(&mut scene, xyz);
            let overridden_rules = if let Some(values) = row["input"].get("type_ini") {
                let (ini, art) = crate::rules::retail_ini_fixture::retail_rules_and_art().unwrap();
                let mut layers = RulesLayerStack::new(ini);
                layers.push(RulesLayerKind::Scenario, scene.map.ini.clone());
                let mut text = "[FV]\n".to_owned();
                for (key, value) in values.as_object().unwrap() {
                    text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
                }
                layers.push(RulesLayerKind::Scenario, IniFile::from_str(&text));
                let mut rules =
                    RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art).unwrap())
                        .unwrap();
                rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
                Some(rules)
            } else {
                None
            };
            let runtime = &mut scene.runtime;
            let rules = overridden_rules
                .as_ref()
                .unwrap_or(&runtime.resources.rules);
            let sim = &mut runtime.simulation;
            sim.resolve_type_handles(&rules);
            let actor = sim.substrate.entities.get_mut(id).unwrap();
            actor.attack_target = Some(AttackTarget::for_cell(87, 54));
            actor.in_playfield = row["input"].get("in_playfield").is_none_or(|v| v == 1);
            actor.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_raw(row["before"]["mission"].as_i64().unwrap() as i32),
                suspended: MissionId::NONE,
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 173,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::from_raw(173, 0),
            });
            if let Some(value) = row["input"].get("retained_nav") {
                let (rx, ry) = cell(value);
                assert!(sim.set_unit_destination(id, NavTargetRef::cell(rx, ry), &rules, true));
            }
            if let Some(values) = row["input"].get("nav_queue") {
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .navigation
                    .nav_queue = values
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| {
                        let (x, y) = cell(value);
                        NavTargetRef::cell(x, y)
                    })
                    .collect();
            }
            // Declared raw occupation inputs match the native controls; these
            // rows test the reader and search, not the mask producer.
            if let Some(values) = row["input"].get("ground_occupation") {
                for value in values.as_array().unwrap() {
                    let (x, y) = cell(value);
                    sim.substrate.raw_cell_occupation.mark_ground(
                        x,
                        y,
                        value[2].as_u64().unwrap() as u8,
                    );
                }
            }
            if let Some(rect) = row["input"].get("block_rectangle") {
                for y in rect[1].as_u64().unwrap()..=rect[3].as_u64().unwrap() {
                    for x in rect[0].as_u64().unwrap()..=rect[2].as_u64().unwrap() {
                        sim.substrate
                            .raw_cell_occupation
                            .mark_ground(x as u16, y as u16, 0x40);
                        sim.substrate
                            .raw_cell_occupation
                            .mark_deck(x as u16, y as u16, 0x40);
                    }
                }
            }
            let before = (
                sim.main_rng.logical_state(),
                sim.scenario_rng.logical_state(),
                sim.mapgen_rng.logical_state(),
            );
            let actor = sim.substrate.entities.get(id).unwrap();
            assert_eq!(
                mission_state(actor),
                row["before_mission_state"],
                "{name} supplied mission state"
            );
            let target_before = actor.attack_target.as_ref().map(|attack| attack.target);
            let result = sim
                .approach_unit_cell_target(id, &rules, Some(&runtime.resources.overlay_registry))
                .unwrap();
            let actor = sim.substrate.entities.get(id).unwrap();
            assert_eq!(
                mission_state(actor),
                row["before_mission_state"],
                "{name} direct Approach preserves mission/rearm state"
            );
            assert_eq!(
                actor.attack_target.is_some(),
                row["target_present"].as_bool().unwrap(),
                "{name} target presence"
            );
            assert_eq!(
                actor.attack_target.as_ref().map(|attack| attack.target) == target_before,
                row["target_unchanged"].as_bool().unwrap(),
                "{name} target identity"
            );
            assert_eq!(
                nav(actor.navigation.nav_com),
                row["nav_cell"],
                "{group}/{name} destination"
            );
            assert_eq!(
                coordinate(
                    actor
                        .locomotor
                        .as_ref()
                        .and_then(|l| l.selected_drive_runtime())
                        .and_then(|r| r.retained())
                        .and_then(|d| d.destination())
                ),
                row["drive_destination"],
                "{name} Drive destination"
            );
            assert_eq!(
                coordinate(
                    actor
                        .locomotor
                        .as_ref()
                        .and_then(|l| l.selected_drive_runtime())
                        .and_then(|r| r.retained())
                        .and_then(|d| d.head_to())
                ),
                row["drive_head"],
                "{name} paid head"
            );
            let timing = actor.navigation.path_runtime;
            assert_eq!(
                json!([
                    timing.movement_timer.start_frame(),
                    timing.movement_timer.duration()
                ]),
                row["movement_timer"],
                "{name} movement timer"
            );
            assert_eq!(
                json!([
                    timing.blocked_timer.start_frame(),
                    timing.blocked_timer.duration()
                ]),
                row["blockage_timer"],
                "{name} blockage timer"
            );
            if let Some(expected) = row.get("target_cell") {
                let actual = actor
                    .attack_target
                    .as_ref()
                    .map(|a| match a.target {
                        crate::sim::combat::TargetKind::Cell(x, y) => json!([x, y]),
                        other => panic!("unexpected target {other:?}"),
                    })
                    .unwrap_or(Value::Null);
                assert_eq!(actual, *expected, "{name} TarCom");
            }
            if let Some(expected) = row.get("nav_queue_cells") {
                let actual: Vec<_> = actor
                    .navigation
                    .nav_queue
                    .iter()
                    .map(|target| nav(Some(*target)))
                    .collect();
                assert_eq!(json!(actual), *expected, "{name} remaining queue");
            }
            assert_eq!(
                result.is_some(),
                row["approach_returned_eax"] != 0,
                "{name} returned Cell"
            );
            assert_eq!(
                (
                    sim.main_rng.logical_state(),
                    sim.scenario_rng.logical_state(),
                    sim.mapgen_rng.logical_state()
                ),
                before,
                "{name} Approach must not draw RNG"
            );
            count += 1;
        }
    }
    assert_eq!(count, 21);
}

#[test]
#[ignore = "requires unmodified physical Anytown and retail TEMPERATE assets"]
fn retail_fv_approaches_a_firing_cell_before_its_first_paid_step() {
    let packet: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fv_cell_attack/pursuit_vectors.json",
    ))
    .unwrap();
    let witness = packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "healthy_maximum_outside_two")
        .unwrap();
    let xyz: [i32; 3] = serde_json::from_value(witness["source_xyz"].clone()).unwrap();
    let destination: [u16; 2] = serde_json::from_value(witness["nav_cell"].clone()).unwrap();
    let mut scene = load_anytown_concrete();
    let runtime = &mut scene.runtime;
    let owner = runtime.simulation.session.current_house.unwrap();
    let owner_name = runtime.simulation.resolve(owner).to_owned();
    let id = runtime
        .simulation
        .spawn_object_with_overlay_registry(
            "FV",
            &owner_name,
            (xyz[0] / 256) as u16,
            (xyz[1] / 256) as u16,
            128,
            &runtime.resources.rules,
            &runtime.resources.overlay_registry,
        )
        .expect("ordinary FV bank placement");
    // Supplied native starting XYZ, in the same already-admitted Cell. Map
    // membership, rules, graph construction, command and AI remain production.
    let actor = runtime.simulation.substrate.entities.get_mut(id).unwrap();
    actor.position.sub_x = SimFixed::from_num(xyz[0] % 256);
    actor.position.sub_y = SimFixed::from_num(xyz[1] % 256);
    actor.position.exact_z_leptons = Some(xyz[2]);
    let command = CommandEnvelope::new(
        owner,
        runtime.simulation.session.tick + 1,
        Command::ForceAttackCell {
            attacker_id: id,
            target_rx: 87,
            target_ry: 54,
        },
    );
    runtime
        .advance_frame(&[command], SIM_TICK_MS, TickLane::Ordinary)
        .unwrap();
    runtime
        .advance_frame(&[], SIM_TICK_MS, TickLane::Ordinary)
        .unwrap();
    let actor = runtime.simulation.substrate.entities.get(id).unwrap();
    assert_eq!(
        actor.navigation.nav_com,
        Some(NavTargetRef::cell(destination[0], destination[1])),
        "native MissionAttack chooses a firing cell before the same object's Drive Process"
    );
    assert_eq!(
        actor.attack_target.as_ref().unwrap().target,
        crate::sim::combat::TargetKind::Cell(87, 54)
    );
    assert!(
        actor.position.sub_y > SimFixed::from_num(xyz[1] % 256),
        "Drive must pay movement during the first Attack object visit"
    );
}
