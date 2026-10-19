//! Land-blob flood fill: carves one land blob out of the all-water map.
//!
//! A 1-indexed binary min-heap on float32 keys grows the blob from a seed,
//! with a Gaussian-drifting center, an ellipse constraint, and either a
//! jittered-distance key (undirected) or a target-directed key. The shore
//! tiler then validates the shoreline; on failure the whole blob rolls back
//! to water. Draw accounting: 2 seed draws per attempt (random-seed path
//! only), 1 jitter draw per accepted neighbor (undirected only), 2 Gaussian
//! values per pop (cached-twin semantics), plus the tiler's own draws.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::rng::{RANGE_K_BITS, RmgRng};
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;
use crate::map::rmg::x87::{self, Gaussian, TruncF64, approx_sqrt};
use crate::util::native_x87::X87Chop53;

use super::shore::{self, ShoreCtx, TileBlocks};

/// 5.0 — undirected jitter scale (double).
const JITTER_SCALE: f64 = 5.0;
/// 1.2 — aspect-weight scale (double).
const ASPECT_SCALE: f64 = 1.2;

/// Everything the flood fill borrows from the shape driver.
pub struct BlobCtx<'a> {
    pub grid: &'a mut RmgGrid,
    pub scratch: &'a mut RmgScratch,
    pub ids: &'a TileIds,
    pub blocks: &'a dyn TileBlocks,
    pub rng: &'a mut RmgRng,
    /// The generation-wide Gaussian state — Marsaglia polar, and its cached
    /// twin persists across blobs, so the driver owns it.
    pub gauss: &'a mut Gaussian,
    /// The retail sine table, for the river's heading; `None` only in unit
    /// tests that skip rivers.
    pub trig: Option<&'a crate::map::retail_trig::TrigTable>,
    /// Map rect dims (the +0xF4/+0xF8 pair): seed draws scale by these.
    pub map_w: i32,
    pub map_h: i32,
    /// Level byte written when rolling a blob's cells back to water.
    pub rollback_level: u8,
}

/// One carve attempt's parameters.
#[derive(Debug, Clone, Copy)]
pub struct BlobParams {
    pub max_cells: i32,
    /// Logical region rect (x, y, w, h). Live callers never pass the zero
    /// sentinel, so the ellipse constraint is always active.
    pub rect: [i32; 4],
    /// Seed in diamond coords; (0, 0) selects a random water seed.
    pub seed: (i32, i32),
    /// Directed-growth target in diamond coords.
    pub target: (i32, i32),
    pub drift_scale: f64,
    pub directed: bool,
}

/// Water-family gate: shore pieces, the narrow water set, or the auxiliary
/// waterfall sets (absent from generated maps; see tiles.rs reasoning).
fn water_family(ids: &TileIds, tile: i32) -> bool {
    (ids.shore != -1 && tile >= ids.shore && tile < ids.shore + 0x2A)
        || (ids.water_base != -1 && tile >= ids.water_base && tile < ids.water_base + 0x0E)
}

/// 1-indexed binary min-heap on f32 keys with the original's tie rules.
///
/// Shared with the starts phase: the per-start clearing flood uses the same
/// native heap helpers (including the silent skip-when-full insert).
pub(crate) struct MinHeap {
    /// Slot 0 unused.
    nodes: Vec<(f32, (i32, i32))>,
    cap: usize,
}

impl MinHeap {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            nodes: vec![(0.0, (0, 0))],
            cap,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.nodes.len() - 1
    }

    /// Insert unless full (`count + 1 >= cap` skips silently).
    pub(crate) fn push(&mut self, key: f32, coord: (i32, i32)) {
        if self.len() + 1 >= self.cap {
            return;
        }
        self.nodes.push((key, coord));
        // Sift up: the parent moves down only while strictly greater.
        let mut i = self.len();
        while i > 1 {
            let parent = i / 2;
            if self.nodes[parent].0 > key {
                self.nodes[i] = self.nodes[parent];
                i = parent;
            } else {
                break;
            }
        }
        self.nodes[i] = (key, coord);
    }

    pub(crate) fn pop(&mut self) -> Option<(f32, (i32, i32))> {
        if self.len() == 0 {
            return None;
        }
        let top = self.nodes[1];
        let last = self.nodes.pop().expect("non-empty");
        if self.len() > 0 {
            self.nodes[1] = last;
            // Sift down: a child is chosen only on strict <, left first.
            let mut i = 1;
            loop {
                let (l, r) = (2 * i, 2 * i + 1);
                let mut best = i;
                if l <= self.len() && self.nodes[l].0 < self.nodes[best].0 {
                    best = l;
                }
                if r <= self.len() && self.nodes[r].0 < self.nodes[best].0 {
                    best = r;
                }
                if best == i {
                    break;
                }
                self.nodes.swap(i, best);
                i = best;
            }
        }
        Some(top)
    }
}

