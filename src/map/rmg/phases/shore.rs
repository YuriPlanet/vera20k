//! Shore tiler: validates a carved land blob's shoreline and stamps
//! shore-piece tiles along it. Its verdict decides whether the blob commits
//! or rolls back, so every detail here is on the bit-exact path.
//!
//! Four full-map passes in native scan order: erosion, thin-water cleanup,
//! then two piece-selection passes (straight/outer pieces, inner corners).
//! The selection passes draw one bounded uniform per iterated cell — even
//! cells that do nothing — which makes the tiler a heavy, load-bearing
//! consumer of the draw stream.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::rng::RmgRng;
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;

/// Number of shore pieces (1-based piece ids 1..=42).
pub const SHORE_PIECES: usize = 42;

/// Piece equivalence classes: equal class means an existing foreign-region
/// piece is interchangeable with the new one (cross-region acceptance).
const PIECE_CLASS: [i32; SHORE_PIECES] = [
    0, 0, 0, 1, 2, 3, 4, 4, 5, 5, 5, 6, 7, 8, 9, 9, 10, 10, 10, 11, 12, 13, 14, 14, 15, 15, 15, 16,
    17, 18, 19, 19, 20, 20, 21, 21, 22, 22, 23, 23, 24, 25,
];

/// Octant-style facing per piece; |difference| in [3,5] = opposing shores.
const PIECE_ORIENT: [i32; SHORE_PIECES] = [
    4, 4, 4, 4, 4, 4, 3, 3, 2, 2, 2, 2, 2, 2, 1, 1, 0, 0, 0, 0, 0, 0, 7, 7, 6, 6, 6, 6, 6, 6, 5, 5,
    3, 3, 1, 1, 7, 7, 5, 5, 4, 4,
];

/// Block-anchor offset per piece (1-based piece p at index p-1).
const PIECE_ANCHOR: [(i16, i16); SHORE_PIECES] = [
    (0, -1),
    (0, -1),
    (0, -1),
    (0, -1),
    (0, -2),
    (-1, -2),
    (-1, -1),
    (-1, -1),
    (-1, 0),
    (-1, 0),
    (-1, 0),
    (-1, 0),
    (-2, -1),
    (-2, 0),
    (-1, 0),
    (-1, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (-1, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, -1),
    (0, 0),
    (0, -1),
    (0, -1),
    (-1, -1),
    (-1, -1),
    (-1, 0),
    (-1, 0),
    (0, 0),
    (0, 0),
    (0, -1),
    (0, -1),
    (0, 0),
    (0, 0),
];

/// The water-spike masks that trigger the thin-water cleanup.
const SPIKE_MASKS: [i32; 8] = [0xC7, 0x7C, 0xF1, 0x1F, 0xC6, 0x6C, 0xB1, 0x1B];

/// Mask bit per direction (+X east, +Y south).
const BIT_N: i32 = 0x80;
const BIT_NE: i32 = 0x01;
const BIT_E: i32 = 0x02;
const BIT_SE: i32 = 0x04;
const BIT_S: i32 = 0x08;
const BIT_SW: i32 = 0x10;
const BIT_W: i32 = 0x20;
const BIT_NW: i32 = 0x40;

/// One sub-tile of a multi-cell tile block; `None` entries are holes.
#[derive(Debug, Clone, Copy, Default)]
pub struct SubTile {
    /// Height byte added to the source cell's level on stamping.
    pub height: u8,
    /// TMP terrain-type byte (sub-tile header +0x29); the zone classifier
    /// maps it to a land type. 0 = clear.
    pub terrain: u8,
    /// TMP ramp-type byte (sub-tile header +0x2A), copied verbatim onto the
    /// cell when the block is stamped.
    ///
    /// This is a property of the stamped sub-tile, **not** a ramp-variant
    /// index chosen by the carver — the same block always contributes the
    /// same value. Ramp carving needs it because a cliff stair is only
    /// walkable if its cells carry the slope the block declares.
    pub slope: u8,
}

/// A tile block's sub-tile grid (row-major, `width * height` entries).
#[derive(Debug, Clone)]
pub struct TileBlock {
    pub width: i32,
    pub height: i32,
    pub subtiles: Vec<Option<SubTile>>,
}

/// Provider of tile-block layouts for shore-piece tiles.
///
/// Unit tests supply synthetic blocks; the integration layer builds this from
/// the theater's TMP data.
pub trait TileBlocks {
    fn block(&self, tile: i32) -> Option<&TileBlock>;
}

/// Everything the tiler borrows from the phase driver.
pub struct ShoreCtx<'a> {
    pub grid: &'a mut RmgGrid,
    pub scratch: &'a mut RmgScratch,
    pub ids: &'a TileIds,
    pub blocks: &'a dyn TileBlocks,
    pub rng: &'a mut RmgRng,
}

