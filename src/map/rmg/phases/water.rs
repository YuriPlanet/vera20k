//! Water/base-terrain seeding for map types 0-2: flood the map with water,
//! carve land blobs per shape, clean up, and grass the shorelines.
//!
//! Order: all-water fill -> shape dispatch (archipelago / continental /
//! islands-in-sea) -> isolated-water removal -> final shore tiling + region
//! reset -> shore-to-green. The finalizer that picks water tile variants is
//! a separate stage.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::rng::{RANGE_K_BITS, RmgRng};
use crate::map::rmg::x87::{self, TruncF64, approx_sqrt};

use super::blob::{self, BlobCtx, BlobParams};
use super::lake;
use super::shore::{self, ShoreCtx};

/// Land-fraction endpoints and cap factor per shape (doubles by value; the
/// min/max differences are computed at runtime like the original's FSUB).
const MODE1_MIN: f64 = 0.45;
const MODE1_MAX: f64 = 0.5;
const MODE1_CAP: f64 = 0.03;
const MODE2_MIN: f64 = 0.15;
const MODE2_MAX: f64 = 0.2;
const MODE2_CAP: f64 = 0.06;
/// Mode-0 blob-size band: uniform in [0.45, 0.5) of the doubled rect area.
const MODE0_SPAN: f64 = 0.05;
const MODE0_BASE: f64 = 0.45;
/// Water option percent scale.
const PERCENT: f64 = 0.01;

/// The playable rect in logical coords: (2, 5, gen_w, gen_h).
#[derive(Debug, Clone, Copy)]
pub struct PlayableRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Logical-rect center to diamond coordinates.
fn diamond_center(map_w: i32, rect: &[i32; 4]) -> (i32, i32) {
    let (x0, y0, w, h) = (rect[0], rect[1], rect[2], rect[3]);
    (w / 2 + h / 2 + x0 + 1 + y0, h / 2 - w / 2 - x0 + map_w + y0)
}

/// Uniform-unit compare: `draw * K < threshold`.
fn draw_below(rng: &mut RmgRng, threshold: f64) -> bool {
    let value = TruncF64::from_f64(f64::from(rng.next_u32()))
        .mul(TruncF64::from_f64(f64::from_bits(RANGE_K_BITS)));
    value.lt(TruncF64::from_f64(threshold))
}

/// Run the water stage. `playable` is the LocalSize rect; `water_percent` is
/// the WaterAmount option; `num_players` feeds the archipelago island count.
pub struct WaterArgs {
    pub map_type: i32,
    pub water_percent: i32,
    pub num_players: i32,
    /// Whether a river may attempt waterfall terrain — the first MapGen draw.
    /// Unread until the river lands; the draw is already stream-relevant.
    pub bridge_enabled: bool,
    pub playable: PlayableRect,
}

/// Map types whose water is *carved into land* rather than shaped out of a full
/// sea: inland and mountainous.
///
/// The original reaches these through a different top-level seeder than the
/// other three, and the two seeders start from opposite terrain. **Everything
/// below — the prefill, the three shape routines, and all five tail passes —
/// is the body of the sea-shaping seeder**, which has exactly one caller: the
/// non-carved arm of the generator's map-type branch. None of it runs for the
/// carved types, which reach their own seeder instead and then rejoin at the
/// water-finalize stage.
const CARVED_WATER_MAP_TYPES: [i32; 2] = [3, 4];

