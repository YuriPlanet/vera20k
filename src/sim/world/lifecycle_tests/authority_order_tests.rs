//! Lifecycle authority ordering tests: reveal, logic-vector membership,
//! conceal and uninit commit their steps in native order, and the ordered
//! drain removes entities and anims together.

use super::*;

#[test]
fn lifecycle_authority_reveal_commits_coords_then_marks_then_registers() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);

    assert_eq!(
        sim.try_reveal_entity(1, request(10, 20, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed {
            logic_registered: true
        }
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        (
            entity.position.rx(),
            entity.position.ry(),
            entity.position.z()
        ),
        (10, 20, 2)
    );
    assert_eq!(entity.position.sub_x(), SimFixed::from_num(128));
    assert_eq!(entity.position.sub_y(), SimFixed::from_num(64));
    assert!(!entity.lifecycle.in_limbo());
    assert!(entity.lifecycle.cell_marked());
    assert!(entity.seldom.in_logic_vector.is_member());
    assert!(sim.substrate.occupancy.contains_entity(10, 20, 1));
    assert_eq!(sim.live_object_order_snapshot(), vec![1]);

    let expected = [
        LifecycleTestEvent::RevealLimboCleared,
        LifecycleTestEvent::RevealCoordinatesCommitted,
        LifecycleTestEvent::MarkPut,
        LifecycleTestEvent::RawOccupationListLinked,
        LifecycleTestEvent::RawOccupationMarked,
        LifecycleTestEvent::CellMarked,
        LifecycleTestEvent::RevealDisplayBoundary,
        LifecycleTestEvent::LogicAppended,
        LifecycleTestEvent::LogicMembershipSet,
    ];
    assert_eq!(sim.lifecycle_test_events.as_slice(), expected.as_slice());
}

#[test]
fn reveal_stops_after_mark_when_object_is_not_alive() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .lifecycle
        .set_object_alive_for_test(false);

    assert_eq!(
        sim.try_reveal_entity(1, request(10, 20, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed {
            logic_registered: false
        }
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(!entity.lifecycle.in_limbo());
    assert!(entity.lifecycle.cell_marked());
    assert!(!entity.seldom.in_logic_vector.is_member());
    assert!(sim.lifecycle_outputs.is_empty());
}

#[test]
fn lifecycle_authority_reveal_mark_failure_keeps_adjusted_coords_alive_limbo() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);

    assert_eq!(
        sim.try_reveal_entity(1, request(10, 20, PlacementEvidence::MarkFailed)),
        RevealOutcome::Failed(RevealFailure::MarkFailed)
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        (
            entity.position.rx(),
            entity.position.ry(),
            entity.position.z()
        ),
        (10, 20, 2)
    );
    assert!(entity.lifecycle.object_alive());
    assert!(entity.lifecycle.in_limbo());
    assert!(!entity.lifecycle.cell_marked());
    assert!(!entity.seldom.in_logic_vector.is_member());
    assert_eq!(
        sim.lifecycle_test_events,
        vec![
            LifecycleTestEvent::RevealLimboCleared,
            LifecycleTestEvent::RevealCoordinatesCommitted,
            LifecycleTestEvent::MarkPut,
        ]
    );
}

#[test]
fn lifecycle_authority_reveal_early_reject_commits_nothing() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let before = sim.substrate.entities.get(1).unwrap().position;

    assert_eq!(
        sim.try_reveal_entity(1, request(10, 20, PlacementEvidence::RejectedEarly)),
        RevealOutcome::Failed(RevealFailure::RejectedEarly)
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        (entity.position.rx(), entity.position.ry()),
        (before.rx(), before.ry())
    );
    assert!(entity.lifecycle.in_limbo());
    assert!(sim.lifecycle_test_events.is_empty());
}

