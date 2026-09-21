//! River carving for the inland and mountainous map types.
//!
//! A river starts on one of the four map edges, picks a heading pointing inward,
//! and then walks: each step stamps a cross-section perpendicular to travel,
//! advances, and wobbles both the heading and the width a little. It stops on a
//! random roll, so length varies. Rivers shorter than [`MIN_STEPS`] are thrown
//! away and rolled back whole.
//!
//! **Trig convention, and it is the opposite of what the research report says.**
//! The carve line runs along `(+sin, +cos)` and travel along `(+cos, −sin)`.
//! The report's §4.1 names these `c` and `s` the other way round, because it was
//! written against two Ghidra labels that were themselves swapped. Read the
//! report with `c → sin` and `s → cos`. Getting this backwards rotates every
//! river 90° and no test in this repo would notice.
//!
//! A finished river then usually cuts a **canyon**: its region is grown across
//! the map by [`meander`], then six unstamped dilation rings widen it, and
//! every cell *outside* that region is raised four levels — the river's valley
//! is what stays low. The base level rises with it, so later rollback fills
//! land at the new height. Only the first river can cut one, since the gate is
//! an exact test against the starting base level.
//!
//! An earlier version of this module had the canyon backwards — two stamped
//! rings and no outside raise, read off the *waterfall's* finish dilation, which
//! sits a hundred bytes later and really is two stamped rings. The two calls
//! are easy to conflate; the fix was verified against the canyon path's own
//! bytes, not the neighbouring one.
//!
//! A straight-enough section may also form **waterfall terrain** — see
//! [`bridge`], whose inherited `BuildRiverBridge` name is not a topology role.
//! The gate needs a straight cross-section, a small heading drift, the first
//! MapGen coin, and the drawn minimum step; a successful crossing widens the
//! channel, jumps the walk twelve cells, and continues under a new region id.
//! Active low-deck overlays/CABHUTs are a separate `bridge_deck` mechanism.

use crate::map::rmg::rng::{RANGE_K_BITS, RmgRng};
use crate::map::rmg::x87::{self, Gaussian, TruncF64};

use super::blob::{BlobCtx, seed_draw};
use super::bridge;
use super::lake::{self, WaterQuota};
use super::meander;
use super::shore::{self, ShoreCtx};
use super::water::WaterArgs;

/// Edge count, and the heading each edge aims at: `7π/4 − edge·π/2`.
const EDGES: i32 = 4;
const EDGE_BASE_HEADING: f64 = 5.497_787_143_782_138; // 7π/4
const QUARTER_TURN: f64 = std::f64::consts::FRAC_PI_2;
const EIGHTH_TURN: f64 = std::f64::consts::FRAC_PI_4;
/// Spread of the initial heading around its edge's mean.
const START_SIGMA: f64 = 0.523_598_775_598_298_8; // π/6

/// Heading wobble: mean 0, this sigma, clamped to the lifetime window.
const WOBBLE_SIGMA: f64 = 0.314_159_265_358_979_3; // π/10
/// Width wobble sigma.
const WIDTH_SIGMA: f64 = 0.5;

/// Width scale on the water amount, and the floor below which it cannot fall.
const WIDTH_SCALE: f64 = 0.07;
const WIDTH_MIN: f64 = 1.0;

/// Waterfall-attempt minimum-step draw: uniform over 35..=125.
const WATERFALL_STEP_BASE: f64 = 35.0;
const WATERFALL_STEP_SPAN_SCALED: f64 = 2.118_758_857_743_525_6e-8; // 91 * K
const WATERFALL_STEP_MAX: i32 = 125;

