//! Zone map construction: flood-fill, adjacency extraction, zone info computation.
//!
//! Extracted from zone_map.rs to keep each file under ~400 lines.
//! This module is private to sim/ — public API lives in zone_map.rs.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/pathfinding, sim/terrain_cost, sim/locomotor.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::collections::VecDeque;

use super::PathGrid;
use super::passability;
use super::terrain_cost::TerrainCostGrid;
use super::zone_hierarchy::{ZoneEdgeRecord, ZoneHierarchy, ZoneLevelGraph, ZoneRecord};
use super::zone_map::{ZONE_INVALID, ZoneAdjacency, ZoneId, ZoneInfo, ZoneMap};
use crate::map::resolved_terrain::{ResolvedTerrainGrid, zone_class};
use crate::rules::locomotor_type::MovementZone;
use crate::rules::terrain_rules::LandType;
use crate::sim::bridge_state::BridgeEndpointRecord;
use crate::sim::movement::locomotor::MovementLayer;
use crate::util::native_x87::{X87Chop53, sqrt_approx_f32};

/// 8-directional neighbor offsets: (dx, dy, is_diagonal).
pub(crate) const NEIGHBORS: [(i32, i32, bool); 8] = [
    (0, -1, false), // N
    (1, -1, true),  // NE
    (1, 0, false),  // E
    (1, 1, true),   // SE
    (0, 1, false),  // S
    (-1, 1, true),  // SW
    (-1, 0, false), // W
    (-1, -1, true), // NW
];

/// Shared persistent topology projected through all 13 MovementZone rows.
#[derive(Debug, Clone)]
pub(crate) struct BaseZoneTopology {
    /// Derived record source Size, shared by full and incremental hierarchy use.
    pub(crate) native_bridge_source_size: Option<(i32, i32)>,
    pub(crate) movement_classes: Vec<u8>,
    /// Unsigned cached heights: Map+68 byte1 and Map+70 byte8. Original
    /// 47D2B0 publishes both with the class on every non-dummy Recalc exit;
    /// 56CB90 and hierarchy flood consume these, not current PathGrid heights.
    pub(crate) levels: Vec<u8>,
    pub(crate) zone_ids: Vec<ZoneId>,
    // Retained as exact base-topology state for incremental-repair parity
    // fixtures; current production projections consume the derived rows.
    pub(crate) zone_count: ZoneId,
    pub(crate) adjacency: ZoneAdjacency,
    /// Raw `MapClass+0x18[row][base_cluster]` values. Label 1 and `0xffff`
    /// remain represented here even though the flattened compatibility maps
    /// expose both as an invalid cell zone.
    pub(crate) raw_zone_ids_by_row: [Vec<ZoneId>; passability::ZONE_LAYER_COUNT],
}

struct BaseEdgeBuckets {
    buckets: Vec<Vec<(ZoneId, ZoneId)>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HierarchyTempEdge {
    existing: ZoneId,
    current: ZoneId,
    flag: u8,
}

/// Native temporary edge table used by `BuildZoneLevel`.
///
/// The directed `(existing, current)` pair is the identity: a later reversed
/// pair remains distinct, while reinserting the exact pair preserves its first
/// flag and discovery position. Buckets drain in ascending index order.
struct HierarchyEdgeBuckets {
    buckets: Vec<Vec<HierarchyTempEdge>>,
}

impl HierarchyEdgeBuckets {
    fn new() -> Self {
        Self {
            buckets: vec![Vec::new(); 256],
        }
    }

    fn register(&mut self, existing: ZoneId, current: ZoneId, flag: u8) {
        let bucket = (((existing & 0x0f) << 4) | (current & 0x0f)) as usize;
        if self.buckets[bucket]
            .iter()
            .any(|edge| edge.existing == existing && edge.current == current)
        {
            return;
        }
        self.buckets[bucket].push(HierarchyTempEdge {
            existing,
            current,
            flag,
        });
    }

