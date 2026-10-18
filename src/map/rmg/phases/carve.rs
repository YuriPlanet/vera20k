//! Cutting a cliff stair into the ground.
//!
//! gamemd: `RmgRegion::CarveConnectorsOrBridges` 0x005905D0,
//! `BuildRampOrientationMask` 0x00590FD0, the four straight-ramp carvers
//! `CarveStraightRamp_ClearSouth` 0x005910F0, `_ClearEast` 0x00591740,
//! `_ClearNorth` 0x00591D80, `_ClearWest` 0x00592440, and the two corner
//! carvers `CarveCornerRamp_Diagonal` 0x00593030 and `_Reflected` 0x00593550.
//! Reached by active stock `.SED` generation on random-map types 3 and 4.
//!
//! [`carve_straight_ramp_clear_south`] is the first of the original's seven
//! carve routines. It takes the two endpoints a ramp site chose and turns the
//! ground between them into a walkable stair: two rectangles handed to the
//! lower plateau, two to the upper, a tile block stamped at each end, then the
//! stepped edge itself and the flat run that leads onto it.
//!
//! Two things here are easy to get wrong and expensive to notice:
//!
//! - **The four fill loops split both the region id and the level**, in matched
//!   pairs. Each half of the corridor is handed to the plateau it belongs to.
//!   Tagging the whole corridor with one id compiles, looks right, and hands
//!   half the ramp to the wrong plateau — which only shows up much later, when
//!   starts and tiberium read those ids.
//! - **The end blocks come from a different tileset base than the stepped
//!   edge.** Conflating them stamps plausible-looking wrong tiles.
//!   End blocks read `SlopeSetPieces`/`DAT_00ABC1F8`; stepped tiles read
//!   `RampBase`/`DAT_00ABC1D8`. Original ReadInt545776 uses default−1,
//!   and publication545E1D/545E26 resolves the ordinal to a cumulative base.
//!   Source: `tools/rules_oracle/theater_general_reader.{py,json,md}`.
//!
//! Driven by `carve_driver`, which runs on the two island map types.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::preview::Playfield;
use crate::map::rmg::rng::RmgRng;
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;

use super::connector::RampShape;
use super::ramp::{RAMP_STEPS, ramp_record, ramp_tile, rect_is_carveable};
use super::shore::TileBlocks;

/// Everything a carve borrows.
pub struct CarveCtx<'a> {
    pub grid: &'a mut RmgGrid,
    pub scratch: &'a mut RmgScratch,
    pub ids: &'a TileIds,
    pub blocks: &'a dyn TileBlocks,
    pub rng: &'a mut RmgRng,
    pub playfield: &'a Playfield,
}

/// The two plateaus a carve joins.
///
/// `lower_level` is looked up from the region list on every cell in the
/// original. Nothing in the carve mutates that list, so resolving it once up
/// front is the same value every time — but it is a *lookup*, not the caller's
/// own level, and the two differ.
#[derive(Debug, Clone, Copy)]
pub struct CarveRegions {
    pub region: i32,
    pub level: u8,
    pub lower_region: i32,
    pub lower_level: u8,
}

/// Clearance the ramp needs along its run beyond the endpoint offset.
const RUN_CLEARANCE: i32 = 5;
/// Cells the flat run is deep.
const FLAT_RUN_DEPTH: i32 = 4;
/// Slope byte and tile offset the flat run stamps.
const FLAT_RUN_SLOPE: u8 = 4;
const FLAT_RUN_TILE_OFFSET: i32 = 3;

/// Stamp a tile block, writing every sub-tile that the block defines.
///
/// Original5A6C10 copies the TMP subtile, identity, optional height and slope
/// into Cell11A/38/11B/11C, then the scratch region. Its actual material comes
/// from the theater-owned bases described above; no RNG is drawn here.
///
/// The unconditional stamper: unlike the shore tiler's, it asks nothing about
/// what is already in the cell. A `level_base` of `None` leaves the cell's
/// level alone — the low-bridge deck uses that; a ramp never does.
pub(crate) fn stamp_iso_block(
    ctx: &mut CarveCtx<'_>,
    tile: i32,
    origin: (i32, i32),
    scratch_id: i32,
    level_base: Option<u8>,
) {
    let Some(block) = ctx.blocks.block(tile) else {
        // Unknown block: the original stamps nothing and says nothing.
        return;
    };
    let block = block.clone();
    for row in 0..block.height {
        for col in 0..block.width {
            let (x, y) = (origin.0 + col, origin.1 + row);
            if !ctx.scratch.in_diamond(x, y) {
                continue;
            }
            let index = (block.width * row + col) as usize;
            let Some(sub) = block.subtiles.get(index).copied().flatten() else {
                continue;
            };
            let cell = ctx.grid.cell_native_mut(x, y);
            cell.sub_tile = index as u8;
            cell.tile = tile;
            if let Some(base) = level_base {
                cell.level = sub.height.wrapping_add(base).wrapping_sub(4);
            }
            cell.slope = sub.slope;
            ctx.scratch.get_mut(x, y).region = scratch_id;
        }
    }
}

/// Paint one fill rectangle: claim each in-bounds cell for `region` and set its
/// level.
fn fill(ctx: &mut CarveCtx<'_>, xs: (i32, i32), ys: (i32, i32), region: i32, level: u8) {
    for y in ys.0..ys.1 {
        for x in xs.0..xs.1 {
            if !ctx.scratch.in_diamond(x, y) {
                continue;
            }
            ctx.scratch.get_mut(x, y).region = region;
            ctx.grid.cell_native_mut(x, y).level = level;
        }
    }
}

/// Carve the east-west stair whose south side is clear.
///
/// `a` is the west endpoint and `b` the east one; the shape guarantees
/// `b.x > a.x`. Returns whether anything was carved — a refusal writes no cell
/// and takes no draw, which is what lets the caller retry cheaply.
pub fn carve_straight_ramp_clear_south(
    ctx: &mut CarveCtx<'_>,
    regions: CarveRegions,
    a: (i32, i32),
    b: (i32, i32),
) -> bool {
    let (min_y, max_y) = (a.1.min(b.1), a.1.max(b.1));
    let rect = (a.0 - 5, min_y - 3, (b.0 - a.0) + 9, (max_y - min_y) + 11);
    if !rect_is_carveable(
        ctx.grid,
        ctx.scratch,
        ctx.ids,
        ctx.playfield,
        rect,
        regions.region,
        regions.lower_region,
    ) {
        return false;
    }

    let dy = b.1 - a.1;
    let run = dy.abs();
    // Not enough length to fit the stair's rise. Refused before any draw.
    if (b.0 - a.0) - RUN_CLEARANCE < run {
        return false;
    }

    let half = ((b.0 - a.0) + 1) / 2 + 3;
    let (rx, ry, _, rh) = rect;
    let bottom = ry + rh + 2;

    // The lower plateau takes the two southern rectangles, with its own id and
    // its own level; the upper plateau takes the two northern ones. Both axes
    // split together — see the module note.
    fill(
        ctx,
        (rx, rx + 2 + half),
        (a.1 + 1, bottom),
        regions.lower_region,
        regions.lower_level,
    );
    fill(
        ctx,
        (b.0 - half + 3, b.0 + 5),
        (b.1 + 1, bottom),
        regions.lower_region,
        regions.lower_level,
    );
    fill(
        ctx,
        (rx + 4, rx + 2 + half),
        (ry + 2, a.1 + 1),
        regions.region,
        regions.level,
    );
    fill(
        ctx,
        (b.0 - half + 2, b.0 + 1),
        (ry + 2, b.1 + 1),
        regions.region,
        regions.level,
    );

    // The two end blocks. Consecutive tiles from the ramp-end base, which is
    // NOT the base the stepped edge below uses.
    let end_base = ctx.ids.slope_set_pieces;
    stamp_iso_block(ctx, end_base, a, regions.lower_region, Some(regions.level));
    stamp_iso_block(
        ctx,
        end_base + 1,
        (b.0 - 2, b.1),
        regions.lower_region,
        Some(regions.level),
    );

    let record = ramp_record(RampShape::ClearSouth).expect("straight shape has a table");
    let mut shift = (0i32, 0i32);

    if dy != 0 {
        // The one draw. It decides whether the flat run is displaced to meet
        // the stair, or the stair is slid along to meet the run.
        let coin = super::connector::jitter(ctx.rng);
        let ascending = dy > 0;
        let y_adjust = if dy >= 0 { 0 } else { -1 };
        let mut x_slide = 0;
        if coin == 1 {
            shift = (run, dy);
        } else {
            x_slide = (b.0 - run - a.0) - RUN_CLEARANCE;
        }
        let slopes = record.slopes(dy);

        for i in 0..run {
            // x advances with the outer counter and y with the inner — the
            // stair walks east one column at a time, each column a full run of
            // steps top to bottom.
            let lift = if ascending { i } else { -i };
            for k in 0..RAMP_STEPS as i32 {
                let x = a.0 + 3 + i + x_slide;
                let y = a.1 + k + y_adjust + lift;
                let slope = slopes[k as usize];
                let cell = ctx.grid.cell_native_mut(x, y);
                cell.level = (record.level_steps[k as usize])
                    .wrapping_add(regions.level as i8)
                    .wrapping_sub(4) as u8;
                cell.slope = slope;
                cell.sub_tile = 0;
                cell.tile = ramp_tile(ctx.ids.ramp_base, slope);
            }
        }
    }

    // The flat run that leads onto the stair. Its scratch id is written
    // directly rather than through the claim helper, and it is the lower
    // plateau's — the run belongs to the ground you walk in from.
    let flat_len = (b.0 - a.0) - run - RUN_CLEARANCE;
    for i in 0..flat_len {
        for k in 0..FLAT_RUN_DEPTH {
            let x = a.0 + 3 + i + shift.0;
            let y = a.1 + k + shift.1;
            if ctx.scratch.in_diamond(x, y) {
                ctx.scratch.get_mut(x, y).region = regions.lower_region;
            }
            let cell = ctx.grid.cell_native_mut(x, y);
            cell.slope = FLAT_RUN_SLOPE;
            cell.level = (regions.level as i8).wrapping_sub(k as i8).wrapping_sub(1) as u8;
            cell.tile = ctx.ids.ramp_base + FLAT_RUN_TILE_OFFSET;
            cell.sub_tile = 0;
        }
    }

    true
}

