//! Guard's computer demolition arm (`0x004D523A..0x004D52A9`) and
//! AreaGuard's (`0x004D6DBE..0x004D6E44`) through the Foot dispatcher.

use crate::sim::combat::TargetKind;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::world::{ObjectAiCtx, Simulation};

/// On retail rules a computer's Tanya (`TANY`, `C4=yes`) guarding with an
/// enemy building as her Target queues Sabotage on either guard dispatch; a
/// human player's Tanya and a computer's rifleman (`E1`) do not.
#[test]
fn a_computer_tanya_guarding_with_a_building_target_queues_sabotage() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = retail.rules;
    assert!(rules.object("TANY").unwrap().c4);
    assert!(!rules.object("E1").unwrap().c4);
    let mut sim = Simulation::with_seed(9);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    for (name, side, human) in [("Americans", 0, true), ("Russians", 1, false)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, side, None, human, 0, 10));
    }
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let mut spawn = |kind, owner, x, y| {
        sim.spawn_object_at_height(kind, owner, x, y, 0, 0, &rules)
            .unwrap()
    };
    let barracks = spawn("NAHAND", "Americans", 20, 20);
    let computer_tanya = spawn("TANY", "Russians", 10, 10);
    let rifleman = spawn("E1", "Russians", 10, 14);
    let human_tanya = spawn("TANY", "Americans", 14, 10);
    let mut queued = |id, mission| {
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(mission),
            queued: MissionId::NONE,
            suspended: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        entity
            .attack_target
            .set_for_test(Some(TargetKind::Entity(barracks)));
        crate::sim::world::dispatch_foot_mission(&mut sim, id, &rules, ObjectAiCtx::default());
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .mission
            .queued()
            .known()
    };
    for mission in [MissionType::Guard, MissionType::AreaGuard] {
        assert_eq!(queued(computer_tanya, mission), Some(MissionType::Sabotage));
        assert_eq!(queued(rifleman, mission), None);
        assert_eq!(queued(human_tanya, mission), None);
    }
}

/// AreaGuard returns before its leash, Approach and cadence draw. A Tanya
/// far from her retained post keeps the building Target, queues rather than
/// commences Sabotage, and lets MissionClass's one timer epilogue write 1.
#[test]
fn area_guard_sabotage_precedes_the_leash_and_draws_nothing() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 9, 64);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let building = sim
        .spawn_object_at_height("GAPOWR", "Americans", 16, 10, 0, 0, &rules)
        .unwrap();
    let tanya = sim
        .spawn_object_at_height("TANY", "Russians", 10, 10, 0, 0, &rules)
        .unwrap();
    let post = TargetKind::Cell(60, 60);
    let target = TargetKind::Entity(building);
    let now = sim.session.binary_frame();
    let entity = sim.substrate.entities.get_mut(tanya).unwrap();
    entity.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_known(MissionType::AreaGuard),
        queued: MissionId::NONE,
        suspended: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: now,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(now),
    });
    entity.set_archive_target(Some(post));
    entity.attack_target.set_for_test(Some(target));
    let before = sim.rng_state();

    crate::sim::world::dispatch_foot_mission(&mut sim, tanya, &rules, ObjectAiCtx::default());

    let entity = sim.substrate.entities.get(tanya).unwrap();
    assert_eq!(
        entity.mission.current().known(),
        Some(MissionType::AreaGuard)
    );
    assert_eq!(entity.mission.queued().known(), Some(MissionType::Sabotage));
    assert_eq!(entity.mission.dispatch_timer().delay(), 1);
    assert_eq!(entity.attack_target.target(), Some(target));
    assert_eq!(entity.navigation.nav_com(), None);
    assert_eq!(entity.archive_target(), Some(post));
    assert_eq!(sim.rng_state(), before);
}