/// Draw an index in `[0, dim)` with the seed-selection chain.
///
/// Shared with the lake seeder, which picks its seed the same way.
pub(crate) fn seed_draw(rng: &mut RmgRng, dim: i32) -> i32 {
    let scale = TruncF64::from_f64(f64::from_bits(RANGE_K_BITS));
    let span = TruncF64::from_f64(f64::from(dim));
    loop {
        let draw = TruncF64::from_f64(f64::from(rng.next_u32()));
        let value = X87Chop53::ftol_f64_low_masked(draw.mul(span).mul(scale).to_f64());
        if value < dim {
            return value;
        }
    }
}

/// Ellipse membership test (`0x0059BAB0`, ellipse mode).
fn ellipse_pass(
    map_w: i32,
    cell: (i32, i32),
    rect: &[i32; 4],
    coeff_a: TruncF64,
    coeff_b: TruncF64,
) -> bool {
    let half = TruncF64::from_f64(0.5);
    let t1 = cell.0 - 2 * rect[0] - cell.1 + map_w - 1;
    let t2 = cell.0 - 2 * rect[1] - map_w + cell.1 - 1;
    let dx = TruncF64::from_f64(f64::from(t1))
        .mul(half)
        .sub(TruncF64::from_f64(f64::from(rect[2])).mul(half));
    let dy = TruncF64::from_f64(f64::from(t2))
        .mul(half)
        .sub(TruncF64::from_f64(f64::from(rect[3])).mul(half));
    let sum = dy.mul(dy).mul(coeff_b).add(dx.mul(dx).mul(coeff_a));
    sum.lt(TruncF64::from_f64(1.0))
}

/// Directed-growth key (`0x0059B940`), as an f32.
fn directed_key(
    center: (i32, i32),
    cand: (i32, i32),
    target: (i32, i32),
    weight_x: f32,
    weight_y: f32,
) -> f32 {
    let dlx = (((cand.0 - cand.1) >> 1) - ((target.0 - target.1) >> 1)).abs();
    let dly = (((cand.0 + cand.1) >> 1) - ((target.0 + target.1) >> 1)).abs();
    if dlx == 0 && dly == 0 {
        return 0.0;
    }
    let norm = approx_sqrt(TruncF64::from_f64(f64::from(dly * dly + dlx * dlx)));
    let cheb = (cand.0 - center.0).abs().max((cand.1 - center.1).abs());
    let tmp = TruncF64::from_f64(f64::from(dlx)).div(norm);
    let key = TruncF64::from_f64(f64::from(dly))
        .div(norm)
        .mul(TruncF64::from_f64(f64::from(weight_y)))
        .add(TruncF64::from_f64(f64::from(weight_x)).mul(tmp))
        .mul(TruncF64::from_f64(f64::from(cheb)));
    key.to_f64() as f32
}

