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

/// A deferred order whose adapter is missing while +34 and a NavCom name the
/// same cell (Enter_Idle_Mode taking a NavQueue waypoint at an arrival, whose
/// true return ends that Process) only reschedules, with no setter tail or
/// timer write.
#[test]
fn deferred_order_with_a_retained_destination_reschedules_without_a_setter() {
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

fn visit(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &crate::map::overlay_types::OverlayTypeRegistry,
    id: u64,
    frame: u32,
) {
    sim.session.binary_frame = frame;
    let grid = sim.path_grid.clone();
    sim.process_ground_locomotor_for_test(id, Some(rules), grid.as_deref(), Some(registry))
        .unwrap();
}

/// Drive 0x4B0583..0x4B0667 / Ship 0x69FC93..0x69FD0E: when Process_Track(0)
/// ends a track short of +34, the same Process runs Process_Movement and
/// Process_Track(1), so the next head is committed in that call and no
/// Process leaves the moving unit without one.
#[test]
fn track_end_selects_the_next_head_in_the_same_process() {
    use crate::sim::movement::track_head::committed_track_head;
    for family in ["drive", "ship"] {
        let (mut sim, rules, registry, id) = unit(&json!({"family": family}));
        sim.substrate.entities.get_mut(id).unwrap().facing = 0x40;
        order(&mut sim, &rules, id, (16, 10));
        let mut heads = Vec::new();
        for frame in 101..600 {
            visit(&mut sim, &rules, &registry, id, frame);
            let e = sim.substrate.entities.get(id).unwrap();
            match committed_track_head(e) {
                Some(head) if heads.last() != Some(&head) => heads.push(head),
                Some(_) => {}
                None if (e.position.rx, e.position.ry) == (16, 10) => break,
                None => assert!(
                    heads.is_empty(),
                    "{family}: frame {frame} left the unit without a head at {:?}",
                    (e.position.rx, e.position.ry)
                ),
            }
        }
        let e = sim.substrate.entities.get(id).unwrap();
        assert_eq!((e.position.rx, e.position.ry), (16, 10), "{family} arrives");
        assert!(heads.len() >= 3, "{family}: {heads:?}");
    }
}

/// A moving tank ordered elsewhere keeps its committed head (Unit 0x741970
/// clears only the path word). The Process whose Process_Track(0) ends that
/// track requests the new route itself: its no-queue arm runs Find_Path in
/// that frame, which installs the route, and the found-route continuation
/// reloads +64C (0x4B3285); head selection may first turn.
#[test]
fn reorder_requests_the_new_route_in_the_process_that_ends_the_head() {
    use crate::sim::movement::track_head::committed_track_head;
    let (mut sim, rules, registry, id) = unit(&json!({"family": "drive"}));
    sim.substrate.entities.get_mut(id).unwrap().facing = 0x40;
    order(&mut sim, &rules, id, (20, 10));
    let mut frame = 101;
    let retained = loop {
        visit(&mut sim, &rules, &registry, id, frame);
        frame += 1;
        if let Some(head) = committed_track_head(sim.substrate.entities.get(id).unwrap()) {
            break head;
        }
    };
    sim.session.binary_frame = frame;
    order(&mut sim, &rules, id, (10, 16));
    loop {
        assert!(frame < 400, "the retained head never ended");
        visit(&mut sim, &rules, &registry, id, frame);
        let e = sim.substrate.entities.get(id).unwrap();
        if committed_track_head(e) == Some(retained) {
            frame += 1;
            continue;
        }
        // The arm's PathDelay write (0x4B284B) is overwritten by the Find_Path
        // wrapper's +640 = (Frame, 0) after any core result (0x4D3EB2).
        let timer = e.navigation.path_runtime.movement_timer;
        assert_eq!((timer.start_frame(), timer.duration()), (frame as i32, 0));
        assert_eq!(e.navigation.path_runtime.retries_left, 10);
        assert!(
            e.movement_target.as_ref().is_some_and(
                |target| target.final_goal == Some((10, 16)) && !target.path.is_empty()
            ),
            "the Process that ended the head installed the new route"
        );
        return;
    }
}

/// Force_Track (0x4B0C40) writes +34 = head. At the track end no NavCom skips
/// the arrival arm (0x4B2121), so +34 stays and Is_Moving holds: the same
/// Process continues into Process_Movement, whose no-queue arm asks Find_Path
/// for the unit's own cell in that frame and keeps +34. Later Processes stop
/// at the outer Guard exact-destination arm (0x4B06D5..0x4B0772, whose NULL
/// setter returns early without a NavCom) and do not search again.
#[test]
fn forced_track_end_requests_its_own_cell_in_the_same_process() {
    use crate::sim::movement::track_head::committed_track_head;
    let (mut sim, rules, registry, id) = unit(&json!({"family": "drive"}));
    // A bib step carries no route: clear the oracle prestate's path words.
    // Force_Track sets only the target fraction; with no acceleration factor
    // in the fixture rules the Accelerates ramp would never leave 0.
    let e = sim.substrate.entities.get_mut(id).unwrap();
    e.navigation.path_replay = FootPathQueue::default();
    e.drive_accelerates = false;
    let head = DriveCoord {
        x: 10 * 256,
        y: 11 * 256,
        z: 0,
    };
    assert!(sim.force_drive_track(id, 0x47, head));
    for frame in 101..300 {
        visit(&mut sim, &rules, &registry, id, frame);
        let e = sim.substrate.entities.get(id).unwrap();
        if committed_track_head(e).is_some() {
            continue;
        }
        assert!(e.navigation.nav_com.is_none());
        assert_eq!(e.drive_locomotion.as_ref().unwrap().destination, Some(head));
        // The Find_Path wrapper's +640 = (Frame, 0) (0x4D3EB2) dates this
        // Process's request.
        let timer = e.navigation.path_runtime.movement_timer;
        assert_eq!((timer.start_frame(), timer.duration()), (frame as i32, 0));
        assert!(
            e.movement_target.is_some(),
            "the retained +34 keeps scheduling"
        );
        let cell = (e.position.rx, e.position.ry);
        for later in frame + 1..frame + 10 {
            visit(&mut sim, &rules, &registry, id, later);
            let e = sim.substrate.entities.get(id).unwrap();
            let timer = e.navigation.path_runtime.movement_timer;
            assert_eq!(timer.start_frame(), frame as i32, "frame {later} searched");
            assert_eq!((e.position.rx, e.position.ry), cell);
        }
        return;
    }
    panic!("the forced track never ended");
}

/// tools/spatial_oracle/track_outer_continuation after-active rows: which
/// states after Process_Track(0) continue into Process_Movement (the native
/// `external_fresh` event) and which Unit->Infantry NavComs are re-aimed
/// (`move_to`). The oracle supplies the Process_Track AL, liveness and
/// selector retirement, Is_Moving and the NavCom answers; each becomes the
/// equivalent Rust state (a destination for Is_Moving, a NavCom object with
/// that coordinate). The out byte and owner gate before Process_Track(1)
/// belong to the Process host and are not replayed here. Rust derives
/// Is_Moving from +34 itself, so a not-moving row carries no +34 and cannot
/// exercise the re-aim's NavCom-to-+34 compare against a retained +34.
#[test]
fn after_active_rows_gate_the_same_call_continuation() {
    use crate::sim::movement::track_process::TrackFamily;
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/track_outer_continuation.json"
    ))
    .unwrap();
    let destination = DriveCoord {
        x: 2688,
        y: 2432,
        z: 48,
    };
    let int = |input: &Value, key: &str, default: i64| input[key].as_i64().unwrap_or(default);
    let mut replayed = 0;
    for row in rows
        .iter()
        .filter(|row| row["input"]["stage"] == "after_active")
    {
        let input = &row["input"];
        let (mut sim, rules, _registry, id) = unit(&json!({"family": input["family"]}));
        let family = if input["family"] == "drive" {
            TrackFamily::Drive
        } else {
            TrackFamily::Ship
        };
        let navcom = (input["navcom"] == true).then(|| {
            let coord = input["navcom_coord"]
                .as_array()
                .map_or(destination, |c| DriveCoord {
                    x: c[0].as_i64().unwrap() as i32,
                    y: c[1].as_i64().unwrap() as i32,
                    z: c[2].as_i64().unwrap() as i32,
                });
            let infantry = int(input, "navcom_rtti", 11) == 15;
            let target = sim
                .spawn_object(
                    if infantry { "ENGINEER" } else { "DRV" },
                    "Americans",
                    20,
                    20,
                    0,
                    &rules,
                    &BTreeMap::new(),
                )
                .unwrap();
            if infantry {
                let t = sim.substrate.entities.get_mut(target).unwrap();
                t.locomotor.as_mut().unwrap().set_step_head(Some(coord));
            }
            (NavTargetRef::Entity { id: target }, coord)
        });
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.lifecycle.object_alive = int(input, "active_alive", 1) != 0;
        let selector = if input["active_retires"] == false {
            0
        } else {
            -1
        };
        let moving = int(input, "is_moving", 1) != 0;
        match family {
            TrackFamily::Drive => {
                let d = e.drive_locomotion.get_or_insert_with(Default::default);
                d.track.turn_index = selector;
                d.destination = moving.then_some(destination);
                d.head_to = None;
            }
            TrackFamily::Ship => {
                let s = e.ship_locomotion.get_or_insert_with(Default::default);
                s.track.turn_index = selector;
                s.destination = moving.then_some(destination);
                s.head_to = None;
            }
        }
        let queue_head = int(input, "queue_head", -1);
        e.navigation.path_replay = FootPathQueue {
            directions: if queue_head < 0 {
                Vec::new()
            } else {
                vec![queue_head as u8]
            },
            cursor: 0,
            reference_cell: None,
        };
        if int(input, "unit_6d1", 0) != 0 {
            let mut miner = crate::sim::miner::Miner::new(
                crate::sim::miner::MinerKind::War,
                &crate::sim::miner::MinerConfig::default(),
                0,
            );
            miner.unload_active = true;
            e.miner = Some(miner);
        }
        e.navigation.nav_com = navcom.map(|(target, _)| target);
        let aborted = int(input, "active_return", 0) & 0xFF != 0;
        let continued = !aborted
            && sim
                .begin_track_end_continuation(id, family, Some(&rules))
                .unwrap();
        let events = row["events"].as_array().unwrap();
        let native_fresh = events
            .iter()
            .any(|event| event["event"] == "external_fresh");
        assert_eq!(continued, native_fresh, "{input}");
        let moved = events.iter().any(|event| event["event"] == "move_to");
        let e = sim.substrate.entities.get(id).unwrap();
        let after = match family {
            TrackFamily::Drive => e.drive_locomotion.as_ref().unwrap().destination,
            TrackFamily::Ship => e.ship_locomotion.as_ref().unwrap().destination,
        };
        let expected = if moved {
            navcom.map(|(_, coord)| coord)
        } else {
            moving.then_some(destination)
        };
        assert_eq!(after, expected, "{input}");
        replayed += 1;
    }
    assert_eq!(replayed, 97 + 14);
}

