use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::{DriveCoord, FootPathQueue, NavTargetRef, TrackProgress};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::DriveLocomotionRuntime;
use crate::sim::movement::drive_track;
use crate::sim::movement::locomotor::LocomotorState;
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
    entity.set_category_for_test(EntityCategory::Unit);
    entity.lifecycle.set_object_alive_for_test(true);
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.lifecycle.set_cell_marked(true);
    entity.set_is_voxel_for_test(false);
    entity.mover_facts.set_drive_accelerates_for_test(false);
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    assert!(
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(
                DriveLocomotionRuntime::default()
                    .with_head_to_for_test(Some(DriveCoord::cell(10, 9, 0)))
                    .with_track_valid_for_test(true)
                    .with_target_speed_fraction_for_test(SimFixed::from_num(1))
                    .with_track_for_test(TrackProgress::for_test(0, 0, false, 0))
            ))
    );
    // The accepted head (10,9) was the route's last cell, so Foot+5E0 holds
    // no word beyond it: the committed head alone models the route.
    sim.interner = crate::sim::intern::test_interner();
    // Drive ProcessMovement4B3673..368D stores the frame anchor and
    // duration. Active TrackProcess4B0F20 does not mutate that timer.
    entity
        .navigation
        .path_runtime
        .set_blocked_timer_for_test(crate::sim::timer::CdTimer::started(10, 9));
    sim.substrate.entities.insert(entity);
    sim.substrate.occupancy =
        crate::sim::occupancy::OccupancyGrid::rebuild(&sim.substrate.entities);
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    (sim, rules)
}

/// Foot4DA53E reloads Object+90 after a supplied Techno callback; only its
/// live continuation clears6B3 at4DA54E. Compare the unchanged-liveness
/// controls at the existing per-object boundary, with no rules/controller
/// work. The dead native row supplies no Conceal/UnInit producer and is not
/// a valid full-world destruction fixture. Whole TechnoAI, the dispatcher and
/// its death lifecycle remain outside this original corpus's coverage.
#[test]
fn object_turn_resets_foot_idle_latch_only_on_native_live_continuation() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/foot_enter_idle.json",
    ))
    .expect("unchanged original Foot EnterIdle corpus");
    for alive in [false, true] {
        let row = rows
            .iter()
            .find(|row| {
                let input = &row["input"];
                input["group"] == "foot_ai_reset"
                    && input["entry_alive"] == u8::from(alive)
                    && input["callback_alive"] == u8::from(alive)
                    && input["entry_latch"] == 1
                    && input["callback_latch"] == 1
            })
            .expect("original unchanged-liveness reset control");
        let (mut sim, _) = fixture();
        let actor = sim.substrate.entities.get_mut(1).unwrap();
        actor.locomotor = None;
        actor.lifecycle.set_object_alive_for_test(alive);
        actor.mission_leaf.set_foot_idle_entry_latch(1);
        let rng = sim.rng_state();
        sim.advance_live_object_turn(1, None, techno_ai::ObjectAiCtx::default())
            .expect("bounded object-turn continuation");
        let actor = sim
            .substrate
            .entities
            .get(1)
            .expect("no terminal cleanup supplied");
        assert_eq!(
            actor.mission_leaf.foot_idle_entry_latch(),
            row["after"]["latch"].as_u64().unwrap() as u8
        );
        assert_eq!(sim.rng_state(), rng);
    }
}

