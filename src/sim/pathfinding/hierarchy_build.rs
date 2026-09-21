// Original581F90/584550/5824A0 hierarchy production. Native comparisons:
// tools/spatial_oracle/bridge_hierarchy.{py,json,meta.json}.
// Retained classes/heights/base IDs and live playfield queries are separate
// inputs. A query may stamp the shared dummy, so preserve its call order.

#[derive(Clone, Copy)]
enum HierarchyCellIndex {
    Represented(usize),
    Padding(usize),
}

#[derive(Clone, Copy)]
struct HierarchyCells<'a> {
    base: &'a BaseZoneTopology,
    width: u16,
    height: u16,
    native_scan_offset: Option<i32>,
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

    fn base_index(self, x: i32, y: i32) -> Option<usize> {
        match self.index(x, y)? {
            HierarchyCellIndex::Represented(index) => Some(index),
            HierarchyCellIndex::Padding(_) => None,
        }
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
        self.base_index(x, y).map_or(0, |i| self.base.levels[i])
    }

    fn base_id(self, x: i32, y: i32) -> ZoneId {
        self.base_index(x, y).map_or(0, |i| self.base.zone_ids[i])
    }

    fn class(self, x: i32, y: i32) -> u8 {
        self.base_index(x, y)
            .map_or(zone_class::OUTSIDE, |i| self.base.movement_classes[i])
    }

    fn zone(self, graph: &ZoneLevelGraph, x: i32, y: i32) -> ZoneId {
        match self.index(x, y) {
            Some(HierarchyCellIndex::Represented(i)) => graph.cell_zone_ids()[i],
            Some(HierarchyCellIndex::Padding(i)) => graph.native_padding_zone(i),
            None => 0,
        }
    }

    fn set_zone(self, graph: &mut ZoneLevelGraph, x: i32, y: i32, zone: ZoneId) {
        match self.index(x, y) {
            Some(HierarchyCellIndex::Represented(i)) => graph.cell_zone_ids_mut()[i] = zone,
            Some(HierarchyCellIndex::Padding(i)) => graph.set_native_padding_zone(i, zone),
            None => {}
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
#[cfg(test)]
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
            if !graph.append_record(ZoneRecord::new(zone, parent_id, cells.class(x, y))) {
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
            if cells.class(x, y) == zone_class::OUTSIDE || cells.zone(&graph, x, y) != 0 {
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
            assert!(graph.append_record(ZoneRecord::new(zone, parent_id, cells.class(x, y))));
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
include!("hierarchy_bridge.rs");

fn flood_fill_hierarchy_scanline(
    seed: (i32, i32),
    zone: ZoneId,
    base_id: ZoneId,
    cells: HierarchyCells<'_>,
    graph: &mut ZoneLevelGraph,
    block: HierarchyBlock,
    buckets: &mut HierarchyEdgeBuckets,
    query: &mut impl FnMut(i32, i32) -> bool,
) -> i32 {
    let cells = cells.scan_from(seed);
    let (sx, sy) = seed;
    let mut last_neighbor = None;
    let mut left = sx;
    let mut previous = cells.level(sx, sy);
    while block.contains(left, sy)
        && cells.index(left, sy).is_some()
        && cells.base_id(left, sy) == base_id
        && cells.level(left, sy).abs_diff(previous) < 2
    {
        cells.set_zone(graph, left, sy, zone);
        previous = cells.level(left, sy);
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
    previous = cells.level(sx, sy);
    while block.contains(right, sy)
        && cells.index(right, sy).is_some()
        && cells.base_id(right, sy) == base_id
        && cells.level(right, sy).abs_diff(previous) < 2
    {
        cells.set_zone(graph, right, sy, zone);
        previous = cells.level(right, sy);
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
            let height_allowed = cells.level(cx, cy).abs_diff(cells.level(rx, sy)) < 2;
            if cells.index(cx, cy).is_some()
                && cells.zone(graph, cx, cy) == 0
                && block.contains(cx, cy)
                && cells.base_id(cx, cy) == base_id
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
            let existing = cells.zone(graph, cx, cy);
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