/// Drive the unit ordered to (20,10) until it commits its first track head.
fn drive_to_first_head(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &crate::map::overlay_types::OverlayTypeRegistry,
    id: u64,
) -> (DriveCoord, u32) {
    use crate::sim::movement::track_head::committed_track_head;
    sim.substrate.entities.get_mut(id).unwrap().facing = 0x40;
    order(sim, rules, id, (20, 10));
    for frame in 101..200 {
        visit(sim, rules, registry, id, frame);
        if let Some(head) = committed_track_head(sim.substrate.entities.get(id).unwrap()) {
            return (head, frame + 1);
        }
    }
    panic!("the unit never committed a head");
}

/// Visits until the committed head changes; returns that frame.
fn visit_until_head_changes(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &crate::map::overlay_types::OverlayTypeRegistry,
    id: u64,
    head: DriveCoord,
    mut frame: u32,
) -> u32 {
    use crate::sim::movement::track_head::committed_track_head;
    loop {
        assert!(frame < 400, "the running track never ended");
        visit(sim, rules, registry, id, frame);
        if committed_track_head(sim.substrate.entities.get(id).unwrap()) != Some(head) {
            return frame;
        }
        frame += 1;
    }
}

/// ReceiveDamage's retaliation, `Override(Attack, attacker, NULL)`, reaches
/// Unit 0x741970(NULL, 1) through Foot::Override_Mission (0x4D8F6D): its
/// locomotor Stop nulls +34 (0x4AFE00) and the path word is cleared, so the
/// tank finishes its running track and the same-call continuation finds it
/// neither moving nor holding a path word.
#[test]
fn retaliation_mid_track_stops_the_tank_at_its_track_end() {
    let (mut sim, rules, registry, id) = unit(&json!({"family": "drive"}));
    let (head, frame) = drive_to_first_head(&mut sim, &rules, &registry, id);
    let attacker = sim
        .spawn_object("DRV", "Russians", 20, 20, 0, &rules, &BTreeMap::new())
        .unwrap();
    assert!(sim.override_mission_on_damage_response(id, attacker, &rules));
    let e = sim.substrate.entities.get(id).unwrap();
    assert_eq!(
        e.navigation.suspended_nav_com,
        Some(NavTargetRef::cell(20, 10))
    );
    assert!(e.navigation.nav_com.is_none());
    assert!(e.drive_locomotion.as_ref().unwrap().destination.is_none());
    for frame in frame..frame + 120 {
        visit(&mut sim, &rules, &registry, id, frame);
    }
    let e = sim.substrate.entities.get(id).unwrap();
    assert!(e.drive_locomotion.as_ref().unwrap().destination.is_none());
    assert!(e.movement_target.is_none(), "no adapter was re-armed");
    assert_eq!(
        (e.position.rx, e.position.ry),
        ((head.x / 256) as u16, (head.y / 256) as u16),
        "the tank stopped at the head of its running track"
    );
}