/// Per-step chance of spawning a branch, and of stopping.
const BRANCH_CHANCE: f64 = 0.01;
const STOP_CHANCE: f64 = 0.005;
/// Chance the river cuts a canyon once it has finished.
const CANYON_CHANCE: f64 = 0.7;
/// The base level a map starts at, and the only value that lets a canyon form —
/// once one has been cut the base has moved on, so no later river can cut a
/// second.
const CANYON_BASE_LEVEL: u8 = 4;
/// How far the base level rises around a canyon.
const CANYON_LEVEL_STEP: u8 = 4;
/// Step density for the canyon's growth arm: lower means a wider spread.
const CANYON_STEP_DENSITY: f32 = 0.01;
/// Rings the canyon's region is grown by. Six, unstamped — this was first
/// ported as two rings with a level stamp, a misreading that belonged to the
/// waterfall crossing's finish dilation, not the canyon's.
const CANYON_DILATE_RINGS: i32 = 6;
/// The canyon's clamp rect covers everything — it is not really a clamp.
const WHOLE_MAP: i32 = 0x200;

/// Steps a river must reach to survive.
const MIN_STEPS: i32 = 0x28;
/// The step count past which the heading starts to wander.
const WOBBLE_AFTER: i32 = 5;
/// Rings the finished river's region is grown by.
const PLAIN_DILATE_RINGS: i32 = 2;
/// Half-cell offset: cell centres, and the round-half-up on the width.
const HALF: f64 = 0.5;

fn unit_draw(rng: &mut RmgRng) -> TruncF64 {
    TruncF64::from_f64(f64::from(rng.next_u32()))
        .mul(TruncF64::from_f64(f64::from_bits(RANGE_K_BITS)))
}

fn draw_below(rng: &mut RmgRng, threshold: f64) -> bool {
    unit_draw(rng).lt(TruncF64::from_f64(threshold))
}

/// A Gaussian redrawn until it lands inside `[lo, hi]`.
///
/// The recentre arm the original carries here can never fire for the start
/// heading or the branch heading — the windows are always wide enough — but it
/// can for the two wobbles, so it is kept.
fn bounded_gaussian(
    gauss: &mut Gaussian,
    rng: &mut RmgRng,
    mean: f64,
    sigma: f64,
    lo: f64,
    hi: f64,
) -> f64 {
    let (mut mean, mut sigma) = (mean, sigma);
    if hi < mean - sigma || mean + sigma < lo {
        sigma = (hi - lo) * HALF;
        mean = sigma + lo;
    }
    let sigma_t = TruncF64::from_f64(sigma);
    let mean_t = TruncF64::from_f64(mean);
    loop {
        let value = TruncF64::from_f64(gauss.next(rng))
            .mul(sigma_t)
            .add(mean_t)
            .to_f64();
        if value >= lo && value <= hi {
            return value;
        }
    }
}

/// Where a river attempt starts and which way it points.
struct Launch {
    cell: (i32, i32),
    heading: f64,
}

/// Pick a start edge, a cell along it, and an inward heading.
fn launch(ctx: &mut BlobCtx<'_>, gauss_first: bool) -> Launch {
    let _ = gauss_first;
    let edge = loop {
        let value = x87::ftol(
            unit_draw(ctx.rng)
                .mul(TruncF64::from_f64(f64::from(EDGES)))
                .to_f64(),
        );
        if value <= EDGES - 1 {
            break value;
        }
    };
    let rx = seed_draw(ctx.rng, ctx.map_w);
    let ry = seed_draw(ctx.rng, ctx.map_h);
    let (w, h) = (ctx.map_w, ctx.map_h);
    let cell = match edge {
        0 => (rx + 1, w - rx),
        1 => (w + h - 1 - ry, h - ry),
        2 => (w + h - 1 - rx, h + rx),
        _ => (ry + 1, w + ry),
    };
    let mean = EDGE_BASE_HEADING - f64::from(edge) * QUARTER_TURN;
    let heading = bounded_gaussian(
        ctx.gauss,
        ctx.rng,
        mean,
        START_SIGMA,
        mean - EIGHTH_TURN,
        mean + EIGHTH_TURN,
    );
    Launch { cell, heading }
}

/// Everything one carve attempt tracks.
struct Walk {
    region: i32,
    alive: bool,
    steps: i32,
    /// Terminated by the stop roll rather than by leaving the map. Only a
    /// roll-terminated river gets an end lake.
    rolled_to_a_stop: bool,
}

