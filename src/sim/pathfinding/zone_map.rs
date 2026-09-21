//! Zone-based connectivity map for hierarchical pathfinding.
//!
//! The map is partitioned into zones — connected regions of passable cells —
//! per `MovementZone`. This enables:
//! - **O(1) reachability checks**: two cells are mutually reachable iff they
//!   share the same zone ID (or zones are connected via the adjacency graph).
//! - **Hierarchical search**: Dijkstra on the zone graph finds a corridor of
//!   zones, then A* only explores cells within that corridor.
//!
//! Zones are computed via flood-fill at map load and rebuilt when terrain
//! changes (building placement/destruction, bridge destruction).
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/pathfinding, sim/terrain_cost, sim/locomotor.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::collections::{BTreeMap, VecDeque};

use super::PathGrid;
use super::terrain_cost::TerrainCostGrid;
use super::zone_build;
use super::zone_hierarchy::{SuperZoneMap, ZoneHierarchy};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::terrain_rules::LandType;
use crate::sim::movement::locomotor::MovementLayer;

/// A native CellStruct pointer may refer to a copied local (Foot4D3810) or
/// retained CellClass+24 (Cell-click4DE1D0). Only the latter follows Dummy
/// coordinate writes performed by intervening map queries.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ZoneQueryCell {
    Copied((i16, i16)),
    Retained(crate::map::cell_index::NativeCellIdentity),
}

impl ZoneQueryCell {
    fn coord(self, cells: &crate::map::resolved_terrain::NativeCellQuery<'_>) -> (i16, i16) {
        match self {
            Self::Copied(coord) => coord,
            Self::Retained(cell) => cells.coord(cell),
        }
    }
}

#[path = "bridge_repair_zones.rs"]
mod bridge_repair_zones;

/// Zone ID: 0 = impassable/unassigned, 1+ = valid zone.
pub type ZoneId = u16;

/// Sentinel for impassable or unassigned cells.
pub const ZONE_INVALID: ZoneId = 0;

/// Per-zone metadata: centroid and cell count.
/// Used by the hierarchical zone Dijkstra to estimate inter-zone distances.
#[derive(Debug, Clone, Copy, Default)]
pub struct ZoneInfo {
    pub center: (u16, u16),
    pub cell_count: u32,
}

/// Per-movement-zone cell-to-zone lookup.
#[derive(Debug, Clone)]
pub struct ZoneMap {
    /// Zone ID per cell, indexed by `y * width + x`. ZONE_INVALID = impassable.
    ///
    /// TODO(RE): RA2/YR does not store zone IDs directly per cell. Each cell carries
    /// a nodeIndex, and each MovementZone has its own zoneIdByNodeIndex table.
    zone_ids: Vec<ZoneId>,
    /// Per-cell bridge redirect: for bridge cells, the ground endpoint cell
    /// whose zone ID should be returned for bridge-layer queries.
    /// None = no bridges on map. Mirrors gamemd.exe GetZoneID redirect (0x0056d230).
    bridge_redirect: Option<Vec<Option<(u16, u16)>>>,
    pub width: u16,
    pub height: u16,
    /// Highest assigned zone ID. Native-derived maps reserve label 1, so this
    /// can be greater than the number of publicly passable components.
    pub zone_count: u16,
    /// Per-zone centroid and cell count (index = zone_id - 1). Reserved labels
    /// retain default metadata.
    pub zone_info: Vec<ZoneInfo>,
}

impl ZoneMap {
    /// Construct a ZoneMap from pre-computed arrays.
    pub(crate) fn new(
        zone_ids: Vec<ZoneId>,
        bridge_redirect: Option<Vec<Option<(u16, u16)>>>,
        width: u16,
        height: u16,
        zone_count: u16,
        zone_info: Vec<ZoneInfo>,
    ) -> Self {
        Self {
            zone_ids,
            bridge_redirect,
            width,
            height,
            zone_count,
            zone_info,
        }
    }

    /// Look up the zone ID for a cell at the given layer.
    ///
    /// For bridge-layer queries on a structural cell, returns the ground zone
    /// selected by the matching high-bridge record. Nonstructural cells a high
    /// record reaches keep their own ground zone. Any cell the redirect table
    /// does not cover — and every cell when the map has no high bridge at all —
    /// has no bridge layer and is invalid. Answering such a query with the
    /// ground zone would report every cell as bridge-reachable.
    pub fn zone_at(&self, x: u16, y: u16, layer: MovementLayer) -> ZoneId {
        if x >= self.width || y >= self.height {
            return ZONE_INVALID;
        }
        let idx = y as usize * self.width as usize + x as usize;
        match layer {
            MovementLayer::Bridge => {
                let Some(redirect) = &self.bridge_redirect else {
                    return ZONE_INVALID;
                };
                let Some(Some((ex, ey))) = redirect.get(idx) else {
                    return ZONE_INVALID;
                };
                let e_idx = *ey as usize * self.width as usize + *ex as usize;
                self.zone_ids.get(e_idx).copied().unwrap_or(ZONE_INVALID)
            }
            _ => self.zone_ids[idx],
        }
    }