    fn drain_into(self, graph: &mut ZoneLevelGraph) {
        for bucket in self.buckets {
            for edge in bucket {
                graph.push_edge(edge.current, ZoneEdgeRecord::new(edge.existing, edge.flag));
                graph.push_edge(edge.existing, ZoneEdgeRecord::new(edge.current, edge.flag));
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct HierarchyBlock {
    x_min: i32,
    x_max: i32,
    y_min: i32,
    y_max: i32,
}

impl HierarchyBlock {
    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x_min && x <= self.x_max && y >= self.y_min && y <= self.y_max
    }
}

impl BaseEdgeBuckets {
    fn new() -> Self {
        Self {
            buckets: vec![Vec::new(); 256],
        }
    }

    fn register(&mut self, neighbor: ZoneId, current: ZoneId) {
        if neighbor == ZONE_INVALID || current == ZONE_INVALID || neighbor == current {
            return;
        }
        let bucket = (((neighbor & 0x0f) << 4) | (current & 0x0f)) as usize;
        let pair = (neighbor, current);
        if !self.buckets[bucket].contains(&pair) {
            self.buckets[bucket].push(pair);
        }
    }

    fn into_adjacency(self, zone_count: ZoneId) -> ZoneAdjacency {
        let mut adjacency = vec![Vec::new(); zone_count as usize + 1];
        for bucket in self.buckets {
            for (neighbor, current) in bucket {
                add_adjacency(&mut adjacency, neighbor, current);
            }
        }
        ZoneAdjacency::new(adjacency)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BridgeRecordFilter {
    /// The bridge loop inside `MapClass::RebuildZoneConnectivity @ 0x0056C510`
    /// gates only on the record's active byte at `+8` and never reads the kind
    /// field at `+0xC`.
    AllActive,
    /// `MapClass::FindBridgeRecord @ 0x0056DA10` skips records where
    /// `bridge_kind != 0`.
    ///
    /// **The `active` term [`bridge_record_matches`] prefixes is VERA-internal
    /// and gamemd has none** — `FindBridgeRecord` tests only the kind field and
    /// deliberately ignores whether the bridge is intact. Trigger: a query
    /// against a destroyed high bridge. Player effect: gamemd still redirects
    /// through the record; VERA would not, refusing a move retail accepts.
    /// Frequency: zero today — this variant is dead, and the live equivalent
    /// `find_high_bridge_record` correctly ignores activity. Downstream risk:
    /// it becomes a bridge move-refusal the moment anything wires this up.
    HighActiveOnly,
}

fn bridge_record_matches(record: &BridgeEndpointRecord, filter: BridgeRecordFilter) -> bool {
    record.active
        && match filter {
            BridgeRecordFilter::AllActive => true,
            BridgeRecordFilter::HighActiveOnly => record.is_high(),
        }
}

/// Build a zone map and adjacency graph for one MovementZone.
#[cfg(test)]
pub(crate) fn build_zone_map(
    path_grid: &PathGrid,
    cost_grid: Option<&TerrainCostGrid>,
    mz: MovementZone,
    width: u16,
    height: u16,
) -> (ZoneMap, ZoneAdjacency) {
    build_zone_map_with_terrain(path_grid, cost_grid, None, mz, width, height)
}

/// Build a zone map using shared base-zone semantics when resolved terrain is available.
/// Falls back to the older direct passable-cell flood-fill when resolved terrain is not
/// available (primarily tests and non-terrain-aware call sites).
pub(crate) fn build_zone_map_with_terrain(
    path_grid: &PathGrid,
    cost_grid: Option<&TerrainCostGrid>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    mz: MovementZone,
    width: u16,
    height: u16,
) -> (ZoneMap, ZoneAdjacency) {
    if let Some(terrain) = resolved_terrain {
        let base = build_base_zone_topology(path_grid, terrain, &[], width, height, None);
        return build_zone_map_from_base_topology(&base, mz, width, height);
    }

    let total = width as usize * height as usize;

    // -- Ground layer flood-fill --
    let mut zone_ids = vec![ZONE_INVALID; total];
    let mut next_zone: ZoneId = 1;

    // Row-major scan for deterministic zone assignment.
    for ry in 0..height {
        for rx in 0..width {
            let idx = ry as usize * width as usize + rx as usize;
            if zone_ids[idx] != ZONE_INVALID {
                continue;
            }
            if !is_passable(
                rx,
                ry,
                mz,
                path_grid,
                cost_grid,
                resolved_terrain,
                MovementLayer::Ground,
            ) {
                continue;
            }
            // BFS flood-fill from this cell.
            flood_fill(
                rx,
                ry,
                next_zone,
                &mut zone_ids,
                width,
                height,
                mz,
                path_grid,
                cost_grid,
                resolved_terrain,
                MovementLayer::Ground,
            );
            next_zone += 1;
        }
    }

    let zone_count = next_zone - 1;

    // -- Extract adjacency (ground only; bridge edges injected by caller) --
    let adj = extract_adjacency(&zone_ids, width, height, zone_count);

    let zone_info = compute_zone_info(&zone_ids, width, height, zone_count);

    let zone_map = ZoneMap::new(
        zone_ids, None, // bridge_redirect set by caller
        width, height, zone_count, zone_info,
    );

    (zone_map, adj)
}

/// Build the one native base topology shared by every MovementZone projection.
pub(crate) fn build_base_zone_topology(
    path_grid: &PathGrid,
    resolved_terrain: &ResolvedTerrainGrid,
    bridge_records: &[BridgeEndpointRecord],
    width: u16,
    height: u16,
    native_bridge_source_size: Option<(i32, i32)>,
) -> BaseZoneTopology {
    let movement_classes: Vec<u8> = (0..height)
        .flat_map(|ry| (0..width).map(move |rx| movement_class_for_cell(resolved_terrain, rx, ry)))
        .collect();

    rebuild_base_zone_topology(
        movement_classes,
        retained_levels_from_path(path_grid, width, height),
        bridge_records,
        width,
        height,
        native_bridge_source_size,
    )
}

/// Original56C510 consumes retained Map+68 class/height bytes. Recalc owns those
/// bytes; connectivity clears/rebuilds only the base IDs and derived rows.
/// Evidence: tools/spatial_oracle/bridge_connectivity.{py,json,meta.json}.
pub(crate) fn rebuild_base_zone_topology(
    movement_classes: Vec<u8>,
    levels: Vec<u8>,
    bridge_records: &[BridgeEndpointRecord],
    width: u16,
    height: u16,
    native_bridge_source_size: Option<(i32, i32)>,
) -> BaseZoneTopology {
    assert_eq!(
        movement_classes.len(),
        usize::from(width) * usize::from(height)
    );

    assert_eq!(levels.len(), movement_classes.len());
    let (zone_ids, zone_count, mut edge_buckets) =
        rebuild_node_indices(&movement_classes, &levels, width, height);
    register_bridge_base_edges(
        &mut edge_buckets,
        &zone_ids,
        bridge_records,
        width,
        native_bridge_source_size,
    );
    let adjacency = edge_buckets.into_adjacency(zone_count);

    let raw_zone_ids_by_row = std::array::from_fn(|row| {
        rebuild_zone_ids_for_movement_zone(
            &movement_classes,
            &zone_ids,
            zone_count,
            &adjacency.neighbors,
            MovementZone::all_ground()[row],
        )
    });

    BaseZoneTopology {
        native_bridge_source_size,
        movement_classes,
        levels,
        zone_ids,
        zone_count,
        adjacency,
        raw_zone_ids_by_row,
    }
}

// Build the one three-level hierarchy shared by every MovementZone row.
//
// `BuildZoneLevel` constructs levels coarse-to-fine (2, 1, 0), and
// `FloodFillScanline` partitions each level by copied base-node
// identity inside aligned blocks. This is deliberately separate from the base
// topology flood fill: its height thresholds, scan history, fringe flags, and
// temporary-edge ordering differ.
include!("hierarchy_build.rs");

/// Project the shared base topology through one exact MovementZone matrix row.
pub(crate) fn build_zone_map_from_base_topology(
    base: &BaseZoneTopology,
    movement_zone: MovementZone,
    width: u16,
    height: u16,
) -> (ZoneMap, ZoneAdjacency) {
    let row_index = movement_zone
        .matrix_row()
        .expect("ZoneGrid builds only the 13 concrete movement rows");
    let derived_by_base = &base.raw_zone_ids_by_row[row_index];
    let zone_ids: Vec<ZoneId> = base
        .zone_ids
        .iter()
        .map(|&base_id| {
            if base_id == ZONE_INVALID {
                return ZONE_INVALID;
            }
            let derived = derived_by_base[base_id as usize];
            (derived > 1 && derived != u16::MAX)
                .then_some(derived)
                .unwrap_or(ZONE_INVALID)
        })
        .collect();
    let zone_count = zone_ids.iter().copied().max().unwrap_or(ZONE_INVALID);

    // Exact derived IDs already are the reachability components. Distinct IDs
    // must not be reconnected by a second flat cell-boundary graph.
    let adj = ZoneAdjacency::new(vec![Vec::new(); zone_count as usize + 1]);
    let zone_info = compute_zone_info(&zone_ids, width, height, zone_count);
    let zone_map = ZoneMap::new(
        zone_ids, None, // bridge_redirect set by caller
        width, height, zone_count, zone_info,
    );

    (zone_map, adj)
}

pub(crate) fn movement_class_for_cell(
    resolved_terrain: &ResolvedTerrainGrid,
    x: u16,
    y: u16,
) -> u8 {
    resolved_terrain
        .cell(x, y)
        .map_or(zone_class::OUTSIDE, |cell| cell.zone_type)
}

/// Initial navigation construction snapshots already projected cell levels.
/// Subsequent Recalc publication updates only the affected retained cell.
fn retained_levels_from_path(path_grid: &PathGrid, width: u16, height: u16) -> Vec<u8> {
    (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| path_grid.cell(x, y).map_or(0, |cell| cell.ground_level))
        })
        .collect()
}

fn rebuild_node_indices(
    movement_classes: &[u8],
    levels: &[u8],
    width: u16,
    height: u16,
) -> (Vec<u16>, u16, BaseEdgeBuckets) {
    let mut node_indices = vec![0u16; movement_classes.len()];
    let mut next_node: u16 = 1;
    let mut edge_buckets = BaseEdgeBuckets::new();

    for ry in 0..height {
        let mut rx = 0i32;
        while rx < i32::from(width) {
            if rx < 0 {
                rx += 1;
                continue;
            }
            let idx = ry as usize * width as usize + rx as usize;
            if movement_classes[idx] == zone_class::OUTSIDE || node_indices[idx] != 0 {
                rx += 1;
                continue;
            }
            let run_advance = flood_fill_node_index(
                rx as u16,
                ry,
                next_node,
                movement_classes,
                &mut node_indices,
                levels,
                width,
                height,
                &mut edge_buckets,
            );
            next_node += 1;
            // Native rechecks the returned rightmost run cell; the ordinary
            // assigned-cell branch then advances past it.
            rx += run_advance;
        }
    }

    (node_indices, next_node.saturating_sub(1), edge_buckets)
}

fn flood_fill_node_index(
    start_x: u16,
    start_y: u16,
    node_id: u16,
    movement_classes: &[u8],
    node_indices: &mut [u16],
    levels: &[u8],
    width: u16,
    height: u16,
    edge_buckets: &mut BaseEdgeBuckets,
) -> i32 {
    let start_idx = start_y as usize * width as usize + start_x as usize;
    let movement_class = movement_classes[start_idx];
    let seed_x = i32::from(start_x);
    let seed_y = i32::from(start_y);
    let Some(mut carried_level) = base_level_at(levels, seed_x, seed_y, width, height) else {
        return 0;
    };

    // The native scan first walks left with a carried <=1 height reference.
    // Horizontal scans overwrite prior zone IDs; only vertical recursion tests
    // for an unassigned candidate.
    let mut left = seed_x;
    loop {
        let Some(index) = base_record_index(left, seed_y, width, height) else {
            break;
        };
        if movement_classes[index] != movement_class {
            break;
        }
        let Some(level) = base_level_at(levels, left, seed_y, width, height) else {
            break;
        };
        if (i16::from(level) - i16::from(carried_level)).abs() >= 2 {
            break;
        }
        node_indices[index] = node_id;
        carried_level = level;
        left -= 1;
    }
    register_scanline_edge(
        edge_buckets,
        left,
        seed_y,
        left + 1,
        seed_y,
        node_id,
        movement_class,
        node_indices,
        levels,
        width,
        height,
    );

    // Right starts at the seed but retains the level carried out of the left
    // scan. The retail comparison is strictly <4.
    let mut right = seed_x;
    loop {
        let Some(index) = base_record_index(right, seed_y, width, height) else {
            break;
        };
        if movement_classes[index] != movement_class {
            break;
        }
        let Some(level) = base_level_at(levels, right, seed_y, width, height) else {
            break;
        };
        if (i16::from(level) - i16::from(carried_level)).abs() >= 4 {
            break;
        }
        node_indices[index] = node_id;
        carried_level = level;
        right += 1;
    }
    register_scanline_edge(
        edge_buckets,
        right,
        seed_y,
        right - 1,
        seed_y,
        node_id,
        movement_class,
        node_indices,
        levels,
        width,
        height,
    );

    // Both adjacent-row scans include the two flanks [L-1, R+1]. Their
    // current-row reference is one cell right, clamped to R.
    let mut scan_x = left;
    let run_right = right - 1;
    while scan_x <= right {
        let candidate_y = seed_y - 1;
        let reference_x = (scan_x + 1).min(run_right);
        if let (Some(candidate), Some(candidate_level), Some(reference_level)) = (
            base_record_index(scan_x, candidate_y, width, height),
            base_level_at(levels, scan_x, candidate_y, width, height),
            base_level_at(levels, reference_x, seed_y, width, height),
        ) {
            let neighbor = node_indices[candidate];
            let height_allowed =
                (i16::from(candidate_level) - i16::from(reference_level)).abs() < 2;
            if neighbor == 0 && movement_classes[candidate] == movement_class && height_allowed {
                let child_advance = flood_fill_node_index(
                    scan_x as u16,
                    candidate_y as u16,
                    node_id,
                    movement_classes,
                    node_indices,
                    levels,
                    width,
                    height,
                    edge_buckets,
                );
                // Above advances beyond the returned child run.
                scan_x += child_advance + 1;
                continue;
            } else if neighbor != 0
                && neighbor != node_id
                && (height_allowed || movement_class == zone_class::IMPASSABLE)
            {
                edge_buckets.register(neighbor, node_id);
            }
        }
        scan_x += 1;
    }

    // Below uses the same range/reference mapping, but rechecks the returned
    // child run's rightmost cell once before advancing.
    scan_x = left;
    while scan_x <= right {
        let candidate_y = seed_y + 1;
        let reference_x = (scan_x + 1).min(run_right);
        if let (Some(candidate), Some(candidate_level), Some(reference_level)) = (
            base_record_index(scan_x, candidate_y, width, height),
            base_level_at(levels, scan_x, candidate_y, width, height),
            base_level_at(levels, reference_x, seed_y, width, height),
        ) {
            let neighbor = node_indices[candidate];
            let height_allowed =
                (i16::from(candidate_level) - i16::from(reference_level)).abs() < 2;
            if neighbor == 0 && movement_classes[candidate] == movement_class && height_allowed {
                let child_advance = flood_fill_node_index(
                    scan_x as u16,
                    candidate_y as u16,
                    node_id,
                    movement_classes,
                    node_indices,
                    levels,
                    width,
                    height,
                    edge_buckets,
                );
                scan_x += child_advance;
                continue;
            } else if neighbor != 0
                && neighbor != node_id
                && (height_allowed || movement_class == zone_class::IMPASSABLE)
            {
                edge_buckets.register(neighbor, node_id);
            }
        }
        scan_x += 1;
    }

    right - seed_x - 1
}

fn base_record_index(x: i32, y: i32, width: u16, height: u16) -> Option<usize> {
    if x < 0 || y < 0 || x >= i32::from(width) || y >= i32::from(height) {
        return None;
    }
    Some(y as usize * width as usize + x as usize)
}

fn base_level_at(levels: &[u8], x: i32, y: i32, width: u16, height: u16) -> Option<u8> {
    levels.get(base_record_index(x, y, width, height)?).copied()
}

#[allow(clippy::too_many_arguments)]
fn register_scanline_edge(
    edge_buckets: &mut BaseEdgeBuckets,
    candidate_x: i32,
    candidate_y: i32,
    reference_x: i32,
    reference_y: i32,
    current_zone: ZoneId,
    captured_class: u8,
    node_indices: &[u16],
    levels: &[u8],
    width: u16,
    height: u16,
) {
    let Some(candidate) = base_record_index(candidate_x, candidate_y, width, height) else {
        return;
    };
    let neighbor = node_indices[candidate];
    if neighbor == ZONE_INVALID || neighbor == current_zone {
        return;
    }
    let height_allowed = match (
        base_level_at(levels, candidate_x, candidate_y, width, height),
        base_level_at(levels, reference_x, reference_y, width, height),
    ) {
        (Some(candidate_level), Some(reference_level)) => {
            (i16::from(candidate_level) - i16::from(reference_level)).abs() < 2
        }
        _ => false,
    };
    if height_allowed || captured_class == zone_class::IMPASSABLE {
        edge_buckets.register(neighbor, current_zone);
    }
}

fn rebuild_zone_ids_for_movement_zone(
    movement_classes: &[u8],
    node_indices: &[u16],
    node_count: u16,
    node_adj: &[Vec<u16>],
    movement_zone: MovementZone,
) -> Vec<u16> {
    let mut node_movement_classes = vec![zone_class::OUTSIDE; node_count as usize + 1];
    for (&node, &movement_class) in node_indices.iter().zip(movement_classes.iter()) {
        if node != 0 && node_movement_classes[node as usize] == zone_class::OUTSIDE {
            node_movement_classes[node as usize] = movement_class;
        }
    }

    let Some(row_index) = movement_zone.matrix_row() else {
        return vec![0u16; node_count as usize + 1];
    };
    let row = passability::MOVEMENT_ZONE_PASSABILITY[row_index];
    let mut zone_id_by_node = vec![1u16; node_count as usize + 1];
    for node in 1..=node_count {
        let movement_class = node_movement_classes[node as usize] as usize;
        if row[movement_class] == 1 {
            zone_id_by_node[node as usize] = 0;
        }
    }

    let mut next_label: u16 = 2;
    for start_node in 1..=node_count {
        if zone_id_by_node[start_node as usize] != 0 {
            continue;
        }
        let mut queue = VecDeque::new();
        zone_id_by_node[start_node as usize] = next_label;
        queue.push_back(start_node);

        while let Some(cur) = queue.pop_front() {
            for &neighbor in &node_adj[cur as usize] {
                if zone_id_by_node[neighbor as usize] != 0 {
                    continue;
                }
                zone_id_by_node[neighbor as usize] = next_label;
                queue.push_back(neighbor);
            }
        }

        next_label += 1;
    }

    zone_id_by_node[0] = u16::MAX;
    zone_id_by_node
}

/// Check if a cell is passable for a given MovementZone.
///
/// This helper is still the direct passable-cell check used by the fallback and legacy
/// incremental paths. Terrain-aware full rebuilds now go through `MovementClass8` +
/// `nodeIndex` reconstruction instead.
pub(crate) fn is_passable(
    x: u16,
    y: u16,
    mz: MovementZone,
    path_grid: &PathGrid,
    cost_grid: Option<&TerrainCostGrid>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    _layer: MovementLayer,
) -> bool {
    // Buildings and static obstacles block ground movement regardless of land
    // type. Fly uses matrix row 9 and should not inherit ground PathGrid blocks.
    // Water zones skip this check since water cells are typically blocked in PathGrid.
    if !mz.is_water_mover() && mz != MovementZone::Fly && !path_grid.is_walkable(x, y) {
        return false;
    }

    // Primary check: passability matrix using land_type from resolved terrain.
    // Uses MovementZone (not SpeedType) for the passability lookup — this matches
    // the original engine's Can_Enter_Cell logic where MovementZone determines
    // which cells are passable, while SpeedType only affects movement speed.
    // Critical: SpeedType::Float maps to zone 9 (hover — everything passable),
    // but MovementZone::Water maps to zone 10 (water cells only).
    if let Some(terrain) = resolved_terrain {
        if let Some(cell) = terrain.cell(x, y) {
            if mz.is_water_mover() {
                return super::cell_entry::is_water_surface_cell_passable(cell, mz);
            }
            return passability::is_passable_for_zone(cell.zone_type, mz);
        }
    }

    if mz == MovementZone::Fly {
        return true;
    }

    // Fallback: TerrainCostGrid-based check (pre-matrix behavior).
    if mz.is_water_mover() {
        if let Some(cg) = cost_grid {
            cg.cost_at(x, y) > 0
        } else {
            false
        }
    } else {
        if let Some(cg) = cost_grid {
            cg.cost_at(x, y) > 0
        } else {
            true
        }
    }
}

/// BFS flood-fill on the ground layer.
///
/// Assigns `zone_id` to all passable cells reachable from `(start_x, start_y)`.
/// Height continuity: adjacent cells with ground_level difference > 1 form zone
/// boundaries (matches original engine flood-fill behavior).
pub(crate) fn flood_fill(
    start_x: u16,
    start_y: u16,
    zone_id: ZoneId,
    zone_ids: &mut [ZoneId],
    width: u16,
    height: u16,
    mz: MovementZone,
    path_grid: &PathGrid,
    cost_grid: Option<&TerrainCostGrid>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    layer: MovementLayer,
) {
    let mut queue = VecDeque::new();
    let start_idx = start_y as usize * width as usize + start_x as usize;
    zone_ids[start_idx] = zone_id;
    queue.push_back((start_x, start_y));

    while let Some((cx, cy)) = queue.pop_front() {
        for &(dx, dy, is_diagonal) in &NEIGHBORS {
            let nx = cx as i32 + dx;
            let ny = cy as i32 + dy;
            if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                continue;
            }
            let nx = nx as u16;
            let ny = ny as u16;
            let n_idx = ny as usize * width as usize + nx as usize;

            if zone_ids[n_idx] != ZONE_INVALID {
                continue;
            }
            if !is_passable(nx, ny, mz, path_grid, cost_grid, resolved_terrain, layer) {
                continue;
            }

            // Diagonal corner-cutting: both adjacent cardinals must be passable.
            if is_diagonal {
                let ax = (cx as i32 + dx) as u16;
                let ay = cy;
                let bx = cx;
                let by = (cy as i32 + dy) as u16;
                if !is_passable(ax, ay, mz, path_grid, cost_grid, resolved_terrain, layer)
                    || !is_passable(bx, by, mz, path_grid, cost_grid, resolved_terrain, layer)
                {
                    continue;
                }
            }

            // Height continuity: original engine enforces abs(h_diff) <= 1 in zone
            // flood-fill. Height jumps > 1 create zone boundaries so the zone system
            // never claims two cells are mutually reachable when A* would fail due to
            // a cliff. Only checked on the ground layer for land-based categories.
            if layer == MovementLayer::Ground {
                if let (Some(cur), Some(nbr)) = (path_grid.cell(cx, cy), path_grid.cell(nx, ny)) {
                    if (cur.ground_level as i16 - nbr.ground_level as i16).abs() > 1 {
                        continue;
                    }
                }
            }

            zone_ids[n_idx] = zone_id;
            queue.push_back((nx, ny));
        }
    }
}

/// Compute per-zone centroid and cell count from the ground zone ID array.
pub(crate) fn compute_zone_info(
    zone_ids: &[ZoneId],
    width: u16,
    _height: u16,
    zone_count: u16,
) -> Vec<ZoneInfo> {
    let mut sums: Vec<(u64, u64, u32)> = vec![(0, 0, 0); zone_count as usize];
    for (idx, &zid) in zone_ids.iter().enumerate() {
        if zid != ZONE_INVALID {
            let x = (idx % width as usize) as u64;
            let y = (idx / width as usize) as u64;
            let entry = &mut sums[zid as usize - 1];
            entry.0 += x;
            entry.1 += y;
            entry.2 += 1;
        }
    }
    sums.iter()
        .map(|&(sx, sy, count)| {
            if count == 0 {
                ZoneInfo::default()
            } else {
                ZoneInfo {
                    center: (
                        u16::try_from(sx / count as u64).unwrap_or(u16::MAX),
                        u16::try_from(sy / count as u64).unwrap_or(u16::MAX),
                    ),
                    cell_count: count,
                }
            }
        })
        .collect()
}

/// Extract adjacency from ground zone ID array (ground-layer only).
///
/// Bridge cross-zone edges are injected separately via `inject_bridge_adjacency`.
pub(crate) fn extract_adjacency(
    ground_zones: &[ZoneId],
    width: u16,
    height: u16,
    zone_count: u16,
) -> ZoneAdjacency {
    let mut adj_sets: Vec<Vec<ZoneId>> = vec![Vec::new(); zone_count as usize + 1];
    let w = width as usize;

    for ry in 0..height {
        for rx in 0..width {
            let idx = ry as usize * w + rx as usize;
            let z = ground_zones[idx];
            if z == ZONE_INVALID {
                continue;
            }
            for &(dx, dy) in &[(1i32, 0i32), (0, 1), (1, 1), (1, -1)] {
                let nx = rx as i32 + dx;
                let ny = ry as i32 + dy;
                if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                    continue;
                }
                let n_idx = ny as usize * w + nx as usize;
                let nz = ground_zones[n_idx];
                if nz != ZONE_INVALID && nz != z {
                    add_adjacency(&mut adj_sets, z, nz);
                }
            }
        }
    }

    ZoneAdjacency::new(adj_sets)
}

/// Native bridge-record consumer inside RebuildZoneConnectivity56C510:
/// reverse records, signed clamped native node lookup, canonical zone pair.
/// See PHASE3_CELL_ITERATION_BRIDGE_RECORDS_20260910.md and native vectors.
fn register_bridge_base_edges(
    edge_buckets: &mut BaseEdgeBuckets,
    ground_zones: &[ZoneId],
    bridge_records: &[BridgeEndpointRecord],
    width: u16,
    native_source_size: Option<(i32, i32)>,
) {
    for record in bridge_records.iter().rev().filter(|r| r.active) {
        let zone =
            |coord| bridge_endpoint_base_zone(ground_zones, width, native_source_size, coord);
        let (Some(a), Some(b)) = (zone(record.endpoint_a), zone(record.endpoint_b)) else {
            continue;
        };
        if a == b {
            continue;
        }
        let pair = (a.min(b), a.max(b));
        let bucket = usize::from(((pair.0 & 15) << 4) | (pair.1 & 15));
        // Native includes node0; class7 prevents traversal, but the edge is
        // retained in native bucket/adjacency state. Generic register excludes0.
        if !edge_buckets.buckets[bucket].contains(&pair) {
            edge_buckets.buckets[bucket].push(pair);
        }
    }
}

pub(crate) fn bridge_endpoint_base_zone(
    zones: &[ZoneId],
    rust_width: u16,
    source_size: Option<(i32, i32)>,
    (x, y): (u16, u16),
) -> Option<ZoneId> {
    let Some((w, h)) = source_size else {
        // Explicit compatibility topology without native Size provenance.
        return zones
            .get(usize::from(y) * usize::from(rust_width) + usize::from(x))
            .copied();
    };
    if rust_width == 0 {
        return None;
    }
    let (nx, ny) = native_zone_grid_position((w, h), (x as i16, y as i16))?;
    let width = usize::from(rust_width);
    if nx as usize >= width || ny as usize >= zones.len() / width {
        // InitZoneMap567110 initializes padding to class7, and the rebuild
        // leaves its base node0 after clearing every zone word.
        Some(0)
    } else {
        zones.get(ny as usize * width + nx as usize).copied()
    }
}

/// Shared56D3F0/56D430 signed packed-coordinate projection. Clamp the linear
/// index only: exterior X can alias a real cell on the next row.
pub(crate) fn native_zone_grid_position(size: (i32, i32), coord: (i16, i16)) -> Option<(i32, i32)> {
    let side = size.0.wrapping_add(size.1).wrapping_add(1);
    let count = side.checked_mul(side)?;
    if side <= 0 || count <= 0 {
        return None;
    }
    let index = i32::from(coord.1)
        .wrapping_mul(side)
        .wrapping_add(i32::from(coord.0))
        .clamp(0, count - 1);
    Some((index % side, index / side))
}

pub(crate) fn projected_zone_record_index(
    width: u16,
    height: u16,
    source_size: Option<(i32, i32)>,
    coord: (i16, i16),
) -> Option<usize> {
    let (x, y) = if let Some(size) = source_size {
        native_zone_grid_position(size, coord)?
    } else {
        (i32::from(coord.0), i32::from(coord.1))
    };
    base_record_index(x, y, width, height)
}

/// Inject bridge adjacency edges into an existing adjacency graph.
///
/// For each active bridge endpoint record, connects the ground zones at
/// endpoint_a and endpoint_b — one undirected pair in the flat per-MovementZone
/// [`ZoneAdjacency`]. The native counterpart is the bridge loop inside
/// `MapClass::RebuildZoneConnectivity` @ `0x0056C510`.
///
/// It is **not** `MapClass::AddBridgeZoneEdges` @ `0x005851B0`, despite the
/// similar name: that one writes the three-level hierarchy graph at
/// `MapClass+0x90` in 0x24-byte zone records, and inserts six directed edges
/// per level from three cell pairs — (A, B), (A+dir, B+dir), (A−dir, B−dir) —
/// with flag byte 0. Porting this function against that citation would inject
/// spurious hierarchy edges and merge zones that are not connected.
pub(crate) fn inject_bridge_adjacency(
    adj: &mut ZoneAdjacency,
    ground_zones: &[ZoneId],
    bridge_records: &[BridgeEndpointRecord],
    width: u16,
    filter: BridgeRecordFilter,
) {
    let w = width as usize;
    for record in bridge_records {
        if !bridge_record_matches(record, filter) {
            continue;
        }
        let (ax, ay) = record.endpoint_a;
        let (bx, by) = record.endpoint_b;

        let a_idx = ay as usize * w + ax as usize;
        let b_idx = by as usize * w + bx as usize;

        if a_idx >= ground_zones.len() || b_idx >= ground_zones.len() {
            continue;
        }

        let za = ground_zones[a_idx];
        let zb = ground_zones[b_idx];

        if za != ZONE_INVALID && zb != ZONE_INVALID && za != zb {
            if !adj.neighbors[za as usize].contains(&zb) {
                adj.neighbors[za as usize].push(zb);
            }
            if !adj.neighbors[zb as usize].contains(&za) {
                adj.neighbors[zb as usize].push(za);
            }
        }
    }
}

/// Starting at `start_index`, return the first high-bridge record whose axis
/// covers `query` within the requested perpendicular tolerance. Record
/// activity is deliberately ignored, matching `MapClass::FindBridgeRecord`
/// @ `0x0056DA10`.
///
/// Native compares the query against the endpoints in their stored order; it
/// does not normalise a reversed span. Active `ComputeBridgeZones @ 0x0056D6E0`
/// stores endpoint A at the scan cell and endpoint B after walking direction 2
/// (east) or 4 (south). Rust's retail-derived high-record builder uses exactly
/// those two `HIGH_BRIDGE_WALK_DIRECTION` values, so its active records are
/// monotone. Keeping the native comparisons here also makes injected/corrupt
/// reversed records fail exactly as `FindBridgeRecord @ 0x0056DA10` does.
pub(crate) fn find_high_bridge_record(
    bridge_records: &[BridgeEndpointRecord],
    start_index: usize,
    query: (u16, u16),
    tolerance: u16,
) -> Option<&BridgeEndpointRecord> {
    find_high_bridge_record_index(bridge_records, start_index, query, tolerance)
        .map(|index| &bridge_records[index])
}

pub(crate) fn find_high_bridge_record_index(
    bridge_records: &[BridgeEndpointRecord],
    start_index: usize,
    query: (u16, u16),
    tolerance: u16,
) -> Option<usize> {
    bridge_records
        .iter()
        .enumerate()
        .skip(start_index)
        .find(|(_, record)| {
            if !record.is_high() {
                return false;
            }
            //56DA10 compares signed endpoint words and widens before distance.
            let (ax, ay) = (
                i32::from(record.endpoint_a.0 as i16),
                i32::from(record.endpoint_a.1 as i16),
            );
            let (bx, by) = (
                i32::from(record.endpoint_b.0 as i16),
                i32::from(record.endpoint_b.1 as i16),
            );
            let (qx, qy) = (i32::from(query.0 as i16), i32::from(query.1 as i16));
            if ax == bx {
                qy >= ay && qy <= by && (qx - ax).abs() <= i32::from(tolerance)
            } else {
                qx >= ax && qx <= bx && (qy - ay).abs() <= i32::from(tolerance)
            }
        })
        .map(|(index, _)| index)
}

///583180/5835D0 subtract packed words before floating distance and retain
/// only the signed low word of Math_ftol for comparison.
pub(crate) fn native_packed_cell_distance(lhs: (u16, u16), rhs: (u16, u16)) -> i16 {
    let dx = X87Chop53::load_i32(i32::from(lhs.0.wrapping_sub(rhs.0) as i16));
    let dy = X87Chop53::load_i32(i32::from(lhs.1.wrapping_sub(rhs.1) as i16));
    let squared = X87Chop53::add(X87Chop53::mul(dx, dx), X87Chop53::mul(dy, dy));
    let root = sqrt_approx_f32(squared).expect("packed distance is finite");
    let value = X87Chop53::load_f32(root).expect("packed distance root is finite");
    X87Chop53::ftol_i64(value).expect("packed distance fits integer") as i16
}

/// Build the exact per-cell bridge redirect used by bridge-aware zone lookup.
/// Nonstructural cells a high record reaches resolve to themselves. Structural
/// cells require the first matching high record at tolerance one. Every other
/// cell is left absent, which reads as "no bridge layer here".
pub(crate) fn build_bridge_redirect(
    _path_grid: &PathGrid,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    bridge_records: &[BridgeEndpointRecord],
    width: u16,
    height: u16,
) -> Option<Vec<Option<(u16, u16)>>> {
    let Some(terrain) = resolved_terrain else {
        return None;
    };

    let total = width as usize * height as usize;
    let mut redirect: Vec<Option<(u16, u16)>> = vec![None; total];
    let mut has_structural_cell = false;

    for ry in 0..height {
        for rx in 0..width {
            let Some(cell) = terrain.cell(rx, ry) else {
                continue;
            };
            let idx = ry as usize * width as usize + rx as usize;
            if !cell.bridge_facts.has_structural_bridge() {
                // A nonstructural cell keeps its own ground zone on the bridge
                // layer only where a high record actually reaches it — the deck
                // approach and its endpoint cells. Cells no high record covers
                // have no bridge layer at all; that includes every cell of a low
                // bridge, which is ground movement rather than deck movement.
                if find_high_bridge_record(bridge_records, 0, (rx, ry), 1).is_some() {
                    redirect[idx] = Some((rx, ry));
                }
                continue;
            }
            has_structural_cell = true;

            redirect[idx] = bridge_redirect_for_structural_cell(terrain, bridge_records, (rx, ry));
        }
    }

    has_structural_cell.then_some(redirect)
}

/// `MapClass::GetZoneID` 0x0056D230, bridge branch. Verified term by term
/// 2026-08-19: a structural cell (`+0x140` bit 0x100) must match the first
/// high `BridgeRecord` at **tolerance 1**; an intact record always resolves
/// through endpoint A; an inactive record walks east or south along the
/// record axis until the cell stops being structural and picks endpoint B
/// only when the exit is still a concrete or wood bridge tile whose LandType
/// is not Rock, otherwise A.
///
/// This is NOT `MapClass::ResolvePathCoord_BridgeAware` 0x00583180, which is
/// the live hierarchy resolver in zone_search::live_hierarchy_projection.
/// This legacy cache remains side-effect free; live queries own dummy writes.
fn bridge_redirect_for_structural_cell(
    terrain: &ResolvedTerrainGrid,
    bridge_records: &[BridgeEndpointRecord],
    query: (u16, u16),
) -> Option<(u16, u16)> {
    let record = find_high_bridge_record(bridge_records, 0, query, 1)?;
    if record.active {
        return Some(record.endpoint_a);
    }

    let direction = if record.endpoint_a.0 == record.endpoint_b.0 {
        4
    } else {
        2
    };
    let mut cursor = query;
    loop {
        let Some(next) = terrain.step_coord_by_direction(cursor, direction) else {
            return Some(record.endpoint_a);
        };
        let Some(cell) = terrain.cell(next.0, next.1) else {
            return Some(record.endpoint_a);
        };
        cursor = next;
        if cell.bridge_facts.has_structural_bridge() {
            continue;
        }
        return Some(
            if terrain.high_bridge_tile_offset(cell).is_some()
                && cell.yr_cell_land_type != LandType::Rock.as_index()
            {
                record.endpoint_b
            } else {
                record.endpoint_a
            },
        );
    }
}

/// Add a bidirectional adjacency edge, preserving first discovery order.
pub(crate) fn add_adjacency(adj: &mut [Vec<ZoneId>], a: ZoneId, b: ZoneId) {
    if !adj[a as usize].contains(&b) {
        adj[a as usize].push(b);
    }
    if !adj[b as usize].contains(&a) {
        adj[b as usize].push(a);
    }
}

#[cfg(test)]
pub(crate) use tests::{
    assert_native_hierarchy_graphs, hierarchy_native_bounds, hierarchy_native_fixture,
};

#[cfg(test)]
mod tests {
    include!("bridge_hierarchy_native_tests.rs");
    include!("tube_hierarchy_native_tests.rs");
    use super::*;
    include!("bridge_base_native_tests.rs");
    use crate::map::resolved_terrain::ResolvedTerrainCell;
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
    use crate::sim::bridge_state::{BridgeEndpointRecord, BridgeRecordKind};

