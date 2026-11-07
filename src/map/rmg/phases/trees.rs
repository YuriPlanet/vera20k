//! Tree scattering: the tree-count formula and the per-tree region walk.
//!
//! Runs after the LAT auto-tiling fixup on every theater. The tree count is
//! derived (no RNG) from the width / vegetation / MaxTrees options; then up to
//! 100 iterations each pick a random clear cell, draw a density and a size, and
//! grow a tree "region" from that cell — a closest-first min-heap flood that
//! places `TREExx` terrain objects on the eligible cells it walks.
//!
//! The region-walk visited flag (`scratch.visited`) is set as regions grow and
//! is *never* cleared between iterations: a later region cannot re-enter cells
//! an earlier region already walked, which is what keeps successive trees from
//! all piling onto the first few origins.
//!
//! Native: the tree loop of `RandomMapGenerator::PaintLatPatchesTreesRocks_Temperate
//! @ 0x005A3AE0` (0x005A3F41..0x005A40BD) and
//! `PaintLatPatchesTrees_NonTemperate @ 0x005A4280`, each region grown by
//! `RandomMapGenerator::PlaceTreeRegion @ 0x005A45E0`.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::rng::RmgRng;
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;
use crate::map::rmg::x87::{self, Gaussian, TruncF64};
use crate::util::native_x87::X87Chop53;

use super::blob::{CellHeap, by_key};
use super::shore::TileBlocks;
use super::zones::{LAND_ROCK, recalc_land};

/// Placement iterations cap (`local_5c < 100`).
const MAX_ITERATIONS: i32 = 100;
/// Non-empty slot draws before the whole-map anchor pick gives up.
const MAX_ANCHOR_DRAWS: i32 = 200;
/// Region node budget: at most `size * 25` cells are ever admitted.
const REGION_MULT: i32 = 25;

/// Tree-count formula: `(width·0.1 + 0.7)·(veg·0.01)·maxtrees`.
const WIDTH_SCALE: f64 = 0.1;
const WIDTH_OFFSET: f64 = 0.7;
const VEG_SCALE: f64 = 0.01;

/// Density draw: `Gaussian·0.1 + 0.2`, rejection-clamped to `[0.05, 0.4]`.
const DENSITY_SCALE: f64 = 0.1;
const DENSITY_OFFSET: f64 = 0.2;
const DENSITY_MIN: f64 = 0.05;
const DENSITY_MAX: f64 = 0.4;

/// Size draw: `Gaussian·10 + 25`, rejection-clamped to `[10, 35]`.
const SIZE_SCALE: f64 = 10.0;
const SIZE_OFFSET: f64 = 25.0;
const SIZE_MIN: f64 = 10.0;
const SIZE_MAX: f64 = 35.0;

/// Tree-index draw: `ftol(rand·scale + 1.0)`, rejected above 25 → `[1, 25]`.
///
/// The `+1.0` offset means index 0 (the nonexistent `TREE00`) never appears:
/// every draw maps onto an existing `TREE01`..`TREE25`. `scale` is the retail
/// bit pattern (≈ 25·2⁻³²) embedded verbatim.
const TREE_INDEX_K_BITS: u64 = 0x3E39_0000_0019_0000;
const TREE_INDEX_OFFSET: f64 = 1.0;
const TREE_INDEX_MAX: i32 = 25;

/// Phase inputs (the MapSeed option fields the count formula reads).
#[derive(Debug, Clone, Copy)]
pub struct TreeArgs {
    /// MapSeed generation width (+0x64) — the count-formula width term.
    pub width: i32,
    /// Vegetation option (+0x5C).
    pub vegetation: i32,
    /// MaxTrees option (+0x2FC); 0 disables trees.
    pub max_trees: i32,
    /// Scratch/grid stride: the whole-map anchor draw is `uniform(0, S²−1)`.
    pub stride: i32,
}

