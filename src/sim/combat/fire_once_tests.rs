//! Original6FF749 FireOnce continuations. The native corpus supplies the
//! admitted caller boundary, prior team orders and launch outcome. These
//! comparisons exercise the production shared tail, not shot admission,
//! Bullet destruction, bomb attachment or the complete native Logic walk.

use super::*;
use crate::sim::components::NavTargetRef;
use crate::sim::mission::MissionId;
use crate::sim::rng::{SimRng, trace_draws};
use crate::sim::snapshot::GameSnapshot;
use crate::sim::team_script_vm::{TeamScriptAction, TeamScriptDefinition, TeamTarget};
use crate::sim::timer::CdTimer;
use serde_json::{Value, json};

fn integer(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn target(value: &Value, targets: [u64; 2]) -> Option<TargetKind> {
    match value.as_str() {
        None if value.is_null() => None,
        Some("old_target") => Some(TargetKind::Entity(targets[0])),
        Some("other_target") => Some(TargetKind::Entity(targets[1])),
        _ => panic!("undeclared native pointer label {value}"),
    }
}

fn label(target: Option<TargetKind>, targets: [u64; 2]) -> Value {
    match target {
        None => Value::Null,
        Some(TargetKind::Entity(id)) if id == targets[0] => json!("old_target"),
        Some(TargetKind::Entity(id)) if id == targets[1] => json!("other_target"),
        _ => panic!("unexpected represented target {target:?}"),
    }
}

fn timer(value: &Value) -> CdTimer {
    // The middle native word is copied stack residue, outside CdTimer's
    // authoritative start/duration state.
    CdTimer::from_raw(integer(&value[0]), integer(&value[2]))
}

fn fixture(row: &Value, rules: &RuleSet) -> (Simulation, Vec<u64>, [u64; 2], Option<u64>) {
    let mut sim = Simulation::with_seed(1);
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    // Keep the native world-lepton poses without granting this tail fixture
    // a claim about the original map's terrain or preceding placement.
    let template = sim.resolved_terrain.as_ref().unwrap().cells()[0].clone();
    let cells = (0..128)
        .flat_map(|ry| (0..128).map(move |rx| (rx, ry)))
        .map(|(rx, ry)| {
            let mut cell = template.clone();
            cell.rx = rx;
            cell.ry = ry;
            cell
        })
        .collect();
    sim.install_resolved_terrain_for_new_map(
        crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(128, 128, cells),
    );
    sim.session.set_map_width_for_test(128);
    sim.session.set_map_height_for_test(128);
    assert!(sim.rebuild_dynamic_navigation(rules));
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    sim.session
        .set_binary_frame_for_test(integer(&row["supplied"]["frame"]) as u32);
    let targets = [(20, 20), (22, 20)].map(|(rx, ry)| {
        sim.spawn_object_at_height("HTNK", "Americans", rx, ry, 0, 0, rules)
            .unwrap()
    });
    let members = row["before"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|before| {
            let xyz = &before["position"];
            let id = sim
                .spawn_object_at_height(
                    before["type_id"].as_str().unwrap(),
                    "Americans",
                    (integer(&xyz[0]) >> 8) as u16,
                    (integer(&xyz[1]) >> 8) as u16,
                    0,
                    0,
                    rules,
                )
                .unwrap();
            sim.substrate
                .mission_assign_exact(id, MissionId::from_raw(integer(&before["mission"])), 0)
                .unwrap();
            assert_eq!(before["queued_mission"], -1);
            assert_eq!(before["burst"], 0);
            assert_eq!(before["scan"], 0);
            assert_eq!(before["nav_queue_count"], 0);
            let actor = sim.substrate.entities.get_mut(id).unwrap();
            actor
                .position
                .set_sub_x_for_test(SimFixed::from_num(integer(&xyz[0]) & 255));
            actor
                .position
                .set_sub_y_for_test(SimFixed::from_num(integer(&xyz[1]) & 255));
            actor
                .position
                .set_exact_z_leptons_for_test(Some(integer(&xyz[2])));
            actor.health.set_for_test(integer(&before["health"]));
            actor
                .lifecycle
                .set_object_alive_for_test(integer(&before["alive"]) != 0);
            actor
                .lifecycle
                .set_in_limbo_for_test(integer(&before["limbo"]) != 0);
            actor
                .mission
                .set_handler_state(integer(&before["status"]) as u32);
            actor.mission.write_dispatch_epilogue(
                integer(&before["dispatch_timer"][0]),
                integer(&before["dispatch_timer"][2]),
            );
            actor
                .rearm_timer
                .set_for_test(timer(&before["rearm_timer"]));
            actor
                .attack_target
                .set_for_test(target(&before["target"], targets).map(|value| match value {
                    TargetKind::Entity(id) => TargetKind::Entity(id),
                    _ => unreachable!(),
                }));
            actor.set_archive_target(target(&before["archive"], targets));
            actor
                .navigation
                .set_nav_com_for_test(target(&before["destination"], targets).map(|value| {
                    let TargetKind::Entity(id) = value else {
                        unreachable!()
                    };
                    NavTargetRef::object(id)
                }));
            actor
                .navigation
                .path_runtime_mut()
                .set_movement_timer_for_test(timer(&before["movement_timer"]));
            actor.navigation.path_replay_mut().set_directions_for_test(
                before["path"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .take_while(|value| integer(value) != -1)
                    .map(|value| u8::try_from(integer(value)).unwrap())
                    .collect(),
            );
            actor
                .mission_leaf
                .set_foot_firing_sequence(integer(&before["firing"]) as u8);
            if !before["doing"].is_null() {
                actor
                    .mission_leaf
                    .set_infantry_doing_verified(integer(&before["doing"]))
                    .unwrap();
            }
            actor.install_native_stage_fixture(crate::sim::stage::StageClass::from_native_fixture(
                integer(&before["frame_f8"]),
                0,
                timer(&before["sequence_timer"]),
                integer(&before["sequence_timer"][3]),
                1,
            ));
            id
        })
        .collect::<Vec<_>>();
    let team = if row["before"]["team"].is_null() {
        None
    } else {
        let script = sim.interner.intern("FireOnce fixture");
        sim.team_script_vm.register_script(TeamScriptDefinition {
            id: script,
            source: crate::rules::team_ai_ini::TeamAiDefinitionSource::Scenario,
            actions: vec![
                TeamScriptAction {
                    action_id: 63,
                    argument: 0,
                },
                TeamScriptAction {
                    action_id: 49,
                    argument: 0,
                },
            ],
        });
        let id = sim
            .team_script_vm
            .create_team(owner, script, members.clone(), 0);
        // Supply the native prior state through the existing serialized owner
        // contract; production visibility and mutation authority stay private.
        let mut vm = serde_json::to_value(&sim.team_script_vm).unwrap();
        let team = &mut vm["teams"][id.to_string()];
        for field in ["mission_target", "focus"] {
            let value = target(&row["before"]["team"][field], targets).map(|value| {
                let TargetKind::Entity(id) = value else {
                    unreachable!()
                };
                TeamTarget::Object(id)
            });
            team[field] = serde_json::to_value(value).unwrap();
        }
        team["advance_pending"] = row["before"]["team"]["advance_pending"].clone();
        team["leaving_map"] = row["before"]["team"]["is_leaving_map"].clone();
        sim.team_script_vm = serde_json::from_value(vm).unwrap();
        Some(id)
    };
    sim.main_rng = serde_json::from_value(row["rng_before"]["main"].clone()).unwrap();
    sim.scenario_rng = serde_json::from_value(row["rng_before"]["scenario"].clone()).unwrap();
    sim.mapgen_rng = serde_json::from_value(row["rng_before"]["mapgen"].clone()).unwrap();
    (sim, members, targets, team)
}

fn assert_boundary(
    sim: &Simulation,
    row: &Value,
    members: &[u64],
    targets: [u64; 2],
    team: Option<u64>,
) {
    let name = row["name"].as_str().unwrap();
    for (&id, after) in members
        .iter()
        .zip(row["after"]["members"].as_array().unwrap())
    {
        let actor = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            label(actor.attack_target.target(), targets),
            after["target"],
            "{name}: target"
        );
        let nav = actor.navigation.nav_com().map(TargetKind::from);
        assert_eq!(label(nav, targets), after["destination"], "{name}: NavCom");
        assert_eq!(
            label(actor.archive_target(), targets),
            after["archive"],
            "{name}: archive"
        );
        assert_eq!(
            actor.mission.current().raw(),
            integer(&after["mission"]),
            "{name}: current"
        );
        assert_eq!(
            actor.mission.queued().raw(),
            integer(&after["queued_mission"]),
            "{name}: queued"
        );
        assert_eq!(
            actor.mission.handler_state(),
            integer(&after["status"]) as u32,
            "{name}: status"
        );
        assert_eq!(
            actor.weapon_burst.index(),
            integer(&after["burst"]),
            "{name}: burst"
        );
        assert_eq!(
            actor.rearm_timer.timer(),
            timer(&after["rearm_timer"]),
            "{name}: rearm"
        );
        assert_eq!(
            [
                actor.mission.dispatch_timer().start_frame(),
                actor.mission.dispatch_timer().delay()
            ],
            [
                integer(&after["dispatch_timer"][0]),
                integer(&after["dispatch_timer"][2])
            ],
            "{name}: dispatch"
        );
        assert_eq!(
            actor.navigation.path_runtime().movement_timer(),
            timer(&after["movement_timer"]),
            "{name}: movement"
        );
        assert_eq!(
            actor.navigation.nav_queue().len(),
            after["nav_queue_count"].as_u64().unwrap() as usize,
            "{name}: NavQueue"
        );
        assert_eq!(
            actor.mission_leaf.foot_firing_sequence_latch(),
            integer(&after["firing"]) as u8,
            "{name}: firing"
        );
        if let Some(leaf) = actor.mission_leaf.as_infantry() {
            assert_eq!(leaf.doing(), integer(&after["doing"]), "{name}: Doing");
        }
        assert_eq!(
            actor.native_stage().value(),
            integer(&after["frame_f8"]),
            "{name}: Stage"
        );
        assert_eq!(
            actor.native_stage().timer(),
            timer(&after["sequence_timer"]),
            "{name}: Stage timer"
        );
        assert_eq!(
            actor.native_stage().rate(),
            integer(&after["sequence_timer"][3]),
            "{name}: Stage rate"
        );
        let directions: Vec<_> = after["path"]
            .as_array()
            .unwrap()
            .iter()
            .take_while(|value| integer(value) != -1)
            .map(|value| u8::try_from(integer(value)).unwrap())
            .collect();
        assert_eq!(
            actor.navigation.path_replay().remaining_directions(),
            directions,
            "{name}: path"
        );
    }
    if let Some(id) = team {
        let state = serde_json::to_value(sim.team_script_vm.team(id).unwrap()).unwrap();
        for field in ["mission_target", "focus"] {
            let value: Option<TeamTarget> = serde_json::from_value(state[field].clone()).unwrap();
            assert_eq!(
                label(value.map(TeamTarget::target_kind), targets),
                row["after"]["team"][field],
                "{name}: team {field}"
            );
        }
        assert_eq!(
            state["advance_pending"], row["after"]["team"]["advance_pending"],
            "{name}: pending"
        );
        assert_eq!(
            state["leaving_map"], row["after"]["team"]["is_leaving_map"],
            "{name}: leaving"
        );
        assert_eq!(
            state["cursor"], 0,
            "{name}: the shot does not advance the cursor"
        );
    }
    for (stream, rng) in [
        ("main", sim.main_rng.logical_state()),
        ("scenario", sim.scenario_rng.logical_state()),
        ("mapgen", sim.mapgen_rng.logical_state()),
    ] {
        let native: SimRng = serde_json::from_value(row["rng_after"][stream].clone()).unwrap();
        assert_eq!(rng, native.logical_state(), "{name}: {stream} RNG");
    }
}

#[test]
fn fire_once_tail_matches_12_original_retained_state_and_rng_boundaries() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/fire_once.json",
    ))
    .unwrap();
    assert_eq!(
        corpus["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    assert_eq!(corpus["cases"].as_array().unwrap().len(), 13);
    let mut compared = 0;
    for row in corpus["cases"].as_array().unwrap() {
        // A constructed Infantry without its Foot bit is a native control,
        // not a representable production object in this chain.
        if row["supplied"]["foot_flag"] == false {
            continue;
        }
        assert_eq!(row["native_text_unchanged"], true);
        assert_eq!(row["native_vtables_unchanged"], true);
        assert_eq!(row["rng_calls"], json!([]));
        let (mut sim, members, targets, team) = fixture(row, &retail.rules);
        let source = sim.substrate.entities.get(members[0]).unwrap();
        let snap = build_attacker_snapshot(source, TargetKind::Entity(targets[0]), None);
        let object = retail
            .rules
            .object(row["supplied"]["source"].as_str().unwrap())
            .unwrap();
        let mut weapon = retail
            .rules
            .weapon(object.primary().unwrap())
            .unwrap()
            .clone();
        assert!(weapon.fire_once, "stock caller's reader");
        weapon.fire_once = row["supplied"]["fire_once"].as_bool().unwrap();
        let bullet = row["return_bullet"].as_bool().unwrap().then_some(u64::MAX);
        let (_, draws) = trace_draws(|| {
            fireat_tail(
                &mut sim,
                &retail.rules,
                &snap,
                &weapon,
                bullet,
                None,
                crate::sim::world::FrameEffects::default(),
            )
        });
        assert!(draws.is_empty());
        assert_boundary(&sim, row, &members, targets, team);
        if row["name"] == "ivan_team_clear_old_focus" {
            let bytes = GameSnapshot::save_validated(&sim, 0, 0, "FireOnce", 0);
            let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
            restored.restore_after_snapshot_load().unwrap();
            // Keep the separate save reader's Scenario reseed out of this
            // FireOnce retained-state comparison.
            restored.scenario_rng = sim.scenario_rng.clone();
            assert_boundary(&restored, row, &members, targets, team);
            restored.run_team_ai_pass(
                &retail.rules,
                None,
                crate::sim::world::FrameEffects::default(),
            );
            let state = restored.team_script_vm.team(team.unwrap()).unwrap();
            assert_eq!(state.cursor(), 1);
            assert!(
                state.succeeded(),
                "the queued completion reaches the next script action"
            );
        }
        compared += 1;
    }
    assert_eq!(compared, 12);
}
