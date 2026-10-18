//! Waterfall/river terrain shaping for active random-map types 3 and 4.
//!
//! gamemd: `RandomMapGenerator::BuildRiverBridge` 0x0059E740 and
//! `RandomMapGenerator::IsUniformLevelBridgeEndArea` 0x005A7440.
//!
//! Stock Create Random Map writes a `.SED`, and `ScenarioClass::Read_Scenario`
//! reaches this generator on launch. The inherited `BuildRiverBridge` name is
//! misleading: this routine creates waterfall/river terrain only. It writes
//! no low-bridge overlays, runtime bridge flags, tubes, CABHUTs, or generated
//! construction events.
//!
//! When a river's cross-section runs straight for long enough, the walk may
//! shape a waterfall crossing: a clearance scan ahead, twelve ranks of water
//! filled in (four near, eight far), the near region grown behind the crossing
//! by the meander arm, the shoreline finalized, and the near side dilated. On success
//! the river jumps twelve cells forward and continues on the far bank — under a
//! **new region id**, which is why the finish dilation absorbs the previous id.
//!
//! Everything here is transcribed from the original's four hand-written heading
//! cases. They are *not* rotations of one another and must not be refactored
//! into one parametric form — the N and W waterfall cases carry level loops
//! the E and S cases lack.
//!
//! The crossing is stamped from the theater's four **waterfall tilesets** —
//! not the bridge sets and not the separate low-deck overlay mechanism.
//! Each set holds four pieces: two ends, a two-cell middle and a one-cell
//! middle, alternated by span parity. The stamping consumes no randomness,
//! but it is not cosmetic: its unassigned-tile sentinels and level
//! adjustments are what let the river-finish shore pass accept the junction
//! between the near and far river regions.

use crate::map::rmg::x87::{self, TruncF64};

use super::blob::BlobCtx;
use super::meander;
use super::shore::{self, ShoreCtx};

/// The water-variant draw: `ftol(r · 6/(2³²−1))`, redrawn while above five.
///
/// The constant is the original's own pre-divided double, multiplied in a
/// single step — a different rounding from drawing a unit float and scaling by
/// the span, and the two are not interchangeable at double precision.
const VARIANT_SCALE_BITS: u64 = 0x3E18_0000_0018_0000; // 6/(2^32-1)
const VARIANT_MAX: u32 = 5;
/// The sub-tile draw: same shape with `4/(2³²−1)`, redrawn while above three.
const SUB_TILE_SCALE_BITS: u64 = 0x3E10_0000_0010_0000; // 4/(2^32-1)
const SUB_TILE_MAX: u32 = 3;

/// Meander density for the near-side growth — an order gentler than the
/// canyon's.
const PLATEAU_STEP_DENSITY: f32 = 0.003;

/// Rings the near side is dilated by after the shore pass.
const PLATEAU_DILATE_RINGS: i32 = 2;

/// How far the river jumps on success, in cells along the travel heading.
pub(crate) const JUMP_CELLS: i32 = 12;

/// Full map extent, for the unclipped clamp-rect axes.
const FULL: i32 = 0x200;

/// Everything one placement attempt needs from the walk.
pub(crate) struct BridgeArgs {
    /// Region the fills are tagged with — the river's id at the attempt.
    pub region: i32,
    /// Travel direction code: 0 N, 2 E, 4 S, 6 W.
    pub heading_dir: usize,
    /// First and last cells of the straight cross-section.
    pub first: (i32, i32),
    pub last: (i32, i32),
    /// Generated map dimensions, for the arm's node pool.
    pub pool_dims: (i32, i32),
}

/// The per-heading geometry, transcribed case by case.
struct Layout {
    /// Clearance rect `(x, y, w, h)` — twelve deep, span+5 wide, ahead.
    clearance: [i32; 4],
    /// The four ranks of water nearest the river.
    fill_near: [i32; 4],
    /// The eight further ranks.
    fill_far: [i32; 4],
    /// Clamp for the arm and dilation: the half-plane behind the crossing.
    clamp: [i32; 4],
    /// The arm's angle reference.
    seed: (i32, i32),
}