/// Carve one blob. Returns the pop count, or 0 on failure/rollback.
///
/// `blob_counter` is the shared blob-id source: the current value tags this
/// blob, and a successful commit increments it here (the failure-path
/// increments belong to the shape drivers).
pub fn carve(ctx: &mut BlobCtx<'_>, blob_counter: &mut i32, params: &BlobParams) -> i32 {
    let blob_id = *blob_counter;
    let max_cells = params.max_cells.max(400);
    let cap = (max_cells * 8 + 2).max(100) as usize;

    // Seed selection (random path only when the caller passes (0,0)).
    let seed = if params.seed == (0, 0) {
        let mut found = None;
        for _ in 0..200 {
            let d1 = seed_draw(ctx.rng, ctx.map_w);
            let d2 = seed_draw(ctx.rng, ctx.map_h);
            let candidate = (d1 + d2 + 1, (ctx.map_w - d1) + d2);
            let free = ctx.scratch.in_diamond(candidate.0, candidate.1)
                && ctx.scratch.get(candidate.0, candidate.1).region == 0;
            let watery = ctx
                .grid
                .get(candidate.0, candidate.1)
                .is_some_and(|cell| water_family(ctx.ids, cell.tile));
            if free && watery {
                found = Some(candidate);
                break;
            }
        }
        match found {
            Some(seed) => seed,
            None => return 0,
        }
    } else {
        params.seed
    };

    // Aspect weights (float32 narrowed) and ellipse coefficients.
    let (rect_w, rect_h) = (params.rect[2], params.rect[3]);
    let scale = TruncF64::from_f64(ASPECT_SCALE);
    let one = TruncF64::from_f64(1.0);
    let (weight_x, weight_y) = if rect_h < rect_w {
        let wy = TruncF64::from_f64(f64::from(rect_w))
            .div(TruncF64::from_f64(f64::from(rect_h)))
            .mul(scale);
        (1.0f32, x87::narrow_to_f32(wy).to_f64() as f32)
    } else {
        let wx = TruncF64::from_f64(f64::from(rect_h))
            .div(TruncF64::from_f64(f64::from(rect_w)))
            .mul(scale);
        (x87::narrow_to_f32(wx).to_f64() as f32, 1.0f32)
    };
    let half = TruncF64::from_f64(0.5);
    let half_w = TruncF64::from_f64(f64::from(rect_w)).mul(half);
    let half_h = TruncF64::from_f64(f64::from(rect_h)).mul(half);
    let coeff_a = one.div(half_w.mul(half_w));
    let coeff_b = one.div(half_h.mul(half_h));

    // Drifting center; seed marked and popped immediately.
    let mut cx = TruncF64::from_f64(f64::from(seed.0));
    let mut cy = TruncF64::from_f64(f64::from(seed.1));
    let drift = TruncF64::from_f64(params.drift_scale);
    ctx.scratch.get_mut(seed.0, seed.1).stamp = blob_id;
    let mut heap = MinHeap::new(cap);
    let mut nodes_used = 1usize; // the seed node
    heap.push(0.0, seed);

    let mut pops = 0i32;
    while pops < max_cells {
        let Some((_, coord)) = heap.pop() else {
            break;
        };
        pops += 1;

        // Commit the popped cell to land.
        ctx.scratch.get_mut(coord.0, coord.1).region = blob_id;
        if let Some(cell) = ctx.grid.get_mut(coord.0, coord.1) {
            cell.tile = 0;
        }

        // Cardinal neighbor scan.
        for dir in [0usize, 2, 4, 6] {
            let (nx, ny) = RmgGrid::step(coord.0, coord.1, dir);
            if !ctx.scratch.in_diamond(nx, ny) {
                continue;
            }
            if ctx.scratch.get(nx, ny).region != 0 || ctx.scratch.get(nx, ny).stamp == blob_id {
                continue;
            }
            let watery = ctx
                .grid
                .get(nx, ny)
                .is_some_and(|cell| water_family(ctx.ids, cell.tile));
            if !watery || nodes_used >= cap {
                continue;
            }
            if !ellipse_pass(ctx.map_w, (nx, ny), &params.rect, coeff_a, coeff_b) {
                continue;
            }
            // Rounded drift center, recomputed per accepted neighbor.
            let rx = X87Chop53::ftol_f64_low_masked(cx.add(half).to_f64());
            let ry = X87Chop53::ftol_f64_low_masked(cy.add(half).to_f64());
            let key = if params.directed {
                directed_key((rx, ry), (nx, ny), params.target, weight_x, weight_y)
            } else {
                let dist2 = (nx - rx) * (nx - rx) + (ny - ry) * (ny - ry);
                let jitter = TruncF64::from_f64(f64::from(ctx.rng.next_u32()))
                    .mul(TruncF64::from_f64(JITTER_SCALE))
                    .mul(TruncF64::from_f64(f64::from_bits(RANGE_K_BITS)));
                approx_sqrt(TruncF64::from_f64(f64::from(dist2)))
                    .add(jitter)
                    .to_f64() as f32
            };
            ctx.scratch.get_mut(nx, ny).stamp = blob_id;
            nodes_used += 1;
            heap.push(key, (nx, ny));
        }

        // Center drift: two Gaussian values per pop.
        let gx = TruncF64::from_f64(self_gauss(ctx));
        cx = cx.add(gx.mul(drift));
        let gy = TruncF64::from_f64(self_gauss(ctx));
        cy = cy.add(gy.mul(drift));
    }

    // Drain: remaining nodes still count as pops; late commits are gated.
    while let Some((_, coord)) = heap.pop() {
        pops += 1;
        let free = ctx.scratch.get(coord.0, coord.1).region == 0;
        let watery = ctx
            .grid
            .get(coord.0, coord.1)
            .is_some_and(|cell| water_family(ctx.ids, cell.tile));
        if free && watery {
            if let Some(cell) = ctx.grid.get_mut(coord.0, coord.1) {
                cell.tile = 0;
            }
            ctx.scratch.get_mut(coord.0, coord.1).region = blob_id;
        }
    }

    // Shore tiling + verdict.
    let ok = {
        let mut shore_ctx = ShoreCtx {
            grid: ctx.grid,
            scratch: ctx.scratch,
            ids: ctx.ids,
            blocks: ctx.blocks,
            rng: ctx.rng,
        };
        shore::run(&mut shore_ctx, blob_id, true)
    };

    // Shore pieces are transient at this stage: reset them either way.
    let coords: Vec<(i32, i32)> = ctx.grid.native_cells().collect();
    for &(x, y) in &coords {
        let tile = ctx.grid.get(x, y).expect("native cell").tile;
        if ctx.ids.shore != -1 && tile >= ctx.ids.shore && tile < ctx.ids.shore + 0x2A {
            let cell = ctx.grid.get_mut(x, y).expect("native cell");
            cell.tile = 0;
            cell.sub_tile = 0;
        }
    }

    if ok {
        *blob_counter += 1;
        return pops;
    }
    // Rollback: re-water everything this blob claimed.
    for &(x, y) in &coords {
        if ctx.scratch.get(x, y).region == blob_id {
            let record = ctx.scratch.get_mut(x, y);
            record.region = 0;
            record.water_region = false;
            let cell = ctx.grid.get_mut(x, y).expect("native cell");
            cell.tile = ctx.ids.water_base;
            cell.sub_tile = 0;
            cell.level = ctx.rollback_level;
        }
    }
    0
}

