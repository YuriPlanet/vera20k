//! Native Fly height vectors exercised through the production cell transaction.

use super::lifecycle_tests::{insert_entity, install_common_raw_terrain};
use super::{PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, Simulation};
use crate::map::entities::EntityCategory;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::passenger::{PassengerCargo, PassengerRole};
use crate::sim::snapshot::GameSnapshot;
use crate::util::fixed_math::SimFixed;

fn vectors() -> Vec<serde_json::Value> {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/fly_height.json"
    ))
    .unwrap()
}

fn fixture(row: &serde_json::Value) -> (Simulation, RuleSet) {
    let input = &row["input"];
    let integer = |name: &str, default: i32| input[name].as_i64().map_or(default, |v| v as i32);
    let flag = |name: &str| input[name].as_bool().unwrap_or(false);
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nFlightLevel=1500\n[AircraftTypes]\n0=TEST\n\
         [TEST]\nStrength=100\nSpeed={}\nLandable={}\n\
         Locomotor={{4A582746-9839-11D1-B709-00A024DDAFD1}}\n\
         FlightLevel={}\nIsDropship={}\nCarryall={}\n\
         [BuildingTypes]\n0=PLAIN\n1=REPAIR\n2=HELIPAD\n\
         [PLAIN]\nStrength=100\n[REPAIR]\nStrength=100\nUnitRepair=yes\n\
         [HELIPAD]\nStrength=100\nHelipad=yes\n",
        integer("speed", 10),
        if input["landable"].as_bool().unwrap_or(true) {
            "yes"
        } else {
            "no"
        },
        integer("flight_level", -1),
        if flag("dropship") { "yes" } else { "no" },
        if flag("carryall") { "yes" } else { "no" },
    )))
    .unwrap();
    let mut sim = Simulation::with_seed(0);
    install_common_raw_terrain(
        &mut sim,
        22,
        22,
        integer("level", 0) as u8,
        flag("bridge").then_some((10, 10)),
    );
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(10, 10)
        .unwrap()
        .slope_type = integer("slope", 0) as u8;
    assert_eq!(sim.allocate_stable_id(), 1);
    let category = if input["kind"] == "unit" {
        EntityCategory::Unit
    } else {
        EntityCategory::Aircraft
    };
    insert_entity(&mut sim, 1, category);
    sim.substrate.entities.get_mut(1).unwrap().locomotor = Some(LocomotorState::from_object_type(
        rules.object("TEST").unwrap(),
        0,
    ));
    assert!(matches!(
        sim.try_reveal_entity(
            1,
            RevealRequest {
                position: RevealPosition {
                    rx: 10,
                    ry: 10,
                    z: integer("level", 0) as u8,
                    sub_x: SimFixed::from_num(128),
                    sub_y: SimFixed::from_num(128)
                },
                placement: PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            }
        ),
        RevealOutcome::Revealed { .. }
    ));
    sim.remove_entity_occupancy(1);
    if flag("loaded") {
        assert_eq!(sim.allocate_stable_id(), 2);
        insert_entity(&mut sim, 2, EntityCategory::Infantry);
        sim.substrate.entities.get_mut(2).unwrap().passenger_role =
            PassengerRole::Inside { transport_id: 1 };
        let mut cargo = PassengerCargo::new(1, 0);
        cargo.board_forced(2, 1);
        sim.substrate.entities.get_mut(1).unwrap().passenger_role =
            PassengerRole::Transport { cargo };
    }
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.position.exact_z_leptons = Some(integer("z", 0));
    entity.on_bridge = flag("on_bridge");
    entity.health.current = integer("health", 100);
    let loco = entity.locomotor.as_mut().unwrap();
    // A deliberately stale cache must not override the physical coordinate.
    loco.altitude = SimFixed::from_num(123);
    if flag("landing") {
        loco.begin_fly_landing();
    }
    loco.set_fly_target_height(integer("target", 0));
    sim.add_entity_occupancy(1);
    (sim, rules)
}

#[test]
fn healthy_native_height_vectors_reach_production_coordinates() {
    let mut count = 0;
    for row in vectors() {
        // Health<=0 belongs to the preceding fall/crash controller, not the
        // healthy production transaction compared here. The kernel test also
        // checks those original bounded-range cases.
        if row["input"]["health"].as_i64().is_some_and(|h| h <= 0) {
            continue;
        }
        let (mut sim, rules) = fixture(&row);
        let before_rng = sim.scenario_rng.logical_state();
        sim.tick_air_movement_with_cell_lists_one(1, Some(&rules));
        let entity = sim.substrate.entities.get(1).unwrap();
        let expected_z = row["z"].as_i64().unwrap() as i32;
        let expected_height = row["height"].as_i64().unwrap() as i32;
        assert_eq!(
            entity.position.exact_z_leptons,
            Some(expected_z),
            "{}",
            row["input"]
        );
        assert_eq!(
            entity.on_bridge,
            row["on_bridge"].as_bool().unwrap(),
            "{}",
            row["input"]
        );
        assert_eq!(
            entity.locomotor.as_ref().unwrap().altitude,
            SimFixed::saturating_from_num(expected_height),
            "{}",
            row["input"]
        );
        assert_eq!(
            sim.foot_navigation_coordinate(1).unwrap().z,
            expected_z,
            "{}",
            row["input"]
        );
        assert_eq!(sim.scenario_rng.logical_state(), before_rng);
        count += 1;
    }
    assert_eq!(count, 132);
}

