//! Movement integration tests — verifies ground movement, repath behavior, blocked handling,
//! stuck recovery, and infantry sub-cell mechanics using minimal simulation setups.

use super::track_head::committed_track_head;

use super::*;
use crate::map::entities::EntityCategory;
use crate::sim::components::{DriveCoord, MovementTarget, NavTargetRef};
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::test_interner;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;
use crate::util::fixed_math::{SIM_ONE, SIM_ZERO, SimFixed};

// --- Facing calculation tests ---
// Computed deltas use the high byte of the active-retail 65,534-scale word.

#[test]
#[ignore = "native Drive tube-word receiver (track_fresh 4B3298) not ported; only the removed lane entered tubes"]
fn ordinary_drive_retires_selector_before_entering_an_explicit_tube() {
    use crate::map::resolved_terrain::YR_CELL_LAND_TUNNEL;
    use crate::map::tube_facts::{TubeFact, TubeId};
    use crate::sim::pathfinding::zone_map::ZoneGrid;

    let mut cells: Vec<_> = (0..6).map(|x| drivable_slope_cell(x, 0, 0)).collect();
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
    let zones =
        ZoneGrid::build_with_native_bridge_geometry(&grid, &terrain, &[], 6, 1, Some((6, 1)));
    let mut sim = Simulation::with_seed(71);
    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 0, 0);
    entity.owner = sim.intern("Americans");
    entity.type_ref = sim.intern("MTNK");
    entity.category = EntityCategory::Unit;
    entity.body_facing.snap(0x4000, 0);
    entity.locomotor = Some(make_drive_loco_for_test());
    assert!(
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(Default::default()))
    );
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
    // Unit741970 accepts without a route; the first Process requests it.
    let accepted = sim.substrate.entities.get(1).unwrap();
    assert!(
        accepted
            .navigation
            .path_replay
            .remaining_directions()
            .is_empty()
    );
    assert!(committed_track_head(accepted).is_none());
    sim.resolved_terrain = Some(terrain.clone());
    sim.zone_grid = Some(zones.clone());
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    let rules = drive_type_rules();
    sim.process_ground_locomotor_for_test(1, Some(&rules), Some(&grid), None)
        .expect("Process commits the ordinary segment before the tube entrance");
    let accepted = sim.substrate.entities.get(1).unwrap();
    // Find_Path installed (0,0)->(1,0)->tube->(5,0); the fresh head
    // acceptance popped the east word and re-referenced at the head cell.
    assert_eq!(accepted.navigation.path_replay.directions, [2, 8]);
    assert_eq!(accepted.navigation.path_replay.remaining_directions(), [8]);
    assert_eq!(accepted.navigation.path_replay.reference_cell, Some((1, 0)));
    assert_eq!(movement_goal_cell(accepted), Some((5, 0)));
    assert!(committed_track_head(accepted).is_some());
    assert!(
        accepted
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .track()
            .turn_index
            >= 0
    );

    for frame in 1..160 {
        sim.session.binary_frame = frame;
        sim.process_ground_locomotor_for_test(1, Some(&rules), Some(&grid), None)
            .unwrap();
        let entity = sim.substrate.entities.get(1).unwrap();
        if entity.low_bridge_tube_state.is_some() {
            // Terminal4B22AF->4B1F5C writes residual and returns AL=0 at
            // 4B25F9; the outer Process continues into Process_Movement
            // (0x4B0647) and Process_Track(1) (0x4B0AAA), whose direction-8
            // admission 4B12xx enters the tube in the SAME Process.
            assert!(
                committed_track_head(entity).is_none(),
                "ordinary curve retires before tube ownership"
            );
            assert_eq!(
                entity
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.selected_drive_runtime())
                    .and_then(|r| r.retained())
                    .unwrap()
                    .track()
                    .turn_index,
                -1
            );
            assert_eq!((entity.position.rx, entity.position.ry), (1, 0));
            assert_eq!(entity.low_bridge_tube_state.unwrap().cursor, 0);
            assert_eq!(entity.navigation.path_replay.cursor, 2);
            assert_eq!(entity.navigation.path_replay.reference_cell, Some((1, 0)));
            assert_eq!(
                entity
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.selected_drive_runtime())
                    .and_then(|r| r.retained())
                    .unwrap()
                    .head_to(),
                Some(DriveCoord {
                    x: 1408,
                    y: 128,
                    z: 0
                })
            );
            assert!(
                entity
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.selected_drive_runtime())
                    .and_then(|r| r.retained())
                    .unwrap()
                    .track_valid()
            );
            assert!(
                entity
                    .navigation
                    .path_replay
                    .remaining_directions()
                    .is_empty()
            );
            assert!(!entity.lifecycle.cell_marked);
            assert!(!sim.substrate.occupancy.contains_entity(1, 0, 1));
            return;
        }
        assert!(
            committed_track_head(entity).is_some(),
            "the curve cannot retire without the same Process entering the tube"
        );
        assert!(
            entity.position.rx <= 1,
            "tube steps cannot become ordinary ground curves"
        );
    }
    let entity = sim.substrate.entities.get(1).unwrap();
    panic!(
        "ordinary move did not hand movement to the tube: position={:?}, track={:?}, runtime={:?}",
        entity.position,
        entity
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .map(|d| d.track()),
        entity
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .cloned()
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
fn test_drive_arrival_clears_navcom_same_tick() {
    let mut entities = EntityStore::new();

    let mut e = GameEntity::test_default(1, "HTNK", "Americans", 0, 0);
    e.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    e.drive_accelerates = false;
    e.foot_speed.set_speed_fraction(SIM_ONE);
    e.navigation.nav_com = Some(NavTargetRef::cell(0, 0));
    assert!(
        e.locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(
                crate::sim::movement::DriveLocomotionRuntime::default()
                    .with_destination_for_test(Some(crate::sim::components::DriveCoord::cell(
                        0, 0, 0
                    )))
                    .with_head_to_for_test(Some(crate::sim::components::DriveCoord::cell(0, 0, 0)))
                    .with_track_valid_for_test(true)
                    .with_target_speed_fraction_for_test(SIM_ONE)
                    .with_track_for_test(crate::sim::components::TrackProgress {
                        turn_index: 0,
                        cursor: drive_track::raw_track_points(1).len() as i32,
                        residual: 7,
                        ..Default::default()
                    })
            ))
    );
    e.movement_target = Some(MovementTarget {
        // One live speed lepton plus residual7 pays the terminal point.
        speed: SimFixed::from_num(15),
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
    let drive = entity
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_drive_runtime())
        .and_then(|r| r.retained())
        .expect("drive state");
    assert_eq!(drive.head_to(), None);
    assert!(!drive.track_valid());
    assert_eq!(drive.track().turn_index, -1);
    assert_eq!(drive.track().cursor, 0);
    assert_eq!(drive.destination(), None);
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

    // The queued Drive order re-enters the ordinary setter: the destination
    // is replaced and the first Process requests the route.
    let entity = entities.get(1).expect("entity exists");
    let movement = entity.movement_target.as_ref().expect("movement target");
    assert!(
        entity
            .navigation
            .path_replay
            .remaining_directions()
            .is_empty()
    );
    assert_eq!(movement.final_goal, None);
    assert_eq!(movement_goal_cell(entity), Some((4, 0)));
    assert_eq!(
        entity
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .and_then(|d| d.destination()),
        Some(DriveCoord {
            x: 4 * 256 + 128,
            y: 128,
            z: 0
        })
    );
    assert_eq!(entity.navigation.nav_com, Some(NavTargetRef::cell(4, 0)));
    assert!(
        entity.navigation.nav_queue.is_empty(),
        "standard player/team/trigger movement must not create Foot NavQueue entries"
    );
}

/// The fixture Drive types: a Unit's Process reads its type.
/// The order-time adapter (here a Jumpjet's) admits a destination in the
/// mover's own cell without asking the AStar core, whose start-equals-goal
/// exit has no route for it (0x00429BF3..0x00429C0A): the setters record any
/// destination unsearched. A queued order to the same cell appends nothing.
#[test]
fn order_time_admission_accepts_a_goal_in_the_start_cell() {
    let mut entities = EntityStore::new();
    let grid = PathGrid::new(10, 10);
    let mut e = GameEntity::test_default(1, "JUMPJET", "Americans", 5, 5);
    e.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Jumpjet));
    entities.insert(e);
    for queue in [false, true] {
        assert!(
            issue_move_command(
                &mut entities,
                &grid,
                1,
                (5, 5),
                SimFixed::from_num(1024),
                queue,
                None,
                None,
                None,
                crate::sim::movement::DestinationTiming::new(0, 60),
            ),
            "queue={queue}"
        );
    }
    let movement = entities.get(1).unwrap().movement_target.as_ref().unwrap();
    assert_eq!(movement.final_goal, Some((5, 5)));
}

