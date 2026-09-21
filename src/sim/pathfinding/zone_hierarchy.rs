//! Super-zone reachability and binary-style zone hierarchy scaffolding.
//!
//! After flood-fill partitions the map into zones and adjacency is extracted,
//! this module computes connected components: groups of zones that are
//! transitively reachable through adjacency edges. This turns the O(V+E) BFS
//! in `zone_graph_connected()` into an O(1) lookup.
//!
//! The `ZoneHierarchy` types below model the separate gamemd-style hierarchy
//! records used by `Zone_precheck`. They intentionally coexist with
//! `SuperZoneMap`: the former is route-selection data, the latter is still the
//! cheap reachability cache used by existing compatibility paths.
//! Synthetic tests in this module do not assert stock Carville route cells,
//! route direction, or full high-bridge player-visible path parity.
//!
//! ## Dependency rules
//! - Part of sim/ - depends on sim/zone_map, pathfinding passability, and rules movement zones.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

use super::passability;
use super::zone_map::{ZONE_INVALID, ZoneAdjacency, ZoneId};
use crate::rules::locomotor_type::MovementZone;

pub(crate) const ZONE_PRECHECK_LEVELS: usize = 3;
const TOP_LEVEL: usize = 2;
/// Per-terrain-type base cost `Zone_precheck` @ `0x0042C290` adds per hop, ×1000.
///
/// The native table is the eight f32s at `0x007E3794`
/// (`[1.0, 0, 0, 1.0, 1.0, 0, 1.0, 1.0]`); this is that table scaled to
/// integers, as is the `+1` edge-flag term against the native `0.001`.
const ZONE_BASE_COSTS: [i32; passability::TERRAIN_TYPE_COUNT] =
    [1000, 0, 0, 1000, 1000, 0, 1000, 1000];

/// Connected-component labels for zones, computed via union-find.
/// Two zones with the same label are transitively reachable.
#[derive(Debug, Clone)]
pub(crate) struct SuperZoneMap {
    /// For each zone ID (1-indexed), the component label.
    /// Index 0 is unused (ZONE_INVALID). Labels are canonical root zone IDs.
    labels: Vec<ZoneId>,
}

impl SuperZoneMap {
    /// Build super-zone labels from zone adjacency using union-find.
    pub fn from_adjacency(adj: &ZoneAdjacency, zone_count: u16) -> Self {
        let n = zone_count as usize + 1; // 1-indexed zones
        let mut parent: Vec<usize> = (0..n).collect();
        let mut rank: Vec<u8> = vec![0; n];

        // Union all adjacent zone pairs.
        for z in 1..=zone_count as usize {
            for &neighbor in adj.neighbors_of(z as ZoneId) {
                union(&mut parent, &mut rank, z, neighbor as usize);
            }
        }

        // Path-compress to get final labels.
        let labels: Vec<ZoneId> = (0..n).map(|i| find(&mut parent, i) as ZoneId).collect();

        SuperZoneMap { labels }
    }

    /// O(1) reachability: are two zones in the same connected component?
    pub fn are_connected(&self, a: ZoneId, b: ZoneId) -> bool {
        if a == ZONE_INVALID || b == ZONE_INVALID {
            return false;
        }
        let ai = a as usize;
        let bi = b as usize;
        if ai >= self.labels.len() || bi >= self.labels.len() {
            return false;
        }
        self.labels[ai] == self.labels[bi]
    }
}

/// Per-zone record in the binary-style hierarchy graph.
///
/// `parent` links to the next coarser level (`level + 1`) and is zero at the
/// top level. `zone_type` is the reduced 0..7 type consumed by the movement-zone
/// passability matrix and base-cost table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ZoneRecord {
    pub zone_id: ZoneId,
    pub parent: ZoneId,
    pub zone_type: u8,
}

impl ZoneRecord {
    pub(crate) fn new(zone_id: ZoneId, parent: ZoneId, zone_type: u8) -> Self {
        Self {
            zone_id,
            parent,
            zone_type,
        }
    }
}

/// Ordered edge record. The order inside a zone's edge list is load-bearing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ZoneEdgeRecord {
    pub neighbor: ZoneId,
    pub flag: u8,
}

impl ZoneEdgeRecord {
    pub(crate) fn new(neighbor: ZoneId, flag: u8) -> Self {
        Self { neighbor, flag }
    }
}

/// One hierarchy level. Index 0 is the invalid sentinel zone.
#[derive(Debug, Clone)]
pub(crate) struct ZoneLevelGraph {
    records: Vec<Option<ZoneRecord>>,
    edges: Vec<Vec<ZoneEdgeRecord>>,
    cell_zone_ids: Vec<ZoneId>,
    /// Native Map+70 also has addressable slots beyond our terrain rectangle.
    /// Original5824A0 can fill these and walk through them into represented
    /// cells. Store only their nonzero IDs; represented cells have one owner.
    /// Evidence: spatial_oracle/bridge_hierarchy padding/full/local witnesses.
    native_padding_zone_ids: BTreeMap<usize, ZoneId>,
    width: u16,
    height: u16,
}

