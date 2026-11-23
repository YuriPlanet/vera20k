//! Shared ComboDropWin (0060E0C0) popup presentation and pointer capture.
//! The item's identity and selected value remain with each control's owner.

use super::geom::{RectPx, ShellScale};
use super::list::{
    ListMetrics, ListScrollPart, ShellListGeometry, scrollbar_thumb_y, thumb_height,
};

/// Default combo metrics, measured from gamemd's `ComboDropWin` and owner-draw
/// combo: the face height, the drop-down's row height, scrollbar width,
/// scrollbar button height and minimum thumb height, and the arrow's reserve
/// at the face's right.
pub const COMBO_FACE_H: i32 = 24;
pub const COMBO_DROPDOWN_ROW_H: i32 = 23;
pub const COMBO_DROPDOWN_SCROLLBAR_W: i32 = 20;
pub const COMBO_DROPDOWN_SCROLLBAR_BUTTON_H: i32 = 22;
pub const COMBO_DROPDOWN_SCROLLBAR_MIN_THUMB_H: i32 = 14;
pub const COMBO_ARROW_RESERVE_W: i32 = 20;

/// The `ComboDropWin` dropdown's device metrics: the authored row, scrollbar and
/// thumb sizes grown by the page's stretch, so a dropdown under a stretched
/// combo has rows as tall as its face and stays the same number of rows deep.
/// Identity on an unscaled mapping, where every field is the default above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropdownMetrics {
    pub row_h: i32,
    pub scrollbar_w: i32,
    pub button_h: i32,
    pub min_thumb_h: i32,
}

impl DropdownMetrics {
    pub fn scaled(scale: ShellScale) -> Self {
        let grow = |px: i32, factor: f32| ((px as f32 * factor).round() as i32).max(1);
        Self {
            row_h: grow(COMBO_DROPDOWN_ROW_H, scale.factor()),
            scrollbar_w: grow(COMBO_DROPDOWN_SCROLLBAR_W, scale.factor()),
            button_h: grow(COMBO_DROPDOWN_SCROLLBAR_BUTTON_H, scale.factor()),
            min_thumb_h: grow(COMBO_DROPDOWN_SCROLLBAR_MIN_THUMB_H, scale.factor()),
        }
    }
}

/// The painted face of a combo whose control rect is `rect`: the control's
/// width, [`COMBO_FACE_H`] tall.
pub fn combo_face_rect(rect: RectPx, scale: ShellScale) -> RectPx {
    scale.derive_rect(rect, |rect| {
        RectPx::new(rect.x, rect.y, rect.w, COMBO_FACE_H)
    })
}

/// Clickable arrow area of OwnerDraw_ComboBox 00617250: `x > right - 0x14`
/// against its GetClientRect `right` (`0x006172E7`, `0x006184BF..0x006184D7`),
/// which is exclusive, so the zone is the face's last 19 columns.
/// The engine's `combo_arrow_rect` is only a zero-size sprite anchor.
pub fn combo_arrow_hit_rect(rect: RectPx, scale: ShellScale) -> RectPx {
    let face = combo_face_rect(rect, scale);
    scale.derive_rect(face, |face| {
        RectPx::new(
            face.x + face.w - COMBO_ARROW_RESERVE_W + 1,
            face.y,
            COMBO_ARROW_RESERVE_W - 1,
            face.h,
        )
    })
}

