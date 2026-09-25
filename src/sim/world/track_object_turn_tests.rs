use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::{
    DriveCoord, DriveLocomotionRuntime, FootPathQueue, MovementTarget, NavTargetRef,
    ShipLocomotionRuntime, TrackProgress,
};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::drive_track;
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::util::fixed_math::SimFixed;

fn passive_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str("[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=500\nSpeed=6\nSensorsSight=1\nWalkRate=1\nIdleRate=0\nPassive=yes\n")).unwrap()
}

fn fixture() -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&IniFile::from_str("[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=500\nSpeed=6\nSensorsSight=1\nWalkRate=1\nIdleRate=0\n")).unwrap();
    let mut sim = Simulation::new();
    sim.fog.width = 32;
    sim.fog.height = 32;
    sim.session.binary_frame = 10;
    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
    entity.category = EntityCategory::Unit;
    entity.lifecycle.object_alive = true;
    entity.lifecycle.in_limbo = false;
    entity.lifecycle.cell_marked = true;
    entity.is_voxel = false;
    entity.drive_accelerates = false;
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    entity.drive_locomotion = Some(DriveLocomotionRuntime {
        head_to: Some(DriveCoord::cell(10, 9, 0)),
        track_valid: true,
        target_speed_fraction: SimFixed::from_num(1),
        track: TrackProgress {
            turn_index: 0,
            cursor: 0,
            reversed: false,
            residual: 0,
        },
        ..Default::default()
    });
    entity.movement_target = Some(MovementTarget {
        path: vec![(10, 10), (10, 9)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        speed: SimFixed::from_num(330),
        current_speed: SimFixed::from_num(330),
        ..Default::default()
    });
    sim.interner = crate::sim::intern::test_interner();
    // Drive ProcessMovement4B3673..368D stores the frame anchor and
    // duration. Active TrackProcess4B0F20 does not mutate that timer.
    entity.navigation.path_runtime.blocked_timer = crate::sim::timer::CdTimer::started(10, 9);
    sim.substrate.entities.insert(entity);
    sim.substrate.occupancy =
        crate::sim::occupancy::OccupancyGrid::rebuild(&sim.substrate.entities);
    (sim, rules)
}

#[test]
fn post_ai_object_alive_gate_precedes_drive_ship_process_slope_sampling() {
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::sim::movement::slope_transition;

    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        for object_alive in [false, true] {
            let (mut sim, rules) = fixture();
            let cells = (0..32)
                .flat_map(|y| {
                    (0..32).map(move |x| {
                        let mut cell = ResolvedTerrainCell::clear_for_test(x, y);
                        cell.slope_type = if (x, y) == (10, 10) { 5 } else { 0 };
                        cell
                    })
                })
                .collect();
            sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(32, 32, cells));
            let entity = sim.substrate.entities.get_mut(1).unwrap();
            entity.locomotor = Some(LocomotorState::for_test_kind(kind));
            if kind == LocomotorKind::Ship {
                let drive = entity.drive_locomotion.take().unwrap();
                entity.ship_locomotion = Some(ShipLocomotionRuntime {
                    head_to: drive.head_to,
                    track: drive.track,
                    track_valid: drive.track_valid,
                    target_speed_fraction: drive.target_speed_fraction,
                    ..Default::default()
                });
            }
            slope_transition::snap_after_successful_unlimbo(entity, 2, 9);
            let old_slope = *slope_transition::state_for_entity(entity).unwrap();
            entity.lifecycle.object_alive = object_alive;
            assert!(entity.health.current > 0 && !entity.dying);

            let outcome = sim
                .advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
                .expect("object turn");
            let entity = sim.substrate.entities.get(1).unwrap();
            assert_eq!(entity.lifecycle.object_alive, object_alive);
            let slope = slope_transition::state_for_entity(entity).unwrap();
            if object_alive {
                assert_eq!(slope.hash_fields(), (2, 5, 10, 3), "{kind:?}");
                assert!(outcome.movement.moved_steps > 0, "live control {kind:?}");
            } else {
                assert_eq!(
                    *slope, old_slope,
                    "dead owner never enters Process: {kind:?}"
                );
                assert_eq!(outcome.movement.moved_steps, 0, "{kind:?}");
                let track = match kind {
                    LocomotorKind::Drive => entity.drive_locomotion.as_ref().unwrap().track,
                    LocomotorKind::Ship => entity.ship_locomotion.as_ref().unwrap().track,
                    _ => unreachable!(),
                };
                assert_eq!(track.cursor, 0, "{kind:?}");
                assert_eq!(entity.body_frame_counter, 0, "{kind:?}");
            }
        }
    }
}