    // Existing projection regressions now invoke the sole production owner.
    // No configured bounds in these synthetic fixtures means membership is supplied.
    fn project_live_cell_for_test(
        terrain: &ResolvedTerrainGrid,
        records: &[BridgeEndpointRecord],
        query: (u16, u16),
        enabled: bool,
    ) -> (u16, u16) {
        let cell = crate::sim::cell_rect::get_cellclass_fallback(
            Some(terrain),
            i32::from(query.0 as i16),
            i32::from(query.1 as i16),
        );
        crate::sim::pathfinding::zone_search::live_hierarchy_projection(
            terrain, records, &cell, enabled, None,
        )
        .expect("fixture has an addressable terminating projection")
    }

    fn redirect_terrain(
        width: u16,
        height: u16,
        bridge_set_start: Option<u16>,
        wood_bridge_set_start: Option<u16>,
        mut configure: impl FnMut(&mut ResolvedTerrainCell),
    ) -> ResolvedTerrainGrid {
        let mut cells = Vec::with_capacity(usize::from(width) * usize::from(height));
        for ry in 0..height {
            for rx in 0..width {
                let mut cell = ResolvedTerrainCell {
                    rx,
                    ry,
                    source_tile_index: 0,
                    source_sub_tile: 0,
                    final_tile_index: 0,
                    final_sub_tile: 0,
                    is_wood_bridge_repair_tile: false,
                    level: 0,
                    filled_clear: false,
                    tileset_index: None,
                    land_type: LandType::Clear.as_index(),
                    yr_cell_land_type: LandType::Clear.as_index(),
                    slope_type: 0,
                    template_height: 0,
                    render_offset_x: 0,
                    render_offset_y: 0,
                    terrain_class: TerrainClass::Clear,
                    speed_costs: SpeedCostProfile::default(),
                    is_water: false,
                    is_cliff_like: false,
                    is_rough: false,
                    is_road: false,
                    accepts_smudge: false,
                    allows_tiberium: false,
                    height_in_pixels: 0,
                    variant: 0,
                    has_ramp: false,
                    canonical_ramp: None,
                    ground_walk_blocked: false,
                    terrain_object_blocks: false,
                    terrain_object_occupation: None,
                    overlay_blocks: false,
                    overlay_zone_type: None,
                    outside_playfield: false,
                    zone_type: zone_class::GROUND,
                    base_ground_walk_blocked: false,
                    base_build_blocked: false,
                    base_land_type: LandType::Clear.as_index(),
                    base_yr_cell_land_type: LandType::Clear.as_index(),
                    base_terrain_class: TerrainClass::Clear,
                    base_speed_costs: SpeedCostProfile::default(),
                    build_blocked: false,
                    has_bridge_deck: false,
                    bridge_walkable: false,
                    bridge_transition: false,
                    bridge_deck_level: 0,
                    bridge_layer: None,
                    bridge_facts: Default::default(),
                    tube_index: None,
                    radar_left: [0, 0, 0],
                    radar_right: [0, 0, 0],
                    has_damaged_data: false,
                    bridgehead_anchor_class_at_load: None,
                };
                configure(&mut cell);
                cells.push(cell);
            }
        }
        let mut terrain = ResolvedTerrainGrid::from_cells(width, height, cells);
        terrain.test_set_high_bridge_set_starts(bridge_set_start, wood_bridge_set_start);
        terrain
    }

