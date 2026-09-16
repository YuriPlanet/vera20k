//! A* pathfinding on the isometric grid.
//!
//! PathGrid stores per-cell walkability and bridge metadata (ground_walkable,
//! bridge_walkable, transition, height levels). Flat A* reads ground_walkable;
//! layered A* reads both layers for bridge-aware routing.
//!
//! TODO(RE): The stock neighbor predicate is richer than the grid-level checks in this
//! module. The RE corpus has closed the existence and numeric shape of the cost/legality
//! classes, but not yet enough of the surrounding runtime state to replace these local
//! passability/cost shortcuts end-to-end.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on map/ (MapCell, TilesetLookup for walkability).
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use super::cell_entry::{
    CanEnterCellContext, CanEnterLayerContext, TerrainEntryMode, evaluate_can_enter_cell,
    search_cell_cost_decision,
};
use super::terrain_cost::TerrainCostGrid;
use super::zone_hierarchy::ZoneLevelGraph;
use super::zone_map::{ZONE_INVALID, ZoneId};
use crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF;
use crate::map::map_file::MapCell;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::theater::TilesetLookup;
use crate::map::tube_facts::{TubeId, TubeSource};
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::sim::bridge_state::BridgeRuntimeState;
use crate::sim::movement::locomotor::MovementLayer;
use std::cell::Cell;
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

/// Reusable scratch buffers for `astar_search`. Lives in a thread-local so
/// repeated repaths share one set of full-map-sized Vecs instead of allocating
/// six fresh `vec![…; total_cells]` arrays per call (was the dominant heap
/// pressure source during repath storms — engineer micro, scatter cascades).
///
/// `reset(total_cells)` grows each buffer if the map is bigger than the
/// previous call and refills with the sentinel value; the `BinaryHeap` is
/// cleared but keeps capacity. Use through `with_workspace` only; the helper
/// asserts non-reentrancy so a future recursive A* caller fails loudly
/// instead of silently sharing buffers.
#[derive(Default)]
struct PathfindWorkspace {
    ground_g: Vec<i32>,
    bridge_g: Vec<i32>,
    ground_from: Vec<usize>,
    bridge_from: Vec<usize>,
    ground_closed: Vec<bool>,
    bridge_closed: Vec<bool>,
    open: BinaryHeap<Reverse<AStarNode>>,
}

impl PathfindWorkspace {
    fn reset(&mut self, total_cells: usize) {
        fn refill<T: Copy>(buf: &mut Vec<T>, len: usize, fill: T) {
            buf.clear();
            buf.resize(len, fill);
        }
        refill(&mut self.ground_g, total_cells, i32::MAX);
        refill(&mut self.bridge_g, total_cells, i32::MAX);
        refill(&mut self.ground_from, total_cells, usize::MAX);
        refill(&mut self.bridge_from, total_cells, usize::MAX);
        refill(&mut self.ground_closed, total_cells, false);
        refill(&mut self.bridge_closed, total_cells, false);
        self.open.clear();
    }
}

thread_local! {
    /// Thread-local reusable A* scratch. Safe because:
    ///   - pathfinding only runs on the sim thread,
    ///   - A* never recursively invokes A* (no reentrancy),
    ///   - the workspace holds zero hashed sim state — pure scratch.
    /// `try_borrow_mut` panics with a clear message if either invariant is
    /// ever broken by a future refactor.
    static PATHFIND_WORKSPACE: RefCell<PathfindWorkspace> =
        RefCell::new(PathfindWorkspace::default());
}

/// Run `body` with exclusive mutable access to the thread-local A* workspace,
/// pre-reset to hold `total_cells` entries.
fn with_pathfind_workspace<R>(
    total_cells: usize,
    body: impl FnOnce(&mut PathfindWorkspace) -> R,
) -> R {
    PATHFIND_WORKSPACE.with(|cell| {
        let mut ws = cell
            .try_borrow_mut()
            .expect("PATHFIND_WORKSPACE re-entered — A* is not allowed to call A* recursively");
        ws.reset(total_cells);
        body(&mut ws)
    })
}

/// Uniform A* edge cost for all 8 compass directions. The original engine has
/// no diagonal upcharge — `AStar_compute_edge_cost` takes no direction
/// parameter. The 1000× scale is chosen so DIR_TIEBREAK [1..=8] sits at
/// exactly 0.001..0.008 of base, matching the binary's tiebreaker ratio.
const STEP_COST: i32 = 1000;

/// Maximum nodes to evaluate before giving up. Prevents freezing on
/// pathologically large or impossible searches.
/// Original engine uses 65,527 (0xFFF7). We match this to avoid failing
/// on complex paths that the original would find.
const MAX_SEARCH_NODES: u32 = 65_527;

/// RA2 computes paths in 24-step segments. When a segment is exhausted before
/// reaching the destination, the pathfinder replans from the current position.
/// This limits lookahead and makes units adapt to obstacles discovered en route.
pub const MAX_PATH_SEGMENT_STEPS: usize = 24;

/// Code-2 (friendly moving) cost multipliers. Matches gamemd.exe
/// AStar_compute_edge_cost (0x00429830). See `compute_code2_multiplier`.
const CODE2_MULT_CLEARING: i32 = 1; // chain clears within 10 hops → baseline
const CODE2_MULT_JAM: i32 = 4; // urgency=1 OR full 10-step jam → traffic penalty
const CODE2_MULT_ROUTE_AROUND: i32 = 1000; // urgency=2 → route around blocker

/// Maximum hops in the code-2 blocker chain walk (urgency=0).
/// Matches the binary's `for (i = 0; i < 10; i++)` at 0x00429830.
const CODE2_CHAIN_MAX_HOPS: usize = 10;

/// Code-5 (enemy unit) cost multiplier. Binary DAT_0081870c[5] = 20.0.
const CODE5_MULT_ENEMY: i32 = 20;

/// Code-6 (stationary friendly) cost multiplier. Binary DAT_0081870c[6] = 8.0.
const CODE6_MULT_STATIONARY_ALLY: i32 = 8;

/// Temporary A* marker cost multiplier for `CellClass+0x140 & 0x40000`.
/// The original toggles this bit around one search; Rust models it as a
/// search-scoped overlay instead of mutating persistent `PathGrid` cells.
const SEARCH_MARKER_COST_MULTIPLIER: i32 = 4;

/// Entry in the entity soft-block map for A* cost computation.
/// Carries the blocker's next cell (for code-2 chain walk) and the
/// Can_Enter_Cell return code (2/5/6) that selects the cost multiplier.
#[derive(Debug, Clone, Copy)]
pub struct EntityBlockEntry {
    /// For code-2 (moving friendly): the blocker's next cell in its path.
    /// For codes 5/6: None (no chain walk — flat cost multiplier).
    pub next_cell: Option<(u16, u16)>,
    /// Can_Enter_Cell return code: 2 (moving friendly), 5 (enemy), 6 (stationary friendly).
    pub cost_code: u8,
    /// Whether the blocker is infantry.
    ///
    /// A* does not care, but the Drive selection gate does: infantry never
    /// hold the cell's vehicle occupation bit -- `0x20` is written only by
    /// `UnitClass__MarkCellOccupationBit20 @ 0x007441B0`, while
    /// `InfantryClass__MarkCellOccupancy @ 0x005217C0` writes
    /// `1 << GetSubCell` into the sub-cell bits of the same byte -- and a
    /// vehicle entitled to crush them must not be refused a curve on their
    /// account.
    pub blocker_is_infantry: bool,
}

/// Entity soft blockers split by object-list layer.
///
/// gamemd.exe scans either `FirstObject` (ground) or `AltObject` (bridge)
/// for soft blocker costs. Keeping those maps separate preserves stacked
/// same-cell ground/bridge occupants instead of collapsing them by coordinate.
#[derive(Debug, Clone, Default)]
pub struct LayeredEntityBlockMap {
    /// `BTreeMap` (not `HashMap`) for deterministic iteration if any future
    /// caller ever iterates these — sim convention since all pathing state
    /// must be lockstep-reproducible.
    ground: BTreeMap<(u16, u16), EntityBlockEntry>,
    bridge: BTreeMap<(u16, u16), EntityBlockEntry>,
    /// Every moving allied occupant, whether or not it raised a code. The
    /// Drive selection lane asks these for the per-mover head-on exit of
    /// `UnitClass::Can_Enter_Cell` (`0x0073F8D4..FA26`), which no per-owner
    /// code can carry because it depends on the mover's own facing and
    /// position.
    moving_allies: BTreeMap<(MovementLayer, (u16, u16)), MovingAllyOccupant>,
}

/// A moving allied occupant as the head-on exit sees it: its facing byte and
/// its lepton coordinates at the time the owner snapshot was built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovingAllyOccupant {
    pub facing: u8,
    pub world: [i32; 3],
}

impl LayeredEntityBlockMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_moving_ally(
        &mut self,
        layer: MovementLayer,
        cell: (u16, u16),
        occupant: MovingAllyOccupant,
    ) {
        if matches!(layer, MovementLayer::Ground | MovementLayer::Bridge) {
            self.moving_allies.insert((layer, cell), occupant);
        }
    }

    pub fn moving_ally(
        &self,
        layer: MovementLayer,
        cell: &(u16, u16),
    ) -> Option<MovingAllyOccupant> {
        self.moving_allies.get(&(layer, *cell)).copied()
    }

    pub fn insert(
        &mut self,
        layer: MovementLayer,
        cell: (u16, u16),
        entry: EntityBlockEntry,
    ) -> Option<EntityBlockEntry> {
        match layer {
            MovementLayer::Bridge => self.bridge.insert(cell, entry),
            MovementLayer::Ground => self.ground.insert(cell, entry),
            MovementLayer::Air | MovementLayer::Underground => None,
        }
    }

    pub fn get(&self, layer: MovementLayer, cell: &(u16, u16)) -> Option<&EntityBlockEntry> {
        match layer {
            MovementLayer::Bridge => self.bridge.get(cell),
            MovementLayer::Ground => self.ground.get(cell),
            MovementLayer::Air | MovementLayer::Underground => None,
        }
    }

    pub fn contains_key(&self, layer: MovementLayer, cell: &(u16, u16)) -> bool {
        self.get(layer, cell).is_some()
    }

    pub fn contains_any(&self, cell: &(u16, u16)) -> bool {
        self.ground.contains_key(cell) || self.bridge.contains_key(cell)
    }
}

/// Search-scoped destination-cell cost overlay for the gamemd.exe temporary
/// `CellClass+0x140 & 0x40000` A* marker.
///
/// Marking uses XOR parity: toggling the same cell twice cancels it, matching
/// the original bitwise lifecycle and peer-path replay behavior.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchMarkerOverlay {
    cells: BTreeSet<(u16, u16)>,
}

impl SearchMarkerOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn toggle(&mut self, cell: (u16, u16)) {
        if !self.cells.insert(cell) {
            self.cells.remove(&cell);
        }
    }

    pub fn contains(&self, cell: (u16, u16)) -> bool {
        self.cells.contains(&cell)
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

/// Per-cell count of adjacent dynamic blockers used by the hierarchy marker gate.
///
/// This is deliberately supplied by callers instead of inferred inside A*: a
/// missing count surface must not be interpreted as "all counts are zero".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlockerNeighborCounts {
    width: u16,
    height: u16,
    counts: Vec<u8>,
}

impl BlockerNeighborCounts {
    #[allow(dead_code)]
    pub(crate) fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            counts: vec![0; width as usize * height as usize],
        }
    }

    pub(crate) fn from_retained_wall_plane(width: u16, height: u16, counts: &[u8]) -> Self {
        assert_eq!(
            counts.len(),
            usize::from(width) * usize::from(height),
            "retained wall-neighbor plane must match pathfinding dimensions"
        );
        Self {
            width,
            height,
            counts: counts.to_vec(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn set_count(&mut self, x: u16, y: u16, count: u8) {
        if x < self.width && y < self.height {
            let idx = y as usize * self.width as usize + x as usize;
            self.counts[idx] = count;
        }
    }

    pub(crate) fn add_single_cell_neighbor_source(&mut self, x: u16, y: u16) {
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                self.increment_i32(x as i32 + dx, y as i32 + dy);
            }
        }
    }

    /// Reverse one single-cell producer's eight raw-byte neighbor increments.
    /// Overlay removal wiring is intentionally deferred to its owning batch.
    #[allow(dead_code)]
    pub(crate) fn remove_single_cell_neighbor_source(&mut self, x: u16, y: u16) {
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                self.decrement_i32(x as i32 + dx, y as i32 + dy);
            }
        }
    }

    pub(crate) fn add_building_expanded_foundation(
        &mut self,
        origin_x: u16,
        origin_y: u16,
        width: u16,
        height: u16,
    ) {
        let min_x = origin_x as i32 - 1;
        let min_y = origin_y as i32 - 1;
        let max_x = origin_x as i32 + width as i32;
        let max_y = origin_y as i32 + height as i32;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                self.increment_i32(x, y);
            }
        }
    }

    fn increment_i32(&mut self, x: i32, y: i32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let idx = y as usize * self.width as usize + x as usize;
        self.counts[idx] = self.counts[idx].wrapping_add(1);
    }

    // Retained with the deferred single-cell removal seam above.
    #[allow(dead_code)]
    fn decrement_i32(&mut self, x: i32, y: i32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let idx = y as usize * self.width as usize + x as usize;
        self.counts[idx] = self.counts[idx].wrapping_sub(1);
    }

    pub(crate) fn count_at(&self, x: u16, y: u16) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.counts[y as usize * self.width as usize + x as usize]
    }
}

/// Search-local equivalent of gamemd's hierarchy progress cell.
///
/// The cell starts at the A* source and advances only when an accepted neighbor
/// reaches the next selected level-0 `Zone_precheck` path zone.
#[derive(Debug)]
pub(crate) struct HierarchyProgressTracker<'a> {
    level0_path: &'a [ZoneId],
    progress_index: Cell<usize>,
    progress_cell: Cell<(u16, u16)>,
}

impl<'a> HierarchyProgressTracker<'a> {
    pub(crate) fn new(start: (u16, u16), level0_path: &'a [ZoneId]) -> Self {
        Self {
            level0_path,
            progress_index: Cell::new(0),
            progress_cell: Cell::new(start),
        }
    }

    fn maybe_advance(&self, zone: ZoneId, cell: (u16, u16)) {
        let next_index = self.progress_index.get().saturating_add(1);
        if self.level0_path.get(next_index).copied() == Some(zone) {
            self.progress_index.set(next_index);
            self.progress_cell.set(cell);
        }
    }

    pub(crate) fn progress_index(&self) -> usize {
        self.progress_index.get()
    }

    pub(crate) fn progress_cell(&self) -> (u16, u16) {
        self.progress_cell.get()
    }
}

/// A* expansion gate produced by `Zone_precheck`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HierarchyGate<'a> {
    pub level0_zones: &'a ZoneLevelGraph,
    pub marked_level0: &'a BTreeSet<ZoneId>,
    pub blocker_neighbor_counts: &'a BlockerNeighborCounts,
}