/// A captured or transferred tank (TechnoClass::ChangeOwner's
/// `Assign_Destination(0, 1)` at 0x7014E9) stops after its running track the
/// same way instead of resuming the old owner's order.
#[test]
fn owner_change_mid_track_stops_the_tank_at_its_track_end() {
    let (mut sim, rules, registry, id) = unit(&json!({"family": "drive"}));
    let (head, frame) = drive_to_first_head(&mut sim, &rules, &registry, id);
    let owner = sim.interner.intern("Russians");
    sim.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 1, None, false, 0, 0),
    );
    sim.change_owner_with_rules(id, owner, &rules);
    let e = sim.substrate.entities.get(id).unwrap();
    assert!(e.navigation.nav_com.is_none());
    assert!(e.drive_locomotion.as_ref().unwrap().destination.is_none());
    for frame in frame..frame + 120 {
        visit(&mut sim, &rules, &registry, id, frame);
    }
    let e = sim.substrate.entities.get(id).unwrap();
    assert!(e.drive_locomotion.as_ref().unwrap().destination.is_none());
    assert_eq!(
        (e.position.rx, e.position.ry),
        ((head.x / 256) as u16, (head.y / 256) as u16),
        "the tank stopped at the head of its running track"
    );
}

/// Retaliation archives the order and a pursuit order moves +34 to its cell;
/// the target's expiry then restores the archived NavCom, which Rust defers
/// (`pending_arrival_clear`) while the track runs. Natively
/// `Assign_Destination(saved, 1)` (Restore_Mission 0x4D8F99) moved +34
/// already, so the Process that ends the track completes the order toward the
/// restored NavCom and requests its route; the stale pursuit cell is not
/// driven to.
#[test]
fn restore_mid_track_heads_for_the_restored_order_at_the_track_end() {
    let (mut sim, rules, registry, id) = unit(&json!({"family": "drive"}));
    let (head, frame) = drive_to_first_head(&mut sim, &rules, &registry, id);
    let attacker = sim
        .spawn_object("DRV", "Russians", 20, 20, 0, &rules, &BTreeMap::new())
        .unwrap();
    assert!(sim.override_mission_on_damage_response(id, attacker, &rules));
    sim.session.binary_frame = frame;
    order(&mut sim, &rules, id, (10, 16));
    sim.substrate.entities.get_mut(id).unwrap().attack_target = None;
    assert!(
        sim.mission_restore_after_target_expiry(id, Some(&rules))
            .unwrap()
    );
    let e = sim.substrate.entities.get(id).unwrap();
    assert_eq!(e.navigation.nav_com, Some(NavTargetRef::cell(20, 10)));
    assert!(e.navigation.pending_arrival_clear);
    let stale = e.drive_locomotion.as_ref().unwrap().destination.unwrap();
    assert_eq!((stale.x / 256, stale.y / 256), (10, 16));
    let ended = visit_until_head_changes(&mut sim, &rules, &registry, id, head, frame);
    let e = sim.substrate.entities.get(id).unwrap();
    let destination = e.drive_locomotion.as_ref().unwrap().destination.unwrap();
    assert_eq!((destination.x / 256, destination.y / 256), (20, 10));
    assert!(!e.navigation.pending_arrival_clear);
    assert!(
        e.movement_target
            .as_ref()
            .is_some_and(|target| target.final_goal == Some((20, 10)) && !target.path.is_empty()),
        "frame {ended}: the Process that ended the track requested the restored route"
    );
}

