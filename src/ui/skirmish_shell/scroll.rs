//! Scrollbar geometry of the skirmish shell's combo dropdowns (and the
//! Options resolution list, which shares their drop-down list): the thumb
//! height, its position and the pointer→top-row mapping for a track click or
//! a thumb drag. The shell list boxes use `ui::shell::list` instead.
//!
//! Depends only on `RectPx` and the scrollbar constants from `layout`; holds no
//! state, render, or UI dependency (pure integer pixel geometry).

use super::layout::{
    COMBO_DROPDOWN_SCROLLBAR_BUTTON_H, COMBO_DROPDOWN_SCROLLBAR_MIN_THUMB_H, DropdownMetrics,
    RectPx,
};

/// A combo dropdown's scroll model: rows visible at once are capped per
/// control (Side 7, Color/Start 9, AiType/Team unbounded). The thumb can be
/// dragged; the wheel does not move it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollModel {
    /// Rows shown at once; 0 is unbounded.
    cap: i32,
    /// The scrollbar's arrow-button height in the pixels the track is measured
    /// in: authored on the retail layout, stretched on a stretched page.
    button_h: i32,
    min_thumb_h: i32,
}

impl ScrollModel {
    /// `cap` is the per-combo max-visible (0 = unbounded), on the authored
    /// metrics.
    pub const fn combo(cap: i32) -> Self {
        Self {
            cap,
            button_h: COMBO_DROPDOWN_SCROLLBAR_BUTTON_H,
            min_thumb_h: COMBO_DROPDOWN_SCROLLBAR_MIN_THUMB_H,
        }
    }

    /// The same model on a stretched dropdown's device metrics.
    pub const fn combo_at(cap: i32, metrics: DropdownMetrics) -> Self {
        Self {
            cap,
            button_h: metrics.button_h,
            min_thumb_h: metrics.min_thumb_h,
        }
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

    /// ComboDropWin 0060E72F creates Scrollbar and 0060E745 subclasses it
    /// to the shared 0061C690 owner. SBM_SETSCROLLINFO at 0060E7BA sets
    /// nMax=count-visible and no page size; use the same native numeric port.
    fn metrics(&self) -> crate::ui::shell::list::ListMetrics {
        crate::ui::shell::list::ListMetrics {
            arrow_h: self.button_h,
            min_thumb_h: self.min_thumb_h,
            ..crate::ui::shell::list::ListMetrics::RETAIL
        }
    }

    pub fn thumb_height(&self, visible_rows: usize, item_count: usize, scrollbar_h: i32) -> i32 {
        crate::ui::shell::list::thumb_height(
            scrollbar_h,
            item_count.saturating_sub(visible_rows),
            self.metrics(),
        )
    }

    pub fn thumb_y(
        &self,
        scrollbar: RectPx,
        thumb_h: i32,
        top_index: usize,
        max_top: usize,
    ) -> i32 {
        crate::ui::shell::list::scrollbar_thumb_y(
            scrollbar,
            thumb_h,
            top_index,
            max_top,
            self.metrics(),
        )
    }

    /// Track press and active thumb drag both center on the pointer. Native
    /// does not retain a grab offset and does not round to the nearest row.
    pub fn top_index_at_pointer(
        &self,
        scrollbar: RectPx,
        thumb_h: i32,
        max_top: usize,
        pointer_y: i32,
    ) -> usize {
        crate::ui::shell::list::scrollbar_top_at_pointer(
            scrollbar,
            thumb_h,
            max_top,
            pointer_y,
            self.metrics(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combo_thumb_uses_the_original_scrollbar_kernel() {
        let vectors: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/saved_scrollbar.json",
        ))
        .unwrap();
        for case in vectors["cases"].as_array().unwrap() {
            let range = case["range"].as_u64().unwrap() as usize;
            for visible in [5, 7, 9] {
                assert_eq!(
                    ScrollModel::combo(visible as i32).thumb_height(
                        visible,
                        visible + range,
                        case["height"].as_i64().unwrap() as i32,
                    ),
                    case["thumb"].as_i64().unwrap() as i32,
                );
            }
        }
    }
}
