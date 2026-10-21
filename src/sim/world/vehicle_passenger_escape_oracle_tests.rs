//! Replay the original dying-Unit passenger caller through Unit Unlimbo,
//! Scatter, nearby-cell search, the destination setter and Drive Move_To.
//! `tools/spatial_oracle/vehicle_passenger_escape.json` supplies every expected
//! state. The scene matches its resolved, unarmed Track/Normal/Drive type and
//! flat map; stock boarding and damage delivery have a separate retail test.
//!
//! Every retained row admits placement. The Sleep input becomes Guard during
//! original Unlimbo; a retained destination selects Move and suppresses the
//! subsequent Scatter search. The original Foot constructor establishes the
//! absent saved MegaMission (-1), which this consumer checks explicitly.
//! Intermediate admission/Unlimbo snapshots remain native evidence; this
//! consumer observes the completed Rust escape owner.

use super::lifecycle_tests::common_raw_terrain_cell;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::SpeedCostProfile;
use crate::sim::bridge_state::BridgeRuntimeState;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::crew_survival::DyingTransport;
use crate::sim::house_state::HouseState;
use crate::sim::mission::MissionId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::movement::track_process::TrackFamily;
use crate::sim::occupancy::RawCellKey;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::passenger::PassengerRole;
use crate::sim::pathfinding::{PathGrid, zone_map::ZoneGrid};
use crate::sim::rng::SimRng;
use crate::sim::world::{FrameEffects, Simulation};
use serde_json::{Value, json};
use std::sync::Arc;

const RULES: &str = "[VehicleTypes]\n0=APC\n1=MTNK\n\
    [APC]\nStrength=300\nPassengers=12\n\
    Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
    [MTNK]\nStrength=300\nSight=8\nCrusher=yes\nROT=5\nSpeed=7\n\
    SpeedType=Track\nMovementZone=Normal\n\
    Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
    [Clear]\nFoot=100%\nTrack=100%\nWheel=100%\nFloat=100%\n\
    Amphibious=100%\nFloatBeach=100%\nHover=100%\nBuildable=yes\n";

fn cell(value: &Value) -> (u16, u16) {
    (
        value[0].as_u64().unwrap() as u16,
        value[1].as_u64().unwrap() as u16,
    )
}

fn world(rules: &RuleSet) -> Simulation {
    let costs = SpeedCostProfile {
        foot: Some(100),
        track: Some(100),
        wheel: Some(100),
        float: Some(100),
        amphibious: Some(100),
        float_beach: Some(100),
        hover: Some(100),
    };
    let mut terrain = ResolvedTerrainGrid::from_cells(
        32,
        32,
        (0..32)
            .flat_map(|y| {
                (0..32).map(move |x| {
                    let mut cell = common_raw_terrain_cell(x, y, 0, false);
                    cell.speed_costs = costs;
                    cell.base_speed_costs = costs;
                    cell
                })
            })
            .collect(),
    );
    crate::map::resolved_terrain::install_ordinary_repair_test_catalog(&mut terrain);
    let bounds =
        crate::map::playfield::PlayfieldBounds::from_normalized_local_size(16, 0, 0, 32, 32);
    let bridges =
        BridgeRuntimeState::from_resolved_terrain_with_map_size(&terrain, true, 300, (16, 16));
    let path = PathGrid::from_resolved_terrain(&terrain);
    let zones = ZoneGrid::build_with_native_map_context(
        &path,
        &terrain,
        bridges.endpoint_records(),
        Some((16, 16)),
        Some(bounds),
    );
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.playfield_bounds = Some(bounds);
    sim.playfield_size_height = Some(16);
    sim.session.set_map_width_for_test(32);
    sim.session.set_map_height_for_test(32);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    sim.session.publish_started();
    sim.overlay_grid = Some(OverlayGrid::new(32, 32));
    sim.bridge_state = Some(bridges);
    sim.zone_grid = Some(zones);
    sim.path_grid = Some(Arc::new(path));
    sim.install_resolved_terrain_for_new_map(terrain);
    let owner = sim.interner.intern("Americans");
    let mut house = HouseState::new(owner, 0, Some(owner), true, 0, 10);
    house.set_player_control_for_test(true);
    sim.houses.register(house);
    sim.session.set_current_house_for_test(Some(owner));
    sim
}

