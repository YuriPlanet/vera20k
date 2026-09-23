//! Drive/Ship `Process_Movement` continuation after `Find_Path`, against every
//! row of tools/spatial_oracle/track_path_continuation (Unit Cell 10,10 to
//! Cell 11,10 or 13,10; setter at frame 100, Process at frame 101).
//! - Supplied found/refused rows enter the continuation with the caller's
//!   PathDelay arm (0x4B284B) and the supplied Foot+5E0 route.
//! - The far native rows run the real Process corridor: an enclosed target
//!   gives the NULL core result, an open map the found route.
//! - The near native rows compose the Unit receiver (+0x500) and the
//!   Find_Path failure tail before the continuation: an adjacent target
//!   cannot be made unreachable for the search while its zone stays connected.
//! - The two far_zone rows are not replayed: splitting the zone of Cell 11,10
//!   needs a terrain rebuild of this fixture. The recheck's refusal arm they
//!   reach (0x4B28CD -> NULL setter) is the one the near rows take on the
//!   receiver-cleared destination.
//!
//! The fixture mirrors the oracle's Unit type: MovementZone Normal and
//! SpeedType Track for both locomotors.
use super::tests::fixture_with_rules;
use super::walk_failed_path_tests::enclose;
use crate::rules::mission_data::MissionType;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{DriveCoord, FootPathQueue, NavTargetRef};
use crate::sim::mission::MissionId;
use crate::sim::movement::{FindPathResult, FootPathOutcome};
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const UNITS: &str = "[VehicleTypes]\n0=DRV\n1=SHP\n\
    [DRV]\nStrength=300\nSpeed=6\nSpeedType=Track\nMovementZone=Normal\n\
    Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
    [SHP]\nStrength=300\nSpeed=6\nSpeedType=Track\nMovementZone=Normal\n\
    Locomotor={2BEA74E1-7CCA-11D3-BE14-00104B62A16C}\n";

fn corpus() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/track_path_continuation.json"
    ))
    .unwrap()
}

fn cell(v: &Value) -> (u16, u16) {
    (v[0].as_u64().unwrap() as u16, v[1].as_u64().unwrap() as u16)
}

/// The oracle fixture's prestates: House control, Rules PathDelay 0.01 (the
/// exact double, 9 frames), BlockagePathDelay 22, CloseEnough, mission, and
/// Foot+5E0 = 2,3,4,5 with reference 9,8 before the setter.
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
    sim.close_enough = SimFixed::from_num(input["close_enough"].as_i64().unwrap_or(128));
    let owner = sim.interner.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 0);
    house.player_control = input["human"] != false;
    sim.houses.insert(owner, house);
    let kind = if input["family"] == "drive" {
        "DRV"
    } else {
        "SHP"
    };
    sim.session.binary_frame = 100;
    let id = sim
        .spawn_object(kind, "Americans", 10, 10, 0, &rules, &BTreeMap::new())
        .unwrap();
    let mission = input["mission"]
        .as_i64()
        .map_or(MissionType::Guard, |m| match m {
            2 => MissionType::Move,
            7 => MissionType::Enter,
            11 => MissionType::AreaGuard,
            _ => panic!("unmapped mission {m}"),
        });
    sim.mission_assign_exact(id, MissionId::from_known(mission), 100)
        .unwrap();
    let e = sim.substrate.entities.get_mut(id).unwrap();
    e.navigation.path_replay = FootPathQueue {
        directions: vec![2, 3, 4, 5],
        cursor: 0,
        reference_cell: Some((9, 8)),
    };
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

fn words(queue: &FootPathQueue) -> Vec<i32> {
    queue
        .directions
        .iter()
        .take(4)
        .map(|&v| if v == u8::MAX { -1 } else { i32::from(v) })
        .collect()
}

fn compare(sim: &Simulation, id: u64, row: &Value, outcome: Option<FootPathOutcome>) {
    let e = sim.substrate.entities.get(id).unwrap();
    let state = &row["state"];
    let (destination, head, selector) = if row["input"]["family"] == "drive" {
        let d = e.drive_locomotion.as_ref().unwrap();
        (d.destination, d.head_to, d.track.turn_index)
    } else {
        let s = e.ship_locomotion.as_ref().unwrap();
        (s.destination, s.head_to, s.track.turn_index)
    };
    let p = &e.navigation.path_runtime;
    let nav = e.navigation.nav_com.map(|n| match n {
        NavTargetRef::Cell { rx, ry } => json!([rx, ry]),
        other => panic!("cell fixture, got {other:?}"),
    });
    let destination = destination.unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
    let mut fields = vec![
        (
            "destination",
            json!([destination.x, destination.y, destination.z]),
        ),
        ("nav", json!(nav)),
        ("retries", json!(p.retries_left)),
        (
            "movement_timer",
            json!([p.movement_timer.start_frame(), p.movement_timer.duration()]),
        ),
        (
            "blocked_timer",
            json!([p.blocked_timer.start_frame(), p.blocked_timer.duration()]),
        ),
        ("mission", json!(e.mission.current().raw())),
        ("queued", json!(e.mission.queued().raw())),
    ];
    // A returned visit ends before head selection; a resumed one ran it, so
    // the head, selector and live path word are no longer the oracle's stop.
    if state["returned"] == true {
        let head = head.unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
        fields.push(("head", json!([head.x, head.y, head.z])));
        fields.push(("selector", json!(selector)));
        fields.push(("path", json!(words(&e.navigation.path_replay))));
    }
    for (key, actual) in fields {
        assert_eq!(actual, state[key], "{key}: {row}");
    }
    if let Some(outcome) = outcome {
        let expected = if state["returned"] == true {
            FootPathOutcome::Returned
        } else {
            FootPathOutcome::Resume
        };
        assert_eq!(outcome, expected, "{row}");
    }
}

