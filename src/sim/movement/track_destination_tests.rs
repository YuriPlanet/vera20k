//! Original MoveTo/Foot and ordinary Unit destination comparisons. The retained
//! class owners also replay the AnyTown MTNK/E1 non-cell AreaGuard destination
//! receipts; additional radio/docking class branches remain required ports.
use super::*;
use crate::sim::components::FootPathQueue;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};
use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};
use crate::sim::timer::CdTimer;
use serde_json::{Value, json};

const ZERO: DriveCoord = DriveCoord { x: 0, y: 0, z: 0 };

fn coord(v: &Value) -> DriveCoord {
    DriveCoord {
        x: v[0].as_i64().unwrap() as i32,
        y: v[1].as_i64().unwrap() as i32,
        z: v[2].as_i64().unwrap() as i32,
    }
}

fn corpus() -> Vec<Value> {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/track_destination.json",
    ))
    .unwrap()
}

/// Original Foot NULL and Aircraft NULL boundary calls with constructed
/// Drive/Ship receivers. This proves the shared outer gate, retained head,
/// timers and RNG continuation; the unusual Aircraft locomotor pairing does
/// not prove Fly Stop or a whole stock aircraft's flight.
#[test]
fn shared_null_destination_matches_original_gate_and_timer_boundaries() {
    use crate::sim::mission::{MissionDispatchTimer, MissionId, state::MissionTestFixture};

    let rows: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/track_destination_null_boundary.json",
    ))
    .unwrap();
    assert_eq!(rows.len(), 84);
    for row in rows {
        let input = &row["input"];
        let mut sim = crate::sim::world::Simulation::with_seed(1);
        sim.session.binary_frame = input["frame"].as_u64().unwrap() as u32;
        let mut rules = crate::rules::ruleset::RuleSet::from_ini(
            &crate::rules::ini_parser::IniFile::from_str(""),
        )
        .unwrap();
        rules.general.blockage_path_delay_ticks = input["blockage"].as_i64().unwrap() as i32;
        let mut e = actor(input);
        let aircraft = input["entry"] == "aircraft";
        if aircraft {
            e.category = crate::map::entities::EntityCategory::Aircraft;
        }
        let current = input["mission"].as_i64().unwrap() as i32;
        e.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(current),
            queued: MissionId::from_raw(input["queued"].as_i64().unwrap() as i32),
            suspended: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        e.attack_target =
            (input["target"] == true).then(|| crate::sim::combat::AttackTarget::for_cell(11, 10));
        e.navigation.nav_com = (input["nav"] != false).then(|| NavTargetRef::cell(11, 10));
        e.foot_locomotor_swap_active = input["swap"] == true;
        if input["bunker"] == true {
            e.bunker_link = crate::sim::game_entity::BunkerLink::Installed(2);
        }
        if input["open_transport"] == true {
            e.passenger_role = crate::sim::passenger::PassengerRole::Inside {
                transport_id: 2,
                open_topped: true,
            };
        }
        sim.substrate.entities.insert(e);
        if aircraft {
            sim.assign_null_destination(
                1,
                Some(&rules),
                None,
                crate::sim::world::FrameEffects::default(),
            );
        } else {
            sim.foot_null_destination(
                1,
                Some(&rules),
                None,
                crate::sim::world::FrameEffects::default(),
            );
        }
        let e = sim.substrate.entities.get(1).unwrap();
        let (destination, head) = if input["family"] == "drive" {
            let runtime = e
                .locomotor
                .as_ref()
                .and_then(|l| l.selected_drive_runtime())
                .and_then(|r| r.retained())
                .unwrap();
            (runtime.destination(), runtime.head_to())
        } else {
            let runtime = e
                .locomotor
                .as_ref()
                .and_then(|l| l.selected_ship_runtime())
                .and_then(|r| r.retained())
                .unwrap();
            (runtime.destination(), runtime.head_to())
        };
        assert_eq!(
            destination.unwrap_or(ZERO),
            coord(&row["destination"]),
            "{input}"
        );
        assert_eq!(head.unwrap_or(ZERO), coord(&row["head"]), "{input}");
        let path = &e.navigation.path_runtime;
        for (key, actual) in [
            ("nav", json!(e.navigation.nav_com.is_some())),
            ("aux", json!(e.navigation.nav_com_aux.is_some())),
            ("path", json!(e.navigation.path_replay.directions)),
            ("blocked", json!(path.path_blocked)),
            ("retries", json!(path.retries_left)),
            (
                "movement_timer",
                json!([
                    path.movement_timer.start_frame() as u32,
                    path.movement_timer.duration()
                ]),
            ),
            (
                "blocked_timer",
                json!([
                    path.blocked_timer.start_frame() as u32,
                    path.blocked_timer.duration()
                ]),
            ),
        ] {
            assert_eq!(actual, row[key], "{key}: {input}");
        }
        assert_eq!(
            json!(sim.scenario_rng.next_u32()),
            row["next_random"],
            "{input}"
        );
    }
}

fn terrain(bridge: bool) -> ResolvedTerrainGrid {
    ResolvedTerrainGrid::from_cells(
        32,
        32,
        (0..32)
            .flat_map(|y| {
                (0..32).map(move |x| {
                    crate::sim::world::common_raw_test_terrain_cell(
                        x,
                        y,
                        0,
                        bridge && (x, y) == (11, 10),
                    )
                })
            })
            .collect(),
    )
}

