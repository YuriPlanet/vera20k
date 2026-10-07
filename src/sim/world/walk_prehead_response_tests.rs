//! Original75B696 supplied-result corpus through the production Walk response.
//! Initial CanEnter and FindPath answers are the declared native boundaries;
//! recursive CanEnter uses the live caller's real direction/height producer.
//! Shared Scatter, mission, setters, uncloaking and head owners execute. Empty
//! scatter/gate lists, ordinary Type+D94=false and Infantry are the corpus bounds.
//! Compare all represented outputs, the retained Foot+68A byte and full
//! Scenario RNG, excluding only native timer padding.
use super::tests::fixture_with_rules;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::cloak_disguise::CloakRuntime;
use crate::sim::combat::TargetKind;
use crate::sim::components::{DriveCoord, MovementTarget, NavTargetRef};
use crate::sim::mission::{MissionDispatchTimer, MissionId, state::MissionTestFixture};
use crate::sim::movement::fresh_oracle_seam::{self, FreshCallRecord, SuppliedPath};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{CellListInsertion, OccupancyGrid, RawCellOccupationGrid};
use crate::sim::rng::SimRng;
use crate::sim::timer::CdTimer;
use crate::sim::world::Simulation;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};
use serde_json::{Value, json};

const CELL: u64 = 0x2000_0000;
const OTHER: u64 = 0x2002_1000;
const ATTACK_CELL: u64 = 0x2001_9400;
// Original walk_prehead_response.py initializes Rules+628 to10, independently
// of the obstacle's cloak component. Exercise the live production Rules reader.
const EXTRA: &str = "[General]\nCloakingStages=10\n[ENGINEER]\nMovementZone=Infantry\n\
    [VehicleTypes]\n0=OBSTACLE\n[OBSTACLE]\nStrength=100\nSpeed=4\n\
    CloakingSpeed=4\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n";

fn corpus() -> Vec<Value> {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/walk_prehead_response.json",
    ))
    .unwrap()
}

fn coord(value: &Value) -> DriveCoord {
    DriveCoord {
        x: value[0].as_i64().unwrap() as i32,
        y: value[1].as_i64().unwrap() as i32,
        z: value[2].as_i64().unwrap() as i32,
    }
}

fn xyz(value: Option<DriveCoord>) -> Value {
    let c = value.unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
    json!([c.x, c.y, c.z])
}