impl ZoneLevelGraph {
    pub(crate) fn new(zone_count: ZoneId) -> Self {
        Self {
            records: vec![None; zone_count as usize + 1],
            edges: vec![Vec::new(); zone_count as usize + 1],
            cell_zone_ids: Vec::new(),
            native_padding_zone_ids: BTreeMap::new(),
            width: 0,
            height: 0,
        }
    }

    pub(crate) fn with_cell_zone_ids(
        mut self,
        cell_zone_ids: Vec<ZoneId>,
        width: u16,
        height: u16,
    ) -> Self {
        debug_assert_eq!(cell_zone_ids.len(), width as usize * height as usize);
        self.cell_zone_ids = cell_zone_ids;
        self.width = width;
        self.height = height;
        self
    }

    pub(crate) fn set_record(&mut self, record: ZoneRecord) {
        let idx = record.zone_id as usize;
        if idx < self.records.len() {
            self.records[idx] = Some(record);
        }
    }

    /// Append one replacement record without reusing stale slots. Native local
    /// hierarchy repair keeps cleared records as holes and advances the
    /// one-past-highest identifier.
    pub(crate) fn append_record(&mut self, record: ZoneRecord) -> bool {
        let Ok(expected) = ZoneId::try_from(self.records.len()) else {
            return false;
        };
        if record.zone_id != expected {
            return false;
        }
        self.records.push(Some(record));
        self.edges.push(Vec::new());
        true
    }

    pub(crate) fn push_edge(&mut self, zone: ZoneId, edge: ZoneEdgeRecord) {
        let idx = zone as usize;
        if idx < self.edges.len() {
            self.edges[idx].push(edge);
        }
    }

    pub(crate) fn record_slot_count(&self) -> usize {
        self.records.len()
    }

    pub(crate) fn clear_edges(&mut self, zone: ZoneId) {
        if let Some(edges) = self.edges.get_mut(zone as usize) {
            edges.clear();
        }
    }

    /// Remove the last reciprocal occurrence while preserving every surviving
    /// edge's relative order.
    pub(crate) fn remove_last_edge_to(&mut self, zone: ZoneId, neighbor: ZoneId) {
        let Some(edges) = self.edges.get_mut(zone as usize) else {
            return;
        };
        if let Some(index) = edges.iter().rposition(|edge| edge.neighbor == neighbor) {
            edges.remove(index);
        }
    }

    pub(crate) fn set_parent(&mut self, zone: ZoneId, parent: ZoneId) {
        if let Some(Some(record)) = self.records.get_mut(zone as usize) {
            record.parent = parent;
        }
    }

    pub(crate) fn set_zone_at(&mut self, x: i32, y: i32, zone: ZoneId) {
        if x < 0 || y < 0 || x >= i32::from(self.width) || y >= i32::from(self.height) {
            return;
        }
        let index = y as usize * self.width as usize + x as usize;
        if let Some(slot) = self.cell_zone_ids.get_mut(index) {
            *slot = zone;
        }
    }

    pub(crate) fn record(&self, zone: ZoneId) -> Option<ZoneRecord> {
        self.records.get(zone as usize).copied().flatten()
    }

