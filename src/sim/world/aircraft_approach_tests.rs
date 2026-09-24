//! Full states0/3 calls, unlike the older selected-range-branch corpus.
use super::tests::{assert_reengagement, reengagement_fixture};
use super::*;
use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};
use crate::sim::aircraft::AircraftMission;
use crate::sim::combat::{build_attacker_snapshot, fire_coord};
use crate::sim::movement::FacingClass;
use crate::sim::snapshot::GameSnapshot;
use crate::util::fixed_math::SimFixed;
use serde_json::{Value, json};

fn fixture(input: &Value) -> (Simulation, RuleSet) {
    let (mut sim, mut rules) = reengagement_fixture(input);
    let flh = input.get("flh").cloned().unwrap_or(json!([0, 0, 0]));
    let elite = input.get("elite_flh").unwrap_or(&flh);
    rules.merge_art_data(&ArtRegistry::from_ini(&IniFile::from_str(&format!(
        "[TEST]\nPrimaryFireFLH={},{},{}\nElitePrimaryFireFLH={},{},{}\nTurretOffset={}\n",
        flh[0],
        flh[1],
        flh[2],
        elite[0],
        elite[1],
        elite[2],
        input["turret_offset"].as_i64().unwrap_or(0),
    ))));
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .lifecycle
        .object_alive = input["target_marked"].as_bool().unwrap_or(true);
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    let state = input["state"].as_u64().unwrap_or(3) as u8;
    entity.aircraft_mission = Some(AircraftMission::Attack { sub_state: state });
    entity.mission.set_handler_state(u32::from(state));
    entity.mission_leaf =
        crate::sim::mission::MissionLeafState::aircraft_raw_for_test(1, state, true);
    entity.navigation.nav_com = input["nav"].as_array().map(|xy| {
        NavTargetRef::cell(
            xy[0].as_u64().unwrap() as u16,
            xy[1].as_u64().unwrap() as u16,
        )
    });
    let loco = entity.locomotor.as_mut().unwrap();
    loco.fly_current_speed = SimFixed::from_num(input["speed"].as_f64().unwrap_or(1.0));
    let mut fly = serde_json::to_value(loco.fly_runtime().unwrap()).unwrap();
    fly["moving"] = json!(input["moving"].as_bool().unwrap_or(true));
    *loco.fly_runtime_mut().unwrap() = serde_json::from_value(fly).unwrap();
    entity.weapon_burst = serde_json::from_value(json!({
        "index": input["burst_index"].as_i64().unwrap_or(0),
    }))
    .unwrap();
    for (key, field) in [
        ("primary", &mut entity.body_facing),
        ("secondary", &mut entity.barrel_facing),
    ] {
        let mut facing = FacingClass::new(0, 5);
        facing.snap(input[key].as_u64().unwrap_or(0) as u16, 100);
        *field = Some(facing);
    }
    (sim, rules)
}

fn assert_flh(sim: &Simulation, rules: &RuleSet, row: &Value) {
    if row["flh"].is_null() {
        return;
    }
    let entity = sim.substrate.entities.get(1).unwrap();
    let snapshot = build_attacker_snapshot(
        entity,
        entity.attack_target.as_ref().unwrap().target,
        None,
        None,
        None,
    );
    let point = fire_coord::fire_coordinate(
        sim,
        rules,
        &fire_coord::FireSource::from(&snapshot),
        rules.object("TEST").unwrap(),
        combat_weapon::WeaponSlot::Primary,
        entity.weapon_burst.index() as u8,
    )
    .coord;
    // Keep the existing deterministic f32 point transform, rather than adding
    // matrix/x87 emulation for coordinate identity alone. Original Translate
    // stores with chop rounding; these three supplied poses differ by one
    // lepton. Pin the exact differences (not a blanket tolerance), and compare
    // the resulting production steering/facing history exactly below.
    let delta = match row["input"]["name"].as_str().unwrap() {
        "flh_0_0_0" | "elite_flh" => [-1, 0, 0],
        "flh_16384_0_1" => [-1, -1, 0],
        _ => [0, 0, 0],
    };
    for (axis, actual) in [point.x, point.y, point.z].into_iter().enumerate() {
        assert_eq!(
            i64::from(actual),
            row["flh"][axis].as_i64().unwrap() + delta[axis],
            "{} FLH axis{axis}",
            row["input"]
        );
    }
}