/// Narrow water predicate: the first 14 tiles of the water set only.
fn is_water(ids: &TileIds, tile: i32) -> bool {
    ids.water_base != -1 && tile >= ids.water_base && tile < ids.water_base + 0x0E
}

/// Run the shore tiler for `region_id`. `keep` mirrors the original's flag:
/// water-seed/flood-fill callers pass true (region checks bypassed), the
/// mode-3/4 drivers pass false. Returns the commit verdict.
pub fn run(ctx: &mut ShoreCtx<'_>, region_id: i32, keep: bool) -> bool {
    ctx.scratch.invalidate_shore_masks();

    // Pass A — land erosion (aborts on the first failing cell).
    let coords: Vec<(i32, i32)> = ctx.grid.native_cells().collect();
    for &(x, y) in &coords {
        if !erode(ctx, x, y, region_id, keep) {
            return false;
        }
    }

    // Pass B — thin-water cleanup (void).
    for &(x, y) in &coords {
        cleanup(ctx, x, y, region_id, keep);
    }

    // Passes C/D — piece selection and stamping.
    for variant in [1, 2] {
        for &(x, y) in &coords {
            if !select(ctx, x, y, variant, region_id, keep) {
                return false;
            }
        }
    }
    true
}

/// Neighbor-water mask with the original's mode gates and cache semantics.
///
/// The cached value is returned for ANY mode once that mode's prechecks pass
/// — the payload is shared, only the gates differ. Computation does NOT store
/// back; only pass A stores.
fn compute_mask(ctx: &mut ShoreCtx<'_>, x: i32, y: i32, mode: i32) -> i32 {
    let Some(cell) = ctx.grid.get(x, y) else {
        return 0;
    };
    let self_water = is_water(ctx.ids, cell.tile);
    let tile = cell.tile;

    if !ctx.scratch.in_diamond(x, y) {
        return 0;
    }
    match mode {
        0 if !ctx.ids.is_clear(tile) => return 0,
        1 if self_water => return 0,
        _ => {}
    }
    let record = ctx.scratch.get(x, y);
    if !record.shore_enable {
        return 0;
    }
    if record.shore_mask >= 0 {
        return record.shore_mask;
    }

    let mut mask = 0;
    let neighbors = [
        (0, -1, BIT_N),
        (1, -1, BIT_NE),
        (1, 0, BIT_E),
        (1, 1, BIT_SE),
        (0, 1, BIT_S),
        (-1, 1, BIT_SW),
        (-1, 0, BIT_W),
        (-1, -1, BIT_NW),
    ];
    for (dx, dy, bit) in neighbors {
        let wet = match ctx.grid.get(x + dx, y + dy) {
            Some(neighbor) => is_water(ctx.ids, neighbor.tile),
            // Missing neighbor inherits the cell's own wetness.
            None => self_water,
        };
        if wet {
            mask |= bit;
        }
    }
    mask
}

