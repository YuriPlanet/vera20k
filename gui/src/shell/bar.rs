//! A `Bar` node's one meaning, the connection dialog's bar (0x0064A7B0):
//! a one-pixel outline in its `Background` ([`OUTLINE_RGB`] without one),
//! then just over it the fill in its `Color`, [`fill_rect`] for its `Fill`
//! percent ([`FULL`] without one). The HUD's paint list and the host's
//! dialog painter both draw it from these.

use super::geom::RectPx;

/// The outline colour `[0x00A8EAE4]`, set once by the static initializer
/// 0x004E8180.
pub const OUTLINE_RGB: [u8; 3] = [255, 0, 0];

/// The `Fill` of a bar that has none.
pub const FULL: i32 = 100;

/// The fill in the bar `rect` for `percent` (held to 0..=100) of its
/// width, at least 6 pixels, inset by 2.
pub fn fill_rect(rect: RectPx, percent: i32) -> RectPx {
    // Widened: a client's bar may be as wide as a rect can be.
    let width = (i64::from(rect.w) * i64::from(percent.clamp(0, 100)) / 100) as i32;
    let width = if width > 5 { width } else { 6 };
    RectPx::new(
        rect.x.saturating_add(2),
        rect.y.saturating_add(2),
        width - 4,
        rect.h.saturating_sub(4),
    )
}

/// A one-pixel outline's edges on `rect`: top, bottom, left, right. None
/// for an empty rect.
pub fn outline_edges(rect: RectPx) -> Option<[RectPx; 4]> {
    if rect.w <= 0 || rect.h <= 0 {
        return None;
    }
    Some([
        RectPx::new(rect.x, rect.y, rect.w, 1),
        RectPx::new(rect.x, rect.y + rect.h - 1, rect.w, 1),
        RectPx::new(rect.x, rect.y, 1, rect.h),
        RectPx::new(rect.x + rect.w - 1, rect.y, 1, rect.h),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fill_keeps_six_pixels_and_insets_by_two() {
        let bar = RectPx::new(10, 20, 100, 12);
        assert_eq!(fill_rect(bar, FULL), RectPx::new(12, 22, 96, 8));
        assert_eq!(fill_rect(bar, 0), RectPx::new(12, 22, 2, 8));
        assert_eq!(fill_rect(bar, -5), fill_rect(bar, 0));
        assert_eq!(fill_rect(bar, i32::MAX), fill_rect(bar, FULL));
    }

    #[test]
    fn an_empty_rect_has_no_outline() {
        assert_eq!(outline_edges(RectPx::new(0, 0, 0, 4)), None);
        let edges = outline_edges(RectPx::new(1, 2, 3, 4)).unwrap();
        assert_eq!(edges[1], RectPx::new(1, 5, 3, 1));
        assert_eq!(edges[3], RectPx::new(3, 2, 1, 4));
    }
}