fn fixture(row: &Value) -> (Simulation, RuleSet, OverlayTypeRegistry, u64, Option<u64>) {
    let input = &row["input"];
    let before = &row["before"];
    let extra = format!(
        "{EXTRA}[O0]\nWall={}\n",
        input["wall"].as_bool().unwrap_or(false)
    );
    let (mut sim, mut rules, registry) = fixture_with_rules(&extra);
    // Native supplies Map+F4 Size8,8 and final LocalSize0,0,8,8. The
    // shared bridge fixture's Size16 makes this sloped current cell a fringe
    // cell, which would short-circuit56D100 before its raw-label comparison.
    sim.playfield_bounds =
        Some(crate::map::playfield::PlayfieldBounds::from_normalized_local_size(8, 0, 0, 8, 8));
    sim.playfield_size_height = Some(8);
    rules.general.path_delay = input["path_delay"].as_f64().unwrap_or(0.01);
    rules.general.blockage_path_delay_ticks = input["blockage_delay"].as_i64().unwrap_or(22) as i32;
    rules.general.close_enough = input["close_enough"].as_i64().unwrap_or(128) as i32;
    // This decoder corpus executes the native750920 quiet return (index-1).
    rules.general.scold_sound = None;
    let owner = sim.interner.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 0);
    house.player_control = true;
    sim.houses.insert(owner, house);
    let id = sim
        .spawn_object("ENGINEER", "Americans", 9, 10, 0, &rules)
        .unwrap();
    let other = if input["obstacle"] == true || input.get("cloak").is_some() {
        Some(
            sim.spawn_object(
                "OBSTACLE",
                if input["allied"] == true {
                    "Americans"
                } else {
                    "Soviets"
                },
                10,
                10,
                0,
                &rules,
            )
            .unwrap(),
        )
    } else {
        None
    };
    // The original supplied cells both have level2/slope1. Their selected
    // map-node row is cluster0->1, except zone_rejected's candidate cluster1->2.
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    for (x, y) in [(9, 10), (10, 10), (11, 10)] {
        let cell = terrain.cell_mut(x, y).unwrap();
        cell.level = 2;
        cell.slope_type = 1;
    }
    terrain.cell_mut(9, 10).unwrap().yr_cell_land_type =
        input["current_land"].as_u64().unwrap_or(0) as u8;
    let candidate = terrain.cell_mut(10, 10).unwrap();
    candidate.level = input["candidate_level"].as_i64().unwrap_or(2) as u8;
    candidate.bridge_facts.raw_flags = input["candidate_flags"].as_u64().unwrap_or(0) as u32;
    candidate.bridge_facts.overlay_id = input.get("wall").map(|_| 0);
    if input["current_missing"] == true {
        let allocated: Vec<(u16, u16)> = (0..33)
            .flat_map(|y| (0..33).map(move |x| (x, y)))
            .filter(|cell| *cell != (9, 10))
            .collect();
        terrain.test_set_native_allocated_cells(&allocated);
        terrain.shared_cell_dummy().stamp_coord(0, 0);
        terrain
            .shared_cell_dummy()
            .test_set_land_type(input["dummy_land"].as_i64().unwrap() as i32);
    }
    let base = sim.zone_grid.as_mut().unwrap().base_topology_mut();
    base.native_bridge_source_size = Some((8, 8));
    base.zone_ids.fill(0);
    for row in &mut base.raw_zone_ids_by_row {
        *row = vec![1, 2];
    }
    if input["zone_rejected"] == true {
        base.zone_ids[10 * 33 + 10] = 1;
    }
    if input["target_zone_rejected"] == true {
        base.zone_ids[10 * 33 + 11] = 1;
    }
    let current = input.get("current").map(coord).unwrap_or(DriveCoord {
        x: 2496,
        y: 2624,
        z: 260,
    });
    let e = sim.substrate.entities.get_mut(id).unwrap();
    e.position.rx = (current.x / 256) as u16;
    e.position.ry = (current.y / 256) as u16;
    e.position.sub_x = SimFixed::from_num(current.x % 256);
    e.position.sub_y = SimFixed::from_num(current.y % 256);
    e.position.exact_z_leptons = Some(current.z);
    e.on_bridge = false;
    e.in_playfield = input["in_playfield"].as_bool().unwrap_or(false);
    e.lifecycle.object_alive = true;
    e.lifecycle.cell_marked = true;
    e.dock_entered_with = input["tethered"].as_bool().unwrap_or(false).then_some(999);
    e.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(before["mission"].as_i64().unwrap() as i32),
        queued: MissionId::from_raw(before["queued_mission"].as_i64().unwrap() as i32),
        suspended: MissionId::from_raw(before["suspended_mission"].as_i64().unwrap() as i32),
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    e.mission_leaf
        .set_infantry_doing_verified(before["doing"].as_i64().unwrap() as i32)
        .unwrap();
    let loco = e.locomotor.as_mut().unwrap();
    loco.set_walk_destination(Some(coord(&before["destination"])));
    // The corpus deliberately supplies motion with no head, a retained byte
    // independent from IsMoving. Its existing writer may be exercised before
    // nulling the head; no new gameplay setter is needed for this fixture.
    if before["motion"] == 1 {
        loco.set_step_head(Some(current));
        loco.begin_walk_motion();
    }
    loco.set_step_head(None);
    e.foot_speed.set_speed_fraction(SIM_ZERO);
    e.navigation.nav_com = Some(NavTargetRef::Cell { rx: 10, ry: 10 });
    e.navigation.suspended_nav_com = None;
    e.attack_target =
        (input["attack_target"] == true).then_some(crate::sim::combat::AttackTarget {
            target: TargetKind::Cell(11, 10),
        });
    e.suspended_attack_target = None;
    e.navigation.path_replay.directions = vec![2, 3, 4, 5];
    e.navigation.path_replay.cursor = 0;
    e.navigation.path_replay.reference_cell =
        Some(input.get("path_reference").map_or((9, 8), |p| {
            (p[0].as_i64().unwrap() as i16, p[1].as_i64().unwrap() as i16)
        }));
    let p = &mut e.navigation.path_runtime;
    p.movement_timer = CdTimer::from_raw(
        before["movement_timer"][0].as_i64().unwrap() as i32,
        before["movement_timer"][2].as_i64().unwrap() as i32,
    );
    p.blocked_timer = CdTimer::from_raw(
        before["blockage_timer"][0].as_i64().unwrap() as i32,
        before["blockage_timer"][2].as_i64().unwrap() as i32,
    );
    p.path_blocked = before["blocked"] == 1;
    p.retries_left = before["retries"].as_i64().unwrap() as u32;
    p.set_scold_latch_for_test(before["flag68a"].as_u64().unwrap() as u8);
    e.movement_target = Some(MovementTarget::default());
    if input["radio"] == true {
        e.radio_contacts.insert(999);
    }
    sim.substrate.occupancy = OccupancyGrid::new();
    sim.substrate.occupancy.add(
        9,
        10,
        id,
        MovementLayer::Ground,
        Some(2),
        CellListInsertion::PrependNonBuilding,
    );
    sim.substrate.raw_cell_occupation = RawCellOccupationGrid::default();
    sim.substrate.raw_cell_occupation.mark_ground(
        10,
        10,
        input["candidate_raw"].as_u64().unwrap_or(0) as u8,
    );
    if let Some(other) = other {
        let e = sim.substrate.entities.get_mut(other).unwrap();
        e.position.sub_x = SimFixed::from_num(current.x % 256);
        e.position.sub_y = SimFixed::from_num(current.y % 256);
        e.position.exact_z_leptons = Some(current.z);
        e.lifecycle.object_alive = input["obstacle_alive"].as_bool().unwrap_or(true);
        e.lifecycle.cell_marked = true;
        e.on_bridge = input["obstacle_deck"] == true;
        let mut cloak = CloakRuntime::new(0);
        cloak.state = input["cloak"].as_i64().unwrap_or(0) as i32;
        e.cloak = Some(cloak);
        sim.substrate.occupancy.add(
            10,
            10,
            other,
            if e.on_bridge {
                MovementLayer::Bridge
            } else {
                MovementLayer::Ground
            },
            None,
            CellListInsertion::PrependNonBuilding,
        );
    }
    sim.session.binary_frame = input["frame"].as_u64().unwrap_or(100) as u32;
    sim.scenario_rng = SimRng::new(input["seed"].as_u64().unwrap_or(31));
    (sim, rules, registry, id, other)
}

