//! Original automatic Guard-family producer and class caller comparisons.
//! Supplied prior state is shared with the existing Stop/action fixture;
//! native outputs establish expected behavior, timers and complete RNG state.

use serde_json::Value;

use super::{dispatch_foot_mission, infantry_automatic_guard_delay};
use crate::sim::deploy_tests::{assert_native_deploy_state, native_deploy_fixture};
use crate::sim::mission::{MissionDispatchTimer, MissionId};

pub(super) fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_auto_deploy.json",
    ))
    .unwrap()
}

#[test]
fn native_automatic_guard_admission_stop_order_and_signed_caller_returns() {
    let data = corpus();
    let rows = data["automatic_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 90);
    for row in rows {
        let (mut sim, rules, id) = native_deploy_fixture(row);
        let now = sim.session.binary_frame();
        // The original receipt starts at the class handler, after MissionAI's
        // due gate. Supply that admission through the timer's owning fixture.
        let actor = sim.substrate.entities.get_mut(id).unwrap();
        actor.mission.write_dispatch_epilogue(now as i32, 0);
        let expected = row["return_signed"].as_i64().unwrap() as i32;
        if row["input"]["caller"] == false {
            let mission = MissionId::from_raw(row["input"]["mission"].as_i64().unwrap() as i32)
                .known()
                .unwrap();
            assert_eq!(
                infantry_automatic_guard_delay(
                    &mut sim,
                    id,
                    &rules,
                    mission,
                    crate::sim::world::FrameEffects::default()
                ),
                expected
            );
        } else {
            dispatch_foot_mission(&mut sim, id, &rules, Default::default());
            assert_eq!(
                sim.substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .mission
                    .dispatch_timer(),
                MissionDispatchTimer::from_raw(now as i32, expected),
                "{}: class caller's single timer epilogue",
                row["input"]["name"]
            );
        }
        assert_native_deploy_state(&sim, id, row);
        assert_eq!(row["original_text_and_vtables_unchanged"], true);
    }
}

#[test]
fn pending_deploy_survives_snapshot_and_paid_head_completion() {
    use crate::sim::snapshot::GameSnapshot;

    let data = corpus();
    let row = data["automatic_rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "Guard_moving_paid_head")
        .unwrap();
    let (mut sim, rules, id) = native_deploy_fixture(row);
    dispatch_foot_mission(&mut sim, id, &rules, Default::default());
    assert_native_deploy_state(&sim, id, row);
    let pending_hash = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mission_leaf
        .set_infantry_pending_deploy(0);
    assert_ne!(sim.state_hash(), pending_hash);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mission_leaf
        .set_infantry_pending_deploy(1);
    let bytes = GameSnapshot::save(&sim, 0, 0, "automatic-deploy", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    let actor = restored.substrate.entities.get(id).unwrap();
    assert_eq!(
        actor.mission_leaf.as_infantry().unwrap().pending_deploy(),
        1
    );
    assert!(actor.locomotor.as_ref().unwrap().step_head().is_some());
    assert!(
        actor
            .locomotor
            .as_ref()
            .unwrap()
            .walk_destination()
            .is_none()
    );
    // The existing paid-head completion owner releases Head_To and invokes
    // the original-compared Stop callback. Orders/restore add no latch reset.
    restored
        .finish_walk_navigation(id, Some(&rules), crate::sim::world::FrameEffects::default())
        .unwrap();
    let actor = restored.substrate.entities.get(id).unwrap();
    assert_eq!(
        actor.mission_leaf.as_infantry().unwrap().pending_deploy(),
        0
    );
    assert_eq!(actor.mission_leaf.as_infantry().unwrap().doing(), 27);
    assert!(actor.locomotor.as_ref().unwrap().step_head().is_none());
    assert!(actor.mission_leaf.as_unit().is_none());
}

#[test]
fn retail_gi_and_guardian_gi_deploy_then_team_move_in_bound_runtime() {
    use crate::map::resolved_terrain::test_flat_ground_grid;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::rules::team_ai_ini::TeamAiDefinitionSource;
    use crate::sim::house_state::{HouseDifficulty, HouseState};
    use crate::sim::mission::MissionType;
    use crate::sim::runtime::{SimResources, SimRuntime};
    use crate::sim::team_script_vm::{TeamScriptAction, TeamScriptDefinition};
    use crate::sim::world::{Simulation, TickLane};

    let Some(rules_bytes) = crate::rules::retail_ini_fixture::retail_ini_bytes("rulesmd.ini")
    else {
        return;
    };
    let Some(art_bytes) = crate::rules::retail_ini_fixture::retail_ini_bytes("artmd.ini") else {
        return;
    };
    let ini = IniFile::from_bytes(&rules_bytes).unwrap();
    let art = IniFile::from_bytes(&art_bytes).unwrap();
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    assert_eq!(rules.general.ai_auto_deploy_frame_delay, [15, 25, 100]);
    for name in ["E1", "GGI"] {
        let object = rules.object(name).unwrap();
        assert!(object.deployer && object.deploy_fire && !object.immune_to_radiation);
        assert_eq!(object.undeploy_delay, -1);
        assert_eq!(object.deploy_fire_weapon, 1);
        assert!(!object.jumpjet);
    }
    assert!(!rules.object("E2").unwrap().deployer);
    assert_eq!(
        rules
            .animation_sequence("E1")
            .unwrap()
            .infantry_action(27)
            .unwrap()
            .frames_per_facing,
        15
    );
    let mut sim = Simulation::with_seed(31);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    // A normal game: its unit totals keep the buildingless Houses undefeated.
    sim.session.game_options_mut_for_test().short_game = false;
    // Supply the Clear/Foot row that ordinary Infantry Unlimbo reads before
    // these retail automatic-deploy frames begin.
    let terrain = test_flat_ground_grid(64);
    sim.session.set_map_width_for_test(64);
    sim.session.set_map_height_for_test(64);
    sim.install_resolved_terrain_for_new_map(terrain.clone());
    sim.playfield_bounds = Some(crate::sim::arena_fixture::OPEN_PLAYFIELD);
    sim.playfield_size_height = Some(20);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    // Funds and a factory keep the computer's Building strategy from selling
    // off and sending every unit to Hunt.
    for (name, human) in [("Americans", false), ("French", true)] {
        let owner = sim.interner.intern(name);
        let mut house = HouseState::new(owner, 0, None, human, 10_000, 10);
        house.set_difficulty(
            HouseDifficulty::Normal,
            &rules.general,
            Default::default(),
            true,
            0,
            0,
        );
        sim.houses.register(house);
    }
    let gi = sim
        .spawn_object("E1", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let guardian = sim
        .spawn_object("GGI", "Americans", 12, 10, 0, &rules)
        .unwrap();
    let conscript = sim
        .spawn_object("E2", "Americans", 20, 20, 0, &rules)
        .unwrap();
    let human = sim.spawn_object("E1", "French", 40, 40, 0, &rules).unwrap();
    sim.spawn_object("GAPILE", "Americans", 50, 50, 0, &rules)
        .unwrap();
    sim.resolve_type_handles(&rules);
    let mut resources = SimResources::empty();
    resources.rules = rules;
    resources.terrain_template = Some(terrain);
    let mut runtime = SimRuntime {
        simulation: sim,
        resources,
    };
    let mut entered = [false; 2];
    for _ in 0..250 {
        runtime
            .advance_frame(
                &[],
                22,
                TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        for (index, id) in [gi, guardian].into_iter().enumerate() {
            let actor = runtime.simulation.substrate.entities.get(id).unwrap();
            entered[index] |= actor.mission_leaf.as_infantry().unwrap().doing() == 27;
            assert!(actor.mission_leaf.as_unit().is_none());
        }
    }
    assert_eq!(
        entered,
        [true, true],
        "both production AI visits reached Deploy27"
    );
    for id in [gi, guardian] {
        let actor = runtime.simulation.substrate.entities.get(id).unwrap();
        assert_eq!(actor.mission_leaf.as_infantry().unwrap().doing(), 28);
        assert_eq!(
            actor.mission_leaf.as_infantry().unwrap().pending_deploy(),
            0
        );
    }
    for id in [conscript, human] {
        assert!(
            !runtime
                .simulation
                .substrate
                .entities
                .get(id)
                .unwrap()
                .infantry_deploy_doing()
        );
    }

    assert!(
        !runtime
            .resources
            .rules
            .object("GGI")
            .unwrap()
            .deployed_crushable
    );
    assert_eq!(
        runtime
            .simulation
            .substrate
            .entities
            .get(guardian)
            .unwrap()
            .native_crush_immunity(),
        1,
        "the real Deploy27 completion granted the Guardian GI's retained immunity"
    );
    // A formed team sends each already-deployed member through real script3
    // and CoordinateMove6EBAD0: QueueMove6EBE24, Ready6EBE2E,
    // Commence6EBE3C, then class destination6EBE56. This supplies only the
    // team membership and script cursor; Doing, Stage and deployment state
    // above and below are produced by ordinary bound-runtime frames.
    let waypoints = crate::map::waypoints::parse_waypoints(&IniFile::from_str(
        "[Waypoints]\n0=10020\n1=10022\n",
    ));
    runtime.simulation.session.install_waypoints(&waypoints);
    for (waypoint, id) in [gi, guardian].into_iter().enumerate() {
        let sim = &mut runtime.simulation;
        let script = sim.interner.intern(&format!("DEPLOYED_MOVE_{waypoint}"));
        sim.team_script_vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::Scenario,
            actions: vec![TeamScriptAction {
                action_id: 3,
                argument: waypoint as i32,
            }],
        });
        let owner = sim.substrate.entities.get(id).unwrap().owner();
        let team = sim.team_script_vm.create_team(
            owner,
            script,
            vec![id],
            sim.session.binary_frame() as i32,
        );
        sim.team_script_vm.seek_line_for_test(team, 0);
    }
    let mut entered_undeploy = [false; 2];
    let mut completed_undeploy = [false; 2];
    let mut arrived = [false; 2];
    for _ in 0..600 {
        runtime
            .advance_frame(
                &[],
                22,
                TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        for (index, id) in [gi, guardian].into_iter().enumerate() {
            if arrived[index] {
                continue;
            }
            let actor = runtime.simulation.substrate.entities.get(id).unwrap();
            let doing = actor.mission_leaf.as_infantry().unwrap().doing();
            if doing == 31 {
                entered_undeploy[index] = true;
                assert_eq!(
                    actor.native_crush_immunity(),
                    u8::from(id == guardian),
                    "Undeploy31 retains crush immunity until its own completion"
                );
            }
            if entered_undeploy[index] && !completed_undeploy[index] && matches!(doing, 0 | 3) {
                assert_eq!(
                    actor.native_crush_immunity(),
                    0,
                    "Undeploy31 completion revoked immunity before normal walking"
                );
                completed_undeploy[index] = true;
            }
            // Guard's idle action and automatic deployment can replace Doing
            // by the time navigation settles; their poses do not own arrival.
            if completed_undeploy[index] {
                let at_destination =
                    (actor.position.rx(), actor.position.ry()) == (20 + index as u16 * 2, 10);
                let stopped = crate::sim::movement::motion_query::is_moving(actor) == Some(false);
                arrived[index] = at_destination
                    && stopped
                    && actor.navigation.nav_com().is_none()
                    && matches!(
                        actor.mission.effective().known(),
                        Some(MissionType::Guard | MissionType::AreaGuard)
                    );
            }
        }
        if arrived == [true, true] {
            break;
        }
    }
    assert_eq!(
        entered_undeploy,
        [true, true],
        "both stock infantry took the AI Move wrapper's Undeploy31 request"
    );
    assert_eq!(
        completed_undeploy,
        [true, true],
        "both completed Undeploy31 and restored ordinary crush admission"
    );
    assert_eq!(
        arrived,
        [true, true],
        "both walked to their team's waypoint, cleared NavCom and returned to Guard: {:?}",
        [gi, guardian].map(|id| {
            let sim = &runtime.simulation;
            let actor = sim.substrate.entities.get(id).unwrap();
            let loco = actor.locomotor.as_ref().unwrap();
            let team = sim
                .team_script_vm
                .team_for_member(id)
                .and_then(|(team, _)| sim.team_script_vm.team(team))
                .map(|team| (team.id(), team.cursor(), team.advance_pending(), team.formed(), team.refusal()));
            format!(
                "id={id} frame={} position={:?} Doing={} current={:?} queued={:?} NavCom={:?} destination={:?} head={:?} moving={:?} immunity={} team={team:?}",
                sim.session.binary_frame(),
                crate::sim::movement::ground_pose::position_world_coord(&actor.position),
                actor.mission_leaf.as_infantry().unwrap().doing(),
                actor.mission.current(),
                actor.mission.queued(),
                actor.navigation.nav_com(),
                loco.walk_destination(),
                loco.step_head(),
                crate::sim::movement::motion_query::is_moving(actor),
                actor.native_crush_immunity(),
            )
        })
    );
}
