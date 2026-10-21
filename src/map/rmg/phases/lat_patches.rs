//! LAT terrain patches: scatters rough / sand / green ground patches over the
//! clear cells, using per-cell probabilities seeded from the Vegetation option.
//!
//! Temperate theaters (`theater == 0`) place all three patch types; every other
//! theater places only rough patches (through the sand-probability slot). Each
//! patch is a min-heap blob of a Gaussian-sized cell count. The base tile is
//! stamped here; the LAT auto-tiling fixup (a separate, RNG-free pass) resolves
//! the transition variants afterwards.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::rng::{RANGE_K_BITS, RmgRng};
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;
use crate::map::rmg::x87::{Gaussian, TruncF64, approx_sqrt};
use crate::util::native_x87::X87Chop53;

use super::blob::{CellHeap, by_key};

/// Vegetation percent scale and the per-type base probabilities.
const VEG_SCALE: f64 = 0.01;
const ROUGH_FACTOR: f64 = 0.02;
const SHORE_ROUGH_MULT: f64 = 10.0;
const SAND_PROB: f64 = 0.005;
const GREEN_PROB_TEMPERATE: f64 = 0.005;
const GREEN_PROB_OTHER: f64 = 0.001;
/// Patch-size mean draw: `ftol(rand * ~21·2⁻³² + 20)`, rejected above 40.
const MEAN_K_BITS: u64 = 0x3E35_0000_0015_0000;
const MEAN_OFFSET: f64 = 20.0;
const MEAN_MAX: i32 = 40;
/// Patch-size Gaussian: `mean + N·20` clamped `[4, 80]` (temperate) or
/// `20 + N·15` clamped `[4, 60]` (other theaters).
const SIZE_SCALE_TEMPERATE: f64 = 20.0;
const SIZE_SCALE_OTHER: f64 = 15.0;
const SIZE_BASE_OTHER: f64 = 20.0;
const SIZE_MIN: f64 = 4.0;
const SIZE_MAX_TEMPERATE: f64 = 80.0;
const SIZE_MAX_OTHER: f64 = 60.0;
/// Patch-placer priority jitter, `rand * 5·2⁻³²`.
const JITTER: f64 = 5.0;
/// Heap headroom over the target size (the frontier never approaches this).
const HEAP_HEADROOM: usize = 512;

/// Everything the phase borrows.
pub struct PatchCtx<'a> {
    pub grid: &'a mut RmgGrid,
    pub scratch: &'a mut RmgScratch,
    pub ids: &'a TileIds,
    pub rng: &'a mut RmgRng,
    pub gauss: &'a mut Gaussian,
}

fn t(value: f64) -> TruncF64 {
    TruncF64::from_f64(value)
}

/// Seed per-cell patch probabilities and paint patches. `theater == 0` is
/// temperate (rough/sand/green); any other theater paints rough only.
pub fn run(ctx: &mut PatchCtx<'_>, theater: i32, vegetation: i32) {
    let veg = t(f64::from(vegetation)).mul(t(VEG_SCALE));
    if theater == 0 {
        prob_setup_temperate(ctx, veg);
        paint_temperate(ctx);
    } else {
        prob_setup_other(ctx);
        paint_other(ctx);
    }
}

/// Temperate probabilities: shore-piece cells (with any in-diamond 5x5
/// neighbour) get a heavy rough bias and no green; other cells get the base
/// mix.
fn prob_setup_temperate(ctx: &mut PatchCtx<'_>, veg: TruncF64) {
    let rough = veg.mul(t(ROUGH_FACTOR)).to_f64();
    let shore_rough = veg.mul(t(ROUGH_FACTOR)).mul(t(SHORE_ROUGH_MULT)).to_f64();
    for (x, y) in ctx.grid.native_cells().collect::<Vec<_>>() {
        let tile = ctx.grid.get(x, y).expect("native cell").tile;
        if ctx.ids.is_shore_piece(tile) && any_5x5_in_diamond(ctx.scratch, x, y) {
            let record = ctx.scratch.get_mut(x, y);
            record.p_sand = SAND_PROB;
            record.p_green = 0.0;
            record.p_rough = shore_rough;
        } else if ctx.scratch.get(x, y).p_sand == 0.0 {
            let record = ctx.scratch.get_mut(x, y);
            record.p_sand = SAND_PROB;
            record.p_rough = rough;
            record.p_green = GREEN_PROB_TEMPERATE;
        }
    }
}