    fn hierarchy_base(movement_classes: Vec<u8>, zone_ids: Vec<ZoneId>) -> BaseZoneTopology {
        assert_eq!(movement_classes.len(), zone_ids.len());
        let zone_count = zone_ids.iter().copied().max().unwrap_or(ZONE_INVALID);
        let adjacency = ZoneAdjacency::new(vec![Vec::new(); zone_count as usize + 1]);
        let raw_zone_ids_by_row = std::array::from_fn(|row| {
            rebuild_zone_ids_for_movement_zone(
                &movement_classes,
                &zone_ids,
                zone_count,
                &adjacency.neighbors,
                MovementZone::all_ground()[row],
            )
        });
        BaseZoneTopology {
            native_bridge_source_size: None,
            levels: vec![0; movement_classes.len()],
            movement_classes,
            zone_ids,
            zone_count,
            adjacency,
            raw_zone_ids_by_row,
        }
    }

    fn level_cell_ids(graph: &ZoneLevelGraph, width: u16, height: u16) -> Vec<ZoneId> {
        (0..height)
            .flat_map(|y| (0..width).map(move |x| graph.zone_at(x, y)))
            .collect()
    }

    fn bridge_record(kind: BridgeRecordKind) -> BridgeEndpointRecord {
        BridgeEndpointRecord {
            endpoint_a: (0, 0),
            endpoint_b: (4, 0),
            group_id: 1,
            active: true,
            bridge_kind: kind,
        }
    }

