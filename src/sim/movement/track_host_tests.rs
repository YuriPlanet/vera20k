use super::*;
use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};
use crate::util::save_codec;

use crate::util::fixed_math::SimFixed;

fn fixture(family: TrackFamily, budget: i32) -> (Simulation, TrackInvocation, i32) {
    let mut sim = Simulation::new();
    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
    entity.set_category_for_test(EntityCategory::Unit);
    entity.locomotor = Some(super::super::locomotor::LocomotorState::for_test_kind(
        if family == TrackFamily::Drive {
            crate::rules::locomotor_type::LocomotorKind::Drive
        } else {
            crate::rules::locomotor_type::LocomotorKind::Ship
        },
    ));
    entity.lifecycle.set_object_alive_for_test(true);
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.lifecycle.set_cell_marked(true);
    let head = DriveCoord {
        x: 10 * 256 + 128,
        y: 9 * 256 + 128,
        z: 0,
    };
    let track = TrackProgress::for_test(0, 0, false, 0);
    match family {
        TrackFamily::Drive => {
            assert!(
                entity
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .install_drive_state_for_test(Some(
                        DriveLocomotionRuntime::default()
                            .with_head_to_for_test(Some(head))
                            .with_track_for_test(track)
                            .with_track_valid_for_test(true)
                    ))
            )
        }
        TrackFamily::Ship => {
            assert!(
                entity
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .install_ship_state_for_test(Some(
                        ShipLocomotionRuntime::default()
                            .with_head_to_for_test(Some(head))
                            .with_track_for_test(track)
                            .with_track_valid_for_test(true)
                    ))
            )
        }
    }
    sim.interner = crate::sim::intern::test_interner();
    sim.substrate.entities.insert(entity);
    sim.substrate.occupancy =
        crate::sim::occupancy::OccupancyGrid::rebuild(&sim.substrate.entities);
    (
        sim,
        TrackInvocation {
            entity_id: 1,
            family,
            active_gate: false,
            retry: false,
        },
        budget,
    )
}

fn valid(entity: &GameEntity, family: TrackFamily) -> bool {
    match family {
        TrackFamily::Drive => entity
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .track_valid(),
        TrackFamily::Ship => entity
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_ship_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .track_valid(),
    }
}

#[test]
fn terminal_horizontal_gate_uses_installed_owner_and_retires_with_class() {
    use crate::sim::components::NavTargetRef;
    let (mut sim, _, _) = fixture(TrackFamily::Drive, 0);
    let e = sim.substrate.entities.get_mut(1).unwrap();
    e.navigation
        .set_nav_com_for_test(Some(NavTargetRef::cell(20, 20)));
    assert!(
        e.locomotor
            .as_mut()
            .unwrap()
            .store_track_destination(TrackFamily::Drive, Some(DriveCoord::cell(20, 20, 0)))
    );
    assert!(
        e.locomotor
            .as_mut()
            .unwrap()
            .store_track_head(TrackFamily::Drive, None)
    );
    assert!(
        !sim.track_reached_destination(1, TrackFamily::Drive, None)
            .unwrap()
    );
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::cell(10, 10)));
    assert!(
        sim.track_reached_destination(1, TrackFamily::Drive, None)
            .unwrap()
    );

    // The retained destination belongs to the installed Drive. Retiring the
    // entire class removes it before the terminal owner-coordinate query.
    sim.substrate.entities.get_mut(1).unwrap().locomotor = None;
    assert!(
        !sim.track_reached_destination(1, TrackFamily::Drive, None)
            .unwrap()
    );
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::cell(20, 20)));
    assert!(
        !sim.track_reached_destination(1, TrackFamily::Drive, None)
            .unwrap()
    );
}

#[test]
fn building_navigation_reaches_drive_and_ship_terminal_callbacks() {
    use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};
    use crate::sim::components::NavTargetRef;
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        for contacted in [false, true] {
            let (mut sim, invocation, budget) = fixture(family, 8);
            let mut rules = RuleSet::from_ini(&IniFile::from_str(
                "[VehicleTypes]\n0=MTNK\n[BuildingTypes]\n0=PAD\n[PAD]\nHelipad=yes\nNumberOfDocks=3\n",
            )).unwrap();
            rules.install_art_data(ArtRegistry::from_ini(&IniFile::from_str(
                "[PAD]\nFoundation=3x3\nDockingOffset0=-256,0,0\nDockingOffset1=0,0,0\nDockingOffset2=256,0,0\n",
            )));
            let mut building = GameEntity::test_default_of_category(
                2,
                "PAD",
                "Americans",
                8,
                8,
                EntityCategory::Structure,
            );
            building.type_facts.set_foundation_for_test("3x3".into());
            building.radio_contacts.set_capacity(3);
            for id in [100, 101, if contacted { 1 } else { 102 }] {
                building.radio_contacts.insert(id);
            }
            building.radio_contacts.remove(101);
            sim.substrate.entities.insert(building);
            sim.interner = crate::sim::intern::test_interner();
            let mover = sim.substrate.entities.get_mut(1).unwrap();
            let destination = head(mover, family);
            mover
                .navigation
                .set_nav_com_for_test(Some(NavTargetRef::Object { id: 2 }));
            match family {
                TrackFamily::Drive => {
                    assert!(mover.locomotor.as_mut().unwrap().store_track_destination(
                        crate::sim::movement::track_process::TrackFamily::Drive,
                        Some(destination)
                    ))
                }
                TrackFamily::Ship => {
                    assert!(mover.locomotor.as_mut().unwrap().store_track_destination(
                        crate::sim::movement::track_process::TrackFamily::Ship,
                        Some(destination)
                    ))
                }
            }
            let loco = mover.locomotor.as_mut().unwrap();
            let mut state = loco.track_progress(family).unwrap();
            state.set_cursor(super::super::drive_track::raw_track_points(1).len() as i32);
            assert!(loco.store_track_progress(family, state));
            let mut callbacks = 0;
            sim.run_track_points_observed(
                invocation,
                budget,
                Some(&rules),
                None,
                &mut |world, _, event| {
                    if event == TrackWorldEvent::PerCell {
                        callbacks += 1;
                        // The native caller keeps the earlier reached decision;
                        // live radio changes here affect only subsequent queries.
                        world
                            .substrate
                            .entities
                            .get_mut(2)
                            .unwrap()
                            .radio_contacts
                            .clear_all();
                    }
                },
            );
            assert_eq!(callbacks, 1);
            let mover = sim.substrate.entities.get(1).unwrap();
            assert_eq!(
                mover.navigation.nav_com().is_none(),
                contacted,
                "{family:?}"
            );
            assert_eq!(
                match family {
                    TrackFamily::Drive => mover
                        .locomotor
                        .as_ref()
                        .and_then(|l| l.selected_drive_runtime())
                        .and_then(|r| r.retained())
                        .unwrap()
                        .destination()
                        .is_none(),
                    TrackFamily::Ship => mover
                        .locomotor
                        .as_ref()
                        .and_then(|l| l.selected_ship_runtime())
                        .and_then(|r| r.retained())
                        .unwrap()
                        .destination()
                        .is_none(),
                },
                contacted,
                "{family:?}"
            );
        }
    }
}

#[test]
fn terminal_queries_retained_target_after_clearing_own_head_before_per_cell() {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::components::NavTargetRef;
    use crate::sim::movement::locomotor::LocomotorState;
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        let (mut sim, invocation, budget) = fixture(family, 8);
        let mover = sim.substrate.entities.get_mut(1).unwrap();
        let destination = head(mover, family);
        mover
            .navigation
            .set_nav_com_for_test(Some(NavTargetRef::Object { id: 2 }));
        match family {
            TrackFamily::Drive => {
                assert!(mover.locomotor.as_mut().unwrap().store_track_destination(
                    crate::sim::movement::track_process::TrackFamily::Drive,
                    Some(destination)
                ))
            }
            TrackFamily::Ship => {
                assert!(mover.locomotor.as_mut().unwrap().store_track_destination(
                    crate::sim::movement::track_process::TrackFamily::Ship,
                    Some(destination)
                ))
            }
        }
        let loco = mover.locomotor.as_mut().unwrap();
        let mut state = loco.track_progress(family).unwrap();
        state.set_cursor(super::super::drive_track::raw_track_points(1).len() as i32);
        assert!(loco.store_track_progress(family, state));
        let mut target = GameEntity::test_default(2, "E1", "Americans", 20, 20);
        target.set_category_for_test(EntityCategory::Infantry);
        target.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
        target
            .locomotor
            .as_mut()
            .unwrap()
            .set_step_head(Some(destination));
        sim.substrate.entities.insert(target);
        let mut per_cell = 0;
        sim.run_track_points_observed(invocation, budget, None, None, &mut |sim, id, event| {
            if event == TrackWorldEvent::MarkPut {
                // The completed track's retained head is cleared before the
                // owner's +4C height read; physical placement stays at Z0.
                set_head(
                    sim.substrate.entities.get_mut(id).unwrap(),
                    family,
                    Some(DriveCoord {
                        z: 1000,
                        ..destination
                    }),
                );
            }
            if event == TrackWorldEvent::PerCell {
                per_cell += 1;
                assert_eq!(
                    head(sim.substrate.entities.get(id).unwrap(), family),
                    DriveCoord { x: 0, y: 0, z: 0 }
                );
                // The reached result must already be saved across this callback.
                sim.substrate
                    .entities
                    .get_mut(2)
                    .unwrap()
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .set_step_head(Some(DriveCoord::cell(25, 25, 0)));
            }
        });
        assert_eq!(per_cell, 1);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().navigation.nav_com(),
            None
        );
    }
}

#[test]
fn production_host_keeps_budget_and_live_cursor_across_world_coordinate_effect() {
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        let (mut sim, invocation, budget) = fixture(family, 22);
        let mut callbacks = 0;
        let moved =
            sim.run_track_points_observed(invocation, budget, None, None, &mut |sim, id, event| {
                if event == TrackWorldEvent::SetCoords {
                    callbacks += 1;
                    if callbacks == 1 {
                        let loco = sim
                            .substrate
                            .entities
                            .get_mut(id)
                            .unwrap()
                            .locomotor
                            .as_mut()
                            .unwrap();
                        let mut state = loco.track_progress(family).unwrap();
                        state.set_cursor(2);
                        assert!(loco.store_track_progress(family, state));
                    }
                }
            });
        let state = progress(sim.substrate.entities.get(1).unwrap(), family).unwrap();
        assert_eq!(moved, 3);
        assert_eq!(
            state.cursor(),
            5,
            "finish the same paid point with the live cursor"
        );
        assert_eq!(state.residual(), 1);
        assert!(callbacks >= 3);
    }
}

#[test]
fn production_host_lifecycle_exit_does_not_store_call_local_residual() {
    let (mut sim, invocation, budget) = fixture(TrackFamily::Drive, 22);
    {
        let mut progress = sim
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .track_progress(crate::sim::movement::track_process::TrackFamily::Drive)
            .unwrap();
        progress.set_residual(4);
        assert!(
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .locomotor
                .as_mut()
                .unwrap()
                .store_track_progress(
                    crate::sim::movement::track_process::TrackFamily::Drive,
                    progress
                )
        );
    };
    sim.run_track_points_observed(invocation, budget, None, None, &mut |sim, id, event| {
        if event == TrackWorldEvent::SetCoords {
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .lifecycle
                .set_object_alive_for_test(false);
        }
    });
    let state = progress(sim.substrate.entities.get(1).unwrap(), TrackFamily::Drive).unwrap();
    assert_eq!(state.residual(), 4);
    assert_eq!(state.cursor(), 0);
}

#[test]
fn crossing_mark_state_and_open_topped_cargo_are_visible_before_paid_continuation() {
    use crate::rules::ini_parser::IniFile;
    use crate::sim::passenger::{PassengerCargo, PassengerRole};
    let (mut sim, invocation, budget) = fixture(TrackFamily::Drive, 8);
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nOpenTopped=yes\n",
    ))
    .unwrap();
    sim.interner = crate::sim::intern::test_interner();
    let mut cargo = PassengerCargo::new(2, 1.0);
    cargo.board_forced_chain(&[(2, 1.0)]);
    sim.substrate.entities.get_mut(1).unwrap().passenger_role = PassengerRole::transport(cargo);
    let passenger = GameEntity::test_default(2, "E1", "Americans", 4, 4);
    sim.substrate.entities.insert(passenger);
    // Force a cell crossing on the first raw point; this test observes the
    // actual world receiver, including Foot4DB810 -> OpenTopped7104F0.
    assert!(
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .store_track_head(
                crate::sim::movement::track_process::TrackFamily::Drive,
                Some(DriveCoord::cell(10, 7, 0))
            )
    );
    let mut events = Vec::new();
    sim.run_track_points_observed(
        invocation,
        budget,
        Some(&rules),
        None,
        &mut |sim, id, event| {
            if matches!(
                event,
                TrackWorldEvent::MarkRemove | TrackWorldEvent::SetCoords | TrackWorldEvent::MarkPut
            ) {
                let entity = sim.substrate.entities.get(id).unwrap();
                events.push((event, entity.lifecycle.cell_marked()));
                if event == TrackWorldEvent::SetCoords {
                    assert_eq!(
                        position_world_coord(&sim.substrate.entities.get(2).unwrap().position),
                        position_world_coord(&entity.position)
                    );
                }
            }
        },
    );
    assert_eq!(
        &events[..3],
        &[
            (TrackWorldEvent::MarkRemove, false),
            (TrackWorldEvent::SetCoords, false),
            (TrackWorldEvent::MarkPut, true)
        ]
    );
    assert_eq!(
        progress(sim.substrate.entities.get(1).unwrap(), TrackFamily::Drive)
            .unwrap()
            .cursor(),
        1
    );
}

