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
use crate::sim::components::{DriveCoord, DriveLocomotionRuntime, FootPathQueue, TrackProgress};
use crate::sim::game_entity::{BuildingGateMissionState, BuildingGateRuntime, GameEntity};
use crate::sim::intern::{test_intern, test_interner};
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::track_host::TrackWorldEvent;
use crate::sim::movement::track_process::{TrackFamily, TrackInvocation};
use crate::sim::world::LifecycleTestEvent;
use crate::sim::world::Simulation;

// Slice 6 acceptance: a snapshot rebuilt at repath time reflects same-tick
// moves — observably equivalent to live per-neighbor Can_Enter_Cell for a
// synchronous search (study CELLCLASS_MAPCLASS..._SERVICE_STUDY §8 Slice 6).
#[test]
fn owner_block_set_refreshes_when_occupancy_generation_advances() {
    let alliances = HouseAllianceMap::new();
    let mut entities = EntityStore::new();
    let mut blocker = GameEntity::test_default(10, "HTNK", "Americans", 5, 5);
    blocker.category = EntityCategory::Unit;
    blocker.lifecycle.in_limbo = false;
    blocker.lifecycle.cell_marked = true;
    entities.insert(blocker);
    // Clone the test interner AFTER the entity is created so it can resolve
    // the just-interned owner string.
    let interner = test_interner();
    let owner = test_intern("Americans");

    // Initial snapshot at gen 0: friendly stationary unit -> soft-block at (5,5).
    let mut sets = BTreeMap::new();
    sets.insert(
        owner,
        bump_crush::build_entity_block_set(&entities, "Americans", &alliances, &interner, None),
    );
    let mut built_at: BTreeMap<crate::sim::intern::InternedId, u64> = BTreeMap::new();
    built_at.insert(owner, 0);
    assert!(sets[&owner].1.contains_key(MovementLayer::Ground, &(5, 5)));
    assert!(!sets[&owner].1.contains_key(MovementLayer::Ground, &(6, 6)));

    // Same-tick move of the blocker to (6,6); occupancy generation advances.
    {
        let b = entities.get_mut(10).unwrap();
        b.position.rx = 6;
        b.position.ry = 6;
    }
    let rebuilt = refresh_owner_block_set_if_stale(
        &mut sets,
        &mut built_at,
        owner,
        7,
        &entities,
        &alliances,
        &interner,
        None,
    );
    assert!(
        rebuilt,
        "stale snapshot must rebuild when generation advances"
    );
    assert!(
        !sets[&owner].1.contains_key(MovementLayer::Ground, &(5, 5)),
        "old cell freed"
    );
    assert!(
        sets[&owner].1.contains_key(MovementLayer::Ground, &(6, 6)),
        "new cell blocked"
    );
}

#[test]
fn owner_block_set_not_rebuilt_when_generation_unchanged() {
    let alliances = HouseAllianceMap::new();
    let mut entities = EntityStore::new();
    let mut blocker = GameEntity::test_default(10, "HTNK", "Americans", 5, 5);
    blocker.category = EntityCategory::Unit;
    blocker.lifecycle.in_limbo = false;
    blocker.lifecycle.cell_marked = true;
    entities.insert(blocker);
    let interner = test_interner();
    let owner = test_intern("Americans");

    let mut sets = BTreeMap::new();
    sets.insert(
        owner,
        bump_crush::build_entity_block_set(&entities, "Americans", &alliances, &interner, None),
    );
    let mut built_at: BTreeMap<crate::sim::intern::InternedId, u64> = BTreeMap::new();
    built_at.insert(owner, 4);

    // Generation matches the recorded build gen -> no rebuild, even though the
    // entity moved underneath us.
    entities.get_mut(10).unwrap().position.rx = 6;
    let rebuilt = refresh_owner_block_set_if_stale(
        &mut sets,
        &mut built_at,
        owner,
        4,
        &entities,
        &alliances,
        &interner,
        None,
    );
    assert!(!rebuilt, "no rebuild when generation is unchanged");
    assert!(
        sets[&owner].1.contains_key(MovementLayer::Ground, &(5, 5)),
        "snapshot left untouched"
    );
}

const MOVER: u64 = 1;
const CANDIDATE: (u16, u16) = (11, 9);

