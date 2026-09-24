//! Drive/Ship `Process_Movement` fresh arm against every row of
//! tools/spatial_oracle/track_fresh_response: Unit Cell 10,10 ordered to its
//! destination at frame 100, the route written to Foot+5E0 after the setter,
//! the body resting on the first word's octant, then Process_Movement(&out,
//! 1, 0) at frame 101 with every recursion it makes.
//! - Unit `Can_Enter_Cell` and `Find_Path` answer from the row's supplied
//!   queues; `Scatter_Objects` and `Override_Mission` are recorded and their
//!   bodies skipped, as in the oracle (`movement::fresh_oracle_seam`).
//! - The gate question and the Unit setter run their Rust owners; their
//!   oracle events are not compared, their effects are (destination, NavCom,
//!   timers).
//! - Drive+64 (the straight byte) is not represented (track_fresh residual);
//!   its rows still compare the selector it forces.
//! - The far_zone rows are not replayed: splitting the zone of Cell 13,10
//!   needs a terrain rebuild of this fixture. The refusal they reach
//!   (0x4B3A3E, SetDestination(NULL)) is the one the code-1/7 rows take.
use super::tests::fixture_with_rules;
use crate::rules::mission_data::MissionType;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::mission::MissionId;
use crate::sim::movement::FacingClass;
use crate::sim::movement::ProcessMovementArgs;
use crate::sim::movement::fresh_oracle_seam::{self, FreshCallRecord, SuppliedPath};
use crate::sim::movement::track_process::TrackFamily;
use crate::sim::world::Simulation;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The oracle's Unit type (MovementZone Normal, SpeedType Track) for both
/// locomotors; O5 carries +22D Crushable and O6 +2A8 Wall, the two supplied
/// overlay kinds.
const UNITS: &str = "[VehicleTypes]\n0=DRV\n1=SHP\n\
    [DRV]\nStrength=300\nSpeed=6\nSpeedType=Track\nMovementZone=Normal\n\
    Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
    [SHP]\nStrength=300\nSpeed=6\nSpeedType=Track\nMovementZone=Normal\n\
    Locomotor={2BEA74E1-7CCA-11D3-BE14-00104B62A16C}\n\
    [O5]\nCrushable=yes\n[O6]\nWall=yes\n";

fn corpus() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/track_fresh_response.json"
    ))
    .unwrap()
}

fn pair(v: &Value) -> (i64, i64) {
    (v[0].as_i64().unwrap(), v[1].as_i64().unwrap())
}

fn words(v: &Value) -> Vec<u8> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_u64().unwrap() as u8)
        .collect()
}

/// The oracle's prestates: House control, Rules PathDelay 0.01 (9 frames),
/// BlockagePathDelay 22 and CloseEnough, a Move mission, and the reference
/// cell 9,8 of `track_destination`'s fixture.
fn unit(
    input: &Value,
) -> (
    Simulation,
    RuleSet,
    crate::map::overlay_types::OverlayTypeRegistry,
    u64,
) {
    let (mut sim, mut rules, registry) = fixture_with_rules(UNITS);
    rules.general.path_delay = 0.01;
    rules.general.blockage_path_delay_ticks = 22;
    assert_eq!(rules.general.path_delay_ticks(), 9);
    rules.general.close_enough = input["close_enough"].as_i64().unwrap_or(576) as i32;
    let owner = sim.interner.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 0);
    house.player_control = true;
    sim.houses.insert(owner, house);
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    for overlay in input["overlays"].as_array().into_iter().flatten() {
        let (x, y) = pair(overlay);
        let id = match (overlay[2] == true, overlay[3] == true) {
            (true, false) => 5,
            (false, true) => 6,
            other => panic!("unmapped overlay flags {other:?}"),
        };
        terrain
            .cell_mut(x as u16, y as u16)
            .unwrap()
            .bridge_facts
            .overlay_id = Some(id);
    }
    // The oracle writes Cell+EC = Tunnel; its unconstructed UnitType leaves
    // ObjectType+0x235 (ctor 0x5F716E: 1) clear, so its Mark skips the
    // Recalc(-1) of Exit 0x5687F0 that a real Unit runs. A real Tunnel Cell
    // keeps +EC through that Recalc because its tile is a Tunnel tile.
    for tunnel in input["tunnel"].as_array().into_iter().flatten() {
        let (x, y) = pair(tunnel);
        crate::map::resolved_terrain::install_tunnel_repair_test_catalog(terrain);
        let cell = terrain.cell_mut(x as u16, y as u16).unwrap();
        cell.final_tile_index = 1;
        cell.yr_cell_land_type = 10;
        cell.base_yr_cell_land_type = 10;
    }
    let kind = if input["family"] == "drive" {
        "DRV"
    } else {
        "SHP"
    };
    sim.session.binary_frame = 100;
    let id = sim
        .spawn_object(kind, "Americans", 10, 10, 0, &rules, &BTreeMap::new())
        .unwrap();
    sim.mission_assign_exact(id, MissionId::from_known(MissionType::Move), 100)
        .unwrap();
    (sim, rules, registry, id)
}