    #[test]
    fn gsi_04_12_topology_find_record_uses_tolerance_first_order_and_ignores_active() {
        let records = [
            BridgeEndpointRecord {
                endpoint_a: (2, 0),
                endpoint_b: (2, 4),
                group_id: 1,
                active: false,
                bridge_kind: BridgeRecordKind::High,
            },
            BridgeEndpointRecord {
                endpoint_a: (1, 0),
                endpoint_b: (1, 4),
                group_id: 2,
                active: true,
                bridge_kind: BridgeRecordKind::High,
            },
        ];

        assert_eq!(
            find_high_bridge_record(&records, 0, (1, 2), 1).map(|record| record.group_id),
            Some(1)
        );
        assert_eq!(
            find_high_bridge_record(&records, 0, (1, 2), 0).map(|record| record.group_id),
            Some(2)
        );
        assert_eq!(
            find_high_bridge_record(&records, 1, (1, 2), 1).map(|record| record.group_id),
            Some(2)
        );
        assert!(find_high_bridge_record(&records, 2, (1, 2), 1).is_none());
        assert!(find_high_bridge_record(&records, 0, (1, 5), 1).is_none());
    }

    #[test]
    fn playfield_bridge_record_lookup_preserves_native_stored_endpoint_order() {
        let records = [
            BridgeEndpointRecord {
                endpoint_a: (2, 4),
                endpoint_b: (2, 0),
                group_id: 1,
                active: true,
                bridge_kind: BridgeRecordKind::High,
            },
            BridgeEndpointRecord {
                endpoint_a: (4, 2),
                endpoint_b: (0, 2),
                group_id: 2,
                active: true,
                bridge_kind: BridgeRecordKind::High,
            },
        ];

        assert!(
            find_high_bridge_record(&records, 0, (2, 2), 0).is_none(),
            "0x0056DA10 does not normalize a reversed vertical A-to-B span"
        );
        assert!(
            find_high_bridge_record(&records, 1, (2, 2), 0).is_none(),
            "0x0056DA10 does not normalize a reversed horizontal A-to-B span"
        );
    }

    #[test]
    fn gsi_04_12_topology_intact_redirect_uses_a_nonstructural_self_missing_invalid() {
        let terrain = redirect_terrain(6, 1, None, None, |cell| {
            if (1..=3).contains(&cell.rx) || cell.rx == 5 {
                cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
            }
        });
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let records = [BridgeEndpointRecord {
            endpoint_a: (0, 0),
            endpoint_b: (4, 0),
            group_id: 1,
            active: true,
            bridge_kind: BridgeRecordKind::High,
        }];
        let redirect = build_bridge_redirect(&path_grid, Some(&terrain), &records, 6, 1).unwrap();

        assert_eq!(redirect[3], Some((0, 0)));
        assert_eq!(redirect[4], Some((4, 0)));
        assert_eq!(redirect[5], None);

        let zone_map = ZoneMap::new(vec![9, 1, 1, 1, 7, 5], Some(redirect), 6, 1, 9, vec![]);
        assert_eq!(zone_map.zone_at(3, 0, MovementLayer::Bridge), 9);
        assert_eq!(zone_map.zone_at(4, 0, MovementLayer::Bridge), 7);
        assert_eq!(zone_map.zone_at(5, 0, MovementLayer::Bridge), ZONE_INVALID);
    }

