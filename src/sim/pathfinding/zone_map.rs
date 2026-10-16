//! Zone-based connectivity map for hierarchical pathfinding.
//!
//! The map is partitioned into zones — connected regions of passable cells —
//! per `MovementZone`, projected from one native base topology built from the
//! resolved terrain (`MapClass::RebuildZoneConnectivity @ 0x0056C510`). This
//! enables:
//! - **O(1) reachability checks**: two cells are mutually reachable iff they
//!   share the same zone ID, as `MapClass::Can_Reach_Zone @ 0x0056D100`
//!   compares them.
//! - **Hierarchical search**: the shared three-level hierarchy feeds
//!   `Zone_precheck`, whose marked zones restrict the cell A*.
//!
//! Zones are built with the navigation caches and repaired or rebuilt when
//! terrain changes (building placement/destruction, bridge destruction).
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/pathfinding, sim/locomotor.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use std::collections::BTreeMap;

use super::PathGrid;
use super::zone_build;
use super::zone_hierarchy::ZoneHierarchy;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::MovementZone;
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

/// Per-movement-zone cell-to-zone lookup.
#[derive(Debug, Clone)]
pub struct ZoneMap {
    /// Zone ID per cell, indexed by `y * width + x`. ZONE_INVALID = impassable.
    ///
    /// Compatibility projection of native retained node indices and each
    /// MovementZone's zoneIdByNodeIndex row. ZoneGrid owns those retained inputs
    /// and rebuilds this projection at navigation publication boundaries.
    zone_ids: Vec<ZoneId>,
    /// Per-cell bridge redirect: for bridge cells, the ground endpoint cell
    /// whose zone ID should be returned for bridge-layer queries.
    /// None = no bridges on map. Mirrors gamemd.exe GetZoneID redirect (0x0056d230).
    bridge_redirect: Option<Vec<Option<(u16, u16)>>>,
    pub width: u16,
    pub height: u16,
}

impl ZoneMap {
    /// Construct a ZoneMap from pre-computed arrays.
    pub(crate) fn new(
        zone_ids: Vec<ZoneId>,
        bridge_redirect: Option<Vec<Option<(u16, u16)>>>,
        width: u16,
        height: u16,
    ) -> Self {
        Self {
            zone_ids,
            bridge_redirect,
            width,
            height,
        }
    }

    /// Highest assigned zone ID. Native-derived maps reserve label 1, so this
    /// can be greater than the number of publicly passable components.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn zone_count(&self) -> u16 {
        self.zone_ids.iter().copied().max().unwrap_or(ZONE_INVALID)
    }

    /// The ground-layer zone ID array (test fixtures only).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn zone_ids_slice(&self) -> &[ZoneId] {
        &self.zone_ids
    }

    /// Mutable ground-layer zone IDs (test fixtures only).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn zone_ids_mut(&mut self) -> &mut Vec<ZoneId> {
        &mut self.zone_ids
    }

    /// Look up the zone ID for a cell at the given layer in the reduced
    /// PathGrid zone labels, not native GetZoneID (that is
    /// [`ZoneGrid::get_zone_id_native`]). The bridge layer reads a redirect
    /// table built at zone rebuild, not the live cells. RESIDUAL (#904):
    /// `ZoneGrid::can_reach` (miner routing, zone_search, move-order
    /// recovery) still answers through it. Trigger: a structural cell with no
    /// matching high record, or an inactive deck. Effect: its answer can
    /// differ from GetZoneID's. Frequency: bridge cells only.
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

    pub(crate) fn set_ground_zone_at_index(&mut self, index: usize, zone: ZoneId) {
        if let Some(slot) = self.zone_ids.get_mut(index) {
            *slot = zone;
        }
    }

    /// Replace the bridge redirect table.
    pub(crate) fn set_bridge_redirect(&mut self, redirect: Option<Vec<Option<(u16, u16)>>>) {
        self.bridge_redirect = redirect;
    }
}

