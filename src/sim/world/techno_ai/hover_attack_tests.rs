//! The HoverAttack re-anchor (`0x004D4DC0..0x004D4E07`,
//! `0x004D51E0..0x004D521F`) through the production Foot dispatcher.

use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::world::{ObjectAiCtx, Simulation};

fn guard_with_target(sim: &mut Simulation, id: u64, target: u64, mission: MissionType) {
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
        .set_for_test(Some(TargetKind::Entity(target)));
}

/// On retail rules a landed Rocketeer (`JUMPJET`, `HoverAttack=yes`) holding
/// a Target takes NearbyLocation as its destination on its Guard dispatch, as
/// on Attack; a rifleman (`E1`) holding one does not, nor a Rocketeer aloft.
#[test]
fn a_landed_rocketeer_holding_a_target_re_anchors_on_guard_and_attack() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    assert!(rules.object("JUMPJET").unwrap().hover_attack);
    assert!(!rules.object("E1").unwrap().hover_attack);
    let mut sim = Simulation::with_seed(5);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    for (name, side) in [("Americans", 0), ("Russians", 1)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, side, None, false, 0, 10));
    }
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let spawn = |sim: &mut Simulation, kind, owner, x, y| {
        sim.spawn_object_at_height(kind, owner, x, y, 0, 0, &rules)
            .unwrap()
    };
    let enemy = spawn(&mut sim, "HTNK", "Russians", 14, 10);
    for mission in [MissionType::Guard, MissionType::Attack] {
        let rocketeer = spawn(&mut sim, "JUMPJET", "Americans", 10, 12);
        let rifleman = spawn(&mut sim, "E1", "Americans", 10, 16);
        let expected = sim
            .techno_nearby_location(rocketeer, Some(rocketeer), &rules)
            .expect("an open arena has a nearby cell");
        for id in [rocketeer, rifleman] {
            guard_with_target(&mut sim, id, enemy, mission);
            crate::sim::world::dispatch_foot_mission(&mut sim, id, &rules, ObjectAiCtx::default());
        }
        let nav_com = |id| sim.substrate.entities.get(id).unwrap().navigation.nav_com();
        assert_eq!(
            nav_com(rocketeer),
            Some(NavTargetRef::cell(expected.0 as u16, expected.1 as u16)),
            "{mission:?}"
        );
        if mission == MissionType::Guard {
            assert_eq!(nav_com(rifleman), None);
        }
    }
    let aloft = spawn(&mut sim, "JUMPJET", "Americans", 20, 20);
    sim.substrate
        .entities
        .get_mut(aloft)
        .unwrap()
        .position
        .set_exact_z_leptons_for_test(Some(500));
    assert_ne!(
        crate::sim::movement::air_movement::current_fly_height(
            sim.substrate.entities.get(aloft).unwrap(),
            sim.resolved_terrain.as_ref(),
        ),
        0
    );
    guard_with_target(&mut sim, aloft, enemy, MissionType::Guard);
    crate::sim::world::dispatch_foot_mission(&mut sim, aloft, &rules, ObjectAiCtx::default());
    assert_eq!(
        sim.substrate
            .entities
            .get(aloft)
            .unwrap()
            .navigation
            .nav_com(),
        None,
        "aloft"
    );
}
