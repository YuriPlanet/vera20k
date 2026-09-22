use super::*;
use crate::sim::components::{DriveLocomotionRuntime, ShipLocomotionRuntime};

fn fixture(family: TrackFamily, budget: i32) -> (Simulation, TrackInvocation, i32) {
    let mut sim = Simulation::new();
    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
    entity.category = EntityCategory::Unit;
    entity.locomotor = Some(super::super::locomotor::LocomotorState::for_test_kind(
        if family == TrackFamily::Drive {
            crate::rules::locomotor_type::LocomotorKind::Drive
        } else {
            crate::rules::locomotor_type::LocomotorKind::Ship
        },
    ));
    entity.lifecycle.object_alive = true;
    entity.lifecycle.in_limbo = false;
    entity.lifecycle.cell_marked = true;
    let head = DriveCoord {
        x: 10 * 256 + 128,
        y: 9 * 256 + 128,
        z: 0,
    };
    let track = TrackProgress {
        turn_index: 0,
        cursor: 0,
        reversed: false,
        residual: 0,
    };
    match family {
        TrackFamily::Drive => {
            entity.drive_locomotion = Some(DriveLocomotionRuntime {
                head_to: Some(head),
                track,
                track_valid: true,
                ..Default::default()
            })
        }
        TrackFamily::Ship => {
            entity.ship_locomotion = Some(ShipLocomotionRuntime {
                head_to: Some(head),
                track,
                track_valid: true,
                ..Default::default()
            })
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
            apply_fresh_occupation: false,
        },
        budget,
    )
}

fn valid(entity: &GameEntity, family: TrackFamily) -> bool {
    match family {
        TrackFamily::Drive => entity.drive_locomotion.as_ref().unwrap().track_valid,
        TrackFamily::Ship => entity.ship_locomotion.as_ref().unwrap().track_valid,
    }
}

#[test]
fn terminal_horizontal_mismatch_does_not_query_missing_owner_locomotor() {
    use crate::sim::components::NavTargetRef;
    let (mut sim, _, _) = fixture(TrackFamily::Drive, 0);
    let e = sim.substrate.entities.get_mut(1).unwrap();
    e.navigation.nav_com = Some(NavTargetRef::cell(20, 20));
    e.drive_locomotion.as_mut().unwrap().destination = Some(DriveCoord::cell(20, 20, 0));
    e.locomotor = None;
    assert!(
        !sim.track_reached_destination(1, TrackFamily::Drive)
            .unwrap()
    );
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .navigation
        .nav_com = Some(NavTargetRef::cell(10, 10));
    assert!(
        sim.track_reached_destination(1, TrackFamily::Drive)
            .is_err()
    );
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
        mover.navigation.nav_com = Some(NavTargetRef::Entity { id: 2 });
        match family {
            TrackFamily::Drive => {
                mover.drive_locomotion.as_mut().unwrap().destination = Some(destination)
            }
            TrackFamily::Ship => {
                mover.ship_locomotion.as_mut().unwrap().destination = Some(destination)
            }
        }
        progress_mut(mover, family).unwrap().cursor =
            super::super::drive_track::raw_track_points(1).len() as i32;
        let mut target = GameEntity::test_default(2, "E1", "Americans", 20, 20);
        target.category = EntityCategory::Infantry;
        target.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
        target
            .locomotor
            .as_mut()
            .unwrap()
            .set_step_head(Some(destination));
        sim.substrate.entities.insert(target);
        let mut per_cell = 0;
        sim.run_track_points_observed(
            invocation,
            budget,
            None,
            None,
            None,
            &mut |sim, id, event| {
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
            },
        );
        assert_eq!(per_cell, 1);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().navigation.nav_com,
            None
        );
    }
}

#[test]
fn production_host_keeps_budget_and_live_cursor_across_world_coordinate_effect() {
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        let (mut sim, invocation, budget) = fixture(family, 22);
        let mut callbacks = 0;
        let moved = sim.run_track_points_observed(
            invocation,
            budget,
            None,
            None,
            None,
            &mut |sim, id, event| {
                if event == TrackWorldEvent::SetCoords {
                    callbacks += 1;
                    if callbacks == 1 {
                        progress_mut(sim.substrate.entities.get_mut(id).unwrap(), family)
                            .unwrap()
                            .cursor = 2;
                    }
                }
            },
        );
        let state = progress(sim.substrate.entities.get(1).unwrap(), family).unwrap();
        assert_eq!(moved, 3);
        assert_eq!(
            state.cursor, 5,
            "finish the same paid point with the live cursor"
        );
        assert_eq!(state.residual, 1);
        assert!(callbacks >= 3);
    }
}

