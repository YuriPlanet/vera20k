//! Ramp-site selection for the island passes: where a cliff line gets a stair.
//!
//! On the two island map types, adjacent plateaus that differ in height are
//! joined by carving a ramp through the cliff between them. Choosing where is
//! two steps, both here:
//!
//! 1. [`ring_orientation_mask`] tiles a 15x15 area around a candidate cell with
//!    nine 5x5 windows and asks, for each of the eight around the centre,
//!    whether the region owns enough cells in it. The answers become one
//!    direction mask.
//! 2. `carve::try_carve_connector_at_cell` reads that mask and walks the ramp
//!    shapes in the original's order, jittering each straight shape's two
//!    endpoints as it goes.
//!
//! **This module selects; it does not carve.** The routines that stamp ramp
//! tiles live in `carve`, and the pass that drives them in `carve_driver`.
//!
//! Depends on the grid/scratch owners and the x87 environment; no rendering,
//! no rules.

use crate::map::rmg::rng::RmgRng;
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::x87::TruncF64;
use crate::util::native_x87::X87Chop53;

/// Side of the 5x5 window grid: nine windows, three per axis.
const RING_SPAN: i32 = 3;
/// Edge of one sampling window, in cells.
const WINDOW: i32 = 5;
/// Offset from the candidate cell to the centre window's north-west corner.
const WINDOW_ORIGIN: i32 = 2;

/// Cell-count bar at leniency 0 and at leniency 1. The bar falls linearly
/// between them, which is the real effect of a late attempt: not "more shapes
/// allowed" but "less of the region needed in each window".
const THRESHOLD_AT_STRICT: f64 = 15.0;
const THRESHOLD_AT_LENIENT: f64 = 5.0;

/// Leniency above which the fixed-geometry fallback shapes are tried. With the
/// 0.01 step this first holds at attempt 51 — attempt 50 lands just under a
/// half because the step is the binary float nearest 0.01, not 0.01 itself.
/// `carve::try_carve_connector_at_cell` makes the comparison.
pub(crate) const FALLBACK_LENIENCY: f32 = 0.5;

/// Leniency added per failed attempt.
pub const LENIENCY_STEP: f32 = 0.01;

/// `2 * (1 + 2^-32) * 2^-32` — the endpoint jitter's uniform over `{0, 1}`,
/// pre-divided so the draw is one multiply and a truncation. Not
/// interchangeable with the river family's `n/(2^32-1)` constants: the
/// roundings differ.
const JITTER_SCALE_BITS: u64 = 0x3E00_0000_0010_0000;

/// Mask bit per ring slot, indexed `col + 3 * row` with row 0 north and column
/// 0 west. The centre slot is skipped and contributes nothing.
///
/// The layout is `1 << ((dir + 7) mod 8)` over the standard direction order, so
/// north is the high bit and north-east the low one — the same layout the cliff
/// edge mask uses.
const RING_SLOT_BITS: [u8; 9] = [0x40, 0x80, 0x01, 0x20, 0x00, 0x02, 0x10, 0x08, 0x04];

pub(crate) const NE: u8 = 0x01;
pub(crate) const E: u8 = 0x02;
pub(crate) const SE: u8 = 0x04;
pub(crate) const S: u8 = 0x08;
pub(crate) const SW: u8 = 0x10;
pub(crate) const W: u8 = 0x20;
pub(crate) const NW: u8 = 0x40;
pub(crate) const N: u8 = 0x80;

/// A straight ramp shape: a run along a face whose named side carries no high
/// ground. Each indexes its row of the executable's ramp tables
/// (`ramp::ramp_record`); the corner carvers read no table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RampShape {
    /// North-south run with the west side clear.
    ClearWest,
    /// North-south run with the east side clear.
    ClearEast,
    /// East-west run with the north side clear.
    ClearNorth,
    /// East-west run with the south side clear.
    ClearSouth,
}

/// Does `region` own at least `threshold` cells inside the rect?
///
/// Stops the moment the bar is met, so it is a threshold test and never a
/// count. Cells outside the diamond are skipped, not counted as misses.
pub(crate) fn region_meets_quota_in_rect(
    scratch: &RmgScratch,
    rect: (i32, i32, i32, i32),
    region: i32,
    threshold: i32,
) -> bool {
    let (rx, ry, w, h) = rect;
    let mut count = 0;
    for row in 0..h {
        for col in 0..w {
            let (x, y) = (rx + col, ry + row);
            if !scratch.in_diamond(x, y) {
                continue;
            }
            if scratch.get(x, y).region != region {
                continue;
            }
            count += 1;
            if count >= threshold {
                return true;
            }
        }
    }
    false
}

/// The per-window cell-count bar for this attempt: `trunc(10 * (1 - l) + 5)`.
///
/// The subtraction happens in single precision because the leniency arrives as
/// a float — at leniency 0.99 that is what puts the result a hair above 5
/// rather than a hair below, so the bar lands on 5 and not 4.
fn window_threshold(leniency: f32) -> i32 {
    let span = TruncF64::from_f64(THRESHOLD_AT_STRICT - THRESHOLD_AT_LENIENT);
    let remaining = TruncF64::from_f64(f64::from(1.0f32 - leniency));
    X87Chop53::ftol_f64_low_masked(
        span.mul(remaining)
            .add(TruncF64::from_f64(THRESHOLD_AT_LENIENT))
            .to_f64(),
    )
}

