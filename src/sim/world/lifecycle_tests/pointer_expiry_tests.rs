//! Uninit and pointer expiry tests: transport cargo teardown, the global
//! pointer expiry walk over targets, navigation and particle links, and
//! duplicate uninit handling.

use super::*;

#[test]
fn lifecycle_authority_transport_uninits_passengers_in_cargo_order_before_carrier_notify() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Infantry);
    insert_entity(&mut sim, 3, EntityCategory::Infantry);
    let _ = sim.reveal(1);
    let mut cargo = PassengerCargo::new(5, 1.0);
    cargo.board_forced_chain(&[(2, 1.0)]);
    cargo.board_forced_chain(&[(3, 1.0)]);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_passenger_role_for_test(PassengerRole::transport(cargo));
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .set_passenger_role_for_test(PassengerRole::inside_for_test(1, false));
    sim.substrate
        .entities
        .get_mut(3)
        .unwrap()
        .set_passenger_role_for_test(PassengerRole::inside_for_test(1, false));
    sim.lifecycle_test_events.clear();

    sim.uninit(1);
    // Cargo list order is head-first: `CargoClass::AddPassenger @ 0x004733A0`
    // prepends, so the passenger boarded last (3) is walked first.
    assert_eq!(sim.substrate.pending_delete.object_ids(), vec![3, 2, 1]);
    let notify_order = sim
        .lifecycle_test_events
        .iter()
        .filter_map(|event| match event {
            LifecycleTestEvent::UninitRemovalNotifyBoundary { stable_id, .. } => Some(*stable_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(notify_order, vec![3, 2, 1]);
    assert!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .passenger_role()
            .is_none()
    );
    assert!(
        sim.substrate
            .entities
            .get(3)
            .unwrap()
            .passenger_role()
            .is_none()
    );
    let cargo = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .passenger_role()
        .cargo()
        .unwrap();
    assert!(cargo.passengers().is_empty());
    assert!(cargo.passenger_sizes().is_empty());
    assert_eq!(cargo.total_size(), 0);
    // A Unit's Cargo teardown has no Building occupant cursor to reset.
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .passenger_role()
            .occupants()
            .fire_index(),
        0
    );
}

#[test]
fn nested_transport_uninit_keeps_inner_cargo_until_its_recursive_transaction() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    insert_entity(&mut sim, 3, EntityCategory::Infantry);
    let _ = sim.reveal(1);
    let mut outer = PassengerCargo::new(12, 6.0);
    outer.board_forced_chain(&[(2, 3.0)]);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_passenger_role_for_test(PassengerRole::transport(outer));
    let mut inner = PassengerCargo::new(5, 2.0);
    inner.board_forced_chain(&[(3, 1.0)]);
    let entity = sim.substrate.entities.get_mut(2).unwrap();
    entity.set_passenger_role_for_test(PassengerRole::transport(inner));
    entity
        .passenger_role_mut_for_test()
        .set_inside_transport(1, false);
    sim.substrate
        .entities
        .get_mut(3)
        .unwrap()
        .passenger_role_mut_for_test()
        .set_inside_transport(2, true);

    sim.uninit(1);
    // The recursive owner reaches the inner rider before either hull; clearing
    // the inner carrier link must not erase its own CargoClass first.
    assert_eq!(sim.substrate.pending_delete.object_ids(), vec![3, 2, 1]);
    for id in [1, 2, 3] {
        let entity = sim.substrate.entities.get(id).unwrap();
        assert!(!entity.lifecycle.object_alive());
        assert_eq!(entity.passenger_role().inside_transport_id(), None);
    }
    for id in [1, 2] {
        assert!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .passenger_role()
                .cargo()
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn nested_transport_parent_expiry_preserves_its_independent_cargo() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(""))
            .unwrap();
    // Steam gamemd, TechnoPointerExpired7078B2..7078DB compares
    // Object+90, exact Health==0 and ScenarioInit, independently of control.
    // These are controlled receiver priors, not a full transport fixture.
    for (alive, health, placement_scope, retain_inside) in [
        (true, 1, false, true),
        (true, -1, false, true),
        (true, 0, false, false),
        (false, 1, false, false),
        (true, 1, true, false),
    ] {
        for (inside, conceal_control) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let mut sim = Simulation::new();
            insert_entity(&mut sim, 1, EntityCategory::Unit);
            insert_entity(&mut sim, 2, EntityCategory::Unit);
            insert_entity(&mut sim, 3, EntityCategory::Infantry);
            let _ = sim.reveal(1);
            let _ = sim.reveal(2);
            let parent = sim.substrate.entities.get_mut(1).unwrap();
            parent.lifecycle.set_object_alive_for_test(alive);
            parent.health.set_for_test(health);
            let mut cargo = PassengerCargo::new(5, 2.0);
            cargo.board_forced_chain(&[(3, 1.0)]);
            let entity = sim.substrate.entities.get_mut(2).unwrap();
            entity.set_passenger_role_for_test(PassengerRole::transport(cargo));
            if inside {
                entity
                    .passenger_role_mut_for_test()
                    .set_inside_transport(1, false);
            }
            sim.substrate
                .entities
                .get_mut(3)
                .unwrap()
                .passenger_role_mut_for_test()
                .set_inside_transport(2, true);

            let announce_expiry = |sim: &mut Simulation| {
                if conceal_control {
                    // ObjectConceal5F4D5D..61 transmits literal control1 while
                    // retaining the expired object's Alive and Health fields.
                    sim.object_conceal(1);
                } else {
                    sim.detach_all_pointer_expired(1, &rules, &mut super::FrameEffects::default());
                }
            };
            if placement_scope {
                sim.with_object_placement_scope(announce_expiry);
            } else {
                announce_expiry(&mut sim);
            }
            let role = sim.substrate.entities.get(2).unwrap().passenger_role();
            assert_eq!(
                role.inside_transport(),
                if inside && retain_inside {
                    Some((1, false))
                } else {
                    None
                },
                "inside={inside}, control1={conceal_control}, alive={alive}, health={health}, scope={placement_scope}"
            );
            assert_eq!(role.cargo().unwrap().passengers(), [3]);
            assert_eq!(role.cargo().unwrap().total_size(), 1);
            assert_eq!(
                sim.substrate
                    .entities
                    .get(3)
                    .unwrap()
                    .passenger_role()
                    .inside_transport(),
                Some((2, true))
            );
        }
    }
}

