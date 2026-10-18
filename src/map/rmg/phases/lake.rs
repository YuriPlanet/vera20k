//! Water seeding for the carved map types (inland and mountainous).
//!
//! These two types are the opposite of the sea-shaping family in `water.rs`:
//! they start from land and *add* water, admitting only clear cells. The seeder
//! makes at most one river system and at most one standalone lake — it is not
//! an iterated fill.
//!
//! This module implements the lake half; `river`, `meander`, and `bridge` own
//! the active river and waterfall-terrain half. The river is gated on a water
//! amount above `RIVER_GATE`, so for `0 < water <= 20` the lake path alone is
//! the whole of the original's output.
//!
//! The lake grows as a priority flood: a random clear seed, a Gaussian target
//! size, then a min-heap walk whose key mixes distance from the seed, a random
//! term and a decay in the number of cells already placed. Growth that touches
//! foreign water fails softly and the whole attempt rolls back.

use crate::map::rmg::rng::RANGE_K_BITS;
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::x87::{self, TruncF64};

use super::blob::{BlobCtx, MinHeap, seed_draw};
use super::shore::{self, ShoreCtx};
use super::water::WaterArgs;

/// Water amount above which the seeder also carves a river. Strictly greater,
/// and signed — the river half is W2' and is implemented in `river`.
pub(crate) const RIVER_GATE: i32 = 0x14;
/// Attempts the driver makes at each of its two phases, stopping on the first
/// success.
const SEED_ATTEMPTS: i32 = 10;

/// Water-cell quota scale and floor: `gen_h * gen_w * water * 0.008 + 100.0`.
const QUOTA_SCALE: f64 = 0.008;
const QUOTA_BASE: f64 = 100.0;
/// A lake needs more than this many cells left in the quota to be worth
/// starting, and more than this many placed to count as a success. Reaching it
/// is the water phase's termination condition, not an error.
const MIN_CELLS: i32 = 0x4B;
/// Lower bound on the drawn lake size.
const SIZE_FLOOR: f64 = 75.0;

/// Growth-key weights. These are single-precision constants in the original and
/// the narrowing matters — `0.02f32` is not `0.02f64`.
const KEY_DIST: f32 = 0.5;
const KEY_RANDOM: f32 = 10.0;
const KEY_DECAY: f32 = 0.02;

/// Heap capacity: two entries per remaining cell plus two, never below this.
const MIN_HEAP_CAP: i32 = 100;

/// Seed-pick budget. The counter is incremented *before* the accept test, so
/// the last attempt draws its coordinates and then bails without being
/// validated: 200 draw pairs, 199 candidates.
const MAX_SEED_ATTEMPTS: i32 = 200;

/// The band of empty cells peeled back from existing water before the allow
/// mask is derived, and the temporary id it is tagged with.
const BAND_RINGS: i32 = 2;
const BAND_TAG: i32 = -2;

/// The four cardinal directions, as `CELL_DELTAS` indices (N, E, S, W).
const CARDINALS: [usize; 4] = [0, 2, 4, 6];

/// The seeder's cross-attempt accumulators.
///
/// The original keeps these on the map-seed object, where they outlive a single
/// lake: `placed` is what turns the water amount into a *cap* rather than a
/// target, so a river that already spent the quota makes every later lake
/// attempt bail before drawing anything.
#[derive(Debug, Default, Clone, Copy)]
pub struct WaterQuota {
    /// Water cells committed so far this generation.
    pub placed: i32,
    /// Water-region id counter; the first water region is 1.
    pub region_id: i32,
}

/// Seed water for the carved map types.
///
/// Only reached when the water amount is non-zero — the original does not enter
/// this seeder at all otherwise, which is why a zero-water inland map is
/// land-only and consumes no draws.
pub fn seed_water_carved(ctx: &mut BlobCtx<'_>, args: &WaterArgs) -> WaterQuota {
    let mut quota = WaterQuota::default();
    quota.region_id += 1;

    // The river phase belongs to W2'. It is gated here rather than omitted so
    // the gate itself is already exact: below the threshold the original carves
    // no river either, and this path is then complete rather than partial.
    if args.water_percent > RIVER_GATE {
        for _ in 0..SEED_ATTEMPTS {
            if super::river::carve(ctx, args, &mut quota, None, false) {
                quota.region_id += 1;
                break;
            }
        }
    }

    for _ in 0..SEED_ATTEMPTS {
        if grow_lake(ctx, args, &mut quota, None) {
            quota.region_id += 1;
            break;
        }
    }
    quota
}

