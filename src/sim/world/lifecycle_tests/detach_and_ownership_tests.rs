//! Detach and ownership tests: the detach sweep that restores missions and
//! clears targets, naval build-const acquisition order, raw infantry owners
//! and destructors that limbo or kill riders.

use super::*;

#[test]
fn lifecycle_authority_alive_queued_object_remains_queued() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate.pending_delete.queue_object(1);

    sim.process_pending_delete();
    assert!(sim.substrate.entities.contains(1));
    assert_eq!(sim.substrate.pending_delete.object_ids(), vec![1]);
}

fn attack_fixture(current: MissionType, suspended: MissionId) -> MissionTestFixture {
    MissionTestFixture {
        current: MissionId::from_known(current),
        suspended,
        queued: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    }
}

/// Two attackers share one target that then leaves play ALIVE â€” sold, captured,
/// mind-controlled or teleported. Nothing dies, so the pointer-expiry broadcast
/// never runs and this sweep is the only thing that releases them.
///
/// Pins the three clauses that make the detach sweep different from the expiry
/// one: the Restore runs before the target clear, the clear is skipped when the
/// Restore replaced the target, and the walk is descending.
#[test]
fn detach_sweep_restores_before_clearing_target_in_descending_id_order() {
    let rules = receiver_authority_rules();
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Infantry);
    insert_entity(&mut sim, 3, EntityCategory::Structure);
    insert_entity(&mut sim, 4, EntityCategory::Unit);

    for attacker in [1, 2] {
        let entity = sim.substrate.entities.get_mut(attacker).unwrap();
        entity
            .attack_target
            .set_for_test(Some(TargetKind::Entity(3)));
        entity
            .seldom
            .suspended_attack_target
            .set_for_test(Some(TargetKind::Entity(4)));
        entity.navigation.set_nav_com_for_test(None);
        entity
            .navigation
            .set_suspended_nav_com_for_test(Some(NavTargetRef::Cell { rx: 7, ry: 8 }));
        entity.mission.apply_test_fixture(attack_fixture(
            MissionType::Attack,
            MissionId::from_known(MissionType::Move),
        ));
    }
    sim.lifecycle_test_events.clear();

    sim.stop_all_targeting_on_detach(3, &rules, &mut crate::sim::world::FrameEffects::default());

    // The detaching object is untouched and still present: this is not removal.
    assert!(sim.substrate.entities.contains(3));

    for attacker in [1, 2] {
        let entity = sim.substrate.entities.get(attacker).unwrap();
        assert_eq!(
            entity.mission.current(),
            MissionId::from_known(MissionType::Move),
            "attacker {attacker} restored its suspended mission"
        );
        assert_eq!(entity.mission.suspended(), MissionId::NONE);
        // Restore-then-clear: the archived target is reinstalled and the
        // null-out is skipped because it no longer matches the detaching object.
        assert_eq!(
            entity.attack_target.target(),
            Some(TargetKind::Entity(4)),
            "attacker {attacker} kept the target the Restore installed"
        );
        assert_eq!(
            entity.navigation.nav_com(),
            Some(NavTargetRef::Cell { rx: 7, ry: 8 }),
            "attacker {attacker} got its destination back"
        );
    }

    let visits: Vec<(u64, bool, bool)> = sim
        .lifecycle_test_events
        .iter()
        .filter_map(|event| match event {
            LifecycleTestEvent::DetachTargetingSweepVisited {
                detach_id: 3,
                listener_id,
                restored,
                target_cleared,
            } => Some((*listener_id, *restored, *target_cleared)),
            _ => None,
        })
        .collect();
    assert_eq!(
        visits,
        vec![(2, true, false), (1, true, false)],
        "the sweep walks descending stable-ID order"
    );
}

/// The other half of the sweep: an attacker with no suspended mission. The
/// Restore writes nothing, so the conditional null-out is the clause that fires
/// and the attacker is left with no target â€” which is what hands it to the
/// Attack handler's idle exit on its next dispatch.
#[test]
fn detach_sweep_clears_target_when_no_mission_was_suspended() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Structure);

    let attacker = sim.substrate.entities.get_mut(1).unwrap();
    attacker
        .attack_target
        .set_for_test(Some(TargetKind::Entity(2)));
    attacker
        .passive_scan
        .set_passively_acquired_target_for_test(true);
    attacker
        .mission
        .apply_test_fixture(attack_fixture(MissionType::Attack, MissionId::NONE));
    sim.lifecycle_test_events.clear();

    sim.stop_all_targeting_on_detach(
        2,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        &mut crate::sim::world::FrameEffects::default(),
    );

    let attacker = sim.substrate.entities.get(1).unwrap();
    assert!(attacker.attack_target.target().is_none());
    assert!(!attacker.passive_scan.acquired());
    assert_eq!(
        attacker.mission.current(),
        MissionId::from_known(MissionType::Attack),
        "a Restore with nothing suspended writes no mission field"
    );
    assert_eq!(attacker.mission.suspended(), MissionId::NONE);
    assert!(matches!(
        sim.lifecycle_test_events.as_slice(),
        [LifecycleTestEvent::DetachTargetingSweepVisited {
            detach_id: 2,
            listener_id: 1,
            restored: false,
            target_cleared: true,
        }]
    ));
}