#[test]
fn fly_integer_target_flags_and_cargo_survive_save_and_continuation() {
    for name in [
        "wide_60000_65536",
        "dropship_True_200_0",
        "ordinary_True_1480",
    ] {
        let row = vectors()
            .into_iter()
            .find(|r| r["input"]["name"] == name)
            .unwrap();
        let (mut sim, rules) = fixture(&row);
        let terrain = sim.resolved_terrain.clone();
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = GameSnapshot::save(&sim, 0, 0, "Fly native height continuation", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.retain_in_scenario_process_state_from(&sim);
        restored.resolved_terrain = terrain;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(restored.state_hash(), sim.state_hash(), "{name}");
        assert_eq!(
            restored
                .substrate
                .entities
                .get(1)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .fly_runtime(),
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .fly_runtime()
        );
        for frame in 1..=8 {
            for instance in [&mut sim, &mut restored] {
                instance.session.tick = frame;
                instance.session.binary_frame = frame as u32;
                instance.tick_air_movement_with_cell_lists_one(1, Some(&rules));
            }
            assert_eq!(
                restored.state_hash(),
                sim.state_hash(),
                "{name} frame {frame}"
            );
            if frame == 1 {
                assert_eq!(
                    sim.substrate
                        .entities
                        .get(1)
                        .unwrap()
                        .position
                        .exact_z_leptons,
                    Some(row["z"].as_i64().unwrap() as i32)
                );
            }
        }
    }
}

#[test]
fn fly_hash_distinguishes_targets_and_native_flags() {
    let row = vectors()
        .into_iter()
        .find(|r| r["input"]["name"] == "ordinary_land_100")
        .unwrap();
    let (mut sim, _) = fixture(&row);
    let baseline = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .set_fly_target_height(65536);
    let target = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .begin_fly_takeoff(0);
    let takeoff = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .begin_fly_landing();
    let landing = sim.state_hash();
    let unique = [baseline, target, takeoff, landing]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        unique.len(),
        4,
        "target and each native flag are hash-visible"
    );
}

#[test]
fn repeated_aircraft_attack_visits_do_not_divide_the_fly_target() {
    use crate::sim::aircraft::{AircraftMission, tick_aircraft_missions};
    let row = vectors()
        .into_iter()
        .find(|r| r["input"]["name"] == "ordinary_False_1500")
        .unwrap();
    let (mut sim, rules) = fixture(&row);
    for sub_state in [3, 4, 3, 4] {
        sim.substrate.entities.get_mut(1).unwrap().aircraft_mission =
            Some(AircraftMission::Attack { sub_state });
        tick_aircraft_missions(&mut sim, &rules, None);
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .fly_target_height(),
            1500
        );
    }
}

fn landing_base_fixture(row: &serde_json::Value) -> (Simulation, RuleSet) {
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId};

    let mut row = row.clone();
    let input = row["input"].as_object_mut().unwrap();
    input.entry("z").or_insert(serde_json::json!(100));
    input.insert("target".into(), serde_json::json!(37));
    let (mut sim, rules) = fixture(&row);
    let input = &row["input"];
    let contacts = input["contacts"].as_array().cloned().unwrap_or_default();
    let mut slots = crate::sim::radio::contacts::Contacts::with_capacity(contacts.len());
    let mut holes = Vec::new();
    for contact in contacts {
        let id = sim.allocate_stable_id();
        slots.insert(id).unwrap();
        let Some(kind) = contact.as_str() else {
            holes.push(id);
            continue;
        };
        insert_entity(
            &mut sim,
            id,
            if kind == "unit" {
                EntityCategory::Unit
            } else {
                EntityCategory::Structure
            },
        );
        let type_ref = sim.interner.intern(match kind {
            "repair" => "REPAIR",
            "helipad" => "HELIPAD",
            _ => "PLAIN",
        });
        sim.substrate.entities.get_mut(id).unwrap().type_ref = type_ref;
    }
    for id in holes {
        slots.remove(id);
    }
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.radio_contacts = slots;
    entity.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(input["current"].as_i64().unwrap_or(7) as i32),
        queued: MissionId::from_raw(input["queued"].as_i64().unwrap_or(-1) as i32),
        suspended: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    // Supply captured native states, including both flags true; production
    // start verbs intentionally cannot construct that contradictory pair.
    *entity
        .locomotor
        .as_mut()
        .unwrap()
        .fly_runtime_mut()
        .unwrap() = serde_json::from_value(serde_json::json!({
    "target_height": 37,
    "taking_off": input["taking_off"].as_bool().unwrap_or(false),
    "landing": input["landing"].as_bool().unwrap_or(false),
    "destination": [0, 0, 0],
    }))
    .unwrap();
    if input["missing_cell"].as_bool().unwrap_or(false) {
        let terrain = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(0, 0, vec![]);
        terrain.shared_cell_dummy().stamp_coord(-7, -8);
        sim.resolved_terrain = Some(terrain);
    }
    (sim, rules)
}

