//! A* pathfinding on the isometric grid.
//!
//! PathGrid stores per-cell walkability and bridge metadata (ground_walkable,
//! bridge_walkable, transition, height levels). A* reads its heights and bridge
//! flags for node heights and closed lists.
//!
//! Every search asks the Foot's Unit/Infantry `+0x1AC` receiver for each
//! neighbor; its native cost class drives expansion and edge cost here. The
//! same admitted search owns its temporary markers through reconstruction and
//! both finishing passes.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on map/ (resolved terrain for walkability).
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use super::native_heap::NativeHeap;
use super::stamped_table::StampedTable;
use super::zone_hierarchy::{MarkedZones, ZoneLevelGraph};
use super::zone_search::PathSearchFailure;
use crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::tube_facts::{TubeId, TubeSource};
use crate::sim::movement::locomotor::MovementLayer;
use crate::util::direction_tables::CELL_DELTAS;
use crate::util::native_x87::{
    NativeF32Bits, NativeF64Bits, X87Chop53, X87Ordering, chop_sum_f32, sqrt_approx_f32,
};
use std::cell::RefCell;
use std::collections::BTreeSet;

/// Accepted native path descriptor (`gamemd.exe 42AA90` reads the parent descriptor's
/// height into each outgoing-height slot). The selected cell/list is stamped
/// closed at generation, so this parent and height are written together once
/// and cannot be replaced while a descendant still points at them. Native execution:
/// `tools/spatial_oracle/astar_path_finishing.{py,json}` (supplied chains).
#[derive(Clone, Copy)]
struct PathDescriptor {
    parent: usize,
    height: i16,
    /// The g its node was created with, which the main loop also keeps per
    /// list (`+0x24` ground, `+0x20` deck; written at `0x0042A125..0x0042A156`)
    /// for its closed-candidate test ([`closed_candidate_retried`]).
    g: f32,
}

impl PathDescriptor {
    const UNVISITED: Self = Self {
        parent: usize::MAX,
        height: 0,
        g: 0.0,
    };
}

/// One layer's closed list: a (cell, layer) is closed in the running search
/// while it holds a descriptor. Closing writes the descriptor
/// (close-on-generation), so a closed cell's parent and height are current.
type ClosedLayer = StampedTable<PathDescriptor>;

/// The open list, unbounded. Residual: `AStar_create_node` takes each node
/// from a pool of 65,536 (`PathfinderClass+0x10`, allocated at `0x0042A7E0`)
/// with no bound check (`0x0042A466..0x0042A479`), so gamemd's 65,537th node
/// lands on the pool's count and the search corrupts memory from there on.
/// The open list's native size, `0x10000` (`0x0042A760`), can only bind after
/// that. VERA creates nodes on demand and bounds neither. Trigger: a search
/// creating more than 65,536 nodes, one per reached cell and layer. Effect:
/// the search goes on where gamemd's corrupts memory. Frequency: searches
/// reaching more than about 65,000 cells, such as long routes on the 500x500
/// scale map (about 125,000 cells). Downstream risk: none within VERA.
type OpenList = NativeHeap<AStarNode, { usize::MAX }>;

/// Reusable scratch for `astar_search`. Lives in a thread-local so repeated
/// repaths share one set of full-map-sized buffers instead of allocating them
/// per call (was the dominant heap pressure source during repath storms —
/// engineer micro, scatter cascades).
///
/// A search touches only the cells it reaches, so `reset` clears nothing: the
/// closed lists forget the previous search at once (see `StampedTable`). The
/// buffers grow when a map is bigger than any before; the open list is
/// cleared but keeps capacity. Use through `with_pathfind_workspace` only; it
/// asserts non-reentrancy so a future recursive A* caller fails loudly instead
/// of silently sharing buffers.
#[derive(Default)]
struct PathfindWorkspace {
    ground: ClosedLayer,
    bridge: ClosedLayer,
    open: OpenList,
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

/// The expansions a search may make. `AStar_main_loop` takes its limit as an
/// argument and resolves a negative one to `0xFFF7` (`0x00429D51..0x00429D59`);
/// both callers (`0x004CBC1E`, `0x00504D9B`) pass -1. The loop tests it
/// before each node's goal test (`0x00429D6D..0x00429D79`).
const MAX_SEARCH_NODES: u32 = 0xFFF7;

/// A route found after exactly this many expansions is refused (`CMP
/// ECX,0x2710` at `0x0042A3E2`), whether at the goal or at the blocked-goal
/// tail.
const REFUSED_EXPANSION_COUNT: u32 = 10_000;

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