    /// A map carrying BOTH a high and a low bridge. The high bridge populates
    /// the redirect table, so the "no table at all" shortcut cannot carry this
    /// case — only the per-cell corridor gate can. Cells no high record reaches,
    /// including every cell of the low bridge, must have no bridge layer.
    ///
    /// Retail grounding: low bridges are ground movement, not deck movement.
    /// Every overlay index any low-bridge destruction path writes lies in
    /// LOBRDG01..LOBRDG26, and retail rulesmd gives all of those `Land=Road`
    /// with `NoUseTileLandType=true`; a low span never carries a deck.
    #[test]
    fn gsi_04_12_topology_low_bridge_cells_have_no_bridge_layer_beside_a_high_bridge() {
        let terrain = redirect_terrain(9, 1, None, None, |cell| {
            if (1..=2).contains(&cell.rx) {
                cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
            }
        });
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        // A high record covering 0..=3, and a Low record covering 6..=8. The Low
        // record must grant nothing: find_high_bridge_record filters on is_high().
        let records = [
            BridgeEndpointRecord {
                endpoint_a: (0, 0),
                endpoint_b: (3, 0),
                group_id: 1,
                active: true,
                bridge_kind: BridgeRecordKind::High,
            },
            BridgeEndpointRecord {
                endpoint_a: (6, 0),
                endpoint_b: (8, 0),
                group_id: 2,
                active: true,
                bridge_kind: BridgeRecordKind::Low,
            },
        ];
        let redirect = build_bridge_redirect(&path_grid, Some(&terrain), &records, 9, 1)
            .expect("a structural high cell must still produce a table");

        // Inside the high corridor: nonstructural cells self-redirect as before.
        assert_eq!(redirect[3], Some((3, 0)));
        // Outside it: no bridge layer, even though a Low record covers these.
        assert_eq!(redirect[6], None);
        assert_eq!(redirect[7], None);
        assert_eq!(redirect[8], None);

        let zone_map = ZoneMap::new(
            vec![9, 1, 1, 7, 4, 4, 5, 5, 5],
            Some(redirect),
            9,
            1,
            9,
            vec![],
        );
        assert_eq!(zone_map.zone_at(3, 0, MovementLayer::Bridge), 7);
        for rx in 6..=8 {
            assert_eq!(
                zone_map.zone_at(rx, 0, MovementLayer::Bridge),
                ZONE_INVALID,
                "low-bridge cell {rx} must not report a bridge layer"
            );
            // The ground layer is unaffected.
            assert_eq!(zone_map.zone_at(rx, 0, MovementLayer::Ground), 5);
        }
    }

    #[test]
    fn gsi_04_12_topology_destroyed_redirect_chooses_b_only_for_nonrock_bridge_exit() {
        let mut terrain = redirect_terrain(7, 1, Some(100), Some(200), |cell| {
            if (1..=2).contains(&cell.rx) {
                cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
            }
            if cell.rx == 3 {
                cell.final_tile_index = 206;
                cell.final_sub_tile = 4;
            }
        });
        let records = [BridgeEndpointRecord {
            endpoint_a: (0, 0),
            endpoint_b: (5, 0),
            group_id: 1,
            active: false,
            bridge_kind: BridgeRecordKind::High,
        }];

        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let redirect = build_bridge_redirect(&path_grid, Some(&terrain), &records, 7, 1).unwrap();
        assert_eq!(redirect[1], Some((5, 0)));

        terrain.cell_mut(3, 0).unwrap().yr_cell_land_type = LandType::Rock.as_index();
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let redirect = build_bridge_redirect(&path_grid, Some(&terrain), &records, 7, 1).unwrap();
        assert_eq!(redirect[1], Some((0, 0)));

        let exit = terrain.cell_mut(3, 0).unwrap();
        exit.yr_cell_land_type = LandType::Clear.as_index();
        exit.final_tile_index = 50;
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let redirect = build_bridge_redirect(&path_grid, Some(&terrain), &records, 7, 1).unwrap();
        assert_eq!(redirect[1], Some((0, 0)));
    }

    #[test]
    fn gsi_04_12_hierarchy_projection_preserves_vertical_and_horizontal_lanes() {
        let vertical = redirect_terrain(8, 7, None, None, |cell| {
            if cell.rx == 4 && cell.ry == 4 {
                cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
            }
        });
        let vertical_record = [BridgeEndpointRecord {
            endpoint_a: (3, 1),
            endpoint_b: (3, 5),
            group_id: 1,
            active: true,
            bridge_kind: BridgeRecordKind::High,
        }];
        assert_eq!(
            project_live_cell_for_test(&vertical, &vertical_record, (4, 4), true),
            (4, 5),
            "clear 0x800 preserves the X lane offset from endpoint A"
        );

        let horizontal = redirect_terrain(8, 7, None, None, |cell| {
            if cell.rx == 4 && cell.ry == 4 {
                cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
                    | crate::map::bridge_facts::BRIDGE_FLAG_DIRECTION_ZERO;
            }
        });
        let horizontal_record = [BridgeEndpointRecord {
            endpoint_a: (1, 3),
            endpoint_b: (5, 3),
            group_id: 1,
            active: true,
            bridge_kind: BridgeRecordKind::High,
        }];
        assert_eq!(
            project_live_cell_for_test(&horizontal, &horizontal_record, (4, 4), true),
            (5, 4),
            "set 0x800 preserves the Y lane offset from endpoint A"
        );
    }

    #[test]
    fn gsi_04_12_hierarchy_projection_measures_lane_adjusted_endpoints() {
        let terrain = redirect_terrain(7, 6, None, None, |cell| {
            if (cell.rx, cell.ry) == (1, 2) {
                cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
            }
        });
        let records = [BridgeEndpointRecord {
            endpoint_a: (3, 1),
            endpoint_b: (3, 4),
            group_id: 1,
            active: true,
            bridge_kind: BridgeRecordKind::High,
        }];

        // Native projects both endpoints onto X=1 before measuring: distances
        // 1 and2 select A. The obsolete raw-endpoint sqrt5/sqrt8 tie was wrong.
        // Original path_entry active_lane_adjusted_distance pins this fixture.
        assert_eq!(
            project_live_cell_for_test(&terrain, &records, (1, 2), true),
            (1, 1)
        );
    }

    #[test]
    fn gsi_04_12_hierarchy_projection_disabled_or_nonstructural_is_raw() {
        let terrain = redirect_terrain(6, 3, None, None, |cell| {
            if (cell.rx, cell.ry) == (2, 1) {
                cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
                    | crate::map::bridge_facts::BRIDGE_FLAG_DIRECTION_ZERO;
            }
        });
        let records = [BridgeEndpointRecord {
            endpoint_a: (0, 1),
            endpoint_b: (5, 1),
            group_id: 1,
            active: true,
            bridge_kind: BridgeRecordKind::High,
        }];

        assert_eq!(
            project_live_cell_for_test(&terrain, &records, (2, 1), false),
            (2, 1)
        );
        assert_eq!(
            project_live_cell_for_test(&terrain, &records, (2, 0), true),
            (2, 0)
        );
    }

    #[test]
    fn gsi_04_12_hierarchy_projection_inactive_uses_east_axis_exit() {
        let mut terrain = redirect_terrain(8, 5, Some(100), None, |cell| {
            if cell.ry == 2 && (2..=3).contains(&cell.rx) {
                cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
                    | crate::map::bridge_facts::BRIDGE_FLAG_DIRECTION_ZERO;
            }
            if (cell.rx, cell.ry) == (4, 2) {
                cell.final_tile_index = 100;
            }
        });
        let records = [BridgeEndpointRecord {
            endpoint_a: (1, 2),
            endpoint_b: (6, 2),
            group_id: 1,
            active: false,
            bridge_kind: BridgeRecordKind::High,
        }];

        assert_eq!(
            project_live_cell_for_test(&terrain, &records, (2, 2), true),
            (6, 2)
        );
        terrain.cell_mut(4, 2).unwrap().yr_cell_land_type = LandType::Rock.as_index();
        assert_eq!(
            project_live_cell_for_test(&terrain, &records, (2, 2), true),
            (1, 2)
        );
    }

    #[test]
    fn gsi_04_12_hierarchy_projection_no_record_ties_choose_east_and_south() {
        let horizontal = redirect_terrain(7, 5, Some(100), None, |cell| {
            if cell.ry == 2 && (2..=4).contains(&cell.rx) {
                cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
                    | crate::map::bridge_facts::BRIDGE_FLAG_DIRECTION_ZERO;
            }
            if cell.ry == 2 && (cell.rx == 1 || cell.rx == 5) {
                cell.final_tile_index = 100;
            }
        });
        assert_eq!(
            project_live_cell_for_test(&horizontal, &[], (3, 2), true),
            (5, 2)
        );

        let vertical = redirect_terrain(5, 7, Some(100), None, |cell| {
            if cell.rx == 2 && (2..=4).contains(&cell.ry) {
                cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
            }
            if cell.rx == 2 && (cell.ry == 1 || cell.ry == 5) {
                cell.final_tile_index = 100;
            }
        });
        assert_eq!(
            project_live_cell_for_test(&vertical, &[], (2, 3), true),
            (2, 5)
        );
    }

    fn unique_hierarchy_base(width: u16, height: u16) -> BaseZoneTopology {
        let zone_ids: Vec<ZoneId> = (1..=width * height).collect();
        hierarchy_base(vec![zone_class::GROUND; zone_ids.len()], zone_ids)
    }

    fn assert_zero_edge(graph: &ZoneLevelGraph, a: (u16, u16), b: (u16, u16)) {
        let za = graph.zone_at(a.0, a.1);
        let zb = graph.zone_at(b.0, b.1);
        assert!(
            graph
                .edges(za)
                .iter()
                .any(|edge| edge.neighbor == zb && edge.flag == 0),
            "missing flag-0 edge {a:?} zone {za} -> {b:?} zone {zb}"
        );
    }

    #[test]
    fn gsi_04_12_hierarchy_geometry_inserts_all_three_pairs_at_all_levels() {
        let width = 7;
        let height = 5;
        let terrain = redirect_terrain(width, height, Some(100), None, |cell| {
            if (cell.rx, cell.ry) == (2, 2) {
                cell.final_tile_index = 100;
            }
        });
        let records = [BridgeEndpointRecord {
            endpoint_a: (2, 2),
            endpoint_b: (4, 2),
            group_id: 1,
            active: true,
            bridge_kind: BridgeRecordKind::High,
        }];
        let hierarchy = build_zone_hierarchy(
            &unique_hierarchy_base(width, height),
            Some(&terrain),
            &records,
            width,
            height,
        );

        for level in 0..3 {
            let graph = hierarchy.level(level).unwrap();
            assert_zero_edge(graph, (2, 2), (4, 2));
            assert_zero_edge(graph, (2, 1), (4, 1));
            assert_zero_edge(graph, (2, 3), (4, 3));
        }
    }