fn native_target(target: Option<TargetKind>, other: Option<u64>) -> u64 {
    match target {
        None => 0,
        Some(TargetKind::Cell(10, 10)) => CELL,
        Some(TargetKind::Cell(11, 10)) => ATTACK_CELL,
        Some(TargetKind::Entity(id)) if Some(id) == other => OTHER,
        target => panic!("unexpected target {target:?}"),
    }
}

fn compare(sim: &Simulation, id: u64, other: Option<u64>, expected: &Value, row: &Value) {
    let e = sim.substrate.entities.get(id).unwrap();
    let loco = e.locomotor.as_ref().unwrap();
    let nav = |n| match n {
        None => 0,
        Some(NavTargetRef::Cell { rx: 10, ry: 10 }) => CELL,
        other => panic!("unexpected NavCom {other:?}"),
    };
    let q = &e.navigation.path_replay;
    let words: Vec<i32> = (0..4)
        .map(|i| {
            q.directions
                .get(usize::from(q.cursor) + i)
                .map_or(-1, |&w| if w == u8::MAX { -1 } else { i32::from(w) })
        })
        .collect();
    let p = &e.navigation.path_runtime;
    for (key, actual) in [
        (
            "doing",
            json!(e.mission_leaf.as_infantry().unwrap().doing()),
        ),
        ("path", json!(words)),
        ("head", xyz(loco.step_head())),
        ("destination", xyz(loco.walk_destination())),
        ("moving", json!(u8::from(loco.walk_is_moving().unwrap()))),
        (
            "motion",
            json!(u8::from(loco.walk_animation_moving().unwrap())),
        ),
        (
            "speed",
            json!(e.foot_speed.applied_fraction().to_num::<f64>()),
        ),
        ("blocked", json!(u8::from(p.path_blocked))),
        ("retries", json!(p.retries_left as i32)),
        ("flag68a", json!(p.scold_latch_raw())),
        ("nav", json!(nav(e.navigation.nav_com))),
        (
            "target",
            json!(native_target(
                e.attack_target.as_ref().map(|a| a.target),
                other
            )),
        ),
        ("suspended_nav", json!(nav(e.navigation.suspended_nav_com))),
        (
            "suspended_target",
            json!(native_target(e.suspended_attack_target, other)),
        ),
        ("mission", json!(e.mission.current().raw())),
        ("queued_mission", json!(e.mission.queued().raw())),
        ("suspended_mission", json!(e.mission.suspended().raw())),
        ("rng", json!(sim.scenario_rng.native_state_hex())),
    ] {
        assert_eq!(actual, expected[key], "{key}: {}", row["input"]);
    }
    for (key, timer) in [
        ("movement_timer", p.movement_timer),
        ("blockage_timer", p.blocked_timer),
    ] {
        assert_eq!(
            [timer.start_frame(), timer.duration()],
            [
                expected[key][0].as_i64().unwrap() as i32,
                expected[key][2].as_i64().unwrap() as i32
            ],
            "{key}: {}",
            row["input"]
        );
    }
    if let Some(expected_dummy) = expected.get("dummy") {
        let dummy = sim.resolved_terrain.as_ref().unwrap().shared_cell_dummy();
        let at = dummy.snapshot().coord;
        assert_eq!(
            json!({"cell": [at.0, at.1], "land": dummy.land_type()}),
            *expected_dummy,
            "retained Dummy: {}",
            row["input"]
        );
    }
    let cloak = other.map(|other| sim.substrate.entities.get(other).unwrap().cloak.unwrap());
    let logical = cloak.map_or([0; 6], |c| {
        [
            c.state,
            c.depth as i32,
            c.step_timer.timer.start_frame(),
            c.step_timer.timer.duration(),
            c.step_timer.speed,
            c.step_delta,
        ]
    });
    let expected_cloak: Vec<i32> = [0, 1, 3, 5, 6, 7]
        .into_iter()
        .map(|i| expected["obstacle_cloak"][i].as_i64().unwrap() as i32)
        .collect();
    assert_eq!(
        logical.as_slice(),
        expected_cloak.as_slice(),
        "cloak: {}",
        row["input"]
    );
}