fn assert_facings(sim: &Simulation, row: &Value) {
    let entity = sim.substrate.entities.get(1).unwrap();
    for (index, facing) in [entity.body_facing, entity.barrel_facing]
        .into_iter()
        .enumerate()
    {
        let value = serde_json::to_value(facing.unwrap()).unwrap();
        assert_eq!(
            json!({
                "destination": value["current"], "previous": value["prev"],
                "start": value["start_frame"], "duration": value["duration_frames"],
                "rate": value["rot_per_frame"],
            }),
            row["facings"][index],
            "{} facing {index}",
            row["input"]
        );
    }
}

#[test]
fn aircraft_approach_matches_original_dispatch_and_restored_continuation() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/aircraft_approach.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 49);
    for row in rows {
        let (mut sim, rules) = fixture(&row["input"]);
        let saved = GameSnapshot::save(&sim, 0, 0, "aircraft approach", 0);
        let mut restored = GameSnapshot::load(&saved).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        // Snapshot-envelope seed policy is separate from persisted mission inputs.
        restored.scenario_rng = sim.scenario_rng.clone();
        assert_eq!(sim.state_hash(), restored.state_hash());
        for world in [&mut sim, &mut restored] {
            // GetFLH reads the headings before this visit changes SecondaryFacing.
            assert_flh(world, &rules, &row);
            crate::sim::aircraft::tick_aircraft_missions(world, &rules, None);
            assert_facings(world, &row);
            let hash = world.state_hash();
            crate::sim::aircraft::tick_aircraft_missions(world, &rules, None);
            assert_eq!(world.state_hash(), hash, "same-frame dispatch must wait");
            assert_reengagement(world, &row);
        }
        assert_eq!(sim.state_hash(), restored.state_hash());
    }
}

#[test]
fn aircraft_initial_attack_reaches_live_search_on_the_next_due_visit() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/aircraft_reengagement.json"
    ))
    .unwrap();
    let row = rows.iter().find(|r| r["input"]["name"] == "base").unwrap();
    let mut input = row["input"].clone();
    input["state"] = json!(0);
    input["target_marked"] = json!(false); // same supplied Foot membership as search corpus
    let (mut sim, rules) = fixture(&input);
    let rng = sim.scenario_rng.logical_state();
    crate::sim::aircraft::tick_aircraft_missions(&mut sim, &rules, None);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(matches!(
        entity.aircraft_mission,
        Some(AircraftMission::Attack { sub_state: 1 })
    ));
    assert_eq!(entity.mission.dispatch_timer().delay(), 1);
    assert!(entity.navigation.nav_com.is_none());
    assert_eq!(sim.scenario_rng.logical_state(), rng);
    let saved = GameSnapshot::save(&sim, 0, 0, "initial attack before search", 0);
    let mut restored = GameSnapshot::load(&saved).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    restored.scenario_rng = sim.scenario_rng.clone();
    for world in [&mut sim, &mut restored] {
        let before = world.state_hash();
        crate::sim::aircraft::tick_aircraft_missions(world, &rules, None);
        assert_eq!(world.state_hash(), before);
        world.session.binary_frame = 101;
        crate::sim::aircraft::tick_aircraft_missions(world, &rules, None);
        let entity = world.substrate.entities.get(1).unwrap();
        assert!(matches!(
            entity.aircraft_mission,
            Some(AircraftMission::Attack { sub_state: 3 })
        ));
        assert_eq!(entity.mission.dispatch_timer().start_frame(), 101);
        assert_eq!(
            i64::from(entity.mission.dispatch_timer().delay()),
            row["delay"].as_i64().unwrap()
        );
        let Some(NavTargetRef::Cell { rx, ry }) = entity.navigation.nav_com else {
            panic!("search must assign its result");
        };
        assert_eq!(json!([rx, ry]), row["nav"]);
        assert_eq!(
            u64::from(world.scenario_rng.next_u32()),
            row["next_random"].as_u64().unwrap()
        );
    }
    assert_eq!(sim.state_hash(), restored.state_hash());
}