#[test]
fn nonterminal_limbo_and_falling_do_not_add_an_early_survival_gate() {
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        let (mut sim, invocation, budget) = fixture(family, 15);
        sim.run_track_points_observed(invocation, budget, None, None, &mut |sim, id, event| {
            if event == TrackWorldEvent::SetCoords {
                let entity = sim.substrate.entities.get_mut(id).unwrap();
                entity.lifecycle.set_in_limbo_for_test(true);
                entity.set_falling_down_for_test(true);
            }
        });
        let state = progress(sim.substrate.entities.get(1).unwrap(), family).unwrap();
        assert_eq!(
            state.cursor(),
            2,
            "4B1A77 tests alive only before common cursor increment"
        );
        assert_eq!(state.residual(), 1);
    }
}

#[test]
fn terminal_per_cell_limbo_keeps_retained_residual_and_navcom() {
    use crate::sim::components::NavTargetRef;
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        let (mut sim, invocation, budget) = fixture(family, 22);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        let target = head(entity, family);
        entity
            .navigation
            .set_nav_com_for_test(Some(NavTargetRef::cell(10, 9)));
        match family {
            TrackFamily::Drive => {
                assert!(entity.locomotor.as_mut().unwrap().store_track_destination(
                    crate::sim::movement::track_process::TrackFamily::Drive,
                    Some(target)
                ))
            }
            TrackFamily::Ship => {
                assert!(entity.locomotor.as_mut().unwrap().store_track_destination(
                    crate::sim::movement::track_process::TrackFamily::Ship,
                    Some(target)
                ))
            }
        }
        let loco = entity.locomotor.as_mut().unwrap();
        let mut state = loco.track_progress(family).unwrap();
        state.set_cursor(super::super::drive_track::raw_track_points(1).len() as i32);
        state.set_residual(4);
        assert!(loco.store_track_progress(family, state));
        let mut calls = 0;
        sim.run_track_points_observed(invocation, budget, None, None, &mut |sim, id, event| {
            if event == TrackWorldEvent::PerCell {
                calls += 1;
                let entity = sim.substrate.entities.get_mut(id).unwrap();
                assert!(entity.navigation.nav_com().is_some());
                assert_eq!(head(entity, family), DriveCoord { x: 0, y: 0, z: 0 });
                assert!(!valid(entity, family));
                entity.lifecycle.set_in_limbo_for_test(true);
            }
        });
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(calls, 1);
        assert_eq!(progress(entity, family).unwrap().residual(), 4);
        assert!(
            entity.navigation.nav_com().is_some(),
            "limbo returns before saved-reached FootStop"
        );
    }
}

#[test]
fn terminal_saved_reached_clears_callback_navcom_and_only_live_queue_head() {
    use crate::sim::components::{FootPathQueue, NavTargetRef};
    let (mut sim, invocation, budget) = fixture(TrackFamily::Drive, 8);
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
        progress.set_cursor(super::super::drive_track::raw_track_points(1).len() as i32);
        assert!(drive.store_track_progress(
            crate::sim::movement::track_process::TrackFamily::Drive,
            progress
        ));
    };
    sim.run_track_points_observed(invocation, budget, None, None, &mut |sim, id, event| {
        if event == TrackWorldEvent::PerCell {
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            assert_eq!(entity.navigation.nav_com(), Some(NavTargetRef::cell(10, 9)));
            assert!(
                entity
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.selected_drive_runtime())
                    .and_then(|r| r.retained())
                    .unwrap()
                    .destination()
                    .is_none()
            );
            entity
                .navigation
                .set_nav_com_for_test(Some(NavTargetRef::cell(25, 25)));
            entity.navigation.path_replay = FootPathQueue::for_test(vec![1, 2, 3], 1, Some((7, 8)));
        }
    });
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(entity.navigation.nav_com().is_none());
    assert_eq!(entity.navigation.path_replay.directions(), vec![1, 255, 3]);
    assert_eq!(entity.navigation.path_replay.cursor(), 1);
    assert_eq!(entity.navigation.path_replay.reference_cell(), Some((7, 8)));
}

#[test]
fn accepted_chain_preserves_call_budget_and_reloads_callback_queue_once() {
    use super::super::drive_track;
    use crate::sim::components::FootPathQueue;
    for (family, retire_in_callback) in [
        (TrackFamily::Drive, false),
        (TrackFamily::Ship, false),
        (TrackFamily::Drive, true),
        (TrackFamily::Ship, true),
    ] {
        let (mut sim, invocation, budget) = fixture(family, 22);
        // The shared predicate observes the destination surface before its
        // object-list and raw-occupation tail. Supply passable land/water.
        let terrain = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
            32,
            32,
            (0..32)
                .flat_map(|y| {
                    (0..32).map(move |x| {
                        let mut cell =
                            crate::map::resolved_terrain::ResolvedTerrainCell::clear_for_test(x, y);
                        cell.speed_costs.track = Some(100);
                        cell.speed_costs.float = Some(100);
                        if family == TrackFamily::Ship {
                            cell.zone_type = crate::map::resolved_terrain::zone_class::WATER;
                            cell.ground_walk_blocked = true;
                            cell.land_type =
                                crate::rules::terrain_rules::LandType::Water.as_index();
                        }
                        cell
                    })
                })
                .collect(),
        );
        sim.path_grid = Some(std::sync::Arc::new(
            crate::sim::pathfinding::PathGrid::from_resolved_terrain(&terrain),
        ));
        sim.resolved_terrain = Some(terrain);
        // Supplied Passive=true exercises the admitted native arm; this is
        // not an assertion that stock MTNK can accept an in-call chain.
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nPassive=yes\n",
        ))
        .unwrap();
        sim.interner = crate::sim::intern::test_interner();
        let selected = drive_track::select_drive_track(32, 64, false).unwrap();
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        if family == TrackFamily::Ship {
            let loco = entity.locomotor.as_mut().unwrap();
            loco.set_movement_zone_for_test(crate::rules::locomotor_type::MovementZone::Water);
            loco.set_speed_type_for_test(crate::rules::locomotor_type::SpeedType::Float);
        }
        assert!(
            entity
                .locomotor
                .as_mut()
                .unwrap()
                .store_track_progress(family, TrackProgress::for_test(1, 37, false, 0))
        );
        entity.navigation.path_replay = FootPathQueue::for_test(vec![2, 3], 0, Some((10, 10)));
        let saved_fraction = entity.foot_speed.applied_fraction();
        let mut callbacks = 0;
        sim.run_track_points_observed(
            invocation,
            budget,
            Some(&rules),
            None,
            &mut |sim, id, event| {
                if event == TrackWorldEvent::PerCell {
                    callbacks += 1;
                    let entity = sim.substrate.entities.get_mut(id).unwrap();
                    let loco = entity.locomotor.as_mut().unwrap();
                    let mut state = loco.track_progress(family).unwrap();
                    assert_eq!(state.turn_index(), selected.turn_track_index as i32);
                    assert_eq!(state.cursor(), i32::from(selected.entry_index) - 1);
                    state.set_cursor(state.cursor() + 1);
                    assert!(loco.store_track_progress(family, state));
                    assert_eq!(head(entity, family), DriveCoord { x: 0, y: 0, z: 0 });
                    assert!(valid(entity, family));
                    set_head(entity, family, Some(DriveCoord::cell(1, 1, 0)));
                    entity.navigation.path_replay =
                        FootPathQueue::for_test(vec![7, 3, 4], 0, Some((6, 7)));
                    entity
                        .foot_speed
                        .set_speed_fraction(SimFixed::from_num(0.25));
                    if retire_in_callback {
                        entity.lifecycle.set_in_limbo_for_test(true);
                    }
                }
            },
        );
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(callbacks, 1);
        if retire_in_callback {
            // Drive4B1D06 / Ship6A1349 clear +63 before the limbo return.
            // That return preserves the callback's head/queue/speed writes;
            // it never reaches accepted candidate publication or restoration.
            assert!(!valid(entity, family));
            assert_eq!(head(entity, family), DriveCoord::cell(1, 1, 0));
            assert_eq!(entity.navigation.path_replay.cursor(), 0);
            assert_eq!(
                entity.navigation.path_replay.remaining_directions(),
                &[7, 3, 4]
            );
            assert_eq!(
                entity.foot_speed.applied_fraction(),
                SimFixed::from_num(0.25)
            );
            continue;
        }
        assert_eq!(head(entity, family), DriveCoord::cell(11, 9, 0));
        assert!(valid(entity, family));
        assert!(progress(entity, family).unwrap().cursor() > i32::from(selected.entry_index));
        assert_eq!(progress(entity, family).unwrap().residual(), 1);
        assert_eq!(entity.navigation.path_replay.cursor(), 1);
        assert_eq!(
            entity.navigation.path_replay.remaining_directions(),
            &[3, 4]
        );
        assert_eq!(entity.foot_speed.applied_fraction(), saved_fraction);
    }
}

#[test]
fn apply_one_marks_handoff_and_full_head_without_foot_enable() {
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        let (mut sim, _, _) = fixture(family, 0);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.set_foot_occupation_enabled(false);
        assert!(
            entity
                .locomotor
                .as_mut()
                .unwrap()
                .store_track_progress(family, TrackProgress::for_test(1, 0, false, 0))
        );
        sim.track_apply_occupation(1, family, true);
        // Drive/Ship Apply1 invokes raw marks directly regardless of +6B6.
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(10, 9) & 0x20,
            0x20
        );
        let query = super::super::at_coord::AtCoordQuery::from_state(
            if family == TrackFamily::Drive {
                crate::rules::locomotor_type::LocomotorKind::Drive
            } else {
                crate::rules::locomotor_type::LocomotorKind::Ship
            },
            position_world_coord(&sim.substrate.entities.get(1).unwrap().position),
            Some(head(sim.substrate.entities.get(1).unwrap(), family)),
            super::super::at_coord::AtCoordTrack {
                turn_index: 1,
                cursor: 0,
                reversed: false,
            },
        )
        .unwrap();
        let handoff = query.cells().0.unwrap();
        assert_eq!(
            sim.substrate
                .raw_cell_occupation
                .ground_bits(handoff.0 as u16, handoff.1 as u16)
                & 0x20,
            0x20
        );
        assert!(
            !sim.substrate
                .entities
                .get(1)
                .unwrap()
                .foot_occupation_enabled()
        );
    }
}

#[test]
fn bridge_placement_retains_selected_cell_but_reads_fresh_world_attributes() {
    let (mut sim, _, _) = fixture(TrackFamily::Drive, 0);
    if sim.path_grid.is_none() {
        sim.path_grid = Some(std::sync::Arc::new(crate::sim::pathfinding::PathGrid::new(
            32, 32,
        )));
    }
    sim.track_place(
        1,
        DriveCoord::cell(10, 9, 0),
        (10, 10),
        false,
        false,
        None,
        None,
        &mut |sim, id, event| {
            if event == TrackWorldEvent::SetCoords {
                // Cached source/selected destination; fresh canonical attributes.
                let mut grid = crate::sim::pathfinding::PathGrid::new(32, 32);
                grid.set_cell_for_test(10, 10, 4, false, false);
                grid.set_cell_for_test(10, 9, 0, true, false);
                sim.path_grid = Some(std::sync::Arc::new(grid));
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .position
                    .set_rx_for_test(20);
            }
        },
        crate::sim::world::FrameEffects::default(),
    );
    assert!(sim.substrate.entities.get(1).unwrap().on_bridge());
}

#[test]
fn ordinary_foot_limbo_releases_live_head_and_handoff_for_both_families() {
    use crate::sim::lifecycle_request::{LifecycleRequest, UninitReason};
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        let (mut sim, _, _) = fixture(family, 0);
        let loco = sim
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap();
        let mut state = loco.track_progress(family).unwrap();
        state.set_turn_index(1);
        assert!(loco.store_track_progress(family, state));
        sim.track_apply_occupation(1, family, true);
        let entity = sim.substrate.entities.get(1).unwrap();
        let (handoff, head) = super::super::at_coord::AtCoordQuery::from_state(
            if family == TrackFamily::Drive {
                crate::rules::locomotor_type::LocomotorKind::Drive
            } else {
                crate::rules::locomotor_type::LocomotorKind::Ship
            },
            position_world_coord(&entity.position),
            Some(head(entity, family)),
            super::super::at_coord::AtCoordTrack {
                turn_index: 1,
                cursor: 0,
                reversed: false,
            },
        )
        .unwrap()
        .cells();
        let head = (head.0 as u16, head.1 as u16);
        let handoff = handoff.map(|cell| (cell.0 as u16, cell.1 as u16)).unwrap();
        assert_ne!(head, (10, 10));
        assert_ne!(head, handoff);
        for cell in [head, handoff] {
            assert_eq!(
                sim.substrate
                    .raw_cell_occupation
                    .ground_bits(cell.0, cell.1)
                    & 0x20,
                0x20
            );
        }
        sim.apply_lifecycle_request(LifecycleRequest::Uninit {
            stable_id: 1,
            reason: UninitReason::Crush,
        });
        assert_eq!(
            sim.substrate
                .raw_cell_occupation
                .ground_bits(head.0, head.1)
                & 0x20,
            0
        );
        assert_eq!(
            sim.substrate
                .raw_cell_occupation
                .ground_bits(handoff.0, handoff.1)
                & 0x20,
            0
        );
    }
}

