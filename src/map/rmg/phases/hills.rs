//! Hills: height-field generation and corner-based terrain morphing.
//!
//! Pipeline (`0x005A35F0`): water-adjacency seed → height random walk → corner
//! grid build → per-cell height push (recursive corner morph) → finalize →
//! 2x2 ramp-quad cleanup. This module currently implements the first two
//! stages — the water seed and the walk — which own all of the phase's RNG
//! (the corner engine consumes none). The corner-morph stages land separately
//! against `RMG_HILLS_CORNER_ENGINE_GHIDRA_REPORT.md`.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::rng::RmgRng;
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;
use crate::map::rmg::x87::{self, Gaussian, TruncF64};
use crate::map::theater::TheaterCliffRanges;

use super::hills_corners::{self, CornerGrid, Morphable};

/// Height a shore-piece neighbour is seeded to.
const SHORE_SEED_HEIGHT: f64 = 0.5;
/// Ruggedness scales: tilt clamp, out-of-diamond velocity, draw window.
const TILT_SLOPE: f64 = 0.0001;
const TILT_BASE: f64 = 0.1;
const OUT_VEL_SCALE: f64 = 0.0025;
const HALF_STEP_SCALE: f64 = 0.005;
/// Walk skip threshold: `out_vel < 0.025` (Ruggedness < 10) → no hills.
const WALK_MIN_OUT_VEL: f64 = 0.025;
/// Velocity forced on water-flagged cells (the exact f64 nearest 0.0025).
const FLAGGED_VEL: f64 = 0.0025;
/// Height clamp bounds for the second draw.
const HEIGHT_LOW: f64 = -2.0;
const HEIGHT_HIGH: f64 = 2.0;
/// Draw-window default half-width.
const WINDOW_HALF: f64 = 0.025;
const HALF: f64 = 0.5;

fn t(value: f64) -> TruncF64 {
    TruncF64::from_f64(value)
}

/// Everything the hills phase borrows.
pub struct HillsCtx<'a> {
    pub grid: &'a mut RmgGrid,
    pub scratch: &'a mut RmgScratch,
    pub ids: &'a TileIds,
    pub gauss: &'a mut Gaussian,
    pub rng: &'a mut RmgRng,
}

/// Phase inputs.
#[derive(Debug, Clone, Copy)]
pub struct HillsArgs {
    pub ruggedness: i32,
    pub map_w: i32,
    pub map_h: i32,
}

/// Run the whole hills phase: seed, walk, then the deterministic corner morph.
///
/// `cliff` classifies obstacle tiles for the water seed; `morphable` reports a
/// tile's `Morphable=` flag for the corner engine. Neither the build nor the
/// morph consumes RNG.
pub fn run(
    ctx: &mut HillsCtx<'_>,
    args: &HillsArgs,
    cliff: &TheaterCliffRanges,
    morphable: Morphable<'_>,
) {
    water_seed(ctx.grid, ctx.scratch, ctx.ids, cliff);
    walk(ctx.scratch, ctx.gauss, ctx.rng, args.ruggedness);
    let span = args.map_w + args.map_h - 1;
    let mut corners = CornerGrid::build(ctx.grid, ctx.scratch, span, ctx.ids, morphable);
    hills_corners::morph(ctx.grid, ctx.scratch, &mut corners, ctx.ids, morphable);
    corners.finalize(ctx.grid, ctx.scratch, ctx.ids, morphable);
    hills_corners::quad_cleanup(ctx.grid);
}

/// Water-adjacency seed (`0x005A33F0`): each shore-piece cell marks its first
/// in-diamond clear neighbour with height 0.5 and the protected flag; each
/// cliff/obstacle cell marks such a neighbour with the flag only.
pub fn water_seed(
    grid: &RmgGrid,
    scratch: &mut RmgScratch,
    ids: &TileIds,
    cliff: &TheaterCliffRanges,
) {
    for (x, y) in grid.native_cells().collect::<Vec<_>>() {
        let cell = *grid.get(x, y).expect("native cell");
        let is_shore = ids.is_shore_piece(cell.tile);
        let is_obstacle =
            !is_shore && cliff.is_special_terrain_tile(cell.tile as u16, cell.sub_tile);
        if !is_shore && !is_obstacle {
            continue;
        }
        for dir in 0..8 {
            let (nx, ny) = RmgGrid::step(x, y, dir);
            if !scratch.in_diamond(nx, ny) {
                continue;
            }
            let clear = grid.get(nx, ny).is_some_and(|c| ids.is_clear(c.tile));
            if !clear {
                continue;
            }
            let record = scratch.get_mut(nx, ny);
            if is_shore {
                record.height = SHORE_SEED_HEIGHT;
            }
            record.water_lock = true;
            break;
        }
    }
}

