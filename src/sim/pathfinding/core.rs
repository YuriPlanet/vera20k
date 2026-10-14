//! A* pathfinding on the isometric grid.
//!
//! PathGrid stores per-cell walkability and bridge metadata (ground_walkable,
//! bridge_walkable, transition, height levels). Flat A* reads ground_walkable;
//! layered A* reads both layers for bridge-aware routing.
//!
//! Live Foot searches delegate neighbor admission to the concrete Unit/Infantry
//! receiver. Its native cost class drives expansion and edge cost here; grid
//! passability remains a compatibility boundary for callers without a Foot.
//! The same admitted search owns its temporary markers through reconstruction
//! and both finishing passes.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on map/ (resolved terrain for walkability).
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use super::cell_entry::{CanEnterCellContext, evaluate_can_enter_cell, search_cell_cost_decision};
use super::stamped_table::StampedTable;
use super::terrain_cost::TerrainCostGrid;
use super::zone_hierarchy::{MarkedZones, ZoneLevelGraph};
use super::zone_search::PathSearchFailure;
use crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::tube_facts::{TubeId, TubeSource};
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::sim::movement::locomotor::MovementLayer;
use crate::util::direction_tables::CELL_DELTAS;
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap};

/// Accepted native path descriptor (`gamemd.exe 42AA90` reads the parent descriptor's
/// height into each outgoing-height slot). The selected cell/list is stamped
/// closed at generation, so this parent and height are written together once
/// and cannot be replaced while a descendant still points at them. Native execution:
/// `tools/spatial_oracle/astar_path_finishing.{py,json}` (supplied chains).
#[derive(Clone, Copy)]
struct PathDescriptor {
    parent: usize,
    height: i16,
}

impl PathDescriptor {
    const UNVISITED: Self = Self {
        parent: usize::MAX,
        height: 0,
    };
}

/// One layer's closed list: a (cell, layer) is closed in the running search
/// while it holds a descriptor. Closing writes the descriptor
/// (close-on-generation), so a closed cell's parent and height are current.
type ClosedLayer = StampedTable<PathDescriptor>;

/// Reusable scratch for `astar_search`. Lives in a thread-local so repeated
/// repaths share one set of full-map-sized buffers instead of allocating them
/// per call (was the dominant heap pressure source during repath storms —
/// engineer micro, scatter cascades).
///
/// A search touches only the cells it reaches, so `reset` clears nothing: the
/// closed lists forget the previous search at once (see `StampedTable`). The
/// buffers grow when a map is bigger than any before; the `BinaryHeap` is
/// cleared but keeps capacity. Use through `with_pathfind_workspace` only; it
/// asserts non-reentrancy so a future recursive A* caller fails loudly instead
/// of silently sharing buffers.
#[derive(Default)]
struct PathfindWorkspace {
    ground: ClosedLayer,
    bridge: ClosedLayer,
    open: BinaryHeap<Reverse<AStarNode>>,
}

impl PathfindWorkspace {
    fn reset(&mut self, total_cells: usize) {
        self.ground.begin(total_cells, PathDescriptor::UNVISITED);
        self.bridge.begin(total_cells, PathDescriptor::UNVISITED);
        self.open.clear();
    }
}

thread_local! {
    /// Thread-local reusable A* scratch. Safe because:
    ///   - each thread has its own (the batched path searches,
    ///     `movement::path_batch`, run A* on worker threads),
    ///   - A* never recursively invokes A* (no reentrancy),
    ///   - the workspace holds zero hashed sim state — pure scratch.
    /// `try_borrow_mut` panics with a clear message if either invariant is
    /// ever broken by a future refactor.
    static PATHFIND_WORKSPACE: RefCell<PathfindWorkspace> =
        RefCell::new(PathfindWorkspace::default());
}

/// Run `body` with exclusive mutable access to the thread-local A* workspace,
/// reset for a search over `total_cells` cells.
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

/// Code-2 (friendly moving) cost multipliers of `AStar_compute_edge_cost`
/// (`0x00429830`). See [`code2_multiplier`].
const CODE2_MULT_CLEARING: i32 = 1; // the class base: the chain clears
const CODE2_MULT_JAM: i32 = 4; // a nonzero urgency, or a jammed chain
const CODE2_MULT_ROUTE_AROUND: i32 = 1000; // urgency 2: route around

/// The objects the urgency-0 chain tests before it jams: its loop counter
/// runs to 10 (`0x00429978..0x0042997E`).
pub(crate) const CODE2_CHAIN_MAX_HOPS: usize = 10;

/// The code-2 multiplier of `AStar_compute_edge_cost @ 0x00429830`
/// (`0x00429878..0x004299A6`). A nonzero urgency (`PathfinderClass+0x3C`)
/// jams (4.0), and urgency 2 routes around (1000.0); urgency 0 asks
/// `chain_jams` whether the traffic ahead jams (4.0) or clears, keeping the
/// class base (1.0).
pub(crate) fn code2_multiplier<E>(
    urgency: u8,
    chain_jams: impl FnOnce() -> Result<bool, E>,
) -> Result<i32, E> {
    Ok(match urgency {
        0 if chain_jams()? => CODE2_MULT_JAM,
        0 => CODE2_MULT_CLEARING,
        2 => CODE2_MULT_ROUTE_AROUND,
        _ => CODE2_MULT_JAM,
    })
}