#[test]
fn uninit_pointer_expiry_walks_global_object_order_before_break_and_conceal() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_anim(&mut sim, 2, false);
    insert_particle_system(&mut sim, 3);
    insert_entity(&mut sim, 4, EntityCategory::Unit);
    let _ = sim.reveal(1);
    let _ = sim.reveal(4);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .seldom
        .radio_contacts
        .insert(4);
    sim.substrate
        .entities
        .get_mut(4)
        .unwrap()
        .seldom
        .radio_contacts
        .insert(1);
    sim.lifecycle_test_events.clear();

    sim.uninit(4);

    let visited = sim
        .lifecycle_test_events
        .iter()
        .filter_map(|event| match event {
            LifecycleTestEvent::UninitRemovalListenerVisited {
                expired_id: 4,
                listener_id,
                target_alive,
                target_in_limbo,
            } => Some((*listener_id, *target_alive, *target_in_limbo)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        visited,
        vec![
            (1, true, false),
            (2, true, false),
            (3, true, false),
            (4, true, false),
            (1, true, false),
            (2, true, false),
            (3, true, false),
            (4, true, false),
        ]
    );

    let break_slot = sim
        .lifecycle_test_events
        .iter()
        .position(|event| {
            matches!(
                event,
                LifecycleTestEvent::BreakSlot {
                    slot: 0,
                    target: Some(1)
                }
            )
        })
        .unwrap();
    let conceal = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::ConcealDeselected)
        .unwrap();
    let direct_last_listener = sim.lifecycle_test_events[..break_slot]
        .iter()
        .rposition(|event| {
            matches!(
                event,
                LifecycleTestEvent::UninitRemovalListenerVisited { expired_id: 4, .. }
            )
        })
        .unwrap();
    let conceal_notify = sim
        .lifecycle_test_events
        .iter()
        .position(|event| {
            matches!(
                event,
                LifecycleTestEvent::ConcealDestroyNotifyBoundary { stable_id: 4, .. }
            )
        })
        .unwrap();
    let second_last_listener = sim
        .lifecycle_test_events
        .iter()
        .rposition(|event| {
            matches!(
                event,
                LifecycleTestEvent::UninitRemovalListenerVisited { expired_id: 4, .. }
            )
        })
        .unwrap();
    let unmark = sim
        .lifecycle_test_events
        .iter()
        .position(|event| *event == LifecycleTestEvent::RawOccupationListUnlinked)
        .unwrap();
    assert!(direct_last_listener < break_slot && break_slot < conceal);
    assert!(conceal < conceal_notify && conceal_notify < second_last_listener);
    assert!(second_last_listener < unmark);
}

#[test]
fn pointer_expiry_clears_live_target_and_navigation_refs() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let _ = sim.try_reveal_entity(2, request(9, 11, PlacementEvidence::MarkSucceeded));

    let listener = sim.substrate.entities.get_mut(1).unwrap();
    listener
        .attack_target
        .set_for_test(Some(TargetKind::Entity(2)));
    listener
        .seldom
        .suspended_attack_target
        .set_for_test(Some(TargetKind::Entity(2)));
    listener
        .navigation
        .set_suspended_nav_com_for_test(Some(NavTargetRef::Object { id: 2 }));
    listener
        .navigation
        .set_nav_com_aux_for_test(Some(NavTargetRef::Object { id: 2 }));
    listener
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::Object { id: 2 }));
    listener.navigation.set_nav_queue_for_test(vec![
        NavTargetRef::Object { id: 2 },
        NavTargetRef::Cell { rx: 7, ry: 8 },
        NavTargetRef::Object { id: 2 },
    ]);
    sim.uninit(2);

    let listener = sim.substrate.entities.get(1).unwrap();
    assert!(listener.attack_target.target().is_none());
    assert!(listener.seldom.suspended_attack_target.target().is_none());
    assert!(listener.navigation.suspended_nav_com().is_none());
    assert!(listener.navigation.nav_com_aux().is_none());
    assert!(listener.navigation.nav_com().is_none());
    assert_eq!(
        listener.navigation.nav_queue(),
        vec![NavTargetRef::Cell { rx: 7, ry: 8 }]
    );
}