fn drive_type_rules() -> crate::rules::ruleset::RuleSet {
    crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[VehicleTypes]\n0=HTNK\n1=MTNK\n2=DRIVE\n[HTNK]\nSpeed=6\n[MTNK]\nSpeed=6\n[DRIVE]\nSpeed=6\n",
    ))
    .unwrap()
}

fn gsi_04_05_tick_production_movement(
    sim: &mut Simulation,
    path_grid: Option<&PathGrid>,
    native_frame: u32,
) {
    sim.session.tick = u64::from(native_frame);
    sim.session.binary_frame = native_frame;
    sim.process_ground_locomotor_for_test(1, Some(&drive_type_rules()), path_grid, None)
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
        let mut retained = FootPathRuntime::at_frame(0);
        retained.movement_timer = timer;
        retained.blocked_timer = CdTimer::started(100, 60);
        retained.path_blocked = true;
        retained.retries_left = u32::MAX;
        let actor = sim.substrate.entities.get_mut(1).unwrap();
        actor.navigation.path_runtime = retained;
        let position = actor.position;
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
            assert!(
                actor
                    .navigation
                    .path_replay
                    .remaining_directions()
                    .is_empty()
            );
            assert!(actor.locomotor.as_ref().unwrap().step_head().is_none());
            assert_eq!(sim.scenario_rng.logical_state(), rng);
        }

        // Native null setter writes persistent timers even without an adapter;
        // the next accepted order must not recreate or truncate the dword count.
        sim.substrate.entities.get_mut(1).unwrap().movement_target = None;
        sim.session.binary_frame = 200;
        assert!(sim.set_infantry_null_destination(1, None, None));
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
            actor.locomotor.as_ref().unwrap().walk_destination_cell(),
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
    crate::sim::arena_fixture::supply_native_map(sim);
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