/// Temporary A* marker cost multiplier for `CellClass+0x140 & 0x40000`.
/// The original toggles this bit around one search; Rust models it as a
/// search-scoped overlay instead of mutating persistent `PathGrid` cells.
const SEARCH_MARKER_COST_MULTIPLIER: i32 = 4;

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

    pub(crate) fn apply_retained_delta(&mut self, index: usize, delta: u8) {
        self.counts[index] = self.counts[index].wrapping_add(delta);
    }

    #[cfg(test)]
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
    /// The counts wrap, so this is the exact inverse of the add.
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

    /// Reverse [`Self::add_building_expanded_foundation`].
    pub(crate) fn remove_building_expanded_foundation(
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
                self.decrement_i32(x, y);
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

/// A* expansion gate produced by `Zone_precheck`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HierarchyGate<'a> {
    pub level0_zones: &'a ZoneLevelGraph,
    pub marked_level0: &'a MarkedZones,
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
/// several routes tie. Index order matches [`CELL_DELTAS`]: N, NE, E, SE, S, SW,
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

/// Threshold for ground vs bridge closed-list selection.
/// Binary: `abs(path_height - cell.height_level) < 2`, the `CMP EAX,0x1` at
/// `0x00429E75` inside the layer-flag block that starts at `0x00429E54`.
const BRIDGE_HEIGHT_THRESHOLD: u16 = 2;

/// Encode source cell index + bridge flag into came_from value.
/// Max map = 512x512 = 262,144 cells -> fits in 18 bits, leaving bit 20 free.
const CAME_FROM_BRIDGE: usize = 1 << 20;

/// AStar429E54 selects its closed list from the candidate's raw0x100 and
/// the CURRENT signed path height, before calling Foot+1AC. Walkability is a
/// different derived field and must not turn an unflagged ramp into a deck.
fn is_at_bridge_level(path_height: i16, cell: &PathCell) -> bool {
    cell.has_structural_bridge()
        && path_height.abs_diff(cell.signed_level()) >= BRIDGE_HEIGHT_THRESHOLD
}

fn initial_search_heights(
    start: &PathCell,
    start_layer: MovementLayer,
    goal: &PathCell,
) -> (i16, i16) {
    (
        start.signed_level()
            + if start_layer == MovementLayer::Bridge {
                4
            } else {
                0
            },
        goal.signed_level() + if goal.has_structural_bridge() { 4 } else { 0 },
    )
}

/// Original42A18B compares the expanded node's current height, not the
/// initial source height; negative ground levels and level127+4 stay signed.
fn blocked_goal_height_matches(current: i16, goal: i16) -> bool {
    current.abs_diff(goal) <= 1
}

/// Original42A460 carries signed Cell+11B, optionally plus four, in a dword.
/// Keep that widened value: raw127 on a deck is131, not wrapped i8(-125).
pub(crate) fn compute_node_height(
    parent_height: i16,
    parent_cell: Option<&PathCell>,
    neighbor_cell: &PathCell,
) -> i16 {
    let Some(parent_cell) = parent_cell else {
        return parent_height;
    };
    let ground = neighbor_cell.signed_level();
    if !neighbor_cell.has_structural_bridge() {
        return ground;
    }
    let deck = if parent_cell.has_structural_bridge() {
        parent_height == parent_cell.signed_level() + 4
    } else {
        (2..=4).contains(&(parent_height - ground))
    };
    ground + if deck { 4 } else { 0 }
}

fn is_structural_bridge_deck_height(path_height: i16, cell: &PathCell) -> bool {
    cell.has_structural_bridge() && path_height == cell.signed_level() + 4
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
/// claim of the unconditional native receiver contract.
pub(crate) fn needs_bridge_traversal_for_edge(
    current_height: i16,
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
    let (dx, dy) = CELL_DELTAS[rotated];
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
    if input.direction == -1 {
        return BridgeTraversalResult {
            allowed: true,
            force_bridge_list: false,
        };
    }
    let mut path_height = input.path_height;

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
            force_bridge_list: false,
        };
    };

    if path_height == -1 && parent.has_structural_bridge() {
        path_height = parent.signed_level() + 4;
        if !input.candidate.has_bridgehead_transition() {
            return BridgeTraversalResult {
                allowed: false,
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
        force_bridge_list,
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

///429E19..429E21 tests the actual Cell pointer before layer, zone or +1AC
/// semantics, for compass and tube candidates alike. This lookup must not
/// stamp shared Dummy. Native controls: astar_structural_height.json.
fn search_cell_allocated(terrain: Option<&ResolvedTerrainGrid>, coord: (u16, u16)) -> bool {
    terrain.is_none_or(|terrain| {
        terrain
            .native_fixed_cell_index(coord.0 as i16, coord.1 as i16)
            .is_some()
    })
}

/// Configuration for the unified A* search. All fields optional; defaults
/// produce a bare ground-only search equivalent to the old `find_path`.
#[derive(Default)]
pub struct AStarOptions<'a> {
    /// Terrain speed multipliers (cost 0 = blocked for this SpeedType).
    pub terrain_costs: Option<&'a TerrainCostGrid>,
    /// The mover type's `IsTrain=` flag (`TechnoTypeClass+0xC94`), which
    /// `AStar_main_loop` reads at `0x00429B64`/`0x00429C79` to zero every
    /// passable `+0x1AC` class. Residual: no mover supplies it (always false);
    /// no retail `rulesmd.ini` type sets `IsTrain=`, so the arm is dormant.
    pub mover_is_train: bool,
    /// Full live Foot +1AC owner. When supplied, its class is the sole terrain
    /// and object admission verdict; the reduced grid/wall adapters are unused.
    pub foot_entry: Option<&'a dyn SearchFootEntry>,
    /// Search-scoped temporary marker overlay equivalent to
    /// `CellClass+0x140 & 0x40000`. Destination hits multiply normal compass
    /// edge cost, but do not change walkability or persistent pathgrid state.
    /// The live Foot search prepares this through the shared
    /// UpdateBridgePassability owner (0x0042ACF0, reached at 0x00429C1A)
    /// after wrapper/zone admission. A* and both finishing passes consume
    /// the transaction before it is discarded ahead of Foot Mark1.
    pub marker_overlay: Option<&'a SearchMarkerOverlay>,
    /// Reduced-admission searches only (no Foot +1AC search entry): selects
    /// the crusher arm of the reduced terrain entry. Searches through the live
    /// +1AC answer it natively and ignore this.
    pub mover_is_crusher: bool,
    /// Code-2 urgency escalation (0 = look-ahead chain walk, 1 = traffic penalty,
    /// 2 = route around). Matches gamemd.exe PathfinderClass+0x3C.
    pub urgency: u8,
    /// Binary-style hierarchy marker gate. Present only when blocker-neighbor
    /// counts are also available for the same search.
    pub(crate) hierarchy_gate: Option<HierarchyGate<'a>>,
    /// Movement zone for water mover bypass and passability matrix.
    pub movement_zone: Option<MovementZone>,
    /// Resolved terrain for cliff cost and water passability checks.
    pub resolved_terrain: Option<&'a ResolvedTerrainGrid>,
    /// Select the Infantry subcell rules in the reduced entry adapter.
    pub is_infantry: bool,
}

/// Candidate input at the shared Foot +1AC boundary. A* expands copied Cell
/// coordinates; finishing42B5AD/42BFB3/42C0B7 forwards its retained Cell pointer.
/// Retained Dummy identity must survive nested lookups changing its coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchEntryCandidate {
    CopiedCoord((u16, u16)),
    RetainedNativeCell(crate::map::cell_index::NativeCellIdentity),
}

/// Arguments supplied by AStar429F37..429F54 to Foot +1AC. Heights are native
/// signed Cell levels, not leptons. A* supplies the expanded previous Cell;
/// native path finishing (`42B5B4`, `42BE20`) supplies NULL instead.
#[derive(Debug, Clone, Copy)]
pub struct SearchEntryQuery {
    pub from: Option<(u16, u16)>,
    pub candidate: SearchEntryCandidate,
    pub direction: i32,
    pub path_height: i32,
}

pub trait SearchFootEntry {
    fn classify(&self, query: SearchEntryQuery) -> Result<u8, String>;

    /// Successful-search finishing reads Foot4DC760 and House threat state.
    /// Missing providers are explicit, never a silently assumed coefficient.
    fn finishing_threat_coefficient(
        &self,
    ) -> Result<crate::util::native_x87::NativeF64Bits, String> {
        Err("Foot finishing coefficient is unavailable".into())
    }
    fn finishing_house_threat(&self, _coord: (i16, i16)) -> Result<i32, String> {
        Err("Foot finishing House threat state is unavailable".into())
    }
    ///585F40 reads House+57E4 directly, without a Map lookup.
    fn hierarchy_house_threat(&self, _padded_index: i32) -> Result<i32, String> {
        Err("Foot hierarchy House threat state is unavailable".into())
    }
    /// The urgency-0 code-2 chain of `AStar_compute_edge_cost`
    /// (`0x00429886..0x00429984`) from the first object of `cell`'s `layer`
    /// list: whether the traffic ahead jams. It reads live objects, so a
    /// receiver without them has no answer.
    fn code2_chain_jams(&self, _cell: (u16, u16), _layer: MovementLayer) -> Result<bool, String> {
        Err("Foot code-2 chain is unavailable".into())
    }
    ///429C1A runs after wrapper admission. One transaction is retained through
    ///A* and both finishing passes; fixtures may supply their static overlay.
    fn prepare_search_markers(
        &self,
    ) -> Result<Option<(u8, std::cell::Ref<'_, SearchMarkerOverlay>)>, String> {
        Ok(None)
    }
    fn prepared_search_markers(&self) -> Option<std::cell::Ref<'_, SearchMarkerOverlay>> {
        None
    }
}

/// The mover facts the A* cost evaluation consults for every neighbour.
///
/// These facts travel together through every search entry, so they travel as
/// one value. `urgency` drives the code-2 escalation, `mover_is_crusher` and
/// `is_infantry` select the `Can_Enter_Cell` class arm, and `foot_entry` is
/// the live Foot `+0x1AC` receiver.
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
    pub foot_entry: Option<&'a dyn SearchFootEntry>,
}

