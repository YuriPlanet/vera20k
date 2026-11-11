//! Building NoInit43B680 restarts the independent repair Stage after player load.
use super::*;
use crate::sim::combat::TargetKind;
use crate::sim::command::Command;
use crate::sim::house_state::HouseState;
use crate::sim::mission::MissionType;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::world::{FrameEffects, TickLane};

fn step(runtime: &mut SimRuntime) {
    let output = runtime
        .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
        .expect("ordinary repair frame");
    assert!(output.tick.frame_committed);
}

fn paid_health(runtime: &SimRuntime, tank: u64) -> (i32, i32, i32) {
    let unit = runtime.simulation.entities().get(tank).unwrap();
    let wallet = runtime.simulation.houses()[&unit.owner()].economy();
    (
        unit.health.current(),
        wallet.credits(),
        wallet.spent_credits(),
    )
}

#[test]
fn player_load_restarts_the_paid_depot_repair_interval() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let strength = rules.object("MTNK").unwrap().strength;
    let mut sim = load_fixture_simulation(true);
    for (name, side) in [("Americans", 0), ("Russians", 1)] {
        let owner = sim.intern(name);
        sim.houses_mut_for_test()
            .register(HouseState::new(owner, side, None, true, 5000, 10));
    }
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    *sim.overlay_grid_mut_for_test() = Some(OverlayGrid::new(32, 32));
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    // Keep the match running while the player's tank is inside its depot.
    sim.spawn_object("NAPOWR", "Russians", 3, 3, 0, &rules)
        .unwrap();
    let depot = sim
        .spawn_object("GADEPT", "Americans", 20, 16, 0, &rules)
        .unwrap();
    let tank = sim
        .spawn_object("MTNK", "Americans", 13, 16, 0, &rules)
        .unwrap();
    sim.entities_mut_for_test()
        .get_mut(tank)
        .unwrap()
        .health
        .set_for_test(strength / 2);
    assert!(sim.apply_command(
        "Americans",
        &Command::MegaMission {
            entity_id: tank,
            mission: MissionType::Enter,
            target: None,
            destination: Some(TargetKind::Entity(depot)),
        },
        Some(&rules),
    ));
    let mut resources = SimResources::new(rules);
    resources.terrain_template = sim.resolved_terrain().cloned();
    let mut runtime = SimRuntime {
        simulation: sim,
        resources: resources.into(),
    };

    // Capture an actual completed frame immediately before the second paid
    // repair. The production mission/Drive establishes all progress and links.
    let mut first_repair = false;
    let mut saved = None;
    for _ in 0..1800 {
        let before = paid_health(&runtime, tank);
        let bytes =
            first_repair.then(|| snapshot_bytes(&runtime.simulation, &runtime.resources.rules));
        step(&mut runtime);
        let after = paid_health(&runtime, tank);
        if after.0 > before.0 {
            assert!(after.1 < before.1 && after.2 > before.2);
            if let Some(bytes) = bytes {
                saved = Some((bytes, before));
                break;
            }
            first_repair = true;
        }
    }
    let (bytes, before) = saved.expect("Enter must reach two paid repair visits");
    let prepared = PreparedLoad::prepare_candidate(
        &bytes,
        Some(&runtime.simulation),
        Some(LOAD_FIXTURE_MAP_HASH),
        Some(&runtime.resources.rules),
        runtime.resources.terrain_template.as_ref(),
        Some(&runtime.resources.overlay_registry),
    )
    .expect("prepare the real mid-repair state");
    prepared.commit_into(&mut runtime);
    assert_eq!(paid_health(&runtime, tank), before);
    let building = runtime.simulation.entities().get(depot).unwrap();
    assert_eq!(
        building.mission.current().known(),
        Some(MissionType::Repair)
    );
    assert_eq!(building.mission.handler_state(), 2);
    assert!(building.has_live_contact_with(tank));
    let unit = runtime.simulation.entities().get(tank).unwrap();
    assert!(unit.has_live_contact_with(depot));
    assert!(!unit.locomotor.as_ref().unwrap().is_powered());

    // Load453ECF -> NoInit43B68D..43B6C3 reconstructs repair progress;
    // Repair44BC18/44BC35 first advances the new Stage, below the retail
    // URepairRate threshold. It cannot inherit the imminent paid visit.
    step(&mut runtime);
    assert_eq!(
        paid_health(&runtime, tank),
        before,
        "player load must restart the interval, without healing or charging immediately"
    );
    for _ in 0..1800 {
        step(&mut runtime);
        let unit = runtime.simulation.entities().get(tank).unwrap();
        if unit.health.current() == strength
            && !unit.has_live_contact_with(depot)
            && !runtime
                .simulation
                .entities()
                .get(depot)
                .unwrap()
                .has_live_contact_with(tank)
            && unit.locomotor.as_ref().unwrap().is_powered()
        {
            break;
        }
    }
    assert_eq!(paid_health(&runtime, tank).0, strength);
    assert!(paid_health(&runtime, tank).1 < before.1);
    let unit = runtime.simulation.entities().get(tank).unwrap();
    assert!(!unit.has_live_contact_with(depot));
    assert!(unit.locomotor.as_ref().unwrap().is_powered());
    assert!(
        !runtime
            .simulation
            .entities()
            .get(depot)
            .unwrap()
            .has_live_contact_with(tank)
    );
}
