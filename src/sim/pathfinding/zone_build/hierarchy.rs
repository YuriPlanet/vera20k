//! Build the one three-level hierarchy shared by every MovementZone row.
//!
//! `BuildZoneLevel` constructs levels coarse-to-fine (2, 1, 0), and
//! `FloodFillScanline` partitions each level by copied base-node
//! identity inside aligned blocks. This is deliberately separate from the base
//! topology flood fill: its height thresholds, scan history, fringe flags, and
//! temporary-edge ordering differ.

use super::{
    BaseZoneTopology, HierarchyBlock, HierarchyEdgeBuckets, HierarchyTempEdge, base_record_index,
    native_zone_grid_position,
};
use crate::map::resolved_terrain::{ResolvedTerrainGrid, zone_class};
use crate::sim::bridge_state::BridgeEndpointRecord;
use crate::sim::pathfinding::zone_hierarchy::{ZoneHierarchy, ZoneLevelGraph, ZoneRecord};
use crate::sim::pathfinding::zone_map::ZoneId;

// Original581F90/584550/5824A0 hierarchy production. Native comparisons:
// tools/spatial_oracle/bridge_hierarchy.{py,json,meta.json}.
// Retained classes/heights/base IDs and live playfield queries are separate
// inputs. A query may stamp the shared dummy, so preserve its call order.

#[derive(Clone, Copy)]
enum HierarchyCellIndex {
    Represented(usize),
    Padding(usize),
}

impl HierarchyCellIndex {
    fn zone(self, graph: &ZoneLevelGraph) -> ZoneId {
        match self {
            Self::Represented(i) => graph.cell_zone_ids()[i],
            Self::Padding(i) => graph.native_padding_zone(i),
        }
    }

    fn set_zone(self, graph: &mut ZoneLevelGraph, zone: ZoneId) {
        match self {
            Self::Represented(i) => graph.cell_zone_ids_mut()[i] = zone,
            Self::Padding(i) => graph.set_native_padding_zone(i, zone),
        }
    }
}

/// What the hierarchy reads at one position, from a single index computation.
/// Padding reads as an outside cell at level 0 with base ID 0; the level,
/// base ID and class answers for absent storage are the same.
#[derive(Clone, Copy)]
struct HierarchyCell {
    index: HierarchyCellIndex,
    level: u8,
    base_id: ZoneId,
    class: u8,
}

#[derive(Clone, Copy)]
pub(super) struct HierarchyCells<'a> {
    pub(super) base: &'a BaseZoneTopology,
    pub(super) width: u16,
    pub(super) height: u16,
    pub(super) native_scan_offset: Option<i32>,
}

impl HierarchyCells<'_> {
    fn index(self, x: i32, y: i32) -> Option<HierarchyCellIndex> {
        let Some(size) = self.base.native_bridge_source_size else {
            return base_record_index(x, y, self.width, self.height)
                .map(HierarchyCellIndex::Represented);
        };
        let side = size.0.wrapping_add(size.1).wrapping_add(1);
        let count = side.checked_mul(side)?;
        if side <= 0 || count <= 0 {
            return None;
        }
        let (nx, ny) = if let Some(offset) = self.native_scan_offset {
            //5824A0 walks raw neighboring pointers from its clamped seed.
            // Padding is valid storage. Only out-of-allocation access is absent.
            let index = y.wrapping_mul(side).wrapping_add(x).wrapping_add(offset);
            if !(0..count).contains(&index) {
                return None;
            }
            (index % side, index / side)
        } else {
            native_zone_grid_position(size, (x as i16, y as i16))?
        };
        Some(base_record_index(nx, ny, self.width, self.height).map_or(
            HierarchyCellIndex::Padding((ny * side + nx) as usize),
            HierarchyCellIndex::Represented,
        ))
    }

    fn cell(self, x: i32, y: i32) -> Option<HierarchyCell> {
        let index = self.index(x, y)?;
        let (level, base_id, class) = match index {
            HierarchyCellIndex::Represented(i) => (
                self.base.levels[i],
                self.base.zone_ids[i],
                self.base.movement_classes[i],
            ),
            HierarchyCellIndex::Padding(_) => (0, 0, zone_class::OUTSIDE),
        };
        Some(HierarchyCell {
            index,
            level,
            base_id,
            class,
        })
    }

    fn scan_from(mut self, seed: (i32, i32)) -> Self {
        if self.native_scan_offset.is_none()
            && let Some(size) = self.base.native_bridge_source_size
            && let Some((x, y)) = native_zone_grid_position(size, (seed.0 as i16, seed.1 as i16))
        {
            let side = size.0.wrapping_add(size.1).wrapping_add(1);
            self.native_scan_offset = Some(
                y.wrapping_sub(seed.1)
                    .wrapping_mul(side)
                    .wrapping_add(x.wrapping_sub(seed.0)),
            );
        }
        self
    }

    fn level(self, x: i32, y: i32) -> u8 {
        self.cell(x, y).map_or(0, |cell| cell.level)
    }

    fn base_id(self, x: i32, y: i32) -> ZoneId {
        self.cell(x, y).map_or(0, |cell| cell.base_id)
    }

    fn class(self, x: i32, y: i32) -> u8 {
        self.cell(x, y)
            .map_or(zone_class::OUTSIDE, |cell| cell.class)
    }

    fn zone(self, graph: &ZoneLevelGraph, x: i32, y: i32) -> ZoneId {
        self.index(x, y).map_or(0, |index| index.zone(graph))
    }

    fn set_zone(self, graph: &mut ZoneLevelGraph, x: i32, y: i32, zone: ZoneId) {
        if let Some(index) = self.index(x, y) {
            index.set_zone(graph, zone);
        }
    }
}