fn assert_native_move_takeoff(sim: &mut Simulation, rules: &RuleSet, row: &serde_json::Value) {
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        crate::sim::aircraft::landing_base::landing_base(
            entity,
            &sim.substrate.entities,
            Some((rules, &sim.interner)),
        ),
        row["landing_base"].as_i64().unwrap() as i32,
        "{row}"
    );
    let before_rng = sim.scenario_rng.logical_state();
    assert!(sim.issue_air_cell_destination(1, (12, 10), SimFixed::from_num(10), Some(rules)));
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        entity.locomotor.as_ref().unwrap().fly_target_height(),
        if row["begin_takeoff"].as_bool().unwrap() {
            1500
        } else {
            37
        },
        "{row}"
    );
    assert_eq!(sim.scenario_rng.logical_state(), before_rng);
    if row["input"]["missing_cell"].as_bool().unwrap_or(false) {
        let actual = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .shared_cell_dummy()
            .snapshot()
            .coord;
        // The native base-decision corpus stops before4CCED9. Full MoveTo
        // then queries ground from the retained destination (4CCEE2), as
        // recorded by fly_destination.json. This production regression must
        // therefore see the final destination stamp, not the earlier prefix's
        // owner-height stamp. Base/BeginTakeoff remain the comparisons above.
        assert_eq!(
            [actual.0, actual.1],
            [12, 10],
            "final mode-query stamp: {row}"
        );
    }
}

#[test]
fn carryall_native_landing_base_reaches_production_move_orders() {
    let vectors: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/fly_landing_base.json"
    ))
    .unwrap();
    let rows = vectors["moves"].as_array().unwrap();
    assert_eq!(rows.len(), 249);
    for row in rows {
        let (mut sim, rules) = landing_base_fixture(row);
        assert_native_move_takeoff(&mut sim, &rules, row);
    }
}

#[test]
fn carryall_landing_base_is_recomputed_from_saved_cargo_contacts_and_mission() {
    let vectors: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/fly_landing_base.json"
    ))
    .unwrap();
    let rows = vectors["moves"].as_array().unwrap();
    for contacts in [
        serde_json::json!(["repair"]),
        serde_json::json!([null, "repair"]),
    ] {
        for loaded in [false, true] {
            let row = rows
                .iter()
                .find(|row| {
                    let input = &row["input"];
                    input["carryall"] == true
                        && input["loaded"] == loaded
                        && input["contacts"] == contacts
                        && input["current"] == -1
                        && input["queued"] == 7
                })
                .unwrap();
            let (mut sim, rules) = landing_base_fixture(row);
            sim.scenario_rng = crate::sim::rng::SimRng::new(0);
            let bytes = GameSnapshot::save(&sim, 0, 0, "Carryall move continuation", 0);
            let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
            restored.retain_in_scenario_process_state_from(&sim);
            restored.resolved_terrain = sim.resolved_terrain.clone();
            restored.restore_after_snapshot_load().unwrap();
            assert_eq!(restored.state_hash(), sim.state_hash());
            assert_native_move_takeoff(&mut sim, &rules, row);
            assert_native_move_takeoff(&mut restored, &rules, row);
            assert_eq!(restored.state_hash(), sim.state_hash());
        }
    }
}

fn destination_vectors() -> Vec<serde_json::Value> {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/fly_destination.json"
    ))
    .unwrap()
}

fn destination_fixture(row: &serde_json::Value) -> (Simulation, RuleSet) {
    use crate::sim::combat::AttackTarget;
    use crate::sim::docking::aircraft_dock::AircraftAmmo;
    let input = &row["input"];
    let height_row = serde_json::json!({"input": {
        "z": input["z"].as_i64().unwrap_or(500),
        "target": 37,
        "flight_level": input["flight_level"].as_i64().unwrap_or(-1),
        "landable": input["landable"].as_bool().unwrap_or(true),
    }});
    let (mut sim, rules) = fixture(&height_row);
    sim.remove_entity_occupancy(1);
    install_common_raw_terrain(&mut sim, 128, 128, 0, None);
    let target_cell = sim
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(64, 64)
        .unwrap();
    target_cell.level = input["level"].as_u64().unwrap_or(0) as u8;
    target_cell.slope_type = input["slope"].as_u64().unwrap_or(0) as u8;
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.position.rx = 40;
    entity.position.ry = 64;
    entity.aircraft_ammo = Some(AircraftAmmo::new(input["ammo"].as_i64().unwrap_or(2) as i32));
    entity.attack_target = input["target"]
        .as_bool()
        .unwrap_or(false)
        .then(|| AttackTarget::new(2));
    entity
        .mission_leaf
        .set_aircraft_action_latch(input["ready"].as_bool().unwrap_or(false));
    let loco = entity.locomotor.as_mut().unwrap();
    loco.powered = input["powered"].as_bool().unwrap_or(true);
    *loco.fly_runtime_mut().unwrap() = serde_json::from_value(serde_json::json!({
        "target_height": 37,
        "taking_off": input["taking_off"].as_bool().unwrap_or(false),
        "landing": input["landing"].as_bool().unwrap_or(false),
        "destination": input.get("previous").cloned().unwrap_or(serde_json::json!([0,0,0])),
        "cruise_mode": input["mode"].as_bool().unwrap_or(false),
    }))
    .unwrap();
    sim.add_entity_occupancy(1);
    (sim, rules)
}