fn actor(input: &Value) -> GameEntity {
    let mut e = GameEntity::test_default(1, "MOVER", "Americans", 10, 10);
    e.category = crate::map::entities::EntityCategory::Unit;
    e.position.sub_x = SimFixed::from_num(128);
    e.position.sub_y = SimFixed::from_num(128);
    let kind = if input["family"] == "drive" {
        LocomotorKind::Drive
    } else {
        LocomotorKind::Ship
    };
    e.locomotor = Some(LocomotorState::for_test_kind(kind));
    e.locomotor.as_mut().unwrap().powered = input["power_off"] != true;
    let head = input.get("head").map_or(
        DriveCoord {
            x: 2816,
            y: 2688,
            z: 123,
        },
        coord,
    );
    let head = (head != ZERO).then_some(head);
    let destination = Some(DriveCoord {
        x: 700,
        y: 800,
        z: 900,
    });
    if kind == LocomotorKind::Drive {
        assert!(
            e.locomotor
                .as_mut()
                .unwrap()
                .install_drive_state_for_test(Some(
                    DriveLocomotionRuntime::default()
                        .with_destination_for_test(destination)
                        .with_head_to_for_test(head)
                ))
        );
    } else {
        assert!(
            e.locomotor
                .as_mut()
                .unwrap()
                .install_ship_state_for_test(Some(
                    ShipLocomotionRuntime::default()
                        .with_destination_for_test(destination)
                        .with_head_to_for_test(head)
                ))
        );
    }
    e.navigation.nav_com_aux = Some(NavTargetRef::cell(0, 0));
    e.navigation.nav_com = (input["same_nav"] == true).then(|| NavTargetRef::cell(11, 10));
    e.setter_force_reassign = input["force_reassign"] == true;
    e.navigation.nav_queue =
        vec![NavTargetRef::cell(11, 10); input["nav_queue"].as_u64().unwrap_or(0) as usize];
    e.navigation.path_replay = FootPathQueue {
        directions: vec![2, 3, 4, 5],
        reference_cell: Some((9, 8)),
        ..Default::default()
    };
    let path_runtime = &mut e.navigation.path_runtime;
    path_runtime.movement_timer = CdTimer::started(50, 5);
    path_runtime.blocked_timer = CdTimer::started(40, 6);
    path_runtime.path_blocked = true;
    path_runtime.retries_left = 7;
    // `+0x270` through a Temporal chain's head (any id); `+0x271` through
    // the teleport's warp-in.
    if input["warp_out"] == true {
        e.temporal = crate::sim::temporal::TemporalState::warped_by_for_test(99);
    }
    if input["warp_in"] == true {
        e.install_teleport_state_for_test(Some(TeleportState::for_test(
            TeleportPhase::ChronoDelay,
            11,
            10,
            3,
        )));
    }
    e
}

fn destination_fixture(
    input: &Value,
) -> (
    crate::sim::world::Simulation,
    crate::rules::ruleset::RuleSet,
) {
    let mut entity = actor(input);
    entity.lifecycle.in_limbo = false;
    let mut sim = crate::sim::world::Simulation::new();
    sim.interner = crate::sim::intern::test_interner();
    sim.session.binary_frame = 100;
    sim.install_resolved_terrain_for_new_map(terrain(input["bridge"] == true));
    sim.install_fixture_path_grid(Some(&crate::sim::pathfinding::PathGrid::new(32, 32)));
    sim.substrate.entities.insert(entity);
    // The concrete dispatch preflight needs the supplied actor's type
    // identity in the registry. Native boundary fields remain the fixture
    // inputs above; this minimal INI is not a native reader comparison.
    let mut rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::rules::ini_parser::IniFile::from_str("[VehicleTypes]\n0=MOVER\n[MOVER]\n"),
    )
    .unwrap();
    rules.general.blockage_path_delay_ticks = 22;
    (sim, rules)
}

fn compare(e: &GameEntity, row: &Value) {
    if let Some(adapter) = &e.movement_target {
        assert_eq!(
            adapter.final_goal, None,
            "track goal belongs to the locomotor"
        );
    }
    let (destination, head) = if row["input"]["family"] == "drive" {
        let d = e
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap();
        (d.destination(), d.head_to())
    } else {
        let d = e
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_ship_runtime())
            .and_then(|r| r.retained())
            .unwrap();
        (d.destination(), d.head_to())
    };
    assert_eq!(
        destination,
        (coord(&row["destination"]) != ZERO).then(|| coord(&row["destination"])),
        "{row}"
    );
    assert_eq!(head.unwrap_or(ZERO), coord(&row["head"]), "{row}");
    assert_eq!(
        u8::from(e.locomotor.as_ref().unwrap().powered),
        row["power"].as_u64().unwrap() as u8,
        "{row}"
    );
    if row["input"]["family"] == "drive" {
        assert_eq!(
            super::super::track_head::motion_state(
                e,
                super::super::track_process::TrackFamily::Drive
            )
            .0,
            row["moving"].as_bool().unwrap(),
            "{row}"
        );
    }
    let p = &e.navigation.path_runtime;
    let nav = e.navigation.nav_com.map(|n| match n {
        NavTargetRef::Cell { rx, ry } => [rx, ry],
        _ => panic!("cell fixture"),
    });
    for (key, actual) in [
        ("nav", json!(nav)),
        ("aux", json!(e.navigation.nav_com_aux.is_some())),
        (
            "path",
            json!(
                e.navigation
                    .path_replay
                    .directions
                    .iter()
                    .map(|&v| { if v == u8::MAX { -1 } else { i32::from(v) } })
                    .collect::<Vec<_>>()
            ),
        ),
        ("reference", json!(e.navigation.path_replay.reference_cell)),
        (
            "movement_timer",
            json!([p.movement_timer.start_frame(), p.movement_timer.duration()]),
        ),
        (
            "blocked_timer",
            json!([p.blocked_timer.start_frame(), p.blocked_timer.duration()]),
        ),
        ("blocked", json!(u8::from(p.path_blocked))),
        ("retries", json!(p.retries_left)),
        ("force_reassign", json!(u8::from(e.setter_force_reassign))),
    ] {
        assert_eq!(actual, row[key], "{key}: {row}");
    }
    if let Some(expected) = row.get("nav_queue") {
        assert_eq!(json!(e.navigation.nav_queue.len()), *expected, "{row}");
    }
}