#[test]
fn lifecycle_authority_reveal_rejects_an_already_marked_limbo_object() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let before = sim.substrate.entities.get(1).unwrap().position;
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .lifecycle
        .set_cell_marked_for_test(true);

    assert_eq!(
        sim.try_reveal_entity(1, request(10, 20, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Failed(RevealFailure::RejectedEarly)
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        (entity.position.rx(), entity.position.ry()),
        (before.rx(), before.ry())
    );
    assert!(entity.lifecycle.in_limbo());
    assert!(entity.lifecycle.cell_marked());
    assert!(sim.lifecycle_test_events.is_empty());
}

#[test]
fn lifecycle_authority_reveal_logic_failure_keeps_successful_mark() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate.logic.force_next_insert_failure_for_test();

    assert_eq!(
        sim.try_reveal_entity(1, request(10, 20, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed {
            logic_registered: false
        }
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(!entity.lifecycle.in_limbo());
    assert!(entity.lifecycle.cell_marked());
    assert!(!entity.seldom.in_logic_vector.is_member());
    assert!(sim.substrate.occupancy.contains_entity(10, 20, 1));
}

#[test]
fn lifecycle_authority_logic_flag_sets_after_append() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);

    sim.register_live_object(1);
    assert_eq!(sim.live_object_order_snapshot(), vec![1]);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .seldom
            .in_logic_vector
            .is_member()
    );
    assert_eq!(
        sim.lifecycle_test_events,
        vec![
            LifecycleTestEvent::LogicAppended,
            LifecycleTestEvent::LogicMembershipSet,
        ]
    );
}

#[test]
fn lifecycle_authority_logic_append_failure_leaves_flag_clear() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate.logic.force_next_insert_failure_for_test();

    sim.register_live_object(1);
    assert!(sim.live_object_order_snapshot().is_empty());
    assert!(
        !sim.substrate
            .entities
            .get(1)
            .unwrap()
            .seldom
            .in_logic_vector
            .is_member()
    );
    assert!(sim.lifecycle_test_events.is_empty());
}

#[test]
fn lifecycle_authority_logic_remove_compacts_then_clears_flag() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    insert_entity(&mut sim, 3, EntityCategory::Unit);
    sim.register_live_object(1);
    sim.register_live_object(2);
    sim.register_live_object(3);

    sim.unregister_live_object(2);
    assert_eq!(sim.live_object_order_snapshot(), vec![1, 3]);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .seldom
            .in_logic_vector
            .is_member()
    );
    assert!(
        !sim.substrate
            .entities
            .get(2)
            .unwrap()
            .seldom
            .in_logic_vector
            .is_member()
    );
    assert!(
        sim.substrate
            .entities
            .get(3)
            .unwrap()
            .seldom
            .in_logic_vector
            .is_member()
    );
}

#[test]
fn lifecycle_authority_flagged_missing_remove_still_clears_flag() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .seldom
        .in_logic_vector
        .set_for_test(true);
    assert!(sim.live_object_order_snapshot().is_empty());

    sim.unregister_live_object(1);
    assert!(sim.live_object_order_snapshot().is_empty());
    assert!(
        !sim.substrate
            .entities
            .get(1)
            .unwrap()
            .seldom
            .in_logic_vector
            .is_member()
    );
}

#[test]
fn particle_logic_membership_uses_the_object_local_guard_and_rebuilds_it() {
    let mut sim = Simulation::new();
    insert_particle_system(&mut sim, 7);

    assert!(sim.reveal_particle_system(7));
    assert!(
        sim.substrate
            .particle_systems
            .get(7)
            .unwrap()
            .in_logic_vector()
    );
    assert_eq!(sim.live_object_order_snapshot(), vec![7]);

    assert!(sim.reveal_particle_system(7));
    assert_eq!(sim.live_object_order_snapshot(), vec![7]);

    sim.substrate
        .particle_systems
        .get_mut(7)
        .unwrap()
        .set_in_logic_vector_for_test(false);
    sim.rebuild_logic_membership();
    assert!(
        sim.substrate
            .particle_systems
            .get(7)
            .unwrap()
            .in_logic_vector()
    );
    sim.debug_assert_logic_membership_consistent();

    assert!(sim.conceal_particle_system(7));
    assert!(
        !sim.substrate
            .particle_systems
            .get(7)
            .unwrap()
            .in_logic_vector()
    );
    assert!(sim.live_object_order_snapshot().is_empty());
}