#[test]
fn supplied_find_path_results_take_the_native_continuations() {
    let mut checked = 0;
    for row in corpus() {
        let input = &row["input"];
        if input["find_path"] == "native" || input.get("far_zone").is_some() {
            continue;
        }
        let (mut sim, rules, registry, id) = unit(input);
        order(
            &mut sim,
            &rules,
            id,
            cell(input.get("destination").unwrap_or(&json!([11, 10]))),
        );
        let e = sim.substrate.entities.get_mut(id).unwrap();
        if input["empty_path"] == true {
            e.navigation.path_replay.clear_live_head();
        }
        e.navigation.path_runtime.retries_left = input["retries"].as_u64().unwrap_or(10) as u32;
        sim.session.binary_frame = 101;
        if row["events"].as_array().unwrap().is_empty() {
            // 0x741C54..0x741C78: Enter without a radio contact keeps the path
            // word, so Process never reaches the no-queue arm.
            let e = sim.substrate.entities.get(id).unwrap();
            assert_eq!(
                e.navigation.path_replay.remaining_directions(),
                &[2, 3, 4, 5]
            );
            compare(&sim, id, &row, None);
            checked += 1;
            continue;
        }
        let e = sim.substrate.entities.get_mut(id).unwrap();
        // 0x4B284B / 0x6A1EA0: the caller arms PathDelay before Find_Path.
        e.navigation
            .path_runtime
            .start_movement(101, rules.general.path_delay_ticks());
        let found = if input["find_path"] == "found" {
            let mut directions: Vec<u8> = input["route"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u8)
                .collect();
            directions.resize(24, u8::MAX);
            e.navigation.path_replay.directions = directions;
            e.navigation.path_replay.cursor = 0;
            FindPathResult::Route
        } else {
            FindPathResult::Failed
        };
        let outcome = sim
            .continue_track_path_request(id, found, &rules, Some(&registry))
            .unwrap();
        compare(&sim, id, &row, Some(outcome));
        checked += 1;
    }
    assert_eq!(checked, 22);
}

#[test]
fn far_native_rows_through_the_process_corridor() {
    let mut checked = 0;
    for row in corpus() {
        let input = &row["input"];
        if input["find_path"] != "native" || cell(&input["destination"]) != (13, 10) {
            continue;
        }
        let (mut sim, rules, registry, id) = unit(input);
        if input.get("route").is_none() {
            // The zone stays connected while the search cannot reach 13,10:
            // the oracle's supplied NULL core result.
            enclose(&mut sim, &rules, (13, 10));
        }
        // The route leaves east; facing it lets head selection install the
        // track in the resumed visit instead of turning first.
        sim.substrate.entities.get_mut(id).unwrap().facing = 0x40;
        order(&mut sim, &rules, id, (13, 10));
        sim.session.binary_frame = 101;
        let grid = sim.path_grid.clone();
        sim.process_ground_locomotor_for_test(id, Some(&rules), grid.as_deref(), Some(&registry))
            .unwrap();
        compare(&sim, id, &row, None);
        if input.get("route").is_some() {
            // The found route [2, 2, 2]; head selection (0x4B32A1 / 0x6A28F1)
            // then commits the first track in the same visit because the
            // body already faces east (a turn would return after Do_Turn,
            // 0x4B343B, as drive_fresh_turn records).
            let e = sim.substrate.entities.get(id).unwrap();
            assert_eq!(words(&e.navigation.path_replay)[..3], [2, 2, 2]);
            assert!(crate::sim::movement::track_head::committed_track_head(e).is_some());
        }
        checked += 1;
    }
    assert_eq!(checked, 6);
}