/// Event4C746F pushes clear_queue=1 before the destination token is resolved
/// at4C7474 and dispatched through virtual+480 at4C747C. Exercise that
/// ordinary Command::Move boundary against the original Unit741970 rows,
/// including its same-NavCom return and force override. Mission/target event
/// writes precede this boundary and are not claims of this destination corpus.
/// The synthetic Foot+6AC skip row has no standalone stored latch in Rust;
/// the represented Teleporter owner produces and consumes it in one call.
#[test]
fn command_move_destinations_match_native_unit_setter() {
    let mut checked = 0;
    for row in corpus() {
        let input = &row["input"];
        if input["entry"] != "unit"
            || input["null"] == true
            || input["flag"] == 0
            || input["skip_move"] == true
        {
            continue;
        }
        let (mut sim, rules) = destination_fixture(input);
        assert!(sim.apply_command(
            "Americans",
            &crate::sim::command::Command::Move {
                entity_id: 1,
                target_rx: 11,
                target_ry: 10,
                queue: false,
            },
            Some(&rules),
        ));
        let entity = sim.substrate.entities.get(1).unwrap();
        compare(entity, &row);
        let restored: GameEntity =
            serde_json::from_value(serde_json::to_value(entity).unwrap()).unwrap();
        compare(&restored, &row);
        checked += 1;
    }
    assert_eq!(checked, 30);
}