fn issue_coordinate(sim: &mut Simulation, rules: &RuleSet, xyz: [i32; 3]) -> bool {
    crate::sim::movement::air_movement::issue_air_coordinate_move_command(
        &mut sim.substrate.entities,
        1,
        crate::sim::components::DriveCoord {
            x: xyz[0],
            y: xyz[1],
            z: xyz[2],
        },
        SimFixed::from_num(10),
        crate::sim::movement::DestinationTiming::from_rules(sim.session.binary_frame, Some(rules)),
        sim.resolved_terrain.as_ref(),
        Some((rules, &sim.interner)),
    )
}

#[test]
fn fly_destination_orders_match_original_retained_xyz_and_refusals() {
    let rows = destination_vectors();
    assert_eq!(rows.len(), 26);
    for row in rows {
        let (mut sim, rules) = destination_fixture(&row);
        let input = &row["input"];
        let request: [i32; 3] = serde_json::from_value(
            input
                .get("request")
                .cloned()
                .unwrap_or(serde_json::json!([16512, 16512, 0])),
        )
        .unwrap();
        let before_rng = sim.scenario_rng.logical_state();
        sim.shared_cell_dummy.stamp_coord(-7, -8);
        let accepted = issue_coordinate(&mut sim, &rules, request);
        // All admitted original cases reach the Aircraft auxiliary call;
        // same-cell landing and power refusals return before any recorded call.
        assert_eq!(
            accepted,
            !row["events"].as_array().unwrap().is_empty(),
            "{input}"
        );
        let entity = sim.substrate.entities.get(1).unwrap();
        let state = entity.locomotor.as_ref().unwrap().fly_runtime().unwrap();
        let actual = state.destination();
        assert_eq!(
            serde_json::json!([actual.x, actual.y, actual.z]),
            row["destination"],
            "{input}"
        );
        assert_eq!(
            state.target_height(),
            row["height"].as_i64().unwrap() as i32,
            "{input}"
        );
        assert_eq!(sim.scenario_rng.logical_state(), before_rng);
        if accepted {
            assert_eq!(
                entity.movement_target.as_ref().unwrap().final_goal,
                Some((
                    (request[0] / 256) as i16 as u16,
                    (request[1] / 256) as i16 as u16
                ))
            );
        } else {
            assert!(entity.movement_target.is_none());
            assert_eq!(sim.shared_cell_dummy.snapshot().coord, (-7, -8), "{input}");
        }
        assert_eq!(
            state.cruise_mode(),
            row["mode"].as_bool().unwrap(),
            "{input}"
        );
        // Moving+34 is still separate from the legacy MovementTarget lifetime.
    }
}

#[test]
fn fly_cell_orders_resolve_ground_before_retaining_the_coordinate() {
    let row = destination_vectors()
        .into_iter()
        .find(|r| r["input"]["name"] == "ground_3_4")
        .unwrap();
    let (mut sim, rules) = destination_fixture(&row);
    assert!(sim.issue_air_cell_destination(1, (64, 64), SimFixed::from_num(10), Some(&rules)));
    let destination = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .locomotor
        .as_ref()
        .unwrap()
        .fly_runtime()
        .unwrap()
        .destination();
    assert_eq!(
        serde_json::json!([destination.x, destination.y, destination.z]),
        row["destination"]
    );
}

#[test]
fn fly_retained_destination_drives_subcell_arrival_after_save_and_restore() {
    let row = destination_vectors()
        .into_iter()
        .find(|r| r["input"]["name"] == "subcell")
        .unwrap();
    let (mut sim, rules) = destination_fixture(&row);
    assert!(issue_coordinate(&mut sim, &rules, [16519, 16523, 333]));
    sim.remove_entity_occupancy(1);
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.position.rx = 64;
    entity.position.ry = 64;
    entity.position.sub_x = SimFixed::from_num(135);
    entity.position.sub_y = SimFixed::from_num(139);
    // Poison the derived cell projection: it must not steer or snap Fly.
    entity.movement_target.as_mut().unwrap().final_goal = Some((10, 10));
    sim.add_entity_occupancy(1);
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let bytes = GameSnapshot::save(&sim, 0, 0, "Fly retained destination", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.retain_in_scenario_process_state_from(&sim);
    restored.resolved_terrain = sim.resolved_terrain.clone();
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.state_hash(), sim.state_hash());
    for instance in [&mut sim, &mut restored] {
        instance.tick_air_movement_with_cell_lists_one(1, Some(&rules));
        let entity = instance.substrate.entities.get(1).unwrap();
        assert_eq!(
            crate::sim::movement::ground_pose::position_world_xy(&entity.position),
            [16519, 16523]
        );
        assert!(entity.movement_target.is_none());
        // Arrival's legacy adapter must not invent a native destination clear.
        assert_eq!(
            entity
                .locomotor
                .as_ref()
                .unwrap()
                .fly_runtime()
                .unwrap()
                .destination()
                .z,
            333
        );
    }
    assert_eq!(restored.state_hash(), sim.state_hash());
}