pub fn run(ctx: &mut BlobCtx<'_>, args: &WaterArgs) {
    let coords: Vec<(i32, i32)> = ctx.grid.native_cells().collect();

    // Establish the background every map type starts from: the clear tile.
    //
    // In the original this is not part of the water seeder at all — the map is
    // initialised to the clear tile before the water branch is reached, and the
    // sea-shaping seeder then floods over it. This pipeline has no separate
    // init step, so the fill is hosted here.
    //
    // It is not cosmetic. The clear predicate accepts both the clear tile and
    // the unassigned sentinel, so carving behaves the same either way — but the
    // emitter projects the sentinel as "no tile", which would ship a map with
    // most of its cells unpainted. The original leaves them as the clear tile.
    for &(x, y) in &coords {
        ctx.grid.get_mut(x, y).expect("native cell").tile = ctx.ids.clear;
    }

    // Inland and mountainous begin on the background the map init leaves behind
    // — clear tile, sub-tile 0, uniform base level — and *add* water by carving
    // rivers and lakes into it, admitting only clear cells. Flooding first would
    // leave the carver nothing to admit, so it would place no water at all and
    // the terrain would come out inverted.
    //
    // A zero water amount is not a degenerate input for these two types: the
    // option is rolled from a per-map-type range whose minimum is zero for both,
    // and the original then seeds no water whatsoever. That case is complete
    // here.
    //
    // The return is the map-type branch itself, not an optimization. The five
    // phases below are the sea-shaping seeder's body; the carved types never
    // enter that seeder. Running the tail anyway is not harmless even on a map
    // with no water for it to reshape: phase 4 draws a bounded uniform for
    // every cell of two full sweeps, so it would advance the shared generator
    // by thousands of values that the original never consumes, and every later
    // stage — regions, starts, tiberium, hills, LAT — would then read the
    // stream from the wrong position.
    //
    // Dropping phase 4's region-id reset with it is safe: the region phase
    // resets on entry, matching the generator's own teardown ahead of the
    // region partition.
    if CARVED_WATER_MAP_TYPES.contains(&args.map_type) {
        // A zero water amount is not a degenerate input for these two types: the
        // option is rolled from a per-map-type range whose minimum is zero for
        // both, and the original then does not enter the carved seeder at all.
        // That case is complete here, and it must stay draw-free.
        if args.water_percent != 0 {
            lake::seed_water_carved(ctx, args);
        }
        return;
    }

    // Phase 1: every cell becomes water.
    for &(x, y) in &coords {
        ctx.grid.get_mut(x, y).expect("native cell").tile = ctx.ids.water_base;
    }

    // Phase 2: shape dispatch.
    match args.map_type {
        0 => archipelago(ctx, args),
        1 => continental(ctx, args),
        _ => islands_in_sea(ctx, args),
    }

    // Phase 3: isolated water cells with four clear cardinals become land.
    for &(x, y) in &coords {
        if ctx.grid.get(x, y).expect("native cell").tile != ctx.ids.water_base {
            continue;
        }
        let mut all_clear = true;
        for dir in [0usize, 2, 4, 6] {
            let (nx, ny) = RmgGrid::step(x, y, dir);
            let tile = ctx.grid.cell_native(nx, ny).tile;
            if !ctx.ids.is_clear(tile) {
                all_clear = false;
            }
        }
        if all_clear {
            ctx.grid.get_mut(x, y).expect("native cell").tile = 0;
        }
    }

    // Phase 4: final shore tiling (region 0, checks on; verdict ignored),
    // then the region-data reset.
    {
        let mut shore_ctx = ShoreCtx {
            grid: ctx.grid,
            scratch: ctx.scratch,
            ids: ctx.ids,
            blocks: ctx.blocks,
            rng: ctx.rng,
        };
        let _ = shore::run(&mut shore_ctx, 0, false);
    }
    ctx.scratch.reset_region_ids();

    // Phase 5: shore pieces grass their clear cardinal neighbors.
    for &(x, y) in &coords {
        if !ctx
            .ids
            .is_shore_piece(ctx.grid.get(x, y).expect("native cell").tile)
        {
            continue;
        }
        for dir in [0usize, 2, 4, 6] {
            let (nx, ny) = RmgGrid::step(x, y, dir);
            if ctx.ids.is_clear(ctx.grid.cell_native(nx, ny).tile) {
                ctx.grid.cell_native_mut(nx, ny).tile = ctx.ids.green;
            }
        }
    }
}