#[test]
fn terminal_sensor_receiver_finishes_before_shp_and_is_not_deferred_to_cell_change() {
    let (mut sim, rules) = fixture();
    // Install a real sensor deposit at the old recorded center, then supply
    // the terminal in the current cell. PerCell2 must remove/add even though
    // this object's cell is unchanged during its movement visit.
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .position
        .set_rx_for_test(5);
    sim.add_unit_sensor_after_unlimbo(1, &rules, None);
    let owner = sim.substrate.entities.get(1).unwrap().owner();
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.position.set_rx_for_test(10);
    entity
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::cell(10, 10)));
    let drive = entity.locomotor.as_mut().unwrap();
    assert!(drive.store_track_head(
        crate::sim::movement::track_process::TrackFamily::Drive,
        Some(DriveCoord::cell(10, 10, 0))
    ));
    assert!(drive.store_track_destination(
        crate::sim::movement::track_process::TrackFamily::Drive,
        drive.track_head(crate::sim::movement::track_process::TrackFamily::Drive)
    ));
    {
        let mut progress = drive
            .track_progress(crate::sim::movement::track_process::TrackFamily::Drive)
            .unwrap();
        progress.set_cursor(drive_track::raw_track_points(1).len() as i32);
        assert!(drive.store_track_progress(
            crate::sim::movement::track_process::TrackFamily::Drive,
            progress
        ));
    };
    entity.body_frame_counter.set_for_test(5);
    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .expect("fixture object turn must complete");
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(entity.sensor_deposit.unwrap().center(), (10, 10));
    assert!(!sim.fog.has_sensor_for_house(owner, 5, 10));
    assert!(sim.fog.has_sensor_for_house(owner, 10, 10));
    assert!(entity.navigation.nav_com().is_none());
    assert!(crate::sim::movement::motion_query::is_moving(&entity) != Some(true));
    assert_eq!(
        entity.body_frame_counter.get(),
        5,
        "terminal retired movement before IdleRate=0 SHP admission"
    );
    assert!(entity.foot_occupation_enabled());
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
    sim.add_unit_sensor_after_unlimbo(1, &rules, None);
    let selection = drive_track::select_drive_track(32, 64, false).unwrap();
    assert!(selection.entry_index > 0);
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.body_facing.snap(0x2000, 0);
    entity.navigation.path_replay = FootPathQueue::for_test(vec![2, 3], 0, Some((10, 10)));
    assert!(entity.locomotor.as_mut().unwrap().store_track_progress(
        crate::sim::movement::track_process::TrackFamily::Drive,
        TrackProgress::for_test(
            1,
            i32::from(drive_track::raw_track_meta(3).unwrap().chain_index),
            false,
            0
        )
    ));
    let outcome = sim
        .advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .expect("fixture object turn must complete");
    let entity = sim.substrate.entities.get(1).unwrap();
    let track = entity
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_drive_runtime())
        .and_then(|r| r.retained())
        .unwrap()
        .track();
    assert_eq!(track.turn_index(), selection.turn_track_index as i32);
    assert!(
        entity
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .track_valid()
    );
    assert!(track.cursor() > i32::from(selection.entry_index));
    assert_eq!(entity.navigation.path_replay.cursor(), 1);
    // Retail RawTrack[3]7E7A58 -> point37 at7E66B4=(-136,136,32).
    // TurnTrack[1]7E7B34 flags8 leaves XY unchanged in Transform4B4780.
    // With head(2688,2432), PerCell4B1CFD runs in signed cell(9,10).
    assert_eq!(entity.sensor_deposit.unwrap().center(), (9, 10));
    // Live MTNK Speed=6 gives budget15: the next paid point must move away
    // from the callback's exact coordinates, even if it stays in that cell.
    let current = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    assert_ne!((current.x, current.y), (2552, 2568));
    assert!(outcome.movement.moved_steps >= 2);
    assert_eq!(
        entity
            .navigation
            .path_runtime
            .blocked_timer()
            .remaining(sim.session.binary_frame as i32),
        9
    );
    assert_eq!(
        entity.navigation.path_runtime.blocked_timer(),
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
            // TrackProcess queries the live type speed: Speed=0 pays nothing.
            let rules = RuleSet::from_ini(&IniFile::from_str(
                "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=500\nSpeed=0\nSensorsSight=1\nWalkRate=1\nIdleRate=0\n",
            ))
            .unwrap();
            let entity = sim.substrate.entities.get_mut(1).unwrap();
            if let Some(loco) = entity.locomotor.as_mut() {
                let _ = loco.install_drive_state_for_test(None);
            };
            if let Some(loco) = entity.locomotor.as_mut() {
                let _ = loco.install_ship_state_for_test(None);
            };
            entity.locomotor = Some(LocomotorState::for_test_kind(kind));
            let grid = crate::sim::pathfinding::PathGrid::new(32, 32);
            assert!(sim.set_unit_destination(
                1,
                NavTargetRef::cell(10, 9),
                &rules,
                true,
                crate::sim::world::FrameEffects::default(),
            ));
            if reload {
                let bytes = GameSnapshot::save(&sim, 0, 0, "pending_apply", 0);
                sim = GameSnapshot::load(&bytes).unwrap().sim;
                // The map inputs come from the scenario, not the save.
                crate::sim::arena_fixture::supply_native_map(&mut sim);
            }
            let entity = sim.substrate.entities.get(1).unwrap();
            let head = if kind == LocomotorKind::Drive {
                let state = entity
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.selected_drive_runtime())
                    .and_then(|r| r.retained())
                    .unwrap();
                assert!(!state.track_valid());
                state.head_to()
            } else {
                entity
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.selected_ship_runtime())
                    .and_then(|r| r.retained())
                    .unwrap()
                    .head_to()
            };
            assert!(head.is_none(), "the command has not run ProcessMovement");
            assert_eq!(
                sim.substrate.raw_cell_occupation.ground_bits(10, 9) & 0x20,
                0
            );
            sim.install_fixture_path_grid(Some(&grid));
            sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
                .expect("fixture object turn must complete");
            let entity = sim.substrate.entities.get(1).unwrap();
            if kind == LocomotorKind::Drive {
                let drive = entity
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.selected_drive_runtime())
                    .and_then(|r| r.retained())
                    .unwrap();
                assert!(drive.track_valid());
                assert_eq!((drive.track().cursor(), drive.track().residual()), (0, 0));
            } else {
                let ship = entity
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.selected_ship_runtime())
                    .and_then(|r| r.retained())
                    .unwrap();
                assert_eq!((ship.track().cursor(), ship.track().residual()), (0, 0));
            }
            assert_eq!(sim.current_speed_for_test(1, &rules), 0);
            assert_eq!(
                sim.substrate.raw_cell_occupation.ground_bits(10, 9) & 0x20,
                0x20
            );
            sim.substrate.raw_cell_occupation.clear_ground(10, 9, 0x20);
            sim.install_fixture_path_grid(Some(&grid));
            sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
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
    entity
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::cell(10, 9)));
    let drive = entity.locomotor.as_mut().unwrap();
    assert!(drive.store_track_destination(
        crate::sim::movement::track_process::TrackFamily::Drive,
        drive.track_head(crate::sim::movement::track_process::TrackFamily::Drive)
    ));
    {
        let mut progress = drive
            .track_progress(crate::sim::movement::track_process::TrackFamily::Drive)
            .unwrap();
        progress.set_cursor(drive_track::raw_track_points(1).len() as i32);
        assert!(drive.store_track_progress(
            crate::sim::movement::track_process::TrackFamily::Drive,
            progress
        ));
    };
    entity.foot_speed.set_speed_fraction(SimFixed::from_num(1));
    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .expect("fixture object turn must complete");
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .foot_speed
            .applied_fraction(),
        SimFixed::from_num(0)
    );
    assert_eq!(sim.current_speed_for_test(1, &rules), 0);
    let grid = crate::sim::pathfinding::PathGrid::new(32, 32);
    assert!(sim.set_unit_destination(
        1,
        NavTargetRef::cell(10, 6),
        &rules,
        true,
        crate::sim::world::FrameEffects::default(),
    ));
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.mover_facts.set_drive_accelerates_for_test(true);
    // The first Process after the order requests the route from the grid.
    sim.install_fixture_path_grid(Some(&grid));
    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .expect("fixture object turn must complete");
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .foot_speed
            .applied_fraction(),
        SimFixed::from_num(0.03)
    );
}

