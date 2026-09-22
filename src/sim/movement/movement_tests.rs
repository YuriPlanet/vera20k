//! Movement integration tests — verifies ground movement, repath behavior, blocked handling,
//! stuck recovery, and infantry sub-cell mechanics using minimal simulation setups.

use super::track_head::committed_track_head;
use crate::sim::movement::locomotion::LocomotorSlot;

use super::*;
use crate::map::entities::EntityCategory;
use crate::sim::components::{DriveCoord, MovementTarget, NavTargetRef};
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::test_interner;
use crate::sim::lifecycle_request::UninitReason;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{CellListInsertion, CellOccupationGrid, OccupancyGrid};
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;
use crate::util::fixed_math::{SIM_HALF, SIM_ONE, SIM_ZERO, SimFixed};

// --- Facing calculation tests ---
// Computed deltas use the high byte of the active-retail 65,534-scale word.

#[test]
fn ordinary_drive_retires_selector_before_entering_an_explicit_tube() {
    use crate::map::resolved_terrain::YR_CELL_LAND_TUNNEL;
    use crate::map::tube_facts::{TubeFact, TubeId};
    use crate::sim::pathfinding::zone_map::ZoneGrid;

    let mut cells: Vec<_> = (0..6).map(|x| slope_cell_at(x, 0, 0)).collect();
    cells[1].yr_cell_land_type = YR_CELL_LAND_TUNNEL;
    cells[1].tube_index = Some(TubeId(0));
    for cell in &mut cells[2..5] {
        cell.ground_walk_blocked = true;
        cell.base_ground_walk_blocked = true;
    }
    let terrain = ResolvedTerrainGrid::from_cells_with_tubes(
        6,
        1,
        cells,
        vec![TubeFact::explicit((1, 0), (5, 0), 2, vec![2, 2, 2, 2])],
    );
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let zones = ZoneGrid::build(&grid, &Default::default(), 6, 1);
    let mut sim = Simulation::with_seed(71);
    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 0, 0);
    entity.owner = sim.intern("Americans");
    entity.type_ref = sim.intern("MTNK");
    entity.category = EntityCategory::Unit;
    entity.facing = 64;
    entity.locomotor = Some(make_drive_loco_for_test());
    entity.drive_locomotion = Some(Default::default());
    // Retail [MTNK] Accelerates=false; this command fixture loads no rules.
    entity.drive_accelerates = false;
    sim.substrate.entities.insert(entity);
    assert!(matches!(
        sim.reveal(1),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    assert!(issue_move_command_with_layered(
        &mut sim.substrate.entities,
        &grid,
        1,
        (5, 0),
        SimFixed::from_num(256),
        false,
        None,
        None,
        Some(&terrain),
        Some(&zones),
        None,
        None,
        None,
        Some(&mut sim.substrate.cell_occupation),
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    let accepted = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        accepted.movement_target.as_ref().unwrap().path,
        [(0, 0), (1, 0), (5, 0)]
    );
    assert!(committed_track_head(accepted).is_none());
    sim.resolved_terrain = Some(terrain.clone());
    sim.zone_grid = Some(zones.clone());
    sim.process_ground_locomotor_for_test(1, None, Some(&grid), None)
        .expect("Process commits the ordinary segment before the tube entrance");
    let accepted = sim.substrate.entities.get(1).unwrap();
    assert!(committed_track_head(accepted).is_some());
    assert!(accepted.drive_locomotion.as_ref().unwrap().track.turn_index >= 0);

    let mut retired_frame = None;
    for frame in 1..160 {
        super::movement_tick::tick_movement_object_with_grids(
            &mut sim.substrate.entities,
            1,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut sim.substrate.occupancy,
            &mut sim.substrate.cell_occupation,
            &mut sim.substrate.raw_cell_occupation,
            &mut sim.substrate.next_occupancy_enter_order,
            &mut sim.scenario_rng,
            u64::from(frame),
            frame,
            Some(&zones),
            Some(&terrain),
            None,
            None,
            None,
            &crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            SIM_ZERO,
            9,
            60,
            &mut sim.interner,
            None,
            &mut Vec::new(),
            &mut Vec::new(),
        );
        let entity = sim.substrate.entities.get(1).unwrap();
        if entity.low_bridge_tube_state.is_some() {
            assert_eq!(
                retired_frame,
                Some(frame - 1),
                "the next Process enters the tube"
            );
            assert!(
                committed_track_head(entity).is_none(),
                "ordinary curve retires before tube ownership"
            );
            assert_eq!(
                entity.drive_locomotion.as_ref().unwrap().track.turn_index,
                -1
            );
            assert_eq!((entity.position.rx, entity.position.ry), (1, 0));
            assert_eq!(entity.low_bridge_tube_state.unwrap().cursor, 0);
            assert_eq!(entity.navigation.path_replay.cursor, 2);
            assert_eq!(entity.navigation.path_replay.reference_cell, Some((1, 0)));
            assert_eq!(
                entity.drive_locomotion.as_ref().unwrap().head_to,
                Some(DriveCoord {
                    x: 1408,
                    y: 128,
                    z: 0
                })
            );
            assert!(entity.drive_locomotion.as_ref().unwrap().track_valid);
            assert_eq!(entity.movement_target.as_ref().unwrap().next_index, 3);
            assert!(!entity.lifecycle.cell_marked);
            assert!(!sim.substrate.occupancy.contains_entity(1, 0, 1));
            return;
        }
        if committed_track_head(entity).is_none() {
            // Terminal4B22AF->4B1F5C writes residual and returns at4B25F9.
            // Direction8 admission4B12xx runs on the following Process visit.
            assert_eq!(retired_frame, None);
            retired_frame = Some(frame);
            assert_eq!((entity.position.rx, entity.position.ry), (1, 0));
            assert!(entity.lifecycle.cell_marked);
        }
        assert!(
            entity.position.rx <= 1,
            "tube steps cannot become ordinary ground curves"
        );
    }
    let entity = sim.substrate.entities.get(1).unwrap();
    panic!(
        "ordinary move did not hand movement to the tube: position={:?}, track={:?}, runtime={:?}",
        entity.position,
        entity.drive_locomotion.as_ref().map(|d| d.track),
        entity.drive_locomotion
    );
}

#[test]
fn test_facing_iso_north() {
    // (0,-1) = north on screen → facing 0.
    let f: u8 = facing_from_delta(0, -1);
    assert_eq!(f, 0, "North (0,-1) should be facing 0");
}

#[test]
fn test_facing_iso_east() {
    let f: u8 = facing_from_delta(1, 0);
    assert_eq!(f, 63, "East (1,0) should be computed facing 63");
}

#[test]
fn test_facing_iso_south() {
    let f: u8 = facing_from_delta(0, 1);
    assert_eq!(f, 127, "South (0,1) should be computed facing 127");
}

#[test]
fn test_facing_iso_west() {
    // (-1,0) = west on screen → facing 192.
    let f: u8 = facing_from_delta(-1, 0);
    assert_eq!(f, 192, "West (-1,0) should be facing 192");
}

#[test]
fn test_facing_iso_northeast() {
    // (1,-1) = NE on screen → facing 32.
    let f: u8 = facing_from_delta(1, -1);
    assert_eq!(f, 32, "NE (1,-1) should be facing 32");
}

#[test]
fn test_facing_iso_southeast() {
    let f: u8 = facing_from_delta(1, 1);
    assert_eq!(f, 95, "SE (1,1) should be computed facing 95");
}

#[test]
fn test_facing_zero_delta() {
    let f: u8 = facing_from_delta(0, 0);
    assert_eq!(f, 63, "Zero delta follows the native conversion path");
}

// --- Movement tick tests ---

#[test]
fn test_tick_movement_advances_position() {
    let mut entities = EntityStore::new();

    // Create an entity at (2, 2) with a path to (5, 2).
    let path: Vec<(u16, u16)> = vec![(2, 2), (3, 2), (4, 2), (5, 2)];

    let mut e = GameEntity::test_default(1, "HTNK", "Americans", 2, 2);
    e.movement_target = Some(MovementTarget {
        path,
        path_layers: vec![MovementLayer::Ground; 4],
        next_index: 1,
        speed: SimFixed::from_num(512), // 512 leptons/sec = 2 cells/sec.
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });
    e.facing = 64;
    entities.insert(e);

    // Tick 500ms at 512 lep/s → 256 leptons = 1 cell → snap to (3,2).
    let mut lifecycle_requests = Vec::new();
    for _ in 0..8 {
        tick_movement(&mut entities, &mut test_interner(), &mut lifecycle_requests);
    }

    let entity = entities.get(1).expect("entity exists");
    assert_eq!(entity.position.rx, 3);
    assert_eq!(entity.position.ry, 2);
    // Entity should still have MovementTarget (not at goal yet).
    assert!(entity.movement_target.is_some());
}

#[test]
fn test_tick_movement_removes_target_at_goal() {
    let mut entities = EntityStore::new();

    // 2-cell path: (0,0) → (1,0). Speed=10 means it finishes instantly.
    let path: Vec<(u16, u16)> = vec![(0, 0), (1, 0)];
    let mut e = GameEntity::test_default(1, "HTNK", "Americans", 0, 0);
    e.movement_target = Some(MovementTarget {
        path,
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        speed: SimFixed::from_num(2560), // 10 cells/sec in leptons.
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });
    entities.insert(e);

    // Large tick to ensure we finish the path.
    let mut lifecycle_requests = Vec::new();
    for _ in 0..2 {
        tick_movement(&mut entities, &mut test_interner(), &mut lifecycle_requests);
    }

    let entity = entities.get(1).expect("entity exists");
    assert_eq!(entity.position.rx, 1);
    assert_eq!(entity.position.ry, 0);
    // MovementTarget should be removed.
    assert!(
        entity.movement_target.is_none(),
        "MovementTarget should be removed when path is complete"
    );
}

#[test]
fn test_drive_arrival_clears_navcom_same_tick() {
    let mut entities = EntityStore::new();

    let mut e = GameEntity::test_default(1, "HTNK", "Americans", 0, 0);
    e.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    e.drive_accelerates = false;
    e.foot_speed.applied_fraction = SIM_ONE;
    e.navigation.nav_com = Some(NavTargetRef::cell(0, 0));
    e.drive_locomotion = Some(crate::sim::components::DriveLocomotionRuntime {
        destination: Some(crate::sim::components::DriveCoord::cell(0, 0, 0)),
        head_to: Some(crate::sim::components::DriveCoord::cell(0, 0, 0)),
        track_valid: true,
        target_speed_fraction: SIM_ONE,
        track: crate::sim::components::TrackProgress {
            turn_index: 0,
            cursor: drive_track::raw_track_points(1).len() as i32,
            residual: 7,
            ..Default::default()
        },
        ..Default::default()
    });
    e.movement_target = Some(MovementTarget {
        path: vec![(0, 0)],
        path_layers: vec![MovementLayer::Ground],
        // One live speed lepton plus residual7 pays the terminal point.
        speed: SimFixed::from_num(15),
        next_index: 1,
        final_goal: Some((0, 0)),
        ..Default::default()
    });
    e.lifecycle.in_limbo = false;
    entities.insert(e);

    // A track that ends at the owner destination stops immediately: the owner
    // destination pair clears on the SAME movement tick, not a deferred pass.
    let mut lifecycle_requests = Vec::new();
    tick_movement(&mut entities, &mut test_interner(), &mut lifecycle_requests);
    let entity = entities.get(1).expect("entity exists");
    assert!(entity.movement_target.is_none());
    assert_eq!(entity.navigation.nav_com, None);
    assert!(!entity.navigation.pending_arrival_clear);
    let drive = entity.drive_locomotion.as_ref().expect("drive state");
    assert_eq!(drive.head_to, None);
    assert!(!drive.track_valid);
    assert_eq!(drive.track.turn_index, -1);
    assert_eq!(drive.track.cursor, 0);
    assert_eq!(drive.destination, None);
}

#[test]
fn test_drive_queue_command_reissues_destination_without_navqueue_append() {
    let mut entities = EntityStore::new();
    let grid = PathGrid::new(8, 4);

    let mut e = GameEntity::test_default(1, "HTNK", "Americans", 0, 0);
    e.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    entities.insert(e);

    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (2, 0),
        SimFixed::from_num(1024),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (4, 0),
        SimFixed::from_num(1024),
        true,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    let entity = entities.get(1).expect("entity exists");
    let movement = entity.movement_target.as_ref().expect("movement target");
    assert_eq!(movement.path.first().copied(), Some((0, 0)));
    assert_eq!(movement.path.last().copied(), Some((4, 0)));
    assert_eq!(movement.final_goal, Some((4, 0)));
    assert_eq!(entity.navigation.nav_com, Some(NavTargetRef::cell(4, 0)));
    assert!(
        entity.navigation.nav_queue.is_empty(),
        "standard player/team/trigger movement must not create Foot NavQueue entries"
    );
}

fn gsi_04_05_tick_production_movement(
    sim: &mut Simulation,
    path_grid: Option<&PathGrid>,
    native_frame: u32,
) {
    sim.session.tick = u64::from(native_frame);
    sim.session.binary_frame = native_frame;
    sim.process_ground_locomotor_for_test(1, None, path_grid, None)
        .unwrap();
}

#[test]
fn walk_path_timer_waits_without_double_aging_or_losing_owner_state() {
    use crate::sim::components::FootPathRuntime;
    use crate::sim::timer::CdTimer;

    let grid = PathGrid::test_all_passable(30, 30);
    for (timer, frame) in [
        (CdTimer::started(100, 9), 108),
        (CdTimer::from_raw(-1, -7), 100),
        (CdTimer::started(i32::MAX - 2, 9), i32::MIN as u32 + 2),
    ] {
        let mut sim = Simulation::with_seed(41);
        let mut actor = GameEntity::test_default(1, "E1", "Americans", 10, 10);
        actor.owner = sim.intern("Americans");
        actor.type_ref = sim.intern("E1");
        actor.category = EntityCategory::Infantry;
        actor.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
        sim.substrate.entities.insert(actor);
        assert!(matches!(
            sim.reveal(1),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));
        assert!(issue_move_command(
            &mut sim.substrate.entities,
            &grid,
            1,
            (20, 10),
            SimFixed::from_num(150),
            false,
            None,
            None,
            None,
            DestinationTiming::new(100, 60),
        ));
        let retained = FootPathRuntime {
            movement_timer: timer,
            blocked_timer: CdTimer::started(100, 60),
            path_blocked: true,
            retries_left: u32::MAX,
        };
        let actor = sim.substrate.entities.get_mut(1).unwrap();
        actor.navigation.path_runtime = retained;
        let position = actor.position.clone();
        let rng = sim.scenario_rng.logical_state();
        // A hut Scatter Process and an ordinary Process can share a frame.
        // Native75AF3C..55 tests a nonzero signed remainder, not a decrement.
        for _ in 0..2 {
            gsi_04_05_tick_production_movement(&mut sim, Some(&grid), frame);
            let actor = sim.substrate.entities.get(1).unwrap();
            assert_eq!(actor.navigation.path_runtime, retained);
            assert_eq!(
                serde_json::to_value(&actor.position).unwrap(),
                serde_json::to_value(&position).unwrap()
            );
            assert!(actor.movement_target.as_ref().unwrap().path.is_empty());
            assert!(actor.locomotor.as_ref().unwrap().step_head().is_none());
            assert_eq!(sim.scenario_rng.logical_state(), rng);
        }

        // Native null setter writes persistent timers even without an adapter;
        // the next accepted order must not recreate or truncate the dword count.
        sim.substrate.entities.get_mut(1).unwrap().movement_target = None;
        sim.session.binary_frame = 200;
        assert!(sim.set_walk_null_destination(1, None));
        let actor = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            actor.navigation.path_runtime.movement_timer,
            CdTimer::started(200, 0)
        );
        assert_eq!(
            actor.navigation.path_runtime.blocked_timer,
            CdTimer::started(200, 60)
        );
        assert!(!actor.navigation.path_runtime.path_blocked);
        assert_eq!(actor.navigation.path_runtime.retries_left, u32::MAX);
        assert!(issue_move_command(
            &mut sim.substrate.entities,
            &grid,
            1,
            (21, 10),
            SimFixed::from_num(150),
            false,
            None,
            None,
            None,
            DestinationTiming::new(201, 22),
        ));
        let actor = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            actor.navigation.path_runtime.movement_timer,
            CdTimer::started(201, 0)
        );
        assert_eq!(
            actor.navigation.path_runtime.blocked_timer,
            CdTimer::started(201, 22)
        );
        assert_eq!(actor.navigation.path_runtime.retries_left, u32::MAX);
        assert_eq!(
            actor.movement_target.as_ref().unwrap().final_goal,
            Some((21, 10))
        );
    }
}

fn drive_ship_slope_process_tick(
    sim: &mut Simulation,
    terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    native_frame: u32,
) {
    sim.resolved_terrain = Some(terrain.clone());
    gsi_04_05_tick_production_movement(sim, None, native_frame);
}

fn slope_cell(rx: u16, slope_type: u8) -> crate::map::resolved_terrain::ResolvedTerrainCell {
    slope_cell_at(rx, 0, slope_type)
}

fn slope_cell_at(
    rx: u16,
    ry: u16,
    slope_type: u8,
) -> crate::map::resolved_terrain::ResolvedTerrainCell {
    crate::map::resolved_terrain::ResolvedTerrainCell {
        slope_type,
        ..drive_speed_test_cell(rx, ry, Default::default())
    }
}

#[test]
fn drive_ship_slope_process_samples_stationary_retargets_and_keeps_rng() {
    let terrain = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
        2,
        1,
        vec![slope_cell(0, 5), slope_cell(1, 11)],
    );
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let mut sim = Simulation::with_seed(0x51_0f_e);
        let mut entity = GameEntity::test_default(1, "SLOPE", "Americans", 0, 0);
        entity.owner = sim.intern("Americans");
        entity.type_ref = sim.intern("SLOPE");
        entity.locomotor = Some(LocomotorState::for_test_kind_at_frame(kind, 2));
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .active_slope_transition_mut()
            .unwrap()
            .snap(2, 2);
        sim.substrate.entities.insert(entity);
        let rng_before = sim.scenario_rng.logical_state();

        drive_ship_slope_process_tick(&mut sim, &terrain, 10);
        let first = crate::sim::movement::slope_transition::state_for_entity(
            sim.substrate.entities.get(1).unwrap(),
        )
        .unwrap()
        .hash_fields();
        assert_eq!(first, (2, 5, 10, 3));
        drive_ship_slope_process_tick(&mut sim, &terrain, 11);
        assert_eq!(
            crate::sim::movement::slope_transition::state_for_entity(
                sim.substrate.entities.get(1).unwrap()
            )
            .unwrap()
            .hash_fields(),
            first,
            "equal stationary Process is a complete no-write"
        );

        sim.substrate.entities.get_mut(1).unwrap().position.rx = 1;
        drive_ship_slope_process_tick(&mut sim, &terrain, 12);
        assert_eq!(
            crate::sim::movement::slope_transition::state_for_entity(
                sim.substrate.entities.get(1).unwrap()
            )
            .unwrap()
            .hash_fields(),
            (5, 11, 12, 3),
            "mid-transition retarget starts from the prior target slope"
        );
        assert_eq!(sim.scenario_rng.logical_state(), rng_before);
    }
}

#[test]
fn drive_slope_boundary_is_detected_on_process_after_ordinary_crossing() {
    let terrain = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
        2,
        1,
        vec![slope_cell(0, 3), slope_cell(1, 8)],
    );
    let mut sim = Simulation::with_seed(3);
    let mut entity = GameEntity::test_default(1, "DRIVE", "Americans", 0, 0);
    entity.owner = sim.intern("Americans");
    entity.type_ref = sim.intern("DRIVE");
    entity.facing = 64;
    entity.locomotor = Some(LocomotorState::for_test_kind_at_frame(
        LocomotorKind::Drive,
        0,
    ));
    entity
        .locomotor
        .as_mut()
        .unwrap()
        .active_slope_transition_mut()
        .unwrap()
        .snap(3, 0);
    entity.foot_speed.applied_fraction = SIM_ONE;
    entity.drive_locomotion = Some(crate::sim::components::DriveLocomotionRuntime {
        target_speed_fraction: SIM_ONE,
        ..Default::default()
    });
    entity.movement_target = Some(MovementTarget {
        path: vec![(0, 0), (1, 0)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        speed: SimFixed::from_num(15_360),
        current_speed: SimFixed::from_num(15_360),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        final_goal: Some((1, 0)),
        ..Default::default()
    });
    sim.substrate.entities.insert(entity);

    let crossing_frame = (20..80)
        .find(|frame| {
            drive_ship_slope_process_tick(&mut sim, &terrain, *frame);
            sim.substrate.entities.get(1).unwrap().position.rx == 1
        })
        .expect("ordinary movement crosses into the adjacent cell");
    assert_eq!(sim.substrate.entities.get(1).unwrap().position.rx, 1);
    assert_eq!(
        crate::sim::movement::slope_transition::state_for_entity(
            sim.substrate.entities.get(1).unwrap()
        )
        .unwrap()
        .hash_fields(),
        (3, 3, 0, 0),
        "the crossing frame has already sampled the old containing cell"
    );
    drive_ship_slope_process_tick(&mut sim, &terrain, crossing_frame + 1);
    assert_eq!(
        crate::sim::movement::slope_transition::state_for_entity(
            sim.substrate.entities.get(1).unwrap()
        )
        .unwrap()
        .hash_fields(),
        (3, 8, (crossing_frame + 1) as i32, 3)
    );
}

#[test]
fn drive_slope_boundary_is_detected_on_process_after_forced_track_crossing() {
    let terrain = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
        1,
        2,
        vec![slope_cell_at(0, 0, 4), slope_cell_at(0, 1, 10)],
    );
    let mut sim = Simulation::with_seed(4);
    let mut entity = GameEntity::test_default(1, "DRIVE", "Americans", 0, 0);
    entity.owner = sim.intern("Americans");
    entity.type_ref = sim.intern("DRIVE");
    entity.locomotor = Some(LocomotorState::for_test_kind_at_frame(
        LocomotorKind::Drive,
        0,
    ));
    entity.drive_locomotion = Some(Default::default());
    sim.substrate.entities.insert(entity);
    assert!(matches!(
        sim.reveal(1),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .active_slope_transition_mut()
        .unwrap()
        .snap(4, 0);
    let rules = forced_drive_rules("DRIVE", 5);
    sim.resolved_terrain = Some(terrain.clone());
    assert!(sim.force_drive_track(1, 0x47, DriveCoord { x: 0, y: 256, z: 0 }));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .foot_speed
        .applied_fraction = SIM_ONE;

    let crossing_frame = (40..120)
        .find(|frame| {
            sim.session.binary_frame = *frame;
            sim.process_ground_locomotor_for_test(1, Some(&rules), None, None)
                .unwrap();
            sim.substrate.entities.get(1).unwrap().position.ry == 1
        })
        .expect("forced track crosses into the adjacent cell");
    assert_eq!(
        crate::sim::movement::slope_transition::state_for_entity(
            sim.substrate.entities.get(1).unwrap()
        )
        .unwrap()
        .hash_fields(),
        (4, 4, 0, 0),
        "the crossing frame samples before the forced track advances"
    );
    sim.session.binary_frame = crossing_frame + 1;
    sim.process_ground_locomotor_for_test(1, Some(&rules), None, None)
        .unwrap();
    assert_eq!(
        crate::sim::movement::slope_transition::state_for_entity(
            sim.substrate.entities.get(1).unwrap()
        )
        .unwrap()
        .hash_fields(),
        (4, 10, (crossing_frame + 1) as i32, 3)
    );
}

#[test]
fn drive_ship_slope_process_uses_foot_class_boundary_not_object_speed_or_art() {
    let terrain = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
        1,
        1,
        vec![slope_cell(0, 12)],
    );
    for (category, kind, expected) in [
        (EntityCategory::Unit, LocomotorKind::Drive, 12),
        (EntityCategory::Unit, LocomotorKind::Ship, 12),
        (EntityCategory::Infantry, LocomotorKind::Drive, 12),
        (EntityCategory::Aircraft, LocomotorKind::Ship, 12),
        (EntityCategory::Structure, LocomotorKind::Drive, 0),
    ] {
        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "MODDED", "Americans", 0, 0);
        entity.owner = sim.intern("Americans");
        entity.type_ref = sim.intern("MODDED");
        entity.category = category;
        entity.locomotor = Some(LocomotorState::for_test_kind(kind));
        sim.substrate.entities.insert(entity);

        drive_ship_slope_process_tick(&mut sim, &terrain, 7);
        let state = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap()
            .active_slope_transition()
            .unwrap();
        assert_eq!(state.hash_fields().1, expected, "{category:?} {kind:?}");
    }
}

#[test]
fn entry_active_tube_excludes_drive_slope_process_for_the_whole_turn() {
    let terrain =
        crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(1, 1, vec![slope_cell(0, 9)]);
    let mut sim = Simulation::new();
    let mut entity = GameEntity::test_default(1, "DRIVE", "Americans", 0, 0);
    entity.owner = sim.intern("Americans");
    entity.type_ref = sim.intern("DRIVE");
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    entity.low_bridge_tube_state = Some(
        crate::sim::movement::tube_movement::LowBridgeTubeMovementState {
            tube_id: crate::map::tube_facts::TubeId(0),
            cursor: 0,
            target: DriveCoord::cell(0, 0, 0),
        },
    );
    sim.substrate.entities.insert(entity);

    drive_ship_slope_process_tick(&mut sim, &terrain, 30);
    assert_eq!(
        crate::sim::movement::slope_transition::state_for_entity(
            sim.substrate.entities.get(1).unwrap()
        )
        .unwrap()
        .hash_fields(),
        (0, 0, 0, 0)
    );
}