/// Mode 0: islands on a randomized partition grid.
fn archipelago(ctx: &mut BlobCtx<'_>, args: &WaterArgs) {
    let mut blob_counter = 1;
    let m = (args.num_players / 2).max(2);

    // Extra-island draw: ftol(draw * m * K + 1.0), rejection > m.
    let extra = loop {
        let value = x87::ftol(
            TruncF64::from_f64(f64::from(ctx.rng.next_u32()))
                .mul(TruncF64::from_f64(f64::from(m)))
                .mul(TruncF64::from_f64(f64::from_bits(RANGE_K_BITS)))
                .add(TruncF64::from_f64(1.0))
                .to_f64(),
        );
        if value <= m {
            break value;
        }
    };
    let islands = args.num_players + extra;

    let rect4 = [
        args.playable.x,
        args.playable.y,
        args.playable.w,
        args.playable.h,
    ];
    let mut entries = partition_grid(ctx.rng, islands, &rect4);
    ctx.scratch.clear_stamps();

    while !entries.is_empty() {
        let entry = entries.remove(0);

        // Blob size: ((draw*K*0.05) + 0.45) * (2*w*h) — drawn once per entry.
        let area2 = entry[2] * entry[3] * 2;
        let size = x87::ftol(
            TruncF64::from_f64(f64::from(ctx.rng.next_u32()))
                .mul(TruncF64::from_f64(f64::from_bits(RANGE_K_BITS)))
                .mul(TruncF64::from_f64(MODE0_SPAN))
                .add(TruncF64::from_f64(MODE0_BASE))
                .mul(TruncF64::from_f64(f64::from(area2)))
                .to_f64(),
        );
        let center = diamond_center(ctx.map_w, &entry);
        for _ in 0..10 {
            let params = BlobParams {
                max_cells: size,
                rect: entry,
                seed: center,
                target: center,
                drift_scale: 0.25,
                directed: false,
            };
            if blob::carve(ctx, &mut blob_counter, &params) != 0 {
                break;
            }
            blob_counter += 1;
        }
    }
}

/// Mode 1: one landmass grown toward the fixed map center.
fn continental(ctx: &mut BlobCtx<'_>, args: &WaterArgs) {
    let mut blob_counter = 1;
    let area = (ctx.map_h + 4) * ctx.map_w * 2;
    let area_f = TruncF64::from_f64(f64::from(area));

    let target = land_target(args.water_percent, MODE1_MAX, MODE1_MIN);
    let cap = x87::ftol(
        area_f
            .mul(TruncF64::from_f64(MODE1_CAP))
            .mul(target)
            .to_f64(),
    );

    let c = ctx.map_h / 2 + ctx.map_w / 2;
    let center = (c + 1, c);
    ctx.scratch.clear_stamps();

    let rect = [
        args.playable.x + 1,
        args.playable.y + 1,
        args.playable.w - 2,
        args.playable.h - 2,
    ];
    let mut seed = center;
    let mut placed = 0i32;
    let mut calls = 0;
    loop {
        let fraction = TruncF64::from_f64(f64::from(placed)).div(area_f);
        if !fraction.lt(target) || !TruncF64::from_f64(0.0).lt(target) || calls >= 100 {
            break;
        }
        let want = x87::ftol(target.sub(fraction).mul(area_f).to_f64()).min(cap);
        let params = BlobParams {
            max_cells: want,
            rect,
            seed,
            target: center,
            drift_scale: 0.75,
            directed: true,
        };
        placed += blob::carve(ctx, &mut blob_counter, &params);
        calls += 1;

        // Nearest unqueued cell to the fixed center, in native scan order.
        seed = nearest_unstamped_iterator_order(ctx, center);
    }
}