    /// Get the centroid and cell count for a zone.
    pub fn info_for(&self, zone_id: ZoneId) -> Option<&ZoneInfo> {
        if zone_id == ZONE_INVALID {
            return None;
        }
        self.zone_info.get(zone_id as usize - 1)
    }

    /// Check if two cells are in the same zone (same layer assumed).
    pub fn same_zone(&self, a: (u16, u16), b: (u16, u16), layer: MovementLayer) -> bool {
        let za = self.zone_at(a.0, a.1, layer);
        let zb = self.zone_at(b.0, b.1, layer);
        za != ZONE_INVALID && za == zb
    }

    /// Immutable access to the ground-layer zone ID array.
    pub(crate) fn zone_ids_slice(&self) -> &[ZoneId] {
        &self.zone_ids
    }

    /// Mutable access to the ground-layer zone ID array.
    pub(crate) fn zone_ids_mut(&mut self) -> &mut Vec<ZoneId> {
        &mut self.zone_ids
    }

    pub(crate) fn set_ground_zone_at_index(&mut self, index: usize, zone: ZoneId) {
        if let Some(slot) = self.zone_ids.get_mut(index) {
            *slot = zone;
        }
    }

    /// Replace the bridge redirect table (e.g. after incremental recomputation).
    pub(crate) fn set_bridge_redirect(&mut self, redirect: Option<Vec<Option<(u16, u16)>>>) {
        self.bridge_redirect = redirect;
    }

    /// Update zone_count (e.g. after incremental zone assignment).
    pub(crate) fn set_zone_count(&mut self, n: u16) {
        self.zone_count = n;
    }

    /// Replace zone_info (e.g. after incremental recomputation).
    pub(crate) fn set_zone_info(&mut self, info: Vec<ZoneInfo>) {
        self.zone_info = info;
    }
}

/// Zone adjacency graph — which zones border each other.
#[derive(Debug, Clone)]
pub struct ZoneAdjacency {
    /// For each zone ID (1-indexed), adjacent zone IDs in discovery order.
    pub neighbors: Vec<Vec<ZoneId>>,
}

impl ZoneAdjacency {
    /// Construct from a pre-built neighbor list.
    pub(crate) fn new(neighbors: Vec<Vec<ZoneId>>) -> Self {
        Self { neighbors }
    }

    /// Check if two zones are directly adjacent.
    #[cfg(test)]
    pub fn are_adjacent(&self, a: ZoneId, b: ZoneId) -> bool {
        if a == ZONE_INVALID || b == ZONE_INVALID {
            return false;
        }
        let idx = a as usize;
        if idx >= self.neighbors.len() {
            return false;
        }
        self.neighbors[idx].contains(&b)
    }

    /// Get the neighbors of a zone.
    pub fn neighbors_of(&self, z: ZoneId) -> &[ZoneId] {
        if z == ZONE_INVALID || z as usize >= self.neighbors.len() {
            return &[];
        }
        &self.neighbors[z as usize]
    }
}

/// Complete zone system: zone maps + adjacency graphs for all movement zones.
#[derive(Debug, Clone)]
pub struct ZoneGrid {
    maps: BTreeMap<MovementZone, ZoneMap>,
    adjacency: BTreeMap<MovementZone, ZoneAdjacency>,
    /// Connected-component labels for O(1) reachability checks.
    super_zones: BTreeMap<MovementZone, SuperZoneMap>,
    /// One optional gamemd-style route-selection hierarchy shared by all rows.
    hierarchy: Option<ZoneHierarchy>,
    /// Cell-owned reduced classes, shared base clusters, and the retained raw
    /// per-row cluster mappings used by exact one-cell repair.
    base_topology: Option<zone_build::BaseZoneTopology>,
    /// Ordered bridge records paired with the native base-zone projection and
    /// hierarchy snapshot; record-only changes invalidate cached connectivity.
    bridge_records: Vec<crate::sim::bridge_state::BridgeEndpointRecord>,
    native_bridge_source_size: Option<(i32, i32)>,
    pub width: u16,
    pub height: u16,
}

impl ZoneGrid {
    /// Build zone maps for all non-trivial categories from terrain data.
    pub fn build(
        path_grid: &PathGrid,
        terrain_costs: &BTreeMap<SpeedType, TerrainCostGrid>,
        width: u16,
        height: u16,
    ) -> Self {
        Self::build_with_terrain(path_grid, terrain_costs, None, &[], width, height)
    }

