//! Scrollbar geometry of the skirmish shell's combo dropdowns (and the
//! Options resolution list, which shares their drop-down list): the thumb
//! height, its position and the pointer→top-row mapping for a track click or
//! a thumb drag. The shell list boxes use `ui::shell::list` instead.
//!
//! Depends only on `RectPx` and the scrollbar constants from `layout`; holds no
//! state, render, or UI dependency (pure integer pixel geometry).

use super::layout::{
    COMBO_DROPDOWN_SCROLLBAR_BUTTON_H, COMBO_DROPDOWN_SCROLLBAR_MIN_THUMB_H, RectPx,
};

/// A combo dropdown's scroll model: rows visible at once are capped per
/// control (Side 7, Color/Start 9, AiType/Team unbounded). The thumb can be
/// dragged; the wheel does not move it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollModel {
    /// Rows shown at once; 0 is unbounded.
    cap: i32,
}

impl ScrollModel {
    /// `cap` is the per-combo max-visible (0 = unbounded).
    pub const fn combo(cap: i32) -> Self {
        Self { cap }
    }

    /// Visible-row count: `item_count.min(cap)`, or `item_count` unbounded.
    pub fn visible_rows(&self, item_count: usize) -> usize {
        if self.cap > 0 {
            item_count.min(self.cap as usize)
        } else {
            item_count
        }
    }

    /// `item_count − visible_rows`, saturating.
    pub fn max_top_index(&self, item_count: usize, visible_rows: usize) -> usize {
        item_count.saturating_sub(visible_rows)
    }

    /// Thumb height in pixels. `scrollbar_h` is the full track (scrollbar rect)
    /// height. An empty list takes the whole track (never reached: an empty
    /// combo shows no scrollbar).
    pub fn thumb_height(&self, visible_rows: usize, item_count: usize, scrollbar_h: i32) -> i32 {
        let track_h = (scrollbar_h - COMBO_DROPDOWN_SCROLLBAR_BUTTON_H * 2).max(1);
        if item_count == 0 || visible_rows == 0 {
            return track_h.max(COMBO_DROPDOWN_SCROLLBAR_MIN_THUMB_H);
        }
        ((track_h * visible_rows as i32) / item_count as i32)
            .max(COMBO_DROPDOWN_SCROLLBAR_MIN_THUMB_H)
            .min(track_h)
    }

    /// Thumb top-Y inside `scrollbar` for `top_index`. `thumb_h` from `thumb_height`;
    /// `max_top` from `max_top_index`.
    pub fn thumb_y(
        &self,
        scrollbar: RectPx,
        thumb_h: i32,
        top_index: usize,
        max_top: usize,
    ) -> i32 {
        let track_span = (scrollbar.h - COMBO_DROPDOWN_SCROLLBAR_BUTTON_H * 2 - thumb_h).max(1);
        scrollbar.y
            + COMBO_DROPDOWN_SCROLLBAR_BUTTON_H
            + if max_top == 0 {
                0
            } else {
                (track_span * top_index.min(max_top) as i32) / max_top as i32
            }
    }

