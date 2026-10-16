//! Navigation cache rebuilds read one explicit live map and bridge view.
//!
//! The cache borrows never move gameplay authority out of Simulation. Resident
//! world rebuilds and synchronous receiver rebuilds share the same policy.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::sim::bridge_state::BridgeRuntimeState;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::StringInterner;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::zone_map::ZoneGrid;

/// The cells one marked structure blocks for movement: its base foundation
/// with the bib edge relaxed. Every structure-presence reader uses this rule.
///
/// Native: Techno enter/exit (0x005683C0 / 0x005687F0) call CellClass
/// AddContent/RemoveContent (0x0047E8A0 / 0x0047EA90), which mark/clear
/// occupation; see docs/research/bridges/02-cell-state-layering-zones/
/// BRIDGE_OCCUPANCY_OBJECT_LISTS_GHIDRA_REPORT.md. Held factory objects and
/// retained attached upgrades have no independent marked footprint. A dying
/// structure still blocks until the lifecycle owner unmarks it.
pub(super) fn marked_structure_movement_cells(
    entity: &GameEntity,
    interner: &StringInterner,
    rules: &RuleSet,
) -> Vec<(u16, u16)> {
    if entity.category() != EntityCategory::Structure || !entity.lifecycle.cell_marked() {
        return Vec::new();
    }
    let object_type = interner.object_type(entity.type_ref(), rules);
    let foundation = object_type
        .map(|object| object.foundation.as_str())
        .unwrap_or("1x1");
    let has_bib = object_type.is_some_and(|object| object.bib);
    let foundation_cells = crate::sim::production::building_base_foundation_cells(
        entity.position.rx(),
        entity.position.ry(),
        foundation,
    );
    crate::sim::production::building_movement_blocking_cells(&foundation_cells, has_bib)
}

/// Structure presence over every marked structure. Consumers are
/// order-insensitive (cell blocks, sets and presence tests).
fn visit_structure_movement_cells(
    entities: &EntityStore,
    interner: &StringInterner,
    rules: &RuleSet,
    mut visit: impl FnMut((u16, u16)),
) {
    for entity in entities.values() {
        for cell in marked_structure_movement_cells(entity, interner, rules) {
            visit(cell);
        }
    }
}

/// Every cell a marked structure blocks for movement.
pub(super) fn structure_movement_cells(
    entities: &EntityStore,
    interner: &StringInterner,
    rules: &RuleSet,
) -> BTreeSet<(u16, u16)> {
    let mut cells = BTreeSet::new();
    visit_structure_movement_cells(entities, interner, rules, |cell| {
        cells.insert(cell);
    });
    cells
}

/// The requested cells a marked structure blocks, from one structure scan.
pub(super) fn structure_blocked_among(
    entities: &EntityStore,
    interner: &StringInterner,
    rules: &RuleSet,
    cells: &[(u16, u16)],
) -> BTreeSet<(u16, u16)> {
    let mut blocked = BTreeSet::new();
    if cells.is_empty() {
        return blocked;
    }
    let requested: BTreeSet<(u16, u16)> = cells.iter().copied().collect();
    visit_structure_movement_cells(entities, interner, rules, |marked| {
        if requested.contains(&marked) {
            blocked.insert(marked);
        }
    });
    blocked
}

pub(super) struct NavigationCaches<'a> {
    pub(super) zones: &'a mut Option<ZoneGrid>,
    pub(super) path: &'a mut Option<Arc<PathGrid>>,
    /// Current operation's world bounds; this borrowed view is never retained.
    pub(super) playfield_bounds: Option<crate::map::playfield::PlayfieldBounds>,
}