/// Carve the north-south stair whose east side is clear.
///
/// The mirror of [`carve_straight_ramp_clear_south`] across the isometric
/// diagonal: the run is along Y rather than X, so `a` is the **south** endpoint
/// and `b` the north one, and the stepped edge advances its column with the
/// inner counter instead of the outer.
///
/// Two constants genuinely differ rather than just swapping axes, and both were
/// transcribed rather than assumed: the half-span carries **no `+3`** here, and
/// the end blocks are the *next pair* of tiles after the east-west routine's.
pub fn carve_straight_ramp_clear_east(
    ctx: &mut CarveCtx<'_>,
    regions: CarveRegions,
    a: (i32, i32),
    b: (i32, i32),
) -> bool {
    let (min_x, max_x) = (a.0.min(b.0), a.0.max(b.0));
    let rect = (min_x - 3, b.1 - 4, (max_x - min_x) + 11, (a.1 - b.1) + 9);
    if !rect_is_carveable(
        ctx.grid,
        ctx.scratch,
        ctx.ids,
        ctx.playfield,
        rect,
        regions.region,
        regions.lower_region,
    ) {
        return false;
    }

    let dx = b.0 - a.0;
    let run = dx.abs();
    let length = a.1 - b.1;
    if length - RUN_CLEARANCE < run {
        return false;
    }

    // No `+ 3` — the east-west routine adds one and this one does not.
    let half = (length + 1) / 2;
    let (rx, ry, _, _) = rect;

    // Same pairing as the east-west routine, whose split was read out of the
    // assembly: the two rectangles that take the lower plateau's level take its
    // id as well, and the other two take the caller's.
    fill(
        ctx,
        (a.0 + 1, a.0 + 8),
        (a.1 - half, a.1 + 5),
        regions.lower_region,
        regions.lower_level,
    );
    fill(
        ctx,
        (b.0 + 1, b.0 + 8),
        (ry, b.1 + half),
        regions.lower_region,
        regions.lower_level,
    );
    fill(
        ctx,
        (rx + 2, a.0 + 1),
        (a.1 - half, a.1 + 1),
        regions.region,
        regions.level,
    );
    fill(
        ctx,
        (rx + 2, b.0 + 1),
        (b.1, b.1 + half),
        regions.region,
        regions.level,
    );

    // The next pair of end-block tiles after the east-west routine's, not the
    // same two.
    let end_base = ctx.ids.slope_set_pieces;
    stamp_iso_block(
        ctx,
        end_base + 2,
        (a.0, a.1 - 2),
        regions.lower_region,
        Some(regions.level),
    );
    stamp_iso_block(
        ctx,
        end_base + 3,
        b,
        regions.lower_region,
        Some(regions.level),
    );

    let record = ramp_record(RampShape::ClearEast).expect("straight shape has a table");
    let mut shift = (0i32, 0i32);

    if dx != 0 {
        let coin = super::connector::jitter(ctx.rng);
        let eastward = dx > 0;
        let x_adjust = if dx >= 0 { 0 } else { -1 };
        let mut y_slide = 0;
        if coin == 1 {
            shift = (dx, -run);
        } else {
            y_slide = -((a.1 - run - b.1) - RUN_CLEARANCE);
        }
        let slopes = record.slopes(dx);

        for i in 0..run {
            // Mirrored from the east-west routine: here y advances with the
            // outer counter and x with the inner, so each ROW of the stair is a
            // full run of steps.
            let lift = if eastward { i } else { -i };
            for k in 0..RAMP_STEPS as i32 {
                let x = a.0 + k + x_adjust + lift;
                let y = a.1 - 3 - i + y_slide;
                let slope = slopes[k as usize];
                let cell = ctx.grid.cell_native_mut(x, y);
                cell.level = (record.level_steps[k as usize])
                    .wrapping_add(regions.level as i8)
                    .wrapping_sub(4) as u8;
                cell.slope = slope;
                cell.sub_tile = 0;
                cell.tile = ramp_tile(ctx.ids.ramp_base, slope);
            }
        }
    }

    // The flat run. Its slope byte and tile offset are one lower than the
    // east-west routine's — a different face of the same stair set.
    let flat_len = length - run - RUN_CLEARANCE;
    for i in 0..flat_len {
        for k in 0..FLAT_RUN_DEPTH {
            let x = a.0 + k + shift.0;
            let y = a.1 - 3 - i + shift.1;
            if ctx.scratch.in_diamond(x, y) {
                ctx.scratch.get_mut(x, y).region = regions.lower_region;
            }
            let cell = ctx.grid.cell_native_mut(x, y);
            cell.slope = FLAT_RUN_SLOPE - 1;
            cell.level = (regions.level as i8).wrapping_sub(k as i8).wrapping_sub(1) as u8;
            cell.tile = ctx.ids.ramp_base + FLAT_RUN_TILE_OFFSET - 1;
            cell.sub_tile = 0;
        }
    }

    true
}

/// Carve the east-west stair whose north side is clear.
///
/// Runs along X like [`carve_straight_ramp_clear_south`], but from the **east**
/// endpoint: `a` is east and `b` west, and the stepped edge climbs northward, so
/// its rows are laid out by subtracting from `b`'s row rather than adding.
///
/// Three end blocks, not two — the end-block tiles are handed out running, not
/// two per routine, so this one takes the fifth through seventh.
pub fn carve_straight_ramp_clear_north(
    ctx: &mut CarveCtx<'_>,
    regions: CarveRegions,
    a: (i32, i32),
    b: (i32, i32),
) -> bool {
    let (min_y, max_y) = (a.1.min(b.1), a.1.max(b.1));
    let rect = (b.0 - 4, min_y - 7, (a.0 - b.0) + 8, (max_y - min_y) + 11);
    if !rect_is_carveable(
        ctx.grid,
        ctx.scratch,
        ctx.ids,
        ctx.playfield,
        rect,
        regions.region,
        regions.lower_region,
    ) {
        return false;
    }

    let dy = b.1 - a.1;
    let run = dy.abs();
    let length = a.0 - b.0;
    if length - RUN_CLEARANCE < run {
        return false;
    }

    let half = (length + 1) / 2 + 3;
    let (rx, ry, _, rh) = rect;
    let far = ry + rh - 2;

    fill(
        ctx,
        (rx, rx + 2 + half),
        (ry, b.1),
        regions.lower_region,
        regions.lower_level,
    );
    fill(
        ctx,
        (a.0 - half + 2, a.0 + 5),
        (ry, a.1),
        regions.lower_region,
        regions.lower_level,
    );
    fill(
        ctx,
        (b.0, b.0 - 2 + half),
        (b.1, far),
        regions.region,
        regions.level,
    );
    fill(
        ctx,
        (a.0 - half + 2, a.0 + 1),
        (a.1, far),
        regions.region,
        regions.level,
    );

    let end_base = ctx.ids.slope_set_pieces;
    stamp_iso_block(
        ctx,
        end_base + 4,
        (b.0, b.1 - 3),
        regions.lower_region,
        Some(regions.level),
    );
    stamp_iso_block(
        ctx,
        end_base + 5,
        (a.0 - 2, a.1 - 3),
        regions.lower_region,
        Some(regions.level),
    );
    stamp_iso_block(
        ctx,
        end_base + 6,
        (a.0 - 1, a.1 - 1),
        regions.lower_region,
        Some(regions.level),
    );

    let record = ramp_record(RampShape::ClearNorth).expect("straight shape has a table");
    let mut shift = (0i32, 0i32);

    if dy != 0 {
        let coin = super::connector::jitter(ctx.rng);
        let descending = dy > 0;
        // Opposite sign convention to the south-facing routine: that one drops a
        // row when the lean is negative, this one lifts one.
        let y_bump = i32::from(dy < 0);
        let mut x_slide = 0;
        if coin == 1 {
            shift = (run, -dy);
        } else {
            x_slide = ((a.0 - run) - b.0) - RUN_CLEARANCE;
        }
        let slopes = record.slopes(dy);

        for i in 0..run {
            // Also opposite: here a positive lean steps the column back, where
            // the south-facing routine steps it forward.
            let lift = if descending { -i } else { i };
            for k in 0..RAMP_STEPS as i32 {
                let x = b.0 + 3 + i + x_slide;
                let y = b.1 - k + y_bump + lift;
                let slope = slopes[k as usize];
                let cell = ctx.grid.cell_native_mut(x, y);
                cell.level = (record.level_steps[k as usize])
                    .wrapping_add(regions.level as i8)
                    .wrapping_sub(4) as u8;
                cell.slope = slope;
                cell.sub_tile = 0;
                cell.tile = ramp_tile(ctx.ids.ramp_base, slope);
            }
        }
    }

    let flat_len = length - run - RUN_CLEARANCE;
    for i in 0..flat_len {
        for k in 0..FLAT_RUN_DEPTH {
            let x = b.0 + 3 + i + shift.0;
            let y = b.1 - k + shift.1;
            if ctx.scratch.in_diamond(x, y) {
                ctx.scratch.get_mut(x, y).region = regions.lower_region;
            }
            let cell = ctx.grid.cell_native_mut(x, y);
            cell.slope = FLAT_RUN_SLOPE - 2;
            cell.level = (regions.level as i8).wrapping_sub(k as i8).wrapping_sub(1) as u8;
            cell.tile = ctx.ids.ramp_base + FLAT_RUN_TILE_OFFSET - 2;
            cell.sub_tile = 0;
        }
    }

    true
}