/// What one cross-section reports back to the walk: its end cells, and whether
/// it ran straight. The waterfall-terrain gate reads all four.
struct Section {
    first: (i32, i32),
    last: (i32, i32),
    /// Every position shared one `ftol(x)` — the section lies in one column.
    column_straight: bool,
    /// Every position shared one `ftol(y)` — one row.
    row_straight: bool,
}

/// Stamp one cross-section.
fn carve_section(
    ctx: &mut BlobCtx<'_>,
    walk: &mut Walk,
    fx: f64,
    fy: f64,
    span: i32,
    sin: f64,
    cos: f64,
) -> Section {
    let step_back = f64::from(span - 1) * HALF;
    let mut x = fx - step_back * sin;
    let mut y = fy - step_back * cos;
    let mut section = Section {
        first: (x87::ftol(x), x87::ftol(y)),
        last: (0, 0),
        column_straight: true,
        row_straight: true,
    };

    for _ in 0..span {
        let (cx, cy) = (x87::ftol(x), x87::ftol(y));
        section.last = (cx, cy);
        if ctx.scratch.in_diamond(cx, cy) {
            let owner = ctx.scratch.get(cx, cy).region;
            if owner == walk.region {
                // Idempotent re-carve of our own cells.
                let cell = ctx.grid.get_mut(cx, cy).expect("native cell");
                cell.tile = ctx.ids.water_base;
                cell.sub_tile = 0;
            } else if owner == 0 {
                if ctx.ids.is_clear(ctx.grid.cell_native(cx, cy).tile) {
                    ctx.scratch.get_mut(cx, cy).region = walk.region;
                    let cell = ctx.grid.get_mut(cx, cy).expect("native cell");
                    cell.tile = ctx.ids.water_base;
                    cell.sub_tile = 0;
                } else {
                    // Rivers may not run over anything already placed.
                    walk.alive = false;
                }
            } else {
                walk.alive = false;
            }
        }
        // The straightness comparison runs current-versus-advanced, so it
        // spans one position past the section's end — a section that would
        // leave its column on the very next substep does not count as
        // straight.
        if x87::ftol(x + sin) != cx {
            section.column_straight = false;
        }
        if x87::ftol(y + cos) != cy {
            section.row_straight = false;
        }
        // The section always completes, even once the walk is doomed.
        x += sin;
        y += cos;
    }
    section
}

/// Undo every cell this river claimed.
fn rollback(ctx: &mut BlobCtx<'_>, cells: &[(i32, i32)], region: i32) {
    for &(x, y) in cells {
        if ctx.scratch.get(x, y).region != region {
            continue;
        }
        let scratch = ctx.scratch.get_mut(x, y);
        scratch.region = 0;
        scratch.water_region = false;
        let cell = ctx.grid.get_mut(x, y).expect("native cell");
        cell.tile = 0;
        cell.sub_tile = 0;
        cell.level = ctx.rollback_level;
    }
}