#[test]
fn gsi_04_05_production_drive_observes_premark_clear_cross_and_finish() {
    let mut sim = Simulation::new();
    let owner = sim.intern("Americans");
    let type_ref = sim.intern("MTNK");
    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 2, 2);
    entity.owner = owner;
    entity.type_ref = type_ref;
    entity.category = EntityCategory::Unit;
    entity.facing = 64;
    entity.locomotor = Some(make_drive_loco_for_test());
    entity.drive_locomotion = Some(Default::default());
    sim.substrate.entities.insert(entity);
    assert!(matches!(
        sim.reveal(1),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));

    let grid = PathGrid::new(8, 8);
    let issued = {
        let (entities, cell_occupation) = (
            &mut sim.substrate.entities,
            &mut sim.substrate.cell_occupation,
        );
        issue_move_command_with_layered(
            entities,
            &grid,
            1,
            (3, 2),
            SimFixed::from_num(128),
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(cell_occupation),
            crate::sim::movement::DestinationTiming::new(0, 60),
        )
    };
    assert!(issued);
    {
        let movement = sim
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .movement_target
            .as_mut()
            .unwrap();
        movement.accel_factor = SimFixed::lit("0.03");
        movement.decel_factor = SimFixed::lit("0.002");
        movement.slowdown_distance = SimFixed::from_num(500);
    }
    assert!(committed_track_head(sim.substrate.entities.get(1).unwrap()).is_none());
    gsi_04_05_tick_production_movement(&mut sim, Some(&grid), 0);
    assert!(sim.substrate.occupancy.contains_entity(2, 2, 1));
    assert!(!sim.substrate.occupancy.contains_entity(3, 2, 1));
    assert_eq!(
        sim.substrate
            .cell_occupation
            .vehicle_bits(2, 2, MovementLayer::Ground),
        crate::sim::occupancy::VEHICLE_OCCUPATION_BIT
    );
    assert_eq!(
        sim.substrate
            .cell_occupation
            .vehicle_bits(3, 2, MovementLayer::Ground),
        crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
        "accepted Drive track must premark its head before moving the list"
    );

    let arrival_order = sim.substrate.next_occupancy_enter_order.current();
    let initial_order = sim.substrate.entities.get(1).unwrap().occupancy_enter_order;
    let initial_point_index = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .drive_locomotion
        .as_ref()
        .unwrap()
        .track
        .cursor;
    let mut first_unpaid_frame = 0;
    let mut paid_point_observed = false;
    for frame in 1..33 {
        gsi_04_05_tick_production_movement(&mut sim, Some(&grid), frame);
        first_unpaid_frame = frame + 1;
        paid_point_observed = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .drive_locomotion
            .as_ref()
            .is_some_and(|drive| drive.track.cursor > initial_point_index);
        if paid_point_observed {
            break;
        }
    }
    assert!(
        paid_point_observed,
        "the real production cursor must consume a paid point within the fixture bound"
    );
    assert!(sim.substrate.occupancy.contains_entity(2, 2, 1));
    assert!(!sim.substrate.occupancy.contains_entity(3, 2, 1));
    assert_eq!(
        sim.substrate
            .cell_occupation
            .vehicle_bits(2, 2, MovementLayer::Ground),
        0,
        "first paid within-cell point clears the committed current bit"
    );
    assert_eq!(
        sim.substrate
            .cell_occupation
            .vehicle_bits(3, 2, MovementLayer::Ground),
        crate::sim::occupancy::VEHICLE_OCCUPATION_BIT
    );

    assert_eq!(
        sim.substrate.next_occupancy_enter_order.current(),
        arrival_order
    );
    assert_eq!(
        sim.substrate.entities.get(1).unwrap().occupancy_enter_order,
        initial_order
    );
    let mut crossed = false;
    for frame in first_unpaid_frame..96 {
        gsi_04_05_tick_production_movement(&mut sim, Some(&grid), frame);
        let entity = sim.substrate.entities.get(1).unwrap();
        if (entity.position.rx, entity.position.ry) == (3, 2) {
            crossed = true;
            break;
        }
    }
    assert!(
        crossed,
        "production Drive tick must cross into the reserved cell"
    );
    assert!(!sim.substrate.occupancy.contains_entity(2, 2, 1));
    assert!(sim.substrate.occupancy.contains_entity(3, 2, 1));
    assert_eq!(
        sim.substrate
            .cell_occupation
            .vehicle_bits(3, 2, MovementLayer::Ground),
        crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
        "AddContent crossing must re-mark the new current cell"
    );

    assert_eq!(
        sim.substrate.entities.get(1).unwrap().occupancy_enter_order,
        arrival_order
    );
    assert_eq!(
        sim.substrate.next_occupancy_enter_order.current(),
        arrival_order + 1
    );
    let mut finished = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .movement_target
        .is_none();
    for frame in 96..192 {
        if finished {
            break;
        }
        gsi_04_05_tick_production_movement(&mut sim, Some(&grid), frame);
        finished = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .movement_target
            .is_none();
    }
    assert!(
        finished,
        "production Drive track must finish within the fixture bound"
    );
    let drive = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .drive_locomotion
        .as_ref()
        .unwrap();
    assert_eq!(drive.head_to, None);
    assert_eq!(drive.occupation_head_to, None);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .foot_occupation_enabled
    );
    assert!(sim.substrate.occupancy.contains_entity(3, 2, 1));
    assert_eq!(
        sim.substrate
            .cell_occupation
            .vehicle_bits(3, 2, MovementLayer::Ground),
        crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
        "completion promotes the head mark instead of clearing the endpoint"
    );
}

#[test]
fn cell_arrival_infantry_keeps_detour_order_and_snapshot_continuation() {
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::rules::terrain_rules::SpeedCostProfile;
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::snapshot::GameSnapshot;
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [E1]\nStrength=100\nSpeed=4\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\nSpeedType=Foot\n",
    ))
    .unwrap();
    let terrain = ResolvedTerrainGrid::from_cells(
        6,
        3,
        (0..3)
            .flat_map(|ry| {
                (0..6).map(move |rx| {
                    let costs = SpeedCostProfile {
                        foot: Some(100),
                        ..Default::default()
                    };
                    let mut cell = drive_speed_test_cell(rx, ry, costs);
                    cell.base_speed_costs = costs;
                    cell
                })
            })
            .collect(),
    );
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let mut sim = Simulation::with_seed(0xc311_a771);
    sim.install_resolved_terrain_for_new_map(terrain.clone());
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let walker = sim
        .spawn_object_at_height("E1", "Americans", 1, 1, 0, 0, &rules)
        .unwrap();
    assert_eq!(
        walker, 1,
        "the production movement fixture visits this object"
    );
    let resident = sim
        .spawn_object_at_height("E1", "Americans", 2, 1, 0, 0, &rules)
        .unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(walker)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap()
            .kind,
        LocomotorKind::Walk,
    );
    assert!(issue_move_command(
        &mut sim.substrate.entities,
        &grid,
        walker,
        (4, 1),
        crate::util::fixed_math::ra2_speed_to_leptons_per_second(rules.object("E1").unwrap().speed),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    let initial_cell = (1, 1);
    let mut previous_cell = initial_cell;
    // The represented Walk admission defers occupied cells and routes around
    // this stationary resident. Exercise its actual accepted arrivals; the
    // occupied-slot claim cases remain covered in bump_crush's leaf tests.
    let mut crossed_detour_cell = false;
    let mut restored: Option<Simulation> = None;
    let mut trace = Vec::new();
    for frame in 0..400 {
        let next_order = sim.substrate.next_occupancy_enter_order.current();
        let before_path_index = sim
            .substrate
            .entities
            .get(walker)
            .unwrap()
            .movement_target
            .as_ref()
            .map(|target| target.next_index);
        gsi_04_05_tick_production_movement(&mut sim, Some(&grid), frame);
        if let Some(loaded) = restored.as_mut() {
            gsi_04_05_tick_production_movement(loaded, Some(&grid), frame);
            assert_eq!(
                loaded.state_hash(),
                sim.state_hash(),
                "restored frame {frame}"
            );
            assert_eq!(
                loaded.scenario_rng.logical_state(),
                sim.scenario_rng.logical_state()
            );
        }
        let entity = sim.substrate.entities.get(walker).unwrap();
        let cell = (entity.position.rx, entity.position.ry);
        assert_ne!(cell, (2, 1), "the stationary resident remains a blocker");
        assert!(sim.substrate.occupancy.contains_entity(2, 1, resident));
        if cell != previous_cell {
            assert!(!sim.substrate.occupancy.contains_entity(
                previous_cell.0,
                previous_cell.1,
                walker
            ));
            let occupants = sim.substrate.occupancy.get(cell.0, cell.1).unwrap();
            let own_entry = occupants
                .iter_layer(MovementLayer::Ground)
                .find(|entry| entry.entity_id == walker)
                .unwrap();
            assert_eq!(own_entry.sub_cell, entity.sub_cell);
            assert_eq!(
                occupants.first_on_layer(MovementLayer::Ground),
                Some(walker)
            );
            assert_eq!(entity.occupancy_enter_order, next_order);
            assert_eq!(
                sim.substrate.next_occupancy_enter_order.current(),
                next_order + 1
            );
            if cell == (2, 0) {
                crossed_detour_cell = true;
                let bytes = GameSnapshot::save(&sim, 0, 0, "arrival", 0);
                let mut loaded = GameSnapshot::load(&bytes).unwrap().sim;
                loaded.restore_after_snapshot_load().unwrap();
                loaded.resolve_type_handles(&rules);
                loaded.resolved_terrain = Some(terrain.clone());
                let loaded_entries: Vec<_> = loaded
                    .substrate
                    .occupancy
                    .get(cell.0, cell.1)
                    .unwrap()
                    .iter_layer(MovementLayer::Ground)
                    .map(|entry| (entry.entity_id, entry.sub_cell))
                    .collect();
                let entries: Vec<_> = occupants
                    .iter_layer(MovementLayer::Ground)
                    .map(|entry| (entry.entity_id, entry.sub_cell))
                    .collect();
                assert_eq!(
                    loaded_entries, entries,
                    "serialized entry order rebuilds the live list"
                );
                // Full Scenario deserialization intentionally applies Seed(0)
                // (see GameSnapshot::load_unchecked). Compare reconstructed
                // movement with the retained state under that same reload
                // rule, rather than asserting uninterrupted RNG continuity.
                assert_eq!(
                    loaded.scenario_rng.logical_state(),
                    SimRng::new(0).logical_state()
                );
                sim.scenario_rng = SimRng::new(0);
                assert_eq!(loaded.state_hash(), sim.state_hash());
                assert!(loaded.substrate.occupancy.contains_entity(2, 1, resident));
                restored = Some(loaded);
            }
            previous_cell = cell;
        } else {
            let after_order = sim.substrate.next_occupancy_enter_order.current();
            if after_order != next_order {
                // Walk's completed-head corridor75BD70 performs Mark REMOVE/
                // PUT even when the polar step already entered this cell.
                assert_eq!(after_order, next_order + 1);
                assert_eq!(entity.occupancy_enter_order, next_order);
                let after_index = entity
                    .movement_target
                    .as_ref()
                    .map(|target| target.next_index);
                assert!(after_index.is_none() || after_index > before_path_index);
            }
        }
        trace.push((
            frame,
            cell,
            crate::sim::movement::ground_pose::position_world_xy(&entity.position),
            entity.sub_cell,
            entity.occupancy_enter_order,
            sim.scenario_rng.state(),
            entity.locomotor.as_ref().and_then(|l| l.step_head()),
            entity
                .movement_target
                .as_ref()
                .map(|t| (t.path.clone(), t.next_index)),
        ));
        if entity.movement_target.is_none() {
            break;
        }
    }
    assert!(
        crossed_detour_cell,
        "production Walk path must take the same detour; trace={trace:?}"
    );
    assert_eq!(previous_cell, (4, 1), "trace={trace:#?}");
    assert!(
        sim.substrate
            .entities
            .get(walker)
            .unwrap()
            .movement_target
            .is_none()
    );
    println!("cell-arrival Infantry frames: {trace:?}");
}

fn forced_drive_rules(name: &str, speed: u32) -> crate::rules::ruleset::RuleSet {
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0={name}\n[{name}]\nStrength=100\nSpeed={speed}\nAccelerates=no\nLocomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\nSpeedType=Track\n"
    ))).unwrap()
}