    /// Bring the retained wall/Foot part of these counts from `kept` to
    /// `now`, cell by cell, and make `kept` a copy of `now`. The counts wrap,
    /// so this is every delta between them applied at once.
    pub(crate) fn apply_retained_difference(&mut self, kept: &mut [u8], now: &[u8]) {
        assert_eq!(
            (kept.len(), now.len()),
            (self.counts.len(), self.counts.len()),
            "retained wall-neighbor plane must match pathfinding dimensions"
        );
        for ((count, kept), &now) in self.counts.iter_mut().zip(kept.iter_mut()).zip(now) {
            *count = count.wrapping_add(now.wrapping_sub(*kept));
            *kept = now;
        }
    }

    #[cfg(any(test, feature = "test-support"))]
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

/// The direction tiebreaks `AStar_main_loop` adds to each compass edge (`FADD`
/// at `0x00429F96`): the binary32 table at `0x0081872C`, about 0.001 to 0.008.
/// Cardinals get lower values than diagonals, which stops path oscillation
/// when several routes tie. Index order matches [`CELL_DELTAS`]: N, NE, E, SE,
/// S, SW, W, NW.
const DIR_TIEBREAK: [f32; 8] = [
    f32::from_bits(0x3a83_126f), // N   ≈0.001
    f32::from_bits(0x3ba3_d70a), // NE  ≈0.005
    f32::from_bits(0x3b03_126f), // E   ≈0.002
    f32::from_bits(0x3bc4_9ba6), // SE  ≈0.006
    f32::from_bits(0x3b44_9ba6), // S   ≈0.003
    f32::from_bits(0x3be5_6042), // SW  ≈0.007
    f32::from_bits(0x3b83_126f), // W   ≈0.004
    f32::from_bits(0x3c03_126f), // NW  ≈0.008
];

/// The double the closed-candidate test adds to the current g (`FADD` at
/// `0x00429EEC`, constant `0x007E37C0`): 1.009.
const CLOSED_RETRY_MARGIN: NativeF64Bits = NativeF64Bits::from_bits(0x3ff0_24dd_2f1a_9fbe);

/// A compass edge's cost (`0x00429F8A..0x00429F9D`): the class's
/// `AStar_compute_edge_cost` (`0x00429830`) cost, an exact small integer
/// (class base, code-2 override, ×4 marker), times `PathfinderClass+0x04`
/// plus the direction's tiebreak, stored as binary32. `+0x04` is the
/// constructor's 1.0 (`0x0042A6EB`), with no other writer, so its `FMUL`
/// changes nothing. Native controls: astar_structural_height.json
/// `edge_costs`.
fn compass_edge_cost(cost: i32, direction: usize) -> f32 {
    chop_sum_f32(cost as f32, DIR_TIEBREAK[direction])
}

/// A node's g and f (`AStar_create_node @ 0x0042A460`): g adds the edge to
/// the parent's g (`0x0042A529..0x0042A530`) and f adds g to the heuristic
/// (`0x0042A590..0x0042A59E`), each stored as binary32. Native controls:
/// astar_structural_height.json `node_costs`.
fn node_costs(parent_g: f32, edge: f32, cell: (u16, u16), goal: (u16, u16)) -> (f32, f32) {
    let g = chop_sum_f32(edge, parent_g);
    (g, chop_sum_f32(heuristic(cell, goal), g))
}

/// The open list's order: `FCOMP` of two binary32 totals. Nothing else breaks
/// a tie; equal totals keep their place in the heap.
fn cheaper(a: &AStarNode, b: &AStarNode) -> bool {
    a.f_cost < b.f_cost
}

/// A closed candidate is asked about again when the g its node was created
/// with is at least the current node's g plus 1.009, summed on the x87
/// (`0x00429EE2..0x00429F31`). Native controls: astar_structural_height.json
/// `closed_retries`.
fn closed_candidate_retried(stored_g: f32, current_g: f32) -> bool {
    let load = |value: f32| {
        X87Chop53::load_f32(NativeF32Bits::from_bits(value.to_bits())).expect("finite g")
    };
    let margin = X87Chop53::load_f64(CLOSED_RETRY_MARGIN).expect("finite margin");
    X87Chop53::compare(load(stored_g), X87Chop53::add(load(current_g), margin)) != X87Ordering::Less
}

/// The node an expansion makes is held while it is the cheapest so far; a
/// cheaper one takes its place and sends it to the open list
/// (`0x0042A037..0x0042A0CB`, the held child at `[ESP+0x1C]`).
fn offer_child(held: &mut Option<AStarNode>, open: &mut OpenList, child: AStarNode) {
    match held {
        Some(best) if child.f_cost < best.f_cost => {
            open.push(std::mem::replace(best, child), cheaper);
        }
        Some(_) => open.push(child, cheaper),
        None => *held = Some(child),
    }
}

/// The next node to expand (`0x0042A1B3..0x0042A3B5`): the held child, unless
/// the open list's root costs no more, which then gives way to it.
fn next_node(held: Option<AStarNode>, open: &mut OpenList) -> Option<AStarNode> {
    match held {
        Some(best) => match open.first() {
            Some(root) if root.f_cost <= best.f_cost => Some(open.replace_first(best, cheaper)),
            _ => Some(best),
        },
        None => open.pop(cheaper),
    }
}

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

fn encode_from(cell_idx: usize, on_bridge: bool) -> usize {
    cell_idx | if on_bridge { CAME_FROM_BRIDGE } else { 0 }
}

fn decode_from(value: usize) -> (usize, bool) {
    (value & !CAME_FROM_BRIDGE, value & CAME_FROM_BRIDGE != 0)
}

/// The exit of the explicit tube entered at `coord`, the direction-8 candidate.
fn explicit_tube_edge(
    terrain: Option<&ResolvedTerrainGrid>,
    coord: (u16, u16),
) -> Option<(u16, u16)> {
    let terrain = terrain?;
    let tube = terrain.tube_at_cell(coord.0, coord.1)?;
    if tube.source != TubeSource::ExplicitMap || tube.path_len() == 0 || tube.exit == (0, 0) {
        return None;
    }
    Some(tube.exit)
}

/// gamemd tests the actual Cell pointer (`0x00429E19..0x00429E21`) before
/// its layer, zone and +1AC tests, for compass and tube candidates alike;
/// `astar_search` asks its closed lists first. This lookup must not stamp the
/// shared Dummy. Native controls: astar_structural_height.json.
fn search_cell_allocated(terrain: Option<&ResolvedTerrainGrid>, coord: (u16, u16)) -> bool {
    terrain.is_none_or(|terrain| {
        terrain
            .native_fixed_cell_index(coord.0 as i16, coord.1 as i16)
            .is_some()
    })
}

/// Configuration for the unified A* search.
pub struct AStarOptions<'a> {
    /// The mover type's `IsTrain=` flag (`TechnoTypeClass+0xC94`):
    /// `AStar_main_loop` sets its gate byte at `0x00429C79` and then zeroes
    /// every passable `+0x1AC` class (`0x00429F5C..0x00429F69`). Residual: no
    /// mover supplies it (always false), and the search's other train arms
    /// are unported: the start height at `0x00429B64..0x00429BB0`, the start
    /// neighbours closed against the train's facing at
    /// `0x00429C87..0x00429D25`, the hierarchy bypass at `0x0042CACC` and
    /// Find_Path's code-6 destination skip at `0x004D3A66`/`0x004D3AA5`. No
    /// retail type sets `IsTrain=`, so all are dormant.
    pub mover_is_train: bool,
    /// The Foot's `+0x1AC` (`0x00429F54`): the only terrain and object
    /// admission verdict, and the edge's cost class.
    pub foot_entry: &'a dyn SearchFootEntry,
    /// Search-scoped temporary marker overlay equivalent to
    /// `CellClass+0x140 & 0x40000`. Destination hits multiply normal compass
    /// edge cost, but do not change walkability or persistent pathgrid state.
    /// The live Foot search prepares this through the shared
    /// UpdateBridgePassability owner (0x0042ACF0, reached at 0x00429C1A)
    /// after wrapper/zone admission. A* and both finishing passes consume
    /// the transaction before it is discarded ahead of Foot Mark1.
    pub marker_overlay: Option<&'a SearchMarkerOverlay>,
    /// Code-2 urgency escalation (0 = look-ahead chain walk, 1 = traffic penalty,
    /// 2 = route around). Matches gamemd.exe PathfinderClass+0x3C.
    pub urgency: u8,
    /// Binary-style hierarchy marker gate. Present only when blocker-neighbor
    /// counts are also available for the same search.
    pub(crate) hierarchy_gate: Option<HierarchyGate<'a>>,
    /// The map's cells, for the allocated-Cell test and Tube edges.
    pub resolved_terrain: Option<&'a ResolvedTerrainGrid>,
}

