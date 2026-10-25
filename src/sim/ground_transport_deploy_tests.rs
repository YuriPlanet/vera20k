//! Unit transport D -> Event9 -> Queue/Commence -> empty Unload admission.
//! Native controls: transport_unload.py --deploy-event. This is a component
//! comparison; Ready/Logic scheduling and loaded passenger placement are not
//! supplied as native goldens.

use super::*;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
use crate::sim::movement::track_process::TrackFamily;
use crate::sim::rng::SimRng;
use serde_json::{Value, json};

fn nav(value: &Value) -> Option<NavTargetRef> {
    value.as_array().map(|cell| {
        NavTargetRef::cell(
            cell[0].as_u64().unwrap() as u16,
            cell[1].as_u64().unwrap() as u16,
        )
    })
}

fn target(value: &Value) -> Option<TargetKind> {
    value.as_array().map(|cell| {
        TargetKind::Cell(
            cell[0].as_u64().unwrap() as u16,
            cell[1].as_u64().unwrap() as u16,
        )
    })
}

fn supplied_fixture(rules: &RuleSet, fixture: &Value, row: &Value) -> (Simulation, u64) {
    let before = &row["before"];
    let mut sim = Simulation::with_seed(fixture["seed"].as_u64().unwrap());
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    let id = sim
        .spawn_object("HTK", "Russians", 10, 10, 0, rules)
        .unwrap();
    assert!(rules.object("HTK").unwrap().passengers > 0);
    sim.session
        .set_binary_frame_for_test(fixture["frame"].as_u64().unwrap() as u32);
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    entity.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(before["mission"].as_i64().unwrap() as i32),
        suspended: MissionId::from_raw(before["suspended_mission"].as_i64().unwrap() as i32),
        queued: MissionId::from_raw(before["queued"].as_i64().unwrap() as i32),
        movement_bypass_latch: before["movement_bypass"].as_u64().unwrap() as u8,
        handler_state: before["status"].as_u64().unwrap() as u32,
        mission_start_frame: before["mission_start"].as_u64().unwrap() as u32,
        ai_counter: before["mission_accum"].as_u64().unwrap() as u32,
        dispatch_timer: MissionDispatchTimer::from_raw(
            before["timer_start"].as_i64().unwrap() as i32,
            before["timer_duration"].as_i64().unwrap() as i32,
        ),
    });
    entity.navigation.set_nav_com_for_test(nav(&before["nav"]));
    entity
        .navigation
        .set_suspended_nav_com_for_test(nav(&before["suspended_nav"]));
    entity.attack_target.set_for_test(target(&before["target"]));
    entity
        .suspended_attack_target
        .set_for_test(target(&before["suspended_target"]));
    entity.set_archive_target(target(&before["archive"]));
    if row["input"]["cargo"] == 1 {
        // The native loaded-admission control also supplies a head sentinel;
        // neither comparison dispatches its passenger handler or placement.
        entity
            .passenger_role
            .cargo_mut()
            .unwrap()
            .board_forced_chain(&[(999, 1.0)]);
    }
    let locomotor = entity.locomotor.as_mut().unwrap();
    assert!(locomotor.ensure_installed_track_state());
    if before["drive_is_moving"] == true {
        let coord = &before["drive_destination"];
        assert!(locomotor.store_track_destination(
            TrackFamily::Drive,
            Some(DriveCoord {
                x: coord[0].as_i64().unwrap() as i32,
                y: coord[1].as_i64().unwrap() as i32,
                z: coord[2].as_i64().unwrap() as i32,
            }),
        ));
    }
    sim.scenario_rng =
        SimRng::from_native_state_hex_for_test(row["scenario_rng_before_hex"].as_str().unwrap());
    (sim, id)
}

fn assert_mission_state(sim: &Simulation, id: u64, expected: &Value) {
    let entity = sim.substrate.entities.get(id).unwrap();
    let state = entity.mission;
    for (key, actual) in [
        ("mission", i64::from(state.current().raw())),
        ("suspended_mission", i64::from(state.suspended().raw())),
        ("queued", i64::from(state.queued().raw())),
        ("movement_bypass", i64::from(state.movement_bypass_latch())),
        ("status", i64::from(state.handler_state())),
        ("mission_start", i64::from(state.mission_start_frame())),
        ("mission_accum", i64::from(state.ai_counter())),
        (
            "timer_start",
            i64::from(state.dispatch_timer().start_frame()),
        ),
        ("timer_duration", i64::from(state.dispatch_timer().delay())),
    ] {
        assert_eq!(actual, expected[key].as_i64().unwrap(), "{key}");
    }
    assert_eq!(entity.navigation.nav_com(), nav(&expected["nav"]));
    assert_eq!(entity.attack_target.target(), target(&expected["target"]));
    assert_eq!(
        entity.navigation.suspended_nav_com(),
        nav(&expected["suspended_nav"])
    );
    assert_eq!(
        entity.suspended_attack_target.target(),
        target(&expected["suspended_target"])
    );
    assert_eq!(entity.archive_target(), target(&expected["archive"]));
    assert_eq!(
        u64::from(entity.passenger_role.passenger_count()),
        expected["cargo"].as_u64().unwrap()
    );
    assert_eq!(
        crate::sim::movement::motion_query::is_moving(entity),
        Some(false)
    );
    assert_eq!(
        entity
            .locomotor
            .as_ref()
            .unwrap()
            .track_destination(TrackFamily::Drive),
        None
    );
}

#[test]
fn ground_transport_deploy_event_and_empty_unload_match_native_state_and_rng() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let corpus = crate::test_fixture::json("tools/spatial_oracle/ground_transport_deploy.json");
    for row in corpus["receiver"].as_array().unwrap() {
        let (mut sim, id) = supplied_fixture(rules, &corpus["fixture"], row);
        let rng = sim.rng_state();
        let sound_count = sim.sound_events.len();
        assert!(
            sim.apply_command(
                "Russians",
                &Command::DeployMcv { entity_id: id },
                Some(rules)
            ),
            "native input {}",
            row["input"]
        );
        assert_mission_state(&sim, id, &row["after_event"]);
        assert_eq!(sim.rng_state(), rng, "Event9 consumes no RNG");
        assert_eq!(sim.sound_events.len(), sound_count);
        let now = sim.session.binary_frame();
        sim.mission_host_promote(id, now, rules);
        assert_mission_state(&sim, id, &row["after_direct_commence"]);
        assert_eq!(sim.rng_state(), rng, "promotion consumes no RNG");
        if row["after_first_empty_unload"].is_object() {
            let delay = crate::sim::transport_unload::unit_mission_unload(
                &mut sim,
                rules,
                id,
                None,
                crate::sim::world::FrameEffects::default(),
            );
            assert_eq!(json!(delay), row["unload_returned_delay"]);
            assert_mission_state(&sim, id, &row["after_first_empty_unload"]);
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                row["scenario_rng_after_unload_hex"].as_str().unwrap(),
                "one ordered Scenario draw from the original empty Unload body"
            );
        }
    }
}