#[test]
fn occupier_capture_retains_live_non_selling_nav_target() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Structure);
    let _ = sim.try_reveal_entity(2, request(9, 11, PlacementEvidence::MarkSucceeded));
    sim.substrate
        .mission_assign_exact(
            1,
            MissionId::from_known(MissionType::Capture),
            sim.session.binary_frame(),
        )
        .unwrap();

    let listener = sim.substrate.entities.get_mut(1).unwrap();
    listener.seldom.removal_facts.set_occupier_for_test(true);
    listener
        .navigation
        .set_suspended_nav_com_for_test(Some(NavTargetRef::Object { id: 2 }));
    listener
        .navigation
        .set_nav_com_aux_for_test(Some(NavTargetRef::Object { id: 2 }));
    listener
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::Object { id: 2 }));
    listener
        .navigation
        .set_nav_queue_for_test(vec![NavTargetRef::Object { id: 2 }]);

    sim.uninit(2);

    let listener = sim.substrate.entities.get(1).unwrap();
    assert!(listener.navigation.suspended_nav_com().is_none());
    assert_eq!(
        listener.navigation.nav_com_aux(),
        Some(NavTargetRef::Object { id: 2 })
    );
    assert_eq!(
        listener.navigation.nav_com(),
        Some(NavTargetRef::Object { id: 2 })
    );
    assert!(listener.navigation.nav_queue().is_empty());
}

#[test]
fn selling_target_disables_occupier_capture_nav_retention() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Structure);
    let _ = sim.try_reveal_entity(2, request(9, 11, PlacementEvidence::MarkSucceeded));
    sim.substrate
        .mission_assign_exact(
            1,
            MissionId::from_known(MissionType::Capture),
            sim.session.binary_frame(),
        )
        .unwrap();
    sim.substrate
        .mission_assign_exact(
            2,
            MissionId::from_known(MissionType::Selling),
            sim.session.binary_frame(),
        )
        .unwrap();

    let listener = sim.substrate.entities.get_mut(1).unwrap();
    listener.seldom.removal_facts.set_occupier_for_test(true);
    listener
        .navigation
        .set_nav_com_aux_for_test(Some(NavTargetRef::Object { id: 2 }));
    listener
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::Object { id: 2 }));

    sim.uninit(2);

    let listener = sim.substrate.entities.get(1).unwrap();
    assert!(listener.navigation.nav_com_aux().is_none());
    assert!(listener.navigation.nav_com().is_none());
}

#[test]
fn target_expiry_rearms_passive_scan_and_restores_suspended_mission() {
    let rules = receiver_authority_rules();
    let mut sim = Simulation::with_seed(0x1234);
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    insert_entity(&mut sim, 3, EntityCategory::Unit);
    let _ = sim.try_reveal_entity(2, request(9, 11, PlacementEvidence::MarkSucceeded));
    sim.session.set_binary_frame_for_test(105);

    let listener = sim.substrate.entities.get_mut(1).unwrap();
    listener
        .attack_target
        .set_for_test(Some(TargetKind::Entity(2)));
    listener
        .seldom
        .suspended_attack_target
        .set_for_test(Some(TargetKind::Entity(3)));
    listener
        .navigation
        .set_suspended_nav_com_for_test(Some(NavTargetRef::Cell { rx: 7, ry: 8 }));
    listener.passive_scan.timer_mut_for_test().arm(100, 20);
    listener.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_known(MissionType::Attack),
        suspended: MissionId::from_known(MissionType::Guard),
        queued: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 100,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(100),
    });

    let mut expected_rng = sim.scenario_rng.clone();
    let delay = expected_rng.next_range_u32_inclusive(4, 8);
    sim.uninit_with_rules(2, &rules);

    let listener = sim.substrate.entities.get(1).unwrap();
    assert_eq!(listener.passive_scan.timer().start_frame(), 105);
    assert_eq!(listener.passive_scan.timer().duration(), delay);
    assert_eq!(
        listener.mission.current(),
        MissionId::from_known(MissionType::Guard)
    );
    assert_eq!(listener.mission.suspended(), MissionId::NONE);
    assert_eq!(listener.attack_target.target(), Some(TargetKind::Entity(3)));
    assert_eq!(
        listener.navigation.nav_com(),
        Some(NavTargetRef::Cell { rx: 7, ry: 8 })
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
}