/// Non-temperate probabilities: uniform sand slot 0.005, green 0.001 (written
/// but never read on this path).
fn prob_setup_other(ctx: &mut PatchCtx<'_>) {
    for (x, y) in ctx.grid.native_cells().collect::<Vec<_>>() {
        let record = ctx.scratch.get_mut(x, y);
        record.p_sand = SAND_PROB;
        record.p_green = GREEN_PROB_OTHER;
    }
}

/// Whether any cell in the 5x5 neighbourhood of `(x, y)` is in the diamond.
fn any_5x5_in_diamond(scratch: &RmgScratch, x: i32, y: i32) -> bool {
    (-2..=2).any(|dy| (-2..=2).any(|dx| scratch.in_diamond(x + dx, y + dy)))
}

/// The temperate painter: three size means, then per clear cell a sequential
/// rough/sand/green probability test with fresh draws.
fn paint_temperate(ctx: &mut PatchCtx<'_>) {
    ctx.scratch.clear_stamps();
    let mean_rough = mean_draw(ctx.rng);
    let mean_sand = mean_draw(ctx.rng);
    let mean_green = mean_draw(ctx.rng);

    for (x, y) in ctx.grid.native_cells().collect::<Vec<_>>() {
        if !clear_paintable(ctx, x, y, true) {
            continue;
        }
        let record = *ctx.scratch.get(x, y);
        // Fresh draws, short-circuiting on the first hit.
        let (tile, mean) = if ctx.rng.next_unit() < record.p_rough {
            (ctx.ids.rough, mean_rough)
        } else if ctx.rng.next_unit() < record.p_sand {
            (ctx.ids.sand, mean_sand)
        } else if ctx.rng.next_unit() < record.p_green {
            (ctx.ids.green, mean_green)
        } else {
            continue;
        };
        let size = patch_size(
            ctx.gauss,
            ctx.rng,
            mean,
            SIZE_SCALE_TEMPERATE,
            SIZE_MAX_TEMPERATE,
        );
        place_patch(ctx, (x, y), tile, size, patch_id(x, y));
    }
}

/// The non-temperate painter: one draw per clear cell against the sand slot,
/// placing a rough patch.
fn paint_other(ctx: &mut PatchCtx<'_>) {
    ctx.scratch.clear_stamps();
    for (x, y) in ctx.grid.native_cells().collect::<Vec<_>>() {
        if !clear_paintable(ctx, x, y, false) {
            continue;
        }
        let threshold = ctx.scratch.get(x, y).p_sand;
        if ctx.rng.next_unit() >= threshold {
            continue;
        }
        let size = patch_size_other(ctx.gauss, ctx.rng);
        place_patch(ctx, (x, y), ctx.ids.rough, size, patch_id(x, y));
    }
}

/// A cell is paintable when it is clear, flat, and unprotected. Temperate also
/// requires no overlay and no occupier; other theaters skip those two checks.
fn clear_paintable(ctx: &PatchCtx<'_>, x: i32, y: i32, temperate: bool) -> bool {
    let Some(cell) = ctx.grid.get(x, y) else {
        return false;
    };
    if !ctx.ids.is_clear(cell.tile) || cell.slope != 0 || ctx.scratch.get(x, y).water_lock {
        return false;
    }
    if temperate && (cell.overlay != -1 || cell.occupied) {
        return false;
    }
    true
}

/// The patch id for a cell — the packed `y*512 + x` the original uses.
fn patch_id(x: i32, y: i32) -> i32 {
    y * 0x200 + x
}

