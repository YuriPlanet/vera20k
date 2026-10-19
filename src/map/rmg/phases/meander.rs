//! Directional region growth — the "meander arm".
//!
//! Given a region that already exists (a carved river), this grows it outward in
//! a chosen direction: it seeds a priority queue from the region's border, then
//! repeatedly claims the cheapest frontier cell, where cost trades a random term
//! against how far the cell sits off the current heading. The heading itself
//! drifts by a Gaussian every step, so the claimed area wanders rather than
//! running straight — hence the name.
//!
//! The active callers are the canyon and the waterfall plateau: the former
//! grows across the whole map, while the latter clamps growth behind the
//! waterfall-terrain crossing.
//!
//! **This arm paints nothing.** It writes only the working grid's region id, its
//! per-pass stamp and the water/region flag. Every tile, level and slope change
//! the canyon produces belongs to the caller.
//!
//! ### The enumeration order is not the usual one
//!
//! Almost every per-cell draw in the generator walks cells in the isometric
//! anti-diagonal order that [`RmgGrid::native_cells`] produces. The border
//! collector this arm seeds from does **not**: it sweeps the working grid's
//! records linearly, so it yields cells in row-major order — ascending `y`, then
//! ascending `x`. That difference is invisible everywhere else it is used,
//! because those callers draw no randomness. Here it decides which border cell
//! gets which random draw, so [`region_border_row_major`] exists to reproduce it
//! rather than reusing the lake's collector.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::rng::{RANGE_K_BITS, RmgRng};
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::x87::{self, TruncF64};
use crate::util::native_x87::X87Chop53;

use super::blob::BlobCtx;

/// The rect edge length that counts as "unclipped" when the heading is derived.
const FULL_EXTENT: i32 = 0x200;

/// Cost weights: a uniform random term, and how far off-heading the cell lies.
const COST_RANDOM: f64 = 2.0;
const COST_ANGLE: f64 = 1.5;

/// Half the step budget is drawn, so the base is halved before the draw.
const STEP_HALF: f64 = 0.5;
/// The step budget's logarithm is floored at one before it is inverted.
const STEP_LOG_FLOOR: f64 = 1.0;

/// Heading drift per step, as a multiple of the Gaussian.
const HEADING_DRIFT: f64 = std::f64::consts::FRAC_PI_4;

const PI: f64 = std::f64::consts::PI;
const TAU: f64 = std::f64::consts::TAU;
const HALF_PI: f64 = std::f64::consts::FRAC_PI_2;
const THREE_HALF_PI: f64 = 4.712_388_980_384_69;

/// The four cardinals, as direction indices. The arm is 4-connected even though
/// the collector that seeds it looks at all eight neighbours.
const CARDINALS: [usize; 4] = [0, 2, 4, 6];

/// Floor on the node pool, below which the original still allocates 100 slots.
const MIN_POOL: i32 = 100;

/// One growth attempt.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MeanderArgs {
    /// Region id being grown; also the value stamped into claimed cells.
    pub tag: i32,
    /// Scales the step budget — smaller means more steps.
    pub step_density: f32,
    /// Clamp rect `(x, y, w, h)`. Also picks the starting heading.
    pub rect: [i32; 4],
    /// The cell angles are measured from.
    pub reference: (i32, i32),
    /// Whether the leftover frontier is claimed once the budget runs out.
    pub claim_frontier: bool,
    /// Generated map dimensions, for the node pool's size.
    pub pool_dims: (i32, i32),
}

/// A frontier candidate: where it is, and what it cost to consider it.
#[derive(Debug, Clone, Copy)]
struct Node {
    coord: (i32, i32),
    cost: f32,
}

/// The original's binary min-heap, reproduced including its tie-breaks.
///
/// Slots are 1-based and hold indices into the node pool. Ties matter: sift-up
/// stops when the parent is *less than or equal*, and the right child only wins
/// on a strict improvement, so equal costs keep the earlier-inserted node above.
struct MinHeap {
    count: usize,
    capacity: usize,
    slots: Vec<usize>,
}

impl MinHeap {
    fn new(capacity: usize) -> Self {
        Self {
            count: 0,
            capacity,
            slots: vec![0; capacity + 2],
        }
    }