/// Anchor and rects per heading. The anchor is whichever endpoint has the
/// smaller coordinate along the channel axis: first cell for N and E, last
/// for S and W.
fn layout(args: &BridgeArgs) -> Layout {
    let (fx, fy) = args.first;
    let (lx, ly) = args.last;
    match args.heading_dir {
        0 => {
            let (ax, ay) = (fx, fy);
            let span = lx - ax;
            Layout {
                clearance: [ax - 2, ay - 12, span + 5, 12],
                fill_near: [ax, ay - 4, span + 1, 4],
                fill_far: [ax, ay - 12, span + 1, 8],
                clamp: [0, ay - 4, FULL, FULL - (ay - 4)],
                seed: (ax, ay - 4),
            }
        }
        2 => {
            let (ax, ay) = (fx, fy);
            let span = ly - ay;
            Layout {
                clearance: [ax + 1, ay - 2, 12, span + 5],
                fill_near: [ax + 1, ay, 4, span + 1],
                fill_far: [ax + 5, ay, 8, span + 1],
                clamp: [0, 0, ax + 4, FULL],
                seed: (ax + 5, ay),
            }
        }
        4 => {
            let (ax, ay) = (lx, ly);
            let span = fx - ax;
            Layout {
                clearance: [ax - 2, ay + 1, span + 5, 12],
                fill_near: [ax, ay + 1, span + 1, 4],
                fill_far: [ax, ay + 5, span + 1, 8],
                clamp: [0, 0, FULL, ay + 4],
                seed: (ax, ay + 1),
            }
        }
        _ => {
            let (ax, ay) = (lx, ly);
            let span = fy - ay;
            Layout {
                clearance: [ax - 12, ay - 2, 12, span + 5],
                fill_near: [ax - 4, ay, 4, span + 1],
                fill_far: [ax - 12, ay, 8, span + 1],
                clamp: [ax - 4, 0, FULL - (ax - 4), FULL],
                seed: (ax - 4, ay),
            }
        }
    }
}

/// One pre-divided single-multiply draw, redrawn while above `max`.
fn scaled_draw(ctx: &mut BlobCtx<'_>, scale_bits: u64, max: u32) -> u32 {
    let scale = TruncF64::from_f64(f64::from_bits(scale_bits));
    loop {
        let value = x87::ftol(
            TruncF64::from_f64(f64::from(ctx.rng.next_u32()))
                .mul(scale)
                .to_f64(),
        ) as u32;
        if value <= max {
            return value;
        }
    }
}

/// Fill a rect with varied water, two draws per in-bounds cell, rows outer.
fn fill_water(ctx: &mut BlobCtx<'_>, rect: [i32; 4], region: i32) {
    for y in rect[1]..rect[1] + rect[3] {
        for x in rect[0]..rect[0] + rect[2] {
            if !ctx.scratch.in_diamond(x, y) {
                // Out-of-bounds cells consume nothing at all.
                continue;
            }
            let variant = scaled_draw(ctx, VARIANT_SCALE_BITS, VARIANT_MAX);
            let sub = scaled_draw(ctx, SUB_TILE_SCALE_BITS, SUB_TILE_MAX);
            let cell = ctx.grid.get_mut(x, y).expect("native cell");
            cell.tile = ctx.ids.water_base + variant as i32;
            cell.sub_tile = sub as u8;
            ctx.scratch.get_mut(x, y).region = region;
        }
    }
}