/// Existing construction without a live MapClass bounds owner. Live-world
/// query threading remains required; the explicit query implementation is shared.
pub(crate) fn build_zone_hierarchy(
    base: &BaseZoneTopology,
    terrain: Option<&ResolvedTerrainGrid>,
    records: &[BridgeEndpointRecord],
    width: u16,
    height: u16,
) -> ZoneHierarchy {
    let cells = HierarchyCells {
        base,
        width,
        height,
        native_scan_offset: None,
    };
    build_zone_hierarchy_with_query(base, terrain, records, width, height, &mut |x, y| {
        cells.class(x, y) != zone_class::OUTSIDE
    })
}

pub(crate) fn build_zone_hierarchy_with_query(
    base: &BaseZoneTopology,
    terrain: Option<&ResolvedTerrainGrid>,
    records: &[BridgeEndpointRecord],
    width: u16,
    height: u16,
    query: &mut impl FnMut(i32, i32) -> bool,
) -> ZoneHierarchy {
    let cells = HierarchyCells {
        base,
        width,
        height,
        native_scan_offset: None,
    };
    let level2 = build_hierarchy_level(cells, terrain, records, 2, None, query);
    let level1 = build_hierarchy_level(cells, terrain, records, 1, Some(&level2), query);
    let level0 = build_hierarchy_level(cells, terrain, records, 0, Some(&level1), query);
    ZoneHierarchy::new(level0, level1, level2)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalHierarchyPatchResult {
    Outside,
    Patched,
    NeedsFullRebuild,
}

/// Existing callers without live bounds retain cached-class admission here.
/// Their live mode1 wiring is a separate required delivery step.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn incremental_rebuild_zone_hierarchy_around_cell(
    hierarchy: &mut ZoneHierarchy,
    base: &BaseZoneTopology,
    terrain: &ResolvedTerrainGrid,
    records: &[BridgeEndpointRecord],
    coord: (i16, i16),
    width: u16,
    height: u16,
) -> LocalHierarchyPatchResult {
    let cells = HierarchyCells {
        base,
        width,
        height,
        native_scan_offset: None,
    };
    patch_zone_hierarchy_with_query(
        hierarchy,
        base,
        terrain,
        records,
        coord,
        width,
        height,
        &mut |x, y| cells.class(x, y) != zone_class::OUTSIDE,
    )
}