    /// Pointer→top_index — the shared core for BOTH a track click and a thumb drag.
    /// `thumb_top_candidate` is `mouse_y − thumb_h/2` for a track click, or
    /// `mouse_y − grab_offset_y` for a drag.
    pub fn top_index_from_thumb_top(
        &self,
        scrollbar: RectPx,
        thumb_h: i32,
        max_top: usize,
        thumb_top_candidate: i32,
    ) -> usize {
        if max_top == 0 {
            return 0;
        }
        let track_span = (scrollbar.h - COMBO_DROPDOWN_SCROLLBAR_BUTTON_H * 2 - thumb_h).max(1);
        let thumb_top = thumb_top_candidate.clamp(
            scrollbar.y + COMBO_DROPDOWN_SCROLLBAR_BUTTON_H,
            scrollbar.y + scrollbar.h - COMBO_DROPDOWN_SCROLLBAR_BUTTON_H - thumb_h,
        );
        let local = thumb_top - scrollbar.y - COMBO_DROPDOWN_SCROLLBAR_BUTTON_H;
        ((local * max_top as i32 + track_span / 2) / track_span) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::super::layout::{
        COMBO_DROPDOWN_SCROLLBAR_BUTTON_H as BUTTON_H,
        COMBO_DROPDOWN_SCROLLBAR_MIN_THUMB_H as MIN_THUMB_H,
    };
    use super::*;

    // ---- Verbatim reference copies of the pre-4E legacy math (the executable spec).
    //      These NEVER change; the unified primitive is proven equal to them. ----

    /// Model A: `combo_dropdown_thumb_height` (combos.rs pre-4E).
    fn legacy_combo_thumb_height(visible_rows: usize, item_count: usize, scrollbar_h: i32) -> i32 {
        let track_h = (scrollbar_h - BUTTON_H * 2).max(1);
        if item_count == 0 {
            return track_h.max(MIN_THUMB_H);
        }
        ((track_h * visible_rows as i32) / item_count as i32)
            .max(MIN_THUMB_H)
            .min(track_h)
    }

    /// Model A `thumb_y` (combo_dropdown_scroll_thumb_rect pre-4E).
    fn legacy_combo_thumb_y(
        scrollbar: RectPx,
        thumb_h: i32,
        top_index: usize,
        max_top: usize,
    ) -> i32 {
        let track_span = (scrollbar.h - BUTTON_H * 2 - thumb_h).max(1);
        scrollbar.y
            + BUTTON_H
            + if max_top == 0 {
                0
            } else {
                (track_span * top_index.min(max_top) as i32) / max_top as i32
            }
    }

    /// Shared pointer→top_index core (identical in A track-click, A drag, B track-click pre-4E).
    fn legacy_pointer_to_top(
        scrollbar: RectPx,
        thumb_h: i32,
        max_top: usize,
        candidate: i32,
    ) -> usize {
        if max_top == 0 {
            return 0;
        }
        let track_span = (scrollbar.h - BUTTON_H * 2 - thumb_h).max(1);
        let thumb_top = candidate.clamp(
            scrollbar.y + BUTTON_H,
            scrollbar.y + scrollbar.h - BUTTON_H - thumb_h,
        );
        let local = thumb_top - scrollbar.y - BUTTON_H;
        ((local * max_top as i32 + track_span / 2) / track_span) as usize
    }

    /// Representative scrollbars incl. degenerate `track_h`-clamp geometries.
    fn scrollbars() -> Vec<RectPx> {
        vec![
            RectPx::new(100, 50, 20, 23 * 7), // a Side combo dropdown (cap 7, row 23)
            RectPx::new(100, 50, 20, 23 * 9), // Color/Start dropdown (cap 9)
            RectPx::new(0, 0, 20, 44),      // degenerate: scrollbar.h - 44 == 0  -> track_h clamp
            RectPx::new(0, 0, 20, 45),      // degenerate: track_h == 1
            RectPx::new(0, 0, 20, 46),
        ]
    }

    const N: usize = 24; // boundary count ceiling; bump if a stock combo can exceed it

    #[test]
    fn unbounded_combo_never_needs_a_scrollbar() {
        // PerControlCap(0): visible_rows == item_count => item > visible is always false.
        let m = ScrollModel::combo(0);
        for n in 0..=N {
            assert_eq!(m.visible_rows(n), n);
        }
    }

    #[test]
    fn unified_matches_combo_model_over_boundaries() {
        for &cap in &[7i32, 9] {
            let model = ScrollModel::combo(cap);
            for sb in scrollbars() {
                // item_count==0 is unreachable under the gate (see reachability test); start at 1.
                for item_count in 1..=N {
                    let visible_rows = model.visible_rows(item_count);
                    if visible_rows == 0 {
                        continue; // unreachable for a combo (visible==0 <=> item==0)
                    }
                    if item_count <= visible_rows {
                        continue; // no scrollbar => thumb never built
                    }
                    let thumb_h = model.thumb_height(visible_rows, item_count, sb.h);
                    assert_eq!(
                        thumb_h,
                        legacy_combo_thumb_height(visible_rows, item_count, sb.h),
                        "thumb_h cap={cap} sb={sb:?} n={item_count}"
                    );
                    let max_top = model.max_top_index(item_count, visible_rows);
                    assert_eq!(max_top, item_count.saturating_sub(visible_rows));
                    for top_index in 0..=max_top {
                        assert_eq!(
                            model.thumb_y(sb, thumb_h, top_index, max_top),
                            legacy_combo_thumb_y(sb, thumb_h, top_index, max_top),
                            "thumb_y cap={cap} sb={sb:?} n={item_count} top={top_index}"
                        );
                    }
                    // The pointer->index clamp requires lo <= hi
                    // (scrollbar.h >= 2*BUTTON_H + thumb_h). Real combo scrollbars are
                    // >=161px so always satisfy it; the degenerate track_h-clamp
                    // fixtures do not, and both unified + legacy clamp identically
                    // (they would panic identically), so only compare where it is
                    // well-formed.
                    if sb.h >= 2 * BUTTON_H + thumb_h {
                        for my in (sb.y - 5)..=(sb.y + sb.h + 5) {
                            // track click anchor:
                            let track_anchor = my - thumb_h / 2;
                            assert_eq!(
                                model.top_index_from_thumb_top(sb, thumb_h, max_top, track_anchor),
                                legacy_pointer_to_top(sb, thumb_h, max_top, track_anchor),
                                "track cap={cap} sb={sb:?} n={item_count} my={my}"
                            );
                            // drag anchor (combo-only) — grab offset of 3px from thumb top:
                            let drag_anchor = my - 3;
                            assert_eq!(
                                model.top_index_from_thumb_top(sb, thumb_h, max_top, drag_anchor),
                                legacy_pointer_to_top(sb, thumb_h, max_top, drag_anchor),
                                "drag cap={cap} sb={sb:?} n={item_count} my={my}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn an_empty_combo_shows_no_scrollbar_and_its_thumb_would_fill_the_track() {
        // visible_rows == 0 <=> item_count == 0, so `item > visible` never opens
        // the scrollbar for an empty combo; the thumb branch is pinned anyway.
        let combo = ScrollModel::combo(7);
        assert_eq!(combo.visible_rows(0), 0);
        let track_h = (100 - BUTTON_H * 2).max(1);
        assert_eq!(combo.thumb_height(0, 0, 100), track_h.max(MIN_THUMB_H));
    }
}