impl HierarchyGate<'_> {
    fn allows(&self, x: u16, y: u16) -> bool {
        let zone = self.level0_zones.zone_at(x, y);
        self.marked_level0.contains(&zone) || self.blocker_neighbor_counts.count_at(x, y) != 0
    }
}

/// Per-direction tie-breaker offsets added to g-cost.
///
/// `AStar_main_loop` @ `0x00429A90` adds the eight floats at `0x0081872C`,
/// `0.001` through `0.008`. With [`STEP_COST`] at 1000 the integers below are
/// the same fractions of a step, so no separate scaling is applied. Cardinals
/// get lower values than diagonals, which is what stops path oscillation when
/// several routes tie. Index order matches [`NEIGHBORS`]: N, NE, E, SE, S, SW,
/// W, NW.
const DIR_TIEBREAK: [i32; 8] = [
    1, // N   (original ≈0.001)
    5, // NE  (original ≈0.005)
    2, // E   (original ≈0.002)
    6, // SE  (original ≈0.006)
    3, // S   (original ≈0.003)
    7, // SW  (original ≈0.007)
    4, // W   (original ≈0.004)
    8, // NW  (original ≈0.008)
];

/// **VERA-internal, gamemd has no equivalent.** The native direction-8 arm at
/// `0x00429FA3` charges the Chebyshev cell distance from the tube entrance to
/// its **exit** and then skips the `FMUL`/`FADD` entirely — a tube edge takes
/// **no tiebreak at all**. VERA charges `STEP_COST × path_len()` plus this
/// constant, which keeps tube jumps ordered after normal edges on a tie.
///
/// Trigger: any expansion across a tube edge. Player effect: the tube's cost
/// differs from retail's whenever the tunnel is not straight (path length vs
/// endpoint distance), which shifts whether the unit takes the tunnel at all.
/// Frequency: tunnel maps only, and there on every cross-map order.
/// Downstream risk: low — it is one arm of the neighbour loop.
const TUBE_DIR_TIEBREAK: i32 = 9;

/// 8-directional neighbor offsets: (dx, dy, is_diagonal).
/// Order: N, NE, E, SE, S, SW, W, NW.
const NEIGHBORS: [(i32, i32, bool); 8] = [
    (0, -1, false), // N
    (1, -1, true),  // NE
    (1, 0, false),  // E
    (1, 1, true),   // SE
    (0, 1, false),  // S
    (-1, 1, true),  // SW
    (-1, 0, false), // W
    (-1, -1, true), // NW
];

/// Threshold for ground vs bridge closed-list selection.
/// Binary: `abs(path_height - cell.height_level) < 2`, the `CMP EAX,0x1` at
/// `0x00429E75` inside the layer-flag block that starts at `0x00429E54`.
const BRIDGE_HEIGHT_THRESHOLD: u8 = 2;

/// Encode source cell index + bridge flag into came_from value.
/// Max map = 512x512 = 262,144 cells -> fits in 18 bits, leaving bit 20 free.
const CAME_FROM_BRIDGE: usize = 1 << 20;

/// Determine whether a node at `path_height` should use the bridge closed list
/// for a given neighbor cell. Uses the CURRENT node's height (not computed
/// neighbor height). Matches the binary inline check at `0x00429E54`.
///
/// **Recorded mismatch, not closed:** the native test is `TEST AH,0x1` on
/// `Cell+0x140` (`0x00429E5A`) — the `0x100` structural-bridge bit, which VERA
/// calls `has_structural_bridge()`. This reads `bridge_walkable`, and the two
/// deliberately differ: the producer marks ramp and bridgehead cells walkable
/// *without* marking them structural. `check_bridge_traversal` in the same
/// expansion reads `has_structural_bridge()` for that same bit, so the two
/// halves of one step disagree about what `0x100` means — and
/// [`compute_neighbor_height`] is a third reader with the same mismatch, at all
/// three of its cases (native gates them on `Cell+0x140 & 0x100` at
/// `0x0042A4B6` and `0x0042A4C2`). Trigger: any ramp or
/// bridgehead cell. Player effect: the closed-list selection and the carried
/// height take the deck branch where gamemd takes ground, so a bridge approach
/// can be planned on the wrong plane. Frequency: every bridge approach on every
/// bridge map. Downstream risk: high — swapping the flag here moves every
/// bridge-adjacent expansion at once, so it needs its own slice with a fixture,
/// not a one-word edit. Native also has a `height == -1` arm this omits;
/// `movement_occupancy`'s runtime twin carries it.
fn is_at_bridge_level(path_height: u8, cell: &PathCell) -> bool {
    cell.bridge_walkable && path_height.abs_diff(cell.ground_level) >= BRIDGE_HEIGHT_THRESHOLD
}

/// Compute what height a new A* node carries forward when expanding into
/// `neighbor_cell` from a parent at `parent_height` in `parent_cell`.
/// Matches AStar_create_node (0x0042a460) 4-case decision tree.
fn compute_neighbor_height(
    parent_height: u8,
    parent_cell: &PathCell,
    neighbor_cell: &PathCell,
) -> u8 {
    // Case 1: Neighbor is not a bridge cell -> ground level
    if !neighbor_cell.bridge_walkable {
        return neighbor_cell.ground_level;
    }

    // Case 2: Parent is also a bridge cell
    if parent_cell.bridge_walkable {
        if parent_height == parent_cell.bridge_deck_level {
            // Parent was on bridge deck -> stay on bridge
            return neighbor_cell.bridge_deck_level;
        } else {
            // Parent was under bridge -> stay under
            return neighbor_cell.ground_level;
        }
    }

    // Case 3: Parent is NOT bridge, neighbor IS bridge.
    //
    // `AStar_create_node` @ `0x0042A460` tests
    // `abs((neighbor.Level - parent_height) + 3) <= 1`, i.e. a drop of **2, 3 or
    // 4**, and reads no bridgehead or transition flag anywhere in the function.
    // Requiring exactly 4 plus `transition` refused two of the three native
    // drops outright and refused the third on every deck cell that is not a
    // flagged bridgehead — in each case carrying `ground_level` instead, which
    // then feeds the closed-list split and `check_bridge_traversal`, so the
    // route planned *under* the bridge or failed.
    let drop = parent_height as i16 - neighbor_cell.ground_level as i16;
    if (2..=4).contains(&drop) {
        neighbor_cell.bridge_deck_level
    } else {
        neighbor_cell.ground_level
    }
}

fn is_structural_bridge_deck_height(path_height: u8, cell: &PathCell) -> bool {
    cell.has_structural_bridge() && path_height as i16 == cell.signed_level() + 4
}

/// Whether an edge needs the bridge-traversal legality check at all.
///
/// **VERA adapter, not a native skip predicate.** `AStar_main_loop` calls the
/// `+0x1AC` slot unconditionally at `0x00429F54`; Unit's receiver in turn calls
/// CheckBridgeTraversal (0x004D9C60). This includes structural deck-to-deck
/// edges: 0x004D9E08 can refuse those when the candidate lacks 0x200, even
/// though both cells carry 0x100 and their raw ground levels match.
///
/// The remaining bypass is between structural cells when the parent is not
/// at its raw level+4 and the candidate lacks 0x200. The caller then uses its
/// local height-difference rule. That reduced under-bridge domain is not a
/// claim of the unconditional native receiver contract; changes must also
/// account for the runtime crossing reader in `movement_occupancy`.
pub(crate) fn needs_bridge_traversal_for_edge(
    current_height: u8,
    current_cell: &PathCell,
    neighbor_cell: &PathCell,
) -> bool {
    let structural_deck_to_structural =
        is_structural_bridge_deck_height(current_height, current_cell)
            && neighbor_cell.has_structural_bridge();
    neighbor_cell.has_bridgehead_transition()
        || !neighbor_cell.has_structural_bridge()
        || !current_cell.has_structural_bridge()
        || structural_deck_to_structural
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BridgeTraversalInput<'a> {
    pub candidate: &'a PathCell,
    pub candidate_coord: (u16, u16),
    pub direction: i8,
    pub path_height: i16,
    pub parent: Option<(&'a PathCell, (u16, u16))>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BridgeTraversalResult {
    pub allowed: bool,
    pub path_height: i16,
    pub force_bridge_list: bool,
}

pub(crate) fn resolve_parent_for_bridge_traversal<'a>(
    grid: &'a PathGrid,
    candidate_coord: (u16, u16),
    direction: i8,
    explicit_parent: Option<(&'a PathCell, (u16, u16))>,
) -> Option<(&'a PathCell, (u16, u16))> {
    if let Some(parent) = explicit_parent {
        return Some(parent);
    }
    if !(0..=7).contains(&direction) {
        return None;
    }
    let rotated = ((direction - 4) & 7) as usize;
    let (dx, dy, _) = NEIGHBORS[rotated];
    let px = candidate_coord.0 as i32 + dx;
    let py = candidate_coord.1 as i32 + dy;
    if px < 0 || py < 0 || px >= grid.width() as i32 || py >= grid.height() as i32 {
        return None;
    }
    let coord = (px as u16, py as u16);
    grid.cell(coord.0, coord.1).map(|cell| (cell, coord))
}

pub(crate) fn check_bridge_traversal(
    grid: &PathGrid,
    input: BridgeTraversalInput<'_>,
) -> BridgeTraversalResult {
    let mut path_height = input.path_height;
    if input.direction == -1 {
        if path_height == -1 && input.candidate.has_structural_bridge() {
            path_height = input.candidate.signed_level() + 4;
        }
        return BridgeTraversalResult {
            allowed: true,
            path_height,
            force_bridge_list: false,
        };
    }

    let Some((parent, _parent_coord)) = resolve_parent_for_bridge_traversal(
        grid,
        input.candidate_coord,
        input.direction,
        input.parent,
    ) else {
        // `0x004D9CC5`: `TEST ESI,ESI; JZ 0x004D9E5E` — a null parent is
        // **allowed** (`XOR EAX,EAX`), not refused. Reached on the outer map
        // ring, where the predecessor cell resolves off-map.
        return BridgeTraversalResult {
            allowed: true,
            path_height,
            force_bridge_list: false,
        };
    };

    if path_height == -1 && parent.has_structural_bridge() {
        path_height = parent.signed_level() + 4;
        if !input.candidate.has_bridgehead_transition() {
            return BridgeTraversalResult {
                allowed: false,
                path_height,
                force_bridge_list: false,
            };
        }
    }

    let candidate_level = input.candidate.signed_level();
    let parent_selected = if parent.has_structural_bridge() {
        parent.signed_level()
    } else {
        path_height
    };
    let diff = parent_selected - candidate_level;
    let mut force_bridge_list = false;
    let allowed = match diff.abs() {
        0 => {
            let all_bridge_transition = input.candidate.has_structural_bridge()
                && input.candidate.has_bridgehead_transition()
                && parent.has_structural_bridge();
            all_bridge_transition || path_height == -1 || path_height == candidate_level
        }
        1 => {
            if diff < 1 {
                parent.slope_type != 0
            } else {
                input.candidate.slope_type != 0
            }
        }
        4 => {
            if parent.signed_level() == candidate_level - 4 {
                path_height == candidate_level && parent.has_structural_bridge()
            } else if candidate_level == parent.signed_level() - 4 {
                if input.candidate.has_structural_bridge()
                    && input.candidate.has_bridgehead_transition()
                {
                    force_bridge_list = true;
                    true
                } else {
                    false
                }
            } else {
                // `0x004D9D8F-0x004D9D94`: `ADD EAX,-0x4; CMP EBX,EAX; JNZ
                // 0x004D9E5E` — when the parent's own level is neither
                // `candidate ± 4`, the step is **allowed** with no deck flag.
                // Reached when the parent carries no structural bridge, so
                // `parent_selected` is the path height rather than the level.
                true
            }
        }
        _ => false,
    };

    BridgeTraversalResult {
        allowed,
        path_height,
        force_bridge_list,
    }
}

pub(crate) fn can_enter_layer_context(
    terrain_layer: MovementLayer,
    object_list_layer: MovementLayer,
    candidate: &PathCell,
    path_height: i16,
) -> CanEnterLayerContext {
    let occupancy_bits_layer = if path_height != -1
        && candidate.has_structural_bridge()
        && path_height == candidate.signed_level() + 4
    {
        MovementLayer::Bridge
    } else {
        MovementLayer::Ground
    };
    CanEnterLayerContext {
        terrain_layer,
        object_list_layer,
        occupancy_bits_layer,
    }
}

/// Read-only A* candidate row emitted by the bridge oracle diagnostics.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct BridgeOracleAStarStep {
    pub search_id: u64,
    pub expansion_index: u64,
    pub current_cell: (u16, u16),
    pub candidate_cell: (u16, u16),
    pub direction: u8,
    pub incoming_path_height: u8,
    pub current_layer: MovementLayer,
    pub initial_candidate_closed_list_layer: MovementLayer,
    pub computed_neighbor_height: u8,
    pub bridge_traversal_ran: bool,
    pub bridge_traversal_allowed: Option<bool>,
    pub bridge_traversal_path_height: Option<i16>,
    pub bridge_traversal_force_bridge_list: Option<bool>,
    pub final_candidate_layer: MovementLayer,
    pub terrain_layer: MovementLayer,
    pub object_list_layer: MovementLayer,
    pub occupancy_bits_layer: MovementLayer,
    pub walkable: Option<bool>,
    pub terrain_cost: Option<u8>,
    pub edge_cost: Option<i32>,
    pub carried_height: u8,
    pub rejected_reason: Option<&'static str>,
}

/// Sink for opt-in A* oracle rows. Normal pathfinding passes no sink.
pub trait AStarTraceSink {
    fn emit_astar_step(&self, step: BridgeOracleAStarStep);
}

/// In-memory trace collector for tests and one-shot diagnostics.
#[derive(Debug, Default)]
pub struct AStarTraceCollector {
    steps: RefCell<Vec<BridgeOracleAStarStep>>,
}

impl AStarTraceCollector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn steps(&self) -> Vec<BridgeOracleAStarStep> {
        self.steps.borrow().clone()
    }
}

impl AStarTraceSink for AStarTraceCollector {
    fn emit_astar_step(&self, step: BridgeOracleAStarStep) {
        self.steps.borrow_mut().push(step);
    }
}

fn encode_from(cell_idx: usize, on_bridge: bool) -> usize {
    cell_idx | if on_bridge { CAME_FROM_BRIDGE } else { 0 }
}

fn decode_from(value: usize) -> (usize, bool) {
    (value & !CAME_FROM_BRIDGE, value & CAME_FROM_BRIDGE != 0)
}

fn explicit_tube_edge(
    terrain: Option<&ResolvedTerrainGrid>,
    coord: (u16, u16),
) -> Option<((u16, u16), usize)> {
    let terrain = terrain?;
    let tube = terrain.tube_at_cell(coord.0, coord.1)?;
    if tube.source != TubeSource::ExplicitMap || tube.path_len() == 0 || tube.exit == (0, 0) {
        return None;
    }
    Some((tube.exit, tube.path_len()))
}