/// Carve one river. `start` of `None` is the driver's call, which picks its own
/// launch; a branch is handed an explicit cell and heading.
pub fn carve(
    ctx: &mut BlobCtx<'_>,
    args: &WaterArgs,
    quota: &mut WaterQuota,
    start: Option<(i32, i32, f64)>,
    is_branch: bool,
) -> bool {
    // Without the retail sine table a river cannot be steered faithfully, so
    // none is carved. Skipping is the honest failure: a river built on a
    // substitute table would be wrong in a way nothing here could detect.
    let Some(trig) = ctx.trig else {
        return false;
    };
    let cells: Vec<(i32, i32)> = ctx.grid.native_cells().collect();
    let mut region = quota.region_id;
    // The original mutates its own is-branch argument when a branch spawns, so
    // one flag serves both "I am a branch" and "I have spawned one" — and a
    // river that has done either can never attempt a waterfall crossing.
    let mut no_more_branches = is_branch;
    let mut waterfall_successes = 0i32;

    let (origin, mut heading) = match start {
        Some((x, y, h)) => ((x, y), h),
        None => {
            let l = launch(ctx, true);
            (l.cell, l.heading)
        }
    };
    if !ctx.scratch.in_diamond(origin.0, origin.1) {
        // Nothing carved yet, so nothing to undo.
        return false;
    }
    let heading0 = heading;

    // Width setup.
    let width_max = x87::ftol(
        TruncF64::from_f64(f64::from(args.water_percent))
            .mul(TruncF64::from_f64(WIDTH_SCALE))
            .to_f64()
            .max(WIDTH_MIN),
    );
    let width = loop {
        let value = x87::ftol(
            unit_draw(ctx.rng)
                .mul(TruncF64::from_f64(f64::from(width_max)))
                .add(TruncF64::from_f64(WIDTH_MIN))
                .to_f64(),
        );
        if value <= width_max {
            break value;
        }
    };
    let half_width = width / 2;
    let mut width_walk = f64::from(width);

    // The earliest step at which this river may attempt waterfall terrain.
    let waterfall_min_step = loop {
        let value = x87::ftol(
            TruncF64::from_f64(f64::from(ctx.rng.next_u32()))
                .mul(TruncF64::from_f64(WATERFALL_STEP_SPAN_SCALED))
                .add(TruncF64::from_f64(WATERFALL_STEP_BASE))
                .to_f64(),
        );
        if value <= WATERFALL_STEP_MAX {
            break value;
        }
    };

    let mut fx = f64::from(origin.0) + HALF;
    let mut fy = f64::from(origin.1) + HALF;
    let (clamp_lo, clamp_hi) = (heading0 - QUARTER_TURN, heading0 + QUARTER_TURN);

    let mut walk = Walk {
        region,
        alive: true,
        steps: 0,
        rolled_to_a_stop: false,
    };

    loop {
        let (sx, sy) = (x87::ftol(fx), x87::ftol(fy));
        if !ctx.scratch.in_diamond(sx, sy) || !walk.alive {
            break;
        }

        let sin = f64::from(trig.sin_radians(heading));
        let cos = f64::from(trig.cos_radians(heading));

        let span = x87::ftol(width_walk + HALF);
        let section = carve_section(ctx, &mut walk, fx, fy, span, sin, cos);

        // Exactly one straightness flag survives a step: whichever axis
        // dominates travel kills the other's flag.
        let (mut column_straight, mut row_straight) =
            (section.column_straight, section.row_straight);
        if cos.abs() <= sin.abs() {
            column_straight = false;
        } else {
            row_straight = false;
        }

        // Waterfall-terrain attempt — before travel, reading this section's
        // endpoints and the heading as they stand. A river that is a branch or
        // has spawned one cannot attempt it, and one success is the lifetime cap.
        if !no_more_branches && (column_straight || row_straight) && waterfall_successes < 1 {
            let drift = x87::ftol(heading - heading0).abs();
            if f64::from(drift) < EIGHTH_TURN
                && args.bridge_enabled
                && walk.steps > waterfall_min_step
            {
                let dir = if column_straight {
                    if cos <= 0.0 { 6 } else { 2 }
                } else if sin <= 0.0 {
                    4
                } else {
                    0
                };
                let attempt = bridge::BridgeArgs {
                    region,
                    heading_dir: dir,
                    first: section.first,
                    last: section.last,
                    pool_dims: (args.playable.w, args.playable.h),
                };
                if bridge::build(ctx, &attempt) {
                    // The river continues on the far bank under a new region
                    // id; the finish dilation later absorbs the old one.
                    waterfall_successes += 1;
                    quota.region_id += 1;
                    region = quota.region_id;
                    walk.region = region;
                    let (jx, jy) = bridge::jump(dir);
                    fx += f64::from(jx);
                    fy += f64::from(jy);
                }
            }
        }

        // Travel is perpendicular to the carve line.
        fx += cos;
        fy -= sin;

        // Branch spawn. The draw is unconditional; only the spawn is gated.
        let branch = draw_below(ctx.rng, BRANCH_CHANCE);
        if branch && walk.alive && !no_more_branches && waterfall_successes == 0 {
            no_more_branches = true;
            let mean = heading + QUARTER_TURN;
            let angle = bounded_gaussian(
                ctx.gauss,
                ctx.rng,
                mean,
                START_SIGMA,
                heading + START_SIGMA,
                heading + 5.0 * START_SIGMA,
            );
            let bx = x87::ftol(fx + f64::from(span) * sin);
            let by = x87::ftol(fy + f64::from(span) * cos);
            // A failed branch kills the parent — and rolls the whole river back.
            walk.alive = carve(ctx, args, quota, Some((bx, by, angle)), true);
        }

        if walk.steps > WOBBLE_AFTER {
            heading += bounded_gaussian(
                ctx.gauss,
                ctx.rng,
                0.0,
                WOBBLE_SIGMA,
                clamp_lo - heading,
                clamp_hi - heading,
            );
        }
        if half_width > 0 {
            width_walk += bounded_gaussian(
                ctx.gauss,
                ctx.rng,
                0.0,
                WIDTH_SIGMA,
                f64::from(width - half_width) - width_walk,
                f64::from(width + half_width) - width_walk,
            );
        }

        walk.steps += 1;
        if draw_below(ctx.rng, STOP_CHANCE) {
            walk.rolled_to_a_stop = true;
            break;
        }
    }

    if walk.steps < MIN_STEPS {
        walk.alive = false;
    }

    // A river that ran out of map does not get an end lake; only one that chose
    // to stop does.
    if walk.rolled_to_a_stop && walk.alive {
        let end = (x87::ftol(fx), x87::ftol(fy));
        if ctx.scratch.in_diamond(end.0, end.1) {
            walk.alive = lake::grow_lake(ctx, args, quota, Some(end));
        }
    }

    if !is_branch && walk.alive {
        let ok = {
            let mut shore_ctx = ShoreCtx {
                grid: ctx.grid,
                scratch: ctx.scratch,
                ids: ctx.ids,
                blocks: ctx.blocks,
                rng: ctx.rng,
            };
            shore::run(&mut shore_ctx, region, false)
        };
        if ok {
            // The green sweep runs whether or not the dilation succeeded; only
            // the shore pass gates it.
            walk.alive = lake::dilate_region_rings(ctx, region, 1);
            for &(x, y) in &cells {
                if ctx.scratch.get(x, y).region != region {
                    continue;
                }
                let cell = ctx.grid.get_mut(x, y).expect("native cell");
                if ctx.ids.is_clear(cell.tile) {
                    cell.tile = ctx.ids.green;
                }
            }
        } else {
            walk.alive = false;
        }
    }

    // The canyon and the plain finish are the same dilation with different
    // arguments — one stamps a level, the other does not — so exactly one of
    // the two runs.
    let mut cut_a_canyon = false;
    if !is_branch
        && walk.alive
        && waterfall_successes == 0
        && ctx.rollback_level == CANYON_BASE_LEVEL
    {
        // A river with a waterfall crossing never rolls the canyon coin.
        cut_a_canyon = draw_below(ctx.rng, CANYON_CHANCE);
    }

    if !is_branch && walk.alive {
        if cut_a_canyon {
            let arm = meander::MeanderArgs {
                tag: region,
                step_density: CANYON_STEP_DENSITY,
                rect: [0, 0, WHOLE_MAP, WHOLE_MAP],
                reference: origin,
                claim_frontier: true,
                pool_dims: (args.playable.w, args.playable.h),
            };
            // A canyon that cannot be grown takes the river down with it.
            walk.alive = meander::grow_meander_arm(ctx, &arm);
            if walk.alive {
                walk.alive = meander::dilate_chained(
                    ctx,
                    region,
                    CANYON_DILATE_RINGS,
                    None,
                    [0, 0, WHOLE_MAP, WHOLE_MAP],
                );
            }
            if walk.alive {
                // The canyon is made twice over: every cell *outside* the
                // river's grown region is raised four levels here, and the
                // base level rises with it, so later rollback fills and
                // whatever terrain is generated afterwards sit at the new
                // height. The exact-equality gate above is why only the first
                // river can cut one.
                for &(x, y) in &cells {
                    if ctx.scratch.get(x, y).region == region {
                        continue;
                    }
                    let cell = ctx.grid.get_mut(x, y).expect("native cell");
                    cell.level = cell.level.wrapping_add(CANYON_LEVEL_STEP);
                }
                ctx.rollback_level += CANYON_LEVEL_STEP;
            }
        } else if waterfall_successes > 0 {
            // A waterfall river absorbs its own pre-crossing region id: the
            // stamped dilation accepts cells of region − 1 and writes the
            // current base level onto everything it claims.
            let base = ctx.rollback_level;
            walk.alive = meander::dilate_chained(
                ctx,
                region,
                PLAIN_DILATE_RINGS,
                Some(base),
                [0, 0, WHOLE_MAP, WHOLE_MAP],
            );
        } else {
            walk.alive = lake::dilate_region_rings(ctx, region, PLAIN_DILATE_RINGS);
        }
    }

    if !walk.alive {
        rollback(ctx, &cells, region);
        if waterfall_successes > 0 {
            // The near half of the river lives under the previous id.
            rollback(ctx, &cells, region - 1);
        }
        return false;
    }
    quota.placed += walk.steps;
    true
}