/// Original rank setters750090/7500B0 -> Unit741970(cell,1) -> the live
/// Drive4B0F20/Ship6A05F0 speed prefix, from track_speed_native.order_histories.
/// A repeated destination preserves the old VERA order-speed stamp; the
/// production prefix reads the promoted rank anyway. The forced-repeat
/// control reaches the Foot tail again. No paid points or promotion AI are
/// claimed by this comparison: the native and Rust prefixes stop at budget.
#[test]
fn repeated_move_orders_keep_the_stamp_but_sample_live_native_speed() {
    use crate::sim::combat::veterancy;
    use crate::sim::movement::track_process::TrackFamily;
    use crate::util::fixed_math::SIM_ONE;

    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/track_speed_native.json",
    ))
    .unwrap();
    let mut checked = 0;
    for history in corpus["order_histories"].as_array().unwrap() {
        let input = &history["input"];
        let (mut sim, _) = destination_fixture(input);
        let mut rules = crate::rules::ruleset::RuleSet::from_ini(
            &crate::rules::ini_parser::IniFile::from_str(&format!(
                "[General]\nVeteranSpeed=1.2\n\
                 [VehicleTypes]\n0=MOVER\n[MOVER]\nSpeed={}\n\
                 VeteranAbilities={}\n",
                input["ini_speed"].as_i64().unwrap(),
                if input["faster"] == true {
                    "FASTER"
                } else {
                    ""
                },
            )),
        )
        .unwrap();
        rules.general.blockage_path_delay_ticks = 22;
        assert_eq!(
            rules.general.veteran_speed,
            input["veteran"].as_f64().unwrap()
        );
        let family = if input["family"] == "drive" {
            TrackFamily::Drive
        } else {
            TrackFamily::Ship
        };
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.drive_accelerates = false;
        entity.foot_speed.set_speed_fraction(SIM_ONE);
        let loco = entity.locomotor.as_mut().unwrap();
        let mut progress = loco.track_progress(family).unwrap();
        progress.turn_index = input["selector"].as_i64().unwrap() as i32;
        progress.residual = input["residual"].as_i64().unwrap() as i32;
        assert_eq!(
            progress.residual, 0,
            "native prefix budget equals its speed"
        );
        loco.store_track_progress(family, progress);
        loco.store_track_valid(family, true);
        loco.store_track_target_fraction(family, SIM_ONE);
        let initial_stamp = SimFixed::from_num(input["raw"].as_i64().unwrap() * 15);
        let rng_before = [
            sim.main_rng.state(),
            sim.scenario_rng.state(),
            sim.mapgen_rng.state(),
        ];

        for step in history["steps"].as_array().unwrap() {
            let entity = sim.substrate.entities.get_mut(1).unwrap();
            match step["rank"].as_u64().unwrap() {
                0 => {}
                1 => veterancy::set_veteran(entity),
                2 => veterancy::set_elite(entity),
                rank => panic!("unexpected native rank {rank}"),
            }
            entity.setter_force_reassign = step["forced"] == true;
            let retained_before = serde_json::to_value((
                &entity.navigation,
                &entity.locomotor,
                &entity.movement_target,
            ))
            .unwrap();
            assert!(sim.apply_command(
                "Americans",
                &crate::sim::command::Command::Move {
                    entity_id: 1,
                    target_rx: 11,
                    target_ry: 10,
                    queue: false,
                },
                Some(&rules),
            ));
            let entity = sim.substrate.entities.get(1).unwrap();
            if step["setter_objects_unchanged"] == true {
                assert_eq!(
                    serde_json::to_value((
                        &entity.navigation,
                        &entity.locomotor,
                        &entity.movement_target,
                    ))
                    .unwrap(),
                    retained_before,
                    "the native same-NavCom setter returns before any write: {step}",
                );
            }
            let loco = entity.locomotor.as_ref().unwrap();
            let destination = loco.track_destination(family).unwrap_or(ZERO);
            let head = loco.track_head(family).unwrap_or(ZERO);
            let nav = entity.navigation.nav_com.map(|nav| match nav {
                NavTargetRef::Cell { rx, ry } => [rx, ry],
                _ => panic!("cell fixture"),
            });
            let path = &entity.navigation.path_runtime;
            assert_eq!(
                json!({
                    "rank_bits": format!("{:08x}", entity.veterancy_raw.bits()),
                    "destination": [destination.x, destination.y, destination.z],
                    "head": [head.x, head.y, head.z],
                    "nav": nav,
                    "nav_queue": entity.navigation.nav_queue.len(),
                    "force_reassign": u8::from(entity.setter_force_reassign),
                    "movement_timer": [
                        path.movement_timer.start_frame(), path.movement_timer.duration(),
                    ],
                    "blocked_timer": [
                        path.blocked_timer.start_frame(), path.blocked_timer.duration(),
                    ],
                }),
                step["setter_state"],
                "{input}: {step}",
            );
            let native_speed = step["current_speed"].as_i64().unwrap() as i32;
            assert_eq!(
                sim.current_speed_for_test(1, &rules),
                native_speed,
                "{step}"
            );
            assert_eq!(
                entity.movement_target.as_ref().unwrap().speed,
                if step["forced"] == true {
                    SimFixed::from_num(native_speed * 15)
                } else {
                    initial_stamp
                },
                "only an accepted new/forced destination refreshes the adapter: {step}",
            );
            let speed = super::super::track_speed::advance(
                sim.substrate.entities.get_mut(1).unwrap(),
                rules.object("MOVER"),
                Some(&rules),
                &sim.houses,
                sim.resolved_terrain.as_ref(),
                None,
            );
            assert_eq!(
                speed,
                step["prefix_budget"].as_i64().unwrap() as i32,
                "{step}"
            );
            assert_eq!(step["prefix_events"], json!(["get_current_speed"]));
            assert!(step["rng_calls"].as_array().unwrap().is_empty());
            assert_eq!(
                [
                    sim.main_rng.state(),
                    sim.scenario_rng.state(),
                    sim.mapgen_rng.state(),
                ],
                rng_before,
                "{step}",
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 16);
}

/// These native Unit741970 calls feed the actual Foot4DC060 draw in the same
/// VM. In particular, an unchanged NavCom retains a populated waypoint queue;
/// a forced repeat clears it. Compare the production command's drawing inputs
/// directly with that composed boundary, including the retained moving head.
#[test]
fn command_move_matches_native_action_line_destination_inputs() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/action_lines.json",
    ))
    .unwrap();
    let mut checked = 0;
    for row in native["destination_cases"].as_array().unwrap() {
        let input = &row["input"];
        if input["null"] == true || input["flag"] == 0 || input["skip_move"] == true {
            continue;
        }
        let (mut sim, rules) = destination_fixture(input);
        let rng_before = [
            sim.main_rng.state(),
            sim.scenario_rng.state(),
            sim.mapgen_rng.state(),
        ];
        assert!(sim.apply_command(
            "Americans",
            &crate::sim::command::Command::Move {
                entity_id: 1,
                target_rx: 11,
                target_ry: 10,
                queue: false,
            },
            Some(&rules),
        ));
        let entity = sim.substrate.entities.get(1).unwrap();
        let drive = entity
            .locomotor
            .as_ref()
            .and_then(|loco| loco.selected_drive_runtime())
            .and_then(|runtime| runtime.retained())
            .unwrap();
        let source = super::super::ground_pose::position_world_coord(&entity.position);
        let destination = drive.destination().unwrap_or(ZERO);
        let head = drive.head_to().unwrap_or(ZERO);
        let nav = entity.navigation.nav_com.map(|nav| match nav {
            NavTargetRef::Cell { rx, ry } => [rx, ry],
            _ => panic!("cell fixture"),
        });
        let path = &entity.navigation.path_runtime;
        assert_eq!(
            json!({
                "source": [source.x, source.y, source.z],
                "destination": [destination.x, destination.y, destination.z],
                "head": [head.x, head.y, head.z],
                "nav": nav,
                "nav_queue": entity.navigation.nav_queue.len(),
                "movement_timer": [
                    path.movement_timer.start_frame(),
                    path.movement_timer.duration(),
                ],
                "blocked_timer": [
                    path.blocked_timer.start_frame(),
                    path.blocked_timer.duration(),
                ],
            }),
            row["setter_result"],
            "{input}",
        );
        assert!(row["rng_entries"].as_array().unwrap().is_empty());
        assert_eq!(
            [
                sim.main_rng.state(),
                sim.scenario_rng.state(),
                sim.mapgen_rng.state(),
            ],
            rng_before,
            "{input}",
        );
        checked += 1;
    }
    assert_eq!(checked, 7);
}