/// Pass A worker: erode land cells pinched between water, recursively.
///
/// The recursion's own verdict is discarded — a foreign-region hit inside the
/// recursion does not fail the pass; only the top-level iterator visit can.
fn erode(ctx: &mut ShoreCtx<'_>, x: i32, y: i32, region_id: i32, keep: bool) -> bool {
    let mask2 = compute_mask(ctx, x, y, 2);
    if ctx.scratch.in_diamond(x, y) {
        ctx.scratch.get_mut(x, y).shore_mask = mask2;
    }
    let m = compute_mask(ctx, x, y, 0);
    if m <= 0 {
        return true;
    }

    let mut bad = m == (BIT_NE | BIT_E | BIT_S) || m == (BIT_E | BIT_S | BIT_SW);
    bad |= (m & (BIT_N | BIT_W)) == (BIT_N | BIT_W) && (m & (BIT_NE | BIT_SW)) == 0;
    bad |= (m & (BIT_N | BIT_E)) == (BIT_N | BIT_E) && (m & (BIT_NW | BIT_SE)) == 0;
    bad |= (m & (BIT_E | BIT_S)) == (BIT_E | BIT_S) && (m & (BIT_NE | BIT_SW)) == 0;
    bad |= (m & (BIT_S | BIT_W)) == (BIT_S | BIT_W) && (m & (BIT_NW | BIT_SE)) == 0;
    // Thin-strip probes: a facing water bit two cells over means a 1-cell
    // land strip.
    if !bad && (m & BIT_W) != 0 {
        bad = (compute_mask(ctx, x + 1, y, 0) & BIT_E) != 0
            || (compute_mask(ctx, x + 2, y, 0) & BIT_E) != 0;
    }
    if !bad && (m & BIT_E) != 0 {
        bad = (compute_mask(ctx, x - 1, y, 0) & BIT_W) != 0
            || (compute_mask(ctx, x - 2, y, 0) & BIT_W) != 0;
    }
    if !bad && (m & BIT_S) != 0 {
        bad = (compute_mask(ctx, x, y - 1, 0) & BIT_N) != 0
            || (compute_mask(ctx, x, y - 2, 0) & BIT_N) != 0;
    }
    if !bad && (m & BIT_N) != 0 {
        bad = (compute_mask(ctx, x, y + 1, 0) & BIT_S) != 0
            || (compute_mask(ctx, x, y + 2, 0) & BIT_S) != 0;
    }

    let flood =
        (m & (BIT_N | BIT_S)) == (BIT_N | BIT_S) || (m & (BIT_E | BIT_W)) == (BIT_E | BIT_W) || bad;
    if !flood {
        return true;
    }

    let owner = ctx.scratch.get(x, y).region;
    if owner > 0 && owner != region_id && !keep {
        return false;
    }
    {
        let cell = ctx
            .grid
            .get_mut(x, y)
            .expect("erode only reaches existing cells");
        cell.sub_tile = 0;
        cell.tile = ctx.ids.water_base;
    }
    ctx.scratch.get_mut(x, y).region = if keep { 0 } else { region_id };

    for dir in 0..8 {
        let (nx, ny) = RmgGrid::step(x, y, dir);
        if ctx.scratch.in_diamond(nx, ny) {
            ctx.scratch.get_mut(nx, ny).shore_mask = -1;
            // Result deliberately discarded, like the original.
            let _ = erode(ctx, nx, ny, region_id, keep);
        }
    }
    true
}

/// Pass B worker: revert water spikes to empty cells.
fn cleanup(ctx: &mut ShoreCtx<'_>, x: i32, y: i32, region_id: i32, keep: bool) {
    let Some(cell) = ctx.grid.get(x, y) else {
        return;
    };
    // Note the narrower 12-tile range: the last two water tiles are exempt.
    let watery = ctx.ids.water_base != -1
        && cell.tile >= ctx.ids.water_base
        && cell.tile < ctx.ids.water_base + 0x0C;
    if !watery {
        return;
    }
    let mask = compute_mask(ctx, x, y, 2);
    if !SPIKE_MASKS.contains(&mask) {
        return;
    }

    let cell = ctx
        .grid
        .get_mut(x, y)
        .expect("cleanup only reaches existing cells");
    cell.tile = crate::map::rmg::tiles::TILE_UNASSIGNED;
    cell.sub_tile = 0;
    if keep {
        ctx.scratch.get_mut(x, y).region = region_id;
    }
    for dir in 0..8 {
        let (nx, ny) = RmgGrid::step(x, y, dir);
        if ctx.scratch.in_diamond(nx, ny) {
            ctx.scratch.get_mut(nx, ny).shore_mask = -1;
            let mask = compute_mask(ctx, nx, ny, 2);
            ctx.scratch.get_mut(nx, ny).shore_mask = mask;
        }
    }
}

