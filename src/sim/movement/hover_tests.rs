//! Hover arithmetic against original execution:
//! `tools/spatial_oracle/hover_speed_altitude.{py,json,meta.json}` runs
//! SpeedUpdate (0x00515ED0) and the altitude controller (0x00513D20), and
//! `tools/robot_offline_oracle.{py,json,meta.json}` runs Power_Off
//! (0x00516BF0).

use super::*;
use crate::rules::ini_parser::IniFile;

/// Original5171C0/514D10/744210 over the oracle's supplied scene. Native
/// execution records full32-bit raw words; the Rust raw-occupation owner
/// represents their low byte. Constructor, head production and outer Foot
/// Limbo are outside these rows; the lifecycle regression covers that join.
#[test]
fn mark_all_occupation_bits_matches_original_head_and_raw_plane_controls() {
    use crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
    use crate::map::resolved_terrain::{test_clear_cell, test_grid};
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::world::Simulation;

    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/locomotor_force_track.hover_mark_all.json",
    ))
    .expect("original Hover Mark_All controls");
    assert_eq!(
        corpus["native_sha256"],
        "3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600"
    );
    let null = DriveCoord { x: 0, y: 0, z: 0 };
    let slot = |value: &serde_json::Value| Some(coord(value)).filter(|coord| *coord != null);
    let unslot = |value: Option<DriveCoord>| {
        let value = value.unwrap_or(null);
        serde_json::json!([value.x, value.y, value.z])
    };
    let mut rows = 0;
    for row in corpus["cases"].as_array().unwrap() {
        let input = &row["input"];
        let structural = input["structural"].as_bool().unwrap();
        let mut sim = Simulation::new();
        sim.install_resolved_terrain_for_new_map(test_grid(32, 32, |rx, ry| {
            let mut cell = test_clear_cell(rx, ry);
            cell.level = 2;
            cell.template_height = 2;
            if structural {
                cell.bridge_facts.raw_flags |= BRIDGE_FLAG_STRUCTURAL;
                cell.has_bridge_deck = true;
                cell.bridge_walkable = true;
                cell.bridge_deck_level = 6;
            }
            cell
        }));
        let mut entity = GameEntity::test_default(1, "YHVR", "Confederation", 9, 10);
        entity
            .position
            .set_exact_z_leptons_for_test(Some(int(&input["location"][2])));
        entity.lifecycle.set_in_limbo_for_test(false);
        entity.set_foot_occupation_enabled(false);
        entity.set_on_bridge(input["on_bridge"].as_bool().unwrap());
        let mut loco =
            LocomotorState::for_test_kind(crate::rules::locomotor_type::LocomotorKind::Hover);
        loco.set_runtime_payload_for_test(
            super::super::locomotion::piggyback::LocomotorRuntimePayload::Hover(HoverRuntime {
                destination: slot(&input["destination"]),
                head: slot(&input["head"]),
                ..HoverRuntime::default()
            }),
        );
        entity.locomotor = Some(loco);
        entity
            .navigation
            .path_replay_mut()
            .set_directions_for_test(vec![2]);
        if input["stop_first"].as_bool().unwrap() {
            assert!(super::process::hover_stop_moving(&mut entity));
        }
        let retained = entity
            .locomotor
            .as_ref()
            .unwrap()
            .hover_runtime()
            .unwrap()
            .clone();
        sim.substrate.entities.insert(entity);
        for cell in row["before_mark"]["raw_cells"].as_array().unwrap() {
            let rx = int(&cell["cell"][0]) as u16;
            let ry = int(&cell["cell"][1]) as u16;
            sim.substrate
                .raw_cell_occupation
                .mark_ground(rx, ry, int(&cell["ground"]) as u8);
            sim.substrate
                .raw_cell_occupation
                .mark_deck(rx, ry, int(&cell["deck"]) as u8);
        }
        let streams = (
            sim.scenario_rng.state(),
            sim.main_rng.state(),
            sim.mapgen_rng.state(),
        );

        sim.locomotor_mark_all_occupation_bits(1, int(&input["mode"]) != 0);

        let entity = sim.substrate.entities.get(1).unwrap();
        let hover = entity.locomotor.as_ref().unwrap().hover_runtime().unwrap();
        assert_eq!(
            *hover, retained,
            "Mark_All preserves the active Hover: {input}"
        );
        let raw_cells: Vec<_> = row["output"]["raw_cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|cell| {
                let rx = int(&cell["cell"][0]) as u16;
                let ry = int(&cell["cell"][1]) as u16;
                serde_json::json!({"cell": [rx, ry],
                "ground": sim.substrate.raw_cell_occupation.ground_bits(rx, ry),
                "deck": sim.substrate.raw_cell_occupation.deck_bits(rx, ry)})
            })
            .collect();
        let location = super::super::foot_coordinate::current_coordinate(entity);
        let actual = serde_json::json!({
            "head": unslot(hover.head), "destination": unslot(hover.destination),
            "location": [location.x, location.y, location.z],
            "occupation_enabled": entity.foot_occupation_enabled(),
            "path_head": entity.navigation.path_replay().remaining_directions().first().map_or(-1, |&word| i32::from(word)),
            "raw_cells": raw_cells,
        });
        let mut expected = row["output"].clone();
        for cell in expected["raw_cells"].as_array_mut().unwrap() {
            for plane in ["ground", "deck"] {
                cell[plane] = serde_json::json!(int(&cell[plane]) as u8);
            }
        }
        assert_eq!(actual, expected, "{input}");
        assert_eq!(
            (
                sim.scenario_rng.state(),
                sim.main_rng.state(),
                sim.mapgen_rng.state()
            ),
            streams
        );
        rows += 1;
    }
    assert_eq!(rows, 16);
}