#[test]
fn ordinary_object_turn_pays_multiple_points_and_runs_prefix_and_shp_once() {
    let (mut sim, rules) = fixture();
    let outcome = sim
        .advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .expect("fixture object turn must complete");
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(outcome.movement.moved_steps >= 2);
    assert!(entity.drive_locomotion.as_ref().unwrap().track.cursor >= 2);
    assert_eq!(
        entity
            .navigation
            .path_runtime
            .blocked_timer
            .remaining(sim.session.binary_frame as i32),
        9
    );
    assert_eq!(entity.body_frame_counter, 1);
    assert_eq!(
        entity.drive_locomotion.as_ref().unwrap().track.turn_index,
        0,
        "paid points retain the active straight-track selector"
    );
    // Drive4B36CA..36E3 queries frame minus anchor. A second Process in
    // the same frame must not age the timer, regardless of paid-point count.
    for frame in [10, 11] {
        sim.session.binary_frame = frame;
        let outcome = sim
            .advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
            .expect("fixture object turn must complete");
        assert!(outcome.movement.moved_steps >= 2);
        let timer = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .navigation
            .path_runtime
            .blocked_timer;
        assert_eq!(timer, crate::sim::timer::CdTimer::started(10, 9));
        assert_eq!(timer.remaining(frame as i32), 19 - frame as i32);
    }
}

#[test]
fn active_drive_ship_track_preserves_target_across_changed_path_and_terrain_requests() {
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::util::fixed_math::SIM_HALF;

    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let (mut sim, rules) = fixture();
        let cells = (0..32)
            .flat_map(|y| {
                (0..32).map(move |x| {
                    let mut cell =
                        crate::sim::world::lifecycle_tests::common_raw_terrain_cell(x, y, 0, false);
                    cell.speed_costs.track = Some(50);
                    cell.speed_costs.float = Some(50);
                    cell
                })
            })
            .collect();
        sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(32, 32, cells));
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.locomotor = Some(LocomotorState::for_test_kind(kind));
        let mut drive = entity.drive_locomotion.take().unwrap();
        drive.target_speed_fraction = SIM_HALF;
        if kind == LocomotorKind::Drive {
            entity.drive_locomotion = Some(drive);
        } else {
            entity.ship_locomotion = Some(ShipLocomotionRuntime {
                head_to: drive.head_to,
                track: drive.track,
                track_valid: drive.track_valid,
                target_speed_fraction: SIM_HALF,
                ..Default::default()
            });
        }
        sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
            .expect("retained track first visit");

        // A changed route and terrain request cannot republish the target on
        // active Process4B055A/69FC6A -> TrackProcess. Only ProcessMovement
        // owns that publication. Both changes would request 0.25 if sampled.
        let next = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(11, 10)
            .unwrap();
        next.speed_costs.track = Some(25);
        next.speed_costs.float = Some(25);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        let target = entity.movement_target.as_mut().unwrap();
        target.path[1] = (11, 10);
        target.final_goal = Some((11, 10));
        let current = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        assert_eq!(
            crate::sim::pathfinding::terrain_speed::compute_cell_speed_modifier(
                entity.locomotor.as_ref().unwrap().speed_type,
                kind,
                (current.x, current.y),
                (11, 10),
                sim.resolved_terrain.as_ref().unwrap(),
                &sim.terrain_speed_config,
                false,
            ),
            SimFixed::lit("0.25"),
            "the changed request must distinguish retained publication: {kind:?}"
        );
        sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
            .expect("retained track changed-request visit");
        let entity = sim.substrate.entities.get(1).unwrap();
        let (retained, progress, head) = if kind == LocomotorKind::Drive {
            let state = entity.drive_locomotion.as_ref().unwrap();
            (state.target_speed_fraction, state.track, state.head_to)
        } else {
            let state = entity.ship_locomotion.as_ref().unwrap();
            (state.target_speed_fraction, state.track, state.head_to)
        };
        assert_eq!(retained, SIM_HALF, "{kind:?}");
        assert_eq!(entity.foot_speed.applied_fraction, SIM_HALF, "{kind:?}");
        assert_eq!(entity.foot_speed.cached_current_speed, 7, "{kind:?}");
        assert_eq!((progress.cursor, progress.residual), (1, 7), "{kind:?}");
        assert_eq!(head, Some(DriveCoord::cell(10, 9, 0)), "{kind:?}");
    }
}