/// Passes C/D worker: pick a shore piece for the cell and stamp it.
fn select(
    ctx: &mut ShoreCtx<'_>,
    x: i32,
    y: i32,
    variant: i32,
    region_id: i32,
    keep: bool,
) -> bool {
    // The bounded draw comes FIRST — every iterated cell consumes it.
    let r = ctx.rng.uniform(0, 5);

    let m = compute_mask(ctx, x, y, if variant == 2 { 1 } else { 0 });
    if m == 0 {
        return true;
    }

    let piece: i32 = if variant == 2 {
        // Inner corners, tested N+W, N+E, S+E, S+W.
        if (m & (BIT_N | BIT_W)) == (BIT_N | BIT_W) {
            if (m & (BIT_NE | BIT_SW)) == (BIT_NE | BIT_SW) {
                (r & 1) + 0x17
            } else if (m & BIT_NE) != 0 {
                0x15
            } else {
                0x1E
            }
        } else if (m & (BIT_N | BIT_E)) == (BIT_N | BIT_E) {
            if (m & (BIT_NW | BIT_SE)) == (BIT_NW | BIT_SE) {
                (r & 1) + 0x0F
            } else if (m & BIT_SE) != 0 {
                0x0E
            } else {
                0x16
            }
        } else if (m & (BIT_S | BIT_E)) == (BIT_S | BIT_E) {
            if (m & (BIT_NE | BIT_SW)) == (BIT_NE | BIT_SW) {
                (r & 1) + 7
            } else if (m & BIT_NE) != 0 {
                0x0D
            } else {
                6
            }
        } else if (m & (BIT_S | BIT_W)) == (BIT_S | BIT_W) {
            if (m & (BIT_NW | BIT_SE)) == (BIT_NW | BIT_SE) {
                (r & 1) + 0x1F
            } else if (m & BIT_SE) != 0 {
                5
            } else {
                0x1D
            }
        } else {
            return true;
        }
    } else {
        // Straight shores (run-parity walk) and outer corners.
        if (m & BIT_E) != 0 {
            let len = run_length(ctx, x, y, 4, 2);
            if (len & 1) == 1 && (m & BIT_N) == 0
                || (m & BIT_SE) == 0
                || (m & (BIT_S | BIT_SW)) != 0
            {
                0x0C
            } else {
                r % 3 + 9
            }
        } else if (m & BIT_W) != 0 {
            let len = run_length(ctx, x, y, 4, 6);
            if (len & 1) == 1 && (m & BIT_N) == 0
                || (m & BIT_SW) == 0
                || (m & (BIT_S | BIT_SE)) != 0
            {
                0x1C
            } else {
                r % 3 + 0x19
            }
        } else if (m & BIT_S) != 0 {
            let len = run_length(ctx, x, y, 2, 4);
            if (len & 1) == 1 || (m & BIT_SE) == 0 || (m & (BIT_NE | BIT_E)) != 0 {
                4
            } else {
                r % 3 + 1
            }
        } else if (m & BIT_N) != 0 {
            let len = run_length(ctx, x, y, 2, 0);
            if (len & 1) == 1 || (m & BIT_NE) == 0 || (m & (BIT_E | BIT_SE)) != 0 {
                0x14
            } else {
                r % 3 + 0x11
            }
        } else if (m & BIT_NE) != 0 {
            (r & 1) + 0x23
        } else if (m & BIT_SE) != 0 {
            (r & 1) + 0x21
        } else if (m & BIT_SW) != 0 {
            (r & 1) + 0x27
        } else if (m & BIT_NW) != 0 {
            (r & 1) + 0x25
        } else {
            return true;
        }
    };

    stamp(ctx, x, y, piece, variant, region_id, keep)
}