#[test]
fn open_topped_direct_registration_keeps_hidden_passenger_live() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let _ = sim.reveal(1);
    let _ = sim.reveal(2);

    assert_eq!(sim.techno_limbo(1), super::ConcealOutcome::Concealed);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_passenger_role_for_test(PassengerRole::inside_for_test(99, true));

    assert!(sim.register_open_topped_passenger(1));
    let passenger = sim.substrate.entities.get(1).unwrap();
    assert!(passenger.lifecycle.object_alive());
    assert!(passenger.lifecycle.in_limbo());
    assert!(!passenger.lifecycle.cell_marked());
    assert!(passenger.seldom.in_logic_vector.is_member());
    assert_eq!(sim.live_object_order_snapshot(), vec![2, 1]);

    assert!(sim.register_open_topped_passenger(1));
    assert_eq!(sim.live_object_order_snapshot(), vec![2, 1]);
}

/// A rider killed inside an open-topped transport is already in limbo, so
/// UnInit's Limbo returns at its entry guard and leaves it in Logic. Its
/// destructor takes it out (`ObjectClass::~ObjectClass`, `0x005F3D65..0x005F3D75`);
/// a slot left behind stops the next multiplayer frame's checksum.
#[test]
fn destructor_takes_a_dead_open_topped_rider_out_of_logic() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let _ = sim.reveal(1);
    let _ = sim.reveal(2);
    assert_eq!(sim.techno_limbo(1), super::ConcealOutcome::Concealed);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_passenger_role_for_test(PassengerRole::inside_for_test(2, true));
    assert!(sim.register_open_topped_passenger(1));

    sim.uninit(1);
    assert_eq!(sim.live_object_order_snapshot(), vec![2, 1]);
    sim.process_pending_delete();

    assert!(!sim.substrate.entities.contains(1));
    assert_eq!(sim.live_object_order_snapshot(), vec![2]);
    assert!(sim.compute_retail_multiplayer_checksum().is_ok());
}

/// The Logic walk can reach a rider that died earlier in the frame: it is past
/// UnInit (`+0x90` clear) but still in the vector until its destructor. Native
/// `FootClass::AI` returns at the IsAlive test (`0x004DA53E`); the visit must
/// not UnInit it again.
#[test]
fn logic_visit_of_a_dead_open_topped_rider_does_not_uninit_it_again() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let _ = sim.reveal(1);
    let _ = sim.reveal(2);
    assert_eq!(sim.techno_limbo(1), super::ConcealOutcome::Concealed);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_passenger_role_for_test(PassengerRole::inside_for_test(2, true));
    assert!(sim.register_open_topped_passenger(1));
    sim.uninit(1);

    let _ = sim.object_ai_visit_one(1, None, &mut ObjectAiCtx::default());

    let uninits = sim
        .lifecycle_test_events
        .iter()
        .filter(|event| **event == LifecycleTestEvent::UninitAliveCleared { stable_id: 1 })
        .count();
    assert_eq!(uninits, 1);
}