#[test]
fn terminal_sensor_receiver_finishes_before_shp_and_is_not_deferred_to_cell_change() {
    let (mut sim, rules) = fixture();
    // Install a real sensor deposit at the old recorded center, then supply
    // the terminal in the current cell. PerCell2 must remove/add even though
    // this object's cell is unchanged during its movement visit.
    sim.substrate.entities.get_mut(1).unwrap().position.rx = 5;
    sim.add_unit_sensor_after_unlimbo(1, &rules);
    let owner = sim.substrate.entities.get(1).unwrap().owner();
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.position.rx = 10;
    entity.navigation.nav_com = Some(NavTargetRef::cell(10, 10));
    let drive = entity.drive_locomotion.as_mut().unwrap();
    drive.head_to = Some(DriveCoord::cell(10, 10, 0));
    drive.destination = drive.head_to;
    drive.track.cursor = drive_track::raw_track_points(1).len() as i32;
    entity.body_frame_counter = 5;
    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .expect("fixture object turn must complete");
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(entity.sensor_deposit.unwrap().center, (10, 10));
    assert!(!sim.fog.has_sensor_for_house(owner, 5, 10));
    assert!(sim.fog.has_sensor_for_house(owner, 10, 10));
    assert!(entity.navigation.nav_com.is_none());
    assert!(entity.movement_target.is_none());
    assert_eq!(
        entity.body_frame_counter, 5,
        "terminal retired movement before IdleRate=0 SHP admission"
    );
    assert!(entity.foot_occupation_enabled);
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(10, 10),
        0,
        "same-cell terminal performs no extra raw mark"
    );
}