/// Attempt one waterfall-terrain crossing. Returns whether it was placed.
///
/// A false return is not fatal to the river — the caller carries on without a
/// crossing, and any water the first fill painted simply belongs to the river's
/// region from then on. That mirrors the original, which does not roll a
/// failed placement back either.
pub(crate) fn build(ctx: &mut BlobCtx<'_>, args: &BridgeArgs) -> bool {
    let layout = layout(args);

    // Clearance: any in-bounds cell ahead that is owned or not bare ground
    // vetoes the crossing before a single draw is consumed.
    let [cx, cy, cw, ch] = layout.clearance;
    for y in cy..cy + ch {
        for x in cx..cx + cw {
            if !ctx.scratch.in_diamond(x, y) {
                continue;
            }
            if ctx.scratch.get(x, y).region != 0 {
                return false;
            }
            if !ctx.ids.is_clear(ctx.grid.cell_native(x, y).tile) {
                return false;
            }
        }
    }

    // The near fill happens before the growth is attempted, so a failed
    // placement still leaves these four ranks as river water.
    fill_water(ctx, layout.fill_near, args.region);

    let arm = meander::MeanderArgs {
        tag: args.region,
        step_density: PLATEAU_STEP_DENSITY,
        rect: layout.clamp,
        reference: layout.seed,
        claim_frontier: false,
        pool_dims: args.pool_dims,
    };
    if !meander::grow_meander_arm(ctx, &arm) {
        return false;
    }

    let shore_ok = {
        let mut shore_ctx = ShoreCtx {
            grid: ctx.grid,
            scratch: ctx.scratch,
            ids: ctx.ids,
            blocks: ctx.blocks,
            rng: ctx.rng,
        };
        shore::run(&mut shore_ctx, args.region, false)
    };
    if !shore_ok {
        return false;
    }

    if !meander::dilate_chained(ctx, args.region, PLATEAU_DILATE_RINGS, None, layout.clamp) {
        return false;
    }

    // The near-side plateau: everything the region claimed rises one level
    // step. This is the opposite predicate from the canyon's sweep, which
    // raises the cells *outside* its region.
    let cells: Vec<(i32, i32)> = ctx.grid.native_cells().collect();
    for &(x, y) in &cells {
        if ctx.scratch.get(x, y).region != args.region {
            continue;
        }
        let cell = ctx.grid.get_mut(x, y).expect("native cell");
        cell.level = cell.level.wrapping_add(4);
    }

    fill_water(ctx, layout.fill_far, args.region);

    stamp_waterfall_pieces(ctx, args, &layout);
    true
}

/// The unassigned-tile sentinel written beside waterfall approach cells. It is
/// the port's own unassigned value, which is what makes those cells read as
/// bare ground to every later pass — that equivalence is what lets a finished
/// river's shore pass cross the junction.
const SENTINEL: i32 = crate::map::rmg::tiles::TILE_UNASSIGNED;

/// One level step, shared by the plateau raise and waterfall adjustments.
const LEVEL_STEP: u8 = 4;

/// Stamp one waterfall tile block at `anchor`.
///
/// This is the iso-tile stamper's waterfall path: the overwrite range covers
/// tile, so water is stamped over freely; unowned or foreign bare ground is
/// adopted into `region`; a foreign non-clear cell refuses hard unless it
/// holds an equivalent shore piece, in which case the whole call succeeds
/// immediately with the rest of the block left unstamped. Each written cell
/// gets the tile, its sub-tile index, and the block's own height on top of
/// `level_base`.
fn stamp_block(
    ctx: &mut BlobCtx<'_>,
    anchor: (i32, i32),
    tile: i32,
    level_base: i32,
    region: i32,
) -> bool {
    let Some(block) = ctx.blocks.block(tile) else {
        // Unknown block: the original silently no-ops.
        return true;
    };
    let block = block.clone();
    for j in 0..block.height {
        for i in 0..block.width {
            let (x, y) = (anchor.0 + i, anchor.1 + j);
            if !ctx.scratch.in_diamond(x, y) {
                continue;
            }
            let index = (block.width * j + i) as usize;
            let Some(sub) = block.subtiles.get(index).copied().flatten() else {
                continue;
            };
            let owner = ctx.scratch.get(x, y).region;
            let target = ctx.grid.cell_native(x, y).tile;
            let clear = ctx.ids.is_clear(target);
            if owner < 1 {
                if clear {
                    ctx.scratch.get_mut(x, y).region = region;
                } else {
                    return false;
                }
            } else if owner != region {
                if clear {
                    ctx.scratch.get_mut(x, y).region = region;
                } else if ctx.ids.is_shore_piece(target) && ctx.ids.is_shore_piece(tile) {
                    // Equivalence would compare piece groups here; this path
                    // never stamps shore pieces, so this arm cannot be taken
                    // by any live caller. Kept for shape, refusing is safer
                    // than guessing a group table result.
                    return false;
                } else {
                    return false;
                }
            }
            let cell = ctx.grid.cell_native_mut(x, y);
            cell.tile = tile;
            cell.sub_tile = index as u8;
            cell.level = (i32::from(sub.height) + level_base) as u8;
            cell.slope = sub.slope;
        }
    }
    true
}

