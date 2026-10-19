//! Mission_Guard's computer demolition arm (`0x004D523A..0x004D52A9`)
//! through the production Foot dispatcher.

use crate::sim::combat::TargetKind;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::world::{ObjectAiCtx, Simulation};

/// On retail rules a computer's Tanya (`TANY`, `C4=yes`) guarding with an
/// enemy building as her Target queues Sabotage on her Guard dispatch; a
/// human player's Tanya and a computer's rifleman (`E1`) do not.
#[test]
fn a_computer_tanya_guarding_with_a_building_target_queues_sabotage() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    assert!(rules.object("TANY").unwrap().c4);
    assert!(!rules.object("E1").unwrap().c4);
    let mut sim = Simulation::with_seed(9);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    for (name, side, human) in [("Americans", 0, true), ("Russians", 1, false)] {
        let id = sim.interner.intern(name);
        sim.houses
            .insert(id, HouseState::new(id, side, None, human, 0, 10));
        sim.session.house_order.push(id);
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
    let mut queued = |id| {
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Guard),
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
    assert_eq!(queued(computer_tanya), Some(MissionType::Sabotage));
    assert_eq!(queued(rifleman), None);
    assert_eq!(queued(human_tanya), None);
}