/// Process_Track(1) after the continuation pays the retained residual alone
/// (0x4B127A): a frame that ends a track spends at most one speed budget
/// across both Process_Track calls. Checked at cruise speed, where both speed
/// prefixes return the same budget; a point costs 7 and the terminal refund
/// adds at most 7 (0x4B1F97..0x4B2006).
#[test]
fn track_end_frame_spends_one_speed_budget() {
    use crate::sim::movement::track_head::committed_track_head;
    for family in ["drive", "ship"] {
        let (mut sim, rules, registry, id) = unit(&json!({"family": family}));
        sim.substrate.entities.get_mut(id).unwrap().facing = 0x40;
        order(&mut sim, &rules, id, (20, 10));
        let grid = sim.path_grid.clone();
        let progress = |sim: &Simulation| {
            let e = sim.substrate.entities.get(id).unwrap();
            let residual = if family == "drive" {
                e.drive_locomotion.as_ref().unwrap().track.residual
            } else {
                e.ship_locomotion.as_ref().unwrap().track.residual
            };
            (
                committed_track_head(e),
                residual,
                e.foot_speed.cached_current_speed,
            )
        };
        let mut checked = 0;
        let mut previous_speed = None;
        for frame in 101..400 {
            let (head_before, residual_before, _) = progress(&sim);
            sim.session.binary_frame = frame;
            let stats = sim
                .process_ground_locomotor_for_test(
                    id,
                    Some(&rules),
                    grid.as_deref(),
                    Some(&registry),
                )
                .unwrap();
            let (head_after, _, speed) = progress(&sim);
            let ended = head_before.is_some() && head_after.is_some() && head_after != head_before;
            if ended && previous_speed == Some(speed) && speed > 7 {
                assert!(
                    7 * stats.moved_steps as i32 <= residual_before + speed + 7,
                    "{family} frame {frame}: {} steps from residual {residual_before} and speed {speed}",
                    stats.moved_steps
                );
                checked += 1;
            }
            previous_speed = Some(speed);
            let e = sim.substrate.entities.get(id).unwrap();
            if (e.position.rx, e.position.ry) == (20, 10) && head_after.is_none() {
                break;
            }
        }
        assert!(
            checked >= 2,
            "{family}: {checked} cruise track ends checked"
        );
    }
}