#[test]
fn repeated_foot_limbo_does_not_clear_a_new_raw_head_claim() {
    use crate::sim::lifecycle_request::{LifecycleRequest, UninitReason};
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        let (mut sim, _, _) = fixture(family, 0);
        let loco = sim
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap();
        let mut state = loco.track_progress(family).unwrap();
        state.set_turn_index(1);
        assert!(loco.store_track_progress(family, state));
        let supplied = head(sim.substrate.entities.get(1).unwrap(), family);
        sim.track_apply_occupation(1, family, true);
        sim.techno_limbo(1);
        assert!(sim.substrate.entities.get(1).unwrap().lifecycle.in_limbo());
        let at = cell(supplied);
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(at.0, at.1) & 0x20,
            0
        );
        // Boarding retains the locomotor head. A later carrier teardown calls
        // UnInit/Limbo again; native Foot's early in-limbo gate preserves the
        // raw bit another mover has since written at that old head.
        sim.track_raw_mark_at(supplied, true);
        sim.apply_lifecycle_request(LifecycleRequest::Uninit {
            stable_id: 1,
            reason: UninitReason::Crush,
        });
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(at.0, at.1) & 0x20,
            0x20
        );
    }
}

/// The terminal retires its completed head before the PerCell callback, and
/// a new order the callback installs (NavCom and Foot+5E0 words) survives the
/// tail; nothing is owed to a later Process.
#[test]
fn terminal_retires_the_head_before_callback_and_keeps_a_new_order() {
    use crate::sim::components::NavTargetRef;
    for retarget in [false, true] {
        let (mut sim, invocation, budget) = fixture(TrackFamily::Drive, 15);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        {
            let mut progress = entity
                .locomotor
                .as_mut()
                .unwrap()
                .track_progress(crate::sim::movement::track_process::TrackFamily::Drive)
                .unwrap();
            progress.set_cursor(super::super::drive_track::raw_track_points(1).len() as i32);
            assert!(entity.locomotor.as_mut().unwrap().store_track_progress(
                crate::sim::movement::track_process::TrackFamily::Drive,
                progress
            ));
        };
        // A stopped committed segment has no NavCom, but its completed head
        // must still retire before callbacks begin a deployment body turn.
        sim.run_track_points_observed(invocation, budget, None, None, &mut |sim, id, event| {
            if event == TrackWorldEvent::PerCell {
                let entity = sim.substrate.entities.get_mut(id).unwrap();
                assert!(crate::sim::movement::track_head::committed_track_head(entity).is_none());
                if retarget {
                    entity
                        .navigation
                        .set_nav_com_for_test(Some(NavTargetRef::cell(12, 9)));
                    // The callback's new route: Foot+5E0 words from (10,9).
                    entity.navigation.path_replay =
                        crate::sim::movement::fixture_path_replay(&[(10, 9), (11, 9), (12, 9)]);
                }
            }
        });
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(entity.navigation.nav_com().is_some(), retarget);
        assert_eq!(
            entity
                .navigation
                .path_replay
                .remaining_directions()
                .is_empty(),
            !retarget
        );
        assert!(!entity.navigation.pending_destination_setter());
    }
}

#[test]
fn per_cell_promotes_queued_mission_before_tail_without_dispatching_handler() {
    use crate::rules::ini_parser::IniFile;
    use crate::sim::miner::{Miner, MinerConfig, MinerKind};
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=500\nSpeed=6\n",
    ))
    .unwrap();
    for unload_active in [false, true] {
        let (mut sim, _, _) = fixture(TrackFamily::Drive, 8);
        sim.session.set_binary_frame_for_test(57);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        assert!(entity.locomotor.as_mut().unwrap().store_track_head(
            crate::sim::movement::track_process::TrackFamily::Drive,
            None
        ));
        let mut miner = Miner::new(MinerKind::War, &MinerConfig::default(), 0);
        miner.set_unload_active_for_test(unload_active);
        entity.miner = Some(miner);
        entity.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Move),
            suspended: MissionId::NONE,
            queued: MissionId::from_known(MissionType::Unload),
            movement_bypass_latch: 0,
            handler_state: 4,
            mission_start_frame: 3,
            ai_counter: 11,
            dispatch_timer: MissionDispatchTimer::from_raw(3, 90),
        });
        sim.unit_per_cell_process(
            1,
            super::super::PerCellReason::Arrival,
            Some(&rules),
            None,
            crate::sim::world::FrameEffects::default(),
        );
        let mission = sim.substrate.entities.get(1).unwrap().mission;
        if unload_active {
            assert_eq!(mission.current().known(), Some(MissionType::Move));
            assert_eq!(mission.queued().known(), Some(MissionType::Unload));
            assert_eq!(mission.handler_state(), 4);
            assert_eq!(mission.ai_counter(), 11);
        } else {
            assert_eq!(mission.current().known(), Some(MissionType::Unload));
            assert_eq!(mission.queued(), MissionId::NONE);
            assert_eq!(mission.handler_state(), 0);
            assert_eq!(mission.ai_counter(), 0);
            assert_eq!(mission.mission_start_frame(), 57);
        }
    }
}

fn factory_per_cell_fixture(
    tethered: bool,
    producer_on_bridge: bool,
    unit_reader_lines: &str,
) -> (Simulation, RuleSet) {
    use crate::rules::ini_parser::IniFile;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
    use crate::sim::radio::{RadioMessage, RadioPayload, RadioResponse, transmit};

    let (mut sim, _, _) = fixture(TrackFamily::Drive, 0);
    sim.session.set_binary_frame_for_test(57);
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\nSpeed=6\n{unit_reader_lines}\n\
         [BuildingTypes]\n0=GAWEAP\n[GAWEAP]\nWeaponsFactory=yes\n",
    )))
    .unwrap();
    let type_ref = sim.intern("MTNK");
    let owner = sim.intern("Americans");
    let guard = MissionTestFixture {
        current: MissionId::from_known(MissionType::Guard),
        suspended: MissionId::NONE,
        queued: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 4,
        mission_start_frame: 3,
        ai_counter: 11,
        dispatch_timer: MissionDispatchTimer::from_raw(3, 90),
    };
    sim.substrate.entities.change_owner(1, owner);
    let unit = sim.substrate.entities.get_mut(1).unwrap();
    unit.set_type_ref_for_test(type_ref);
    unit.mission.apply_test_fixture(guard);
    assert!(unit.locomotor.as_mut().unwrap().store_track_head(
        crate::sim::movement::track_process::TrackFamily::Drive,
        None
    ));
    assert!(unit.locomotor.as_mut().unwrap().store_track_valid(
        crate::sim::movement::track_process::TrackFamily::Drive,
        false
    ));
    sim.houses
        .insert_for_test(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    let mut producer = GameEntity::test_default_of_category(
        2,
        "GAWEAP",
        "Americans",
        10,
        10,
        EntityCategory::Structure,
    );
    producer.set_type_ref_for_test(sim.intern("GAWEAP"));
    producer.set_owner_for_test(owner);
    producer.type_facts.set_foundation_for_test("1x1".into());
    producer.set_on_bridge(producer_on_bridge);
    producer.lifecycle.set_object_alive_for_test(true);
    producer.lifecycle.set_in_limbo_for_test(false);
    producer.lifecycle.set_cell_marked(true);
    producer.mission.apply_test_fixture(guard);
    sim.substrate.entities.insert(producer);
    sim.substrate.occupancy =
        crate::sim::occupancy::OccupancyGrid::rebuild(&sim.substrate.entities);
    assert_eq!(
        transmit(
            &mut sim,
            1,
            2,
            RadioMessage::Hello,
            RadioPayload::default(),
            Some(&rules),
            crate::sim::world::FrameEffects::default()
        ),
        RadioResponse::Roger
    );
    if tethered {
        assert_eq!(
            transmit(
                &mut sim,
                1,
                2,
                RadioMessage::Tether,
                RadioPayload::default(),
                Some(&rules),
                crate::sim::world::FrameEffects::default()
            ),
            RadioResponse::Roger
        );
    }
    crate::sim::radio::take_transmit_log();
    // No native Map Size is installed: these are gate/ordering regressions,
    // so null Scatter runs its existing missing-map boundary. The composed
    // native factory control separately supplies the whole Scatter/search.
    (sim, rules)
}

///73A93D precedes73ACC2: promoting queued Unload first would suppress the
/// clearance. The nested radio receipt comes from the original saved8 calls;
/// this fixture adds the existing PerCell/Ready production integration.
#[test]
fn per_cell_factory_clearance_precedes_ready_and_preserves_foot_stop_scope() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};

    let (mut sim, rules) = factory_per_cell_fixture(true, false, "");
    let unit = sim.substrate.entities.get_mut(1).unwrap();
    unit.navigation
        .set_nav_com_aux_for_test(Some(NavTargetRef::cell(11, 10)));
    unit.navigation
        .set_suspended_nav_com_for_test(Some(NavTargetRef::cell(12, 10)));
    unit.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_known(MissionType::Guard),
        suspended: MissionId::NONE,
        queued: MissionId::from_known(MissionType::Unload),
        movement_bypass_latch: 0,
        handler_state: 4,
        mission_start_frame: 3,
        ai_counter: 11,
        dispatch_timer: MissionDispatchTimer::from_raw(3, 90),
    });
    let position = unit.position;
    let drive = serde_json::to_value(
        unit.locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .cloned(),
    )
    .unwrap();
    let path_runtime = unit.navigation.path_runtime;
    let producer_mission = sim.substrate.entities.get(2).unwrap().mission;
    let rng = sim.rng_state();
    assert_eq!(
        sim.substrate
            .occupancy
            .first_building_on_layer(10, 10, MovementLayer::Ground),
        Some(2)
    );
    sim.per_cell_process(
        1,
        super::super::PerCellReason::Arrival,
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();

    let sends: Vec<_> = crate::sim::radio::take_transmit_log()
        .into_iter()
        .map(|send| (send.sender_sid, send.msg, send.target_sid, send.reply))
        .collect();
    assert_eq!(
        sends,
        vec![
            (1, 8, 2, Some(23)),
            (2, 25, 1, Some(1)),
            (1, 25, 2, Some(1)),
            (2, 25, 1, Some(0)),
            (2, 3, 1, Some(1)),
        ]
    );
    for id in [1, 2] {
        let entity = sim.substrate.entities.get(id).unwrap();
        assert_eq!(entity.radio_contacts.slot(0), None);
        assert_eq!(entity.tether.partner(), None);
    }
    let unit = sim.substrate.entities.get(1).unwrap();
    assert_eq!(unit.mission.current().known(), Some(MissionType::Unload));
    assert_eq!(unit.mission.queued(), MissionId::NONE);
    assert_eq!(unit.mission.handler_state(), 0);
    assert_eq!(unit.mission.ai_counter(), 0);
    assert_eq!(
        serde_json::to_value(unit.position).unwrap(),
        serde_json::to_value(position).unwrap()
    );
    assert_eq!(
        serde_json::to_value(
            unit.locomotor
                .as_ref()
                .and_then(|l| l.selected_drive_runtime())
                .and_then(|r| r.retained())
                .cloned()
        )
        .unwrap(),
        drive
    );
    assert_eq!(unit.navigation.path_runtime, path_runtime);
    assert_eq!(unit.navigation.nav_com(), None);
    assert_eq!(unit.navigation.nav_com_aux(), None);
    assert_eq!(
        unit.navigation.suspended_nav_com(),
        Some(NavTargetRef::cell(12, 10))
    );
    assert_eq!(
        sim.substrate.entities.get(2).unwrap().mission,
        producer_mission
    );
    assert_eq!(sim.rng_state(), rng);
}