/// Carve the north-south stair whose west side is clear.
///
/// The last of the four straights. Runs along Y from the **north** endpoint, so
/// `a` is north and `b` south, and its stepped edge lays each row **westward** —
/// x decreases as the step index rises, where every other routine increases it.
///
/// Takes the last three end-block tiles, the eighth through tenth.
pub fn carve_straight_ramp_clear_west(
    ctx: &mut CarveCtx<'_>,
    regions: CarveRegions,
    a: (i32, i32),
    b: (i32, i32),
) -> bool {
    let (min_x, max_x) = (a.0.min(b.0), a.0.max(b.0));
    let rect = (min_x - 7, a.1 - 4, (max_x - min_x) + 11, (b.1 - a.1) + 9);
    if !rect_is_carveable(
        ctx.grid,
        ctx.scratch,
        ctx.ids,
        ctx.playfield,
        rect,
        regions.region,
        regions.lower_region,
    ) {
        return false;
    }

    let dx = b.0 - a.0;
    let run = dx.abs();
    let length = b.1 - a.1;
    if length - RUN_CLEARANCE < run {
        return false;
    }

    // No `+ 3`, matching the other along-Y routine. The two along-X routines
    // both add it; these two both do not.
    let half = (length + 1) / 2;
    let (rx, ry, rw, _) = rect;
    let far = rx + rw - 1;
    let south_start = b.1 - half + 1;

    fill(
        ctx,
        (rx, a.0),
        (ry, ry + half + 4),
        regions.lower_region,
        regions.lower_level,
    );
    fill(
        ctx,
        (rx, b.0),
        (south_start, b.1 + 5),
        regions.lower_region,
        regions.lower_level,
    );
    fill(
        ctx,
        (a.0, far),
        (a.1, a.1 + half + 1),
        regions.region,
        regions.level,
    );
    fill(
        ctx,
        (b.0, far),
        (south_start, b.1 + 1),
        regions.region,
        regions.level,
    );

    let end_base = ctx.ids.slope_set_pieces;
    stamp_iso_block(
        ctx,
        end_base + 7,
        (a.0 - 3, a.1),
        regions.lower_region,
        Some(regions.level),
    );
    stamp_iso_block(
        ctx,
        end_base + 8,
        (b.0 - 3, b.1 - 2),
        regions.lower_region,
        Some(regions.level),
    );
    stamp_iso_block(
        ctx,
        end_base + 9,
        (b.0 - 1, b.1 - 1),
        regions.lower_region,
        Some(regions.level),
    );

    let record = ramp_record(RampShape::ClearWest).expect("straight shape has a table");
    let mut shift = (0i32, 0i32);

    if dx != 0 {
        let coin = super::connector::jitter(ctx.rng);
        let eastward = dx > 0;
        // Bumps on a POSITIVE lean here; the north-facing routine bumps on a
        // negative one and the south-facing routine subtracts instead.
        let x_bump = i32::from(dx > 0);
        let mut y_slide = 0;
        if coin == 1 {
            shift = (dx, run);
        } else {
            y_slide = ((b.1 - run) - a.1) - RUN_CLEARANCE;
        }
        let slopes = record.slopes(dx);

        for i in 0..run {
            let lift = if eastward { i } else { -i };
            for k in 0..RAMP_STEPS as i32 {
                // x DECREASES with the step index -- unique to this routine.
                let x = a.0 - k + x_bump + lift;
                let y = a.1 + 3 + i + y_slide;
                let slope = slopes[k as usize];
                let cell = ctx.grid.cell_native_mut(x, y);
                cell.level = (record.level_steps[k as usize])
                    .wrapping_add(regions.level as i8)
                    .wrapping_sub(4) as u8;
                cell.slope = slope;
                cell.sub_tile = 0;
                cell.tile = ramp_tile(ctx.ids.ramp_base, slope);
            }
        }
    }

    // The last face: slope 1 and the base tile itself, completing the 4/3/2/1
    // walk across the four routines.
    let flat_len = length - run - RUN_CLEARANCE;
    for i in 0..flat_len {
        for k in 0..FLAT_RUN_DEPTH {
            let x = a.0 - k + shift.0;
            let y = a.1 + 3 + i + shift.1;
            if ctx.scratch.in_diamond(x, y) {
                ctx.scratch.get_mut(x, y).region = regions.lower_region;
            }
            let cell = ctx.grid.cell_native_mut(x, y);
            cell.slope = FLAT_RUN_SLOPE - 3;
            cell.level = (regions.level as i8).wrapping_sub(k as i8).wrapping_sub(1) as u8;
            cell.tile = ctx.ids.ramp_base;
            cell.sub_tile = 0;
        }
    }

    true
}

/// Carve a ramp that turns a plateau corner.
///
/// A different family from the four straights, and it shares almost none of
/// their structure:
///
/// - **It takes no random draw at all.** The straights each take one when their
///   endpoints differ across the run; a corner never does.
/// - **Two extra refusals up front:** both endpoint deltas must exceed two,
///   checked before the rect is even built.
/// - **Five fill rectangles, not four**, and three separate tail runs rather
///   than one.
/// - **Its end blocks reuse tiles the straights also use** (`+2` and `+4`), so
///   the ten-tile allocation is shared across families, not carved up between
///   them.
///
/// `a` is the outer corner (greater on both axes) and `b` the inner one. The
/// original calls this routine for two of the eight ramp shapes — the pair
/// mirrored across the isometric diagonal — with the endpoints swapped between
/// them, so the geometry here is written once and driven twice.
pub fn carve_corner_ramp(
    ctx: &mut CarveCtx<'_>,
    regions: CarveRegions,
    a: (i32, i32),
    b: (i32, i32),
) -> bool {
    let span_x = a.0 - b.0;
    let span_y = a.1 - b.1;
    // Both refusals come before the rect is built, so a corner that is too
    // tight costs nothing at all.
    if span_x <= 2 || span_y <= 2 {
        return false;
    }

    let rect = (b.0 - 2, b.1 - 7, span_x + 10, span_y + 10);
    if !rect_is_carveable(
        ctx.grid,
        ctx.scratch,
        ctx.ids,
        ctx.playfield,
        rect,
        regions.region,
        regions.lower_region,
    ) {
        return false;
    }

    let (rx, ry, rw, rh) = rect;

    // Two rectangles for the lower plateau, three for the upper.
    fill(
        ctx,
        (rx, rx + rw),
        (ry, ry + 7),
        regions.lower_region,
        regions.lower_level,
    );
    fill(
        ctx,
        (a.0 + 1, a.0 + 8),
        (ry, ry + rh),
        regions.lower_region,
        regions.lower_level,
    );
    fill(
        ctx,
        (b.0, b.0 + 1),
        (b.1, b.1 + 2),
        regions.region,
        regions.level,
    );
    fill(
        ctx,
        (a.0 - 1, a.0 + 1),
        (a.1, a.1 + 1),
        regions.region,
        regions.level,
    );
    fill(
        ctx,
        (b.0 + 1, a.0),
        (b.1 + 1, a.1),
        regions.region,
        regions.level,
    );

    let end_base = ctx.ids.slope_set_pieces;
    stamp_iso_block(
        ctx,
        end_base + 2,
        (a.0, a.1 - 2),
        regions.lower_region,
        Some(regions.level),
    );
    stamp_iso_block(
        ctx,
        end_base + 4,
        (b.0, b.1 - 3),
        regions.lower_region,
        Some(regions.level),
    );

    // Three tails. The first lays the diagonal itself, one cell per step; the
    // other two run the two straight approaches into it, and both grow one cell
    // longer per step so the corner opens out.
    for k in 0..FLAT_RUN_DEPTH {
        let cell = ctx.grid.cell_native_mut(a.0 + k, b.1 - k);
        cell.slope = 6;
        cell.level = (regions.level as i8).wrapping_sub(k as i8).wrapping_sub(1) as u8;
        cell.tile = ctx.ids.ramp_base + 5;
        cell.sub_tile = 0;
    }
    for k in 0..FLAT_RUN_DEPTH {
        for j in 0..(span_y - 3 + k) {
            let cell = ctx.grid.cell_native_mut(a.0 + k, a.1 - j - 3);
            cell.slope = 3;
            cell.level = (regions.level as i8).wrapping_sub(k as i8).wrapping_sub(1) as u8;
            cell.tile = ctx.ids.ramp_base + 2;
            cell.sub_tile = 0;
        }
    }
    for k in 0..FLAT_RUN_DEPTH {
        for j in 0..(span_x - 3 + k) {
            let cell = ctx.grid.cell_native_mut(b.0 + 3 + j, b.1 - k);
            cell.slope = 2;
            cell.level = (regions.level as i8).wrapping_sub(k as i8).wrapping_sub(1) as u8;
            cell.tile = ctx.ids.ramp_base + 1;
            cell.sub_tile = 0;
        }
    }

    true
}

/// Carve a ramp that turns the opposite corner.
///
/// Same family as [`carve_corner_ramp`] and the same five-fill, three-tail
/// shape, but reflected: `a` is north-east and `b` south-west, so the span
/// guards read `a.x - b.x` and `b.y - a.y`.
///
/// Two things are not reflections and had to be read:
///
/// - **Four end blocks, not two** (`+5`, `+6`, `+8`, `+9`).
/// - **Every face sits one lower**: slopes 5/2/1 against the other corner's
///   6/3/2, tiles `+4`/`+1`/`+0` against `+5`/`+2`/`+1`. The two corners lay
///   adjacent faces of the same set, not the same ones.
pub fn carve_corner_ramp_reflected(
    ctx: &mut CarveCtx<'_>,
    regions: CarveRegions,
    a: (i32, i32),
    b: (i32, i32),
) -> bool {
    let span_x = a.0 - b.0;
    let span_y = b.1 - a.1;
    if span_x <= 2 || span_y <= 2 {
        return false;
    }

    let rect = (b.0 - 7, a.1 - 7, span_x + 10, span_y + 10);
    if !rect_is_carveable(
        ctx.grid,
        ctx.scratch,
        ctx.ids,
        ctx.playfield,
        rect,
        regions.region,
        regions.lower_region,
    ) {
        return false;
    }

    let (rx, ry, rw, rh) = rect;

    fill(
        ctx,
        (rx, rx + rw),
        (ry, ry + 7),
        regions.lower_region,
        regions.lower_level,
    );
    fill(
        ctx,
        (rx, rx + 7),
        (ry, ry + rh),
        regions.lower_region,
        regions.lower_level,
    );
    fill(
        ctx,
        (a.0, a.0 + 1),
        (a.1, a.1 + 2),
        regions.region,
        regions.level,
    );
    fill(
        ctx,
        (b.0, b.0 + 2),
        (b.1, b.1 + 1),
        regions.region,
        regions.level,
    );
    fill(
        ctx,
        (b.0 + 1, a.0),
        (a.1 + 1, b.1),
        regions.region,
        regions.level,
    );

    let end_base = ctx.ids.slope_set_pieces;
    for (tile, origin) in [
        (end_base + 5, (a.0 - 2, a.1 - 3)),
        (end_base + 6, (a.0 - 1, a.1 - 1)),
        (end_base + 8, (b.0 - 3, b.1 - 2)),
        (end_base + 9, (b.0 - 1, b.1 - 1)),
    ] {
        stamp_iso_block(ctx, tile, origin, regions.lower_region, Some(regions.level));
    }

    // The diagonal, then the two approaches. One face lower than the other
    // corner's throughout.
    for k in 0..FLAT_RUN_DEPTH {
        let cell = ctx.grid.cell_native_mut(b.0 - k, a.1 - k);
        cell.slope = 5;
        cell.level = (regions.level as i8).wrapping_sub(k as i8).wrapping_sub(1) as u8;
        cell.tile = ctx.ids.ramp_base + 4;
        cell.sub_tile = 0;
    }
    for k in 0..FLAT_RUN_DEPTH {
        for j in 0..(span_x - 3 + k) {
            let cell = ctx.grid.cell_native_mut(a.0 - j - 3, a.1 - k);
            cell.slope = 2;
            cell.level = (regions.level as i8).wrapping_sub(k as i8).wrapping_sub(1) as u8;
            cell.tile = ctx.ids.ramp_base + 1;
            cell.sub_tile = 0;
        }
    }
    for k in 0..FLAT_RUN_DEPTH {
        for j in 0..(span_y - 3 + k) {
            let cell = ctx.grid.cell_native_mut(b.0 - k, b.1 - j - 3);
            cell.slope = 1;
            cell.level = (regions.level as i8).wrapping_sub(k as i8).wrapping_sub(1) as u8;
            cell.tile = ctx.ids.ramp_base;
            cell.sub_tile = 0;
        }
    }

    true
}