/// A flank cell beside a waterfall end: unassigned, one level up, adopted.
fn flank(ctx: &mut BlobCtx<'_>, x: i32, y: i32, region: i32) {
    if !ctx.scratch.in_diamond(x, y) {
        return;
    }
    let cell = ctx.grid.cell_native_mut(x, y);
    cell.tile = SENTINEL;
    cell.level = cell.level.wrapping_add(LEVEL_STEP);
    cell.sub_tile = 0;
    ctx.scratch.get_mut(x, y).region = region;
}

/// Stamp the crossing out of the heading's waterfall set.
///
/// Four pieces per set: ends at +0 and +3, a two-cell middle at +2 and a
/// one-cell middle at +1, chosen by the parity of the remaining span. The four
/// heading cases are hand-written in the original and are not rotations: N and
/// W carry a level-lowering loop along the end row the E and S cases lack, N
/// and W write bare-tile sentinels E and S do not, and the W case's second
/// sentinel uses a fixed offset where N's uses the span — transcribed as
/// found.
///
/// The stamp refusals are deliberately ignored here: the original discards
/// every ok-flag on this path, so an incomplete waterfall stamp still
/// counts as placed.
fn stamp_waterfall_pieces(ctx: &mut BlobCtx<'_>, args: &BridgeArgs, _layout: &Layout) {
    let base = ctx.ids.special.waterfalls[args.heading_dir / 2];
    if base < 0 {
        // No waterfall set in this theater: nothing to stamp. The placement
        // itself stands, exactly as if every stamp had refused.
        return;
    }
    let region = args.region;
    let (fx, fy) = args.first;
    let (lx, ly) = args.last;

    match args.heading_dir {
        0 => {
            let (ax, ay) = (fx, fy);
            let span = lx - ax;
            let level = i32::from(ctx.grid.cell_native(ax - 2, ay - 8).level as i8);
            let _ = stamp_block(ctx, (ax - 2, ay - 6), base, level, region);
            let _ = stamp_block(ctx, (ax + span + 1, ay - 6), base + 3, level, region);
            ctx.grid.cell_native_mut(ax - 1, ay - 6).tile = SENTINEL;
            ctx.grid.cell_native_mut(ax + span + 1, ay - 6).tile = SENTINEL;
            for i in 1..=(span + 3) {
                let cell = ctx.grid.cell_native_mut(ax - 2 + i, ay - 6);
                cell.level = cell.level.wrapping_sub(LEVEL_STEP);
            }
            flank(ctx, ax - 3, ay - 5, region);
            flank(ctx, ax + span + 3, ay - 5, region);
            let mut remaining = span + 1;
            let mut i = 0;
            while i < span + 1 {
                let anchor = (ax + i, ay - 5);
                if remaining & 1 == 0 {
                    let _ = stamp_block(ctx, anchor, base + 2, level, region);
                    i += 2;
                    remaining -= 2;
                } else {
                    let _ = stamp_block(ctx, anchor, base + 1, level, region);
                    i += 1;
                    remaining -= 1;
                }
            }
        }
        2 => {
            let (ax, ay) = (fx, fy);
            let span = ly - ay;
            let south = ay + span + 1;
            let level = i32::from(ctx.grid.cell_native(ax + 7, south).level as i8);
            let _ = stamp_block(ctx, (ax + 5, south), base, level, region);
            let _ = stamp_block(ctx, (ax + 5, ay - 2), base + 3, level, region);
            flank(ctx, ax + 5, south + 2, region);
            flank(ctx, ax + 5, ay - 3, region);
            let mut remaining = span + 1;
            let mut i = 0;
            while i < span + 1 {
                if remaining & 1 == 0 {
                    let _ = stamp_block(ctx, (ax + 5, south - (i + 2)), base + 2, level, region);
                    i += 2;
                    remaining -= 2;
                } else {
                    let _ = stamp_block(ctx, (ax + 5, south - (i + 1)), base + 1, level, region);
                    i += 1;
                    remaining -= 1;
                }
            }
        }
        4 => {
            let (ax, ay) = (lx, ly);
            let span = fx - ax;
            let row = ay + 5;
            let level = i32::from(ctx.grid.cell_native(ax - 2, ay + 7).level as i8);
            let _ = stamp_block(ctx, (ax - 2, row), base, level, region);
            let _ = stamp_block(ctx, (ax + span + 1, row), base + 3, level, region);
            flank(ctx, ax - 3, row, region);
            flank(ctx, ax + span + 3, row, region);
            let mut remaining = span + 1;
            let mut i = 0;
            while i < span + 1 {
                let anchor = (ax + i, row);
                if remaining & 1 == 0 {
                    let _ = stamp_block(ctx, anchor, base + 2, level, region);
                    i += 2;
                    remaining -= 2;
                } else {
                    let _ = stamp_block(ctx, anchor, base + 1, level, region);
                    i += 1;
                    remaining -= 1;
                }
            }
        }
        _ => {
            let (ax, ay) = (lx, ly);
            let span = fy - ay;
            let south = ay + span + 1;
            let col = ax - 6;
            let level = i32::from(ctx.grid.cell_native(ax - 8, south).level as i8);
            let _ = stamp_block(ctx, (col, south), base, level, region);
            let _ = stamp_block(ctx, (col, ay - 2), base + 3, level, region);
            ctx.grid.cell_native_mut(col, south - 1).tile = SENTINEL;
            // The original computes this sentinel's offset from the far
            // fill's fixed width (8) + 2, not from the span like the north
            // case does — a hand-written asymmetry, kept as found.
            ctx.grid.cell_native_mut(col, south + 10).tile = SENTINEL;
            for i in 0..=(span + 2) {
                let cell = ctx.grid.cell_native_mut(col, south - i);
                cell.level = cell.level.wrapping_sub(LEVEL_STEP);
            }
            flank(ctx, col + 1, south + 2, region);
            flank(ctx, col + 1, ay - 3, region);
            let mut remaining = span + 1;
            let mut i = 0;
            while i < span + 1 {
                if remaining & 1 == 0 {
                    let _ = stamp_block(ctx, (col + 1, south - (i + 2)), base + 2, level, region);
                    i += 2;
                    remaining -= 2;
                } else {
                    let _ = stamp_block(ctx, (col + 1, south - (i + 1)), base + 1, level, region);
                    i += 1;
                    remaining -= 1;
                }
            }
        }
    }
}