/// Instruction-level gate coverage for73A7D2..73A943, through the production
/// Unit virtual dispatcher. These are Rust regressions; the original composed
/// controls establish bounded native execution parity separately.
#[test]
fn per_cell_factory_clearance_keeps_reason_mission_rtti_and_ground_building_gates() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId};
    use crate::sim::movement::PerCellReason;

    let cases = [
        (
            "turn",
            PerCellReason::TurnComplete,
            true,
            false,
            5,
            -1,
            None,
            false,
        ),
        (
            "untethered",
            PerCellReason::Arrival,
            false,
            false,
            5,
            -1,
            None,
            false,
        ),
        (
            "effective_unload",
            PerCellReason::Arrival,
            true,
            false,
            -1,
            16,
            None,
            false,
        ),
        (
            "enter_null",
            PerCellReason::Arrival,
            true,
            false,
            -1,
            7,
            None,
            false,
        ),
        (
            "enter_cell",
            PerCellReason::Arrival,
            true,
            false,
            7,
            -1,
            Some(NavTargetRef::cell(10, 10)),
            false,
        ),
        (
            "enter_contact",
            PerCellReason::Arrival,
            true,
            true,
            7,
            -1,
            Some(NavTargetRef::object(2)),
            false,
        ),
        (
            "current_cell",
            PerCellReason::Arrival,
            true,
            false,
            5,
            -1,
            Some(NavTargetRef::cell(10, 10)),
            true,
        ),
        (
            "ground_building",
            PerCellReason::Arrival,
            true,
            false,
            5,
            -1,
            Some(NavTargetRef::cell(11, 10)),
            false,
        ),
        (
            "deck_building",
            PerCellReason::Arrival,
            true,
            true,
            5,
            -1,
            Some(NavTargetRef::cell(11, 10)),
            true,
        ),
        (
            "current_over_queued_enter",
            PerCellReason::Arrival,
            true,
            false,
            5,
            7,
            None,
            true,
        ),
    ];
    for (name, reason, tethered, deck, current, queued, nav_com, sends_clearance) in cases {
        let (mut sim, rules) = factory_per_cell_fixture(tethered, deck, "");
        let unit = sim.substrate.entities.get_mut(1).unwrap();
        unit.navigation.set_nav_com_for_test(nav_com);
        unit.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(current),
            suspended: MissionId::NONE,
            queued: MissionId::from_raw(queued),
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        let rng = sim.rng_state();
        sim.per_cell_process(
            1,
            reason,
            Some(&rules),
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
        let sends = crate::sim::radio::take_transmit_log();
        // Object-destination arrival73A31F..A547 runs DockNow21 and returns
        // before the later factory-clearance gate. The shared original
        // per-cell controls in refinery_dock.json cover that earlier arm.
        let object_dock = name == "enter_contact";
        assert_eq!(
            sends.first().map(|send| send.msg),
            if object_dock {
                Some(21)
            } else {
                sends_clearance.then_some(8)
            },
            "{name}"
        );
        if object_dock {
            assert!(sends.iter().all(|send| send.msg != 8));
            assert!(
                !sim.substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .locomotor
                    .as_ref()
                    .unwrap()
                    .is_powered()
            );
        }
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .radio_contacts
                .slot(0),
            (!sends_clearance).then_some(2),
            "{name}"
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(2)
                .unwrap()
                .radio_contacts
                .slot(0),
            (!sends_clearance).then_some(1),
            "{name}"
        );
        assert_eq!(sim.rng_state(), rng, "{name}");
    }
    // As_Techno40DD70 admits all four Techno RTTIs, not only Building. Each
    // Enter destination differs from Contact0; the producer is on deck so
    // the independent first-ground-Building gate does not hide this check.
    for category in [
        EntityCategory::Unit,
        EntityCategory::Aircraft,
        EntityCategory::Structure,
        EntityCategory::Infantry,
    ] {
        let (mut sim, rules) = factory_per_cell_fixture(true, true, "");
        let target = GameEntity::test_default_of_category(3, "MTNK", "Americans", 12, 10, category);
        sim.substrate.entities.insert(target);
        let unit = sim.substrate.entities.get_mut(1).unwrap();
        unit.navigation
            .set_nav_com_for_test(Some(NavTargetRef::Object { id: 3 }));
        unit.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(7),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        sim.per_cell_process(
            1,
            PerCellReason::Arrival,
            Some(&rules),
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
        let sends = crate::sim::radio::take_transmit_log();
        assert_eq!(sends.first().map(|send| send.msg), Some(8), "{category:?}");
    }
}

/// 73A9CA..73A9D9: a Unit holding a SlaveManager (+2D8) starts its hunt
/// (6B0CC0) at the factory clearance, where another Unit takes its archive
/// as its destination.
#[test]
fn per_cell_factory_clearance_starts_a_slave_miners_hunt() {
    use crate::sim::combat::TargetKind;
    use crate::sim::slave_manager::{ManagerState, SlaveManager};

    let (mut sim, rules) = factory_per_cell_fixture(true, false, "");
    let slave_type = sim.intern("SLAV");
    let unit = sim.substrate.entities.get_mut(1).unwrap();
    unit.set_archive_target(Some(TargetKind::Cell(12, 10)));
    unit.slave_manager = Some(SlaveManager::new(slave_type, [None; 0], 0, 0, 0));
    sim.per_cell_process(
        1,
        super::super::PerCellReason::Arrival,
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(
        crate::sim::radio::take_transmit_log()
            .first()
            .map(|send| send.msg),
        Some(8)
    );
    let unit = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        unit.slave_manager.as_ref().unwrap().state(),
        ManagerState::Scanning
    );
    assert_eq!(unit.navigation.nav_com(), None, "no archive destination");
}

/// A computer House's tank as a 5x3 war factory clears it: on the factory's
/// Move with nothing queued, standing on the foundation at location +(4,1)
/// where the exit track ends, HeadTo empty, NavCom null, an archive of its own.
fn computer_product_at_factory_exit() -> (Simulation, RuleSet) {
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};

    let (mut sim, rules) = factory_per_cell_fixture(true, false, "");
    let owner = sim.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 10);
    house.base_center = Some((20, 20));
    sim.houses.insert_for_test(house);
    let producer = sim.substrate.entities.get_mut(2).unwrap();
    producer.type_facts.set_foundation_for_test("5x3".into());
    producer.position.put_location(DriveCoord::cell(6, 9, 0));
    sim.substrate.occupancy =
        crate::sim::occupancy::OccupancyGrid::rebuild(&sim.substrate.entities);
    let unit = sim.substrate.entities.get_mut(1).unwrap();
    unit.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_known(MissionType::Move),
        suspended: MissionId::NONE,
        queued: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 4,
        mission_start_frame: 3,
        ai_counter: 11,
        dispatch_timer: MissionDispatchTimer::from_raw(3, 90),
    });
    unit.set_archive_target(Some(crate::sim::combat::TargetKind::Cell(12, 10)));
    (sim, rules)
}

/// 73AA0D..73AA9C: the computer House's tank takes its House500200 post,
/// not its archive, and the following Ready/Commence (73ACC2/73ACD1)
/// commences the queued AreaGuard at once: the unit is not moving there,
/// as in the native factory-unload trace.
#[test]
fn per_cell_factory_clearance_sends_a_computer_unit_to_its_post() {
    use crate::sim::combat::TargetKind;
    use crate::sim::components::NavTargetRef;
    use crate::sim::mission::{MissionId, MissionType};

    let world = || {
        let (mut sim, rules) = computer_product_at_factory_exit();
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        (sim, rules)
    };
    let (mut twin, _) = world();
    let owner = twin.substrate.entities.get(1).unwrap().owner();
    let post = twin
        .house_return_cell(owner, 1)
        .expect("a post near the base");
    let (mut sim, rules) = world();
    sim.per_cell_process(
        1,
        super::super::PerCellReason::Arrival,
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(
        crate::sim::radio::take_transmit_log()
            .first()
            .map(|send| (send.msg, send.reply)),
        Some((8, Some(23)))
    );
    let unit = sim.substrate.entities.get(1).unwrap();
    assert_eq!(unit.mission.current().known(), Some(MissionType::AreaGuard));
    assert_eq!(unit.mission.queued(), MissionId::NONE);
    assert_eq!(unit.mission.mission_start_frame(), 57);
    assert_eq!(unit.mission.handler_state(), 0);
    assert_eq!(
        unit.navigation.nav_com(),
        Some(NavTargetRef::cell(post.0, post.1))
    );
    assert_eq!(
        unit.archive_target(),
        Some(TargetKind::Cell(post.0, post.1))
    );
    assert_eq!(
        sim.rng_state(),
        twin.rng_state(),
        "House500200's draws only"
    );
}

/// 73AB43: without a post the archive clears and nothing is queued or
/// assigned. Mapless, House500200 has no map authority and answers no post
/// before drawing; native's zero post follows both draws.
#[test]
fn per_cell_factory_clearance_without_a_post_clears_the_archive() {
    use crate::sim::components::NavTargetRef;

    let (mut sim, rules) = computer_product_at_factory_exit();
    let mission = sim.substrate.entities.get(1).unwrap().mission;
    sim.per_cell_process(
        1,
        super::super::PerCellReason::Arrival,
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(
        crate::sim::radio::take_transmit_log()
            .first()
            .map(|send| send.msg),
        Some(8)
    );
    let unit = sim.substrate.entities.get(1).unwrap();
    assert_eq!(unit.archive_target(), None);
    assert_ne!(
        unit.navigation.nav_com(),
        Some(NavTargetRef::cell(12, 10)),
        "no archive destination"
    );
    assert_eq!(unit.mission, mission);
}

#[test]
fn per_cell_factory_clearance_reads_unit_archive_through_the_shared_setter() {
    use crate::sim::combat::TargetKind;
    use crate::sim::components::NavTargetRef;

    let (mut sim, rules) = factory_per_cell_fixture(true, false, "");
    let unit = sim.substrate.entities.get_mut(1).unwrap();
    unit.set_archive_target(Some(TargetKind::Cell(12, 10)));
    let mission = unit.mission;
    let position = unit.position;
    let rng = sim.rng_state();
    sim.per_cell_process(
        1,
        super::super::PerCellReason::Arrival,
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(
        crate::sim::radio::take_transmit_log()
            .first()
            .map(|send| send.msg),
        Some(8)
    );
    let unit = sim.substrate.entities.get(1).unwrap();
    assert_eq!(unit.navigation.nav_com(), Some(NavTargetRef::cell(12, 10)));
    assert_eq!(
        unit.locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .destination(),
        Some(DriveCoord::cell(12, 10, 0))
    );
    assert_eq!(unit.archive_target(), Some(TargetKind::Cell(12, 10)));
    assert_eq!(
        unit.mission, mission,
        "this native arm does not queue or enter idle"
    );
    assert_eq!(
        serde_json::to_value(unit.position).unwrap(),
        serde_json::to_value(position).unwrap(),
        "SetDestination does not execute a movement turn"
    );
    assert_eq!(sim.rng_state(), rng);
}

/// Type+E0E/E0F ->73AAE6 queues Harvest with commence=1. This consumer must
/// replace the factory's queued Move through the shared Mission authority;
/// live Unit744270 readiness can defer it while the factory's middle row
/// remains underneath. No harvest handler or miner lifecycle runs here.
#[test]
fn per_cell_factory_harvester_queue_uses_live_readiness_and_preserves_miner_state() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::miner::{Miner, MinerConfig, MinerKind};
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};

    for (key, reader_line) in [("Harvester", "Harvester=yes"), ("Weeder", "Weeder=true")] {
        for middle_row_underneath in [false, true] {
            let (mut sim, rules) = factory_per_cell_fixture(true, false, reader_line);
            let object = rules.object("MTNK").unwrap();
            assert_eq!(
                (object.harvester, object.weeder),
                (key == "Harvester", key == "Weeder")
            );
            if middle_row_underneath {
                let producer = sim.substrate.entities.get_mut(2).unwrap();
                producer.type_facts.set_foundation_for_test("1x2".into());
                producer.position.put_location(DriveCoord::cell(10, 9, 0));
                sim.substrate.occupancy =
                    crate::sim::occupancy::OccupancyGrid::rebuild(&sim.substrate.entities);
            }
            let unit = sim.substrate.entities.get_mut(1).unwrap();
            unit.miner = Some(Miner::new(MinerKind::War, &MinerConfig::default(), 0));
            unit.navigation
                .set_nav_com_aux_for_test(Some(NavTargetRef::cell(13, 10)));
            unit.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_known(MissionType::Guard),
                suspended: MissionId::NONE,
                queued: MissionId::from_known(MissionType::Move),
                movement_bypass_latch: 0,
                handler_state: 4,
                mission_start_frame: 3,
                ai_counter: 11,
                dispatch_timer: MissionDispatchTimer::from_raw(3, 90),
            });
            let miner = serde_json::to_value(&unit.miner).unwrap();
            let lifecycle = unit.lifecycle;
            let position = unit.position;
            let rng = sim.rng_state();
            sim.per_cell_process(
                1,
                super::super::PerCellReason::Arrival,
                Some(&rules),
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
            let sends = crate::sim::radio::take_transmit_log();
            assert_eq!(
                sends.first().map(|send| (send.msg, send.reply)),
                Some((8, Some(23)))
            );
            let unit = sim.substrate.entities.get(1).unwrap();
            if middle_row_underneath {
                assert_eq!(unit.mission.current().known(), Some(MissionType::Guard));
                assert_eq!(unit.mission.queued().known(), Some(MissionType::Harvest));
                assert_eq!(unit.mission.handler_state(), 4);
            } else {
                assert_eq!(unit.mission.current().known(), Some(MissionType::Harvest));
                assert_eq!(unit.mission.queued(), MissionId::NONE);
                assert_eq!(unit.mission.handler_state(), 0);
                assert_eq!(unit.mission.mission_start_frame(), 57);
            }
            assert_eq!(serde_json::to_value(&unit.miner).unwrap(), miner);
            assert_eq!(unit.lifecycle, lifecycle);
            assert_eq!(
                serde_json::to_value(unit.position).unwrap(),
                serde_json::to_value(position).unwrap()
            );
            assert_eq!(unit.navigation.nav_com(), None);
            assert_eq!(
                unit.navigation.nav_com_aux(),
                Some(NavTargetRef::cell(13, 10))
            );
            for id in [1, 2] {
                let entity = sim.substrate.entities.get(id).unwrap();
                assert_eq!(entity.radio_contacts.slot(0), None);
                assert_eq!(entity.tether.partner(), None);
            }
            assert_eq!(
                sim.rng_state(),
                rng,
                "{key}, middle_row={middle_row_underneath}"
            );
        }
    }
}

