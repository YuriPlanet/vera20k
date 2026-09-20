//! Resource economy: harvester ticking and legacy ore-node test utilities.
//!
//! Dispatches to the Miner-component systems (War, Chrono, Slave miners)
//! while preserving the retired node selector for compatibility fixtures.

use crate::rules::ruleset::RuleSet;
use crate::sim::miner::MinerConfig;
use crate::sim::pathfinding;
use crate::sim::world::Simulation;

pub(super) fn tick_resource_economy(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&pathfinding::PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
    // War + Chrono miners no longer tick here: the Harvest mission handler is
    // dispatched per-object from the AI host (techno_ai's Unit arm), at the
    // native Mission_Dispatch position before ground movement.
    let live_order = sim.live_object_order_snapshot();

    // Tick Slave Miner subsystems: slave harvest AI + slave regeneration.
    super::super::slave_miner::tick_slave_harvesters(
        sim,
        &live_order,
        rules,
        config,
        path_grid,
        overlay_registry,
    );
    super::super::slave_miner::tick_slave_regen(sim, &live_order, rules, overlay_registry);
}

pub fn is_harvester_type(rules: &RuleSet, type_id: &str) -> bool {
    rules
        .object_case_insensitive(type_id)
        .is_some_and(|obj| obj.harvester)
}