    pub(crate) fn edges(&self, zone: ZoneId) -> &[ZoneEdgeRecord] {
        self.edges
            .get(zone as usize)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub(crate) fn zone_count(&self) -> ZoneId {
        self.records.len().saturating_sub(1) as ZoneId
    }

    pub(crate) fn zone_at(&self, x: u16, y: u16) -> ZoneId {
        if x >= self.width || y >= self.height {
            return ZONE_INVALID;
        }
        let idx = y as usize * self.width as usize + x as usize;
        self.cell_zone_ids.get(idx).copied().unwrap_or(ZONE_INVALID)
    }

    pub(crate) fn cell_zone_ids(&self) -> &[ZoneId] {
        &self.cell_zone_ids
    }

    /// Original56D3F0 packed linear lookup, used by5851B0 and42C900's
    /// projected hierarchy endpoints. Padding can hold live IDs after5824A0;
    /// the base-cluster helper's constant padding0 does not apply here.
    pub(crate) fn native_zone_at(
        &self,
        coord: (u16, u16),
        source_size: Option<(i32, i32)>,
    ) -> Option<ZoneId> {
        let Some(size) = source_size else {
            return super::zone_build::bridge_endpoint_base_zone(
                &self.cell_zone_ids,
                self.width,
                None,
                coord,
            );
        };
        let (x, y) =
            super::zone_build::native_zone_grid_position(size, (coord.0 as i16, coord.1 as i16))?;
        if x < i32::from(self.width) && y < i32::from(self.height) {
            Some(self.zone_at(x as u16, y as u16))
        } else {
            let side = size.0.wrapping_add(size.1).wrapping_add(1);
            Some(self.native_padding_zone((y * side + x) as usize))
        }
    }

    /// The586990 fine-zone clear uses the same signed/clamped native record
    /// as56D3F0 reads, including mutable padding beyond represented cells.
    pub(crate) fn set_native_zone_at(
        &mut self,
        coord: (i16, i16),
        source_size: Option<(i32, i32)>,
        zone: ZoneId,
    ) -> Option<()> {
        let (x, y) = if let Some(size) = source_size {
            super::zone_build::native_zone_grid_position(size, coord)?
        } else {
            let (x, y) = (i32::from(coord.0), i32::from(coord.1));
            if x < 0 || y < 0 || x >= i32::from(self.width) || y >= i32::from(self.height) {
                return None;
            }
            (x, y)
        };
        if x < i32::from(self.width) && y < i32::from(self.height) {
            self.set_zone_at(x, y, zone);
        } else {
            let size = source_size?;
            let side = size.0.wrapping_add(size.1).wrapping_add(1);
            self.set_native_padding_zone((y * side + x) as usize, zone);
        }
        Some(())
    }

    pub(crate) fn cell_zone_ids_mut(&mut self) -> &mut [ZoneId] {
        &mut self.cell_zone_ids
    }

    pub(crate) fn native_padding_zone(&self, index: usize) -> ZoneId {
        self.native_padding_zone_ids
            .get(&index)
            .copied()
            .unwrap_or(0)
    }

    pub(crate) fn set_native_padding_zone(&mut self, index: usize, zone: ZoneId) {
        if zone == 0 {
            self.native_padding_zone_ids.remove(&index);
        } else {
            self.native_padding_zone_ids.insert(index, zone);
        }
    }
}

/// Three-level hierarchy searched by `Zone_precheck` @ `0x0042C290`.
#[derive(Debug, Clone)]
pub(crate) struct ZoneHierarchy {
    levels: [ZoneLevelGraph; ZONE_PRECHECK_LEVELS],
}

impl ZoneHierarchy {
    /// Levels are passed low-to-high: level 0, level 1, level 2.
    pub(crate) fn new(
        level0: ZoneLevelGraph,
        level1: ZoneLevelGraph,
        level2: ZoneLevelGraph,
    ) -> Self {
        Self {
            levels: [level0, level1, level2],
        }
    }

    pub(crate) fn level(&self, level: usize) -> Option<&ZoneLevelGraph> {
        self.levels.get(level)
    }

    pub(crate) fn levels_mut(&mut self) -> &mut [ZoneLevelGraph; ZONE_PRECHECK_LEVELS] {
        &mut self.levels
    }

    fn ancestors_from_level0(&self, zone: ZoneId) -> Option<[ZoneId; ZONE_PRECHECK_LEVELS]> {
        let l0 = self.level(0)?.record(zone)?;
        let l1_zone = l0.parent;
        let l1 = self.level(1)?.record(l1_zone)?;
        let l2_zone = l1.parent;
        self.level(2)?.record(l2_zone)?;
        Some([zone, l1_zone, l2_zone])
    }
}

/// Canonical undirected zone-edge key used by `Zone_precheck` exclusions.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct ZoneEdgeKey {
    a: ZoneId,
    b: ZoneId,
}

impl ZoneEdgeKey {
    pub(crate) fn new(a: ZoneId, b: ZoneId) -> Option<Self> {
        if a == ZONE_INVALID || b == ZONE_INVALID || a == b {
            return None;
        }
        Some(if a < b {
            Self { a, b }
        } else {
            Self { a: b, b: a }
        })
    }
}

/// Search-local/preseeded exclusions consumed by `zone_precheck_flat`.
///
/// This is the consumer side only. The exact failed-A* producer that chooses
/// which edge to invalidate remains deferred.
#[derive(Debug, Clone, Default)]
pub(crate) struct ZonePrecheckExclusions {
    lookup: [BTreeSet<ZoneEdgeKey>; ZONE_PRECHECK_LEVELS],
    // Preserves the verified producer's append order once failed-A* exclusion
    // production is activated.
    ordered: [Vec<ZoneEdgeKey>; ZONE_PRECHECK_LEVELS],
}

impl ZonePrecheckExclusions {
    #[cfg(test)]
    pub(crate) fn insert(&mut self, level: usize, a: ZoneId, b: ZoneId) -> bool {
        let Some(key) = ZoneEdgeKey::new(a, b) else {
            return false;
        };
        self.lookup
            .get_mut(level)
            .is_some_and(|set| set.insert(key))
    }

    // Exact failed-A* producer wiring is intentionally deferred.
    #[cfg(test)]
    pub(crate) fn append_producer_edge(&mut self, level: usize, a: ZoneId, b: ZoneId) -> bool {
        let Some(key) = ZoneEdgeKey::new(a, b) else {
            return false;
        };
        let Some((lookup, ordered)) = self.lookup.get_mut(level).zip(self.ordered.get_mut(level))
        else {
            return false;
        };
        lookup.insert(key);
        ordered.push(key);
        true
    }