/// Unit 0x741970(cell, 1) through the ordinary command owner at frame 100.
fn order(sim: &mut Simulation, rules: &RuleSet, id: u64, target: (u16, u16)) {
    let speed = sim.resolve_move_info(id, Some(rules)).unwrap().speed;
    let grid = sim.path_grid.clone().unwrap();
    assert!(crate::sim::movement::issue_move_command_with_layered(
        &mut sim.substrate.entities,
        &grid,
        id,
        target,
        speed,
        false,
        None,
        None,
        sim.resolved_terrain.as_ref(),
        sim.zone_grid.as_ref(),
        None,
        None,
        sim.playfield_bounds,
        None,
        crate::sim::movement::DestinationTiming::new(100, 22),
    ));
}

/// The row's supplied answers, in call order.
fn supplied(input: &Value) -> (Vec<u8>, Vec<SuppliedPath>) {
    let codes = input["codes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|code| code.as_u64().unwrap() as u8)
        .collect();
    let paths = input["find_path"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|path| match path {
            Value::String(failed) if failed == "failed" => SuppliedPath::Failed,
            words_value => SuppliedPath::Found(words(words_value)),
        })
        .collect();
    (codes, paths)
}

/// The oracle's substituted calls with the caller's arguments. The gate
/// question and the Unit setter are original bytes there, not calls here.
fn expected_records(events: &Value) -> Vec<FreshCallRecord> {
    events
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| {
            let kind = event[0].as_str()?;
            Some(match kind {
                "can_enter" => {
                    // Unit+1AC(cell, dir, height, 0, 1).
                    assert_eq!((event[4].as_i64(), event[5].as_i64()), (Some(0), Some(1)));
                    let (x, y) = pair(&event[1]);
                    FreshCallRecord::CanEnter {
                        cell: (x as i16, y as i16),
                        direction: event[2].as_i64().unwrap() as i32,
                        height: event[3].as_i64().unwrap() as i32,
                        code: event[6].as_u64().unwrap() as u8,
                    }
                }
                "find_path" => {
                    // Find_Path(cell, IsTrain = 0, urgency).
                    assert_eq!(event[3].as_i64(), Some(0));
                    FreshCallRecord::FindPath {
                        cell: (
                            event[1].as_i64().unwrap() as i32,
                            event[2].as_i64().unwrap() as i32,
                        ),
                        urgency: event[4].as_u64().unwrap() as u8,
                    }
                }
                "scatter" => {
                    // Scatter_Objects(Null, 1, force, deck).
                    assert_eq!(event[2].as_i64(), Some(1));
                    let (x, y) = pair(&event[1]);
                    FreshCallRecord::Scatter {
                        cell: (x as i16, y as i16),
                        forced: event[3] != 0,
                        deck: event[4] != 0,
                    }
                }
                "override" => {
                    // Override_Mission(Attack, cell, NULL).
                    assert_eq!((event[1].as_i64(), event[3].as_i64()), (Some(1), Some(0)));
                    let (x, y) = pair(&event[2]);
                    FreshCallRecord::Override {
                        target: TargetKind::Cell(x as u16, y as u16),
                    }
                }
                "unit_destination" => return None,
                other => panic!("unmapped oracle event {other}"),
            })
        })
        .collect()
}

