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
//! gets which random draw, and [`border_cells_of`] keeps that order.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::rng::{RANGE_K_BITS, RmgRng};
use crate::map::rmg::x87::{self, TruncF64};
use crate::util::direction_tables::native_atan_from_table;
use crate::util::native_heap::NativeHeap;
use crate::util::native_x87::X87Chop53;

use super::adjacency::border_cells_of;
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

/// The frontier's order: the nodes' costs, compared as native's `FCOMP` does
/// for finite floats.
fn by_cost(nodes: &[Node]) -> impl Fn(&usize, &usize) -> bool + '_ {
    move |&a, &b| nodes[a].cost < nodes[b].cost
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
///
/// `GrowMeanderArm` computes this inline twice, for the seeds
/// (0x005A0ACA..0x005A0B4F) and the frontier (0x005A0E81..0x005A0F0B), in
/// x87 under chop: `FILD dy; FIDIV dx; FCHS` into `Math::AtanFromTable @
/// 0x004CADE0`, the table lookup every RMG angle shares, then `+π` for a
/// negative `dx` (`π/2` when `dx` is zero), `|· − heading|`, folded by `2π`
/// while not below it and mirrored above `π`. The same angle and cost also
/// sit out of line, unreferenced, at 0x005A1190.
fn angle_off_heading(cell: (i32, i32), reference: (i32, i32), heading: f64) -> f64 {
    let tf = TruncF64::from_f64;
    let dx = cell.0 - reference.0;
    let angle = if dx == 0 {
        tf(HALF_PI)
    } else {
        let ratio = tf(f64::from(cell.1 - reference.1))
            .div(tf(f64::from(dx)))
            .neg();
        let angle = TruncF64::from(native_atan_from_table(ratio.into()));
        if dx < 0 { angle.add(tf(PI)) } else { angle }
    };
    let mut off = angle.sub(tf(heading)).abs();
    while !off.lt(tf(TAU)) {
        off = off.sub(tf(TAU));
    }
    if tf(PI).lt(off) {
        off = tf(TAU).sub(off);
    }
    off.to_f64()
}

/// The frontier priority: a random term plus the off-heading penalty, stored
/// with `FSTP m32` (`GrowMeanderArm` 0x005A0B9F seeds, 0x005A0F4F frontier).
fn frontier_cost(angle: f64, draw: u32) -> f32 {
    let scale = TruncF64::from_f64(f64::from_bits(RANGE_K_BITS));
    let random_term = TruncF64::from_f64(f64::from(draw))
        .mul(scale)
        .mul(TruncF64::from_f64(COST_RANDOM));
    let angle_term = TruncF64::from_f64(angle).mul(TruncF64::from_f64(COST_ANGLE));
    x87::store_f32(random_term.add(angle_term))
}

/// How many cells the arm may claim.
///
/// Transcribed in the original's operation order, which is not the tidy
/// division it is algebraically equal to: the reciprocal is taken, scaled, and
/// then inverted again. Reordering it changes the last bits and so the budget.
///
/// Residual: the native log is `FLDLN2; FILD; FYL2X` under chop (0x005A0C40);
/// [`x87::ln`] is libm's, rounded to nearest. Nothing narrows it here, so a
/// last-bit difference changes the budget only when the result lands within
/// an ulp of an integer.
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