/// Configuration for the unified A* search. All fields optional; defaults
/// produce a bare ground-only search equivalent to the old `find_path`.
#[derive(Default)]
pub struct AStarOptions<'a> {
    /// Terrain speed multipliers (cost 0 = blocked for this SpeedType).
    pub terrain_costs: Option<&'a TerrainCostGrid>,
    /// Search-only bridge/coercion gate for the native Foot predicate result.
    ///
    /// This never receives a terrain speed percentage: those remain in
    /// `terrain_costs`. The default is off until a mover's native gate source is
    /// represented by runtime state.
    pub search_cost_class_coerce_to_zero: bool,
    /// Optional native cost-class producer for the Foot `+0x1ac` search call.
    /// The classifier is deliberately cell/search scoped and never receives a
    /// `TerrainCostGrid` speed percentage.
    pub search_cost_classifier: Option<&'a dyn SearchCellCostClassifier>,
    /// Hard-blocked cells on ground layer (stationary/enemy units). Goal exempt.
    pub entity_blocks: Option<&'a BTreeSet<(u16, u16)>>,
    /// Hard-blocked cells on bridge layer. Goal exempt.
    pub bridge_blocks: Option<&'a BTreeSet<(u16, u16)>>,
    /// Code-2 blocker map: friendly-moving blocker's current cell → that
    /// blocker's next cell (movement_target.path[next_index]). Used by the
    /// cost function for the 10-hop chain walk (matches gamemd.exe
    /// AStar_compute_edge_cost). The map is denormalized so no EntityStore
    /// lookup is required inside A*.
    pub entity_block_map: Option<&'a LayeredEntityBlockMap>,
    /// Search-scoped temporary marker overlay equivalent to
    /// `CellClass+0x140 & 0x40000`. Destination hits multiply normal compass
    /// edge cost, but do not change walkability or persistent pathgrid state.
    pub marker_overlay: Option<&'a SearchMarkerOverlay>,
    /// Crusher units bypass all entity soft-block costs (codes 1-6).
    /// Buildings (code 7, in entity_blocks BTreeSet) still block.
    pub mover_is_crusher: bool,
    /// Code-2 urgency escalation (0 = look-ahead chain walk, 1 = traffic penalty,
    /// 2 = route around). Matches gamemd.exe PathfinderClass+0x3C.
    pub urgency: u8,
    /// Zone corridor restriction — only expand cells in these zones.
    pub corridor: Option<(
        &'a super::zone_map::ZoneMap,
        &'a BTreeSet<super::zone_map::ZoneId>,
    )>,
    /// Binary-style hierarchy marker gate. Present only when blocker-neighbor
    /// counts are also available for the same search.
    pub(crate) hierarchy_gate: Option<HierarchyGate<'a>>,
    /// Optional progress-cell sink for the exact failed-hierarchy retry producer.
    pub(crate) hierarchy_progress: Option<&'a HierarchyProgressTracker<'a>>,
    /// Movement zone for water mover bypass and passability matrix.
    pub movement_zone: Option<MovementZone>,
    /// Resolved terrain for cliff cost and water passability checks.
    pub resolved_terrain: Option<&'a ResolvedTerrainGrid>,
    /// Infantry units always target ground level at bridge destinations.
    pub is_infantry: bool,
    /// Optional bridge-oracle A* row sink. Inert unless explicitly supplied.
    pub trace_sink: Option<&'a dyn AStarTraceSink>,
    /// Search id copied into trace rows so comparator matching never guesses.
    pub trace_search_id: u64,
    /// Optional route/window filter for trace rows.
    pub trace_window: Option<&'a BTreeSet<(u16, u16)>>,
}

/// Caller-owned adapter for the native Foot search cost-class virtual.
pub trait SearchCellCostClassifier {
    fn classify(&self, from: (u16, u16), candidate: (u16, u16), bridge: bool) -> u8;
}

/// The mover facts the A* cost evaluation consults for every neighbour.
///
/// These four travel together through every search entry, so they travel as
/// one value. `urgency` drives the code-2 escalation, `mover_is_crusher` and
/// `is_infantry` select the `Can_Enter_Cell` class arm, and `wall_cost` is the
/// optional producer for the Foot `+0x1AC` cost class (ledger I9b).
///
/// This deliberately excludes `allow_zone_hierarchy` and `playfield_bounds`
/// (hierarchy admission, not per-neighbour cost) and both movement-zone
/// parameters: `mz` selects the zone map/hierarchy to consult, while
/// `movement_zone` is the mover's own zone for passability, and
/// `movement_zone.unwrap_or(mz)` is the documented fallback between them.
/// Deliberately **not** `Default`. A defaulted value would mean
/// `mover_is_crusher: false`, and ledger row I9c records exactly that shape as a
/// landed regression: five first searches passed a literal `false` and crushers
/// detoured around sandbags their own crossing would drive through. Every
/// construction site names every field, so the facts always come from a mover.
#[derive(Clone, Copy)]
pub struct MoverSearchFacts<'a> {
    pub urgency: u8,
    pub mover_is_crusher: bool,
    pub is_infantry: bool,
    pub wall_cost: Option<&'a dyn SearchCellCostClassifier>,
}

fn emit_astar_trace(options: &AStarOptions<'_>, step: BridgeOracleAStarStep) {
    let Some(sink) = options.trace_sink else {
        return;
    };
    if let Some(window) = options.trace_window {
        if !window.contains(&step.current_cell) && !window.contains(&step.candidate_cell) {
            return;
        }
    }
    sink.emit_astar_step(step);
}

/// Reconstruct a layered path from dual came_from arrays.
/// Walks backward from goal using `decode_from` to follow the parent chain
/// across ground/bridge transitions.
fn reconstruct_path_dual(
    ground_from: &[usize],
    bridge_from: &[usize],
    start_idx: usize,
    start_on_bridge: bool,
    goal_idx: usize,
    goal_on_bridge: bool,
    width: usize,
) -> Vec<LayeredPathStep> {
    let mut path = Vec::new();
    let mut current_idx = goal_idx;
    let mut current_bridge = goal_on_bridge;

    loop {
        let x = (current_idx % width) as u16;
        let y = (current_idx / width) as u16;
        let layer = if current_bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        path.push(LayeredPathStep {
            rx: x,
            ry: y,
            layer,
        });

        if current_idx == start_idx && current_bridge == start_on_bridge {
            break;
        }

        let from_array = if current_bridge {
            bridge_from
        } else {
            ground_from
        };
        let encoded = from_array[current_idx];
        debug_assert_ne!(
            encoded,
            usize::MAX,
            "reconstruct_path_dual: hit unvisited cell at idx={} bridge={}",
            current_idx,
            current_bridge
        );
        let (parent_idx, parent_bridge) = decode_from(encoded);
        current_idx = parent_idx;
        current_bridge = parent_bridge;
    }

    path.reverse();
    path
}