#[test]
fn lifecycle_authority_second_reveal_is_idempotent() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    assert_eq!(
        sim.try_reveal_entity(1, request(10, 20, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed {
            logic_registered: true
        }
    );
    let first_position = sim.substrate.entities.get(1).unwrap().position;
    sim.lifecycle_outputs.clear_for_test();
    sim.lifecycle_test_events.clear();

    assert_eq!(
        sim.try_reveal_entity(1, request(30, 40, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::AlreadyRevealed
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        (
            entity.position.rx(),
            entity.position.ry(),
            entity.position.z(),
            entity.position.sub_x(),
            entity.position.sub_y(),
        ),
        (
            first_position.rx(),
            first_position.ry(),
            first_position.z(),
            first_position.sub_x(),
            first_position.sub_y(),
        )
    );
    assert!(sim.substrate.occupancy.contains_entity(10, 20, 1));
    assert!(!sim.substrate.occupancy.contains_entity(30, 40, 1));
    assert_eq!(sim.live_object_order_snapshot(), vec![1]);
    assert!(sim.lifecycle_outputs.is_empty());
    assert!(sim.lifecycle_test_events.is_empty());
}

#[test]
fn lifecycle_authority_conceal_without_dirty_eligibility_still_clears_drawn_state() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.reveal(1);
    sim.lifecycle_outputs.clear_for_test();

    let _ = sim.object_conceal(1);
    assert_eq!(
        sim.lifecycle_outputs.as_slice(),
        vec![
            LifecycleOutput::ObjectPointerExpired { stable_id: 1 },
            LifecycleOutput::DetachAttachedAnims { stable_id: 1 },
            LifecycleOutput::ClearDrawnState { stable_id: 1 },
            LifecycleOutput::ClearRedraw { stable_id: 1 },
        ]
    );
}

#[test]
fn lifecycle_authority_conceal_dirty_eligibility_emits_dirty_before_drawn_clear() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .seldom
        .removal_facts
        .set_dirty_rect_eligible_for_test(true);
    let _ = sim.reveal(1);
    sim.lifecycle_outputs.clear_for_test();

    let _ = sim.object_conceal(1);
    assert_eq!(
        sim.lifecycle_outputs.as_slice(),
        vec![
            LifecycleOutput::ObjectPointerExpired { stable_id: 1 },
            LifecycleOutput::DetachAttachedAnims { stable_id: 1 },
            LifecycleOutput::DirtyTacticalRect { stable_id: 1 },
            LifecycleOutput::ClearDrawnState { stable_id: 1 },
            LifecycleOutput::ClearRedraw { stable_id: 1 },
        ]
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(entity.lifecycle.object_alive());
    assert!(entity.lifecycle.in_limbo());
    assert!(!entity.lifecycle.cell_marked());
    assert!(!entity.seldom.in_logic_vector.is_member());
}

#[test]
fn lifecycle_authority_conceal_deselects_unmarks_unregisters_then_sets_limbo() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.reveal(1);
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.seldom.selection.set_selected_for_test(true);
    entity
        .seldom
        .removal_facts
        .set_dirty_rect_eligible_for_test(true);
    sim.lifecycle_test_events.clear();

    let _ = sim.object_conceal(1);
    assert_eq!(
        sim.lifecycle_test_events,
        vec![
            LifecycleTestEvent::ConcealDeselected,
            LifecycleTestEvent::ConcealDestroyNotifyBoundary {
                stable_id: 1,
                object_alive: true,
                cell_marked: true,
                resolvable: true,
            },
            LifecycleTestEvent::UninitRemovalListenerVisited {
                expired_id: 1,
                listener_id: 1,
                target_alive: true,
                target_in_limbo: false,
            },
            LifecycleTestEvent::RawOccupationListUnlinked,
            LifecycleTestEvent::RawOccupationCleared,
            LifecycleTestEvent::ConcealUnmarked,
            LifecycleTestEvent::ConcealDisplayBoundary,
            LifecycleTestEvent::ConcealAnimBoundary,
            LifecycleTestEvent::ConcealVocBoundary,
            LifecycleTestEvent::ConcealLogicRemoved,
            LifecycleTestEvent::ConcealDirtyTacticalRectBoundary,
            LifecycleTestEvent::ConcealClearDrawnStateBoundary,
            LifecycleTestEvent::ConcealLimboSet,
            LifecycleTestEvent::ConcealClearRedrawBoundary,
        ]
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(!entity.seldom.selection.is_selected());
    assert!(!entity.lifecycle.cell_marked());
    assert!(!entity.seldom.in_logic_vector.is_member());
    assert!(entity.lifecycle.in_limbo());
}

#[test]
fn gsi_04_05_conceal_removes_only_selected_object_list_layer() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.reveal(1);

    let (rx, ry) = {
        let entity = sim.substrate.entities.get(1).unwrap();
        (entity.position.rx(), entity.position.ry())
    };
    sim.substrate.occupancy.add(
        rx,
        ry,
        1,
        MovementLayer::Bridge,
        None,
        CellListInsertion::PrependNonBuilding,
    );
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(rx, ry, MovementLayer::Ground),
        1
    );
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(rx, ry, MovementLayer::Bridge),
        1
    );

    let _ = sim.object_conceal(1);

    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(rx, ry, MovementLayer::Ground),
        0,
        "conceal must unlink the current ground-list entry"
    );
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(rx, ry, MovementLayer::Bridge),
        1,
        "conceal must not cross-scan the bridge object list"
    );
    assert!(
        !sim.substrate
            .entities
            .get(1)
            .unwrap()
            .lifecycle
            .cell_marked()
    );
}