    fn push(&mut self, nodes: &[Node], index: usize) {
        let mut slot = self.count + 1;
        // The original drops the insert when the pool is full rather than
        // growing. Verified unreachable for the sizes this generator uses, but
        // dropping silently is what it does, so it is kept.
        if slot >= self.capacity {
            return;
        }
        let cost = nodes[index].cost;
        while slot > 1 {
            let parent = self.slots[slot >> 1];
            if nodes[parent].cost <= cost {
                break;
            }
            self.slots[slot] = parent;
            slot >>= 1;
        }
        self.slots[slot] = index;
        self.count += 1;
    }

    fn pop(&mut self, nodes: &[Node]) -> Option<usize> {
        if self.count == 0 {
            return None;
        }
        let top = self.slots[1];
        self.slots[1] = self.slots[self.count];
        self.slots[self.count] = 0;
        self.count -= 1;
        self.sift_down(nodes, 1);
        Some(top)
    }

    fn sift_down(&mut self, nodes: &[Node], start: usize) {
        let mut parent = start;
        let mut best = self.pick(nodes, parent);
        while best != parent {
            self.slots.swap(parent, best);
            parent = best;
            best = self.pick(nodes, parent);
        }
    }

    /// The smaller of a node and its two children, preferring the parent and
    /// then the left child when costs tie.
    fn pick(&self, nodes: &[Node], parent: usize) -> usize {
        let (left, right) = (parent * 2, parent * 2 + 1);
        let mut best = parent;
        if left <= self.count && nodes[self.slots[left]].cost < nodes[self.slots[parent]].cost {
            best = left;
        }
        if right <= self.count && nodes[self.slots[right]].cost < nodes[self.slots[best]].cost {
            best = right;
        }
        best
    }
}

/// Cells of `region` that touch a differently-owned cell, in row-major order.
///
/// The original sweeps working-grid records by raw offset, and a record's index
/// is `y * width + x`, so this is ascending `y` then ascending `x` — *not* the
/// isometric order the rest of the generator draws in. See the module header.
fn region_border_row_major(scratch: &RmgScratch, region: i32) -> Vec<(i32, i32)> {
    let width = scratch.width() as i32;
    let mut border = Vec::new();
    for y in 0..width {
        for x in 0..width {
            if !scratch.in_diamond(x, y) || scratch.get(x, y).region != region {
                continue;
            }
            for dir in 0..8usize {
                let (nx, ny) = RmgGrid::step(x, y, dir);
                if scratch.in_diamond(nx, ny) && scratch.get(nx, ny).region != region {
                    border.push((x, y));
                    break;
                }
            }
        }
    }
    border
}

/// Where the growth points before the first Gaussian nudges it.
///
/// Whichever rect edge is clipped decides the direction. The `y` test runs
/// second and overwrites the `x` test's answer, so a rect clipped on both axes
/// takes its heading from `y`. A rect covering the whole map leaves the heading
/// at zero — which is the case the canyon uses.
fn heading_from_rect(rect: [i32; 4]) -> f64 {
    let mut heading = 0.0;
    if rect[0] == 0 {
        if rect[2] != FULL_EXTENT {
            heading = PI;
        }
    } else {
        heading = 0.0;
    }
    if rect[1] == 0 {
        if rect[3] != FULL_EXTENT {
            heading = HALF_PI;
        }
    } else {
        heading = THREE_HALF_PI;
    }
    heading
}

/// How far `cell` sits off `heading`, seen from `reference`, folded to `[0, π]`.
fn angle_off_heading(cell: (i32, i32), reference: (i32, i32), heading: f64) -> f64 {
    let dx = cell.0 - reference.0;
    let mut angle = if dx == 0 {
        HALF_PI
    } else {
        let ratio = -(f64::from(cell.1 - reference.1) / f64::from(dx));
        let angle = ratio.atan();
        if dx < 0 { angle + PI } else { angle }
    };
    angle = (angle - heading).abs();
    while angle >= TAU {
        angle -= TAU;
    }
    if angle > PI {
        angle = TAU - angle;
    }
    angle
}

/// The frontier priority: a random term plus the off-heading penalty.
fn frontier_cost(angle: f64, draw: u32) -> f32 {
    let scale = TruncF64::from_f64(f64::from_bits(RANGE_K_BITS));
    let random_term = TruncF64::from_f64(f64::from(draw))
        .mul(scale)
        .mul(TruncF64::from_f64(COST_RANDOM));
    let angle_term = TruncF64::from_f64(angle).mul(TruncF64::from_f64(COST_ANGLE));
    random_term.add(angle_term).to_f64() as f32
}