/// Complete zone system: zone maps for all movement zones, their shared base
/// topology and the route-selection hierarchy.
#[derive(Debug, Clone)]
pub struct ZoneGrid {
    maps: BTreeMap<MovementZone, ZoneMap>,
    /// The gamemd-style route-selection hierarchy shared by all rows.
    hierarchy: ZoneHierarchy,
    /// Derived digest of this owner's retained navigation history. Mutable
    /// entry points invalidate it before returning any writable borrow.
    navigation_history_hash: std::sync::OnceLock<u64>,
    /// Deserialization restores the saved base plane/rows first. Native
    /// LoadContent67E8CD rebuilds the hierarchy only after Cell/object load.
    pending_native_load: bool,
    /// Cell-owned reduced classes, shared base clusters, and the retained raw
    /// per-row cluster mappings used by exact one-cell repair.
    base_topology: zone_build::BaseZoneTopology,
    /// Ordered bridge records paired with the native base-zone projection and
    /// hierarchy snapshot; record-only changes invalidate cached connectivity.
    bridge_records: Vec<crate::sim::bridge_state::BridgeEndpointRecord>,
    native_bridge_source_size: Option<(i32, i32)>,
    pub width: u16,
    pub height: u16,
}

/// Native Mouse Save5BE727..767 / Load5BE355..3A7 retain Map+68's
/// classbyte,heightbyte,baseIDshort plane and all13 raw movement rows. The
/// hierarchy, compatibility projections and digest are rebuilt, not saved.
#[derive(serde::Deserialize)]
struct RetainedZoneGrid {
    width: u16,
    height: u16,
    base_topology: zone_build::BaseZoneTopology,
}

impl serde::Serialize for ZoneGrid {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(serde::Serialize)]
        struct RetainedZoneGridRef<'a> {
            width: u16,
            height: u16,
            base_topology: &'a zone_build::BaseZoneTopology,
        }
        RetainedZoneGridRef {
            width: self.width,
            height: self.height,
            base_topology: &self.base_topology,
        }
        .serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for ZoneGrid {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let retained = RetainedZoneGrid::deserialize(deserializer)?;
        let count = usize::from(retained.width) * usize::from(retained.height);
        let base = retained.base_topology;
        if base.movement_classes.len() != count
            || base.levels.len() != count
            || base.zone_ids.len() != count
        {
            return Err(serde::de::Error::custom(
                "saved navigation plane dimensions differ",
            ));
        }
        let labels = base.raw_zone_ids_by_row[0].len();
        if labels == 0
            || base
                .raw_zone_ids_by_row
                .iter()
                .any(|row| row.len() != labels)
            || base.zone_ids.iter().any(|id| usize::from(*id) >= labels)
        {
            return Err(serde::de::Error::custom(
                "saved navigation raw rows do not cover base IDs",
            ));
        }
        let maps = MovementZone::all_ground()
            .iter()
            .map(|&movement| {
                (
                    movement,
                    zone_build::build_zone_map_from_base_topology(
                        &base,
                        movement,
                        retained.width,
                        retained.height,
                    ),
                )
            })
            .collect();
        let native_bridge_source_size = base.native_bridge_source_size;
        let empty = || super::zone_hierarchy::ZoneLevelGraph::new(0);
        Ok(Self {
            maps,
            hierarchy: ZoneHierarchy::new(empty(), empty(), empty()),
            navigation_history_hash: std::sync::OnceLock::new(),
            pending_native_load: true,
            base_topology: base,
            bridge_records: Vec::new(),
            native_bridge_source_size,
            width: retained.width,
            height: retained.height,
        })
    }
}

impl ZoneGrid {
    pub(crate) fn is_native_load_pending(&self) -> bool {
        self.pending_native_load
    }

