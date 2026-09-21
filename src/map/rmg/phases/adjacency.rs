//! Which regions touch which, and how big each one is.
//!
//! The island passes rebuild the region partition from scratch, and the ramp
//! carver then needs to know, for every region, which other regions it borders.
//! That list is built here — once for every region, before any carving starts.
//!
//! **The list is per-pass scratch, not region state.** The original allocates
//! it, runs the carver over it, and frees it again in a third loop, so it never
//! outlives the pass. Storing it on the region would misrepresent that
//! lifetime, so these functions hand it back instead.
//!
//! No randomness here at all.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::scratch::RmgScratch;

/// Region ids of every region that touches `region`, ascending.
///
/// **Ascending, not the order they were found.** The original flags adjacency
/// into one slot per region id and then reads that array back in order, which
/// throws discovery order away. That is what makes the carve pass
/// deterministic: with the driver visiting only pairs where its own id is the
/// lower, every adjacent pair comes up exactly once, in a fixed order. Emitting
/// neighbours as they are discovered would carve the same ramps in a different
/// order and drift apart as soon as two pairs want the same border cells.
///
/// `region_count` is the number of ids the partition has issued; ids run
/// `0..region_count`.
pub(crate) fn neighbour_ids(scratch: &RmgScratch, region: i32, region_count: i32) -> Vec<i32> {
    if region_count <= 0 {
        return Vec::new();
    }
    let mut touched = vec![false; region_count as usize];

    for (x, y) in border_cells_of(scratch, region) {
        for dir in 0..8usize {
            let (nx, ny) = RmgGrid::step(x, y, dir);
            if !scratch.in_diamond(nx, ny) {
                continue;
            }
            let owner = scratch.get(nx, ny).region;
            // `>= 0` — the unassigned marker is excluded, but region 0 is a
            // real region and does get flagged.
            //
            // Loosening this to admit -1 is provably inert here, because the
            // bounds-checked lookup below drops any id outside the range. The
            // check is kept because it is what the original tests, and because
            // relying on the lookup to absorb it would also silently swallow
            // an id ABOVE the range — which would be a real partition bug
            // rather than a marker.
            if owner < 0 {
                continue;
            }
            if let Some(slot) = touched.get_mut(owner as usize) {
                *slot = true;
            }
        }
    }

    (0..region_count)
        .filter(|id| touched[*id as usize] && *id != region)
        .collect()
}

/// Cells belonging to `region` that touch a differently-owned in-bounds cell.
pub(crate) fn border_cells_of(scratch: &RmgScratch, region: i32) -> Vec<(i32, i32)> {
    let width = scratch.width() as i32;
    let mut border = Vec::new();
    for y in 0..width {
        for x in 0..width {
            if !scratch.in_diamond(x, y) || scratch.get(x, y).region != region {
                continue;
            }
            for dir in 0..8usize {
                let (nx, ny) = RmgGrid::step(x, y, dir);
                if scratch.in_diamond(nx, ny) && scratch.get(nx, ny).region != region {
                    border.push((x, y));
                    break;
                }
            }
        }
    }
    border
}

/// How many cells `region` owns.
///
/// Counted over the whole working grid rather than taken from the flood that
/// built the region, and **the cell at (0, 0) is never counted** — the original
/// skips any slot whose packed coordinate is zero. That corner sits outside the
/// playable diamond on every real map, so the exclusion does no work in
/// practice; it is kept because a count that silently differs by one is exactly
/// the sort of thing that surfaces much later as an off-by-one somewhere else.
/// The active connector/low-deck driver uses this to gate whether a region is
/// substantial enough for its candidate search.
pub(crate) fn region_cell_count(scratch: &RmgScratch, region: i32) -> i32 {
    let width = scratch.width() as i32;
    let mut count = 0;
    for y in 0..width {
        for x in 0..width {
            if x == 0 && y == 0 {
                continue;
            }
            if scratch.get(x, y).region == region {
                count += 1;
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scratch with every slot unassigned.
    ///
    /// A fresh `RmgScratch` zeroes its region ids, which would read as
    /// "everything belongs to region 0" — and region 0 is a real region here,
    /// so the fixture has to clear to the unassigned marker first. That is
    /// also the state the pass actually runs against: the island passes reset
    /// every slot to -1 before rebuilding.
    fn scratch() -> RmgScratch {
        let (dmin, dmax) = (34, 34 + 2 * 42);
        let mut s = RmgScratch::new((34 + 42 + 1) as usize, dmin, dmax);
        for slot in s.cells_mut() {
            slot.region = -1;
        }
        s
    }

    fn paint(scratch: &mut RmgScratch, rect: (i32, i32, i32, i32), region: i32) {
        let (rx, ry, w, h) = rect;
        for row in 0..h {
            for col in 0..w {
                let (x, y) = (rx + col, ry + row);
                if scratch.in_diamond(x, y) {
                    scratch.get_mut(x, y).region = region;
                }
            }
        }
    }

    #[test]
    fn neighbours_come_back_in_ascending_id_order() {
        // The whole point: discovery order is thrown away. Region 5 is laid
        // out so that walking its border row-major meets region 9 before
        // region 2, and the answer must still be [2, 9].
        let mut s = scratch();
        paint(&mut s, (40, 44, 6, 12), 5);
        paint(&mut s, (40, 42, 6, 2), 9); // north of 5 -- met first
        paint(&mut s, (40, 56, 6, 2), 2); // south of 5 -- met last
        assert_eq!(neighbour_ids(&s, 5, 12), vec![2, 9]);
    }

    #[test]
    fn a_region_is_never_its_own_neighbour() {
        let mut s = scratch();
        paint(&mut s, (40, 44, 6, 12), 5);
        paint(&mut s, (40, 42, 6, 2), 9);
        assert!(!neighbour_ids(&s, 5, 12).contains(&5));
    }

    #[test]
    fn region_zero_is_a_real_neighbour_but_unassigned_is_not() {
        // The guard admits id 0 and rejects the -1 marker. Treating 0 as
        // "no region" would drop a real adjacency; treating -1 as one would
        // invent a neighbour out of bare ground.
        let mut s = scratch();
        paint(&mut s, (40, 44, 6, 12), 5);
        paint(&mut s, (40, 42, 6, 2), 0);
        // Everything else in the diamond is still the -1 default.
        assert_eq!(neighbour_ids(&s, 5, 12), vec![0]);
    }

    #[test]
    fn regions_that_only_touch_diagonally_still_count() {
        // Adjacency is over all eight directions, not four.
        let mut s = scratch();
        paint(&mut s, (44, 48, 2, 2), 5);
        paint(&mut s, (46, 50, 2, 2), 7); // corner-to-corner with 5
        assert_eq!(neighbour_ids(&s, 5, 12), vec![7]);
    }

    #[test]
    fn an_isolated_region_has_no_neighbours() {
        let mut s = scratch();
        paint(&mut s, (44, 48, 4, 4), 5);
        assert!(
            neighbour_ids(&s, 5, 12).is_empty(),
            "only bare ground around"
        );
    }

    #[test]
    fn the_cell_count_skips_the_origin() {
        // (0, 0) is never counted. It lies outside the playable diamond on a
        // real map, so this is inert there -- but the count must still be the
        // original's, not a tidied-up one.
        let mut s = scratch();
        paint(&mut s, (44, 48, 4, 4), 5);
        assert_eq!(region_cell_count(&s, 5), 16, "the painted block");
        s.get_mut(0, 0).region = 5;
        assert_eq!(region_cell_count(&s, 5), 16, "the origin does not add one");
    }
}