    // Diagnostic view of the deferred producer's native append order.
    #[cfg(test)]
    pub(crate) fn ordered_edges(&self, level: usize) -> &[ZoneEdgeKey] {
        self.ordered.get(level).map(Vec::as_slice).unwrap_or(&[])
    }

    fn contains(&self, level: usize, a: ZoneId, b: ZoneId) -> bool {
        ZoneEdgeKey::new(a, b)
            .and_then(|key| self.lookup.get(level).map(|set| set.contains(&key)))
            .unwrap_or(false)
    }
}

/// Successful precheck output: selected paths and marker sets per hierarchy level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ZonePrecheckResult {
    pub paths: [Vec<ZoneId>; ZONE_PRECHECK_LEVELS],
    pub marked: [BTreeSet<ZoneId>; ZONE_PRECHECK_LEVELS],
}

impl ZonePrecheckResult {
    fn new() -> Self {
        Self {
            paths: std::array::from_fn(|_| Vec::new()),
            marked: std::array::from_fn(|_| BTreeSet::new()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ZonePrecheckOutcome {
    Passed(ZonePrecheckResult),
    Failed,
}

/// **VERA-internal tie-break, gamemd equivalent UNCHECKED.** `Zone_precheck`
/// @ `0x0042C290` uses a raw binary min-heap keyed on the float cost — sift-up
/// stops on `parent <= new`, sift-down uses strict `<` — so its pop order among
/// equal costs is heap-array order, not insertion order. This entry breaks ties
/// FIFO on `sequence` instead, because heap-array order is not reproducible
/// from a `BinaryHeap`.
///
/// Trigger: any two frontier zones with the same accumulated cost — pervasive,
/// since [`ZONE_BASE_COSTS`] only ever contributes 0 or 1000 plus the `+1` edge
/// flag. Player effect: a different coarse route is chosen, which stamps a
/// different corridor and can hand the cell A* a different (still valid) path,
/// so units take a different but legal route. Frequency: continuous on any real
/// map. Downstream risk: the corridor is deterministic within VERA, so this
/// costs replay nothing; it costs frame-for-frame comparison against gamemd.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct PrecheckQueueEntry {
    cost: i32,
    sequence: u32,
    zone: ZoneId,
}

impl Ord for PrecheckQueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for PrecheckQueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Flat/no-slope `Zone_precheck` @ `0x0042C290` foundation.
///
/// This intentionally models the precheck consumer and output shape only. Slope
/// contribution is zero in this slice, and automatic failed-A* exclusion
/// producers remain a separate follow-up.
pub(crate) fn zone_precheck_flat(
    hierarchy: &ZoneHierarchy,
    start_level0: ZoneId,
    goal_level0: ZoneId,
    movement_zone: MovementZone,
    exclusions: &ZonePrecheckExclusions,
) -> ZonePrecheckOutcome {
    let Some(start_zones) = hierarchy.ancestors_from_level0(start_level0) else {
        return ZonePrecheckOutcome::Failed;
    };
    let Some(goal_zones) = hierarchy.ancestors_from_level0(goal_level0) else {
        return ZonePrecheckOutcome::Failed;
    };

    let mut result = ZonePrecheckResult::new();
    for level in (0..ZONE_PRECHECK_LEVELS).rev() {
        let parent_marked = if level < TOP_LEVEL {
            Some(&result.marked[level + 1])
        } else {
            None
        };
        let Some(path) = search_precheck_level(
            hierarchy.level(level).expect("fixed hierarchy level"),
            level,
            start_zones[level],
            goal_zones[level],
            movement_zone,
            parent_marked,
            exclusions,
        ) else {
            return ZonePrecheckOutcome::Failed;
        };
        result.marked[level] = path.iter().copied().collect();
        result.paths[level] = path;
    }

    ZonePrecheckOutcome::Passed(result)
}

fn search_precheck_level(
    graph: &ZoneLevelGraph,
    level: usize,
    start: ZoneId,
    goal: ZoneId,
    movement_zone: MovementZone,
    parent_marked: Option<&BTreeSet<ZoneId>>,
    exclusions: &ZonePrecheckExclusions,
) -> Option<Vec<ZoneId>> {
    if start == ZONE_INVALID || goal == ZONE_INVALID {
        return None;
    }
    let start_record = graph.record(start)?;
    let goal_record = graph.record(goal)?;
    // VERA-internal, gamemd has no equivalent: the two `start_record` terms.
    // `Zone_precheck` @ 0x0042C290 never type-checks the start zone — it stamps
    // it and searches, and short-circuits `start == goal` to success without
    // any check at all. The goal is effectively gated on both sides, because
    // native can only reach it through the `matrix[row*8+type] == 1` neighbour
    // gate. Trigger: a start zone whose record type is impassable for the
    // mover's row. Player effect: VERA refuses the whole precheck where gamemd
    // would search. Frequency: near zero — a level-0 zone's record type is the
    // class of its whole base node, so a unit standing on a cell legal for it
    // has a passable record; it takes a bridge or wall boundary oddity to fire.
    // Downstream risk: none, it is a head predicate.
    if !is_valid_zone_type(start_record.zone_type)
        || !is_valid_zone_type(goal_record.zone_type)
        || !passability::is_passable_for_zone(start_record.zone_type, movement_zone)
        || !passability::is_passable_for_zone(goal_record.zone_type, movement_zone)
    {
        return None;
    }
    if start == goal {
        return Some(vec![start]);
    }

    let zone_count = graph.zone_count() as usize;
    let mut dist = vec![i32::MAX; zone_count + 1];
    let mut prev = vec![ZONE_INVALID; zone_count + 1];
    let mut heap = BinaryHeap::new();
    let mut next_sequence = 1u32;

    dist[start as usize] = 0;
    heap.push(PrecheckQueueEntry {
        cost: 0,
        sequence: 0,
        zone: start,
    });

    while let Some(PrecheckQueueEntry { cost, zone, .. }) = heap.pop() {
        if zone == goal {
            return reconstruct_zone_path(&prev, goal);
        }
        if cost > dist[zone as usize] {
            continue;
        }

        for edge in graph.edges(zone) {
            let neighbor = edge.neighbor;
            if neighbor as usize > zone_count || exclusions.contains(level, zone, neighbor) {
                continue;
            }
            let Some(record) = graph.record(neighbor) else {
                continue;
            };
            if let Some(marked) = parent_marked {
                let parent_allowed = record.zone_type == 1 || marked.contains(&record.parent);
                if !parent_allowed {
                    continue;
                }
            }
            if !passability::is_passable_for_zone(record.zone_type, movement_zone) {
                continue;
            }
            let Some(base_cost) = ZONE_BASE_COSTS.get(record.zone_type as usize).copied() else {
                continue;
            };
            let edge_flag_cost = i32::from(edge.flag != 0);
            let new_cost = cost
                .saturating_add(base_cost)
                .saturating_add(edge_flag_cost);
            if new_cost < dist[neighbor as usize] {
                dist[neighbor as usize] = new_cost;
                prev[neighbor as usize] = zone;
                heap.push(PrecheckQueueEntry {
                    cost: new_cost,
                    sequence: next_sequence,
                    zone: neighbor,
                });
                next_sequence = next_sequence.wrapping_add(1);
            }
        }
    }

    None
}

fn is_valid_zone_type(zone_type: u8) -> bool {
    (zone_type as usize) < passability::TERRAIN_TYPE_COUNT
}

fn reconstruct_zone_path(prev: &[ZoneId], goal: ZoneId) -> Option<Vec<ZoneId>> {
    let mut path = Vec::new();
    let mut current = goal;
    while current != ZONE_INVALID {
        let idx = current as usize;
        if idx >= prev.len() {
            return None;
        }
        path.push(current);
        current = prev[idx];
    }
    path.reverse();
    Some(path)
}

// ---------------------------------------------------------------------------
// Union-find with path compression and union by rank
// ---------------------------------------------------------------------------

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]]; // path halving
        x = parent[x];
    }
    x
}