/// How many cells the arm may claim.
///
/// Transcribed in the original's operation order, which is not the tidy
/// division it is algebraically equal to: the reciprocal is taken, scaled, and
/// then inverted again. Reordering it changes the last bits and so the budget.
fn step_budget(border_total: usize, step_density: f32, rng: &mut RmgRng) -> i32 {
    let log = x87::ln(TruncF64::from_f64(border_total as f64)).to_f64();
    let floored = if STEP_LOG_FLOOR > log {
        STEP_LOG_FLOOR
    } else {
        log
    };
    let mut term = TruncF64::from_f64(STEP_LOG_FLOOR).div(TruncF64::from_f64(floored));
    term = term.mul(TruncF64::from_f64(f64::from(step_density)));
    term = TruncF64::from_f64(STEP_LOG_FLOOR).div(term);
    term = term.mul(TruncF64::from_f64(STEP_HALF));
    let base = X87Chop53::ftol_f64_low_masked(term.to_f64());

    let half = base / 2;
    let scale = TruncF64::from_f64(f64::from_bits(RANGE_K_BITS));
    // The rejection can only fire on the single draw whose scaled value reaches
    // exactly one, so in practice this is one draw — but it is reachable.
    let extra = loop {
        let value = X87Chop53::ftol_f64_low_masked(
            TruncF64::from_f64(f64::from(rng.next_u32()))
                .mul(TruncF64::from_f64(f64::from(half + 1)))
                .mul(scale)
                .to_f64(),
        );
        if value <= half {
            break value;
        }
    };
    base + extra
}

/// Grow `args.tag`'s region along a wandering heading.
///
/// Returns false when the growth ran into a cell owned by a *different* region —
/// that is the original's only failure, and its callers treat it as fatal.
/// Running out of budget, of frontier, or of map is success.
pub(crate) fn grow_meander_arm(ctx: &mut BlobCtx<'_>, args: &MeanderArgs) -> bool {
    let mut heading = heading_from_rect(args.rect);
    let mut alive = true;

    let pool = (args.pool_dims.0 * args.pool_dims.1 * 2).max(MIN_POOL) as usize;
    let mut nodes: Vec<Node> = Vec::new();
    let mut heap = MinHeap::new(pool);

    // The per-pass stamp is cleared across the whole map first; it is this
    // function's private "already queued" marker.
    let cells: Vec<(i32, i32)> = ctx.grid.native_cells().collect();
    for &(x, y) in &cells {
        ctx.scratch.get_mut(x, y).stamp = 0;
    }

    let border = region_border_row_major(ctx.scratch, args.tag);
    for &(x, y) in &border {
        if !in_rect(args.rect, x, y) {
            // Cells outside the rect consume nothing at all.
            continue;
        }
        let angle = angle_off_heading((x, y), args.reference, heading);
        let cost = frontier_cost(angle, ctx.rng.next_u32());
        nodes.push(Node {
            coord: (x, y),
            cost,
        });
        heap.push(&nodes, nodes.len() - 1);
    }

    // The budget scales with the *whole* border, not the part inside the rect.
    let steps = step_budget(border.len(), args.step_density, ctx.rng);

    let mut current = heap.pop(&nodes);
    if steps > 0 {
        let mut executed = 0;
        while let Some(index) = current {
            if !alive {
                break;
            }
            let (cx, cy) = nodes[index].coord;

            // Claim the cell, if it is unowned and still bare ground.
            if ctx.scratch.get(cx, cy).region == 0 {
                let tile = ctx.grid.cell_native(cx, cy).tile;
                if ctx.ids.is_clear(tile) {
                    let cell = ctx.scratch.get_mut(cx, cy);
                    cell.water_region = true;
                    cell.region = args.tag;
                }
            }

            for &dir in &CARDINALS {
                let (nx, ny) = RmgGrid::step(cx, cy, dir);
                if !ctx.scratch.in_diamond(nx, ny) {
                    continue;
                }
                let neighbour = *ctx.scratch.get(nx, ny);
                let free = neighbour.region == 0
                    && neighbour.stamp != args.tag
                    && in_rect(args.rect, nx, ny);
                if free && ctx.ids.is_clear(ctx.grid.cell_native(nx, ny).tile) {
                    let angle = angle_off_heading((nx, ny), args.reference, heading);
                    let cost = frontier_cost(angle, ctx.rng.next_u32());
                    ctx.scratch.get_mut(nx, ny).stamp = args.tag;
                    nodes.push(Node {
                        coord: (nx, ny),
                        cost,
                    });
                    heap.push(&nodes, nodes.len() - 1);
                    continue;
                }
                // Not enqueued: an unowned neighbour is simply skipped, but one
                // owned by somebody else ends the whole growth.
                let owner = ctx.scratch.get(nx, ny).region;
                if owner != 0 && owner != args.tag {
                    alive = false;
                }
            }

            executed += 1;
            // Drawn every step, including the step that failed and the step
            // that emptied the queue — both are only noticed at the loop head.
            heading += ctx.gauss.next(ctx.rng) * HEADING_DRIFT;
            current = heap.pop(&nodes);
            if executed >= steps {
                break;
            }
        }
    }

    if args.claim_frontier {
        // Whatever is still queued is claimed outright — no clear-ground test
        // and no randomness, but a foreign owner still fails the growth.
        let mut node = heap.pop(&nodes);
        while let Some(index) = node {
            if !alive {
                break;
            }
            let (x, y) = nodes[index].coord;
            let owner = ctx.scratch.get(x, y).region;
            if owner != 0 {
                if owner != args.tag {
                    alive = false;
                }
            } else {
                ctx.scratch.get_mut(x, y).region = args.tag;
            }
            if heap.count == 0 {
                break;
            }
            node = heap.pop(&nodes);
        }
    }

    alive
}

