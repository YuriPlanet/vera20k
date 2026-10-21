//! `AircraftClass::AI @ 0x00414BB0`'s head clear of `+0x6D2` and the
//! aircraft-Patrol arm after a Restore.
use crate::map::entities::EntityCategory;
use crate::sim::combat::TargetKind;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{FrameEffects, ObjectAiCtx};

fn latch(sim: &crate::sim::world::Simulation, id: u64) -> u8 {
    let plane = sim.substrate.entities.get(id).unwrap();
    plane.mission_leaf.as_aircraft().unwrap().action_latch()
}

/// The entry fixture with an Americans and a Russians HORNET, both houses
/// registered.
fn two_hornets() -> (
    crate::sim::world::Simulation,
    crate::rules::ruleset::RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
    u64,
    u64,
) {
    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture_with_rules("");
    for (index, name) in ["Americans", "Russians"].into_iter().enumerate() {
        let house = sim.interner.intern(name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                house,
                index as i32,
                None,
                index == 0,
                0,
                10,
            ));
    }
    let mut spawn = |owner: &str, rx: u16| {
        sim.spawn_object_with_overlay_registry("HORNET", owner, rx, 15, 0, &rules, &registry)
            .unwrap()
    };
    let plane = spawn("Americans", 15);
    let other = spawn("Russians", 18);
    (sim, rules, registry, plane, other)
}

/// A Guard aircraft retaliates (ReceiveDamage `0x00702B41`, Override(Attack,
/// attacker)), latches `+0x6D2` in its strike, and the attacker's expiry
/// restores Guard (`0x00707A4B`, Restore writes `+0xAC` with no Commence).
/// Ready_To_Commence (`0x0041B5F0`) refuses while the latch holds outside
/// SpyplaneApproach; the next AircraftClass::AI head clears it
/// (`0x00414BD4`), so a queued mission can commence.
#[test]
fn a_latch_left_by_a_restore_clears_at_the_next_ai_head() {
    let (mut sim, rules, registry, plane, attacker) = two_hornets();
    sim.substrate
        .mission_assign_exact(plane, MissionId::from_known(MissionType::Guard), 0)
        .unwrap();
    sim.substrate.mission_commence_exact(plane, 0).unwrap();

    assert!(sim.override_mission_on_damage_response(
        plane,
        attacker,
        &rules,
        FrameEffects::default()
    ));
    // Mission_Attack's strike latches it (`0x004184DC`).
    let entity = sim.substrate.entities.get_mut(plane).unwrap();
    assert_eq!(
        entity.mission.current(),
        MissionId::from_known(MissionType::Attack)
    );
    entity.mission_leaf.set_aircraft_action_latch(true);
    sim.mission_restore_represented(plane, &rules, Some(&registry), FrameEffects::default())
        .unwrap();
    let entity = sim.substrate.entities.get(plane).unwrap();
    assert_eq!(
        entity.mission.current(),
        MissionId::from_known(MissionType::Guard)
    );
    assert_eq!(latch(&sim, plane), 1, "Restore leaves the latch");
    assert!(!sim.mission_ready_to_commence(plane, &rules));

    let ctx = ObjectAiCtx {
        overlay_registry: Some(&registry),
        ..ObjectAiCtx::default()
    };
    sim.object_ai_visit_one(plane, Some(&rules), ctx);
    assert_eq!(latch(&sim, plane), 0, "the AI head clears it");
    assert!(sim.mission_ready_to_commence(plane, &rules));
}

/// The head's mission test over every `+0xAC` around the table's range: only
/// the zero bytes of `0x004151C0` (+0xAC - 1 = 0, 26, 29, 30) keep the latch;
/// `DEC EAX; CMP EAX,0x1E; JA` sends NONE, Sleep and ids past 31 to the clear.
#[test]
fn the_ai_head_keeps_the_latch_only_on_the_tables_four_missions() {
    let kept = [1, 27, 30, 31];
    for raw in -2..=33 {
        let mut plane = crate::sim::game_entity::GameEntity::test_default_of_category(
            1,
            "ORCA",
            "Americans",
            0,
            0,
            EntityCategory::Aircraft,
        );
        plane.mission_leaf =
            crate::sim::mission::leaf::MissionLeafState::aircraft_raw_for_test(1, 1);
        plane
            .mission
            .set_current_for_test(MissionId::from_raw(raw), 0);
        super::ai_head_latch_clear(&mut plane);
        assert_eq!(
            plane.mission_leaf.as_aircraft().unwrap().action_latch(),
            u8::from(kept.contains(&raw)),
            "+0xAC {raw}"
        );
    }
}

/// The detach sweep (`0x0070D4A0`) restores an aircraft's suspended Patrol
/// and then restarts it: Mission+0xBC 0, `+0x6D2` clear
/// (`0x0070D513..0x0070D532`).
#[test]
fn the_detach_sweep_restarts_a_restored_aircraft_patrol() {
    let (mut sim, rules, registry, plane, target) = two_hornets();
    let entity = sim.substrate.entities.get_mut(plane).unwrap();
    entity
        .mission
        .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
            current: MissionId::from_known(MissionType::Attack),
            suspended: MissionId::from_known(MissionType::Patrol),
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 6,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
        });
    entity.mission_leaf.set_aircraft_action_latch(true);
    entity
        .attack_target
        .set_for_test(Some(TargetKind::Entity(target)));

    sim.stop_all_targeting_on_detach(target, &rules, Some(&registry), FrameEffects::default());
    let entity = sim.substrate.entities.get(plane).unwrap();
    assert_eq!(
        entity.mission.current(),
        MissionId::from_known(MissionType::Patrol)
    );
    assert_eq!(entity.mission.handler_state(), 0);
    assert_eq!(latch(&sim, plane), 0);
}