fn union(parent: &mut [usize], rank: &mut [u8], a: usize, b: usize) {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra == rb {
        return;
    }
    // Union by rank: attach smaller tree under larger.
    if rank[ra] < rank[rb] {
        parent[ra] = rb;
    } else if rank[ra] > rank[rb] {
        parent[rb] = ra;
    } else {
        parent[rb] = ra;
        rank[ra] += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adj_from_edges(zone_count: u16, edges: &[(ZoneId, ZoneId)]) -> ZoneAdjacency {
        let mut neighbors: Vec<Vec<ZoneId>> = vec![Vec::new(); zone_count as usize + 1];
        for &(a, b) in edges {
            if !neighbors[a as usize].contains(&b) {
                neighbors[a as usize].push(b);
            }
            if !neighbors[b as usize].contains(&a) {
                neighbors[b as usize].push(a);
            }
        }
        ZoneAdjacency::new(neighbors)
    }

    fn fixture_hierarchy() -> ZoneHierarchy {
        let mut level2 = ZoneLevelGraph::new(2);
        level2.set_record(ZoneRecord::new(1, 0, 0));
        level2.set_record(ZoneRecord::new(2, 0, 0));
        level2.push_edge(1, ZoneEdgeRecord::new(2, 0));
        level2.push_edge(2, ZoneEdgeRecord::new(1, 0));

        let mut level1 = ZoneLevelGraph::new(4);
        for (zone, parent) in [(1, 1), (2, 1), (3, 2), (4, 2)] {
            level1.set_record(ZoneRecord::new(zone, parent, 0));
        }
        level1.push_edge(1, ZoneEdgeRecord::new(2, 0));
        level1.push_edge(2, ZoneEdgeRecord::new(1, 0));
        level1.push_edge(2, ZoneEdgeRecord::new(3, 0));
        level1.push_edge(3, ZoneEdgeRecord::new(2, 0));
        level1.push_edge(3, ZoneEdgeRecord::new(4, 0));
        level1.push_edge(4, ZoneEdgeRecord::new(3, 0));

        let mut level0 = ZoneLevelGraph::new(7);
        for (zone, parent) in [(1, 1), (2, 1), (3, 2), (4, 3), (5, 4), (6, 4)] {
            level0.set_record(ZoneRecord::new(zone, parent, 0));
        }
        for (a, b, flag) in [(1, 2, 0), (2, 3, 0), (3, 4, 1), (4, 5, 0), (5, 6, 0)] {
            level0.push_edge(a, ZoneEdgeRecord::new(b, flag));
            level0.push_edge(b, ZoneEdgeRecord::new(a, flag));
        }

        ZoneHierarchy::new(level0, level1, level2)
    }

    #[test]
    fn single_zone_connected_to_self() {
        let adj = adj_from_edges(1, &[]);
        let sz = SuperZoneMap::from_adjacency(&adj, 1);
        assert!(sz.are_connected(1, 1));
    }

    #[test]
    fn two_adjacent_zones_connected() {
        let adj = adj_from_edges(2, &[(1, 2)]);
        let sz = SuperZoneMap::from_adjacency(&adj, 2);
        assert!(sz.are_connected(1, 2));
    }

    #[test]
    fn two_isolated_zones_not_connected() {
        let adj = adj_from_edges(2, &[]);
        let sz = SuperZoneMap::from_adjacency(&adj, 2);
        assert!(!sz.are_connected(1, 2));
    }

    #[test]
    fn transitive_connectivity() {
        let adj = adj_from_edges(3, &[(1, 2), (2, 3)]);
        let sz = SuperZoneMap::from_adjacency(&adj, 3);
        assert!(sz.are_connected(1, 3));
        assert!(sz.are_connected(1, 2));
        assert!(sz.are_connected(2, 3));
    }

    #[test]
    fn two_components() {
        let adj = adj_from_edges(3, &[(1, 2)]);
        let sz = SuperZoneMap::from_adjacency(&adj, 3);
        assert!(sz.are_connected(1, 2));
        assert!(!sz.are_connected(1, 3));
        assert!(!sz.are_connected(2, 3));
    }

    #[test]
    fn invalid_zone_never_connected() {
        let adj = adj_from_edges(2, &[(1, 2)]);
        let sz = SuperZoneMap::from_adjacency(&adj, 2);
        assert!(!sz.are_connected(ZONE_INVALID, 1));
        assert!(!sz.are_connected(1, ZONE_INVALID));
    }

    #[test]
    fn zone_precheck_searches_levels_2_1_0_and_retains_paths() {
        let outcome = zone_precheck_flat(
            &fixture_hierarchy(),
            1,
            6,
            MovementZone::Crusher,
            &ZonePrecheckExclusions::default(),
        );
        let ZonePrecheckOutcome::Passed(result) = outcome else {
            panic!("precheck should pass");
        };
        assert_eq!(result.paths[2], vec![1, 2]);
        assert_eq!(result.paths[1], vec![1, 2, 3, 4]);
        assert_eq!(result.paths[0], vec![1, 2, 3, 4, 5, 6]);
        assert!(result.marked[0].contains(&4));
    }

    #[test]
    fn zone_precheck_equal_cost_keeps_edge_insertion_order() {
        let mut level2 = ZoneLevelGraph::new(1);
        level2.set_record(ZoneRecord::new(1, 0, 0));

        let mut level1 = ZoneLevelGraph::new(1);
        level1.set_record(ZoneRecord::new(1, 1, 0));

        let mut level0 = ZoneLevelGraph::new(4);
        for zone in 1..=4 {
            level0.set_record(ZoneRecord::new(zone, 1, 0));
        }
        level0.push_edge(1, ZoneEdgeRecord::new(3, 0));
        level0.push_edge(1, ZoneEdgeRecord::new(2, 0));
        level0.push_edge(2, ZoneEdgeRecord::new(4, 0));
        level0.push_edge(3, ZoneEdgeRecord::new(4, 0));
        let hierarchy = ZoneHierarchy::new(level0, level1, level2);

        let ZonePrecheckOutcome::Passed(result) = zone_precheck_flat(
            &hierarchy,
            1,
            4,
            MovementZone::Crusher,
            &ZonePrecheckExclusions::default(),
        ) else {
            panic!("precheck should pass");
        };
        assert_eq!(result.paths[0], vec![1, 3, 4]);
    }

    #[test]
    fn zone_precheck_edge_flag_adds_tiny_tiebreak_cost() {
        let mut level2 = ZoneLevelGraph::new(1);
        level2.set_record(ZoneRecord::new(1, 0, 0));
        let mut level1 = ZoneLevelGraph::new(1);
        level1.set_record(ZoneRecord::new(1, 1, 0));
        let mut level0 = ZoneLevelGraph::new(4);
        for zone in 1..=4 {
            level0.set_record(ZoneRecord::new(zone, 1, 0));
        }
        level0.push_edge(1, ZoneEdgeRecord::new(2, 1));
        level0.push_edge(1, ZoneEdgeRecord::new(3, 0));
        level0.push_edge(2, ZoneEdgeRecord::new(4, 0));
        level0.push_edge(3, ZoneEdgeRecord::new(4, 0));
        let hierarchy = ZoneHierarchy::new(level0, level1, level2);

        let ZonePrecheckOutcome::Passed(result) = zone_precheck_flat(
            &hierarchy,
            1,
            4,
            MovementZone::Normal,
            &ZonePrecheckExclusions::default(),
        ) else {
            panic!("precheck should pass");
        };
        assert_eq!(result.paths[0], vec![1, 3, 4]);
    }

    #[test]
    fn zone_precheck_parent_gate_prunes_off_corridor_child_edges() {
        let mut hierarchy = fixture_hierarchy();
        hierarchy.levels[0].set_record(ZoneRecord::new(7, 99, 0));
        hierarchy.levels[0].push_edge(2, ZoneEdgeRecord::new(7, 0));
        hierarchy.levels[0].push_edge(7, ZoneEdgeRecord::new(6, 0));

        let ZonePrecheckOutcome::Passed(result) = zone_precheck_flat(
            &hierarchy,
            1,
            6,
            MovementZone::Normal,
            &ZonePrecheckExclusions::default(),
        ) else {
            panic!("precheck should pass");
        };
        assert_eq!(
            result.paths[0],
            vec![1, 2, 3, 4, 5, 6],
            "level-0 shortcut through an off-corridor parent is rejected"
        );
    }

    #[test]
    fn zone_precheck_parent_gate_allows_type_1_exception() {
        let mut hierarchy = fixture_hierarchy();
        hierarchy.levels[0].set_record(ZoneRecord::new(7, 99, 1));
        hierarchy.levels[0].push_edge(2, ZoneEdgeRecord::new(7, 0));
        hierarchy.levels[0].push_edge(7, ZoneEdgeRecord::new(6, 0));

        let ZonePrecheckOutcome::Passed(result) = zone_precheck_flat(
            &hierarchy,
            1,
            6,
            MovementZone::Crusher,
            &ZonePrecheckExclusions::default(),
        ) else {
            panic!("precheck should pass");
        };
        assert_eq!(result.paths[0], vec![1, 2, 7, 6]);
    }

    #[test]
    fn zone_precheck_type_1_exception_still_obeys_passability_matrix() {
        let mut level2 = ZoneLevelGraph::new(1);
        level2.set_record(ZoneRecord::new(1, 0, 0));

        let mut level1 = ZoneLevelGraph::new(1);
        level1.set_record(ZoneRecord::new(1, 1, 0));

        let mut level0 = ZoneLevelGraph::new(7);
        level0.set_record(ZoneRecord::new(1, 1, 0));
        level0.set_record(ZoneRecord::new(6, 1, 0));
        level0.set_record(ZoneRecord::new(7, 99, 1));
        level0.push_edge(1, ZoneEdgeRecord::new(7, 0));
        level0.push_edge(7, ZoneEdgeRecord::new(6, 0));
        let hierarchy = ZoneHierarchy::new(level0, level1, level2);

        assert_eq!(
            zone_precheck_flat(
                &hierarchy,
                1,
                6,
                MovementZone::Normal,
                &ZonePrecheckExclusions::default(),
            ),
            ZonePrecheckOutcome::Failed,
            "type 1 bypasses the parent gate, not the movement-zone passability matrix"
        );

        let ZonePrecheckOutcome::Passed(result) = zone_precheck_flat(
            &hierarchy,
            1,
            6,
            MovementZone::Crusher,
            &ZonePrecheckExclusions::default(),
        ) else {
            panic!("crusher should be allowed through type 1");
        };
        assert_eq!(result.paths[0], vec![1, 7, 6]);
    }

    #[test]
    fn zone_precheck_rejects_invalid_zone_type() {
        let mut level2 = ZoneLevelGraph::new(1);
        level2.set_record(ZoneRecord::new(1, 0, 0));

        let mut level1 = ZoneLevelGraph::new(1);
        level1.set_record(ZoneRecord::new(1, 1, 0));

        let mut level0 = ZoneLevelGraph::new(2);
        level0.set_record(ZoneRecord::new(1, 1, 0));
        level0.set_record(ZoneRecord::new(2, 1, passability::TERRAIN_TYPE_COUNT as u8));
        level0.push_edge(1, ZoneEdgeRecord::new(2, 0));
        let hierarchy = ZoneHierarchy::new(level0, level1, level2);

        assert_eq!(
            zone_precheck_flat(
                &hierarchy,
                1,
                2,
                MovementZone::Crusher,
                &ZonePrecheckExclusions::default(),
            ),
            ZonePrecheckOutcome::Failed
        );
    }

    #[test]
    fn zone_precheck_manual_exclusion_skips_only_matching_edge() {
        let mut hierarchy = fixture_hierarchy();
        hierarchy.levels[0].push_edge(3, ZoneEdgeRecord::new(5, 0));
        hierarchy.levels[0].push_edge(5, ZoneEdgeRecord::new(3, 0));
        let mut exclusions = ZonePrecheckExclusions::default();
        assert!(exclusions.insert(0, 3, 4));

        let ZonePrecheckOutcome::Passed(result) =
            zone_precheck_flat(&hierarchy, 1, 6, MovementZone::Normal, &exclusions)
        else {
            panic!("precheck should pass through another route");
        };
        assert_eq!(result.paths[0], vec![1, 2, 3, 5, 6]);
    }

    #[test]
    fn zone_precheck_manual_exclusion_does_not_ban_endpoint_zone() {
        let mut hierarchy = fixture_hierarchy();
        hierarchy.levels[0].push_edge(3, ZoneEdgeRecord::new(5, 0));
        hierarchy.levels[0].push_edge(5, ZoneEdgeRecord::new(3, 0));
        let mut exclusions = ZonePrecheckExclusions::default();
        assert!(exclusions.insert(0, 4, 5));

        let ZonePrecheckOutcome::Passed(result) =
            zone_precheck_flat(&hierarchy, 1, 6, MovementZone::Normal, &exclusions)
        else {
            panic!("precheck should pass through alternate edge into zone 5");
        };
        assert_eq!(result.paths[0], vec![1, 2, 3, 5, 6]);
    }

    #[test]
    fn zone_level_graph_cell_lookup_returns_invalid_out_of_bounds() {
        let graph = ZoneLevelGraph::new(2).with_cell_zone_ids(vec![1, 2], 2, 1);

        assert_eq!(graph.zone_at(0, 0), 1);
        assert_eq!(graph.zone_at(1, 0), 2);
        assert_eq!(graph.zone_at(2, 0), ZONE_INVALID);
        assert_eq!(graph.zone_at(0, 1), ZONE_INVALID);
    }

    #[test]
    fn zone_precheck_manual_exclusion_is_undirected() {
        let mut hierarchy = fixture_hierarchy();
        hierarchy.levels[0].push_edge(3, ZoneEdgeRecord::new(5, 0));
        hierarchy.levels[0].push_edge(5, ZoneEdgeRecord::new(3, 0));
        let mut exclusions = ZonePrecheckExclusions::default();
        assert!(exclusions.insert(0, 4, 3));

        let ZonePrecheckOutcome::Passed(result) =
            zone_precheck_flat(&hierarchy, 1, 6, MovementZone::Normal, &exclusions)
        else {
            panic!("precheck should treat reversed exclusion as the same edge");
        };
        assert_eq!(result.paths[0], vec![1, 2, 3, 5, 6]);
    }

    #[test]
    fn zone_precheck_producer_edges_preserve_append_order_and_duplicates() {
        let mut exclusions = ZonePrecheckExclusions::default();

        assert!(exclusions.append_producer_edge(0, 3, 4));
        assert!(exclusions.append_producer_edge(0, 4, 3));
        assert!(exclusions.append_producer_edge(0, 2, 4));

        assert_eq!(
            exclusions.ordered_edges(0),
            &[
                ZoneEdgeKey::new(3, 4).unwrap(),
                ZoneEdgeKey::new(3, 4).unwrap(),
                ZoneEdgeKey::new(2, 4).unwrap(),
            ]
        );
        assert!(exclusions.contains(0, 3, 4));
        assert!(exclusions.contains(0, 2, 4));
    }

    #[test]
    fn zone_precheck_missing_parent_record_fails_closed() {
        let mut level2 = ZoneLevelGraph::new(1);
        level2.set_record(ZoneRecord::new(1, 0, 0));

        let level1 = ZoneLevelGraph::new(1);
        let mut level0 = ZoneLevelGraph::new(2);
        level0.set_record(ZoneRecord::new(1, 1, 0));
        level0.set_record(ZoneRecord::new(2, 1, 0));
        level0.push_edge(1, ZoneEdgeRecord::new(2, 0));
        let hierarchy = ZoneHierarchy::new(level0, level1, level2);

        assert_eq!(
            zone_precheck_flat(
                &hierarchy,
                1,
                2,
                MovementZone::Normal,
                &ZonePrecheckExclusions::default(),
            ),
            ZonePrecheckOutcome::Failed
        );
    }

    #[test]
    fn zone_precheck_bridge_edges_are_zero_flagged_fixture_contract() {
        let edge = ZoneEdgeRecord::new(2, 0);
        assert_eq!(edge.flag, 0);
    }
}