/// Unit741A80..741A9C returns before any write for the same NavCom without
/// Techno+1F8. Replay the original supplied-prior Drive row through the
/// production ground-order boundary, which must reach that class guard rather
/// than composing the accepted Foot/Move_To suffix again. This is a setter
/// boundary comparison, not a stock MTNK input or whole movement history.
#[test]
fn ground_orders_preserve_the_native_unit_same_destination_noop() {
    use crate::sim::world::{GroundMove, Simulation};

    // The native guard returns before type tuning is read. The registry only
    // supplies the represented Unit identity required by class preflight.
    let rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::rules::ini_parser::IniFile::from_str("[VehicleTypes]\n0=MOVER\n[MOVER]\n"),
    )
    .unwrap();
    let mut checked = 0;
    for row in corpus() {
        let input = &row["input"];
        if input["entry"] != "unit"
            || input["family"] != "drive"
            || input["same_nav"] != true
            || input["force_reassign"] == true
            || input["null"] == true
        {
            continue;
        }
        checked += 1;
        for published_grid in [true, false] {
            let mut sim = Simulation::new();
            sim.session.binary_frame = 100;
            let mut e = actor(input);
            e.navigation.nav_com = Some(NavTargetRef::cell(11, 10));
            sim.interner = crate::sim::intern::test_interner();
            sim.substrate.entities.insert(e);
            sim.install_resolved_terrain_for_new_map(terrain(false));
            if published_grid {
                sim.install_fixture_path_grid(Some(&crate::sim::pathfinding::PathGrid::new(
                    32, 32,
                )));
            }
            let rng = sim.rng_state();
            assert!(sim.issue_ground_move(
                GroundMove {
                    entity_id: 1,
                    target: (11, 10),
                    speed: SimFixed::from_num(768),
                    queue: false,
                    speed_type: None,
                    owner_blocks: true,
                    object_destination: None,
                },
                Some(&rules),
                None,
                crate::sim::world::FrameEffects::default(),
            ));
            compare(sim.substrate.entities.get(1).unwrap(), &row);
            assert_eq!(sim.rng_state(), rng);
        }
    }
    assert_eq!(checked, 1);
}

#[test]
fn ordinary_track_orders_match_native_without_an_eager_path_or_power_change() {
    let mut checked = 0;
    for row in corpus() {
        let input = &row["input"];
        if input["entry"] != "unit"
            || input.get("same_nav").is_some()
            || input.get("skip_move").is_some()
        {
            continue;
        }
        checked += 1;
        let terrain = terrain(input["bridge"] == true);
        for blocked in [false, true] {
            let mut entities = EntityStore::new();
            entities.insert(actor(input));
            let mut grid = crate::sim::pathfinding::PathGrid::new(32, 32);
            if blocked {
                // A wall surrounding the requested cell is irrelevant until
                // Process; its accepted NavCom must not be redirected/refused.
                for y in 0..32 {
                    grid.set_blocked(11, y, true);
                }
            }
            assert!(
                super::super::movement_commands::issue_move_command_with_layered(
                    &mut entities,
                    &grid,
                    1,
                    (11, 10),
                    SimFixed::from_num(768),
                    false,
                    None,
                    None,
                    Some(&terrain),
                    None,
                    None,
                    None,
                    None,
                    None,
                    super::super::DestinationTiming::new(100, 22),
                )
            );
            let entity = entities.get(1).unwrap();
            compare(entity, &row);
            let restored: GameEntity =
                serde_json::from_value(serde_json::to_value(entity).unwrap()).unwrap();
            compare(&restored, &row);
            let request = entity.movement_target.as_ref().unwrap();
            assert_eq!(request.final_goal, None);
            assert_eq!(
                crate::sim::movement::movement_goal_cell(entity),
                Some((11, 10))
            );
            assert!(
                entity
                    .navigation
                    .path_replay
                    .remaining_directions()
                    .is_empty()
            );
        }
    }
    assert_eq!(checked, 24);
}

#[test]
fn track_move_to_and_accepted_foot_calls_match_original_warp_and_zero_semantics() {
    let rows = corpus();
    assert_eq!(rows.len(), 132);
    let mut counts = [0, 0, 0];
    for row in rows {
        let input = &row["input"];
        // These36 original full Unit calls preserve the still-required class
        // queue/force/one-shot latch evidence; no Rust whole-Unit claim here.
        if input["entry"] == "unit" {
            counts[2] += 1;
            continue;
        }
        let mut e = actor(input);
        let terrain = terrain(input["bridge"] == true);
        if input["entry"] == "move" {
            counts[0] += 1;
            if input["family"] == "drive" {
                drive_set_destination(&mut e, coord(&input["request"]), Some(&terrain));
            } else {
                ship_set_destination(&mut e, coord(&input["request"]), Some(&terrain));
            }
        } else {
            counts[1] += 1;
            set_destination_internal_cell(&mut e, (11, 10), Some(&terrain), 0);
            super::super::DestinationTiming::new(100, 22).accept(&mut e);
        }
        compare(&e, &row);
        let restored: GameEntity =
            serde_json::from_value(serde_json::to_value(&e).unwrap()).unwrap();
        compare(&restored, &row);
    }
    assert_eq!(counts, [72, 24, 36]);
}

