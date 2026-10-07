//! Original automatic Guard-family producer and class caller comparisons.
//! Supplied prior state is shared with the existing Stop/action fixture;
//! native outputs establish expected behavior, timers and complete RNG state.

use serde_json::Value;

use super::{dispatch_foot_mission, infantry_automatic_guard_delay};
use crate::sim::deploy_tests::{assert_native_deploy_state, native_deploy_fixture};
use crate::sim::mission::{MissionDispatchTimer, MissionId};

pub(super) fn corpus() -> Value {
    let meta: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_auto_deploy.meta.json",
    ))
    .unwrap();
    assert_eq!(
        meta["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
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
        let now = sim.session.binary_frame;
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
                infantry_automatic_guard_delay(&mut sim, id, &rules, mission),
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
    restored.finish_walk_navigation(id, Some(&rules)).unwrap();
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
fn retail_gi_and_guardian_gi_complete_deploy_in_bound_runtime() {
    use crate::map::resolved_terrain::test_flat_ground_grid;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::house_state::{HouseDifficulty, HouseState};
    use crate::sim::runtime::{SimResources, SimRuntime};
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
    sim.session.game_mode_nonzero = true;
    // Supply the Clear/Foot row that ordinary Infantry Unlimbo reads before
    // these retail automatic-deploy frames begin.
    let terrain = test_flat_ground_grid(64);
    sim.resolved_terrain = Some(terrain.clone());
    for (name, human) in [("Americans", false), ("French", true)] {
        let owner = sim.interner.intern(name);
        let mut house = HouseState::new(owner, 0, None, human, 0, 10);
        house.set_difficulty(HouseDifficulty::Normal, &rules.general, 1.0, true, 0, 0);
        sim.houses.insert(owner, house);
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
        runtime.advance_frame(&[], 22, TickLane::Ordinary).unwrap();
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
}