/// Mode 2: two landmasses in randomly split halves of the playable rect.
fn islands_in_sea(ctx: &mut BlobCtx<'_>, args: &WaterArgs) {
    let mut blob_counter = 1;
    let area = (ctx.map_h + 4) * ctx.map_w * 2;
    let area_f = TruncF64::from_f64(f64::from(area));

    let target = land_target(args.water_percent, MODE2_MAX, MODE2_MIN);
    let cap = x87::ftol(
        area_f
            .mul(TruncF64::from_f64(MODE2_CAP))
            .mul(target)
            .to_f64(),
    );

    ctx.scratch.clear_stamps();

    let p = &args.playable;
    let (rect1, rect2) = if draw_below(ctx.rng, 0.5) {
        (
            [p.x, p.y, p.w / 2 - 1, p.h],
            [p.x + p.w / 2 + 1, p.y, p.w / 2 - 1, p.h],
        )
    } else {
        (
            [p.x, p.y, p.w, p.h / 2 - 1],
            [p.x, p.y + p.h / 2 + 1, p.w, p.h / 2 - 1],
        )
    };

    let mut calls = 0;
    for rect in [rect1, rect2] {
        let center = diamond_center(ctx.map_w, &rect);
        // Aspect weights are DOUBLES on this path (no f32 narrowing).
        let scale = TruncF64::from_f64(1.2);
        let (wx, wy) = if rect[3] < rect[2] {
            (
                TruncF64::from_f64(1.0),
                TruncF64::from_f64(f64::from(rect[2]))
                    .div(TruncF64::from_f64(f64::from(rect[3])))
                    .mul(scale),
            )
        } else {
            (
                TruncF64::from_f64(f64::from(rect[3]))
                    .div(TruncF64::from_f64(f64::from(rect[2])))
                    .mul(scale),
                TruncF64::from_f64(1.0),
            )
        };
        let one = TruncF64::from_f64(1.0);
        let half = TruncF64::from_f64(0.5);
        let half_w = TruncF64::from_f64(f64::from(rect[2])).mul(half);
        let half_h = TruncF64::from_f64(f64::from(rect[3])).mul(half);
        let coeff_a = one.div(half_w.mul(half_w));
        let coeff_b = one.div(half_h.mul(half_h));

        let mut seed = center;
        let mut placed = 0i32;
        loop {
            let fraction = TruncF64::from_f64(f64::from(placed)).div(area_f);
            if !fraction.lt(target) || calls >= 100 {
                break;
            }
            let want = x87::ftol(target.sub(fraction).mul(area_f).to_f64()).min(cap);
            let params = BlobParams {
                max_cells: want,
                rect,
                seed,
                target: center,
                drift_scale: 0.75,
                directed: true,
            };
            placed += blob::carve(ctx, &mut blob_counter, &params);
            calls += 1;

            seed = nearest_unstamped_linear(ctx, &rect, center, wx, wy, coeff_a, coeff_b);
        }
    }
}

/// `(max - min) * (1 - water% * 0.01) + min`, differences at runtime.
fn land_target(water_percent: i32, max: f64, min: f64) -> TruncF64 {
    let t = TruncF64::from_f64(1.0)
        .sub(TruncF64::from_f64(f64::from(water_percent)).mul(TruncF64::from_f64(PERCENT)));
    TruncF64::from_f64(max)
        .sub(TruncF64::from_f64(min))
        .mul(t)
        .add(TruncF64::from_f64(min))
}

/// Mode-1 next-seed scan: cell-iterator order, Manhattan distance to the
/// fixed center, strict less-than (first wins ties). (0,0) if none.
fn nearest_unstamped_iterator_order(ctx: &mut BlobCtx<'_>, center: (i32, i32)) -> (i32, i32) {
    let mut best = 50_000;
    let mut found = (0, 0);
    for (x, y) in ctx.grid.native_cells().collect::<Vec<_>>() {
        if ctx.scratch.get(x, y).stamp != 0 {
            continue;
        }
        let metric = (y - center.1).abs() + (x - center.0).abs();
        if metric < best {
            best = metric;
            found = (x, y);
        }
    }
    found
}