/// Unified A* search with height-based bridge routing.
///
/// Matches gamemd.exe's single AStar_main_loop (0x00429a90). Uses dual closed
/// lists (ground/bridge) per cell, with closed-list selection based on the
/// CURRENT node's height vs neighbor's ground_level (not computed neighbor height).
///
/// Always returns `Vec<LayeredPathStep>` with per-cell layer info derived from
/// height comparison. Thin public wrappers extract `(u16, u16)` for callers that
/// don't need layer info.
///
/// Accepts blocked start cells: a unit standing in an impassable cell (e.g.
/// inside a building footprint) can still pathfind out via any walkable
/// neighbor. Returns `None` only when the goal is unreachable from any
/// neighbor of the start.
pub fn astar_search(
    grid: &PathGrid,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    options: &AStarOptions<'_>,
) -> Option<Vec<LayeredPathStep>> {
    let is_water_mover = options.movement_zone.is_some_and(|mz| mz.is_water_mover());

    // Start cell may be blocked (e.g. unit standing inside a building footprint
    // after undock). The start node is seeded into the open set without a
    // passability check; only neighbor expansion calls Can_Enter_Cell. If all
    // 8 neighbors are also blocked, the open set exhausts and we return None
    // naturally — no need for an explicit start-cell rejection.

    // --- Goal passability ---
    // An impassable destination does NOT abort the search. gamemd runs the
    // search anyway; when a cell adjacent to the blocked goal is reached it
    // drops into the success tail and returns the path to that adjacent cell
    // ("walk as close to the blocked target as you can"). The near-miss branch
    // in the neighbour loop below is that tail. Only the layer selection below
    // consumes these two flags.
    let goal_ground_ok = is_cell_passable_for_category_on_layer(
        grid,
        goal.0,
        goal.1,
        MovementLayer::Ground,
        options.movement_zone,
        None,
        options.resolved_terrain,
        options.terrain_costs,
        false,
        TerrainEntryMode::AStarNeighbor,
        options.is_infantry,
        options.mover_is_crusher,
    );
    let goal_bridge_ok = is_cell_passable_for_mover_on_layer_with_speed(
        grid,
        goal.0,
        goal.1,
        MovementLayer::Bridge,
        options.movement_zone,
        None,
        options.resolved_terrain,
        options.terrain_costs,
        false,
        TerrainEntryMode::AStarNeighbor,
    );
    let _ = goal_ground_ok;

    // --- Height initialization ---
    let start_cell = grid.cell(start.0, start.1).unwrap_or(&DEFAULT_BLOCKED_CELL);
    let start_height = match start_layer {
        MovementLayer::Bridge => start_cell.bridge_deck_level,
        _ => start_cell.ground_level,
    };

    let goal_cell = grid.cell(goal.0, goal.1).unwrap_or(&DEFAULT_BLOCKED_CELL);
    // Bridge-deck destinations resolve to the deck height for every mover.
    //
    // This deliberately does NOT except infantry. A former `!is_infantry` guard
    // here ("infantry always target ground level at bridge destinations") was
    // inert for as long as the flag was never set by any production caller, and
    // it carries no source: stock YR infantry walk over high bridges on every
    // bridge map, so forcing an infantry move order onto a deck cell to aim at
    // the ground beneath it would break ordinary bridge crossings. The flag now
    // reaches this function, so the guard is removed rather than silently
    // switched on.
    let goal_height = if goal_bridge_ok {
        goal_cell.bridge_deck_level
    } else {
        goal_cell.ground_level
    };

    // Trivial: already at goal with matching height
    if start == goal && start_height == goal_height {
        let layer = if is_at_bridge_level(start_height, start_cell) {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        return Some(vec![LayeredPathStep {
            rx: start.0,
            ry: start.1,
            layer,
        }]);
    }

    // --- Arrays ---
    let w = grid.width() as usize;
    let h = grid.height() as usize;
    let total_cells = w * h;

    // Persistent A* scratch buffers reused across calls. Destructured into
    // local mutable references so the rest of the function reads/writes
    // `ground_g[..]`, `open.push(..)`, etc. unchanged.
    with_pathfind_workspace(total_cells, |ws| {
        // Split-borrow the workspace fields. The g_cost and came_from buffers
        // get re-borrowed inline in the body's `(&mut bridge_g, &mut bridge_from)`
        // tuple, which needs the bindings to be `mut`-declared.
        let PathfindWorkspace {
            ref mut ground_g,
            ref mut bridge_g,
            ref mut ground_from,
            ref mut bridge_from,
            ref mut ground_closed,
            ref mut bridge_closed,
            ref mut open,
        } = *ws;
        let mut ground_g = &mut *ground_g;
        let mut bridge_g = &mut *bridge_g;
        let mut ground_from = &mut *ground_from;
        let mut bridge_from = &mut *bridge_from;

        let start_idx = start.1 as usize * w + start.0 as usize;
        let start_on_bridge = is_at_bridge_level(start_height, start_cell);
        // Close-on-generation: gamemd stamps a (cell, layer) closed at the
        // moment a node is created for it and freezes its g there. The start
        // node is created before the loop, so it is stamped here.
        if start_on_bridge {
            bridge_g[start_idx] = 0;
            bridge_closed[start_idx] = true;
        } else {
            ground_g[start_idx] = 0;
            ground_closed[start_idx] = true;
        }

        open.push(Reverse(AStarNode {
            f_cost: euclidean_heuristic(start.0, start.1, goal.0, goal.1),
            g_cost: 0,
            x: start.0,
            y: start.1,
            height: start_height,
            on_bridge: start_on_bridge,
        }));

        let mut nodes_evaluated: u32 = 0;
        let mut expansion_index: u64 = 0;

        // --- Main loop ---
        while let Some(Reverse(current)) = open.pop() {
            let cx = current.x;
            let cy = current.y;
            let c_idx = cy as usize * w + cx as usize;
            let cur_cell = grid.cell(cx, cy).unwrap_or(&DEFAULT_BLOCKED_CELL);
            // Use the push-time layer flag carried on the node, matching gamemd.exe:
            // the layer is decided once at push-time from predecessor.height vs this
            // cell, never re-derived from the node's own height at pop. Re-derivation
            // diverged at bridgehead transition cells when compute_neighbor_height
            // collapsed to ground_level but the predecessor was still high enough to
            // select the bridge arrays — causing reconstruct_path_dual to read
            // usize::MAX from the wrong came_from array.
            let on_bridge = current.on_bridge;

            // No close-at-pop and no stale-duplicate skip: every (cell, layer)
            // is stamped closed when its node is created, so the open set can
            // never hold two nodes for the same (cell, layer).

            // Goal check: cell AND height must match
            if (cx, cy) == goal && current.height == goal_height {
                // Use the node's push-time layer flag (same value came_from was keyed on).
                return Some(reconstruct_path_dual(
                    &ground_from,
                    &bridge_from,
                    start_idx,
                    start_on_bridge,
                    c_idx,
                    on_bridge,
                    w,
                ));
            }

            nodes_evaluated += 1;
            if nodes_evaluated >= MAX_SEARCH_NODES {
                log::warn!(
                    "A* search exhausted {} nodes without finding path from ({},{}) to ({},{})",
                    MAX_SEARCH_NODES,
                    start.0,
                    start.1,
                    goal.0,
                    goal.1,
                );
                return None;
            }

            // --- Neighbor expansion ---
            for (dir_index, &(dx, dy, _is_diagonal)) in NEIGHBORS.iter().enumerate() {
                let nx_i = cx as i32 + dx;
                let ny_i = cy as i32 + dy;
                if nx_i < 0
                    || ny_i < 0
                    || nx_i >= grid.width() as i32
                    || ny_i >= grid.height() as i32
                {
                    continue;
                }
                let nx = nx_i as u16;
                let ny = ny_i as u16;
                let this_expansion_index = expansion_index;
                expansion_index = expansion_index.saturating_add(1);
                let n_idx = ny as usize * w + nx as usize;
                let neighbor_cell = grid.cell(nx, ny).unwrap_or(&DEFAULT_BLOCKED_CELL);

                // Closed-list selection: uses CURRENT node's height vs neighbor ground_level
                let mut neighbor_use_bridge = is_at_bridge_level(current.height, neighbor_cell);
                let mut layer_context = CanEnterLayerContext::single(if neighbor_use_bridge {
                    MovementLayer::Bridge
                } else {
                    MovementLayer::Ground
                });
                let initial_candidate_layer = layer_context.terrain_layer;

                // Compute what height the NEW node carries forward (separate computation)
                let neighbor_height =
                    compute_neighbor_height(current.height, cur_cell, neighbor_cell);
                let mut trace_step = BridgeOracleAStarStep {
                    search_id: options.trace_search_id,
                    expansion_index: this_expansion_index,
                    current_cell: (cx, cy),
                    candidate_cell: (nx, ny),
                    direction: dir_index as u8,
                    incoming_path_height: current.height,
                    current_layer: if on_bridge {
                        MovementLayer::Bridge
                    } else {
                        MovementLayer::Ground
                    },
                    initial_candidate_closed_list_layer: initial_candidate_layer,
                    computed_neighbor_height: neighbor_height,
                    bridge_traversal_ran: false,
                    bridge_traversal_allowed: None,
                    bridge_traversal_path_height: None,
                    bridge_traversal_force_bridge_list: None,
                    final_candidate_layer: initial_candidate_layer,
                    terrain_layer: layer_context.terrain_layer,
                    object_list_layer: layer_context.object_list_layer,
                    occupancy_bits_layer: layer_context.occupancy_bits_layer,
                    walkable: None,
                    terrain_cost: None,
                    edge_cost: None,
                    carried_height: neighbor_height,
                    rejected_reason: None,
                };

                // Height-diff legality gate. Diff-1 transitions require the LOWER cell's
                // raw slope byte to be nonzero; diff ∈ {±2, ±3, ±4, ±5+} is
                // always blocked. Legitimate bridge transitions arrive here as diff-0
                // because `compute_neighbor_height` already shifts unit Z onto/off the deck.
                let needs_bridge_traversal =
                    needs_bridge_traversal_for_edge(current.height, cur_cell, neighbor_cell);
                if needs_bridge_traversal {
                    let bridge_traversal = check_bridge_traversal(
                        grid,
                        BridgeTraversalInput {
                            candidate: neighbor_cell,
                            candidate_coord: (nx, ny),
                            direction: dir_index as i8,
                            path_height: i16::from(current.height as i8),
                            parent: Some((cur_cell, (cx, cy))),
                        },
                    );
                    trace_step.bridge_traversal_ran = true;
                    trace_step.bridge_traversal_allowed = Some(bridge_traversal.allowed);
                    trace_step.bridge_traversal_path_height = Some(bridge_traversal.path_height);
                    trace_step.bridge_traversal_force_bridge_list =
                        Some(bridge_traversal.force_bridge_list);
                    if !bridge_traversal.allowed {
                        trace_step.rejected_reason = Some("bridge_traversal_blocked");
                        emit_astar_trace(options, trace_step);
                        continue;
                    }
                    if bridge_traversal.force_bridge_list {
                        neighbor_use_bridge = true;
                    }
                    layer_context = can_enter_layer_context(
                        if neighbor_use_bridge {
                            MovementLayer::Bridge
                        } else {
                            MovementLayer::Ground
                        },
                        if bridge_traversal.force_bridge_list {
                            MovementLayer::Bridge
                        } else {
                            layer_context.object_list_layer
                        },
                        neighbor_cell,
                        bridge_traversal.path_height,
                    );
                    trace_step.terrain_layer = layer_context.terrain_layer;
                    trace_step.object_list_layer = layer_context.object_list_layer;
                    trace_step.occupancy_bits_layer = layer_context.occupancy_bits_layer;
                } else {
                    let layer = if neighbor_use_bridge {
                        MovementLayer::Bridge
                    } else {
                        MovementLayer::Ground
                    };
                    layer_context = CanEnterLayerContext::single(layer);
                    let diff = i16::from(neighbor_height as i8) - i16::from(current.height as i8);
                    let lower_slope = if diff < 0 {
                        neighbor_cell.slope_type
                    } else {
                        cur_cell.slope_type
                    };
                    let legal = match diff.abs() {
                        0 => true,
                        1 => lower_slope != 0,
                        _ => false,
                    };
                    if !legal {
                        trace_step.rejected_reason = Some("height_diff_illegal");
                        emit_astar_trace(options, trace_step);
                        continue;
                    }
                }
                trace_step.final_candidate_layer = if neighbor_use_bridge {
                    MovementLayer::Bridge
                } else {
                    MovementLayer::Ground
                };

                // Closed check on appropriate list
                // Binary `1.009` handling is an early closed-neighbor skip/fallback
                // nuance, not true A* reopen. Do not reinsert selected-layer closed
                // cells without a dedicated parity fixture for the blocked-goal path.
                if neighbor_use_bridge {
                    if bridge_closed[n_idx] {
                        trace_step.rejected_reason = Some("bridge_closed");
                        emit_astar_trace(options, trace_step);
                        continue;
                    }
                } else if ground_closed[n_idx] {
                    trace_step.rejected_reason = Some("ground_closed");
                    emit_astar_trace(options, trace_step);
                    continue;
                }

                // Walkability check on the determined layer. Ground->Bridge entry
                // still requires 0x200. Deck-to-deck moves passed the same
                // flag's diff-0 gate in CheckBridgeTraversal; bridge_walkable
                // alone does not admit a Forward2-style transverse stamp slot.
                let neighbor_passable = if neighbor_use_bridge {
                    let prev_on_bridge = is_at_bridge_level(current.height, cur_cell);
                    let bridge_terrain_passable = is_cell_passable_for_mover_on_layer_with_speed(
                        grid,
                        nx,
                        ny,
                        MovementLayer::Bridge,
                        options.movement_zone,
                        None,
                        options.resolved_terrain,
                        options.terrain_costs,
                        false,
                        TerrainEntryMode::AStarNeighbor,
                    );
                    if prev_on_bridge {
                        bridge_terrain_passable
                    } else {
                        bridge_terrain_passable && neighbor_cell.transition
                    }
                } else {
                    is_cell_passable_for_category_on_layer(
                        grid,
                        nx,
                        ny,
                        MovementLayer::Ground,
                        options.movement_zone,
                        None,
                        options.resolved_terrain,
                        options.terrain_costs,
                        false,
                        TerrainEntryMode::AStarNeighbor,
                        options.is_infantry,
                        options.mover_is_crusher,
                    )
                };
                trace_step.walkable = Some(neighbor_passable);
                if !neighbor_passable {
                    // Impassable-destination abort. When the goal cell itself is
                    // impassable and the search has reached a cell adjacent to
                    // it, gamemd leaves the main loop immediately and drops into
                    // the success tail — "walk as close to the blocked target as
                    // you can". The search does NOT continue past this point.
                    // The tail additionally requires the aborting node to be at
                    // least one real step from the start, so a start-adjacent
                    // blocked goal fails the search outright.
                    if (nx, ny) == goal && start_height.abs_diff(goal_height) <= 1 {
                        if c_idx == start_idx && on_bridge == start_on_bridge {
                            return None;
                        }
                        // Use the current node's push-time layer flag (same value
                        // came_from was keyed on when the node was pushed).
                        return Some(reconstruct_path_dual(
                            &ground_from,
                            &bridge_from,
                            start_idx,
                            start_on_bridge,
                            c_idx,
                            on_bridge,
                            w,
                        ));
                    }
                    trace_step.rejected_reason = Some("walkability_blocked");
                    emit_astar_trace(options, trace_step);
                    continue;
                }

                // Entity blocks (layer-separated). Goal exempt.
                if (nx, ny) != goal {
                    let blocks_for_layer = |layer| match layer {
                        MovementLayer::Bridge => options.bridge_blocks,
                        MovementLayer::Ground => options.entity_blocks,
                        MovementLayer::Air | MovementLayer::Underground => None,
                    };
                    let blocked_by_selected_layers = [
                        blocks_for_layer(layer_context.object_list_layer),
                        blocks_for_layer(layer_context.occupancy_bits_layer),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|blocks| blocks.contains(&(nx, ny)));
                    if blocked_by_selected_layers {
                        trace_step.rejected_reason = Some("entity_blocked");
                        emit_astar_trace(options, trace_step);
                        continue;
                    }
                }

                // Zone_precheck marker gate for normal compass edges. Direction-8
                // tube jumps are handled below; callers that enable this gate must
                // defer explicit tube scenarios until their hierarchy semantics are verified.
                //
                // A bridge deck is exempt from this gate.
                //
                // VERIFIED in `AStar_main_loop` 0x00429A90, read at the disassembly
                // 2026-08-27. `0x00429E54-0x00429E78` builds a stack flag at
                // `[ESP+0x60]`: `TEST AH,0x1` on `Cell+0x140` at 0x00429E5A (the
                // 0x100 structural-bridge bit), then `CMP EAX,0x1` at 0x00429E75 on
                // `abs(path_height - Cell+0x11B)`. The flag is 0 for, and only for, a
                // structural bridge cell more than one level from the carried path
                // height — the same predicate as `is_at_bridge_level` above. At
                // 0x00429EA4 a per-zone word is compared against `[ESI+0x28]`; on
                // equal it proceeds, and on NOT equal the flag is tested at
                // 0x00429EAD and a deck takes `JZ 0x00429F04` at 0x00429EAF, skipping
                // the `Cell+0x122` test at 0x00429EB1 and the `[ESP+0x74]` test at
                // 0x00429EBB that an ordinary cell must still pass. So a deck that
                // fails the zone comparison is genuinely admitted where a ground cell
                // is not.
                //
                // **UNCHECKED, and load-bearing for calling this parity:** that the
                // native zone comparison at 0x00429EA4 is the same gate as
                // `HierarchyGate` here. Ours additionally consults
                // `blocker_neighbor_counts`, for which no native counterpart has been
                // identified. Note also that 0x00429F04 is the *bridge* closed-list
                // and cost-array branch (`[ESI+0x1c]` / `[ESI+0x20]`, against the
                // ground pair `[ESI+0x18]` / `[ESI+0x24]` at 0x00429ECF) — dual-list
                // selection this crate already models — not a dedicated
                // skip-the-gate landing. The exemption below is therefore justified
                // by the observed native branch shape and by production behaviour (an
                // ordinary move order across a retail span is refused without it),
                // not by a proven one-to-one mapping of the two gates.
                //
                // Without this, `HierarchyGate::allows` resolves a deck cell through
                // a ground-plane `zone_at` with no layer term, so every deck cell
                // answers the level-0 zone of the water underneath. That zone is
                // never on the coarse route, so every deck neighbour is rejected and
                // the search exhausts: an ordinary move order across any high bridge
                // is refused and the unit never leaves its start cell.
                //
                // Keyed on `has_structural_bridge()` — the native 0x100 bit — and not
                // on the `bridge_walkable` form used by `is_at_bridge_level` above,
                // because the producer marks ramps and bridgeheads walkable without
                // marking them structural; the walkable form would exempt every ramp
                // approach as well and widen this well past what 0x00429EAF skips.
                let neighbor_is_bridge_deck = neighbor_cell.has_structural_bridge()
                    && current.height.abs_diff(neighbor_cell.ground_level)
                        >= BRIDGE_HEIGHT_THRESHOLD;
                if let Some(gate) = options.hierarchy_gate
                    && !neighbor_is_bridge_deck
                {
                    if !gate.allows(nx, ny) {
                        trace_step.rejected_reason = Some("hierarchy_gate_blocked");
                        emit_astar_trace(options, trace_step);
                        continue;
                    }
                }

                // Zone corridor filter
                if let Some((zone_map, allowed)) = options.corridor {
                    let cell_zone = zone_map.zone_at(nx, ny, MovementLayer::Ground);
                    if cell_zone != ZONE_INVALID && !allowed.contains(&cell_zone) {
                        trace_step.rejected_reason = Some("zone_corridor_blocked");
                        emit_astar_trace(options, trace_step);
                        continue;
                    }
                }

                // The land-type × SpeedType row, read as a **passability
                // predicate only**: zero closes the cell, any non-zero value
                // opens it and weighs exactly the same as any other.
                //
                // Retail provenance: land-type speed table — the twelve rows at
                // `0x0089EA40` filled by `RulesClass::ReadSpeedTypeLandTypeTable`
                // @ `0x00674000`. All twelve read sites, exhaustively:
                // passability gates (`CellClass::CheckCellPassability` @
                // `0x004835DE`, `InfantryClass::Can_Enter_Cell` @ `0x0051C750`
                // and `0x0051C7BC`, `UnitClass::Can_Enter_Cell` @ `0x0073FAB5`,
                // building placement @ `0x0047CA58`, and the Foot-column
                // `FCOMP 0.0f` at `0x0051895A` in the unnamed function around
                // `0x00518930`); runtime speed consumers
                // (`DriveLocomotionClass::Process_Movement` @ `0x004B3CA3`,
                // `ShipLocomotionClass::Process_Movement` @ `0x006A32F2`);
                // AI/action queries (`UnitClass::What_Action_OnObject` @
                // `0x007400A1`, `UnitClass::Mission_Hunt` @ `0x00740C9C`); and
                // the RulesClass save/load pair that streams the block whole
                // (`0x0067F882`, `0x0067FAF6`). **No path-search function is
                // among them.**
                //
                // The search's own edge cost is built by `AStar_compute_edge_cost`
                // @ `0x00429830` for `AStar_main_loop` @ `0x00429A90` out of the
                // `FootClass` `+0x1AC` cost class (base table `0x0081870C`), the
                // code-2 blocker override, a `CellClass+0x140 & 0x40000` term,
                // the dormant `PathfinderClass+0x01` bridge-flank term and the
                // direction tiebreaks at `0x0081872C`. Both `Can_Enter_Cell`
                // implementations reduce the land row to `== 0.0 → class 7`, so
                // no gradation reaches the cost class either.
                let terrain_cost: u8 = if neighbor_use_bridge {
                    100 // bridge layer: no terrain cost modifiers
                } else if is_water_mover {
                    100
                } else if let Some(cost_grid) = options.terrain_costs {
                    cost_grid.cost_at(nx, ny)
                } else {
                    100 // no cost grid: uniform cost
                };
                trace_step.terrain_cost = Some(terrain_cost);
                if terrain_cost == 0 {
                    trace_step.rejected_reason = Some("terrain_cost_blocked");
                    emit_astar_trace(options, trace_step);
                    continue;
                }

                // Original: `AStar_main_loop` @ `0x00429A90` calls the FootClass
                // `+0x1AC` slot (`Can_Enter_Cell`). There is no `FindPathRegular`
                // symbol in this program.
                //
                // The classifier is consulted **only on a refusal**, and that is
                // deliberate. Native computes one code per neighbour inside
                // `Can_Enter_Cell`; VERA reaches the same answer in two steps,
                // because `neighbor_passable` above carries terms the cell-scoped
                // classifier cannot see — the ground/bridge layer split, and the
                // `neighbor_cell.transition` (`0x200`) gate a ground->bridge entry
                // must still pass. Letting the classifier *replace* that verdict
                // would silently drop those terms on every search that supplies
                // one. Asking it only "is this refusal a wall this mover may
                // shoot?" keeps every pre-I9b routing decision byte-identical and
                // still produces the native wall classes.
                //
                // Producers: `cell_entry::WallSearchCostClassifier` answers 4 for
                // an allied wall and 5 for any other (`0x0073F4EB` / `0x0073F50E`),
                // which `apply_search_cost_class_multiplier` prices at 60x and 20x
                // from `0x0081870C` — so a wall line is routed *through* at cost
                // rather than reported unreachable, and the crossing's Override
                // arm attacks it. Class 3 (the gate arm) still has no producer.
                // VERA's other cost-class source is the `entity_block_map` below,
                // whose 2/5/6 codes reproduce the `0x0081870C` entries
                // 1.0/20.0/8.0 and the code-2 prediction override.
                let raw_cost_class = if neighbor_passable {
                    0
                } else {
                    options.search_cost_classifier.map_or(7, |classifier| {
                        classifier.classify((cx, cy), (nx, ny), neighbor_use_bridge)
                    })
                };
                let search_cost = search_cell_cost_decision(
                    raw_cost_class,
                    options.search_cost_class_coerce_to_zero,
                );
                if !search_cost.expands {
                    trace_step.rejected_reason = Some("search_cost_class_blocked");
                    emit_astar_trace(options, trace_step);
                    continue;
                }

                // Step cost — uniform across all 8 compass directions.
                // AStar_main_loop checks only the destination through
                // Can_Enter_Cell. PathfinderClass+0x01 is constructor-zero and
                // has no retail writer, so the dormant bridge-flank block does
                // not inspect either cardinal beside a diagonal destination.
                let mut step_cost = STEP_COST;
                step_cost = apply_search_cost_class_multiplier(
                    step_cost,
                    search_cost.effective_cost_class.unwrap_or(0),
                );

                // No height term. `AStar_compute_edge_cost` @ `0x00429830`
                // multiplies by exactly four things — the `0x0081870C` class
                // base, the code-2 blocker-prediction override, `4.0` when the
                // destination carries `CellClass+0x140 & 0x40000` (the
                // search-scoped bridge marker, applied below), and the dormant
                // `PathfinderClass+0x01` flank term — and `AStar_main_loop` @
                // `0x00429A90` adds only the direction tiebreak and one `FMUL`
                // by `PathfinderClass+0x04`. Its seven reads of the cell level
                // byte `+0x11B` all feed the bridge/layer selection, never a
                // cost. A ×4 on height change would have made every ramp step
                // cost as much as four flat ones.

                // Entity soft-block cost (codes 2/5/6). Goal exempt. Crusher exempt.
                if (nx, ny) != goal && !options.mover_is_crusher {
                    if let Some(map) = options.entity_block_map {
                        if let Some(entry) = map.get(layer_context.object_list_layer, &(nx, ny)) {
                            let mult = match entry.cost_code {
                                2 => compute_code2_multiplier(
                                    options.urgency,
                                    (nx, ny),
                                    layer_context.object_list_layer,
                                    map,
                                ),
                                5 => CODE5_MULT_ENEMY,
                                6 => CODE6_MULT_STATIONARY_ALLY,
                                _ => 1,
                            };
                            step_cost *= mult;
                        }
                    }
                }

                step_cost = apply_search_marker_cost(step_cost, options.marker_overlay, (nx, ny));
                trace_step.edge_cost = Some(step_cost);

                // Direction tie-breaker
                let tentative_g = current.g_cost + step_cost + DIR_TIEBREAK[dir_index];

                // Node creation. gamemd stamps the selected layer's closed
                // array and records g at this point and never revisits the
                // cell, so g is write-once and there is no relaxation: the
                // route that discovers a cell first owns it. The closed test
                // for this (cell, layer) already ran above.
                let (g_array, from_array) = if neighbor_use_bridge {
                    (&mut bridge_g, &mut bridge_from)
                } else {
                    (&mut ground_g, &mut ground_from)
                };
                g_array[n_idx] = tentative_g;
                from_array[n_idx] = encode_from(c_idx, on_bridge);
                if neighbor_use_bridge {
                    bridge_closed[n_idx] = true;
                } else {
                    ground_closed[n_idx] = true;
                }
                let h = euclidean_heuristic(nx, ny, goal.0, goal.1);
                open.push(Reverse(AStarNode {
                    f_cost: tentative_g + h,
                    g_cost: tentative_g,
                    x: nx,
                    y: ny,
                    height: neighbor_height,
                    on_bridge: neighbor_use_bridge,
                }));
                if let (Some(gate), Some(progress)) =
                    (options.hierarchy_gate, options.hierarchy_progress)
                {
                    progress.maybe_advance(gate.level0_zones.zone_at(nx, ny), (nx, ny));
                }
                emit_astar_trace(options, trace_step);
            }

            // Direction 8 is a TubeClass jump. It is not an adjacent neighbor and
            // must not use the normal terrain/height/corner-cut predicates. Auto
            // low-bridge shells have path_len=0 and remain predicate-only.
            if !on_bridge {
                if let Some(((nx, ny), path_len)) =
                    explicit_tube_edge(options.resolved_terrain, (cx, cy))
                {
                    if nx < grid.width() && ny < grid.height() {
                        let n_idx = ny as usize * w + nx as usize;
                        if !ground_closed[n_idx] {
                            if let Some((zone_map, allowed)) = options.corridor {
                                let cell_zone = zone_map.zone_at(nx, ny, MovementLayer::Ground);
                                if cell_zone != ZONE_INVALID && !allowed.contains(&cell_zone) {
                                    continue;
                                }
                            }

                            let neighbor_cell = grid.cell(nx, ny).unwrap_or(&DEFAULT_BLOCKED_CELL);
                            let neighbor_height = neighbor_cell.ground_level;
                            let tube_steps = i32::try_from(path_len).unwrap_or(1).max(1);
                            let tentative_g =
                                current.g_cost + STEP_COST * tube_steps + TUBE_DIR_TIEBREAK;

                            // Same close-on-generation rule as the compass edges.
                            ground_g[n_idx] = tentative_g;
                            ground_from[n_idx] = encode_from(c_idx, on_bridge);
                            ground_closed[n_idx] = true;
                            let h = euclidean_heuristic(nx, ny, goal.0, goal.1);
                            open.push(Reverse(AStarNode {
                                f_cost: tentative_g + h,
                                g_cost: tentative_g,
                                x: nx,
                                y: ny,
                                height: neighbor_height,
                                on_bridge: false,
                            }));
                        }
                    }
                }
            }
        }

        None
    })
}

fn apply_search_cost_class_multiplier(step_cost: i32, cost_class: u8) -> i32 {
    // Original: `AStar_compute_edge_cost` @ `0x00429830` indexes the class base
    // table at `0x0081870C` (read site `0x00429848`, its only reader). There is
    // no `NeighborStepCost` symbol in this program.
    let multiplier = match cost_class {
        0 | 2 | 3 => 1,
        1 => 1000,
        4 => 60,
        5 => 20,
        6 => 8,
        _ => 1,
    };
    step_cost.saturating_mul(multiplier)
}

/// Check if a cell is passable for pathfinding purposes.
///
/// For water movers (`MovementZone::Water` / `WaterBeach`), the normal PathGrid
/// marks water cells as non-walkable. Ships need to bypass PathGrid entirely and
/// use the passability matrix instead (zone 10 = water only).
///
/// For all other movers (or when `movement_zone` is `None`), uses the shared
/// terrain-entry evaluator above `PathGrid`.
pub fn is_cell_passable_for_mover(
    grid: &PathGrid,
    x: u16,
    y: u16,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
) -> bool {
    is_cell_passable_for_mover_with_speed(
        grid,
        x,
        y,
        movement_zone,
        None,
        resolved_terrain,
        None,
        false,
        TerrainEntryMode::AStarNeighbor,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn is_cell_passable_for_mover_with_speed(
    grid: &PathGrid,
    x: u16,
    y: u16,
    movement_zone: Option<MovementZone>,
    speed_type: Option<SpeedType>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    terrain_costs: Option<&TerrainCostGrid>,
    bypass_grid: bool,
    mode: TerrainEntryMode,
) -> bool {
    is_cell_passable_for_mover_on_layer_with_speed(
        grid,
        x,
        y,
        MovementLayer::Ground,
        movement_zone,
        speed_type,
        resolved_terrain,
        terrain_costs,
        bypass_grid,
        mode,
    )
}

/// Infantry-aware variant of `is_cell_passable_for_mover_on_layer_with_speed`.
///
/// `is_infantry` selects the sub-cell view of terrain-object occupation: retail
/// terrain objects close a cell to infantry only when every functional sub-cell
/// bit is set.
#[allow(clippy::too_many_arguments)]
pub fn is_cell_passable_for_category_on_layer(
    grid: &PathGrid,
    x: u16,
    y: u16,
    terrain_layer: MovementLayer,
    movement_zone: Option<MovementZone>,
    speed_type: Option<SpeedType>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    terrain_costs: Option<&TerrainCostGrid>,
    bypass_grid: bool,
    mode: TerrainEntryMode,
    is_infantry: bool,
    mover_is_crusher: bool,
) -> bool {
    evaluate_can_enter_cell(CanEnterCellContext {
        wall: None,
        target: (x, y),
        terrain_layer,
        movement_zone,
        speed_type,
        path_grid: Some(grid),
        resolved_terrain,
        terrain_costs,
        bypass_grid,
        mode,
        is_infantry,
        mover_is_crusher,
    })
    .is_clear()
}

#[allow(clippy::too_many_arguments)]
pub fn is_cell_passable_for_mover_on_layer_with_speed(
    grid: &PathGrid,
    x: u16,
    y: u16,
    terrain_layer: MovementLayer,
    movement_zone: Option<MovementZone>,
    speed_type: Option<SpeedType>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    terrain_costs: Option<&TerrainCostGrid>,
    bypass_grid: bool,
    mode: TerrainEntryMode,
) -> bool {
    evaluate_can_enter_cell(CanEnterCellContext {
        wall: None,
        target: (x, y),
        terrain_layer,
        movement_zone,
        speed_type,
        path_grid: Some(grid),
        resolved_terrain,
        terrain_costs,
        bypass_grid,
        mode,
        is_infantry: false,
        // Callers of this wrapper (bridge plane, scheduling, placement) carry
        // no mover type; the crusher route of the wall arm is not theirs.
        mover_is_crusher: false,
    })
    .is_clear()
}

/// Per-cell walkability and bridge metadata for pathfinding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathCell {
    pub ground_walkable: bool,
    pub bridge_walkable: bool,
    /// True only for cells stamped with the authoritative high-bridge structural flag.
    /// Ramp/transition cells may be bridge-walkable for A* without being structural.
    pub bridge_structural: bool,
    pub bridge_marker_0x80: bool,
    pub transition: bool,
    pub ground_level: u8,
    pub bridge_deck_level: u8,
    /// Per-cell ramp/slope index (1-20 = canonical ramp direction; 0 = cliff or no ramp).
    /// Sourced from the TMP tile `ramp_type` byte via `ResolvedTerrainCell.slope_type`.
    /// Read by the A* height-diff legality gate for diff-1 transitions.
    pub slope_type: u8,
    /// CellClass+0x116 equivalent for low bridge/tube cells.
    pub tube_index: Option<TubeId>,
    /// True when this cell has a valid tube index and final YR CellClass LandType 10.
    pub low_bridge_tube_cell: bool,
}

/// Sub-cell bits an infantryman can actually stand in.
///
/// Retail terrain objects write only sub-cells 2/3/4 into the ground occupation
/// plane, and the infantry cell-entry gate refuses the cell only when **all
/// three** are set. Any partial pattern leaves the cell enterable and the
/// sub-cell allocator picks a free slot. Stock `rulesmd.ini` gives 56 of 60
/// temperate terrain types `TemperateOccupationBits=4` (a single bit), so most
/// tree cells hold two infantry; only `=7` closes the cell.
pub const INFANTRY_SUBCELL_MASK: u8 = 0x1C;

/// Terrain-object INI occupation bits (`1|2|4`) shifted into the cell occupation
/// plane (`0x04|0x08|0x10`). Mirrors `sim::terrain_object::terrain_raw_occupation_mask`.
pub fn terrain_object_cell_bits_from_ini(ini_bits: u8) -> u8 {
    (ini_bits & 0x07) << 2
}

/// Whether a terrain object with these cell-plane bits closes the cell to infantry.
pub fn terrain_object_blocks_infantry(cell_bits: u8) -> bool {
    cell_bits & INFANTRY_SUBCELL_MASK == INFANTRY_SUBCELL_MASK
}

impl PathCell {
    pub fn is_bridge_transition_cell(&self) -> bool {
        self.transition
    }

    pub fn is_elevated_bridge_cell(&self) -> bool {
        self.bridge_deck_level_if_any()
            .is_some_and(|deck| deck > self.ground_level)
    }

    pub fn bridge_deck_level_if_any(&self) -> Option<u8> {
        self.bridge_walkable.then_some(self.bridge_deck_level)
    }

    pub fn has_structural_bridge(&self) -> bool {
        self.bridge_structural
    }

    pub fn has_bridge_marker_0x80(&self) -> bool {
        self.bridge_marker_0x80
    }

    pub fn has_bridgehead_transition(&self) -> bool {
        self.transition
    }

    pub fn signed_level(&self) -> i16 {
        self.ground_level as i8 as i16
    }

    pub fn effective_cell_z_for_layer(&self, layer: MovementLayer) -> u8 {
        match layer {
            MovementLayer::Bridge => self.bridge_deck_level_if_any().unwrap_or(self.ground_level),
            MovementLayer::Ground | MovementLayer::Air | MovementLayer::Underground => {
                self.ground_level
            }
        }
    }

    pub fn can_enter_bridge_layer_from_ground(&self) -> bool {
        self.bridge_walkable && self.is_bridge_transition_cell()
    }

    pub fn is_low_bridge_tube_cell(&self) -> bool {
        self.low_bridge_tube_cell
    }
}

/// Default ground-only cell: walkable, no bridges, level 0.
const DEFAULT_WALKABLE_CELL: PathCell = PathCell {
    ground_walkable: true,
    bridge_walkable: false,
    bridge_structural: false,
    bridge_marker_0x80: false,
    transition: false,
    ground_level: 0,
    bridge_deck_level: 0,
    slope_type: 0,
    tube_index: None,
    low_bridge_tube_cell: false,
};

/// Default blocked cell: not walkable, no bridges, level 0.
const DEFAULT_BLOCKED_CELL: PathCell = PathCell {
    ground_walkable: false,
    bridge_walkable: false,
    bridge_structural: false,
    bridge_marker_0x80: false,
    transition: false,
    ground_level: 0,
    bridge_deck_level: 0,
    slope_type: 0,
    tube_index: None,
    low_bridge_tube_cell: false,
};

/// Unified walkability grid for pathfinding.
///
/// Each cell stores ground walkability, bridge walkability, transition flags,
/// and height levels. Flat A* reads `ground_walkable` via `is_walkable()`;
/// layered A* reads both layers for bridge-aware routing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathGrid {
    cells: Vec<PathCell>,
    width: u16,
    height: u16,
    /// Per-cell terrain-object sub-cell occupation, already shifted into
    /// cell-plane bits (`0x04|0x08|0x10`). Retail terrain occupation is a
    /// sub-cell mask, not a whole-cell block, and only infantry read it.
    /// Empty when the grid was not built from resolved terrain; every cell then
    /// reads 0 and infantry fall back to `ground_walkable`.
    terrain_object_cell_bits: Vec<u8>,
    /// `ground_walkable` recomputed as if the cell held no terrain object.
    /// Same emptiness rule as `terrain_object_cell_bits`.
    ground_walkable_without_terrain_object: Vec<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayeredPathStep {
    pub rx: u16,
    pub ry: u16,
    pub layer: MovementLayer,
}

/// Shared scalar projection used during map construction and synchronous Recalc.
/// Derived path state is separate from structure occupancy and zone history.
fn project_terrain_path_cell(
    cell: &crate::map::resolved_terrain::ResolvedTerrainCell,
    bridge_state: Option<&BridgeRuntimeState>,
) -> (PathCell, u8, bool) {
    let bridge_structural = cell.bridge_facts.has_structural_bridge()
        || (cell.has_bridge_deck
            && !cell.bridge_layer.as_ref().is_some_and(|layer| {
                layer.direction == crate::map::resolved_terrain::BridgeDirection::Low
            })
            && cell.bridge_facts.family == crate::map::bridge_facts::BridgeStampFamily::None);
    let bridge_intact = !bridge_structural
        || bridge_state.map_or(true, |state| state.is_bridge_walkable(cell.rx, cell.ry));
    let path_cell = PathCell {
        // Walkability rules (matching old PathGrid::from_resolved_terrain):
        // - Overlay blocks / terrain object blocks → blocked
        // - Intact bridge deck → walkable (overrides underlying terrain)
        // - Destroyed bridge deck → revert to underlying terrain
        // - Cliff → blocked
        // - Water → walkable (SpeedType cost=0 blocks ground in A*)
        // - Everything else → use ground_walk_blocked
        ground_walkable: if cell.overlay_blocks || cell.terrain_object_blocks {
            false
        } else if bridge_structural {
            if bridge_intact {
                true
            } else {
                // Destroyed bridge: revert to underlying terrain walkability.
                !cell.is_cliff_like && !cell.ground_walk_blocked
            }
        } else if cell.bridge_walkable && cell.bridge_transition {
            // Bridgehead ramp: walkable on the ground layer regardless
            // of the TMP ramp tile's ground_walk_blocked flag. gamemd
            // gates ground entry through the SpeedType/LandType matrix
            // (land_type=Clear/Road → passable for vehicles/infantry);
            // we don't yet route non-water movers through that matrix,
            // so the boolean would otherwise reject a same-height
            // plateau→bridgehead step and trap the unit on the wrong
            // side. The bridge-layer gate at A* expansion still
            // enforces "enter bridge via bridgehead" via the
            // bridge_transition flag on the next deck cell.
            true
        } else if cell.is_cliff_like {
            false
        } else {
            !cell.ground_walk_blocked || cell.is_water
        },
        bridge_walkable: if bridge_structural {
            cell.bridge_walkable && bridge_intact
        } else {
            cell.bridge_walkable
        },
        bridge_structural,
        bridge_marker_0x80: cell.bridge_facts.has_flag(BRIDGE_FLAG_ANCHOR_SELF),
        transition: if bridge_structural {
            cell.bridge_transition && bridge_intact
        } else {
            cell.bridge_transition
        },
        ground_level: cell.level,
        bridge_deck_level: bridge_state
            .and_then(|state| state.cell(cell.rx, cell.ry))
            .map(|runtime| runtime.deck_level)
            .unwrap_or(cell.bridge_deck_level),
        slope_type: cell.slope_type,
        tube_index: cell.tube_index,
        low_bridge_tube_cell: cell.is_low_bridge_tube_cell(),
    };
    // Infantry overlay: recompute the same walkability decision with the
    // terrain object removed, so only the sub-cell mask decides.
    let walkable_without_terrain_object = if cell.overlay_blocks {
        false
    } else if bridge_structural {
        if bridge_intact {
            true
        } else {
            !cell.is_cliff_like && !(cell.base_ground_walk_blocked || cell.overlay_blocks)
        }
    } else if cell.bridge_walkable && cell.bridge_transition {
        true
    } else if cell.is_cliff_like {
        false
    } else {
        !(cell.base_ground_walk_blocked || cell.overlay_blocks) || cell.is_water
    };
    (
        path_cell,
        cell.terrain_object_occupation
            .map_or(0, terrain_object_cell_bits_from_ini),
        walkable_without_terrain_object,
    )
}

impl PathGrid {
    /// Create a new grid where all cells are ground-walkable with no bridges.
    pub fn new(width: u16, height: u16) -> Self {
        let size = width as usize * height as usize;
        Self {
            cells: vec![DEFAULT_WALKABLE_CELL; size],
            width,
            height,
            terrain_object_cell_bits: Vec::new(),
            ground_walkable_without_terrain_object: Vec::new(),
        }
    }

    /// Terrain-object sub-cell occupation bits at a cell, in cell-plane form
    /// (`0x04|0x08|0x10`). Zero when the cell holds no terrain object or the
    /// grid carries no terrain-object overlay.
    pub fn terrain_object_cell_bits_at(&self, x: u16, y: u16) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        let idx = y as usize * self.width as usize + x as usize;
        self.terrain_object_cell_bits.get(idx).copied().unwrap_or(0)
    }

    /// Ground walkability for an infantry mover.
    ///
    /// Identical to `is_walkable` except for the terrain-object clause: retail
    /// terrain objects occupy sub-cells, so a tree or rock only closes the cell
    /// to infantry when its mask fills every functional sub-cell. Any other
    /// blocking clause (cliff, overlay, `ground_walk_blocked`, destroyed bridge)
    /// still blocks.
    pub fn is_walkable_for_infantry(&self, x: u16, y: u16) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let idx = y as usize * self.width as usize + x as usize;
        let bits = self.terrain_object_cell_bits.get(idx).copied().unwrap_or(0);
        if bits == 0 {
            return self.cells[idx].ground_walkable;
        }
        if terrain_object_blocks_infantry(bits) {
            return false;
        }
        self.ground_walkable_without_terrain_object
            .get(idx)
            .copied()
            .unwrap_or(self.cells[idx].ground_walkable)
    }

    /// Grid width accessor.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Grid height accessor.
    pub fn height(&self) -> u16 {
        self.height
    }

    /// Build a flat ground-level view derived from this path grid.
    ///
    /// This is intentionally a derived value, not persisted simulation state:
    /// `ResolvedTerrainGrid` is the terrain-level owner and `PathGrid` is the
    /// current path/cache projection.
    pub fn ground_height_grid(&self) -> Vec<u8> {
        self.cells.iter().map(|cell| cell.ground_level).collect()
    }

    /// Mark a cell as blocked (ground layer) or unblocked.
    pub fn set_blocked(&mut self, x: u16, y: u16, blocked: bool) {
        if x < self.width && y < self.height {
            let idx = y as usize * self.width as usize + x as usize;
            self.cells[idx].ground_walkable = !blocked;
        }
    }

    /// Check if a cell is ground-walkable. Out-of-bounds = impassable.
    pub fn is_walkable(&self, x: u16, y: u16) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let idx = y as usize * self.width as usize + x as usize;
        self.cells[idx].ground_walkable
    }

    /// Check if a cell is walkable on a specific movement layer.
    pub fn is_walkable_on_layer(&self, x: u16, y: u16, layer: MovementLayer) -> bool {
        let Some(cell) = self.cell(x, y) else {
            return false;
        };
        match layer {
            MovementLayer::Ground => cell.ground_walkable,
            MovementLayer::Bridge => cell.bridge_walkable,
            MovementLayer::Air | MovementLayer::Underground => false,
        }
    }

    /// Check if a cell is walkable on either ground or bridge layer.
    pub fn is_any_layer_walkable(&self, x: u16, y: u16) -> bool {
        if self.is_walkable(x, y) {
            return true;
        }
        self.is_walkable_on_layer(x, y, MovementLayer::Bridge)
    }

    /// Access full cell data. Returns `None` for out-of-bounds.
    pub fn cell(&self, x: u16, y: u16) -> Option<&PathCell> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.cells
            .get(y as usize * self.width as usize + x as usize)
    }

    /// Replace one derived terrain cell while retaining every other dynamic
    /// blocker already stamped into other cells. Callers must guarantee that
    /// the refreshed coordinate has no independent structure blocker. Canonical
    /// grids copy both terrain-object side tables; tableless test/headless grids
    /// retain their documented PathCell fallback.
    pub(crate) fn replace_cell_from(&mut self, source: &PathGrid, x: u16, y: u16) -> bool {
        let Some(source_cell) = source.cell(x, y).copied() else {
            return false;
        };
        let source_idx = y as usize * source.width as usize + x as usize;
        let source_size = source.cells.len();
        let source_aux = if source.terrain_object_cell_bits.len() == source_size
            && source.ground_walkable_without_terrain_object.len() == source_size
        {
            Some((
                source.terrain_object_cell_bits[source_idx],
                source.ground_walkable_without_terrain_object[source_idx],
            ))
        } else if source.terrain_object_cell_bits.is_empty()
            && source.ground_walkable_without_terrain_object.is_empty()
        {
            None
        } else {
            return false;
        };
        if x >= self.width || y >= self.height {
            return false;
        }
        let idx = y as usize * self.width as usize + x as usize;
        let target_size = self.cells.len();
        let target_has_aux = self.terrain_object_cell_bits.len() == target_size
            && self.ground_walkable_without_terrain_object.len() == target_size;
        let target_has_no_aux = self.terrain_object_cell_bits.is_empty()
            && self.ground_walkable_without_terrain_object.is_empty();
        if self.cells.get(idx).is_none() || (!target_has_aux && !target_has_no_aux) {
            return false;
        }
        self.cells[idx] = source_cell;
        if target_has_aux {
            let (terrain_object_bits, walkable_without_terrain_object) =
                source_aux.unwrap_or((0, source_cell.ground_walkable));
            self.terrain_object_cell_bits[idx] = terrain_object_bits;
            self.ground_walkable_without_terrain_object[idx] = walkable_without_terrain_object;
        }
        true
    }

    /// Whether this cell is a bridge transition point (units can switch layers here).
    pub fn is_transition(&self, x: u16, y: u16) -> bool {
        self.cell(x, y)
            .is_some_and(|c| c.is_bridge_transition_cell())
    }

    pub fn bridge_deck_level(&self, x: u16, y: u16) -> Option<u8> {
        self.cell(x, y).and_then(PathCell::bridge_deck_level_if_any)
    }

    pub fn can_enter_bridge_layer_from_ground(&self, x: u16, y: u16) -> bool {
        self.cell(x, y)
            .is_some_and(PathCell::can_enter_bridge_layer_from_ground)
    }

    pub fn tube_index_at(&self, x: u16, y: u16) -> Option<TubeId> {
        self.cell(x, y).and_then(|cell| cell.tube_index)
    }

    pub fn is_low_bridge_tube_cell(&self, x: u16, y: u16) -> bool {
        self.cell(x, y)
            .is_some_and(PathCell::is_low_bridge_tube_cell)
    }

    /// Find the nearest ground-walkable cell to `(x, y)`, searching in expanding rings.
    pub fn nearest_walkable(
        &self,
        x: u16,
        y: u16,
        max_radius: u16,
        entity_blocks: Option<&BTreeSet<(u16, u16)>>,
        allow: Option<(u16, u16)>,
    ) -> Option<(u16, u16)> {
        if self.is_walkable(x, y)
            && (allow == Some((x, y)) || entity_blocks.map_or(true, |b| !b.contains(&(x, y))))
        {
            return Some((x, y));
        }
        for radius in 1..=max_radius {
            let r = radius as i32;
            for d in -r..=r {
                let candidates = [
                    (x as i32 + d, y as i32 - r),
                    (x as i32 + d, y as i32 + r),
                    (x as i32 - r, y as i32 + d),
                    (x as i32 + r, y as i32 + d),
                ];
                for (cx, cy) in candidates {
                    if cx < 0 || cy < 0 || cx >= self.width as i32 || cy >= self.height as i32 {
                        continue;
                    }
                    let cx = cx as u16;
                    let cy = cy as u16;
                    if self.is_walkable(cx, cy)
                        && (allow == Some((cx, cy))
                            || entity_blocks.map_or(true, |b| !b.contains(&(cx, cy))))
                    {
                        return Some((cx, cy));
                    }
                }
            }
        }
        None
    }

    /// Find nearest walkable cell on either layer, searching in expanding rings.
    pub fn nearest_walkable_any_layer(
        &self,
        x: u16,
        y: u16,
        max_radius: u16,
        entity_blocks: Option<&BTreeSet<(u16, u16)>>,
        allow: Option<(u16, u16)>,
    ) -> Option<(u16, u16)> {
        let check = |cx: u16, cy: u16| -> bool {
            self.is_any_layer_walkable(cx, cy)
                && (allow == Some((cx, cy))
                    || entity_blocks.map_or(true, |b| !b.contains(&(cx, cy))))
        };
        if check(x, y) {
            return Some((x, y));
        }
        for radius in 1..=max_radius {
            let r = radius as i32;
            for d in -r..=r {
                let candidates = [
                    (x as i32 + d, y as i32 - r),
                    (x as i32 + d, y as i32 + r),
                    (x as i32 - r, y as i32 + d),
                    (x as i32 + r, y as i32 + d),
                ];
                for (cx, cy) in candidates {
                    if cx < 0 || cy < 0 || cx >= self.width as i32 || cy >= self.height as i32 {
                        continue;
                    }
                    if check(cx as u16, cy as u16) {
                        return Some((cx as u16, cy as u16));
                    }
                }
            }
        }
        None
    }

    /// Build a walkability grid from raw map cell data and tileset names.
    ///
    /// This is a legacy test/diagnostic fallback. Runtime pathing should use
    /// `from_resolved_terrain`, because resolved terrain carries TMP bytes,
    /// theater numeric cliff/ramp ranges, bridge overlays, and terrain objects.
    ///
    /// Strategy: start with all cells **blocked**, then mark cells that have
    /// valid terrain data (non-water, non-cliff) as walkable. This ensures
    /// cells outside the map bounds are impassable by default.
    /// No bridge data is populated — use `from_resolved_terrain_with_bridges` for that.
    pub fn from_map_data(
        cells: &[MapCell],
        lookup: Option<&TilesetLookup>,
        map_width: u16,
        map_height: u16,
    ) -> Self {
        let size = map_width as usize * map_height as usize;
        let mut grid = PathGrid {
            cells: vec![DEFAULT_BLOCKED_CELL; size],
            width: map_width,
            height: map_height,
            terrain_object_cell_bits: Vec::new(),
            ground_walkable_without_terrain_object: Vec::new(),
        };

        let mut walkable_count: u32 = 0;
        let mut water_count: u32 = 0;
        let mut cliff_count: u32 = 0;

        for cell in cells {
            if cell.tile_index < 0 {
                continue;
            }
            if cell.rx >= map_width || cell.ry >= map_height {
                continue;
            }
            let tile_id: u16 = if cell.tile_index == 0xFFFF {
                0
            } else {
                cell.tile_index as u16
            };
            let is_water = lookup.map_or(false, |l| l.is_water(tile_id));
            let is_cliff = lookup.map_or(false, |l| l.is_cliff(tile_id));
            if is_water {
                water_count += 1;
            }
            if is_cliff {
                cliff_count += 1;
                continue;
            }
            let idx = cell.ry as usize * map_width as usize + cell.rx as usize;
            if idx < grid.cells.len() {
                grid.cells[idx].ground_walkable = true;
                walkable_count += 1;
            }
        }

        log::info!(
            "PathGrid: {}x{} — {} walkable, {} water, {} cliff, {} total blocked",
            map_width,
            map_height,
            walkable_count,
            water_count,
            cliff_count,
            size as u32 - walkable_count,
        );

        grid
    }

    /// Build from resolved terrain without bridge data.
    pub fn from_resolved_terrain(terrain: &ResolvedTerrainGrid) -> Self {
        Self::from_resolved_terrain_with_bridges(terrain, None)
    }

    /// Build from resolved terrain with bridge metadata.
    ///
    /// Ground walkability: water cells and bridge deck cells are kept walkable
    /// even when `ground_walk_blocked` is true — SpeedType-dependent blocking
    /// is handled by TerrainCostGrid (cost=0 blocks ground units in A*).
    /// This preserves the behavior of the old flat PathGrid where Float/Hover/
    /// Amphibious units could path through water via cost > 0.
    pub fn from_resolved_terrain_with_bridges(
        terrain: &ResolvedTerrainGrid,
        bridge_state: Option<&BridgeRuntimeState>,
    ) -> Self {
        let size = terrain.width() as usize * terrain.height() as usize;
        let mut cells = vec![DEFAULT_BLOCKED_CELL; size];
        // Retail terrain occupation is a per-cell *sub-cell* mask that only the
        // infantry entry gate reads; vehicles keep the whole-cell block. These
        // two side tables carry the infantry view without changing
        // `PathCell::ground_walkable`, which every non-infantry consumer reads.
        let mut terrain_object_cell_bits = vec![0u8; size];
        let mut ground_walkable_without_terrain_object = vec![false; size];
        for cell in terrain.iter() {
            let (path_cell, bits, walkable_without_terrain_object) =
                project_terrain_path_cell(cell, bridge_state);
            let index = cell.ry as usize * terrain.width() as usize + cell.rx as usize;
            if let Some(slot) = cells.get_mut(index) {
                *slot = path_cell;
            }
            if let Some(slot) = terrain_object_cell_bits.get_mut(index) {
                *slot = bits;
            }
            if let Some(slot) = ground_walkable_without_terrain_object.get_mut(index) {
                *slot = walkable_without_terrain_object;
            }
        }
        Self {
            cells,
            width: terrain.width(),
            height: terrain.height(),
            terrain_object_cell_bits,
            ground_walkable_without_terrain_object,
        }
    }

    /// Publish just one recalculated terrain cell. Existing structure presence
    /// is supplied by the world owner; unrelated cells and zone graphs stay intact.
    /// Tableless headless grids retain their documented PathCell-only fallback.
    pub(crate) fn refresh_resolved_cell(
        &mut self,
        cell: &crate::map::resolved_terrain::ResolvedTerrainCell,
        bridge_state: Option<&BridgeRuntimeState>,
        structure_blocked: bool,
    ) -> bool {
        if cell.rx >= self.width || cell.ry >= self.height {
            return false;
        }
        let index = usize::from(cell.ry) * usize::from(self.width) + usize::from(cell.rx);
        let (path_cell, bits, without_terrain) = project_terrain_path_cell(cell, bridge_state);
        self.cells[index] = path_cell;
        if let Some(slot) = self.terrain_object_cell_bits.get_mut(index) {
            *slot = bits;
        }
        if let Some(slot) = self.ground_walkable_without_terrain_object.get_mut(index) {
            *slot = without_terrain;
        }
        if structure_blocked {
            self.block_structure_cell(cell.rx, cell.ry);
        }
        true
    }

    /// Read-only counterpart of the one-cell publisher. Ordinary Enter/Exit
    /// usually recomputes unchanged terrain; retain the shared grid Arc then.
    pub(crate) fn resolved_cell_is_current(
        &self,
        cell: &crate::map::resolved_terrain::ResolvedTerrainCell,
        bridge_state: Option<&BridgeRuntimeState>,
        structure_blocked: bool,
    ) -> bool {
        if cell.rx >= self.width || cell.ry >= self.height {
            return false;
        }
        let index = usize::from(cell.ry) * usize::from(self.width) + usize::from(cell.rx);
        let (mut projected, bits, mut without_terrain) =
            project_terrain_path_cell(cell, bridge_state);
        if structure_blocked {
            projected.ground_walkable = false;
            without_terrain = false;
        }
        self.cells[index] == projected
            && self
                .terrain_object_cell_bits
                .get(index)
                .is_none_or(|old| *old == bits)
            && self
                .ground_walkable_without_terrain_object
                .get(index)
                .is_none_or(|old| *old == without_terrain)
    }

    /// A marked structure closes ground movement even where terrain occupation
    /// leaves an infantry sub-cell free. Deck movement above it is independent.
    pub(crate) fn block_structure_cell(&mut self, x: u16, y: u16) {
        if x >= self.width || y >= self.height {
            return;
        }
        let index = usize::from(y) * usize::from(self.width) + usize::from(x);
        self.cells[index].ground_walkable = false;
        if let Some(slot) = self.ground_walkable_without_terrain_object.get_mut(index) {
            *slot = false;
        }
    }

    /// Compute cells whose path-relevant walkability differs between two grids.
    /// Returns `None` if grids have different dimensions (full rebuild needed).
    pub fn diff_cells(&self, other: &PathGrid) -> Option<Vec<(u16, u16)>> {
        if self.width != other.width || self.height != other.height {
            return None;
        }
        let w = self.width as usize;
        let mut changed = Vec::new();
        for (idx, (a, b)) in self.cells.iter().zip(other.cells.iter()).enumerate() {
            if a.ground_walkable != b.ground_walkable
                || a.bridge_walkable != b.bridge_walkable
                || a.bridge_structural != b.bridge_structural
                || a.bridge_marker_0x80 != b.bridge_marker_0x80
                || a.transition != b.transition
                || a.ground_level != b.ground_level
                || a.bridge_deck_level != b.bridge_deck_level
                || a.slope_type != b.slope_type
                || a.tube_index != b.tube_index
                || a.low_bridge_tube_cell != b.low_bridge_tube_cell
                || self.terrain_object_cell_bits.get(idx) != other.terrain_object_cell_bits.get(idx)
                || self.ground_walkable_without_terrain_object.get(idx)
                    != other.ground_walkable_without_terrain_object.get(idx)
            {
                changed.push(((idx % w) as u16, (idx / w) as u16));
            }
        }
        Some(changed)
    }

    /// Mark cells occupied by a building footprint as blocked (ground layer).
    /// Buildings only block the ground layer — bridge decks above are unaffected.
    /// AddOccupy/RemoveOccupy art.ini overrides are intentionally ignored here:
    /// they affect hidden occupancy counters, not the real foundation. When
    /// `has_bib` is true, the east-edge column of the foundation is left unblocked so units
    /// can drive across the bib strip — matches the original engine's HasBib
    /// relaxation in the per-cell occupant chain check (probes east neighbor;
    /// skips blocking when that neighbor isn't the same building).
    /// NumberImpassableRows is live `Can_Enter_Cell` state, not static grid data.
    pub fn block_building_movement_cells(
        &mut self,
        cell_rx: u16,
        cell_ry: u16,
        foundation: &str,
        has_bib: bool,
    ) {
        let foundation_cells =
            crate::sim::production::building_base_foundation_cells(cell_rx, cell_ry, foundation);
        let blocking =
            crate::sim::production::building_movement_blocking_cells(&foundation_cells, has_bib);
        for (rx, ry) in blocking {
            self.block_structure_cell(rx, ry);
        }
    }

    /// Compatibility wrapper for older callers. Add/Remove parameters are
    /// intentionally ignored for movement blocking.
    pub fn block_building_footprint(
        &mut self,
        cell_rx: u16,
        cell_ry: u16,
        foundation: &str,
        _add_occupy: &[(i16, i16)],
        _remove_occupy: &[(i16, i16)],
        has_bib: bool,
    ) {
        self.block_building_movement_cells(cell_rx, cell_ry, foundation, has_bib);
    }

    /// Construct from raw cell data (test helper).
    #[cfg(test)]
    pub fn from_cells(cells: Vec<PathCell>, width: u16, height: u16) -> Self {
        Self {
            cells,
            width,
            height,
            terrain_object_cell_bits: Vec::new(),
            ground_walkable_without_terrain_object: Vec::new(),
        }
    }

    /// Test-only helper: directly write a cell's bridge fields.
    #[cfg(test)]
    pub fn set_cell_for_test(
        &mut self,
        x: u16,
        y: u16,
        ground_level: u8,
        bridge_walkable: bool,
        transition: bool,
    ) {
        if x < self.width && y < self.height {
            let idx = y as usize * self.width as usize + x as usize;
            let bridge_deck_level = if bridge_walkable {
                ground_level.saturating_add(4)
            } else {
                0
            };
            self.cells[idx] = PathCell {
                ground_walkable: true,
                bridge_walkable,
                bridge_structural: bridge_walkable,
                bridge_marker_0x80: false,
                transition,
                ground_level,
                bridge_deck_level,
                slope_type: 0,
                tube_index: None,
                low_bridge_tube_cell: false,
            };
        }
    }

    /// Set a bridge cell with `bridge_structural`, `bridge_walkable` and
    /// `bridge_deck_level` chosen independently.
    ///
    /// `set_cell_for_test` derives `bridge_structural` from `bridge_walkable` and
    /// `bridge_deck_level` from `ground_level`, so it cannot express a cell that
    /// is structurally part of a span while its walkability permission or its
    /// stored deck value says otherwise. gamemd has no such coupling — the deck
    /// height there is terrain plus a constant gated only on the mover's own
    /// OnBridge byte (`FootClass::Set_Height_On_Bridge` 0x005F5FA0) — so tests
    /// need to be able to build the decoupled state to prove the mover's height
    /// is immune to it.
    #[cfg(test)]
    pub fn set_bridge_cell_decoupled_for_test(
        &mut self,
        x: u16,
        y: u16,
        ground_level: u8,
        bridge_structural: bool,
        bridge_walkable: bool,
        bridge_deck_level: u8,
        transition: bool,
    ) {
        if x < self.width && y < self.height {
            let idx = y as usize * self.width as usize + x as usize;
            self.cells[idx] = PathCell {
                ground_walkable: true,
                bridge_walkable,
                bridge_structural,
                bridge_marker_0x80: false,
                transition,
                ground_level,
                bridge_deck_level,
                slope_type: 0,
                tube_index: None,
                low_bridge_tube_cell: false,
            };
        }
    }
}

