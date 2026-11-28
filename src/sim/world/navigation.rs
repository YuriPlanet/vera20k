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
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::zone_map::ZoneGrid;
use crate::util::forked::Forked;

/// The cells one marked structure blocks for movement: its base foundation
/// with the bib edge relaxed. Every structure-presence reader uses this rule.
///
/// Native: Techno enter/exit (0x005683C0 / 0x005687F0) call CellClass
/// AddContent/RemoveContent (0x0047E8A0 / 0x0047EA90), which mark/clear
/// occupation; see docs/research/bridges/02-cell-state-layering-zones/
/// BRIDGE_OCCUPANCY_OBJECT_LISTS_GHIDRA_REPORT.md. Held factory objects and
/// retained attached upgrades have no independent marked footprint. A dying
/// structure still blocks until the lifecycle owner unmarks it.
fn marked_structure_movement_cells<'a>(
    entity: &'a GameEntity,
    interner: &StringInterner,
    rules: &'a RuleSet,
) -> impl Iterator<Item = (u16, u16)> + 'a {
    let marked = entity.category() == EntityCategory::Structure && entity.lifecycle.cell_marked();
    marked
        .then(|| {
            let object_type = interner.object_type(entity.type_ref(), rules);
            let foundation = object_type
                .map(|object| &object.foundation)
                .unwrap_or_else(|| entity.seldom.type_facts.foundation());
            let has_bib = object_type.is_some_and(|object| object.bib);
            crate::sim::production::building_movement_blocking_cells(
                (entity.position.rx(), entity.position.ry()),
                foundation,
                has_bib,
            )
        })
        .into_iter()
        .flatten()
}

/// The real cells blocked by marked structures. MapClass::Place_Down
/// (0x0056846B) and Pick_Up (0x005688AD) resolve each foundation offset
/// through the map's cell array and skip missing slots.
pub(super) fn structure_movement_cells<'a>(
    entities: &'a EntityStore,
    interner: &'a StringInterner,
    rules: &'a RuleSet,
    terrain: &'a ResolvedTerrainGrid,
) -> impl Iterator<Item = (u16, u16)> + 'a {
    entities
        .values_of_category(EntityCategory::Structure)
        .flat_map(move |entity| marked_structure_movement_cells(entity, interner, rules))
        .filter_map(move |coord| terrain.allocated_cell_coord(coord))
}

/// The requested cells a marked structure blocks, from one structure scan.
pub(super) fn structure_blocked_among(
    entities: &EntityStore,
    interner: &StringInterner,
    rules: &RuleSet,
    terrain: Option<&ResolvedTerrainGrid>,
    cells: &[(u16, u16)],
) -> BTreeSet<(u16, u16)> {
    let Some(terrain) = terrain else {
        return BTreeSet::new();
    };
    let requested: BTreeSet<_> = cells
        .iter()
        .filter_map(|&coord| terrain.allocated_cell_coord(coord))
        .collect();
    if requested.is_empty() {
        return requested;
    }
    structure_movement_cells(entities, interner, rules, terrain)
        .filter(|cell| requested.contains(cell))
        .collect()
}

/// The world's zone and path caches are its fields as they are
/// (`util::forked`), so a publication that finds a cell current only reads
/// them.
pub(super) struct NavigationCaches<'a> {
    pub(super) zones: &'a mut Forked<Option<ZoneGrid>>,
    pub(super) path: &'a mut Forked<Option<Arc<PathGrid>>>,
    /// The live overlay surface the path projection reads for bridge sprites.
    pub(super) overlays: Option<&'a OverlayGrid>,
    /// Current operation's world bounds; this borrowed view is never retained.
    pub(super) playfield_bounds: Option<crate::map::playfield::PlayfieldBounds>,
}

impl NavigationCaches<'_> {
    /// Publish one real Recalc receiver with its marked-structure presence.
    pub(super) fn publish_recalculated_cell_with_presence(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        coord: (u16, u16),
        structure_blocked: bool,
    ) -> Result<(), String> {
        let cell = terrain
            .cell(coord.0, coord.1)
            .ok_or("Recalc cell is outside terrain")?;
        // Only a changed cell rewrites the grid, and only it takes the grid
        // for writing.
        if self
            .zones
            .as_ref()
            .is_some_and(|zones| !zones.base_cell_attributes_current(terrain, coord.0, coord.1))
            && let Some(zones) = self.zones.as_mut()
        {
            zones.refresh_base_cell_attributes_at(terrain, coord.0, coord.1);
        }
        if let Some(path) = self.path.as_ref() {
            if path.width() != terrain.width() || path.height() != terrain.height() {
                return Err("path dimensions differ from terrain".into());
            }
            let overlays = self.overlays;
            if !path.resolved_cell_is_current(cell, overlays, structure_blocked)
                && !self.path.as_mut().is_some_and(|path| {
                    Arc::make_mut(path).refresh_resolved_cell(cell, overlays, structure_blocked)
                })
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
        let mut grid = PathGrid::from_resolved_terrain_with_bridges(terrain, self.overlays);

        let structure_cells: BTreeSet<_> =
            structure_movement_cells(entities, interner, rules, terrain).collect();
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
            **self.path = Some(Arc::new(grid));
        } else {
            self.rebuild_zones(Arc::new(grid), terrain, bridges);
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
        // The bridge callback already holds the real Cell receiver.
        let blocked =
            structure_movement_cells(entities, interner, rules, terrain).any(|cell| cell == coord);
        self.publish_recalculated_cell_with_presence(terrain, coord, blocked)
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
            let structure_blocked = structure_movement_cells(entities, interner, rules, terrain)
                .any(|marked| marked == coord);
            if !Arc::make_mut(path).refresh_resolved_cell(cell, self.overlays, structure_blocked) {
                return Err("current path cell could not be published".into());
            }
        }
        Ok(())
    }

    /// Publish `path_grid` and the zones for it.
    pub(super) fn rebuild_zones(
        &mut self,
        path_grid: Arc<PathGrid>,
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
                .diff_cells(&path_grid)
                .is_some_and(|changed| changed.is_empty())
            && zones.movement_classes_match(terrain)
        {
            // PathGrid does not carry CellClass reduced zone type.
            // Both path state and retained base classes must match.
            **self.path = Some(path_grid);
            return;
        }
        self.rebuild_zones_full(path_grid, terrain, bridges);
    }

    /// Bypass reuse when a changed reduced zone type needs a full rebuild even
    /// though boolean walkability remains identical (e.g. OccupationBits=0).
    pub(super) fn rebuild_zones_full(
        &mut self,
        path_grid: Arc<PathGrid>,
        terrain: &ResolvedTerrainGrid,
        bridges: Option<&BridgeRuntimeState>,
    ) {
        **self.zones = Some(
            ZoneGrid::build_with_native_map_context(
                &path_grid,
                terrain,
                bridges
                    .map(BridgeRuntimeState::endpoint_records)
                    .unwrap_or(&[]),
                bridges.and_then(BridgeRuntimeState::native_zone_source_size),
                self.playfield_bounds,
            )
            .into(),
        );
        **self.path = Some(path_grid);
    }
}