/// Everything the phase borrows.
pub struct TreeCtx<'a> {
    pub grid: &'a mut RmgGrid,
    pub scratch: &'a mut RmgScratch,
    pub ids: &'a TileIds,
    pub rng: &'a mut RmgRng,
    /// Shared with the water/blob phases — the Box-Muller cache persists.
    pub gauss: &'a mut Gaussian,
    pub recalc_rock: &'a RecalcRock,
}

/// The cells whose land type the last full recalc before the trees left Rock
/// (`CellClass::RecalcAttributes @ 0x0047D2B0`, see [`recalc_land`]): the
/// tree gate reads that stored land type, not one for the current levels.
/// An overlay cell's native land type is its overlay's, but the gate refuses
/// overlay cells before reading it.
#[derive(Debug, Default)]
pub struct RecalcRock {
    stride: usize,
    rock: Vec<bool>,
}

impl RecalcRock {
    pub fn snapshot(grid: &RmgGrid, blocks: &dyn TileBlocks, cliff_back_impassability: u8) -> Self {
        let stride = grid.width();
        let rock = (0..stride * stride)
            .map(|index| {
                let (x, y) = ((index % stride) as i32, (index / stride) as i32);
                grid.get(x, y).is_some()
                    && recalc_land(grid, blocks, cliff_back_impassability, None, x, y) == LAND_ROCK
            })
            .collect();
        Self { stride, rock }
    }

    /// Only asked for a valid cell.
    fn get(&self, x: i32, y: i32) -> bool {
        self.rock[y as usize * self.stride + x as usize]
    }
}

fn t(value: f64) -> TruncF64 {
    TruncF64::from_f64(value)
}

/// Scatter trees. Returns each placed tree as `(name, x, y)` for the emit stage
/// (trees are `[Terrain]` objects, not grid overlays).
pub fn run(ctx: &mut TreeCtx<'_>, args: &TreeArgs) -> Vec<(String, i16, i16)> {
    let mut trees = Vec::new();

    // count = ftol((width·0.1 + 0.7)·(veg·0.01)·maxtrees) — derived, no RNG.
    let width_term = t(f64::from(args.width))
        .mul(t(WIDTH_SCALE))
        .add(t(WIDTH_OFFSET));
    let veg_term = t(f64::from(args.vegetation)).mul(t(VEG_SCALE));
    let mut remaining = X87Chop53::ftol_f64_low_masked(
        width_term
            .mul(veg_term)
            .mul(t(f64::from(args.max_trees)))
            .to_f64(),
    );

    let mut iterations = 0;
    while remaining > 0 && iterations < MAX_ITERATIONS {
        let anchor = draw_clear_anchor(ctx, args.stride);
        // Density and size are drawn every iteration, even when the anchor pick
        // exhausted its tries and fell back to the (0,0) border cell.
        let density = density_draw(ctx.gauss, ctx.rng);
        let size = size_draw(ctx.gauss, ctx.rng);
        let placed = place_region(ctx, anchor, size, density, &mut trees);
        remaining -= placed;
        iterations += 1;
    }
    trees
}

/// Draw a clear map anchor: reject empty (coord `(0,0)`) scratch slots on every
/// draw, and reject non-clear cells for up to 200 non-empty draws. On
/// exhaustion the original keeps a `(0,0)` anchor, which walks nothing — so the
/// port returns that sentinel and `place_region` grows an empty region there.
fn draw_clear_anchor(ctx: &mut TreeCtx<'_>, stride: i32) -> (i32, i32) {
    let max = stride * stride - 1;
    let mut non_empty_draws = 0;
    loop {
        let (x, y) = loop {
            let idx = ctx.rng.uniform(0, max);
            let (cx, cy) = (idx % stride, idx / stride);
            let record = ctx.scratch.get(cx, cy);
            if (record.x, record.y) != (0, 0) {
                break (cx, cy);
            }
        };
        non_empty_draws += 1;
        if non_empty_draws > MAX_ANCHOR_DRAWS {
            return (0, 0);
        }
        let clear = ctx
            .grid
            .get(x, y)
            .is_some_and(|cell| ctx.ids.is_clear(cell.tile));
        if clear {
            return (x, y);
        }
    }
}