/// A* search node stored in the open set (priority queue).
///
/// Implements Ord so BinaryHeap<Reverse<AStarNode>> gives us a min-heap
/// ordered by f_cost (lowest cost explored first).
#[derive(Debug, Clone, Eq, PartialEq)]
struct AStarNode {
    /// Total estimated cost: g_cost + heuristic.
    f_cost: i32,
    /// Cost from start to this node (actual, not estimated).
    g_cost: i32,
    /// Cell coordinates.
    x: u16,
    y: u16,
    /// Path height at this node — used for bridge-aware routing.
    /// Ground-only searches carry ground_level throughout.
    height: u8,
    /// Layer flag decided at push-time from predecessor.height vs this cell,
    /// matching gamemd.exe's push-time layer selection. Used at pop-time for
    /// closed-list marking and for `reconstruct_path_dual` array selection
    /// so storage and retrieval always agree (fixes bridgehead transition
    /// cells where pop-time re-derivation from own height would diverge).
    on_bridge: bool,
}

impl Ord for AStarNode {
    /// **VERA-internal tie-break, gamemd equivalent UNCHECKED.** Native
    /// resolves an exact f-tie by its heap's insertion order, which a
    /// `BinaryHeap` cannot reproduce; this orders by g descending, then y,
    /// then x so the search is reproducible for lockstep and replay.
    ///
    /// Trigger: two open nodes with identical f. Player effect: a different
    /// expansion order, and so possibly a different equal-cost route.
    /// Frequency: uncommon once the 0.001-0.008 direction tiebreaks apply,
    /// since they separate most otherwise-equal edges. Downstream risk: none
    /// within VERA — it is what makes the search deterministic.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Primary: f_cost ascending (lower is better).
        // Tiebreak: higher g_cost preferred (closer to goal, more "explored").
        self.f_cost
            .cmp(&other.f_cost)
            .then_with(|| other.g_cost.cmp(&self.g_cost))
            .then_with(|| self.y.cmp(&other.y))
            .then_with(|| self.x.cmp(&other.x))
    }
}