/// Direction mask of the eight 5x5 windows around `cell` that `region` fills.
///
/// Returns `None` when the **centre** window fails the same bar — the candidate
/// cell is then rejected outright rather than carved with an empty mask.
pub(crate) fn ring_orientation_mask(
    scratch: &RmgScratch,
    cell: (i32, i32),
    region: i32,
    leniency: f32,
) -> Option<u8> {
    let threshold = window_threshold(leniency);
    let origin = (cell.0 - WINDOW_ORIGIN, cell.1 - WINDOW_ORIGIN);
    if !region_meets_quota_in_rect(
        scratch,
        (origin.0, origin.1, WINDOW, WINDOW),
        region,
        threshold,
    ) {
        return None;
    }

    let mut mask = 0u8;
    for row in 0..RING_SPAN {
        for col in 0..RING_SPAN {
            if row == 1 && col == 1 {
                continue;
            }
            let rect = (
                origin.0 + WINDOW * (col - 1),
                origin.1 + WINDOW * (row - 1),
                WINDOW,
                WINDOW,
            );
            if region_meets_quota_in_rect(scratch, rect, region, threshold) {
                mask |= RING_SLOT_BITS[(col + RING_SPAN * row) as usize];
            }
        }
    }
    Some(mask)
}

/// One uniform over `{0, 1}`, the endpoint jitter.
///
/// The redraw guard cannot fire under the generator's truncating rounding — the
/// largest draw lands exactly on 1 — but it is what the original does and costs
/// nothing.
pub(crate) fn jitter(rng: &mut RmgRng) -> i32 {
    let scale = TruncF64::from_f64(f64::from_bits(JITTER_SCALE_BITS));
    loop {
        let value = X87Chop53::ftol_f64_low_masked(
            TruncF64::from_f64(f64::from(rng.next_u32()))
                .mul(scale)
                .to_f64(),
        );
        if value <= 1 {
            return value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> RmgScratch {
        let (dmin, dmax) = (34, 34 + 2 * 42);
        RmgScratch::new((34 + 42 + 1) as usize, dmin, dmax)
    }

    /// Paint a filled rect into the scratch as `region`.
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
    fn the_threshold_falls_from_fifteen_to_five() {
        // The whole point of leniency: not more shapes, a lower bar. Pinning
        // both ends and the step catches a sign flip or a swapped pair.
        assert_eq!(window_threshold(0.0), 15, "first attempt is strictest");
        assert_eq!(window_threshold(99.0 * LENIENCY_STEP), 5, "last attempt");
        assert_eq!(window_threshold(50.0 * LENIENCY_STEP), 10, "halfway");
    }

    #[test]
    fn the_leniency_subtraction_happens_in_single_precision() {
        // Attempt 10 is the ONE index in the whole run where doing
        // `1 - leniency` in double instead of float changes the answer: ten
        // steps of the float nearest 0.01 land a hair below 0.1, so the float
        // subtraction gives just under 0.9 and the bar drops to 13, while
        // widening first gives just over and it stays at 14. Every other
        // attempt agrees, which is exactly why this needs pinning — the bug
        // would hide in a spot check.
        assert_eq!(window_threshold(10.0 * LENIENCY_STEP), 13);
        assert_eq!(window_threshold(9.0 * LENIENCY_STEP), 14, "the step before");
    }

    #[test]
    fn a_window_over_the_diamond_edge_still_counts_its_inside() {
        // Windows near the map edge hang off the diamond. Those cells are
        // skipped, not counted against the region — treating them as misses
        // would strip ramps off every map edge, where players actually meet
        // them. This rect has 19 of its 25 cells inside.
        let mut s = scratch();
        let rect = (16, 16, WINDOW, WINDOW);
        paint(&mut s, rect, 7);
        assert!(!s.in_diamond(16, 16), "the rect really does hang off");
        assert!(region_meets_quota_in_rect(&s, rect, 7, 15));
        assert!(
            !region_meets_quota_in_rect(&s, rect, 7, 20),
            "only 19 inside"
        );
    }

    #[test]
    fn the_ring_mask_puts_north_in_the_high_bit() {
        // Same bit layout as the cliff edge mask. A rotation here would mirror
        // every ramp on the map.
        let mut s = scratch();
        let cell = (40, 50);
        // Centre window plus the window one span north.
        paint(&mut s, (38, 48, 5, 5), 7);
        paint(&mut s, (38, 43, 5, 5), 7);
        assert_eq!(
            ring_orientation_mask(&s, cell, 7, 0.0),
            Some(0x80),
            "north only"
        );
    }

    #[test]
    fn a_lower_bar_can_turn_a_reject_into_a_mask() {
        // The leniency ramp has to be able to rescue a cell that a strict
        // attempt threw away, or the 100-attempt loop would be pointless.
        let mut s = scratch();
        paint(&mut s, (38, 48, 3, 3), 7); // 9 cells: under 15, over 5
        assert_eq!(ring_orientation_mask(&s, (40, 50), 7, 0.0), None);
        assert_eq!(
            ring_orientation_mask(&s, (40, 50), 7, 99.0 * LENIENCY_STEP),
            Some(0)
        );
    }
}