/// Carve the north-east corner — the last of the seven.
///
/// Here `a` is the inner corner and `b` the outer one, so both spans read
/// `b - a`: the reverse of both other corner routines.
///
/// Its faces are a third distinct set — slopes 8/1/4 and tiles `+7`/`+0`/`+3`,
/// against 6/3/2 and 5/2/1 for the other two. Between them the three corner
/// routines cover most of the ramp set rather than repeating one another, and
/// slope 8 appears nowhere else.
///
/// Two end blocks, `+7` and `+1` — a pair no other routine uses together.
pub fn carve_corner_ramp_north_east(
    ctx: &mut CarveCtx<'_>,
    regions: CarveRegions,
    a: (i32, i32),
    b: (i32, i32),
) -> bool {
    let span_x = b.0 - a.0;
    let span_y = b.1 - a.1;
    if span_x <= 2 || span_y <= 2 {
        return false;
    }

    let rect = (a.0 - 7, a.1 - 4, span_x + 10, span_y + 10);
    if !rect_is_carveable(
        ctx.grid,
        ctx.scratch,
        ctx.ids,
        ctx.playfield,
        rect,
        regions.region,
        regions.lower_region,
    ) {
        return false;
    }

    let (rx, ry, rw, rh) = rect;

    fill(
        ctx,
        (rx, rx + 7),
        (ry, ry + rh),
        regions.lower_region,
        regions.lower_level,
    );
    fill(
        ctx,
        (rx, rx + rw),
        (b.1 + 1, b.1 + 8),
        regions.lower_region,
        regions.lower_level,
    );
    fill(
        ctx,
        (a.0, a.0 + 2),
        (a.1, a.1 + 1),
        regions.region,
        regions.level,
    );
    fill(
        ctx,
        (b.0, b.0 + 1),
        (b.1 - 1, b.1 + 1),
        regions.region,
        regions.level,
    );
    fill(
        ctx,
        (a.0 + 1, b.0),
        (a.1 + 1, b.1),
        regions.region,
        regions.level,
    );

    let end_base = ctx.ids.slope_set_pieces;
    stamp_iso_block(
        ctx,
        end_base + 7,
        (a.0 - 3, a.1),
        regions.lower_region,
        Some(regions.level),
    );
    stamp_iso_block(
        ctx,
        end_base + 1,
        (b.0 - 2, b.1),
        regions.lower_region,
        Some(regions.level),
    );

    // The diagonal runs south-west here: x falls while y rises. Both other
    // corners have them falling together.
    for k in 0..FLAT_RUN_DEPTH {
        let cell = ctx.grid.cell_native_mut(a.0 - k, b.1 + k);
        cell.slope = 8;
        cell.level = (regions.level as i8).wrapping_sub(k as i8).wrapping_sub(1) as u8;
        cell.tile = ctx.ids.ramp_base + 7;
        cell.sub_tile = 0;
    }
    for k in 0..FLAT_RUN_DEPTH {
        for j in 0..(span_y - 3 + k) {
            let cell = ctx.grid.cell_native_mut(a.0 - k, a.1 + 3 + j);
            cell.slope = 1;
            cell.level = (regions.level as i8).wrapping_sub(k as i8).wrapping_sub(1) as u8;
            cell.tile = ctx.ids.ramp_base;
            cell.sub_tile = 0;
        }
    }
    for k in 0..FLAT_RUN_DEPTH {
        for j in 0..(span_x - 3 + k) {
            let cell = ctx.grid.cell_native_mut(b.0 - j - 3, b.1 + k);
            cell.slope = 4;
            cell.level = (regions.level as i8).wrapping_sub(k as i8).wrapping_sub(1) as u8;
            cell.tile = ctx.ids.ramp_base + 3;
            cell.sub_tile = 0;
        }
    }

    true
}