    /// Build zone maps using resolved terrain passability when available.
    /// Bridge endpoint records inject cross-bridge adjacency edges for
    /// ground-capable movement zones.
    pub fn build_with_terrain(
        path_grid: &PathGrid,
        terrain_costs: &BTreeMap<SpeedType, TerrainCostGrid>,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        bridge_records: &[crate::sim::bridge_state::BridgeEndpointRecord],
        width: u16,
        height: u16,
    ) -> Self {
        Self::build_with_native_bridge_geometry(
            path_grid,
            terrain_costs,
            resolved_terrain,
            bridge_records,
            width,
            height,
            None,
        )
    }

    pub(crate) fn build_with_native_bridge_geometry(
        path_grid: &PathGrid,
        terrain_costs: &BTreeMap<SpeedType, TerrainCostGrid>,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        bridge_records: &[crate::sim::bridge_state::BridgeEndpointRecord],
        width: u16,
        height: u16,
        native_bridge_source_size: Option<(i32, i32)>,
    ) -> Self {
        Self::build_with_hierarchy_query(
            path_grid,
            terrain_costs,
            resolved_terrain,
            bridge_records,
            (width, height),
            native_bridge_source_size,
            None,
        )
    }

    /// Live581F90 construction. Bounds are borrowed from the current world
    /// operation, never inferred from Size or retained in the navigation cache.
    pub(crate) fn build_with_native_map_context(
        path_grid: &PathGrid,
        terrain_costs: &BTreeMap<SpeedType, TerrainCostGrid>,
        terrain: &ResolvedTerrainGrid,
        bridge_records: &[crate::sim::bridge_state::BridgeEndpointRecord],
        native_bridge_source_size: Option<(i32, i32)>,
        bounds: Option<crate::map::playfield::PlayfieldBounds>,
    ) -> Self {
        Self::build_with_hierarchy_query(
            path_grid,
            terrain_costs,
            Some(terrain),
            bridge_records,
            (terrain.width(), terrain.height()),
            native_bridge_source_size,
            Some(&mut |x, y| {
                crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                    (x, y),
                    bounds,
                    Some(terrain),
                )
            }),
        )
    }

    fn build_with_hierarchy_query(
        path_grid: &PathGrid,
        terrain_costs: &BTreeMap<SpeedType, TerrainCostGrid>,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        bridge_records: &[crate::sim::bridge_state::BridgeEndpointRecord],
        (width, height): (u16, u16),
        native_bridge_source_size: Option<(i32, i32)>,
        query: Option<&mut dyn FnMut(i32, i32) -> bool>,
    ) -> Self {
        let mut maps = BTreeMap::new();
        let mut adjacency = BTreeMap::new();
        let mut super_zones = BTreeMap::new();
        let base_topology = resolved_terrain.map(|terrain| {
            zone_build::build_base_zone_topology(
                path_grid,
                terrain,
                bridge_records,
                width,
                height,
                native_bridge_source_size,
            )
        });
        let hierarchy = base_topology.as_ref().map(|base| {
            if let Some(query) = query {
                zone_build::build_zone_hierarchy_with_query(
                    base,
                    resolved_terrain,
                    bridge_records,
                    width,
                    height,
                    &mut |x, y| query(x, y),
                )
            } else {
                zone_build::build_zone_hierarchy(
                    base,
                    resolved_terrain,
                    bridge_records,
                    width,
                    height,
                )
            }
        });

        for &mz in MovementZone::all_ground() {
            let speed_type = mz.speed_type();
            let cost_grid = terrain_costs.get(&speed_type);

            let (mut zone_map, mut adj) = if let Some(base) = &base_topology {
                zone_build::build_zone_map_from_base_topology(base, mz, width, height)
            } else {
                zone_build::build_zone_map_with_terrain(
                    path_grid, cost_grid, None, mz, width, height,
                )
            };

            if mz.can_use_bridges() {
                if base_topology.is_none() {
                    zone_build::inject_bridge_adjacency(
                        &mut adj,
                        zone_map.zone_ids_slice(),
                        bridge_records,
                        width,
                        zone_build::BridgeRecordFilter::AllActive,
                    );
                }
                zone_map.set_bridge_redirect(zone_build::build_bridge_redirect(
                    path_grid,
                    resolved_terrain,
                    bridge_records,
                    width,
                    height,
                ));
            }

            let sz = SuperZoneMap::from_adjacency(&adj, zone_map.zone_count);
            super_zones.insert(mz, sz);
            maps.insert(mz, zone_map);
            adjacency.insert(mz, adj);
        }

        ZoneGrid {
            maps,
            adjacency,
            super_zones,
            hierarchy,
            base_topology,
            bridge_records: bridge_records.to_vec(),
            native_bridge_source_size,
            width,
            height,
        }
    }

    pub(crate) fn bridge_inputs_match(
        &self,
        records: &[crate::sim::bridge_state::BridgeEndpointRecord],
        source_size: Option<(i32, i32)>,
    ) -> bool {
        self.bridge_records == records && self.native_bridge_source_size == source_size
    }

    /// Get the zone map for a movement zone.
    pub fn map_for(&self, mz: MovementZone) -> Option<&ZoneMap> {
        self.maps.get(&mz)
    }

    /// Exact non-bridge `MapClass::GetZoneID` raw-row lookup.
    ///
    /// Native `MapClass::GetZoneID @ 0x0056D230` packs both coordinate
    /// components to signed16-bit and indexes the retained source Size square
    /// `(Size.width + Size.height + 1)^2`. Legacy square fixtures without a
    /// source receipt retain their prior backing-width+1 inferred stride. Native
    /// clamps only the resulting signed linear index, and projects the base
    /// cluster through the requested raw movement-zone row. The extra final
    /// row and column are zero-initialized base cluster 0; raw labels `1` and
    /// `0xffff` are returned unchanged.
    ///
    /// This seam deliberately refuses compatibility-only or malformed Rust
    /// topology. Native permits an unchecked movement-row access, but Rust has
    /// no sound equivalent for that undefined read, so a non-concrete row or a
    /// missing cluster entry fails explicitly.
    pub(crate) fn get_zone_id_nonbridge_native(
        &self,
        coord: (i32, i32),
        movement_zone: MovementZone,
    ) -> Option<ZoneId> {
        let base = self.base_topology.as_ref()?;
        let cell_count = usize::from(self.width) * usize::from(self.height);
        if base.zone_ids.len() != cell_count || base.movement_classes.len() != cell_count {
            return None;
        }
        let row = movement_zone.matrix_row()?;
        let raw_row = base.raw_zone_ids_by_row.get(row)?;
        // Same signed native node projection as56D430/56C510; source Size
        // travels with the derived record set, not the materialized rectangle.
        let source_size = base
            .native_bridge_source_size
            .or_else(|| (self.width == self.height).then_some((i32::from(self.width), 0)))?;
        let cluster = zone_build::bridge_endpoint_base_zone(
            &base.zone_ids,
            self.width,
            Some(source_size),
            (coord.0 as u16, coord.1 as u16),
        )?;
        raw_row.get(cluster as usize).copied()
    }

    ///56D230 result as consumed by42C900: missing structural high record is
    /// DWORDFFFFFFFF, distinct from raw rowFFFF. Query current native cell
    /// identity/flags rather than inferring structural presence from a cache.
    pub(crate) fn get_path_zone_id_native(
        &self,
        terrain: &ResolvedTerrainGrid,
        coord: (u16, u16),
        movement_zone: MovementZone,
        check_bridge: bool,
    ) -> Option<u32> {
        self.get_path_zone_id_native_in_query(terrain, coord, movement_zone, check_bridge, None)
    }

    pub(crate) fn get_path_zone_id_native_in_query(
        &self,
        terrain: &ResolvedTerrainGrid,
        coord: (u16, u16),
        movement_zone: MovementZone,
        check_bridge: bool,
        query: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    ) -> Option<u32> {
        use crate::sim::cell_rect::{CellRef, get_cellclass_in_query};
        let mut selected = coord;
        if check_bridge {
            let cell = get_cellclass_in_query(
                Some(terrain),
                i32::from(coord.0 as i16),
                i32::from(coord.1 as i16),
                query,
            );
            let structural = match cell {
                CellRef::Real(cell) => cell.bridge_facts.has_structural_bridge(),
                CellRef::Dummy { cell } => cell.snapshot().bridge_flags_0x1180 & 0x100 != 0,
            };
            if structural {
                let Some(record) =
                    zone_build::find_high_bridge_record(&self.bridge_records, 0, coord, 1)
                else {
                    return Some(u32::MAX);
                };
                selected = record.endpoint_a;
                if !record.active {
                    //56D2B3 reloads the cell;481810 steps from the returned
                    // CellClass+24, including fixed-stride aliases and dummy.
                    // This live query owns writes; cache construction must not.
                    let mut current = get_cellclass_in_query(
                        Some(terrain),
                        i32::from(coord.0 as i16),
                        i32::from(coord.1 as i16),
                        query,
                    );
                    let vertical = record.endpoint_a.0 == record.endpoint_b.0;
                    let mut visited = std::collections::BTreeSet::new();
                    while current.bridge_flags_0x1180() & 0x100 != 0 {
                        let (position, is_dummy) = match &current {
                            CellRef::Real(cell) => ((cell.rx as i16, cell.ry as i16), false),
                            CellRef::Dummy { cell } => {
                                let c = cell.snapshot().coord;
                                ((c.0 as i16, c.1 as i16), true)
                            }
                        };
                        if !visited.insert((position, is_dummy)) {
                            // Native never returns on a repeated walk state.
                            // Explicit unresolved domain: None currently uses
                            // the caller's compatibility equality fallback.
                            log::warn!("unresolved cyclic native inactive bridge zone query");
                            return None;
                        }
                        let next = if vertical {
                            (position.0, position.1.wrapping_add(1))
                        } else {
                            (position.0.wrapping_add(1), position.1)
                        };
                        current = get_cellclass_in_query(
                            Some(terrain),
                            i32::from(next.0),
                            i32::from(next.1),
                            query,
                        );
                    }
                    // Constructor dummy tile65535 is outside both high sets
                    // in all six hash-bound retail theaters (bridge report).
                    if let CellRef::Real(exit) = current {
                        if terrain.high_bridge_tile_offset(exit).is_some()
                            && exit.yr_cell_land_type != LandType::Rock.as_index()
                        {
                            selected = record.endpoint_b;
                        }
                    }
                }
            }
        }
        self.get_zone_id_nonbridge_native(
            (i32::from(selected.0), i32::from(selected.1)),
            movement_zone,
        )
        .map(u32::from)
    }

    /// Map56D100: asymmetric playfield/Size shortcuts, then target and source
    /// raw zone queries in that order. A missing topology is unavailable input,
    /// not a negative native predicate. Raw WORDFFFF and DWORDFFFFFFFF remain
    /// distinct. See walk_move_admission and walk_failed_path native corpora.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn can_reach_native(
        &self,
        cells: &crate::map::resolved_terrain::NativeCellQuery<'_>,
        source: ZoneQueryCell,
        destination: ZoneQueryCell,
        movement_zone: MovementZone,
        source_bridge: bool,
        destination_bridge: bool,
        allow_destination_fringe: bool,
        bounds: crate::sim::cell_rect::PlayfieldBounds,
        size: (i32, i32),
    ) -> Option<bool> {
        use crate::sim::cell_rect::cell_is_in_playfield_height_aware_in_query;
        if movement_zone == MovementZone::Invalid {
            return Some(true);
        }
        let in_playfield = |cell: ZoneQueryCell| {
            let p = cell.coord(cells);
            cell_is_in_playfield_height_aware_in_query(
                (i32::from(p.0), i32::from(p.1)),
                Some(bounds),
                Some(cells.terrain()),
                Some(cells),
            )
        };
        let in_size = |cell: ZoneQueryCell| {
            let p = cell.coord(cells);
            cell_is_in_native_map_diamond((i32::from(p.0), i32::from(p.1)), size.0, size.1)
        };
        let source_in_playfield = in_playfield(source);
        //56D12D reloads the pointer after the height-aware query, which may
        //have stamped an aliased Dummy. Do not keep its pre-query coordinates.
        if in_size(source) && !source_in_playfield {
            return Some(true);
        }
        //56D187 executes even when argument6 disables this second shortcut.
        let destination_in_playfield = in_playfield(destination);
        let destination_in_size = in_size(destination);
        if allow_destination_fringe
            && source_in_playfield
            && !destination_in_playfield
            && destination_in_size
        {
            return Some(true);
        }
        let target = destination.coord(cells);
        let target_zone = self.get_path_zone_id_native_in_query(
            cells.terrain(),
            (target.0 as u16, target.1 as u16),
            movement_zone,
            destination_bridge,
            Some(cells),
        )?;
        let source = source.coord(cells);
        let source_zone = self.get_path_zone_id_native_in_query(
            cells.terrain(),
            (source.0 as u16, source.1 as u16),
            movement_zone,
            source_bridge,
            Some(cells),
        )?;
        Some(source_zone == target_zone)
    }

    /// Exact `MapClass::Can_Reach_Zone @ 0x0056D100` surface as the
    /// base-defence response consumes it.
    ///
    /// `None` is native MovementZone `-1`. The response passes the candidate
    /// destination as source A, the protected victim destination as B, the
    /// candidate's `ShouldBeOnBridge` for A, false for B, and disables the
    /// second asymmetric B-fringe shortcut. Both raw invalid labels compare
    /// equal exactly as native; no adjacency/super-zone widening is consulted.
    pub(crate) fn can_reach_base_defense_response(
        &self,
        movement_zone: Option<MovementZone>,
        source: (i32, i32),
        destination: (i32, i32),
        source_should_be_on_bridge: bool,
        source_in_tactical_playfield: bool,
        map_size_width: i32,
        map_size_height: i32,
    ) -> bool {
        let Some(movement_zone) = movement_zone else {
            return true;
        };

        if !source_in_tactical_playfield
            && cell_is_in_native_map_diamond(source, map_size_width, map_size_height)
        {
            return true;
        }

        let source_zone =
            self.get_zone_id_native(source, movement_zone, source_should_be_on_bridge);
        let destination_zone = self.get_zone_id_native(destination, movement_zone, false);
        source_zone.is_some() && source_zone == destination_zone
    }

    /// `MapClass::GetZoneID @ 0x0056D230` with its third argument, the
    /// bridge-resolution flag, honoured.
    ///
    /// Native takes `(CellStruct *cell, int movementZone, char checkBridge)`.
    /// With `checkBridge` set and the cell carrying the bridge flag `0x100`, it
    /// resolves the matching `BridgeRecord` and answers with the ground
    /// endpoint's zone; otherwise it projects the cell's own base cluster
    /// through the requested movement row. `checkBridge` clear skips the
    /// record lookup entirely.
    ///
    /// Legacy cached16-bit consumers pass the source/destination bridge flag.
    /// Missing-record DWORD and live inactive-walk parity remain unresolved
    /// here;42C900 uses get_path_zone_id_native with current terrain instead:
    /// - `Can_Reach_Zone @ 0x0056D100` (base-defence response), the candidate's
    ///   `ShouldBeOnBridge` for source A and `false` for B;
    /// - `TechnoClass::Greatest_Threat @ 0x006F8EBF`, a literal `1` for the
    ///   scanner's own cell, and `TechnoClass::Evaluate_Candidate @
    ///   0x006F7E95`, the candidate's `Object+0x8C` on-bridge byte.
    pub(crate) fn get_zone_id_native(
        &self,
        coord: (i32, i32),
        movement_zone: MovementZone,
        check_bridge: bool,
    ) -> Option<ZoneId> {
        if !check_bridge {
            return self.get_zone_id_nonbridge_native(coord, movement_zone);
        }

        let packed = (coord.0 as i16 as i32, coord.1 as i16 as i32);
        if packed.0 >= 0
            && packed.1 >= 0
            && packed.0 < i32::from(self.width)
            && packed.1 < i32::from(self.height)
        {
            let map = self.maps.get(&movement_zone)?;
            let index = packed.1 as usize * usize::from(self.width) + packed.0 as usize;
            if let Some(Some(endpoint)) = map
                .bridge_redirect
                .as_ref()
                .and_then(|redirect| redirect.get(index))
            {
                return self.get_zone_id_nonbridge_native(
                    (i32::from(endpoint.0), i32::from(endpoint.1)),
                    movement_zone,
                );
            }
        }
        self.get_zone_id_nonbridge_native(packed, movement_zone)
    }

    /// Get the adjacency graph for a movement zone.
    pub fn adjacency_for(&self, mz: MovementZone) -> Option<&ZoneAdjacency> {
        self.adjacency.get(&mz)
    }

    /// The native projected endpoint can address padding or a linear alias.
    /// Ordinary A* expansion keeps its represented-cell lookup on the graph.
    pub(crate) fn hierarchy_zone_at_native(
        &self,
        level: usize,
        coord: (u16, u16),
    ) -> Option<ZoneId> {
        let graph = self.hierarchy.as_ref()?.level(level)?;
        if self.native_bridge_source_size.is_some() {
            graph.native_zone_at(coord, self.native_bridge_source_size)
        } else {
            Some(graph.zone_at(coord.0, coord.1))
        }
    }

    /// Get the shared route-selection hierarchy when this movement row exists.
    pub(crate) fn hierarchy_for(&self, mz: MovementZone) -> Option<&ZoneHierarchy> {
        if !self.maps.contains_key(&mz) {
            return None;
        }
        self.hierarchy.as_ref()
    }

    pub(crate) fn bridge_records(&self) -> &[crate::sim::bridge_state::BridgeEndpointRecord] {
        &self.bridge_records
    }

    pub(crate) fn movement_classes_match(&self, terrain: &ResolvedTerrainGrid) -> bool {
        let Some(base) = &self.base_topology else {
            return false;
        };
        base.movement_classes.len() == self.width as usize * self.height as usize
            && (0..self.height).all(|y| {
                (0..self.width).all(|x| {
                    let index = y as usize * self.width as usize + x as usize;
                    base.movement_classes[index]
                        == zone_build::movement_class_for_cell(terrain, x, y)
                })
            })
    }

    /// Project one RecalcAttributes cell into the retained base topology
    /// without assigning a zone or rebuilding hierarchy. Mutation owners that
    /// have a later native repair callback use this to make the new class/height
    /// visible to earlier ordered neighbor repairs.
    pub(crate) fn refresh_base_cell_attributes_at(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        x: u16,
        y: u16,
    ) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let index = y as usize * self.width as usize + x as usize;
        let Some(base) = self.base_topology.as_mut() else {
            return false;
        };
        let Some(slot) = base.movement_classes.get_mut(index) else {
            return false;
        };
        *slot = zone_build::movement_class_for_cell(terrain, x, y);
        base.levels[index] = terrain.cell(x, y).map_or(0, |cell| cell.level);
        true
    }

    #[cfg(test)]
    pub(crate) fn base_movement_class_at(&self, x: u16, y: u16) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.base_topology
            .as_ref()?
            .movement_classes
            .get(y as usize * self.width as usize + x as usize)
            .copied()
    }

    pub(crate) fn base_topology_mut(&mut self) -> Option<&mut zone_build::BaseZoneTopology> {
        self.base_topology.as_mut()
    }

    pub(crate) fn base_and_hierarchy_mut(
        &mut self,
    ) -> Option<(&zone_build::BaseZoneTopology, &mut ZoneHierarchy)> {
        Some((self.base_topology.as_ref()?, self.hierarchy.as_mut()?))
    }

    /// Project one adopted base cluster through the retained raw 13-row maps.
    /// No topology, count, adjacency, or unrelated cell is rewritten.
    pub(crate) fn project_adopted_base_cell(&mut self, cell_index: usize) {
        let Some(base) = &self.base_topology else {
            return;
        };
        let Some(&cluster) = base.zone_ids.get(cell_index) else {
            return;
        };
        let (width, height) = (self.width, self.height);
        for &movement_zone in MovementZone::all_ground() {
            let row = movement_zone.matrix_row().expect("concrete movement row");
            let raw = base.raw_zone_ids_by_row[row]
                .get(cluster as usize)
                .copied()
                .unwrap_or(u16::MAX);
            let projected = (raw > 1 && raw != u16::MAX)
                .then_some(raw)
                .unwrap_or(ZONE_INVALID);
            if let Some(map) = self.maps.get_mut(&movement_zone) {
                map.set_ground_zone_at_index(cell_index, projected);
                let zone_info = zone_build::compute_zone_info(
                    map.zone_ids_slice(),
                    width,
                    height,
                    map.zone_count,
                );
                map.set_zone_info(zone_info);
            }
        }
    }

    pub(crate) fn replace_hierarchy(&mut self, hierarchy: ZoneHierarchy) {
        self.hierarchy = Some(hierarchy);
    }

    /// Rebuild the products owned by the base connectivity pass while retaining
    /// the hierarchy's append-only identifiers for the following local patch.
    pub(crate) fn rebuild_base_connectivity_preserving_hierarchy(
        &mut self,
        path_grid: &PathGrid,
        resolved_terrain: &ResolvedTerrainGrid,
        bridge_records: &[crate::sim::bridge_state::BridgeEndpointRecord],
    ) {
        let base_topology = if let Some(base) = &self.base_topology {
            zone_build::rebuild_base_zone_topology(
                base.movement_classes.clone(),
                base.levels.clone(),
                bridge_records,
                self.width,
                self.height,
                self.native_bridge_source_size,
            )
        } else {
            // Compatibility bootstrap has no retained native node plane yet.
            zone_build::build_base_zone_topology(
                path_grid,
                resolved_terrain,
                bridge_records,
                self.width,
                self.height,
                self.native_bridge_source_size,
            )
        };
        let mut maps = BTreeMap::new();
        let mut adjacency = BTreeMap::new();
        let mut super_zones = BTreeMap::new();

        for &movement_zone in MovementZone::all_ground() {
            let (mut zone_map, graph) = zone_build::build_zone_map_from_base_topology(
                &base_topology,
                movement_zone,
                self.width,
                self.height,
            );
            if movement_zone.can_use_bridges() {
                zone_map.set_bridge_redirect(zone_build::build_bridge_redirect(
                    path_grid,
                    Some(resolved_terrain),
                    bridge_records,
                    self.width,
                    self.height,
                ));
            }
            super_zones.insert(
                movement_zone,
                SuperZoneMap::from_adjacency(&graph, zone_map.zone_count),
            );
            maps.insert(movement_zone, zone_map);
            adjacency.insert(movement_zone, graph);
        }

        self.maps = maps;
        self.adjacency = adjacency;
        self.super_zones = super_zones;
        self.base_topology = Some(base_topology);
        self.bridge_records = bridge_records.to_vec();
    }

    /// Mutable access to the zone map for a movement zone (for incremental updates).
    pub(crate) fn map_mut(&mut self, mz: MovementZone) -> Option<&mut ZoneMap> {
        self.hierarchy = None;
        self.maps.get_mut(&mz)
    }

    /// Mutable access to the adjacency graph for a movement zone (for incremental updates).
    pub(crate) fn adjacency_mut(&mut self, mz: MovementZone) -> Option<&mut ZoneAdjacency> {
        self.hierarchy = None;
        self.adjacency.get_mut(&mz)
    }

    /// Replace the super-zone map for a movement zone (after incremental adjacency update).
    pub(crate) fn set_super_zone(&mut self, mz: MovementZone, sz: SuperZoneMap) {
        self.hierarchy = None;
        self.super_zones.insert(mz, sz);
    }

    /// Replace the one shared route-selection hierarchy (test fixtures only).
    #[cfg(test)]
    pub(crate) fn set_hierarchy(&mut self, hierarchy: ZoneHierarchy) {
        self.hierarchy = Some(hierarchy);
    }

    /// O(1) reachability check: can a unit with this movement zone reach `to`
    /// from `from`?
    ///
    /// `MapClass::Can_Reach_Zone` @ `0x0056D100` is a **pure equality compare**
    /// of the two `GetZoneID` results — it consults no adjacency graph and no
    /// connected-components structure.
    ///
    /// **VERA-internal widening, gamemd has no equivalent:** the two arms below
    /// that accept distinct zone IDs when the super-zone labels or the adjacency
    /// graph connect them. Trigger: any pair of distinct zone IDs joined by an
    /// adjacency edge. Player effect: VERA accepts a move order gamemd's
    /// reachability test refuses. Frequency: **zero on the production path
    /// today** — `build_zone_map_from_base_topology` deliberately returns an
    /// empty adjacency, so `are_connected` is false for distinct IDs and this
    /// collapses to the native equality. It becomes live the moment the legacy
    /// incremental path (`zone_incremental`, which repopulates adjacency and
    /// replaces the super-zone map) owns a production rebuild, and then it fires
    /// on every ground move order. Downstream risk: high — it is the difference
    /// between "one zone per reachable region" and "zones plus a graph", and the
    /// two designs cannot both be right.
    ///
    /// Also not modelled, recorded: the native opens with `if (speed_type == -1)
    /// return true`, a sentinel arm no caller can reach through
    /// `AStar_pathfind_search` (it resolves `-1` to `TechnoType+0x5B4` first) but
    /// which a caller passing a raw speed type would. Trigger and therefore
    /// frequency: UNCHECKED — no VERA call site passes a sentinel today. Player
    /// effect if it ever fires: gamemd waves the order through; VERA runs the
    /// zone test. Downstream risk: none, it is the first line of the function.
    ///
    /// And the native's two off-playfield
    /// short-circuits, which return `true` early when the source cell fails
    /// `Is_Cell_In_Playfield(cell, 1)` but lies inside the isometric diamond,
    /// and when the caller's flag is set with the source inside and the
    /// destination outside-but-in-diamond. VERA returns `false` for either,
    /// because an off-playfield cell yields `ZONE_INVALID`. Trigger: an order
    /// whose endpoint is in the map border outside the playfield rect. Player
    /// effect: the unit refuses an order retail accepts. Frequency: map-edge
    /// clicks only. Downstream risk: none; both are head-of-function predicates.
    pub fn can_reach(
        &self,
        mz: MovementZone,
        from: (u16, u16),
        from_layer: MovementLayer,
        to: (u16, u16),
        to_layer: MovementLayer,
    ) -> bool {
        let Some(zone_map) = self.maps.get(&mz) else {
            return true; // No zone data — assume reachable (conservative)
        };
        let za = zone_map.zone_at(from.0, from.1, from_layer);
        let zb = zone_map.zone_at(to.0, to.1, to_layer);
        if za == ZONE_INVALID || zb == ZONE_INVALID {
            return false;
        }
        if za == zb {
            return true;
        }
        // Different zones — O(1) super-zone check (union-find connected components).
        if let Some(sz) = self.super_zones.get(&mz) {
            return sz.are_connected(za, zb);
        }
        // Fallback to BFS if super-zones not available (should not happen).
        let Some(adj) = self.adjacency.get(&mz) else {
            return false;
        };
        zone_graph_connected(adj, za, zb, zone_map.zone_count)
    }
}