/// The Unit Cell setter clears NavQueue behind its flag (0x7422E8..0x7422F4)
/// and the NULL setter at 0x7423BE; the NULL rows start with a NavCom, since
/// 0x741A80 returns before any write without one. Ordinary Move passes1;
/// shared class callers such as Foot Approach pass0 to retain the queue.
#[test]
fn unit_setters_clear_navqueue_like_the_original() {
    let mut checked = 0;
    for row in corpus() {
        let input = &row["input"];
        if input.get("nav_queue").is_none() {
            continue;
        }
        let (mut sim, rules) = destination_fixture(input);
        if input["null"] == true {
            assert!(sim.set_unit_null_destination(
                1,
                Some(&rules),
                None,
                crate::sim::world::FrameEffects::default()
            ));
        } else {
            assert!(sim.set_unit_destination(
                1,
                NavTargetRef::cell(11, 10),
                &rules,
                input["flag"] != 0,
                crate::sim::world::FrameEffects::default(),
            ));
        }
        compare(sim.substrate.entities.get(1).unwrap(), &row);
        checked += 1;
    }
    assert_eq!(checked, 6);
}

#[test]
fn refused_track_request_does_not_allocate_payload_or_stamp_dummy() {
    for family in ["drive", "ship"] {
        let mut e = actor(&json!({"family":family,"warp_out":true}));
        if let Some(loco) = e.locomotor.as_mut() {
            let _ = loco.install_drive_state_for_test(None);
        };
        if let Some(loco) = e.locomotor.as_mut() {
            let _ = loco.install_ship_state_for_test(None);
        };
        let terrain = ResolvedTerrainGrid::from_cells(1, 1, Vec::new());
        terrain.stamp_dummy_cell_requested_coord(7, 8);
        let before = terrain.shared_cell_dummy().snapshot();
        let coord = DriveCoord {
            x: -4096,
            y: 800000,
            z: 17,
        };
        if family == "drive" {
            assert!(!drive_set_destination(&mut e, coord, Some(&terrain)));
        } else {
            ship_set_destination(&mut e, coord, Some(&terrain));
        }
        assert!(
            e.locomotor
                .as_ref()
                .and_then(|l| l.selected_drive_runtime())
                .and_then(|r| r.retained())
                .is_none()
                && e.locomotor
                    .as_ref()
                    .and_then(|l| l.selected_ship_runtime())
                    .and_then(|r| r.retained())
                    .is_none()
        );
        assert_eq!(terrain.shared_cell_dummy().snapshot(), before);
    }
}

#[test]
fn live_drive_target_refresh_resumes_after_owner_warp_ends() {
    let mut e = actor(&json!({"family":"drive","warp_in":true}));
    let before = e
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_drive_runtime())
        .and_then(|r| r.retained())
        .cloned();
    let terrain = terrain(false);
    let destination = DriveCoord {
        x: 2944,
        y: 2688,
        z: -123,
    };
    assert!(!refresh_drive_destination_coord(
        &mut e,
        destination,
        Some(&terrain)
    ));
    assert_eq!(
        e.locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .cloned(),
        before
    );
    // Existing teleport owner clears the arrival byte when its timer expires.
    e.teleport_state_for_test_mut()
        .unwrap()
        .set_ticks_for_test(0);
    assert!(refresh_drive_destination_coord(
        &mut e,
        destination,
        Some(&terrain)
    ));
    assert_eq!(
        e.locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .destination(),
        Some(destination)
    );
    assert_eq!(
        e.locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .head_to(),
        before.unwrap().head_to()
    );
}

// This supplies the retail cell and type dependencies used by the two original
// destination bodies, not a whole native Scenario_Load or movement comparison.
// The recorded pose, path/vector and timers are explicit native caller controls.
#[cfg(test)]
fn anytown_destination_dependencies() -> Option<(
    crate::sim::world::Simulation,
    crate::rules::ruleset::RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
)> {
    use crate::rules::retail_ini_fixture::{retail_assets, retail_rules_owner};

    let (root, mut assets) = retail_assets()?;
    let mut process = retail_rules_owner(&assets);
    assets
        .register_neutral_archives()
        .expect("register retail neutral archives");
    crate::map::scenario_sources::list_skirmish_scenario_records_with_assets(
        &root,
        &mut assets,
        None,
    )
    .expect("register retail scenario archives");
    let modes = crate::skirmish_modes::skirmish_modes_from_assets(&assets)
        .expect("read retail mode roster");
    let mode = crate::skirmish_modes::mode_by_id(&modes, 1).expect("stock Battle mode");
    let map =
        crate::map::source::load_map_by_name_or_path_with_assets(&root, "XMP03T4.MAP", &assets)
            .expect("stock AnyTown map");
    let theater = crate::map::theater::load_theater(&mut assets, &map.map.header.theater)
        .expect("AnyTown theater");
    let mode = crate::rules::retail_sources::select_ini(&assets, &mode.override_file)
        .expect("stock Battle override");
    let (mut rules, processed, art, _) = process
        .load_scenario(crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(Some(&mode.ini)), &map.map.ini)
        .expect("AnyTown/Battle production Rules layers")
        .into_parts();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    let overlays =
        crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&processed, Some(&art));
    let terrain = ResolvedTerrainGrid::build(
        &map.map,
        Some(&theater),
        Some(&assets),
        Some(&rules.terrain_rules),
        Some(&overlays),
        true,
        rules.general.cliff_back_impassability,
    );
    let mut sim = crate::sim::world::Simulation::new();
    sim.install_playfield_from_map_header(&map.map.header);
    sim.install_resolved_terrain_for_new_map(terrain);
    Some((sim, rules, overlays))
}

