//! Ordered post-connector cliff jitter and face materialization.
//!
//! Original gamemd.exe `5A19E0`, then `578E60`/`579620`; the active
//! Generate path calls these before the separate `5A17F0` restamp pass.
//! Source: original instruction bodies and the consumed native controls in
//! `tools/rmg_oracle/cliff_materialization.py`.

use crate::map::rmg::grid::{CellRef, RmgGrid};
use crate::map::rmg::rng::RmgRng;
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;

use super::island_passes::{raise_pinched, ring_mask};
use super::shore::{BlockPlacement, TileBlocks, place_block};

/// Signed 16-bit step from a held reference's live coordinate, then the
/// coordinate-stamping lookup.
fn neighbor(grid: &mut RmgGrid, held: CellRef, dx: i16, dy: i16) -> CellRef {
    let (x, y) = grid.ref_coord(held);
    grid.cell_ref(
        i32::from((x as i16).wrapping_add(dx)),
        i32::from((y as i16).wrapping_add(dy)),
    )
}

fn mask(grid: &mut RmgGrid, scratch: &RmgScratch, ids: &TileIds, held: CellRef) -> u8 {
    let (x, y) = grid.ref_coord(held);
    ring_mask(grid, scratch, ids, x, y)
}

fn neighbor_mask(
    grid: &mut RmgGrid,
    scratch: &RmgScratch,
    ids: &TileIds,
    held: CellRef,
    dx: i16,
    dy: i16,
) -> u8 {
    let next = neighbor(grid, held, dx, dy);
    mask(grid, scratch, ids, next)
}

/// Original `5A19E0..5A1C65`: adjust zigzag edges before placing faces.
///
/// The five neighbor pointers are acquired before any mask call. A retained
/// border pointer therefore sees the shared Dummy's latest coordinate, even
/// after a mask has looked up more neighbors. Each admitted 83/38 case makes
/// the original bounded coin draw; E0 consumes no random word.
pub(in crate::map::rmg) fn jitter(
    grid: &mut RmgGrid,
    scratch: &mut RmgScratch,
    ids: &TileIds,
    rng: &mut RmgRng,
) {
    for (x, y) in grid.native_cells() {
        let current = CellRef::Cell(x as i16, y as i16);
        let southwest = neighbor(grid, current, -1, 1);
        let northeast = neighbor(grid, current, 1, -1);
        let southeast = neighbor(grid, current, 1, 1);
        let east = neighbor(grid, current, 1, 0);
        let west = neighbor(grid, current, -1, 0);
        let m = mask(grid, scratch, ids, current);
        let target = match m {
            0x83 => {
                if mask(grid, scratch, ids, southeast) != 0x83
                    || !ids.is_clear(grid.ref_cell(east).tile)
                    || rng.uniform(0, 1) != 0
                {
                    continue;
                }
                let target = grid.ref_cell_mut(east);
                target.level = target.level.wrapping_sub(4);
                east
            }
            0x38 => {
                if mask(grid, scratch, ids, southeast) != 0x38
                    || !ids.is_clear(grid.ref_cell(west).tile)
                    || rng.uniform(0, 1) != 0
                {
                    continue;
                }
                let target = grid.ref_cell_mut(west);
                target.level = target.level.wrapping_sub(4);
                west
            }
            0xE0 => {
                if mask(grid, scratch, ids, southwest) != 0xE0
                    || mask(grid, scratch, ids, northeast) != 0xE0
                    || !ids.is_clear(grid.ref_cell(current).tile)
                {
                    continue;
                }
                let target = grid.ref_cell_mut(current);
                target.level = target.level.wrapping_add(4);
                current
            }
            _ => continue,
        };
        let source = if m == 0xE0 { west } else { current };
        let (sx, sy) = grid.ref_coord(source);
        let region = scratch.get(sx, sy).region;
        let (tx, ty) = grid.ref_coord(target);
        scratch.get_mut(tx, ty).region = region;
    }
}