/// `RandomMapGenerator::GrowMeanderArm @ 0x005A08D0`: grow `args.tag`'s
/// region along a wandering heading. Callers: the canyon in `CarveRiver`
/// (0x0059E3C2) and the bridge plateau in `BuildRiverBridge` (0x0059EC65).
///
/// Returns false when the growth ran into a cell owned by a *different* region —
/// that is the original's only failure, and its callers treat it as fatal.
/// Running out of budget, of frontier, or of map is success.
pub(crate) fn grow_meander_arm(ctx: &mut BlobCtx<'_>, args: &MeanderArgs) -> bool {
    let mut heading = heading_from_rect(args.rect);
    let mut alive = true;

    let pool = (args.pool_dims.0 * args.pool_dims.1 * 2).max(MIN_POOL) as usize;
    let mut nodes: Vec<Node> = Vec::new();
    let mut heap = NativeHeap::new(pool);

    // The per-pass stamp is cleared across the whole map first; it is this
    // function's private "already queued" marker.
    let cells: Vec<(i32, i32)> = ctx.grid.native_cells().collect();
    for &(x, y) in &cells {
        ctx.scratch.get_mut(x, y).stamp = 0;
    }

    let border = border_cells_of(ctx.scratch, args.tag);
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
        heap.push(nodes.len() - 1, by_cost(&nodes));
    }

    // The budget scales with the *whole* border, not the part inside the rect.
    let steps = step_budget(border.len(), args.step_density, ctx.rng);

    let mut current = heap.pop(by_cost(&nodes));
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
                    heap.push(nodes.len() - 1, by_cost(&nodes));
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
            // 0x005A1051..0x005A105F: `FMUL π/4; FADD heading` under chop.
            heading = TruncF64::from_f64(ctx.gauss.next(ctx.rng))
                .mul(TruncF64::from_f64(HEADING_DRIFT))
                .add(TruncF64::from_f64(heading))
                .to_f64();
            current = heap.pop(by_cost(&nodes));
            if executed >= steps {
                break;
            }
        }
    }

    if args.claim_frontier {
        // Whatever is still queued is claimed outright — no clear-ground test
        // and no randomness, but a foreign owner still fails the growth.
        let mut node = heap.pop(by_cost(&nodes));
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
            if heap.is_empty() {
                break;
            }
            node = heap.pop(by_cost(&nodes));
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
    let mut frontier = border_cells_of(ctx.scratch, region);
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

    /// Whole original `RandomMapGenerator::GrowMeanderArm @ 0x005A08D0`
    /// calls from `tools/rmg_oracle/meander_arm.py`: a declared flat scene
    /// around a river row (not a full Generate prefix), seeded streams, and
    /// every claimed cell, +4B flag, result and draw the original produced.
    #[test]
    fn native_meander_arm_controls() {
        use crate::map::rmg::scratch::RmgScratch;
        use crate::map::rmg::x87::Gaussian;
        let doc = crate::test_fixture::json("tools/rmg_oracle/meander_arm.json");
        let (identity, blocks) = super::super::cliff_native_tests::physical_tiles();
        let cliff = doc["scene"]["cliff_base"].as_i64().unwrap() as i32;
        let pair =
            |c: &serde_json::Value| (c[0].as_i64().unwrap() as i32, c[1].as_i64().unwrap() as i32);
        let pairs = |value: &serde_json::Value| -> Vec<(i32, i32)> {
            value.as_array().unwrap().iter().map(pair).collect()
        };
        let rows = doc["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 7);
        for row in rows {
            let input = &row["input"];
            let context = input["id"].as_str().unwrap();
            let tag = input["tag"].as_i64().unwrap() as i32;
            let mut grid = RmgGrid::new(33, 16, 48);
            let mut scratch = RmgScratch::new(33, 16, 48);
            for (x, y) in grid.native_cells().collect::<Vec<_>>() {
                let cell = grid.get_mut(x, y).unwrap();
                (cell.tile, cell.sub_tile, cell.level) = (0, 0, 0);
            }
            for (x, y) in pairs(&input["river"]) {
                scratch.get_mut(x, y).region = tag;
            }
            for (x, y) in pairs(&input["foreign"]) {
                scratch.get_mut(x, y).region = 3;
            }
            for (x, y) in pairs(&input["rough"]) {
                if scratch.in_diamond(x, y) {
                    grid.get_mut(x, y).unwrap().tile = cliff;
                }
            }
            let fields = |scratch: &RmgScratch| {
                let mut out = Vec::new();
                for y in 0..33 {
                    for x in 0..33 {
                        if scratch.in_diamond(x, y) {
                            let cell = scratch.get(x, y);
                            out.push([x, y, cell.region, i32::from(cell.water_region)]);
                        }
                    }
                }
                out
            };
            let prior = fields(&scratch);
            let mut rng = RmgRng::new(input["seed"].as_u64().unwrap() as u16);
            let mut gauss = Gaussian::default();
            let rect = &input["rect"];
            let size = pair(&input["gen"]);
            let args = MeanderArgs {
                tag,
                step_density: f32::from_bits(input["density"].as_u64().unwrap() as u32),
                rect: std::array::from_fn(|i| rect[i].as_i64().unwrap() as i32),
                reference: pair(&input["ref"]),
                claim_frontier: input["claim"].as_i64().unwrap() != 0,
                pool_dims: size,
            };
            let ok = {
                let mut ctx = BlobCtx {
                    grid: &mut grid,
                    scratch: &mut scratch,
                    ids: &identity,
                    blocks: &blocks,
                    rng: &mut rng,
                    gauss: &mut gauss,
                    trig: None,
                    map_w: size.0,
                    map_h: size.1,
                    rollback_level: 0,
                };
                grow_meander_arm(&mut ctx, &args)
            };
            assert_eq!(
                ok,
                row["result"].as_u64().unwrap() != 0,
                "{context}: result"
            );
            assert_eq!(
                rng.into_continuation().into_native_parts(),
                super::super::cliff_native_tests::native_state(doc, &row["rng_after"])
                    .into_native_parts(),
                "{context}: exit RNG"
            );
            let cells: Vec<[i32; 4]> = fields(&scratch)
                .into_iter()
                .zip(prior)
                .filter(|(now, before)| now != before)
                .map(|(now, _)| now)
                .collect();
            let native: Vec<[i32; 4]> = row["cells"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| std::array::from_fn(|i| c[i].as_i64().unwrap() as i32))
                .collect();
            assert_eq!(cells, native, "{context}: cells");
        }
    }

    /// `GrowMeanderArm` stores the cost with `FSTP m32` under chop
    /// (0x005A0F4F): `u32::MAX * K * 2` stays just below 2.0.
    #[test]
    fn frontier_cost_store_truncates() {
        assert_eq!(
            frontier_cost(0.0, u32::MAX),
            f32::from_bits(2.0f32.to_bits() - 1)
        );
    }
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
}
