//! Binary-style zone hierarchy: the gamemd-style hierarchy records used by
//! `Zone_precheck`, the route-selection data behind the hierarchy-marked A*.
//! Synthetic tests in this module do not assert stock Carville route cells,
//! route direction, or full high-bridge player-visible path parity.
//!
//! ## Dependency rules
//! - Part of sim/ - depends on sim/zone_map, pathfinding passability, and rules movement zones.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};

use super::native_heap::NativeHeap;
use super::passability;
use super::search_table::SearchTable;
use super::zone_map::{ZONE_INVALID, ZoneId};
use crate::rules::locomotor_type::MovementZone;
use crate::util::lookup_hash::LookupMap;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53};

/// Retained House+57E4 reader. No Map/GetCell lookup belongs to585F40.
pub struct ZonePrecheckThreat<'a> {
    coefficient: NativeF64Bits,
    padded_lookup: &'a dyn Fn(i32) -> Result<i32, String>,
}

impl<'a> ZonePrecheckThreat<'a> {
    pub(crate) fn new(
        coefficient: NativeF64Bits,
        padded_lookup: &'a dyn Fn(i32) -> Result<i32, String>,
    ) -> Self {
        Self {
            coefficient,
            padded_lookup,
        }
    }
}

/// Original585F40. Native seed records identify the four-cell threat grid;
/// coarse level2 uses two leading corners of each endpoint block. Signed ADD
/// wraps before SAR1, and every coordinate addition wraps as a signed word.
/// Executable controls: fv_cell_attack/zone_threat.{py,json,meta.json}.
fn estimate_zone_threat(
    graph: &ZoneLevelGraph,
    level: usize,
    source: ZoneId,
    target: ZoneId,
    lookup: &dyn Fn(i32) -> Result<i32, String>,
) -> Result<i32, String> {
    if level == 0 || level > 2 {
        return Ok(0);
    }
    let index = |zone| {
        graph
            .record(zone)
            .and_then(ZoneRecord::native_threat_index)
            .ok_or_else(|| format!("hierarchy level{level} zone{zone} lacks native threat seed"))
    };
    let target_index = index(target)?;
    if level == 1 {
        return lookup(target_index);
    }
    let decode = |index: i32| {
        let remainder = index.wrapping_sub(1) % 130;
        let x = remainder.wrapping_mul(4);
        let y = (index.wrapping_sub(remainder) / 130)
            .wrapping_mul(4)
            .wrapping_sub(4);
        let raw = (x as i16, y as i16);
        let aligned = (
            raw.0.wrapping_sub(i16::from((x / 4) % 2 != 0)),
            raw.1.wrapping_sub(i16::from((y / 4) % 2 != 0)),
        );
        (raw, aligned)
    };
    let (source_raw, source) = decode(index(source)?);
    let (target_raw, target) = decode(target_index);
    let read =
        |coord: (i16, i16)| lookup(crate::sim::house_threat::HouseSpatialThreat::index(coord));
    if source == target {
        let a = read(target_raw)?;
        return Ok(a.wrapping_add(read(source_raw)?) >> 1);
    }
    let direction = if source.0 < target.0 {
        if source.1 < target.1 {
            3
        } else if source.1 == target.1 {
            2
        } else {
            1
        }
    } else if source.0 > target.0 {
        if source.1 < target.1 {
            5
        } else if source.1 == target.1 {
            6
        } else {
            7
        }
    } else if source.1 < target.1 {
        4
    } else {
        0
    };
    // Original bytes82A984/82A9C4; offsets populated by CRT585EE0.
    const START: [[usize; 2]; 8] = [
        [0, 1],
        [1, 1],
        [1, 3],
        [3, 3],
        [2, 3],
        [2, 2],
        [0, 2],
        [0, 0],
    ];
    const END: [[usize; 2]; 8] = [
        [2, 3],
        [2, 2],
        [0, 2],
        [0, 0],
        [0, 1],
        [1, 1],
        [1, 3],
        [3, 3],
    ];
    const CORNERS: [(i16, i16); 4] = [(0, 0), (4, 0), (0, 4), (4, 4)];
    let sample = |base: (i16, i16), corner: usize| {
        let offset = CORNERS[corner];
        read((base.0.wrapping_add(offset.0), base.1.wrapping_add(offset.1)))
    };
    let a = sample(source, START[direction][0])?;
    let b = sample(source, START[direction][1])?;
    let c = sample(target, END[direction][0])?;
    let d = sample(target, END[direction][1])?;
    Ok(a.min(b).wrapping_add(c.min(d)) >> 1)
}

