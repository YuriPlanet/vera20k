//! A retail bridge gives real hierarchy zones an edge to record 0, and every
//! retail miner searches with a threat coefficient: the online-match failure
//! `hierarchy level2 zone0 lacks native threat seed` through production owners.

use crate::rules::locomotor_type::MovementZone;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::world::{FrameEffects, TickLane};

/// C1A02MD: the cell beside the bridge (166,146)..(177,146) and a far ground goal.
const MINER_CELL: (u16, u16) = (165, 147);
const GOAL_CELL: (u16, u16) = (96, 26);

#[test]
#[ignore = "requires the retail install: loads C1A02MD.MAP"]
fn retail_miner_beside_a_bridge_searches_through_record_zero_edge() {
    let Some((retail, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let profile: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/map_observation.building-opening.example.json",
    ))
    .unwrap();
    let mut launch = profile["launch"].clone();
    launch["selected_map_file"] = "C1A02MD.MAP".into();
    let launch = serde_json::from_value(launch).unwrap();
    let descriptor =
        crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(launch).unwrap();
    let mut scene =
        crate::headless_scenario::load_with_launch(&retail, "C1A02MD.MAP", 1, descriptor).unwrap();
    let runtime = &mut scene.runtime;
    let rules = runtime.resources.rules.clone();

    let sim = &mut runtime.simulation;
    let zones = sim.zone_grid.as_ref().unwrap();
    let start_zone = zones.hierarchy_zone_at_native(2, MINER_CELL).unwrap();
    let level2 = zones
        .hierarchy_for(MovementZone::Normal)
        .unwrap()
        .level(2)
        .unwrap();
    assert!(
        level2
            .edges(start_zone)
            .iter()
            .any(|edge| edge.neighbor == 0)
    );
    let house = sim.session.current_house().unwrap();
    let owner = sim.resolve(house).to_string();
    let miner = sim
        .spawn_object("HARV", &owner, MINER_CELL.0, MINER_CELL.1, 0, &rules)
        .unwrap();
    let tick = sim.session.tick();
    sim.queue_commands(vec![CommandEnvelope::new(
        house,
        tick,
        Command::Move {
            entity_id: miner,
            target_rx: GOAL_CELL.0,
            target_ry: GOAL_CELL.1,
        },
    )]);

    for _ in 0..60 {
        let due = runtime.simulation.take_due_commands();
        runtime
            .advance_frame(&due, 66, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
    }
    let position = &runtime.simulation.entities().get(miner).unwrap().position;
    assert_ne!((position.rx(), position.ry()), MINER_CELL);
}