/// Techno707964..70799E and the Foot sensor call4D9A57 ask the
/// predicate only for an expiring Techno (+14 bit0). This explicitly supplied
/// control0 callback isolates that class guard: ordinary Terrain lethal
/// integration uses control1, and the public DetachAll entry has Techno callers.
/// It does not claim a Terrain control0 broadcast or expiry-index traversal.
#[test]
fn detach_sensor_guard_applies_to_techno_but_not_constructed_terrain() {
    use super::lifecycle::PointerExpiryControl;
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::vision::FogState;

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n0=SENSOR\n1=TARGET\n\
         [AircraftTypes]\n[BuildingTypes]\n[TerrainTypes]\n0=TREE01\n\
         [SENSOR]\nStrength=100\nSight=0\nSpeed=0\nSensorsSight=4\n\
         [TARGET]\nStrength=100\nSight=0\nSpeed=0\n\
         [TREE01]\nStrength=100\n",
    ))
    .unwrap();
    for terrain_expires in [true, false] {
        let mut sim = Simulation::with_seed(0x1234);
        *sim.fog = FogState::new(32, 32);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        assert_eq!(
            crate::sim::terrain_spawn::construct_terrain_objects(
                &mut sim,
                &[crate::map::overlay::TerrainObject {
                    rx: 16,
                    ry: 16,
                    name: "TREE01".to_owned(),
                }],
                &rules,
                false,
            ),
            1
        );
        let tree = sim.terrain_objects.cell_index()[&(16, 16)];
        let listener = sim
            .spawn_object_at_height("SENSOR", "Americans", 15, 16, 0, 0, &rules)
            .unwrap();
        let references = sim
            .spawn_object_at_height("SENSOR", "Americans", 15, 17, 0, 0, &rules)
            .unwrap();
        let techno = sim
            .spawn_object_at_height("TARGET", "Soviet", 18, 16, 0, 0, &rules)
            .unwrap();
        let saved = sim
            .spawn_object_at_height("TARGET", "Soviet", 20, 20, 0, 0, &rules)
            .unwrap();
        let (expired, cell, owner) = if terrain_expires {
            (tree, (16, 16), None)
        } else {
            (
                techno,
                (18, 16),
                Some(sim.substrate.entities.get(techno).unwrap().owner()),
            )
        };
        let sensing_house = sim.substrate.entities.get(listener).unwrap().owner();
        assert!(sim.fog.has_sensor_for_house(sensing_house, cell.0, cell.1));
        let sensor_counts = sim.fog.sensors_by_house().clone();
        let registered = sim.live_object_order_snapshot();
        sim.session.set_binary_frame_for_test(105);
        {
            let actor = sim.substrate.entities.get_mut(listener).unwrap();
            actor
                .attack_target
                .set_for_test(Some(TargetKind::Entity(expired)));
            actor
                .seldom
                .suspended_attack_target
                .set_for_test(Some(TargetKind::Entity(saved)));
            actor.set_archive_target(Some(TargetKind::Entity(expired)));
            actor
                .navigation
                .set_nav_com_for_test(Some(NavTargetRef::object(expired)));
            actor
                .navigation
                .set_suspended_nav_com_for_test(Some(NavTargetRef::cell(20, 16)));
            actor.passive_scan.timer_mut_for_test().arm(100, 20);
            actor.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_known(MissionType::Attack),
                suspended: MissionId::from_known(MissionType::Guard),
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 100,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::at_frame(100),
            });
        }
        {
            // A separate unsuspended listener keeps every exact expired
            // reference observable; Restore above installs a surviving target.
            let actor = sim.substrate.entities.get_mut(references).unwrap();
            actor
                .attack_target
                .set_for_test(Some(TargetKind::Entity(expired)));
            actor
                .seldom
                .suspended_attack_target
                .set_for_test(Some(TargetKind::Entity(expired)));
            actor.set_archive_target(Some(TargetKind::Entity(expired)));
            actor
                .navigation
                .set_nav_com_for_test(Some(NavTargetRef::object(expired)));
            actor
                .navigation
                .set_nav_com_aux_for_test(Some(NavTargetRef::object(expired)));
            actor
                .navigation
                .set_suspended_nav_com_for_test(Some(NavTargetRef::object(expired)));
            actor.navigation.set_nav_queue_for_test(vec![
                NavTargetRef::object(expired),
                NavTargetRef::cell(20, 16),
            ]);
            actor.passive_scan.timer_mut_for_test().arm(0, 0);
        }
        let timer_before = sim
            .substrate
            .entities
            .get(listener)
            .unwrap()
            .passive_scan
            .timer();
        let mut expected_rng = sim.scenario_rng.clone();
        let delay = terrain_expires.then(|| expected_rng.next_range_u32_inclusive(4, 8));
        for receiver in [listener, references] {
            sim.notify_entity_pointer_expired(
                receiver,
                expired,
                Some(cell),
                true,
                100,
                false,
                owner,
                PointerExpiryControl::DetachAll,
                &rules,
                &mut super::FrameEffects::default(),
            );
        }
        let actor = sim.substrate.entities.get(listener).unwrap();
        assert_eq!(
            actor.attack_target.target(),
            Some(TargetKind::Entity(if terrain_expires {
                saved
            } else {
                expired
            },))
        );
        assert_eq!(
            actor.mission.current(),
            MissionId::from_known(if terrain_expires {
                MissionType::Guard
            } else {
                MissionType::Attack
            },)
        );
        assert_eq!(
            actor.mission.suspended(),
            if terrain_expires {
                MissionId::NONE
            } else {
                MissionId::from_known(MissionType::Guard)
            }
        );
        assert_eq!(
            actor.seldom.suspended_attack_target.target(),
            Some(TargetKind::Entity(saved))
        );
        assert_eq!(
            actor.archive_target(),
            None,
            "Foot archive clear ignores the sensor gate"
        );
        assert_eq!(
            actor.navigation.nav_com(),
            Some(if terrain_expires {
                NavTargetRef::cell(20, 16)
            } else {
                NavTargetRef::object(expired)
            })
        );
        if let Some(delay) = delay {
            assert_eq!(actor.passive_scan.timer().start_frame(), 105);
            assert_eq!(actor.passive_scan.timer().duration(), delay);
        } else {
            assert_eq!(actor.passive_scan.timer(), timer_before);
        }
        let actor = sim.substrate.entities.get(references).unwrap();
        let retained = (!terrain_expires).then_some(TargetKind::Entity(expired));
        assert_eq!(actor.attack_target.target(), retained);
        assert_eq!(actor.seldom.suspended_attack_target.target(), retained);
        assert_eq!(actor.archive_target(), None);
        let retained_nav = (!terrain_expires).then_some(NavTargetRef::object(expired));
        assert_eq!(actor.navigation.nav_com(), retained_nav);
        assert_eq!(actor.navigation.nav_com_aux(), retained_nav);
        assert_eq!(actor.navigation.suspended_nav_com(), None);
        assert_eq!(
            actor.navigation.nav_queue(),
            vec![NavTargetRef::cell(20, 16)]
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state()
        );

        // The first callback consumed the only Restore and shortening event.
        // A repeat must not spend another draw, even while the Techno control
        // still retains its sensed target.
        let after_first = sim.substrate.entities.get(listener).unwrap().clone();
        for receiver in [listener, references] {
            sim.notify_entity_pointer_expired(
                receiver,
                expired,
                Some(cell),
                true,
                100,
                false,
                owner,
                PointerExpiryControl::DetachAll,
                &rules,
                &mut super::FrameEffects::default(),
            );
        }
        let actor = sim.substrate.entities.get(listener).unwrap();
        assert_eq!(actor.attack_target, after_first.attack_target);
        assert_eq!(actor.mission, after_first.mission);
        assert_eq!(actor.passive_scan, after_first.passive_scan);
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state()
        );
        assert_eq!(sim.fog.sensors_by_house(), &sensor_counts);
        assert_eq!(sim.terrain_objects.cell_index()[&(16, 16)], tree);
        assert!(sim.terrain_objects.objects()[&tree].is_live());
        assert_eq!(sim.live_object_order_snapshot(), registered);
    }
}