#[test]
fn accepted_chain_runs_sensor_callback_and_consumes_more_paid_points_in_same_object_turn() {
    let (mut sim, _) = fixture();
    let rules = passive_rules();
    // The common CanEnter receiver now observes real map geometry rather
    // than accepting the chain from the object list alone.
    let terrain = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
        32,
        32,
        (0..32)
            .flat_map(|y| {
                (0..32).map(move |x| {
                    let mut cell =
                        crate::map::resolved_terrain::ResolvedTerrainCell::clear_for_test(x, y);
                    cell.speed_costs.track = Some(100);
                    cell
                })
            })
            .collect(),
    );
    sim.path_grid = Some(std::sync::Arc::new(
        crate::sim::pathfinding::PathGrid::from_resolved_terrain(&terrain),
    ));
    sim.resolved_terrain = Some(terrain);
    sim.add_unit_sensor_after_unlimbo(1, &rules);
    let selection = drive_track::select_drive_track(32, 64, false).unwrap();
    assert!(selection.entry_index > 0);
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.facing = 32;
    entity.navigation.path_replay = FootPathQueue {
        directions: vec![2, 3],
        cursor: 0,
        reference_cell: Some((10, 10)),
    };
    entity.drive_locomotion.as_mut().unwrap().track = TrackProgress {
        turn_index: 1,
        cursor: i32::from(drive_track::raw_track_meta(3).unwrap().chain_index),
        reversed: false,
        residual: 0,
    };
    let outcome = sim
        .advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .expect("fixture object turn must complete");
    let entity = sim.substrate.entities.get(1).unwrap();
    let track = entity.drive_locomotion.as_ref().unwrap().track;
    assert_eq!(track.turn_index, selection.turn_track_index as i32);
    assert!(entity.drive_locomotion.as_ref().unwrap().track_valid);
    assert!(track.cursor > i32::from(selection.entry_index));
    assert_eq!(entity.navigation.path_replay.cursor, 1);
    // Retail RawTrack[3]7E7A58 -> point37 at7E66B4=(-136,136,32).
    // TurnTrack[1]7E7B34 flags8 leaves XY unchanged in Transform4B4780.
    // With head(2688,2432), PerCell4B1CFD runs in signed cell(9,10).
    assert_eq!(entity.sensor_deposit.unwrap().center, (9, 10));
    // Live MTNK Speed=6 gives budget15: the next paid point must move away
    // from the callback's exact coordinates, even if it stays in that cell.
    let current = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    assert_ne!((current.x, current.y), (2552, 2568));
    assert!(outcome.movement.moved_steps >= 2);
    assert_eq!(
        entity
            .navigation
            .path_runtime
            .blocked_timer
            .remaining(sim.session.binary_frame as i32),
        9
    );
    assert_eq!(
        entity.navigation.path_runtime.blocked_timer,
        crate::sim::timer::CdTimer::started(10, 9),
        "chain callbacks and additional paid points do not rewrite the timer"
    );
}

#[test]
fn first_process_after_command_applies_raw_head_once_without_a_paid_point_and_after_load() {
    use crate::sim::snapshot::GameSnapshot;
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        for reload in [false, true] {
            let (mut sim, _) = fixture();
            // TrackProcess queries the live type speed; a zero speed on the
            // command's path adapter alone does not stop this getter.
            let rules = RuleSet::from_ini(&IniFile::from_str(
                "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=500\nSpeed=0\nSensorsSight=1\nWalkRate=1\nIdleRate=0\n",
            ))
            .unwrap();
            let entity = sim.substrate.entities.get_mut(1).unwrap();
            entity.drive_locomotion = None;
            entity.ship_locomotion = None;
            entity.movement_target = None;
            entity.locomotor = Some(LocomotorState::for_test_kind(kind));
            let grid = crate::sim::pathfinding::PathGrid::new(32, 32);
            assert!(crate::sim::movement::issue_move_command(
                &mut sim.substrate.entities,
                &grid,
                1,
                (10, 9),
                SimFixed::from_num(0),
                false,
                None,
                None,
                None,
                crate::sim::movement::DestinationTiming::new(0, 60),
            ));
            if reload {
                let bytes = GameSnapshot::save(&sim, 0, 0, "pending_apply", 0);
                sim = GameSnapshot::load(&bytes).unwrap().sim;
            }
            let entity = sim.substrate.entities.get(1).unwrap();
            let head = if kind == LocomotorKind::Drive {
                let state = entity.drive_locomotion.as_ref().unwrap();
                assert!(!state.track_valid);
                state.head_to
            } else {
                entity.ship_locomotion.as_ref().unwrap().head_to
            };
            assert!(head.is_none(), "the command has not run ProcessMovement");
            assert_eq!(
                sim.substrate.raw_cell_occupation.ground_bits(10, 9) & 0x20,
                0
            );
            sim.advance_live_object_turn(
                1,
                Some(&rules),
                techno_ai::ObjectAiCtx {
                    path_grid: Some(&grid),
                    ..Default::default()
                },
            )
            .expect("fixture object turn must complete");
            let entity = sim.substrate.entities.get(1).unwrap();
            if kind == LocomotorKind::Drive {
                let drive = entity.drive_locomotion.as_ref().unwrap();
                assert!(drive.track_valid);
                assert_eq!((drive.track.cursor, drive.track.residual), (0, 0));
            } else {
                let ship = entity.ship_locomotion.as_ref().unwrap();
                assert_eq!((ship.track.cursor, ship.track.residual), (0, 0));
            }
            assert_eq!(entity.foot_speed.cached_current_speed, 0);
            assert_eq!(
                sim.substrate.raw_cell_occupation.ground_bits(10, 9) & 0x20,
                0x20
            );
            sim.substrate.raw_cell_occupation.clear_ground(10, 9, 0x20);
            sim.advance_live_object_turn(
                1,
                Some(&rules),
                techno_ai::ObjectAiCtx {
                    path_grid: Some(&grid),
                    ..Default::default()
                },
            )
            .expect("fixture object turn must complete");
            assert_eq!(
                sim.substrate.raw_cell_occupation.ground_bits(10, 9) & 0x20,
                0,
                "cursor0 does not replay Apply1"
            );
        }
    }
}