/// One size-mean draw: `ftol(rand * ~21·2⁻³² + 20)`, rejected above 40.
fn mean_draw(rng: &mut RmgRng) -> i32 {
    let scale = t(f64::from_bits(MEAN_K_BITS));
    let offset = t(MEAN_OFFSET);
    loop {
        let value = X87Chop53::ftol_f64_low_masked(
            t(f64::from(rng.next_u32())).mul(scale).add(offset).to_f64(),
        );
        if value <= MEAN_MAX {
            return value;
        }
    }
}

/// Temperate patch size: `mean + N·20` rejection-clamped to `[4, 80]`.
fn patch_size(gauss: &mut Gaussian, rng: &mut RmgRng, mean: i32, scale: f64, max: f64) -> i32 {
    loop {
        let value = t(gauss.next(rng))
            .mul(t(scale))
            .add(t(f64::from(mean)))
            .to_f64();
        if (SIZE_MIN..=max).contains(&value) {
            return X87Chop53::ftol_f64_low_masked(value);
        }
    }
}

/// Non-temperate patch size: `20 + N·15` rejection-clamped to `[4, 60]`.
fn patch_size_other(gauss: &mut Gaussian, rng: &mut RmgRng) -> i32 {
    loop {
        let value = t(gauss.next(rng))
            .mul(t(SIZE_SCALE_OTHER))
            .add(t(SIZE_BASE_OTHER))
            .to_f64();
        if (SIZE_MIN..=SIZE_MAX_OTHER).contains(&value) {
            return X87Chop53::ftol_f64_low_masked(value);
        }
    }
}