/// Total water cells this map is allowed, from the map's generated dimensions
/// and the water amount.
fn water_target(args: &WaterArgs) -> i32 {
    if args.water_percent == 0 {
        return 0;
    }
    // Operand order is the original's: height, then width, then the amount.
    x87::ftol(
        TruncF64::from_f64(f64::from(args.playable.h))
            .mul(TruncF64::from_f64(f64::from(args.playable.w)))
            .mul(TruncF64::from_f64(f64::from(args.water_percent)))
            .mul(TruncF64::from_f64(QUOTA_SCALE))
            .add(TruncF64::from_f64(QUOTA_BASE))
            .to_f64(),
    )
}

/// Cells of `region` that touch a cell of any other region.
///
/// The original scans every scratch slot and skips the ones whose stored
/// coordinate is the invalid `(0, 0)`; walking the diamond covers the same set,
/// because `(0, 0)` is never inside it.
fn region_border(scratch: &RmgScratch, grid_cells: &[(i32, i32)], region: i32) -> Vec<(i32, i32)> {
    let mut border = Vec::new();
    for &(x, y) in grid_cells {
        if scratch.get(x, y).region != region {
            continue;
        }
        for dir in 0..8usize {
            let (nx, ny) = crate::map::rmg::grid::RmgGrid::step(x, y, dir);
            if scratch.in_diamond(nx, ny) && scratch.get(nx, ny).region != region {
                border.push((x, y));
                break;
            }
        }
    }
    border
}

/// Peel `rings` border rings off `region`, retagging them and flattening their
/// tiles. Consumes no randomness and always succeeds.
fn retag_border_band(
    ctx: &mut BlobCtx<'_>,
    cells: &[(i32, i32)],
    region: i32,
    rings: i32,
    new_id: i32,
) {
    for _ in 0..rings {
        let border = region_border(ctx.scratch, cells, region);
        for &(x, y) in &border {
            ctx.scratch.get_mut(x, y).region = new_id;
            let cell = ctx.grid.get_mut(x, y).expect("native cell");
            cell.tile = 0;
            cell.sub_tile = 0;
            cell.level = ctx.rollback_level;
        }
    }
}

/// Grow `region` outward by `rings`, claiming clear unowned neighbours.
///
/// Returns false as soon as a neighbour belongs to a different region or is not
/// clear — the caller treats that as the whole lake failing.
///
/// The growth itself lives in [`super::meander::dilate_chained`], because the
/// ring-to-ring frontier is shared with the canyon's stamping variant. This once
/// re-collected the region's whole border every ring; the original chains, and
/// the difference shows from the second ring on.
pub(crate) fn dilate_region_rings(ctx: &mut BlobCtx<'_>, region: i32, rings: i32) -> bool {
    super::meander::dilate_chained(ctx, region, rings, None, [0, 0, 0x200, 0x200])
}

/// The growth key, as a single-precision value.
///
/// Distance is narrowed to `f32` before it is weighted — that narrowing is the
/// square-root helper's own store, not a rounding choice made here.
fn growth_key(distance: f32, draw: u32, placed: i32) -> f32 {
    let scale = TruncF64::from_f64(f64::from_bits(RANGE_K_BITS));
    let dist_term =
        TruncF64::from_f64(f64::from(distance)).mul(TruncF64::from_f64(f64::from(KEY_DIST)));
    let random_term = TruncF64::from_f64(f64::from(KEY_RANDOM))
        .mul(TruncF64::from_f64(f64::from(draw)))
        .mul(scale);
    let decay_term =
        TruncF64::from_f64(f64::from(placed)).mul(TruncF64::from_f64(f64::from(KEY_DECAY)));
    dist_term.add(random_term).sub(decay_term).to_f64() as f32
}