pub(crate) const ZONE_PRECHECK_LEVELS: usize = 3;
const TOP_LEVEL: usize = 2;
/// Per-terrain-type base cost `Zone_precheck` @ `0x0042C290` adds per hop.
///
/// The native table is the eight f32s at `0x007E3794`
/// (`[1.0, 0, 0, 1.0, 1.0, 0, 1.0, 1.0]`). Costs are spilled to binary32
/// after every edge, including the binary64 `0.001` edge-flag contribution.
const ZONE_BASE_COSTS: [i32; passability::TERRAIN_TYPE_COUNT] = [1, 0, 0, 1, 1, 0, 1, 1];

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
    /// Native record+20, produced from the scanline seed (not a centroid).
    native_threat_index: Option<i32>,
}

impl ZoneRecord {
    pub(crate) fn new(zone_id: ZoneId, parent: ZoneId, zone_type: u8) -> Self {
        Self {
            zone_id,
            parent,
            zone_type,
            native_threat_index: None,
        }
    }

    /// Original581F90 and584550 truncate signed seed words by4, then add
    /// the House+57E4 padding. Both full and local rebuilds use this owner.
    pub(crate) fn from_seed(
        zone_id: ZoneId,
        parent: ZoneId,
        zone_type: u8,
        seed: (i16, i16),
    ) -> Self {
        let mut record = Self::new(zone_id, parent, zone_type);
        record.native_threat_index =
            Some(crate::sim::house_threat::HouseSpatialThreat::index(seed));
        record
    }

