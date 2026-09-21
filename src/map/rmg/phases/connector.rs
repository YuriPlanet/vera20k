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
use crate::map::rmg::x87::{self, TruncF64};

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

/// Which ramp shape the mask selected.
///
/// The four corner shapes turn a ramp around a plateau corner; the four
/// straight shapes run it along a face whose named side carries no high ground.
/// `CornerSw` and `CornerNw` are mirror images across the isometric diagonal
/// and share one carve routine in the original — kept separate here because
/// their endpoint geometry differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RampShape {
    CornerNe,
    CornerSe,
    CornerSw,
    CornerNw,
    /// North-south run with the west side clear.
    ClearWest,
    /// North-south run with the east side clear.
    ClearEast,
    /// East-west run with the north side clear.
    ClearNorth,
    /// East-west run with the south side clear.
    ClearSouth,
}

/// A selected ramp: the shape and the two endpoints to carve between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RampSite {
    pub shape: RampShape,
    pub a: (i32, i32),
    pub b: (i32, i32),
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
    x87::ftol(
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
        let value = x87::ftol(
            TruncF64::from_f64(f64::from(rng.next_u32()))
                .mul(scale)
                .to_f64(),
        );
        if value <= 1 {
            return value;
        }
    }
}

/// Pick the ramp shape for `cell`, or `None` if none fits.
///
/// **Consumes RNG, and the count depends on which shape is chosen** — the four
/// straight shapes jitter both endpoints (two draws), the four corner shapes do
/// not draw at all. That is why this cannot be wired into the pipeline until
/// the carve routines exist: the original retries a shape only when its carve
/// *fails*, so without carving there is no way to know whether to move on to
/// the next shape, and guessing would put the draw stream out of step for the
/// rest of the map.
///
/// What this does model exactly is the **first** candidate: the guards are
/// evaluated in the original's order, and the winner's draws are taken in the
/// original's order.
/// SUPERSEDED by `carve::try_carve_connector_at_cell`, which walks the shapes
/// in order and moves on when a carve refuses. **Do not call this from new
/// code** — it stops at the first shape whose guard matches, which spends the
/// wrong number of draws. Kept only because its tests still pin the guard
/// order, the endpoint geometry and the per-shape draw counts, all of which
/// the replacement relies on. Delete once those tests are moved across.
#[cfg(test)]
pub(crate) fn select_ramp_site(mask: u8, cell: (i32, i32), rng: &mut RmgRng) -> Option<RampSite> {
    let (x, y) = (cell.0, cell.1);

    // A cell whose region surrounds it on all eight sides is interior, not an
    // edge, and never carries a ramp.
    //
    // Redundant, provably: every shape guard below also requires some bit to
    // be clear, so a full mask falls through all of them and returns `None`
    // anyway, without drawing. Kept because the original checks it, and
    // because it says what the shape guards only imply.
    if mask == 0xFF {
        return None;
    }

    let site = |shape, a, b| Some(RampSite { shape, a, b });

    if mask & (N | E) == (N | E) && mask & (S | SW | W) == 0 {
        let ay = if mask & NW != 0 { y - 4 } else { y - 5 };
        let bx = if mask & SE != 0 { x + 4 } else { x + 5 };
        return site(RampShape::CornerNe, (x - 1, ay), (bx, y + 1));
    }
    if mask & (S | E) == (S | E) && mask & (NE | SW) == 0 {
        let ax = if mask & NE != 0 { x + 4 } else { x + 5 };
        let by = if mask & SW != 0 { y + 4 } else { y + 5 };
        return site(RampShape::CornerSe, (ax, y - 1), (x - 1, by));
    }
    if mask & (S | W) == (S | W) && mask & (N | NE | E) == 0 {
        let ay = if mask & SE != 0 { y + 4 } else { y + 5 };
        let bx = if mask & NW != 0 { x - 6 } else { x - 5 };
        return site(RampShape::CornerSw, (x + 1, ay), (bx, y - 1));
    }
    if mask & (N | W) == (N | W) && mask & (E | SE | S) == 0 {
        let ax = if mask & SW != 0 { x - 4 } else { x - 5 };
        let by = if mask & NE != 0 { y - 4 } else { y - 5 };
        return site(RampShape::CornerNw, (ax, y + 1), (x + 1, by));
    }

    // The straight shapes. Each nudges its two endpoints apart by one more cell
    // when the far cardinal is set, pulls both back one when the south bit is
    // set, then jitters each endpoint independently.
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
        let ax = x - 1 + jitter(rng);
        let bx = x - 1 + jitter(rng);
        return site(RampShape::ClearWest, (ax, ay), (bx, by));
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
        let ax = x + 1 + jitter(rng) - 1;
        let bx = x + 1 + jitter(rng) - 1;
        return site(RampShape::ClearEast, (ax, ay), (bx, by));
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
        let ay = y - 1 + jitter(rng);
        let by = y - 1 + jitter(rng);
        return site(RampShape::ClearNorth, (ax, ay), (bx, by));
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
        let ay = y + 1 + jitter(rng) - 1;
        let by = y + 1 + jitter(rng) - 1;
        return site(RampShape::ClearSouth, (ax, ay), (bx, by));
    }

    None
}