#[test]
fn fly_destination_is_hashed_and_persisted_in_active_and_stashed_runtime() {
    use super::hash_schema::HashSchema;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::movement::locomotion::piggyback;
    use crate::sim::movement::locomotor::MovementLayer;
    let row = destination_vectors().remove(0);
    for stashed in [false, true] {
        let (mut sim, rules) = destination_fixture(&row);
        assert!(issue_coordinate(&mut sim, &rules, [16519, 16523, 111]));
        let before = sim.state_hash();
        let old_projection = sim.state_hash_with_schema(HashSchema::Before(188));
        assert!(issue_coordinate(&mut sim, &rules, [16519, 16523, 333]));
        assert_ne!(
            before,
            sim.state_hash(),
            "retained Z changes the hash without changing cell cache"
        );
        assert_eq!(
            old_projection,
            sim.state_hash_with_schema(HashSchema::Before(188))
        );
        if stashed {
            let loco = sim
                .substrate
                .entities
                .get_mut(1)
                .unwrap()
                .locomotor
                .as_mut()
                .unwrap();
            assert_eq!(
                piggyback::begin(loco, LocomotorKind::Drive, MovementLayer::Ground, 0),
                piggyback::BeginOutcome::Installed
            );
        }
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = GameSnapshot::save(&sim, 0, 0, "Fly active/stashed destination", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.retain_in_scenario_process_state_from(&sim);
        restored.resolved_terrain = sim.resolved_terrain.clone();
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(restored.state_hash(), sim.state_hash());
        let loco = restored
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap();
        if stashed {
            assert!(piggyback::end(loco).is_some());
        }
        let destination = loco.fly_runtime().unwrap().destination();
        assert_eq!(
            [destination.x, destination.y, destination.z],
            [16519, 16523, 333]
        );
        assert!(loco.fly_runtime().unwrap().cruise_mode());
    }
}

fn takeoff_fixture(row: &serde_json::Value) -> (Simulation, RuleSet) {
    let input = &row["input"];
    let mut height_row = row.clone();
    height_row["input"]["target"] = input
        .get("target")
        .cloned()
        .unwrap_or(serde_json::json!(1500));
    let (mut sim, rules) = fixture(&height_row);
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    let rot = input["rot"].as_i64().unwrap_or(5) as i32;
    for (slot, initial, destination) in [
        (&mut entity.body_facing, 0x4000, 0xC000),
        (&mut entity.barrel_facing, 0x6000, 0x2000),
    ] {
        let mut facing = crate::sim::movement::FacingClass::new(initial, rot);
        facing.snap(initial, 90);
        facing.set(destination, 90);
        *slot = Some(facing);
    }
    let loco = entity.locomotor.as_mut().unwrap();
    loco.rot = rot;
    loco.speed_fraction = SimFixed::lit("0.25");
    *loco.fly_runtime_mut().unwrap() = serde_json::from_value(serde_json::json!({
        "target_height": input["target"].as_i64().unwrap_or(1500),
        "taking_off": input["taking_off"].as_bool().unwrap_or(true),
        "landing": input["landing"].as_bool().unwrap_or(false),
        "destination": input.get("destination").cloned().unwrap_or(serde_json::json!([3456,2688,0])),
        "cruise_mode": input["mode"].as_bool().unwrap_or(false),
    })).unwrap();
    sim.session.binary_frame = 100;
    (sim, rules)
}

fn assert_native_takeoff_result(sim: &Simulation, row: &serde_json::Value) {
    let entity = sim.substrate.entities.get(1).unwrap();
    let loco = entity.locomotor.as_ref().unwrap();
    let (_, taking_off, landing) = loco.fly_runtime().unwrap().height_hash_fields();
    assert_eq!(
        serde_json::json!([u8::from(taking_off), u8::from(landing)]),
        row["phase"],
        "{row}"
    );
    assert_eq!(
        loco.speed_fraction,
        SimFixed::from_num(row["speed"].as_f64().unwrap()),
        "{row}"
    );
    let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    assert_eq!(
        serde_json::json!([coord.x, coord.y, coord.z]),
        row["coordinates"],
        "{row}"
    );
    assert_eq!(
        entity.on_bridge,
        row["on_bridge"].as_bool().unwrap(),
        "{row}"
    );
    for (facing, expected) in [entity.body_facing.unwrap(), entity.barrel_facing.unwrap()]
        .into_iter()
        .zip(row["facings"].as_array().unwrap())
    {
        let fields = serde_json::to_value(facing).unwrap();
        assert_eq!(fields["current"], expected["destination"], "{row}");
        assert_eq!(fields["prev"], expected["previous"], "{row}");
        assert_eq!(fields["start_frame"], expected["start"], "{row}");
        assert_eq!(fields["duration_frames"], expected["duration"], "{row}");
        assert_eq!(
            serde_json::json!(facing.rot_per_frame()),
            expected["rate"],
            "{row}"
        );
        assert_eq!(
            serde_json::json!(facing.current(100)),
            expected["current"],
            "{row}"
        );
    }
}

#[test]
fn fly_takeoff_callback_matches_all_original_histories() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/fly_takeoff.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 80);
    for row in rows {
        let (mut sim, rules) = takeoff_fixture(&row);
        let rng = sim.scenario_rng.logical_state();
        sim.apply_fly_takeoff_callback(1, Some(&rules));
        assert_native_takeoff_result(&sim, &row);
        assert_eq!(sim.scenario_rng.logical_state(), rng);
    }
}

