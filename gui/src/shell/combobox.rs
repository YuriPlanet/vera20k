//! Shared ComboDropWin (0060E0C0) popup presentation and pointer capture.
//! The item's identity and selected value remain with each control's owner.

use super::geom::{RectPx, ShellScale};
use super::list::{ListMetrics, ListScrollPart, ShellListGeometry};

/// Default combo metrics, measured from gamemd's `ComboDropWin` and owner-draw
/// combo: the face height, the drop-down's row height (its scrollbar is the
/// list's, [`ListMetrics::combo`]), and the arrow's reserve at the face's right.
pub const COMBO_FACE_H: i32 = 24;
pub const COMBO_DROPDOWN_ROW_H: i32 = 23;
pub const COMBO_ARROW_RESERVE_W: i32 = 20;

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
        let row_height = ListMetrics::combo(scale).row_h;
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
        self.panes().1
    }
    /// The shared scrollbar child's metrics on this list's rows.
    fn metrics(&self) -> ListMetrics {
        ListMetrics {
            row_h: self.row_height,
            ..ListMetrics::combo(self.scale)
        }
    }
    /// The row content and scrollbar: the list's, without its frame inset.
    fn panes(&self) -> (RectPx, Option<RectPx>) {
        let scrolls = self.total > self.visible_rows;
        ShellListGeometry::panes(self.rect, 0, scrolls, self.metrics())
    }
    pub fn thumb(&self) -> Option<RectPx> {
        self.scroll_geometry().thumb
    }
    pub fn scroll_geometry(&self) -> ShellListGeometry {
        ShellListGeometry::unframed(
            self.rect,
            self.total,
            self.visible_rows,
            self.first,
            self.metrics(),
        )
    }
    pub fn scroll_part_at(&self, x: i32, y: i32) -> Option<ListScrollPart> {
        self.scroll_geometry().scroll_part_at(x, y)
    }
    pub fn content(&self) -> RectPx {
        self.panes().0
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
    pub(crate) fn track_hot_row(&self, hot_row: &mut Option<usize>, x: i32, y: i32) {
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