/// Height random walk (`0x005A2F50`): row-major smoothing from the already
/// processed W/N neighbours plus two Gaussian draws per cell, then a final
/// truncation. Skipped entirely below Ruggedness 10.
pub fn walk(scratch: &mut RmgScratch, gauss: &mut Gaussian, rng: &mut RmgRng, ruggedness: i32) {
    let r = t(f64::from(ruggedness));
    let tilt_clamp = r.mul(t(TILT_SLOPE)).add(t(TILT_BASE));
    let out_vel = r.mul(t(OUT_VEL_SCALE));
    let half_step = r.mul(t(HALF_STEP_SCALE));
    if out_vel.lt(t(WALK_MIN_OUT_VEL)) {
        return;
    }
    let half = t(HALF);
    let zero = t(0.0);

    let width = scratch.width();
    for k in 0..width * width {
        let (x, y) = {
            let cell = scratch.cells()[k];
            (i32::from(cell.x), i32::from(cell.y))
        };
        if (x, y) == (0, 0) {
            continue;
        }

        let nw_ref = if scratch.in_diamond(x - 1, y - 1) {
            t(scratch.get(x - 1, y - 1).height)
        } else {
            zero
        };
        let mut height = t(scratch.get(x, y).height);
        let mut vel;
        let mut tilt;

        // West neighbour: smooth height, copy velocity, seed the tilt term.
        if scratch.in_diamond(x - 1, y) {
            let wh = t(scratch.get(x - 1, y).height);
            vel = t(scratch.get(x - 1, y).velocity);
            height = height.add(wh);
            tilt = wh.sub(nw_ref);
        } else {
            vel = out_vel;
            tilt = zero;
        }
        // North neighbour: accumulate.
        if scratch.in_diamond(x, y - 1) {
            let nh = t(scratch.get(x, y - 1).height);
            let nv = t(scratch.get(x, y - 1).velocity);
            height = height.add(nh);
            vel = vel.add(nv);
            tilt = nh.sub(nw_ref).add(tilt);
        } else {
            vel = vel.add(out_vel);
        }
        height = height.mul(half);
        vel = vel.mul(half);

        // Tilt clamp: sign(tilt) * clamp, with 0 -> 0.
        tilt = if zero.lt(tilt) {
            tilt_clamp
        } else if tilt.lt(zero) {
            tilt_clamp.neg()
        } else {
            zero
        };

        // Protected cells clamp height >= 0 and force a fixed velocity.
        if scratch.get(x, y).water_lock {
            if !zero.lt(height) {
                height = zero;
            }
            vel = t(FLAGGED_VEL);
        }

        // Draw 1 — velocity: Gaussian into [-vel, half_step - vel].
        let lo1 = vel.neg();
        let hi1 = half_step.sub(vel);
        let (scale1, center1) = window(t(WINDOW_HALF), zero, lo1, hi1, half);
        vel = vel.add(draw(gauss, rng, scale1, center1, lo1, hi1));

        // Draw 2 — height: Gaussian into [-2 - h, 2 - h], tilt-biased centre,
        // scaled by the just-updated velocity.
        let lo2 = t(HEIGHT_LOW).sub(height);
        let hi2 = t(HEIGHT_HIGH).sub(height);
        let mut center2 = lo2;
        if !tilt.lt(lo2) {
            center2 = tilt;
            if hi2.lt(tilt) {
                center2 = hi2;
            }
        }
        let (scale2, center2) = window(vel, center2, lo2, hi2, half);
        height = height.add(draw(gauss, rng, scale2, center2, lo2, hi2));

        let record = scratch.get_mut(x, y);
        record.height = height.to_f64();
        record.velocity = vel.to_f64();
    }

    // Final pass: truncate every height to a whole number.
    for k in 0..width * width {
        let height = scratch.cells()[k].height;
        scratch.cells_mut()[k].height = f64::from(x87::ftol(height));
    }
}

/// Re-centre a Gaussian draw window when the default `[center-scale,
/// center+scale]` reach cannot cover `[lo, hi]`.
fn window(
    scale: TruncF64,
    center: TruncF64,
    lo: TruncF64,
    hi: TruncF64,
    half: TruncF64,
) -> (TruncF64, TruncF64) {
    if hi.lt(center.sub(scale)) || scale.add(center).lt(lo) {
        let new_scale = hi.sub(lo).mul(half);
        (new_scale, new_scale.add(lo))
    } else {
        (scale, center)
    }
}