/// Density draw: `Gaussian·0.1 + 0.2`, rejection-clamped to `[0.05, 0.4]`.
fn density_draw(gauss: &mut Gaussian, rng: &mut RmgRng) -> f64 {
    loop {
        let value = t(gauss.next(rng))
            .mul(t(DENSITY_SCALE))
            .add(t(DENSITY_OFFSET))
            .to_f64();
        if (DENSITY_MIN..=DENSITY_MAX).contains(&value) {
            return value;
        }
    }
}

/// Size draw: `ftol(Gaussian·10 + 25)`, rejection-clamped to `[10, 35]`.
fn size_draw(gauss: &mut Gaussian, rng: &mut RmgRng) -> i32 {
    loop {
        let value = t(gauss.next(rng))
            .mul(t(SIZE_SCALE))
            .add(t(SIZE_OFFSET))
            .to_f64();
        if (SIZE_MIN..=SIZE_MAX).contains(&value) {
            return X87Chop53::ftol_f64_low_masked(value);
        }
    }
}

/// The tree-index draw → an existing `TREE01`..`TREE25` name (never `TREE00`).
fn tree_index_draw(rng: &mut RmgRng) -> i32 {
    let scale = t(f64::from_bits(TREE_INDEX_K_BITS));
    let offset = t(TREE_INDEX_OFFSET);
    loop {
        let value = X87Chop53::ftol_f64_low_masked(
            t(f64::from(rng.next_u32())).mul(scale).add(offset).to_f64(),
        );
        if value <= TREE_INDEX_MAX {
            return value;
        }
    }
}