/// Original584550; never changes the retained base topology or13 movement rows.
pub(crate) fn patch_zone_hierarchy_with_query(
    hierarchy: &mut ZoneHierarchy,
    base: &BaseZoneTopology,
    terrain: &ResolvedTerrainGrid,
    records: &[BridgeEndpointRecord],
    coord: (i16, i16),
    width: u16,
    height: u16,
    query: &mut impl FnMut(i32, i32) -> bool,
) -> LocalHierarchyPatchResult {
    let (x, y) = (i32::from(coord.0), i32::from(coord.1));
    if !query(x, y) {
        return LocalHierarchyPatchResult::Outside;
    }
    let cells = HierarchyCells {
        base,
        width,
        height,
        native_scan_offset: None,
    };
    for level in (0..3).rev() {
        let size = 1i32 << (level + 1);
        let block = HierarchyBlock {
            x_min: x - x % size,
            x_max: x - x % size + size - 1,
            y_min: y - y % size,
            y_max: y - y % size + size - 1,
        };
        let (lower, upper) = hierarchy.levels_mut().split_at_mut(level + 1);
        if !patch_hierarchy_level(
            &mut lower[level],
            upper.first(),
            cells,
            terrain,
            records,
            block,
            query,
        ) {
            return LocalHierarchyPatchResult::NeedsFullRebuild;
        }
    }

    // This is a new mode1 query per cell, independent of cached class7.
    // Native writes parents of record0 too (584DF7), using the final live IDs.
    let levels = hierarchy.levels_mut();
    for by in y - y % 8..y - y % 8 + 8 {
        for bx in x - x % 8..x - x % 8 + 8 {
            if !query(bx, by) {
                continue;
            }
            let z0 = cells.zone(&levels[0], bx, by);
            let z1 = cells.zone(&levels[1], bx, by);
            let z2 = cells.zone(&levels[2], bx, by);
            levels[0].set_parent(z0, z1);
            levels[1].set_parent(z1, z2);
        }
    }
    LocalHierarchyPatchResult::Patched
}

fn patch_hierarchy_level(
    graph: &mut ZoneLevelGraph,
    parent: Option<&ZoneLevelGraph>,
    cells: HierarchyCells<'_>,
    terrain: &ResolvedTerrainGrid,
    records: &[BridgeEndpointRecord],
    block: HierarchyBlock,
    query: &mut impl FnMut(i32, i32) -> bool,
) -> bool {
    let mut buckets = HierarchyEdgeBuckets::new();
    let mut old_ids = Vec::new();
    // Native56D3F0 packs, linearizes and clamps; no playfield gate here.
    // A partial right block can alias represented cells on the next row.
    for y in block.y_min..=block.y_max {
        for x in block.x_min..=block.x_max {
            let old = cells.zone(graph, x, y);
            if old != 0 && !old_ids.iter().rev().any(|&seen| seen == old) {
                old_ids.push(old);
            }
            cells.set_zone(graph, x, y, 0);
        }
    }
    for &old in old_ids.iter().rev() {
        for edge in graph.edges(old).to_vec().iter().rev() {
            graph.remove_last_edge_to(edge.neighbor, old);
        }
        graph.clear_edges(old);
    }
    for y in block.y_min..=block.y_max {
        for x in block.x_min..=block.x_max {
            // Query precedes cached-class and ID checks, including padding.
            if !query(x, y)
                || cells.class(x, y) == zone_class::OUTSIDE
                || cells.zone(graph, x, y) != 0
            {
                continue;
            }
            let Ok(zone) = ZoneId::try_from(graph.record_slot_count()) else {
                return false;
            };
            let parent_id = parent.map_or(0, |parent| cells.zone(parent, x, y));
            if !graph.append_record(ZoneRecord::from_seed(
                zone,
                parent_id,
                cells.class(x, y),
                (x as i16, y as i16),
            )) {
                return false;
            }
            flood_fill_hierarchy_scanline(
                (x, y),
                zone,
                cells.base_id(x, y),
                cells,
                graph,
                block,
                &mut buckets,
                query,
            );
        }
    }
    for record in records.iter().rev() {
        if record.active
            && (hierarchy_block_contains_coord(block, record.endpoint_a)
                || hierarchy_block_contains_coord(block, record.endpoint_b))
        {
            register_bridge_hierarchy_edges_with_lookup(
                &mut buckets,
                terrain,
                record,
                &mut |(x, y)| cells.zone(graph, i32::from(x as i16), i32::from(y as i16)),
            );
        }
    }
    buckets.drain_into(graph);
    true
}

fn hierarchy_block_contains_coord(block: HierarchyBlock, coord: (u16, u16)) -> bool {
    block.contains(i32::from(coord.0 as i16), i32::from(coord.1 as i16))
}