    pub(crate) fn native_threat_index(self) -> Option<i32> {
        self.native_threat_index
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
pub struct ZoneLevelGraph {
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

    pub fn zone_at(&self, x: u16, y: u16) -> ZoneId {
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
pub struct ZoneHierarchy {
    levels: [ZoneLevelGraph; ZONE_PRECHECK_LEVELS],
    memo: PrecheckMemo,
}

impl ZoneHierarchy {
    /// Fold the retained navigation history, including ordered native edges.
    /// Original584550 appends/replaces records after local changes;581F90 can
    /// produce a different graph from the same current Cell facts. Record+20
    /// then supplies585F40 threat reads and42C290 route selection. Ordinary
    /// LoadContent67E8CD intentionally rebuilds this history through581F50.
    pub(crate) fn fold_history_hash(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;

        for graph in &self.levels {
            graph.width.hash(hasher);
            graph.height.hash(hasher);
            (graph.records.len() as u64).hash(hasher);
            for record in &graph.records {
                record.is_some().hash(hasher);
                if let Some(record) = record {
                    record.zone_id.hash(hasher);
                    record.parent.hash(hasher);
                    record.zone_type.hash(hasher);
                    record.native_threat_index.hash(hasher);
                }
            }
            (graph.edges.len() as u64).hash(hasher);
            for edges in &graph.edges {
                (edges.len() as u64).hash(hasher);
                for edge in edges {
                    edge.neighbor.hash(hasher);
                    edge.flag.hash(hasher);
                }
            }
            (graph.cell_zone_ids.len() as u64).hash(hasher);
            for id in &graph.cell_zone_ids {
                id.hash(hasher);
            }
            (graph.native_padding_zone_ids.len() as u64).hash(hasher);
            for (index, id) in &graph.native_padding_zone_ids {
                (*index as u64).hash(hasher);
                id.hash(hasher);
            }
        }
    }

    /// Levels are passed low-to-high: level 0, level 1, level 2.
    pub(crate) fn new(
        level0: ZoneLevelGraph,
        level1: ZoneLevelGraph,
        level2: ZoneLevelGraph,
    ) -> Self {
        Self {
            levels: [level0, level1, level2],
            memo: PrecheckMemo::default(),
        }
    }

    pub fn level(&self, level: usize) -> Option<&ZoneLevelGraph> {
        self.levels.get(level)
    }

    /// Every change to the levels comes through here, so it drops the
    /// precheck answers given for the old levels.
    pub(crate) fn levels_mut(&mut self) -> &mut [ZoneLevelGraph; ZONE_PRECHECK_LEVELS] {
        self.memo.clear();
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

/// The zone pairs `zone_precheck_flat` may not cross, per level: the three
/// vectors at `PathfinderClass+0x74` (`0x18` apart), which hold
/// `min << 16 | max` and which `Zone_precheck` scans for each edge it would
/// take (`0x0042C618..0x0042C664`). One path search owns them:
/// `AStar_pathfind_search` clears them as it starts (`0x0042C912..0x0042C925`)
/// and its retries add to them (`zone_search::precheck_retry`). Native
/// appends duplicates; a set answers the same scan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ZonePrecheckExclusions {
    lookup: [BTreeSet<ZoneEdgeKey>; ZONE_PRECHECK_LEVELS],
}

impl ZonePrecheckExclusions {
    /// Exclude the pair `a`, `b` at `level`. Native never adds a pair of one
    /// zone (`0x0042CE04`, `0x0042D04E`, `0x0042D0EB`). One naming zone 0,
    /// the Outside sentinel no row may enter, is not kept.
    pub(crate) fn insert(&mut self, level: usize, a: ZoneId, b: ZoneId) -> bool {
        let Some(key) = ZoneEdgeKey::new(a, b) else {
            return false;
        };
        self.lookup
            .get_mut(level)
            .is_some_and(|set| set.insert(key))
    }

    fn contains(&self, level: usize, a: ZoneId, b: ZoneId) -> bool {
        ZoneEdgeKey::new(a, b)
            .and_then(|key| self.lookup.get(level).map(|set| set.contains(&key)))
            .unwrap_or(false)
    }

    fn is_empty(&self) -> bool {
        self.lookup.iter().all(BTreeSet::is_empty)
    }
}

/// The zones one precheck level marked: its route's zones, as a bitset by
/// zone id. The cell A*'s corridor gate asks it for every neighbour, so
/// membership is one word test.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarkedZones {
    /// Bit `z % 64` of word `z / 64` is set for each marked zone `z`; the last
    /// word holds the highest marked zone, so equal sets compare equal.
    bits: Vec<u64>,
}

impl MarkedZones {
    pub(crate) fn contains(&self, zone: &ZoneId) -> bool {
        let zone = usize::from(*zone);
        self.bits
            .get(zone / 64)
            .is_some_and(|word| word & (1 << (zone % 64)) != 0)
    }
}

impl FromIterator<ZoneId> for MarkedZones {
    fn from_iter<I: IntoIterator<Item = ZoneId>>(zones: I) -> Self {
        let mut bits = Vec::new();
        for zone in zones {
            let zone = usize::from(zone);
            if bits.len() <= zone / 64 {
                bits.resize(zone / 64 + 1, 0);
            }
            bits[zone / 64] |= 1 << (zone % 64);
        }
        Self { bits }
    }
}

/// Successful precheck output: selected paths and marker sets per hierarchy level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZonePrecheckResult {
    pub paths: [Vec<ZoneId>; ZONE_PRECHECK_LEVELS],
    pub marked: [MarkedZones; ZONE_PRECHECK_LEVELS],
    #[cfg(any(test, feature = "test-support"))]
    pub pops: Vec<(usize, ZoneId, u32, u32)>,
}

impl ZonePrecheckResult {
    fn new() -> Self {
        Self {
            paths: std::array::from_fn(|_| Vec::new()),
            marked: std::array::from_fn(|_| MarkedZones::default()),
            #[cfg(any(test, feature = "test-support"))]
            pops: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZonePrecheckOutcome {
    Passed(ZonePrecheckResult),
    Failed,
}

/// One native `+64` search record per accepted edge, not per zone. A queued
/// record retains its own predecessor even if that zone receives a cheaper
/// record later (`42C666..42C690`).
struct PrecheckNode {
    parent: Option<usize>,
    zone: ZoneId,
    cost: NativeF32Bits,
    depth: u32,
}

/// The queue orders nodes by their cost through the shared native heap.
/// Signed binary32 costs compare through the existing deterministic FP owner;
/// original ftol's consumed low dword may be negative in raw overflow controls.
fn by_cost(nodes: &[PrecheckNode]) -> impl Fn(&usize, &usize) -> bool + '_ {
    move |a, b| precheck_cost_less(nodes[*a].cost, nodes[*b].cost)
}

/// `42C5BB..42C5D2`: add base type cost, then the edge flag and spill once.
/// The active-retail control word is PC53/chop, including this binary32
/// store. Native FV heap-pop bits and equal-cost route controls are pinned in
/// `tools/spatial_oracle/fv_cell_attack/zone_cost.json`.
/// Its two eleven-edge/five-flag routes have equal rational cost, but the
/// different spill histories choose different paths than integer1000 scaling.
/// The selected hierarchy corridor therefore requires these deterministic
/// binary32 stores; reusing the existing arithmetic owner avoids host FP modes.
fn precheck_cost_less(a: NativeF32Bits, b: NativeF32Bits) -> bool {
    X87Chop53::compare(
        X87Chop53::load_f32(a).expect("finite hierarchy cost"),
        X87Chop53::load_f32(b).expect("finite hierarchy cost"),
    ) == crate::util::native_x87::X87Ordering::Less
}

fn precheck_edge_cost(cost: NativeF32Bits, base: i32, threat: i32, flag: u8) -> NativeF32Bits {
    let mut total = X87Chop53::add(
        X87Chop53::load_i32(base),
        X87Chop53::load_f32(cost).expect("finite precheck node cost"),
    );
    total = X87Chop53::add(total, X87Chop53::load_i32(threat));
    if flag != 0 {
        total = X87Chop53::add(
            total,
            X87Chop53::load_f64(NativeF64Bits::from_bits(0x3f50_624d_d2f1_a9fc))
                .expect("native 0.001 edge cost"),
        );
    }
    X87Chop53::store_f32(total).expect("bounded hierarchy cost")
}

#[derive(Clone, Debug, PartialEq)]
struct PrecheckLevelResult {
    path: Vec<ZoneId>,
    #[cfg(any(test, feature = "test-support"))]
    pops: Vec<(usize, ZoneId, u32, u32)>,
}

/// Level searches already answered for one hierarchy's levels.
///
/// Without exclusions or an active threat, a level search reads only the
/// levels, its start and goal zones and the movement zone: the parent
/// corridor it is confined to is the parent level's answer for the start's
/// and goal's own parents. Path searches keep asking for the same corridors,
/// and the top level, which no corridor confines, searches much of its graph
/// each time. The answers are exact, so the memo changes no result;
/// [`ZoneHierarchy::levels_mut`] clears it. Each level's answers are split
/// over [`PRECHECK_MEMO_SHARDS`] locks by start zone, so a path batch's
/// workers seldom wait on each other; a kept answer is shared, and copied
/// outside the lock.
#[derive(Default)]
struct PrecheckMemo([[Mutex<PrecheckAnswers>; PRECHECK_MEMO_SHARDS]; ZONE_PRECHECK_LEVELS]);

type PrecheckAnswers = LookupMap<PrecheckKey, Arc<Option<PrecheckLevelResult>>>;

/// Locks per memo level.
const PRECHECK_MEMO_SHARDS: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct PrecheckKey {
    movement_zone: MovementZone,
    start: ZoneId,
    goal: ZoneId,
}

/// Answers kept per level shard before that shard starts over. Bounds memory
/// on maps whose movers spread over many base-level zones; the coarse levels'
/// few, expensive answers outlive a base-level reset, and a dropped answer is
/// only searched again.
const PRECHECK_MEMO_ANSWERS: usize = (1 << 16) / PRECHECK_MEMO_SHARDS;

impl PrecheckMemo {
    fn shard(&self, level: usize, key: &PrecheckKey) -> &Mutex<PrecheckAnswers> {
        &self.0[level][key.start as usize % PRECHECK_MEMO_SHARDS]
    }

    /// The memoized answer for `key` at `level`, else `search`'s, kept for
    /// next time. No lock is held while `search` runs.
    fn answer(
        &self,
        level: usize,
        key: PrecheckKey,
        search: impl FnOnce() -> Result<Option<PrecheckLevelResult>, String>,
    ) -> Result<Option<PrecheckLevelResult>, String> {
        let shard = self.shard(level, &key);
        let kept = lock(shard).get(&key).cloned();
        if let Some(answer) = kept {
            let answer = Option::clone(&answer);
            // Every test that prechecks also checks the memo against a new
            // search, including after production hierarchy repairs.
            #[cfg(any(test, feature = "test-support"))]
            assert_eq!(answer, search()?, "a kept precheck answer went stale");
            return Ok(answer);
        }
        let answer = search()?;
        let mut answers = lock(shard);
        if answers.len() >= PRECHECK_MEMO_ANSWERS {
            answers.clear();
        }
        answers.insert(key, Arc::new(answer.clone()));
        Ok(answer)
    }

    fn clear(&mut self) {
        for shard in self.0.iter_mut().flatten() {
            shard
                .get_mut()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    fn len(&self) -> usize {
        self.0.iter().flatten().map(|shard| lock(shard).len()).sum()
    }
}

fn lock(shard: &Mutex<PrecheckAnswers>) -> std::sync::MutexGuard<'_, PrecheckAnswers> {
    shard.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A copy answers its own searches.
impl Clone for PrecheckMemo {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for PrecheckMemo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PrecheckMemo")
    }
}

/// Original42C290: retained House threat is estimated before best-cost,
/// parent/passability and exclusion gates, multiplied by Foot4DC760's
/// coefficient, and converted by original7C5F00 before the binary32 cost spill.
/// `None` models its NULL Foot input; live Foot callers supply the shared House.
pub fn zone_precheck_flat(
    hierarchy: &ZoneHierarchy,
    start_level0: ZoneId,
    goal_level0: ZoneId,
    movement_zone: MovementZone,
    exclusions: &ZonePrecheckExclusions,
    threat: Option<ZonePrecheckThreat<'_>>,
) -> Result<ZonePrecheckOutcome, String> {
    let coefficient = threat
        .as_ref()
        .map(|context| X87Chop53::load_f64(context.coefficient))
        .transpose()
        .map_err(|e| e.to_string())?;
    let active_threat = coefficient.filter(|coefficient| {
        X87Chop53::compare(
            *coefficient,
            X87Chop53::load_f64(NativeF64Bits::from_bits(0x3ee4_f8b5_88e3_68f1))
                .expect("native threshold"),
        ) == crate::util::native_x87::X87Ordering::Greater
    });
    let Some(start_zones) = hierarchy.ancestors_from_level0(start_level0) else {
        return Ok(ZonePrecheckOutcome::Failed);
    };
    let Some(goal_zones) = hierarchy.ancestors_from_level0(goal_level0) else {
        return Ok(ZonePrecheckOutcome::Failed);
    };

    // Exclusions and threat costs are inputs the memo key does not hold.
    let memo = (exclusions.is_empty() && active_threat.is_none()).then_some(&hierarchy.memo);
    let mut result = ZonePrecheckResult::new();
    for level in (0..ZONE_PRECHECK_LEVELS).rev() {
        let parent_marked = if level < TOP_LEVEL {
            Some(&result.marked[level + 1])
        } else {
            None
        };
        let search = || {
            search_precheck_level(
                hierarchy.level(level).expect("fixed hierarchy level"),
                level,
                start_zones[level],
                goal_zones[level],
                movement_zone,
                parent_marked,
                exclusions,
                active_threat.zip(threat.as_ref().map(|context| context.padded_lookup)),
            )
        };
        let selected = match memo {
            Some(memo) => memo.answer(
                level,
                PrecheckKey {
                    movement_zone,
                    start: start_zones[level],
                    goal: goal_zones[level],
                },
                search,
            )?,
            None => search()?,
        };
        let Some(selected) = selected else {
            return Ok(ZonePrecheckOutcome::Failed);
        };
        result.marked[level] = selected.path.iter().copied().collect();
        result.paths[level] = selected.path;
        #[cfg(any(test, feature = "test-support"))]
        result.pops.extend(selected.pops);
    }

    Ok(ZonePrecheckOutcome::Passed(result))
}

/// Active threat coefficient paired with the padded House threat lookup.
type ActiveZoneThreat<'a> = (
    crate::util::native_x87::X87Value,
    &'a dyn Fn(i32) -> Result<i32, String>,
);

/// Reusable scratch for `search_precheck_level`: pure scratch, nothing in it
/// outlives one level search.
#[derive(Default)]
struct PrecheckScratch {
    /// The best cost each zone has been reached with in the running search.
    best: SearchTable<NativeF32Bits>,
    nodes: Vec<PrecheckNode>,
    /// The size the PathfinderClass constructor gives this heap (`0x2710` at
    /// `0x0042A7B2`). A full heap drops the insert, but the record and the
    /// zone's best cost are kept.
    heap: NativeHeap<usize, 0x2710>,
}

thread_local! {
    /// Precheck scratch reused across calls; each thread runs one precheck at
    /// a time and a precheck never starts another.
    static PRECHECK_SCRATCH: std::cell::RefCell<PrecheckScratch> =
        std::cell::RefCell::new(PrecheckScratch::default());
}

fn search_precheck_level(
    graph: &ZoneLevelGraph,
    level: usize,
    start: ZoneId,
    goal: ZoneId,
    movement_zone: MovementZone,
    parent_marked: Option<&MarkedZones>,
    exclusions: &ZonePrecheckExclusions,
    threat: Option<ActiveZoneThreat<'_>>,
) -> Result<Option<PrecheckLevelResult>, String> {
    PRECHECK_SCRATCH.with(|cell| {
        let mut scratch = cell
            .try_borrow_mut()
            .expect("zone precheck re-entered: a precheck must not start another");
        search_precheck_level_in(
            &mut scratch,
            graph,
            level,
            start,
            goal,
            movement_zone,
            parent_marked,
            exclusions,
            threat,
        )
    })
}

fn search_precheck_level_in(
    scratch: &mut PrecheckScratch,
    graph: &ZoneLevelGraph,
    level: usize,
    start: ZoneId,
    goal: ZoneId,
    movement_zone: MovementZone,
    parent_marked: Option<&MarkedZones>,
    exclusions: &ZonePrecheckExclusions,
    threat: Option<ActiveZoneThreat<'_>>,
) -> Result<Option<PrecheckLevelResult>, String> {
    if graph.record(start).is_none() || graph.record(goal).is_none() {
        return Ok(None);
    }
    // 42C3B0..42C3E4 checks equality before any movement-type gate. The
    // start record is not type-gated even when a search is needed.
    if start == goal {
        return Ok(Some(PrecheckLevelResult {
            path: vec![start],
            #[cfg(any(test, feature = "test-support"))]
            pops: Vec::new(),
        }));
    }

    let zone_count = graph.zone_count() as usize;
    let PrecheckScratch { best, nodes, heap } = scratch;
    best.begin(zone_count + 1, NativeF32Bits::from_bits(0));
    nodes.clear();
    heap.clear();
    nodes.push(PrecheckNode {
        parent: None,
        zone: start,
        cost: NativeF32Bits::from_bits(0),
        depth: 0,
    });
    best.set(usize::from(start), NativeF32Bits::from_bits(0));
    heap.push(0, by_cost(nodes));
    #[cfg(any(test, feature = "test-support"))]
    let mut pops = Vec::new();

    while let Some(index) = heap.pop(by_cost(nodes)) {
        let node = &nodes[index];
        let (zone, cost, depth) = (node.zone, node.cost, node.depth);
        #[cfg(any(test, feature = "test-support"))]
        pops.push((level, zone, cost.bits(), depth));
        if zone == goal {
            return Ok(Some(PrecheckLevelResult {
                path: reconstruct_zone_path(nodes, index),
                #[cfg(any(test, feature = "test-support"))]
                pops,
            }));
        }

        // Native pops and expands retained records even after a cheaper
        // record for that zone was queued; the neighbour best-cost gate is
        // the only repeated-visit filter (42C5D8..42C5EA).
        for edge in graph.edges(zone) {
            let neighbor = edge.neighbor;
            if neighbor as usize > zone_count {
                continue;
            }
            let Some(record) = graph.record(neighbor) else {
                continue;
            };
            //42C580 precedes all route-admission gates, including a known
            //best cost and explicit edge exclusion. Preserve read order.
            let threat_cost = if let Some((coefficient, lookup)) = threat {
                let estimate = estimate_zone_threat(graph, level, zone, neighbor, lookup)?;
                X87Chop53::ftol_i32_low_masked(X87Chop53::mul(
                    X87Chop53::load_i32(estimate),
                    coefficient,
                ))
            } else {
                0
            };
            let Some(base_cost) = ZONE_BASE_COSTS.get(record.zone_type as usize).copied() else {
                continue;
            };
            let new_cost = precheck_edge_cost(cost, base_cost, threat_cost, edge.flag);
            if best
                .get(usize::from(neighbor))
                .is_some_and(|best| !precheck_cost_less(new_cost, best))
            {
                continue;
            }
            if let Some(marked) = parent_marked {
                let parent_allowed = record.zone_type == 1 || marked.contains(&record.parent);
                if !parent_allowed {
                    continue;
                }
            }
            if !passability::is_passable_for_zone(record.zone_type, movement_zone) {
                continue;
            }
            if exclusions.contains(level, zone, neighbor) {
                continue;
            }
            {
                best.set(usize::from(neighbor), new_cost);
                let next = nodes.len();
                nodes.push(PrecheckNode {
                    parent: Some(index),
                    zone: neighbor,
                    cost: new_cost,
                    depth: depth + 1,
                });
                heap.push(next, by_cost(nodes));
            }
        }
    }

    Ok(None)
}

fn reconstruct_zone_path(nodes: &[PrecheckNode], goal: usize) -> Vec<ZoneId> {
    let mut path = Vec::new();
    let mut current = Some(goal);
    while let Some(index) = current {
        path.push(nodes[index].zone);
        current = nodes[index].parent;
    }
    path.reverse();
    path
}

#[cfg(test)]
#[path = "zone_cost_native_tests.rs"]
mod zone_cost_native_tests;

#[cfg(test)]
mod tests {
    use super::*;

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
    fn zone_precheck_searches_levels_2_1_0_and_retains_paths() {
        let outcome = zone_precheck_flat(
            &fixture_hierarchy(),
            1,
            6,
            MovementZone::Crusher,
            &ZonePrecheckExclusions::default(),
            None,
        )
        .unwrap();
        let ZonePrecheckOutcome::Passed(result) = outcome else {
            panic!("precheck should pass");
        };
        assert_eq!(result.paths[2], vec![1, 2]);
        assert_eq!(result.paths[1], vec![1, 2, 3, 4]);
        assert_eq!(result.paths[0], vec![1, 2, 3, 4, 5, 6]);
        assert!(result.marked[0].contains(&4));
    }

    #[test]
    fn marked_zones_hold_ids_past_the_first_word() {
        let marked: MarkedZones = [3, 64, 200].into_iter().collect();
        for zone in [3, 64, 200] {
            assert!(marked.contains(&zone), "{zone}");
        }
        for zone in [4, 63, 65, 199, 10_000] {
            assert!(!marked.contains(&zone), "{zone}");
        }
        assert_eq!(marked, [200, 64, 3].into_iter().collect());
        assert_ne!(marked, [3, 64].into_iter().collect());
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
            None,
        )
        .unwrap() else {
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
            None,
        )
        .unwrap() else {
            panic!("precheck should pass");
        };
        assert_eq!(result.paths[0], vec![1, 3, 4]);
    }

    #[test]
    fn zone_precheck_parent_gate_prunes_off_corridor_child_edges() {
        let mut hierarchy = fixture_hierarchy();
        let level0 = &mut hierarchy.levels_mut()[0];
        level0.set_record(ZoneRecord::new(7, 99, 0));
        level0.push_edge(2, ZoneEdgeRecord::new(7, 0));
        level0.push_edge(7, ZoneEdgeRecord::new(6, 0));

        let ZonePrecheckOutcome::Passed(result) = zone_precheck_flat(
            &hierarchy,
            1,
            6,
            MovementZone::Normal,
            &ZonePrecheckExclusions::default(),
            None,
        )
        .unwrap() else {
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
        hierarchy.levels_mut()[0].set_record(ZoneRecord::new(7, 99, 1));
        hierarchy.levels_mut()[0].push_edge(2, ZoneEdgeRecord::new(7, 0));
        hierarchy.levels_mut()[0].push_edge(7, ZoneEdgeRecord::new(6, 0));

        let ZonePrecheckOutcome::Passed(result) = zone_precheck_flat(
            &hierarchy,
            1,
            6,
            MovementZone::Crusher,
            &ZonePrecheckExclusions::default(),
            None,
        )
        .unwrap() else {
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
                None
            )
            .unwrap(),
            ZonePrecheckOutcome::Failed,
            "type 1 bypasses the parent gate, not the movement-zone passability matrix"
        );

        let ZonePrecheckOutcome::Passed(result) = zone_precheck_flat(
            &hierarchy,
            1,
            6,
            MovementZone::Crusher,
            &ZonePrecheckExclusions::default(),
            None,
        )
        .unwrap() else {
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
                None
            )
            .unwrap(),
            ZonePrecheckOutcome::Failed
        );
    }

    #[test]
    fn zone_precheck_manual_exclusion_skips_only_matching_edge() {
        let mut hierarchy = fixture_hierarchy();
        hierarchy.levels_mut()[0].push_edge(3, ZoneEdgeRecord::new(5, 0));
        hierarchy.levels_mut()[0].push_edge(5, ZoneEdgeRecord::new(3, 0));
        let mut exclusions = ZonePrecheckExclusions::default();
        assert!(exclusions.insert(0, 3, 4));

        let ZonePrecheckOutcome::Passed(result) =
            zone_precheck_flat(&hierarchy, 1, 6, MovementZone::Normal, &exclusions, None).unwrap()
        else {
            panic!("precheck should pass through another route");
        };
        assert_eq!(result.paths[0], vec![1, 2, 3, 5, 6]);
    }

    #[test]
    fn zone_precheck_manual_exclusion_does_not_ban_endpoint_zone() {
        let mut hierarchy = fixture_hierarchy();
        hierarchy.levels_mut()[0].push_edge(3, ZoneEdgeRecord::new(5, 0));
        hierarchy.levels_mut()[0].push_edge(5, ZoneEdgeRecord::new(3, 0));
        let mut exclusions = ZonePrecheckExclusions::default();
        assert!(exclusions.insert(0, 4, 5));

        let ZonePrecheckOutcome::Passed(result) =
            zone_precheck_flat(&hierarchy, 1, 6, MovementZone::Normal, &exclusions, None).unwrap()
        else {
            panic!("precheck should pass through alternate edge into zone 5");
        };
        assert_eq!(result.paths[0], vec![1, 2, 3, 5, 6]);
    }

    #[test]
    fn zone_precheck_manual_exclusion_is_undirected() {
        let mut hierarchy = fixture_hierarchy();
        hierarchy.levels_mut()[0].push_edge(3, ZoneEdgeRecord::new(5, 0));
        hierarchy.levels_mut()[0].push_edge(5, ZoneEdgeRecord::new(3, 0));
        let mut exclusions = ZonePrecheckExclusions::default();
        assert!(exclusions.insert(0, 4, 3));

        let ZonePrecheckOutcome::Passed(result) =
            zone_precheck_flat(&hierarchy, 1, 6, MovementZone::Normal, &exclusions, None).unwrap()
        else {
            panic!("precheck should treat reversed exclusion as the same edge");
        };
        assert_eq!(result.paths[0], vec![1, 2, 3, 5, 6]);
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
                None
            )
            .unwrap(),
            ZonePrecheckOutcome::Failed
        );
    }

    /// Every start/goal pair, for movement zones the fixture's type-0 zones
    /// admit (Normal, Crusher) and refuse (Water), asked twice: each answer
    /// equals a fresh copy's, and the second round finds every answer kept.
    #[test]
    fn memoized_prechecks_match_fresh_searches() {
        let hierarchy = fixture_hierarchy();
        let mut kept_after_first_round = 0;
        let (mut passed, mut failed) = (0, 0);
        for round in 0..2 {
            for start in 1..=6 {
                for goal in 1..=6 {
                    for movement_zone in [
                        MovementZone::Normal,
                        MovementZone::Crusher,
                        MovementZone::Water,
                    ] {
                        let precheck = |hierarchy: &ZoneHierarchy| {
                            zone_precheck_flat(
                                hierarchy,
                                start,
                                goal,
                                movement_zone,
                                &ZonePrecheckExclusions::default(),
                                None,
                            )
                            .unwrap()
                        };
                        let answer = precheck(&hierarchy);
                        assert_eq!(answer, precheck(&hierarchy.clone()));
                        match answer {
                            ZonePrecheckOutcome::Passed(_) => passed += 1,
                            ZonePrecheckOutcome::Failed => failed += 1,
                        }
                    }
                }
            }
            let kept = hierarchy.memo.len();
            if round == 0 {
                kept_after_first_round = kept;
            } else {
                assert_eq!(kept, kept_after_first_round);
            }
        }
        assert!(kept_after_first_round > 0 && passed > 0 && failed > 0);
    }

    /// A level change drops the answers given for the old levels: a new
    /// base-level shortcut is taken at once.
    #[test]
    fn changing_a_level_clears_the_memo() {
        let mut hierarchy = fixture_hierarchy();
        let route = |hierarchy: &ZoneHierarchy| {
            let ZonePrecheckOutcome::Passed(result) = zone_precheck_flat(
                hierarchy,
                1,
                6,
                MovementZone::Normal,
                &ZonePrecheckExclusions::default(),
                None,
            )
            .unwrap() else {
                panic!("precheck should pass");
            };
            result.paths[0].clone()
        };
        assert_eq!(route(&hierarchy), vec![1, 2, 3, 4, 5, 6]);
        let level0 = &mut hierarchy.levels_mut()[0];
        level0.push_edge(2, ZoneEdgeRecord::new(5, 0));
        level0.push_edge(5, ZoneEdgeRecord::new(2, 0));
        assert_eq!(route(&hierarchy), vec![1, 2, 5, 6]);
    }
}