/// Cliff face selection and placement, original `579620..579B68`.
/// Every admitted flat clear cell draws `[0,5]` before deriving its mask,
/// including cells for which no piece is selected.
pub(super) fn select_cell(
    grid: &mut RmgGrid,
    scratch: &mut RmgScratch,
    ids: &TileIds,
    blocks: &dyn TileBlocks,
    rng: &mut RmgRng,
    x: i32,
    y: i32,
    region: i32,
) -> bool {
    let held = grid.cell_ref(x, y);
    let choice = rng.uniform(0, 5);
    let m = mask(grid, scratch, ids, held);
    let piece = if m & 0xA0 == 0xA0 {
        let south = neighbor_mask(grid, scratch, ids, held, 0, 1);
        let east = neighbor_mask(grid, scratch, ids, held, 1, 0);
        if south & 0x20 != 0 || east & 0x80 != 0 {
            34
        } else {
            9 + choice % 3
        }
    } else if m & 0x82 == 0x82 {
        39
    } else if m & 0x0A == 0x0A {
        33
    } else if m & 0x28 == 0x28 {
        40
    } else if m & 0x02 != 0 {
        let south = neighbor_mask(grid, scratch, ids, held, 0, 1);
        let southeast = neighbor_mask(grid, scratch, ids, held, 1, 1);
        if m & 0x04 != 0 && m & 0x18 == 0 && south & 0x0A != 0x0A {
            35 + choice % 3
        } else if m & 0x0C == 0 && southeast & 0x02 == 0 && choice & 1 != 0 {
            1
        } else {
            38
        }
    } else if m & 0x20 != 0 {
        let south = neighbor_mask(grid, scratch, ids, held, 0, 1);
        if m & 0x10 != 0 && m & 0x0C == 0 && south & 0x28 != 0x28 {
            15 + choice % 3
        } else {
            18
        }
    } else if m & 0x08 != 0 {
        let east = neighbor_mask(grid, scratch, ids, held, 1, 0);
        let southeast = neighbor_mask(grid, scratch, ids, held, 1, 1);
        if m & 0x04 != 0 && m & 0x03 == 0 && east & 0x0A != 0x0A {
            23 + choice % 3
        } else if m & 0x06 == 0 && southeast & 0x08 == 0 && choice & 1 != 0 {
            21
        } else {
            26
        }
    } else if m & 0x80 != 0 {
        let east = neighbor_mask(grid, scratch, ids, held, 1, 0);
        if m & 0x01 != 0 && m & 0x06 == 0 && east & 0x82 != 0x82 {
            5 + choice % 3
        } else {
            8
        }
    } else if m & 0x01 != 0 {
        2
    } else if m & 0x04 != 0 {
        29 + (choice & 1)
    } else if m & 0x10 != 0 {
        22
    } else if m & 0x40 != 0 {
        12 + choice % 3
    } else {
        return true;
    };

    // Original CRT initializer579320..57961A explicitly stores (0,0) for
    // pieces1..40 except piece32=(1,0), via CellStruct constructor42D470.
    // This is initialized BSS ABDDA4/DDA6, indexed by piece, not piece-1.
    let anchor_x: i16 = if piece == 32 { 1 } else { 0 };
    let (cx, cy) = grid.ref_coord(held);
    let origin = (i32::from((cx as i16).wrapping_add(anchor_x)), cy);
    let level = grid.ref_cell(held).level;
    let tile = ids.cliffs.cliff_set.map_or(-1, i32::from) + piece - 1;
    let result = place_block(
        grid,
        scratch,
        ids,
        blocks,
        BlockPlacement {
            tile,
            origin,
            level,
            window: (0, 0),
            region,
            keep: false,
        },
    );
    #[cfg(test)]
    super::cliff_native_tests::observe_placement(tile, origin, &result);
    // 579B44 accepts either AL or the caller-owned admission out-bool.
    result.complete || result.admitted
}

/// Original `578E60..57900C`: reset enables, repair all cliffs, then stamp
/// flat clear cells in a second full native traversal. A hard refusal stops
/// the remaining work and preserves any writes made before the refusal.
///
/// The generator already owns scratch, so this borrowed pass keeps it alive;
/// native allocation/free applies only to a caller that supplied no scratch.
/// Selected TileType/origin are explicit placement inputs rather than mutable
/// globals, so no selected-type or temporary-object state escapes the pass.
pub(in crate::map::rmg) fn materialize(
    grid: &mut RmgGrid,
    scratch: &mut RmgScratch,
    ids: &TileIds,
    blocks: &dyn TileBlocks,
    rng: &mut RmgRng,
    region: i32,
) -> bool {
    for record in scratch.cells_mut() {
        record.shore_enable = true;
    }
    for (x, y) in grid.native_cells() {
        if !raise_pinched(grid, scratch, ids, x, y, region) {
            return false;
        }
    }
    for (x, y) in grid.native_cells() {
        let current = *grid.cell_native(x, y);
        if current.slope == 0
            && ids.is_clear(current.tile)
            && !select_cell(grid, scratch, ids, blocks, rng, x, y, region)
        {
            return false;
        }
    }
    true
}