pub(crate) fn cell_is_in_native_map_diamond(
    coord: (i32, i32),
    map_size_width: i32,
    map_size_height: i32,
) -> bool {
    let x = coord.0 as i16 as i32;
    let y = coord.1 as i16 as i32;
    let sum = x.wrapping_add(y);
    map_size_width < sum
        && x.wrapping_sub(y) < map_size_width
        && y.wrapping_sub(x) < map_size_width
        && sum <= map_size_width.wrapping_add(map_size_height.wrapping_mul(2))
}

/// BFS on the zone adjacency graph to check connectivity.
pub(crate) fn zone_graph_connected(
    adj: &ZoneAdjacency,
    start: ZoneId,
    goal: ZoneId,
    max_zones: u16,
) -> bool {
    if start == goal {
        return true;
    }
    let mut visited = vec![false; max_zones as usize + 1];
    let mut queue = VecDeque::new();
    visited[start as usize] = true;
    queue.push_back(start);

    while let Some(z) = queue.pop_front() {
        for &neighbor in adj.neighbors_of(z) {
            if neighbor == goal {
                return true;
            }
            if !visited[neighbor as usize] {
                visited[neighbor as usize] = true;
                queue.push_back(neighbor);
            }
        }
    }
    false
}

// Tests are declared in zone/mod.rs (zone_map_tests.rs).
