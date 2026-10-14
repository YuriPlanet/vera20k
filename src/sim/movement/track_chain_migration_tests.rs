//! Production Process_Track coverage replacing the retired deferred executor.
//!
//! This file is a child of movement_tick so the two independent path-cache
//! regressions keep access to their existing private helper. Chain fixtures
//! execute the shared paid-loop body with supplied budgets and world receivers;
//! they exclude the entry guard and scalar speed prefix.
//! These are Rust integration checks, not new native parity goldens.
//!
//! Coverage accounting: the old code0/code2 truth table used Process_Movement
//! offsets for Process_Track and is intentionally replaced by the real host's
//! code0/code2 + Passive matrix (jump table4B2608, Unit+2C746E20). The gate-open
//! and code6 cases now execute their production receivers. Code2 does not run
//! the fresh-selection Find_Path ladder. Eager admission crush is replaced by
//! reached-cell PerCell lifecycle checks. Native selector/cursor publication
//! and callback mutation comparisons remain in track_process_tests and
//! track_host_tests::accepted_chain_preserves_call_budget_and_reloads_callback_queue_once.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::components::{DriveCoord, FootPathQueue, TrackProgress};
use crate::sim::game_entity::{BuildingGateRuntime, GameEntity};
use crate::sim::intern::test_interner;
use crate::sim::mission::MissionType;
use crate::sim::movement::DriveLocomotionRuntime;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::track_host::TrackWorldEvent;
use crate::sim::movement::track_process::{TrackFamily, TrackInvocation};
use crate::sim::world::LifecycleTestEvent;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;

const MOVER: u64 = 1;
const CANDIDATE: (u16, u16) = (11, 9);

fn chain_fixture(passive: bool) -> (Simulation, RuleSet, TrackInvocation) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=MTNK\n[InfantryTypes]\n0=E1\n[BuildingTypes]\n0=GAGATE_A\n\
         [MTNK]\nSpeed=4\nCrusher=yes\nPassive={}\n\
         [E1]\nCrushable=yes\nStrength=100\nCrushSound=Squish\n\
         [GAGATE_A]\nFoundation=3x1\nGate=yes\nDeployTime=.044\nGateCloseDelay=.2\n",
        if passive { "yes" } else { "no" },
    )))
    .unwrap();
    let mut sim = Simulation::with_seed(17);
    // The chain query (0x4B1C3E) asks the native Unit+1AC over map cells.
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_flat_ground_grid(
        24,
    ));
    let head = DriveCoord::cell(10, 9, 0);
    let mut mover = GameEntity::test_default(MOVER, "MTNK", "Americans", 10, 10);
    mover.set_category_for_test(EntityCategory::Unit);
    mover.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    mover.lifecycle.set_object_alive_for_test(true);
    mover.lifecycle.set_in_limbo_for_test(false);
    mover.lifecycle.cell_marked = true;
    assert!(
        mover
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(
                DriveLocomotionRuntime::default()
                    .with_head_to_for_test(Some(head))
                    .with_track_valid_for_test(true)
                    .with_track_for_test(TrackProgress {
                        turn_index: 1,
                        cursor: 37,
                        reversed: false,
                        residual: 0,
                    })
            ))
    );
    mover.navigation.path_replay = FootPathQueue {
        directions: vec![2, 3],
        cursor: 0,
        reference_cell: Some((10, 10)),
    };
    // Stand at the actual raw3 chain sample, keeping unrelated crossing effects
    // outside these chain-dispatch fixtures.
    let point = super::super::drive_track::raw_track_points(3)[37];
    let xy = [head.x + i32::from(point.x), head.y + i32::from(point.y)];
    mover.position.rx = (xy[0] / 256) as u16;
    mover.position.ry = (xy[1] / 256) as u16;
    mover.position.sub_x = SimFixed::from_num(xy[0] % 256);
    mover.position.sub_y = SimFixed::from_num(xy[1] % 256);
    sim.substrate.entities.insert(mover);
    sim.interner = test_interner();
    sim.substrate.occupancy = OccupancyGrid::rebuild(&sim.substrate.entities);
    sim.path_grid = Some(std::sync::Arc::new(PathGrid::test_all_passable(24, 24)));
    (
        sim,
        rules,
        TrackInvocation {
            entity_id: MOVER,
            family: TrackFamily::Drive,
            apply_fresh_occupation: false,
            active_gate: false,
            retry: false,
        },
    )
}

fn add_blocker(sim: &mut Simulation, moving: bool) {
    let mut blocker = GameEntity::test_default(2, "MTNK", "Americans", CANDIDATE.0, CANDIDATE.1);
    blocker.set_category_for_test(EntityCategory::Unit);
    blocker.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    blocker.lifecycle.set_object_alive_for_test(true);
    blocker.lifecycle.set_in_limbo_for_test(false);
    blocker.lifecycle.cell_marked = true;
    if moving {
        // Unit Can_Enter_Cell's code2 body arm (0x73F86B) sees a NavCom and
        // retains Foot+6B6 while moving.
        blocker.navigation.nav_com = Some(NavTargetRef::cell(20, 9));
        assert!(
            blocker
                .locomotor
                .as_mut()
                .unwrap()
                .install_drive_state_for_test(Some(
                    DriveLocomotionRuntime::default()
                        .with_destination_for_test(Some(DriveCoord::cell(20, 9, 0)))
                ))
        );
        blocker.foot_occupation_enabled = true;
    }
    sim.substrate.entities.insert(blocker);
    sim.interner = test_interner();
    sim.substrate.occupancy = OccupancyGrid::rebuild(&sim.substrate.entities);
}