#[test]
fn ship_fresh_claim_survives_next_object_visit_and_snapshot() {
    use crate::sim::snapshot::GameSnapshot;
    let (mut sim, rules) = fixture();
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Ship));
    if let Some(loco) = entity.locomotor.as_mut() {
        let _ = loco.install_drive_state_for_test(None);
    };
    if let Some(loco) = entity.locomotor.as_mut() {
        let _ = loco.install_ship_state_for_test(None);
    };
    let grid = crate::sim::pathfinding::PathGrid::new(32, 32);
    assert!(sim.set_unit_destination(
        1,
        NavTargetRef::cell(10, 7),
        &rules,
        true,
        crate::sim::world::FrameEffects::default(),
    ));
    sim.install_fixture_path_grid(Some(&grid));
    let ctx = techno_ai::ObjectAiCtx::default;
    sim.advance_live_object_turn(1, Some(&rules), ctx())
        .expect("fixture object turn must complete");
    let head = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_ship_runtime())
        .and_then(|r| r.retained())
        .unwrap()
        .head_to()
        .unwrap();
    let mark = ((head.x / 256) as u16, (head.y / 256) as u16);
    assert!(
        !sim.substrate
            .entities
            .get(1)
            .unwrap()
            .foot_occupation_enabled()
    );
    sim.advance_live_object_turn(1, Some(&rules), ctx())
        .expect("fixture object turn must complete");
    let bytes = GameSnapshot::save(&sim, 0, 0, "ship_head", 0);
    let loaded = GameSnapshot::load(&bytes).unwrap().sim;
    assert_eq!(
        loaded
            .substrate
            .raw_cell_occupation
            .ground_bits(mark.0, mark.1)
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
    assert!(entity.locomotor.as_mut().unwrap().store_track_progress(
        crate::sim::movement::track_process::TrackFamily::Drive,
        TrackProgress::for_test(1, 37, false, 0)
    ));
    entity.navigation.path_replay = FootPathQueue::for_test(vec![2, 3], 0, Some((10, 10)));
    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .expect("fixture object turn must complete");
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        entity
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .track()
            .turn_index(),
        1
    );
    assert_eq!(entity.navigation.path_replay.cursor(), 0);
}