fn destination_receipt_actor(
    sim: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    id: u64,
    family: &str,
    before: &Value,
) -> GameEntity {
    use crate::map::entities::EntityCategory;
    use crate::sim::components::Health;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId};

    let xyz = coord(&before["position"]);
    let object = rules.object(family).unwrap();
    let (rx, ry) = ((xyz.x / 256) as u16, (xyz.y / 256) as u16);
    let level = sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .cell(rx, ry)
        .unwrap()
        .level;
    let owner = sim.interner.intern("Americans");
    let mut actor = GameEntity::new_at_frame_zero_for_test(
        id,
        rx,
        ry,
        level,
        0,
        owner,
        Health {
            current: object.strength,
        },
        sim.interner.intern(family),
        if family == "E1" {
            EntityCategory::Infantry
        } else {
            EntityCategory::Unit
        },
        0,
        0,
        family != "E1",
    );
    actor.lifecycle.in_limbo = false;
    actor.position.sub_x = SimFixed::from_num(xyz.x % 256);
    actor.position.sub_y = SimFixed::from_num(xyz.y % 256);
    actor.position.exact_z_leptons = Some(xyz.z);
    actor.on_bridge = before["on_bridge"].as_u64().unwrap() != 0;
    actor.locomotor = Some(LocomotorState::from_object_type(
        object,
        sim.session.binary_frame,
    ));
    let destination = coord(&before["locomotor"]["destination"]);
    let head = coord(&before["locomotor"]["head"]);
    if family == "MTNK" {
        assert!(
            actor
                .locomotor
                .as_mut()
                .unwrap()
                .install_drive_state_for_test(Some(
                    DriveLocomotionRuntime::default()
                        .with_destination_for_test((destination != ZERO).then_some(destination))
                        .with_head_to_for_test((head != ZERO).then_some(head))
                ))
        );
    } else {
        actor
            .locomotor
            .as_mut()
            .unwrap()
            .set_step_head((head != ZERO).then_some(head));
        actor
            .locomotor
            .as_mut()
            .unwrap()
            .set_walk_destination((destination != ZERO).then_some(destination));
        actor
            .mission_leaf
            .set_infantry_doing_verified(before["doing"].as_i64().unwrap() as i32)
            .unwrap();
    }
    actor.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(before["mission"].as_i64().unwrap() as i32),
        suspended: MissionId::NONE,
        queued: MissionId::from_raw(before["queued"].as_i64().unwrap() as i32),
        movement_bypass_latch: 0,
        handler_state: before["status"].as_u64().unwrap() as u32,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    actor
}