#[test]
fn infantry_target_expiry_clears_firing_action_before_target() {
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    let ini = IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=TEST\n[AircraftTypes]\n\
         [BuildingTypes]\n[E1]\nImage=GI\nStrength=125\n\
         [TEST]\nStrength=100\n",
    );
    let art = IniFile::from_str(crate::rules::retail_ini_fixture::GI_ART_EXCERPT);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    // Original 51B20E drops the firing latch before its unforced Ready
    // request. FireUp (4) admits it; Die2 (12) refuses and retains its stage.
    // The corresponding class-target receipts pin this ordering without RNG.
    for (doing, expected_doing) in [(4, 0), (12, 12)] {
        let mut sim = Simulation::new();
        assert_eq!(
            sim.spawn_object_at_height("E1", "Americans", 2, 3, 0, 0, &rules),
            Some(1)
        );
        insert_entity(&mut sim, 2, EntityCategory::Unit);
        let _ = sim.try_reveal_entity(2, request(9, 11, PlacementEvidence::MarkSucceeded));
        assert!(
            sim.infantry_do_action(
                1,
                doing,
                true,
                &rules,
                &mut crate::sim::world::FrameEffects::default()
            )
            .unwrap()
        );
        let listener = sim.substrate.entities.get_mut(1).unwrap();
        listener
            .attack_target
            .set_for_test(Some(TargetKind::Entity(2)));
        listener.mission_leaf =
            crate::sim::mission::MissionLeafState::infantry_raw_for_test(1, doing);
        // Isolate the class action: the separate PointerExpired timer gate
        // (7079D1) draws RandomRanged(4,8) only above ten remaining frames.
        listener.passive_scan.timer_mut_for_test().arm(0, 0);
        let before = sim.scenario_rng.logical_state();
        sim.uninit_with_rules(2, &rules);
        let listener = sim.substrate.entities.get(1).unwrap();
        assert!(listener.attack_target.target().is_none());
        assert_eq!(listener.infantry_sprite_pose().unwrap().0, expected_doing);
        let leaf = listener.mission_leaf.as_infantry().unwrap();
        assert_eq!(leaf.firing_sequence_latch(), 0);
        assert_eq!(leaf.doing(), expected_doing);
        assert_eq!(sim.scenario_rng.logical_state(), before);
    }
}

#[test]
fn particle_system_retirement_clears_only_the_indexed_techno_spark_link() {
    let mut sim = Simulation::new();
    insert_particle_system(&mut sim, 1);
    insert_particle_system(&mut sim, 2);
    insert_entity(&mut sim, 3, EntityCategory::Unit);
    insert_entity(&mut sim, 4, EntityCategory::Unit);
    sim.substrate
        .entities
        .get_mut(3)
        .unwrap()
        .seldom
        .spark_particle_system
        .set_for_test(Some(1));
    sim.substrate
        .entities
        .get_mut(4)
        .unwrap()
        .seldom
        .spark_particle_system
        .set_for_test(Some(2));
    sim.substrate
        .particle_systems
        .get_mut(1)
        .unwrap()
        .set_done_spawning_for_test(true);
    let rng = [
        sim.scenario_rng.native_state_hex(),
        sim.main_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ];
    sim.retire_particle_system(1, super::UninitContext::default());
    assert_eq!(
        sim.substrate
            .entities
            .get(3)
            .unwrap()
            .seldom
            .spark_particle_system
            .system(),
        None
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(4)
            .unwrap()
            .seldom
            .spark_particle_system
            .system(),
        Some(2)
    );
    assert!(sim.particle_systems().get(1).unwrap().in_limbo());
    assert!(sim.substrate.pending_delete.object_ids().contains(&1));
    assert_eq!(
        [
            sim.scenario_rng.native_state_hex(),
            sim.main_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex()
        ],
        rng
    );
    sim.process_pending_delete();
    assert!(sim.particle_systems().get(1).is_none());
}

