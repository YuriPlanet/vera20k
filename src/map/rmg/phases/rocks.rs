//! Rock overlay scattering (temperate theater only).
//!
//! After trees, the temperate painter sprinkles SROCK / TROCK overlays. A
//! target count is drawn from the map area, then up to `target·5` attempts each
//! pick a random non-border cell and, if it carries no overlay, stamp a rock:
//! SROCK on the sand-LAT transition tiles, TROCK on clear or green terrain. Any
//! overlay-free cell counts toward the target even when it matches neither rock
//! terrain — a native quirk this reproduces.
//!
//! Non-temperate theaters skip this phase entirely (the orchestrator gates it):
//! `PaintLatPatchesTrees_NonTemperate @ 0x005A4280` has no rock pass.
//! Native comparison: `tools/rmg_oracle/rock_pass.py`.
//!
//! Native: the tail of `RandomMapGenerator::PaintLatPatchesTreesRocks_Temperate
//! @ 0x005A3AE0` (0x005A40C3..0x005A4271). The target is drawn against
//! `SurfaceDataCount @ 0x0042B1F0` (`(H + 4) * W * 2`) / 200; the variant draws
//! multiply by `5 * K` in one constant (0x007EDAB0), which equals
//! `uniform(0, 4)`'s two multiplies exactly.

use crate::map::rmg::rng::RmgRng;
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;

use crate::map::rmg::grid::RmgGrid;

/// Sand-rock overlay base (`0xA8`); five variants SROCK01..SROCK05.
const SROCK_BASE: i32 = 168;
/// Terrain-rock overlay base (`0xAD`); five variants TROCK01..TROCK05.
const TROCK_BASE: i32 = 173;
/// Rock-variant draw is `uniform(0, 4)`.
const ROCK_VARIANT_MAX: i32 = 4;
/// Target count is `uniform(0, area2 / 200)`.
const AREA_DIVISOR: i32 = 200;
/// Attempts cap is `target · 5`.
const ATTEMPTS_MULT: i32 = 5;
/// Sand-LAT transition span (`sand_lat .. sand_lat + 0x10`).
const LAT_SPAN: i32 = 0x10;

/// Phase inputs.
#[derive(Debug, Clone, Copy)]
pub struct RockArgs {
    /// Full map width (`DAT_0087f8dc`).
    pub map_w: i32,
    /// Full map height (`DAT_0087f8e0`).
    pub map_h: i32,
    /// Grid/scratch stride: the slot draw is `uniform(0, S²−1)`.
    pub stride: i32,
}

/// Everything the phase borrows.
pub struct RockCtx<'a> {
    pub grid: &'a mut RmgGrid,
    pub scratch: &'a RmgScratch,
    pub ids: &'a TileIds,
    pub rng: &'a mut RmgRng,
}

/// Scatter rock overlays onto the grid. Returns the number of cells the pass
/// counted (including the overlay-free-but-no-terrain-match quirk cells).
pub fn run(ctx: &mut RockCtx<'_>, args: &RockArgs) -> i32 {
    // area2 = (H + 4) · W · 2; target = uniform(0, area2 / 200).
    let area2 = (args.map_h + 4) * args.map_w * 2;
    let target = ctx.rng.uniform(0, area2 / AREA_DIVISOR);
    let attempts_cap = target * ATTEMPTS_MULT;
    if attempts_cap <= 0 {
        return 0;
    }

    let max_slot = args.stride * args.stride - 1;
    let mut placed = 0i32;
    let mut attempts = 0i32;
    loop {
        if placed >= target {
            return placed;
        }
        let (x, y) = draw_nonborder_slot(ctx.rng, ctx.scratch, args.stride, max_slot);

        let (overlay, tile) = {
            let cell = ctx.grid.get(x, y).expect("picked slot is an in-band cell");
            (cell.overlay, cell.tile)
        };
        if overlay == -1 {
            // 0x005A4253: every overlay-free pick zeroes the density byte.
            // Residual: its `Recalc_Attributes(-1)` (0x005A4259) is not ported;
            // it matters only if a later generator pass reads those attributes.
            ctx.grid.get_mut(x, y).unwrap().density = 0;
            if is_sand_lat_transition(ctx.ids, tile) {
                let variant = ctx.rng.uniform(0, ROCK_VARIANT_MAX);
                ctx.grid.get_mut(x, y).unwrap().overlay = variant + SROCK_BASE;
            } else if ctx.ids.is_clear(tile) || ctx.ids.is_green_lat(tile) {
                let variant = ctx.rng.uniform(0, ROCK_VARIANT_MAX);
                ctx.grid.get_mut(x, y).unwrap().overlay = variant + TROCK_BASE;
            }
            // else: overlay-free but no rock terrain — still counts.
            placed += 1;
        }

        attempts += 1;
        if attempts >= attempts_cap {
            return placed;
        }
    }
}