/// Shoreline run-length walk: `a` advances along `walk_dir` starting one step
/// from the cell, `b` is `a` stepped toward the water (`water_dir`); the run
/// continues while `a` is land and `b` is water.
fn run_length(ctx: &mut ShoreCtx<'_>, x: i32, y: i32, walk_dir: usize, water_dir: usize) -> i32 {
    let mut len = 1;
    let (mut ax, mut ay) = RmgGrid::step(x, y, walk_dir);
    let (mut bx, mut by) = RmgGrid::step(ax, ay, water_dir);
    loop {
        let a_water = {
            let tile = ctx.grid.cell_native(ax, ay).tile;
            is_water(ctx.ids, tile)
        };
        let b_water = {
            let tile = ctx.grid.cell_native(bx, by).tile;
            is_water(ctx.ids, tile)
        };
        if a_water || !b_water {
            break;
        }
        len += 1;
        let (nax, nay) = RmgGrid::step(ax, ay, walk_dir);
        let (nbx, nby) = RmgGrid::step(bx, by, walk_dir);
        (ax, ay) = (nax, nay);
        (bx, by) = (nbx, nby);
    }
    len
}

/// The stamping tail shared by both selection passes.
fn stamp(
    ctx: &mut ShoreCtx<'_>,
    x: i32,
    y: i32,
    piece: i32,
    variant: i32,
    region_id: i32,
    keep: bool,
) -> bool {
    debug_assert!((1..=SHORE_PIECES as i32).contains(&piece));
    let new_tile = ctx.ids.shore + piece - 1;
    let (adx, ady) = PIECE_ANCHOR[(piece - 1) as usize];
    let anchor = (x + i32::from(adx), y + i32::from(ady));
    let level = ctx
        .grid
        .get(x, y)
        .expect("select only reaches existing cells")
        .level;

    let (lo, hi) = if variant == 2 {
        (ctx.ids.shore, ctx.ids.shore + 0x29)
    } else {
        (0, 0)
    };
    place_block(
        ctx.grid,
        ctx.scratch,
        ctx.ids,
        ctx.blocks,
        BlockPlacement {
            tile: new_tile,
            origin: anchor,
            level,
            window: (lo, hi),
            region: region_id,
            keep,
        },
    )
    .admitted
}

/// Inputs to the selected TMP placement owner, native `0x0057B440`.
/// The selected TileType and placement origin are native globals; keeping
/// them explicit here avoids shared mutable selection state.
pub(super) struct BlockPlacement {
    pub tile: i32,
    pub origin: (i32, i32),
    pub level: u8,
    pub window: (i32, i32),
    pub region: i32,
    pub keep: bool,
}

/// Native AL and caller-owned out-bool are separate result channels. Shore
/// selection consumes only admission, cliff selection accepts either channel
/// and BuildRiverBridge reads neither.
pub(super) struct PlacementResult {
    pub complete: bool,
    pub admitted: bool,
}