    #[test]
    fn gsi_04_06_local_hierarchy_bridge_injection_is_reverse_touch_filtered() {
        let width = 12;
        let height = 5;
        let terrain = redirect_terrain(width, height, Some(100), None, |cell| {
            if matches!((cell.rx, cell.ry), (2, 2) | (8, 2) | (2, 4)) {
                cell.final_tile_index = 100;
            }
        });
        let base = unique_hierarchy_base(width, height);
        let mut hierarchy = build_zone_hierarchy(&base, Some(&terrain), &[], width, height);
        let records = [
            BridgeEndpointRecord {
                endpoint_a: (2, 2),
                endpoint_b: (6, 2),
                group_id: 1,
                active: true,
                bridge_kind: BridgeRecordKind::High,
            },
            BridgeEndpointRecord {
                endpoint_a: (8, 2),
                endpoint_b: (11, 2),
                group_id: 2,
                active: true,
                bridge_kind: BridgeRecordKind::High,
            },
            BridgeEndpointRecord {
                endpoint_a: (2, 4),
                endpoint_b: (6, 4),
                group_id: 3,
                active: false,
                bridge_kind: BridgeRecordKind::High,
            },
        ];

        assert_eq!(
            incremental_rebuild_zone_hierarchy_around_cell(
                &mut hierarchy,
                &base,
                &terrain,
                &records,
                (2, 2),
                width,
                height,
            ),
            LocalHierarchyPatchResult::Patched
        );

        for level in 0..3 {
            let graph = hierarchy.level(level).unwrap();
            assert_zero_edge(graph, (2, 2), (6, 2));
            let untouched_a = graph.zone_at(8, 2);
            let untouched_b = graph.zone_at(11, 2);
            assert!(
                !graph
                    .edges(untouched_a)
                    .iter()
                    .any(|edge| edge.neighbor == untouched_b),
                "level {level}: record outside the aligned block must not inject"
            );
            let inactive_a = graph.zone_at(2, 4);
            let inactive_b = graph.zone_at(6, 4);
            assert!(
                !graph
                    .edges(inactive_a)
                    .iter()
                    .any(|edge| edge.neighbor == inactive_b),
                "level {level}: inactive touching record must not inject"
            );
        }
    }

    #[test]
    fn gsi_04_12_hierarchy_geometry_negative_table_entry_wraps_nw_and_se() {
        let width = 7;
        let height = 5;
        let terrain = redirect_terrain(width, height, Some(100), None, |cell| {
            if (cell.rx, cell.ry) == (2, 2) {
                cell.final_tile_index = 102;
            }
        });
        let records = [BridgeEndpointRecord {
            endpoint_a: (2, 2),
            endpoint_b: (4, 2),
            group_id: 1,
            active: true,
            bridge_kind: BridgeRecordKind::High,
        }];
        let hierarchy = build_zone_hierarchy(
            &unique_hierarchy_base(width, height),
            Some(&terrain),
            &records,
            width,
            height,
        );

        for level in 0..3 {
            let graph = hierarchy.level(level).unwrap();
            assert_zero_edge(graph, (1, 1), (3, 1));
            assert_zero_edge(graph, (3, 3), (5, 3));
        }
    }

    #[test]
    fn gsi_04_12_hierarchy_geometry_clamps_side_pair_linear_indices_at_boundaries() {
        // Original corpus high_native_boundary_negative/padding uses this
        // Size2,2 square: native stride5 with a final zero padding row/column.
        let width = 4;
        let height = 4;
        let terrain = redirect_terrain(width, height, Some(100), None, |cell| {
            if matches!((cell.rx, cell.ry), (0, 0) | (1, 1)) {
                cell.final_tile_index = 102;
            }
        });
        let records = [
            BridgeEndpointRecord {
                endpoint_a: (0, 0),
                endpoint_b: (2, 1),
                group_id: 1,
                active: true,
                bridge_kind: BridgeRecordKind::High,
            },
            BridgeEndpointRecord {
                endpoint_a: (1, 1),
                endpoint_b: (3, 3),
                group_id: 2,
                active: true,
                bridge_kind: BridgeRecordKind::High,
            },
        ];
        let zone_ids: Vec<ZoneId> = (1..=width * height).collect();
        let mut buckets = HierarchyEdgeBuckets::new();
        register_high_bridge_hierarchy_edges(
            &mut buckets,
            &zone_ids,
            &terrain,
            &records,
            width,
            height,
            Some((2, 2)),
        );
        let mut graph = ZoneLevelGraph::new(width * height);
        buckets.drain_into(&mut graph);

        assert!(
            graph.edges(1).contains(&ZoneEdgeRecord::new(2, 0)),
            "NW (-1,-1) must clamp its negative linear index to the first zone cell"
        );
        assert!(
            graph.edges(11).contains(&ZoneEdgeRecord::new(0, 0)),
            "SE (4,4) selects native padding zone0, not the last materialized cell"
        );
    }

    #[test]
    fn gsi_04_12_hierarchy_geometry_keeps_scanline_flag_order_and_skips_inactive() {
        let terrain = redirect_terrain(5, 3, Some(100), None, |cell| {
            if (cell.rx, cell.ry) == (1, 1) || (cell.rx, cell.ry) == (0, 1) {
                cell.final_tile_index = 100;
            }
        });
        let mut zone_ids = vec![ZONE_INVALID; 15];
        zone_ids[6] = 1;
        zone_ids[8] = 2;
        zone_ids[1] = 3;
        zone_ids[3] = 4;
        zone_ids[11] = 5;
        zone_ids[13] = 6;
        zone_ids[5] = 7;
        zone_ids[9] = 8;
        let records = [
            BridgeEndpointRecord {
                endpoint_a: (1, 1),
                endpoint_b: (3, 1),
                group_id: 1,
                active: true,
                bridge_kind: BridgeRecordKind::High,
            },
            BridgeEndpointRecord {
                endpoint_a: (0, 1),
                endpoint_b: (4, 1),
                group_id: 2,
                active: false,
                bridge_kind: BridgeRecordKind::High,
            },
        ];
        let mut buckets = HierarchyEdgeBuckets::new();
        buckets.register(1, 2, 1);
        register_high_bridge_hierarchy_edges(
            &mut buckets,
            &zone_ids,
            &terrain,
            &records,
            5,
            3,
            None,
        );
        let mut graph = ZoneLevelGraph::new(8);
        buckets.drain_into(&mut graph);

        assert_eq!(graph.edges(1), &[ZoneEdgeRecord::new(2, 1)]);
        assert_eq!(graph.edges(2), &[ZoneEdgeRecord::new(1, 1)]);
        assert_eq!(graph.edges(3), &[ZoneEdgeRecord::new(4, 0)]);
        assert_eq!(graph.edges(4), &[ZoneEdgeRecord::new(3, 0)]);
        assert_eq!(graph.edges(5), &[ZoneEdgeRecord::new(6, 0)]);
        assert_eq!(graph.edges(6), &[ZoneEdgeRecord::new(5, 0)]);
        assert!(
            graph.edges(7).is_empty(),
            "inactive record must add no edge"
        );
        assert!(
            graph.edges(8).is_empty(),
            "inactive record must add no edge"
        );
    }

    #[test]
    fn gsi_04_06_hierarchy_uses_aligned_blocks_and_coarse_parents() {
        let width = 9;
        let height = 1;
        let base = hierarchy_base(vec![zone_class::GROUND; 9], vec![1; 9]);
        let hierarchy = build_zone_hierarchy(&base, None, &[], width, height);

        let level2 = hierarchy.level(2).unwrap();
        assert_eq!(level2.zone_count(), 2);
        assert_eq!(
            level_cell_ids(level2, width, height),
            vec![1, 1, 1, 1, 1, 1, 1, 1, 2]
        );
        assert_eq!(
            level2.record(0),
            Some(ZoneRecord::new(0, 0, zone_class::OUTSIDE))
        );
        assert_eq!(
            level2.record(1),
            Some(ZoneRecord::new(1, 0, zone_class::GROUND))
        );
        assert_eq!(
            level2.record(2),
            Some(ZoneRecord::new(2, 0, zone_class::GROUND))
        );

        let level1 = hierarchy.level(1).unwrap();
        assert_eq!(level1.zone_count(), 3);
        assert_eq!(
            level_cell_ids(level1, width, height),
            vec![1, 1, 1, 1, 2, 2, 2, 2, 3]
        );
        assert_eq!(
            level1.record(1),
            Some(ZoneRecord::new(1, 1, zone_class::GROUND))
        );
        assert_eq!(
            level1.record(2),
            Some(ZoneRecord::new(2, 1, zone_class::GROUND))
        );
        assert_eq!(
            level1.record(3),
            Some(ZoneRecord::new(3, 2, zone_class::GROUND))
        );

        let level0 = hierarchy.level(0).unwrap();
        assert_eq!(level0.zone_count(), 5);
        assert_eq!(
            level_cell_ids(level0, width, height),
            vec![1, 1, 2, 2, 3, 3, 4, 4, 5]
        );
        for (zone, parent) in [(1, 1), (2, 1), (3, 2), (4, 2), (5, 3)] {
            assert_eq!(
                level0.record(zone),
                Some(ZoneRecord::new(zone, parent, zone_class::GROUND))
            );
        }
        assert_eq!(level0.edges(1), &[ZoneEdgeRecord::new(2, 0)]);
        assert_eq!(
            level0.edges(2),
            &[ZoneEdgeRecord::new(1, 0), ZoneEdgeRecord::new(3, 0)]
        );
        assert_eq!(
            level0.edges(3),
            &[ZoneEdgeRecord::new(2, 0), ZoneEdgeRecord::new(4, 0)]
        );
        assert_eq!(
            level0.edges(4),
            &[ZoneEdgeRecord::new(3, 0), ZoneEdgeRecord::new(5, 0)]
        );
        assert_eq!(level0.edges(5), &[ZoneEdgeRecord::new(4, 0)]);
    }