#[test]
fn fly_takeoff_phase_matches_native_display_reordering_and_gates() {
    use super::display_layers::DisplayLayer;
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/fly_takeoff_phase.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 75);
    for row in rows {
        let (mut sim, rules) = takeoff_fixture(&row);
        if !row["input"]["marked"].as_bool().unwrap_or(false) {
            sim.remove_entity_occupancy(1);
        }
        let peer = sim.allocate_stable_id();
        insert_entity(&mut sim, peer, EntityCategory::Aircraft);
        let owner = sim.substrate.entities.get(1).unwrap();
        let (position, on_bridge, loco) = (
            owner.position.clone(),
            owner.on_bridge,
            owner.locomotor.clone(),
        );
        let other = sim.substrate.entities.get_mut(peer).unwrap();
        other.position = position;
        other.on_bridge = on_bridge;
        other.locomotor = loco;
        sim.submit_entity_display(1, Some(&rules), None);
        sim.submit_entity_display(peer, Some(&rules), None);
        let rng = sim.scenario_rng.logical_state();
        let admitted = sim.complete_fly_phase(1, Some(&rules));
        assert_eq!(
            admitted,
            !row["phase_calls"].as_array().unwrap().is_empty(),
            "{row}"
        );
        assert_native_takeoff_result(&sim, &row);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().lifecycle.cell_marked,
            row["marked"].as_bool().unwrap(),
            "{row}"
        );
        for index in 0..5 {
            let actual: Vec<u64> = sim
                .substrate
                .display
                .members(DisplayLayer::from_index(index).unwrap())
                .iter()
                .filter_map(|&id| {
                    if id == 1 {
                        Some(0)
                    } else if id == peer {
                        Some(1)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(
                serde_json::json!(actual),
                row["layers"][index as usize],
                "{row}"
            );
        }
        assert_eq!(sim.scenario_rng.logical_state(), rng);
    }
}

#[test]
fn fly_nonlandable_phase_matches_native_without_display_resubmission() {
    use super::display_layers::DisplayLayer;
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/fly_nonlandable_phase.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 64);
    for row in rows {
        let (mut sim, rules) = takeoff_fixture(&row);
        let peer = sim.allocate_stable_id();
        insert_entity(&mut sim, peer, EntityCategory::Aircraft);
        let owner = sim.substrate.entities.get(1).unwrap();
        let (position, on_bridge, loco) = (
            owner.position.clone(),
            owner.on_bridge,
            owner.locomotor.clone(),
        );
        let other = sim.substrate.entities.get_mut(peer).unwrap();
        other.position = position;
        other.on_bridge = on_bridge;
        other.locomotor = loco;
        sim.submit_entity_display(1, Some(&rules), None);
        sim.submit_entity_display(peer, Some(&rules), None);
        let rng = sim.scenario_rng.logical_state();
        assert!(!sim.complete_fly_phase(1, Some(&rules)));
        assert_native_takeoff_result(&sim, &row);
        let entity = sim.substrate.entities.get(1).unwrap();
        let state = entity.locomotor.as_ref().unwrap().fly_runtime().unwrap();
        assert_eq!(state.cruise_mode(), row["mode"].as_bool().unwrap(), "{row}");
        assert_eq!(
            state.target_height(),
            row["target_height"].as_i64().unwrap() as i32,
            "{row}"
        );
        assert_eq!(
            entity.lifecycle.cell_marked,
            row["marked"].as_bool().unwrap(),
            "{row}"
        );
        for index in 0..5 {
            let actual: Vec<_> = sim
                .substrate
                .display
                .members(DisplayLayer::from_index(index).unwrap())
                .iter()
                .map(|&id| {
                    if id == 1 {
                        0
                    } else if id == peer {
                        1
                    } else {
                        -1
                    }
                })
                .collect();
            assert_eq!(
                serde_json::json!(actual),
                row["layers"][index as usize],
                "{row}"
            );
        }
        assert_eq!(rng, sim.scenario_rng.logical_state());
    }
}