    /// Original LoadContent67E8CD ->581F50 rebuilds levels2,1,0 from the
    /// loaded Map+68 plane and raw movement rows. It does not call56C510 to
    /// renumber base connectivity from current terrain. Executable retained
    /// Save/Load and physical rebuild controls: anytown_navigation_restore.
    pub(crate) fn finish_native_load(
        &mut self,
        path: &PathGrid,
        terrain: &ResolvedTerrainGrid,
        records: &[crate::sim::bridge_state::BridgeEndpointRecord],
        bounds: Option<crate::map::playfield::PlayfieldBounds>,
    ) -> Result<(), String> {
        if terrain.width() != self.width || terrain.height() != self.height {
            return Err("saved navigation dimensions differ from the bound map".into());
        }
        self.invalidate_navigation_history();
        self.maps = Self::project_movement_rows(
            &self.base_topology,
            path,
            terrain,
            records,
            self.width,
            self.height,
        );
        self.hierarchy = zone_build::hierarchy::build_zone_hierarchy_with_query(
            &self.base_topology,
            Some(terrain),
            records,
            self.width,
            self.height,
            &mut |x, y| {
                crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                    (x, y),
                    bounds,
                    Some(terrain),
                )
            },
        );
        self.bridge_records = records.to_vec();
        self.pending_native_load = false;
        Ok(())
    }

    /// Read-only contribution of the retained native navigation state.
    /// Local56CB90/586990 updates preserve IDs, raw movement rows and ordered
    /// hierarchy history. These facts affect live queries and future repairs,
    /// so equal current terrain does not justify excluding them from the
    /// lockstep hash. Ordinary load intentionally rebuilds the hierarchy at
    /// LoadContent67E8CD; this method does not change save/load policy.
    pub(crate) fn fold_navigation_history(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        // Dimensions remain public read fields. Fold them outside the digest
        // so an existing dimension assignment cannot leave a stale cache.
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.navigation_history_hash
            .get_or_init(|| self.compute_navigation_history_hash())
            .hash(hasher);
    }

    fn invalidate_navigation_history(&mut self) {
        self.navigation_history_hash.take();
    }

    fn compute_navigation_history_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        fn fold_slice<T: Hash>(values: &[T], hasher: &mut impl Hasher) {
            (values.len() as u64).hash(hasher);
            for value in values {
                value.hash(hasher);
            }
        }

        let mut hasher = crate::sim::state_hasher::StateHasher::new();
        self.base_topology
            .native_bridge_source_size
            .hash(&mut hasher);
        self.native_bridge_source_size.hash(&mut hasher);
        fold_slice(&self.bridge_records, &mut hasher);
        fold_slice(&self.base_topology.movement_classes, &mut hasher);
        fold_slice(&self.base_topology.levels, &mut hasher);
        fold_slice(&self.base_topology.zone_ids, &mut hasher);
        for row in &self.base_topology.raw_zone_ids_by_row {
            fold_slice(row, &mut hasher);
        }
        (self.maps.len() as u64).hash(&mut hasher);
        for (movement_zone, map) in &self.maps {
            (movement_zone.matrix_row().expect("concrete movement row") as u32).hash(&mut hasher);
            map.width.hash(&mut hasher);
            map.height.hash(&mut hasher);
            fold_slice(&map.zone_ids, &mut hasher);
            map.bridge_redirect.is_some().hash(&mut hasher);
            if let Some(redirect) = &map.bridge_redirect {
                fold_slice(redirect, &mut hasher);
            }
        }
        self.hierarchy.fold_history_hash(&mut hasher);
        hasher.finish()
    }

    /// Build zone maps from resolved terrain with no playfield bounds and no
    /// native Map Size receipt.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn build_with_terrain(
        path_grid: &PathGrid,
        terrain: &ResolvedTerrainGrid,
        bridge_records: &[crate::sim::bridge_state::BridgeEndpointRecord],
        width: u16,
        height: u16,
    ) -> Self {
        Self::build_with_native_bridge_geometry(
            path_grid,
            terrain,
            bridge_records,
            width,
            height,
            None,
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn build_with_native_bridge_geometry(
        path_grid: &PathGrid,
        terrain: &ResolvedTerrainGrid,
        bridge_records: &[crate::sim::bridge_state::BridgeEndpointRecord],
        width: u16,
        height: u16,
        native_bridge_source_size: Option<(i32, i32)>,
    ) -> Self {
        Self::build_with_hierarchy_query(
            path_grid,
            terrain,
            bridge_records,
            (width, height),
            native_bridge_source_size,
            None,
        )
    }

    /// Zones from terrain whose zone classes follow `grid`: walkable cells are
    /// ground, the rest rock (test fixtures only).
    #[cfg(test)]
    pub(crate) fn following_path_grid(grid: &PathGrid) -> Self {
        let (width, height) = (grid.width(), grid.height());
        let mut classes = Vec::with_capacity(usize::from(width) * usize::from(height));
        let mut levels = Vec::with_capacity(classes.capacity());
        for y in 0..height {
            for x in 0..width {
                classes.push(if grid.is_walkable(x, y) {
                    crate::map::resolved_terrain::zone_class::GROUND
                } else {
                    crate::map::resolved_terrain::zone_class::IMPASSABLE
                });
                levels.push(grid.cell(x, y).map_or(0, |cell| cell.ground_level));
            }
        }
        let terrain =
            super::zone_map_tests::terrain_from_zone_classes(width, height, &classes, &levels);
        Self::build_with_terrain(grid, &terrain, &[], width, height)
    }

    /// Live581F90 construction. Bounds are borrowed from the current world
    /// operation, never inferred from Size or retained in the navigation cache.
    pub(crate) fn build_with_native_map_context(
        path_grid: &PathGrid,
        terrain: &ResolvedTerrainGrid,
        bridge_records: &[crate::sim::bridge_state::BridgeEndpointRecord],
        native_bridge_source_size: Option<(i32, i32)>,
        bounds: Option<crate::map::playfield::PlayfieldBounds>,
    ) -> Self {
        Self::build_with_hierarchy_query(
            path_grid,
            terrain,
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
        terrain: &ResolvedTerrainGrid,
        bridge_records: &[crate::sim::bridge_state::BridgeEndpointRecord],
        (width, height): (u16, u16),
        native_bridge_source_size: Option<(i32, i32)>,
        query: Option<&mut dyn FnMut(i32, i32) -> bool>,
    ) -> Self {
        let base_topology = zone_build::build_base_zone_topology(
            path_grid,
            terrain,
            bridge_records,
            width,
            height,
            native_bridge_source_size,
        );
        let hierarchy = if let Some(query) = query {
            zone_build::hierarchy::build_zone_hierarchy_with_query(
                &base_topology,
                Some(terrain),
                bridge_records,
                width,
                height,
                &mut |x, y| query(x, y),
            )
        } else {
            zone_build::hierarchy::build_zone_hierarchy(
                &base_topology,
                Some(terrain),
                bridge_records,
                width,
                height,
            )
        };
        let maps = Self::project_movement_rows(
            &base_topology,
            path_grid,
            terrain,
            bridge_records,
            width,
            height,
        );
        ZoneGrid {
            maps,
            hierarchy,
            navigation_history_hash: std::sync::OnceLock::new(),
            pending_native_load: false,
            base_topology,
            bridge_records: bridge_records.to_vec(),
            native_bridge_source_size,
            width,
            height,
        }
    }

    /// Project the base topology through every movement row, with the bridge
    /// redirect on the rows that use bridges.
    fn project_movement_rows(
        base: &zone_build::BaseZoneTopology,
        path_grid: &PathGrid,
        terrain: &ResolvedTerrainGrid,
        bridge_records: &[crate::sim::bridge_state::BridgeEndpointRecord],
        width: u16,
        height: u16,
    ) -> BTreeMap<MovementZone, ZoneMap> {
        MovementZone::all_ground()
            .iter()
            .map(|&mz| {
                let mut zone_map =
                    zone_build::build_zone_map_from_base_topology(base, mz, width, height);
                if mz.can_use_bridges() {
                    zone_map.set_bridge_redirect(zone_build::build_bridge_redirect(
                        path_grid,
                        Some(terrain),
                        bridge_records,
                        width,
                        height,
                    ));
                }
                (mz, zone_map)
            })
            .collect()
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

    /// Mutable access to one row's zone map (test fixtures only).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn map_mut(&mut self, mz: MovementZone) -> Option<&mut ZoneMap> {
        self.invalidate_navigation_history();
        self.maps.get_mut(&mz)
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
        let base = &self.base_topology;
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

    /// `MapClass::GetZoneID @ 0x0056D230(cell, movementZone, checkBridge)`,
    /// the one port. With `checkBridge` set on a cell carrying the bridge
    /// flag `0x100`, it resolves the matching high `BridgeRecord` and answers
    /// the ground endpoint's zone, walking an inactive deck from the live
    /// cells; a missing record answers DWORD `0xFFFFFFFF`, distinct from the
    /// raw row value `0xFFFF`. Otherwise it projects the cell's own base
    /// cluster through the movement row. It reads the current native cell
    /// identity and flags rather than a cache.
    pub(crate) fn get_zone_id_native(
        &self,
        terrain: &ResolvedTerrainGrid,
        coord: (u16, u16),
        movement_zone: MovementZone,
        check_bridge: bool,
    ) -> Option<u32> {
        self.get_zone_id_native_in_query(terrain, coord, movement_zone, check_bridge, None)
    }

    pub(crate) fn get_zone_id_native_in_query(
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
                    if let CellRef::Real(exit) = current
                        && terrain.high_bridge_tile_offset(exit).is_some()
                        && exit.yr_cell_land_type != LandType::Rock.as_index()
                    {
                        selected = record.endpoint_b;
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
        let target_zone = self.get_zone_id_native_in_query(
            cells.terrain(),
            (target.0 as u16, target.1 as u16),
            movement_zone,
            destination_bridge,
            Some(cells),
        )?;
        let source = source.coord(cells);
        let source_zone = self.get_zone_id_native_in_query(
            cells.terrain(),
            (source.0 as u16, source.1 as u16),
            movement_zone,
            source_bridge,
            Some(cells),
        )?;
        Some(source_zone == target_zone)
    }

    /// The native projected endpoint can address padding or a linear alias.
    /// Ordinary A* expansion keeps its represented-cell lookup on the graph.
    pub(crate) fn hierarchy_zone_at_native(
        &self,
        level: usize,
        coord: (u16, u16),
    ) -> Option<ZoneId> {
        let graph = self.hierarchy.level(level)?;
        if self.native_bridge_source_size.is_some() {
            graph.native_zone_at(coord, self.native_bridge_source_size)
        } else {
            Some(graph.zone_at(coord.0, coord.1))
        }
    }

    /// Get the shared route-selection hierarchy when this movement row exists.
    pub fn hierarchy_for(&self, mz: MovementZone) -> Option<&ZoneHierarchy> {
        self.maps.contains_key(&mz).then_some(&self.hierarchy)
    }

    pub(crate) fn bridge_records(&self) -> &[crate::sim::bridge_state::BridgeEndpointRecord] {
        &self.bridge_records
    }

    /// Map 583820: find a structural bridge's exit cell in one hierarchy
    /// zone. EstimateZoneCost uses this for its two bridge-distance terms.
    /// Original instructions and bounded controls are retained alongside
    /// tools/spatial_oracle/fv_cell_attack/zone_cost.json.
    pub(crate) fn bridge_cell_for_hierarchy_zone(
        &self,
        terrain: &ResolvedTerrainGrid,
        original: (u16, u16),
        level: usize,
        zone: ZoneId,
        bounds: Option<crate::map::playfield::PlayfieldBounds>,
    ) -> Result<(u16, u16), String> {
        use crate::sim::cell_rect::{
            CellRef, cell_is_in_playfield_height_aware, get_cellclass_fallback,
        };
        use crate::util::direction::DIRECTION_DELTAS;

        let lookup = |coord: (u16, u16)| {
            get_cellclass_fallback(
                Some(terrain),
                i32::from(coord.0 as i16),
                i32::from(coord.1 as i16),
            )
        };
        let coord = |cell: &CellRef<'_>| match cell {
            CellRef::Real(cell) => (cell.rx, cell.ry),
            CellRef::Dummy { cell } => {
                let xy = cell.snapshot().coord;
                (xy.0 as u16, xy.1 as u16)
            }
        };
        let flags = |cell: &CellRef<'_>| match cell {
            CellRef::Real(cell) => cell.bridge_facts.raw_flags,
            CellRef::Dummy { cell } => cell.retained_bridge_flags(),
        };
        let is_exit = |cell: &CellRef<'_>| {
            matches!(cell, CellRef::Real(cell)
                if terrain.high_bridge_tile_offset(cell).is_some()
                && cell.yr_cell_land_type != LandType::Rock.as_index())
        };
        let step = |xy: (u16, u16), direction: usize| {
            let delta = DIRECTION_DELTAS[direction];
            (
                xy.0.wrapping_add(delta.0 as u16),
                xy.1.wrapping_add(delta.1 as u16),
            )
        };
        let mut positive = lookup(original);
        if flags(&positive) & 0x100 == 0 {
            return Ok(coord(&positive));
        }
        let direction = if flags(&positive) & 0x800 != 0 { 2 } else { 4 };
        let mut negative = positive.clone();
        let mut positive_exit = (0, 0);
        let mut negative_exit = (0, 0);
        let mut visited = std::collections::BTreeSet::new();
        loop {
            let state = (
                coord(&positive),
                matches!(positive, CellRef::Dummy { .. }),
                coord(&negative),
                matches!(negative, CellRef::Dummy { .. }),
            );
            if !visited.insert(state) {
                return Err("cyclic583820 bridge exit walk".into());
            }
            if flags(&positive) & 0x100 != 0 {
                positive = lookup(step(coord(&positive), direction));
                if flags(&positive) & 0x100 == 0 && is_exit(&positive) {
                    positive_exit = coord(&positive);
                }
            }
            if flags(&negative) & 0x100 != 0 {
                negative = lookup(step(coord(&negative), (direction + 4) & 7));
                if flags(&negative) & 0x100 == 0 && is_exit(&negative) {
                    negative_exit = coord(&negative);
                }
            }
            if flags(&positive) & 0x100 == 0 && flags(&negative) & 0x100 == 0 {
                break;
            }
        }
        // 583972..583AA0 probes six copied coordinates, including offsets
        // from the (0,0) sentinel. Each mode-1 bounds query executes in order.
        for candidate in [
            positive_exit,
            step(positive_exit, (direction + 2) & 7),
            step(positive_exit, (direction + 6) & 7),
            negative_exit,
            step(negative_exit, (direction + 2) & 7),
            step(negative_exit, (direction + 6) & 7),
        ] {
            if bounds.is_some_and(|bounds| {
                !cell_is_in_playfield_height_aware(
                    (i32::from(candidate.0 as i16), i32::from(candidate.1 as i16)),
                    Some(bounds),
                    Some(terrain),
                )
            }) {
                continue;
            }
            let found = self
                .hierarchy_zone_at_native(level, candidate)
                .ok_or("583820 requires the native hierarchy node")?;
            // The native load sign-extends the stored short, whereas its
            // requested zone argument is the zero-extended path identifier.
            if i32::from(found as i16) == i32::from(zone) {
                return Ok(candidate);
            }
        }
        Ok((0, 0))
    }

    pub(crate) fn movement_classes_match(&self, terrain: &ResolvedTerrainGrid) -> bool {
        let base = &self.base_topology;
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
        let class = zone_build::movement_class_for_cell(terrain, x, y);
        let level = terrain.cell(x, y).map_or(0, |cell| cell.level);
        if self.base_topology.movement_classes.get(index) == Some(&class)
            && self.base_topology.levels.get(index) == Some(&level)
        {
            return true;
        }
        self.invalidate_navigation_history();
        let base = &mut self.base_topology;
        let Some(slot) = base.movement_classes.get_mut(index) else {
            return false;
        };
        *slot = class;
        base.levels[index] = level;
        true
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn base_movement_class_at(&self, x: u16, y: u16) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.base_topology
            .movement_classes
            .get(y as usize * self.width as usize + x as usize)
            .copied()
    }

    pub(crate) fn base_topology_mut(&mut self) -> &mut zone_build::BaseZoneTopology {
        self.invalidate_navigation_history();
        &mut self.base_topology
    }

    /// Supplied raw Map+18 row premise in native query fixtures. This does
    /// not claim to reproduce the topology producer or its cluster numbering.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_supply_uniform_raw_zone_rows(&mut self, zone: ZoneId) {
        self.invalidate_navigation_history();
        for row in &mut self.base_topology.raw_zone_ids_by_row {
            row.fill(zone);
        }
        for index in 0..self.base_topology.zone_ids.len() {
            self.project_adopted_base_cell(index);
        }
    }

    pub(crate) fn base_and_hierarchy_mut(
        &mut self,
    ) -> (&zone_build::BaseZoneTopology, &mut ZoneHierarchy) {
        self.invalidate_navigation_history();
        (&self.base_topology, &mut self.hierarchy)
    }

    /// Project one adopted base cluster through the retained raw 13-row maps.
    /// No topology, count, adjacency, or unrelated cell is rewritten.
    pub(crate) fn project_adopted_base_cell(&mut self, cell_index: usize) {
        self.invalidate_navigation_history();
        let Some(&cluster) = self.base_topology.zone_ids.get(cell_index) else {
            return;
        };
        for &movement_zone in MovementZone::all_ground() {
            let row = movement_zone.matrix_row().expect("concrete movement row");
            let raw = self.base_topology.raw_zone_ids_by_row[row]
                .get(cluster as usize)
                .copied()
                .unwrap_or(u16::MAX);
            let projected = if raw > 1 && raw != u16::MAX {
                raw
            } else {
                ZONE_INVALID
            };
            if let Some(map) = self.maps.get_mut(&movement_zone) {
                map.set_ground_zone_at_index(cell_index, projected);
            }
        }
    }

    pub(crate) fn replace_hierarchy(&mut self, hierarchy: ZoneHierarchy) {
        self.invalidate_navigation_history();
        self.hierarchy = hierarchy;
    }

    /// Rebuild the products owned by the base connectivity pass while retaining
    /// the hierarchy's append-only identifiers for the following local patch.
    pub(crate) fn rebuild_base_connectivity_preserving_hierarchy(
        &mut self,
        path_grid: &PathGrid,
        resolved_terrain: &ResolvedTerrainGrid,
        bridge_records: &[crate::sim::bridge_state::BridgeEndpointRecord],
    ) {
        self.invalidate_navigation_history();
        let base_topology = zone_build::rebuild_base_zone_topology(
            self.base_topology.movement_classes.clone(),
            self.base_topology.levels.clone(),
            bridge_records,
            self.width,
            self.height,
            self.native_bridge_source_size,
        );
        self.maps = Self::project_movement_rows(
            &base_topology,
            path_grid,
            resolved_terrain,
            bridge_records,
            self.width,
            self.height,
        );
        self.base_topology = base_topology;
        self.bridge_records = bridge_records.to_vec();
    }

    /// Replace the one shared route-selection hierarchy (test fixtures only).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_hierarchy(&mut self, hierarchy: ZoneHierarchy) {
        self.invalidate_navigation_history();
        self.hierarchy = hierarchy;
    }

    /// O(1) reachability check: can a unit with this movement zone reach `to`
    /// from `from`?
    ///
    /// This derived-cache predicate compares layer labels for the existing
    /// graph clients. It does not implement the ordered raw-zone and mutable
    /// Dummy protocol of Map56D100; live native callers use can_reach_native.
    ///
    /// Not modelled, recorded: the native opens with `if (speed_type == -1)
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
        za != ZONE_INVALID && za == zone_map.zone_at(to.0, to.1, to_layer)
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

// Tests are declared in zone/mod.rs (zone_map_tests.rs).

#[cfg(test)]
#[path = "zone_history_hash_tests.rs"]
mod history_hash_tests;