#[test]
fn production_host_lifecycle_exit_does_not_store_call_local_residual() {
    let (mut sim, invocation, budget) = fixture(TrackFamily::Drive, 22);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .drive_locomotion
        .as_mut()
        .unwrap()
        .track
        .residual = 4;
    sim.run_track_points_observed(
        invocation,
        budget,
        None,
        None,
        None,
        &mut |sim, id, event| {
            if event == TrackWorldEvent::SetCoords {
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .lifecycle
                    .object_alive = false;
            }
        },
    );
    let state = progress(sim.substrate.entities.get(1).unwrap(), TrackFamily::Drive).unwrap();
    assert_eq!(state.residual, 4);
    assert_eq!(state.cursor, 0);
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
    let mut cargo = PassengerCargo::new(2, 1);
    assert!(cargo.board(2, 1));
    sim.substrate.entities.get_mut(1).unwrap().passenger_role = PassengerRole::Transport { cargo };
    let passenger = GameEntity::test_default(2, "E1", "Americans", 4, 4);
    sim.substrate.entities.insert(passenger);
    // Force a cell crossing on the first raw point; this test observes the
    // actual world receiver, including Foot4DB810 -> OpenTopped7104F0.
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .drive_locomotion
        .as_mut()
        .unwrap()
        .head_to = Some(DriveCoord::cell(10, 7, 0));
    let mut events = Vec::new();
    sim.run_track_points_observed(
        invocation,
        budget,
        Some(&rules),
        None,
        None,
        &mut |sim, id, event| {
            if matches!(
                event,
                TrackWorldEvent::MarkRemove | TrackWorldEvent::SetCoords | TrackWorldEvent::MarkPut
            ) {
                let entity = sim.substrate.entities.get(id).unwrap();
                events.push((event, entity.lifecycle.cell_marked));
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
            .cursor,
        1
    );
}

#[test]
fn nonterminal_limbo_and_falling_do_not_add_an_early_survival_gate() {
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        let (mut sim, invocation, budget) = fixture(family, 15);
        sim.run_track_points_observed(
            invocation,
            budget,
            None,
            None,
            None,
            &mut |sim, id, event| {
                if event == TrackWorldEvent::SetCoords {
                    let entity = sim.substrate.entities.get_mut(id).unwrap();
                    entity.lifecycle.in_limbo = true;
                    entity.object_is_falling_down = 1;
                }
            },
        );
        let state = progress(sim.substrate.entities.get(1).unwrap(), family).unwrap();
        assert_eq!(
            state.cursor, 2,
            "4B1A77 tests alive only before common cursor increment"
        );
        assert_eq!(state.residual, 1);
    }
}

#[test]
fn terminal_per_cell_limbo_keeps_retained_residual_and_navcom() {
    use crate::sim::components::NavTargetRef;
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        let (mut sim, invocation, budget) = fixture(family, 22);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        let target = head(entity, family);
        entity.navigation.nav_com = Some(NavTargetRef::cell(10, 9));
        match family {
            TrackFamily::Drive => {
                entity.drive_locomotion.as_mut().unwrap().destination = Some(target)
            }
            TrackFamily::Ship => {
                entity.ship_locomotion.as_mut().unwrap().destination = Some(target)
            }
        }
        let state = progress_mut(entity, family).unwrap();
        state.cursor = super::super::drive_track::raw_track_points(1).len() as i32;
        state.residual = 4;
        let mut calls = 0;
        sim.run_track_points_observed(
            invocation,
            budget,
            None,
            None,
            None,
            &mut |sim, id, event| {
                if event == TrackWorldEvent::PerCell {
                    calls += 1;
                    let entity = sim.substrate.entities.get_mut(id).unwrap();
                    assert!(entity.navigation.nav_com.is_some());
                    assert_eq!(head(entity, family), DriveCoord { x: 0, y: 0, z: 0 });
                    assert!(!valid(entity, family));
                    entity.lifecycle.in_limbo = true;
                }
            },
        );
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(calls, 1);
        assert_eq!(progress(entity, family).unwrap().residual, 4);
        assert!(
            entity.navigation.nav_com.is_some(),
            "limbo returns before saved-reached FootStop"
        );
    }
}