#[test]
fn lifecycle_authority_conceal_keeps_object_alive() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.reveal(1);

    let _ = sim.object_conceal(1);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(entity.lifecycle.object_alive());
    assert!(entity.lifecycle.in_limbo());
    assert!(!entity.death.is_dying());
    assert!(!sim.substrate.pending_delete.contains_object(&1));
}

#[test]
fn lifecycle_authority_techno_limbo_breaks_contacts_before_common_conceal() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let _ = sim.reveal(1);
    let _ = sim.reveal(2);
    assert_eq!(
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .seldom
            .radio_contacts
            .insert(2),
        Some(0)
    );
    assert_eq!(
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .seldom
            .radio_contacts
            .insert(1),
        Some(0)
    );
    sim.lifecycle_test_events.clear();

    let _ = sim.techno_limbo(1);
    assert_eq!(
        sim.lifecycle_test_events,
        vec![
            LifecycleTestEvent::BreakSlot {
                slot: 0,
                target: Some(2),
            },
            LifecycleTestEvent::BreakSenderCleared { target: 2 },
            LifecycleTestEvent::BreakReceiverClassEffect { target: 2 },
            LifecycleTestEvent::BreakReceiverCleared { target: 2 },
            LifecycleTestEvent::ConcealDeselected,
            LifecycleTestEvent::ConcealDestroyNotifyBoundary {
                stable_id: 1,
                object_alive: true,
                cell_marked: true,
                resolvable: true,
            },
            LifecycleTestEvent::UninitRemovalListenerVisited {
                expired_id: 1,
                listener_id: 1,
                target_alive: true,
                target_in_limbo: false,
            },
            LifecycleTestEvent::UninitRemovalListenerVisited {
                expired_id: 1,
                listener_id: 2,
                target_alive: true,
                target_in_limbo: false,
            },
            LifecycleTestEvent::RawOccupationListUnlinked,
            LifecycleTestEvent::RawOccupationCleared,
            LifecycleTestEvent::ConcealUnmarked,
            LifecycleTestEvent::ConcealDisplayBoundary,
            LifecycleTestEvent::ConcealAnimBoundary,
            LifecycleTestEvent::ConcealVocBoundary,
            LifecycleTestEvent::ConcealLogicRemoved,
            LifecycleTestEvent::ConcealClearDrawnStateBoundary,
            LifecycleTestEvent::ConcealLimboSet,
            LifecycleTestEvent::ConcealClearRedrawBoundary,
        ]
    );
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .seldom
            .radio_contacts
            .is_empty()
    );
    assert!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .seldom
            .radio_contacts
            .is_empty()
    );
}