impl PartialOrd for AStarNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Euclidean distance heuristic for 8-directional grid movement.
///
/// Matches the original engine's per-node heuristic computation —
/// sqrt(dx² + dy²) — applied at A* node creation time.
///
/// Intentionally **inadmissible** under uniform edge cost: when both dx and
/// dy are nonzero, Euclidean overestimates the true minimum path cost
/// (which is `max(dx, dy) * STEP_COST` since one diagonal step covers
/// both axes for the same price). The original engine accepts this
/// inadmissibility — A* trades optimality for shorter expansion, and
/// the resulting path geometry is part of the engine's character.
///
/// Implementation: scaled integer sqrt at full precision. `u64`
/// intermediate avoids overflow on 512×512 maps.
fn euclidean_heuristic(ax: u16, ay: u16, bx: u16, by: u16) -> i32 {
    let dx = (ax as i64 - bx as i64).unsigned_abs();
    let dy = (ay as i64 - by as i64).unsigned_abs();
    let sum_sq: u64 = dx * dx + dy * dy;
    // sqrt(sum_sq) * STEP_COST, computed as sqrt(sum_sq * STEP_COST²) for full precision.
    (sum_sq * 1_000_000).isqrt() as i32
}

/// Compute the code-2 cost multiplier for a friendly-moving blocker.
///
/// Matches gamemd.exe AStar_compute_edge_cost @ 0x00429830 — the dynamic
/// cost computation for Can_Enter_Cell return code 2 (friendly moving).
///
/// - `urgency == 2` → `1000` (route around the blocker)
/// - `urgency == 1` → `4`   (traffic penalty, no look-ahead)
/// - `urgency == 0` → walk up to 10 hops along the blocker chain. Each hop
///   reads `map[cur_cell]` which gives the blocker's next cell, then jumps
///   to that cell and repeats. Returns `1` if the chain clears (next cell
///   has no blocker) within 10 hops. Returns `4` if the chain lasts all
///   10 hops.
fn compute_code2_multiplier(
    urgency: u8,
    start_cell: (u16, u16),
    layer: MovementLayer,
    map: &LayeredEntityBlockMap,
) -> i32 {
    if urgency >= 2 {
        return CODE2_MULT_ROUTE_AROUND;
    }
    if urgency == 1 {
        return CODE2_MULT_JAM;
    }
    // urgency == 0: chain walk.
    let mut cur = start_cell;
    for _ in 0..CODE2_CHAIN_MAX_HOPS {
        let Some(entry) = map.get(layer, &cur) else {
            // No blocker at this cell → chain clears.
            return CODE2_MULT_CLEARING;
        };
        let Some(next) = entry.next_cell else {
            // Entry exists but has no next_cell (code 5/6 stationary) →
            // chain terminates. The code-2 mover upstream IS vacating,
            // so this counts as "clearing" from the mover's perspective.
            return CODE2_MULT_CLEARING;
        };
        if next == cur {
            // Degenerate self-loop — treat as jam.
            return CODE2_MULT_JAM;
        }
        cur = next;
    }
    // Full 10 hops still jammed.
    CODE2_MULT_JAM
}