#[test]
fn terminal_saved_reached_clears_callback_navcom_and_only_live_queue_head() {
    use crate::sim::components::{FootPathQueue, NavTargetRef};
    let (mut sim, invocation, budget) = fixture(TrackFamily::Drive, 8);
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.navigation.nav_com = Some(NavTargetRef::cell(10, 9));
    let drive = entity.drive_locomotion.as_mut().unwrap();
    drive.destination = drive.head_to;
    drive.track.cursor = super::super::drive_track::raw_track_points(1).len() as i32;
    sim.run_track_points_observed(
        invocation,
        budget,
        None,
        None,
        None,
        &mut |sim, id, event| {
            if event == TrackWorldEvent::PerCell {
                let entity = sim.substrate.entities.get_mut(id).unwrap();
                assert_eq!(entity.navigation.nav_com, Some(NavTargetRef::cell(10, 9)));
                assert!(
                    entity
                        .drive_locomotion
                        .as_ref()
                        .unwrap()
                        .destination
                        .is_none()
                );
                entity.navigation.nav_com = Some(NavTargetRef::cell(25, 25));
                entity.navigation.path_replay = FootPathQueue {
                    directions: vec![1, 2, 3],
                    cursor: 1,
                    reference_cell: Some((7, 8)),
                };
            }
        },
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(entity.navigation.nav_com.is_none());
    assert_eq!(entity.navigation.path_replay.directions, vec![1, 255, 3]);
    assert_eq!(entity.navigation.path_replay.cursor, 1);
    assert_eq!(entity.navigation.path_replay.reference_cell, Some((7, 8)));
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
                            cell.is_water = true;
                            cell.ground_walk_blocked = true;
                            cell.land_type =
                                crate::rules::terrain_rules::LandType::Water.as_index();
                            cell.yr_cell_land_type = cell.land_type;
                        }
                        cell
                    })
                })
                .collect(),
        );
        sim.path_grid = Some(std::sync::Arc::new(PathGrid::from_resolved_terrain(
            &terrain,
        )));
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
            loco.movement_zone = crate::rules::locomotor_type::MovementZone::Water;
            loco.speed_type = crate::rules::locomotor_type::SpeedType::Float;
        }
        *progress_mut(entity, family).unwrap() = TrackProgress {
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
        let saved_fraction = entity.foot_speed.applied_fraction;
        let mut callbacks = 0;
        sim.run_track_points_observed(
            invocation,
            budget,
            Some(&rules),
            None,
            None,
            &mut |sim, id, event| {
                if event == TrackWorldEvent::PerCell {
                    callbacks += 1;
                    let entity = sim.substrate.entities.get_mut(id).unwrap();
                    let state = progress_mut(entity, family).unwrap();
                    assert_eq!(state.turn_index, selected.turn_track_index as i32);
                    assert_eq!(state.cursor, i32::from(selected.entry_index) - 1);
                    state.cursor += 1;
                    assert_eq!(head(entity, family), DriveCoord { x: 0, y: 0, z: 0 });
                    assert!(valid(entity, family));
                    set_head(entity, family, Some(DriveCoord::cell(1, 1, 0)));
                    entity.navigation.path_replay = FootPathQueue {
                        directions: vec![7, 3, 4],
                        cursor: 0,
                        reference_cell: Some((6, 7)),
                    };
                    entity.foot_speed.applied_fraction = SimFixed::from_num(0.25);
                    if retire_in_callback {
                        entity.lifecycle.in_limbo = true;
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
            assert_eq!(entity.navigation.path_replay.cursor, 0);
            assert_eq!(
                entity.navigation.path_replay.remaining_directions(),
                &[7, 3, 4]
            );
            assert_eq!(entity.foot_speed.applied_fraction, SimFixed::from_num(0.25));
            continue;
        }
        assert_eq!(head(entity, family), DriveCoord::cell(11, 9, 0));
        assert!(valid(entity, family));
        assert!(progress(entity, family).unwrap().cursor > i32::from(selected.entry_index));
        assert_eq!(progress(entity, family).unwrap().residual, 1);
        assert_eq!(entity.navigation.path_replay.cursor, 1);
        assert_eq!(
            entity.navigation.path_replay.remaining_directions(),
            &[3, 4]
        );
        assert_eq!(entity.foot_speed.applied_fraction, saved_fraction);
    }
}

#[test]
fn apply_one_marks_handoff_and_full_head_without_foot_enable() {
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        let (mut sim, _, _) = fixture(family, 0);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.foot_occupation_enabled = false;
        *progress_mut(entity, family).unwrap() = TrackProgress {
            turn_index: 1,
            cursor: 0,
            reversed: false,
            residual: 0,
        };
        sim.track_apply_occupation(1, family, true, None);
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
                .foot_occupation_enabled
        );
    }
}