#[test]
fn near_native_rows_stop_through_the_continuation_null_setter() {
    let mut checked = 0;
    for row in corpus() {
        let input = &row["input"];
        if input["find_path"] != "native" || cell(&input["destination"]) != (11, 10) {
            continue;
        }
        let (mut sim, rules, registry, id) = unit(input);
        order(&mut sim, &rules, id, (11, 10));
        sim.session.binary_frame = 101;
        // 0x4D4044 Unit +0x500 = 0x4D55C0 (locomotor Stop), then the
        // 0x4D404A tail, which returns for a Chebyshev-1 target.
        sim.run_find_path_failed_receiver(id, &rules, Some(&registry))
            .unwrap();
        sim.finish_find_path_failure(id, DriveCoord::cell(11, 10, 0), &rules)
            .unwrap();
        let e = sim.substrate.entities.get(id).unwrap();
        assert!(e.navigation.nav_com.is_some(), "the near tail keeps NavCom");
        let outcome = sim
            .continue_track_path_request(id, FindPathResult::Failed, &rules, Some(&registry))
            .unwrap();
        compare(&sim, id, &row, Some(outcome));
        checked += 1;
    }
    assert_eq!(checked, 4);
}

/// A mission restore writes NavCom alone while +34 may still hold an older
/// order (here a pursuit cell whose adapter was dropped). Native NavCom and
/// +34 never disagree after Unit 0x741970, so the deferred order completes the
/// class setter toward NavCom instead of rescheduling the stale cell.
#[test]
fn deferred_restore_completes_toward_navcom_over_a_stale_destination() {
    let (mut sim, rules, _registry, id) = unit(&json!({"family": "drive"}));
    order(&mut sim, &rules, id, (13, 10));
    let e = sim.substrate.entities.get_mut(id).unwrap();
    e.movement_target = None;
    e.navigation.nav_com = Some(NavTargetRef::cell(10, 13));
    e.navigation.pending_arrival_clear = true;
    sim.session.binary_frame = 101;
    sim.complete_pending_track_order(id, Some(&rules));
    let e = sim.substrate.entities.get(id).unwrap();
    let destination = e.drive_locomotion.as_ref().unwrap().destination.unwrap();
    assert_eq!((destination.x / 256, destination.y / 256), (10, 13));
    assert_eq!(
        e.movement_target.as_ref().unwrap().final_goal,
        Some((10, 13))
    );
    assert!(!e.navigation.pending_arrival_clear);
    // Foot 0x4D96C2..0x4D9707: the completed setter's accept tail.
    let timer = e.navigation.path_runtime.movement_timer;
    assert_eq!((timer.start_frame(), timer.duration()), (101, 0));
}

/// A track that ended short keeps +34 and a NavCom naming the same cell: the
/// deferred order only reschedules, with no setter tail or timer write.
#[test]
fn deferred_short_track_end_reschedules_without_a_setter() {
    let (mut sim, rules, _registry, id) = unit(&json!({"family": "drive"}));
    order(&mut sim, &rules, id, (13, 10));
    let e = sim.substrate.entities.get_mut(id).unwrap();
    e.movement_target = None;
    e.navigation.pending_arrival_clear = true;
    e.navigation.path_runtime.start_movement(100, 9);
    sim.session.binary_frame = 101;
    sim.complete_pending_track_order(id, Some(&rules));
    let e = sim.substrate.entities.get(id).unwrap();
    assert_eq!(
        e.movement_target.as_ref().unwrap().final_goal,
        Some((13, 10))
    );
    assert!(!e.navigation.pending_arrival_clear);
    let timer = e.navigation.path_runtime.movement_timer;
    assert_eq!((timer.start_frame(), timer.duration()), (100, 9));
}

/// An ordered Attack reaches Unit 0x741970(NULL, 1) through EventClass
/// 0x4C747C. A moving tank finishes its running track at the head and stays
/// put: nothing re-arms the old destination at the track end. The command
/// path's `issue_attack_command` drops a non-Walk adapter before the setter;
/// the active descriptor still reaches Process_Track (Drive4B055A..0576).
#[test]
fn ordered_attack_null_destination_stops_a_moving_tank_after_its_track() {
    for adapter_dropped in [true, false] {
        let (mut sim, rules, registry, id) = unit(&json!({"family": "drive"}));
        sim.substrate.entities.get_mut(id).unwrap().facing = 0x40;
        order(&mut sim, &rules, id, (20, 10));
        let grid = sim.path_grid.clone();
        let mut frame = 101;
        let mut visit = |sim: &mut Simulation, frame: &mut u32| {
            sim.session.binary_frame = *frame;
            sim.process_ground_locomotor_for_test(
                id,
                Some(&rules),
                grid.as_deref(),
                Some(&registry),
            )
            .unwrap();
            *frame += 1;
        };
        visit(&mut sim, &mut frame);
        let head = crate::sim::movement::track_head::committed_track_head(
            sim.substrate.entities.get(id).unwrap(),
        )
        .expect("the tank is driving its first track");
        if adapter_dropped {
            sim.substrate.entities.get_mut(id).unwrap().movement_target = None;
        }
        sim.finish_ordered_attack_destination(id, Some(&rules));
        let e = sim.substrate.entities.get(id).unwrap();
        assert!(e.navigation.nav_com.is_none());
        assert!(e.drive_locomotion.as_ref().unwrap().destination.is_none());
        for _ in 0..120 {
            visit(&mut sim, &mut frame);
        }
        let e = sim.substrate.entities.get(id).unwrap();
        assert!(e.navigation.nav_com.is_none());
        assert!(e.drive_locomotion.as_ref().unwrap().destination.is_none());
        assert!(e.movement_target.is_none(), "no adapter was re-armed");
        assert!(!e.navigation.pending_arrival_clear);
        assert_eq!(
            (e.position.rx, e.position.ry),
            ((head.x / 256) as u16, (head.y / 256) as u16),
            "the tank stopped at its running track's head (adapter dropped: {adapter_dropped})"
        );
    }
}