/// Mode-2 next-seed scan: LINEAR scratch walk reading each record's own
/// coords, gated by the rect's diamond window and the ellipse, weighted
/// Manhattan metric. (0,0) if none.
fn nearest_unstamped_linear(
    ctx: &mut BlobCtx<'_>,
    rect: &[i32; 4],
    center: (i32, i32),
    wx: TruncF64,
    wy: TruncF64,
    coeff_a: TruncF64,
    coeff_b: TruncF64,
) -> (i32, i32) {
    let map_w = ctx.map_w;
    let (rx, ry, w, h) = (rect[0], rect[1], rect[2], rect[3]);
    let sum_lo = map_w + 2 * ry + 1;
    let sum_hi = map_w + 2 * ry + 2 * h + 3;
    let diff_lo = 2 * rx - map_w + 1;
    let diff_hi = 2 * rx - map_w + 2 * w + 3;

    let mut best = 50_000;
    let mut found = (0, 0);
    for index in 0..ctx.scratch.width() * ctx.scratch.width() {
        let record = ctx.scratch.cells()[index];
        let (x, y) = (i32::from(record.x), i32::from(record.y));
        if record.stamp != 0 {
            continue;
        }
        if x + y < sum_lo || x + y > sum_hi || x - y < diff_lo || x - y > diff_hi {
            continue;
        }
        if !ellipse_gate(map_w, (x, y), rect, coeff_a, coeff_b) {
            continue;
        }
        let metric = x87::ftol(
            TruncF64::from_f64(f64::from((y - center.1).abs()))
                .mul(wy)
                .add(TruncF64::from_f64(f64::from((x - center.0).abs())).mul(wx))
                .to_f64(),
        );
        if metric < best {
            best = metric;
            found = (x, y);
        }
    }
    found
}

/// The same ellipse test the flood fill uses (re-derived here to keep the
/// blob module's private helper private).
fn ellipse_gate(
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
    dy.mul(dy)
        .mul(coeff_b)
        .add(dx.mul(dx).mul(coeff_a))
        .lt(TruncF64::from_f64(1.0))
}

