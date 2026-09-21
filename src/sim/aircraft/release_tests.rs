//! Original-instruction witnesses for the retained release lifecycle. These
//! tests do not certify the still-unwired Mission_Attack emission/navigation.

use super::{AircraftMission, tick_aircraft_missions};
use crate::map::entities::EntityCategory;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::combat::AttackTarget;
use crate::sim::docking::aircraft_dock::AircraftAmmo;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::test_interner;
use crate::sim::mission::leaf::MissionLeafState;
use crate::sim::mission::{MissionCom, MissionId};
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::Simulation;
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/aircraft_attack_release.json"
    ))
    .unwrap()
}

fn fixture(input: &Value) -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[AircraftTypes]\n0=ORCA\n[ORCA]\nStrength=150\nSpeed=8\nAmmo=2\n\
         Locomotor={4A582746-9839-11d1-B709-00A024DDAFD1}\n",
    ))
    .unwrap();
    // The snapshot envelope restores Scenario RNG to its native seed-zero
    // state; these prefixes consume no RNG. Keep that independent load policy
    // out of the state/continuation comparison.
    let mut sim = Simulation::with_seed(0);
    let mut entity = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
    entity.category = EntityCategory::Aircraft;
    entity.mission_leaf = MissionLeafState::aircraft_raw_for_test(
        u8::from(input["latch_6d2"].as_bool().unwrap()),
        1,
        false,
    );
    entity.mission = MissionCom::at_frame(0);
    let mut ammo = AircraftAmmo::new(2);
    ammo.current = input["ammo"].as_i64().unwrap() as i32;
    if input["pending"].as_bool().unwrap() {
        ammo.begin_release();
    }
    entity.aircraft_ammo = Some(ammo);
    entity.locomotor = Some(LocomotorState::from_object_type(
        rules.object("ORCA").unwrap(),
        0,
    ));
    // Supply the post-Commence selector just as the original-prefix oracle
    // does. Assign's independent request policy is not under test here.
    let raw = input["mission"].as_i64().unwrap_or(1) as i32;
    entity
        .mission
        .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
            current: MissionId::from_raw(raw),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
        });
    sim.substrate.entities.insert(entity);
    sim.substrate.next_stable_object_id = 2;
    sim.interner = test_interner();
    (sim, rules)
}

fn assert_native(sim: &Simulation, row: &Value) {
    let entity = sim.substrate.entities.get(1).unwrap();
    let ammo = entity.aircraft_ammo.as_ref().unwrap();
    assert_eq!(ammo.current, row["ammo"].as_i64().unwrap() as i32, "{row}");
    assert_eq!(
        ammo.release_pending(),
        row["pending"].as_bool().unwrap(),
        "{row}"
    );
    assert_eq!(
        entity.mission_leaf.as_aircraft().unwrap().action_latch() != 0,
        row["latch_6d2"].as_bool().unwrap(),
        "{row}"
    );
}

#[test]
fn aircraft_mission_entry_debits_match_original_instructions_and_save_restore() {
    let corpus = corpus();
    let rows = corpus["entries"].as_array().unwrap();
    assert_eq!(rows.len(), 72);
    for row in rows {
        let (mut sim, rules) = fixture(&row["input"]);
        sim.substrate.entities.get_mut(1).unwrap().aircraft_mission =
            Some(AircraftMission::Attack {
                sub_state: row["input"]["state"].as_u64().unwrap() as u8,
            });
        let saved = GameSnapshot::save(&sim, 0, 0, "pending aircraft release", 0);
        let mut restored = GameSnapshot::load(&saved).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(sim.state_hash(), restored.state_hash());
        for world in [&mut sim, &mut restored] {
            tick_aircraft_missions(world, &rules, None);
            assert_native(world, row);
        }
        assert_eq!(sim.state_hash(), restored.state_hash());
    }
}

#[test]
fn aircraft_ai_consumes_pending_after_commence_using_native_current_mission() {
    let corpus = corpus();
    let rows = corpus["ai_entries"].as_array().unwrap();
    assert_eq!(rows.len(), 144);
    for row in rows {
        let (mut sim, rules) = fixture(&row["input"]);
        // Deliberately contradictory legacy mirror: the original AI reads the
        // canonical selector, not AircraftMission or target presence.
        sim.substrate.entities.get_mut(1).unwrap().aircraft_mission =
            Some(AircraftMission::Attack { sub_state: 4 });
        sim.object_ai_post_movement_promote_one(1, Some(&rules));
        assert_native(&sim, row);
        sim.object_ai_post_movement_promote_one(1, Some(&rules));
        assert_native(&sim, row); // the cleared pending flag prevents a second debit
    }
}

#[test]
fn aircraft_queued_order_debit_observes_successful_and_blocked_commence() {
    for action in [false, true] {
        let input = serde_json::json!({"ammo":2,"pending":true,"latch_6d2":action});
        let (mut sim, rules) = fixture(&input);
        sim.mission_queue_exact(
            1,
            MissionId::from_raw(2),
            0,
            0,
            &crate::sim::mission::authority::EntityReadyInputProvider,
        )
        .unwrap();
        sim.object_ai_post_movement_promote_one(1, Some(&rules));
        let entity = sim.substrate.entities.get(1).unwrap();
        let ammo = entity.aircraft_ammo.as_ref().unwrap();
        assert_eq!(entity.mission.current().raw(), if action { 1 } else { 2 });
        assert_eq!(ammo.current, if action { 2 } else { 1 });
        assert_eq!(ammo.release_pending(), action);
    }
}

#[test]
fn aircraft_pending_charge_is_hashed_and_init_does_not_clear_it() {
    let input = serde_json::json!({"ammo":2,"pending":false,"latch_6d2":true});
    let (mut sim, rules) = fixture(&input);
    let before = sim.state_hash();
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.aircraft_ammo.as_mut().unwrap().begin_release();
    assert_ne!(before, sim.state_hash());
    sim.substrate.entities.get_mut(1).unwrap().aircraft_mission =
        Some(AircraftMission::Attack { sub_state: 0 });
    tick_aircraft_missions(&mut sim, &rules, None);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(entity.aircraft_ammo.as_ref().unwrap().release_pending());
    assert_eq!(entity.aircraft_ammo.as_ref().unwrap().current, 2);
    assert_eq!(entity.mission_leaf.as_aircraft().unwrap().action_latch(), 0);
}

#[test]
fn aircraft_mission_request_does_not_invent_a_successful_release() {
    let input = serde_json::json!({"ammo":1,"pending":false,"latch_6d2":false});
    let (mut sim, rules) = fixture(&input);
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.aircraft_mission = Some(AircraftMission::Attack { sub_state: 4 });
    entity.attack_target = Some(AttackTarget::for_cell(10, 9));
    for _ in 0..4 {
        tick_aircraft_missions(&mut sim, &rules, None);
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(entity.aircraft_ammo.as_ref().unwrap().current, 1);
        assert!(!entity.aircraft_ammo.as_ref().unwrap().release_pending());
        assert!(matches!(
            entity.aircraft_mission,
            Some(AircraftMission::Attack { sub_state: 4 })
        ));
        assert!(entity.attack_target.is_some());
    }
}
