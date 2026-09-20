//! Live world inputs for the shared Drive/Ship Can_Enter_Cell query.
//!
//! Fresh4B34C0/6A2B0F and chain4B1C3E reach the same Unit+1AC predicate.
//! Mark brackets, caller train/crusher coercions and response tables remain
//! with those callers. Each invocation rebuilds its facts from the live world;
//! a previous candidate's classification is never reused.

use super::MoverSnapshot;
use super::locomotor::MovementLayer;
use super::movement_occupancy::{
    RuntimeCanEnterCellArgs, build_live_building_entry_skip_map,
    evaluate_runtime_can_enter_cell_with_transition,
};
use super::movement_tick::{TrackEntryQuery, classify_track_entry, snapshot_mover};
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::cell_entry::{CellEntryResult, WallArmTables};
use crate::sim::world::Simulation;

/// One query's semantic result and split-layer arguments. The mover facts are
/// returned for the immediate response (notably the code3 gate receiver), so
/// that response does not take another snapshot or classify after Mark1.
pub(super) struct TrackEntryEvaluation {
    pub result: CellEntryResult,
    pub query: TrackEntryQuery,
    pub mover: MoverSnapshot,
}

impl Simulation {
    /// Evaluate one native track-entry call against current world state.
    ///
    /// `effective_height` is the caller's actual argument: fresh movement
    /// retains it across its two candidate calls, while chain derives it from
    /// its live owner at its own call site. Do not replace it with candidate Z
    /// or reload it here. Bridge-transition state changes at this query, before
    /// the shared classifier walks terrain, ordered objects and raw occupation.
    ///
    /// None means the actor disappeared before a query could be made, not a
    /// native entry code. A caller owns any lifecycle/refusal response.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn query_track_entry(
        &mut self,
        id: u64,
        candidate: DriveCoord,
        direction: i8,
        effective_height: i16,
        rules: Option<&RuleSet>,
        fallback_grid: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Option<TrackEntryEvaluation> {
        let mover = snapshot_mover(
            &self.substrate.entities,
            id,
            self.playfield_bounds,
            Some(&self.type_handles),
            rules,
        )?;
        let grid = self.path_grid.as_deref().or(fallback_grid);
        // Original candidate packing truncates signed leptons toward zero;
        // the existing cell-query substrate carries the resulting words.
        let target_cell = ((candidate.x / 256) as u16, (candidate.y / 256) as u16);
        let entity = self.substrate.entities.get_mut(id)?;
        let layer = if entity.on_bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        let entry = evaluate_runtime_can_enter_cell_with_transition(
            grid,
            layer,
            &mut entity.runtime_bridge_transition,
            entity.on_bridge,
            RuntimeCanEnterCellArgs::runtime(target_cell, direction, effective_height),
        );
        let query = TrackEntryQuery {
            target_cell,
            layers: entry.layers,
            bridge_traversal_allowed: entry.bridge_traversal_allowed,
        };
        let skips =
            build_live_building_entry_skip_map(&self.substrate.entities, id, &self.interner, rules);
        let result = classify_track_entry(
            query,
            id,
            &mover,
            grid,
            self.resolved_terrain.as_ref(),
            mover
                .speed_type
                .and_then(|speed| self.terrain_costs.get(&speed)),
            Some(WallArmTables {
                overlay_grid: self.overlay_grid.as_ref(),
                overlay_registry: registry,
                alliances: Some(&self.house_alliances),
                interner: Some(&self.interner),
            }),
            &self.substrate.occupancy,
            &self.substrate.cell_occupation,
            &self.substrate.raw_cell_occupation,
            self.session.binary_frame,
            &skips,
            &self.substrate.entities,
            &self.house_alliances,
            &self.interner,
        );
        Some(TrackEntryEvaluation {
            result,
            query,
            mover,
        })
    }
}