#[test]
fn techno_spark_pointer_expiry_is_exact_and_independent_of_control() {
    use super::lifecycle::PointerExpiryControl;
    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/welding.json");
    let rows: Vec<_> = native["lifecycle_cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["input"]["receiver"] == "owner" && row["input"]["expired"].is_string())
        .collect();
    assert_eq!(rows.len(), 4);
    for row in rows {
        let control = if row["input"]["removed"] == 0 {
            PointerExpiryControl::DetachAll
        } else {
            PointerExpiryControl::Uninit
        };
        let mut sim = Simulation::new();
        insert_entity(&mut sim, 3, EntityCategory::Unit);
        sim.substrate
            .entities
            .get_mut(3)
            .unwrap()
            .seldom
            .spark_particle_system
            .set_for_test(Some(1));
        let before = [
            sim.main_rng.native_state_hex(),
            sim.scenario_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        ];
        assert_eq!(row["before"]["spark_link"], "system");
        let expired = if row["input"]["expired"] == "system" {
            1
        } else {
            2
        };
        sim.notify_entity_pointer_expired(
            3,
            expired,
            None,
            true,
            1,
            false,
            None,
            control,
            crate::rules::ruleset::RuleSet::empty_for_test(),
            &mut super::FrameEffects::default(),
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(3)
                .unwrap()
                .seldom
                .spark_particle_system
                .system()
                .map(|_| "system"),
            row["after"]["spark_link"].as_str()
        );
        assert_eq!(
            [
                sim.main_rng.native_state_hex(),
                sim.scenario_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex(),
            ],
            before
        );
        assert_eq!(row["rng_unchanged"], true);
    }
}

#[test]
fn pointer_expiry_stops_particle_owner_without_deleting_living_children() {
    let mut sim = Simulation::new();
    insert_particle_system(&mut sim, 1);
    insert_particle_system(&mut sim, 2);
    insert_entity(&mut sim, 3, EntityCategory::Unit);
    insert_particle_system(&mut sim, 4);
    let _ = sim.reveal(3);
    let owned = sim.substrate.particle_systems.get_mut(1).unwrap();
    owned.set_owner_entity_for_test(Some(3));
    let mut child = crate::sim::particles::Particle::test_fixture(
        crate::rules::particle_type::ParticleTypeId(0),
        IVec3::new(500, 700, 90),
    );
    child.set_lifetime_remaining_for_test(71);
    owned.particles_mut_for_test().push(child);
    let attached = sim.substrate.particle_systems.get_mut(2).unwrap();
    attached.set_owner_entity_for_test(Some(3));
    attached.set_target_ref_for_test(Some(crate::sim::combat::TargetKind::Entity(3)));
    sim.substrate
        .particle_systems
        .get_mut(4)
        .unwrap()
        .set_target_ref_for_test(Some(crate::sim::combat::TargetKind::Entity(3)));
    let rng = [
        sim.scenario_rng.native_state_hex(),
        sim.main_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ];

    sim.uninit(3);

    let owned = sim.substrate.particle_systems.get(1).unwrap();
    assert!(owned.owner_entity().is_none());
    assert!(owned.done_spawning(), "owner+E0 expiry sets done+F8");
    assert_eq!(owned.particles().len(), 1);
    assert_eq!(owned.particles()[0].lifetime_remaining(), 71);
    assert_eq!(owned.particles()[0].coords(), IVec3::new(500, 700, 90));
    assert!(!owned.particles()[0].marked_for_deletion());
    assert!(
        owned.alive(),
        "living children retain their system for its own AI"
    );
    let attached = sim.substrate.particle_systems.get(2).unwrap();
    assert!(attached.owner_entity().is_none());
    assert!(attached.target_ref().is_none());
    assert!(attached.done_spawning());
    let target_only = sim.substrate.particle_systems.get(4).unwrap();
    assert!(target_only.target_ref().is_none());
    assert!(
        !target_only.done_spawning(),
        "target+E4 expiry has no done write"
    );
    assert_eq!(
        [
            sim.scenario_rng.native_state_hex(),
            sim.main_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        ],
        rng,
        "the represented owner/target expiry bodies consume no RNG"
    );
}

#[test]
fn completed_empty_smoke_limbo_clears_every_matching_techno_before_late_drain() {
    let mut sim = Simulation::new();
    for id in [1, 2, 3] {
        insert_entity(&mut sim, id, EntityCategory::Structure);
    }
    for id in [4, 5] {
        insert_particle_system(&mut sim, id);
        assert!(sim.reveal_particle_system(id));
    }
    sim.substrate
        .particle_systems
        .get_mut(4)
        .unwrap()
        .set_owner_entity_for_test(Some(1));
    for (id, smoke_id) in [(1, 4), (2, 4), (3, 5)] {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .seldom
            .damage_smoke_system_id
            .set_for_test(Some(smoke_id));
    }
    let rng = [
        sim.scenario_rng.native_state_hex(),
        sim.main_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ];

    sim.retire_particle_system(4, super::UninitContext::default());
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .seldom
            .damage_smoke_system_id
            .system(),
        Some(4)
    );
    assert!(sim.substrate.pending_delete.is_empty());

    let system = sim.substrate.particle_systems.get_mut(4).unwrap();
    system.set_done_spawning_for_test(true);
    system
        .particles_mut_for_test()
        .push(crate::sim::particles::Particle::test_fixture(
            crate::rules::particle_type::ParticleTypeId(0),
            IVec3::ZERO,
        ));
    sim.retire_particle_system(4, super::UninitContext::default());
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .seldom
            .damage_smoke_system_id
            .system(),
        Some(4)
    );
    assert!(sim.substrate.pending_delete.is_empty());

    sim.substrate
        .particle_systems
        .get_mut(4)
        .unwrap()
        .particles_mut_for_test()
        .clear();
    sim.lifecycle_test_events.clear();
    sim.retire_particle_system(4, super::UninitContext::default());

    for id in [1, 2] {
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .seldom
                .damage_smoke_system_id
                .system(),
            None,
            "the expiry callback clears every exact match, not only the producer"
        );
    }
    assert_eq!(
        sim.substrate
            .entities
            .get(3)
            .unwrap()
            .seldom
            .damage_smoke_system_id
            .system(),
        Some(5)
    );
    let system = sim.substrate.particle_systems.get(4).unwrap();
    assert!(!system.alive());
    assert!(system.in_limbo());
    assert!(!system.in_logic_vector());
    assert_eq!(sim.substrate.pending_delete.object_ids(), vec![4]);
    assert_eq!(sim.substrate.display.layer_of(4), None);
    let visited = sim
        .lifecycle_test_events
        .iter()
        .filter_map(|event| match event {
            LifecycleTestEvent::UninitRemovalListenerVisited {
                expired_id: 4,
                listener_id,
                target_alive,
                target_in_limbo,
            } => Some((*listener_id, *target_alive, *target_in_limbo)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        visited,
        vec![
            (1, true, false),
            (2, true, false),
            (3, true, false),
            (4, true, false),
            (5, true, false)
        ]
    );

    sim.retire_particle_system(4, super::UninitContext::default());
    assert_eq!(
        sim.substrate.pending_delete.object_ids(),
        vec![4],
        "the Alive gate prevents repeat AI retirement"
    );
    sim.lifecycle_test_events.clear();
    sim.process_pending_delete();
    assert!(sim.substrate.particle_systems.get(4).is_none());
    assert_eq!(
        sim.substrate
            .entities
            .get(3)
            .unwrap()
            .seldom
            .damage_smoke_system_id
            .system(),
        Some(5)
    );
    assert!(
        sim.lifecycle_test_events.iter().any(|event| matches!(
            event,
            LifecycleTestEvent::UninitRemovalListenerVisited {
                expired_id: 4,
                target_alive: false,
                target_in_limbo: true,
                ..
            }
        )),
        "the physical destructor repeats the shared expiry callback after Limbo"
    );
    assert_eq!(
        [
            sim.scenario_rng.native_state_hex(),
            sim.main_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        ],
        rng,
    );
}

// Ares simulates this Smoke system itself (hooks 0x62DF05, 0x62FD60) and
// builds no ParticleClass children.
#[test]
fn particle_system_physical_disposal_removes_retained_child_air_entries() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[Particles]\n0=Smoke\n[Smoke]\nBehavesLike=Smoke\nMaxEC=80\n\
             [ParticleSystems]\n0=SmokeSys\n[SmokeSys]\nBehavesLike=Smoke\nHoldsWhat=Smoke\n",
        ))
        .unwrap();
    let mut sim = Simulation::new();
    let system_id = sim
        .spawn_particle_system(
            crate::rules::particle_system_type::ParticleSystemTypeId(0),
            IVec3::new(500, 700, 90),
            None,
            None,
            IVec3::ZERO,
            None,
            &rules,
        )
        .unwrap();
    assert!(crate::sim::particles::system_ai::tick_particle_system(
        &mut sim,
        &rules,
        system_id,
        crate::sim::world::UninitContext::new(&rules)
    ));
    let child_id = sim.particle_systems().get(system_id).unwrap().particles()[0].stable_id();
    assert_eq!(
        sim.substrate.display.layer_of(child_id),
        Some(super::display_layers::DisplayLayer::AIR)
    );
    assert!(!sim.live_object_order_snapshot().contains(&child_id));
    let rng = [
        sim.scenario_rng.native_state_hex(),
        sim.main_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ];

    sim.finalize_and_remove_common(system_id, super::UninitContext::new(&rules), 1);

    assert!(sim.substrate.particle_systems.get(system_id).is_none());
    assert_eq!(sim.substrate.display.layer_of(system_id), None);
    assert_eq!(sim.substrate.display.layer_of(child_id), None);
    assert!(!sim.live_object_order_snapshot().contains(&system_id));
    assert_eq!(
        [
            sim.scenario_rng.native_state_hex(),
            sim.main_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        ],
        rng
    );
}