/// A slope cell with a clear Track row, for a Drive that moves across it.
fn drivable_slope_cell(
    rx: u16,
    ry: u16,
    slope_type: u8,
) -> crate::map::resolved_terrain::ResolvedTerrainCell {
    let clear = crate::rules::terrain_rules::SpeedCostProfile {
        track: Some(100),
        ..Default::default()
    };
    crate::map::resolved_terrain::ResolvedTerrainCell {
        slope_type,
        base_speed_costs: clear,
        ..drive_speed_test_cell(rx, ry, clear)
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
fn drive_slope_boundary_is_detected_on_process_after_forced_track_crossing() {
    let terrain = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
        1,
        2,
        vec![drivable_slope_cell(0, 0, 4), drivable_slope_cell(0, 1, 10)],
    );
    let mut sim = Simulation::with_seed(4);
    let mut entity = GameEntity::test_default(1, "DRIVE", "Americans", 0, 0);
    entity.owner = sim.intern("Americans");
    entity.type_ref = sim.intern("DRIVE");
    entity.locomotor = Some(LocomotorState::for_test_kind_at_frame(
        LocomotorKind::Drive,
        0,
    ));
    assert!(
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(Default::default()))
    );
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
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    assert!(sim.force_track(1, 0x47, DriveCoord { x: 0, y: 256, z: 0 }, None, None));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .foot_speed
        .set_speed_fraction(SIM_ONE);

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
fn walk_arrival_keeps_list_order_and_snapshot_continuation() {
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
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    // Full publication reads Map Size from the bridge-runtime map owner, even
    // for an empty bridge set. Preserve this non-square fixture's 6x3 geometry.
    sim.bridge_state = Some(
        crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain_with_map_size(
            &terrain,
            true,
            rules.bridge_rules.strength,
            sim.map_size_diamond().unwrap(),
        ),
    );
    // Publish after supplying map geometry, matching the native load phase.
    assert!(sim.rebuild_dynamic_navigation(&rules));
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
    let walk_tick = |sim: &mut Simulation, frame: u32| {
        sim.session.tick = u64::from(frame);
        sim.session.binary_frame = frame;
        sim.process_ground_locomotor_for_test(walker, Some(&rules), Some(&grid), None)
            .unwrap();
    };
    let initial_cell = (1, 1);
    let mut previous_cell = initial_cell;
    // Native Infantry admission shares the resident's cell on a free
    // sub-cell; each accepted arrival prepends the walker to its new cell.
    let mut snapshotted = false;
    let mut restored: Option<Simulation> = None;
    let mut trace = Vec::new();
    for frame in 0..400 {
        let before_generation = sim.substrate.occupancy.generation();
        let before_route = sim
            .substrate
            .entities
            .get(walker)
            .unwrap()
            .navigation
            .path_replay
            .clone();
        walk_tick(&mut sim, frame);
        if let Some(loaded) = restored.as_mut() {
            walk_tick(loaded, frame);
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
            if !snapshotted {
                snapshotted = true;
                let bytes = GameSnapshot::save(&sim, 0, 0, "arrival", 0);
                let mut loaded = GameSnapshot::load(&bytes).unwrap().sim;
                assert_eq!(
                    loaded
                        .bridge_state
                        .as_ref()
                        .unwrap()
                        .native_zone_source_size(),
                    sim.bridge_state.as_ref().unwrap().native_zone_source_size(),
                );
                loaded.retain_in_scenario_process_state_from(&sim);
                loaded.restore_after_snapshot_load().unwrap();
                loaded.resolve_type_handles(&rules);
                assert!(loaded.zone_grid.as_ref().unwrap().is_native_load_pending());
                loaded.rebuild_caches_after_load(
                    terrain.clone(),
                    sim.terrain_speed_config.clone(),
                    &rules,
                );
                assert!(loaded.rebuild_dynamic_navigation(&rules));
                assert!(!loaded.zone_grid.as_ref().unwrap().is_native_load_pending());
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
        } else if sim.substrate.occupancy.generation() != before_generation {
            // Walk's completed-head corridor 75BD70 performs Mark REMOVE/PUT
            // even when the polar step already entered this cell.
            assert_eq!(sim.substrate.occupancy.generation(), before_generation + 2);
            // 75BD89 consumes the completed head's Foot+5E0 word, if any.
            assert!(
                before_route.remaining_directions().is_empty()
                    || entity.navigation.path_replay.cursor > before_route.cursor
            );
        }
        trace.push((
            frame,
            cell,
            crate::sim::movement::ground_pose::position_world_xy(&entity.position),
            entity.sub_cell,
            sim.scenario_rng.state(),
            entity.locomotor.as_ref().and_then(|l| l.step_head()),
            entity.navigation.path_replay.clone(),
        ));
        if entity.movement_target.is_none() {
            break;
        }
    }
    assert!(
        snapshotted,
        "the walker never arrived in a new cell; trace={trace:?}"
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
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let mut entity = GameEntity::test_default(1, "CMIN", "Americans", 13, 11);
    entity.owner = sim.intern("Americans");
    entity.type_ref = sim.intern("CMIN");
    entity.category = EntityCategory::Unit;
    entity.locomotor = Some(make_drive_loco_for_test());
    entity.drive_accelerates = false;
    assert!(
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(
                crate::sim::movement::DriveLocomotionRuntime::default().with_track_for_test(
                    crate::sim::components::TrackProgress {
                        residual: 5,
                        ..Default::default()
                    }
                )
            ))
    );
    sim.substrate.entities.insert(entity);
    assert!(matches!(
        sim.reveal_entity_with_rules(1, &rules),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .cached_spatial_threat()
            .is_some()
    );
    let head = DriveCoord {
        x: 13 * 256,
        y: 12 * 256,
        z: 0,
    };
    // Unlimbo runs Foot idle's speed(0) receiver. This force-track fixture's
    // paid speed is prior state at Force_Track, after ordinary placement.
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .foot_speed
        .set_speed_fraction(SimFixed::lit("0.25"));
    assert!(sim.force_track(1, 0x47, head, None, None));
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(entity.foot_speed.applied_fraction(), SimFixed::lit("0.25"));
    let drive = entity
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_drive_runtime())
        .and_then(|r| r.retained())
        .unwrap();
    assert_eq!(drive.destination(), Some(head));
    assert_eq!(drive.head_to(), Some(head));
    assert_eq!(drive.track().turn_index, 0x47);
    assert_eq!(drive.track().cursor, 0);
    assert_eq!(drive.track().residual, 5);
    assert!(drive.track_valid());
    assert!(sim.substrate.occupancy.contains_entity(13, 11, 1));
    assert!(!sim.substrate.occupancy.contains_entity(13, 12, 1));
    let mut previous_cell = (13, 11);
    let mut crossed = false;
    for frame in 0..128 {
        sim.session.binary_frame = frame;
        sim.process_ground_locomotor_for_test(1, Some(&rules), None, None)
            .unwrap();
        let entity = sim.substrate.entities.get(1).unwrap();
        // Only the track end's same-Process continuation requests a route.
        assert!(committed_track_head(entity).is_none() || entity.movement_target.is_none());
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
    let drive = entity
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_drive_runtime())
        .and_then(|r| r.retained())
        .unwrap();
    // The terminal keeps +34 (no NavCom skips the arrival arm, 0x4B2121), so
    // the same Process continues into Process_Movement;
    // `forced_track_end_requests_its_own_cell_in_the_same_process` covers
    // that continuation.
    assert_eq!(drive.head_to(), None);
    assert_eq!(drive.occupation_head_to(), None);
    assert!(!drive.track_valid());
    assert_eq!(drive.track().turn_index, -1);
    assert_eq!(drive.track().cursor, 0);
    assert_eq!(
        sim.substrate
            .cell_occupation
            .vehicle_bits(13, 12, MovementLayer::Ground),
        crate::sim::occupancy::VEHICLE_OCCUPATION_BIT
    );
}

// --- GSI-06.02: order-time reachability gate + zone-constrained substitution ---

/// A 5x1 corridor blocked at x=2, so `(0,0)/(1,0)` and `(3,0)/(4,0)` are two
/// disconnected zones for a ground mover.
fn split_corridor_fixture() -> (PathGrid, crate::sim::pathfinding::zone_map::ZoneGrid) {
    let mut grid = PathGrid::new(5, 1);
    grid.set_blocked(2, 0, true);
    let zone_grid = crate::sim::pathfinding::zone_map::ZoneGrid::following_path_grid(&grid);
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

/// A cross-zone ground move order is accepted unchanged: Unit741970 installs
/// NavCom and the Drive destination without a search or redirect. The player
/// click reaches it only through the ordinary 4DE1D0 resolver, which picks a
/// reachable cell (move_cell_input); the first Process drops an unreachable
/// destination through Find_Path's precheck and the continuation's recheck
/// (track_path_continuation).
#[test]
fn gsi_06_02_cross_zone_move_order_is_accepted_without_redirect() {
    let (grid, zone_grid) = split_corridor_fixture();
    let mut entities = EntityStore::new();
    let mut mover = GameEntity::test_default(1, "MTNK", "Americans", 0, 0);
    mover.category = EntityCategory::Unit;
    mover.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    assert!(
        mover
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(Default::default()))
    );
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
    let entity = entities.get(1).unwrap();
    let target = entity.movement_target.as_ref().unwrap();
    assert_eq!(target.final_goal, None);
    assert_eq!(movement_goal_cell(entity), Some((4, 0)));
    assert!(
        entity
            .navigation
            .path_replay
            .remaining_directions()
            .is_empty()
    );
    assert_eq!(entity.navigation.nav_com, Some(NavTargetRef::cell(4, 0)));
}

/// `AStar @ 0x0042CAD6` uses the hierarchy only for a +3D5 (in playfield)
/// mover. A queued Walk order keeps the command-time search adapter, so the
/// order's appended search shows the flat fallback (Drive and Ship search at
/// their first Process through the same `allow_zone_hierarchy` snapshot).
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
    let zone_grid = crate::sim::pathfinding::zone_map::ZoneGrid::following_path_grid(&reduced);
    assert!(!zone_grid.can_reach(
        MovementZone::Normal,
        (0, 0),
        MovementLayer::Ground,
        (4, 0),
        MovementLayer::Ground,
    ));

    let mut entities = EntityStore::new();
    let mut mover = GameEntity::test_default(1, "E1", "Americans", 0, 0);
    mover.category = EntityCategory::Infantry;
    let mut walk = LocomotorState::for_test_kind(LocomotorKind::Walk);
    walk.movement_zone = MovementZone::Normal;
    mover.locomotor = Some(walk);
    mover.in_playfield = false;
    mover.movement_target = Some(MovementTarget::default());
    entities.insert(mover);

    assert!(issue_move_command_with_layered(
        &mut entities,
        &grid,
        1,
        (4, 0),
        SimFixed::from_num(1024),
        true,
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
    // The appended search keeps no cells; its acceptance above is the
    // observable: the hierarchy would have aborted it (no reduced-zone route
    // past (2,0)) and refused the queued order.
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
    // The command-time search only admits the order; the adapter keeps the
    // goal and speed, and the first Process's Find_Path installs the route.
    assert_eq!(target.final_goal, Some((7, 3)));
    assert!(
        entity
            .navigation
            .path_replay
            .remaining_directions()
            .is_empty()
    );
    assert_eq!(target.speed, SimFixed::from_num(768));
}

// `TechnoClass::Set_Destination` @ `0x00741970` records the new destination
// (NavCom @ `0x004D94B0`, Drive coordinate @ `0x004AFD40`) without touching the
// Drive track cursor: a curve already in flight keeps driving to its committed
// head cell and the new path takes over there.

// The player-visible symptom of replacing an in-flight curve: the fresh curve's
// cursor restarted at the lead-in point on the current cell centre, so every
// mid-drive re-order teleported the body backward by its mid-cell progress.

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
    // The appended search admits the queued order but keeps no cells (the
    // adapter keeps its goal and speed); the acceptance above is what remains.
    assert_eq!(movement.speed, SimFixed::from_num(768));
}

// --- Friendly-passable pathfinding tests ---

#[test]
fn test_friendly_passable_moving_unit_not_blocked() {
    // A moving friendly unit should NOT appear in the entity block set.

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
        speed: SimFixed::from_num(1024),
        ..Default::default()
    });
    // Moving: its Foot+5E0 head word steps east from (4,0).
    b.navigation.path_replay = crate::sim::movement::fixture_path_replay(&[(4, 0), (5, 0), (6, 0)]);
    entities.insert(b);

    let alliances = HouseAllianceMap::new();
    let (blocks, _penalty) = block_index::build_owner_block_set(
        &entities,
        "Americans",
        &alliances,
        &test_interner(),
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

    let mut entities = EntityStore::new();

    // Enemy unit moving at (3, 0).
    let mut enemy = GameEntity::test_default(1, "HTNK", "Russians", 3, 0);
    enemy.lifecycle.in_limbo = false;
    enemy.lifecycle.cell_marked = true;
    enemy.movement_target = Some(MovementTarget {
        speed: SimFixed::from_num(1024),
        ..Default::default()
    });
    // Moving: its Foot+5E0 head word steps east from (3,0).
    enemy.navigation.path_replay = crate::sim::movement::fixture_path_replay(&[(3, 0), (4, 0)]);
    entities.insert(enemy);

    let alliances = HouseAllianceMap::new();
    let (blocks, _penalty) = block_index::build_owner_block_set(
        &entities,
        "Americans",
        &alliances,
        &test_interner(),
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

/// A Drive order on clear ground and its first Process: the order installs
/// no route (Unit741970); the Process's Find_Path installs the segment.
/// Returns the route that Find_Path installed from `start` and the adapter's
/// goal.
fn drive_first_process_route(
    start: (u16, u16),
    goal: (u16, u16),
) -> (Vec<(u16, u16)>, Option<(u16, u16)>) {
    let mut sim = Simulation::with_seed(7);
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    let grid = PathGrid::from_resolved_terrain(sim.resolved_terrain.as_ref().unwrap());
    let mut e = GameEntity::test_default(1, "HTNK", "Americans", start.0, start.1);
    e.owner = sim.intern("Americans");
    e.type_ref = sim.intern("HTNK");
    e.category = EntityCategory::Unit;
    e.locomotor = Some(make_drive_loco_for_test());
    assert!(
        e.locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(Default::default()))
    );
    sim.substrate.entities.insert(e);
    assert!(matches!(
        sim.reveal(1),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    assert!(issue_move_command(
        &mut sim.substrate.entities,
        &grid,
        1,
        goal,
        SimFixed::from_num(1024),
        false,
        None,
        None,
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    let accepted = sim.substrate.entities.get(1).unwrap();
    assert!(
        accepted
            .navigation
            .path_replay
            .remaining_directions()
            .is_empty(),
        "the order installs no route"
    );
    sim.process_ground_locomotor_for_test(1, Some(&drive_type_rules()), Some(&grid), None)
        .unwrap();
    let entity = sim.substrate.entities.get(1).unwrap();
    (
        entity.navigation.path_replay.installed_cells(start),
        movement_goal_cell(entity),
    )
}

#[test]
fn test_short_path_no_truncation() {
    // A 5-step path (well under 24) should be delivered intact.
    let (route, final_goal) = drive_first_process_route((0, 0), (5, 0));
    assert_eq!(route.len(), 6, "5-step path = 6 entries (start + 5 moves)");
    assert_eq!(final_goal, Some((5, 0)));
}

#[test]
fn test_long_path_truncated_to_24_steps() {
    // A path longer than 24 steps should be truncated to 25 entries.
    let (route, final_goal) = drive_first_process_route((0, 0), (40, 0));
    // Path truncated: 24 steps + start = 25 entries.
    assert_eq!(
        route.len(),
        25,
        "Long path should be truncated to 25 entries"
    );
    assert_eq!(route[0], (0, 0), "Path starts at origin");
    assert_eq!(route[24], (24, 0), "Path ends at 24th step");
    assert_eq!(final_goal, Some((40, 0)), "Final goal preserved");
}

#[test]
fn test_blocked_repath_preserves_locomotor_destination_beyond_segment_end() {
    // The locomotor retains the destination beyond the installed path segment.
    let (route, final_goal) = drive_first_process_route((0, 2), (40, 2));
    assert_eq!(final_goal, Some((40, 2)));
    // The segment path ends at (24, 2), but final_goal is (40, 2).
    assert_eq!(route.last(), Some(&(24, 2)));
}

/// Build a minimal Drive LocomotorState for layered-pathfinding tests. Required
/// because the layered A* branch in find_move_path is only entered when the
/// mover has a Drive/Walk locomotor; `test_default` leaves locomotor=None.
fn make_drive_loco_for_test() -> crate::sim::movement::locomotor::LocomotorState {
    crate::sim::movement::locomotor::LocomotorState::for_test_kind(
        crate::rules::locomotor_type::LocomotorKind::Drive,
    )
}

fn drive_speed_test_cell(
    rx: u16,
    ry: u16,
    speed_costs: crate::rules::terrain_rules::SpeedCostProfile,
) -> crate::map::resolved_terrain::ResolvedTerrainCell {
    crate::map::resolved_terrain::ResolvedTerrainCell {
        speed_costs,
        ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
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

/// A Walk infantry (E1, Foot) scene on `terrain` (clear 64x64 when `None`)
/// with the native map inputs a production Walk Process reads.
fn walk_scene(
    seed: u64,
    terrain: Option<crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> (Simulation, crate::rules::ruleset::RuleSet) {
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[Clear]\nFoot=100%\nTrack=100%\nWheel=100%\nFloat=0%\n\
         [E1]\nStrength=100\nSpeed=4\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(seed);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Americans");
    sim.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 0),
    );
    sim.session.house_order.push(owner);
    if let Some(terrain) = terrain {
        sim.install_resolved_terrain_for_new_map(terrain);
    }
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    (sim, rules)
}

fn walk_move(
    sim: &mut Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    id: u64,
    to: (u16, u16),
) {
    assert!(sim.apply_command_with_overlays(
        "Americans",
        &crate::sim::command::Command::Move {
            entity_id: id,
            target_rx: to.0,
            target_ry: to.1,
            queue: false,
        },
        Some(rules),
        None,
    ));
}

fn walk_frame(sim: &mut Simulation, rules: &crate::rules::ruleset::RuleSet) {
    let grid = sim.path_grid_snapshot();
    sim.advance_tick(&[], Some(rules), grid.as_deref(), None, 67);
}

/// Order one infantryman from (0,0) to (2,0) on a 3x1 corridor whose middle
/// cell is `middle`, through the production Move command and frame. Returns
/// the distinct cells he stood in, in order.
fn walk_infantry_corridor(
    middle: crate::map::resolved_terrain::ResolvedTerrainCell,
) -> Vec<(u16, u16)> {
    let clear_costs = crate::rules::terrain_rules::SpeedCostProfile {
        foot: Some(100),
        track: Some(100),
        wheel: Some(100),
        ..Default::default()
    };
    let clear = |cell: crate::map::resolved_terrain::ResolvedTerrainCell| {
        crate::map::resolved_terrain::ResolvedTerrainCell {
            speed_costs: clear_costs,
            base_speed_costs: clear_costs,
            ..cell
        }
    };
    let terrain = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
        3,
        1,
        vec![
            clear(crate::map::resolved_terrain::test_flat_cell(0, 0)),
            clear(middle),
            clear(crate::map::resolved_terrain::test_flat_cell(2, 0)),
        ],
    );
    let (mut sim, rules) = walk_scene(0x75bd25, Some(terrain));
    let id = sim
        .spawn_object("E1", "Americans", 0, 0, 0, &rules)
        .unwrap();
    walk_move(&mut sim, &rules, id, (2, 0));
    let mut visited = vec![(0, 0)];
    for _ in 0..400 {
        walk_frame(&mut sim, &rules);
        let e = sim.substrate.entities.get(id).unwrap();
        let cell = (e.position.rx, e.position.ry);
        if visited.last() != Some(&cell) {
            visited.push(cell);
        }
        if e.navigation.nav_com.is_none() && e.movement_target.is_none() {
            break;
        }
    }
    visited
}

#[test]
fn infantry_walks_a_plain_corridor_end_to_end() {
    let visited = walk_infantry_corridor(crate::map::resolved_terrain::test_flat_cell(1, 0));
    assert_eq!(
        visited.last().copied(),
        Some((2, 0)),
        "control walker must cross a corridor with no terrain object: {visited:?}",
    );
}

/// The search and the runtime step-in have to be ONE predicate, the way the
/// original reaches its cell gate through a single per-class slot.
///
/// A* plans an infantryman straight through a partially-occupied tree cell. If
/// the runtime crossing asked the whole-cell question, the walker would reach
/// the tree, refuse to enter and repath onto the identical route until the
/// stuck counter aborts the order. Every temperate retail map carries hundreds
/// of such cells, so this would fire on ordinary infantry movement.
#[test]
fn infantry_traverses_a_partially_occupied_tree_cell_at_runtime() {
    let (_terrain, grid) = tree_corridor();
    // Precondition: this corridor is exactly the split the test is about.
    assert!(
        !grid.is_walkable(1, 0),
        "the tree closes the cell to the whole-cell view",
    );
    assert!(
        grid.is_walkable_for_infantry(1, 0),
        "one occupation bit leaves sub-cells free for infantry",
    );
    let visited = walk_infantry_corridor(tree_speed_test_cell(1, 0, 4));
    assert!(
        visited.contains(&(1, 0)),
        "the walker never entered the tree cell: {visited:?}",
    );
    assert_eq!(
        visited.last().copied(),
        Some((2, 0)),
        "the walker must come out the far side of the tree cell: {visited:?}",
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
        assert!(
            tank.locomotor
                .as_mut()
                .unwrap()
                .install_drive_state_for_test(Some(Default::default()))
        );
        tank.movement_target = Some(MovementTarget {
            speed: SimFixed::from_num(1024),
            final_goal: Some((2, 0)),
            ..Default::default()
        });
        entities.insert(tank);

        let mut lifecycle_requests = Vec::new();
        let mut occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(0);
        let mut interner = test_interner();
        for tick in 0..120u64 {
            let _ = tick_movement_with_grid(
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

// ============================================================================
// Bridge on_bridge timing integration tests (Plan: 2026-05-11 G2 fix).
// Pin: predicate fires at Ramp→Body exactly, clears at Ramp→Ground exactly,
// no anticipatory on_bridge pre-claim.
// ============================================================================

use crate::map::houses::HouseAllianceMap;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone};
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::tick_movement_with_grid;
use crate::sim::pathfinding::PathGrid;

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
    // The fresh arm reads the map cells.
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_flat_ground_grid(20));
    let grid = PathGrid::new(20, 20);
    sim.process_ground_locomotor_for_test(1, Some(&drive_type_rules()), Some(&grid), None)
        .unwrap();
    let entity = sim.substrate.entities.get(1).unwrap();
    let drive_locomotion = &entity
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_drive_runtime())
        .and_then(|r| r.retained())
        .cloned();

    assert!(
        drive_locomotion.as_ref().unwrap().head_to().is_some(),
        "the null-curve substitute should have started a straight drive track"
    );
    assert_eq!(
        entity.navigation.path_replay.cursor, 1,
        "the substitute must consume exactly one path node, not two"
    );
    let drive = drive_locomotion.as_ref().expect("substitute curve");
    assert_eq!(
        drive_track::turn_track_at(drive.track().turn_index as usize)
            .unwrap()
            .normal_track,
        1,
        "the straight cardinal curve"
    );
    assert_eq!(
        drive.head_to().map(|head| (head.x, head.y)),
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
    // The fresh arm reads the map cells.
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_flat_ground_grid(20));
    let grid = PathGrid::new(20, 20);
    sim.process_ground_locomotor_for_test(1, Some(&drive_type_rules()), Some(&grid), None)
        .unwrap();
    let entity = sim.substrate.entities.get(1).unwrap();
    let drive_locomotion = &entity
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_drive_runtime())
        .and_then(|r| r.retained())
        .cloned();
    assert_eq!(entity.navigation.path_replay.cursor, 0);

    assert!(
        drive_locomotion.as_ref().unwrap().head_to().is_none(),
        "no curve while the hull is off-octant"
    );
    assert_eq!(
        entity.body_facing.destination(),
        0x6000,
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
    e.body_facing.snap(u16::from(facing) << 8, 0);
    e.lifecycle.in_limbo = false;
    e.lifecycle.cell_marked = true;
    let goal = *path.last().expect("non-empty path");
    e.movement_target = Some(MovementTarget {
        speed: SimFixed::from_num(768),
        final_goal: Some(goal),
        ..Default::default()
    });
    e.navigation.path_replay = crate::sim::components::FootPathQueue {
        directions,
        cursor: 0,
        reference_cell: Some((start.0 as i16, start.1 as i16)),
    };
    e.foot_speed.set_speed_fraction(SIM_ONE);
    assert!(
        e.locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(
                crate::sim::movement::DriveLocomotionRuntime::default()
                    .with_target_speed_fraction_for_test(SIM_ONE)
            ))
    );
    // A seeded path is only the route adapter. Native terminal +504 checks
    // NavCom independently; install the real destination before executing it.
    super::navcom::set_destination_internal_cell(&mut e, goal, None, 0);
    e
}

// ---------------------------------------------------------------------------
// Drive-track cell crossings are coordinate-derived, not path-derived
// ---------------------------------------------------------------------------

// Reproduces the eight-GI ordinary-move report through the production Move
// command and frame. Native scatter gate vectors are retained in
// tools/infantry_scatter_oracle.json.
#[test]
fn group_gis_do_not_jump_or_lose_their_goal() {
    let mut reached = 0;
    for seed in 0..8 {
        let (mut sim, rules) = walk_scene(seed, None);
        let ids: Vec<u64> = (1..=8u16)
            .map(|n| {
                sim.spawn_object("E1", "Americans", 10 + n % 3, 10 + n / 3, 0, &rules)
                    .unwrap()
            })
            .collect();
        for &id in &ids {
            walk_move(&mut sim, &rules, id, (3, 3));
        }
        for tick in 0..1000 {
            let previous: Vec<_> = ids
                .iter()
                .map(|&id| sim.substrate.entities.get(id).unwrap().position)
                .collect();
            walk_frame(&mut sim, &rules);
            for (n, &id) in ids.iter().enumerate() {
                let e = sim.substrate.entities.get(id).unwrap();
                let p = &e.position;
                let old = &previous[n];
                let dx = (i32::from(p.rx) - i32::from(old.rx)) * 256
                    + (p.sub_x - old.sub_x).to_num::<i32>();
                let dy = (i32::from(p.ry) - i32::from(old.ry)) * 256
                    + (p.sub_y - old.sub_y).to_num::<i32>();
                assert!(
                    dx * dx + dy * dy <= 32 * 32,
                    "unexpected jump seed={seed} tick={tick} GI={id} delta=({dx},{dy})"
                );
                if e.movement_target.is_some() {
                    if e.locomotor.as_ref().unwrap().walk_destination_cell() != Some((3, 3)) {
                        let dx = i32::from(p.rx) - 3;
                        let dy = i32::from(p.ry) - 3;
                        assert!(
                            dx * dx + dy * dy <= 9,
                            "only a GI that reached the destination may be scattered: seed={seed} tick={tick} id={id}"
                        );
                    }
                }
            }
            if ids.iter().all(|&id| {
                let e = sim.substrate.entities.get(id).unwrap();
                e.movement_target.is_none() && e.navigation.nav_com.is_none()
            }) {
                break;
            }
        }
        for &id in &ids {
            let e = sim.substrate.entities.get(id).unwrap();
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

// Clock regression for the still-existing deferred crossing corridor. This
// exercises its actual Process caller, not native crossing geometry parity.