fn assert_old_chain_continues(sim: &Simulation) {
    let mover = sim.substrate.entities.get(MOVER).unwrap();
    let drive = mover
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_drive_runtime())
        .and_then(|r| r.retained())
        .unwrap();
    assert_eq!(drive.track().turn_index, 1);
    assert_eq!(
        drive.track().cursor,
        38,
        "a refused chain still completes its paid point"
    );
    assert_eq!(drive.track().residual, 1);
    assert_eq!(drive.head_to(), Some(DriveCoord::cell(10, 9, 0)));
    assert_eq!(mover.navigation.path_replay.cursor, 0);
}

#[test]
fn production_chain_code3_opens_gate_without_consuming_the_queue() {
    let (mut sim, rules, invocation) = chain_fixture(false);
    let mut gate = GameEntity::test_default_of_category(
        100,
        "GAGATE_A",
        "Americans",
        CANDIDATE.0,
        CANDIDATE.1,
        EntityCategory::Structure,
    );
    gate.building_gate = Some(BuildingGateRuntime::default());
    gate.lifecycle.set_object_alive_for_test(true);
    gate.lifecycle.set_in_limbo_for_test(false);
    gate.lifecycle.cell_marked = true;
    sim.substrate.entities.insert(gate);
    sim.interner = test_interner();
    sim.substrate.occupancy = OccupancyGrid::rebuild(&sim.substrate.entities);
    let rng_before = sim.scenario_rng.logical_state();
    let mut per_cell = 0;
    sim.run_track_points_observed(invocation, 8, Some(&rules), None, &mut |_, _, event| {
        per_cell += usize::from(event == TrackWorldEvent::PerCell);
    });
    let gate = sim.substrate.entities.get(100).unwrap();
    assert_eq!(gate.mission.current().known(), Some(MissionType::Open));
    assert_eq!(gate.mission.handler_state(), 0);
    assert_old_chain_continues(&sim);
    assert_eq!(per_cell, 0);
    assert_eq!(sim.scenario_rng.logical_state(), rng_before);
}

#[test]
fn production_chain_code6_scatters_once_without_adopting_the_candidate() {
    let (mut sim, rules, invocation) = chain_fixture(false);
    add_blocker(&mut sim, false);
    // The Unit receiver's Find_Nearby_Passable_Cell bounds its search by the
    // map Size.
    sim.playfield_bounds = Some(crate::sim::arena_fixture::OPEN_PLAYFIELD);
    sim.playfield_size_height = Some(20);
    let before = sim.substrate.entities.get(2).unwrap().position;
    let rng = sim.scenario_rng.logical_state();
    sim.run_track_points(invocation, 8, Some(&rules), None);
    let blocker = sim.substrate.entities.get(2).unwrap();
    assert!(
        blocker.navigation.nav_com.is_some(),
        "the Unit receiver sets a destination"
    );
    assert_eq!(
        serde_json::to_value(blocker.position).unwrap(),
        serde_json::to_value(before).unwrap(),
        "scatter does not teleport the occupant"
    );
    assert_old_chain_continues(&sim);
    // The Unit receiver's null arm draws nothing (0x00743A50).
    assert_eq!(sim.scenario_rng.logical_state(), rng);
}