#[test]
fn idle_receiver_cancels_burst_through_the_shared_target_setter() {
    use crate::sim::combat::TargetKind;
    use crate::sim::components::NavTargetRef;
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        for has_destination in [false, true] {
            let (mut sim, _, _) = fixture(family, 0);
            let entity = sim.substrate.entities.get_mut(1).unwrap();
            entity
                .attack_target
                .set_for_test(Some(TargetKind::Cell(12, 10)));
            entity
                .passive_scan
                .set_passively_acquired_target_for_test(true);
            entity.weapon_burst.complete_shot(2);
            entity
                .navigation
                .set_nav_com_for_test(has_destination.then(|| NavTargetRef::cell(12, 10)));
            sim.unit_enter_idle_mode(1, None, false, crate::sim::world::FrameEffects::default());
            let entity = sim.substrate.entities.get(1).unwrap();
            assert_eq!(entity.weapon_burst.index(), i32::from(has_destination));
            assert_eq!(entity.passive_scan.acquired(), has_destination);
            assert_eq!(
                entity.attack_target.target(),
                has_destination.then_some(TargetKind::Cell(12, 10)),
            );
            assert_eq!(
                entity.mission.queued(),
                MissionId::from_known(if has_destination {
                    MissionType::Move
                } else {
                    MissionType::Guard
                }),
            );
        }
    }
}

/// Original Drive4B1F97..RET4 joins PerCell739EC0, radio21, StopMoving4DF0D0
/// and Foot vt5044DB9B0. The selected armed Enter tail queues Guard after
/// DOCK_NOW queues Sleep. This comparison includes the actual intermediate
/// PerCell result; preserving Sleep would regress the native arrival path.
#[test]
fn native_depot_arrival_uses_the_original_terminal_handoff() {
    use crate::map::resolved_terrain::test_flat_ground_grid;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::sim::combat::TargetKind;
    use crate::sim::components::NavTargetRef;
    use crate::sim::estimated_health::EstimatedHealth;
    use crate::sim::mission::MissionDispatchTimer;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::pathfinding::PathGrid;
    use crate::sim::rng::SimRng;
    use crate::sim::stage::StageClass;
    use crate::sim::timer::CdTimer;
    use serde_json::{Value, json};
    use std::sync::Arc;

    fn integer(value: &Value) -> i32 {
        value.as_i64().unwrap() as i32
    }

    fn ini_text(sections: &Value) -> String {
        let mut text = String::new();
        for (section, keys) in sections.as_object().unwrap() {
            text.push_str(&format!("[{section}]\n"));
            for (key, value) in keys.as_object().unwrap() {
                text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
            }
        }
        text
    }

    fn observed(sim: &Simulation, tank: u64, depot: u64, other: Option<u64>) -> Value {
        let unit = sim.substrate.entities.get(tank).unwrap();
        let building = sim.substrate.entities.get(depot).unwrap();
        let house = &sim.houses[&unit.owner()];
        let name = |id: Option<u64>| match id {
            None => Value::Null,
            Some(id) if id == tank => json!("miner"),
            Some(id) if id == depot => json!("refinery"),
            Some(id) if Some(id) == other => json!("other"),
            Some(id) => panic!("unrepresented arrival object {id}"),
        };
        let nav = |target: Option<NavTargetRef>| match target {
            None => Value::Null,
            Some(NavTargetRef::Cell { rx, ry }) => json!([rx, ry]),
            Some(NavTargetRef::Object { id }) => name(Some(id)),
        };
        let target = |target: Option<TargetKind>| match target {
            None => Value::Null,
            Some(TargetKind::Cell(rx, ry)) => json!([rx, ry]),
            Some(TargetKind::Entity(id)) => name(Some(id)),
        };
        let at = super::super::ground_pose::object_get_coords(unit, sim.resolved_terrain.as_ref());
        let destination = unit
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .destination();
        let stage = building
            .mission_leaf
            .as_building()
            .unwrap()
            .repair_progress();
        let raw_stage = serde_json::to_value(stage).unwrap();
        let rng = sim.scenario_rng.logical_view();
        json!({
            "frame": sim.session.binary_frame(),
            "unit_coordinate": [at.x,at.y,at.z],
            "health": unit.health.current(),
            "estimate": unit.estimated_health.get(),
            "balance": house.economy.credits(),
            "spent": house.economy.spent_credits(),
            "unit_mission": unit.mission.current().raw(),
            "unit_queued": unit.mission.queued().raw(),
            "unit_nav": nav(unit.navigation.nav_com()),
            "unit_nav_aux": nav(unit.navigation.nav_com_aux()),
            "unit_archive": target(unit.archive_target()),
            "pending_entry": name(unit.pending_entry()),
            "unit_contacts": (0..unit.radio_contacts.capacity()).map(|n| name(unit.radio_contacts.slot(n))).collect::<Vec<_>>(),
            "building_contacts": (0..building.radio_contacts.capacity()).map(|n| name(building.radio_contacts.slot(n))).collect::<Vec<_>>(),
            "unit_tether": u8::from(unit.tether.partner().is_some()),
            "building_tether": u8::from(building.tether.partner().is_some()),
            "building_mission": building.mission.current().raw(),
            "building_queued": building.mission.queued().raw(),
            "status": building.mission.handler_state(),
            "dispatch_words": [building.mission.dispatch_timer().start_frame(),building.mission.dispatch_timer().delay()],
            "stage": [json!(stage.value()),raw_stage["changed"].clone(),json!(stage.timer().start_frame()),json!(stage.timer().duration()),json!(stage.rate()),raw_stage["increment"].clone()],
            "repairing": building.building_ready_latch(),
            "locomotor_powered": u8::from(unit.locomotor.as_ref().unwrap().is_powered()),
            "locomotor_destination": destination.map_or_else(|| json!([0,0,0]), |at| json!([at.x,at.y,at.z])),
            "scenario_rng": {"disabled":rng.disabled,"index_a":rng.index_a,"index_b":rng.index_b,"state":rng.words}
        })
    }

    fn compare(
        sim: &Simulation,
        tank: u64,
        depot: u64,
        other: Option<u64>,
        native: &Value,
        context: &str,
    ) {
        for (field, actual) in observed(sim, tank, depot, other).as_object().unwrap() {
            assert_eq!(actual, &native[field], "{context}: {field}");
        }
    }

    let golden: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_repair.depot_service.json",
    ))
    .unwrap();
    let arrival = &golden["arrival_terminal"];
    let controls = arrival["controls"].as_array().unwrap();
    assert_eq!(controls.len(), 7);
    for row in controls {
        let input = &row["input"];
        let before = &row["before"];
        let context = input["name"].as_str().unwrap();
        let mut stack = None;
        for (base, extra) in golden["inputs"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .zip(arrival["native_inputs"]["layers"].as_array().unwrap())
        {
            if base["absent"] == true {
                continue;
            }
            assert_eq!(base["file"], extra["file"]);
            let mut sections = base["selected"].clone();
            for line in extra["source_lines"].as_array().unwrap() {
                let section = line["section"].as_str().unwrap();
                let key = line["key"].as_str().unwrap();
                if sections[section].is_null() {
                    sections[section] = json!({});
                }
                sections[section][key] = line["value"].clone();
            }
            let text = format!(
                "[VehicleTypes]\n0=HTNK\n1=MTNK\n[BuildingTypes]\n0=NADEPT\n1=GADEPT\n{}",
                ini_text(&sections)
            );
            // This bounded native arrival control supplies admitted actors
            // on clear ground, but its selected-key packet omits LandType.
            // Supply that synthetic construction prior in the first layer;
            // the extracted retail keys and later passes keep precedence.
            let ini = if base["file"] == "RULESMD.INI" {
                crate::sim::arena_fixture::clear_ground_ini(&text)
            } else {
                IniFile::from_str(&text)
            };
            match base["file"].as_str().unwrap() {
                "RULESMD.INI" => stack = Some(RulesLayerStack::new(ini)),
                name => stack.as_mut().unwrap().push(
                    match name {
                        "LANGRULE.INI" => RulesLayerKind::LangRule,
                        "MPBattleMD.ini" => RulesLayerKind::GameMode,
                        "XMP03T4.MAP" => RulesLayerKind::Scenario,
                        other => panic!("unrepresented arrival layer {other}"),
                    },
                    ini,
                ),
            }
        }
        let unit_name = row["unit"].as_str().unwrap();
        let building_name = row["building"].as_str().unwrap();
        let service = input["unit_repair"].as_bool().unwrap_or(true);
        let mut stack = stack.unwrap();
        stack.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(&format!(
                "[{unit_name}]\nLocomotor={{4A582741-9839-11D1-B709-00A024DDAFD1}}\n\
                 [{building_name}]\nUnitRepair={}\n",
                if service { "yes" } else { "no" }
            )),
        );
        let art = IniFile::from_str(&ini_text(&golden["inputs"]["art"]["selected"]));
        let mut rules =
            RuleSet::from_processed_rules(&stack.process_with_fixed_art(&art).unwrap()).unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&art));
        let mut sim = Simulation::new();
        sim.session
            .set_binary_frame_for_test(integer(&before["frame"]) as u32);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        // Same declared clear-map membership as the native joined fixture.
        let bounds = crate::map::playfield::PlayfieldBounds::from_normalized_local_size(
            16, -16, -16, 64, 64,
        );
        sim.playfield_bounds = Some(bounds);
        sim.playfield_size_height = Some(16);
        sim.session.set_map_width_for_test(16);
        sim.session.set_map_height_for_test(16);
        let terrain = test_flat_ground_grid(32);
        sim.path_grid = Some(Arc::new(PathGrid::from_resolved_terrain(&terrain)));
        sim.install_resolved_terrain_for_new_map(terrain);
        let owner = sim.intern("Russians");
        sim.houses
            .insert_for_test(crate::sim::house_state::HouseState::new(
                owner, 1, None, true, 0, 0,
            ));
        let depot = sim
            .spawn_object(building_name, "Russians", 6, 9, 0, &rules)
            .unwrap();
        let tank = sim
            .spawn_object(unit_name, "Russians", 10, 10, 0, &rules)
            .unwrap();
        let object = rules.object(unit_name).unwrap();
        assert_eq!(
            object.primary(),
            arrival["native_inputs"]["after"]["types"][unit_name]["primary_name"].as_str(),
            "{context}: production Primary reader"
        );
        let native_getter = arrival["native_inputs"]["original_getters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|getter| getter["type"] == unit_name)
            .unwrap();
        assert_eq!(
            crate::sim::combat::combat_weapon::is_armed(
                sim.substrate.entities.get(tank).unwrap(),
                object
            ),
            native_getter["is_armed_eax"] != 0,
            "{context}: canonical IsArmed"
        );
        let other = (row["supplied_before"]["target"] == "other").then(|| {
            sim.spawn_object(building_name, "Russians", 20, 20, 0, &rules)
                .unwrap()
        });
        for (id, peer, mission, queued, contacts, tether) in [
            (
                tank,
                depot,
                "unit_mission",
                "unit_queued",
                "unit_contacts",
                "unit_tether",
            ),
            (
                depot,
                tank,
                "building_mission",
                "building_queued",
                "building_contacts",
                "building_tether",
            ),
        ] {
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_raw(integer(&before[mission])),
                suspended: MissionId::NONE,
                queued: MissionId::from_raw(integer(&before[queued])),
                movement_bypass_latch: 0,
                handler_state: if id == depot {
                    integer(&before["status"]) as u32
                } else {
                    0
                },
                mission_start_frame: integer(&before["frame"]) as u32,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::from_raw(-1, 0),
            });
            entity.radio_contacts.clear_all();
            entity
                .radio_contacts
                .set_capacity(before[contacts].as_array().unwrap().len());
            if before[contacts][0].is_string() {
                entity.radio_contacts.insert(peer);
            }
            entity
                .tether
                .set_for_test((integer(&before[tether]) != 0).then_some(peer));
        }
        let at = &before["unit_coordinate"];
        let coordinate = DriveCoord {
            x: integer(&at[0]),
            y: integer(&at[1]),
            z: integer(&at[2]),
        };
        let unit = sim.substrate.entities.get_mut(tank).unwrap();
        unit.position.set_rx_for_test((coordinate.x / 256) as u16);
        unit.position.set_ry_for_test((coordinate.y / 256) as u16);
        unit.position
            .set_sub_x_for_test(SimFixed::from_num(coordinate.x % 256));
        unit.position
            .set_sub_y_for_test(SimFixed::from_num(coordinate.y % 256));
        unit.position
            .set_exact_z_leptons_for_test(Some(coordinate.z));
        unit.health.set_for_test(integer(&before["health"]));
        unit.estimated_health = EstimatedHealth::from_raw(integer(&before["estimate"]));
        unit.navigation.set_nav_com_for_test(
            before["unit_nav"]
                .is_string()
                .then_some(NavTargetRef::Object { id: depot }),
        );
        unit.navigation.set_nav_com_aux_for_test(None);
        unit.attack_target
            .set_for_test(other.map(TargetKind::Entity));
        let destination = &before["locomotor_destination"];
        let destination = DriveCoord {
            x: integer(&destination[0]),
            y: integer(&destination[1]),
            z: integer(&destination[2]),
        };
        let terminal = input["entry"] == "terminal_drive";
        assert!(
            unit.locomotor
                .as_mut()
                .unwrap()
                .install_drive_state_for_test(Some(
                    DriveLocomotionRuntime::default()
                        .with_destination_for_test(
                            (destination != DriveCoord { x: 0, y: 0, z: 0 }).then_some(destination)
                        )
                        .with_head_to_for_test(terminal.then_some(coordinate))
                        .with_track_for_test(TrackProgress::for_test(
                            integer(&row["supplied_before"]["track_selector"]),
                            integer(&row["supplied_before"]["track_cursor"]),
                            false,
                            0
                        ))
                        .with_track_valid_for_test(terminal)
                ))
        );
        if integer(&before["locomotor_powered"]) == 0 {
            unit.locomotor.as_mut().unwrap().power_off();
        }
        let building = sim.substrate.entities.get_mut(depot).unwrap();
        building
            .mission_leaf
            .set_building_ready_latch(integer(&before["repairing"]) as u8);
        let stage = &before["stage"];
        building
            .mission_leaf
            .install_building_repair_progress_fixture(StageClass::from_native_fixture(
                integer(&stage[0]),
                integer(&stage[1]) as u8,
                CdTimer::from_raw(integer(&stage[2]), integer(&stage[3])),
                integer(&stage[4]),
                integer(&stage[5]),
            ));
        let house = sim.houses.get_mut(&owner).unwrap();
        house
            .economy
            .set_credits_for_test(integer(&before["balance"]));
        house.economy.set_spent_for_test(integer(&before["spent"]));
        sim.substrate.occupancy =
            crate::sim::occupancy::OccupancyGrid::rebuild(&sim.substrate.entities);
        sim.scenario_rng = SimRng::new(input["seed"].as_u64().unwrap_or(1));
        assert!(sim.track_survives(tank), "{context}: admitted live Unit");
        let building = sim.substrate.entities.get(depot).unwrap();
        let center =
            super::super::ground_pose::object_get_coords(building, sim.resolved_terrain.as_ref());
        assert_eq!(
            json!([center.x, center.y, center.z]),
            row["center_coordinate"],
            "{context}: original Building GetCoords"
        );
        if service {
            // The negative control clears UnitRepair after choosing the stock
            // placement point. Its saved dock_coordinate is that supplied
            // placement, not a getter observation of the altered type.
            let dock = super::super::navcom::building_dock_coordinate(
                &sim.substrate.entities,
                depot,
                Some(tank),
                sim.resolved_terrain.as_ref(),
                &rules,
                &sim.interner,
            )
            .unwrap();
            assert_eq!(
                json!([dock.x, dock.y, dock.z]),
                row["dock_coordinate"],
                "{context}: original stock Building GetDockCoord"
            );
        }
        compare(&sim, tank, depot, other, before, context);

        if input["entry"] == "unit_idle" {
            // Both original second-argument controls have ctor TubeIndex=-1.
            // This owner has no retained tube continuation to resume.
            sim.unit_enter_idle_mode(
                tank,
                Some(&rules),
                false,
                crate::sim::world::FrameEffects::default(),
            );
        } else if input["entry"] == "navigation_gate" {
            let returns = sim.track_navigation_gate(
                tank,
                Some(&rules),
                crate::sim::world::FrameEffects::default(),
            );
            assert_eq!(
                u8::from(returns),
                row["returned_al"].as_u64().unwrap() as u8,
                "{context}: Foot gate AL"
            );
        } else {
            assert!(terminal, "{context}: declared terminal admission");
            let sample = &arrival["native_inputs"]["terminal_sample"];
            assert_eq!(
                sample["original_sample"]["budget"], row["entry_registers"]["local_budget"],
                "{context}: native terminal entry follows original point payment"
            );
            let mut per_cell_calls = 0;
            let pass = sim
                .try_run_track_points_observed(
                    TrackInvocation {
                        entity_id: tank,
                        family: TrackFamily::Drive,
                        active_gate: false,
                        retry: false,
                    },
                    // Rust starts before point payment; the original joined
                    // terminal body starts after it. OriginalCursor executes
                    // this supplied budget and records the paid entry budget.
                    integer(&sample["supplied_budget"]),
                    Some(&rules),
                    None,
                    &mut |world, _, event| {
                        if event == TrackWorldEvent::PerCell {
                            per_cell_calls += 1;
                            let native = row["interior_snapshots"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .find(|snapshot| snapshot["marker"] == "after_per_cell")
                                .unwrap();
                            compare(world, tank, depot, other, &native["state"], context);
                        }
                    },
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap();
            assert_eq!(per_cell_calls, 1, "{context}: original terminal PerCell");
            assert_eq!(
                u8::from(pass.aborted),
                row["returned_al"].as_u64().unwrap() as u8,
                "{context}: terminal AL"
            );
        }
        compare(&sim, tank, depot, other, &row["after"], context);
        assert_eq!(
            sim.substrate
                .entities
                .get(tank)
                .unwrap()
                .attack_target
                .target(),
            (row["supplied_after"]["target"] == "other")
                .then(|| TargetKind::Entity(other.unwrap())),
            "{context}: retained target"
        );
    }
}