#[test]
fn lifecycle_authority_uninit_limbos_while_alive_then_clears_alive_then_queues() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.reveal(1);
    sim.lifecycle_test_events.clear();

    sim.uninit(1);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(!entity.lifecycle.object_alive());
    assert!(entity.lifecycle.in_limbo());
    assert!(!entity.lifecycle.cell_marked());
    assert!(entity.death.is_dying());
    assert_eq!(sim.substrate.pending_delete.object_ids(), vec![1]);
    let limbo = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::ConcealLimboSet)
        .unwrap();
    let alive_clear = sim
        .lifecycle_test_events
        .iter()
        .position(|event| {
            matches!(
                *event,
                LifecycleTestEvent::UninitAliveCleared { stable_id: 1 }
            )
        })
        .unwrap();
    let queued = sim
        .lifecycle_test_events
        .iter()
        .position(|event| {
            matches!(
                *event,
                LifecycleTestEvent::PendingDeleteQueued { stable_id: 1 }
            )
        })
        .unwrap();
    assert!(limbo < alive_clear && alive_clear < queued);
}

#[test]
fn lifecycle_authority_uninit_removal_boundary_sees_alive_marked_target() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.reveal(1);
    sim.lifecycle_test_events.clear();

    sim.uninit(1);
    let removal = sim
        .lifecycle_test_events
        .iter()
        .position(|event| {
            matches!(
                *event,
                LifecycleTestEvent::UninitRemovalNotifyBoundary {
                    stable_id: 1,
                    object_alive: true,
                    cell_marked: true,
                    resolvable: true,
                }
            )
        })
        .expect("removal boundary");
    let conceal = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::ConcealDeselected)
        .expect("common Conceal");
    assert!(removal < conceal);
}

#[test]
fn gsi_05_03_conceal_destroy_and_uninit_broadcast_before_unmark() {
    let mut conceal_only = Simulation::new();
    insert_entity(&mut conceal_only, 1, EntityCategory::Unit);
    let _ = conceal_only.reveal(1);
    conceal_only.lifecycle_test_events.clear();

    assert_eq!(
        conceal_only.object_conceal(1),
        super::ConcealOutcome::Concealed
    );
    let conceal_boundary = conceal_only
        .lifecycle_test_events
        .iter()
        .position(|event| {
            *event
                == LifecycleTestEvent::ConcealDestroyNotifyBoundary {
                    stable_id: 1,
                    object_alive: true,
                    cell_marked: true,
                    resolvable: true,
                }
        })
        .expect("Conceal Destroy notification boundary");
    let conceal_unmark = conceal_only
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListUnlinked)
        .expect("Conceal unmark");
    assert!(conceal_boundary < conceal_unmark);
    assert!(conceal_only.substrate.entities.contains(1));
    assert!(
        conceal_only
            .substrate
            .entities
            .get(1)
            .unwrap()
            .lifecycle
            .object_alive()
    );

    let mut uninit = Simulation::new();
    insert_entity(&mut uninit, 1, EntityCategory::Unit);
    insert_entity(&mut uninit, 2, EntityCategory::Unit);
    let _ = uninit.reveal(2);
    uninit.lifecycle_test_events.clear();
    uninit.uninit(2);

    let direct = uninit
        .lifecycle_test_events
        .iter()
        .position(|event| {
            *event
                == LifecycleTestEvent::UninitRemovalNotifyBoundary {
                    stable_id: 2,
                    object_alive: true,
                    cell_marked: true,
                    resolvable: true,
                }
        })
        .expect("direct UnInit expiry boundary");
    let destroy = uninit
        .lifecycle_test_events
        .iter()
        .position(|event| {
            *event
                == LifecycleTestEvent::ConcealDestroyNotifyBoundary {
                    stable_id: 2,
                    object_alive: true,
                    cell_marked: true,
                    resolvable: true,
                }
        })
        .expect("virtual Conceal Destroy expiry boundary");
    let unmark = uninit
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListUnlinked)
        .expect("UnInit Conceal unmark");
    let alive_clear = uninit
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::UninitAliveCleared { stable_id: 2 })
        .expect("UnInit alive clear");
    assert!(direct < destroy && destroy < unmark && unmark < alive_clear);
    assert_eq!(
        uninit
            .lifecycle_test_events
            .iter()
            .filter(|event| matches!(
                event,
                LifecycleTestEvent::UninitRemovalListenerVisited { expired_id: 2, .. }
            ))
            .count(),
        4,
        "two listeners observe both expiry broadcasts"
    );
}