#[test]
fn production_chain_clear_and_code2_share_admission_and_never_scatter_or_repath() {
    for passive in [false, true] {
        // The body arm and bare occupation arm both yield native code2.
        for blocker in [0, 1, 2] {
            let (mut sim, rules, invocation) = chain_fixture(passive);
            if blocker == 1 {
                add_blocker(&mut sim, true);
            } else if blocker == 2 {
                sim.substrate
                    .raw_cell_occupation
                    .mark_ground(CANDIDATE.0, CANDIDATE.1, 0x20);
            }
            let cell = sim
                .resolved_terrain
                .as_ref()
                .unwrap()
                .native_cell_identity((CANDIDATE.0 as i16, CANDIDATE.1 as i16));
            let classified = sim
                .foot_can_enter(
                    MOVER,
                    cell,
                    crate::sim::movement::infantry_entry::InfantryEntryArgs {
                        direction: 2,
                        height: 0,
                        previous_cell: None,
                    },
                    &rules,
                    None,
                )
                .unwrap();
            assert_eq!(classified, if blocker == 0 { 0 } else { 2 });
            let rng_before = sim.scenario_rng.logical_state();
            let path_before = sim
                .substrate
                .entities
                .get(MOVER)
                .unwrap()
                .navigation
                .path_runtime;
            let blocker_before = sim
                .substrate
                .entities
                .get(2)
                .map(|e| serde_json::to_value(&e.navigation).unwrap());
            let mut per_cell = 0;
            sim.run_track_points_observed(invocation, 8, Some(&rules), None, &mut |_, _, event| {
                per_cell += usize::from(event == TrackWorldEvent::PerCell);
            });
            if passive {
                let mover = sim.substrate.entities.get(MOVER).unwrap();
                let drive = mover
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.selected_drive_runtime())
                    .and_then(|r| r.retained())
                    .unwrap();
                let selected =
                    super::super::drive_track::select_drive_track(32, 64, false).unwrap();
                assert_eq!(drive.track().turn_index, selected.turn_track_index as i32);
                assert_eq!(drive.track().cursor, i32::from(selected.entry_index));
                assert_eq!(drive.track().residual, 1);
                assert_eq!(
                    drive.head_to(),
                    Some(DriveCoord::cell(CANDIDATE.0, CANDIDATE.1, 0))
                );
                assert_eq!(mover.navigation.path_replay.cursor, 1);
                assert_eq!(per_cell, 1);
            } else {
                assert_old_chain_continues(&sim);
                assert_eq!(per_cell, 0);
            }
            assert_eq!(
                sim.substrate
                    .entities
                    .get(MOVER)
                    .unwrap()
                    .navigation
                    .path_runtime,
                path_before
            );
            assert_eq!(
                sim.substrate
                    .entities
                    .get(2)
                    .map(|e| serde_json::to_value(&e.navigation).unwrap()),
                blocker_before
            );
            assert_eq!(
                sim.scenario_rng.logical_state(),
                rng_before,
                "code{blocker}, Passive={passive}"
            );
        }
    }
}

#[test]
fn accepted_chain_per_cell_crush_finishes_lifecycle_in_list_order_before_continuation() {
    let (mut sim, rules, invocation) = chain_fixture(true);
    let position = sim.substrate.entities.get(MOVER).unwrap().position;
    sim.substrate
        .entities
        .get_mut(MOVER)
        .unwrap()
        .regular_crusher = true;
    // Two victims prove the saved list successor survives removal of the first.
    for id in [20, 10] {
        let mut victim = GameEntity::test_default(id, "E1", "Soviets", position.rx, position.ry);
        victim.set_category_for_test(EntityCategory::Infantry);
        victim.mission_leaf = crate::sim::mission::leaf::MissionLeafState::for_entity_category(
            EntityCategory::Infantry,
        );
        victim.is_voxel = false;
        victim.sub_cell = Some(0);
        victim.position = position;
        victim.crushable = true;
        victim.lifecycle.set_object_alive_for_test(true);
        victim.lifecycle.set_in_limbo_for_test(false);
        victim.lifecycle.cell_marked = true;
        sim.substrate.entities.insert(victim);
    }
    sim.interner = test_interner();
    sim.substrate.occupancy = OccupancyGrid::rebuild(&sim.substrate.entities);
    let expected = sim
        .substrate
        .occupancy
        .get(position.rx, position.ry)
        .unwrap()
        .iter_layer(MovementLayer::Ground)
        .map(|entry| entry.entity_id)
        .filter(|id| *id != MOVER)
        .collect::<Vec<_>>();
    sim.clear_lifecycle_test_events_for_test();
    let mut observed = false;
    sim.run_track_points_observed(invocation, 8, Some(&rules), None, &mut |sim, _, event| {
        if event == TrackWorldEvent::PerCell {
            observed = true;
            for id in [20, 10] {
                let victim = sim
                    .substrate
                    .entities
                    .get(id)
                    .expect("UnInit retains identity until drain");
                assert!(!victim.lifecycle.object_alive());
                assert!(!victim.lifecycle.cell_marked);
                assert!(victim.lifecycle.in_limbo());
                assert!(
                    !sim.substrate
                        .occupancy
                        .contains_entity(position.rx, position.ry, id)
                );
                assert!(sim.substrate.pending_delete.contains_object(&id));
            }
        }
    });
    assert!(observed, "the real accepted-chain PerCell receiver ran");
    let retired = sim
        .lifecycle_test_events_for_test()
        .iter()
        .filter_map(|event| {
            if let LifecycleTestEvent::UninitAliveCleared { stable_id } = event {
                Some(*stable_id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(retired, expected);
    assert_eq!(
        sim.sound_events
            .iter()
            .filter(|event| matches!(
                event,
                crate::sim::world::SimSoundEvent::EntityCrushed { .. }
            ))
            .count(),
        2
    );
    let drive = sim
        .substrate
        .entities
        .get(MOVER)
        .unwrap()
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_drive_runtime())
        .and_then(|r| r.retained())
        .unwrap();
    assert_eq!(
        drive.head_to(),
        Some(DriveCoord::cell(CANDIDATE.0, CANDIDATE.1, 0))
    );
    assert_eq!(
        drive.track().residual,
        1,
        "the same paid call resumes after both removals"
    );
}