#[test]
fn bridge_placement_retains_selected_cell_but_reads_fresh_world_attributes() {
    let (mut sim, _, _) = fixture(TrackFamily::Drive, 0);
    let fallback = PathGrid::new(32, 32);
    sim.track_place(
        1,
        DriveCoord::cell(10, 9, 0),
        (10, 10),
        false,
        None,
        Some(&fallback),
        None,
        &mut |sim, id, event| {
            if event == TrackWorldEvent::SetCoords {
                // Cached source/selected destination; fresh canonical attributes.
                let mut grid = PathGrid::new(32, 32);
                grid.set_cell_for_test(10, 10, 4, false, false);
                grid.set_cell_for_test(10, 9, 0, true, false);
                sim.path_grid = Some(std::sync::Arc::new(grid));
                sim.substrate.entities.get_mut(id).unwrap().position.rx = 20;
            }
        },
    );
    assert!(sim.substrate.entities.get(1).unwrap().on_bridge);
}

#[test]
fn ordinary_foot_limbo_releases_live_head_and_handoff_for_both_families() {
    use crate::sim::lifecycle_request::{LifecycleRequest, UninitReason};
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        let (mut sim, _, _) = fixture(family, 0);
        progress_mut(sim.substrate.entities.get_mut(1).unwrap(), family)
            .unwrap()
            .turn_index = 1;
        sim.track_apply_occupation(1, family, true, None);
        let entity = sim.substrate.entities.get(1).unwrap();
        let (head, handoff) = match family {
            TrackFamily::Drive => {
                let state = entity.drive_locomotion.as_ref().unwrap();
                (
                    state.occupation_head_to.unwrap(),
                    state.occupation_handoff.unwrap(),
                )
            }
            TrackFamily::Ship => {
                let state = entity.ship_locomotion.as_ref().unwrap();
                (
                    state.occupation_head_to.unwrap(),
                    state.occupation_handoff.unwrap(),
                )
            }
        };
        assert_ne!((head.rx, head.ry), (10, 10));
        assert_ne!(head, handoff);
        sim.apply_lifecycle_request(LifecycleRequest::Uninit {
            stable_id: 1,
            reason: UninitReason::Crush,
        });
        assert_eq!(
            sim.substrate
                .raw_cell_occupation
                .ground_bits(head.rx, head.ry)
                & 0x20,
            0
        );
        assert_eq!(
            sim.substrate
                .raw_cell_occupation
                .ground_bits(handoff.rx, handoff.ry)
                & 0x20,
            0
        );
        assert!(
            !sim.substrate
                .cell_occupation
                .occupied_by_other(head.rx, head.ry, head.layer, 99)
        );
    }
}

#[test]
fn raw_clear_retires_aliasing_roles_before_reconciliation() {
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        let (mut sim, _, _) = fixture(family, 0);
        progress_mut(sim.substrate.entities.get_mut(1).unwrap(), family)
            .unwrap()
            .turn_index = 1;
        sim.track_apply_occupation(1, family, true, None);
        let entity = sim.substrate.entities.get(1).unwrap();
        let handoff = match family {
            TrackFamily::Drive => entity
                .drive_locomotion
                .as_ref()
                .unwrap()
                .occupation_handoff
                .unwrap(),
            TrackFamily::Ship => entity
                .ship_locomotion
                .as_ref()
                .unwrap()
                .occupation_handoff
                .unwrap(),
        };
        sim.track_raw_mark_at(1, DriveCoord::cell(handoff.rx, handoff.ry, 0), false, None);
        sim.substrate
            .cell_occupation
            .reconcile_entity(sim.substrate.entities.get(1).unwrap());
        assert!(!sim.substrate.cell_occupation.occupied_by_other(
            handoff.rx,
            handoff.ry,
            handoff.layer,
            99
        ));
    }
}