/// A cell target is not the departing Techno pointer, so the live-object detach
/// sweep does not match it. AreaDamage and cliff callers explicitly invoke
/// the same restore-first sweep with their retained Cell identity.
#[test]
fn detach_sweep_never_matches_a_cell_target() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Structure);

    let attacker = sim.substrate.entities.get_mut(1).unwrap();
    attacker
        .attack_target
        .set_for_test(Some(TargetKind::Cell(9, 11)));
    attacker.mission.apply_test_fixture(attack_fixture(
        MissionType::Attack,
        MissionId::from_known(MissionType::Move),
    ));

    sim.stop_all_targeting_on_detach(
        2,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        &mut crate::sim::world::FrameEffects::default(),
    );

    let attacker = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        attacker.attack_target.target(),
        Some(TargetKind::Cell(9, 11))
    );
    assert_eq!(
        attacker.mission.current(),
        MissionId::from_known(MissionType::Attack),
        "a cell-targeted object is never restored by the detach sweep"
    );
}

#[test]
fn bridge_cell_success_restores_before_conditional_target_clear() {
    let rules = receiver_authority_rules();
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let attacker = sim.substrate.entities.get_mut(1).unwrap();
    attacker
        .attack_target
        .set_for_test(Some(TargetKind::Cell(9, 11)));
    attacker
        .seldom
        .suspended_attack_target
        .set_for_test(Some(TargetKind::Entity(2)));
    attacker
        .navigation
        .set_suspended_nav_com_for_test(Some(NavTargetRef::Cell { rx: 7, ry: 8 }));
    attacker.mission.apply_test_fixture(attack_fixture(
        MissionType::Attack,
        MissionId::from_known(MissionType::Move),
    ));
    sim.stop_all_targeting_cell(
        9,
        11,
        &rules,
        &mut crate::sim::world::FrameEffects::default(),
    );
    let attacker = sim.substrate.entities.get(1).unwrap();
    assert_eq!(attacker.attack_target.target(), Some(TargetKind::Entity(2)));
    assert_eq!(
        attacker.navigation.nav_com(),
        Some(NavTargetRef::Cell { rx: 7, ry: 8 })
    );
    assert_eq!(
        attacker.mission.current(),
        MissionId::from_known(MissionType::Move)
    );
}