#[derive(Debug, Clone, Copy)]
pub struct ComboPopup {
    pub rect: RectPx,
    pub first: usize,
    pub visible_rows: usize,
    pub total: usize,
    pub row_height: i32,
    scale: ShellScale,
}
impl ComboPopup {
    pub fn new(face: RectPx, scale: ShellScale, total: usize, cap: usize, first: usize) -> Self {
        let visible_rows = if cap == 0 { total } else { total.min(cap) };
        let row_height = DropdownMetrics::scaled(scale).row_h;
        Self {
            rect: RectPx::new(
                face.x,
                face.y + face.h + 1,
                face.w,
                visible_rows as i32 * row_height,
            ),
            first: first.min(total.saturating_sub(visible_rows)),
            visible_rows,
            total,
            row_height,
            scale,
        }
    }
    /// The same list on rows `row_height` tall, for a control drawn smaller
    /// than the default.
    pub fn with_row_height(mut self, row_height: i32) -> Self {
        self.row_height = row_height.max(1);
        self.rect.h = self.visible_rows as i32 * self.row_height;
        self
    }
    pub fn scrollbar(&self) -> Option<RectPx> {
        (self.total > self.visible_rows).then(|| {
            let width = DropdownMetrics::scaled(self.scale).scrollbar_w;
            RectPx::new(
                self.rect.x + self.rect.w - width,
                self.rect.y,
                width,
                self.rect.h,
            )
        })
    }
    /// The shared scrollbar child's metrics (ComboDropWin 0060E72F creates
    /// it, 0060E745 subclasses it to 0061C690).
    fn metrics(&self) -> ListMetrics {
        let metrics = DropdownMetrics::scaled(self.scale);
        ListMetrics {
            row_h: self.row_height,
            scrollbar_w: metrics.scrollbar_w,
            arrow_h: metrics.button_h,
            min_thumb_h: metrics.min_thumb_h,
        }
    }
    pub fn thumb(&self) -> Option<RectPx> {
        let bar = self.scrollbar()?;
        let (metrics, max_top) = (self.metrics(), self.total - self.visible_rows);
        let height = thumb_height(bar.h, max_top, metrics);
        let y = scrollbar_thumb_y(bar, height, self.first, max_top, metrics);
        Some(RectPx::new(bar.x + 1, y, bar.w - 2, height))
    }
    pub fn scroll_geometry(&self) -> ShellListGeometry {
        ShellListGeometry {
            outer: self.rect,
            content: self.content(),
            scrollbar: self.scrollbar(),
            thumb: self.thumb(),
            visible_rows: self.visible_rows,
            max_top: self.total - self.visible_rows,
            metrics: self.metrics(),
        }
    }
    pub fn scroll_part_at(&self, x: i32, y: i32) -> Option<ListScrollPart> {
        self.scroll_geometry().scroll_part_at(x, y)
    }
    pub fn content(&self) -> RectPx {
        RectPx::new(
            self.rect.x,
            self.rect.y,
            self.rect.w - self.scrollbar().map_or(0, |bar| bar.w),
            self.rect.h,
        )
    }
    pub fn row_rect(&self, index: usize) -> Option<RectPx> {
        if index < self.first || index >= self.first + self.visible_rows {
            return None;
        }
        let content = self.content();
        Some(RectPx::new(
            content.x,
            content.y + (index - self.first) as i32 * self.row_height,
            content.w,
            self.row_height,
        ))
    }
    pub fn row_at(&self, x: i32, y: i32) -> Option<usize> {
        self.content()
            .contains(x, y)
            .then(|| self.first + ((y - self.rect.y) / self.row_height) as usize)
            .filter(|row| *row < self.first + self.visible_rows)
    }
    pub fn track_hot_row(&self, hot_row: &mut Option<usize>, x: i32, y: i32) {
        // 60E262 retains the previous hot row after leaving the row content.
        if let Some(row) = self.row_at(x, y) {
            *hot_row = Some(row);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_press_opens_only_right_of_client_right_minus_0x14() {
        // 0x006184CD..0x006184D7: `x > right - 0x14`, `right` exclusive.
        let rect = RectPx::new(10, 20, 120, 21);
        let right = rect.x + rect.w;
        let opens = |x| combo_arrow_hit_rect(rect, ShellScale::unscaled(800, 600)).contains(x, 25);
        assert!(!opens(right - 0x14));
        assert!(opens(right - 0x13));
        assert!(opens(right - 1));
    }
}