fn compare(sim: &Simulation, id: u64, row: &Value, out: bool) {
    let e = sim.substrate.entities.get(id).unwrap();
    let state = &row["state"];
    let (destination, head, valid, selector, target) = if row["input"]["family"] == "drive" {
        let d = e.drive_locomotion.as_ref().unwrap();
        (
            d.destination,
            d.head_to,
            d.track_valid,
            d.track.turn_index,
            d.target_speed_fraction,
        )
    } else {
        let s = e.ship_locomotion.as_ref().unwrap();
        (
            s.destination,
            s.head_to,
            s.track_valid,
            s.track.turn_index,
            s.target_speed_fraction,
        )
    };
    let coord = |c: Option<DriveCoord>| {
        let c = c.unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
        json!([c.x, c.y, c.z])
    };
    let nav = e.navigation.nav_com.map(|n| match n {
        NavTargetRef::Cell { rx, ry } => json!([rx, ry]),
        other => panic!("cell fixture, got {other:?}"),
    });
    // Native shifts consumed words out of Foot+5E0; the Rust queue advances
    // a cursor over the same words.
    let queue = &e.navigation.path_replay;
    let path: Vec<i32> = (0..6)
        .map(|i| {
            queue
                .directions
                .get(usize::from(queue.cursor) + i)
                .map_or(-1, |&w| if w == u8::MAX { -1 } else { i32::from(w) })
        })
        .collect();
    let reference = queue.reference_cell.unwrap_or((0, 0));
    let p = &e.navigation.path_runtime;
    let fields = [
        ("destination", coord(destination)),
        ("head", coord(head)),
        ("valid", json!(u8::from(valid))),
        ("selector", json!(selector)),
        ("nav", json!(nav)),
        ("path", json!(path)),
        ("reference", json!([reference.0, reference.1])),
        (
            "movement_timer",
            json!([p.movement_timer.start_frame(), p.movement_timer.duration()]),
        ),
        (
            "blocked_timer",
            json!([p.blocked_timer.start_frame(), p.blocked_timer.duration()]),
        ),
        ("latched", json!(u8::from(p.path_blocked))),
        ("retries", json!(p.retries_left)),
        (
            "facing",
            json!(e.body_facing.as_ref().unwrap().destination()),
        ),
        ("mission", json!(e.mission.current().raw())),
        ("out", json!(u8::from(out))),
    ];
    for (key, actual) in fields {
        assert_eq!(actual, state[key], "{key}: {row}");
    }
    // Loco+50 and Foot+578 are doubles; SimFixed holds 16 fraction bits.
    for (key, actual) in [
        ("target_speed", target),
        ("applied_speed", e.foot_speed.applied_fraction),
    ] {
        let expected = state[key].as_f64().unwrap();
        assert!(
            (actual.to_num::<f64>() - expected).abs() < 1.0 / 65536.0,
            "{key}: {actual} vs {expected}: {row}"
        );
    }
}

#[test]
fn fresh_arm_rows_match_the_original_responses() {
    let mut checked = 0;
    for row in corpus() {
        let input = &row["input"];
        if input.get("far_zone").is_some() {
            continue;
        }
        let (mut sim, rules, registry, id) = unit(input);
        let destination = input.get("destination").map_or((13, 10), pair);
        order(
            &mut sim,
            &rules,
            id,
            (destination.0 as u16, destination.1 as u16),
        );
        let family = if input["family"] == "drive" {
            TrackFamily::Drive
        } else {
            TrackFamily::Ship
        };
        let route = words(&input["route"]);
        let rest = input["facing"]
            .as_u64()
            .map_or(u16::from(route[0] & 7) << 13, |facing| facing as u16);
        let frame = |v: &Value, key: &str, default: [i64; 3]| {
            v.get(key).map_or(default, |t| {
                [
                    t[0].as_i64().unwrap(),
                    t[1].as_i64().unwrap(),
                    t[2].as_i64().unwrap(),
                ]
            })
        };
        let movement = frame(input, "movement_timer", [100, 0, 0]);
        let blocked = frame(input, "blocked_timer", [100, 0, 22]);
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.navigation.path_replay.directions = route;
        e.navigation.path_replay.cursor = 0;
        e.navigation.path_replay.reference_cell = Some((9, 8));
        let rot = e.locomotor.as_ref().map_or(0, |loco| loco.rot);
        e.body_facing = Some(FacingClass::new(rest, rot));
        e.facing = (rest >> 8) as u8;
        let runtime = &mut e.navigation.path_runtime;
        runtime.start_movement(movement[0] as u32, movement[2] as i32);
        runtime.start_blocked(blocked[0] as u32, blocked[2] as i32);
        runtime.path_blocked = input["latched"] == true;
        runtime.retries_left = input["retries"].as_u64().unwrap_or(10) as u32;
        sim.session.binary_frame = 101;
        let (codes, paths) = supplied(input);
        fresh_oracle_seam::install(codes, paths);
        let grid = sim.path_grid.clone();
        let out = sim.run_track_process_movement(
            id,
            family,
            ProcessMovementArgs::OUTER,
            &rules,
            grid.as_deref(),
            Some(&registry),
        );
        let (records, unused) = fresh_oracle_seam::finish();
        let out = out.unwrap();
        assert_eq!(records, expected_records(&row["events"]), "{row}");
        assert_eq!(unused, 0, "unused answers: {row}");
        compare(&sim, id, &row, out);
        checked += 1;
    }
    assert_eq!(checked, 58);
}