#[test]
fn repeated_foot_limbo_does_not_clear_a_new_raw_head_claim() {
    use crate::sim::lifecycle_request::{LifecycleRequest, UninitReason};
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        let (mut sim, _, _) = fixture(family, 0);
        progress_mut(sim.substrate.entities.get_mut(1).unwrap(), family)
            .unwrap()
            .turn_index = 1;
        let supplied = head(sim.substrate.entities.get(1).unwrap(), family);
        sim.track_apply_occupation(1, family, true, None);
        sim.techno_limbo(1);
        assert!(sim.substrate.entities.get(1).unwrap().lifecycle.in_limbo);
        let at = cell(supplied);
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(at.0, at.1) & 0x20,
            0
        );
        // Boarding retains the locomotor head. A later carrier teardown calls
        // UnInit/Limbo again; native Foot's early in-limbo gate preserves the
        // raw bit another mover has since written at that old head.
        sim.track_raw_mark_at(99, supplied, true, None);
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

#[test]
fn terminal_retires_only_completed_adapter_before_callback() {
    use crate::sim::components::{MovementTarget, NavTargetRef};
    for retarget in [false, true] {
        let (mut sim, invocation, budget) = fixture(TrackFamily::Drive, 15);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.drive_locomotion.as_mut().unwrap().track.cursor =
            super::super::drive_track::raw_track_points(1).len() as i32;
        entity.movement_target = Some(MovementTarget {
            path: vec![(10, 10), (10, 9)],
            path_layers: vec![MovementLayer::Ground; 2],
            next_index: 1,
            ..Default::default()
        });
        // A stopped committed segment has no NavCom, but its completed path
        // must still retire before callbacks begin a deployment body turn.
        sim.run_track_points_observed(
            invocation,
            budget,
            None,
            None,
            None,
            &mut |sim, id, event| {
                if event == TrackWorldEvent::PerCell {
                    let entity = sim.substrate.entities.get_mut(id).unwrap();
                    assert!(entity.movement_target.is_none());
                    if retarget {
                        entity.navigation.nav_com = Some(NavTargetRef::cell(12, 9));
                        entity.movement_target = Some(MovementTarget {
                            path: vec![(10, 9), (11, 9), (12, 9)],
                            path_layers: vec![MovementLayer::Ground; 3],
                            next_index: 1,
                            ..Default::default()
                        });
                    }
                }
            },
        );
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(entity.movement_target.is_some(), retarget);
        assert!(!entity.navigation.pending_arrival_clear);
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
        sim.session.binary_frame = 57;
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.drive_locomotion.as_mut().unwrap().head_to = None;
        let mut miner = Miner::new(MinerKind::War, &MinerConfig::default(), 0);
        miner.unload_active = unload_active;
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
        sim.unit_track_per_cell(
            1,
            super::super::track_turn::PerCellReason::Arrival,
            Some(&rules),
            None,
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

#[test]
fn idle_receiver_cancels_burst_through_the_shared_target_setter() {
    use crate::sim::combat::{AttackTarget, TargetKind};
    use crate::sim::components::NavTargetRef;
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        for has_destination in [false, true] {
            let (mut sim, _, _) = fixture(family, 0);
            let entity = sim.substrate.entities.get_mut(1).unwrap();
            entity.attack_target = Some(AttackTarget::for_cell(12, 10));
            entity.passively_acquired_target = true;
            entity.weapon_burst.complete_shot(2);
            entity.navigation.nav_com = has_destination.then(|| NavTargetRef::cell(12, 10));
            sim.track_enter_idle_mode(1, None);
            let entity = sim.substrate.entities.get(1).unwrap();
            assert_eq!(entity.weapon_burst.index(), i32::from(has_destination));
            assert_eq!(entity.passively_acquired_target, has_destination);
            assert_eq!(
                entity.attack_target.as_ref().map(|target| target.target),
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