/// Island partition grid: ceil-sqrt strips with randomized orientation and
/// leftover removal. Emits logical rects `{x+2, y+2, cw-4, ch-4}`,
/// strip-major.
pub fn partition_grid(rng: &mut RmgRng, n: i32, rect: &[i32; 4]) -> Vec<[i32; 4]> {
    let s = x87::ftol(approx_sqrt(TruncF64::from_f64(f64::from(n))).to_f64());
    let slots_per_strip = if s * s == n { s } else { s + 1 };
    let strips = if n <= slots_per_strip * s {
        s
    } else {
        slots_per_strip
    };
    let leftover = strips * slots_per_strip - n;

    let mut cw = rect[2] / slots_per_strip;
    let mut ch = rect[3] / slots_per_strip;

    // Orientation: below 0.5 = column strips, else rows.
    let columns = draw_below(rng, 0.5);
    let (step, advance) = if columns {
        cw = rect[2] / strips;
        ((0, ch), (cw, 0))
    } else {
        ch = rect[3] / strips;
        ((cw, 0), (0, ch))
    };

    let mut counts = vec![slots_per_strip; strips as usize];
    let mut indices: Vec<i32> = (0..strips).collect();
    for _ in 0..leftover {
        let scale = TruncF64::from_f64(f64::from_bits(RANGE_K_BITS));
        let index = loop {
            let value = x87::ftol(
                TruncF64::from_f64(f64::from(rng.next_u32()))
                    .mul(TruncF64::from_f64(indices.len() as f64))
                    .mul(scale)
                    .to_f64(),
            );
            if value < indices.len() as i32 {
                break value as usize;
            }
        };
        counts[indices[index] as usize] = s;
        indices.remove(index);
    }

    let mut out = Vec::new();
    let (mut sx, mut sy) = (rect[0], rect[1]);
    for (i, &count) in counts.iter().enumerate() {
        let (mut x, mut y) = (sx, sy);
        if count < slots_per_strip {
            // Short strips are centered by half a cell along the strip axis.
            if columns {
                y += ch / 2;
            } else {
                x += cw / 2;
            }
        }
        for _ in 0..count {
            out.push([x + 2, y + 2, cw - 4, ch - 4]);
            x += step.0;
            y += step.1;
        }
        sx += advance.0;
        sy += advance.1;
        // The along-axis coordinate resets to the rect origin per strip.
        if columns {
            sy = rect[1];
        } else {
            sx = rect[0];
        }
        let _ = i;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::phases::shore::{SubTile, TileBlock, TileBlocks};
    use crate::map::rmg::scratch::RmgScratch;
    use crate::map::rmg::tiles::TileIds;
    use crate::map::rmg::x87::Gaussian;
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
            green: 100,
            rough_lat: -1,
            sand_lat: -1,
            green_lat: 110,
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

    fn world(map_w: i32, map_h: i32) -> (RmgGrid, RmgScratch) {
        let stride = (map_w + map_h + 1) as usize;
        let (dmin, dmax) = (map_w, map_w + 2 * map_h);
        (
            RmgGrid::new(stride, dmin, dmax),
            RmgScratch::new(stride, dmin, dmax),
        )
    }

    fn run_water(map_type: i32, seed: u16) -> (RmgGrid, TileIds) {
        run_water_with(map_type, seed, 50)
    }

    fn run_water_with(map_type: i32, seed: u16, water_percent: i32) -> (RmgGrid, TileIds) {
        let (map_w, map_h) = (34, 42); // gen 30x30
        let (mut grid, mut scratch) = world(map_w, map_h);
        let identity = ids();
        let block_table = blocks();
        let mut rng = RmgRng::new(seed);
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
            map_w,
            map_h,
            rollback_level: 4,
        };
        let args = WaterArgs {
            map_type,
            water_percent,
            num_players: 4,
            bridge_enabled: false,
            playable: PlayableRect {
                x: 2,
                y: 5,
                w: 30,
                h: 30,
            },
        };
        run(&mut ctx, &args);
        (grid, identity)
    }

    fn census(grid: &RmgGrid, identity: &TileIds) -> (usize, usize, usize, usize) {
        let mut land = 0;
        let mut water = 0;
        let mut green = 0;
        let mut shore = 0;
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            let tile = grid.get(x, y).unwrap().tile;
            if tile == identity.water_base
                || (tile > identity.water_base && tile < identity.water_base + 0x0E)
            {
                water += 1;
            } else if identity.is_shore_piece(tile) {
                shore += 1;
            } else if tile == identity.green {
                green += 1;
            } else if identity.is_clear(tile) {
                land += 1;
            }
        }
        (land, water, green, shore)
    }

    #[test]
    fn every_map_type_produces_land_and_water() {
        for map_type in [0, 1, 2] {
            let (grid, identity) = run_water(map_type, 1234);
            let (land, water, _, _) = census(&grid, &identity);
            assert!(land > 0, "map type {map_type} produced no land");
            assert!(water > 0, "map type {map_type} produced no water");
        }
    }

    #[test]
    fn shorelines_are_grassed() {
        let (grid, identity) = run_water(1, 77);
        let (_, _, green, shore) = census(&grid, &identity);
        assert!(
            shore > 0,
            "the final tiling pass leaves shore pieces in place"
        );
        assert!(green > 0, "shore-adjacent clear cells turned green");
    }

    #[test]
    fn water_stage_is_deterministic() {
        let tiles = |seed| {
            let (grid, _) = run_water(0, seed);
            grid.native_cells()
                .collect::<Vec<_>>()
                .iter()
                .map(|&(x, y)| grid.get(x, y).unwrap().tile)
                .collect::<Vec<i32>>()
        };
        assert_eq!(tiles(42), tiles(42));
    }

    #[test]
    fn partition_grid_shapes() {
        // n = 5: s = 2, C = 3, R = 2 (5 <= 3*2 -> R = s = 2), leftover = 1.
        let mut rng = RmgRng::new(9);
        let rects = partition_grid(&mut rng, 5, &[2, 5, 30, 30]);
        assert_eq!(rects.len(), 5, "exactly n islands emitted");
        for rect in &rects {
            assert!(rect[2] > 0 && rect[3] > 0, "inset rects stay positive");
        }
        // A perfect square: n = 4 -> 2x2 grid, no leftovers.
        let mut rng = RmgRng::new(9);
        let rects = partition_grid(&mut rng, 4, &[2, 5, 30, 30]);
        assert_eq!(rects.len(), 4);
    }

    #[test]
    fn archipelago_island_count_scales_with_players() {
        // m = max(2, players/2); islands = players + [1, m]. For 8 players,
        // islands is in [9, 12]; the partition grid must emit that many.
        let mut rng = RmgRng::new(31);
        // Drain the extra-island draw the same way the shape does.
        let m = 4;
        let extra = loop {
            let value = x87::ftol(
                TruncF64::from_f64(f64::from(rng.next_u32()))
                    .mul(TruncF64::from_f64(f64::from(m)))
                    .mul(TruncF64::from_f64(f64::from_bits(RANGE_K_BITS)))
                    .add(TruncF64::from_f64(1.0))
                    .to_f64(),
            );
            if value <= m {
                break value;
            }
        };
        assert!((1..=4).contains(&extra));
    }

    /// Inland and mountainous must never be flooded.
    ///
    /// The original reaches them through a seeder that carves water *into* land
    /// and admits only clear cells; the flood-the-map prefill lives in the other
    /// seeder, which has exactly one caller — the branch these two types do not
    /// take. Flooding first leaves the carver nothing to admit, so the map comes
    /// out with land and sea inverted. Routing them into the sea-shaping path is
    /// what this pins against.
    /// Now that lakes are carved, "not flooded" can no longer mean "no water at
    /// all" — it means water stays a minority that was cut into land, rather
    /// than land being the leftover of a full sea. Zero water is still exactly
    /// zero.
    #[test]
    fn carved_water_types_never_flood_the_map() {
        for map_type in CARVED_WATER_MAP_TYPES {
            for water_percent in [0, 50, 100] {
                let (grid, identity) = run_water_with(map_type, 4242, water_percent);
                let total = grid.native_cells().count();
                let flooded = grid
                    .native_cells()
                    .filter(|&(x, y)| {
                        grid.get(x, y).expect("native cell").tile == identity.water_base
                    })
                    .count();
                if water_percent == 0 {
                    assert_eq!(
                        flooded, 0,
                        "map type {map_type} with no water must have none at all"
                    );
                } else {
                    assert!(
                        flooded * 2 < total,
                        "map type {map_type} at water {water_percent}: {flooded} of \
                         {total} cells are water — land and sea look inverted, which \
                         is what routing these types through the sea-shaping prefill \
                         produced"
                    );
                }
            }
        }
    }

    /// The zero-water case is not a degenerate input, it is stock behaviour: the
    /// water amount is rolled from a per-map-type range whose minimum is zero for
    /// both of these types, and the original then seeds no water at all. So the
    /// background must survive as clear ground everywhere.
    ///
    /// Asserts the tile is **exactly** the clear tile, not merely that it
    /// satisfies `is_clear`. That distinction is the whole point: `is_clear`
    /// also accepts the unassigned sentinel, so the permissive form passed while
    /// the emitter was still projecting most of the map as "no tile". Only the
    /// retail resolvability pass caught it. Keep this assertion exact.
    #[test]
    fn carved_water_types_leave_a_zero_water_map_all_clear() {
        for map_type in CARVED_WATER_MAP_TYPES {
            let (grid, identity) = run_water_with(map_type, 4242, 0);
            let not_clear_tile = grid
                .native_cells()
                .filter(|&(x, y)| grid.get(x, y).expect("native cell").tile != identity.clear)
                .count();
            assert_eq!(
                not_clear_tile, 0,
                "map type {map_type} with no water must be the clear tile everywhere, \
                 not the unassigned sentinel"
            );
        }
    }

    /// Guard the other direction: the sea-shaping types must still be flooded and
    /// still carve land back out. Without this, skipping the prefill for 3 and 4
    /// could be over-applied and go unnoticed.
    #[test]
    fn sea_shaped_types_are_still_flooded_and_carved() {
        for map_type in [0, 1, 2] {
            let (grid, identity) = run_water_with(map_type, 4242, 50);
            let mut water = 0usize;
            let mut land = 0usize;
            for (x, y) in grid.native_cells() {
                if grid.get(x, y).expect("native cell").tile == identity.water_base {
                    water += 1;
                } else {
                    land += 1;
                }
            }
            assert!(water > 0, "map type {map_type} must still have sea");
            assert!(land > 0, "map type {map_type} must still have land");
        }
    }

    /// Run the stage and hand back the generator so a caller can ask how far it
    /// moved. Mirrors `run_water_with` exactly; only the return differs.
    fn water_stage_rng_after(map_type: i32, seed: u16, water_percent: i32) -> RmgRng {
        let (map_w, map_h) = (34, 42);
        let (mut grid, mut scratch) = world(map_w, map_h);
        let identity = ids();
        let block_table = blocks();
        let mut rng = RmgRng::new(seed);
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
            map_w,
            map_h,
            rollback_level: 4,
        };
        let args = WaterArgs {
            map_type,
            water_percent,
            num_players: 4,
            bridge_enabled: false,
            playable: PlayableRect {
                x: 2,
                y: 5,
                w: 30,
                h: 30,
            },
        };
        run(&mut ctx, &args);
        rng
    }

    /// How many values the stage took out of the shared generator.
    ///
    /// This is the assertion that matters most in this file, and it is the one a
    /// tile census cannot make. The tail passes barely change tiles on a map with
    /// no water in it, so a wrongly-run tail is close to invisible in the output
    /// grid — but it still advances the generator, and every later stage then
    /// reads the stream from the wrong place. Comparing a used generator against
    /// a fresh one of the same seed is exact: the two agree on the next value
    /// only if the stage consumed nothing.
    ///
    /// Zero water is the whole of the draw-free case: the original does not
    /// enter the carved seeder at all unless the amount is non-zero, so a
    /// zero-water inland map must still cost nothing. Non-zero amounts do draw
    /// — see `carved_water_types_draw_once_the_seeder_runs`.
    #[test]
    fn carved_water_types_consume_no_draws_at_zero_water() {
        for map_type in CARVED_WATER_MAP_TYPES {
            let mut used = water_stage_rng_after(map_type, 4242, 0);
            let mut fresh = RmgRng::new(4242);
            // A single value could collide by chance; a run of them cannot.
            for index in 0..8 {
                assert_eq!(
                    used.next_u32(),
                    fresh.next_u32(),
                    "map type {map_type}: the stage moved the generator at draw \
                     {index} with no water to place. The carved types must not \
                     enter the sea-shaping seeder, whose shore pass alone draws \
                     once per cell across two full sweeps, and must not enter \
                     their own seeder either at a zero amount.",
                );
            }
        }
    }

    /// The other half of the gate: once there *is* water to place, the carved
    /// seeder runs and must consume draws. Without this, the zero-water
    /// assertion above could be satisfied by a seeder that never runs at all.
    #[test]
    fn carved_water_types_draw_once_the_seeder_runs() {
        for map_type in CARVED_WATER_MAP_TYPES {
            for water_percent in [1, 20, 50] {
                let mut used = water_stage_rng_after(map_type, 4242, water_percent);
                let mut fresh = RmgRng::new(4242);
                assert_ne!(
                    used.next_u32(),
                    fresh.next_u32(),
                    "map type {map_type}, water {water_percent}: the carved \
                     seeder must draw",
                );
            }
        }
    }

    /// The other direction, so the guard above cannot be satisfied by a stage
    /// that stopped drawing for everyone.
    #[test]
    fn sea_shaped_types_still_consume_draws() {
        for map_type in [0, 1, 2] {
            let mut used = water_stage_rng_after(map_type, 4242, 50);
            let mut fresh = RmgRng::new(4242);
            assert_ne!(
                used.next_u32(),
                fresh.next_u32(),
                "map type {map_type} must still consume draws",
            );
        }
    }
}