/// The success jump, per heading: `(dx, dy)` in cells.
pub(crate) fn jump(heading_dir: usize) -> (i32, i32) {
    match heading_dir {
        0 => (0, -JUMP_CELLS),
        2 => (JUMP_CELLS, 0),
        4 => (0, JUMP_CELLS),
        _ => (-JUMP_CELLS, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::grid::RmgGrid;
    use crate::map::rmg::{RmgConstructionTrace, emit};
    use crate::map::rmg::options::RmgOptions;
    use crate::map::rmg::phases::shore::{SubTile, TileBlock, TileBlocks};
    use crate::map::rmg::rng::RmgRng;
    use crate::map::rmg::scratch::RmgScratch;
    use crate::map::rmg::tiles::SpecialTerrain;
    use crate::map::rmg::tiles::{TILE_UNASSIGNED, TileIds};
    use crate::map::rmg::x87::Gaussian;
    use crate::map::resolved_terrain::ResolvedTerrainGrid;

    struct OneByOne(TileBlock);
    impl TileBlocks for OneByOne {
        fn block(&self, _tile: i32) -> Option<&TileBlock> {
            Some(&self.0)
        }
    }

    fn harness() -> (RmgGrid, RmgScratch, TileIds, OneByOne) {
        let (dmin, dmax) = (34, 34 + 2 * 42);
        let stride = (34 + 42 + 1) as usize;
        let mut grid = RmgGrid::new(stride, dmin, dmax);
        let scratch = RmgScratch::new(stride, dmin, dmax);
        let mut ids = TileIds {
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
            special: SpecialTerrain::default(),
        };
        ids.special.waterfalls = [600, 610, 620, 630];
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            grid.get_mut(x, y).expect("native cell").tile = ids.clear;
        }
        let blocks = OneByOne(TileBlock {
            width: 1,
            height: 1,
            subtiles: vec![Some(SubTile {
                height: 0,
                terrain: 0,
                slope: 0,
            })],
        });
        (grid, scratch, ids, blocks)
    }

    #[test]
    fn north_waterfall_stamps_ends_middles_and_sentinels() {
        // A hand-built north crossing: section from (40,50) to (43,50).
        // Pins the case-0 geometry cell by cell against the derived contract —
        // ends at the row two above the near fill, sentinels beside them, the
        // −4 sweep along that row, flanks one row down, and the middles
        // alternating by parity along the waterfall row.
        let (mut grid, mut scratch, ids, blocks) = harness();
        let mut rng = RmgRng::new(1);
        let mut gauss = Gaussian::default();
        let topology_before: Vec<_> = grid
            .native_cells()
            .map(|(x, y)| {
                let cell = grid.get(x, y).unwrap();
                ((x, y), cell.overlay, cell.density, cell.occupied)
            })
            .collect();
        let mut ctx = BlobCtx {
            grid: &mut grid,
            scratch: &mut scratch,
            ids: &ids,
            blocks: &blocks,
            rng: &mut rng,
            gauss: &mut gauss,
            trig: None,
            map_w: 34,
            map_h: 42,
            rollback_level: 4,
        };
        let args = BridgeArgs {
            region: 5,
            heading_dir: 0,
            first: (40, 50),
            last: (43, 50),
            pool_dims: (30, 30),
        };
        let layout = layout(&args);
        stamp_waterfall_pieces(&mut ctx, &args, &layout);

        // span = 3. End piece A: base+0 at (38,44).
        assert_eq!(ctx.grid.cell_native(38, 44).tile, 600, "end piece A");
        // End piece B is stamped at (44,44) and then the second sentinel is
        // written over the same cell — the original's order, kept as found.
        // With a real multi-cell block the rest of the piece survives; the
        // anchor cell always ends unassigned.
        assert_eq!(
            ctx.grid.cell_native(44, 44).tile,
            TILE_UNASSIGNED,
            "end B anchor"
        );
        assert_eq!(
            ctx.grid.cell_native(39, 44).tile,
            TILE_UNASSIGNED,
            "sentinel"
        );
        // The −4 sweep ran along row 44 (wrapping below the base of 4).
        assert_eq!(ctx.grid.cell_native(40, 44).level, 0, "channel row lowered");
        // Flanks at (37,45) and (46,45): unassigned, one level up, adopted.
        assert_eq!(ctx.grid.cell_native(37, 45).tile, TILE_UNASSIGNED);
        assert_eq!(ctx.grid.cell_native(37, 45).level, 8);
        assert_eq!(ctx.scratch.get(37, 45).region, 5);
        // Waterfall row 45: remaining starts at 4 (even) → two 2-cell pieces at
        // x = 40 and 42.
        assert_eq!(ctx.grid.cell_native(40, 45).tile, 602, "first middle");
        assert_eq!(ctx.grid.cell_native(42, 45).tile, 602, "second middle");
        let topology_after: Vec<_> = ctx
            .grid
            .native_cells()
            .map(|(x, y)| {
                let cell = ctx.grid.get(x, y).unwrap();
                ((x, y), cell.overlay, cell.density, cell.occupied)
            })
            .collect();
        assert_eq!(
            topology_after, topology_before,
            "waterfall terrain cannot create low overlays or occupiers"
        );
    }

    #[test]
    fn the_jump_follows_the_travel_heading() {
        assert_eq!(jump(0), (0, -12), "north");
        assert_eq!(jump(2), (12, 0), "east");
        assert_eq!(jump(4), (0, 12), "south");
        assert_eq!(jump(6), (-12, 0), "west");
    }

    #[test]
    fn direct_waterfall_build_changes_terrain_without_creating_bridge_topology() {
        // Fixed successful direct-BuildRiverBridge fixture. Active retail
        // 0x0059E740 may mutate terrain working state only: never overlay/data,
        // CellClass bridge flags, Tube topology, structures, or constructors.
        let run = || {
            let (mut grid, mut scratch, ids, mut blocks) = harness();
            blocks.0.subtiles[0].as_mut().expect("one-by-one subtile").slope = 1;
            let cells_before: Vec<_> = grid
                .native_cells()
                .map(|coord @ (x, y)| (coord, *grid.get(x, y).expect("native cell")))
                .collect();
            let topology_before: Vec<_> = cells_before
                .iter()
                .map(|(coord, cell)| {
                    (*coord, cell.overlay, cell.density, cell.occupied, cell.start_marker)
                })
                .collect();
            let scratch_before = scratch.cells().to_vec();
            let structures: Vec<(String, i16, i16)> = Vec::new();
            let construction_trace = RmgConstructionTrace::default();
            let construction_trace_before = construction_trace.clone();
            let mut rng = RmgRng::new(0);
            let mut gauss = Gaussian::default();
            {
                let mut ctx = BlobCtx {
                    grid: &mut grid,
                    scratch: &mut scratch,
                    ids: &ids,
                    blocks: &blocks,
                    rng: &mut rng,
                    gauss: &mut gauss,
                    trig: None,
                    map_w: 34,
                    map_h: 42,
                    rollback_level: 4,
                };
                let args = BridgeArgs {
                    region: 5,
                    heading_dir: 0,
                    first: (40, 50),
                    last: (43, 50),
                    pool_dims: (34, 42),
                };
                assert!(build(&mut ctx, &args), "seed 0 is the fixed success oracle");
            }

            let cells_after: Vec<_> = grid
                .native_cells()
                .map(|coord @ (x, y)| (coord, *grid.get(x, y).expect("native cell")))
                .collect();
            assert!(
                cells_before
                    .iter()
                    .zip(&cells_after)
                    .any(|((_, before), (_, after))| before.tile != after.tile),
                "successful waterfall construction changes tiles"
            );
            assert!(
                cells_before
                    .iter()
                    .zip(&cells_after)
                    .any(|((_, before), (_, after))| before.sub_tile != after.sub_tile),
                "successful waterfall construction changes sub-tiles"
            );
            assert!(
                cells_before
                    .iter()
                    .zip(&cells_after)
                    .any(|((_, before), (_, after))| before.slope != after.slope),
                "successful waterfall construction changes shore slopes"
            );
            assert!(
                cells_before
                    .iter()
                    .zip(&cells_after)
                    .any(|((_, before), (_, after))| before.level != after.level),
                "successful waterfall construction changes terrain levels"
            );
            assert_ne!(scratch.cells(), scratch_before.as_slice(), "scratch state changes");
            assert!(
                cells_after
                    .iter()
                    .any(|(_, cell)| (600..=603).contains(&cell.tile)),
                "north waterfall tile set appears"
            );

            let topology_after: Vec<_> = cells_after
                .iter()
                .map(|(coord, cell)| {
                    (*coord, cell.overlay, cell.density, cell.occupied, cell.start_marker)
                })
                .collect();
            assert_eq!(
                topology_after, topology_before,
                "waterfall terrain cannot write overlay/data, occupancy, or start markers"
            );
            assert_eq!(construction_trace, construction_trace_before);
            assert!(structures.is_empty(), "waterfall terrain constructs no buildings");

            let mut map = emit::empty_map_file(&RmgOptions::default(), 34, 42);
            emit::populate(&mut map, &grid, &[], &structures, &[]);
            assert!(map.overlays.is_empty(), "no emitted OverlayPack entries");
            assert!(
                !map
                    .overlay_data_pack()
                    .expect("new RMG map retains raw overlay data")
                    .is_present(),
                "no emitted OverlayDataPack"
            );
            assert!(map.entities.is_empty(), "no emitted structures");
            assert!(map.explicit_tubes.is_empty(), "no explicit Tube topology");

            let resolved = ResolvedTerrainGrid::build_generated_materialized_for_test(&map);
            assert!(resolved.tube_facts().is_empty(), "no synthesized low-deck tubes");
            assert!(
                resolved
                    .iter()
                    .all(|cell| cell.bridge_flags() == 0 && !cell.has_bridge_deck),
                "generated materialization has no modeled raw bridge flags or deck"
            );

            let projection: Vec<_> = map
                .cells
                .iter()
                .map(|cell| (cell.rx, cell.ry, cell.tile_index, cell.sub_tile, cell.z))
                .collect();
            (
                cells_after,
                scratch.cells().to_vec(),
                projection,
                rng.into_continuation().into_native_parts(),
            )
        };

        let first = run();
        let second = run();
        assert_eq!(first, second, "fixed input repeats output and MapGen continuation");
    }

    #[test]
    fn the_scale_constants_are_the_pre_divided_doubles() {
        // 6/(2^32-1) and 4/(2^32-1), read out of the retail image as bit
        // patterns. A recomputed division that differs in the last bit would
        // shift a draw at the acceptance boundary.
        assert_eq!(
            f64::from_bits(VARIANT_SCALE_BITS),
            6.0 / 4294967295.0_f64,
            "variant scale"
        );
        assert_eq!(
            f64::from_bits(SUB_TILE_SCALE_BITS),
            4.0 / 4294967295.0_f64,
            "sub-tile scale"
        );
    }

    #[test]
    fn north_layout_matches_the_case_table() {
        let args = BridgeArgs {
            region: 3,
            heading_dir: 0,
            first: (40, 50),
            last: (42, 50),
            pool_dims: (30, 30),
        };
        let l = layout(&args);
        assert_eq!(l.clearance, [38, 38, 7, 12]);
        assert_eq!(l.fill_near, [40, 46, 3, 4]);
        assert_eq!(l.fill_far, [40, 38, 3, 8]);
        assert_eq!(l.clamp, [0, 46, 0x200, 0x200 - 46]);
        assert_eq!(l.seed, (40, 46));
    }

    #[test]
    fn west_layout_anchors_on_the_last_cell() {
        let args = BridgeArgs {
            region: 3,
            heading_dir: 6,
            first: (60, 44),
            last: (60, 41),
            pool_dims: (30, 30),
        };
        let l = layout(&args);
        assert_eq!(l.clearance, [48, 39, 12, 8]);
        assert_eq!(l.fill_near, [56, 41, 4, 4]);
        assert_eq!(l.fill_far, [48, 41, 8, 4]);
        assert_eq!(l.clamp, [56, 0, 0x200 - 56, 0x200]);
        assert_eq!(l.seed, (56, 41));
    }
}