#[test]
fn expiring_mixed_size_passenger_updates_transport_total_exactly() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Infantry);
    insert_entity(&mut sim, 3, EntityCategory::Infantry);
    let mut cargo = PassengerCargo::new(5, 0.0);
    cargo.board_forced_chain(&[(2, 3.0)]);
    cargo.board_forced_chain(&[(3, 1.0)]);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_passenger_role_for_test(PassengerRole::transport(cargo));
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .set_passenger_role_for_test(PassengerRole::inside_for_test(1, false));
    sim.substrate
        .entities
        .get_mut(3)
        .unwrap()
        .set_passenger_role_for_test(PassengerRole::inside_for_test(1, false));

    sim.uninit(2);

    let cargo = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .passenger_role()
        .cargo()
        .unwrap();
    assert_eq!(cargo.passengers(), vec![3]);
    assert_eq!(cargo.passenger_sizes(), vec![1.0]);
    assert_eq!(cargo.total_size(), 1);
}

#[test]
fn gsi_05_03_duplicate_uninit_repeats_direct_expiry_and_queue_but_finalizes_once() {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    // As its construction would: Add_Tracking.
    crate::sim::house_tracking::HouseTracking::register_object(
        &mut sim.substrate,
        &mut sim.houses,
        1,
    );
    let _ = sim.reveal(1);

    sim.lifecycle_test_events.clear();
    sim.uninit(1);
    assert_eq!(
        sim.lifecycle_test_events
            .iter()
            .filter(|event| matches!(
                event,
                LifecycleTestEvent::UninitRemovalNotifyBoundary { stable_id: 1, .. }
            ))
            .count(),
        1
    );
    assert_eq!(
        sim.lifecycle_test_events
            .iter()
            .filter(|event| matches!(
                event,
                LifecycleTestEvent::ConcealDestroyNotifyBoundary { stable_id: 1, .. }
            ))
            .count(),
        1
    );
    assert_eq!(
        sim.lifecycle_test_events
            .iter()
            .filter(|event| matches!(
                event,
                LifecycleTestEvent::UninitRemovalListenerVisited { expired_id: 1, .. }
            ))
            .count(),
        2,
        "the first active UnInit performs its direct dispatch and Conceal's Destroy dispatch"
    );

    let repeat_start = sim.lifecycle_test_events.len();
    sim.uninit(1);
    let repeated_events = &sim.lifecycle_test_events[repeat_start..];
    // Removed_From_Game ran once (the first Limbo); the tracking waits for
    // the destructor at the drain.
    let tracking = sim.houses.get(&owner).unwrap().tracking();
    assert_eq!(tracking.active_for_test().0, 0);
    assert_eq!(tracking.units_for_test(), 1);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .seldom
            .kill_record
            .is_recorded()
    );
    assert_eq!(sim.substrate.pending_delete.object_ids(), vec![1, 1]);
    let direct = repeated_events
        .iter()
        .position(|event| {
            *event
                == LifecycleTestEvent::UninitRemovalNotifyBoundary {
                    stable_id: 1,
                    object_alive: false,
                    cell_marked: false,
                    resolvable: true,
                }
        })
        .expect("repeat UnInit direct expiry boundary");
    let direct_listener = repeated_events
        .iter()
        .position(|event| {
            *event
                == LifecycleTestEvent::UninitRemovalListenerVisited {
                    expired_id: 1,
                    listener_id: 1,
                    target_alive: false,
                    target_in_limbo: true,
                }
        })
        .expect("repeat UnInit direct expiry dispatch");
    let conceal_return = repeated_events
        .iter()
        .position(|event| {
            *event
                == LifecycleTestEvent::ConcealAlreadyLimboReturn {
                    stable_id: 1,
                    object_alive: false,
                    resolvable: true,
                }
        })
        .expect("repeat Limbo reached Conceal's InLimbo return");
    assert!(direct < direct_listener && direct_listener < conceal_return);
    assert_eq!(
        repeated_events
            .iter()
            .filter(|event| matches!(
                event,
                LifecycleTestEvent::UninitRemovalListenerVisited { expired_id: 1, .. }
            ))
            .count(),
        1,
        "the repeated UnInit adds exactly one direct expiry dispatch"
    );
    assert!(
        !repeated_events.iter().any(|event| matches!(
            event,
            LifecycleTestEvent::ConcealDestroyNotifyBoundary { stable_id: 1, .. }
        )),
        "Conceal's InLimbo return occurs before Destroy"
    );

    sim.lifecycle_test_events.clear();
    sim.process_pending_delete();
    assert!(!sim.substrate.entities.contains(1));
    assert!(sim.substrate.pending_delete.is_empty());
    assert_eq!(
        *sim.lifecycle_test_events,
        vec![
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
}

/// UnInit records the destruction once; the tracking leaves with the
/// destructor at the drain, once however often the object was queued.
#[test]
fn lifecycle_authority_uninit_records_once_and_the_drain_removes_the_tracking() {
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

    sim.uninit(1);
    sim.uninit(1);
    assert_eq!(
        sim.houses.get(&owner).unwrap().tracking().units_for_test(),
        1
    );
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .seldom
            .kill_record
            .is_recorded()
    );
    sim.flush_pending_delete();
    assert_eq!(
        sim.houses.get(&owner).unwrap().tracking().units_for_test(),
        0
    );
}