fn idle_base_fixture(category: EntityCategory) -> Simulation {
    let mut sim = Simulation::with_seed(0x6B3);
    let type_id = match category {
        EntityCategory::Unit => "MTNK",
        EntityCategory::Infantry => "E1",
        EntityCategory::Aircraft => "ORCA",
        _ => panic!("the idle base fixture supplies only Foot categories"),
    };
    let mut actor = GameEntity::test_default_of_category(1, type_id, "Americans", 10, 10, category);
    sim.interner = crate::sim::intern::test_interner();
    // Original fixture end=none, +687=0 and no planning/Archive work. No
    // active controller or class tail is supplied by this comparison.
    actor.locomotor = None;
    actor.lifecycle.set_object_alive_for_test(true);
    actor.lifecycle.set_in_limbo_for_test(false);
    actor.lifecycle.set_cell_marked(false);
    sim.substrate.entities.insert(actor);
    sim.substrate.next_stable_object_id = 2;
    sim
}

fn idle_base_native_rows() -> Vec<serde_json::Value> {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/foot_enter_idle.json",
    ))
    .expect("unchanged original Foot EnterIdle corpus")
}

/// Foot4D82B0..4D8557 executes unchanged in the original oracle. Its
/// Techno709A40 and speed setter are supplied callbacks; this comparison
/// establishes base admission/AL and the inherited byte, not those callees,
/// the class tail, actual END policy or Infantry Archive pursuit.
#[test]
fn foot_idle_empty_queue_admission_matches_original_latch_rows() {
    let rows = idle_base_native_rows();
    let selected = rows.iter().filter(|row| {
        let input = &row["input"];
        input["group"] == "core"
            && input["end"] == "none"
            && input["scatter_pending"] == 0
            && input["queue"].as_array().is_some_and(Vec::is_empty)
            // The supplied Techno callback sees these args. This Rust base
            // API does not claim to reproduce their forwarding or effects.
            && input["args"] == serde_json::json!([0, 0])
    });
    let mut compared = 0;
    for row in selected {
        let mut sim = idle_base_fixture(EntityCategory::Unit);
        let actor = sim.substrate.entities.get_mut(1).unwrap();
        assert_eq!(actor.mission_leaf.foot_idle_entry_latch(), 0);
        actor
            .mission_leaf
            .set_foot_idle_entry_latch(row["before"]["latch"].as_u64().unwrap() as u8);
        let rng = sim.rng_state();
        let returned =
            sim.foot_enter_idle_base(1, None, None, crate::sim::world::FrameEffects::default());
        assert_eq!(
            u8::from(returned),
            row["returned_al"].as_u64().unwrap() as u8
        );
        let actor = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            actor.mission_leaf.foot_idle_entry_latch(),
            row["after"]["latch"].as_u64().unwrap() as u8
        );
        assert!(actor.navigation.nav_queue().is_empty());
        assert_eq!(actor.navigation.nav_com(), None);
        assert_eq!(sim.rng_state(), rng);
        compared += 1;
    }
    assert_eq!(compared, 2, "both zero and set native latch controls");
}

/// Original nonzero6B3 returns before Techno/Scatter/END/destination/speed.
/// The native queued Cell target is opaque on this branch; its coordinates
/// and the speed sentinel below are supplied refusal-control priors.
#[test]
fn foot_idle_set_latch_refuses_queue_and_speed_setters_without_effects() {
    let rows = idle_base_native_rows();
    let row = rows
        .iter()
        .find(|row| {
            let input = &row["input"];
            input["group"] == "core"
                && input["end"] == "none"
                && input["scatter_pending"] == 0
                && input["latch"] == 1
                && input["queue"] == serde_json::json!([0])
                && input["args"] == serde_json::json!([0, 0])
        })
        .expect("original entered-latch/queued-Cell control");
    assert!(row["events"].as_array().unwrap().is_empty());
    assert!(row["writes"].as_array().unwrap().is_empty());
    let mut sim = idle_base_fixture(EntityCategory::Unit);
    let actor = sim.substrate.entities.get_mut(1).unwrap();
    actor
        .mission_leaf
        .set_foot_idle_entry_latch(row["before"]["latch"].as_u64().unwrap() as u8);
    actor
        .navigation
        .push_nav_queue_for_test(crate::sim::components::NavTargetRef::cell(20, 20));
    actor.foot_speed.set_speed_fraction(SimFixed::lit("0.625"));
    let before = save_codec::serialize(actor).unwrap();
    let hash = sim.state_hash();
    let rng = sim.rng_state();
    assert_eq!(
        u8::from(sim.foot_enter_idle_base(
            1,
            None,
            None,
            crate::sim::world::FrameEffects::default()
        )),
        row["returned_al"].as_u64().unwrap() as u8
    );
    assert_eq!(
        save_codec::serialize(sim.substrate.entities.get(1).unwrap()).unwrap(),
        before
    );
    assert_eq!(sim.state_hash(), hash);
    assert_eq!(sim.rng_state(), rng);
}

/// Foot4D831A..4D8376 asks the active locomotor's IPiggyback, whatever its
/// class, and the original rows supply its permission and the object END
/// hands back. A Chrono Warp's Teleport whose warp is over may end here, as
/// a Drive may; mid-warp it refuses. AL, the restore and the zero-speed
/// setter follow the rows. Teleport's own gate (`0x00719F30`) is read, not
/// executed by this corpus.
#[test]
fn foot_idle_ends_a_warp_teleport_like_original_end_rows() {
    use crate::rules::locomotor_type::LocomotorKind;
    let rows = idle_base_native_rows();
    let mut compared = 0;
    for row in rows.iter().filter(|row| {
        let input = &row["input"];
        input["group"] == "core"
            && input["latch"] == 0
            && input["scatter_pending"] == 0
            && input["queue"].as_array().is_some_and(Vec::is_empty)
            && input["args"] == serde_json::json!([0, 0])
            && (input["end"] == "success" || input["end"] == "denied")
    }) {
        let mut sim = idle_base_fixture(EntityCategory::Unit);
        let actor = sim.substrate.entities.get_mut(1).unwrap();
        let mut locomotor =
            super::super::locomotor::LocomotorState::for_test_kind(LocomotorKind::Drive);
        assert!(locomotor.begin_piggyback(LocomotorKind::Teleport, 20));
        if row["input"]["end"] == "denied" {
            let warp =
                super::super::teleport_movement::ChronoWarp::new(DriveCoord::cell(12, 8, 0), 20);
            locomotor.teleport_runtime_mut().unwrap().arm_chrono(warp);
        }
        actor.locomotor = Some(locomotor);
        actor.foot_speed.set_speed_fraction(SimFixed::lit("0.625"));
        let returned =
            sim.foot_enter_idle_base(1, None, None, crate::sim::world::FrameEffects::default());

        let events: Vec<&str> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| event["event"].as_str().unwrap())
            .collect();
        assert_eq!(
            u8::from(returned),
            row["returned_al"].as_u64().unwrap() as u8
        );
        let actor = sim.substrate.entities.get(1).unwrap();
        let locomotor = actor.locomotor.as_ref().unwrap();
        let restored = events.contains(&"restore");
        assert_eq!(locomotor.active_kind() == LocomotorKind::Drive, restored);
        assert_eq!(locomotor.piggyback().is_none(), restored);
        assert_eq!(
            actor.foot_speed.applied_fraction() == crate::util::fixed_math::SIM_ZERO,
            events.contains(&"speed")
        );
        compared += 1;
    }
    assert_eq!(compared, 2, "the permitted and the refused END rows");
}