#[test]
fn fly_nonlandable_production_tick_replaces_landing_target_and_restores() {
    let row = serde_json::json!({"input": {
        "z":900, "target":0, "taking_off":false, "landing":true,
        "landable":false, "flight_level":40000,
    }});
    let (mut sim, rules) = takeoff_fixture(&row);
    sim.submit_entity_display(1, Some(&rules), None);
    sim.tick_air_movement_with_cell_lists_one(1, Some(&rules));
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        entity.position.exact_z_leptons,
        Some(855),
        "movement precedes phase override"
    );
    let state = entity.locomotor.as_ref().unwrap().fly_runtime().unwrap();
    assert!(state.cruise_mode());
    assert!(!state.has_phase_callback());
    assert_eq!(state.target_height(), 40000);
    let bytes = GameSnapshot::save(&sim, 0, 0, "Non-Landable Fly continuation", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.retain_in_scenario_process_state_from(&sim);
    restored.resolved_terrain = sim.resolved_terrain.clone();
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.state_hash(), sim.state_hash());
    for frame in 101..105 {
        for instance in [&mut sim, &mut restored] {
            instance.session.binary_frame = frame;
            instance.tick_air_movement_with_cell_lists_one(1, Some(&rules));
            assert_eq!(
                instance
                    .substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .position
                    .exact_z_leptons,
                Some(855 + (frame as i32 - 100) * 20)
            );
        }
        assert_eq!(restored.state_hash(), sim.state_hash(), "frame{frame}");
    }
}

#[test]
fn fly_phase_outer_health_power_and_life_gates_precede_nonlandable_override() {
    for gate in ["health", "power", "life"] {
        let row = serde_json::json!({"input": {
            "z":900, "target":123, "landing":true, "landable":false,
        }});
        let (mut sim, rules) = takeoff_fixture(&row);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        match gate {
            "health" => entity.health.current = 0,
            "power" => entity.locomotor.as_mut().unwrap().powered = false,
            "life" => entity.lifecycle.object_alive = false,
            _ => unreachable!(),
        }
        let before = sim.state_hash();
        assert!(!sim.complete_fly_phase(1, Some(&rules)));
        assert_eq!(sim.state_hash(), before, "{gate}");
    }
}

#[test]
fn fly_cruise_mode_hashes_separately_from_destination() {
    use super::hash_schema::HashSchema;
    let row = destination_vectors().remove(0);
    let (mut sim, _) = destination_fixture(&row);
    let before = sim.state_hash();
    let old = sim.state_hash_with_schema(HashSchema::Before(191));
    // A readiness change can change mode for the identical retained XYZ.
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .fly_runtime_mut()
        .unwrap()
        .select_destination_mode(0, false, true, false);
    assert_ne!(sim.state_hash(), before);
    assert_eq!(sim.state_hash_with_schema(HashSchema::Before(191)), old);
}

#[test]
fn fly_production_process_resets_enter_mode_using_native_mission_precedence() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/fly_mission_mode.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 30);
    for row in rows {
        let (mut sim, rules) = fixture(&serde_json::json!({"input":{"z":900,"target":900}}));
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        let mut mission = serde_json::to_value(entity.mission).unwrap();
        mission["current"] = row["current"].clone();
        mission["queued"] = row["queued"].clone();
        entity.mission = serde_json::from_value(mission).unwrap();
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .fly_runtime_mut()
            .unwrap()
            .select_destination_mode(0, false, row["before"].as_bool().unwrap(), false);
        let rng = sim.scenario_rng.logical_state();
        sim.tick_air_movement_with_cell_lists_one(1, Some(&rules));
        let mode = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap()
            .fly_runtime()
            .unwrap()
            .cruise_mode();
        assert_eq!(mode, row["after"].as_bool().unwrap(), "{row}");
        assert_eq!(sim.scenario_rng.logical_state(), rng);
    }
}