/// Enter_Idle_Mode taking a NavQueue waypoint at a Move arrival returns true
/// (Foot 0x4D8382..0x4D83E2), and that Process_Track AL (0x4B2273) returns the
/// whole Process before the continuation: the waypoint's route is requested
/// by the next Process, not the arrival Process.
#[test]
fn queued_waypoint_arrival_returns_before_the_continuation() {
    use crate::sim::movement::track_head::committed_track_head;
    let (mut sim, rules, registry, id) = unit(&json!({"family": "drive", "mission": 2}));
    sim.substrate.entities.get_mut(id).unwrap().facing = 0x40;
    order(&mut sim, &rules, id, (12, 10));
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .navigation
        .nav_queue
        .push(NavTargetRef::cell(14, 10));
    for frame in 101..400 {
        visit(&mut sim, &rules, &registry, id, frame);
        let e = sim.substrate.entities.get(id).unwrap();
        if e.navigation.nav_com != Some(NavTargetRef::cell(14, 10)) {
            continue;
        }
        assert_eq!((e.position.rx, e.position.ry), (12, 10));
        assert!(e.navigation.pending_arrival_clear);
        assert!(committed_track_head(e).is_none());
        assert!(
            e.movement_target
                .as_ref()
                .is_none_or(|target| target.path.is_empty()),
            "frame {frame}: the arrival Process requested no route"
        );
        visit(&mut sim, &rules, &registry, id, frame + 1);
        let e = sim.substrate.entities.get(id).unwrap();
        assert!(!e.navigation.pending_arrival_clear);
        assert!(
            e.movement_target.as_ref().is_some_and(
                |target| target.final_goal == Some((14, 10)) && !target.path.is_empty()
            ),
            "the next Process requested the waypoint route"
        );
        return;
    }
    panic!("the unit never took its waypoint");
}