/// Reconstruct a layered path from dual came_from arrays.
/// Walks backward from goal using `decode_from` to follow the parent chain
/// across ground/bridge transitions.
fn reconstruct_path_dual(
    ground: &ClosedLayer,
    bridge: &ClosedLayer,
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
        let descriptor = if current_bridge {
            bridge.get(current_idx)
        } else {
            ground.get(current_idx)
        }
        .expect("path reconstruction reached a cell this search never closed");
        path.push(LayeredPathStep {
            rx: x,
            ry: y,
            layer,
            path_height: descriptor.height,
        });

        if current_idx == start_idx && current_bridge == start_on_bridge {
            break;
        }

        let encoded = descriptor.parent;
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
/// neighbor. Refusal and unavailable live inputs remain distinct errors.
pub fn astar_search(
    grid: &PathGrid,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    options: &AStarOptions<'_>,
) -> Result<Vec<LayeredPathStep>, PathSearchFailure> {
    let is_water_mover = options.movement_zone.is_some_and(|mz| mz.is_water_mover());

    // Start cell may be blocked (e.g. unit standing inside a building footprint
    // after undock). The start node is seeded into the open set without a
    // passability check; only neighbor expansion calls Can_Enter_Cell. If all
    // 8 neighbors are also blocked, the open set exhausts and reports refusal
    // naturally — no need for an explicit start-cell rejection.

    // Original429A90..429B5A widens signed levels before adding the bridge
    // offset. The goal's raw structural flag selects its height even when
    // its eventual Foot admission refuses it (the blocked-goal tail still runs).
    let start_cell = grid.cell(start.0, start.1).unwrap_or(&DEFAULT_BLOCKED_CELL);
    let goal_cell = grid.cell(goal.0, goal.1).unwrap_or(&DEFAULT_BLOCKED_CELL);
    let (start_height, goal_height) = initial_search_heights(start_cell, start_layer, goal_cell);

    // 0x00429BF3..0x00429C0A: a start cell and height that are the goal's
    // skip the search, and AStar returns no route (0x0042A451).
    if start == goal && start_height == goal_height {
        return Err(PathSearchFailure::CellSearchExhausted);
    }

    // --- Arrays ---
    let w = grid.width() as usize;
    let h = grid.height() as usize;
    let total_cells = w * h;

    // Persistent A* scratch reused across calls; see `PathfindWorkspace`.
    // A node's g travels in the open set: the closed lists hold only the
    // stamp and the parent descriptor.
    with_pathfind_workspace(total_cells, |ws| {
        let PathfindWorkspace {
            ref mut ground,
            ref mut bridge,
            ref mut open,
        } = *ws;

        let start_idx = start.1 as usize * w + start.0 as usize;
        let start_on_bridge = is_at_bridge_level(start_height, start_cell);
        // Close-on-generation: gamemd stamps a (cell, layer) closed at the
        // moment a node is created for it and freezes its g there. The start
        // node is created before the loop, so it is stamped here.
        let start_descriptor = PathDescriptor {
            parent: usize::MAX,
            height: compute_node_height(start_height, None, start_cell),
        };
        if start_on_bridge {
            bridge.set(start_idx, start_descriptor);
        } else {
            ground.set(start_idx, start_descriptor);
        }

        open.push(Reverse(AStarNode {
            f_cost: euclidean_heuristic(start.0, start.1, goal.0, goal.1),
            g_cost: 0,
            x: start.0,
            y: start.1,
            height: start_descriptor.height,
            on_bridge: start_on_bridge,
        }));

        let mut nodes_evaluated: u32 = 0;

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
                return Ok(reconstruct_path_dual(
                    ground,
                    bridge,
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
                return Err(PathSearchFailure::CellSearchExhausted);
            }

            // --- Neighbor expansion ---
            for (dir_index, &(dx, dy)) in CELL_DELTAS.iter().enumerate() {
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
                if !search_cell_allocated(options.resolved_terrain, (nx, ny)) {
                    continue;
                }
                let n_idx = ny as usize * w + nx as usize;
                let neighbor_cell = grid.cell(nx, ny).unwrap_or(&DEFAULT_BLOCKED_CELL);

                // Closed-list selection: uses CURRENT node's height vs neighbor ground_level
                let mut neighbor_use_bridge = is_at_bridge_level(current.height, neighbor_cell);

                // Compute what height the NEW node carries forward (separate computation)
                let neighbor_height =
                    compute_node_height(current.height, Some(cur_cell), neighbor_cell);

                // Live +1AC owns these height/list predicates (Foot4D9C60).
                // Keep the reduced checks only for callers without a live owner.
                if options.foot_entry.is_none() {
                    // Height-diff legality gate. Diff-1 transitions require the LOWER cell's
                    // raw slope byte to be nonzero; diff ∈ {±2, ±3, ±4, ±5+} is
                    // always blocked. Legitimate bridge transitions arrive here as diff-0
                    // because `compute_node_height` already shifts unit Z onto/off the deck.
                    let needs_bridge_traversal =
                        needs_bridge_traversal_for_edge(current.height, cur_cell, neighbor_cell);
                    if needs_bridge_traversal {
                        let bridge_traversal = check_bridge_traversal(
                            grid,
                            BridgeTraversalInput {
                                candidate: neighbor_cell,
                                candidate_coord: (nx, ny),
                                direction: dir_index as i8,
                                path_height: current.height,
                                parent: Some((cur_cell, (cx, cy))),
                            },
                        );
                        if !bridge_traversal.allowed {
                            continue;
                        }
                        if bridge_traversal.force_bridge_list {
                            neighbor_use_bridge = true;
                        }
                    } else {
                        let diff = neighbor_height - current.height;
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
                            continue;
                        }
                    }
                }

                // Closed check on appropriate list
                // Binary `1.009` handling is an early closed-neighbor skip/fallback
                // nuance, not true A* reopen. Do not reinsert selected-layer closed
                // cells without a dedicated parity fixture for the blocked-goal path.
                if neighbor_use_bridge {
                    if bridge.contains(n_idx) {
                        continue;
                    }
                } else if ground.contains(n_idx) {
                    continue;
                }

                // Original429E54..429EAF exempts structural deck candidates
                // from the ordinary zone comparison. The signed-height/list
                // predicate is executed in astar_structural_height.json.
                // Residual: HierarchyGate also consults blocker-neighbor counts;
                // full equivalence of that adapter to the original hierarchy
                // comparison is not established by the scalar fragment corpus.
                let neighbor_is_bridge_deck = is_at_bridge_level(current.height, neighbor_cell);
                if let Some(gate) = options.hierarchy_gate
                    && !neighbor_is_bridge_deck
                    && !gate.allows(nx, ny)
                {
                    continue;
                }

                // Original429F54 calls the complete Foot+1AC for every reached
                // neighbor. In particular, Capture/NavCom may admit a building
                // that the static PathGrid closes. 429FEA tests THIS class before
                // the blocked-goal tail42A17D. No second terrain/object verdict.
                let refused_cost_class = if let Some(entry) = options.foot_entry {
                    Some(
                        entry
                            .classify(SearchEntryQuery {
                                from: Some((cx, cy)),
                                candidate: SearchEntryCandidate::CopiedCoord((nx, ny)),
                                direction: dir_index as i32,
                                path_height: i32::from(current.height),
                            })
                            .map_err(PathSearchFailure::CellEntryUnavailable)?,
                    )
                } else {
                    // Walkability check on the determined layer. Ground->Bridge entry
                    // still requires 0x200. Deck-to-deck moves passed the same
                    // flag's diff-0 gate in CheckBridgeTraversal; bridge_walkable
                    // alone does not admit a Forward2-style transverse stamp slot.
                    let neighbor_passable = if neighbor_use_bridge {
                        let prev_on_bridge = is_at_bridge_level(current.height, cur_cell);
                        let bridge_terrain_passable =
                            is_cell_passable_for_mover_on_layer_with_speed(
                                grid,
                                nx,
                                ny,
                                MovementLayer::Bridge,
                                options.movement_zone,
                                None,
                                options.resolved_terrain,
                                options.terrain_costs,
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
                            options.is_infantry,
                            options.mover_is_crusher,
                        )
                    };
                    (!neighbor_passable).then_some(7)
                };
                if let Some(refused_class) = refused_cost_class
                    && refused_class >= 7
                {
                    // Impassable-destination abort. When the goal cell is
                    // refused and the search has reached a cell adjacent to
                    // it, gamemd leaves the main loop immediately and drops
                    // into the success tail — "walk as close to the blocked
                    // target as you can". The tail additionally requires the
                    // aborting node to be at least one real step from the
                    // start, so a start-adjacent blocked goal fails outright.
                    if (nx, ny) == goal && blocked_goal_height_matches(current.height, goal_height)
                    {
                        if c_idx == start_idx && on_bridge == start_on_bridge {
                            return Err(PathSearchFailure::CellSearchExhausted);
                        }
                        // Use the current node's push-time layer flag (same
                        // value came_from was keyed on when pushed).
                        return Ok(reconstruct_path_dual(
                            ground,
                            bridge,
                            start_idx,
                            start_on_bridge,
                            c_idx,
                            on_bridge,
                            w,
                        ));
                    }
                    continue;
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
                let terrain_cost: u8 = if neighbor_use_bridge || is_water_mover {
                    100 // bridge layer or water mover: no terrain cost modifiers
                } else if let Some(cost_grid) = options.terrain_costs {
                    cost_grid.cost_at(nx, ny)
                } else {
                    100 // no cost grid: uniform cost
                };
                // A neighbour the cost class already admitted is NOT re-judged
                // here. Review caught this making the whole wall route inert in
                // production: a non-crushable `Wall=yes` overlay reduces to zone
                // `WALL`, which sets `overlay_blocks`, which `terrain_cost.rs`
                // turns into `COST_BLOCKED` (0) for **every** SpeedType. So the
                // gate below dropped exactly the cells the class had just priced
                // at 60x/20x, a few lines after admitting them, and the only
                // reason the first test passed was that it supplied no cost grid.
                //
                // Native takes one verdict from one slot: `AStar_main_loop`
                // has no second terrain gate at this position, and the land-row
                // check that belongs to this decision is already inside the
                // class - `0x0073FAB5` reads the stored LandType and returns 7
                // for a zero row, which `evaluate_can_enter_cell` models and
                // `the_wall_arm_answers_seven_where_the_land_row_refuses` pins.
                // Applying it twice is what refused the wall.
                if terrain_cost == 0 && refused_cost_class.is_none() {
                    continue;
                }

                // One native class supplies both admission and the edge base.
                // Reduced adapters use None for clear; live Foot returns Some(0).
                let raw_cost_class = refused_cost_class.unwrap_or(0);
                let search_cost = search_cell_cost_decision(raw_cost_class, options.mover_is_train);
                if !search_cost.expands {
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

                // The class's code-2 multiplier; the live receiver walks the
                // chain on the list the class was asked about.
                if let Some(entry) = options.foot_entry
                    && search_cost.effective_cost_class == Some(2)
                {
                    let list = if neighbor_use_bridge {
                        MovementLayer::Bridge
                    } else {
                        MovementLayer::Ground
                    };
                    step_cost *= code2_multiplier(options.urgency, || {
                        entry.code2_chain_jams((nx, ny), list)
                    })
                    .map_err(PathSearchFailure::CellEntryUnavailable)?;
                }

                step_cost = apply_search_marker_cost(step_cost, options.marker_overlay, (nx, ny));

                // Direction tie-breaker
                let tentative_g = current.g_cost + step_cost + DIR_TIEBREAK[dir_index];

                // Node creation. gamemd stamps the selected layer's closed
                // array and records g at this point and never revisits the
                // cell, so g is write-once and there is no relaxation: the
                // route that discovers a cell first owns it. The closed test
                // for this (cell, layer) already ran above.
                let layer = if neighbor_use_bridge {
                    &mut *bridge
                } else {
                    &mut *ground
                };
                layer.set(
                    n_idx,
                    PathDescriptor {
                        parent: encode_from(c_idx, on_bridge),
                        height: neighbor_height,
                    },
                );
                let h = euclidean_heuristic(nx, ny, goal.0, goal.1);
                open.push(Reverse(AStarNode {
                    f_cost: tentative_g + h,
                    g_cost: tentative_g,
                    x: nx,
                    y: ny,
                    height: neighbor_height,
                    on_bridge: neighbor_use_bridge,
                }));
            }

            // Direction 8 is a TubeClass jump. It is not an adjacent neighbor and
            // must not use the normal terrain/height/corner-cut predicates. Auto
            // low-bridge shells have path_len=0 and remain predicate-only.
            if !on_bridge
                && let Some(((nx, ny), path_len)) =
                    explicit_tube_edge(options.resolved_terrain, (cx, cy))
                && nx < grid.width()
                && ny < grid.height()
                && search_cell_allocated(options.resolved_terrain, (nx, ny))
            {
                let n_idx = ny as usize * w + nx as usize;
                if !ground.contains(n_idx) {
                    let neighbor_cell = grid.cell(nx, ny).unwrap_or(&DEFAULT_BLOCKED_CELL);
                    let neighbor_height = neighbor_cell.signed_level();
                    let tube_steps = i32::try_from(path_len).unwrap_or(1).max(1);
                    let tentative_g = current.g_cost + STEP_COST * tube_steps + TUBE_DIR_TIEBREAK;

                    // Same close-on-generation rule as the compass edges.
                    ground.set(
                        n_idx,
                        PathDescriptor {
                            parent: encode_from(c_idx, on_bridge),
                            height: neighbor_height,
                        },
                    );
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

        Err(PathSearchFailure::CellSearchExhausted)
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
    is_cell_passable_for_mover_on_layer_with_speed(
        grid,
        x,
        y,
        MovementLayer::Ground,
        movement_zone,
        None,
        resolved_terrain,
        None,
    )
}

/// Infantry-aware variant of `is_cell_passable_for_mover_on_layer_with_speed`.
///
/// `is_infantry` selects the sub-cell view of terrain-object occupation: retail
/// terrain objects close a cell to infantry only when every functional sub-cell
/// bit is set.
pub fn is_cell_passable_for_category_on_layer(
    grid: &PathGrid,
    x: u16,
    y: u16,
    terrain_layer: MovementLayer,
    movement_zone: Option<MovementZone>,
    speed_type: Option<SpeedType>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    terrain_costs: Option<&TerrainCostGrid>,
    is_infantry: bool,
    mover_is_crusher: bool,
) -> bool {
    evaluate_can_enter_cell(CanEnterCellContext {
        target: (x, y),
        terrain_layer,
        movement_zone,
        speed_type,
        path_grid: Some(grid),
        resolved_terrain,
        terrain_costs,
        is_infantry,
        mover_is_crusher,
    })
    .is_clear()
}

pub fn is_cell_passable_for_mover_on_layer_with_speed(
    grid: &PathGrid,
    x: u16,
    y: u16,
    terrain_layer: MovementLayer,
    movement_zone: Option<MovementZone>,
    speed_type: Option<SpeedType>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    terrain_costs: Option<&TerrainCostGrid>,
) -> bool {
    evaluate_can_enter_cell(CanEnterCellContext {
        target: (x, y),
        terrain_layer,
        movement_zone,
        speed_type,
        path_grid: Some(grid),
        resolved_terrain,
        terrain_costs,
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
    /// Accepted descriptor height, preserved through reconstruction. This is
    /// not necessarily the cell ground level or a resampled bridge layer.
    path_height: i16,
}

impl LayeredPathStep {
    /// Native `42AA90` copies this descriptor's height to its outgoing edge.
    /// Both finishing passes preserve that array even when directions compact.
    pub fn path_height(self) -> i16 {
        self.path_height
    }
}

/// Shared scalar projection used during map construction and synchronous Recalc.
/// Derived path state is separate from structure occupancy and zone history.
fn project_terrain_path_cell(
    cell: &crate::map::resolved_terrain::ResolvedTerrainCell,
) -> (PathCell, u8, bool) {
    let bridge_structural = cell.bridge_facts.has_structural_bridge()
        || (cell.has_bridge_deck
            && !cell.bridge_layer.as_ref().is_some_and(|layer| {
                layer.direction == crate::map::resolved_terrain::BridgeDirection::Low
            })
            && cell.bridge_facts.family == crate::map::bridge_facts::BridgeStampFamily::None);
    // Original47E470 stamps structural side cells without giving each its own
    // overlay (bridge_constructor success25: flags0x11300, overlay-1). Raw100
    // already admits that deck; a live bridge sprite is only a legacy fallback
    // for decks that carry no bit0x100.
    let bridge_intact = cell.bridge_facts.has_structural_bridge()
        || !bridge_structural
        || crate::sim::bridge_state::cell_render_state(cell.bridge_facts).is_some();
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
        bridge_deck_level: cell.bridge_deck_level,
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
            .map_or(0, crate::sim::terrain_object::terrain_raw_occupation_mask),
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
    #[cfg(test)]
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
    #[cfg(test)]
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

    pub fn bridge_deck_level(&self, x: u16, y: u16) -> Option<u8> {
        self.cell(x, y).and_then(PathCell::bridge_deck_level_if_any)
    }

    pub fn can_enter_bridge_layer_from_ground(&self, x: u16, y: u16) -> bool {
        self.cell(x, y)
            .is_some_and(PathCell::can_enter_bridge_layer_from_ground)
    }

    pub fn is_low_bridge_tube_cell(&self, x: u16, y: u16) -> bool {
        self.cell(x, y)
            .is_some_and(PathCell::is_low_bridge_tube_cell)
    }

    /// Find the nearest ground-walkable cell to `(x, y)`, searching in expanding rings.
    #[cfg(test)]
    pub fn nearest_walkable(
        &self,
        x: u16,
        y: u16,
        max_radius: u16,
        entity_blocks: Option<&BTreeSet<(u16, u16)>>,
        allow: Option<(u16, u16)>,
    ) -> Option<(u16, u16)> {
        if self.is_walkable(x, y)
            && (allow == Some((x, y)) || entity_blocks.is_none_or(|b| !b.contains(&(x, y))))
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
                            || entity_blocks.is_none_or(|b| !b.contains(&(cx, cy))))
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
    ) -> Option<(u16, u16)> {
        let check = |cx: u16, cy: u16| self.is_any_layer_walkable(cx, cy);
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

    /// Build from resolved terrain without bridge data.
    pub fn from_resolved_terrain(terrain: &ResolvedTerrainGrid) -> Self {
        Self::from_resolved_terrain_with_bridges(terrain)
    }

    /// Build from resolved terrain with bridge metadata.
    ///
    /// Ground walkability: water cells and bridge deck cells are kept walkable
    /// even when `ground_walk_blocked` is true — SpeedType-dependent blocking
    /// is handled by TerrainCostGrid (cost=0 blocks ground units in A*).
    /// This preserves the behavior of the old flat PathGrid where Float/Hover/
    /// Amphibious units could path through water via cost > 0.
    pub fn from_resolved_terrain_with_bridges(terrain: &ResolvedTerrainGrid) -> Self {
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
                project_terrain_path_cell(cell);
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
        structure_blocked: bool,
    ) -> bool {
        if cell.rx >= self.width || cell.ry >= self.height {
            return false;
        }
        let index = usize::from(cell.ry) * usize::from(self.width) + usize::from(cell.rx);
        let (path_cell, bits, without_terrain) = project_terrain_path_cell(cell);
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
        structure_blocked: bool,
    ) -> bool {
        if cell.rx >= self.width || cell.ry >= self.height {
            return false;
        }
        let index = usize::from(cell.ry) * usize::from(self.width) + usize::from(cell.rx);
        let (mut projected, bits, mut without_terrain) = project_terrain_path_cell(cell);
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
    /// OnBridge byte (`ObjectClass::SetHeight` 0x005F5FA0) — so tests
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
    height: i16,
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
    )
    .ok()?;
    Some(steps.into_iter().map(|s| (s.rx, s.ry)).collect())
}

/// A* pathfinding with optional land-row passability.
///
/// When `costs` is `Some`, a cell whose land row is zero is closed; every
/// non-zero row expands at the same step cost, which is what the native search
/// does (see the provenance in `astar_search`).
#[cfg(test)]
pub fn find_path_with_costs(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    costs: Option<&TerrainCostGrid>,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    mover_is_crusher: bool,
    is_infantry: bool,
) -> Option<Vec<(u16, u16)>> {
    find_path_with_costs_marker(
        grid,
        start,
        goal,
        costs,
        None,
        movement_zone,
        resolved_terrain,
        None,
        MoverSearchFacts {
            urgency: 0,
            mover_is_crusher,
            is_infantry,
            foot_entry: None,
        },
    )
    .ok()
    .map(|steps| steps.into_iter().map(|step| (step.rx, step.ry)).collect())
}

pub(crate) fn find_path_with_costs_marker(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    costs: Option<&TerrainCostGrid>,
    hierarchy_gate: Option<HierarchyGate<'_>>,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    facts: MoverSearchFacts<'_>,
) -> Result<Vec<LayeredPathStep>, PathSearchFailure> {
    astar_search(
        grid,
        start,
        MovementLayer::Ground,
        goal,
        &AStarOptions {
            terrain_costs: costs,
            hierarchy_gate,
            marker_overlay,
            urgency: facts.urgency,
            mover_is_crusher: facts.mover_is_crusher,
            is_infantry: facts.is_infantry,
            foot_entry: facts.foot_entry,
            movement_zone,
            resolved_terrain,
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
#[cfg(test)]
pub fn truncate_path(path: Vec<(u16, u16)>, max_steps: usize) -> Vec<(u16, u16)> {
    let max_len: usize = max_steps + 1; // +1 for start cell at index 0
    if path.len() <= max_len {
        path
    } else {
        path[..max_len].to_vec()
    }
}

/// A path's cells and the movement layer of each cell, index-aligned.
pub type LayeredPath = (Vec<(u16, u16)>, Vec<MovementLayer>);

/// Truncate a layered path (coords + layers) to at most `max_steps` movement steps.
pub fn truncate_layered_path(
    path: Vec<(u16, u16)>,
    layers: Vec<MovementLayer>,
    max_steps: usize,
) -> LayeredPath {
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
#[cfg(test)]
pub fn find_layered_path(
    grid: &PathGrid,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    terrain_costs: Option<&TerrainCostGrid>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    mover_is_crusher: bool,
    is_infantry: bool,
) -> Option<Vec<LayeredPathStep>> {
    // Test-facing wrapper for pure layered pathfinding: no mover zone, so the
    // Foot class arm of the cell predicate is not consulted. Production enters
    // through `zone_search::find_layered_path_zoned_marker_detailed`, which
    // passes the mover's zone to `find_layered_path_marker`.
    find_layered_path_marker(
        grid,
        start,
        start_layer,
        goal,
        terrain_costs,
        None,
        None,
        resolved_terrain,
        None,
        MoverSearchFacts {
            urgency: 0,
            mover_is_crusher,
            is_infantry,
            foot_entry: None,
        },
    )
    .ok()
}

pub(crate) fn find_layered_path_marker(
    grid: &PathGrid,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    terrain_costs: Option<&TerrainCostGrid>,
    hierarchy_gate: Option<HierarchyGate<'_>>,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    facts: MoverSearchFacts<'_>,
) -> Result<Vec<LayeredPathStep>, PathSearchFailure> {
    if !matches!(start_layer, MovementLayer::Ground | MovementLayer::Bridge) {
        return Err(PathSearchFailure::CellSearchExhausted);
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
            hierarchy_gate,
            marker_overlay,
            urgency: facts.urgency,
            mover_is_crusher: facts.mover_is_crusher,
            is_infantry: facts.is_infantry,
            foot_entry: facts.foot_entry,
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

#[cfg(test)]
#[path = "astar_entry_tests.rs"]
mod astar_entry_tests;