fn apply_search_marker_cost(
    step_cost: i32,
    marker_overlay: Option<&SearchMarkerOverlay>,
    destination: (u16, u16),
) -> i32 {
    if marker_overlay.is_some_and(|overlay| overlay.contains(destination)) {
        step_cost * SEARCH_MARKER_COST_MULTIPLIER
    } else {
        step_cost
    }
}

/// Find a path from start to goal using A* search.
///
/// Returns `Some(path)` where path is a sequence of (rx, ry) cells from
/// start to goal (both inclusive). Returns `None` if no path exists or
/// the search exceeds MAX_SEARCH_NODES.
pub fn find_path(grid: &PathGrid, start: (u16, u16), goal: (u16, u16)) -> Option<Vec<(u16, u16)>> {
    let steps = astar_search(
        grid,
        start,
        MovementLayer::Ground,
        goal,
        &AStarOptions::default(),
    )?;
    Some(steps.into_iter().map(|s| (s.rx, s.ry)).collect())
}

/// A* pathfinding with optional land-row passability and entity blocking.
///
/// When `costs` is `Some`, a cell whose land row is zero is closed; every
/// non-zero row expands at the same step cost, which is what the native search
/// does (see the provenance in `astar_search`).
/// When `entity_blocks` is `Some`, cells in the set are treated as blocked
/// UNLESS they are the goal cell.
pub fn find_path_with_costs(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    costs: Option<&TerrainCostGrid>,
    entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    urgency: u8,
    mover_is_crusher: bool,
    is_infantry: bool,
) -> Option<Vec<(u16, u16)>> {
    find_path_with_costs_marker(
        grid,
        start,
        goal,
        costs,
        entity_blocks,
        movement_zone,
        resolved_terrain,
        entity_block_map,
        None,
        MoverSearchFacts {
            urgency,
            mover_is_crusher,
            is_infantry,
            wall_cost: None,
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub fn find_path_with_costs_marker(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    costs: Option<&TerrainCostGrid>,
    entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    facts: MoverSearchFacts<'_>,
) -> Option<Vec<(u16, u16)>> {
    let steps = astar_search(
        grid,
        start,
        MovementLayer::Ground,
        goal,
        &AStarOptions {
            terrain_costs: costs,
            entity_blocks,
            entity_block_map,
            marker_overlay,
            urgency: facts.urgency,
            mover_is_crusher: facts.mover_is_crusher,
            is_infantry: facts.is_infantry,
            search_cost_classifier: facts.wall_cost,
            movement_zone,
            resolved_terrain,
            ..Default::default()
        },
    )?;
    Some(steps.into_iter().map(|s| (s.rx, s.ry)).collect())
}

/// Corridor-restricted A*: only expands cells whose zone ID is in `allowed_zones`.
pub fn find_path_with_costs_corridor(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    costs: Option<&TerrainCostGrid>,
    entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    zone_map: &super::zone_map::ZoneMap,
    allowed_zones: &BTreeSet<super::zone_map::ZoneId>,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    urgency: u8,
    mover_is_crusher: bool,
    is_infantry: bool,
) -> Option<Vec<(u16, u16)>> {
    find_path_with_costs_corridor_marker(
        grid,
        start,
        goal,
        costs,
        entity_blocks,
        zone_map,
        allowed_zones,
        movement_zone,
        resolved_terrain,
        entity_block_map,
        None,
        MoverSearchFacts {
            urgency,
            mover_is_crusher,
            is_infantry,
            wall_cost: None,
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub fn find_path_with_costs_corridor_marker(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    costs: Option<&TerrainCostGrid>,
    entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    zone_map: &super::zone_map::ZoneMap,
    allowed_zones: &BTreeSet<super::zone_map::ZoneId>,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    facts: MoverSearchFacts<'_>,
) -> Option<Vec<(u16, u16)>> {
    let steps = astar_search(
        grid,
        start,
        MovementLayer::Ground,
        goal,
        &AStarOptions {
            terrain_costs: costs,
            entity_blocks,
            corridor: Some((zone_map, allowed_zones)),
            entity_block_map,
            marker_overlay,
            urgency: facts.urgency,
            mover_is_crusher: facts.mover_is_crusher,
            is_infantry: facts.is_infantry,
            search_cost_classifier: facts.wall_cost,
            movement_zone,
            resolved_terrain,
            ..Default::default()
        },
    )?;
    Some(steps.into_iter().map(|s| (s.rx, s.ry)).collect())
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
pub(crate) fn find_path_with_costs_hierarchy_marker(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    costs: Option<&TerrainCostGrid>,
    entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    level0_zones: &ZoneLevelGraph,
    marked_level0: &BTreeSet<ZoneId>,
    blocker_neighbor_counts: &BlockerNeighborCounts,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    urgency: u8,
    mover_is_crusher: bool,
    is_infantry: bool,
) -> Option<Vec<(u16, u16)>> {
    Some(
        find_path_with_costs_hierarchy_marker_progress(
            grid,
            start,
            goal,
            costs,
            entity_blocks,
            level0_zones,
            marked_level0,
            blocker_neighbor_counts,
            &[],
            movement_zone,
            resolved_terrain,
            entity_block_map,
            marker_overlay,
            MoverSearchFacts {
                urgency,
                mover_is_crusher,
                is_infantry,
                wall_cost: None,
            },
        )?
        .path,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HierarchyMarkerPathResult {
    pub path: Vec<(u16, u16)>,
    pub progress_cell: (u16, u16),
    pub progress_index: usize,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn find_path_with_costs_hierarchy_marker_progress(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    costs: Option<&TerrainCostGrid>,
    entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    level0_zones: &ZoneLevelGraph,
    marked_level0: &BTreeSet<ZoneId>,
    blocker_neighbor_counts: &BlockerNeighborCounts,
    level0_path: &[ZoneId],
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    facts: MoverSearchFacts<'_>,
) -> Option<HierarchyMarkerPathResult> {
    let progress = HierarchyProgressTracker::new(start, level0_path);
    let steps = astar_search(
        grid,
        start,
        MovementLayer::Ground,
        goal,
        &AStarOptions {
            terrain_costs: costs,
            entity_blocks,
            hierarchy_gate: Some(HierarchyGate {
                level0_zones,
                marked_level0,
                blocker_neighbor_counts,
            }),
            hierarchy_progress: Some(&progress),
            entity_block_map,
            marker_overlay,
            urgency: facts.urgency,
            mover_is_crusher: facts.mover_is_crusher,
            is_infantry: facts.is_infantry,
            search_cost_classifier: facts.wall_cost,
            movement_zone,
            resolved_terrain,
            ..Default::default()
        },
    )?;
    Some(HierarchyMarkerPathResult {
        path: steps.into_iter().map(|s| (s.rx, s.ry)).collect(),
        progress_cell: progress.progress_cell(),
        progress_index: progress.progress_index(),
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn find_layered_path_hierarchy_marker(
    grid: &PathGrid,
    ground_blocks: Option<&BTreeSet<(u16, u16)>>,
    bridge_blocks: Option<&BTreeSet<(u16, u16)>>,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    terrain_costs: Option<&TerrainCostGrid>,
    level0_zones: &ZoneLevelGraph,
    marked_level0: &BTreeSet<ZoneId>,
    blocker_neighbor_counts: &BlockerNeighborCounts,
    level0_path: &[ZoneId],
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    facts: MoverSearchFacts<'_>,
) -> Option<Vec<LayeredPathStep>> {
    if !matches!(start_layer, MovementLayer::Ground | MovementLayer::Bridge) {
        return None;
    }
    let progress = HierarchyProgressTracker::new(start, level0_path);
    astar_search(
        grid,
        start,
        start_layer,
        goal,
        &AStarOptions {
            terrain_costs,
            resolved_terrain,
            entity_blocks: ground_blocks,
            bridge_blocks,
            hierarchy_gate: Some(HierarchyGate {
                level0_zones,
                marked_level0,
                blocker_neighbor_counts,
            }),
            hierarchy_progress: Some(&progress),
            entity_block_map,
            marker_overlay,
            urgency: facts.urgency,
            mover_is_crusher: facts.mover_is_crusher,
            is_infantry: facts.is_infantry,
            search_cost_classifier: facts.wall_cost,
            movement_zone,
            ..Default::default()
        },
    )
}

/// Resolve a gamemd foundation name into pathfinding footprint dimensions.
#[cfg(test)]
fn parse_foundation(foundation: &str) -> (u16, u16) {
    crate::rules::foundation::foundation_dimensions(foundation)
}

/// Truncate a path to at most `max_steps` movement steps.
///
/// The path includes the start cell at index 0, so a path with `max_steps`
/// movement steps has `max_steps + 1` entries. If the path is already short
/// enough, it is returned unchanged.
pub fn truncate_path(path: Vec<(u16, u16)>, max_steps: usize) -> Vec<(u16, u16)> {
    let max_len: usize = max_steps + 1; // +1 for start cell at index 0
    if path.len() <= max_len {
        path
    } else {
        path[..max_len].to_vec()
    }
}

/// Truncate a layered path (coords + layers) to at most `max_steps` movement steps.
pub fn truncate_layered_path(
    path: Vec<(u16, u16)>,
    layers: Vec<MovementLayer>,
    max_steps: usize,
) -> (Vec<(u16, u16)>, Vec<MovementLayer>) {
    debug_assert_eq!(
        path.len(),
        layers.len(),
        "truncate_layered_path: input length mismatch: {} vs {}",
        path.len(),
        layers.len()
    );
    let max_len: usize = max_steps + 1;
    if path.len() <= max_len {
        (path, layers)
    } else {
        (path[..max_len].to_vec(), layers[..max_len].to_vec())
    }
}

/// Bridge-aware A* pathfinding with height-based routing.
///
/// Uses dual closed lists (ground/bridge) per cell for bridge-aware routing.
/// Returns per-cell layer assignment derived from height comparison.
pub fn find_layered_path(
    grid: &PathGrid,
    ground_blocks: Option<&BTreeSet<(u16, u16)>>,
    bridge_blocks: Option<&BTreeSet<(u16, u16)>>,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    terrain_costs: Option<&TerrainCostGrid>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    urgency: u8,
    mover_is_crusher: bool,
    is_infantry: bool,
) -> Option<Vec<LayeredPathStep>> {
    // Test-facing wrapper for pure layered pathfinding: no mover zone, so the
    // Foot class arm of the cell predicate is not consulted. Production enters
    // through `zone_search::find_layered_path_zoned_marker_detailed`, which
    // passes the mover's zone to `find_layered_path_marker`.
    find_layered_path_marker(
        grid,
        ground_blocks,
        bridge_blocks,
        start,
        start_layer,
        goal,
        terrain_costs,
        None,
        resolved_terrain,
        entity_block_map,
        None,
        MoverSearchFacts {
            urgency,
            mover_is_crusher,
            is_infantry,
            wall_cost: None,
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub fn find_layered_path_marker(
    grid: &PathGrid,
    ground_blocks: Option<&BTreeSet<(u16, u16)>>,
    bridge_blocks: Option<&BTreeSet<(u16, u16)>>,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    terrain_costs: Option<&TerrainCostGrid>,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    facts: MoverSearchFacts<'_>,
) -> Option<Vec<LayeredPathStep>> {
    if !matches!(start_layer, MovementLayer::Ground | MovementLayer::Bridge) {
        return None;
    }
    astar_search(
        grid,
        start,
        start_layer,
        goal,
        &AStarOptions {
            terrain_costs,
            // The mover's zone selects its SpeedType row and the Foot class arm
            // of the cell predicate (walls, crushable walls, under-span ground
            // admission). Every other search wrapper already carried it; this
            // one ran the layered fallbacks with `None`, which made
            // `evaluate_shared_cell_leaf` return on land passability alone.
            movement_zone,
            resolved_terrain,
            entity_blocks: ground_blocks,
            bridge_blocks,
            entity_block_map,
            marker_overlay,
            urgency: facts.urgency,
            mover_is_crusher: facts.mover_is_crusher,
            is_infantry: facts.is_infantry,
            search_cost_classifier: facts.wall_cost,
            ..Default::default()
        },
    )
}

// Tests extracted to core_tests.rs to stay under 400 lines.
#[cfg(test)]
#[path = "core_tests.rs"]
mod core_tests;

#[cfg(test)]
impl PathGrid {
    /// Test helper: build a grid with every cell ground-walkable.
    pub fn test_all_passable(width: u16, height: u16) -> Self {
        let size = width as usize * height as usize;
        PathGrid {
            cells: vec![DEFAULT_WALKABLE_CELL; size],
            width,
            height,
            terrain_object_cell_bits: Vec::new(),
            ground_walkable_without_terrain_object: Vec::new(),
        }
    }

    /// Test helper: build a grid with every cell blocked.
    pub fn test_all_blocked(width: u16, height: u16) -> Self {
        let size = width as usize * height as usize;
        PathGrid {
            cells: vec![DEFAULT_BLOCKED_CELL; size],
            width,
            height,
            terrain_object_cell_bits: Vec::new(),
            ground_walkable_without_terrain_object: Vec::new(),
        }
    }
}