#[test]
fn gsi_05_03_dead_nonlimbo_stored_object_still_runs_conceal() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.reveal(1);
    {
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.lifecycle.set_object_alive_for_test(false);
        assert!(!entity.lifecycle.in_limbo());
        assert!(entity.lifecycle.cell_marked());
    }
    sim.lifecycle_test_events.clear();

    assert_eq!(sim.techno_limbo(1), super::ConcealOutcome::Concealed);
    let destroy = sim
        .lifecycle_test_events
        .iter()
        .position(|event| {
            *event
                == LifecycleTestEvent::ConcealDestroyNotifyBoundary {
                    stable_id: 1,
                    object_alive: false,
                    cell_marked: true,
                    resolvable: true,
                }
        })
        .expect("dead non-limbo object reached Conceal's Destroy boundary");
    let unmark = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListUnlinked)
        .expect("dead non-limbo object reached Conceal's Mark(REMOVE)");
    let limbo_set = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::ConcealLimboSet)
        .expect("dead non-limbo object completed Conceal");
    assert!(destroy < unmark && unmark < limbo_set);

    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(!entity.lifecycle.object_alive());
    assert!(entity.lifecycle.in_limbo());
    assert!(!entity.lifecycle.cell_marked());
    assert!(!entity.seldom.in_logic_vector.is_member());
}

#[test]
fn lifecycle_authority_uninit_dead_limbo_remains_resolvable_until_drain() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.reveal(1);

    sim.uninit(1);
    let entity = sim
        .substrate
        .entities
        .get(1)
        .expect("dead object remains stored until the ordinary drain");
    assert!(!entity.lifecycle.object_alive());
    assert!(entity.lifecycle.in_limbo());
    assert!(!entity.lifecycle.cell_marked());
    assert!(!entity.seldom.in_logic_vector.is_member());
    assert_eq!(sim.substrate.pending_delete.object_ids(), vec![1]);

    sim.process_pending_delete();
    assert!(!sim.substrate.entities.contains(1));
    assert!(sim.substrate.pending_delete.is_empty());
}

#[test]
fn lifecycle_authority_animated_death_stays_represented_until_uninit() {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    crate::sim::house_tracking::HouseTracking::register_object(
        &mut sim.substrate,
        &mut sim.houses,
        1,
    );
    let _ = sim.reveal(1);

    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.health.set_for_test(0);
    entity.death.set_dying_for_test(true);
    let tracking = sim.houses.get(&owner).unwrap().tracking();
    assert_eq!(
        (tracking.units_for_test(), tracking.active_for_test().0),
        (1, 1)
    );
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(!entity.seldom.kill_record.is_recorded());
    assert!(entity.lifecycle.object_alive());
    assert!(!entity.lifecycle.in_limbo());
    assert!(entity.lifecycle.cell_marked());
    assert!(entity.seldom.in_logic_vector.is_member());

    sim.uninit(1);
    let tracking = sim.houses.get(&owner).unwrap().tracking();
    assert_eq!(
        (tracking.units_for_test(), tracking.active_for_test().0),
        (1, 0)
    );
    assert!(
        !sim.substrate
            .entities
            .get(1)
            .unwrap()
            .lifecycle
            .object_alive()
    );
    assert!(
        !sim.substrate
            .entities
            .get(1)
            .unwrap()
            .seldom
            .in_logic_vector
            .is_member()
    );
}