/// Grow one tree region from `origin`: a closest-first min-heap flood. Each
/// popped cell that is clear, unoccupied and overlay-free draws against
/// `density`; on a hit it draws a `TREExx` variant, marks the cell occupied and
/// records the tree. Admits any in-band, unvisited, non-water 8-neighbour (one
/// jitter draw each) up to the `size·25` node budget. Returns trees placed.
fn place_region(
    ctx: &mut TreeCtx<'_>,
    origin: (i32, i32),
    size: i32,
    density: f64,
    out: &mut Vec<(String, i16, i16)>,
) -> i32 {
    if size <= 0 {
        return 0;
    }
    let budget = size * REGION_MULT;
    // 0x005A45E0: a `25 * size` pool and heap; `local_58` counts admissions.
    let mut heap = CellHeap::new(budget as usize);
    ctx.scratch.get_mut(origin.0, origin.1).visited = true;
    heap.push((0.0, origin), by_key);
    // `admitted` is the monotonic total of cells ever pushed (native `local_58`,
    // seeded at 1 for the origin); it caps the region and never decreases.
    let mut admitted = 1i32;
    let mut current = heap.pop(by_key).map(|(_, coord)| coord);
    let mut placed = 0i32;

    while placed < size {
        let Some(cur) = current else {
            break;
        };
        if admitted >= budget {
            break;
        }

        // Eligibility gate: clear, unoccupied, overlay-free and not Rock; a
        // clear tile is Rock when the CliffBack reclass put it behind a cliff.
        let eligible = ctx.grid.get(cur.0, cur.1).is_some_and(|cell| {
            ctx.ids.is_clear(cell.tile)
                && !cell.occupied
                && cell.overlay == -1
                && !ctx.recalc_rock.get(cur.0, cur.1)
        });
        if eligible && ctx.rng.next_unit() < density {
            let v = tree_index_draw(ctx.rng);
            out.push((
                format!("TREE{}{}", v / 10, v % 10),
                cur.0 as i16,
                cur.1 as i16,
            ));
            if let Some(cell) = ctx.grid.get_mut(cur.0, cur.1) {
                cell.occupied = true;
            }
            placed += 1;
        }

        // Neighbour admission: raw diamond band only (no tile/slope gate); the
        // AND order — in-band, then visited, then water, then budget — decides
        // exactly which cells consume a jitter draw.
        for dir in 0..8 {
            let (nx, ny) = RmgGrid::step(cur.0, cur.1, dir);
            if !ctx.scratch.in_diamond(nx, ny) {
                continue;
            }
            let record = ctx.scratch.get(nx, ny);
            if record.visited || record.water_lock {
                continue;
            }
            if admitted >= budget {
                continue;
            }
            let dx = origin.0 - nx;
            let dy = origin.1 - ny;
            let priority = x87::jittered_distance_key(dx * dx + dy * dy, ctx.rng.next_u32());
            ctx.scratch.get_mut(nx, ny).visited = true;
            heap.push((priority, (nx, ny)), by_key);
            admitted += 1;
        }

        current = heap.pop(by_key).map(|(_, coord)| coord);
    }
    placed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::theater::TheaterCliffRanges;

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

    struct World {
        grid: RmgGrid,
        scratch: RmgScratch,
        stride: i32,
    }

    fn world() -> World {
        let (gen_w, gen_h) = (40, 36);
        let (map_w, map_h) = (gen_w + 4, gen_h + 12);
        let stride = map_w + map_h + 1;
        let (dmin, dmax) = (map_w, map_w + 2 * map_h);
        let mut grid = RmgGrid::new(stride as usize, dmin, dmax);
        let scratch = RmgScratch::new(stride as usize, dmin, dmax);
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            grid.get_mut(x, y).unwrap().tile = 0;
        }
        World {
            grid,
            scratch,
            stride,
        }
    }

    fn args(w: &World, max_trees: i32) -> TreeArgs {
        TreeArgs {
            width: 40,
            vegetation: 100,
            max_trees,
            stride: w.stride,
        }
    }

    /// No block data: every tile reads land type Clear.
    struct NoBlocks;

    impl TileBlocks for NoBlocks {
        fn block(&self, _tile: i32) -> Option<&crate::map::rmg::phases::shore::TileBlock> {
            None
        }
    }

    fn run_once(seed: u16, max_trees: i32) -> (Vec<(String, i16, i16)>, World) {
        let mut w = world();
        let targs = args(&w, max_trees);
        let rock = RecalcRock::snapshot(&w.grid, &NoBlocks, 2);
        let identity = ids();
        let mut rng = RmgRng::new(seed);
        let mut gauss = Gaussian::default();
        let trees = {
            let mut ctx = TreeCtx {
                grid: &mut w.grid,
                scratch: &mut w.scratch,
                ids: &identity,
                rng: &mut rng,
                gauss: &mut gauss,
                recalc_rock: &rock,
            };
            run(&mut ctx, &targs)
        };
        (trees, w)
    }

    #[test]
    fn cell_behind_a_cliff_takes_no_tree() {
        // The origin sits four levels under its (x, y-1) neighbour.
        let first_tree = |cliff_back_impassability| {
            let mut w = world();
            let (x, y) = (60, 40);
            let level = w.grid.get(x, y).unwrap().level;
            w.grid.get_mut(x, y - 1).unwrap().level = level + 4;
            let rock = RecalcRock::snapshot(&w.grid, &NoBlocks, cliff_back_impassability);
            let identity = ids();
            let mut rng = RmgRng::new(7);
            let mut gauss = Gaussian::default();
            let mut ctx = TreeCtx {
                grid: &mut w.grid,
                scratch: &mut w.scratch,
                ids: &identity,
                rng: &mut rng,
                gauss: &mut gauss,
                recalc_rock: &rock,
            };
            let mut out = Vec::new();
            place_region(&mut ctx, (x, y), 1, 1.0, &mut out);
            (out[0].1, out[0].2)
        };
        assert_ne!(first_tree(2), (60, 40));
        assert_eq!(first_tree(1), (60, 40));
    }

    #[test]
    fn tree_placement_is_deterministic() {
        let snapshot = |seed| {
            let (trees, world) = run_once(seed, 600);
            let occupied: Vec<(i32, i32)> = world
                .grid
                .native_cells()
                .filter(|&(x, y)| world.grid.get(x, y).unwrap().occupied)
                .collect();
            (trees, occupied)
        };
        assert_eq!(snapshot(2024), snapshot(2024));
    }

    /// Whole original `RandomMapGenerator::PlaceTreeRegion @ 0x005A45E0`
    /// calls from `tools/rmg_oracle/tree_region.py`: each tree's name and
    /// cell, the returned count, the visited marks and the exit RNG.
    #[test]
    fn native_place_tree_region_controls() {
        let doc = crate::test_fixture::json("tools/rmg_oracle/tree_region.json");
        let int = |value: &serde_json::Value| value.as_i64().unwrap() as i32;
        let cells = |value: &serde_json::Value| -> Vec<(i32, i32)> {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|c| (int(&c[0]), int(&c[1])))
                .collect()
        };
        let identity = ids();
        let rows = doc["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 7);
        for row in rows {
            let input = &row["input"];
            let context = input["id"].as_str().unwrap();
            let mut grid = RmgGrid::new(33, 16, 48);
            let mut scratch = RmgScratch::new(33, 16, 48);
            for (x, y) in grid.native_cells().collect::<Vec<_>>() {
                grid.get_mut(x, y).unwrap().tile = 0;
            }
            for (x, y) in cells(&input["rough"]) {
                grid.get_mut(x, y).unwrap().tile = int(&doc["scene"]["rough"]);
            }
            for (x, y) in cells(&input["occupied"]) {
                grid.get_mut(x, y).unwrap().occupied = true;
            }
            for (x, y) in cells(&input["overlay"]) {
                grid.get_mut(x, y).unwrap().overlay = 0;
            }
            for (x, y) in cells(&input["locked"]) {
                scratch.get_mut(x, y).water_lock = true;
            }
            for (x, y) in cells(&input["visited"]) {
                scratch.get_mut(x, y).visited = true;
            }
            let mut rock = RecalcRock {
                stride: 33,
                rock: vec![false; 33 * 33],
            };
            for (x, y) in cells(&input["rock"]) {
                rock.rock[y as usize * 33 + x as usize] = true;
            }
            let mut rng = RmgRng::new(input["seed"].as_u64().unwrap() as u16);
            let mut gauss = Gaussian::default();
            let mut ctx = TreeCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                rng: &mut rng,
                gauss: &mut gauss,
                recalc_rock: &rock,
            };
            let origin = (int(&input["origin"][0]), int(&input["origin"][1]));
            let size = int(&input["size"]);
            let density = input["density"].as_f64().unwrap();
            let mut trees = Vec::new();
            let placed = place_region(&mut ctx, origin, size, density, &mut trees);
            assert_eq!(placed, int(&row["placed"]), "{context}: placed");
            let native: Vec<(String, i16, i16)> = row["trees"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| {
                    (
                        t[0].as_str().unwrap().to_owned(),
                        int(&t[1]) as i16,
                        int(&t[2]) as i16,
                    )
                })
                .collect();
            assert_eq!(trees, native, "{context}: trees");
            let visited: Vec<(i32, i32)> = (0..33)
                .flat_map(|y| (0..33).map(move |x| (x, y)))
                .filter(|&(x, y)| scratch.in_diamond(x, y) && scratch.get(x, y).visited)
                .collect();
            assert_eq!(visited, cells(&row["visited"]), "{context}: visited");
            assert_eq!(
                rng.into_continuation().into_native_parts(),
                super::super::cliff_native_tests::native_state(doc, &row["rng_after"])
                    .into_native_parts(),
                "{context}: exit RNG"
            );
        }
    }
}