impl<'a> AStarOptions<'a> {
    /// A search asking `foot_entry`, with no hierarchy gate, markers,
    /// urgency, train flag or map cells.
    pub fn new(foot_entry: &'a dyn SearchFootEntry) -> Self {
        Self {
            mover_is_train: false,
            foot_entry,
            marker_overlay: None,
            urgency: 0,
            hierarchy_gate: None,
            resolved_terrain: None,
        }
    }
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
    /// Whether the Foot is a Unit (`WhatAmI` 1) whose type is `Passive=`
    /// (`UnitType+0xE0C`), which AStar reads once (`0x00429D2F..0x00429D4C`):
    /// such a search never takes the blocked-goal tail.
    fn is_passive_unit(&self) -> bool {
        false
    }
}

/// The mover facts every search entry carries: `urgency` drives the code-2
/// escalation, and `foot_entry` is the Foot's `+0x1AC` receiver.
///
/// This deliberately excludes `allow_zone_hierarchy` and `playfield_bounds`
/// (hierarchy admission, not per-neighbour cost) and both movement-zone
/// parameters: `mz` selects the zone map/hierarchy to consult, while
/// `movement_zone` is the mover's own zone, and `movement_zone.unwrap_or(mz)`
/// is the documented fallback between them.
#[derive(Clone, Copy)]
pub struct MoverSearchFacts<'a> {
    pub urgency: u8,
    pub foot_entry: &'a dyn SearchFootEntry,
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
    // Start cell may be blocked (e.g. unit standing inside a building footprint
    // after undock). The start node is expanded first without a passability
    // check; only neighbor expansion calls Can_Enter_Cell. If all 8 neighbors
    // are also blocked, the open list stays empty and reports refusal
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
        // node is created before the loop, so it is stamped here, with g 0.
        let start_descriptor = PathDescriptor {
            parent: usize::MAX,
            height: compute_node_height(start_height, None, start_cell),
            g: 0.0,
        };
        if start_on_bridge {
            bridge.set(start_idx, start_descriptor);
        } else {
            ground.set(start_idx, start_descriptor);
        }

        // The start node is expanded first and never enters the open list
        // (created at `0x00429BE6`).
        let mut current = AStarNode {
            f_cost: heuristic(start, goal),
            g_cost: 0.0,
            x: start.0,
            y: start.1,
            height: start_descriptor.height,
            on_bridge: start_on_bridge,
        };
        // The expansions made so far, `[ESP+0x28]`.
        let mut expanded: u32 = 0;
        // `[ESP+0x13]` (`0x00429D2F..0x00429D4C`): the Foot is a Unit whose
        // type is `Passive=`.
        let passive_unit = options.foot_entry.is_passive_unit();

        // --- Main loop ---
        let tail = 'search: loop {
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

            if expanded >= MAX_SEARCH_NODES {
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

            // Goal check: cell AND height must match
            // (`0x00429D7F..0x00429D9B`).
            if (cx, cy) == goal && current.height == goal_height {
                break 'search current;
            }

            let mut held = None;

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
                let n_idx = ny as usize * w + nx as usize;
                let neighbor_cell = grid.cell(nx, ny).unwrap_or(&DEFAULT_BLOCKED_CELL);

                // Closed-list selection (0x00429E54): the CURRENT node's height
                // against the candidate. Foot+1AC's own 0x004D9C60 list choice
                // stays local to the class.
                let neighbor_use_bridge = is_at_bridge_level(current.height, neighbor_cell);
                let closed = if neighbor_use_bridge {
                    bridge.get(n_idx)
                } else {
                    ground.get(n_idx)
                };
                // A closed cell other than the goal never gets a node or ends
                // the route here (the retry below only re-asks gamemd's class
                // and edge cost, a recorded residual), and every test gamemd
                // makes before its closed test only passes a candidate over,
                // so such a cell is passed over first. Most candidates are
                // closed.
                if closed.is_some() && (nx, ny) != goal {
                    continue;
                }
                if !search_cell_allocated(options.resolved_terrain, (nx, ny)) {
                    continue;
                }

                // Compute what height the NEW node carries forward (separate computation)
                let neighbor_height =
                    compute_node_height(current.height, Some(cur_cell), neighbor_cell);

                // Original429E54..429EAF exempts structural deck candidates
                // from the ordinary zone comparison. The signed-height/list
                // predicate is executed in astar_structural_height.json.
                // Residual: HierarchyGate also consults blocker-neighbor counts;
                // full equivalence of that adapter to the original hierarchy
                // comparison is not established by the scalar fragment corpus.
                if let Some(gate) = options.hierarchy_gate
                    && !neighbor_use_bridge
                    && !gate.allows(nx, ny)
                {
                    continue;
                }

                // A closed candidate (`0x00429EC7..0x00429F31`), by now only the
                // goal, is passed over unless its node's g is at least the
                // current g plus 1.009. Then the class and edge cost are asked,
                // but a closed cell never gets a second node
                // (`0x00429FF3..0x0042A018`), so the goal is asked again only
                // for the blocked-goal tail below. Residual: gamemd asks
                // Foot+1AC (`0x00429F54`) and the edge cost (`0x00429F8A`, whose
                // class-2 chain walk looks cells up) about every retried cell
                // and keeps only a refused goal's class. Here only the goal is
                // asked, and only Foot+1AC. The discarded answers' only lasting
                // effect is the shared Dummy cell's coordinate words, which a
                // lookup of an off-map cell stamps. Trigger: a retried closed
                // candidate whose Foot+1AC or chain walk reads an off-map cell.
                // Effect: the Dummy keeps an older stamp. Frequency: only near
                // map edges, the only place a lookup leaves the map. Downstream
                // risk: only a later read of the Dummy's coordinates without its
                // own lookup, as recorded for the batched searches
                // (`movement::path_batch`).
                if closed.is_some_and(|descriptor| {
                    !closed_candidate_retried(descriptor.g, current.g_cost)
                }) {
                    continue;
                }

                // Original429F54 calls the complete Foot+1AC for every reached
                // neighbor; its class is the only terrain and object verdict.
                // In particular, Capture/NavCom may admit a building that the
                // static PathGrid closes. 429FEA tests THIS class before the
                // blocked-goal tail42A17D.
                let cost_class = options
                    .foot_entry
                    .classify(SearchEntryQuery {
                        from: Some((cx, cy)),
                        candidate: SearchEntryCandidate::CopiedCoord((nx, ny)),
                        direction: dir_index as i32,
                        path_height: i32::from(current.height),
                    })
                    .map_err(PathSearchFailure::CellEntryUnavailable)?;
                if cost_class >= 7 {
                    // Impassable-destination abort. When the goal cell is
                    // refused and the search has reached a cell adjacent to
                    // it, gamemd leaves the main loop immediately and drops
                    // into the success tail — "walk as close to the blocked
                    // target as you can". The tail additionally requires the
                    // aborting node to be at least one real step from the
                    // start (depth 2, `0x0042A3F8`), so a start-adjacent
                    // blocked goal fails outright. A `Passive=` Unit never
                    // takes it (`0x0042A183`).
                    if (nx, ny) == goal
                        && !passive_unit
                        && blocked_goal_height_matches(current.height, goal_height)
                    {
                        if c_idx == start_idx && on_bridge == start_on_bridge {
                            return Err(PathSearchFailure::CellSearchExhausted);
                        }
                        break 'search current;
                    }
                    continue;
                }
                if closed.is_some() {
                    continue;
                }

                // A train (`IsTrain=`, gate byte set at `0x00429C79`) takes
                // class 0 for every admitted class (`0x00429F5C..0x00429F69`).
                let cost_class = if options.mover_is_train {
                    0
                } else {
                    cost_class
                };

                // The edge cost (`AStar_compute_edge_cost @ 0x00429830`) is
                // built from this class (base table `0x0081870C`), the code-2
                // blocker override, the `CellClass+0x140 & 0x40000` marker,
                // the dormant `PathfinderClass+0x01` bridge-flank term and the
                // direction tiebreaks at `0x0081872C`. No land row enters it:
                // the classes read the row only as passability (Unit
                // `0x0073FAB5`, Infantry `0x0051C750`), answering 7 for zero.
                //
                // Step cost — uniform across all 8 compass directions.
                // AStar_main_loop checks only the destination through
                // Can_Enter_Cell. PathfinderClass+0x01 is constructor-zero and
                // has no retail writer, so the dormant bridge-flank block does
                // not inspect either cardinal beside a diagonal destination.
                //
                // No height term. `AStar_compute_edge_cost` multiplies by
                // exactly four things — the class base, the code-2
                // blocker-prediction override, `4.0` when the destination
                // carries `CellClass+0x140 & 0x40000` (the search-scoped bridge
                // marker, applied below), and the dormant flank term — and
                // `AStar_main_loop` adds only the direction tiebreak and one
                // `FMUL` by `PathfinderClass+0x04`. Its seven reads of the cell
                // level byte `+0x11B` all feed the bridge/layer selection,
                // never a cost. A ×4 on height change would have made every
                // ramp step cost as much as four flat ones.
                let mut cost = CLASS_BASE[usize::from(cost_class)];

                // The class's code-2 multiplier; the receiver walks the chain
                // on the list the class was asked about.
                if cost_class == 2 {
                    let list = if neighbor_use_bridge {
                        MovementLayer::Bridge
                    } else {
                        MovementLayer::Ground
                    };
                    cost *= code2_multiplier(options.urgency, || {
                        options.foot_entry.code2_chain_jams((nx, ny), list)
                    })
                    .map_err(PathSearchFailure::CellEntryUnavailable)?;
                }

                cost = apply_search_marker_cost(cost, options.marker_overlay, (nx, ny));
                let edge = compass_edge_cost(cost, dir_index);
                let (g_cost, f_cost) = node_costs(current.g_cost, edge, (nx, ny), goal);

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
                        g: g_cost,
                    },
                );
                offer_child(
                    &mut held,
                    open,
                    AStarNode {
                        f_cost,
                        g_cost,
                        x: nx,
                        y: ny,
                        height: neighbor_height,
                        on_bridge: neighbor_use_bridge,
                    },
                );
            }

            // Direction 8 is a TubeClass jump. It is not an adjacent neighbor and
            // must not use the normal terrain/height/corner-cut predicates. Auto
            // low-bridge shells have path_len=0 and remain predicate-only.
            // Residual: gamemd offers the direction-8 candidate whenever the
            // current cell has a tube index (`CellClass+0x116`,
            // `0x00429DCB..0x00429E02`), on either layer, and runs it through
            // the compass arms' layer choice, hierarchy gate, closed test and
            // Foot+1AC with direction 8 (`0x00429F54`), so a class of 7 or
            // more refuses it and a blocked goal can end the route there
            // (`0x00429FEA..0x0042A19B`). Here it is offered only off the
            // bridge layer, never asked of Foot+1AC and tested closed on the
            // ground list only. Trigger: a search reaching a tube entrance.
            // Effect: a tube gamemd refuses (class 7 or more, or outside the
            // corridor) can be taken here, one it offers from a deck-layer
            // node is not, and a blocked goal at a tube exit does not end the
            // route there. Frequency: maps with tubes only. Downstream risk:
            // low; it is one arm of the neighbour loop.
            if !on_bridge
                && let Some((nx, ny)) = explicit_tube_edge(options.resolved_terrain, (cx, cy))
                && nx < grid.width()
                && ny < grid.height()
                && search_cell_allocated(options.resolved_terrain, (nx, ny))
            {
                let n_idx = ny as usize * w + nx as usize;
                if !ground.contains(n_idx) {
                    let neighbor_cell = grid.cell(nx, ny).unwrap_or(&DEFAULT_BLOCKED_CELL);
                    let neighbor_height = neighbor_cell.signed_level();
                    // `0x00429FA3..0x00429FE6`: the Chebyshev cell distance to
                    // the exit, stored as binary32, with no tiebreak or `FMUL`.
                    let edge = f32::from(cx.abs_diff(nx).max(cy.abs_diff(ny)));
                    let (g_cost, f_cost) = node_costs(current.g_cost, edge, (nx, ny), goal);

                    // Same close-on-generation rule as the compass edges.
                    ground.set(
                        n_idx,
                        PathDescriptor {
                            parent: encode_from(c_idx, on_bridge),
                            height: neighbor_height,
                            g: g_cost,
                        },
                    );
                    offer_child(
                        &mut held,
                        open,
                        AStarNode {
                            f_cost,
                            g_cost,
                            x: nx,
                            y: ny,
                            height: neighbor_height,
                            on_bridge: false,
                        },
                    );
                }
            }

            expanded += 1;
            current = next_node(held, open).ok_or(PathSearchFailure::CellSearchExhausted)?;
        };

        if expanded == REFUSED_EXPANSION_COUNT {
            return Err(PathSearchFailure::CellSearchExhausted);
        }
        // Use the tail node's push-time layer flag (same value came_from was
        // keyed on).
        Ok(reconstruct_path_dual(
            ground,
            bridge,
            start_idx,
            start_on_bridge,
            tail.y as usize * w + tail.x as usize,
            tail.on_bridge,
            w,
        ))
    })
}