fn vectors() -> Vec<serde_json::Value> {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/hover_speed_altitude.json",
    ))
    .expect("hover oracle vectors")
}

fn bits(value: &serde_json::Value) -> NativeF64Bits {
    NativeF64Bits::from_bits(value.as_u64().expect("f64 bits"))
}

fn int(value: &serde_json::Value) -> i32 {
    value.as_i64().expect("integer") as i32
}

/// The oracle's two rule sets (`RULE_SETS`), written at their Rules offsets.
fn oracle_rules(name: &str) -> RuleSet {
    let [bob, boost, accel, brake, dampen] = match name {
        "constructor" => [
            0x403e_0000_0000_0000,
            0x3ff4_cccc_cccc_cccd,
            0x3f9e_b851_eb85_1eb8,
            0x3f9e_b851_eb85_1eb8,
            0x3fe9_9999_9999_999a,
        ],
        "retail" => [
            0x3fa4_7ae1_4000_0000,
            0x3ff8_0000_0000_0000,
            0x3f94_7ae1_4000_0000,
            0x3f9e_b851_e000_0000,
            0x3fd9_9999_9999_9999,
        ],
        other => panic!("rule set {other}"),
    }
    .map(NativeF64Bits::from_bits);
    let mut rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
    let general = &mut rules.general;
    general.hover_height = 120;
    general.hover_bob = bob;
    general.hover_boost = boost;
    general.hover_acceleration = accel;
    general.hover_brake = brake;
    general.hover_dampen = dampen;
    general.gravity = 6;
    rules
}

/// The reader's defaults are the constructor's (0x00665E2B..0x00665E81), and
/// retail `rulesmd.ini` gives the oracle's retail set.
#[test]
fn hover_keys_read_the_constructor_defaults_and_retail_values() {
    let read = |rules: &RuleSet| {
        let general = &rules.general;
        (
            general.hover_height,
            general.hover_bob,
            general.hover_boost,
            general.hover_acceleration,
            general.hover_brake,
            general.hover_dampen,
        )
    };
    let expected = |name| read(&oracle_rules(name));
    assert_eq!(
        read(&RuleSet::from_ini(&IniFile::from_str("")).unwrap()),
        expected("constructor")
    );
    let Some(retail) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    assert_eq!(read(&retail), expected("retail"));
    assert_eq!(retail.general.gravity, 6);
}

fn coord(value: &serde_json::Value) -> DriveCoord {
    DriveCoord {
        x: int(&value[0]),
        y: int(&value[1]),
        z: int(&value[2]),
    }
}

/// SpeedUpdate from its head arm: the request (distance, power and Push
/// arms), SpeedMult and the ramp toward `min(mult * request, 1)`. The
/// oracle's zeroed steering facing turns at once, so its turn gate always
/// passes; the gate and the look-ahead facing are not compared.
#[test]
fn request_and_ramp_match_original_speed_update() {
    let mut rows = 0;
    for row in vectors()
        .iter()
        .filter(|row| row["input"]["kind"] == "speed")
    {
        let input = &row["input"];
        let rules = oracle_rules(input["rules"].as_str().unwrap());
        let path = [int(&input["path"][0]), int(&input["path"][1])];
        let destination = coord(&input["destination"]);
        let mut hover = HoverRuntime {
            destination: (destination != DriveCoord { x: 0, y: 0, z: 0 }).then_some(destination),
            head: Some(coord(&input["head"])),
            speed_request: bits(&input["request"]),
            speed_current: bits(&input["current"]),
            speed_mult: bits(&input["mult"]),
            pushed: input["pushed"] == 1,
            ..HoverRuntime::default()
        };
        // Is_Powered 0x00516C70: the powered byte or a positive height.
        let airborne = input["powered"] == 1 || int(&input["height"]) > 0;
        hover.choose_request(
            coord(&input["location"]),
            coord(&input["head"]),
            airborne,
            int(&input["frame"]) as u32,
        );
        hover.ramp_speed(path[0] != -1 && path[0] == path[1], &rules);
        assert_eq!(
            (hover.speed_request, hover.speed_current, hover.speed_mult),
            (
                bits(&row["request"]),
                bits(&row["current"]),
                bits(&row["mult"])
            ),
            "{input}"
        );
        rows += 1;
    }
    assert_eq!(rows, 336);
}