impl NavigationCaches<'_> {
    /// Ordinary movement supplies structure presence from this cell's live
    /// list. It must not scan every object/footprint for every paid crossing.
    pub(super) fn publish_recalculated_cell_with_presence(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        coord: (u16, u16),
        structure_blocked: bool,
    ) -> Result<(), String> {
        let cell = terrain
            .cell(coord.0, coord.1)
            .ok_or("Recalc cell is outside terrain")?;
        if let Some(zones) = self.zones.as_mut() {
            zones.refresh_base_cell_attributes_at(terrain, coord.0, coord.1);
        }
        if let Some(path) = self.path.as_mut() {
            if path.width() != terrain.width() || path.height() != terrain.height() {
                return Err("path dimensions differ from terrain".into());
            }
            if !path.resolved_cell_is_current(cell, structure_blocked)
                && !Arc::make_mut(path).refresh_resolved_cell(cell, structure_blocked)
            {
                return Err("current path cell could not be published".into());
            }
        }
        Ok(())
    }

    /// Returns the structure cells it blocked, for the caller's derived cache.
    pub(super) fn rebuild_dynamic(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        bridges: Option<&BridgeRuntimeState>,
        entities: &EntityStore,
        interner: &StringInterner,
        rules: &RuleSet,
    ) -> Result<BTreeSet<(u16, u16)>, String> {
        let mut grid = PathGrid::from_resolved_terrain_with_bridges(terrain);

        let structure_cells = structure_movement_cells(entities, interner, rules);
        for &(x, y) in &structure_cells {
            grid.block_structure_cell(x, y);
        }

        if let Some(zones) = self.zones.as_mut()
            && zones.is_native_load_pending()
        {
            zones.finish_native_load(
                &grid,
                terrain,
                bridges
                    .map(BridgeRuntimeState::endpoint_records)
                    .unwrap_or(&[]),
                self.playfield_bounds,
            )?;
            *self.path = Some(Arc::new(grid));
        } else {
            self.rebuild_zones(&grid, terrain, bridges);
        }
        Ok(structure_cells)
    }

    /// Publish the path view of one completed47D2B0 Recalc before another
    /// bridge callback reads them. Zone IDs and hierarchy remain owned by the
    /// separate56C510/586990 callbacks. Height/slope changes require publication
    /// even when the narrower overlay passability receipt reports no change.
    pub(super) fn publish_recalculated_cell(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        entities: &EntityStore,
        interner: &StringInterner,
        rules: &RuleSet,
        coord: (u16, u16),
    ) -> Result<(), String> {
        self.publish_recalculated_cells(terrain, entities, interner, rules, &[coord])
    }

    /// Batch form of [`Self::publish_recalculated_cell`]: one structure scan
    /// supplies every cell's presence, then each cell publishes in order.
    pub(super) fn publish_recalculated_cells(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        entities: &EntityStore,
        interner: &StringInterner,
        rules: &RuleSet,
        cells: &[(u16, u16)],
    ) -> Result<(), String> {
        let blocked = structure_blocked_among(entities, interner, rules, cells);
        for &coord in cells {
            self.publish_recalculated_cell_with_presence(terrain, coord, blocked.contains(&coord))?;
        }
        Ok(())
    }

    /// Current Cell/path view only. Native ramp repair's raw +11B writes can
    /// precede a count30 abort, with no Recalc or cached class/height publication.
    /// Keep the same path projection as Recalc without changing zones.
    pub(super) fn publish_current_path_cell(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        entities: &EntityStore,
        interner: &StringInterner,
        rules: &RuleSet,
        coord: (u16, u16),
    ) -> Result<(), String> {
        let cell = terrain
            .cell(coord.0, coord.1)
            .ok_or("path cell is outside terrain")?;
        // Cache owners are optional during loading/headless execution. Refresh
        // installed views without inventing a zone rebuild or eager map load.
        if let Some(path) = self.path.as_mut() {
            if path.width() != terrain.width() || path.height() != terrain.height() {
                return Err("path dimensions differ from terrain".into());
            }
            let mut structure_blocked = false;
            visit_structure_movement_cells(entities, interner, rules, |marked| {
                structure_blocked |= marked == coord;
            });
            if !Arc::make_mut(path).refresh_resolved_cell(cell, structure_blocked) {
                return Err("current path cell could not be published".into());
            }
        }
        Ok(())
    }

    pub(super) fn rebuild_zones(
        &mut self,
        path_grid: &PathGrid,
        terrain: &ResolvedTerrainGrid,
        bridges: Option<&BridgeRuntimeState>,
    ) {
        let records = bridges
            .map(BridgeRuntimeState::endpoint_records)
            .unwrap_or(&[]);
        let geometry = bridges.and_then(BridgeRuntimeState::native_zone_source_size);
        if self
            .zones
            .as_ref()
            .is_some_and(|zones| !zones.bridge_inputs_match(records, geometry))
        {
            self.rebuild_zones_full(path_grid, terrain, bridges);
            return;
        }
        // Reuse the zones only when nothing changed: a changed batch carries no
        // Assign-vs-Merge provenance, so it rebuilds in full (exact one-cell
        // repairs go through `repair_zone_cell` at their mutation owners).
        if let (Some(prev), Some(zones)) = (self.path.as_deref(), self.zones.as_ref())
            && prev
                .diff_cells(path_grid)
                .is_some_and(|changed| changed.is_empty())
            && zones.movement_classes_match(terrain)
        {
            // PathGrid does not carry CellClass reduced zone type.
            // Both path state and retained base classes must match.
            *self.path = Some(Arc::new(path_grid.clone()));
            return;
        }
        self.rebuild_zones_full(path_grid, terrain, bridges);
    }

    /// Bypass reuse when a changed reduced zone type needs a full rebuild even
    /// though boolean walkability remains identical (e.g. OccupationBits=0).
    pub(super) fn rebuild_zones_full(
        &mut self,
        path_grid: &PathGrid,
        terrain: &ResolvedTerrainGrid,
        bridges: Option<&BridgeRuntimeState>,
    ) {
        *self.zones = Some(ZoneGrid::build_with_native_map_context(
            path_grid,
            terrain,
            bridges
                .map(BridgeRuntimeState::endpoint_records)
                .unwrap_or(&[]),
            bridges.and_then(BridgeRuntimeState::native_zone_source_size),
            self.playfield_bounds,
        ));
        *self.path = Some(Arc::new(path_grid.clone()));
    }
}