/// The late-attempt fallback shapes, tried in order once every ordinary shape
/// has failed and the attempt is past halfway.
///
/// Fixed geometry, no jitter, and each is still gated on a clear mask bit. The
/// first and third share the same guard bit in the original; that is not a
/// transcription slip.
/// SUPERSEDED with [`select_ramp_site`]; the live fallbacks are inline in
/// `carve::try_carve_connector_at_cell`.
#[cfg(test)]
pub(crate) fn fallback_ramp_sites(mask: u8, cell: (i32, i32)) -> Vec<RampSite> {
    let (x, y) = (cell.0, cell.1);
    let mut sites = Vec::new();
    if mask & S == 0 {
        sites.push(RampSite {
            shape: RampShape::ClearSouth,
            a: (x - 4, y + 1),
            b: (x + 4, y + 1),
        });
    }
    if mask & E == 0 {
        sites.push(RampSite {
            shape: RampShape::ClearEast,
            a: (x + 1, y + 4),
            b: (x + 1, y - 4),
        });
    }
    if mask & S == 0 {
        sites.push(RampSite {
            shape: RampShape::ClearNorth,
            a: (x + 4, y - 1),
            b: (x - 4, y - 1),
        });
    }
    if mask & W == 0 {
        sites.push(RampSite {
            shape: RampShape::ClearWest,
            a: (x - 1, y - 4),
            b: (x - 1, y + 4),
        });
    }
    sites
}

/// Is this attempt late enough for the fallback shapes?
/// SUPERSEDED with [`select_ramp_site`].
#[cfg(test)]
pub(crate) fn fallback_allowed(leniency: f32) -> bool {
    leniency > FALLBACK_LENIENCY
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
    fn the_fallback_opens_at_attempt_fiftyone() {
        // Attempt 50 must NOT qualify: 50 steps of the float nearest 0.01 land
        // just under a half, so a port using exact 0.01 would open one attempt
        // early and carve ramps the original never would.
        assert!(!fallback_allowed(50.0 * LENIENCY_STEP), "attempt 50");
        assert!(fallback_allowed(51.0 * LENIENCY_STEP), "attempt 51");
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
    fn a_thin_centre_window_rejects_the_cell_outright() {
        // Fewer than the bar in the centre window is a hard reject, distinct
        // from "mask came back empty".
        let mut s = scratch();
        paint(&mut s, (38, 48, 2, 2), 7); // 4 cells, bar is 15
        assert_eq!(ring_orientation_mask(&s, (40, 50), 7, 0.0), None);
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

    #[test]
    fn an_interior_cell_gets_no_ramp() {
        let mut rng = RmgRng::new(1);
        assert_eq!(select_ramp_site(0xFF, (40, 50), &mut rng), None);
    }

    #[test]
    fn a_corner_shape_takes_no_draws() {
        // Corner shapes must leave the stream untouched; only the straights
        // jitter. Getting this wrong desynchronises everything downstream.
        let mut rng = RmgRng::new(1);
        let site = select_ramp_site(N | E, (40, 50), &mut rng).expect("north-east corner");
        assert_eq!(site.shape, RampShape::CornerNe);
        assert_eq!(site.a, (39, 45));
        assert_eq!(site.b, (45, 51));
        let mut fresh = RmgRng::new(1);
        assert_eq!(rng.next_u32(), fresh.next_u32(), "corner drew nothing");
    }

    #[test]
    fn a_straight_shape_takes_exactly_two_draws() {
        let mut rng = RmgRng::new(1);
        let site = select_ramp_site(N, (40, 50), &mut rng).expect("west face clear");
        assert_eq!(site.shape, RampShape::ClearWest);
        // North set, south clear: endpoints spread to y-3 and y+5.
        assert_eq!(site.a.1, 47);
        assert_eq!(site.b.1, 55);
        // Both x endpoints sit at x-1 plus a 0-or-1 jitter.
        assert!((39..=40).contains(&site.a.0), "a.x jittered from 39");
        assert!((39..=40).contains(&site.b.0), "b.x jittered from 39");

        let mut probe = RmgRng::new(1);
        probe.next_u32();
        probe.next_u32();
        let mut after = rng;
        assert_eq!(after.next_u32(), probe.next_u32(), "exactly two draws");
    }

    #[test]
    fn the_corner_guards_are_tried_before_the_straights() {
        // A mask that satisfies both a corner and a straight must pick the
        // corner — the original's order, and the one that costs no draws.
        let mut rng = RmgRng::new(1);
        let site = select_ramp_site(N | W, (40, 50), &mut rng).expect("north-west corner");
        assert_eq!(site.shape, RampShape::CornerNw);
        let mut fresh = RmgRng::new(1);
        assert_eq!(rng.next_u32(), fresh.next_u32(), "no draws taken");
    }

    #[test]
    fn the_fallback_repeats_the_south_guard() {
        // Two of the four fallback shapes share the south bit. It reads like a
        // slip and is not one; a "tidied" port would emit a different set.
        let all_clear = fallback_ramp_sites(0, (40, 50));
        assert_eq!(all_clear.len(), 4, "every guard clear");
        let south_blocked = fallback_ramp_sites(S, (40, 50));
        assert_eq!(south_blocked.len(), 2, "the south bit removes two of them");
        assert_eq!(south_blocked[0].shape, RampShape::ClearEast);
        assert_eq!(south_blocked[1].shape, RampShape::ClearWest);
    }
}