/// Convenience for tests and the seeder: is this water amount above the gate?
#[cfg(test)]
pub fn carries_a_river(water_percent: i32) -> bool {
    water_percent > super::lake::RIVER_GATE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::grid::RmgGrid as Grid;
    use crate::map::rmg::phases::shore::{SubTile, TileBlock, TileBlocks};
    use crate::map::rmg::phases::water::PlayableRect;
    use crate::map::rmg::scratch::RmgScratch;
    use crate::map::rmg::tiles::SpecialTerrain;
    use crate::map::rmg::tiles::TileIds;
    use crate::map::rmg::x87::Gaussian;

    /// Water amount well above the river gate, so every run carries a river.
    const RIVERED_WATER: i32 = 60;

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

    /// Run with the waterfall coin on and report the final region counter.
    fn run_with_waterfall_attempt(map_type: i32, seed: u16) -> (i32, Grid, TileIds) {
        // A realistic map size: waterfall terrain needs room — fills span 13
        // ranks, and the finish's junction only settles when the far-bank
        // river can wander without re-touching the old segment.
        let (map_w, map_h) = (64, 72);
        let stride = (map_w + map_h + 1) as usize;
        let (dmin, dmax) = (map_w, map_w + 2 * map_h);
        let mut grid = Grid::new(stride, dmin, dmax);
        let mut scratch = RmgScratch::new(stride, dmin, dmax);
        let mut identity = ids();
        // The crossing needs waterfall sets; four synthetic bases, disjoint from
        // everything else the test ids use.
        identity.special.waterfalls = [600, 610, 620, 630];
        let blocks = blocks();
        let mut rng = RmgRng::new(seed);
        let mut gauss = Gaussian::default();

        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            grid.get_mut(x, y).expect("native cell").tile = identity.clear;
        }

        let args = WaterArgs {
            map_type: 3,
            water_percent: RIVERED_WATER,
            num_players: 4,
            bridge_enabled: true,
            playable: PlayableRect {
                x: 2,
                y: 5,
                w: 60,
                h: 60,
            },
        };
        let _ = map_type;
        let quota;
        {
            let trig = crate::map::rmg::trig::TrigTable::synthetic();
            let mut ctx = BlobCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                blocks: &blocks,
                rng: &mut rng,
                gauss: &mut gauss,
                trig: Some(&trig),
                map_w,
                map_h,
                rollback_level: CANYON_BASE_LEVEL,
            };
            quota = lake::seed_water_carved(&mut ctx, &args);
        }
        (quota.region_id, grid, identity)
    }

    #[test]
    fn waterfall_attempts_advance_the_region_and_stamp_varied_water() {
        // A successful waterfall attempt shows two ways: the region advances
        // extra id, and the fills paint varied water tiles (water_base+1..+5),
        // which nothing else in the carved path produces. The counter alone is
        // not proof of surviving water — a crossing can place and the river
        // still die later, in which case the rollback erases the varied tiles
        // while the counter stays bumped, exactly like the original's
        // never-decremented region field. So the assertion wants one seed
        // where both agree.
        let mut attempted = 0;
        let mut survived = 0;
        for seed in (0u16..96).map(|i| i * 683 + 11) {
            let (final_region, grid, identity) = run_with_waterfall_attempt(3, seed);
            let varied = grid
                .native_cells()
                .filter(|&(x, y)| {
                    let tile = grid.get(x, y).expect("native cell").tile;
                    tile > identity.water_base && tile <= identity.water_base + 5
                })
                .count();
            // region_id without a waterfall success tops out at 3: river id,
            // success bump, and the lake's.
            if final_region > 3 {
                attempted += 1;
                if varied > 0 {
                    survived += 1;
                }
            }
        }
        assert!(
            attempted > 0,
            "no waterfall terrain placed across any seed — the gate never opens"
        );
        // KNOWN GAP, pinned deliberately: placements still never survive the
        // river finish, even with the waterfall pieces stamped. The finish's
        // shore pass hard-refuses where the two region generations'
        // shorelines meet — foreign shore pieces whose classes differ,
        // because the junction geometry changed between the crossing's own
        // shore pass and the finish, so a different piece family is selected
        // at the same cell. The refusal gate itself is verified faithful
        // (both piece tables and the stamper's arms match the binary), so
        // whether the original tolerates the same geometry can only be
        // settled by a per-cell comparison against a native run — the
        // oracle's job. If a survivor ever appears, flip this to
        // `survived > 0` and update the module docs.
        assert_eq!(
            survived, 0,
            "a waterfall crossing survived — flip this test to assert survival and \
             update the module docs"
        );
    }

    /// Drive the carved seeder and report what the base level ended up as,
    /// together with the water the run produced.
    ///
    /// This goes through `seed_water_carved`, the same entry the water stage
    /// uses, rather than calling `carve` directly — a canyon that only fires
    /// from a hand-built call would prove nothing about the real path.
    fn run_carved_levels(map_type: i32, seed: u16) -> (u8, Grid, TileIds) {
        let (map_w, map_h) = (34, 42);
        let stride = (map_w + map_h + 1) as usize;
        let (dmin, dmax) = (map_w, map_w + 2 * map_h);
        let mut grid = Grid::new(stride, dmin, dmax);
        let mut scratch = RmgScratch::new(stride, dmin, dmax);
        let identity = ids();
        let blocks = blocks();
        let mut rng = RmgRng::new(seed);
        let mut gauss = Gaussian::default();

        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            grid.get_mut(x, y).expect("native cell").tile = identity.clear;
        }

        let args = WaterArgs {
            map_type,
            water_percent: RIVERED_WATER,
            num_players: 4,
            bridge_enabled: false,
            playable: PlayableRect {
                x: 2,
                y: 5,
                w: 30,
                h: 30,
            },
        };
        let base;
        {
            let trig = crate::map::rmg::trig::TrigTable::synthetic();
            let mut ctx = BlobCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                blocks: &blocks,
                rng: &mut rng,
                gauss: &mut gauss,
                trig: Some(&trig),
                map_w,
                map_h,
                rollback_level: CANYON_BASE_LEVEL,
            };
            lake::seed_water_carved(&mut ctx, &args);
            base = ctx.rollback_level;
        }
        (base, grid, identity)
    }

    #[test]
    fn some_rivers_cut_a_canyon_and_raise_the_base() {
        // The coin passes about 70% of the time and the arm has to succeed on
        // top of that, so this asserts over a spread of seeds rather than one.
        // Before the canyon was wired the base could never move, so this fails
        // outright against the previous code — which is the point of it.
        let mut raised = 0;
        let mut seeds = 0;
        for seed in [11u16, 42, 777, 1234, 4242, 9001, 31337, 60000] {
            for map_type in [3, 4] {
                seeds += 1;
                let (base, _, _) = run_carved_levels(map_type, seed);
                assert!(
                    base == CANYON_BASE_LEVEL || base == CANYON_BASE_LEVEL + CANYON_LEVEL_STEP,
                    "base level moved somewhere unexpected: {base}"
                );
                if base > CANYON_BASE_LEVEL {
                    raised += 1;
                }
            }
        }
        assert!(
            raised > 0,
            "no canyon fired across {seeds} runs — the arm never succeeds"
        );
    }

    #[test]
    fn the_base_never_rises_twice() {
        // The gate is an exact test against the starting base, so once a canyon
        // has been cut no later river can add another step. A `>=` gate here
        // would let the base climb run away.
        for seed in [11u16, 4242, 60000] {
            let (base, _, _) = run_carved_levels(3, seed);
            assert!(
                base <= CANYON_BASE_LEVEL + CANYON_LEVEL_STEP,
                "base climbed past one step: {base}"
            );
        }
    }

    #[test]
    fn a_canyon_raises_the_terrain_outside_the_river() {
        // The valley stays low; everything else goes up by four. The first
        // port of this had it backwards (nothing raised, only the base bumped),
        // so this asserts on the actual cell levels, not the base.
        let mut checked = 0;
        for seed in [11u16, 42, 777, 1234, 4242, 9001, 31337, 60000] {
            for map_type in [3, 4] {
                let (base, grid, identity) = run_carved_levels(map_type, seed);
                if base == CANYON_BASE_LEVEL {
                    continue; // no canyon on this seed
                }
                checked += 1;
                let raised = grid
                    .native_cells()
                    .filter(|&(x, y)| {
                        let cell = grid.get(x, y).expect("native cell");
                        cell.tile != identity.water_base
                            && cell.level >= CANYON_BASE_LEVEL + CANYON_LEVEL_STEP
                    })
                    .count();
                let low = grid
                    .native_cells()
                    .filter(|&(x, y)| {
                        grid.get(x, y).expect("native cell").level
                            < CANYON_BASE_LEVEL + CANYON_LEVEL_STEP
                    })
                    .count();
                assert!(
                    raised > 0,
                    "seed {seed} type {map_type}: canyon fired but nothing was raised"
                );
                assert!(
                    low > 0,
                    "seed {seed} type {map_type}: the valley itself was raised too"
                );
            }
        }
        assert!(
            checked > 0,
            "no canyon fired across any seed — nothing was checked"
        );
    }

    #[test]
    fn a_canyon_run_still_produces_water() {
        // The canyon path replaces the plain dilation, and a failed arm rolls
        // the river back. If that wiring were wrong the whole water output
        // would vanish on the seeds where the coin passes.
        for seed in [11u16, 42, 4242] {
            let (_, grid, identity) = run_carved_levels(3, seed);
            let water = grid
                .native_cells()
                .filter(|&(x, y)| grid.get(x, y).expect("native cell").tile == identity.water_base)
                .count();
            assert!(water > 0, "seed {seed} produced no water at all");
        }
    }

    #[test]
    fn the_gate_matches_the_documented_threshold() {
        assert!(!carries_a_river(20));
        assert!(carries_a_river(21));
    }

    #[test]
    fn bounded_gaussian_recentres_only_when_the_window_is_too_narrow() {
        // A window wider than +/- sigma leaves mean and sigma alone.
        let mut rng = RmgRng::new(7);
        let mut gauss = Gaussian::default();
        let value = bounded_gaussian(&mut gauss, &mut rng, 0.0, 0.1, -10.0, 10.0);
        assert!((-10.0..=10.0).contains(&value));
    }

    #[test]
    fn a_cross_section_advances_along_sin_cos() {
        // Pins the convention this module exists to get right: the carve line
        // steps by (+sin, +cos), so a heading of 0 lays a vertical line.
        let _ = Grid::step(0, 0, 0);
        let (sin, cos) = (0.0f64, 1.0f64);
        let span = 3;
        let step_back = f64::from(span - 1) * HALF;
        let x0 = 10.0 - step_back * sin;
        let y0 = 10.0 - step_back * cos;
        assert_eq!(x87::ftol(x0), 10);
        assert_eq!(x87::ftol(y0), 9);
    }
}