/// Draw one slot uniform over `[0, S²−1]`, rejecting the shared `(0,0)` border
/// cell (every out-of-band scratch record reads coord `(0,0)`).
fn draw_nonborder_slot(
    rng: &mut RmgRng,
    scratch: &RmgScratch,
    stride: i32,
    max_slot: i32,
) -> (i32, i32) {
    loop {
        let idx = rng.uniform(0, max_slot);
        let (cx, cy) = (idx % stride, idx / stride);
        let record = scratch.get(cx, cy);
        if (record.x, record.y) != (0, 0) {
            return (cx, cy);
        }
    }
}

/// The sand-rock terrain test: the clear-to-sand LAT transition range only.
///
/// This deliberately *excludes* the sand base tile, unlike `TileIds::is_sand_lat`
/// — the original's sand-rock predicate checks only `[sand_lat, sand_lat+0x10)`,
/// while its green predicate (used below via `is_green_lat`) does include the
/// green base tile. The asymmetry is real and reproduced.
/// `CellClass::Tile_Is_ClearToSandLAT @ 0x00486790`, which compares even an
/// absent (-1) base.
fn is_sand_lat_transition(ids: &TileIds, tile: i32) -> bool {
    tile >= ids.sand_lat && tile < ids.sand_lat + LAT_SPAN
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
            sand: 800,
            green: 100,
            rough_lat: -1,
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

    struct World {
        grid: RmgGrid,
        scratch: RmgScratch,
        args: RockArgs,
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
            args: RockArgs {
                map_w,
                map_h,
                stride,
            },
        }
    }

    fn run_once(seed: u16) -> (i32, World) {
        run_with(seed, ids(), 0)
    }

    fn run_with(seed: u16, identity: TileIds, density: u8) -> (i32, World) {
        let mut w = world();
        for (x, y) in w.grid.native_cells().collect::<Vec<_>>() {
            w.grid.get_mut(x, y).unwrap().density = density;
        }
        let mut rng = RmgRng::new(seed);
        let placed = {
            let mut ctx = RockCtx {
                grid: &mut w.grid,
                scratch: &w.scratch,
                ids: &identity,
                rng: &mut rng,
            };
            run(&mut ctx, &w.args)
        };
        (placed, w)
    }

    /// The original temperate rock tail (`0x005A40C3..0x005A4277`) from
    /// `tools/rmg_oracle/rock_pass.py`: a declared scene (not a full Generate
    /// prefix), and every rock, cleared density and draw the original made.
    /// Native's `RecalcAttributes` calls, one per counted pick, give the count.
    #[test]
    fn native_rock_pass_controls() {
        let doc = crate::test_fixture::json("tools/rmg_oracle/rock_pass.json");
        let int = |v: &serde_json::Value| v.as_i64().unwrap() as i32;
        let tiles = &doc["scene"]["tiles"];
        let identity = TileIds {
            sand_lat: int(&tiles["sand_lat"]),
            green: int(&tiles["green"]),
            green_lat: int(&tiles["green_lat"]),
            ..ids()
        };
        let rows = doc["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 7);
        for row in rows {
            let input = &row["input"];
            let context = input["id"].as_str().unwrap();
            let mut grid = RmgGrid::new(33, 16, 48);
            let scratch = RmgScratch::new(33, 16, 48);
            for (x, y) in grid.native_cells().collect::<Vec<_>>() {
                let cell = grid.get_mut(x, y).unwrap();
                (cell.tile, cell.overlay, cell.density) = (0, -1, 0);
            }
            for t in input["tiles"].as_array().unwrap() {
                let family = int(&tiles[t[2].as_str().unwrap()]);
                grid.get_mut(int(&t[0]), int(&t[1])).unwrap().tile = family + int(&t[3]);
            }
            for c in input["overlay"].as_array().unwrap() {
                grid.get_mut(int(&c[0]), int(&c[1])).unwrap().overlay = 0;
            }
            for c in input["density"].as_array().unwrap() {
                grid.get_mut(int(&c[0]), int(&c[1])).unwrap().density = int(&c[2]) as u8;
            }
            let state = |grid: &RmgGrid| {
                let mut out = Vec::new();
                for y in 0..33 {
                    for x in 0..33 {
                        if scratch.in_diamond(x, y) {
                            let cell = grid.get(x, y).unwrap();
                            out.push([x, y, cell.overlay, i32::from(cell.density)]);
                        }
                    }
                }
                out
            };
            let before = state(&grid);
            let mut rng = RmgRng::new(input["seed"].as_u64().unwrap() as u16);
            let size = &input["size"];
            let args = RockArgs {
                map_w: int(&size[0]),
                map_h: int(&size[1]),
                stride: 33,
            };
            let placed = {
                let mut ctx = RockCtx {
                    grid: &mut grid,
                    scratch: &scratch,
                    ids: &identity,
                    rng: &mut rng,
                };
                run(&mut ctx, &args)
            };
            let recalcs = row["recalcs"].as_array().unwrap();
            assert_eq!(placed as usize, recalcs.len(), "{context}: counted picks");
            let changed: Vec<[i32; 4]> = state(&grid)
                .into_iter()
                .zip(before)
                .filter(|(now, before)| now != before)
                .map(|(now, _)| now)
                .collect();
            let native: Vec<[i32; 4]> = row["changed"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| std::array::from_fn(|i| int(&c[i])))
                .collect();
            assert_eq!(changed, native, "{context}: overlays and densities");
            assert_eq!(
                rng.into_continuation().into_native_parts(),
                super::super::cliff_native_tests::native_state(&doc, &row["rng_after"])
                    .into_native_parts(),
                "{context}: exit RNG"
            );
        }
    }

    #[test]
    fn rock_placement_is_deterministic() {
        let snapshot = |seed| {
            let (_placed, world) = run_once(seed);
            world
                .grid
                .native_cells()
                .map(|(x, y)| world.grid.get(x, y).unwrap().overlay)
                .collect::<Vec<_>>()
        };
        assert_eq!(snapshot(2024), snapshot(2024));
    }

    /// 0x005A4253 zeroes the density byte of every overlay-free pick.
    #[test]
    fn a_rocked_cell_loses_its_density() {
        let (placed, world) = run_with(2024, ids(), 5);
        assert!(placed > 0);
        for (x, y) in world.grid.native_cells() {
            let cell = world.grid.get(x, y).unwrap();
            if cell.overlay != -1 {
                assert_eq!(cell.density, 0);
            }
        }
    }

    /// 0x00486790 has no absent-base guard: with no sand LAT, the clear tile
    /// falls in `[-1, 15)` and takes sand rocks.
    #[test]
    fn an_absent_sand_lat_base_still_matches_low_tiles() {
        let mut identity = ids();
        identity.sand_lat = -1;
        let (placed, world) = run_with(2024, identity, 0);
        assert!(placed > 0);
        let overlays: Vec<i32> = world
            .grid
            .native_cells()
            .map(|(x, y)| world.grid.get(x, y).unwrap().overlay)
            .filter(|&overlay| overlay != -1)
            .collect();
        assert!(!overlays.is_empty());
        assert!(
            overlays
                .iter()
                .all(|o| (SROCK_BASE..SROCK_BASE + 5).contains(o))
        );
    }
}