fn self_gauss(ctx: &mut BlobCtx<'_>) -> f64 {
    ctx.gauss.next(ctx.rng)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::phases::shore::{SubTile, TileBlock};
    use crate::map::theater::TheaterCliffRanges;

    struct OneByOne(TileBlock);

    impl TileBlocks for OneByOne {
        fn block(&self, _tile: i32) -> Option<&TileBlock> {
            Some(&self.0)
        }
    }

    fn blocks() -> OneByOne {
        OneByOne(TileBlock {
            width: 1,
            height: 1,
            subtiles: vec![Some(SubTile {
                height: 0,
                terrain: 0,
                slope: 0,
            })],
        })
    }

    fn ids() -> TileIds {
        TileIds {
            clear: 0,
            ramp_base: -1,
            ramp_smooth: -1,
            slope_set_pieces: -1,
            rough: -1,
            sand: -1,
            green: -1,
            rough_lat: -1,
            sand_lat: -1,
            green_lat: -1,
            pave_lat: -1,
            pave: -1,
            water_base: 500,
            shore: 400,
            water_bridge: -1,
            misc_pave: -1,
            paved_roads: -1,
            paved_road_ends: -1,
            medians: -1,
            cliffs: TheaterCliffRanges::default(),
        }
    }

    /// A water-filled grid mirroring a small generated map: map_w x map_h
    /// rect padded into the diamond band.
    fn water_world(map_w: i32, map_h: i32) -> (RmgGrid, RmgScratch) {
        let stride = (map_w + map_h + 1) as usize;
        let (dmin, dmax) = (map_w, map_w + 2 * map_h);
        let mut grid = RmgGrid::new(stride, dmin, dmax);
        let scratch = RmgScratch::new(stride, dmin, dmax);
        let coords: Vec<(i32, i32)> = grid.native_cells().collect();
        for (x, y) in coords {
            grid.get_mut(x, y).unwrap().tile = 500;
        }
        (grid, scratch)
    }

    #[test]
    fn carving_is_deterministic() {
        let run_once = || {
            let (mut grid, mut scratch) = water_world(30, 24);
            let identity = ids();
            let block_table = blocks();
            let mut rng = RmgRng::new(77);
            let mut gauss = Gaussian::default();
            let trig = crate::map::retail_trig::TrigTable::embedded();
            let mut ctx = BlobCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                blocks: &block_table,
                rng: &mut rng,
                gauss: &mut gauss,
                trig: Some(trig),
                map_w: 30,
                map_h: 24,
                rollback_level: 4,
            };
            let center = (28, 27);
            let params = BlobParams {
                max_cells: 50,
                rect: [3, 3, 24, 18],
                seed: center,
                target: center,
                drift_scale: 0.75,
                directed: true,
            };
            let mut blob = 1;
            let pops = carve(&mut ctx, &mut blob, &params);
            let tiles: Vec<i32> = grid
                .native_cells()
                .collect::<Vec<_>>()
                .iter()
                .map(|&(x, y)| grid.get(x, y).unwrap().tile)
                .collect();
            (pops, tiles)
        };
        assert_eq!(run_once(), run_once());
    }
}