/// The inherited byte is saved by VERA snapshot286 and contributes to the
/// canonical world hash for Unit, Infantry and Aircraft. This is a Rust
/// persistence/hash regression, not original gamemd save-file equivalence.
#[test]
fn inherited_foot_idle_latch_roundtrips_and_hashes_for_every_foot_category() {
    use crate::sim::snapshot::GameSnapshot;
    for category in [
        EntityCategory::Unit,
        EntityCategory::Infantry,
        EntityCategory::Aircraft,
    ] {
        let mut sim = idle_base_fixture(category);
        // The production deserializer resets Scenario RNG; isolate this byte
        // by normalizing that independent load behavior before whole hashes.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let clear_hash = sim.state_hash();
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .mission_leaf
                .foot_idle_entry_latch(),
            0
        );
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .mission_leaf
            .set_foot_idle_entry_latch(1);
        let entered_hash = sim.state_hash();
        assert_ne!(
            entered_hash, clear_hash,
            "inherited6B3 category {category:?}"
        );
        let bytes = GameSnapshot::save(&sim, 0, 0, "foot-idle-entry-latch", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        let actor = restored.substrate.entities.get(1).unwrap();
        assert_eq!(actor.category(), category);
        assert_eq!(actor.mission_leaf.foot_idle_entry_latch(), 1);
        assert_eq!(restored.state_hash(), entered_hash);
        let rng = restored.rng_state();
        assert!(!restored.foot_enter_idle_base(
            1,
            None,
            None,
            crate::sim::world::FrameEffects::default()
        ));
        assert_eq!(restored.rng_state(), rng);
        assert_eq!(restored.state_hash(), entered_hash);
        restored
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .mission_leaf
            .set_foot_idle_entry_latch(0);
        assert_eq!(
            restored.state_hash(),
            clear_hash,
            "only inherited6B3 changed"
        );
    }
}

/// `0x0073AB6C..0x0073ABD0`: a harvester its war factory clears on the
/// factory's own foundation, with no NavCom and nothing listed or queued,
/// scatters. An order given during the exit (listed), or a NavQueue entry,
/// skips it.
#[test]
fn per_cell_factory_clearance_scatters_a_harvester_off_the_factory() {
    use crate::sim::components::NavTargetRef;

    for case in ["idle", "listed", "nav_queue"] {
        let (mut sim, rules) = factory_per_cell_fixture(true, false, "Harvester=yes");
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let producer = sim.substrate.entities.get_mut(2).unwrap();
        producer.type_facts.set_foundation_for_test("5x3".into());
        producer.position.put_location(DriveCoord::cell(6, 9, 0));
        sim.substrate.occupancy =
            crate::sim::occupancy::OccupancyGrid::rebuild(&sim.substrate.entities);
        let order = NavTargetRef::cell(14, 14);
        let unit = sim.substrate.entities.get_mut(1).unwrap();
        unit.navigation.set_nav_com_for_test(None);
        if case == "listed" {
            unit.navigation.set_nav_list_for_test(vec![order], false);
        }
        if case == "nav_queue" {
            unit.navigation.push_nav_queue_for_test(order);
        }
        sim.per_cell_process(
            1,
            super::super::PerCellReason::Arrival,
            Some(&rules),
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
        assert_eq!(
            crate::sim::radio::take_transmit_log()
                .first()
                .map(|send| (send.msg, send.reply)),
            Some((8, Some(23))),
            "{case}"
        );
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            unit.navigation.nav_com().is_some(),
            case == "idle",
            "{case}"
        );
        assert_eq!(
            unit.navigation.nav_list().len(),
            usize::from(case == "listed")
        );
        assert_eq!(
            unit.navigation.nav_queue().len(),
            usize::from(case == "nav_queue")
        );
    }
}

/// A retail world on flat ground with an Americans House at `iq`, human or
/// not, and a `type_name` Unit at (10, 10) on Move with nothing queued.
fn retail_unit_on_move(
    type_name: &str,
    human: bool,
    iq: i32,
) -> Option<(Simulation, RuleSet, u64)> {
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};

    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()?;
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let owner = sim.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, human, 0, 10);
    house.current_iq = iq;
    sim.houses.register(house);
    let id = sim
        .spawn_object(type_name, "Americans", 10, 10, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mission
        .apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Move),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
    Some((sim, rules, id))
}

/// `UnitClass::Enter_Idle_Mode`'s armed guard pick (`0x00738B67..
/// 0x00738C98`) through the production receiver on retail rules, after a Move
/// with no NavCom: a House IQ at `[IQ] GuardArea=`, a human's too, or a
/// `DefaultToGuardArea=` type takes Area Guard outside a team, a slave link
/// either way and a tank bunker; otherwise Guard. No retail type lists the
/// GUARD_AREA ability.
#[test]
#[ignore = "slow; run when changing what it covers"]
fn the_unit_guard_pick_follows_house_iq_team_and_type() {
    use crate::sim::mission::MissionType;

    let Some((_, rules, _)) = retail_unit_on_move("MTNK", true, 0) else {
        return;
    };
    assert!(rules.object("DRON").unwrap().default_to_guard_area);
    assert!(!rules.object("MTNK").unwrap().default_to_guard_area);
    let low = rules.general.iq_guard_area - 1;
    let skirmish = rules.general.max_iq_levels;
    let (guard, area_guard) = (Some(MissionType::Guard), Some(MissionType::AreaGuard));
    for (type_name, human, iq, link, expected) in [
        ("MTNK", true, 0, "", guard),
        ("MTNK", true, skirmish, "", area_guard),
        ("MTNK", false, low, "", guard),
        ("MTNK", false, skirmish, "", area_guard),
        ("MTNK", false, skirmish, "team", guard),
        ("MTNK", false, skirmish, "slave", guard),
        ("MTNK", false, skirmish, "slave_manager", guard),
        ("MTNK", false, skirmish, "bunker", guard),
        ("DRON", true, 0, "", area_guard),
        ("DRON", true, 0, "team", guard),
    ] {
        let (mut sim, rules, id) = retail_unit_on_move(type_name, human, iq).unwrap();
        let slave_type = sim.intern("SLAV");
        let unit = sim.substrate.entities.get_mut(id).unwrap();
        let object = rules.object(type_name).unwrap();
        assert!(crate::sim::combat::combat_weapon::is_armed(unit, object));
        match link {
            "slave" => {
                unit.slave = crate::sim::slave_manager::SlaveLink::for_test(Some(id + 1), vec![]);
            }
            "slave_manager" => {
                unit.slave_manager = Some(crate::sim::slave_manager::SlaveManager::new(
                    slave_type,
                    [],
                    0,
                    0,
                    0,
                ));
            }
            "bunker" => unit.bunker.set_bunker_link_for_test(
                crate::sim::docking::bunker_link::BunkerLink::Installed(id + 1),
            ),
            "team" => {
                crate::sim::team_script_vm::join_team_for_test(&mut sim, &[id], false, false);
            }
            _ => {}
        }
        sim.unit_enter_idle_mode(
            id,
            Some(&rules),
            false,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .mission
                .queued()
                .known(),
            expected,
            "{type_name}: human {human}, IQ {iq}, link {link:?}"
        );
    }
}

/// `0x007389C4..0x00738A27`: a tank on Move with no NavCom, standing exactly
/// on its Cell's centre at height 0, sends its radio contact OVER_OUT. Off the
/// centre, above height 0, with a NavCom or on Guard, it keeps the link.
#[test]
fn a_unit_at_rest_on_a_cell_centre_breaks_its_radio_link() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};

    for case in ["centre", "off_centre", "raised", "nav_com", "guard"] {
        let Some((mut sim, rules, id)) = retail_unit_on_move("MTNK", true, 0) else {
            return;
        };
        let factory = sim
            .spawn_object("GAWEAP", "Americans", 20, 20, 0, &rules)
            .unwrap();
        let unit = sim.substrate.entities.get_mut(id).unwrap();
        unit.radio_contacts.insert(factory);
        if case == "off_centre" || case == "raised" {
            unit.position.put_location(DriveCoord {
                x: 10 * 256 + if case == "raised" { 128 } else { 129 },
                y: 10 * 256 + 128,
                z: if case == "raised" { 104 } else { 0 },
            });
        }
        if case == "guard" {
            unit.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_known(MissionType::Guard),
                suspended: MissionId::NONE,
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::at_frame(0),
            });
        }
        if case == "nav_com" {
            unit.navigation
                .set_nav_com_for_test(Some(NavTargetRef::cell(14, 14)));
        }
        crate::sim::radio::take_transmit_log();
        sim.unit_enter_idle_mode(
            id,
            Some(&rules),
            false,
            crate::sim::world::FrameEffects::default(),
        );
        let over_out = crate::sim::radio::take_transmit_log()
            .iter()
            .any(|send| send.sender_sid == id && send.target_sid == factory && send.msg == 3);
        assert_eq!(over_out, case == "centre", "{case}");
    }
}

/// `0x00738A7C..0x00738AAF`: an unarmed mission-only transport (`+0x3D4`)
/// carrying a passenger, outside a team, selects Unload after a Move and
/// skips the unarmed setters. Without a passenger, without the flag or in a
/// team it takes the unarmed Guard.
#[test]
fn a_mission_only_transport_with_passengers_selects_unload() {
    use crate::sim::mission::MissionType;

    for case in ["loaded", "empty", "commanded", "teamed"] {
        let Some((mut sim, rules, id)) = retail_unit_on_move("SAPC", true, 0) else {
            return;
        };
        let passenger = sim
            .spawn_object("E1", "Americans", 12, 12, 0, &rules)
            .unwrap();
        let unit = sim.substrate.entities.get_mut(id).unwrap();
        assert!(!crate::sim::combat::combat_weapon::is_armed(
            unit,
            rules.object("SAPC").unwrap()
        ));
        if case != "commanded" {
            unit.mission_only.mark();
        }
        if case != "empty" {
            unit.passenger_role
                .cargo_mut()
                .unwrap()
                .board_forced_chain(&[(passenger, 1.0)]);
        }
        if case == "teamed" {
            crate::sim::team_script_vm::join_team_for_test(&mut sim, &[id], false, false);
        }
        sim.unit_enter_idle_mode(
            id,
            Some(&rules),
            false,
            crate::sim::world::FrameEffects::default(),
        );
        let expected = if case == "loaded" {
            MissionType::Unload
        } else {
            MissionType::Guard
        };
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .mission
                .queued()
                .known(),
            Some(expected),
            "{case}"
        );
    }
}

/// `0x00738C9D..0x00738CF8`: a tank on Enter whose radio contact 0 is a tank
/// bunker (NATBNK) still ahead of it queues nothing. Standing on the bunker's
/// Cell, it queues its pick.
#[test]
fn a_unit_entering_a_bunker_ahead_of_it_queues_nothing() {
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};

    for on_bunker in [false, true] {
        let Some((mut sim, rules, id)) = retail_unit_on_move("MTNK", true, 0) else {
            return;
        };
        assert!(rules.object("NATBNK").unwrap().bunker);
        let bunker = sim
            .spawn_object("NATBNK", "Americans", 14, 9, 0, &rules)
            .unwrap();
        assert_eq!(
            sim.substrate
                .occupancy
                .first_building_on_layer(14, 9, MovementLayer::Ground),
            Some(bunker)
        );
        let unit = sim.substrate.entities.get_mut(id).unwrap();
        if on_bunker {
            unit.position.put_location(DriveCoord::cell(14, 9, 0));
        }
        unit.radio_contacts.insert(bunker);
        unit.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Enter),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        sim.unit_enter_idle_mode(
            id,
            Some(&rules),
            false,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .mission
                .queued()
                .known(),
            on_bunker.then_some(MissionType::Guard),
            "on the bunker {on_bunker}"
        );
    }
}

/// Original Drive4B1999..4B19FA stores BD4CCCCD after a crossing paid
/// point's Mark(DOWN), before SetHeight or any terminal PerCell. The existing
/// paid-loop fixture supplies admission/budget; this is not a natural route.
#[test]
fn paid_drive_crossing_sets_wall_tilt_after_mark_put_before_per_cell() {
    use crate::rules::ini_parser::IniFile;
    use crate::sim::overlay_grid::OverlayGrid;
    use crate::util::native_x87::NativeF32Bits;

    for (straight, crusher, tilts, wall, overlay, remove_at_mark, expected) in [
        (true, true, true, true, true, false, true),
        (false, true, true, true, true, false, false),
        (true, false, true, true, true, false, false),
        (true, true, false, true, true, false, false),
        (true, true, true, false, true, false, false),
        (true, true, true, true, false, false, false),
        (true, true, true, true, true, true, false),
    ] {
        let (mut sim, invocation, budget) = fixture(TrackFamily::Drive, 8);
        let ini = IniFile::from_str(&format!(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nCrusher={}\nTiltsWhenCrushes={}\n\
             [OverlayTypes]\n0=FENCE\n[FENCE]\nWall={}\nCrushable=yes\nStrength=100\n",
            if crusher { "yes" } else { "no" },
            if tilts { "yes" } else { "no" },
            if wall { "yes" } else { "no" },
        ));
        let rules = RuleSet::from_ini(&ini).unwrap();
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        sim.install_resolved_terrain_for_new_map(
            crate::map::resolved_terrain::test_flat_ground_grid(33),
        );
        let mut grid = OverlayGrid::new(33, 33);
        // A uniform synthetic overlay input keeps the test about the existing
        // paid receiver, independently of which native raw point first crosses.
        if overlay {
            for y in 0..33 {
                for x in 0..33 {
                    grid.place_overlay(x, y, 0, 0);
                }
            }
        }
        sim.overlay_grid = Some(grid);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        let loco = entity.locomotor.as_mut().unwrap();
        assert!(loco.store_drive_straight(straight));
        // As in the existing crossing test, the first real paid point crosses.
        assert!(loco.store_track_head(TrackFamily::Drive, Some(DriveCoord::cell(10, 7, 0)),));
        let mut rocking = crate::sim::rocking::RockingState::default();
        rocking.set_angle_sideways(SimFixed::lit("0.125"));
        rocking.set_angle_forwards_for_test(SimFixed::lit("0.25"));
        rocking.set_vel_sideways_for_test(SimFixed::lit("0.125"));
        rocking.set_vel_forwards_native(NativeF32Bits::from_bits(0x3e80_0000));
        let before = rocking.native_bits();
        entity.rocking = Some(rocking);
        let mut marked = false;
        let mut per_cell = 0;
        let moved = sim.run_track_points_observed(
            invocation,
            budget,
            Some(&rules),
            Some(&registry),
            &mut |sim, id, event| {
                if event == TrackWorldEvent::MarkPut {
                    marked = true;
                    assert_eq!(
                        sim.substrate
                            .entities
                            .get(id)
                            .unwrap()
                            .rocking
                            .as_ref()
                            .unwrap()
                            .native_bits(),
                        before,
                        "pre-entry tilt runs only after Mark(DOWN) returns"
                    );
                    if remove_at_mark {
                        let entity = sim.substrate.entities.get(id).unwrap();
                        sim.overlay_grid
                            .as_mut()
                            .unwrap()
                            .clear_overlay(entity.position.rx(), entity.position.ry());
                    }
                }
                per_cell += usize::from(event == TrackWorldEvent::PerCell);
            },
        );
        assert_eq!(moved, 1);
        assert!(marked);
        assert_eq!(per_cell, 0, "physical crossing is not terminal PerCell");
        let actual = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .rocking
            .as_ref()
            .unwrap()
            .native_bits();
        assert_eq!(&actual[..3], &before[..3]);
        assert_eq!(actual[3], if expected { 0xbd4c_cccd } else { before[3] });
    }
}