/// Production-shaped depot repair: the order names the depot's dock cell
/// (inside the foundation), the setter accepts it unchanged and the first
/// Process's Find_Path must still bring the tank to the pad. A NULL core
/// result there would drop the order (NULL setter and a queued Guard).
#[test]
fn depot_repair_order_reaches_the_pad_through_find_path() {
    let depot_rules = format!(
        "{UNITS}[BuildingTypes]\n1=DEPOT\n\
         [DEPOT]\nStrength=800\nFoundation=3x3\nUnitRepair=yes\n"
    );
    let (mut sim, rules, registry) = fixture_with_rules(&depot_rules);
    let owner = sim.interner.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 5000, 0);
    house.player_control = true;
    sim.houses.insert(owner, house);
    let depot = sim
        .spawn_object("DEPOT", "Americans", 16, 9, 0, &rules, &BTreeMap::new())
        .unwrap();
    let tank = sim
        .spawn_object("DRV", "Americans", 10, 10, 0, &rules, &BTreeMap::new())
        .unwrap();
    sim.substrate.entities.get_mut(tank).unwrap().health.current = 150;
    let grid = sim.path_grid.clone();
    assert!(sim.apply_command(
        "Americans",
        &crate::sim::command::Command::RepairAtDepot {
            entity_id: tank,
            depot_id: depot,
        },
        Some(&rules),
        grid.as_deref(),
        &BTreeMap::new(),
    ));
    let mut docked = false;
    for _ in 0..400 {
        sim.advance_tick(
            &[],
            Some(&rules),
            &BTreeMap::new(),
            None,
            Some(&registry),
            67,
        );
        let e = sim.substrate.entities.get(tank).unwrap();
        let phase = e.dock_state.as_ref().map(|state| state.phase);
        if matches!(
            phase,
            Some(crate::sim::docking::building_dock::DockPhase::EnterDock)
                | Some(crate::sim::docking::building_dock::DockPhase::Servicing)
        ) {
            docked = true;
            break;
        }
    }
    let e = sim.substrate.entities.get(tank).unwrap();
    assert!(
        docked,
        "tank never docked: cell {:?} dock {:?} nav {:?} mission {:?}/{:?}",
        (e.position.rx, e.position.ry),
        e.dock_state.as_ref().map(|state| state.phase),
        e.navigation.nav_com,
        e.mission.current(),
        e.mission.queued()
    );
}

/// tools/spatial_oracle/track_order_path: after the accepted setter the first
/// Process requests a route only once Foot+640 has expired (Drive 0x4B2825 /
/// Ship 0x6A1E75), powered or not, and neither the setter nor the Process
/// changes the power byte.
#[test]
fn first_process_request_waits_for_the_movement_timer_and_keeps_power() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/track_order_path.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 8);
    for row in rows {
        let input = &row["input"];
        let (mut sim, rules, registry, id) = unit(&json!({"family": input["family"]}));
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.locomotor.as_mut().unwrap().powered = input["power_off"] != true;
        order(&mut sim, &rules, id, (11, 10));
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.navigation
            .path_runtime
            .start_movement(100, input["delay"].as_i64().unwrap() as i32);
        sim.session.binary_frame = 100;
        let grid = sim.path_grid.clone();
        sim.process_ground_locomotor_for_test(id, Some(&rules), grid.as_deref(), Some(&registry))
            .unwrap();
        let e = sim.substrate.entities.get(id).unwrap();
        let requested = e
            .movement_target
            .as_ref()
            .is_some_and(|target| !target.path.is_empty());
        assert_eq!(requested, row["requested_path"] == true, "{row}");
        assert_eq!(
            u64::from(e.locomotor.as_ref().unwrap().powered),
            row["process"]["power"].as_u64().unwrap(),
            "{row}"
        );
    }
}