fn chain_fixture(passive: bool) -> (Simulation, RuleSet, PathGrid, TrackInvocation) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=MTNK\n[InfantryTypes]\n0=E1\n[BuildingTypes]\n0=GAGATE_A\n\
         [MTNK]\nSpeed=4\nCrusher=yes\nPassive={}\n\
         [E1]\nCrushable=yes\nStrength=100\nCrushSound=Squish\n\
         [GAGATE_A]\nFoundation=3x1\nGate=yes\nDeployTime=.044\nGateCloseDelay=.2\n",
        if passive { "yes" } else { "no" },
    )))
    .unwrap();
    let mut sim = Simulation::with_seed(17);
    let head = DriveCoord::cell(10, 9, 0);
    let mut mover = GameEntity::test_default(MOVER, "MTNK", "Americans", 10, 10);
    mover.category = EntityCategory::Unit;
    mover.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    mover.lifecycle.object_alive = true;
    mover.lifecycle.in_limbo = false;
    mover.lifecycle.cell_marked = true;
    mover.drive_locomotion = Some(DriveLocomotionRuntime {
        head_to: Some(head),
        track_valid: true,
        track: TrackProgress {
            turn_index: 1,
            cursor: 37,
            reversed: false,
            residual: 0,
        },
        ..Default::default()
    });
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
    (
        sim,
        rules,
        PathGrid::test_all_passable(24, 24),
        TrackInvocation {
            entity_id: MOVER,
            family: TrackFamily::Drive,
            apply_fresh_occupation: false,
        },
    )
}

fn add_blocker(sim: &mut Simulation, moving: bool) {
    let mut blocker = GameEntity::test_default(2, "MTNK", "Americans", CANDIDATE.0, CANDIDATE.1);
    blocker.category = EntityCategory::Unit;
    blocker.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    blocker.lifecycle.object_alive = true;
    blocker.lifecycle.in_limbo = false;
    blocker.lifecycle.cell_marked = true;
    if moving {
        // Unit Can_Enter_Cell's code2 body arm retains Foot+6B6 while moving.
        blocker.movement_target = Some(MovementTarget::default());
        blocker.foot_occupation_enabled = true;
    }
    sim.substrate.entities.insert(blocker);
    sim.interner = test_interner();
    sim.substrate.occupancy = OccupancyGrid::rebuild(&sim.substrate.entities);
}

fn assert_old_chain_continues(sim: &Simulation) {
    let mover = sim.substrate.entities.get(MOVER).unwrap();
    let drive = mover.drive_locomotion.as_ref().unwrap();
    assert_eq!(drive.track.turn_index, 1);
    assert_eq!(
        drive.track.cursor, 38,
        "a refused chain still completes its paid point"
    );
    assert_eq!(drive.track.residual, 1);
    assert_eq!(drive.head_to, Some(DriveCoord::cell(10, 9, 0)));
    assert_eq!(mover.navigation.path_replay.cursor, 0);
}

#[test]
fn production_chain_code3_opens_gate_without_consuming_the_queue() {
    let (mut sim, rules, grid, invocation) = chain_fixture(false);
    let mut gate = GameEntity::test_default(100, "GAGATE_A", "Americans", CANDIDATE.0, CANDIDATE.1);
    gate.category = EntityCategory::Structure;
    gate.building_gate = Some(BuildingGateRuntime::default());
    gate.lifecycle.object_alive = true;
    gate.lifecycle.in_limbo = false;
    gate.lifecycle.cell_marked = true;
    sim.substrate.entities.insert(gate);
    sim.interner = test_interner();
    sim.substrate.occupancy = OccupancyGrid::rebuild(&sim.substrate.entities);
    let rng_before = sim.scenario_rng.logical_state();
    let mut per_cell = 0;
    sim.run_track_points_observed(
        invocation,
        8,
        Some(&rules),
        Some(&grid),
        None,
        &mut |_, _, event| {
            per_cell += usize::from(event == TrackWorldEvent::PerCell);
        },
    );
    let gate = sim
        .substrate
        .entities
        .get(100)
        .unwrap()
        .building_gate
        .unwrap();
    assert!(gate.mission_18_active);
    assert_eq!(gate.mission_state, BuildingGateMissionState::Setup);
    assert_old_chain_continues(&sim);
    assert_eq!(per_cell, 0);
    assert_eq!(sim.scenario_rng.logical_state(), rng_before);
}

