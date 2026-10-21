//! UnitClass Mission_Harvest73E5E0, selected by the common Foot/Unit
//! mission dispatcher on committed Harvest10. MissionCom owns the FSM cursor;
//! this body commits its effects and returns the delay to the dispatcher's
//! single MissionClass5B3060 timer epilogue.
//!
//! Native executable rows: tools/spatial_oracle/harvest_field.json and
//! refinery_dock.json. Productive states return one frame; other search/return
//! exits use the existing Rate + Scenario RandomRanged(0,2) owner. A scan miss
//! returns 105 without drawing. The slave-master prologue73E5E9 runs returned
//! slaves before its Rate tail; a non-harvester returns450 at73E62F.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

// Read only by the debug-build cursor assertion below.
#[cfg(debug_assertions)]
use super::MinerState;
use super::miner_system::{
    MinerSnapshot, build_miner_snapshot, commit_miner_snapshot, process_miner,
};

/// One Unit Mission_Harvest73E5E0 call. Timer admission and its epilogue
/// belong to the common dispatcher, not this handler.
pub(crate) fn mission_harvest(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    id: u64,
    frame_effects: FrameEffects<'_>,
) -> Option<i32> {
    let entity = sim.substrate.entities.get(id)?;
    if entity.death.is_dying() {
        return None;
    }
    // The slave-master arm precedes the real-harvester gate in original
    //73E5E9..73E62F. A Slave Miner component is only an order marker.
    let slave_master = entity.slave_manager.is_some()
        && sim
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| object.resource_destination && object.resource_gatherer);
    if slave_master {
        sim.handle_returned_slaves(id, rules, frame_effects);
        return Some(Simulation::mission_rate_epilogue_for(
            &mut sim.scenario_rng,
            &sim.substrate,
            rules,
            id,
            crate::sim::mission::MissionType::Harvest,
        ));
    }
    if !entity.is_harvester() {
        return Some(450);
    }
    let mut snap = build_miner_snapshot(sim, id)?;
    harvest_mission_step(sim, rules, overlay_registry, &mut snap, frame_effects);
    commit_miner_snapshot(&mut sim.substrate, &snap);
    Some(snap.dispatch_delay)
}

fn harvest_mission_step(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
    frame_effects: FrameEffects<'_>,
) {
    // Cursor sanity (debug-only, never hashed): the working cursor must have
    // decoded from the entity's handler state — pins the cursor round-trip the
    // substate-authority flip relies on.
    #[cfg(debug_assertions)]
    if let Some(entity) = sim.substrate.entities.get(snap.entity_id) {
        debug_assert_eq!(
            MinerState::from_cursor(entity.mission.handler_state())
                .unwrap_or(MinerState::SearchOre),
            snap.state,
            "Harvest dispatch entry: entity {} working cursor must equal the \
             decoded MissionCom.handler_state",
            snap.entity_id,
        );
    }

    process_miner(sim, rules, overlay_registry, snap, frame_effects);
}