fn in_rect(rect: [i32; 4], x: i32, y: i32) -> bool {
    rect[0] <= x && x < rect[0] + rect[2] && rect[1] <= y && y < rect[1] + rect[3]
}

/// Grow `region` outward by `rings`, optionally stamping each newly claimed
/// clear cell to `level`.
///
/// **The frontier chains.** Each ring expands only the cells the previous ring
/// claimed — not a freshly collected border of the whole region. With a single
/// ring the two are the same, which is why re-collecting went unnoticed for as
/// long as it did; from the second ring on, re-collecting also re-expands cells
/// claimed earlier and grows a wider region than the original does.
///
/// Passing a level also widens what counts as claimable: cells belonging to the
/// immediately preceding region are taken as well as unowned ones. The
/// The active waterfall caller carries terrain only. Low-deck overlay
/// construction is owned separately by `bridge_deck` and never enters this arm.
///
/// `rect` clamps which neighbours may be claimed at all: an out-of-rect
/// neighbour is skipped silently, exactly like an out-of-diamond one. The
/// river and canyon pass the whole map, which makes it a no-op there; the
/// waterfall crossing passes the half-plane behind itself.
///
/// Returns false the moment it meets a cell owned by another region. Cells
/// already claimed at that point stay claimed — the original does not unwind
/// either, and its callers roll the whole feature back instead.
pub(crate) fn dilate_chained(
    ctx: &mut BlobCtx<'_>,
    region: i32,
    rings: i32,
    level: Option<u8>,
    rect: [i32; 4],
) -> bool {
    let mut frontier = region_border_row_major(ctx.scratch, region);
    for _ in 0..rings {
        let mut claimed = Vec::new();
        for &(x, y) in &frontier {
            for dir in 0..8usize {
                let (nx, ny) = RmgGrid::step(x, y, dir);
                if !ctx.scratch.in_diamond(nx, ny) || !in_rect(rect, nx, ny) {
                    continue;
                }
                let owner = ctx.scratch.get(nx, ny).region;
                let previous = level.is_some() && owner == region - 1;
                let tile = ctx.grid.cell_native(nx, ny).tile;
                let clear = ctx.ids.is_clear(tile);
                // A previous-region cell may also be water, shore, or a
                // waterfall — that escape lets absorption cross the old river
                // segment rather than failing on its own water family.
                let absorbable = previous && ctx.ids.is_water_shore_or_waterfall(tile);
                if (owner == 0 || previous) && (clear || absorbable) {
                    claimed.push((nx, ny));
                    ctx.scratch.get_mut(nx, ny).region = region;
                    if let Some(level) = level {
                        // Only bare ground takes the stamp; absorbed water
                        // keeps the level it already had.
                        if clear {
                            ctx.grid.cell_native_mut(nx, ny).level = level;
                        }
                    }
                } else if owner != region {
                    return false;
                }
            }
        }
        frontier = claimed;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_map_rect_points_east() {
        // The canyon's rect is unclipped on both axes, so neither test fires
        // and the heading stays at zero. Getting this wrong aims every canyon
        // in the wrong direction.
        assert_eq!(heading_from_rect([0, 0, 0x200, 0x200]), 0.0);
    }

    #[test]
    fn the_y_test_overwrites_the_x_test() {
        // Clipped on both axes: x alone would say pi, but y runs second.
        assert_eq!(heading_from_rect([0, 0, 100, 100]), HALF_PI);
        assert_eq!(heading_from_rect([5, 3, 100, 100]), THREE_HALF_PI);
        // Clipped on x only.
        assert_eq!(heading_from_rect([0, 0, 100, 0x200]), PI);
    }

    #[test]
    fn the_angle_folds_into_half_a_turn() {
        // Whatever the geometry, the penalty is bounded by pi — the fold is
        // what keeps a cell behind the heading from being infinitely costly.
        for dx in -4..=4 {
            for dy in -4..=4 {
                let angle = angle_off_heading((10 + dx, 10 + dy), (10, 10), 0.3);
                assert!((0.0..=PI).contains(&angle), "dx={dx} dy={dy} -> {angle}");
            }
        }
    }

    #[test]
    fn a_column_of_cells_reads_as_a_quarter_turn() {
        // dx == 0 short-circuits to pi/2 rather than dividing by zero.
        assert_eq!(angle_off_heading((10, 3), (10, 10), 0.0), HALF_PI);
        assert_eq!(angle_off_heading((10, 17), (10, 10), 0.0), HALF_PI);
    }

    #[test]
    fn the_step_budget_inverts_twice() {
        // Pinned against the original's operation order, not the algebraically
        // equal single division. With a border of 100 and density 0.01:
        //   ln(100) = 4.605..., 1/4.605... = 0.2171...,
        //   * 0.01 = 0.0021714..., 1/that = 460.51..., * 0.5 = 230.25... -> 230
        let mut rng = RmgRng::new(1);
        let budget = step_budget(100, 0.01, &mut rng);
        assert!(
            (230..=345).contains(&budget),
            "base 230 plus at most half again, got {budget}"
        );
    }

    #[test]
    fn the_budget_floors_the_logarithm_at_one() {
        // A tiny border would otherwise divide by a logarithm below one and
        // hand out a much larger budget than the original does.
        let mut rng = RmgRng::new(1);
        let tiny = step_budget(2, 0.01, &mut rng);
        let mut rng = RmgRng::new(1);
        let unit = step_budget(1, 0.01, &mut rng);
        // ln(2) = 0.693 < 1, so both floor to 1 and give the same base of 50.
        assert_eq!(tiny, unit);
    }

    #[test]
    fn the_heap_pops_in_cost_order() {
        let nodes = vec![
            Node {
                coord: (0, 0),
                cost: 3.0,
            },
            Node {
                coord: (1, 0),
                cost: 1.0,
            },
            Node {
                coord: (2, 0),
                cost: 2.0,
            },
            Node {
                coord: (3, 0),
                cost: 0.5,
            },
        ];
        let mut heap = MinHeap::new(64);
        for i in 0..nodes.len() {
            heap.push(&nodes, i);
        }
        let mut order = Vec::new();
        while let Some(i) = heap.pop(&nodes) {
            order.push(nodes[i].coord.0);
        }
        assert_eq!(order, vec![3, 1, 2, 0]);
    }

    #[test]
    fn equal_costs_keep_their_insertion_order() {
        // The original's sift-up stops on parent <= child, so a later insert
        // never overtakes an equal-cost earlier one.
        let nodes: Vec<Node> = (0..6)
            .map(|i| Node {
                coord: (i, 0),
                cost: 1.0,
            })
            .collect();
        let mut heap = MinHeap::new(64);
        for i in 0..nodes.len() {
            heap.push(&nodes, i);
        }
        assert_eq!(heap.pop(&nodes).map(|i| nodes[i].coord.0), Some(0));
    }
}