    #[test]
    fn gsi_04_06_hierarchy_vertical_fringe_preserves_flag_one() {
        let width = 4;
        let height = 2;
        let base = hierarchy_base(
            vec![
                zone_class::GROUND,
                zone_class::OUTSIDE,
                zone_class::BEACH,
                zone_class::BEACH,
                zone_class::GROUND,
                zone_class::GROUND,
                zone_class::BEACH,
                zone_class::BEACH,
            ],
            vec![1, 0, 2, 2, 1, 1, 2, 2],
        );
        let hierarchy = build_zone_hierarchy(&base, None, &[], width, height);

        let level0 = hierarchy.level(0).unwrap();
        assert_eq!(level0.zone_count(), 2);
        assert_eq!(
            level_cell_ids(level0, width, height),
            vec![1, 0, 2, 2, 1, 1, 2, 2]
        );
        assert_eq!(
            level0.record(1),
            Some(ZoneRecord::new(1, 1, zone_class::GROUND))
        );
        assert_eq!(
            level0.record(2),
            Some(ZoneRecord::new(2, 2, zone_class::BEACH))
        );
        assert_eq!(level0.edges(2), &[ZoneEdgeRecord::new(1, 1)]);
        assert_eq!(level0.edges(1), &[ZoneEdgeRecord::new(2, 1)]);

        for level in [1, 2] {
            let graph = hierarchy.level(level).unwrap();
            assert_eq!(graph.zone_count(), 2);
            assert_eq!(
                level_cell_ids(graph, width, height),
                vec![1, 0, 2, 2, 1, 1, 2, 2]
            );
            assert_eq!(graph.edges(2), &[ZoneEdgeRecord::new(1, 0)]);
            assert_eq!(graph.edges(1), &[ZoneEdgeRecord::new(2, 0)]);
        }
        assert_eq!(hierarchy.level(1).unwrap().record(1).unwrap().parent, 1);
        assert_eq!(hierarchy.level(1).unwrap().record(2).unwrap().parent, 2);
    }

    #[test]
    #[should_panic(expected = "zone hierarchy level 0 exceeds ZoneId capacity at real zone 65536")]
    fn gsi_04_06_hierarchy_rejects_65536th_real_zone_before_aliasing() {
        let width: u16 = 512;
        let height: u16 = 512;
        let cell_count = usize::from(width) * usize::from(height);
        let base = hierarchy_base(vec![zone_class::GROUND; cell_count], vec![1; cell_count]);

        let _ = build_zone_hierarchy(&base, None, &[], width, height);
    }

    #[test]
    fn gsi_04_06_hierarchy_edge_buckets_keep_directed_first_insertion() {
        let mut buckets = HierarchyEdgeBuckets::new();
        buckets.register(1, 2, 1);
        buckets.register(3, 2, 0);
        buckets.register(2, 1, 0);
        buckets.register(1, 2, 0);

        let mut graph = ZoneLevelGraph::new(3);
        buckets.drain_into(&mut graph);

        assert_eq!(
            graph.edges(1),
            &[ZoneEdgeRecord::new(2, 1), ZoneEdgeRecord::new(2, 0)]
        );
        assert_eq!(
            graph.edges(2),
            &[
                ZoneEdgeRecord::new(1, 1),
                ZoneEdgeRecord::new(1, 0),
                ZoneEdgeRecord::new(3, 0),
            ]
        );
        assert_eq!(graph.edges(3), &[ZoneEdgeRecord::new(2, 0)]);
    }

    #[test]
    fn gsi_04_06_scanline_run_entry_overwrites_prior_vertical_assignment() {
        let mut grid = PathGrid::new(2, 2);
        grid.set_cell_for_test(0, 0, 0, false, false);
        grid.set_cell_for_test(1, 0, 0, false, false);
        grid.set_cell_for_test(0, 1, 2, false, false);
        grid.set_cell_for_test(1, 1, 0, false, false);

        let movement_classes = vec![0; 4];
        let (zone_ids, zone_count, edge_buckets) = rebuild_node_indices(
            &movement_classes,
            &retained_levels_from_path(&grid, 2, 2),
            2,
            2,
        );
        let base = BaseZoneTopology {
            native_bridge_source_size: None,
            adjacency: edge_buckets.into_adjacency(zone_count),
            levels: retained_levels_from_path(&grid, 2, 2),
            movement_classes,
            zone_ids,
            zone_count,
            raw_zone_ids_by_row: std::array::from_fn(|_| Vec::new()),
        };

        assert_eq!(base.zone_ids, vec![1, 1, 2, 2]);
        assert_eq!(base.zone_count, 2);
        assert!(base.adjacency.are_adjacent(1, 2));
    }

    #[test]
    fn gsi_04_06_scanline_history_does_not_invent_final_cardinal_edge() {
        let mut grid = PathGrid::new(2, 2);
        for (index, level) in [2, 3, 1, 0].into_iter().enumerate() {
            grid.set_cell_for_test((index % 2) as u16, (index / 2) as u16, level, false, false);
        }

        let (zone_ids, zone_count, edge_buckets) =
            rebuild_node_indices(&[0; 4], &retained_levels_from_path(&grid, 2, 2), 2, 2);
        let adjacency = edge_buckets.into_adjacency(zone_count);

        assert_eq!(zone_ids, vec![1, 1, 2, 2]);
        assert_eq!(zone_count, 2);
        assert!(!adjacency.are_adjacent(1, 2));
    }

    #[test]
    fn gsi_04_06_scanline_storage_fringe_has_one_base_zone() {
        let mut grid = PathGrid::new(2, 2);
        for y in 0..2 {
            for x in 0..2 {
                grid.set_cell_for_test(x, y, 0, false, false);
            }
        }

        let (zone_ids, zone_count, _) = rebuild_node_indices(
            &[zone_class::GROUND, 7, 7, zone_class::GROUND],
            &retained_levels_from_path(&grid, 2, 2),
            2,
            2,
        );

        assert_eq!(zone_ids, vec![1, 0, 0, 1]);
        assert_eq!(zone_count, 1);
    }

    #[test]
    fn gsi_04_06_scanline_rejects_right_delta_four() {
        let mut grid = PathGrid::new(2, 1);
        grid.set_cell_for_test(0, 0, 0, false, false);
        grid.set_cell_for_test(1, 0, 4, false, false);

        let (zone_ids, zone_count, _) =
            rebuild_node_indices(&[0; 2], &retained_levels_from_path(&grid, 2, 1), 2, 1);

        assert_eq!(zone_ids, vec![1, 2]);
        assert_eq!(zone_count, 2);
    }

    #[test]
    fn bridge_redirect_ignores_low_bridge_records() {
        let mut grid = PathGrid::new(5, 1);
        for rx in 1..=3 {
            grid.set_cell_for_test(rx, 0, 0, true, false);
        }

        let records = [bridge_record(BridgeRecordKind::Low)];
        let redirect = build_bridge_redirect(&grid, None, &records, 5, 1);

        assert!(redirect.is_none());
    }

    #[test]
    fn bridge_adjacency_filter_all_active_includes_low_records() {
        let ground_zones = [1, ZONE_INVALID, ZONE_INVALID, ZONE_INVALID, 2];
        let records = [bridge_record(BridgeRecordKind::Low)];
        let mut adj = ZoneAdjacency::new(vec![vec![], vec![], vec![]]);

        inject_bridge_adjacency(
            &mut adj,
            &ground_zones,
            &records,
            5,
            BridgeRecordFilter::AllActive,
        );

        assert!(adj.are_adjacent(1, 2));
    }

    #[test]
    fn bridge_adjacency_filter_high_active_only_skips_low_records() {
        let ground_zones = [1, ZONE_INVALID, ZONE_INVALID, ZONE_INVALID, 2];
        let records = [bridge_record(BridgeRecordKind::Low)];
        let mut adj = ZoneAdjacency::new(vec![vec![], vec![], vec![]]);

        inject_bridge_adjacency(
            &mut adj,
            &ground_zones,
            &records,
            5,
            BridgeRecordFilter::HighActiveOnly,
        );

        assert!(!adj.are_adjacent(1, 2));
    }

    /// OPEN: accepted raw path tokens can address native process data outside
    /// the proven direction/zero slots; missing Tube pairs may change routes.
    /// See PHASE3_TUBE_HIERARCHY_20260910.md for trigger and bounded delivery.
    #[test]
    #[ignore = "429780 arbitrary raw direction-data and invalid registry reads remain unproved"]
    fn tube_hierarchy_raw_process_memory_domain_is_unresolved() {
        panic!("unresolved: raw process-memory reads accepted by7283C0 into429780");
    }

    /// `MapClass::RegisterBridgeOrTubeHierarchyPairs` 0x00582D70 enters its
    /// bridge branch on `CellClass::IsBridge` 0x00486750 **OR**
    /// `CellClass::IsWoodBridge` 0x00486770. VERA used to return early unless
    /// the record was High, so every wooden bridge contributed no hierarchy
    /// edge and long-range paths were planned as if it were not there.
    #[test]
    fn wood_bridge_records_register_hierarchy_edges_like_concrete_ones() {
        fn edges_for(kind: BridgeRecordKind, tile: i32, wood_base: Option<u16>) -> usize {
            let terrain = redirect_terrain(4, 1, Some(200), wood_base, |cell| {
                cell.final_tile_index = tile;
            });

            let record = BridgeEndpointRecord {
                endpoint_a: (0, 0),
                endpoint_b: (3, 0),
                group_id: 1,
                active: true,
                bridge_kind: kind,
            };
            let zone_ids: Vec<ZoneId> = vec![1, 2, 3, 4];
            let mut buckets = HierarchyEdgeBuckets::new();
            register_bridge_hierarchy_edges_for_record(
                &mut buckets,
                &zone_ids,
                &terrain,
                &record,
                4,
                1,
                None,
            );
            buckets.buckets.iter().map(|b| b.len()).sum()
        }

        // Wood tile (base 300, offset 0) on a Low record: the native registers,
        // and so must this.
        let wood = edges_for(BridgeRecordKind::Low, 300, Some(300));
        assert!(
            wood > 0,
            "a wooden bridge must contribute hierarchy edges; before the fix this was 0"
        );

        // Same geometry on a concrete tile and a High record, for comparison.
        let concrete = edges_for(BridgeRecordKind::High, 200, Some(300));
        assert_eq!(
            wood, concrete,
            "both branches register the same three pairs in 0x00582D70"
        );

        // A tile in neither tileset window still registers nothing - that is
        // where this synthetic record has no Tube to enter the other branch.
        let neither = edges_for(BridgeRecordKind::Low, 900, Some(300));
        assert_eq!(neither, 0, "no bridge tile, no bridge pairs");
    }
}