fn build_hierarchy_level(
    cells: HierarchyCells<'_>,
    terrain: Option<&ResolvedTerrainGrid>,
    records: &[BridgeEndpointRecord],
    level: usize,
    parent: Option<&ZoneLevelGraph>,
    query: &mut impl FnMut(i32, i32) -> bool,
) -> ZoneLevelGraph {
    let mut graph = ZoneLevelGraph::new(0).with_cell_zone_ids(
        vec![0; usize::from(cells.width) * usize::from(cells.height)],
        cells.width,
        cells.height,
    );
    graph.set_record(ZoneRecord::new(0, 0, zone_class::OUTSIDE));
    let mut buckets = HierarchyEdgeBuckets::new();
    let size = 1i32 << (level + 1);
    for y in 0..i32::from(cells.height) {
        let mut x = 0;
        while x < i32::from(cells.width) {
            // Full581F90 seeds on cached class only. Live queries occur in
            // flood edge admission; adding a seed query changes native order.
            if cells.cell(x, y).is_none_or(|cell| {
                cell.class == zone_class::OUTSIDE || cell.index.zone(&graph) != 0
            }) {
                x += 1;
                continue;
            }
            let next_zone = graph.record_slot_count();
            let zone = ZoneId::try_from(next_zone).unwrap_or_else(|_| {
                panic!(
                    "zone hierarchy level {level} exceeds ZoneId capacity at real zone {next_zone}"
                )
            });
            let parent_id = parent.map_or(0, |parent| cells.zone(parent, x, y));
            assert!(graph.append_record(ZoneRecord::from_seed(
                zone,
                parent_id,
                cells.class(x, y),
                (x as i16, y as i16)
            )));
            let block = HierarchyBlock {
                x_min: x & !(size - 1),
                x_max: (x & !(size - 1)) + size - 1,
                y_min: y & !(size - 1),
                y_max: (y & !(size - 1)) + size - 1,
            };
            x += flood_fill_hierarchy_scanline(
                (x, y),
                zone,
                cells.base_id(x, y),
                cells,
                &mut graph,
                block,
                &mut buckets,
                query,
            );
        }
    }
    if let Some(terrain) = terrain {
        for record in records.iter().filter(|record| record.active) {
            register_bridge_hierarchy_edges_with_lookup(
                &mut buckets,
                terrain,
                record,
                &mut |(x, y)| cells.zone(&graph, i32::from(x as i16), i32::from(y as i16)),
            );
        }
    }
    buckets.drain_into(&mut graph);
    graph
}

pub(crate) const HIGH_BRIDGE_HIERARCHY_DIRECTIONS: [i8; 16] =
    [0, 0, -1, 2, 2, -1, 0, 0, 0, 0, 0, 2, 2, 2, 2, 2];
// Original gamemd582D70, full581F90 / local584550 callers, path walk429780.
// Evidence: PHASE3_TUBE_HIERARCHY_20260910.md and spatial_oracle/tube_hierarchy.