/// Try to cut a ramp at one candidate cell.
///
/// Reads the ring mask, then walks the eight ramp shapes **in the original's
/// order, moving on whenever a shape's carve refuses**. That last part is the
/// whole point and is easy to get wrong: a shape whose surroundings fit can
/// still fail its own rect precheck, and the cell then falls through to the
/// next fitting shape rather than giving up.
///
/// **The draws of every straight shape attempted are spent on the way**, two
/// apiece, whether or not that shape goes on to carve. Stopping at the first
/// shape whose guard matches would leave the rest of the map drawing from the
/// wrong place in the stream.
///
/// Past halfway through the attempt budget, four fixed-geometry fallback
/// shapes are tried after all eight have refused.
pub fn try_carve_connector_at_cell(
    ctx: &mut CarveCtx<'_>,
    regions: CarveRegions,
    cell: (i32, i32),
    leniency: f32,
) -> bool {
    use super::connector::{E, N, NE, NW, S, SE, SW, W};

    // A centre window too thin for the bar rejects the cell outright — this is
    // distinct from "the mask came back empty".
    let Some(mask) =
        super::connector::ring_orientation_mask(ctx.scratch, cell, regions.region, leniency)
    else {
        return false;
    };
    // Surrounded on all eight sides: interior ground, never an edge.
    if mask == 0xFF {
        return false;
    }

    let (x, y) = cell;

    // --- the eight shapes, in order, each falling through on refusal -------
    if mask & (N | E) == (N | E) && mask & (S | SW | W) == 0 {
        let ay = if mask & NW != 0 { y - 4 } else { y - 5 };
        let bx = if mask & SE != 0 { x + 4 } else { x + 5 };
        if carve_corner_ramp_north_east(ctx, regions, (x - 1, ay), (bx, y + 1)) {
            return true;
        }
    }
    if mask & (S | E) == (S | E) && mask & (NE | SW) == 0 {
        let ax = if mask & NE != 0 { x + 4 } else { x + 5 };
        let by = if mask & SW != 0 { y + 4 } else { y + 5 };
        if carve_corner_ramp_reflected(ctx, regions, (ax, y - 1), (x - 1, by)) {
            return true;
        }
    }
    if mask & (S | W) == (S | W) && mask & (N | NE | E) == 0 {
        let ay = if mask & SE != 0 { y + 4 } else { y + 5 };
        let bx = if mask & NW != 0 { x - 6 } else { x - 5 };
        if carve_corner_ramp(ctx, regions, (x + 1, ay), (bx, y - 1)) {
            return true;
        }
    }
    if mask & (N | W) == (N | W) && mask & (E | SE | S) == 0 {
        // The same routine as the shape above, mirrored across the isometric
        // diagonal — seven routines cover eight shapes because of this pair.
        let ax = if mask & SW != 0 { x - 4 } else { x - 5 };
        let by = if mask & NE != 0 { y - 4 } else { y - 5 };
        if carve_corner_ramp(ctx, regions, (ax, y + 1), (x + 1, by)) {
            return true;
        }
    }
    if mask & (SW | W | NW) == 0 && mask & (N | S) != 0 {
        let (mut ay, mut by) = if mask & N != 0 {
            (y - 3, y + 5)
        } else {
            (y - 4, y + 4)
        };
        if mask & S != 0 {
            ay -= 1;
            by -= 1;
        }
        let ax = x - 1 + super::connector::jitter(ctx.rng);
        let bx = x - 1 + super::connector::jitter(ctx.rng);
        if carve_straight_ramp_clear_west(ctx, regions, (ax, ay), (bx, by)) {
            return true;
        }
    }
    if mask & (NE | E | SE) == 0 && mask & (N | S) != 0 {
        let (mut ay, mut by) = if mask & N != 0 {
            (y + 5, y - 3)
        } else {
            (y + 4, y - 4)
        };
        if mask & S != 0 {
            ay -= 1;
            by -= 1;
        }
        let ax = x + 1 + super::connector::jitter(ctx.rng) - 1;
        let bx = x + 1 + super::connector::jitter(ctx.rng) - 1;
        if carve_straight_ramp_clear_east(ctx, regions, (ax, ay), (bx, by)) {
            return true;
        }
    }
    if mask & (NE | NW | N) == 0 && mask & (E | W) != 0 {
        let (mut ax, mut bx) = if mask & W != 0 {
            (x + 5, x - 3)
        } else {
            (x + 4, x - 4)
        };
        if mask & E != 0 {
            ax -= 1;
            bx -= 1;
        }
        let ay = y - 1 + super::connector::jitter(ctx.rng);
        let by = y - 1 + super::connector::jitter(ctx.rng);
        if carve_straight_ramp_clear_north(ctx, regions, (ax, ay), (bx, by)) {
            return true;
        }
    }
    if mask & (SE | S | SW) == 0 && mask & (E | W) != 0 {
        let (mut ax, mut bx) = if mask & W != 0 {
            (x - 3, x + 5)
        } else {
            (x - 4, x + 4)
        };
        if mask & E != 0 {
            ax -= 1;
            bx -= 1;
        }
        let ay = y + 1 + super::connector::jitter(ctx.rng) - 1;
        let by = y + 1 + super::connector::jitter(ctx.rng) - 1;
        if carve_straight_ramp_clear_south(ctx, regions, (ax, ay), (bx, by)) {
            return true;
        }
    }

    // --- late-attempt fallbacks, fixed geometry, no jitter ----------------
    if leniency <= super::connector::FALLBACK_LENIENCY {
        return false;
    }
    if mask & S == 0
        && carve_straight_ramp_clear_south(ctx, regions, (x - 4, y + 1), (x + 4, y + 1))
    {
        return true;
    }
    if mask & E == 0 && carve_straight_ramp_clear_east(ctx, regions, (x + 1, y + 4), (x + 1, y - 4))
    {
        return true;
    }
    // The south bit again, deliberately — the original gates the first and
    // third fallbacks on the same bit. Not a transcription slip.
    if mask & S == 0
        && carve_straight_ramp_clear_north(ctx, regions, (x + 4, y - 1), (x - 4, y - 1))
    {
        return true;
    }
    if mask & W == 0 && carve_straight_ramp_clear_west(ctx, regions, (x - 1, y - 4), (x - 1, y + 4))
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::phases::shore::{SubTile, TileBlock};
    use crate::map::rmg::tiles::SpecialTerrain;
    use std::cell::RefCell;

    const REGION: i32 = 7;
    const REGION_LEVEL: u8 = 8;
    const LOWER: i32 = 3;
    const LOWER_LEVEL: u8 = 4;
    const END_BASE: i32 = 900;

    /// A one-cell block for every tile, recording which tiles were asked for.
    struct RecordingBlocks {
        block: TileBlock,
        seen: RefCell<Vec<i32>>,
    }

    impl RecordingBlocks {
        fn new() -> Self {
            Self {
                block: TileBlock {
                    width: 1,
                    height: 1,
                    subtiles: vec![Some(SubTile {
                        height: 0,
                        terrain: 0,
                        slope: 0,
                    })],
                },
                seen: RefCell::new(Vec::new()),
            }
        }
    }

    impl TileBlocks for RecordingBlocks {
        fn block(&self, tile: i32) -> Option<&TileBlock> {
            self.seen.borrow_mut().push(tile);
            Some(&self.block)
        }
    }

    fn ids() -> TileIds {
        TileIds {
            clear: 0,
            ramp_base: 200,
            ramp_smooth: 220,
            slope_set_pieces: END_BASE,
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

    /// Flat clear ground, every cell owned by the upper region at its level.
    fn harness() -> (RmgGrid, RmgScratch) {
        let (dmin, dmax) = (34, 34 + 2 * 42);
        let stride = (34 + 42 + 1) as usize;
        let mut grid = RmgGrid::new(stride, dmin, dmax);
        let mut scratch = RmgScratch::new(stride, dmin, dmax);
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            let cell = grid.get_mut(x, y).expect("native cell");
            cell.tile = 0;
            cell.level = REGION_LEVEL;
            scratch.get_mut(x, y).region = REGION;
        }
        (grid, scratch)
    }

    fn regions() -> CarveRegions {
        CarveRegions {
            region: REGION,
            level: REGION_LEVEL,
            lower_region: LOWER,
            lower_level: LOWER_LEVEL,
        }
    }

    #[test]
    fn the_fill_rectangles_split_between_the_two_plateaus() {
        // The bug this exists to catch: handing the whole corridor to one
        // region. Both the id AND the level split, in matched pairs, so a cell
        // in the southern fill must carry the LOWER region id and level while
        // one in the northern fill carries the upper one.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let playfield = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = CarveCtx {
            grid: &mut grid,
            scratch: &mut scratch,
            ids: &ids,
            blocks: &blocks,
            rng: &mut rng,
            playfield: &playfield,
        };
        assert!(carve_straight_ramp_clear_south(
            &mut ctx,
            regions(),
            (40, 50),
            (52, 50)
        ));

        // (38, 55) is in the southern fill only.
        assert_eq!(ctx.scratch.get(38, 55).region, LOWER, "south id");
        assert_eq!(
            ctx.grid.cell_native(38, 55).level,
            LOWER_LEVEL,
            "south level"
        );
        // (40, 49) is in the northern fill only.
        assert_eq!(ctx.scratch.get(40, 49).region, REGION, "north id");
        assert_eq!(
            ctx.grid.cell_native(40, 49).level,
            REGION_LEVEL,
            "north level"
        );
    }

    #[test]
    fn the_two_end_blocks_are_consecutive_tiles_from_the_ramp_end_base() {
        // Not the stepped edge base -- a different tileset entirely.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let playfield = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = CarveCtx {
            grid: &mut grid,
            scratch: &mut scratch,
            ids: &ids,
            blocks: &blocks,
            rng: &mut rng,
            playfield: &playfield,
        };
        assert!(carve_straight_ramp_clear_south(
            &mut ctx,
            regions(),
            (40, 50),
            (52, 50)
        ));
        assert_eq!(
            blocks.seen.borrow().as_slice(),
            &[END_BASE, END_BASE + 1],
            "two stamps, consecutive, in order"
        );
        assert_ne!(END_BASE, ids.ramp_base, "the fixture keeps the bases apart");
    }

    #[test]
    fn a_level_run_takes_no_draw_and_a_sloped_one_takes_exactly_one() {
        // The draw is conditional on the endpoints differing in row. Making it
        // unconditional, or dropping it, desynchronises every later phase.
        for (b, expected_draws) in [((52, 50), 0u32), ((52, 52), 1u32)] {
            let (mut grid, mut scratch) = harness();
            let ids = ids();
            let blocks = RecordingBlocks::new();
            let mut rng = RmgRng::new(1);
            let playfield = Playfield::from_local_size(34, 0, 0, 34, 42);
            let mut ctx = CarveCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &ids,
                blocks: &blocks,
                rng: &mut rng,
                playfield: &playfield,
            };
            assert!(carve_straight_ramp_clear_south(
                &mut ctx,
                regions(),
                (40, 50),
                b
            ));
            let mut probe = RmgRng::new(1);
            for _ in 0..expected_draws {
                probe.next_u32();
            }
            assert_eq!(
                ctx.rng.next_u32(),
                probe.next_u32(),
                "b = {b:?} should take {expected_draws} draw(s)"
            );
        }
    }

    #[test]
    fn both_refusals_write_nothing_and_draw_nothing() {
        // A cheap refusal is what makes the caller hundred-attempt loop
        // affordable. If either path drew, the retry loop would burn the
        // stream.
        // The third case sits ONE cell past the length limit. Both other cases
        // are far from it, so on their own they leave the comparison free to
        // slide by one in either direction; a stair that is one step too steep
        // for its run would then get carved.
        for (label, blocker, b) in [
            ("rect not carveable", true, (52, 50)),
            ("run far too short for the rise", false, (47, 58)),
            ("run exactly one past the limit", false, (46, 52)),
        ] {
            let (mut grid, mut scratch) = harness();
            let ids = ids();
            if blocker {
                grid.get_mut(41, 49).expect("native cell").tile = 500;
            }
            let blocks = RecordingBlocks::new();
            let mut rng = RmgRng::new(1);
            let playfield = Playfield::from_local_size(34, 0, 0, 34, 42);
            let mut ctx = CarveCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &ids,
                blocks: &blocks,
                rng: &mut rng,
                playfield: &playfield,
            };
            assert!(
                !carve_straight_ramp_clear_south(&mut ctx, regions(), (40, 50), b),
                "{label}: must refuse"
            );
            let mut fresh = RmgRng::new(1);
            assert_eq!(ctx.rng.next_u32(), fresh.next_u32(), "{label}: no draw");
            assert!(blocks.seen.borrow().is_empty(), "{label}: no stamp");
            assert_eq!(
                ctx.scratch.get(38, 55).region,
                REGION,
                "{label}: no cell claimed"
            );
        }
    }

    #[test]
    fn the_stepped_edge_descends_down_a_column_not_along_a_row() {
        // The axis assignment: x advances with the OUTER counter and y with the
        // inner, so each column of the stair is a full run of steps top to
        // bottom. Swapping them lays the stair on its side and is invisible in
        // any test that only checks which cells were touched.
        //
        // Endpoints chosen so the flat run is empty and the slide is zero,
        // which puts the stepped edge in one known column whatever the coin
        // comes up as.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let playfield = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = CarveCtx {
            grid: &mut grid,
            scratch: &mut scratch,
            ids: &ids,
            blocks: &blocks,
            rng: &mut rng,
            playfield: &playfield,
        };
        assert!(carve_straight_ramp_clear_south(
            &mut ctx,
            regions(),
            (40, 50),
            (46, 51)
        ));
        // Going DOWN the column, the stair steps 3,2,1,0,0 above level-4.
        let expected = [7u8, 6, 5, 4, 4];
        for (k, want) in expected.iter().enumerate() {
            assert_eq!(
                ctx.grid.cell_native(43, 50 + k as i32).level,
                *want,
                "column cell {k}"
            );
        }
        // And the ascending slope row, tiles one below the slope value.
        let slopes = [12u8, 16, 16, 16, 8];
        for (k, slope) in slopes.iter().enumerate() {
            let cell = ctx.grid.cell_native(43, 50 + k as i32);
            assert_eq!(cell.slope, *slope, "slope {k}");
            assert_eq!(cell.tile, 200 + i32::from(*slope) - 1, "tile {k}");
        }
    }

    /// Build a context over a fresh flat harness.
    macro_rules! ctx_over {
        ($grid:ident, $scratch:ident, $ids:ident, $blocks:ident, $rng:ident, $pf:ident) => {
            CarveCtx {
                grid: &mut $grid,
                scratch: &mut $scratch,
                ids: &$ids,
                blocks: &$blocks,
                rng: &mut $rng,
                playfield: &$pf,
            }
        };
    }

    #[test]
    fn the_north_south_stair_splits_ownership_the_same_way() {
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_straight_ramp_clear_east(
            &mut ctx,
            regions(),
            (45, 52),
            (45, 40)
        ));
        // (51, 54) falls in the southern fill only.
        assert_eq!(ctx.scratch.get(51, 54).region, LOWER, "south id");
        assert_eq!(
            ctx.grid.cell_native(51, 54).level,
            LOWER_LEVEL,
            "south level"
        );
        // (44, 51) falls in the western fill only.
        assert_eq!(ctx.scratch.get(44, 51).region, REGION, "upper id");
        assert_eq!(
            ctx.grid.cell_native(44, 51).level,
            REGION_LEVEL,
            "upper level"
        );
    }

    #[test]
    fn the_north_south_stair_uses_the_second_pair_of_end_blocks() {
        // Four straight routines, eight consecutive end-block tiles, two each.
        // Reusing the east-west pair here would stamp the wrong faces.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_straight_ramp_clear_east(
            &mut ctx,
            regions(),
            (45, 52),
            (45, 40)
        ));
        assert_eq!(
            blocks.seen.borrow().as_slice(),
            &[END_BASE + 2, END_BASE + 3],
            "the second pair, in order"
        );
    }

    #[test]
    fn the_north_south_stepped_edge_runs_along_a_row_not_down_a_column() {
        // The mirror of the east-west routine: here y advances with the OUTER
        // counter and x with the inner, so the stair steps across a row. If the
        // two routines shared an axis assignment, one of them would be lying
        // on its side.
        //
        // Endpoints chosen so the flat run is empty and the slide is zero
        // whatever the coin gives.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_straight_ramp_clear_east(
            &mut ctx,
            regions(),
            (45, 52),
            (46, 46)
        ));
        let expected = [7u8, 6, 5, 4, 4];
        for (k, want) in expected.iter().enumerate() {
            assert_eq!(
                ctx.grid.cell_native(45 + k as i32, 49).level,
                *want,
                "row cell {k}"
            );
        }
        // Record 1 ascending row -- a different set from the east-west stair.
        let slopes = [11u8, 15, 15, 15, 7];
        for (k, slope) in slopes.iter().enumerate() {
            let cell = ctx.grid.cell_native(45 + k as i32, 49);
            assert_eq!(cell.slope, *slope, "slope {k}");
            assert_eq!(cell.tile, 200 + i32::from(*slope) - 1, "tile {k}");
        }
    }

    #[test]
    fn the_north_south_stair_draws_once_and_only_when_it_leans() {
        for (b, draws) in [((45, 40), 0u32), ((47, 40), 1u32)] {
            let (mut grid, mut scratch) = harness();
            let ids = ids();
            let blocks = RecordingBlocks::new();
            let mut rng = RmgRng::new(1);
            let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
            let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
            assert!(carve_straight_ramp_clear_east(
                &mut ctx,
                regions(),
                (45, 52),
                b
            ));
            let mut probe = RmgRng::new(1);
            for _ in 0..draws {
                probe.next_u32();
            }
            assert_eq!(
                ctx.rng.next_u32(),
                probe.next_u32(),
                "b = {b:?} should take {draws} draw(s)"
            );
        }
    }

    #[test]
    fn the_north_south_refusal_boundary_is_pinned_on_both_sides() {
        // Exactly at the limit must carve; one past it must not. Neither the
        // length nor the lean is free to slide a cell.
        // Sited well inside the map so the length rule is what decides.
        // Nearer the edge the rect check fires first and the test passes for
        // the wrong reason.
        let cases = [((46, 46), true), ((47, 46), false)];
        for (b, should_carve) in cases {
            let (mut grid, mut scratch) = harness();
            let ids = ids();
            let blocks = RecordingBlocks::new();
            let mut rng = RmgRng::new(1);
            let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
            let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
            let carved = carve_straight_ramp_clear_east(&mut ctx, regions(), (45, 52), b);
            assert_eq!(carved, should_carve, "b = {b:?}");
            if !carved {
                let mut fresh = RmgRng::new(1);
                assert_eq!(ctx.rng.next_u32(), fresh.next_u32(), "refusal took a draw");
                assert!(blocks.seen.borrow().is_empty(), "refusal stamped");
            }
        }
    }

    #[test]
    fn the_north_south_half_span_carries_no_extra_three() {
        // The two routines compute their half-span differently -- the
        // east-west one adds three and this one does not. In most geometries
        // the difference is invisible, because the four fill rectangles overlap
        // and the pairs write identical values. It shows only where a LATER
        // rectangle would reach a cell an earlier one already claimed for the
        // other plateau.
        //
        // Leaning stair with an empty flat run, so nothing downstream can
        // repaint the probe: (47, 50) is claimed by the southern fill at the
        // real half-span, and by the upper-plateau fill at the inflated one.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_straight_ramp_clear_east(
            &mut ctx,
            regions(),
            (45, 52),
            (47, 45)
        ));
        assert_eq!(ctx.scratch.get(47, 50).region, LOWER, "half-span id");
        assert_eq!(
            ctx.grid.cell_native(47, 50).level,
            LOWER_LEVEL,
            "half-span level"
        );
    }

    #[test]
    fn each_flat_run_stamps_its_own_face_of_the_stair() {
        // The two routines lay different faces: the east-west run takes slope 4
        // and the tile three above the base, the north-south run takes slope 3
        // and the tile two above. Sharing either constant puts one stair's
        // approach on the wrong face, which reads as a seam in the ground.
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);

        let (mut grid, mut scratch) = harness();
        let ids1 = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let mut ctx = ctx_over!(grid, scratch, ids1, blocks, rng, pf);
        assert!(carve_straight_ramp_clear_south(
            &mut ctx,
            regions(),
            (40, 50),
            (52, 50)
        ));
        let cell = ctx.grid.cell_native(45, 51);
        assert_eq!(cell.slope, 4, "east-west flat slope");
        assert_eq!(cell.tile, 203, "east-west flat tile");
        assert_eq!(cell.level, 6, "east-west flat level");

        let (mut grid, mut scratch) = harness();
        let ids2 = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let mut ctx = ctx_over!(grid, scratch, ids2, blocks, rng, pf);
        assert!(carve_straight_ramp_clear_east(
            &mut ctx,
            regions(),
            (45, 52),
            (45, 40)
        ));
        let cell = ctx.grid.cell_native(46, 47);
        assert_eq!(cell.slope, 3, "north-south flat slope");
        assert_eq!(cell.tile, 202, "north-south flat tile");
        assert_eq!(cell.level, 6, "north-south flat level");
    }

    #[test]
    fn the_north_facing_stair_splits_ownership_and_takes_three_end_blocks() {
        // Three blocks, not two: the end-block tiles are handed out running
        // across the four routines, so this one takes the fifth through
        // seventh. Assuming a fixed two-per-routine stride would stamp the
        // wrong faces here.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_straight_ramp_clear_north(
            &mut ctx,
            regions(),
            (52, 50),
            (40, 50)
        ));
        assert_eq!(
            blocks.seen.borrow().as_slice(),
            &[END_BASE + 4, END_BASE + 5, END_BASE + 6],
            "three consecutive end blocks"
        );
        // (38, 45) is in the far fill only, (41, 51) in the near fill only.
        assert_eq!(ctx.scratch.get(38, 45).region, LOWER, "far id");
        assert_eq!(ctx.grid.cell_native(38, 45).level, LOWER_LEVEL, "far level");
        assert_eq!(ctx.scratch.get(41, 51).region, REGION, "near id");
        assert_eq!(
            ctx.grid.cell_native(41, 51).level,
            REGION_LEVEL,
            "near level"
        );
    }

    #[test]
    fn the_north_facing_stepped_edge_climbs_northward() {
        // Rows are laid out by SUBTRACTING from the west endpoint row, so the
        // column descends in level as it goes north. The south-facing routine
        // adds instead; sharing the sign would run the stair the wrong way.
        //
        // Empty flat run and zero slide whichever way the coin falls.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_straight_ramp_clear_north(
            &mut ctx,
            regions(),
            (46, 50),
            (40, 51)
        ));
        // k climbs north from y = 51, and the stair steps down as it goes.
        let expected = [7u8, 6, 5, 4, 4];
        for (k, want) in expected.iter().enumerate() {
            assert_eq!(
                ctx.grid.cell_native(43, 51 - k as i32).level,
                *want,
                "column cell {k}"
            );
        }
        // Record 2 ascending row -- distinct from both earlier routines.
        let slopes = [9u8, 13, 13, 13, 5];
        for (k, slope) in slopes.iter().enumerate() {
            let cell = ctx.grid.cell_native(43, 51 - k as i32);
            assert_eq!(cell.slope, *slope, "slope {k}");
            assert_eq!(cell.tile, 200 + i32::from(*slope) - 1, "tile {k}");
        }
    }

    #[test]
    fn the_north_facing_flat_run_lays_the_third_face() {
        // Slope 2 and the tile one above the base -- the four routines walk
        // down 4/3/2/... as they go, so an off-by-one here is a seam.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_straight_ramp_clear_north(
            &mut ctx,
            regions(),
            (52, 50),
            (40, 50)
        ));
        let cell = ctx.grid.cell_native(45, 49);
        assert_eq!(cell.slope, 2, "north-facing flat slope");
        assert_eq!(cell.tile, 201, "north-facing flat tile");
        assert_eq!(cell.level, 6, "north-facing flat level");
    }

    #[test]
    fn the_north_facing_stair_draws_once_and_refuses_cheaply() {
        for (b, draws) in [((40, 50), 0u32), ((40, 52), 1u32)] {
            let (mut grid, mut scratch) = harness();
            let ids = ids();
            let blocks = RecordingBlocks::new();
            let mut rng = RmgRng::new(1);
            let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
            let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
            assert!(carve_straight_ramp_clear_north(
                &mut ctx,
                regions(),
                (52, 50),
                b
            ));
            let mut probe = RmgRng::new(1);
            for _ in 0..draws {
                probe.next_u32();
            }
            assert_eq!(ctx.rng.next_u32(), probe.next_u32(), "b = {b:?}");
        }

        // Boundary, both sides, sited so the length rule is what decides.
        for (b, should_carve) in [((40, 51), true), ((40, 52), false)] {
            let (mut grid, mut scratch) = harness();
            let ids = ids();
            let blocks = RecordingBlocks::new();
            let mut rng = RmgRng::new(1);
            let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
            let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
            let carved = carve_straight_ramp_clear_north(&mut ctx, regions(), (46, 50), b);
            assert_eq!(carved, should_carve, "b = {b:?}");
            if !carved {
                let mut fresh = RmgRng::new(1);
                assert_eq!(ctx.rng.next_u32(), fresh.next_u32(), "refusal drew");
                assert!(blocks.seen.borrow().is_empty(), "refusal stamped");
            }
        }
    }

    #[test]
    fn a_two_step_northward_lean_pins_both_sign_terms() {
        // The previous north-facing fixture leans one step in the positive
        // direction, which zeroes BOTH sign terms: the per-column lift is
        // multiplied by a counter that never leaves zero, and the negative-lean
        // bump never applies. Either could have been flipped unnoticed.
        //
        // This one leans two steps the other way, so both terms are live, and
        // keeps the flat run empty so nothing repaints the columns.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_straight_ramp_clear_north(
            &mut ctx,
            regions(),
            (47, 50),
            (40, 48)
        ));

        // First column: the bump lifts it one row north of the west endpoint.
        let expected = [7u8, 6, 5, 4, 4];
        for (k, want) in expected.iter().enumerate() {
            assert_eq!(
                ctx.grid.cell_native(43, 49 - k as i32).level,
                *want,
                "first column cell {k}"
            );
        }
        // Second column: a negative lean steps it one row further south, not
        // north. Flipping the lift would put this column at y = 48 downward.
        for (k, want) in expected.iter().enumerate() {
            assert_eq!(
                ctx.grid.cell_native(44, 50 - k as i32).level,
                *want,
                "second column cell {k}"
            );
        }
        // Descending slope row, since the lean is negative.
        assert_eq!(ctx.grid.cell_native(43, 49).slope, 10, "descending head");
    }

    #[test]
    fn the_north_facing_half_span_keeps_its_plus_three() {
        // Same shape of trap as the north-south routine: the half-span is
        // invisible almost everywhere because the fill rectangles overlap. It
        // shows at (43, 45), which the far fill reaches only with the wider
        // span -- with the narrower one no rectangle claims the cell at all and
        // it keeps the harness ownership.
        //
        // Asserted on the region id rather than the level, because the stepped
        // edge repaints that cell later and the id survives it.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_straight_ramp_clear_north(
            &mut ctx,
            regions(),
            (47, 50),
            (40, 48)
        ));
        assert_eq!(ctx.scratch.get(43, 45).region, LOWER, "half-span reach");
    }

    #[test]
    fn the_west_facing_stair_takes_the_last_three_end_blocks() {
        // Ten end-block tiles across the four straights: 2 + 2 + 3 + 3.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_straight_ramp_clear_west(
            &mut ctx,
            regions(),
            (50, 44),
            (50, 56)
        ));
        assert_eq!(
            blocks.seen.borrow().as_slice(),
            &[END_BASE + 7, END_BASE + 8, END_BASE + 9],
            "the last three"
        );
        // (44, 45) is in the northern fill only, (52, 46) the eastern one.
        assert_eq!(ctx.scratch.get(44, 45).region, LOWER, "north id");
        assert_eq!(
            ctx.grid.cell_native(44, 45).level,
            LOWER_LEVEL,
            "north level"
        );
        assert_eq!(ctx.scratch.get(52, 46).region, REGION, "east id");
        assert_eq!(
            ctx.grid.cell_native(52, 46).level,
            REGION_LEVEL,
            "east level"
        );
    }

    #[test]
    fn the_west_facing_flat_run_lays_the_base_tile_itself() {
        // Slope 1 and the ramp base with no offset, closing the 4/3/2/1 and
        // +3/+2/+1/+0 walks. An off-by-one here would reach past the set.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_straight_ramp_clear_west(
            &mut ctx,
            regions(),
            (50, 44),
            (50, 56)
        ));
        let cell = ctx.grid.cell_native(48, 50);
        assert_eq!(cell.slope, 1, "west-facing flat slope");
        assert_eq!(cell.tile, 200, "west-facing flat tile");
        assert_eq!(cell.level, 5, "west-facing flat level");
    }

    #[test]
    fn the_west_facing_stepped_edge_lays_its_row_westward() {
        // Unique to this routine: x DECREASES as the step index rises, so the
        // row runs east to west. Every other straight increases it, and sharing
        // that sign would mirror the stair.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_straight_ramp_clear_west(
            &mut ctx,
            regions(),
            (50, 44),
            (51, 50)
        ));
        let expected = [7u8, 6, 5, 4, 4];
        for (k, want) in expected.iter().enumerate() {
            assert_eq!(
                ctx.grid.cell_native(51 - k as i32, 47).level,
                *want,
                "row cell {k}"
            );
        }
        let slopes = [12u8, 16, 16, 16, 8];
        for (k, slope) in slopes.iter().enumerate() {
            let cell = ctx.grid.cell_native(51 - k as i32, 47);
            assert_eq!(cell.slope, *slope, "slope {k}");
            assert_eq!(cell.tile, 200 + i32::from(*slope) - 1, "tile {k}");
        }
    }

    #[test]
    fn a_two_step_westward_lean_pins_the_bump_and_the_lift() {
        // Both sign terms live: a two-step NEGATIVE lean, so the positive-lean
        // bump must stay off and the lift must run backwards. Flat run empty.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_straight_ramp_clear_west(
            &mut ctx,
            regions(),
            (50, 44),
            (48, 51)
        ));
        // First row: no bump, no lift.
        let expected = [7u8, 6, 5, 4, 4];
        for (k, want) in expected.iter().enumerate() {
            assert_eq!(
                ctx.grid.cell_native(50 - k as i32, 47).level,
                *want,
                "first row cell {k}"
            );
        }
        // Second row steps one WEST, not east.
        for (k, want) in expected.iter().enumerate() {
            assert_eq!(
                ctx.grid.cell_native(49 - k as i32, 48).level,
                *want,
                "second row cell {k}"
            );
        }
        assert_eq!(ctx.grid.cell_native(50, 47).slope, 9, "descending head");
    }

    #[test]
    fn the_west_facing_stair_draws_once_and_refuses_cheaply() {
        for (b, draws) in [((50, 56), 0u32), ((52, 56), 1u32)] {
            let (mut grid, mut scratch) = harness();
            let ids = ids();
            let blocks = RecordingBlocks::new();
            let mut rng = RmgRng::new(1);
            let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
            let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
            assert!(carve_straight_ramp_clear_west(
                &mut ctx,
                regions(),
                (50, 44),
                b
            ));
            let mut probe = RmgRng::new(1);
            for _ in 0..draws {
                probe.next_u32();
            }
            assert_eq!(ctx.rng.next_u32(), probe.next_u32(), "b = {b:?}");
        }

        for (b, should_carve) in [((51, 50), true), ((52, 50), false)] {
            let (mut grid, mut scratch) = harness();
            let ids = ids();
            let blocks = RecordingBlocks::new();
            let mut rng = RmgRng::new(1);
            let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
            let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
            let carved = carve_straight_ramp_clear_west(&mut ctx, regions(), (50, 44), b);
            assert_eq!(carved, should_carve, "b = {b:?}");
            if !carved {
                let mut fresh = RmgRng::new(1);
                assert_eq!(ctx.rng.next_u32(), fresh.next_u32(), "refusal drew");
                assert!(blocks.seen.borrow().is_empty(), "refusal stamped");
            }
        }
    }

    #[test]
    fn the_west_facing_half_span_leaves_its_one_row_gap() {
        // Ownership here is decided by column, so the half-span cannot be seen
        // in who owns what. What it does control is the REACH of the two lower
        // fills along Y: at the real span they stop one row short of each other
        // and leave (44, 50) unclaimed. Widening the span closes that gap and
        // the cell becomes the lower plateau.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_straight_ramp_clear_west(
            &mut ctx,
            regions(),
            (50, 44),
            (50, 56)
        ));
        assert_eq!(
            ctx.scratch.get(44, 50).region,
            REGION,
            "the gap row must stay unclaimed"
        );
        // Its neighbours either side are claimed, so the gap is a gap and not
        // simply out of reach.
        assert_eq!(ctx.scratch.get(44, 49).region, LOWER, "row above");
        assert_eq!(ctx.scratch.get(44, 51).region, LOWER, "row below");
    }

    /// A corner carve over the flat harness, returning the recorder.
    macro_rules! corner_carve {
        ($ctx:ident, $a:expr, $b:expr) => {
            carve_corner_ramp(&mut $ctx, regions(), $a, $b)
        };
    }

    #[test]
    fn a_corner_carve_takes_no_draw_at_all() {
        // The four straights each take one when their endpoints differ across
        // the run. A corner never draws, so wiring one into the retry loop as
        // though it did would put the stream out of step.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(corner_carve!(ctx, (52, 54), (44, 46)));
        let mut fresh = RmgRng::new(1);
        assert_eq!(ctx.rng.next_u32(), fresh.next_u32(), "corner drew");
    }

    #[test]
    fn a_corner_too_tight_on_either_axis_refuses_before_anything_else() {
        // Both deltas must EXCEED two, and the check happens before the rect is
        // built -- so a tight corner costs nothing, not even the rect sweep.
        for (a, b, label) in [
            ((46, 54), (44, 46), "x span of two"),
            ((52, 48), (44, 46), "y span of two"),
            ((47, 49), (44, 46), "both exactly three -- carves"),
        ] {
            let (mut grid, mut scratch) = harness();
            let ids = ids();
            let blocks = RecordingBlocks::new();
            let mut rng = RmgRng::new(1);
            let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
            let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
            let carved = corner_carve!(ctx, a, b);
            let expected = label.contains("carves");
            assert_eq!(carved, expected, "{label}");
            if !carved {
                assert!(blocks.seen.borrow().is_empty(), "{label}: stamped");
                let mut fresh = RmgRng::new(1);
                assert_eq!(ctx.rng.next_u32(), fresh.next_u32(), "{label}: drew");
            }
        }
    }

    #[test]
    fn the_corner_end_blocks_reuse_tiles_the_straights_also_use() {
        // The ten end-block tiles are shared across families, not divided
        // between them: this routine takes the third and fifth, which the
        // east-facing and north-facing straights also stamp.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(corner_carve!(ctx, (52, 54), (44, 46)));
        assert_eq!(
            blocks.seen.borrow().as_slice(),
            &[END_BASE + 2, END_BASE + 4],
            "two stamps, not consecutive"
        );
    }

    #[test]
    fn the_corner_fills_split_across_five_rectangles() {
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        // Seed the interior probe with the OTHER plateau first. The harness
        // already starts every cell owned by the upper region, so asserting
        // that afterwards would pass even if the rectangle were never written
        // -- the assertion has to have something to undo.
        scratch.get_mut(48, 50).region = LOWER;
        grid.get_mut(48, 50).expect("native cell").level = LOWER_LEVEL;
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(corner_carve!(ctx, (52, 54), (44, 46)));
        // (43, 41) is in the outer lower rectangle only.
        assert_eq!(ctx.scratch.get(43, 41).region, LOWER, "lower id");
        assert_eq!(
            ctx.grid.cell_native(43, 41).level,
            LOWER_LEVEL,
            "lower level"
        );
        // (48, 50) is in the big interior rectangle, which is the upper
        // plateau -- the one a four-rectangle port would not have.
        assert_eq!(ctx.scratch.get(48, 50).region, REGION, "interior id");
        assert_eq!(
            ctx.grid.cell_native(48, 50).level,
            REGION_LEVEL,
            "interior level"
        );
    }

    #[test]
    fn the_three_corner_tails_lay_three_different_faces() {
        // The diagonal takes slope 6 and the tile five above the base; the two
        // approaches take 3/+2 and 2/+1. Sharing any of them would flatten the
        // turn.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(corner_carve!(ctx, (52, 54), (44, 46)));

        // Diagonal, step 1. It sits exactly one row below where the second
        // tail stops, so it survives -- that near miss is worth pinning.
        let d = ctx.grid.cell_native(53, 45);
        assert_eq!((d.slope, d.tile, d.level), (6, 205, 6), "diagonal");
        // Second tail: the along-Y approach.
        let v = ctx.grid.cell_native(52, 51);
        assert_eq!((v.slope, v.tile, v.level), (3, 202, 7), "along-Y approach");
        // Third tail: the along-X approach.
        let h = ctx.grid.cell_native(48, 46);
        assert_eq!((h.slope, h.tile, h.level), (2, 201, 7), "along-X approach");
    }

    #[test]
    fn the_corner_approaches_grow_one_cell_per_step() {
        // Each approach runs one cell longer for every step of depth, which is
        // what opens the turn out. A fixed length would leave a notch.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(corner_carve!(ctx, (52, 54), (44, 46)));
        // Along-X approach at depth 0 reaches x = 47..51; at depth 1 one
        // further, to 52. Checking the cell that only the longer run reaches.
        assert_eq!(
            ctx.grid.cell_native(52, 45).slope,
            2,
            "depth 1 reaches one further"
        );
        assert_ne!(
            ctx.grid.cell_native(52, 46).slope,
            2,
            "depth 0 stops short of it"
        );
        // The along-Y approach grows the same way, and needs its own probe --
        // the two share no code path. (53, 46) is reached only at depth 1.
        assert_eq!(
            ctx.grid.cell_native(53, 46).slope,
            3,
            "along-Y depth 1 reaches one further"
        );
    }

    #[test]
    fn the_reflected_corner_takes_four_end_blocks() {
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_corner_ramp_reflected(
            &mut ctx,
            regions(),
            (52, 46),
            (44, 54)
        ));
        assert_eq!(
            blocks.seen.borrow().as_slice(),
            &[END_BASE + 5, END_BASE + 6, END_BASE + 8, END_BASE + 9],
            "four stamps, with a gap at +7"
        );
        let mut fresh = RmgRng::new(1);
        assert_eq!(ctx.rng.next_u32(), fresh.next_u32(), "corners never draw");
    }

    #[test]
    fn the_reflected_corner_guards_read_the_other_way_round() {
        // Its spans are a.x - b.x and b.y - a.y, so a corner the first routine
        // would accept can be too tight here and vice versa.
        for (a, b, expected) in [
            ((52, 46), (44, 54), true),
            ((46, 46), (44, 54), false),
            ((52, 52), (44, 54), false),
        ] {
            let (mut grid, mut scratch) = harness();
            let ids = ids();
            let blocks = RecordingBlocks::new();
            let mut rng = RmgRng::new(1);
            let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
            let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
            let carved = carve_corner_ramp_reflected(&mut ctx, regions(), a, b);
            assert_eq!(carved, expected, "a = {a:?}, b = {b:?}");
            if !carved {
                assert!(blocks.seen.borrow().is_empty(), "refusal stamped");
            }
        }
    }

    #[test]
    fn the_reflected_corner_fills_five_rectangles() {
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        // Seed the interior probe with the other plateau so the assertion has
        // something to undo.
        scratch.get_mut(48, 50).region = LOWER;
        grid.get_mut(48, 50).expect("native cell").level = LOWER_LEVEL;
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_corner_ramp_reflected(
            &mut ctx,
            regions(),
            (52, 46),
            (44, 54)
        ));
        assert_eq!(ctx.scratch.get(50, 40).region, LOWER, "outer lower id");
        assert_eq!(
            ctx.grid.cell_native(50, 40).level,
            LOWER_LEVEL,
            "outer level"
        );
        assert_eq!(ctx.scratch.get(48, 50).region, REGION, "interior id");
        assert_eq!(
            ctx.grid.cell_native(48, 50).level,
            REGION_LEVEL,
            "interior level"
        );
    }

    #[test]
    fn the_reflected_corner_lays_the_faces_one_lower() {
        // 5/2/1 and +4/+1/+0 against the other corner's 6/3/2 and +5/+2/+1.
        // The two corners lay adjacent faces, not the same ones.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_corner_ramp_reflected(
            &mut ctx,
            regions(),
            (52, 46),
            (44, 54)
        ));
        let d = ctx.grid.cell_native(43, 45);
        assert_eq!((d.slope, d.tile, d.level), (5, 204, 6), "diagonal");
        let h = ctx.grid.cell_native(49, 46);
        assert_eq!((h.slope, h.tile, h.level), (2, 201, 7), "along-X approach");
        let v = ctx.grid.cell_native(44, 51);
        assert_eq!((v.slope, v.tile, v.level), (1, 200, 7), "along-Y approach");
    }

    #[test]
    fn the_reflected_corner_approaches_both_grow() {
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_corner_ramp_reflected(
            &mut ctx,
            regions(),
            (52, 46),
            (44, 54)
        ));
        // Along-X at depth 1 reaches one cell further west than at depth 0.
        assert_eq!(ctx.grid.cell_native(44, 45).slope, 2, "along-X depth 1");
        // Along-Y at depth 1 reaches one row further north.
        assert_eq!(ctx.grid.cell_native(43, 45).slope, 5, "diagonal survives");
        assert_eq!(ctx.grid.cell_native(43, 46).slope, 1, "along-Y depth 1");
    }

    #[test]
    fn the_north_east_corner_spans_read_from_the_inner_endpoint() {
        // Both spans are b - a here, the reverse of both other corners. A
        // corner one routine accepts another will refuse outright.
        for (a, b, expected) in [
            ((44, 46), (52, 54), true),
            ((44, 46), (46, 54), false),
            ((44, 46), (52, 48), false),
        ] {
            let (mut grid, mut scratch) = harness();
            let ids = ids();
            let blocks = RecordingBlocks::new();
            let mut rng = RmgRng::new(1);
            let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
            let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
            let carved = carve_corner_ramp_north_east(&mut ctx, regions(), a, b);
            assert_eq!(carved, expected, "a = {a:?}, b = {b:?}");
            if !carved {
                assert!(blocks.seen.borrow().is_empty(), "refusal stamped");
            }
        }
    }

    #[test]
    fn the_north_east_corner_takes_a_pair_no_one_else_uses() {
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_corner_ramp_north_east(
            &mut ctx,
            regions(),
            (44, 46),
            (52, 54)
        ));
        assert_eq!(
            blocks.seen.borrow().as_slice(),
            &[END_BASE + 7, END_BASE + 1],
            "descending, not ascending"
        );
        let mut fresh = RmgRng::new(1);
        assert_eq!(ctx.rng.next_u32(), fresh.next_u32(), "corners never draw");
    }

    #[test]
    fn the_north_east_corner_lays_a_third_distinct_face_set() {
        // 8/1/4 and +7/+0/+3, against 6/3/2 and 5/2/1 for the other corners.
        // Slope 8 appears in no other routine.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        scratch.get_mut(48, 50).region = LOWER;
        grid.get_mut(48, 50).expect("native cell").level = LOWER_LEVEL;
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_corner_ramp_north_east(
            &mut ctx,
            regions(),
            (44, 46),
            (52, 54)
        ));
        let d = ctx.grid.cell_native(43, 55);
        assert_eq!((d.slope, d.tile, d.level), (8, 207, 6), "diagonal");
        let v = ctx.grid.cell_native(44, 49);
        assert_eq!((v.slope, v.tile, v.level), (1, 200, 7), "along-Y approach");
        let h = ctx.grid.cell_native(49, 54);
        assert_eq!((h.slope, h.tile, h.level), (4, 203, 7), "along-X approach");
        // Five fills, including the interior block.
        assert_eq!(ctx.scratch.get(38, 44).region, LOWER, "outer lower");
        assert_eq!(ctx.scratch.get(48, 50).region, REGION, "interior");
        assert_eq!(
            ctx.grid.cell_native(48, 50).level,
            REGION_LEVEL,
            "interior level"
        );
    }

    #[test]
    fn the_north_east_diagonal_runs_south_west() {
        // x falls while y RISES. Both other corners have them falling
        // together, so a copied sign puts this diagonal in the wrong quadrant.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);
        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(carve_corner_ramp_north_east(
            &mut ctx,
            regions(),
            (44, 46),
            (52, 54)
        ));
        for k in 0..4i32 {
            let cell = ctx.grid.cell_native(44 - k, 54 + k);
            assert_eq!(cell.slope, 8, "diagonal step {k}");
        }
        // And both approaches grow with depth.
        assert_eq!(ctx.grid.cell_native(43, 54).slope, 1, "along-Y depth 1");
        // The BOUNDARY cell of the along-X run: depth 1 reaches exactly one
        // cell further west than depth 0, and (44, 55) is that cell. A probe
        // anywhere inside the run is reached either way and proves nothing.
        assert_eq!(ctx.grid.cell_native(44, 55).slope, 4, "along-X depth 1");
    }

    #[test]
    fn a_refused_shape_falls_through_and_still_spends_its_draws() {
        // The behaviour the whole restructure exists for. A bare north bit
        // satisfies BOTH straight shapes whose run is north-south: the
        // west-clear one and the east-clear one. With the ground unusable both
        // refuse, and the original tries them in turn -- spending two draws
        // apiece on the way, four in total.
        //
        // Stopping at the first shape whose guard matched, which is what this
        // function used to do, would spend two. That is not a smaller carve;
        // it is every later draw on the map coming from the wrong place.
        let (mut grid, mut scratch) = harness();
        let ids = ids();
        let blocks = RecordingBlocks::new();
        let mut rng = RmgRng::new(1);
        let pf = Playfield::from_local_size(34, 0, 0, 34, 42);

        // The harness owns the whole diamond for the region, which would make
        // every window pass and the mask come back full -- an interior cell,
        // rejected before any shape is tried. Clear it first.
        for slot in scratch.cells_mut() {
            slot.region = -1;
        }
        // Give the ring mask a north bit and nothing else.
        for (x, y) in [(38, 48), (38, 43)] {
            for row in 0..5 {
                for col in 0..5 {
                    scratch.get_mut(x + col, y + row).region = REGION;
                }
            }
        }
        // And make every carve refuse: no cell is bare ground.
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            grid.get_mut(x, y).expect("native cell").tile = 500;
        }

        let mut ctx = ctx_over!(grid, scratch, ids, blocks, rng, pf);
        assert!(
            !try_carve_connector_at_cell(&mut ctx, regions(), (40, 50), 0.0),
            "unusable ground carves nothing"
        );

        let mut probe = RmgRng::new(1);
        for _ in 0..4 {
            probe.next_u32();
        }
        assert_eq!(
            ctx.rng.next_u32(),
            probe.next_u32(),
            "both north-south shapes were tried, two draws each"
        );
    }
}