fn expected_calls(row: &Value, other: Option<u64>) -> Vec<FreshCallRecord> {
    row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| {
            if event.as_str() == Some("0x483480") {
                return Some(FreshCallRecord::UncloakContacts { cell: (10, 10) });
            }
            let event = event.as_array()?;
            Some(match event[0].as_str()? {
                "supplied_find_path" => {
                    let packed = event[1][0].as_u64().unwrap() as u32;
                    assert_eq!(event[1][1], 0);
                    FreshCallRecord::FindPath {
                        cell: ((packed as i16) as i32, ((packed >> 16) as i16) as i32),
                        urgency: event[1][2].as_u64().unwrap() as u8,
                    }
                }
                "supplied_recursive_admission" => {
                    let args = &event[1];
                    assert_eq!(args[0], CELL);
                    assert_eq!(args[3], 0);
                    assert_eq!(args[4], 1);
                    FreshCallRecord::CanEnter {
                        cell: (10, 10),
                        direction: args[1].as_i64().unwrap() as i32,
                        height: args[2].as_i64().unwrap() as i32,
                        code: row["input"]["recursive_code"]
                            .as_u64()
                            .unwrap_or(row["input"]["code"].as_u64().unwrap())
                            as u8,
                    }
                }
                "scatter" => {
                    assert_eq!(event[1], CELL);
                    assert_eq!(event[2][1], 1);
                    assert_eq!(event[2][2], 1);
                    FreshCallRecord::Scatter {
                        cell: (10, 10),
                        no_kidding: true,
                        deck: event[2][3] != 0,
                    }
                }
                "override" => {
                    assert_eq!(event[1][0], 1);
                    assert_eq!(event[1][2], 0);
                    let target = match event[1][1].as_u64().unwrap() {
                        CELL => TargetKind::Cell(10, 10),
                        OTHER => TargetKind::Entity(other.unwrap()),
                        other => panic!("native target {other}"),
                    };
                    FreshCallRecord::Override { target }
                }
                _ => return None,
            })
        })
        .collect()
}