fn state(sim: &Simulation, id: u64) -> Value {
    let entity = sim.substrate.entities.get(id).unwrap();
    let position = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    let nav = entity.navigation.nav_com().map(|nav| match nav {
        NavTargetRef::Cell { rx, ry } => [rx, ry],
        NavTargetRef::Object { .. } => panic!("unexpected object NavCom"),
    });
    // The native Drive destination is a zero CoordStruct when Rust has None.
    let destination = entity
        .locomotor
        .as_ref()
        .unwrap()
        .track_destination(TrackFamily::Drive)
        .map_or([0, 0, 0], |coord| [coord.x, coord.y, coord.z]);
    json!({
        "alive": entity.lifecycle.object_alive(),
        "limbo": entity.lifecycle.in_limbo(),
        "marked": entity.lifecycle.cell_marked(),
        "logic_registered": sim.live_object_order_snapshot().contains(&id),
        "position": [position.x, position.y, position.z],
        "mission": entity.mission.current().raw(),
        "queued": entity.mission.queued().raw(),
        "mega_mission": if entity.mega_mission.is_attack_move() { 29 } else { -1 },
        "transporter": entity.passenger_role.is_inside_transport(),
        "nav_cell": nav,
        "drive_destination": destination,
    })
}

#[test]
fn vehicle_passenger_escape_matches_native_caller_scatter() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/vehicle_passenger_escape.json");
    assert_eq!(
        corpus["map_input"],
        json!({
            "allocated_cells": [32, 32], "size": [16, 16],
            "local_size": [0, 0, 32, 32], "land": 0, "level": 0,
            "clear_speed_rows": vec![1.0; 9],
        })
    );
    let rules = RuleSet::from_ini(&IniFile::from_str(RULES)).unwrap();
    let typ = rules.object("MTNK").unwrap();
    assert_eq!(
        corpus["type_input"],
        json!({
            "strength": typ.strength, "sight": typ.sight, "speed": typ.speed,
            "crusher": typ.crusher, "rot": typ.turret_rot,
            "speed_type": "Track", "movement_zone": "Normal", "locomotor": "Drive",
            "armed": false, "native_logic_default": true, "native_cell_occupier_default": true,
        })
    );
    assert_eq!(typ.speed_type, SpeedType::Track);
    assert_eq!(typ.movement_zone, MovementZone::Normal);
    assert_eq!(typ.locomotor, LocomotorKind::Drive);

    let rows = corpus["rows"].as_array().unwrap();
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let (x, y) = cell(&input["source_cell"]);
        let mut sim = world(&rules);
        sim.session
            .set_binary_frame_for_test(input["frame"].as_u64().unwrap() as u32);
        let transport = sim
            .construct_object_limbo_at_height(
                "APC",
                "Americans",
                x,
                y,
                0,
                128,
                &rules,
                FrameEffects::default(),
            )
            .unwrap();
        sim.substrate
            .entities
            .get_mut(transport)
            .unwrap()
            .position
            .put_location(DriveCoord::cell(x, y, 0));
        let mut passengers = Vec::new();
        for spec in input["passengers"].as_array().unwrap() {
            assert_eq!(
                spec["mega_mission"], -1,
                "{name}: constructor saved-mission input"
            );
            let id = sim
                .construct_object_limbo_at_height(
                    "MTNK",
                    "Americans",
                    x,
                    y,
                    0,
                    0,
                    &rules,
                    FrameEffects::default(),
                )
                .unwrap();
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            assert_eq!(
                entity.mega_mission,
                crate::sim::mission::mega_mission::MegaMission::default(),
                "{name}: Rust Foot constructor has no saved mission or payload"
            );
            entity.position.put_location(DriveCoord::cell(x, y, 0));
            entity.mission.set_current_for_test(
                MissionId::from_raw(spec["mission"].as_i64().unwrap() as i32),
                0,
            );
            entity.passenger_role = PassengerRole::inside_for_test(transport, false);
            if !spec["nav_cell"].is_null() {
                let (nx, ny) = cell(&spec["nav_cell"]);
                // Explicit retained raw NavCom input; no destination setter
                // has run, so the native Drive destination remains zero.
                entity
                    .navigation
                    .set_nav_com_for_test(Some(NavTargetRef::cell(nx, ny)));
            }
            passengers.push(id);
        }
        for &id in passengers.iter().rev() {
            sim.substrate
                .entities
                .get_mut(transport)
                .unwrap()
                .passenger_role
                .cargo_mut()
                .unwrap()
                .board_forced_chain(&[(id, 1.0)]);
        }
        for blocked in input["blocked_cells"].as_array().unwrap() {
            let (bx, by) = cell(blocked);
            // The native control supplies only a raw occupation bit, with no
            // object in that cell's list.
            sim.substrate.raw_cell_occupation.write_occupant(
                RawCellKey::Real(bx, by),
                MovementLayer::Ground,
                0x20,
                None,
                true,
            );
        }
        for (index, &id) in passengers.iter().enumerate() {
            assert_eq!(
                state(&sim, id),
                row["passengers"][index]["before"],
                "{name}: passenger {index} input state"
            );
        }
        sim.scenario_rng = SimRng::new(input["seed"].as_u64().unwrap());
        let rng_before = sim.scenario_rng.state();
        sim.release_dying_unit_passengers(
            &rules,
            None,
            transport,
            DyingTransport {
                attacker: None,
                ignore_defenses: false,
                selected_by_player: false,
            },
            FrameEffects::default(),
        );
        assert!(
            sim.substrate
                .entities
                .get(transport)
                .unwrap()
                .passenger_role
                .cargo()
                .unwrap()
                .is_empty(),
            "{name}: all supplied passengers left cargo"
        );
        assert_eq!(
            sim.scenario_rng.state() == rng_before,
            row["scenario_rng_unchanged"].as_bool().unwrap(),
            "{name}: Scenario RNG"
        );
        assert!(
            !sim.object_placement_scope_active(),
            "{name}: the completed Rust caller balances its placement scope"
        );
        assert_eq!(
            row["scenario_init_counter"].as_u64().unwrap(),
            0,
            "{name}: native trace includes its balanced human tail"
        );
        assert_eq!(
            sim.substrate
                .raw_cell_occupation
                .bits_at(RawCellKey::Real(x, y), MovementLayer::Ground),
            row["source_occupation"].as_u64().unwrap() as u8,
            "{name}: source occupation"
        );
        let members: Vec<_> =
            sim.substrate
                .occupancy
                .get(x, y)
                .map_or_else(Vec::new, |occupancy| {
                    occupancy
                        .snapshot_layer(MovementLayer::Ground)
                        .iter()
                        .map(|id| {
                            passengers
                                .iter()
                                .position(|passenger| passenger == id)
                                .expect("passenger cell member")
                        })
                        .collect()
                });
        assert_eq!(
            json!(members),
            row["source_members"],
            "{name}: source member order"
        );
        for (index, &id) in passengers.iter().enumerate() {
            let expected = &row["passengers"][index];
            let placed = expected["placement_admitted"].as_bool().unwrap();
            assert!(placed, "{name}: retained native row admits placement");
            assert_eq!(
                sim.substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .lifecycle
                    .cell_marked(),
                placed,
                "{name}: passenger {index} placement"
            );
            assert_eq!(
                state(&sim, id),
                expected["after"],
                "{name}: passenger {index} after native caller/Scatter"
            );
        }
    }
    assert_eq!(rows.len(), 6);
}