#[test]
fn lifecycle_authority_entity_and_anim_share_ordered_drain() {
    let mut sim = Simulation::new();
    insert_anim(&mut sim, 2, true);
    sim.substrate.pending_delete.queue_object(2);
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.uninit(1);
    assert_eq!(sim.substrate.pending_delete.object_ids(), vec![2, 1]);
    sim.lifecycle_test_events.clear();

    sim.process_pending_delete();
    assert_eq!(
        sim.lifecycle_test_events,
        vec![
            LifecycleTestEvent::PendingDeleteDrainStarted,
            LifecycleTestEvent::DeferredDeleteSelected {
                ticket: super::DeferredDeleteTicket::Object(2),
            },
            LifecycleTestEvent::FinalizedCommon { stable_id: 2 },
            LifecycleTestEvent::DeferredDeleteSelected {
                ticket: super::DeferredDeleteTicket::Object(1),
            },
            // The Unit destructor's own Limbo (`0x0073584F`), already in limbo.
            LifecycleTestEvent::ConcealAlreadyLimboReturn {
                stable_id: 1,
                object_alive: false,
                resolvable: true,
            },
            LifecycleTestEvent::UninitRemovalListenerVisited {
                expired_id: 1,
                listener_id: 1,
                target_alive: false,
                target_in_limbo: true,
            },
            LifecycleTestEvent::FinalizedCommon { stable_id: 1 },
        ]
    );
    assert!(!sim.substrate.anims.contains_key(2));
    assert!(!sim.substrate.entities.contains(1));
    assert!(sim.substrate.pending_delete.is_empty());
}

#[test]
fn lifecycle_authority_late_tail_commits_frame_before_drain() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.uninit(1);
    sim.lifecycle_test_events.clear();

    let _ = sim.advance_tick(&[], None, None, None, 67);
    assert_eq!(
        sim.lifecycle_test_events,
        vec![
            LifecycleTestEvent::BinaryFrameCommitted,
            LifecycleTestEvent::PendingDeleteDrainStarted,
            LifecycleTestEvent::DeferredDeleteSelected {
                ticket: super::DeferredDeleteTicket::Object(1),
            },
            // The Unit destructor's own Limbo (`0x0073584F`), already in limbo.
            LifecycleTestEvent::ConcealAlreadyLimboReturn {
                stable_id: 1,
                object_alive: false,
                resolvable: true,
            },
            LifecycleTestEvent::UninitRemovalListenerVisited {
                expired_id: 1,
                listener_id: 1,
                target_alive: false,
                target_in_limbo: true,
            },
            LifecycleTestEvent::FinalizedCommon { stable_id: 1 },
        ]
    );
    assert_eq!(sim.session.binary_frame(), 1);
    assert!(!sim.substrate.entities.contains(1));
}

#[test]
fn lifecycle_authority_set_logic_order_for_test_synchronizes_all_membership_flags() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    insert_anim(&mut sim, 3, false);
    insert_particle_system(&mut sim, 4);

    sim.set_logic_order_for_test(vec![3, 4, 1]);
    assert_eq!(sim.live_object_order_snapshot(), vec![3, 4, 1]);
    assert!(sim.substrate.anims.get(3).unwrap().in_logic_vector());
    assert!(
        sim.substrate
            .particle_systems
            .get(4)
            .unwrap()
            .in_logic_vector()
    );
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .seldom
            .in_logic_vector
            .is_member()
    );
    assert!(
        !sim.substrate
            .entities
            .get(2)
            .unwrap()
            .seldom
            .in_logic_vector
            .is_member()
    );
    sim.debug_assert_logic_membership_consistent();

    sim.set_logic_order_for_test(vec![2]);
    assert_eq!(sim.live_object_order_snapshot(), vec![2]);
    assert!(!sim.substrate.anims.get(3).unwrap().in_logic_vector());
    assert!(
        !sim.substrate
            .particle_systems
            .get(4)
            .unwrap()
            .in_logic_vector()
    );
    assert!(
        !sim.substrate
            .entities
            .get(1)
            .unwrap()
            .seldom
            .in_logic_vector
            .is_member()
    );
    assert!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .seldom
            .in_logic_vector
            .is_member()
    );
    sim.debug_assert_logic_membership_consistent();
}