fn insert_naval_build_const_fixture(
    sim: &mut Simulation,
    stable_id: u64,
    owner: crate::sim::intern::InternedId,
    rx: u16,
    ry: u16,
) {
    let type_ref = sim.interner.intern("GACNST");
    let mut building = GameEntity::new_at_frame_zero_for_test(
        stable_id,
        rx,
        ry,
        0,
        0,
        owner,
        Health::new(1000),
        type_ref,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    building
        .seldom
        .type_facts
        .set_foundation_for_test("1x1".to_string().into());
    building
        .seldom
        .type_facts
        .set_build_const_eligible_for_test(true);
    sim.substrate.entities.insert(building);
}

#[test]
fn naval_build_const_reveal_conceal_and_reentry_preserve_acquisition_order() {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    insert_naval_build_const_fixture(&mut sim, 20, owner, 2, 2);
    insert_naval_build_const_fixture(&mut sim, 10, owner, 4, 2);
    insert_naval_build_const_fixture(&mut sim, 30, owner, 6, 2);

    assert!(matches!(
        sim.try_reveal_entity(20, request(2, 2, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));
    assert!(matches!(
        sim.try_reveal_entity(10, request(4, 2, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));
    assert_eq!(
        sim.houses[&owner].build_const_order(),
        vec![20, 10],
        "acquisition order differs from stable-ID order"
    );
    // House+68, the Prism walk's list, is appended on the same Unlimbo path
    // (`0x00441553..0x00441594`).
    assert_eq!(house_buildings(&sim, owner), vec![20, 10]);

    assert_eq!(
        sim.try_reveal_entity(20, request(2, 2, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::AlreadyRevealed
    );
    assert_eq!(sim.houses[&owner].build_const_order(), vec![20, 10]);
    assert_eq!(
        sim.try_reveal_entity(30, request(6, 2, PlacementEvidence::MarkFailed)),
        RevealOutcome::Failed(RevealFailure::MarkFailed)
    );
    assert_eq!(sim.houses[&owner].build_const_order(), vec![20, 10]);

    // A native DynamicVector stable-removes one matching pointer and shifts
    // one tail; this intentionally corrupt duplicate proves it is not retain.
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_build_const_order_for_test(vec![20, 10, 20]);
    Simulation::remove_build_const_from_owner(&sim.substrate, &mut sim.houses, 20);
    assert_eq!(sim.houses[&owner].build_const_order(), vec![10, 20]);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_build_const_order_for_test(vec![20, 10]);

    assert_eq!(sim.conceal(20), super::ConcealOutcome::Concealed);
    assert_eq!(sim.houses[&owner].build_const_order(), vec![10]);
    // The pointer expiry stable-removes it from House+68 too (`0x004FBC1C`).
    assert_eq!(house_buildings(&sim, owner), vec![10]);

    // Already-limbo returns before the House expiry callback, so even an
    // intentionally stale fixture entry is untouched by this no-op path.
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .build_const_order_mut_for_test()
        .push(20);
    assert_eq!(sim.conceal(20), super::ConcealOutcome::AlreadyConcealed);
    assert_eq!(sim.houses[&owner].build_const_order(), vec![10, 20]);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .build_const_order_mut_for_test()
        .pop();

    assert!(matches!(
        sim.try_reveal_entity(20, request(8, 2, PlacementEvidence::MarkSucceeded)),
        RevealOutcome::Revealed { .. }
    ));
    assert_eq!(
        sim.houses[&owner].build_const_order(),
        vec![10, 20],
        "successful re-entry appends at the live tail"
    );
    assert_eq!(house_buildings(&sim, owner), vec![10, 20]);
}

/// House+68 in vector order.
fn house_buildings(sim: &Simulation, house: crate::sim::intern::InternedId) -> Vec<u64> {
    sim.houses[&house].base_projection().buildings().to_vec()
}

#[test]
fn naval_build_const_capture_moves_old_entry_to_new_owner_tail() {
    let rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::rules::ini_parser::IniFile::from_str("[BuildingTypes]\n0=GACNST\n"),
    )
    .expect("GACNST rules");
    let mut sim = Simulation::new();
    let old_owner = sim.interner.intern("Americans");
    let new_owner = sim.interner.intern("Russians");
    sim.houses
        .register(HouseState::new(old_owner, 0, None, true, 0, 10));
    sim.houses
        .register(HouseState::new(new_owner, 1, None, false, 0, 10));
    insert_naval_build_const_fixture(&mut sim, 50, new_owner, 8, 8);
    insert_naval_build_const_fixture(&mut sim, 40, old_owner, 2, 2);
    insert_naval_build_const_fixture(&mut sim, 12, old_owner, 4, 2);
    for (stable_id, rx, ry) in [(50, 8, 8), (40, 2, 2), (12, 4, 2)] {
        assert!(matches!(
            sim.try_reveal_entity_with_context(
                stable_id,
                request(rx, ry, PlacementEvidence::MarkSucceeded),
                super::UninitContext::new(&rules),
            ),
            RevealOutcome::Revealed { .. }
        ));
    }
    assert_eq!(sim.houses[&old_owner].build_const_order(), vec![40, 12]);
    assert_eq!(sim.houses[&new_owner].build_const_order(), vec![50]);

    assert_eq!(house_buildings(&sim, old_owner), vec![40, 12]);
    assert_eq!(house_buildings(&sim, new_owner), vec![50]);

    sim.change_owner(40, new_owner);

    assert_eq!(sim.houses[&old_owner].build_const_order(), vec![12]);
    assert_eq!(sim.houses[&new_owner].build_const_order(), vec![50, 40]);
    // ChangeOwner moves House+68 the same way: stable-removed from the old
    // House (`0x00448A78..0x00448AB0`), appended at the new one's tail
    // (`0x00449197..0x004491D2`).
    assert_eq!(house_buildings(&sim, old_owner), vec![12]);
    assert_eq!(house_buildings(&sim, new_owner), vec![50, 40]);
    assert_eq!(sim.substrate.entities.get(40).unwrap().owner(), new_owner);
}

#[test]
fn infantry_raw_owner_remains_the_marking_house_after_live_owner_change() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    let old = sim.substrate.entities.get(1).unwrap().owner();
    sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 192, 64));
    let changed = sim.interner.intern("Russians");
    sim.change_owner(1, changed);
    assert_eq!(sim.substrate.entities.get(1).unwrap().owner(), changed);
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .ground_infantry_owner(3, 4),
        Some(old)
    );
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 4);
}

#[test]
fn walk_first_limbo_releases_head_but_repeated_limbo_preserves_new_claim() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 192, 64));
    let head = crate::sim::components::DriveCoord {
        x: 4 * 256 + 192,
        y: 4 * 256 + 64,
        z: 0,
    };
    let owner = sim.substrate.entities.get(1).unwrap().owner();
    let e = sim.substrate.entities.get_mut(1).unwrap();
    e.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
    e.locomotor.as_mut().unwrap().set_step_head(Some(head));
    crate::sim::movement::walk_head::raw_at(
        &mut sim.substrate.raw_cell_occupation,
        owner,
        head,
        true,
        None,
        None,
    );
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .set_walk_destination(Some(head));
    sim.techno_limbo(1);
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(4, 4) & 0x1c,
        0
    );
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .ground_infantry_owner(4, 4),
        None
    );
    // Foot4DB260 then Locks Walk (75CB30): no destination or head survives
    // to resume after Unlimbo.
    let loco = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .locomotor
        .as_ref()
        .unwrap();
    assert_eq!((loco.step_head(), loco.walk_destination()), (None, None));
    let other = sim.interner.intern("Russians");
    crate::sim::movement::walk_head::raw_at(
        &mut sim.substrate.raw_cell_occupation,
        other,
        head,
        true,
        None,
        None,
    );
    sim.techno_limbo(1);
    sim.uninit(1);
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(4, 4) & 0x1c,
        4
    );
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .ground_infantry_owner(4, 4),
        Some(other)
    );
}