#[cfg(any(test, feature = "test-support"))]
pub(super) fn register_high_bridge_hierarchy_edges(
    edge_buckets: &mut HierarchyEdgeBuckets,
    zone_ids: &[ZoneId],
    terrain: &ResolvedTerrainGrid,
    bridge_records: &[BridgeEndpointRecord],
    width: u16,
    height: u16,
    source_size: Option<(i32, i32)>,
) {
    // Full581F90 uses forward order; local584550's reverse gated loop lives
    // at its caller and reaches the same per-record helper below.
    for record in bridge_records.iter().filter(|record| record.active) {
        register_bridge_hierarchy_edges_for_record(
            edge_buckets,
            zone_ids,
            terrain,
            record,
            width,
            height,
            source_size,
        );
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn register_bridge_hierarchy_edges_for_record(
    edge_buckets: &mut HierarchyEdgeBuckets,
    zone_ids: &[ZoneId],
    terrain: &ResolvedTerrainGrid,
    record: &BridgeEndpointRecord,
    width: u16,
    height: u16,
    source_size: Option<(i32, i32)>,
) {
    debug_assert_eq!(zone_ids.len(), usize::from(width) * usize::from(height));
    register_bridge_hierarchy_edges_with_lookup(edge_buckets, terrain, record, &mut |coord| {
        super::bridge_endpoint_base_zone(zone_ids, width, source_size, coord).unwrap_or(0)
    });
}

pub(super) fn register_bridge_hierarchy_edges_with_lookup(
    edge_buckets: &mut HierarchyEdgeBuckets,
    terrain: &ResolvedTerrainGrid,
    record: &BridgeEndpointRecord,
    zone_at: &mut impl FnMut((u16, u16)) -> ZoneId,
) {
    use crate::sim::cell_rect::{CellRef, get_cellclass_fallback};
    let a = record.endpoint_a;
    let b = record.endpoint_b;
    let cell = get_cellclass_fallback(Some(terrain), i32::from(a.0 as i16), i32::from(a.1 as i16));
    let high_offset = match cell {
        CellRef::Real(cell) => terrain.high_bridge_tile_offset(cell),
        // The hash-bound retail theater ranges exclude constructor tile65535.
        CellRef::Dummy { .. } => None,
    };
    let (side_a, opposite_a, side_b, opposite_b) = if let Some(offset) = high_offset {
        let direction = i32::from(HIGH_BRIDGE_HIERARCHY_DIRECTIONS[offset]);
        (
            hierarchy_side_coord(a, direction),
            hierarchy_side_coord(a, direction.wrapping_sub(4)),
            hierarchy_side_coord(b, direction),
            hierarchy_side_coord(b, direction.wrapping_sub(4)),
        )
    } else {
        // Active producer records supply a center Tube. Invalid externally
        // supplied records return safely instead of native's null dereference.
        let Some(tube) = terrain.tube_at_native_coord(a) else {
            return;
        };
        let side = hierarchy_side_coord(a, tube.direction.wrapping_add(2));
        let opposite = hierarchy_side_coord(a, tube.direction.wrapping_sub(2));
        // Both lookups happen before either null test, including dummy writes.
        let side_tube = terrain.tube_at_native_coord(side);
        let opposite_tube = terrain.tube_at_native_coord(opposite);
        let (Some(side_tube), Some(opposite_tube)) = (side_tube, opposite_tube) else {
            return;
        };
        let far =
            hierarchy_walk_tube_path(terrain, side, &side_tube.path_steps).and_then(|far_side| {
                hierarchy_walk_tube_path(terrain, opposite, &opposite_tube.path_steps)
                    .map(|far_opposite| (far_side, far_opposite))
            });
        let (far_side, far_opposite) = match far {
            Ok(far) => far,
            Err(error) => {
                // Explicit parity residual: retain the prior absent-pair result
                // for unmodeled process-memory reads; never mask or panic.
                log::warn!("unresolved native Tube hierarchy path read: {error:?}");
                return;
            }
        };
        (side, opposite, far_side, far_opposite)
    };
    for (from, to) in [(a, b), (side_a, side_b), (opposite_a, opposite_b)] {
        let from = zone_at(from);
        let to = zone_at(to);
        register_native_hierarchy_pair(edge_buckets, from, to, 0);
    }
}

fn hierarchy_side_coord(coord: (u16, u16), direction: i32) -> (u16, u16) {
    let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[(direction as u32 & 7) as usize];
    (
        coord.0.wrapping_add(dx as u16),
        coord.1.wrapping_add(dy as u16),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HierarchyPathReadError {
    DirectionData { raw_token: i32, native_address: u32 },
    TubeIndex(i16),
}

pub(super) fn hierarchy_walk_tube_path(
    terrain: &ResolvedTerrainGrid,
    mut coord: (u16, u16),
    steps: &[i32],
) -> Result<(u16, u16), HierarchyPathReadError> {
    for &step in steps {
        if step == 8 {
            let index = terrain.raw_tube_index_at_native_coord(coord);
            coord = if index == -1 {
                (0, 0)
            } else {
                terrain
                    .tube(crate::map::tube_facts::TubeId(index as u16))
                    .filter(|_| index >= 0)
                    .ok_or(HierarchyPathReadError::TubeIndex(index))?
                    .exit
            };
        } else {
            // x86 effective address89F688 + step*4 wraps in32 bits. Exact8
            // above is special; address aliases of slot8 are ordinary reads.
            let slot = step as u32 & 0x3fff_ffff;
            let (dx, dy) = match slot {
                0..=7 => crate::util::direction::DIRECTION_DELTAS[slot as usize],
                // Original49F2D0/49F280 initialize slots8..12 to0; slot13
                // is zero BSS with no direct xrefs in the examined image.
                8..=13 => (0, 0),
                // Original startup49F0E0/49F190 at captured FPCW0xE7F.
                // Their initialized bytes match the unpatched process capture
                // (tube_startup_capture.json); arbitrary neighboring data stays open.
                14 => (0, 0x6000),
                15 => (-24418, 16502),
                0x3fff_fffe => (11544, 21572),
                0x3fff_ffff => (8699, 16377),
                _ => {
                    return Err(HierarchyPathReadError::DirectionData {
                        raw_token: step,
                        native_address: 0x0089_f688u32.wrapping_add((step as u32).wrapping_mul(4)),
                    });
                }
            };
            coord = (
                coord.0.wrapping_add(dx as u16),
                coord.1.wrapping_add(dy as u16),
            );
        }
    }
    Ok(coord)
}

pub(super) fn register_native_hierarchy_pair(
    buckets: &mut HierarchyEdgeBuckets,
    a: ZoneId,
    b: ZoneId,
    flag: u8,
) {
    //582FFD..583012 sign-extends both words before OR. Preserve exact packed
    //transcript, including the native high-bit zone-ID limitation.
    let bucket = (((a & 15) << 4) | (b & 15)) as usize;
    let packed = (u32::from(a) << 16) | (i32::from(b as i16) as u32);
    let edge = HierarchyTempEdge {
        existing: (packed >> 16) as u16,
        current: packed as u16,
        flag,
    };
    // No zero/equality filter. First exact key retains its flag and place.
    buckets.push_new(bucket, edge);
}

pub(super) fn flood_fill_hierarchy_scanline(
    seed: (i32, i32),
    zone: ZoneId,
    base_id: ZoneId,
    cells: HierarchyCells<'_>,
    graph: &mut ZoneLevelGraph,
    block: HierarchyBlock,
    buckets: &mut HierarchyEdgeBuckets,
    query: &mut impl FnMut(i32, i32) -> bool,
) -> i32 {
    // Each position's index is computed once per visit; the order of zone
    // reads, recursion and queries is the native one.
    let cells = cells.scan_from(seed);
    let (sx, sy) = seed;
    let mut last_neighbor = None;
    let seed_level = cells.level(sx, sy);
    let mut left = sx;
    let mut previous = seed_level;
    while block.contains(left, sy)
        && let Some(cell) = cells.cell(left, sy)
        && cell.base_id == base_id
        && cell.level.abs_diff(previous) < 2
    {
        cell.index.set_zone(graph, zone);
        previous = cell.level;
        left -= 1;
    }
    register_hierarchy_horizontal_edge(
        (left, sy),
        (left + 1, sy),
        zone,
        cells,
        graph,
        buckets,
        query,
        &mut last_neighbor,
    );
    let mut right = sx;
    previous = seed_level;
    while block.contains(right, sy)
        && let Some(cell) = cells.cell(right, sy)
        && cell.base_id == base_id
        && cell.level.abs_diff(previous) < 2
    {
        cell.index.set_zone(graph, zone);
        previous = cell.level;
        right += 1;
    }
    register_hierarchy_horizontal_edge(
        (right, sy),
        (right - 1, sy),
        zone,
        cells,
        graph,
        buckets,
        query,
        &mut last_neighbor,
    );
    for cy in [sy - 1, sy + 1] {
        for cx in left..=right {
            let rx = cx.clamp(left + 1, right - 1);
            let cell = cells.cell(cx, cy);
            let level = cell.map_or(0, |cell| cell.level);
            let height_allowed = level.abs_diff(cells.level(rx, sy)) < 2;
            if let Some(cell) = cell
                && cell.index.zone(graph) == 0
                && block.contains(cx, cy)
                && cell.base_id == base_id
                && height_allowed
            {
                // Recursion owns a fresh last-neighbor; it must not replace
                // this caller's state. Native then reloads the same candidate.
                flood_fill_hierarchy_scanline(
                    (cx, cy),
                    zone,
                    base_id,
                    cells,
                    graph,
                    block,
                    buckets,
                    query,
                );
            }
            let existing = cell.map_or(0, |cell| cell.index.zone(graph));
            if existing != 0
                && existing != zone
                && last_neighbor != Some(existing)
                && height_allowed
                && query(rx, sy)
                && query(cx, cy)
            {
                let flag = u8::from(cx < block.x_min || cx > block.x_max);
                buckets.register(existing, zone, flag);
                last_neighbor = Some(existing);
            }
        }
    }
    right - 1 - sx
}

fn register_hierarchy_horizontal_edge(
    candidate: (i32, i32),
    reference: (i32, i32),
    zone: ZoneId,
    cells: HierarchyCells<'_>,
    graph: &ZoneLevelGraph,
    buckets: &mut HierarchyEdgeBuckets,
    query: &mut impl FnMut(i32, i32) -> bool,
    last_neighbor: &mut Option<ZoneId>,
) {
    let existing = cells.zone(graph, candidate.0, candidate.1);
    if existing as i16 > 0
        && *last_neighbor != Some(existing)
        && cells
            .level(candidate.0, candidate.1)
            .abs_diff(cells.level(reference.0, reference.1))
            < 2
        && query(candidate.0, candidate.1)
        && query(reference.0, reference.1)
    {
        buckets.register(existing, zone, 0);
        // A duplicate bucket pair still updates the native local variable.
        *last_neighbor = Some(existing);
    }
}