/// Grow a patch of `size` cells from `origin`: a closest-first min-heap blob
/// that stamps the base `tile` on each popped cell and admits clear, flat,
/// unclaimed 8-neighbours (one jitter draw per admitted neighbour).
fn place_patch(ctx: &mut PatchCtx<'_>, origin: (i32, i32), tile: i32, size: i32, patch_id: i32) {
    if size <= 0 {
        return;
    }
    let cap = size as usize + HEAP_HEADROOM;
    let mut heap = CellHeap::new(cap);
    ctx.scratch.get_mut(origin.0, origin.1).stamp = patch_id;
    heap.push((0.0, origin), by_key);

    let mut placed = 0;
    while placed < size {
        let Some((_, (x, y))) = heap.pop(by_key) else {
            break;
        };
        if let Some(cell) = ctx.grid.get_mut(x, y) {
            cell.tile = tile;
        }
        placed += 1;
        for dir in 0..8 {
            let (nx, ny) = RmgGrid::step(x, y, dir);
            if !ctx.scratch.in_diamond(nx, ny) {
                continue;
            }
            let admit = ctx.grid.get(nx, ny).is_some_and(|cell| {
                ctx.ids.is_clear(cell.tile)
                    && cell.slope == 0
                    && cell.overlay == -1
                    && !cell.occupied
            }) && ctx.scratch.get(nx, ny).stamp != patch_id;
            if !admit {
                continue;
            }
            let dx = nx - origin.0;
            let dy = ny - origin.1;
            let jitter = t(f64::from(ctx.rng.next_u32()))
                .mul(t(JITTER))
                .mul(t(f64::from_bits(RANGE_K_BITS)));
            let priority = approx_sqrt(t(f64::from(dx * dx + dy * dy)))
                .add(jitter)
                .to_f64() as f32;
            ctx.scratch.get_mut(nx, ny).stamp = patch_id;
            heap.push((priority, (nx, ny)), by_key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::tiles::TILE_UNASSIGNED;
    use crate::map::theater::TheaterCliffRanges;

    fn ids() -> TileIds {
        TileIds {
            clear: 0,
            ramp_base: 600,
            ramp_smooth: 620,
            slope_set_pieces: -1,
            rough: 700,
            sand: 800,
            green: 100,
            rough_lat: 710,
            sand_lat: 810,
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

    fn world() -> (RmgGrid, RmgScratch) {
        let (map_w, map_h) = (30, 26);
        let stride = (map_w + map_h + 1) as usize;
        let (dmin, dmax) = (map_w, map_w + 2 * map_h);
        let mut grid = RmgGrid::new(stride, dmin, dmax);
        let scratch = RmgScratch::new(stride, dmin, dmax);
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            grid.get_mut(x, y).unwrap().tile = 0;
        }
        (grid, scratch)
    }

    fn run_theater(seed: u16, theater: i32, veg: i32) -> (RmgGrid, RmgScratch, RmgRng) {
        let (mut grid, mut scratch) = world();
        let identity = ids();
        let mut rng = RmgRng::new(seed);
        let mut gauss = Gaussian::default();
        {
            let mut ctx = PatchCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                rng: &mut rng,
                gauss: &mut gauss,
            };
            run(&mut ctx, theater, veg);
        }
        (grid, scratch, rng)
    }

    #[test]
    fn temperate_paints_rough_sand_and_green_patches() {
        let (grid, _scratch, _rng) = run_theater(1234, 0, 100);
        let mut kinds = std::collections::HashSet::new();
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            let tile = grid.get(x, y).unwrap().tile;
            if tile == 700 {
                kinds.insert("rough");
            } else if tile == 800 {
                kinds.insert("sand");
            } else if tile == 100 {
                kinds.insert("green");
            }
        }
        // With high vegetation the map gets a mix of patch types.
        assert!(kinds.contains("rough"), "rough patches placed");
        assert!(kinds.len() >= 2, "more than one patch type appears");
    }

    #[test]
    fn non_temperate_paints_only_rough() {
        let (grid, _scratch, _rng) = run_theater(77, 1, 100);
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            let tile = grid.get(x, y).unwrap().tile;
            assert!(
                tile == 0 || tile == 700 || tile == TILE_UNASSIGNED,
                "({x},{y}) is clear or rough only, got {tile}"
            );
        }
    }

    #[test]
    fn patches_only_land_on_flat_clear_ground() {
        let (mut grid, mut scratch) = world();
        let identity = ids();
        // A sloped cell and a water cell must never be overwritten.
        grid.get_mut(30, 26).unwrap().slope = 3;
        grid.get_mut(28, 24).unwrap().tile = 500;
        let (slope_tile, water_tile) = (
            grid.get(30, 26).unwrap().tile,
            grid.get(28, 24).unwrap().tile,
        );
        let mut rng = RmgRng::new(9);
        let mut gauss = Gaussian::default();
        {
            let mut ctx = PatchCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                rng: &mut rng,
                gauss: &mut gauss,
            };
            run(&mut ctx, 0, 100);
        }
        assert_eq!(
            grid.get(30, 26).unwrap().tile,
            slope_tile,
            "slope untouched"
        );
        assert_eq!(
            grid.get(28, 24).unwrap().tile,
            water_tile,
            "water untouched"
        );
    }

    #[test]
    fn a_patch_covers_roughly_its_target_size() {
        // Place a single deterministic patch and count the cells.
        let (mut grid, mut scratch) = world();
        let identity = ids();
        let mut rng = RmgRng::new(5);
        let mut gauss = Gaussian::default();
        {
            let mut ctx = PatchCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                rng: &mut rng,
                gauss: &mut gauss,
            };
            place_patch(&mut ctx, (30, 26), 700, 20, patch_id(30, 26));
        }
        let count = grid
            .native_cells()
            .filter(|&(x, y)| grid.get(x, y).unwrap().tile == 700)
            .count();
        assert_eq!(count, 20, "the placer lays exactly the target size");
    }

    #[test]
    fn patches_are_deterministic() {
        let snapshot = |seed| {
            let (grid, _scratch, _rng) = run_theater(seed, 0, 80);
            grid.native_cells()
                .map(|(x, y)| grid.get(x, y).unwrap().tile)
                .collect::<Vec<_>>()
        };
        assert_eq!(snapshot(2024), snapshot(2024));
    }

    #[test]
    fn zero_vegetation_still_paints_sand_and_green() {
        // Rough probability scales with vegetation, but sand/green do not.
        let (grid, _scratch, _rng) = run_theater(31, 0, 0);
        let painted = grid
            .native_cells()
            .filter(|&(x, y)| {
                let tile = grid.get(x, y).unwrap().tile;
                tile == 800 || tile == 100
            })
            .count();
        assert!(painted > 0, "sand/green patches appear even at veg 0");
    }
}