#[test]
fn fly_production_tick_uses_primary_current_and_continues_after_restore() {
    let row = serde_json::json!({"input":{"z":900}});
    let (mut sim, rules) = takeoff_fixture(&row);
    // Tick remains stationary so the native phase evidence applies at z900.
    // A stale byte heading must not replace the retained timer-based turn.
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .set_fly_target_height(900);
    sim.substrate.entities.get_mut(1).unwrap().facing = 222;
    let primary = sim.substrate.entities.get(1).unwrap().body_facing.unwrap();
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let bytes = GameSnapshot::save(&sim, 0, 0, "Fly takeoff continuation", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.retain_in_scenario_process_state_from(&sim);
    restored.resolved_terrain = sim.resolved_terrain.clone();
    restored.restore_after_snapshot_load().unwrap();
    for frame in 100..104 {
        for instance in [&mut sim, &mut restored] {
            instance.session.binary_frame = frame;
            instance.tick_air_movement_with_cell_lists_one(1, Some(&rules));
            let entity = instance.substrate.entities.get(1).unwrap();
            assert_eq!(
                entity.body_facing.unwrap(),
                primary,
                "callback copies Primary destination to Secondary"
            );
            assert_eq!(
                entity.barrel_facing.unwrap().destination(),
                primary.destination()
            );
            assert_eq!(entity.facing, (primary.current(frame) >> 8) as u8);
            assert!(
                !entity
                    .locomotor
                    .as_ref()
                    .unwrap()
                    .fly_runtime()
                    .unwrap()
                    .has_phase_callback()
            );
        }
        assert_eq!(restored.state_hash(), sim.state_hash(), "frame{frame}");
    }
}

#[test]
fn fly_paid_step_matches_native_math_and_production_type_speed() {
    use crate::sim::movement::ground_pose::position_world_coord;
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/fly_paid_step.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 199);
    let mut production_cases = 0;
    for row in rows {
        let input = &row["input"];
        let integer = |key: &str| input[key].as_i64().unwrap() as i32;
        let current: [i32; 3] =
            std::array::from_fn(|axis| input["current"][axis].as_i64().unwrap() as i32);
        let native_type_speed =
            crate::util::fixed_math::ra2_speed_to_leptons_per_frame(integer("ini_speed"));
        assert_eq!(
            i64::from(native_type_speed),
            row["type_speed"].as_i64().unwrap(),
            "{input}"
        );
        let fraction = SimFixed::from_bits(integer("fraction_bits"));
        let speed =
            crate::sim::movement::air_movement::current_fly_speed(native_type_speed, fraction);
        assert_eq!(i64::from(speed), row["speed"].as_i64().unwrap(), "{input}");
        let rot = input["rot"].as_i64().unwrap_or(5) as i32;
        let mut primary = crate::sim::movement::FacingClass::new(integer("facing") as u16, rot);
        primary.snap(integer("facing") as u16, 90);
        if let Some(destination) = input["destination"].as_u64() {
            primary.set(destination as u16, 90);
        }
        let frame = input["frame"].as_u64().unwrap_or(100) as u32;
        let facing = primary.current(frame);
        assert_eq!(
            u64::from(facing),
            row["facing"].as_u64().unwrap(),
            "{input}"
        );
        let proposed = if speed > 0 {
            crate::util::native_trig::facing_step_world_xy([current[0], current[1]], facing, speed)
        } else {
            [current[0], current[1]]
        };
        assert_eq!(
            serde_json::json!([proposed[0], proposed[1], current[2]]),
            row["proposed"],
            "{input}"
        );

        // The candidate-placement/map-edge correction is a separate open
        // branch. Every in-grid paid candidate also reaches production here.
        if [current[0], current[1], proposed[0], proposed[1]]
            .into_iter()
            .any(|v| !(0..131072).contains(&v))
        {
            continue;
        }
        production_cases += 1;
        let (mut sim, rules) = fixture(&serde_json::json!({"input": {
            "z":current[2], "target":current[2], "speed":integer("ini_speed"),
        }}));
        sim.remove_entity_occupancy(1);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.position.rx = (current[0] / 256) as u16;
        entity.position.ry = (current[1] / 256) as u16;
        entity.position.sub_x = SimFixed::from_num(current[0] % 256);
        entity.position.sub_y = SimFixed::from_num(current[1] % 256);
        entity.body_facing = Some(primary);
        entity.facing = 222; // poison the byte cache
        entity.veterancy = 2; // Fly's getter bypasses Foot's FASTER path
        sim.add_entity_occupancy(1);
        assert!(issue_coordinate(
            &mut sim,
            &rules,
            [current[0] + 5000, current[1] + 5000, 0]
        ));
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.movement_target.as_mut().unwrap().speed = SimFixed::from_num(3000);
        entity.locomotor.as_mut().unwrap().fly_current_speed = fraction;
        sim.session.binary_frame = frame;
        let mut restored = if input["name"] == "turn_5_100" {
            sim.scenario_rng = crate::sim::rng::SimRng::new(0);
            let bytes = GameSnapshot::save(&sim, 0, 0, "Fly paid movement", 0);
            let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
            restored.retain_in_scenario_process_state_from(&sim);
            restored.resolved_terrain = sim.resolved_terrain.clone();
            restored.restore_after_snapshot_load().unwrap();
            assert_eq!(restored.state_hash(), sim.state_hash());
            Some(restored)
        } else {
            None
        };
        let rng = sim.scenario_rng.logical_state();
        sim.tick_air_movement_with_cell_lists_one(1, Some(&rules));
        let actual = position_world_coord(&sim.substrate.entities.get(1).unwrap().position);
        assert_eq!(
            serde_json::json!([actual.x, actual.y, actual.z]),
            row["proposed"],
            "production {input}"
        );
        assert_eq!(sim.scenario_rng.logical_state(), rng);
        if let Some(restored) = restored.as_mut() {
            restored.tick_air_movement_with_cell_lists_one(1, Some(&rules));
            assert_eq!(restored.state_hash(), sim.state_hash());
            for next_frame in frame + 1..frame + 5 {
                for instance in [&mut sim, &mut *restored] {
                    instance.session.binary_frame = next_frame;
                    instance.tick_air_movement_with_cell_lists_one(1, Some(&rules));
                }
                assert_eq!(restored.state_hash(), sim.state_hash(), "frame{next_frame}");
            }
        }
    }
    assert_eq!(production_cases, 196);
}