/// Derive the allow mask, then pick a seed cell.
///
/// The mask is what keeps lakes off the immediate surroundings of water that
/// already exists: the band peel tags a two-ring margin around foreign water,
/// and only untagged empty cells or cells of this lake's own region stay
/// eligible.
fn prepare_and_seed(
    ctx: &mut BlobCtx<'_>,
    cells: &[(i32, i32)],
    quota: &WaterQuota,
) -> Option<(i32, i32)> {
    for &(x, y) in cells {
        ctx.scratch.get_mut(x, y).stamp = 0;
        ctx.scratch.get_mut(x, y).lake_allow = false;
    }

    retag_border_band(ctx, cells, 0, BAND_RINGS, BAND_TAG);

    for &(x, y) in cells {
        let region = ctx.scratch.get(x, y).region;
        if region == 0 || region == quota.region_id {
            ctx.scratch.get_mut(x, y).lake_allow = true;
        } else if region == BAND_TAG {
            // The scratch tag is what gets reset, not the tile — the peeler
            // already flattened that. Leaving the tag behind would change the
            // region-0 set the next attempt's band peel sees, and with it the
            // seed and the shape of every later lake.
            ctx.scratch.get_mut(x, y).region = 0;
        }
    }

    for attempt in 1..=MAX_SEED_ATTEMPTS {
        let rx = seed_draw(ctx.rng, ctx.map_w);
        let ry = seed_draw(ctx.rng, ctx.map_h);
        let seed = (rx + ry + 1, (ctx.map_w - rx) + ry);
        if attempt == MAX_SEED_ATTEMPTS {
            // The budget is spent after the draws and before the test, so this
            // last candidate is never validated.
            return None;
        }
        if !ctx.scratch.in_diamond(seed.0, seed.1) || ctx.scratch.get(seed.0, seed.1).region != 0 {
            continue;
        }
        if ctx.ids.is_clear(ctx.grid.cell_native(seed.0, seed.1).tile)
            && ctx.scratch.get(seed.0, seed.1).lake_allow
        {
            return Some(seed);
        }
    }
    None
}

/// Draw the target lake size.
fn size_draw(ctx: &mut BlobCtx<'_>, remaining: i32) -> i32 {
    let upper = if remaining > MIN_CELLS {
        remaining
    } else {
        0x4C
    };
    let upper_f = f64::from(upper);
    // Both divisions are integer and round toward zero before widening.
    let mut sigma = f64::from(remaining / 6);
    let mut mean = f64::from(remaining / 3);
    if upper_f < mean - sigma || mean + sigma < SIZE_FLOOR {
        sigma = (upper_f - SIZE_FLOOR) * 0.5;
        mean = sigma + SIZE_FLOOR;
    }
    let sigma_t = TruncF64::from_f64(sigma);
    let mean_t = TruncF64::from_f64(mean);
    loop {
        let value = loop {
            let value = TruncF64::from_f64(ctx.gauss.next(ctx.rng))
                .mul(sigma_t)
                .add(mean_t)
                .to_f64();
            if value >= SIZE_FLOOR {
                break value;
            }
        };
        if value <= upper_f {
            return x87::ftol(value);
        }
    }
}

/// Undo every cell this attempt claimed.
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
        // The base level, which defaults to 4 — not 0.
        cell.level = ctx.rollback_level;
    }
}