/// The altitude controller: SetHeight's argument and the spring offset. The
/// climbing test's answer is the original run's (0x00513DC8 compare); the
/// ground-ahead lookup that feeds it (`hover_altitude`) is not compared.
#[test]
fn altitude_step_matches_original_controller() {
    let mut rows = 0;
    for row in vectors()
        .iter()
        .filter(|row| row["input"]["kind"] == "altitude")
    {
        let input = &row["input"];
        let rules = oracle_rules(input["rules"].as_str().unwrap());
        let mut hover = HoverRuntime {
            bob_offset: bits(&input["bob"]),
            ..HoverRuntime::default()
        };
        let visible = hover.altitude_step(
            int(&input["height"]),
            row["climbing"].as_bool().unwrap(),
            int(&input["id"]),
            int(&input["frame"]),
            input["powered"] == 1,
            &rules,
        );
        assert_eq!(
            (visible, hover.bob_offset),
            (int(&row["visible"]), bits(&row["bob"])),
            "{input}"
        );
        rows += 1;
    }
    assert_eq!(rows, 800);
}

/// Power_Off (0x00516BF0) through [`Simulation::hover_power_off`]: the spin
/// of a powered Foot (byte or height) neither sleeping nor entering, with
/// its two Scenario draws, then Stop_Moving (0x00516320) and the base
/// Power_Off. With no terrain, GetHeight (0x005F5F40) is the Location's Z.
#[test]
fn power_off_matches_original_execution() {
    use crate::sim::world::Simulation;

    let vectors: serde_json::Value =
        serde_json::from_str(crate::test_fixture::text("tools/robot_offline_oracle.json"))
            .expect("robot offline oracle vectors");
    let null = DriveCoord { x: 0, y: 0, z: 0 };
    let slot = |value: &serde_json::Value| Some(coord(value)).filter(|coord| *coord != null);
    let mut rows = 0;
    for row in vectors["hover_power_off"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(1, "ROBO", "Americans", 10, 12);
        let mut locomotor = super::super::locomotor::LocomotorState::for_test_kind(
            crate::rules::locomotor_type::LocomotorKind::Hover,
        );
        locomotor.set_powered_for_test(row["powered"].as_bool().unwrap());
        locomotor.set_runtime_payload_for_test(
            super::super::locomotion::piggyback::LocomotorRuntimePayload::Hover(HoverRuntime {
                destination: slot(&row["destination"]),
                head: slot(&row["head"]),
                shove_turning: row["shove"][0].as_bool().unwrap(),
                shove_steps: int(&row["shove"][1]),
                ..HoverRuntime::default()
            }),
        );
        entity.locomotor = Some(locomotor);
        entity
            .position
            .set_exact_z_leptons_for_test(Some(int(&row["height"])));
        entity.mission.set_current_for_test(
            crate::sim::mission::MissionId::from_raw(int(&row["mission"])),
            0,
        );
        entity
            .navigation
            .path_replay_mut()
            .set_directions_for_test(vec![int(&row["path_head"]) as u8]);
        let mut sim = Simulation::new();
        sim.substrate.entities.insert(entity);
        let seed = row["seed"].as_u64().unwrap();
        sim.scenario_rng = crate::sim::rng::SimRng::new(seed);
        assert!(
            crate::sim::world::Simulation::hover_power_off(
                &mut sim.scenario_rng,
                &mut sim.substrate,
                &sim.resolved_terrain,
                1
            ),
            "{name}"
        );

        let mut expected = row["expected"].clone();
        let draws = expected.as_object_mut().unwrap().remove("draws").unwrap();
        let entity = sim.substrate.entities.get(1).unwrap();
        let locomotor = entity.locomotor.as_ref().unwrap();
        let hover = locomotor.hover_runtime().unwrap();
        let rng = sim.scenario_rng.logical_view();
        let unslot = |coord: Option<DriveCoord>| {
            let coord = coord.unwrap_or(null);
            serde_json::json!([coord.x, coord.y, coord.z])
        };
        let actual = serde_json::json!({
            "shove": [hover.shove_turning, hover.shove_steps],
            "powered": locomotor.is_powered(),
            "destination": unslot(hover.destination),
            "head": unslot(hover.head),
            "path_head": entity
                .navigation
                .path_replay()
                .remaining_directions()
                .first()
                .map_or(-1, |&word| i32::from(word)),
            "rng_indices": [rng.index_a, rng.index_b],
        });
        assert_eq!(actual, expected, "{name}");
        // The recorded draws, replayed from the same seed, end where the
        // port's did.
        let mut replay = crate::sim::rng::SimRng::new(seed);
        for draw in draws.as_array().unwrap() {
            assert_eq!(
                replay.next_range_i32_inclusive(int(&draw[0]), int(&draw[1])),
                int(&draw[2]),
                "{name}"
            );
        }
        assert_eq!(replay.state(), sim.scenario_rng.state(), "{name}");
        rows += 1;
    }
    assert_eq!(rows, 49);
}