/// Shared selected-TMP placement, original gamemd.exe `0x0057B440..0x0057B78D`,
/// for the shore tiler, the cliff selector and BuildRiverBridge's waterfalls.
/// Source: original instruction audit and executable controls in
/// `tools/rmg_oracle/cliff_materialization.py`, whose per-call AL/out-bool
/// results `cliff_native_tests` compares. Writes tile, subtile and height;
/// unlike unconditional `5A6C10`, this function never writes Cell+11C (slope).
pub(super) fn place_block(
    grid: &mut RmgGrid,
    scratch: &mut RmgScratch,
    ids: &TileIds,
    blocks: &dyn TileBlocks,
    placement: BlockPlacement,
) -> PlacementResult {
    let soft_stop = PlacementResult {
        complete: false,
        admitted: true,
    };
    let hard_stop = PlacementResult {
        complete: false,
        admitted: false,
    };
    let complete = PlacementResult {
        complete: true,
        admitted: true,
    };
    let Some(block) = blocks.block(placement.tile) else {
        // A missing physical TMP/type does not clear the caller's out-bool.
        return soft_stop;
    };
    let region = if placement.keep { 0 } else { placement.region };
    for j in 0..block.height {
        for i in 0..block.width {
            let tx = i32::from((placement.origin.0 as i16).wrapping_add(i as i16));
            let ty = i32::from((placement.origin.1 as i16).wrapping_add(j as i16));
            if !scratch.in_diamond(tx, ty) {
                continue;
            }
            // Native lookup precedes the sparse-TMP check, including the
            // shared border cell's coordinate update on an absent header.
            let target = *grid.cell_native(tx, ty);
            let index = j * block.width + i;
            let Some(sub) = block.subtiles[index as usize] else {
                continue;
            };
            // Only keep zeroes the stored owner (57B576..57B57E). A non-keep
            // owner <= 0 jumps past that `xor` (57B5DE `jle 57B580`) and is
            // compared raw, so a stored -1 equals an unowned (-1) request.
            let owner = if placement.keep {
                0
            } else {
                scratch.get(tx, ty).region
            };
            let target_clear = ids.is_clear(target.tile);
            let old = target.tile.wrapping_sub(ids.shore);
            let new = placement.tile.wrapping_sub(ids.shore);
            if owner != region {
                if owner > 0 && region != -1 {
                    if !target_clear {
                        if (0..=0x29).contains(&old)
                            && (0..=0x29).contains(&new)
                            && PIECE_CLASS[old as usize] == PIECE_CLASS[new as usize]
                        {
                            return complete;
                        }
                        return hard_stop;
                    }
                } else if !target_clear {
                    if region == -1 {
                        continue;
                    }
                    return hard_stop;
                }
                scratch.get_mut(tx, ty).region = region;
            } else if (0..=0x29).contains(&old) && (0..=0x29).contains(&new) {
                let diff = (PIECE_ORIENT[old as usize] - PIECE_ORIENT[new as usize]).abs();
                if (3..=5).contains(&diff) {
                    return hard_stop;
                }
            }
            if target_clear
                || (target.tile >= placement.window.0 && target.tile <= placement.window.1)
            {
                let cell = grid.cell_native_mut(tx, ty);
                cell.tile = placement.tile;
                cell.sub_tile = index as u8;
                cell.level = sub.height.wrapping_add(placement.level);
                continue;
            }
            if ids.is_shore_piece(target.tile)
                && ids.cliffs.is_on_bridge_ramp_tile(placement.tile, index)
            {
                return hard_stop;
            }
            if ids.is_shore_piece(placement.tile)
                && ids
                    .cliffs
                    .is_special_terrain_tile(target.tile, target.sub_tile)
            {
                return hard_stop;
            }
            return soft_stop;
        }
    }
    complete
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::theater::RmgTileKeys;

    /// A selected non-shore TMP; the test ids leave every cliff family unset.
    const CLIFF_TILE: i32 = 900;
    /// Non-clear identity outside the shore window and every cliff family.
    const ROCK_TILE: i32 = 77;
    const SHORE_BASE: i32 = 400;
    /// The block's two subtiles, in native row-major order.
    const FIRST: (i32, i32) = (40, 50);
    const SECOND: (i32, i32) = (41, 50);

    /// Every TileType is a 2x1 block, so a stop at the first subtile leaves
    /// the second cell visibly untouched.
    struct TwoByOne(TileBlock);
    impl TileBlocks for TwoByOne {
        fn block(&self, _tile: i32) -> Option<&TileBlock> {
            Some(&self.0)
        }
    }

    struct Fixture {
        grid: RmgGrid,
        scratch: RmgScratch,
        ids: TileIds,
        blocks: TwoByOne,
    }

    impl Fixture {
        fn new() -> Self {
            let (dmin, dmax) = (34, 34 + 2 * 42);
            let stride = 77;
            let mut grid = RmgGrid::new(stride, dmin, dmax);
            for (x, y) in grid.native_cells().collect::<Vec<_>>() {
                grid.get_mut(x, y).expect("native cell").tile = 0;
            }
            let sub = SubTile {
                height: 1,
                terrain: 0,
                slope: 0,
            };
            Self {
                grid,
                scratch: RmgScratch::new(stride, dmin, dmax),
                ids: TileIds {
                    shore: SHORE_BASE,
                    ..TileIds::from_keys(&RmgTileKeys::default())
                },
                blocks: TwoByOne(TileBlock {
                    width: 2,
                    height: 1,
                    subtiles: vec![Some(sub); 2],
                }),
            }
        }

        fn set(&mut self, (x, y): (i32, i32), tile: i32, owner: i32) {
            self.grid.get_mut(x, y).expect("native cell").tile = tile;
            self.scratch.get_mut(x, y).region = owner;
        }

        fn cell(&self, (x, y): (i32, i32)) -> (i32, u8, u8, i32) {
            let cell = self.grid.get(x, y).expect("native cell");
            let owner = self.scratch.get(x, y).region;
            (cell.tile, cell.sub_tile, cell.level, owner)
        }

        /// One `0x57B440` call at FIRST with window (0, 0) and level 4;
        /// returns (AL, caller out-bool) as (complete, admitted).
        fn place(&mut self, tile: i32, region: i32, keep: bool) -> (bool, bool) {
            let result = place_block(
                &mut self.grid,
                &mut self.scratch,
                &self.ids,
                &self.blocks,
                BlockPlacement {
                    tile,
                    origin: FIRST,
                    level: 4,
                    window: (0, 0),
                    region,
                    keep,
                },
            );
            (result.complete, result.admitted)
        }
    }

    /// Untouched cells keep the default level 4.
    const UNTOUCHED: (i32, u8, u8, i32) = (0, 0, 4, 0);

    /// 57B5DE `jle 57B580` skips the keep-only `xor eax,eax` at 57B57E, so a
    /// stored -1 equals an unowned (-1) request raw. The orientation check
    /// passes, and the write gate soft-stops at a non-clear cell outside the
    /// window (57B784: AL=0, out-bool untouched) before the second subtile.
    #[test]
    fn unowned_request_soft_stops_at_a_nonclear_cell_it_already_owns() {
        let mut fixture = Fixture::new();
        fixture.set(FIRST, ROCK_TILE, -1);
        assert_eq!(fixture.place(CLIFF_TILE, -1, false), (false, true));
        assert_eq!(fixture.cell(FIRST), (ROCK_TILE, 0, 4, -1));
        assert_eq!(fixture.cell(SECOND), UNTOUCHED, "later subtile untouched");
    }

    /// A stored 0 differs from the -1 request (57B6B6): the non-clear cell is
    /// skipped (57B6DF), and the clear second cell is adopted and written
    /// with its subtile index and height + level.
    #[test]
    fn unowned_request_skips_a_nonclear_cell_owned_by_nobody() {
        let mut fixture = Fixture::new();
        fixture.set(FIRST, ROCK_TILE, 0);
        assert_eq!(fixture.place(CLIFF_TILE, -1, false), (true, true));
        assert_eq!(fixture.cell(FIRST), (ROCK_TILE, 0, 4, 0));
        assert_eq!(fixture.cell(SECOND), (CLIFF_TILE, 1, 5, -1));
    }

    /// 57B6D6..57B77D: a non-clear cell whose stored owner is not positive
    /// refuses an owned request: out-bool cleared, AL=0, nothing written.
    #[test]
    fn owned_request_refuses_a_nonclear_cell_without_a_positive_owner() {
        let mut fixture = Fixture::new();
        fixture.set(FIRST, ROCK_TILE, -1);
        assert_eq!(fixture.place(CLIFF_TILE, 5, false), (false, false));
        assert_eq!(fixture.cell(SECOND), UNTOUCHED);
    }

    /// 57B463/57B57E: keep zeroes both the request and the stored owner, so
    /// foreign cells take the orientation/write path without being adopted.
    #[test]
    fn keep_writes_foreign_cells_without_adopting_them() {
        let mut fixture = Fixture::new();
        fixture.set(FIRST, 0, 9);
        fixture.set(SECOND, 0, 9);
        assert_eq!(fixture.place(CLIFF_TILE, 5, true), (true, true));
        assert_eq!(fixture.cell(FIRST), (CLIFF_TILE, 0, 5, 9));
        assert_eq!(fixture.cell(SECOND), (CLIFF_TILE, 1, 5, 9));
    }

    /// 57B5EF..57B705: a non-clear foreign shore piece in the selected
    /// piece's class ends the call with AL=1 before the later subtile.
    #[test]
    fn equivalent_foreign_shore_piece_ends_the_block_successfully() {
        let mut fixture = Fixture::new();
        // Pieces 0, 1 and 2 share class 0 (0x82A7F4).
        fixture.set(FIRST, SHORE_BASE + 1, 7);
        assert_eq!(fixture.place(SHORE_BASE, 5, false), (true, true));
        assert_eq!(fixture.cell(FIRST), (SHORE_BASE + 1, 0, 4, 7));
        assert_eq!(fixture.cell(SECOND), UNTOUCHED);
    }
}