/// Grow one lake. Returns whether it was committed.
pub(crate) fn grow_lake(
    ctx: &mut BlobCtx<'_>,
    args: &WaterArgs,
    quota: &mut WaterQuota,
    given_seed: Option<(i32, i32)>,
) -> bool {
    let remaining = water_target(args) - quota.placed;
    if remaining <= MIN_CELLS {
        return false;
    }

    let cells: Vec<(i32, i32)> = ctx.grid.native_cells().collect();
    let region = quota.region_id;

    // The driver derives the allow mask and hunts for a seed. The river's
    // end-lake hands one over instead, and admits every cell.
    let seed = match given_seed {
        Some(seed) => {
            for &(x, y) in &cells {
                let cell = ctx.scratch.get_mut(x, y);
                cell.stamp = 0;
                cell.lake_allow = true;
            }
            seed
        }
        None => match prepare_and_seed(ctx, &cells, quota) {
            Some(seed) => seed,
            None => return false,
        },
    };

    let size = size_draw(ctx, remaining);

    let cap = (remaining * 2 + 2).max(MIN_HEAP_CAP) as usize;
    let mut heap = MinHeap::new(cap);
    ctx.scratch.get_mut(seed.0, seed.1).stamp = region;
    heap.push(0.0, seed);

    let mut alive = true;
    let mut placed = 0i32;
    let mut current = heap.pop();

    // Growth: pop the cheapest frontier cell, carve it, offer its four cardinal
    // neighbours to the heap.
    while placed < size {
        let (_, (x, y)) = match current {
            Some(entry) if alive => entry,
            _ => break,
        };
        ctx.scratch.get_mut(x, y).region = region;
        let cell = ctx.grid.get_mut(x, y).expect("native cell");
        cell.tile = ctx.ids.water_base;
        cell.sub_tile = 0;

        for dir in CARDINALS {
            let (nx, ny) = crate::map::rmg::grid::RmgGrid::step(x, y, dir);
            if !ctx.scratch.in_diamond(nx, ny) {
                continue;
            }
            let neighbour = *ctx.scratch.get(nx, ny);
            if neighbour.region == 0 && neighbour.stamp != region {
                let clear = ctx.ids.is_clear(ctx.grid.cell_native(nx, ny).tile);
                if clear && neighbour.lake_allow {
                    let dx = seed.0 - nx;
                    let dy = seed.1 - ny;
                    let distance = x87::approx_sqrt_f32(dx * dx + dy * dy);
                    let draw = ctx.rng.next_u32();
                    ctx.scratch.get_mut(nx, ny).stamp = region;
                    heap.push(growth_key(distance, draw, placed), (nx, ny));
                    continue;
                }
            }
            // Running into another water region does not stop the walk, but the
            // attempt can no longer be committed.
            if neighbour.region != 0 && neighbour.region != region {
                alive = false;
            }
        }

        placed += 1;
        current = heap.pop();
    }

    // Drain: whatever is still queued must remain carvable, or the attempt dies.
    // These cells still count toward `placed`, so a committed lake is larger
    // than the drawn size by the length of its final frontier.
    if let Some(mut entry) = current.or_else(|| heap.pop()) {
        loop {
            if !alive {
                break;
            }
            let (_, (x, y)) = entry;
            let scratch = *ctx.scratch.get(x, y);
            let clear = ctx.ids.is_clear(ctx.grid.cell_native(x, y).tile);
            if scratch.region == 0 && clear && scratch.lake_allow {
                let cell = ctx.grid.get_mut(x, y).expect("native cell");
                cell.tile = ctx.ids.water_base;
                cell.sub_tile = 0;
                ctx.scratch.get_mut(x, y).region = region;
            } else {
                alive = false;
            }
            placed += 1;
            match heap.pop() {
                Some(next) => entry = next,
                None => break,
            }
        }
    }

    // The shore, dilation and green passes belong to the driver's own lake. A
    // lake grown at the end of a river skips all three — the river finishes its
    // own region afterwards, and running them here would do it twice.
    let is_driver_call = given_seed.is_none();
    let committed = alive
        && placed > MIN_CELLS
        && placed > size / 4
        && (!is_driver_call || {
            let mut shore_ctx = ShoreCtx {
                grid: ctx.grid,
                scratch: ctx.scratch,
                ids: ctx.ids,
                blocks: ctx.blocks,
                rng: ctx.rng,
            };
            shore::run(&mut shore_ctx, region, false)
        })
        && (!is_driver_call || dilate_region_rings(ctx, region, 1));

    if !committed {
        rollback(ctx, &cells, region);
        return false;
    }

    // Anything still bare inside the finished region becomes green — driver
    // lakes only, for the same reason as the passes above.
    for &(x, y) in &cells {
        if !is_driver_call {
            break;
        }
        if ctx.scratch.get(x, y).region != region {
            continue;
        }
        let cell = ctx.grid.get_mut(x, y).expect("native cell");
        if ctx.ids.is_clear(cell.tile) {
            cell.tile = ctx.ids.green;
        }
    }

    quota.placed += placed;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::grid::RmgGrid;
    use crate::map::rmg::phases::shore::{SubTile, TileBlock, TileBlocks};
    use crate::map::rmg::phases::water::PlayableRect;
    use crate::map::rmg::rng::RmgRng;
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

    /// Run the carved seeder and hand back the grid plus the generator.
    fn run_carved(
        map_type: i32,
        seed: u16,
        water_percent: i32,
    ) -> (RmgGrid, RmgScratch, TileIds, RmgRng) {
        let (map_w, map_h) = (34, 42); // gen 30x30
        let stride = (map_w + map_h + 1) as usize;
        let (dmin, dmax) = (map_w, map_w + 2 * map_h);
        let mut grid = RmgGrid::new(stride, dmin, dmax);
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
        {
            let trig = crate::map::retail_trig::TrigTable::embedded();
            let mut ctx = BlobCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                blocks: &blocks,
                rng: &mut rng,
                gauss: &mut gauss,
                trig: Some(trig),
                map_w,
                map_h,
                rollback_level: 4,
            };
            seed_water_carved(&mut ctx, &args);
        }
        (grid, scratch, identity, rng)
    }

    fn water_cells(grid: &RmgGrid, identity: &TileIds) -> Vec<(i32, i32)> {
        grid.native_cells()
            .filter(|&(x, y)| grid.get(x, y).expect("native cell").tile == identity.water_base)
            .collect()
    }

    /// Same options and seed must give the same lake.
    #[test]
    fn lake_generation_is_deterministic() {
        for map_type in [3, 4] {
            let (first, _, identity, _) = run_carved(map_type, 30011, 20);
            let (second, _, _, _) = run_carved(map_type, 30011, 20);
            assert_eq!(
                water_cells(&first, &identity),
                water_cells(&second, &identity),
                "map type {map_type} is not deterministic"
            );
        }
    }
}
