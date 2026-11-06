//! The area tests that decide whether something may be placed on a patch of
//! ground.
//!
//! Two phases ask the same question in the same words. The start placer offers
//! a 6x6 window around each candidate waypoint; the low-bridge placer offers a
//! 3-cell-wide strip as it walks a corridor outward from its seed. Both go
//! through **one routine in the original**, so they are one routine here — two
//! copies of a native predicate is how the copies start disagreeing.
//!
//! The routine takes two override bytes that would let paved roads and
//! road-ends through instead of refusing them. Both call sites pass zero for
//! both, so the overrides are folded away and the rule is simply: a road
//! refuses, and what is left has to be clear ground, misc-pave or pave.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;

/// The four corners of `rect`, each tested against the map diamond.
///
/// The diamond is convex, so four corners inside it put the whole rectangle
/// inside — which is what lets the sweeps that follow read cells without
/// re-testing every one.
pub fn corners_in_diamond(scratch: &RmgScratch, rect: (i32, i32, i32, i32)) -> bool {
    let (rx, ry, w, h) = rect;
    [
        (rx, ry),
        (rx + w - 1, ry),
        (rx, ry + h - 1),
        (rx + w - 1, ry + h - 1),
    ]
    .iter()
    .all(|&(x, y)| scratch.in_diamond(x, y))
}

/// May something stand on this tile?
///
/// The refusal is a separate step rather than just an absence from the accept
/// list, and that matters: these spans are hardcoded lengths, not the real size
/// of each tileset, so a short set lets a span run on into its neighbour and a
/// tile can land in two families at once. Where that happens the refusal wins.
pub fn tile_is_placeable(ids: &TileIds, tile: i32) -> bool {
    if ids.is_paved_road(tile) || ids.is_paved_road_end(tile) {
        return false;
    }
    ids.is_clear(tile) || ids.is_misc_pave(tile) || ids.is_pave(tile)
}

/// `RandomMapGenerator::AreaIsPavedClear @ 0x005A7250`: is every cell of
/// `rect` clear of roads and standable?
///
/// Its two override bytes (EDX and the stack byte) admit roads and road ends.
/// Every reachable call passes zero: `PlaceLowBridgeDeck @ 0x0058F2C0` and
/// `RmgRegion::PlaceStartingPoints` (0x00594992, EBP cleared at 0x00594892).
/// Only 0x005A5B50/0x005A6D80/0x005A7070, reached from the unreferenced
/// 0x005A5020, and the unreferenced 0x005A80B0 set them, so the port has none.
///
/// Corners first, then **exactly `w x h`** — no inclusive margin. There is no
/// level test here; a rect spanning a step in the ground still passes. The
/// low-bridge end-piece check is this test plus level uniformity, and the deck
/// validator is a different sweep again, so none of the three may share a walk.
pub fn area_is_paved_clear(
    grid: &RmgGrid,
    scratch: &RmgScratch,
    ids: &TileIds,
    rect: (i32, i32, i32, i32),
) -> bool {
    let (rx, ry, w, h) = rect;

    if !corners_in_diamond(scratch, rect) {
        return false;
    }

    for y in ry..(ry + h) {
        for x in rx..(rx + w) {
            // The corner probes put the whole rect inside the diamond, and the
            // diamond is inside the array, so this never actually falls out.
            let Some(cell) = grid.get(x, y) else {
                return false;
            };
            if !tile_is_placeable(ids, cell.tile) {
                return false;
            }
        }
    }
    true
}