#[test]
fn production_chain_code6_scatters_once_without_adopting_the_candidate() {
    let (mut sim, rules, grid, invocation) = chain_fixture(false);
    add_blocker(&mut sim, false);
    let before = sim.substrate.entities.get(2).unwrap().position.clone();
    let mut expected_rng = sim.scenario_rng.clone();
    expected_rng.next_range_u32(8);
    sim.run_track_points(invocation, 8, Some(&rules), Some(&grid), None);
    let blocker = sim.substrate.entities.get(2).unwrap();
    assert!(
        blocker.movement_target.is_some(),
        "the production scatter receiver issues movement"
    );
    assert_eq!(
        serde_json::to_value(&blocker.position).unwrap(),
        serde_json::to_value(&before).unwrap(),
        "scatter does not teleport the occupant"
    );
    assert_old_chain_continues(&sim);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
}

#[test]
fn production_chain_clear_and_code2_share_admission_and_never_scatter_or_repath() {
    for passive in [false, true] {
        // The body arm and bare occupation arm both yield native code2.
        for blocker in [0, 1, 2] {
            let (mut sim, rules, grid, invocation) = chain_fixture(passive);
            if blocker == 1 {
                add_blocker(&mut sim, true);
            } else if blocker == 2 {
                sim.substrate
                    .raw_cell_occupation
                    .mark_ground(CANDIDATE.0, CANDIDATE.1, 0x20);
            }
            let classified =
                cell_entry::classify_occupied_cell_with_layers_and_ignored_and_occupation(
                    CANDIDATE,
                    cell_entry::CanEnterLayerContext::single(MovementLayer::Ground),
                    MOVER,
                    bump_crush::CrushCapability::new(true, false),
                    "Americans",
                    LocomotorKind::Drive,
                    false,
                    None,
                    &sim.substrate.occupancy,
                    &sim.substrate.cell_occupation,
                    &sim.substrate.raw_cell_occupation,
                    sim.session.binary_frame,
                    &sim.substrate.entities,
                    &sim.house_alliances,
                    &sim.interner,
                );
            assert_eq!(classified.yr_code(), if blocker == 0 { 0 } else { 2 });
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
                .map(|e| serde_json::to_value(&e.movement_target).unwrap());
            let mut per_cell = 0;
            sim.run_track_points_observed(
                invocation,
                8,
                Some(&rules),
                Some(&grid),
                None,
                &mut |_, _, event| {
                    per_cell += usize::from(event == TrackWorldEvent::PerCell);
                },
            );
            if passive {
                let mover = sim.substrate.entities.get(MOVER).unwrap();
                let drive = mover.drive_locomotion.as_ref().unwrap();
                let selected =
                    super::super::drive_track::select_drive_track(32, 64, false).unwrap();
                assert_eq!(drive.track.turn_index, selected.turn_track_index as i32);
                assert_eq!(drive.track.cursor, i32::from(selected.entry_index));
                assert_eq!(drive.track.residual, 1);
                assert_eq!(
                    drive.head_to,
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
                    .map(|e| serde_json::to_value(&e.movement_target).unwrap()),
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
    let (mut sim, rules, grid, invocation) = chain_fixture(true);
    let position = sim.substrate.entities.get(MOVER).unwrap().position.clone();
    sim.substrate
        .entities
        .get_mut(MOVER)
        .unwrap()
        .regular_crusher = true;
    // Two victims prove the saved list successor survives removal of the first.
    for id in [20, 10] {
        let mut victim = GameEntity::test_default(id, "E1", "Soviets", position.rx, position.ry);
        victim.category = EntityCategory::Infantry;
        victim.is_voxel = false;
        victim.sub_cell = Some(0);
        victim.position = position.clone();
        victim.crushable = true;
        victim.lifecycle.object_alive = true;
        victim.lifecycle.in_limbo = false;
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
    sim.run_track_points_observed(
        invocation,
        8,
        Some(&rules),
        Some(&grid),
        None,
        &mut |sim, _, event| {
            if event == TrackWorldEvent::PerCell {
                observed = true;
                for id in [20, 10] {
                    let victim = sim
                        .substrate
                        .entities
                        .get(id)
                        .expect("UnInit retains identity until drain");
                    assert!(!victim.lifecycle.object_alive);
                    assert!(!victim.lifecycle.cell_marked);
                    assert!(victim.lifecycle.in_limbo);
                    assert!(
                        !sim.substrate
                            .occupancy
                            .contains_entity(position.rx, position.ry, id)
                    );
                    assert!(sim.substrate.pending_delete.contains(&id));
                }
            }
        },
    );
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
        .drive_locomotion
        .as_ref()
        .unwrap();
    assert_eq!(
        drive.head_to,
        Some(DriveCoord::cell(CANDIDATE.0, CANDIDATE.1, 0))
    );
    assert_eq!(
        drive.track.residual, 1,
        "the same paid call resumes after both removals"
    );
}