/// `UnitClass::~UnitClass @ 0x00735780` Limbos the Unit itself
/// (`FootClass::Limbo`, `0x0073584F`), so one destroyed while placed leaves
/// its cell.
#[test]
fn unit_destructor_limbos_a_unit_still_on_the_map() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let _ = sim.try_reveal_entity(1, common_raw_request(3, 4, 0, 128, 128));
    assert!(sim.substrate.occupancy.contains_entity(3, 4, 1));

    sim.finalize_and_remove_common(1, super::UninitContext::default(), 1);

    assert!(!sim.substrate.entities.contains(1));
    assert!(!sim.substrate.occupancy.contains_entity(3, 4, 1));
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
}

/// `AircraftClass::~AircraftClass @ 0x00414080` kills its passengers
/// (`KillPassengers(NULL)`, `0x004140E6`): none outlives a carrier destroyed
/// without UnInit.
#[test]
fn aircraft_destructor_kills_its_passengers() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Aircraft);
    insert_entity(&mut sim, 2, EntityCategory::Infantry);
    let mut cargo = PassengerCargo::new(5, 1.0);
    cargo.board_forced_chain(&[(2, 1.0)]);
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

    sim.finalize_and_remove_common(1, super::UninitContext::default(), 1);

    assert!(!sim.substrate.entities.contains(1));
    assert!(
        !sim.substrate
            .entities
            .get(2)
            .unwrap()
            .lifecycle
            .object_alive()
    );
    assert_eq!(sim.substrate.pending_delete.object_ids(), vec![2]);
}

/// `TechnoClass::PointerExpired @ 0x007077C0` removes the expiring object from
/// the listener's cargo only when its `removed` argument is set
/// (`0x007077D8..0x007077E9`). A rider that cloaks sends Detach_All(false) and
/// stays aboard.
#[test]
fn detach_all_false_keeps_a_rider_in_its_transports_cargo() {
    let mut sim = Simulation::new();
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Infantry);
    let _ = sim.reveal(1);
    let mut cargo = PassengerCargo::new(5, 1.0);
    cargo.board_forced_chain(&[(2, 1.0)]);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_passenger_role_for_test(PassengerRole::transport(cargo));
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .set_passenger_role_for_test(PassengerRole::inside_for_test(1, true));

    sim.detach_all(2, false, super::UninitContext::default());

    let transport = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        transport.passenger_role().cargo().unwrap().passengers(),
        &[2]
    );
}

/// The Restore inside the expiry broadcast (`TechnoClass::PointerExpired`
/// `0x00707A51`) cannot fail natively. VERA's preflight can refuse it, here
/// for an archived NavCom that names no object; the broadcast must carry on
/// and still clear the expiring target.
#[test]
fn target_expiry_carries_on_when_the_suspended_mission_cannot_be_restored() {
    let rules = receiver_authority_rules();
    let mut sim = Simulation::with_seed(0x1234);
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    insert_entity(&mut sim, 2, EntityCategory::Unit);
    let _ = sim.try_reveal_entity(2, request(9, 11, PlacementEvidence::MarkSucceeded));

    let listener = sim.substrate.entities.get_mut(1).unwrap();
    listener
        .attack_target
        .set_for_test(Some(TargetKind::Entity(2)));
    listener
        .navigation
        .set_suspended_nav_com_for_test(Some(NavTargetRef::Object { id: 99 }));
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

    sim.uninit_with_rules(2, &rules);

    let listener = sim.substrate.entities.get(1).unwrap();
    assert_eq!(listener.attack_target.target(), None);
    assert!(!sim.substrate.entities.get(2).unwrap().is_object_alive());
}