#[test]
fn terminal_arrival_resets_owner_speed_before_next_accelerating_move() {
    let (mut sim, rules) = fixture();
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.navigation.nav_com = Some(NavTargetRef::cell(10, 9));
    let drive = entity.drive_locomotion.as_mut().unwrap();
    drive.destination = drive.head_to;
    drive.track.cursor = drive_track::raw_track_points(1).len() as i32;
    entity.foot_speed.applied_fraction = SimFixed::from_num(1);
    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .expect("fixture object turn must complete");
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .foot_speed
            .applied_fraction,
        SimFixed::from_num(0)
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .foot_speed
            .cached_current_speed,
        0
    );
    let grid = crate::sim::pathfinding::PathGrid::new(32, 32);
    assert!(crate::sim::movement::issue_move_command(
        &mut sim.substrate.entities,
        &grid,
        1,
        (10, 6),
        SimFixed::from_num(330),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.drive_accelerates = true;
    entity.movement_target.as_mut().unwrap().accel_factor = SimFixed::from_num(0.03);
    // The first Process after the order requests the route from the grid.
    sim.advance_live_object_turn(
        1,
        Some(&rules),
        techno_ai::ObjectAiCtx {
            path_grid: Some(&grid),
            ..Default::default()
        },
    )
    .expect("fixture object turn must complete");
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .foot_speed
            .applied_fraction,
        SimFixed::from_num(0.03)
    );
}

#[test]
fn ship_fresh_claim_survives_next_object_visit_and_snapshot_rebuild() {
    use crate::sim::snapshot::GameSnapshot;
    let (mut sim, rules) = fixture();
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Ship));
    entity.drive_locomotion = None;
    entity.ship_locomotion = None;
    entity.movement_target = None;
    let grid = crate::sim::pathfinding::PathGrid::new(32, 32);
    assert!(crate::sim::movement::issue_move_command(
        &mut sim.substrate.entities,
        &grid,
        1,
        (10, 7),
        SimFixed::from_num(330),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    let ctx = || techno_ai::ObjectAiCtx {
        path_grid: Some(&grid),
        ..Default::default()
    };
    sim.advance_live_object_turn(1, Some(&rules), ctx())
        .expect("fixture object turn must complete");
    let mark = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .ship_locomotion
        .as_ref()
        .unwrap()
        .occupation_head_to
        .unwrap();
    assert!(
        !sim.substrate
            .entities
            .get(1)
            .unwrap()
            .foot_occupation_enabled
    );
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .movement_target
        .as_mut()
        .unwrap()
        .speed = SimFixed::from_num(15);
    sim.advance_live_object_turn(1, Some(&rules), ctx())
        .expect("fixture object turn must complete");
    assert!(
        sim.substrate
            .cell_occupation
            .occupied_by_other(mark.rx, mark.ry, mark.layer, 99)
    );
    let bytes = GameSnapshot::save(&sim, 0, 0, "ship_head", 0);
    let loaded = GameSnapshot::load(&bytes).unwrap().sim;
    let rebuilt = crate::sim::occupancy::CellOccupationGrid::rebuild(&loaded.substrate.entities);
    assert!(
        rebuilt.occupied_by_other(mark.rx, mark.ry, mark.layer, 99),
        "follower entry sees the saved Ship reservation"
    );
    assert_eq!(
        loaded
            .substrate
            .raw_cell_occupation
            .ground_bits(mark.rx, mark.ry)
            & 0x20,
        0x20
    );
}

#[test]
fn ordinary_default_passive_false_does_not_accept_a_chain() {
    let (mut sim, rules) = fixture();
    // The chain query (0x4B1C3E) asks the native Unit+1AC over map cells.
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_flat_ground_grid(
        32,
    ));
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.drive_locomotion.as_mut().unwrap().track = TrackProgress {
        turn_index: 1,
        cursor: 37,
        reversed: false,
        residual: 0,
    };
    entity.navigation.path_replay = FootPathQueue {
        directions: vec![2, 3],
        cursor: 0,
        reference_cell: Some((10, 10)),
    };
    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .expect("fixture object turn must complete");
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        entity.drive_locomotion.as_ref().unwrap().track.turn_index,
        1
    );
    assert_eq!(entity.navigation.path_replay.cursor, 0);
}