/// `AStar_compute_edge_cost`'s class bases, the binary32 table at `0x0081870C`
/// (read at `0x00429848`, its only reader), as the integers they are.
const CLASS_BASE: [i32; 8] = [1, 1000, 1, 1, 60, 20, 8, 10_000];

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
fn terrain_object_blocks_infantry(cell_bits: u8) -> bool {
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
/// and height levels. A* reads only the heights and bridge flags, for its node
/// heights and closed lists; whether a cell may be entered is the Foot's
/// `+0x1AC` answer.
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
    /// Every cell's `ground_level` in cell order, kept by each cell writer,
    /// so sight's line of sight borrows the plane
    /// ([`Self::ground_height_grid`]) instead of copying it per reveal.
    ground_levels: Vec<u8>,
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
        // - Water → walkable (the mover's Can_Enter_Cell reads the land row)
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
            // gates ground entry through the mover's SpeedType/LandType
            // row and its bridge entry through Foot `0x004D9C60`, both
            // inside the class's Can_Enter_Cell.
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
            ground_levels: vec![DEFAULT_WALKABLE_CELL.ground_level; size],
        }
    }

    /// Terrain-object sub-cell occupation bits at a cell, in cell-plane form
    /// (`0x04|0x08|0x10`). Zero when the cell holds no terrain object or the
    /// grid carries no terrain-object overlay.
    #[cfg(any(test, feature = "test-support"))]
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

    /// Every cell's ground level in cell order (`y * width + x`).
    ///
    /// A derived value, not persisted simulation state:
    /// `ResolvedTerrainGrid` is the terrain-level owner and `PathGrid` is the
    /// current path/cache projection.
    pub fn ground_height_grid(&self) -> &[u8] {
        debug_assert!(
            self.cells
                .iter()
                .map(|cell| cell.ground_level)
                .eq(self.ground_levels.iter().copied()),
            "the level table matches the cells"
        );
        &self.ground_levels
    }

    /// Mark a cell as blocked (ground layer) or unblocked.
    #[cfg(any(test, feature = "test-support"))]
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
    #[cfg(any(test, feature = "test-support"))]
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
    /// even when `ground_walk_blocked` is true; SpeedType-dependent blocking
    /// belongs to the readers' land rows (a search's Foot `+0x1AC` reads its
    /// own).
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
        let ground_levels = cells.iter().map(|cell| cell.ground_level).collect();
        Self {
            cells,
            width: terrain.width(),
            height: terrain.height(),
            terrain_object_cell_bits,
            ground_walkable_without_terrain_object,
            ground_levels,
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
        self.ground_levels[index] = path_cell.ground_level;
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
    #[cfg(any(test, feature = "test-support"))]
    pub fn from_cells(cells: Vec<PathCell>, width: u16, height: u16) -> Self {
        let ground_levels = cells.iter().map(|cell| cell.ground_level).collect();
        Self {
            cells,
            width,
            height,
            terrain_object_cell_bits: Vec::new(),
            ground_walkable_without_terrain_object: Vec::new(),
            ground_levels,
        }
    }

    /// Test-only helper: directly write a cell's bridge fields.
    #[cfg(any(test, feature = "test-support"))]
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
            self.ground_levels[idx] = ground_level;
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
    #[cfg(any(test, feature = "test-support"))]
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
            self.ground_levels[idx] = ground_level;
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

/// An A* search node: being expanded, held as an expansion's cheapest child,
/// or in the open list.
#[derive(Debug, Clone, Copy)]
struct AStarNode {
    /// Binary32 total: g plus the heuristic. The open list orders by it alone.
    f_cost: f32,
    /// Binary32 cost from the start.
    g_cost: f32,
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

/// The heuristic `AStar_create_node` adds: `Sqrt_Approx` (`0x004CAC40`) of
/// the cell's whole-cell offset to the goal squared and summed in 32-bit
/// integers (`0x0042A54A..0x0042A590`).
///
/// Intentionally **inadmissible** under uniform edge cost: when both dx and
/// dy are nonzero, Euclidean overestimates the true minimum path cost
/// (which is `max(dx, dy)` steps since one diagonal step covers both axes for
/// the same price). The original engine accepts this inadmissibility — A*
/// trades optimality for shorter expansion, and the resulting path geometry
/// is part of the engine's character. The table root never exceeds the exact
/// one.
fn heuristic(cell: (u16, u16), goal: (u16, u16)) -> f32 {
    let dx = i32::from(cell.0) - i32::from(goal.0);
    let dy = i32::from(cell.1) - i32::from(goal.1);
    let squares = dy.wrapping_mul(dy).wrapping_add(dx.wrapping_mul(dx));
    let root = sqrt_approx_f32(X87Chop53::load_i32(squares)).expect("an integer root is finite");
    f32::from_bits(root.bits())
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

/// Test receiver over a bare `PathGrid`: class 0 where the ground plane is
/// walkable, 7 elsewhere, and no threat. A* behaviour tests ask it in place of
/// a Foot; it ports no `Can_Enter_Cell`, which `world::object_entry` owns.
#[cfg(any(test, feature = "test-support"))]
pub(crate) struct WalkableEntry<'a> {
    grid: &'a PathGrid,
    cells: Option<&'a ResolvedTerrainGrid>,
}

#[cfg(any(test, feature = "test-support"))]
impl<'a> WalkableEntry<'a> {
    pub(crate) fn new(grid: &'a PathGrid) -> Self {
        Self { grid, cells: None }
    }

    /// Also answers the retained cells path finishing forwards, by their
    /// coordinates in `cells`.
    pub(crate) fn with_cells(grid: &'a PathGrid, cells: &'a ResolvedTerrainGrid) -> Self {
        Self {
            grid,
            cells: Some(cells),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl SearchFootEntry for WalkableEntry<'_> {
    fn classify(&self, query: SearchEntryQuery) -> Result<u8, String> {
        let (x, y) = match query.candidate {
            SearchEntryCandidate::CopiedCoord(coord) => coord,
            SearchEntryCandidate::RetainedNativeCell(cell) => {
                let cells = self
                    .cells
                    .ok_or("WalkableEntry without cells answers copied coordinates only")?;
                let (x, y) = cells.native_cell_coord(cell);
                (x as u16, y as u16)
            }
        };
        Ok(if self.grid.is_walkable(x, y) { 0 } else { 7 })
    }
    fn finishing_threat_coefficient(
        &self,
    ) -> Result<crate::util::native_x87::NativeF64Bits, String> {
        Ok(crate::util::native_x87::NativeF64Bits::POSITIVE_ZERO)
    }
    fn finishing_house_threat(&self, _coord: (i16, i16)) -> Result<i32, String> {
        Ok(0)
    }
    fn hierarchy_house_threat(&self, _padded_index: i32) -> Result<i32, String> {
        Ok(0)
    }
}

/// A ground search over `grid` asking [`WalkableEntry`]: the cells from
/// start to goal, both inclusive, or `None`.
#[cfg(any(test, feature = "test-support"))]
pub fn find_path(grid: &PathGrid, start: (u16, u16), goal: (u16, u16)) -> Option<Vec<(u16, u16)>> {
    let steps = astar_search(
        grid,
        start,
        MovementLayer::Ground,
        goal,
        &AStarOptions::new(&WalkableEntry::new(grid)),
    )
    .ok()?;
    Some(steps.into_iter().map(|s| (s.rx, s.ry)).collect())
}

pub(crate) fn find_path_marker(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    hierarchy_gate: Option<HierarchyGate<'_>>,
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
            hierarchy_gate,
            marker_overlay,
            urgency: facts.urgency,
            resolved_terrain,
            ..AStarOptions::new(facts.foot_entry)
        },
    )
}

/// Resolve a gamemd foundation name into pathfinding footprint dimensions.
#[cfg(any(test, feature = "test-support"))]
fn parse_foundation(foundation: &str) -> (u16, u16) {
    crate::rules::foundation::foundation_dimensions(foundation)
}

/// Truncate a path to at most `max_steps` movement steps.
///
/// The path includes the start cell at index 0, so a path with `max_steps`
/// movement steps has `max_steps + 1` entries. If the path is already short
/// enough, it is returned unchanged.
#[cfg(any(test, feature = "test-support"))]
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

pub(crate) fn find_layered_path_marker(
    grid: &PathGrid,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    hierarchy_gate: Option<HierarchyGate<'_>>,
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
            resolved_terrain,
            hierarchy_gate,
            marker_overlay,
            urgency: facts.urgency,
            ..AStarOptions::new(facts.foot_entry)
        },
    )
}

// Tests extracted to core_tests.rs to stay under 400 lines.
#[cfg(test)]
#[path = "core_tests.rs"]
mod core_tests;

#[cfg(any(test, feature = "test-support"))]
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
            ground_levels: vec![DEFAULT_WALKABLE_CELL.ground_level; size],
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
            ground_levels: vec![DEFAULT_BLOCKED_CELL.ground_level; size],
        }
    }
}

#[cfg(test)]
#[path = "astar_entry_tests.rs"]
mod astar_entry_tests;
