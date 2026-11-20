//! Phobos `Powered.KillSpawns` through the production operational edge
//! (`BuildingClass::AI` hook `0x43FBEF`, `Ext/Building/Hooks.cpp:30-37`):
//! a building that falls offline resets its spawn manager and kills the
//! children that are attacking or coming home.

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::power_system::PowerState;
use crate::sim::spawn_manager::SpawnSlotState;
use crate::sim::world::{FrameEffects, Simulation};

/// A powered 1x1 building with a three-child aircraft pool. `key` is the
/// extra BuildingType line under test.
fn scene(key: &str) -> (Simulation, RuleSet, u64) {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[InfantryTypes]\n[VehicleTypes]\n\
         [AircraftTypes]\n0=HORN\n\
         [BuildingTypes]\n0=PB\n\
         [PB]\nName=Spawner\nStrength=500\nArmor=concrete\nFoundation=1x1\n\
         Powered=yes\nPower=-100\nSpawns=HORN\nSpawnsNumber=3\n\
         SpawnRegenRate=100\nSpawnReloadRate=0\n{key}\
         [HORN]\nName=Hornet\nStrength=75\nArmor=light\nSpeed=12\n\
         Locomotor={{4A582746-9839-11d1-B709-00A024DDAFD1}}\n\
         MovementZone=Fly\nSpawned=yes\nAmmo=1\n\
         [Warheads]\n0=HIT\n\
         [HIT]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [CombatDamage]\nC4Warhead=HIT\n"
    )))
    .expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x0D_FB_EF);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let building = sim
        .spawn_object("PB", "Americans", 10, 10, 0, &rules)
        .expect("building");
    (sim, rules, building)
}

/// The building's house has no output, so its `Power=-100` drain leaves it
/// below full power. The existing sample is operational, making the next
/// visit an offline edge.
fn fall_offline(sim: &mut Simulation, building: u64) {
    let owner = sim
        .substrate
        .entities
        .get(building)
        .expect("building")
        .owner();
    let mut state = PowerState::default();
    state.set_total_drain_for_test(100);
    state.set_total_output_for_test(0);
    sim.power_states.map_mut_for_test().insert(owner, state);
    sim.substrate
        .entities
        .get_mut(building)
        .expect("building")
        .building.building_last_operational
        .set_for_test(true);
}

fn stay_powered(sim: &mut Simulation, building: u64) {
    let owner = sim
        .substrate
        .entities
        .get(building)
        .expect("building")
        .owner();
    let mut state = PowerState::default();
    state.set_total_drain_for_test(100);
    state.set_total_output_for_test(200);
    sim.power_states.map_mut_for_test().insert(owner, state);
    sim.substrate
        .entities
        .get_mut(building)
        .expect("building")
        .building.building_last_operational
        .set_for_test(false);
}

/// Slot states `Attacking`, `ComingHome`, `InFlight`, with every child out
/// of limbo and the manager holding a target.
fn arm_pool(sim: &mut Simulation, building: u64) -> [u64; 3] {
    for state in [
        SpawnSlotState::Attacking,
        SpawnSlotState::ComingHome,
        SpawnSlotState::InFlight,
    ] {
        let manager = sim
            .substrate
            .entities
            .get_mut(building)
            .expect("building")
            .spawn_manager
            .as_mut()
            .expect("spawn manager");
        let index = manager
            .slots()
            .iter()
            .position(|slot| slot.state() == SpawnSlotState::ReadyDocked)
            .expect("a docked child");
        manager.slots_mut_for_test()[index].set_state_for_test(state);
    }
    let children: [u64; 3] = {
        let manager = sim
            .substrate
            .entities
            .get(building)
            .expect("building")
            .spawn_manager
            .as_ref()
            .expect("spawn manager");
        std::array::from_fn(|index| manager.slots()[index].spawn().expect("child"))
    };
    for child in children {
        let entity = sim.substrate.entities.get_mut(child).expect("child");
        entity.lifecycle.set_in_limbo_for_test(false);
        entity.lifecycle.set_cell_marked_for_test(true);
    }
    sim.substrate
        .entities
        .get_mut(building)
        .expect("building")
        .spawn_manager
        .as_mut()
        .expect("spawn manager")
        .set_target(Some(TargetKind::Entity(building)));
    children
}

fn health(sim: &Simulation, id: u64) -> Option<i32> {
    sim.substrate
        .entities
        .get(id)
        .map(|entity| entity.health.current())
}

#[test]
fn an_offline_building_kills_its_attacking_and_returning_children() {
    let (mut sim, rules, building) = scene("Powered.KillSpawns=yes\n");
    let children = arm_pool(&mut sim, building);
    fall_offline(&mut sim, building);
    assert_eq!(
        sim.building_operational_state(building, &rules),
        Some(false),
        "the fixture building is offline"
    );
    sim.visit_building_operational(building, &rules, None, FrameEffects::default());
    for child in [children[0], children[1]] {
        assert!(
            health(&sim, child).is_none_or(|health| health == 0),
            "an attacking or returning child takes its whole Health"
        );
    }
    assert_eq!(
        health(&sim, children[2]),
        Some(75),
        "an in-flight child is not on the kill list"
    );
    let entity = sim.substrate.entities.get(building).expect("building");
    assert!(
        entity.spawn_manager.as_ref().is_some_and(|manager| {
            manager.current_target().is_none() && manager.queued_target().is_none()
        }),
        "ResetTarget precedes the node walk"
    );
    assert!(
        !entity.building.building_last_operational.is_operational(),
        "43FBEF stores the offline sample after the hook"
    );
}

#[test]
fn the_key_and_the_offline_edge_are_both_required() {
    // No key: the manager keeps its target and every child survives.
    let (mut sim, rules, building) = scene("");
    let children = arm_pool(&mut sim, building);
    fall_offline(&mut sim, building);
    sim.visit_building_operational(building, &rules, None, FrameEffects::default());
    for child in children {
        assert_eq!(health(&sim, child), Some(75), "the key is off");
    }
    assert!(
        sim.substrate
            .entities
            .get(building)
            .expect("building")
            .spawn_manager
            .as_ref()
            .is_some_and(|manager| manager.queued_target().is_some()),
        "no reset without the key"
    );

    // Key on, but the building comes online: the changed edge runs without
    // the kill arm.
    let (mut sim, rules, building) = scene("Powered.KillSpawns=yes\n");
    let children = arm_pool(&mut sim, building);
    stay_powered(&mut sim, building);
    assert_eq!(sim.building_operational_state(building, &rules), Some(true));
    sim.visit_building_operational(building, &rules, None, FrameEffects::default());
    for child in children {
        assert_eq!(health(&sim, child), Some(75), "the building is online");
    }
    assert!(
        sim.substrate
            .entities
            .get(building)
            .expect("building")
            .building.building_last_operational
            .is_operational(),
        "the online sample is stored"
    );
}