#[test]
fn bridge_terminal_uses_owner_height_to_reach_ground_navcom_target() {
    let (mut sim, rules) = fixture();
    let mut grid = crate::sim::pathfinding::PathGrid::new(32, 32);
    grid.set_cell_for_test(10, 9, 0, true, false);
    sim.path_grid = Some(std::sync::Arc::new(grid));
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.on_bridge = true;
    entity.navigation.nav_com = Some(NavTargetRef::cell(10, 9));
    entity.foot_speed.applied_fraction = SimFixed::from_num(1);
    let drive = entity.drive_locomotion.as_mut().unwrap();
    let deck = DriveCoord::cell(10, 9, 416);
    drive.head_to = Some(deck);
    drive.destination = Some(deck);
    drive.track.cursor = drive_track::raw_track_points(1).len() as i32;
    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .expect("fixture object turn must complete");
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(entity.position.exact_z_leptons, Some(416));
    assert!(entity.navigation.nav_com.is_none());
    assert!(
        entity
            .drive_locomotion
            .as_ref()
            .unwrap()
            .destination
            .is_none()
    );
    assert!(entity.movement_target.is_none());
    assert_eq!(entity.foot_speed.applied_fraction, SimFixed::from_num(0));
}

#[test]
fn terminal_piggyback_end_is_synchronous_and_preserves_owner_speed() {
    let (mut sim, rules) = fixture();
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Teleport));
    assert!(crate::sim::movement::locomotor_owner::begin_drive_for_teleporter(entity, 10));
    entity.drive_locomotion = Some(DriveLocomotionRuntime {
        head_to: Some(DriveCoord::cell(10, 9, 0)),
        destination: Some(DriveCoord::cell(10, 9, 0)),
        target_speed_fraction: SimFixed::from_num(1),
        track_valid: true,
        track: TrackProgress {
            turn_index: 0,
            cursor: drive_track::raw_track_points(1).len() as i32,
            ..Default::default()
        },
        ..Default::default()
    });
    entity.navigation.nav_com = Some(NavTargetRef::cell(10, 9));
    entity.locomotor.as_mut().unwrap().phase =
        crate::sim::movement::locomotor::GroundMovePhase::Cruising;
    entity.foot_speed.applied_fraction = SimFixed::from_num(1);
    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .expect("fixture object turn must complete");
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        entity.locomotor.as_ref().unwrap().kind,
        LocomotorKind::Teleport
    );
    assert!(entity.drive_locomotion.is_none());
    assert_eq!(entity.foot_speed.applied_fraction, SimFixed::from_num(1));
    assert!(entity.navigation.nav_com.is_none());
}