#[test]
fn ordinary_and_recursive_walk_responses_match_original_decoder() {
    let rows = corpus();
    assert_eq!(rows.len(), 124);
    for row in rows {
        let input = &row["input"];
        let (mut sim, rules, registry, id, other) = fixture(&row);
        compare(&sim, id, other, &row["before"], &row);
        if input["zone_rejected"] == true {
            assert!(
                !sim.foot_path_zone_precheck(id, coord(&row["before"]["destination"]), &rules)
                    .unwrap(),
                "native supplied zone refusal: {input}"
            );
        }
        let events = row["events"].as_array().unwrap();
        let codes = events
            .iter()
            .filter(|e| e[0] == "supplied_recursive_admission")
            .map(|_| {
                input["recursive_code"]
                    .as_u64()
                    .unwrap_or(input["code"].as_u64().unwrap()) as u8
            })
            .collect();
        let paths = events
            .iter()
            .filter(|e| e[0] == "supplied_find_path")
            .map(|_| {
                if input["path_found"] != true {
                    SuppliedPath::Failed
                } else if input["continue_recursive"] == true {
                    SuppliedPath::Found(vec![2, 3, 4, 5])
                } else {
                    SuppliedPath::FoundUnchanged
                }
            })
            .collect();
        fresh_oracle_seam::install_with_live_effects(codes, paths);
        let current = crate::sim::movement::ground_pose::position_world_coord(
            &sim.substrate.entities.get(id).unwrap().position,
        );
        let result = sim.replay_walk_admission_response(
            id,
            input["restart"] == true,
            DriveCoord {
                x: current.x + 256,
                ..current
            },
            input["code"].as_u64().unwrap() as u8,
            &rules,
            Some(&registry),
        );
        let result = result.and_then(|retry| {
            if input["continue_recursive"] == true {
                let request = retry.ok_or("missing recursive Walk request")?;
                let found =
                    sim.run_walk_path_request(&request, None, Some(&rules), Some(&registry))?;
                if found {
                    let again = sim.run_walk_admission_request(
                        request.into_walk_admission_for_test(),
                        None,
                        Some(&rules),
                        Some(&registry),
                    )?;
                    assert!(again.is_none(), "recursive refusal recursed twice: {input}");
                }
                Ok(false)
            } else {
                Ok(retry.is_some())
            }
        });
        let (calls, unused) = fresh_oracle_seam::finish();
        let recursive_boundary = result.unwrap_or_else(|e| panic!("{e}: {input}"));
        assert_eq!(
            recursive_boundary,
            !row["recursive_process"].is_null(),
            "retry boundary: {input}"
        );
        assert_eq!(unused, 0, "unused native answers: {input}");
        assert_eq!(calls, expected_calls(&row, other), "call order: {input}");
        compare(&sim, id, other, &row["after"], &row);
    }
}

#[test]
fn exhausted_walk_retry_emits_native_retained_scold_request() {
    // Compose the actual failed-search continuation from the124-row corpus
    // with the native0/1/255 guard controls and valid MenuScold binding. This
    // checks production Walk reaches the sound owner before clearing its byte;
    // sound_dispatch separately checks the centred registered-Voc consumer.
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/foot_scold_latch.json",
    ))
    .unwrap();
    let row = corpus()
        .into_iter()
        .find(|row| {
            let input = &row["input"];
            input["failed_retry_probe"] == true
                && input["retries"] == 0
                && input.get("in_playfield").is_none()
                && input.get("attack_target").is_none()
        })
        .unwrap();
    for guard in native["scold_guard"].as_array().unwrap() {
        let (mut sim, mut rules, registry, id, _) = fixture(&row);
        let sound = native["sound"]["resolved_name"].as_str().unwrap();
        rules.general.scold_sound = Some(sound.into());
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .navigation
            .path_runtime
            .set_scold_latch_for_test(guard["supplied_byte"].as_u64().unwrap() as u8);
        let current = crate::sim::movement::ground_pose::position_world_coord(
            &sim.substrate.entities.get(id).unwrap().position,
        );
        fresh_oracle_seam::install_with_live_effects(vec![], vec![SuppliedPath::Failed]);
        let request = sim
            .replay_walk_admission_response(
                id,
                true,
                DriveCoord {
                    x: current.x + 256,
                    ..current
                },
                7,
                &rules,
                Some(&registry),
            )
            .unwrap()
            .unwrap();
        let result = sim.run_walk_path_request(&request, None, Some(&rules), Some(&registry));
        let (_, unused) = fresh_oracle_seam::finish();
        assert!(!result.unwrap());
        assert_eq!(unused, 0);
        assert_eq!(
            json!(
                sim.substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .navigation
                    .path_runtime
                    .scold_latch_raw()
            ),
            guard["final_byte"]
        );
        let actual: Vec<&str> = sim
            .sound_events
            .iter()
            .filter_map(|event| match event {
                crate::sim::world::SimSoundEvent::VocCentered { sound_id } => {
                    Some(sound_id.as_str())
                }
                _ => None,
            })
            .collect();
        let calls = guard["sound_entry_calls"].as_array().unwrap();
        assert_eq!(actual, vec![sound; calls.len()]);
        for call in calls {
            assert_eq!(
                call["sound_index"],
                native["sound"]["resolved_index_fixture_relative"]
            );
            assert_eq!(call["pan"], 0x2000);
            assert_eq!(call["volume_bits"], "0x3f800000");
            assert_eq!(call["trailing"], 0);
        }
    }
}