#[test]
fn forced_track_object_turn_relinks_each_committed_cell_without_a_movement_target() {
    let rules = forced_drive_rules("CMIN", 5);
    let mut sim = Simulation::new();
    let mut entity = GameEntity::test_default(1, "CMIN", "Americans", 13, 11);
    entity.owner = sim.intern("Americans");
    entity.type_ref = sim.intern("CMIN");
    entity.category = EntityCategory::Unit;
    entity.locomotor = Some(make_drive_loco_for_test());
    entity.drive_accelerates = false;
    entity.drive_locomotion = Some(crate::sim::components::DriveLocomotionRuntime {
        track: crate::sim::components::TrackProgress {
            residual: 5,
            ..Default::default()
        },
        ..Default::default()
    });
    entity.foot_speed.applied_fraction = SimFixed::lit("0.25");
    entity.foot_speed.cached_current_speed = 123;
    sim.substrate.entities.insert(entity);
    assert!(matches!(
        sim.reveal(1),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    let head = DriveCoord {
        x: 13 * 256,
        y: 12 * 256,
        z: 0,
    };
    assert!(sim.force_drive_track(1, 0x47, head));
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(entity.foot_speed.applied_fraction, SimFixed::lit("0.25"));
    assert_eq!(entity.foot_speed.cached_current_speed, 123);
    let drive = entity.drive_locomotion.as_ref().unwrap();
    assert_eq!(drive.destination, Some(head));
    assert_eq!(drive.head_to, Some(head));
    assert_eq!(drive.track.turn_index, 0x47);
    assert_eq!(drive.track.cursor, 0);
    assert_eq!(drive.track.residual, 5);
    assert!(drive.track_valid);
    assert!(sim.substrate.occupancy.contains_entity(13, 11, 1));
    assert!(!sim.substrate.occupancy.contains_entity(13, 12, 1));
    let mut previous_cell = (13, 11);
    let mut crossed = false;
    for frame in 0..128 {
        sim.session.binary_frame = frame;
        sim.process_ground_locomotor_for_test(1, Some(&rules), None, None)
            .unwrap();
        let entity = sim.substrate.entities.get(1).unwrap();
        assert!(entity.movement_target.is_none());
        assert_ne!(
            entity.foot_speed.cached_current_speed, 123,
            "live getter replaces the seeded value"
        );
        let current_cell = (entity.position.rx, entity.position.ry);
        assert!(
            sim.substrate
                .occupancy
                .contains_entity(current_cell.0, current_cell.1, 1)
        );
        if current_cell != previous_cell {
            assert!(
                !sim.substrate
                    .occupancy
                    .contains_entity(previous_cell.0, previous_cell.1, 1)
            );
            crossed = true;
        }
        previous_cell = current_cell;
        if committed_track_head(entity).is_none() {
            break;
        }
    }
    assert!(crossed);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(committed_track_head(entity).is_none());
    assert_eq!((entity.position.rx, entity.position.ry), (13, 12));
    assert_eq!(
        (entity.position.sub_x, entity.position.sub_y),
        (SIM_ZERO, SIM_ZERO)
    );
    let drive = entity.drive_locomotion.as_ref().unwrap();
    assert_eq!(drive.destination, Some(head));
    assert_eq!(drive.head_to, None);
    assert_eq!(drive.occupation_head_to, None);
    assert!(!drive.track_valid);
    assert_eq!(drive.track.turn_index, -1);
    assert_eq!(drive.track.cursor, 0);
    assert_eq!(
        sim.substrate
            .cell_occupation
            .vehicle_bits(13, 12, MovementLayer::Ground),
        crate::sim::occupancy::VEHICLE_OCCUPATION_BIT
    );
}

#[test]
fn gsi_04_05_production_finish_promotes_endpoint_without_clearing_bit() {
    let mut entities = EntityStore::new();
    let mut entity = GameEntity::test_default(1, "HTNK", "Americans", 3, 2);
    entity.category = EntityCategory::Unit;
    entity.lifecycle.cell_marked = true;
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    entity.navigation.nav_com = Some(NavTargetRef::cell(3, 2));
    entity.drive_locomotion = Some(crate::sim::components::DriveLocomotionRuntime {
        occupation_head_to: Some(crate::sim::components::DriveOccupationFootprint {
            rx: 3,
            ry: 2,
            layer: MovementLayer::Ground,
        }),
        ..Default::default()
    });
    entity.movement_target = Some(MovementTarget {
        path: vec![(3, 2)],
        path_layers: vec![MovementLayer::Ground],
        next_index: 1,
        final_goal: Some((3, 2)),
        ..Default::default()
    });
    entity.foot_occupation_enabled = false;
    entities.insert(entity);

    let mut lifecycle_requests = Vec::new();
    tick_movement(&mut entities, &mut test_interner(), &mut lifecycle_requests);

    let drive = entities.get(1).unwrap().drive_locomotion.as_ref().unwrap();
    assert_eq!(drive.occupation_head_to, None);
    assert!(entities.get(1).unwrap().foot_occupation_enabled);
    let rebuilt = CellOccupationGrid::rebuild(&entities);
    assert_eq!(
        rebuilt.vehicle_bits(3, 2, MovementLayer::Ground),
        crate::sim::occupancy::VEHICLE_OCCUPATION_BIT
    );
}

// --- GSI-06.02: order-time reachability gate + zone-constrained substitution ---

/// A 5x1 corridor blocked at x=2, so `(0,0)/(1,0)` and `(3,0)/(4,0)` are two
/// disconnected zones for a ground mover.
fn split_corridor_fixture() -> (PathGrid, crate::sim::pathfinding::zone_map::ZoneGrid) {
    let mut grid = PathGrid::new(5, 1);
    grid.set_blocked(2, 0, true);
    let zone_grid = crate::sim::pathfinding::zone_map::ZoneGrid::build(
        &grid,
        &std::collections::BTreeMap::new(),
        5,
        1,
    );
    (grid, zone_grid)
}

fn resolve_split_goal(
    grid: &PathGrid,
    zone_grid: Option<&crate::sim::pathfinding::zone_map::ZoneGrid>,
    goal: (u16, u16),
) -> Option<(u16, u16)> {
    super::movement_path::resolve_reachable_move_goal(
        grid,
        zone_grid,
        None,
        (0, 0),
        MovementLayer::Ground,
        goal,
        MovementZone::Normal,
        crate::rules::locomotor_type::SpeedType::Track,
    )
}

/// Gamemd's destination resolver ACCEPTS an order whose destination is in
/// another zone: `Can_Reach_Zone` fails, it takes the mover's own zone id and
/// runs the nearby-passable-cell search seeded at the click requiring that zone,
/// so the unit drives to the near bank instead of standing still.
#[test]
fn gsi_06_02_unreachable_move_goal_retargets_into_the_movers_own_zone() {
    let (grid, zone_grid) = split_corridor_fixture();
    let cell = resolve_split_goal(&grid, Some(&zone_grid), (4, 0))
        .expect("the order is accepted with a substituted near-side cell");
    assert_ne!(cell, (4, 0), "the far-side cell must not survive the gate");
    assert!(
        cell.0 <= 1,
        "substitute must lie in the mover's own zone, got {cell:?}"
    );
}

/// When `Can_Reach_Zone` succeeds the clicked cell is used verbatim — no
/// substitution, no retarget.
#[test]
fn gsi_06_02_reachable_move_goal_is_used_verbatim() {
    let (grid, zone_grid) = split_corridor_fixture();
    assert_eq!(
        resolve_split_goal(&grid, Some(&zone_grid), (1, 0)),
        Some((1, 0))
    );
}

/// Gamemd's `mzRow == -1` short-circuit returns "reachable"; the Rust
/// equivalent is "no zone data", which must not refuse the order.
#[test]
fn gsi_06_02_missing_zone_data_short_circuits_to_reachable() {
    let (grid, _zone_grid) = split_corridor_fixture();
    assert_eq!(resolve_split_goal(&grid, None, (4, 0)), Some((4, 0)));
}

/// End-to-end: a cross-zone ground move order is accepted and the mover is sent
/// to its own side of the split. Previously the command was refused and the unit
/// did not move at all.
#[test]
fn gsi_06_02_cross_zone_move_order_is_accepted_and_moves_the_unit() {
    let (grid, zone_grid) = split_corridor_fixture();
    let mut entities = EntityStore::new();
    let mut mover = GameEntity::test_default(1, "MTNK", "Americans", 0, 0);
    mover.category = EntityCategory::Unit;
    mover.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    mover.drive_locomotion = Some(Default::default());
    entities.insert(mover);

    assert!(
        issue_move_command_with_layered(
            &mut entities,
            &grid,
            1,
            (4, 0),
            SimFixed::from_num(1024),
            false,
            None,
            None,
            None,
            Some(&zone_grid),
            None,
            None,
            None,
            None,
            crate::sim::movement::DestinationTiming::new(0, 60),
        ),
        "gamemd accepts a ground move order across a disconnected boundary"
    );
    let target = entities
        .get(1)
        .and_then(|entity| entity.movement_target.as_ref())
        .expect("an accepted order installs a movement target");
    let goal = target
        .final_goal
        .or_else(|| target.path.last().copied())
        .expect("the installed target names a destination");
    assert!(
        goal.0 <= 1,
        "the unit must be sent to its own side of the split, got {goal:?}"
    );
}

#[test]
fn techno_playfield_false_mover_uses_flat_astar_instead_of_hierarchy_abort() {
    let grid = PathGrid::new(5, 1);
    let terrain = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
        5,
        1,
        (0..5)
            .map(|rx| drive_speed_test_cell(rx, 0, Default::default()))
            .collect(),
    );
    let bounds = crate::sim::cell_rect::PlayfieldBounds {
        base: 0,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    };
    assert!(bounds.contains_height_aware_packed(0, 0, 0, 0));
    assert!(bounds.contains_height_aware_packed(4, 0, 0, 0));
    let mut reduced = PathGrid::new(5, 1);
    reduced.set_blocked(2, 0, true);
    let zone_grid = crate::sim::pathfinding::zone_map::ZoneGrid::build(
        &reduced,
        &std::collections::BTreeMap::new(),
        5,
        1,
    );
    assert!(!zone_grid.can_reach(
        MovementZone::Normal,
        (0, 0),
        MovementLayer::Ground,
        (4, 0),
        MovementLayer::Ground,
    ));

    let mut entities = EntityStore::new();
    let mut mover = GameEntity::test_default(1, "MTNK", "Americans", 0, 0);
    mover.category = EntityCategory::Unit;
    mover.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    mover.drive_locomotion = Some(Default::default());
    mover.in_playfield = false;
    entities.insert(mover);

    assert!(issue_move_command_with_layered(
        &mut entities,
        &grid,
        1,
        (4, 0),
        SimFixed::from_num(1024),
        false,
        None,
        None,
        Some(&terrain),
        Some(&zone_grid),
        None,
        None,
        Some(bounds),
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    let target = entities.get(1).unwrap().movement_target.as_ref().unwrap();
    assert_eq!(target.final_goal, Some((4, 0)));
    assert!(target.path.contains(&(2, 0)));
}

#[test]
fn gsi_04_05_second_mover_cannot_adopt_reserved_head_to_endpoint() {
    let mut sim = Simulation::new();
    let grid = PathGrid::new(6, 6);
    let mut first = GameEntity::test_default(1, "MTNK", "Americans", 1, 1);
    first.category = EntityCategory::Unit;
    first.facing = 64;
    first.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    first.drive_locomotion = Some(Default::default());
    let mut second = GameEntity::test_default(2, "MTNK", "Americans", 2, 2);
    second.category = EntityCategory::Unit;
    second.facing = 0;
    second.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    second.drive_locomotion = Some(Default::default());
    sim.substrate.entities.insert(first);
    sim.substrate.entities.insert(second);
    sim.interner = test_interner();
    for id in [1, 2] {
        assert!(matches!(
            sim.reveal(id),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));
    }

    assert!(issue_move_command_with_layered(
        &mut sim.substrate.entities,
        &grid,
        1,
        (2, 1),
        SimFixed::from_num(1024),
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(&mut sim.substrate.cell_occupation),
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    sim.process_ground_locomotor_for_test(1, None, Some(&grid), None)
        .expect("the first mover must reserve its head during Process");
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .drive_locomotion
            .as_ref()
            .unwrap()
            .occupation_head_to
            .map(|head| (head.rx, head.ry)),
        Some((2, 1))
    );
    assert!(
        sim.substrate
            .cell_occupation
            .occupied_by_other(2, 1, MovementLayer::Ground, 2)
    );

    let second_issued = issue_move_command_with_layered(
        &mut sim.substrate.entities,
        &grid,
        2,
        (2, 1),
        SimFixed::from_num(1024),
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(&mut sim.substrate.cell_occupation),
        crate::sim::movement::DestinationTiming::new(0, 60),
    );
    let second_goal = sim
        .substrate
        .entities
        .get(2)
        .and_then(|entity| entity.movement_target.as_ref())
        .and_then(|target| target.final_goal);
    assert!(
        !second_issued || second_goal != Some((2, 1)),
        "another mover must not adopt a bit-reserved endpoint"
    );
    sim.process_ground_locomotor_for_test(2, None, Some(&grid), None)
        .expect("the second mover observes the first Process reservation");
    let second = sim.substrate.entities.get(2).unwrap();
    assert_ne!(
        second
            .drive_locomotion
            .as_ref()
            .unwrap()
            .occupation_head_to
            .map(|head| (head.rx, head.ry)),
        Some((2, 1))
    );
    assert!(sim.substrate.occupancy.contains_entity(1, 1, 1));
    assert!(sim.substrate.occupancy.contains_entity(2, 2, 2));
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(2, 1, MovementLayer::Ground),
        0
    );
}

#[test]
fn test_drive_queued_arrival_pops_navqueue_and_reissues_destination() {
    let mut entities = EntityStore::new();
    let grid = PathGrid::new(8, 4);

    let mut e = GameEntity::test_default(1, "HTNK", "Americans", 0, 0);
    e.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    e.drive_accelerates = false;
    e.foot_speed.applied_fraction = SIM_ONE;
    // The arrival advance (queue pop) is gated on a current Move mission.
    e.mission
        .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
            current: crate::sim::mission::MissionId::from_known(
                crate::sim::mission::MissionType::Move,
            ),
            suspended: crate::sim::mission::MissionId::NONE,
            queued: crate::sim::mission::MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
        });
    e.navigation.nav_com = Some(NavTargetRef::cell(0, 0));
    e.navigation.nav_queue.push(NavTargetRef::cell(3, 0));
    e.drive_locomotion = Some(crate::sim::components::DriveLocomotionRuntime {
        destination: Some(crate::sim::components::DriveCoord::cell(0, 0, 0)),
        head_to: Some(crate::sim::components::DriveCoord::cell(0, 0, 0)),
        track_valid: true,
        target_speed_fraction: SIM_ONE,
        track: crate::sim::components::TrackProgress {
            turn_index: 0,
            cursor: drive_track::raw_track_points(1).len() as i32,
            residual: 7,
            ..Default::default()
        },
        ..Default::default()
    });
    e.movement_target = Some(MovementTarget {
        path: vec![(0, 0)],
        path_layers: vec![MovementLayer::Ground],
        // One live speed lepton plus residual7 pays the terminal point.
        speed: SimFixed::from_num(15),
        next_index: 1,
        final_goal: Some((0, 0)),
        ..Default::default()
    });
    e.lifecycle.in_limbo = false;
    entities.insert(e);

    // Arrival tick: the owner destination clears and the queued waypoint is
    // advanced into a fresh destination on the SAME tick (the arrival
    // advance under a Move mission); the path build stays deferred to the
    // next tick's process-entry pass.
    let mut lifecycle_requests = Vec::new();
    tick_movement_with_grid(
        &mut entities,
        Some(&grid),
        &Default::default(),
        &Default::default(),
        &mut OccupancyGrid::new(),
        &mut SimRng::new(0),
        0,
        &mut test_interner(),
        &mut lifecycle_requests,
    );
    let entity = entities.get(1).expect("entity exists");
    assert!(entity.movement_target.is_none());
    assert_eq!(entity.navigation.nav_com, Some(NavTargetRef::cell(3, 0)));
    assert!(entity.navigation.nav_queue.is_empty());
    assert!(entity.navigation.pending_arrival_clear);

    tick_movement_with_grid(
        &mut entities,
        Some(&grid),
        &Default::default(),
        &Default::default(),
        &mut OccupancyGrid::new(),
        &mut SimRng::new(0),
        1,
        &mut test_interner(),
        &mut lifecycle_requests,
    );
    let entity = entities.get(1).expect("entity exists");
    let movement = entity.movement_target.as_ref().expect("movement target");
    assert_eq!(movement.final_goal, Some((3, 0)));
    assert_eq!(movement.path.first().copied(), Some((0, 0)));
    assert_eq!(movement.path.last().copied(), Some((3, 0)));
    assert_eq!(entity.navigation.nav_com, Some(NavTargetRef::cell(3, 0)));
    assert!(entity.navigation.nav_queue.is_empty());
    assert!(!entity.navigation.pending_arrival_clear);
    assert_eq!(
        entity
            .drive_locomotion
            .as_ref()
            .and_then(|drive| drive.destination),
        Some(crate::sim::components::DriveCoord::cell(3, 0, 0))
    );
}

#[test]
fn test_drive_off_destination_finish_defers_then_resumes_toward_navcom() {
    let mut entities = EntityStore::new();
    let grid = PathGrid::new(8, 4);

    // A truncated drive segment ends at (1,0) while the owner destination
    // (NavCom) is still (3,0): an off-destination finish.
    let mut e = GameEntity::test_default(1, "HTNK", "Americans", 1, 0);
    e.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    e.drive_accelerates = false;
    e.foot_speed.applied_fraction = SIM_ONE;
    e.navigation.nav_com = Some(NavTargetRef::cell(3, 0));
    e.drive_locomotion = Some(crate::sim::components::DriveLocomotionRuntime {
        destination: Some(DriveCoord::cell(3, 0, 0)),
        head_to: Some(DriveCoord::cell(1, 0, 0)),
        track_valid: true,
        target_speed_fraction: SIM_ONE,
        track: crate::sim::components::TrackProgress {
            turn_index: 0,
            cursor: drive_track::raw_track_points(1).len() as i32,
            residual: 7,
            ..Default::default()
        },
        ..Default::default()
    });
    e.movement_target = Some(MovementTarget {
        path: vec![(0, 0), (1, 0)],
        path_layers: vec![MovementLayer::Ground; 2],
        // One live speed lepton plus residual7 pays the terminal point.
        speed: SimFixed::from_num(15),
        next_index: 2,
        final_goal: Some((1, 0)),
        ..Default::default()
    });
    e.lifecycle.in_limbo = false;
    entities.insert(e);

    // Tick 1: the segment finishes short of the owner destination. The owner
    // keeps its destination and the deferred process-entry flag arms; no
    // same-tick clear.
    let mut lifecycle_requests = Vec::new();
    tick_movement_with_grid(
        &mut entities,
        Some(&grid),
        &Default::default(),
        &Default::default(),
        &mut OccupancyGrid::new(),
        &mut SimRng::new(0),
        0,
        &mut test_interner(),
        &mut lifecycle_requests,
    );
    let entity = entities.get(1).expect("entity exists");
    assert!(entity.movement_target.is_none());
    assert_eq!(entity.navigation.nav_com, Some(NavTargetRef::cell(3, 0)));
    assert!(entity.navigation.pending_arrival_clear);

    // Tick 2: the process-entry pass repaths toward the surviving NavCom and
    // the unit resumes moving.
    tick_movement_with_grid(
        &mut entities,
        Some(&grid),
        &Default::default(),
        &Default::default(),
        &mut OccupancyGrid::new(),
        &mut SimRng::new(0),
        1,
        &mut test_interner(),
        &mut lifecycle_requests,
    );
    let entity = entities.get(1).expect("entity exists");
    let movement = entity.movement_target.as_ref().expect("movement target");
    assert_eq!(movement.final_goal, Some((3, 0)));
    assert_eq!(movement.path.first().copied(), Some((1, 0)));
    assert_eq!(movement.path.last().copied(), Some((3, 0)));
    assert_eq!(entity.navigation.nav_com, Some(NavTargetRef::cell(3, 0)));
    assert!(!entity.navigation.pending_arrival_clear);
}

#[test]
fn test_drive_deferred_repath_failure_rearms_retry_instead_of_dead_end() {
    let mut entities = EntityStore::new();
    let mut grid = PathGrid::new(8, 4);
    // Wall off the goal: a full blocked column between (1,0) and (3,0).
    for y in 0..4 {
        grid.set_blocked(2, y, true);
    }

    // Deferred process-entry state: owner destination survives, no active
    // movement, retry flag armed (the off-destination finish already
    // happened).
    let mut e = GameEntity::test_default(1, "HTNK", "Americans", 1, 0);
    e.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    e.navigation.nav_com = Some(NavTargetRef::cell(3, 0));
    e.navigation.pending_arrival_clear = true;
    entities.insert(e);

    // Two ticks: each process-entry repath fails against the wall. The retry
    // flag must stay armed and the owner destination must survive — never the
    // dead-end state (nav_com held with no movement and no retry flag).
    let mut lifecycle_requests = Vec::new();
    for tick in 0..2u64 {
        tick_movement_with_grid(
            &mut entities,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut OccupancyGrid::new(),
            &mut SimRng::new(0),
            tick,
            &mut test_interner(),
            &mut lifecycle_requests,
        );
        let entity = entities.get(1).expect("entity exists");
        assert!(entity.movement_target.is_none());
        assert_eq!(entity.navigation.nav_com, Some(NavTargetRef::cell(3, 0)));
        assert!(
            entity.navigation.pending_arrival_clear,
            "repath failure must re-arm the deferred retry flag (tick {tick})"
        );
    }
}

#[test]
fn test_tick_movement_partial_progress() {
    let mut entities = EntityStore::new();

    let path: Vec<(u16, u16)> = vec![(0, 0), (1, 0), (2, 0)];
    let mut e = GameEntity::test_default(1, "HTNK", "Americans", 0, 0);
    e.movement_target = Some(MovementTarget {
        path,
        path_layers: vec![MovementLayer::Ground; 3],
        next_index: 1,
        speed: SimFixed::from_num(512), // 512 lep/s = 2 cells/sec.
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });
    entities.insert(e);

    // 250ms at 512 lep/s → 128 leptons traveled. sub_x starts at 128 (center),
    // moves to 256 which is the cell boundary — entity should cross to next cell.
    // Use 125ms instead: 512 * 0.125 = 64 leptons → sub_x = 128 + 64 = 192 (mid-cell).
    let mut lifecycle_requests = Vec::new();
    tick_movement(&mut entities, &mut test_interner(), &mut lifecycle_requests);

    let entity = entities.get(1).expect("entity exists");
    assert_eq!(
        entity.position.rx, 0,
        "Should not have moved to next cell yet"
    );
    assert_eq!(entity.position.ry, 0);

    // sub_x should be ~192 (128 center + 64 leptons traveled).
    let sub_x_f32: f32 = entity.position.sub_x.to_num();
    assert!(
        (sub_x_f32 - 162.0).abs() < 2.0,
        "sub_x should be ~162, got {sub_x_f32}"
    );
}

#[test]
fn test_tick_movement_updates_facing() {
    let mut entities = EntityStore::new();

    // Path goes east then south.
    let path: Vec<(u16, u16)> = vec![(0, 0), (1, 0), (1, 1)];
    let mut e = GameEntity::test_default(1, "HTNK", "Americans", 0, 0);
    e.movement_target = Some(MovementTarget {
        path,
        path_layers: vec![MovementLayer::Ground; 3],
        next_index: 1,
        speed: SimFixed::from_num(1280), // 5 cells/sec in leptons.
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });
    e.facing = 64; // Initially facing east.
    entities.insert(e);

    // Move to (1,0). The next delta is computed south, whose active-retail
    // 65,534-scale high byte is 127 (distinct from authored facing 128).
    let mut lifecycle_requests = Vec::new();
    for _ in 0..3 {
        tick_movement(&mut entities, &mut test_interner(), &mut lifecycle_requests);
    }

    let entity = entities.get(1).expect("entity exists");
    assert_eq!(
        entity.facing, 127,
        "Should use the computed retail south facing after first step"
    );
}

#[test]
fn test_issue_move_command_sets_path() {
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(20, 20);

    let e = GameEntity::test_default(1, "HTNK", "Americans", 2, 3);
    entities.insert(e);

    let result: bool = issue_move_command(
        &mut entities,
        &grid,
        1,
        (7, 3),
        SimFixed::from_num(768), // 3 cells/sec × 256 = 768 leptons/sec.
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    );
    assert!(result, "Should find a path on open grid");

    let entity = entities.get(1).expect("entity exists");
    let target = entity
        .movement_target
        .as_ref()
        .expect("should have MovementTarget");
    assert_eq!(*target.path.first().expect("non-empty"), (2, 3));
    assert_eq!(*target.path.last().expect("non-empty"), (7, 3));
    assert_eq!(target.next_index, 1);
    assert_eq!(target.speed, SimFixed::from_num(768));
}

#[test]
fn move_order_defers_drive_track_admission_until_process() {
    let mut entities = EntityStore::new();
    let grid = PathGrid::new(20, 20);
    let mut entity = GameEntity::test_default(1, "HTNK", "Americans", 2, 3);
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    entity.facing = 64;
    entity.lifecycle.in_limbo = false;
    entity.lifecycle.cell_marked = true;
    entities.insert(entity);
    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (7, 3),
        SimFixed::from_num(768),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    let entity = entities.get(1).unwrap();
    let drive = entity.drive_locomotion.as_ref().unwrap();
    assert_eq!(entity.navigation.nav_com, Some(NavTargetRef::cell(7, 3)));
    assert_eq!(drive.destination, Some(DriveCoord::cell(7, 3, 0)));
    assert_eq!(drive.head_to, None);
    assert_eq!(drive.track.turn_index, -1);
    assert_eq!(drive.occupation_head_to, None);
    assert!(
        entity
            .navigation
            .path_replay
            .remaining_directions()
            .is_empty()
    );
    assert_eq!(entity.navigation.path_replay.cursor, 0);
    assert_eq!(entity.navigation.path_replay.reference_cell, None);
    assert_eq!(drive.target_speed_fraction, SIM_ZERO);
    assert_eq!(entity.foot_speed.applied_fraction, SIM_ZERO);
    assert_eq!(drive.turn.target_direction, None);
    assert_eq!(drive.turn.target_facing_16, None);
    assert!(entity.movement_target.as_ref().unwrap().path.is_empty());

    tick_movement_with_grid(
        &mut entities,
        Some(&grid),
        &Default::default(),
        &Default::default(),
        &mut OccupancyGrid::new(),
        &mut SimRng::new(0),
        0,
        &mut test_interner(),
        &mut Vec::new(),
    );
    let entity = entities.get(1).unwrap();
    let drive = entity.drive_locomotion.as_ref().unwrap();
    assert_eq!(drive.head_to, Some(DriveCoord::cell(3, 3, 0)));
    assert_eq!(
        drive_track::turn_track_at(drive.track.turn_index as usize)
            .unwrap()
            .normal_track,
        1
    );
    assert_eq!(
        drive.occupation_head_to.map(|head| (head.rx, head.ry)),
        Some((3, 3))
    );
    assert_eq!(entity.navigation.path_replay.cursor, 1);
    assert_eq!(entity.navigation.path_replay.reference_cell, Some((3, 3)));
    assert_eq!(drive.target_speed_fraction, SIM_ONE);
    assert_eq!(entity.facing_target, None);
}

#[test]
fn test_issue_move_command_starts_drive_track_for_initial_drive_turn() {
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(20, 20);

    let mut e = GameEntity::test_default(1, "HTNK", "Americans", 2, 2);
    e.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    e.facing = 64;
    e.lifecycle.in_limbo = false;
    e.lifecycle.cell_marked = true;
    entities.insert(e);

    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (6, 6),
        SimFixed::from_num(768),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    assert_eq!(entities.get(1).unwrap().facing_target, None);
    tick_movement_with_grid(
        &mut entities,
        Some(&grid),
        &Default::default(),
        &Default::default(),
        &mut OccupancyGrid::new(),
        &mut SimRng::new(0),
        0,
        &mut test_interner(),
        &mut Vec::new(),
    );
    let entity = entities.get(1).expect("entity exists");
    // The hull faces east but the first path node is south-east. gamemd's
    // exact-facing precondition commands that turn and selects no curve until
    // the hull is on the node's octant — the curve does not start mid-turn.
    assert!(
        committed_track_head(entity).is_none(),
        "an off-octant hull must turn before any curve is selected"
    );
    assert_eq!(entity.facing_target, Some(0x60));
}

// `TechnoClass::Set_Destination` @ `0x00741970` records the new destination
// (NavCom @ `0x004D94B0`, Drive coordinate @ `0x004AFD40`) without touching the
// Drive track cursor: a curve already in flight keeps driving to its committed
// head cell and the new path takes over there.
#[test]
fn test_reissue_mid_curve_keeps_track_and_anchors_path_at_head() {
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(20, 20);

    let mut e = GameEntity::test_default(1, "HTNK", "Americans", 2, 3);
    e.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    e.facing = 64;
    e.lifecycle.in_limbo = false;
    e.lifecycle.cell_marked = true;
    entities.insert(e);

    // First Process searches and commits its head (3,3).
    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (7, 3),
        SimFixed::from_num(768),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    tick_movement_with_grid(
        &mut entities,
        Some(&grid),
        &Default::default(),
        &Default::default(),
        &mut OccupancyGrid::new(),
        &mut SimRng::new(0),
        0,
        &mut test_interner(),
        &mut Vec::new(),
    );
    let entity = entities.get(1).expect("entity exists");
    let track_before = entity
        .drive_locomotion
        .as_ref()
        .expect("curve installed")
        .track;
    let drive = entity.drive_locomotion.as_ref().expect("drive state");
    assert_eq!(
        drive.occupation_head_to.map(|head| (head.rx, head.ry)),
        Some((3, 3)),
    );
    {
        let entity = entities.get_mut(1).expect("entity exists");
        let drive = entity.drive_locomotion.as_mut().expect("drive state");
        drive.target_speed_fraction = SimFixed::lit("0.4");
        entity.foot_speed.applied_fraction = SimFixed::lit("0.25");
        entity.foot_speed.cached_current_speed = 7;
    }

    // Re-order behind the body while the curve is in flight. Pre-fix this
    // cleared/replaced the curve; the curve must survive untouched and the new
    // path must start on its head so no position rewrite can occur.
    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (0, 3),
        SimFixed::from_num(768),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    let entity = entities.get(1).expect("entity exists");
    let track_after = entity
        .drive_locomotion
        .as_ref()
        .expect("in-flight curve kept")
        .track;
    assert_eq!(
        track_after, track_before,
        "the in-flight curve must survive the re-order untouched"
    );
    let movement = entity.movement_target.as_ref().expect("movement target");
    assert_eq!(
        movement.path.last().copied(),
        Some((3, 3)),
        "only the curve's committed head survives until Process searches again"
    );
    assert_eq!(
        movement.next_index, 1,
        "the still-unreached head is itself the first queued node"
    );
    assert_eq!(movement.final_goal, Some((0, 3)));
    let drive = entity.drive_locomotion.as_ref().expect("drive state");
    assert_eq!(
        drive.occupation_head_to.map(|head| (head.rx, head.ry)),
        Some((3, 3)),
        "the kept curve keeps its head-to occupation claim"
    );
    assert_eq!(entity.navigation.path_replay.reference_cell, Some((3, 3)));
    assert_eq!(entity.navigation.path_replay.cursor, 1);
    assert!(
        entity
            .navigation
            .path_replay
            .remaining_directions()
            .is_empty()
    );
    assert_eq!(drive.target_speed_fraction, SimFixed::lit("0.4"));
    assert_eq!(entity.foot_speed.applied_fraction, SimFixed::lit("0.25"));
    assert_eq!(entity.foot_speed.cached_current_speed, 7);
    assert_eq!(entity.navigation.nav_com, Some(NavTargetRef::cell(0, 3)));
}

// The player-visible symptom of replacing an in-flight curve: the fresh curve's
// cursor restarted at the lead-in point on the current cell centre, so every
// mid-drive re-order teleported the body backward by its mid-cell progress.
#[test]
fn test_reissue_mid_curve_does_not_snap_position_backward() {
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(20, 20);

    let mut e = GameEntity::test_default(1, "HTNK", "Americans", 2, 3);
    e.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    e.facing = 64;
    // No rules are loaded, so `accel_factor` is 0 and the Accelerates= ramp
    // would hold the speed fraction at 0 forever; drive at constant speed.
    e.drive_accelerates = false;
    entities.insert(e);

    // Slow enough that each tick pays about one 7-cost track point.
    let speed = SimFixed::from_num(120);
    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (7, 3),
        speed,
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    // Advance until the body sits visibly past its cell centre (sub_x 128)
    // but has not crossed into (3,3) yet.
    let mut lifecycle_requests = Vec::new();
    let mut observed_mid_cell = false;
    for _ in 0..50 {
        tick_movement(&mut entities, &mut test_interner(), &mut lifecycle_requests);
        let entity = entities.get(1).expect("entity exists");
        if entity.position.rx > 2 {
            break;
        }
        if entity.position.sub_x.to_num::<i32>() >= 168 {
            observed_mid_cell = true;
            break;
        }
    }
    assert!(
        observed_mid_cell,
        "the curve never presented a mid-cell state past the centre"
    );
    let entity = entities.get(1).expect("entity exists");
    assert!(
        committed_track_head(entity).is_some(),
        "curve still in flight"
    );
    let sub_x_before = entity.position.sub_x;
    let east_before = i32::from(entity.position.rx) * 256 + sub_x_before.to_num::<i32>();

    // Re-order further east mid-curve: the order itself must not move the
    // body, and the next tick must continue forward — never snap back toward
    // the cell centre.
    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (9, 3),
        speed,
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    let entity = entities.get(1).expect("entity exists");
    assert_eq!(
        entity.position.sub_x, sub_x_before,
        "issuing the order must not move the body"
    );
    tick_movement(&mut entities, &mut test_interner(), &mut lifecycle_requests);
    let entity = entities.get(1).expect("entity exists");
    let east_after = i32::from(entity.position.rx) * 256 + entity.position.sub_x.to_num::<i32>();
    assert!(
        east_after > east_before,
        "body must keep moving forward across a mid-curve re-order \
         (east {east_before} -> {east_after}); a regression here is the \
         backward-teleport-on-click symptom"
    );
}

#[test]
fn test_issue_move_command_no_path() {
    let mut entities = EntityStore::new();
    let mut grid: PathGrid = PathGrid::new(10, 10);

    // Block column 5 completely.
    for y in 0..10 {
        grid.set_blocked(5, y, true);
    }

    let e = GameEntity::test_default(1, "HTNK", "Americans", 0, 0);
    entities.insert(e);

    let result: bool = issue_move_command(
        &mut entities,
        &grid,
        1,
        (9, 9),
        SimFixed::from_num(768),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    );
    assert!(!result, "Should fail with blocked path");
    let entity = entities.get(1).expect("entity exists");
    assert!(
        entity.movement_target.is_none(),
        "Should not have MovementTarget when no path found"
    );
}

#[test]
fn test_issue_move_command_queue_appends_waypoint_path() {
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(32, 32);

    let e = GameEntity::test_default(1, "HTNK", "Americans", 2, 2);
    entities.insert(e);

    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (8, 2),
        SimFixed::from_num(768),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (12, 2),
        SimFixed::from_num(768),
        true,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    let entity = entities.get(1).expect("entity exists");
    let movement = entity
        .movement_target
        .as_ref()
        .expect("should keep movement target");
    assert_eq!(
        movement.path.last().copied(),
        Some((12, 2)),
        "Queued command should append final waypoint"
    );
    assert!(
        movement.path.len() > 7,
        "Queued command should extend path beyond initial destination"
    );
}

#[test]
fn test_tick_movement_repaths_when_next_cell_becomes_blocked() {
    let mut entities = EntityStore::new();
    let mut grid: PathGrid = PathGrid::new(8, 8);

    let e = GameEntity::test_default(1, "HTNK", "Americans", 1, 1);
    entities.insert(e);

    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (5, 1),
        SimFixed::from_num(1024),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    // Simulate a dynamic blocker appearing on the immediate next step.
    grid.set_blocked(2, 1, true);

    // With blockage_path_delay_ticks=60, the entity must wait 60 ticks for
    // blocked_delay to expire before a repath is attempted. After a successful
    // repath, it needs additional ticks to travel the detour to (5,1).
    let mut lifecycle_requests = Vec::new();
    for _ in 0..80 {
        tick_movement_with_grid(
            &mut entities,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut OccupancyGrid::new(),
            &mut SimRng::new(0),
            0,
            &mut test_interner(),
            &mut lifecycle_requests,
        );
    }

    let entity = entities.get(1).expect("entity exists");
    assert_eq!(
        (entity.position.rx, entity.position.ry),
        (5, 1),
        "Entity should recover and reach destination after repath"
    );
}

/// GSI-06.01 G1: gamemd's code-2 dispatch calls `FootClass::Find_Path` on every
/// tick the movement-delay rate limiter allows — the `BlockagePathDelay` timer
/// selects the URGENCY (1 while running, 2 once expired), it does not suppress
/// the call. VERA used to do nothing at all for the whole grace window, which
/// made urgency 1 dead code.
#[test]
fn gsi_06_01_code_two_grace_window_still_repaths_at_urgency_one() {
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(10, 10);
    let mut occupancy = OccupancyGrid::new();

    let mut mover = GameEntity::test_default(1, "HTNK", "Americans", 1, 1);
    mover.movement_target = Some(MovementTarget {
        path: vec![(1, 1), (2, 1), (3, 1)],
        path_layers: vec![MovementLayer::Ground; 3],
        next_index: 1,
        speed: SimFixed::from_num(1024),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        final_goal: Some((3, 1)),
        ..Default::default()
    });
    mover.facing = 64;
    entities.insert(mover);
    occupancy.add(
        1,
        1,
        1,
        MovementLayer::Ground,
        None,
        CellListInsertion::PrependNonBuilding,
    );

    // A friendly that is itself moving occupies the mover's next cell — the
    // native moving-ally branch, code 2. It crawls so it stays in the way.
    let mut blocker = GameEntity::test_default(2, "HTNK", "Americans", 2, 1);
    blocker.movement_target = Some(MovementTarget {
        path: vec![(2, 1), (2, 4)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        speed: SimFixed::from_num(15),
        move_dir_x: SIM_ZERO,
        move_dir_y: SimFixed::from_num(256),
        move_dir_len: SimFixed::from_num(256),
        final_goal: Some((2, 4)),
        ..Default::default()
    });
    blocker.facing = 128;
    entities.insert(blocker);
    occupancy.add(
        2,
        1,
        2,
        MovementLayer::Ground,
        None,
        CellListInsertion::PrependNonBuilding,
    );

    let mut lifecycle_requests = Vec::new();
    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    let mut repaths_on_first_blocked_tick = None;
    let mut grace_on_first_blocked_tick = 0u16;
    for native_frame in 0..12 {
        let stats = tick_movement_with_grid(
            &mut entities,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut occupancy,
            &mut rng,
            native_frame,
            &mut interner,
            &mut lifecycle_requests,
        );
        let blocked = entities
            .get(1)
            .filter(|e| e.movement_target.is_some())
            .map(|e| {
                (
                    e.navigation.path_runtime.path_blocked,
                    e.navigation
                        .path_runtime
                        .blocked_timer
                        .remaining(native_frame as i32) as u16,
                )
            });
        if let Some((true, delay)) = blocked
            && repaths_on_first_blocked_tick.is_none()
        {
            repaths_on_first_blocked_tick = Some(stats.repath_attempts);
            grace_on_first_blocked_tick = delay;
        }
    }

    let repaths = repaths_on_first_blocked_tick.expect("mover must hit the moving-ally block");
    assert!(
        grace_on_first_blocked_tick > 0,
        "the BlockagePathDelay grace must still be running on the first blocked tick"
    );
    assert!(
        repaths >= 1,
        "gamemd repaths on the very first code-2 tick at urgency 1; got {repaths} attempts"
    );
}

/// Code 2 raises the latch, arms its wait ONCE from `BlockagePathDelay`, and
/// scatters nobody.
///
/// `CellClass::Scatter_Objects 0x00481670` has exactly four call sites inside
/// `Process_Movement`, and a forward control-flow walk from the code-2 entry at
/// `0x004B364D` reaches none of them. What the arm does instead:
///
///   `0x004B3659` reads the latch `+0x6B7`, `0x004B3661 JNZ 0x004B3690` skips
///   the arming when it is already raised, `0x004B3663` raises it, and
///   `0x004B3678`/`0x004B367E`/`0x004B3684` arm the timer at `+0x668` from
///   `Rules+0x1768` = `[AI] BlockagePathDelay` (60 in this harness).
///
/// So the series falls monotonically from 60. A re-arm would show as a rise, and
/// the 10 this port used to write would show as a wrong first reading. The 10 was
/// never a wait at all: it is `Foot+0x64C = 0xA` at `0x004B3285`, a retry-counter
/// reload consumed by the decrement at `0x004B2DC8`, on a different arm.
///
/// An earlier version of this comment asserted the opposite of all of the above
/// - a 10-frame wait re-armed every pass, and `BlockagePathDelay` as "a different
/// timer with a different consumer". That was the belief this test now refutes.
///
/// Reaching the timer at all is what the fixture is for. On an open grid the
/// mover repaths around the blocker within a couple of ticks and nothing is
/// observable. Here the map is a TWO-cell corridor holding a converging friendly
/// pair (the ally faces south, so the pair is not the head-on exit's opposed-
/// octant case): there is no route around, and each unit's only walkable
/// neighbour is the other one, so both stay classified as a moving ally
/// indefinitely. That mutual stall is native-correct - code 2 has no escape
/// hatch in gamemd either - and it is what keeps the mover in the arm long
/// enough to watch the timer run down.
#[test]
fn code_two_arms_blockage_path_delay_once_and_never_scatters() {
    const WAIT: i32 = crate::sim::movement::bump_crush::POST_SCATTER_WAIT_FRAMES;
    // `[AI] BlockagePathDelay` as this harness supplies it. `handle_blocked_tick`
    // writes it on the `path_blocked` 0 -> 1 transition and is now its only
    // writer: the code-2 arm used to pre-raise the flag and then re-implement the
    // arming with its own constant. Watching which of the two shows up in the
    // timer is the observable form of that claim.
    const HARNESS_BLOCKAGE_PATH_DELAY: u16 = 60;
    // Three full spans plus one sample, so a single re-arm cannot satisfy it.
    const REQUIRED_SAMPLES: usize = (WAIT as usize) * 3 + 1;
    const TICKS: u32 = 60;

    let mut grid: PathGrid = PathGrid::new(4, 3);
    for y in 0..3 {
        for x in 0..4 {
            grid.set_blocked(x, y, true);
        }
    }
    grid.set_blocked(1, 1, false);
    grid.set_blocked(2, 1, false);
    // A third cell south of the ally, held by a parked friendly, gives the
    // ally somewhere to be heading that is not the mover: two vehicles driving
    // straight at each other are the head-on exit of `UnitClass::Can_Enter_Cell`
    // (`0x0073F8D4..FA26`, opposed octants inside `0x1FF` leptons on the
    // mover's bearing), which answers 7 and never reaches the code-2 arm this
    // fixture measures.
    grid.set_blocked(2, 2, false);

    let mut entities = EntityStore::new();
    let mut occupancy = OccupancyGrid::new();

    // Mover at the west end, stepping east into the corridor's other cell.
    let mut mover = GameEntity::test_default(1, "HTNK", "Americans", 1, 1);
    mover.movement_target = Some(MovementTarget {
        path: vec![(1, 1), (2, 1)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        speed: SimFixed::from_num(1024),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        final_goal: Some((2, 1)),
        ..Default::default()
    });
    mover.facing = 64;
    entities.insert(mover);
    occupancy.add(
        1,
        1,
        1,
        MovementLayer::Ground,
        None,
        CellListInsertion::PrependNonBuilding,
    );

    // Friendly at the east end, stepping south into a cell a parked ally holds.
    // It keeps a live movement target for the whole run — its repath through
    // the parked ally's cell (a code-6 soft block, not a hard one) succeeds
    // every time, so it never exhausts its stuck counter and never reverts to
    // a stationary ally. Facing south, it is not the mover's head-on case.
    let mut blocker = GameEntity::test_default(2, "HTNK", "Americans", 2, 1);
    blocker.movement_target = Some(MovementTarget {
        path: vec![(2, 1), (2, 2)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        speed: SimFixed::from_num(1024),
        move_dir_x: SIM_ZERO,
        move_dir_y: SimFixed::from_num(256),
        move_dir_len: SimFixed::from_num(256),
        final_goal: Some((2, 2)),
        ..Default::default()
    });
    blocker.facing = 128;
    entities.insert(blocker);
    occupancy.add(
        2,
        1,
        2,
        MovementLayer::Ground,
        None,
        CellListInsertion::PrependNonBuilding,
    );
    // Parked friendly holding the ally's destination.
    let parked = GameEntity::test_default(3, "HTNK", "Americans", 2, 2);
    entities.insert(parked);
    occupancy.add(
        2,
        2,
        3,
        MovementLayer::Ground,
        None,
        CellListInsertion::PrependNonBuilding,
    );

    let mut lifecycle_requests = Vec::new();
    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    // Post-tick wait readings, collected from the first blocked tick onward.
    let mut waits: Vec<u16> = Vec::new();
    let mut scatter_calls = 0u32;
    for native_frame in 0..TICKS {
        let tick_stats = tick_movement_with_grid(
            &mut entities,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut occupancy,
            &mut rng,
            native_frame as u64,
            &mut interner,
            &mut lifecycle_requests,
        );
        scatter_calls = scatter_calls.saturating_add(tick_stats.scatter_successes);
        let blocked = entities
            .get(1)
            .filter(|e| e.movement_target.is_some())
            .map(|e| {
                (
                    e.navigation.path_runtime.path_blocked,
                    e.navigation
                        .path_runtime
                        .blocked_timer
                        .remaining(native_frame as i32) as u16,
                )
            });
        match blocked {
            Some((true, delay)) => waits.push(delay),
            // Once the run has started, any gap means the fixture stopped
            // holding the block and the samples below would be meaningless.
            _ if !waits.is_empty() => break,
            _ => {}
        }
        assert_eq!(
            entities.get(1).map(|e| (e.position.rx, e.position.ry)),
            Some((1, 1)),
            "the corridor must hold the mover in place; waits so far: {waits:?}"
        );
    }

    assert!(
        waits.len() >= REQUIRED_SAMPLES,
        "the mover must stay blocked for at least three post-scatter spans; \
         got {} readings: {waits:?}",
        waits.len()
    );
    assert_eq!(
        scatter_calls, 0,
        "the code-2 arm must scatter nobody; a forward walk from 0x004B364D \
         reaches none of the four Scatter_Objects sites"
    );
    assert_eq!(
        waits[0], HARNESS_BLOCKAGE_PATH_DELAY,
        "the code-2 wait is armed from BlockagePathDelay (Rules+0x1768, read at \
         0x004B367E), not from a hardcoded constant; series: {waits:?}"
    );
    // Armed once, behind the latch: native's store sits after
    // `0x004B3661 JNZ 0x004B3690`, which skips it whenever `+0x6B7` is already
    // raised, so a re-arm would show up here as a rise.
    for pair in waits.windows(2) {
        assert!(
            pair[1] <= pair[0],
            "the code-2 wait must never re-arm while the block holds; series: {waits:?}"
        );
    }
    // It counts all the way down rather than resetting: the fixture runs 60
    // ticks, so the series ends near zero instead of at it. What matters is the
    // total fall, which a re-arming timer could never show.
    let last = *waits.last().expect("samples");
    assert!(
        last < HARNESS_BLOCKAGE_PATH_DELAY / 2,
        "the wait must run down, not reset; series: {waits:?}"
    );
}

/// GSI-07.03: the blocked-step Override fires exactly ONCE per block, and the
/// mover stops where it stood.
///
/// The original's blocking-object body runs `Override_Mission(Attack, blocker,
/// NULL)` and then falls into the shared tail — clear the stored path array,
/// drive the applied speed fraction to zero, call the locomotor's own
/// `Stop_Moving` — and returns. With the Override's NULL destination the walk
/// step has neither a destination nor a path, so it cannot re-enter the arm.
///
/// This is the whole reason the trigger needs the stop. A second Override with
/// an empty queue archives the CURRENT mission, so a mover that re-entered on
/// tick two would overwrite its archived Move with Attack and every later
/// Restore would hand it back Attack instead of its order — a unit losing its
/// move order every time an enemy blocks it.
/// Four locomotors own the arm in gamemd — Walk, Hover, Drive and Ship — and
/// this test covers the two VERA currently routes here: Walk and HOVER. Hover's
/// movement processor `FUN_00514F70` carries its own `case 4: case 5:` pair
/// (object arm Override at 0x00515C2C, wall arm at 0x00515C9C) — the Override
/// itself is the same, though the stop route around it is not; see the residual
/// on the gate in `movement_occupancy`. `Locomotor={4A582742-...}` has four
/// uncommented users: [ROBO] the Robot Tank and the [LCRF]/[SAPC]/[YHVR]
/// transports.
#[test]
fn gsi_07_03_blocked_mover_overrides_onto_attack_exactly_once() {
    use crate::rules::locomotor_type::LocomotorKind;
    for kind in [LocomotorKind::Walk, LocomotorKind::Hover] {
        blocked_override_fires_once_for(kind);
    }
}

fn blocked_override_fires_once_for(mover_kind: crate::rules::locomotor_type::LocomotorKind) {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::combat::TargetKind;
    use crate::sim::mission::leaf::MissionLeafState;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionId, MissionType};

    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(10, 10);
    let mut occupancy = OccupancyGrid::new();

    let mut mover = GameEntity::test_default(1, "E1", "Americans", 1, 1);
    mover.category = EntityCategory::Infantry;
    mover.mission_leaf = MissionLeafState::for_entity_category(EntityCategory::Infantry);
    mover.locomotor = Some(LocomotorState::for_test_kind(mover_kind));
    let timer = mover.mission.dispatch_timer();
    mover.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_known(MissionType::Move),
        suspended: MissionId::NONE,
        queued: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: timer,
    });
    mover.movement_target = Some(MovementTarget {
        path: vec![(1, 1), (2, 1), (3, 1)],
        path_layers: vec![MovementLayer::Ground; 3],
        next_index: 1,
        speed: SimFixed::from_num(1024),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        final_goal: Some((3, 1)),
        ..Default::default()
    });
    mover.facing = 64;
    entities.insert(mover);
    occupancy.add(
        1,
        1,
        1,
        MovementLayer::Ground,
        Some(2),
        CellListInsertion::PrependNonBuilding,
    );

    // A stationary enemy infantryman standing in the next cell: native
    // cell-entry class 5, blocking-object arm.
    let mut blocker = GameEntity::test_default(2, "E1", "Soviets", 2, 1);
    blocker.category = EntityCategory::Infantry;
    blocker.mission_leaf = MissionLeafState::for_entity_category(EntityCategory::Infantry);
    blocker.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
    entities.insert(blocker);
    occupancy.add(
        2,
        1,
        2,
        MovementLayer::Ground,
        Some(2),
        CellListInsertion::PrependNonBuilding,
    );

    let mut lifecycle_requests = Vec::new();
    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    for native_frame in 0..24 {
        tick_movement_with_grid(
            &mut entities,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut occupancy,
            &mut rng,
            native_frame,
            &mut interner,
            &mut lifecycle_requests,
        );
    }

    let mover = entities.get(1).expect("mover survives");
    assert_eq!(
        mover.mission.current(),
        MissionId::from_known(MissionType::Attack),
        "the blocked step overrides onto Attack"
    );
    assert_eq!(
        mover.mission.suspended(),
        MissionId::from_known(MissionType::Move),
        "a second Override would have archived Attack over the Move — the \
         Override must fire exactly once per block"
    );
    assert_eq!(
        mover.attack_target.as_ref().map(|target| target.target),
        Some(TargetKind::Entity(2)),
        "the blocker is the installed target"
    );
    assert!(
        mover.movement_target.is_none(),
        "the tail clears the stored path and the mover stops where it stood"
    );
    assert!(
        mover.navigation.nav_com.is_none(),
        "the Override passes a NULL destination"
    );
    assert_eq!(
        mover.navigation.suspended_nav_com,
        Some(NavTargetRef::cell(3, 1)),
        "the archived destination is what a later Restore hands back"
    );
    assert_eq!(
        (mover.position.rx, mover.position.ry),
        (1, 1),
        "the mover never entered the blocked cell"
    );
}

/// GSI-06.06 G1: gamemd clears the owner's `path_blocked` impatience flag on the
/// first paid track point of a segment and again at track termination — real
/// forward progress, not repath success. VERA cleared it only for infantry, so a
/// vehicle carried a stale grace timer into its next block.
#[test]
fn gsi_06_06_vehicle_clears_path_blocked_on_forward_progress() {
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(10, 10);
    let mut occupancy = OccupancyGrid::new();

    let mut mover = GameEntity::test_default(1, "HTNK", "Americans", 1, 1);
    mover.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    // `[HTNK] Accelerates=false` — the tank snaps to its target fraction, so
    // the fixture does not depend on a rules-supplied AccelerationFactor.
    mover.drive_accelerates = false;
    mover.facing = 64;
    entities.insert(mover);
    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (6, 1),
        SimFixed::from_num(1024),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    // Pretend a block already happened: the mover is impatient with a full
    // grace window still to run, and the lane ahead is now clear.
    {
        let entity = entities.get_mut(1).expect("mover");
        assert!(entity.movement_target.is_some());
        entity.navigation.path_runtime.path_blocked = true;
        entity.navigation.path_runtime.start_blocked(0, 60);
    }

    let mut lifecycle_requests = Vec::new();
    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    for native_frame in 0..4 {
        tick_movement_with_grid(
            &mut entities,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut occupancy,
            &mut rng,
            native_frame,
            &mut interner,
            &mut lifecycle_requests,
        );
    }

    let entity = entities.get(1).expect("mover");
    assert!(entity.movement_target.is_some(), "mover still moving");
    assert!(
        !entity.navigation.path_runtime.path_blocked,
        "a vehicle that paid a track point must have its impatience flag cleared"
    );
}

#[test]
fn test_tick_movement_no_stacking_same_target_cell() {
    let mut entities = EntityStore::new();

    let mut e1 = GameEntity::test_default(1, "HTNK", "Americans", 1, 1);
    e1.movement_target = Some(MovementTarget {
        path: vec![(1, 1), (2, 1)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        speed: SimFixed::from_num(1024), // 4 cells/sec in leptons.
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });
    e1.facing = 64;
    entities.insert(e1);

    let mut e2 = GameEntity::test_default(2, "HTNK", "Americans", 1, 2);
    e2.movement_target = Some(MovementTarget {
        path: vec![(1, 2), (2, 1)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        // Equalize the diagonal component with e1 so both reach the boundary
        // on the same native frame and the live processing order decides.
        speed: SimFixed::from_num(1448),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SimFixed::from_num(-256),
        move_dir_len: SimFixed::from_num(362), // ~sqrt(256^2 + 256^2)
        ..Default::default()
    });
    e2.facing = 64;
    entities.insert(e2);

    let mut lifecycle_requests = Vec::new();
    let mut occupancy = OccupancyGrid::new();
    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    for native_frame in 0..2 {
        tick_movement_with_grid(
            &mut entities,
            None,
            &Default::default(),
            &Default::default(),
            &mut occupancy,
            &mut rng,
            native_frame,
            &mut interner,
            &mut lifecycle_requests,
        );
    }

    let ent1 = entities.get(1).expect("e1 exists");
    let ent2 = entities.get(2).expect("e2 exists");
    assert_eq!(
        (ent1.position.rx, ent1.position.ry),
        (2, 1),
        "first mover should claim destination"
    );
    assert_eq!(
        (ent2.position.rx, ent2.position.ry),
        (1, 2),
        "second mover should stay blocked"
    );
}

fn contested_same_cell_sim() -> crate::sim::world::Simulation {
    let mut sim = crate::sim::world::Simulation::new();
    let owner = sim.intern("Americans");
    let type_ref = sim.intern("HTNK");

    let mut e1 = GameEntity::test_default(1, "HTNK", "Americans", 1, 1);
    e1.owner = owner;
    e1.type_ref = type_ref;
    e1.movement_target = Some(MovementTarget {
        path: vec![(1, 1), (2, 1)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        speed: SimFixed::from_num(1024),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });
    e1.facing = 64;
    sim.substrate.entities.insert(e1);

    let mut e2 = GameEntity::test_default(2, "HTNK", "Americans", 1, 2);
    e2.owner = owner;
    e2.type_ref = type_ref;
    e2.movement_target = Some(MovementTarget {
        path: vec![(1, 2), (2, 1)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        // Equal diagonal travel makes both movers reach (2,1) on frame two.
        speed: SimFixed::from_num(1448),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SimFixed::from_num(-256),
        move_dir_len: SimFixed::from_num(362),
        ..Default::default()
    });
    e2.facing = 64;
    sim.substrate.entities.insert(e2);

    sim.reveal(2);
    sim.reveal(1);
    sim.substrate.occupancy = OccupancyGrid::rebuild(&sim.substrate.entities);
    sim
}

#[test]
fn two_movers_contest_same_cell_in_live_object_order_not_stable_id() {
    let mut stable_order = contested_same_cell_sim();
    let mut live_order = contested_same_cell_sim();
    assert_eq!(stable_order.state_hash(), live_order.state_hash());

    let terrain_costs = Default::default();
    let mut stable_sounds = Vec::new();
    let mut stable_lifecycle_requests = Vec::new();
    for native_frame in 0..2u32 {
        tick_movement_with_grids(
            &mut stable_order.substrate.entities,
            None,
            None,
            &terrain_costs,
            &Default::default(),
            &mut stable_order.substrate.occupancy,
            &mut stable_order.substrate.cell_occupation,
            &mut stable_order.substrate.raw_cell_occupation,
            &mut stable_order.substrate.next_occupancy_enter_order,
            &mut stable_order.scenario_rng,
            u64::from(native_frame),
            native_frame,
            None,
            None,
            None,
            &crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            SIM_ZERO,
            9,
            60,
            &mut stable_order.interner,
            None,
            &mut stable_sounds,
            &mut stable_lifecycle_requests,
        );
    }

    let movement_order = live_order.live_object_order_snapshot();
    let mut live_sounds = Vec::new();
    let mut live_lifecycle_requests = Vec::new();
    for native_frame in 0..2u32 {
        tick_movement_with_grids(
            &mut live_order.substrate.entities,
            Some(&movement_order),
            None,
            &terrain_costs,
            &Default::default(),
            &mut live_order.substrate.occupancy,
            &mut live_order.substrate.cell_occupation,
            &mut live_order.substrate.raw_cell_occupation,
            &mut live_order.substrate.next_occupancy_enter_order,
            &mut live_order.scenario_rng,
            u64::from(native_frame),
            native_frame,
            None,
            None,
            None,
            &crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            SIM_ZERO,
            9,
            60,
            &mut live_order.interner,
            None,
            &mut live_sounds,
            &mut live_lifecycle_requests,
        );
    }

    assert_eq!(
        (
            stable_order.substrate.entities.get(1).unwrap().position.rx,
            stable_order.substrate.entities.get(1).unwrap().position.ry,
        ),
        (2, 1),
        "stable-id fallback lets id 1 claim the contested cell first"
    );
    assert_eq!(
        (
            live_order.substrate.entities.get(2).unwrap().position.rx,
            live_order.substrate.entities.get(2).unwrap().position.ry,
        ),
        (2, 1),
        "live object order lets id 2 claim the contested cell first"
    );
    assert_ne!(stable_order.state_hash(), live_order.state_hash());
}

#[test]
fn lifecycle_authority_empty_logic_order_does_not_fall_back_to_entity_store() {
    let mut sim = Simulation::new();
    let mut entity = GameEntity::test_default(1, "HTNK", "Americans", 2, 2);
    entity.movement_target = Some(MovementTarget {
        path: vec![(2, 2), (3, 2)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        speed: SimFixed::from_num(512),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });
    sim.substrate.entities.insert(entity);
    let before = sim
        .substrate
        .entities
        .get(1)
        .map(|entity| {
            (
                entity.position.rx,
                entity.position.ry,
                entity.position.z,
                entity.position.sub_x,
                entity.position.sub_y,
            )
        })
        .unwrap();
    let mut sounds = Vec::new();
    let mut lifecycle_requests = Vec::new();

    let stats = tick_movement_with_grids(
        &mut sim.substrate.entities,
        Some(&[]),
        None,
        &Default::default(),
        &Default::default(),
        &mut sim.substrate.occupancy,
        &mut sim.substrate.cell_occupation,
        &mut sim.substrate.raw_cell_occupation,
        &mut sim.substrate.next_occupancy_enter_order,
        &mut sim.scenario_rng,
        0,
        0,
        None,
        None,
        None,
        &crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        SIM_ZERO,
        9,
        60,
        &mut sim.interner,
        None,
        &mut sounds,
        &mut lifecycle_requests,
    );

    assert_eq!(stats.movers_total, 0);
    assert_eq!(stats.moved_steps, 0);
    assert_eq!(stats.blocked_attempts, 0);
    assert_eq!(stats.crush_kills, 0);
    let after = sim
        .substrate
        .entities
        .get(1)
        .map(|entity| {
            (
                entity.position.rx,
                entity.position.ry,
                entity.position.z,
                entity.position.sub_x,
                entity.position.sub_y,
            )
        })
        .unwrap();
    assert_eq!(after, before);
    assert!(lifecycle_requests.is_empty());
}

#[test]
fn test_repath_cooldown_prevents_thrashing_on_unrecoverable_block() {
    let mut entities = EntityStore::new();
    let mut grid: PathGrid = PathGrid::new(8, 8);

    let e = GameEntity::test_default(1, "HTNK", "Americans", 1, 1);
    entities.insert(e);

    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (5, 1),
        SimFixed::from_num(1024),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    // Make the route truly unreachable — block the entire column 2 so no
    // detour exists. (The previous 3-cell block left rows 3-7 open.)
    for y in 0..8u16 {
        grid.set_blocked(2, y, true);
    }

    // Under binary-faithful semantics, a terrain/impassable block (gamemd
    // code-7) does NOT spend the blocked_delay grace period — the unit
    // goes straight to urgency=2. path_stuck_counter decrements once per
    // urgency=2 failure. With path_stuck_init=10 and code-7 skipping
    // grace, all 10 retries fire on consecutive ticks and the unit
    // aborts (movement_target removed) within ~10 ticks.
    //
    // Run 30 ticks — well past the abort window. Verify the movement
    // target is GONE (thrashing prevented by hard abort, not by waiting).
    let mut lifecycle_requests = Vec::new();
    for _ in 0..30 {
        tick_movement_with_grid(
            &mut entities,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut OccupancyGrid::new(),
            &mut SimRng::new(0),
            0,
            &mut test_interner(),
            &mut lifecycle_requests,
        );
    }
    let entity = entities.get(1).expect("entity exists");
    assert!(
        entity.movement_target.is_none(),
        "movement target should be aborted after path_stuck_counter exhaustion (unrecoverable code-7 block)",
    );
}

#[test]
fn test_dynamic_occupancy_repath_routes_around_stationary_blocker() {
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(10, 10);

    // Stationary blocker at (3,4). Different owner so bump doesn't apply.
    let mut blocker = GameEntity::test_default(1, "HTNK", "Soviet", 3, 4);
    blocker.lifecycle.in_limbo = false;
    blocker.lifecycle.cell_marked = true;
    entities.insert(blocker);

    let mut mover = GameEntity::test_default(2, "HTNK", "Americans", 1, 4);
    mover.lifecycle.in_limbo = false;
    mover.lifecycle.cell_marked = true;
    entities.insert(mover);

    assert!(issue_move_command(
        &mut entities,
        &grid,
        2,
        (7, 4),
        SimFixed::from_num(1024),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    // With blockage_path_delay_ticks=60, the mover must wait ~60 ticks after
    // hitting the occupied cell before a repath is attempted. After repath
    // succeeds, it needs additional ticks to travel the detour to (7,4).
    let mut occupancy = OccupancyGrid::rebuild(&entities);
    let mut saw_repath_success = false;
    let mut lifecycle_requests = Vec::new();
    for _ in 0..80 {
        let stats = tick_movement_with_grid(
            &mut entities,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut occupancy,
            &mut SimRng::new(0),
            0,
            &mut test_interner(),
            &mut lifecycle_requests,
        );
        if stats.repath_successes > 0 {
            saw_repath_success = true;
        }
    }

    let entity = entities.get(2).expect("mover should still exist");
    assert_eq!(
        (entity.position.rx, entity.position.ry),
        (7, 4),
        "Mover should reach destination by routing around occupied cell"
    );
    assert!(
        saw_repath_success,
        "Should perform at least one dynamic repath"
    );
}

#[test]
fn test_stuck_recovery_clears_unreachable_movement_target() {
    let mut entities = EntityStore::new();
    let mut grid: PathGrid = PathGrid::new(7, 7);
    for y in 0..7 {
        for x in 0..7 {
            if y != 3 {
                grid.set_blocked(x, y, true);
            }
        }
    }

    // Stationary building at (3,3). Buildings hard-block in entity_blocks BTreeSet.
    let mut blocker = GameEntity::test_default(1, "GAWALL", "Soviet", 3, 3);
    blocker.category = EntityCategory::Structure;
    blocker.lifecycle.in_limbo = false;
    blocker.lifecycle.cell_marked = true;
    entities.insert(blocker);

    let mut mover = GameEntity::test_default(2, "HTNK", "Americans", 1, 3);
    mover.lifecycle.in_limbo = false;
    mover.lifecycle.cell_marked = true;
    entities.insert(mover);

    assert!(issue_move_command(
        &mut entities,
        &grid,
        2,
        (5, 3),
        SimFixed::from_num(1024),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    // A blocked retry uses elapsed binary frames. Repeated Process calls at
    // frame0 must not expire either timer, even after hundreds of calls.
    // Advance time while retaining the existing recovery bound/assertions.
    let mut occupancy = OccupancyGrid::rebuild(&entities);
    let mut recovered = false;
    let mut lifecycle_requests = Vec::new();
    for frame in 0..700 {
        let stats = tick_movement_with_grid(
            &mut entities,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut occupancy,
            &mut SimRng::new(0),
            frame,
            &mut test_interner(),
            &mut lifecycle_requests,
        );
        if stats.stuck_recoveries > 0 {
            recovered = true;
            break;
        }
    }

    assert!(
        recovered,
        "Stuck recovery should trigger for permanent deadlock"
    );
    let entity = entities.get(2).expect("mover exists");
    assert!(
        entity.movement_target.is_none(),
        "MovementTarget should be removed after stuck recovery"
    );
    assert_ne!(
        (entity.position.rx, entity.position.ry),
        (5, 3),
        "Stuck recovery should stop before unreachable destination"
    );
}

#[test]
fn test_movement_tick_stats_report_blocked_attempts() {
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(8, 8);

    // Stationary blocker at (2,2) owned by a different house so bump won't trigger.
    let mut blocker = GameEntity::test_default(1, "HTNK", "Soviets", 2, 2);
    blocker.lifecycle.in_limbo = false;
    blocker.lifecycle.cell_marked = true;
    entities.insert(blocker);

    let mut mover = GameEntity::test_default(2, "HTNK", "Americans", 1, 2);
    mover.lifecycle.in_limbo = false;
    mover.lifecycle.cell_marked = true;
    entities.insert(mover);

    assert!(issue_move_command(
        &mut entities,
        &grid,
        2,
        (4, 2),
        SimFixed::from_num(1024),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    let mut occupancy = OccupancyGrid::rebuild(&entities);
    let mut lifecycle_requests = Vec::new();
    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    let _ = tick_movement_with_grid(
        &mut entities,
        Some(&grid),
        &Default::default(),
        &Default::default(),
        &mut occupancy,
        &mut rng,
        0,
        &mut interner,
        &mut lifecycle_requests,
    );
    let stats = tick_movement_with_grid(
        &mut entities,
        Some(&grid),
        &Default::default(),
        &Default::default(),
        &mut occupancy,
        &mut rng,
        1,
        &mut interner,
        &mut lifecycle_requests,
    );
    assert_eq!(stats.movers_total, 1);
    assert_eq!(stats.moved_steps, 0);
    assert_eq!(stats.blocked_attempts, 1);
}

#[test]
fn lifecycle_authority_crush_emits_one_request_without_store_removal() {
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(8, 8);

    let mut victim = GameEntity::test_default(1, "E1", "Soviets", 2, 2);
    victim.category = EntityCategory::Infantry;
    victim.crushable = true;
    victim.lifecycle.in_limbo = false;
    victim.lifecycle.cell_marked = true;
    victim.mark_live_contact_with(2);
    entities.insert(victim);

    let mut mover = GameEntity::test_default(2, "HTNK", "Americans", 1, 2);
    mover.regular_crusher = true;
    mover.lifecycle.in_limbo = false;
    mover.lifecycle.cell_marked = true;
    mover.movement_target = Some(MovementTarget {
        path: vec![(1, 2), (2, 2)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        speed: SimFixed::from_num(1024),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        final_goal: Some((2, 2)),
        ..Default::default()
    });
    mover.mark_live_contact_with(1);
    entities.insert(mover);

    let mut total_crush_kills = 0;
    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    let mut lifecycle_requests = Vec::new();
    for tick in 0..16 {
        let mut occupancy = OccupancyGrid::rebuild(&entities);
        let stats = tick_movement_with_grid(
            &mut entities,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut occupancy,
            &mut rng,
            tick,
            &mut interner,
            &mut lifecycle_requests,
        );
        total_crush_kills += stats.crush_kills;
        if !lifecycle_requests.is_empty() {
            break;
        }
    }

    assert_eq!(total_crush_kills, 1);
    assert_eq!(
        lifecycle_requests,
        vec![LifecycleRequest::Uninit {
            stable_id: 1,
            reason: UninitReason::Crush,
        }]
    );
    let victim = entities
        .get(1)
        .expect("movement must leave teardown to lifecycle authority");
    assert_eq!(victim.health.current, 0);
    assert!(
        entities.get(2).unwrap().has_live_contact_with(1),
        "movement must not bypass ordered BREAK/UnInit contact cleanup"
    );
}

#[test]
fn lifecycle_authority_crushed_victim_skips_all_remaining_movement_postpasses() {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::movement::locomotor::GroundMovePhase;

    let mut entities = EntityStore::new();
    let grid = PathGrid::new(8, 8);

    // Crusher ID 1 runs first in the test wrapper's stable-id order. Victim ID
    // 2 therefore proves a queued crush victim is skipped later in the same pass.
    let mut victim = GameEntity::test_default(2, "E1", "Soviets", 2, 2);
    victim.category = EntityCategory::Infantry;
    victim.crushable = true;
    victim.lifecycle.in_limbo = false;
    victim.lifecycle.cell_marked = true;
    let mut victim_locomotor = LocomotorState::for_test_kind(LocomotorKind::Hover);
    victim_locomotor.phase = GroundMovePhase::Accelerating;
    victim.locomotor = Some(victim_locomotor);
    victim.movement_target = Some(MovementTarget {
        path: vec![(2, 2)],
        path_layers: vec![MovementLayer::Ground],
        next_index: 1,
        ..Default::default()
    });
    entities.insert(victim);

    let mut crusher = GameEntity::test_default(1, "HTNK", "Americans", 1, 2);
    // Start just before the cell edge so the native-frame visit queues the
    // crush before victim ID 2 reaches any remaining movement postpass.
    crusher.position.sub_x = SimFixed::from_num(240);
    crusher.regular_crusher = true;
    crusher.lifecycle.in_limbo = false;
    crusher.lifecycle.cell_marked = true;
    crusher.movement_target = Some(MovementTarget {
        path: vec![(1, 2), (2, 2)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        speed: SimFixed::from_num(1024),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        final_goal: Some((2, 2)),
        ..Default::default()
    });
    entities.insert(crusher);

    let mut occupancy = OccupancyGrid::rebuild(&entities);
    let mut lifecycle_requests = Vec::new();
    let stats = tick_movement_with_grid(
        &mut entities,
        Some(&grid),
        &Default::default(),
        &Default::default(),
        &mut occupancy,
        &mut SimRng::new(0),
        0,
        &mut test_interner(),
        &mut lifecycle_requests,
    );

    assert_eq!(stats.crush_kills, 1);
    assert_eq!(
        lifecycle_requests,
        vec![LifecycleRequest::Uninit {
            stable_id: 2,
            reason: UninitReason::Crush,
        }]
    );
    let victim = entities
        .get(2)
        .expect("crush request must leave the victim resolvable for UnInit");
    assert_eq!(victim.health.current, 0);
    assert!(!victim.lifecycle.cell_marked);
    assert!(
        victim.movement_target.is_some(),
        "finished-target cleanup must skip a queued crush victim"
    );
    let locomotor = victim.locomotor.as_ref().expect("hover locomotor");
    assert_eq!(
        locomotor.phase,
        GroundMovePhase::Accelerating,
        "phase postpass must not mutate a queued crush victim"
    );
    assert_eq!(
        locomotor.altitude, SIM_ZERO,
        "hover vertical postpass must not mutate a queued crush victim"
    );
    assert_eq!(locomotor.hover_bob_offset, SIM_ZERO);
}

#[test]
fn test_friendly_scatter_issues_move_command() {
    // A friendly stationary blocker should receive a scatter movement
    // command — the blocker walks away instead of being teleported.
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(8, 8);

    // Stationary friendly blocker at (2,2).
    let mut blocker = GameEntity::test_default(1, "HTNK", "Americans", 2, 2);
    blocker.locomotor = Some(locomotor::LocomotorState::for_test_kind(
        crate::rules::locomotor_type::LocomotorKind::Drive,
    ));
    blocker.lifecycle.in_limbo = false;
    blocker.lifecycle.cell_marked = true;
    entities.insert(blocker);

    let mut mover = GameEntity::test_default(2, "HTNK", "Americans", 1, 2);
    mover.lifecycle.in_limbo = false;
    mover.lifecycle.cell_marked = true;
    entities.insert(mover);

    assert!(issue_move_command(
        &mut entities,
        &grid,
        2,
        (4, 2),
        SimFixed::from_num(1024),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    let mut occupancy = OccupancyGrid::rebuild(&entities);
    let mut lifecycle_requests = Vec::new();
    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    let _ = tick_movement_with_grid(
        &mut entities,
        Some(&grid),
        &Default::default(),
        &Default::default(),
        &mut occupancy,
        &mut rng,
        0,
        &mut interner,
        &mut lifecycle_requests,
    );
    let stats = tick_movement_with_grid(
        &mut entities,
        Some(&grid),
        &Default::default(),
        &Default::default(),
        &mut occupancy,
        &mut rng,
        1,
        &mut interner,
        &mut lifecycle_requests,
    );
    assert_eq!(stats.movers_total, 1);
    // Scatter succeeded: blocker was given a movement command.
    assert_eq!(stats.scatter_successes, 1);
    // Blocker should still be at (2,2) but now has a movement_target
    // (it walks away on subsequent ticks, not teleported).
    let bl = entities.get(1).expect("blocker exists");
    assert!(
        bl.movement_target.is_some(),
        "Blocker should have a scatter movement command"
    );
    assert_eq!(
        (bl.position.rx, bl.position.ry),
        (2, 2),
        "Blocker position unchanged this tick — walks next tick"
    );
}

// --- Friendly-passable pathfinding tests ---

#[test]
fn test_friendly_passable_moving_unit_not_blocked() {
    // A moving friendly unit should NOT appear in the entity block set.
    use crate::map::houses::HouseAllianceMap;
    use crate::sim::movement::bump_crush;

    let mut entities = EntityStore::new();
    let _grid = PathGrid::new(10, 10);

    // Unit A: stationary friendly at (3, 0).
    let mut a = GameEntity::test_default(1, "HTNK", "Americans", 3, 0);
    a.lifecycle.in_limbo = false;
    a.lifecycle.cell_marked = true;
    entities.insert(a);

    // Unit B: moving friendly at (4, 0) — has a movement target.
    let mut b = GameEntity::test_default(2, "HTNK", "Americans", 4, 0);
    b.lifecycle.in_limbo = false;
    b.lifecycle.cell_marked = true;
    b.movement_target = Some(MovementTarget {
        path: vec![(4, 0), (5, 0), (6, 0)],
        path_layers: vec![MovementLayer::Ground; 3],
        next_index: 1,
        speed: SimFixed::from_num(1024),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });
    entities.insert(b);

    let alliances = HouseAllianceMap::new();
    let (blocks, _penalty) = bump_crush::build_entity_block_set(
        &entities,
        "Americans",
        &alliances,
        &mut test_interner(),
        None,
    );

    // Stationary friendly at (3,0) is now soft-blocked (code 6, cost 8x) in
    // entity_block_map, not in the hard-block BTreeSet.
    assert!(
        !blocks.contains(&(3, 0)),
        "Stationary friendly should be soft-blocked, not hard-blocked"
    );
    assert!(
        _penalty.contains_key(
            crate::sim::movement::locomotor::MovementLayer::Ground,
            &(3, 0)
        ),
        "Stationary friendly should be in entity_block_map"
    );
    assert_eq!(
        _penalty
            .get(
                crate::sim::movement::locomotor::MovementLayer::Ground,
                &(3, 0)
            )
            .expect("ground stationary friendly soft blocker")
            .cost_code,
        6,
        "Stationary friendly should have cost_code 6"
    );
    // Moving friendly at (4,0) should be in entity_block_map with code 2.
    assert!(
        !blocks.contains(&(4, 0)),
        "Moving friendly should be passable"
    );
    assert!(
        _penalty.contains_key(
            crate::sim::movement::locomotor::MovementLayer::Ground,
            &(4, 0)
        ),
        "Moving friendly should be in entity_block_map"
    );
    assert_eq!(
        _penalty
            .get(
                crate::sim::movement::locomotor::MovementLayer::Ground,
                &(4, 0)
            )
            .expect("ground moving friendly soft blocker")
            .cost_code,
        2,
        "Moving friendly should have cost_code 2"
    );
}

#[test]
fn test_enemy_unit_always_blocks_even_when_moving() {
    use crate::map::houses::HouseAllianceMap;
    use crate::sim::movement::bump_crush;

    let mut entities = EntityStore::new();

    // Enemy unit moving at (3, 0).
    let mut enemy = GameEntity::test_default(1, "HTNK", "Russians", 3, 0);
    enemy.lifecycle.in_limbo = false;
    enemy.lifecycle.cell_marked = true;
    enemy.movement_target = Some(MovementTarget {
        path: vec![(3, 0), (4, 0)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        speed: SimFixed::from_num(1024),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });
    entities.insert(enemy);

    let alliances = HouseAllianceMap::new();
    let (blocks, _penalty) = bump_crush::build_entity_block_set(
        &entities,
        "Americans",
        &alliances,
        &mut test_interner(),
        None,
    );

    // Enemy at (3,0) is now soft-blocked (code 5, cost 20x) in entity_block_map,
    // not in the hard-block BTreeSet.
    assert!(
        !blocks.contains(&(3, 0)),
        "Enemy should be soft-blocked, not hard-blocked"
    );
    assert!(
        _penalty.contains_key(
            crate::sim::movement::locomotor::MovementLayer::Ground,
            &(3, 0)
        ),
        "Enemy should be in entity_block_map"
    );
    assert_eq!(
        _penalty
            .get(
                crate::sim::movement::locomotor::MovementLayer::Ground,
                &(3, 0)
            )
            .expect("ground enemy soft blocker")
            .cost_code,
        5,
        "Enemy should have cost_code 5"
    );
}

#[test]
fn test_friendly_passable_path_goes_through_moving_friendly() {
    // Unit should be able to pathfind THROUGH a moving friendly's cell.
    use crate::sim::pathfinding::find_path_with_costs;
    use std::collections::BTreeSet;

    let grid = PathGrid::new(10, 3);
    // Only block (3,1) — force path through row 0.
    let mut blocks: BTreeSet<(u16, u16)> = BTreeSet::new();
    // (3,0) has a moving friendly — NOT in blocks.
    // (3,1) is a stationary friendly — in blocks.
    blocks.insert((3, 1));

    let path = find_path_with_costs(
        &grid,
        (0, 0),
        (6, 0),
        None,
        Some(&blocks),
        None,
        None,
        None,
        0,
        false,
        false,
    );
    assert!(
        path.is_some(),
        "Should find path through moving-friendly cell"
    );
    let path = path.unwrap();
    // Path can go through (3,0) since it's not blocked (moving friendly).
    assert_eq!(path.last(), Some(&(6, 0)));
}

// --- 24-step path segmentation tests ---

#[test]
fn test_short_path_no_truncation() {
    // A 5-step path (well under 24) should be delivered intact.
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(32, 32);

    let e = GameEntity::test_default(1, "HTNK", "Americans", 0, 0);
    entities.insert(e);

    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (5, 0),
        SimFixed::from_num(1024),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    let entity = entities.get(1).expect("entity exists");
    let target = entity.movement_target.as_ref().expect("has target");
    assert_eq!(
        target.path.len(),
        6,
        "5-step path = 6 entries (start + 5 moves)"
    );
    assert_eq!(target.final_goal, Some((5, 0)));
}

#[test]
fn test_long_path_truncated_to_24_steps() {
    // A path longer than 24 steps should be truncated to 25 entries.
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(50, 1);

    let e = GameEntity::test_default(1, "HTNK", "Americans", 0, 0);
    entities.insert(e);

    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (40, 0),
        SimFixed::from_num(1024),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    let entity = entities.get(1).expect("entity exists");
    let target = entity.movement_target.as_ref().expect("has target");
    // Path truncated: 24 steps + start = 25 entries.
    assert_eq!(
        target.path.len(),
        25,
        "Long path should be truncated to 25 entries"
    );
    assert_eq!(target.path[0], (0, 0), "Path starts at origin");
    assert_eq!(target.path[24], (24, 0), "Path ends at 24th step");
    assert_eq!(target.final_goal, Some((40, 0)), "Final goal preserved");
}

#[test]
fn test_segment_exhaustion_triggers_auto_repath() {
    // Walk a truncated 24-step segment, verify auto-repath continues to final goal.
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(50, 1);

    let e = GameEntity::test_default(1, "HTNK", "Americans", 0, 0);
    entities.insert(e);

    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (30, 0),
        SimFixed::from_num(15360), // Very fast — finishes segment quickly.
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    // Tick enough times to exhaust the first 24-step segment and auto-repath.
    let mut lifecycle_requests = Vec::new();
    for _ in 0..30 {
        tick_movement_with_grid(
            &mut entities,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut OccupancyGrid::new(),
            &mut SimRng::new(0),
            0,
            &mut test_interner(),
            &mut lifecycle_requests,
        );
    }

    let entity = entities.get(1).expect("entity exists");
    assert_eq!(
        (entity.position.rx, entity.position.ry),
        (30, 0),
        "Entity should reach final destination via auto-repath"
    );
    assert!(
        entity.movement_target.is_none(),
        "Movement should be complete"
    );
}

#[test]
fn test_exact_24_step_path_no_repath_needed() {
    // A path of exactly 24 steps should complete without needing auto-repath.
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(50, 1);

    let e = GameEntity::test_default(1, "HTNK", "Americans", 0, 0);
    entities.insert(e);

    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (24, 0),
        SimFixed::from_num(15360),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    let entity = entities.get(1).expect("entity exists");
    let target = entity.movement_target.as_ref().expect("has target");
    assert_eq!(target.path.len(), 25, "24-step path = 25 entries");

    // Walk the full path.
    let mut lifecycle_requests = Vec::new();
    for _ in 0..20 {
        tick_movement_with_grid(
            &mut entities,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut OccupancyGrid::new(),
            &mut SimRng::new(0),
            0,
            &mut test_interner(),
            &mut lifecycle_requests,
        );
    }

    let entity = entities.get(1).expect("entity exists");
    assert_eq!(
        (entity.position.rx, entity.position.ry),
        (24, 0),
        "Should reach destination"
    );
    assert!(entity.movement_target.is_none(), "Movement should be done");
}

#[test]
fn test_auto_repath_fails_entity_stops() {
    // If auto-repath fails (goal unreachable after segment), entity should stop.
    let mut entities = EntityStore::new();
    let mut grid: PathGrid = PathGrid::new(50, 3);

    let e = GameEntity::test_default(1, "HTNK", "Americans", 0, 1);
    entities.insert(e);

    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (40, 1),
        SimFixed::from_num(15360),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    // After the path is issued, block column 25 completely so repath fails.
    for y in 0..3 {
        grid.set_blocked(25, y, true);
    }

    // Tick enough to exhaust the first segment (reaches cell 24) and attempt repath.
    let mut lifecycle_requests = Vec::new();
    for _ in 0..30 {
        tick_movement_with_grid(
            &mut entities,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut OccupancyGrid::new(),
            &mut SimRng::new(0),
            0,
            &mut test_interner(),
            &mut lifecycle_requests,
        );
    }

    let entity = entities.get(1).expect("entity exists");
    // Entity should have stopped — either at segment end or earlier.
    assert!(
        entity.movement_target.is_none(),
        "Movement should be cleared when repath fails"
    );
    assert!(
        entity.position.rx <= 24,
        "Entity should not pass the blocked column"
    );
}

#[test]
fn test_blocked_repath_uses_final_goal_not_segment_end() {
    // When blocked mid-segment, repath should target final_goal, not segment end.
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(50, 5);

    let e = GameEntity::test_default(1, "HTNK", "Americans", 0, 2);
    entities.insert(e);

    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (40, 2),
        SimFixed::from_num(1024),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    let entity = entities.get(1).expect("entity exists");
    let target = entity.movement_target.as_ref().expect("has target");
    assert_eq!(target.final_goal, Some((40, 2)));
    // The segment path ends at (24, 2), but final_goal is (40, 2).
    assert_eq!(target.path.last(), Some(&(24, 2)));
}

/// Build a minimal Drive LocomotorState for layered-pathfinding tests. Required
/// because the layered A* branch in find_move_path is only entered when the
/// mover has a Drive/Walk/Mech locomotor; `test_default` leaves locomotor=None.
fn make_drive_loco_for_test() -> crate::sim::movement::locomotor::LocomotorState {
    use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
    use crate::sim::movement::locomotor::{GroundMovePhase, LocomotorState, MovementLayer};
    use crate::util::fixed_math::SIM_ONE;
    LocomotorState {
        kind: LocomotorKind::Drive,
        slot: LocomotorSlot::from_kind(LocomotorKind::Drive),
        powered: true,
        piggyback: None,
        runtime_payload: crate::sim::movement::locomotion::LocomotorRuntimePayload::for_kind(
            LocomotorKind::Drive,
            0,
        ),
        layer: MovementLayer::Ground,
        phase: GroundMovePhase::Idle,

        speed_multiplier: SIM_ONE,
        speed_fraction: SIM_ONE,
        fly_current_speed: SIM_ZERO,
        altitude: SIM_ZERO,

        jumpjet_speed: SIM_ZERO,
        jumpjet_accel: SIM_ZERO,
        jumpjet_current_speed: SIM_ZERO,
        jumpjet_deviation: 0,
        jumpjet_crash_speed: SIM_ZERO,
        jumpjet_turn_rate: 0,
        balloon_hover: false,
        hover_attack: false,
        speed_type: SpeedType::Track,
        movement_zone: MovementZone::Normal,
        rot: 0,
        air_progress: SIM_ZERO,
        infantry_wobble_phase: 0.0,
        subcell_dest: None,
        hover_throttle: crate::util::fixed_math::SIM_ZERO,
        hover_speed_request: crate::util::fixed_math::SIM_ZERO,
        hover_bob_offset: crate::util::fixed_math::SIM_ZERO,
    }
}

fn drive_speed_test_cell(
    rx: u16,
    ry: u16,
    speed_costs: crate::rules::terrain_rules::SpeedCostProfile,
) -> crate::map::resolved_terrain::ResolvedTerrainCell {
    crate::map::resolved_terrain::ResolvedTerrainCell {
        rx,
        ry,
        source_tile_index: 0,
        source_sub_tile: 0,
        final_tile_index: 0,
        final_sub_tile: 0,
        is_wood_bridge_repair_tile: false,
        level: 0,
        filled_clear: false,
        tileset_index: Some(0),
        land_type: 0,
        yr_cell_land_type: 0,
        slope_type: 0,
        template_height: 0,
        render_offset_x: 0,
        render_offset_y: 0,
        terrain_class: crate::rules::terrain_rules::TerrainClass::Clear,
        speed_costs,
        is_water: false,
        is_cliff_like: false,
        is_rough: false,
        is_road: false,
        accepts_smudge: false,
        allows_tiberium: false,
        height_in_pixels: 0,
        variant: 0,
        has_ramp: false,
        canonical_ramp: None,
        ground_walk_blocked: false,
        terrain_object_blocks: false,
        terrain_object_occupation: None,
        overlay_blocks: false,
        overlay_zone_type: None,
        outside_playfield: false,
        zone_type: 0,
        base_ground_walk_blocked: false,
        base_build_blocked: false,
        base_land_type: 0,
        base_yr_cell_land_type: 0,
        base_terrain_class: Default::default(),
        base_speed_costs: Default::default(),
        build_blocked: false,
        has_bridge_deck: false,
        bridge_walkable: false,
        bridge_transition: false,
        bridge_deck_level: 0,
        bridge_layer: None,
        bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
        tube_index: None,
        radar_left: [0, 0, 0],
        radar_right: [0, 0, 0],
        has_damaged_data: false,
        bridgehead_anchor_class_at_load: None,
    }
}

/// A cell holding a terrain object whose INI occupation bits are `bits`.
/// Resolved terrain folds the object into `ground_walk_blocked`;
/// `base_ground_walk_blocked` is the same cell without it.
fn tree_speed_test_cell(
    rx: u16,
    ry: u16,
    bits: u8,
) -> crate::map::resolved_terrain::ResolvedTerrainCell {
    crate::map::resolved_terrain::ResolvedTerrainCell {
        terrain_object_occupation: Some(bits),
        terrain_object_blocks: bits != 0,
        ground_walk_blocked: bits != 0,
        base_ground_walk_blocked: false,
        ..drive_speed_test_cell(rx, ry, Default::default())
    }
}

/// A 3x1 corridor whose only middle cell is a terrain object with a single
/// occupation bit — the shape of 56 of the 60 stock temperate terrain types.
fn tree_corridor() -> (crate::map::resolved_terrain::ResolvedTerrainGrid, PathGrid) {
    let terrain = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
        3,
        1,
        vec![
            drive_speed_test_cell(0, 0, Default::default()),
            tree_speed_test_cell(1, 0, 4),
            drive_speed_test_cell(2, 0, Default::default()),
        ],
    );
    let grid = PathGrid::from_resolved_terrain(&terrain);
    (terrain, grid)
}

/// Order one infantryman from (0,0) to (2,0) on a 3x1 corridor and tick until
/// he settles. Returns the distinct cells he stood in, in order.
fn walk_infantry_corridor(grid: &PathGrid) -> (Vec<(u16, u16)>, Vec<(u16, u16)>) {
    let mut entities = EntityStore::new();
    let mut walker = GameEntity::test_default(1, "E1", "Americans", 0, 0);
    walker.category = EntityCategory::Infantry;
    walker.lifecycle.in_limbo = false;
    walker.lifecycle.cell_marked = true;
    // A live infantryman always stands in a functional sub-cell; without one he
    // registers as a whole-cell blocker and the arrival claim refuses him.
    walker.sub_cell = Some(2);
    let mut loco = LocomotorState::for_test_kind(LocomotorKind::Walk);
    loco.speed_type = SpeedType::Foot;
    walker.locomotor = Some(loco);
    entities.insert(walker);

    assert!(
        issue_move_command(
            &mut entities,
            grid,
            1,
            (2, 0),
            SimFixed::from_num(1024),
            false,
            None,
            None,
            None,
            crate::sim::movement::DestinationTiming::new(0, 60),
        ),
        "infantry must get a route down the corridor",
    );
    let planned = entities
        .get(1)
        .and_then(|e| e.movement_target.as_ref())
        .map(|mt| mt.path.clone())
        .expect("walker has a movement target");

    let mut lifecycle_requests = Vec::new();
    let mut occupancy = OccupancyGrid::new();
    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    let mut visited: Vec<(u16, u16)> = vec![(0, 0)];
    for tick in 0..400u64 {
        tick_movement_with_grid(
            &mut entities,
            Some(grid),
            &Default::default(),
            &Default::default(),
            &mut occupancy,
            &mut rng,
            tick,
            &mut interner,
            &mut lifecycle_requests,
        );
        let e = entities.get(1).expect("walker exists");
        let p = (e.position.rx, e.position.ry);
        if visited.last() != Some(&p) {
            visited.push(p);
        }
    }
    (planned, visited)
}

/// Control: the same corridor with no terrain object at all. This pins the
/// harness itself, so a failure in the tree case below can only be the terrain
/// predicate and not the test setup.
#[test]
fn infantry_walks_a_plain_corridor_end_to_end() {
    let terrain = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
        3,
        1,
        vec![
            drive_speed_test_cell(0, 0, Default::default()),
            drive_speed_test_cell(1, 0, Default::default()),
            drive_speed_test_cell(2, 0, Default::default()),
        ],
    );
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let (planned, visited) = walk_infantry_corridor(&grid);
    assert_eq!(
        visited.last().copied(),
        Some((2, 0)),
        "control walker must cross a corridor with no terrain object. \
         planned={planned:?} visited={visited:?}",
    );
}

/// The search and the runtime step-in have to be ONE predicate, the way the
/// original reaches its cell gate through a single per-class slot.
///
/// A* plans an infantryman straight through a partially-occupied tree cell. If
/// the runtime crossing still asks the whole-cell question, the walker reaches
/// the tree, refuses to enter, repaths onto the identical route — tree cells are
/// terrain, so they never enter the dynamic block set — and block/repath-loops
/// until the stuck counter aborts the order. Every temperate retail map carries
/// hundreds of such cells, so this fires on ordinary infantry movement.
#[test]
fn infantry_traverses_a_partially_occupied_tree_cell_at_runtime() {
    let (_terrain, grid) = tree_corridor();

    // Precondition: this corridor is exactly the split the fix is about.
    assert!(
        !grid.is_walkable(1, 0),
        "the tree closes the cell to the whole-cell view",
    );
    assert!(
        grid.is_walkable_for_infantry(1, 0),
        "one occupation bit leaves sub-cells free for infantry",
    );

    let (planned, visited) = walk_infantry_corridor(&grid);
    assert!(
        visited.contains(&(1, 0)),
        "the walker never entered the tree cell: the runtime step-in refused a \
         cell the search planned. planned={planned:?} visited={visited:?}",
    );
    assert_eq!(
        visited.last().copied(),
        Some((2, 0)),
        "the walker must come out the far side of the tree cell. \
         planned={planned:?} visited={visited:?}",
    );
}

/// The companion half: threading the category must not relax the gate for
/// everyone. A tracked vehicle handed the same corridor as an explicit path —
/// its own search refuses to plan one — must still be stopped by the tree.
#[test]
fn gsi_04_10_crusher_and_omnicrusher_never_enter_or_crush_a_terrain_object_cell() {
    let (_terrain, grid) = tree_corridor();

    for movement_zone in [MovementZone::Crusher, MovementZone::CrusherAll] {
        let mut entities = EntityStore::new();
        let mut tank = GameEntity::test_default(1, "HTNK", "Americans", 0, 0);
        tank.category = EntityCategory::Unit;
        tank.lifecycle.in_limbo = false;
        tank.lifecycle.cell_marked = true;
        let mut locomotor = make_drive_loco_for_test();
        locomotor.movement_zone = movement_zone;
        tank.locomotor = Some(locomotor);
        tank.drive_locomotion = Some(Default::default());
        tank.movement_target = Some(MovementTarget {
            path: vec![(0, 0), (1, 0), (2, 0)],
            path_layers: vec![MovementLayer::Ground; 3],
            next_index: 1,
            speed: SimFixed::from_num(1024),
            current_speed: SimFixed::from_num(1024),
            move_dir_x: SimFixed::from_num(256),
            move_dir_y: SIM_ZERO,
            move_dir_len: SimFixed::from_num(256),
            final_goal: Some((2, 0)),
            ..Default::default()
        });
        entities.insert(tank);

        let mut lifecycle_requests = Vec::new();
        let mut occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(0);
        let mut interner = test_interner();
        for tick in 0..120u64 {
            let stats = tick_movement_with_grid(
                &mut entities,
                Some(&grid),
                &Default::default(),
                &Default::default(),
                &mut occupancy,
                &mut rng,
                tick,
                &mut interner,
                &mut lifecycle_requests,
            );
            assert_eq!(stats.crush_kills, 0);
            assert!(lifecycle_requests.is_empty());
            let p = &entities.get(1).expect("tank exists").position;
            assert_ne!(
                (p.rx, p.ry),
                (1, 0),
                "{movement_zone:?} must never enter a Terrain-object cell",
            );
        }
    }
}

#[test]
fn drive_accelerates_false_tick_stores_modified_fraction_without_mutating_speed() {
    let terrain = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
        2,
        1,
        vec![
            drive_speed_test_cell(0, 0, Default::default()),
            drive_speed_test_cell(
                1,
                0,
                crate::rules::terrain_rules::SpeedCostProfile {
                    track: Some(50),
                    ..Default::default()
                },
            ),
        ],
    );
    let mut entities = EntityStore::new();
    let mut mover = GameEntity::test_default(1, "GTNK", "Americans", 0, 0);
    mover.locomotor = Some(make_drive_loco_for_test());
    mover.drive_locomotion = Some(Default::default());
    mover.drive_accelerates = false;
    mover.movement_target = Some(MovementTarget {
        path: vec![(0, 0), (1, 0)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        speed: SimFixed::from_num(100),
        current_speed: SimFixed::from_num(100),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        final_goal: Some((1, 0)),
        ..Default::default()
    });
    mover.facing = 64;
    mover.lifecycle.in_limbo = false;
    mover.lifecycle.cell_marked = true;
    mover.navigation.path_replay = crate::sim::components::FootPathQueue {
        directions: vec![2],
        cursor: 0,
        reference_cell: Some((0, 0)),
    };
    super::navcom::set_destination_internal_cell(&mut mover, (1, 0), None);
    entities.insert(mover);

    let mut occupancy = OccupancyGrid::new();
    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    let mut sounds = Vec::new();
    let mut next_occupancy_enter_order = crate::sim::world::EnterOrderCounter::new();
    let terrain_costs: std::collections::BTreeMap<
        crate::rules::locomotor_type::SpeedType,
        crate::sim::pathfinding::terrain_cost::TerrainCostGrid,
    > = std::collections::BTreeMap::new();

    let mut lifecycle_requests = Vec::new();
    tick_movement_with_grids(
        &mut entities,
        None,
        None,
        &terrain_costs,
        &Default::default(),
        &mut occupancy,
        &mut crate::sim::occupancy::CellOccupationGrid::new(),
        &mut crate::sim::occupancy::RawCellOccupationGrid::new(),
        &mut next_occupancy_enter_order,
        &mut rng,
        0,
        0, // native_frame (test)
        None,
        Some(&terrain),
        None,
        &crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        SIM_ZERO,
        9,
        60,
        &mut interner,
        None,
        &mut sounds,
        &mut lifecycle_requests,
    );

    let entity = entities.get(1).expect("mover exists");
    let drive = entity.drive_locomotion.as_ref().expect("drive state");
    assert_eq!(drive.target_speed_fraction, SIM_HALF);
    assert_eq!(entity.foot_speed.applied_fraction, SIM_HALF);
    assert_eq!(
        entity.movement_target.as_ref().expect("still moving").speed,
        SimFixed::from_num(100),
        "Drive speed fraction must not mutate raw top speed"
    );
    assert_eq!(
        entity
            .movement_target
            .as_ref()
            .expect("still moving")
            .current_speed,
        SimFixed::from_num(50),
        "Drive current speed should be raw speed scaled by current fraction"
    );
}

#[test]
fn drive_accelerates_true_tick_ramps_fraction_before_movement_speed() {
    let mut entities = EntityStore::new();
    let mut mover = GameEntity::test_default(1, "GTNK", "Americans", 0, 0);
    mover.locomotor = Some(make_drive_loco_for_test());
    mover.drive_locomotion = Some(Default::default());
    mover.drive_accelerates = true;
    mover.movement_target = Some(MovementTarget {
        path: vec![(0, 0), (1, 0)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        speed: SimFixed::from_num(100),
        current_speed: SIM_ZERO,
        accel_factor: SimFixed::lit("0.03"),
        decel_factor: SimFixed::lit("0.002"),
        slowdown_distance: SIM_ZERO,
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        final_goal: Some((1, 0)),
        ..Default::default()
    });
    mover.facing = 64;
    mover.lifecycle.in_limbo = false;
    mover.lifecycle.cell_marked = true;
    mover.navigation.path_replay = crate::sim::components::FootPathQueue {
        directions: vec![2],
        cursor: 0,
        reference_cell: Some((0, 0)),
    };
    super::navcom::set_destination_internal_cell(&mut mover, (1, 0), None);
    entities.insert(mover);

    let mut occupancy = OccupancyGrid::new();
    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    let mut sounds = Vec::new();
    let mut next_occupancy_enter_order = crate::sim::world::EnterOrderCounter::new();
    let terrain_costs: std::collections::BTreeMap<
        crate::rules::locomotor_type::SpeedType,
        crate::sim::pathfinding::terrain_cost::TerrainCostGrid,
    > = std::collections::BTreeMap::new();

    let mut lifecycle_requests = Vec::new();
    tick_movement_with_grids(
        &mut entities,
        None,
        None,
        &terrain_costs,
        &Default::default(),
        &mut occupancy,
        &mut crate::sim::occupancy::CellOccupationGrid::new(),
        &mut crate::sim::occupancy::RawCellOccupationGrid::new(),
        &mut next_occupancy_enter_order,
        &mut rng,
        0,
        0, // native_frame (test)
        None,
        None,
        None,
        &crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        SIM_ZERO,
        9,
        60,
        &mut interner,
        None,
        &mut sounds,
        &mut lifecycle_requests,
    );

    let entity = entities.get(1).expect("mover exists");
    let drive = entity.drive_locomotion.as_ref().expect("drive state");
    assert_eq!(drive.target_speed_fraction, SIM_ONE);
    assert_eq!(entity.foot_speed.applied_fraction, SimFixed::lit("0.03"));
    assert_eq!(
        entity
            .movement_target
            .as_ref()
            .expect("still moving")
            .current_speed,
        SimFixed::from_num(100) * SimFixed::lit("0.03"),
    );
}

#[test]
fn test_initial_layered_path_avoids_friendly_building_footprint() {
    // A friendly Drive-locomotor unit ordered across a 2x2 friendly building
    // foundation must plan a path that does NOT visit any foundation cell on
    // the FIRST attempt — gamemd's Can_Enter_Cell returns code 7 (impassable)
    // for unrelated allied buildings, so the layered A* must hard-block them.
    use crate::sim::production::building_footprint_cells;
    use std::collections::BTreeSet;

    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(15, 15);

    // 2x2 friendly building anchored at (5,5) — covers (5,5), (6,5), (5,6), (6,6).
    let foundation: BTreeSet<(u16, u16)> = building_footprint_cells(5, 5, "2x2", &[], &[])
        .into_iter()
        .collect();
    let mut blocks = BTreeSet::new();
    blocks.extend(foundation.iter().copied());

    // Mover at (1,5), goal at (10,5) — straight east through the foundation.
    let mut mover = GameEntity::test_default(1, "HTNK", "Americans", 1, 5);
    mover.locomotor = Some(make_drive_loco_for_test());
    entities.insert(mover);

    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (10, 5),
        SimFixed::from_num(1024),
        false,         // queue
        None,          // terrain_costs
        Some(&blocks), // entity_blocks
        None,          // entity_block_map
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    let entity = entities.get(1).expect("mover exists");
    let target = entity
        .movement_target
        .as_ref()
        .expect("initial path was planned");

    for &cell in &target.path {
        assert!(
            !foundation.contains(&cell),
            "Initial path visited foundation cell {:?} — layered A* did not see \
             ground_blocks/bridge_blocks on the first plan. Path: {:?}",
            cell,
            target.path,
        );
    }
    assert_eq!(target.path.first().copied(), Some((1, 5)));
    assert_eq!(target.path.last().copied(), Some((10, 5)));
}

#[test]
fn test_queued_drive_reissue_layered_path_avoids_friendly_building_footprint() {
    // Issue an initial Drive move, then a queued player move that crosses a
    // 2x2 friendly building. Drive reissues the destination without using
    // Foot NavQueue, and the replacement path must avoid the foundation.
    use crate::sim::production::building_footprint_cells;
    use std::collections::BTreeSet;

    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(15, 15);

    let foundation: BTreeSet<(u16, u16)> = building_footprint_cells(5, 5, "2x2", &[], &[])
        .into_iter()
        .collect();
    let mut blocks = BTreeSet::new();
    blocks.extend(foundation.iter().copied());

    // Mover at (1,5). First move to (3,5) (no obstacle). The queued Drive
    // command reissues to (10,5), beyond the foundation.
    let mut mover = GameEntity::test_default(1, "HTNK", "Americans", 1, 5);
    mover.locomotor = Some(make_drive_loco_for_test());
    entities.insert(mover);

    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (3, 5),
        SimFixed::from_num(1024),
        false, // queue=false (initial)
        None,
        Some(&blocks),
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (10, 5),
        SimFixed::from_num(1024),
        true, // queue=true (Drive destination reissue)
        None,
        Some(&blocks),
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    let entity = entities.get(1).expect("mover exists");
    let target = entity
        .movement_target
        .as_ref()
        .expect("reissued path exists");

    for &cell in &target.path {
        assert!(
            !foundation.contains(&cell),
            "Queued Drive reissue path visited foundation cell {:?}. Path: {:?}",
            cell,
            target.path,
        );
    }
    assert_eq!(target.path.first().copied(), Some((1, 5)));
    assert_eq!(target.path.last().copied(), Some((10, 5)));
}

#[test]
fn test_segment_exhaustion_repath_avoids_friendly_building_footprint() {
    // A long path with a 2x2 friendly building at cell 30 (beyond the first
    // 24-step segment). The initial segment doesn't see the foundation; the
    // auto-repath at segment exhaustion must avoid it.
    //
    // The auto-repath at movement_tick.rs:166 builds its hard-block set freshly
    // from EntityStore via bump_crush::build_entity_block_set, NOT from the
    // entity_blocks arg passed to issue_move_command. So the foundation must be
    // present as Structure entities in the store. Without rules wired into the
    // test, build_entity_block_set adds the anchor cell of each Structure to
    // mover_entity_blocks, so we insert one Structure per foundation cell.
    use crate::sim::movement::tick_movement_with_grid;
    use crate::sim::production::building_footprint_cells;
    use std::collections::BTreeSet;

    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(45, 5);

    // 2x2 building footprint at (30,2): covers (30,2), (31,2), (30,3), (31,3).
    let foundation: BTreeSet<(u16, u16)> = building_footprint_cells(30, 2, "2x2", &[], &[])
        .into_iter()
        .collect();

    // Insert one Structure entity per foundation cell so build_entity_block_set
    // (called inside tick_movement_with_grid) puts every cell in mover_entity_blocks.
    for (i, &(rx, ry)) in foundation.iter().enumerate() {
        let mut blocker = GameEntity::test_default(100 + i as u64, "GAWALL", "Americans", rx, ry);
        blocker.category = EntityCategory::Structure;
        blocker.lifecycle.in_limbo = false;
        blocker.lifecycle.cell_marked = true;
        entities.insert(blocker);
    }

    let mut mover = GameEntity::test_default(1, "HTNK", "Americans", 1, 2);
    mover.locomotor = Some(make_drive_loco_for_test());
    // No rules in this harness → accel_factor is 0, so an accelerating drive
    // fraction would sit at 0 forever. Accelerates=no snaps to full fraction
    // (real matches always parse a positive acceleration from rules).
    mover.drive_accelerates = false;
    mover.lifecycle.in_limbo = false;
    mover.lifecycle.cell_marked = true;
    entities.insert(mover);

    // entity_blocks=None at command time → initial path goes straight east,
    // truncated to 24 steps (1,2)..(24,2) which doesn't reach the foundation.
    // The post-segment-exhaustion auto-repath is what must route around it.
    assert!(issue_move_command(
        &mut entities,
        &grid,
        1,
        (40, 2),
        SimFixed::from_num(15360), // very fast — exhausts segment quickly
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));

    // Tick until the first segment is exhausted and auto-repath fires. Capture
    // the first path whose first cell is not (1,2) — that is the post-auto-repath
    // segment, planned by the call site this test pins.
    let mut occupancy = OccupancyGrid::rebuild(&entities);
    let mut post_repath_path: Option<Vec<(u16, u16)>> = None;
    let mut lifecycle_requests = Vec::new();
    for _ in 0..40 {
        tick_movement_with_grid(
            &mut entities,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut occupancy,
            &mut SimRng::new(0),
            0,
            &mut test_interner(),
            &mut lifecycle_requests,
        );
        if post_repath_path.is_none() {
            if let Some(t) = entities.get(1).and_then(|e| e.movement_target.as_ref()) {
                if t.path.first().is_some_and(|&c| c != (1, 2)) {
                    post_repath_path = Some(t.path.clone());
                }
            }
        }
    }

    let path = post_repath_path
        .expect("auto-repath at segment exhaustion must fire and produce a new path");
    for &cell in &path {
        assert!(
            !foundation.contains(&cell),
            "Post-segment-exhaustion repath visited foundation cell {:?}. Path: {:?}",
            cell,
            path,
        );
    }
}

// ============================================================================
// Bridge on_bridge timing integration tests (Plan: 2026-05-11 G2 fix).
// Pin: predicate fires at Ramp→Body exactly, clears at Ramp→Ground exactly,
// no anticipatory BridgeOccupancy pre-claim.
// ============================================================================

use crate::map::houses::HouseAllianceMap;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
use crate::sim::components::BridgeOccupancy;
use crate::sim::movement::locomotor::{GroundMovePhase, LocomotorState};
use crate::sim::movement::tick_movement_with_grid;
use crate::sim::pathfinding::{PathGrid, terrain_cost::TerrainCostGrid};
use std::collections::BTreeMap;

fn make_drive_loco(layer: MovementLayer) -> LocomotorState {
    LocomotorState {
        kind: LocomotorKind::Drive,
        slot: LocomotorSlot::from_kind(LocomotorKind::Drive),
        powered: true,
        piggyback: None,
        runtime_payload: crate::sim::movement::locomotion::LocomotorRuntimePayload::for_kind(
            LocomotorKind::Drive,
            0,
        ),
        layer,
        phase: GroundMovePhase::Idle,

        speed_multiplier: SIM_ONE,
        speed_fraction: SIM_ONE,
        fly_current_speed: SIM_ZERO,
        altitude: SIM_ZERO,

        jumpjet_speed: SIM_ZERO,
        jumpjet_accel: SIM_ZERO,
        jumpjet_current_speed: SIM_ZERO,
        jumpjet_deviation: 0,
        jumpjet_crash_speed: SIM_ZERO,
        jumpjet_turn_rate: 4,
        balloon_hover: false,
        hover_attack: false,
        speed_type: SpeedType::Track,
        movement_zone: MovementZone::Normal,
        rot: 0,
        air_progress: SIM_ZERO,
        infantry_wobble_phase: 0.0,
        subcell_dest: None,
        hover_throttle: crate::util::fixed_math::SIM_ZERO,
        hover_speed_request: crate::util::fixed_math::SIM_ZERO,
        hover_bob_offset: crate::util::fixed_math::SIM_ZERO,
    }
}

fn make_ship_loco(layer: MovementLayer) -> LocomotorState {
    let mut loco = make_drive_loco(layer);
    loco.kind = LocomotorKind::Ship;
    loco.slot = LocomotorSlot::from_kind(LocomotorKind::Ship);
    loco.speed_type = SpeedType::Float;
    loco.movement_zone = MovementZone::Water;
    loco
}

fn tick_bridge(
    entities: &mut EntityStore,
    grid: &PathGrid,
    occupancy: &mut OccupancyGrid,
    rng: &mut SimRng,
    interner: &mut crate::sim::intern::StringInterner,
    frames: u32,
    lifecycle_requests: &mut Vec<LifecycleRequest>,
) {
    let costs: BTreeMap<SpeedType, TerrainCostGrid> = BTreeMap::new();
    let alliances = HouseAllianceMap::new();
    for native_frame in 0..frames {
        let _ = tick_movement_with_grid(
            entities,
            Some(grid),
            &costs,
            &alliances,
            occupancy,
            rng,
            u64::from(native_frame),
            interner,
            lifecycle_requests,
        );
    }
}

fn tick_bridge_until_cell(
    entities: &mut EntityStore,
    grid: &PathGrid,
    occupancy: &mut OccupancyGrid,
    rng: &mut SimRng,
    interner: &mut crate::sim::intern::StringInterner,
    target_cell: (u16, u16),
    lifecycle_requests: &mut Vec<LifecycleRequest>,
) {
    for _ in 0..32 {
        let current = entities
            .get(1)
            .map(|entity| (entity.position.rx, entity.position.ry));
        if current == Some(target_cell) {
            return;
        }
        tick_bridge(
            entities,
            grid,
            occupancy,
            rng,
            interner,
            1,
            lifecycle_requests,
        );
    }
    let current = entities
        .get(1)
        .map(|entity| (entity.position.rx, entity.position.ry));
    panic!("entity did not reach {target_cell:?} within 32 native frames; got {current:?}");
}

#[test]
fn ship_high_bridge_ramp_to_body_relinks_after_on_bridge_update() {
    // Body cells are LANE cells: the certified bridge stamping puts the 0x200
    // transition flag on Anchor+Forward1 (the crossable deck lane) of every
    // bridge stamp — transition=false structural cells are the Forward2 edge
    // lane, which is NOT crossable (see bridge_facts.rs stamp slots).
    let mut grid = PathGrid::new(10, 10);
    grid.set_cell_for_test(1, 1, 4, true, true);
    grid.set_cell_for_test(2, 1, 0, true, true);

    let mut entities = EntityStore::new();
    let mut e = GameEntity::test_default(1, "DEST", "Americans", 1, 1);
    e.position.z = 4;
    e.on_bridge = false;
    e.bridge_occupancy = None;
    e.locomotor = Some(make_ship_loco(MovementLayer::Bridge));
    e.movement_target = Some(MovementTarget {
        path: vec![(1, 1), (2, 1)],
        path_layers: vec![MovementLayer::Bridge, MovementLayer::Bridge],
        next_index: 1,
        speed: SimFixed::from_num(512),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });
    e.lifecycle.in_limbo = false;
    e.lifecycle.cell_marked = true;
    e.drive_accelerates = false;
    match e.locomotor.as_ref().unwrap().kind {
        LocomotorKind::Drive => {
            e.drive_locomotion.get_or_insert_with(Default::default);
        }
        LocomotorKind::Ship => {
            e.ship_locomotion.get_or_insert_with(Default::default);
        }
        _ => {}
    }
    entities.insert(e);

    let mut occupancy = OccupancyGrid::new();
    occupancy.add(
        1,
        1,
        1,
        MovementLayer::Ground,
        None,
        CellListInsertion::PrependNonBuilding,
    );
    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    let mut lifecycle_requests = Vec::new();

    tick_bridge(
        &mut entities,
        &grid,
        &mut occupancy,
        &mut rng,
        &mut interner,
        8,
        &mut lifecycle_requests,
    );

    let entity = entities.get(1).expect("entity exists");
    assert_eq!((entity.position.rx, entity.position.ry), (2, 1));
    assert!(entity.on_bridge, "Ship must set OnBridge on Ramp->Body");
    assert_eq!(
        entity
            .bridge_occupancy
            .as_ref()
            .expect("BridgeOccupancy set on Ship Enter")
            .deck_level,
        4
    );
    assert!(
        occupancy.get(1, 1).is_none_or(|cell| {
            cell.count_on(MovementLayer::Ground) + cell.count_on(MovementLayer::Bridge) == 0
        }),
        "Ship must be removed from the old ground object list before relink"
    );
    let body_cell = occupancy.get(2, 1).expect("body occupancy");
    assert_eq!(
        body_cell.count_on(MovementLayer::Bridge),
        1,
        "Ship must insert into the bridge object list after OnBridge=true"
    );
    assert_eq!(body_cell.count_on(MovementLayer::Ground), 0);
}

#[test]
fn on_bridge_fires_at_ramp_to_body_only() {
    // Layout: (1,1) is a ramp/bridgehead at raw h=4 (bridge_walkable, transition=true).
    // (2,1) is a body LANE cell at raw h=0. Effective deck = 4. Lane cells carry
    // the transition flag (Anchor+Forward1 stamping); transition=false structural
    // cells are the non-crossable Forward2 edge lane.
    let mut grid = PathGrid::new(10, 10);
    grid.set_cell_for_test(1, 1, 4, true, true);
    grid.set_cell_for_test(2, 1, 0, true, true);

    let mut entities = EntityStore::new();
    let mut e = GameEntity::test_default(1, "HTNK", "Americans", 1, 1);
    e.position.z = 4;
    e.on_bridge = false;
    e.locomotor = Some(make_drive_loco(MovementLayer::Bridge));
    e.movement_target = Some(MovementTarget {
        path: vec![(1, 1), (2, 1)],
        path_layers: vec![MovementLayer::Bridge, MovementLayer::Bridge],
        next_index: 1,
        speed: SimFixed::from_num(512),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });
    e.lifecycle.in_limbo = false;
    e.lifecycle.cell_marked = true;
    e.drive_accelerates = false;
    match e.locomotor.as_ref().unwrap().kind {
        LocomotorKind::Drive => {
            e.drive_locomotion.get_or_insert_with(Default::default);
        }
        LocomotorKind::Ship => {
            e.ship_locomotion.get_or_insert_with(Default::default);
        }
        _ => {}
    }
    entities.insert(e);

    let mut occupancy = OccupancyGrid::new();
    occupancy.add(
        1,
        1,
        1,
        MovementLayer::Ground,
        None,
        CellListInsertion::PrependNonBuilding,
    );
    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    let mut lifecycle_requests = Vec::new();

    assert!(
        !entities.get(1).unwrap().on_bridge,
        "pre-tick: on_bridge must be false on ramp"
    );

    // 512 lep/sec * 500ms = 256 leptons = exactly one cell jump (1,1)→(2,1).
    tick_bridge(
        &mut entities,
        &grid,
        &mut occupancy,
        &mut rng,
        &mut interner,
        8,
        &mut lifecycle_requests,
    );

    let entity = entities.get(1).expect("entity exists");
    assert_eq!((entity.position.rx, entity.position.ry), (2, 1));
    assert!(
        entity.on_bridge,
        "on_bridge must fire on Ramp→Body transition"
    );
    assert_eq!(
        entity
            .bridge_occupancy
            .as_ref()
            .expect("BridgeOccupancy set on Enter")
            .deck_level,
        4
    );
    let cell = occupancy.get(2, 1).expect("destination occupancy");
    assert_eq!(
        cell.count_on(MovementLayer::Bridge),
        1,
        "Ramp->Body inserts into bridge object list after on_bridge projects true"
    );
    assert_eq!(cell.count_on(MovementLayer::Ground), 0);
}

#[test]
fn on_bridge_clears_at_ramp_to_ground_only() {
    // body (1,1) raw h=0 bridge_walkable; ramp (2,1) raw h=4 bridge_walkable+transition;
    // ground (3,1) raw h=4 no bridge_walkable.
    // Path: (1,1)→(2,1)→(3,1). on_bridge stays true through the ramp tick and clears
    // on Ramp→Ground.
    let mut grid = PathGrid::new(10, 10);
    grid.set_cell_for_test(1, 1, 0, true, false); // body
    grid.set_cell_for_test(2, 1, 4, true, true); // ramp
    grid.set_cell_for_test(3, 1, 4, false, false); // ground at h=4

    let mut entities = EntityStore::new();
    let mut e = GameEntity::test_default(1, "HTNK", "Americans", 1, 1);
    e.position.z = 4;
    e.on_bridge = true;
    // The mover is already driving east along the deck; the hull has to be on
    // the head node's octant or selection stops to turn it there first.
    e.facing = 0x40;
    // Retail [HTNK] Accelerates=false. This transition fixture loads no
    // acceleration rules, including after fresh selection creates its runtime.
    e.drive_accelerates = false;
    e.bridge_occupancy = Some(BridgeOccupancy { deck_level: 4 });
    e.locomotor = Some(make_drive_loco(MovementLayer::Bridge));
    e.movement_target = Some(MovementTarget {
        path: vec![(1, 1), (2, 1), (3, 1)],
        // body→ramp goes on Ground layer per is_at_bridge_level
        // (parent at deck=4, neighbor h=4 → diff=0 < 2 → not at bridge level).
        path_layers: vec![
            MovementLayer::Bridge,
            MovementLayer::Ground,
            MovementLayer::Ground,
        ],
        next_index: 1,
        speed: SimFixed::from_num(512),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });
    e.lifecycle.in_limbo = false;
    e.lifecycle.cell_marked = true;
    e.drive_accelerates = false;
    match e.locomotor.as_ref().unwrap().kind {
        LocomotorKind::Drive => {
            e.drive_locomotion.get_or_insert_with(Default::default);
        }
        LocomotorKind::Ship => {
            e.ship_locomotion.get_or_insert_with(Default::default);
        }
        _ => {}
    }
    entities.insert(e);

    let mut occupancy = OccupancyGrid::new();
    occupancy.add(
        1,
        1,
        1,
        MovementLayer::Bridge,
        None,
        CellListInsertion::PrependNonBuilding,
    );
    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    let mut lifecycle_requests = Vec::new();

    // First physical crossing: body → ramp. on_bridge must STAY true
    // (predicate NoChange).
    tick_bridge_until_cell(
        &mut entities,
        &grid,
        &mut occupancy,
        &mut rng,
        &mut interner,
        (2, 1),
        &mut lifecycle_requests,
    );
    let entity = entities.get(1).expect("entity exists");
    assert_eq!(
        (entity.position.rx, entity.position.ry),
        (2, 1),
        "after the first crossing: at ramp"
    );
    assert!(entity.on_bridge, "on the ramp: on_bridge must stay true");
    let ramp_cell = occupancy.get(2, 1).expect("ramp occupancy");
    assert_eq!(
        ramp_cell.count_on(MovementLayer::Bridge),
        1,
        "Body->Ramp keeps bridge object list while on_bridge remains true"
    );
    assert_eq!(ramp_cell.count_on(MovementLayer::Ground), 0);

    // Next physical crossing: ramp → ground. on_bridge must CLEAR
    // (predicate Exit).
    tick_bridge_until_cell(
        &mut entities,
        &grid,
        &mut occupancy,
        &mut rng,
        &mut interner,
        (3, 1),
        &mut lifecycle_requests,
    );
    let entity = entities.get(1).expect("entity exists");
    assert_eq!(
        (entity.position.rx, entity.position.ry),
        (3, 1),
        "after the next crossing: at ground"
    );
    assert!(!entity.on_bridge, "after Ramp→Ground: on_bridge must clear");
    assert!(
        entity.bridge_occupancy.is_none(),
        "after Exit: BridgeOccupancy must be None"
    );
    let ground_cell = occupancy.get(3, 1).expect("ground occupancy");
    assert_eq!(ground_cell.count_on(MovementLayer::Ground), 1);
    assert_eq!(ground_cell.count_on(MovementLayer::Bridge), 0);
}

#[test]
fn no_bridge_lookahead_pre_claim() {
    // Regression: the deleted apply_bridge_lookahead_if_needed must not have crept
    // back via another path. BridgeOccupancy must NOT be set before the unit
    // physically crosses onto a body cell.
    // ground (1,1) h=4 → ramp (2,1) raw h=4 bridge_walkable+transition → body
    // (3,1) raw h=0 bridge_walkable.
    let mut grid = PathGrid::new(10, 10);
    grid.set_cell_for_test(1, 1, 4, false, false);
    grid.set_cell_for_test(2, 1, 4, true, true);
    grid.set_cell_for_test(3, 1, 0, true, false);

    let mut entities = EntityStore::new();
    let mut e = GameEntity::test_default(1, "HTNK", "Americans", 1, 1);
    e.position.z = 4;
    e.on_bridge = false;
    e.bridge_occupancy = None;
    e.locomotor = Some(make_drive_loco(MovementLayer::Ground));
    e.movement_target = Some(MovementTarget {
        path: vec![(1, 1), (2, 1), (3, 1)],
        path_layers: vec![
            MovementLayer::Ground,
            MovementLayer::Bridge,
            MovementLayer::Bridge,
        ],
        next_index: 1,
        speed: SimFixed::from_num(512),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });
    e.lifecycle.in_limbo = false;
    e.lifecycle.cell_marked = true;
    e.drive_accelerates = false;
    match e.locomotor.as_ref().unwrap().kind {
        LocomotorKind::Drive => {
            e.drive_locomotion.get_or_insert_with(Default::default);
        }
        LocomotorKind::Ship => {
            e.ship_locomotion.get_or_insert_with(Default::default);
        }
        _ => {}
    }
    entities.insert(e);

    let mut occupancy = OccupancyGrid::new();
    occupancy.add(
        1,
        1,
        1,
        MovementLayer::Ground,
        None,
        CellListInsertion::PrependNonBuilding,
    );
    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    let mut lifecycle_requests = Vec::new();

    assert!(
        entities.get(1).unwrap().bridge_occupancy.is_none(),
        "pre-tick: no pre-claim"
    );

    // First physical crossing: ground → ramp. Predicate NoChange
    // (src.bridge_walkable=false; entry
    // would need src_h-4 = dst_h: src=4, dst=4 → no. Exit needs src.bridge_walkable;
    // it's false → no). BridgeOccupancy stays None.
    tick_bridge_until_cell(
        &mut entities,
        &grid,
        &mut occupancy,
        &mut rng,
        &mut interner,
        (2, 1),
        &mut lifecycle_requests,
    );
    let entity = entities.get(1).expect("entity exists");
    assert_eq!(
        (entity.position.rx, entity.position.ry),
        (2, 1),
        "after the first crossing: at ramp"
    );
    assert!(
        entity.bridge_occupancy.is_none(),
        "regression: BridgeOccupancy must NOT be pre-claimed on the ramp"
    );
    let ramp_cell = occupancy.get(2, 1).expect("ramp occupancy");
    assert_eq!(
        ramp_cell.count_on(MovementLayer::Ground),
        1,
        "Ground->Ramp stays ground object list while on_bridge remains false"
    );
    assert_eq!(ramp_cell.count_on(MovementLayer::Bridge), 0);

    // Next physical crossing: ramp → body. Now predicate fires Enter
    // (src.bridge_walkable=true,
    // dst.bridge_walkable=true, dst_h(0) == src_h(4)-4 → entry fires).
    tick_bridge_until_cell(
        &mut entities,
        &grid,
        &mut occupancy,
        &mut rng,
        &mut interner,
        (3, 1),
        &mut lifecycle_requests,
    );
    let entity = entities.get(1).expect("entity exists");
    assert_eq!(
        (entity.position.rx, entity.position.ry),
        (3, 1),
        "after the next crossing: on body"
    );
    assert!(entity.on_bridge, "after Ramp→Body: on_bridge must be true");
    assert_eq!(
        entity
            .bridge_occupancy
            .as_ref()
            .expect("set on Enter")
            .deck_level,
        4
    );
    let body_cell = occupancy.get(3, 1).expect("body occupancy");
    assert_eq!(body_cell.count_on(MovementLayer::Bridge), 1);
    assert_eq!(body_cell.count_on(MovementLayer::Ground), 0);
}

#[test]
fn multi_crossing_preserves_first_bridge_set_update() {
    // Body cells (2,1)/(3,1) are LANE cells (transition flag on the crossable
    // deck lane, per the Anchor+Forward1 stamping in bridge_facts.rs).
    let mut grid = PathGrid::new(10, 10);
    grid.set_cell_for_test(1, 1, 4, true, true);
    grid.set_cell_for_test(2, 1, 0, true, true);
    grid.set_cell_for_test(3, 1, 0, true, true);

    let mut entities = EntityStore::new();
    let mut e = GameEntity::test_default(1, "HTNK", "Americans", 1, 1);
    e.position.z = 4;
    e.on_bridge = false;
    e.locomotor = Some(make_drive_loco(MovementLayer::Bridge));
    e.movement_target = Some(MovementTarget {
        path: vec![(1, 1), (2, 1), (3, 1)],
        path_layers: vec![
            MovementLayer::Bridge,
            MovementLayer::Bridge,
            MovementLayer::Bridge,
        ],
        next_index: 1,
        speed: SimFixed::from_num(1024),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });
    e.lifecycle.in_limbo = false;
    e.lifecycle.cell_marked = true;
    e.drive_accelerates = false;
    match e.locomotor.as_ref().unwrap().kind {
        LocomotorKind::Drive => {
            e.drive_locomotion.get_or_insert_with(Default::default);
        }
        LocomotorKind::Ship => {
            e.ship_locomotion.get_or_insert_with(Default::default);
        }
        _ => {}
    }
    entities.insert(e);

    let mut occupancy = OccupancyGrid::new();
    occupancy.add(
        1,
        1,
        1,
        MovementLayer::Ground,
        None,
        CellListInsertion::PrependNonBuilding,
    );
    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    let mut lifecycle_requests = Vec::new();

    tick_bridge(
        &mut entities,
        &grid,
        &mut occupancy,
        &mut rng,
        &mut interner,
        8,
        &mut lifecycle_requests,
    );

    let entity = entities.get(1).expect("entity exists");
    assert_eq!((entity.position.rx, entity.position.ry), (3, 1));
    assert!(
        entity.on_bridge,
        "first Ramp->Body Set must survive later Unchanged"
    );
    assert_eq!(
        entity
            .bridge_occupancy
            .as_ref()
            .expect("BridgeOccupancy set")
            .deck_level,
        4
    );
    let cell = occupancy.get(3, 1).expect("final occupancy");
    assert_eq!(cell.count_on(MovementLayer::Bridge), 1);
    assert_eq!(cell.count_on(MovementLayer::Ground), 0);
}

// --- Hover throttle integration (M2 P2a) ---

/// Minimal hover mover: hover locomotor, straight eastward path, cell-center start.
fn make_hover_mover(path: Vec<(u16, u16)>, sub_x: i32) -> GameEntity {
    let goal = *path.last().expect("non-empty path");
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        1,
        path[0].0,
        path[0].1,
        0,
        64,
        crate::sim::intern::test_intern("Americans"),
        crate::sim::components::Health { current: 100 },
        crate::sim::intern::test_intern("LCRF"),
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    entity.position.sub_x = SimFixed::from_num(sub_x);
    entity.position.sub_y = SimFixed::from_num(128);
    entity.locomotor = Some(
        crate::sim::movement::locomotor::LocomotorState::for_test_kind(
            crate::rules::locomotor_type::LocomotorKind::Hover,
        ),
    );
    let path_len = path.len();
    entity.movement_target = Some(MovementTarget {
        path,
        path_layers: vec![MovementLayer::Ground; path_len],
        next_index: 1,
        speed: SimFixed::from_num(11),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        final_goal: Some(goal),
        ..Default::default()
    });
    entity
}

/// One movement tick over a bare world (no grids, no rules → stock hover
/// defaults). `binary_frame` must advance per call — hover steering runs a
/// binary-frame FacingClass, which never progresses on a constant frame.
fn tick_hover_world(
    entities: &mut EntityStore,
    native_frame: u32,
    lifecycle_requests: &mut Vec<LifecycleRequest>,
) {
    tick_hover_world_on(
        entities,
        native_frame,
        &mut OccupancyGrid::new(),
        &mut crate::sim::occupancy::CellOccupationGrid::new(),
        lifecycle_requests,
    );
}

/// `tick_hover_world` over caller-owned object lists and owner plane, so the
/// plane side effects of a run can be observed across frames.
fn tick_hover_world_on(
    entities: &mut EntityStore,
    native_frame: u32,
    occupancy: &mut OccupancyGrid,
    cell_occupation: &mut crate::sim::occupancy::CellOccupationGrid,
    lifecycle_requests: &mut Vec<LifecycleRequest>,
) {
    let mut rng = SimRng::new(0);
    let mut interner = test_interner();
    let mut sounds = Vec::new();
    let mut next_occupancy_enter_order = crate::sim::world::EnterOrderCounter::new();
    let terrain_costs: std::collections::BTreeMap<
        crate::rules::locomotor_type::SpeedType,
        crate::sim::pathfinding::terrain_cost::TerrainCostGrid,
    > = std::collections::BTreeMap::new();
    tick_movement_with_grids(
        entities,
        None,
        None,
        &terrain_costs,
        &Default::default(),
        occupancy,
        cell_occupation,
        &mut crate::sim::occupancy::RawCellOccupationGrid::new(),
        &mut next_occupancy_enter_order,
        &mut rng,
        0,
        native_frame,
        None,
        None,
        None,
        &crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        SIM_ZERO,
        9,
        60,
        &mut interner,
        None,
        &mut sounds,
        lifecycle_requests,
    );
}

/// `HoverLocomotionClass::Move 0x00514310` zeroes the Foot occupation enable
/// (`+0x6B6`) on its first translating frame (0x005147D5, gated on speed > 0,
/// the enable still set and a live head) and restores it in the arrival arm
/// (0x0051451E). Native established from the body; this pins VERA's flag over
/// one straight hover run: enabled while the throttle ramps from rest, cleared
/// from the first frame that moves the body, restored exactly at arrival.
#[test]
fn hover_mover_is_in_transit_from_first_translating_frame_until_arrival() {
    let mut entities = EntityStore::new();
    let mut mover = make_hover_mover(vec![(1, 1), (2, 1)], 128);
    mover.lifecycle.cell_marked = true;
    entities.insert(mover);
    let world_xy = |p: &crate::sim::components::Position| (p.rx, p.ry, p.sub_x, p.sub_y);
    let start = world_xy(&entities.get(1).unwrap().position);
    let mut lifecycle_requests = Vec::new();
    let mut translated = false;
    // The bare world's hover integrator covers one cell in roughly 470 frames.
    for frame in 1..1500u32 {
        tick_hover_world(&mut entities, frame, &mut lifecycle_requests);
        let e = entities.get(1).unwrap();
        translated |= world_xy(&e.position) != start;
        if e.movement_target.is_some() {
            assert_eq!(
                e.foot_occupation_enabled, !translated,
                "frame {frame}: +0x6B6 follows the first translating frame"
            );
        } else {
            assert!(translated, "hover arrived without moving");
            assert!(e.foot_occupation_enabled, "arrival restores +0x6B6");
            return;
        }
    }
    let e = entities.get(1).unwrap();
    panic!(
        "hover never arrived: pos=({},{},{:?},{:?}) idx={:?} flag={}",
        e.position.rx,
        e.position.ry,
        e.position.sub_x,
        e.position.sub_y,
        e.movement_target.as_ref().map(|t| t.next_index),
        e.foot_occupation_enabled
    );
}

/// The consumer side of the enable: the owner plane (`CellOccupationGrid`,
/// which `detect_deferred_cell_check` and the Drive selection lane read) drops
/// the hover's claim for the whole transit and carries it again at arrival.
#[test]
fn hover_owner_plane_claim_is_absent_in_transit_and_present_at_arrival() {
    let mut entities = EntityStore::new();
    let mut mover = make_hover_mover(vec![(1, 1), (2, 1)], 128);
    mover.lifecycle.cell_marked = true;
    entities.insert(mover);
    let mut occupancy = OccupancyGrid::new();
    occupancy.add(
        1,
        1,
        1,
        MovementLayer::Ground,
        None,
        crate::sim::occupancy::CellListInsertion::from_category(EntityCategory::Unit),
    );
    let mut plane = crate::sim::occupancy::CellOccupationGrid::new();
    plane.reconcile_entity(entities.get(1).unwrap());
    let bit = crate::sim::occupancy::VEHICLE_OCCUPATION_BIT;
    assert_eq!(plane.vehicle_bits(1, 1, MovementLayer::Ground), bit);
    let mut lifecycle_requests = Vec::new();
    let mut frames_in_transit = 0u32;
    for frame in 1..1500u32 {
        tick_hover_world_on(
            &mut entities,
            frame,
            &mut occupancy,
            &mut plane,
            &mut lifecycle_requests,
        );
        let e = entities.get(1).unwrap();
        let here = (e.position.rx, e.position.ry);
        if e.movement_target.is_some() {
            if !e.foot_occupation_enabled {
                frames_in_transit += 1;
                assert_eq!(
                    plane.vehicle_bits(here.0, here.1, MovementLayer::Ground),
                    0,
                    "frame {frame}: no claim on {here:?} while in transit"
                );
            }
        } else {
            assert!(frames_in_transit > 0);
            assert_eq!(here, (2, 1));
            assert_eq!(plane.vehicle_bits(2, 1, MovementLayer::Ground), bit);
            assert_eq!(plane.vehicle_bits(1, 1, MovementLayer::Ground), 0);
            return;
        }
    }
    panic!("hover never arrived");
}

/// A hover whose next step is refused (parked ally ahead, code 2 wait) ends the
/// frame with the enable set and its cell claimed, as the native arrival arm
/// leaves it (0x0051451E, refused return 0x00514711).
#[test]
fn hover_refused_next_step_re_enables_occupation_while_it_waits() {
    let mut entities = EntityStore::new();
    let mut mover = make_hover_mover(vec![(1, 1), (2, 1)], 128);
    mover.lifecycle.cell_marked = true;
    entities.insert(mover);
    let mut parked = GameEntity::test_default(2, "MTNK", "Americans", 2, 1);
    parked.category = EntityCategory::Unit;
    parked.lifecycle.cell_marked = true;
    parked.locomotor = Some(
        crate::sim::movement::locomotor::LocomotorState::for_test_kind(
            crate::rules::locomotor_type::LocomotorKind::Drive,
        ),
    );
    entities.insert(parked);
    let mut occupancy = OccupancyGrid::new();
    for (id, x) in [(1u64, 1u16), (2, 2)] {
        occupancy.add(
            x,
            1,
            id,
            MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::from_category(EntityCategory::Unit),
        );
    }
    let mut plane = crate::sim::occupancy::CellOccupationGrid::new();
    for id in [1, 2] {
        plane.reconcile_entity(entities.get(id).unwrap());
    }
    let mut lifecycle_requests = Vec::new();
    let mut saw_transit = false;
    let mut prev_sub_x = entities.get(1).unwrap().position.sub_x;
    for frame in 1..1500u32 {
        tick_hover_world_on(
            &mut entities,
            frame,
            &mut occupancy,
            &mut plane,
            &mut lifecycle_requests,
        );
        let e = entities.get(1).unwrap();
        assert_eq!(
            (e.position.rx, e.position.ry),
            (1, 1),
            "frame {frame}: held"
        );
        saw_transit |= !e.foot_occupation_enabled;
        let held = e.position.sub_x <= prev_sub_x;
        prev_sub_x = e.position.sub_x;
        if saw_transit && held && e.movement_target.is_some() {
            // The refused step held the body: the wait leaves the hover an
            // occupant of the cell it is still in.
            assert!(
                e.foot_occupation_enabled,
                "frame {frame}: refused step re-enables"
            );
            assert_eq!(
                plane.vehicle_bits(1, 1, MovementLayer::Ground),
                crate::sim::occupancy::VEHICLE_OCCUPATION_BIT
            );
            return;
        }
    }
    panic!("hover never reached the refused boundary");
}

/// VERA-internal: a hover whose `movement_target` was dropped while in transit
/// (stop order) re-enables its occupation on its next own turn. Native reaches
/// the same state through the arrival arm, which every hover passes because
/// Head_To survives the stop.
#[test]
fn stopped_hover_left_in_transit_re_enables_occupation_on_its_next_turn() {
    let mut entities = EntityStore::new();
    let mut mover = make_hover_mover(vec![(1, 1), (2, 1)], 128);
    mover.lifecycle.cell_marked = true;
    mover.movement_target = None;
    mover.foot_occupation_enabled = false;
    entities.insert(mover);
    let mut lifecycle_requests = Vec::new();
    tick_hover_world(&mut entities, 1, &mut lifecycle_requests);
    assert!(entities.get(1).unwrap().foot_occupation_enabled);
}

#[test]
fn hover_mover_ramps_throttle_from_rest_and_persists_on_locomotor() {
    use crate::sim::movement::hover;
    // Far from the goal (4 cells ≈ 1024 leptons > 255): cruise request 1.0,
    // spin-up from rest at one accel step per tick.
    let mut entities = EntityStore::new();
    entities.insert(make_hover_mover(
        vec![(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)],
        128,
    ));

    let step = hover::hover_ramp_step(hover::HOVER_ACCELERATION_DEFAULT_MINUTES);
    let mut lifecycle_requests = Vec::new();

    tick_hover_world(&mut entities, 0, &mut lifecycle_requests);
    let e = entities.get(1).expect("mover");
    let throttle = e.locomotor.as_ref().expect("loco").hover_throttle;
    assert_eq!(
        throttle, step,
        "tick 1: throttle = one accel step from rest"
    );
    assert_eq!(
        e.movement_target.as_ref().expect("target").current_speed,
        SimFixed::from_num(11) * step,
        "current_speed = base speed × throttle"
    );

    tick_hover_world(&mut entities, 1, &mut lifecycle_requests);
    let e = entities.get(1).expect("mover");
    let throttle2 = e.locomotor.as_ref().expect("loco").hover_throttle;
    assert!(
        throttle2 > throttle,
        "tick 2: throttle keeps ramping ({throttle2} > {throttle})"
    );
}

#[test]
fn hover_mover_brakes_toward_half_throttle_on_final_approach() {
    use crate::sim::movement::hover;
    // Within 255 leptons of the goal (sub_x=200 → 184 leptons to the next cell
    // center): approach request 0.5, so a full-throttle mover brakes one step.
    let mut entities = EntityStore::new();
    let mut mover = make_hover_mover(vec![(0, 0), (1, 0)], 200);
    mover.locomotor.as_mut().expect("loco").hover_throttle = SIM_ONE;
    entities.insert(mover);

    let mut lifecycle_requests = Vec::new();
    tick_hover_world(&mut entities, 0, &mut lifecycle_requests);
    let e = entities.get(1).expect("mover");
    let throttle = e.locomotor.as_ref().expect("loco").hover_throttle;
    assert_eq!(
        throttle,
        SIM_ONE - hover::hover_ramp_step(hover::HOVER_BRAKE_DEFAULT_MINUTES),
        "full-throttle mover on approach brakes by one brake step"
    );
}

#[test]
fn hover_mover_swings_through_corner_braking_not_freezing() {
    use crate::sim::movement::hover;
    // Mover at (0,1) facing EAST with its waypoint due NORTH (0,0): a 90° turn.
    // Contract: while the swing exceeds 45° the throttle BRAKES (request 0) and
    // the position holds; once the facing converges within 45°, movement
    // resumes along the (new) hull heading and the mover crosses into (0,0).
    // The old stop-rotate-go model froze the throttle instead.
    let mut entities = EntityStore::new();
    let mut mover = make_hover_mover(vec![(0, 1), (0, 0)], 128);
    mover.locomotor.as_mut().expect("loco").hover_throttle = SIM_ONE;
    entities.insert(mover);
    let mut lifecycle_requests = Vec::new();

    // Tick 1 (frame 0): hard turn → throttle brakes one step, position holds.
    tick_hover_world(&mut entities, 0, &mut lifecycle_requests);
    let e = entities.get(1).expect("mover");
    assert_eq!(
        e.locomotor.as_ref().expect("loco").hover_throttle,
        SIM_ONE - hover::hover_ramp_step(hover::HOVER_BRAKE_DEFAULT_MINUTES),
        "hard turn brakes the throttle (gamemd turn-stall), never freezes it"
    );
    assert_eq!(
        (e.position.rx, e.position.ry),
        (0, 1),
        "position held during hard turn"
    );
    assert_eq!(
        e.position.sub_y,
        SimFixed::from_num(128),
        "no lepton drift while stalled"
    );

    // Run the swing + native-frame travel out. ROT=5 takes 12 frames for the
    // 90° swing; Speed=11 at approach throttle then needs hundreds of 15 Hz
    // visits to cover the 128 leptons from cell center to the north edge.
    let mut crossed_at = None;
    for frame in 1..600u32 {
        tick_hover_world(&mut entities, frame, &mut lifecycle_requests);
        let cell = entities
            .get(1)
            .map(|entity| (entity.position.rx, entity.position.ry));
        if cell == Some((0, 0)) {
            crossed_at = Some(frame);
            break;
        }
    }
    assert!(
        crossed_at.is_some(),
        "mover must resume after the swing and cross within 600 native frames"
    );
    let e = entities.get(1).expect("mover");
    assert_eq!(
        (e.position.rx, e.position.ry),
        (0, 0),
        "mover crossed into the northern cell after the swing"
    );
    // Facing-lagged curve: the mover drifts east while the hull swings, then
    // re-aims at the cell center from the southeast — so the final heading is
    // northern-half, NOT exactly 0 (exact-north snap was the old stop-rotate
    // behavior this replaces).
    assert!(
        e.facing >= 192 || e.facing <= 64,
        "hull converged into the northern half-circle, got {}",
        e.facing
    );
    assert_ne!(e.facing, 64, "hull is no longer facing due east");
}

#[test]
fn hover_units_float_and_bob_vertically() {
    // Vertical controller: a MOVING hover unit lifts off toward cruise height,
    // and a PARKED hover unit (no movement target) floats too — the idle pass
    // covers every hover unit, not just movers.
    let mut entities = EntityStore::new();
    entities.insert(make_hover_mover(
        vec![(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)],
        128,
    ));
    let mut parked = make_hover_mover(vec![(5, 5), (6, 5)], 128);
    parked.stable_id = 2;
    parked.movement_target = None;
    entities.insert(parked);

    let mut lifecycle_requests = Vec::new();
    for frame in 0..60u32 {
        tick_hover_world(&mut entities, frame, &mut lifecycle_requests);
    }

    let mover_alt = entities
        .get(1)
        .expect("mover")
        .locomotor
        .as_ref()
        .expect("loco")
        .altitude;
    let parked_alt = entities
        .get(2)
        .expect("parked")
        .locomotor
        .as_ref()
        .expect("loco")
        .altitude;
    assert!(
        mover_alt > SIM_ZERO,
        "moving hover unit lifted off (altitude {mover_alt})"
    );
    assert!(
        parked_alt > SIM_ZERO,
        "parked hover unit floats too (altitude {parked_alt})"
    );
    // Under the rules-None defaults (HoverHeight=120) both should sit somewhere
    // below the height cap; the spring never runs away.
    assert!(
        mover_alt < SimFixed::from_num(200) && parked_alt < SimFixed::from_num(200),
        "spring settles near cruise, no runaway (mover {mover_alt}, parked {parked_alt})"
    );
}

// --- Sharp-turn substitute: path-node accounting ---

/// The null-curve substitute consumes exactly ONE path node — the same single
/// node any straight step consumes — never two.
///
/// gamemd substitutes the straight `path[0]_dir * 9` entry, whose flags carry no
/// "turns" bit, and then converges with the ordinary path into the one-node
/// queue shift that every non-turning step takes. The substitute is not special
/// in its node accounting. Consuming a second node left the vehicle one waypoint
/// further off-route on every sharp turn, and it was the producer of the
/// non-adjacent step that the since-removed tube abort used to cancel move
/// orders over.
///
/// Retail: null-curve test and `path[0]_dir * 9` substitution immediately before
/// the queue-width test; one-node shift on the clear-flag branch.
///
/// Parity status: UNCHECKED. The node count is derived from the native contract,
/// not from a gamemd-derived executable check.
#[test]
fn sharp_turn_preserves_path_node_count() {
    // Precondition: E -> SW is a three-octant kink with no precomputed curve. If
    // this ever yields a curve, the test has stopped exercising the substitute.
    assert_eq!(
        super::drive_track::TURN_TRACKS[2 * 8 + 5].normal_track,
        0,
        "E->SW must be a null table entry for this test to exercise the substitute"
    );

    let mut entity = gsi_06_13_fixture_mover(
        (10, 10),
        0x40,
        vec![(10, 10), (11, 10), (10, 11)],
        vec![2, 5],
    );
    entity.movement_target.as_mut().unwrap().speed = SIM_ZERO;
    let mut sim = crate::sim::world::Simulation::new();
    sim.interner = test_interner();
    sim.substrate.entities.insert(entity);
    let grid = PathGrid::new(20, 20);
    sim.process_ground_locomotor_for_test(1, None, Some(&grid), None)
        .unwrap();
    let entity = sim.substrate.entities.get(1).unwrap();
    let drive_locomotion = &entity.drive_locomotion;

    assert!(
        drive_locomotion.as_ref().unwrap().head_to.is_some(),
        "the null-curve substitute should have started a straight drive track"
    );
    assert_eq!(
        entity.navigation.path_replay.cursor, 1,
        "the substitute must consume exactly one path node, not two"
    );
    let drive = drive_locomotion.as_ref().expect("substitute curve");
    assert_eq!(
        drive_track::turn_track_at(drive.track.turn_index as usize)
            .unwrap()
            .normal_track,
        1,
        "the straight cardinal curve"
    );
    assert_eq!(
        drive.head_to.map(|head| (head.x, head.y)),
        Some((11 * 256 + 128, 10 * 256 + 128)),
        "the substitute heads for the real path node one cell east, \
         never a cell synthesized from the hull facing"
    );
}

/// The exact-facing precondition: a hull that is not on the head path node's
/// octant selects nothing, consumes nothing, and is commanded to turn.
#[test]
fn off_octant_hull_turns_before_any_curve_is_selected() {
    let entity =
        gsi_06_13_fixture_mover((10, 10), 0, vec![(10, 10), (11, 11), (12, 12)], vec![3, 3]);
    let mut sim = crate::sim::world::Simulation::new();
    sim.interner = test_interner();
    sim.substrate.entities.insert(entity);
    let grid = PathGrid::new(20, 20);
    sim.process_ground_locomotor_for_test(1, None, Some(&grid), None)
        .unwrap();
    let entity = sim.substrate.entities.get(1).unwrap();
    let drive_locomotion = &entity.drive_locomotion;
    assert_eq!(entity.navigation.path_replay.cursor, 0);

    assert!(
        drive_locomotion.as_ref().unwrap().head_to.is_none(),
        "no curve while the hull is off-octant"
    );
    assert_eq!(
        entity.facing_target,
        Some(0x60),
        "the commanded turn lands on the head node's exact octant (SE)"
    );
}

// ---------------------------------------------------------------------------
// GSI-06.13 — drive-track curve selection basis, end to end
// ---------------------------------------------------------------------------

/// Build the fixture mover: a Drive vehicle with an explicit path and the
/// matching direction replay, so the curve selection runs on the fixture's
/// route instead of whatever A* would produce for the same endpoints.
fn gsi_06_13_fixture_mover(
    start: (u16, u16),
    facing: u8,
    path: Vec<(u16, u16)>,
    directions: Vec<u8>,
) -> GameEntity {
    let mut e = GameEntity::test_default(1, "HTNK", "Americans", start.0, start.1);
    e.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    e.facing = facing;
    e.lifecycle.in_limbo = false;
    e.lifecycle.cell_marked = true;
    let goal = *path.last().expect("non-empty path");
    let layers = vec![MovementLayer::Ground; path.len()];
    e.movement_target = Some(MovementTarget {
        path,
        path_layers: layers,
        next_index: 1,
        speed: SimFixed::from_num(768),
        current_speed: SimFixed::from_num(768),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        final_goal: Some(goal),
        ..Default::default()
    });
    e.navigation.path_replay = crate::sim::components::FootPathQueue {
        directions,
        cursor: 0,
        reference_cell: Some((start.0 as i16, start.1 as i16)),
    };
    e.foot_speed.applied_fraction = SIM_ONE;
    e.drive_locomotion = Some(crate::sim::components::DriveLocomotionRuntime {
        target_speed_fraction: SIM_ONE,
        ..Default::default()
    });
    // A seeded path is only the route adapter. Native terminal +504 checks
    // NavCom independently; install the real destination before executing it.
    super::navcom::set_destination_internal_cell(&mut e, goal, None);
    e
}

/// Fixture A, end to end. Tank at (10,10) facing E, route (10,10) → (11,10) →
/// (11,11). gamemd selects the E->S curve *before leaving (10,10)*, so the hull
/// is already turning when it enters (11,10) and the cell it reserves is the
/// two-cell endpoint (11,11).
///
/// Before this change VERA selected the straight-east curve first and only
/// began the arc after arriving at (11,10) — the hull was still exactly on E at
/// that moment and the reserved head was (11,10).
#[test]
fn gsi_06_13_turn_begins_before_the_corner_cell_and_reserves_two_cells() {
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(20, 20);
    entities.insert(gsi_06_13_fixture_mover(
        (10, 10),
        0x40,
        vec![(10, 10), (11, 10), (11, 11)],
        vec![2, 4],
    ));

    let mut occupancy = OccupancyGrid::new();
    let mut rng = SimRng::new(0);
    let mut lifecycle_requests = Vec::new();
    let mut reserved_head_at_selection: Option<(u16, u16)> = None;
    let mut facing_entering_corner: Option<u8> = None;
    let mut reached_endpoint = false;
    let mut final_facing = 0u8;

    for tick in 0..200u64 {
        tick_movement_with_grid(
            &mut entities,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut occupancy,
            &mut rng,
            tick,
            &mut test_interner(),
            &mut lifecycle_requests,
        );
        let Some(entity) = entities.get(1) else { break };
        if reserved_head_at_selection.is_none()
            && let Some(drive) = entity.drive_locomotion.as_ref()
            && let Some(head) = drive.occupation_head_to
        {
            reserved_head_at_selection = Some((head.rx, head.ry));
        }
        let cell = (entity.position.rx, entity.position.ry);
        if cell == (11, 10) && facing_entering_corner.is_none() {
            facing_entering_corner = Some(entity.facing);
        }
        final_facing = entity.facing;
        if cell == (11, 11) {
            reached_endpoint = true;
            if committed_track_head(entity).is_none() {
                break;
            }
        }
    }

    assert_eq!(
        reserved_head_at_selection,
        Some((11, 11)),
        "a turning curve reserves its two-cell endpoint, not the first node"
    );
    let corner_facing = facing_entering_corner.expect("mover must pass through (11,10)");
    assert_ne!(
        corner_facing, 0x40,
        "the hull must already be turning when it enters (11,10); \
         staying exactly on east means the curve was applied a cell late"
    );
    assert!(
        corner_facing > 0x40 && corner_facing < 0x80,
        "hull facing entering (11,10) should sit between east and south, got {corner_facing:#04x}"
    );
    assert!(reached_endpoint, "mover must reach (11,11)");
    assert_eq!(
        final_facing, 0x80,
        "the curve ends on the table's south facing"
    );
}

/// Fixture C, end to end. Tank at (30,30) facing E, route (30,30) → (31,30) →
/// (30,31): a three-octant kink. gamemd's null-curve fallback is keyed on the
/// *head node's* direction, so the mover still drives east onto (31,30) and only
/// then deals with the kink. The old body-facing-keyed substitute synthesized a
/// cell in the hull's direction and drove to (32,30) instead, off the route.
#[test]
fn gsi_06_13_sharp_kink_fallback_still_drives_to_the_real_path_node() {
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(40, 40);
    entities.insert(gsi_06_13_fixture_mover(
        (30, 30),
        0x40,
        vec![(30, 30), (31, 30), (30, 31)],
        vec![2, 5],
    ));

    let mut occupancy = OccupancyGrid::new();
    let mut rng = SimRng::new(0);
    let mut lifecycle_requests = Vec::new();
    let mut visited: Vec<(u16, u16)> = Vec::new();

    for tick in 0..200u64 {
        tick_movement_with_grid(
            &mut entities,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut occupancy,
            &mut rng,
            tick,
            &mut test_interner(),
            &mut lifecycle_requests,
        );
        let Some(entity) = entities.get(1) else { break };
        let cell = (entity.position.rx, entity.position.ry);
        if visited.last() != Some(&cell) {
            visited.push(cell);
        }
        if cell == (30, 31) {
            break;
        }
    }

    assert!(
        !visited.contains(&(32, 30)),
        "the sharp-kink fallback must not drive past the path node: visited {visited:?}"
    );
    assert!(
        visited.contains(&(31, 30)),
        "the mover must still take its real next path node: visited {visited:?}"
    );
}

// ---------------------------------------------------------------------------
// Drive-track cell crossings are coordinate-derived, not path-derived
// ---------------------------------------------------------------------------

/// Half a cell in leptons. A mover at the fixture speed advances ~16 leptons
/// per axis per frame; anything past half a cell in one frame is a teleport,
/// not motion.
const HALF_CELL_LEPTONS: i32 = 128;

/// Straight NE diagonals must render as a continuous glide.
///
/// The NE straight curve has one point that lands exactly on a cell corner, so
/// it makes *two* cell transitions — first east, then north — while consuming a
/// single path node. gamemd derives the object's cell from its one absolute
/// coordinate, so both the coordinate and the cell move by the same delta and
/// the world position stays continuous. If the cell instead comes from the path
/// node while the sub-cell offset comes from the coordinate, the two disagree
/// and the mover snaps a full cell sideways for one frame.
#[test]
fn drive_track_ne_diagonal_world_position_never_teleports() {
    let mut entities = EntityStore::new();
    let grid: PathGrid = PathGrid::new(20, 20);
    let mut mover = gsi_06_13_fixture_mover(
        (10, 10),
        0x20,
        vec![(10, 10), (11, 9), (12, 8), (13, 7)],
        vec![1, 1, 1],
    );
    if let Some(target) = mover.movement_target.as_mut() {
        target.speed = SimFixed::from_num(256);
        target.current_speed = SimFixed::from_num(256);
    }
    entities.insert(mover);

    let mut occupancy = OccupancyGrid::new();
    let mut rng = SimRng::new(0);
    let mut lifecycle_requests = Vec::new();
    let mut previous: Option<(i32, i32)> = None;
    let mut worst = (0i32, 0i32, 0usize);
    let mut trace: Vec<(usize, i32, i32)> = Vec::new();
    let mut reached = false;

    for tick in 0..400u64 {
        tick_movement_with_grid(
            &mut entities,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut occupancy,
            &mut rng,
            tick,
            &mut test_interner(),
            &mut lifecycle_requests,
        );
        let Some(entity) = entities.get(1) else { break };
        let world_x = entity.position.rx as i32 * 256 + entity.position.sub_x.to_num::<i32>();
        let world_y = entity.position.ry as i32 * 256 + entity.position.sub_y.to_num::<i32>();
        trace.push((tick as usize, world_x, world_y));
        if let Some((px, py)) = previous {
            let dx = world_x - px;
            let dy = world_y - py;
            if dx.abs().max(dy.abs()) > worst.0.abs().max(worst.1.abs()) {
                worst = (dx, dy, tick as usize);
            }
        }
        previous = Some((world_x, world_y));
        if (entity.position.rx, entity.position.ry) == (13, 7) {
            reached = true;
            break;
        }
    }

    assert!(
        reached,
        "mover must reach (13,7); trace tail {:?}; drive={:?}; speed={:?}; target={:?}; queue={:?}",
        &trace[trace.len().saturating_sub(6)..],
        entities.get(1).unwrap().drive_locomotion,
        entities.get(1).unwrap().foot_speed,
        entities.get(1).unwrap().movement_target,
        entities.get(1).unwrap().navigation.path_replay
    );
    assert!(
        worst.0.abs() <= HALF_CELL_LEPTONS && worst.1.abs() <= HALF_CELL_LEPTONS,
        "world position jumped ({}, {}) leptons at tick {} \
         (= {:.2}, {:.2} cells) on a straight NE diagonal; trace {:?}",
        worst.0,
        worst.1,
        worst.2,
        worst.0 as f32 / 256.0,
        worst.1 as f32 / 256.0,
        &trace[worst.2.saturating_sub(3)..(worst.2 + 2).min(trace.len())]
    );
}

/// NE diagonals must not run at double speed.
///
/// Both orientations must visit every queued cell without skipping a node.
/// Retail Raw2 skips the exact corner; residual interpolation can cross into
/// the queued cell before paid-point bookkeeping consumes it. Drive4B253F..
/// 4B25C3 performs that residual transfer without consuming a path step. The
/// next paid point must catch up once. SE uses the same raw curve as a control.
#[test]
fn drive_track_ne_diagonal_costs_the_same_ticks_per_cell_as_se() {
    fn run(path: Vec<(u16, u16)>, facing: u8, dir: u8) -> (u64, Vec<(u16, u16)>) {
        let start = path[0];
        let goal = *path.last().unwrap();
        let steps = path.len() - 1;
        let mut entities = EntityStore::new();
        let grid: PathGrid = PathGrid::new(24, 24);
        let mut mover = gsi_06_13_fixture_mover(start, facing, path, vec![dir; steps]);
        if let Some(target) = mover.movement_target.as_mut() {
            target.speed = SimFixed::from_num(256);
            target.current_speed = SimFixed::from_num(256);
        }
        entities.insert(mover);

        let mut occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(0);
        let mut lifecycle_requests = Vec::new();
        occupancy.add(
            start.0,
            start.1,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let mut visited: Vec<(u16, u16)> = vec![start];
        let mut residual_crossings = 0;
        let mut pending_consumption = None;
        let mut ticks = 0u64;
        for tick in 0..400u64 {
            let before = entities.get(1).unwrap();
            let prior_cell = (before.position.rx, before.position.ry);
            let prior_target = before.movement_target.as_ref().unwrap();
            let prior_index = prior_target.next_index;
            let queued_cell = prior_target.path.get(prior_index).copied();
            tick_movement_with_grid(
                &mut entities,
                Some(&grid),
                &Default::default(),
                &Default::default(),
                &mut occupancy,
                &mut rng,
                tick,
                &mut test_interner(),
                &mut lifecycle_requests,
            );
            let Some(entity) = entities.get(1) else { break };
            let cell = (entity.position.rx, entity.position.ry);
            let next_index = entity
                .movement_target
                .as_ref()
                .map_or(steps + 1, |target| target.next_index);
            if let Some(expected) = pending_consumption.take() {
                // This fixture's speed pays points every tick. A residual
                // crossing may defer this bookkeeping only until that payment.
                assert_eq!(
                    next_index, expected,
                    "the next paid tick consumes the residual-reached node once"
                );
            }
            assert!(
                next_index >= prior_index && next_index <= prior_index + 1,
                "one diagonal cell must not skip path nodes"
            );
            if visited.last() != Some(&cell) {
                assert!(!occupancy.contains_entity(prior_cell.0, prior_cell.1, 1));
                assert!(occupancy.contains_entity(cell.0, cell.1, 1));
                if Some(cell) == queued_cell {
                    if next_index == prior_index {
                        residual_crossings += 1;
                        pending_consumption = Some(prior_index + 1);
                    }
                } else {
                    assert_eq!(
                        next_index, prior_index,
                        "an intermediate track cell must not consume the queued destination"
                    );
                }
                visited.push(cell);
            }
            ticks = tick + 1;
            if cell == goal && pending_consumption.is_none() {
                break;
            }
        }
        assert_eq!(visited.last(), Some(&goal), "the mover reaches its goal");
        assert!(pending_consumption.is_none());
        if dir == 1 {
            assert!(
                residual_crossings > 0,
                "NE fixture exercises a residual crossing followed by paid bookkeeping"
            );
        }
        (ticks, visited)
    }

    let (ne_ticks, ne_visited) = run(
        vec![(10, 10), (11, 9), (12, 8), (13, 7)],
        0x20,
        1, // NE octant
    );
    let (se_ticks, se_visited) = run(
        vec![(10, 10), (11, 11), (12, 12), (13, 13)],
        0x60,
        3, // SE octant
    );

    for node in [(11, 9), (12, 8), (13, 7)] {
        assert!(
            ne_visited.contains(&node),
            "NE mover must occupy every path node; visited {ne_visited:?}"
        );
    }
    assert!(
        ne_ticks * 2 > se_ticks * 3 / 2,
        "NE ({ne_ticks} ticks) must not run at roughly double the SE rate \
         ({se_ticks} ticks) over the same 3-cell diagonal; \
         NE visited {ne_visited:?}, SE visited {se_visited:?}"
    );
}

// Reproduces the eight-GI ordinary-move report. Native scatter gate vectors
// are retained in tools/infantry_scatter_oracle.json.
#[test]
fn group_gis_do_not_acquire_scatter_speed_or_lose_their_goal() {
    let grid = PathGrid::new(30, 30);
    let mut reached = 0;
    for seed in 0..8 {
        let mut entities = EntityStore::new();
        for id in 1..=8u64 {
            let mut e = GameEntity::test_default(
                id,
                "E1",
                "Americans",
                10 + (id % 3) as u16,
                10 + (id / 3) as u16,
            );
            e.category = EntityCategory::Infantry;
            e.lifecycle.in_limbo = false;
            e.lifecycle.cell_marked = true;
            e.sub_cell = Some(2);
            let mut loco = LocomotorState::for_test_kind(LocomotorKind::Walk);
            loco.speed_type = SpeedType::Foot;
            e.locomotor = Some(loco);
            entities.insert(e);
        }
        for id in 1..=8u64 {
            assert!(issue_move_command(
                &mut entities,
                &grid,
                id,
                (3, 3),
                SimFixed::from_num(150),
                false,
                None,
                None,
                None,
                crate::sim::movement::DestinationTiming::new(0, 60),
            ));
        }
        let mut occupancy = OccupancyGrid::new();
        for (id, e) in entities.iter_sorted() {
            occupancy.add(
                e.position.rx,
                e.position.ry,
                id,
                MovementLayer::Ground,
                e.sub_cell,
                CellListInsertion::PrependNonBuilding,
            );
        }
        let mut rng = SimRng::new(seed);
        let mut interner = test_interner();
        let mut requests = Vec::new();
        for tick in 0..1000 {
            let previous: Vec<_> = (1..=8u64)
                .map(|id| entities.get(id).unwrap().position.clone())
                .collect();
            tick_movement_with_grid(
                &mut entities,
                Some(&grid),
                &Default::default(),
                &Default::default(),
                &mut occupancy,
                &mut rng,
                tick,
                &mut interner,
                &mut requests,
            );
            for id in 1..=8u64 {
                let p = &entities.get(id).unwrap().position;
                let old = &previous[(id - 1) as usize];
                let dx = (i32::from(p.rx) - i32::from(old.rx)) * 256
                    + (p.sub_x - old.sub_x).to_num::<i32>();
                let dy = (i32::from(p.ry) - i32::from(old.ry)) * 256
                    + (p.sub_y - old.sub_y).to_num::<i32>();
                assert!(
                    dx * dx + dy * dy <= 32 * 32,
                    "unexpected jump seed={seed} tick={tick} GI={id} delta=({dx},{dy})"
                );
                if let Some(mt) = entities.get(id).unwrap().movement_target.as_ref() {
                    assert_eq!(
                        mt.speed,
                        SimFixed::from_num(150),
                        "seed={seed} tick={tick} GI={id}"
                    );
                    if mt.final_goal != Some((3, 3)) {
                        let e = entities.get(id).unwrap();
                        let dx = i32::from(e.position.rx) - 3;
                        let dy = i32::from(e.position.ry) - 3;
                        assert!(
                            dx * dx + dy * dy <= 9,
                            "only a GI that reached the destination may be scattered: seed={seed} tick={tick} id={id}"
                        );
                    }
                }
            }
        }
        for id in 1..=8u64 {
            let e = entities.get(id).unwrap();
            let dx = i32::from(e.position.rx) - 3;
            let dy = i32::from(e.position.ry) - 3;
            if dx * dx + dy * dy <= 9 {
                reached += 1;
            }
        }
    }
    assert_eq!(
        reached, 64,
        "the entire group must reach the destination area"
    );
}

#[test]
fn blocked_walk_keeps_exact_pre_step_position() {
    let grid = PathGrid::new(30, 30);
    let mut entities = EntityStore::new();
    for id in 1..=2 {
        let mut e = GameEntity::test_default(id, "E1", "Americans", 9 + id as u16, 10);
        e.category = EntityCategory::Infantry;
        e.lifecycle.in_limbo = false;
        e.lifecycle.cell_marked = true;
        e.sub_cell = Some(2);
        let mut loco = LocomotorState::for_test_kind(LocomotorKind::Walk);
        loco.speed_type = SpeedType::Foot;
        e.locomotor = Some(loco);
        entities.insert(e);
        assert!(issue_move_command(
            &mut entities,
            &grid,
            id,
            (20, 10),
            SimFixed::from_num(150),
            false,
            None,
            None,
            None,
            crate::sim::movement::DestinationTiming::new(0, 60),
        ));
    }
    entities.get_mut(1).unwrap().position.sub_x = SimFixed::from_num(253);
    entities.get_mut(1).unwrap().position.sub_y = SimFixed::from_num(77);
    let before = entities.get(1).unwrap().position.clone();
    let mut occupancy = OccupancyGrid::new();
    for (id, e) in entities.iter_sorted() {
        occupancy.add(
            e.position.rx,
            e.position.ry,
            id,
            MovementLayer::Ground,
            e.sub_cell,
            CellListInsertion::PrependNonBuilding,
        );
    }
    let mut rng = SimRng::new(0);
    tick_movement_with_grid(
        &mut entities,
        Some(&grid),
        &Default::default(),
        &Default::default(),
        &mut occupancy,
        &mut rng,
        0,
        &mut test_interner(),
        &mut Vec::new(),
    );
    let e = entities.get(1).unwrap();
    assert_eq!(
        (
            e.position.rx,
            e.position.ry,
            e.position.sub_x,
            e.position.sub_y
        ),
        (before.rx, before.ry, before.sub_x, before.sub_y)
    );
    assert_eq!(
        e.movement_target.as_ref().unwrap().final_goal,
        Some((20, 10))
    );
    assert_eq!(
        entities
            .get(2)
            .unwrap()
            .movement_target
            .as_ref()
            .unwrap()
            .final_goal,
        Some((20, 10))
    );
}

/// I9c regression. The proactive segment repath must pass the same crush
/// authority as the initial order (`CrushCapability::of`): native reads the
/// type's `Crusher=` (`UnitTypeClass+0xD28`, wall arm `0x0073F438..F446`), not
/// its MovementZone. A `Crusher=yes` mover ordered past a sandbag cell beyond
/// its first 24-step segment must continue through it; a non-crusher's repath
/// is refused there, which shows the arm is live. The fixture has no locomotor
/// (the legacy tick only moves a Drive mover with a prepared path replay), so
/// the old zone-based derivation read no zone here; the Normal-zone Drive case
/// is covered by the process-entry and attack-move-resume tests below.
#[test]
fn segment_repath_lets_a_crusher_tank_through_a_sandbag_line() {
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
    let mut cells = Vec::with_capacity(50);
    for rx in 0..50u16 {
        let mut cell = ResolvedTerrainCell::clear_for_test(rx, 0);
        if rx == 27 {
            cell.overlay_zone_type = Some(zone_class::CRUSHABLE);
            cell.zone_type = zone_class::CRUSHABLE;
        }
        cells.push(cell);
    }
    let terrain = ResolvedTerrainGrid::from_cells(50, 1, cells);
    let grid = PathGrid::from_resolved_terrain(&terrain);

    let run = |regular_crusher: bool| {
        let mut entities = EntityStore::new();
        let mut e = GameEntity::test_default(1, "MTNK", "Americans", 0, 0);
        e.regular_crusher = regular_crusher;
        entities.insert(e);
        assert!(issue_move_command(
            &mut entities,
            &grid,
            1,
            (30, 0),
            SimFixed::from_num(15360),
            false,
            None,
            None,
            None,
            crate::sim::movement::DestinationTiming::new(0, 60),
        ));
        let mut lifecycle_requests = Vec::new();
        let mut sounds = Vec::new();
        let mut occupancy = OccupancyGrid::new();
        let mut cell_occupation = crate::sim::occupancy::CellOccupationGrid::new();
        let mut raw = crate::sim::occupancy::RawCellOccupationGrid::new();
        let mut enter_order = crate::sim::world::EnterOrderCounter::new();
        let mut interner = test_interner();
        let mut rng = SimRng::new(0);
        for frame in 0..40u32 {
            tick_movement_with_grids(
                &mut entities,
                None,
                Some(&grid),
                &Default::default(),
                &Default::default(),
                &mut occupancy,
                &mut cell_occupation,
                &mut raw,
                &mut enter_order,
                &mut rng,
                u64::from(frame),
                frame,
                None,
                Some(&terrain),
                None,
                &crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
                SIM_ZERO,
                9,
                60,
                &mut interner,
                None,
                &mut sounds,
                &mut lifecycle_requests,
            );
        }
        let e = entities.get(1).expect("entity exists");
        (e.position.rx, e.position.ry)
    };
    assert_eq!(
        run(true),
        (30, 0),
        "crusher continues through the sandbag cell"
    );
    assert!(
        run(false).0 < 27,
        "non-crusher repath is refused at the sandbag cell"
    );
}

/// I9c regression, order path. The move order's own search reads the crusher
/// flags from the mover: no caller passes them, so no caller can pass `false`
/// for a tank. One row, a sandbag at x=5, the goal beyond it.
#[test]
fn move_order_search_reads_the_crusher_flags_from_the_mover() {
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
    let mut cells = Vec::with_capacity(12);
    for rx in 0..12u16 {
        let mut cell = ResolvedTerrainCell::clear_for_test(rx, 0);
        if rx == 5 {
            cell.overlay_zone_type = Some(zone_class::CRUSHABLE);
            cell.zone_type = zone_class::CRUSHABLE;
        }
        cells.push(cell);
    }
    let terrain = ResolvedTerrainGrid::from_cells(12, 1, cells);
    let grid = PathGrid::from_resolved_terrain(&terrain);

    let ordered_path = |regular_crusher: bool| {
        let mut entities = EntityStore::new();
        let mut e = GameEntity::test_default(1, "MTNK", "Americans", 0, 0);
        e.regular_crusher = regular_crusher;
        e.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        entities.insert(e);
        let accepted = crate::sim::movement::movement_commands::issue_move_command_with_layered(
            &mut entities,
            &grid,
            1,
            (10, 0),
            SimFixed::from_num(15360),
            false,
            None,
            None,
            Some(&terrain),
            None,
            None,
            None,
            None,
            None,
            crate::sim::movement::DestinationTiming::new(0, 60),
        );
        let path = entities
            .get(1)
            .and_then(|e| e.movement_target.as_ref())
            .map(|target| target.path.clone())
            .unwrap_or_default();
        (accepted, path)
    };
    let (accepted, path) = ordered_path(true);
    assert!(accepted);
    assert!(
        path.contains(&(5, 0)) && path.last() == Some(&(10, 0)),
        "the crusher's ordered path crosses the sandbag: {path:?}"
    );
    let (_, path) = ordered_path(false);
    assert!(
        !path.contains(&(5, 0)),
        "a non-crusher is never routed through the sandbag: {path:?}"
    );
}

/// I9c regression, process-entry repath. A Drive `Crusher=yes` tank whose owner
/// destination survives with no active path rebuilds its route on its next
/// turn; that search must admit the sandbag cell a non-crusher is refused.
#[test]
fn process_entry_repath_lets_a_crusher_tank_through_a_sandbag_cell() {
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
    let mut cells = Vec::with_capacity(20);
    for rx in 0..20u16 {
        let mut cell = ResolvedTerrainCell::clear_for_test(rx, 0);
        if rx == 5 {
            cell.overlay_zone_type = Some(zone_class::CRUSHABLE);
            cell.zone_type = zone_class::CRUSHABLE;
        }
        cells.push(cell);
    }
    let terrain = ResolvedTerrainGrid::from_cells(20, 1, cells);
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let run = |regular_crusher: bool| {
        let mut entities = EntityStore::new();
        let mut e = GameEntity::test_default(1, "MTNK", "Americans", 0, 0);
        e.regular_crusher = regular_crusher;
        e.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        e.navigation.nav_com = Some(NavTargetRef::cell(10, 0));
        e.navigation.pending_arrival_clear = true;
        entities.insert(e);
        let mut lifecycle_requests = Vec::new();
        tick_movement_with_grids(
            &mut entities,
            None,
            Some(&grid),
            &Default::default(),
            &Default::default(),
            &mut OccupancyGrid::new(),
            &mut crate::sim::occupancy::CellOccupationGrid::new(),
            &mut crate::sim::occupancy::RawCellOccupationGrid::new(),
            &mut crate::sim::world::EnterOrderCounter::new(),
            &mut SimRng::new(0),
            0,
            0,
            None,
            Some(&terrain),
            None,
            &crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            SIM_ZERO,
            9,
            60,
            &mut test_interner(),
            None,
            &mut Vec::new(),
            &mut lifecycle_requests,
        );
        let e = entities.get(1).expect("entity exists");
        (
            e.movement_target.as_ref().map(|t| t.path.clone()),
            e.navigation.pending_arrival_clear,
        )
    };
    let (path, _) = run(true);
    let path = path.expect("crusher rebuilds its route");
    assert!(
        path.contains(&(5, 0)),
        "crusher route crosses the sandbag: {path:?}"
    );
    let (path, rearmed) = run(false);
    assert!(
        path.is_none(),
        "non-crusher is refused at the sandbag: {path:?}"
    );
    assert!(rearmed, "the failed rebuild re-arms the retry");
}

// Clock regression for the still-existing deferred crossing corridor. This
// exercises its actual Process caller, not native crossing geometry parity.
#[test]
fn deferred_crush_uses_binary_frame_after_cell_classification_with_offset_clocks() {
    use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};
    for shield in [InvulnKind::IronCurtain, InvulnKind::ForceShield] {
        for (tick, frame, protected) in [(0, 100, true), (100, 130, false)] {
            let mut sim = Simulation::with_seed(0);
            sim.session.tick = tick;
            sim.session.binary_frame = frame;
            let mut victim = GameEntity::test_default(2, "E1", "Soviets", 2, 2);
            victim.owner = sim.intern("Soviets");
            victim.type_ref = sim.intern("E1");
            victim.category = EntityCategory::Infantry;
            victim.crushable = true;
            victim.lifecycle.in_limbo = false;
            victim.lifecycle.cell_marked = true;
            victim.invulnerability = Some(InvulnerabilityState {
                start_frame: 90,
                duration_frames: 30,
                kind: shield,
            });
            let health = victim.health.current;
            sim.substrate.entities.insert(victim);
            let mut crusher = GameEntity::test_default(1, "HTNK", "Americans", 1, 2);
            crusher.owner = sim.intern("Americans");
            crusher.type_ref = sim.intern("HTNK");
            crusher.position.sub_x = SimFixed::from_num(240);
            crusher.regular_crusher = true;
            crusher.lifecycle.in_limbo = false;
            crusher.lifecycle.cell_marked = true;
            crusher.movement_target = Some(MovementTarget {
                path: vec![(1, 2), (2, 2)],
                path_layers: vec![MovementLayer::Ground; 2],
                next_index: 1,
                speed: SimFixed::from_num(1024),
                move_dir_x: SimFixed::from_num(256),
                move_dir_y: SIM_ZERO,
                move_dir_len: SimFixed::from_num(256),
                final_goal: Some((2, 2)),
                ..Default::default()
            });
            sim.substrate.entities.insert(crusher);
            sim.substrate.occupancy = OccupancyGrid::rebuild(&sim.substrate.entities);
            sim.substrate.cell_occupation =
                crate::sim::occupancy::CellOccupationGrid::rebuild(&sim.substrate.entities);
            let stats = sim
                .process_ground_locomotor_for_test(1, None, Some(&PathGrid::new(8, 8)), None)
                .unwrap();
            let context = format!("{shield:?} tick={tick} binary_frame={frame}");
            assert_eq!(stats.crush_kills, u32::from(!protected), "{context}");
            assert_eq!(
                sim.substrate.entities.get(2).unwrap().health.current,
                if protected { health } else { 0 },
                "{context}"
            );
            assert_eq!(
                sim.pending_lifecycle_requests
                    .iter()
                    .any(|request| matches!(
                        request,
                        LifecycleRequest::Uninit {
                            stable_id: 2,
                            reason: UninitReason::Crush
                        }
                    )),
                !protected,
                "{context}"
            );
        }
    }
}

/// The production movement host must carry CurrentIQ/type abilities into the
/// cell-entry dispatcher; a classifier-only test cannot detect dropped context.
#[test]
fn cell_scatter_world_reads_live_house_and_veteran_ability() {
    let rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::rules::ini_parser::IniFile::from_str(
            "[General]\nCloseEnough=0\n[VehicleTypes]\n0=LCRF\n[InfantryTypes]\n0=E1\n[LCRF]\nSpeed=6\nCrusher=yes\n[E1]\nSpeed=4\nVeteranAbilities=SCATTER\n[IQ]\nScatter=2\n[CombatDamage]\nPlayerScatter=no\n"
        )
    ).unwrap();
    assert_eq!(rules.general.iq_scatter, 2);
    assert!(!rules.general.player_scatter);
    for (iq, rank, expected) in [(0, 0, false), (2, 0, true), (0, 100, true)] {
        let mut sim = crate::sim::world::Simulation::new();
        // The ordinary 576-lepton CloseEnough would finish this short route
        // before its occupied cell, so require arrival for this fixture.
        sim.close_enough = SIM_ZERO;
        let mut mover = make_hover_mover(vec![(1, 1), (2, 1), (3, 1)], 250);
        mover.regular_crusher = true;
        mover.lifecycle.cell_marked = true;
        mover.lifecycle.in_limbo = false;
        sim.substrate.entities.insert(mover);
        let mut victim = GameEntity::test_default(2, "E1", "Soviets", 2, 1);
        victim.category = EntityCategory::Infantry;
        victim.crushable = true;
        victim.lifecycle.cell_marked = true;
        victim.lifecycle.in_limbo = false;
        // Outside the full-cell crush radius, so this test observes the entering
        // cell's scatter dispatch instead of a simultaneous destruction.
        victim.position.sub_x = SIM_ZERO;
        victim.position.sub_y = SIM_ZERO;
        victim.veterancy = rank;
        victim.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Walk,
            ),
        );
        let owner = victim.owner();
        sim.substrate.entities.insert(victim);
        sim.interner = test_interner();
        let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10);
        house.current_iq = iq;
        sim.houses.insert(owner, house);
        sim.substrate.occupancy = OccupancyGrid::rebuild(&sim.substrate.entities);
        let classification = |sim: &crate::sim::world::Simulation| {
            crate::sim::pathfinding::cell_entry::classify_occupied_cell_with_occupation_and_slave_query(
                (2, 1), crate::sim::pathfinding::cell_entry::CanEnterLayerContext::single(MovementLayer::Ground),
                1, bump_crush::CrushCapability::new(true, false), "Americans",
                crate::rules::locomotor_type::LocomotorKind::Hover, false, None,
                &sim.substrate.occupancy, &sim.substrate.cell_occupation,
                &sim.substrate.raw_cell_occupation, sim.session.binary_frame,
                &sim.substrate.entities, &sim.house_alliances, &sim.interner, None,
            )
        };
        assert!(
            matches!(
                classification(&sim),
                crate::sim::pathfinding::cell_entry::CellEntryResult::Crushable { .. }
            ),
            "initial classification {:?}",
            classification(&sim)
        );
        let mut scattered = false;
        let mut visited_cell_receiver = false;
        for frame in 1..1500 {
            let before_x = sim.substrate.entities.get(1).unwrap().position.sub_x;
            sim.session.binary_frame = frame;
            let timing = super::MovementConfig::from_rules(frame, SIM_ZERO, Some(&rules));
            let stats = sim
                .process_ground_locomotor_with_config_for_test(1, Some(&rules), None, None, timing)
                .unwrap();
            scattered |= stats.scatter_successes != 0;
            // The non-centred infantry survives the crush-radius test, so the
            // existing entering-cell receiver returns the hover to its old
            // centre. Requiring a whole-cell crossing would incorrectly reject
            // the intended no-scatter case, which continues waiting here.
            let after = &sim.substrate.entities.get(1).unwrap().position;
            visited_cell_receiver |= before_x > SimFixed::from_num(128)
                && after.rx == 1
                && after.sub_x == SimFixed::from_num(128);
            if scattered || visited_cell_receiver {
                break;
            }
        }
        assert!(
            scattered || visited_cell_receiver,
            "fixture must reach occupied cell; IQ={iq}, rank={rank}, mover={:?}, path={:?}, classification={:?}",
            sim.substrate.entities.get(1).unwrap().position,
            sim.substrate.entities.get(1).unwrap().movement_target,
            classification(&sim)
        );
        assert_eq!(scattered, expected, "IQ={iq}, rank={rank}");
        assert_eq!(
            sim.substrate
                .entities
                .get(2)
                .unwrap()
                .movement_target
                .is_some(),
            expected
        );
    }
}