/// One rejection-sampled Gaussian draw kept inside `[lo, hi]`.
fn draw(
    gauss: &mut Gaussian,
    rng: &mut RmgRng,
    scale: TruncF64,
    center: TruncF64,
    lo: TruncF64,
    hi: TruncF64,
) -> TruncF64 {
    loop {
        let value = t(gauss.next(rng)).mul(scale).add(center);
        if !value.lt(lo) && !hi.lt(value) {
            return value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::tiles::SpecialTerrain;

    fn ids() -> TileIds {
        TileIds {
            clear: 0,
            ramp_base: 600,
            ramp_smooth: 620,
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
            special: SpecialTerrain::default(),
        }
    }

    fn world() -> (RmgGrid, RmgScratch) {
        let (map_w, map_h) = (24, 20);
        let stride = (map_w + map_h + 1) as usize;
        let (dmin, dmax) = (map_w, map_w + 2 * map_h);
        let mut grid = RmgGrid::new(stride, dmin, dmax);
        let scratch = RmgScratch::new(stride, dmin, dmax);
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            grid.get_mut(x, y).unwrap().tile = 0;
        }
        (grid, scratch)
    }

    #[test]
    fn shore_neighbour_gets_height_and_flag_cliff_neighbour_gets_flag_only() {
        let (mut grid, mut scratch) = world();
        let identity = ids();
        let cliff = TheaterCliffRanges {
            cliff_set: Some(300),
            ..Default::default()
        };
        let cells: Vec<(i32, i32)> = grid.native_cells().collect();
        let shore = cells[cells.len() / 3];
        let cliffc = cells[2 * cells.len() / 3];
        grid.get_mut(shore.0, shore.1).unwrap().tile = 400; // shore piece
        grid.get_mut(cliffc.0, cliffc.1).unwrap().tile = 300; // cliff

        water_seed(&grid, &mut scratch, &identity, &cliff);

        // Each seeded cell flagged at least one clear neighbour.
        let flagged = scratch.cells().iter().filter(|c| c.water_lock).count();
        assert!(flagged >= 2);
        let with_height = scratch
            .cells()
            .iter()
            .filter(|c| c.height == SHORE_SEED_HEIGHT && c.water_lock)
            .count();
        assert!(with_height >= 1, "the shore neighbour carries 0.5");
        // A cliff neighbour is flagged but has no seeded height.
        let flag_no_height = scratch
            .cells()
            .iter()
            .filter(|c| c.water_lock && c.height == 0.0)
            .count();
        assert!(flag_no_height >= 1, "the cliff neighbour has flag only");
    }

    #[test]
    fn low_ruggedness_skips_the_walk() {
        let (_grid, mut scratch) = world();
        // Seed a nonzero height; a skipped walk leaves it untouched.
        scratch.get_mut(10, 10).height = 0.5;
        let mut gauss = Gaussian::default();
        let mut rng = RmgRng::new(1);
        walk(&mut scratch, &mut gauss, &mut rng, 9); // R=9 < 10
        assert_eq!(scratch.get(10, 10).height, 0.5, "R<10 leaves heights");
        // No RNG consumed.
        let mut fresh = RmgRng::new(1);
        assert_eq!(rng.next_u32(), fresh.next_u32());
    }

    #[test]
    fn walk_truncates_heights_into_minus_two_to_two() {
        let (_grid, mut scratch) = world();
        let mut gauss = Gaussian::default();
        let mut rng = RmgRng::new(4242);
        walk(&mut scratch, &mut gauss, &mut rng, 40);
        for cell in scratch.cells() {
            if (cell.x, cell.y) == (0, 0) {
                continue;
            }
            let h = cell.height;
            assert_eq!(h, h.trunc(), "heights are whole numbers");
            assert!((-2.0..=2.0).contains(&h), "height {h} in [-2, 2]");
        }
    }

    #[test]
    fn protected_cells_never_go_below_zero() {
        let (_grid, mut scratch) = world();
        // Flag a band of cells; their post-walk height must be >= 0.
        let flagged: Vec<(i32, i32)> = scratch
            .cells()
            .iter()
            .filter(|c| (c.x, c.y) != (0, 0) && i32::from(c.x) % 3 == 0)
            .map(|c| (i32::from(c.x), i32::from(c.y)))
            .collect();
        for &(x, y) in &flagged {
            scratch.get_mut(x, y).water_lock = true;
        }
        let mut gauss = Gaussian::default();
        let mut rng = RmgRng::new(7);
        walk(&mut scratch, &mut gauss, &mut rng, 60);
        for &(x, y) in &flagged {
            assert!(scratch.get(x, y).height >= 0.0, "flagged ({x},{y}) >= 0");
        }
    }

    #[test]
    fn walk_is_deterministic() {
        let snapshot = |seed| {
            let (_grid, mut scratch) = world();
            let mut gauss = Gaussian::default();
            let mut rng = RmgRng::new(seed);
            walk(&mut scratch, &mut gauss, &mut rng, 55);
            scratch.cells().iter().map(|c| c.height).collect::<Vec<_>>()
        };
        assert_eq!(snapshot(2024), snapshot(2024));
    }
}