/// Original Unit741970/Infantry51AA40 -> Foot4D94B0 -> target+4C ->
/// Drive4AFD40/Walk75ACB0, reached through original AreaGuard Foot-stray.
/// Compare only the class call boundary: AreaGuard's later Scenario(1,5)
/// cadence draw and Infantry's earlier threat-timer writes have their owners.
#[cfg(test)]
#[test]
fn noncell_foot_destinations_match_original_anytown_class_calls() {
    use crate::sim::rng::SimRng;

    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/anytown_damage/foot_missions.json",
    ))
    .unwrap();
    let rows = native["navigation_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        let Some((mut sim, rules, overlays)) = anytown_destination_dependencies() else {
            return;
        };
        let input = &row["input"];
        let family = input["family"].as_str().unwrap();
        let readback = &row["destination_input_readback"];
        sim.session.binary_frame = readback["frame"].as_u64().unwrap() as u32;
        assert_eq!(
            rules.general.blockage_path_delay_ticks,
            readback["rules_repath_delay"].as_i64().unwrap() as i32
        );
        let before = &row["before"];
        let mut actor = destination_receipt_actor(&mut sim, &rules, 1, family, before);
        // The vector's two native receivers are the archive Foot and Cell87,53.
        // Neither element is dereferenced in these clear/retain class arms.
        actor.navigation.nav_queue =
            vec![NavTargetRef::Object { id: 2 }, NavTargetRef::cell(87, 53)];
        actor.navigation.nav_com_aux = Some(NavTargetRef::Object { id: 3 });
        actor.navigation.path_replay = FootPathQueue {
            directions: before["path"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as u8)
                .collect(),
            reference_cell: Some((
                before["reference_cell"][0].as_u64().unwrap() as i16,
                before["reference_cell"][1].as_u64().unwrap() as i16,
            )),
            ..Default::default()
        };
        let runtime = &mut actor.navigation.path_runtime;
        runtime.movement_timer = CdTimer::from_raw(
            before["movement_timer_words"][0].as_i64().unwrap() as i32,
            before["movement_timer_words"][2].as_i64().unwrap() as i32,
        );
        runtime.blocked_timer = CdTimer::from_raw(
            before["blocked_timer_words"][0].as_i64().unwrap() as i32,
            before["blocked_timer_words"][2].as_i64().unwrap() as i32,
        );
        runtime.path_blocked = before["blocked"].as_u64().unwrap() != 0;
        runtime.retries_left = before["retry"].as_u64().unwrap() as u32;
        let mut archive = readback["archive_object"].clone();
        archive["locomotor"] = readback["archive_locomotor"].clone();
        let archive = destination_receipt_actor(&mut sim, &rules, 2, "MTNK", &archive);
        sim.substrate.entities.insert(archive);
        let queue = actor.navigation.nav_queue.clone();
        assert_eq!(
            actor.navigation.nav_queue.len(),
            before["nav_queue"]["count"].as_u64().unwrap() as usize
        );
        sim.substrate.entities.insert(actor);
        let events = row["events"].as_array().unwrap();
        let class = events
            .iter()
            .find(|event| {
                event["kind"]
                    == if family == "MTNK" {
                        "unit_destination"
                    } else {
                        "infantry_destination"
                    }
            })
            .unwrap();
        let entry = &class["rng_at_entry"];
        sim.main_rng = serde_json::from_value::<SimRng>(entry["main"].clone())
            .unwrap()
            .into();
        sim.scenario_rng = serde_json::from_value::<SimRng>(entry["scenario"].clone()).unwrap();
        sim.mapgen_rng = serde_json::from_value::<SimRng>(entry["mapgen"].clone()).unwrap();
        let requested = NavTargetRef::Object { id: 2 };
        if family == "E1" {
            assert!(sim.infantry_destination_inputs_available(
                1,
                requested,
                &rules,
                Some(&overlays)
            ));
        }
        let accepted = if family == "MTNK" {
            sim.set_unit_destination(
                1,
                requested,
                &rules,
                true,
                crate::sim::world::FrameEffects::default(),
            )
        } else {
            sim.set_infantry_destination(
                1,
                requested,
                &rules,
                Some(&overlays),
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap()
        };
        assert!(accepted, "{input}");
        for (stream, rng) in [
            ("main", serde_json::to_value(&sim.main_rng).unwrap()),
            ("scenario", serde_json::to_value(&sim.scenario_rng).unwrap()),
            ("mapgen", serde_json::to_value(&sim.mapgen_rng).unwrap()),
        ] {
            assert_eq!(rng, class["rng_at_return"][stream], "{input}: {stream}");
        }
        // Prove the sole target+4C read and the structural bit used by MoveTo
        // against its actual returned native Cell, not invented flat terrain.
        let projection = events
            .iter()
            .find(|event| event["kind"] == "navigation_coordinate")
            .unwrap();
        assert_eq!(
            projection["output_coordinate"],
            row["after"]["locomotor"]["destination"]
        );
        for query in events.iter().filter_map(|event| event.get("resolved_cell")) {
            let xy = (
                query["cell"][0].as_i64().unwrap() as i16,
                query["cell"][1].as_i64().unwrap() as i16,
            );
            let terrain = sim.resolved_terrain.as_ref().unwrap();
            assert_eq!(
                terrain.native_cell_flags(terrain.native_cell_identity(xy)) & 0x100,
                query["flags"].as_u64().unwrap() as u32 & 0x100,
                "{input}: {xy:?}"
            );
        }
        let after = &row["after"];
        let goal_cell = events
            .iter()
            .find(|event| event["kind"] == "xyz_cell")
            .unwrap();
        let goal_cell = &goal_cell["resolved_cell"]["cell"];
        let actual = sim.substrate.entities.get(1).unwrap();
        let restored: GameEntity =
            serde_json::from_value(serde_json::to_value(actual).unwrap()).unwrap();
        for actor in [actual, &restored] {
            let loco = actor.locomotor.as_ref().unwrap();
            let (destination, head, moving) = if family == "MTNK" {
                let drive = actor
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.selected_drive_runtime())
                    .and_then(|r| r.retained())
                    .unwrap();
                (
                    drive.destination(),
                    drive.head_to(),
                    super::super::track_head::motion_state(
                        actor,
                        super::super::track_process::TrackFamily::Drive,
                    )
                    .0,
                )
            } else {
                (
                    loco.walk_destination(),
                    loco.step_head(),
                    loco.walk_is_moving().unwrap(),
                )
            };
            assert_eq!(
                destination.unwrap_or(ZERO),
                coord(&after["locomotor"]["destination"]),
                "{input}"
            );
            assert_eq!(
                head.unwrap_or(ZERO),
                coord(&after["locomotor"]["head"]),
                "{input}"
            );
            assert_eq!(
                u8::from(moving),
                after["locomotor"]["is_moving_al"].as_u64().unwrap() as u8,
                "{input}"
            );
            assert_eq!(actor.navigation.nav_com, Some(requested), "{input}");
            assert!(actor.navigation.nav_com_aux.is_none(), "{input}");
            assert_eq!(
                actor
                    .navigation
                    .path_replay
                    .directions
                    .iter()
                    .map(|&v| if v == u8::MAX { -1 } else { i32::from(v) })
                    .collect::<Vec<_>>(),
                serde_json::from_value::<Vec<i32>>(after["path"].clone()).unwrap(),
                "{input}"
            );
            assert_eq!(
                json!(actor.navigation.path_replay.reference_cell),
                after["reference_cell"],
                "{input}"
            );
            assert_eq!(
                actor.navigation.nav_queue.len(),
                after["nav_queue"]["count"].as_u64().unwrap() as usize,
                "{input}"
            );
            if family == "E1" {
                assert_eq!(actor.navigation.nav_queue, queue, "{input}");
            }
            let runtime = &actor.navigation.path_runtime;
            // Native timer auxiliary words copy stack residue. Only their
            // native-significant start and duration are represented in Rust.
            for (key, timer) in [
                ("movement_timer_words", runtime.movement_timer),
                ("blocked_timer_words", runtime.blocked_timer),
            ] {
                assert_eq!(
                    json!([timer.start_frame(), timer.duration()]),
                    json!([after[key][0], after[key][2]]),
                    "{input}: {key}"
                );
            }
            assert_eq!(
                u8::from(runtime.path_blocked),
                after["blocked"].as_u64().unwrap() as u8,
                "{input}"
            );
            assert_eq!(
                runtime.retries_left,
                after["retry"].as_u64().unwrap() as u32,
                "{input}"
            );
            assert_eq!(
                actor
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.walk_destination_cell())
                    .or_else(|| crate::sim::movement::movement_goal_cell(actor)),
                Some((
                    goal_cell[0].as_u64().unwrap() as u16,
                    goal_cell[1].as_u64().unwrap() as u16
                )),
                "{input}"
            );
        }
    }
}