fn drive_crush_fixture(
    input: &serde_json::Value,
) -> (Simulation, TrackInvocation, RuleSet, OverlayTypeRegistry) {
    use crate::rules::ini_parser::IniFile;
    use crate::sim::overlay_grid::OverlayGrid;
    use crate::util::fixed_math::SIM_ONE;
    use crate::util::native_x87::NativeF32Bits;

    let (mut sim, invocation, _) = fixture(TrackFamily::Drive, 8);
    let yes = |key: &str| {
        if input[key].as_bool().unwrap() {
            "yes"
        } else {
            "no"
        }
    };
    let zone = match input["zone"].as_i64().unwrap() {
        0 => "Normal",
        11 => "WaterBeach",
        12 => "CrusherAll",
        zone => panic!("unexpected native zone {zone}"),
    };
    let ini = IniFile::from_str(&format!(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\nSpeed=6\n\
         Crusher={}\nMovementZone={zone}\nAccelerates={}\nTiltsWhenCrushes={}\n\
         [OverlayTypes]\n0=FENCE\n[FENCE]\nWall={}\nCrushable={}\nStrength=100\n",
        yes("crusher"),
        yes("accelerates"),
        yes("tilts"),
        yes("wall"),
        yes("crushable"),
    ));
    let rules = RuleSet::from_ini(&ini).unwrap();
    let registry = OverlayTypeRegistry::from_ini(&ini, None);
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_flat_ground_grid(
        33,
    ));
    let mut overlays = OverlayGrid::new(33, 33);
    if input["overlay"].as_bool().unwrap() {
        for y in 0..33 {
            for x in 0..33 {
                overlays.place_overlay(x, y, 0, 0);
            }
        }
    }
    sim.overlay_grid = Some(overlays);
    let actor = sim.substrate.entities.get_mut(1).unwrap();
    actor.health = crate::sim::combat::Health::new(300);
    actor
        .mover_facts
        .set_drive_accelerates_for_test(input["accelerates"].as_bool().unwrap());
    actor.foot_speed.set_speed_fraction(SIM_ONE);
    actor
        .foot_speed
        .set_crush_slowdown(input["initial_flag"].as_bool().unwrap());
    let mut rocking = crate::sim::rocking::RockingState::default();
    rocking.set_angles_native_for_test([0.0, f32::from_bits(0x3e99_999a)]);
    rocking.set_vel_forwards_native(NativeF32Bits::from_bits(0x3d23_d70a));
    actor.rocking = Some(rocking);
    let loco = actor.locomotor.as_mut().unwrap();
    loco.store_drive_straight(input["straight"].as_bool().unwrap());
    loco.store_track_target_fraction(TrackFamily::Drive, SIM_ONE);
    loco.store_track_destination(TrackFamily::Drive, Some(DriveCoord::cell(20, 10, 0)));
    loco.store_track_head(TrackFamily::Drive, Some(DriveCoord::cell(10, 7, 0)));
    (sim, invocation, rules, registry)
}

fn assert_crush_history_state(actor: &GameEntity, expected: &serde_json::Value) {
    let word =
        |field: &serde_json::Value| u32::from_str_radix(field.as_str().unwrap(), 16).unwrap();
    let fraction = |key: &str| {
        f64::from_bits(u64::from_str_radix(expected[key].as_str().unwrap(), 16).unwrap())
    };
    assert_eq!(
        actor.foot_speed.crush_slowdown(),
        expected["crush_flag"] == 1
    );
    assert_eq!(
        actor.rocking.unwrap().native_bits(),
        [
            word(&expected["rocking_bits"][0]),
            word(&expected["rocking_bits"][1]),
            word(&expected["rocking_bits"][2]),
            word(&expected["rocking_bits"][3]),
        ]
    );
    // The existing speed owner uses I16F16 instead of native binary64.
    assert!(
        (actor.foot_speed.applied_fraction().to_num::<f64>() - fraction("applied_bits")).abs()
            < 0.0001
    );
    assert!(
        (actor
            .locomotor
            .as_ref()
            .unwrap()
            .track_target_fraction(TrackFamily::Drive)
            .unwrap()
            .to_num::<f64>()
            - fraction("target_bits"))
        .abs()
            < 0.0001
    );
}

/// Original component histories plus production paid-point/Mark and full Unit
/// PerCell integration. Inputs supply admission, budget and uniform overlays;
/// this does not certify whole native path admission or rendering.
#[test]
fn crusher_all_paid_crossing_caps_the_next_track_speed_prefix() {
    use crate::sim::occupancy::CellListInsertion;
    use crate::util::fixed_math::SIM_ONE;

    let rows =
        crate::test_fixture::json("tools/spatial_oracle/unit_per_cell_overlay.drive_crush.json");
    let mut checked = 0;
    for row in rows.as_array().unwrap() {
        let input = &row["input"];
        // Drive Process is scheduled only in active gameplay. The additional
        // native GameActive=false control documents the query's loader gate.
        if !input["game_active"].as_bool().unwrap() {
            continue;
        }
        let (mut sim, invocation, rules, registry) = drive_crush_fixture(input);
        let occupant = input["occupant"].as_str().unwrap();
        let mut marked = false;
        let moved = sim.run_track_points_observed(
            invocation,
            8,
            Some(&rules),
            Some(&registry),
            &mut |sim, id, event| {
                if event != TrackWorldEvent::MarkPut {
                    return;
                }
                marked = true;
                let actor = sim.substrate.entities.get(id).unwrap();
                assert_eq!(
                    actor.foot_speed.crush_slowdown(),
                    input["initial_flag"].as_bool().unwrap()
                );
                let at = (actor.position.rx(), actor.position.ry());
                // These three controls supply the same post-Mark selected
                // lists as the native boundary, without changing the query.
                if occupant != "self" {
                    sim.substrate.occupancy.remove(at.0, at.1, id);
                    match occupant {
                        "none" => {}
                        "deck" => sim.substrate.occupancy.add(
                            at.0,
                            at.1,
                            id,
                            MovementLayer::Bridge,
                            None,
                            CellListInsertion::PrependNonBuilding,
                        ),
                        "infantry" => {
                            sim.substrate
                                .entities
                                .insert(GameEntity::test_default_of_category(
                                    2,
                                    "E1",
                                    "Americans",
                                    at.0,
                                    at.1,
                                    EntityCategory::Infantry,
                                ));
                            sim.substrate.occupancy.add(
                                at.0,
                                at.1,
                                2,
                                MovementLayer::Ground,
                                None,
                                CellListInsertion::PrependNonBuilding,
                            );
                        }
                        other => panic!("unexpected occupant {other}"),
                    }
                }
            },
        );
        assert_eq!(moved, 1, "{input}");
        assert!(marked);
        assert_crush_history_state(sim.substrate.entities.get(1).unwrap(), &row["pre_entry"]);
        let advance_speed = |sim: &mut Simulation| {
            super::super::track_speed::advance(
                sim.substrate.entities.get_mut(1).unwrap(),
                rules.object("MTNK"),
                Some(&rules),
                &sim.houses,
                sim.resolved_terrain.as_ref(),
                sim.path_grid.as_deref(),
            );
        };
        advance_speed(&mut sim);
        assert_crush_history_state(sim.substrate.entities.get(1).unwrap(), &row["speed"]);
        assert!(!crate::sim::rocking::rocking_system::update(
            sim.substrate.entities.get_mut(1).unwrap(),
            rules.object("MTNK").unwrap(),
            SIM_ONE,
            0,
        ));
        assert_crush_history_state(sim.substrate.entities.get(1).unwrap(), &row["rocking"]);
        let at = {
            let actor = sim.substrate.entities.get(1).unwrap();
            (actor.position.rx(), actor.position.ry())
        };
        sim.per_cell_process(
            1,
            super::super::PerCellReason::TurnComplete,
            Some(&rules),
            Some(&registry),
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
        assert_crush_history_state(sim.substrate.entities.get(1).unwrap(), &row["per_cell"]);
        assert_eq!(
            sim.overlay_grid
                .as_ref()
                .unwrap()
                .cell(at.0, at.1)
                .overlay_id
                .is_none(),
            !input["overlay"].as_bool().unwrap()
                || (input["crusher"].as_bool().unwrap() && input["wall"].as_bool().unwrap())
        );
        advance_speed(&mut sim);
        assert_crush_history_state(
            sim.substrate.entities.get(1).unwrap(),
            &row["retained_speed"],
        );
        super::super::track_speed::publish_fresh_track_target(
            sim.substrate.entities.get_mut(1).unwrap(),
            300,
            &rules,
            sim.resolved_terrain.as_ref().unwrap(),
            &crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            (20, 10),
            false,
        );
        advance_speed(&mut sim);
        assert_crush_history_state(
            sim.substrate.entities.get(1).unwrap(),
            &row["refreshed_speed"],
        );
        checked += 1;
    }
    assert_eq!(checked, 13);
}

#[test]
fn crush_slowdown_is_foot_owned_saved_and_hashed() {
    use crate::sim::snapshot::GameSnapshot;
    let mut sim = Simulation::new();
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    sim.interner = crate::sim::intern::test_interner();
    sim.substrate
        .entities
        .insert(GameEntity::test_default(1, "BFRT", "Americans", 10, 10));
    let clear = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .foot_speed
        .set_crush_slowdown(true);
    assert_ne!(sim.state_hash(), clear);
    let actor = sim.substrate.entities.get_mut(1).unwrap();
    actor.locomotor = Some(super::super::locomotor::LocomotorState::for_test_kind(
        crate::rules::locomotor_type::LocomotorKind::Ship,
    ));
    assert!(
        actor.foot_speed.crush_slowdown(),
        "a locomotor replacement does not own Foot+6B5"
    );
    let hash = sim.state_hash();
    let bytes = GameSnapshot::save(&sim, 0, 0, "crush-slowdown", 0);
    let restored = GameSnapshot::load(&bytes).unwrap().sim;
    assert_eq!(restored.state_hash(), hash);
    assert!(
        restored
            .substrate
            .entities
            .get(1)
            .unwrap()
            .foot_speed
            .crush_slowdown()
    );
}

#[test]
fn retail_battle_fortress_preserves_nonaccelerating_speed_during_crush() {
    use crate::util::fixed_math::SIM_ONE;
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    let object = rules.object("BFRT").unwrap();
    assert_eq!(
        object.movement_zone,
        crate::rules::locomotor_type::MovementZone::CrusherAll
    );
    assert!(object.crusher);
    assert!(!object.accelerates);
    let rows =
        crate::test_fixture::json("tools/spatial_oracle/unit_per_cell_overlay.drive_crush.json");
    let (mut sim, invocation, _, registry) = drive_crush_fixture(&rows[0]["input"]);
    let type_ref = sim.intern("BFRT");
    let actor = sim.substrate.entities.get_mut(1).unwrap();
    actor.set_type_ref_for_test(type_ref);
    actor.health = crate::sim::combat::Health::new(object.strength);
    actor
        .mover_facts
        .set_drive_accelerates_for_test(object.accelerates);
    // The paid fixture supplies only track admission/budget. Type gates and
    // speed parameters below are read through the stock production reader.
    assert_eq!(
        sim.run_track_points(invocation, 8, Some(&rules), Some(&registry)),
        1
    );
    let actor = sim.substrate.entities.get_mut(1).unwrap();
    assert!(actor.foot_speed.crush_slowdown());
    assert_eq!(
        actor.foot_speed.applied_fraction(),
        SIM_ONE,
        "the current call paid its budget before crossing"
    );
    super::super::track_speed::advance(
        actor,
        Some(object),
        Some(&rules),
        &sim.houses,
        sim.resolved_terrain.as_ref(),
        sim.path_grid.as_deref(),
    );
    assert_eq!(actor.foot_speed.applied_fraction(), SIM_ONE);
}
