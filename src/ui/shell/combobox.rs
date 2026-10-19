//! Shared ComboDropWin (0060E0C0) popup presentation and pointer capture.
//! The item's identity and selected value remain with each control's owner.

use super::geom::{RectPx, ShellScale};
use super::list::{ListMetrics, ListScrollInteraction, ListScrollPart, ShellListGeometry};
use crate::ui::skirmish_shell::{DropdownMetrics, ScrollModel, combo_dropdown_row_rect};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ComboPopup {
    pub(crate) rect: RectPx,
    pub(crate) first: usize,
    pub(crate) visible_rows: usize,
    pub(crate) total: usize,
    pub(crate) row_height: i32,
    scale: ShellScale,
}
impl ComboPopup {
    pub(crate) fn new(
        face: RectPx,
        scale: ShellScale,
        total: usize,
        cap: usize,
        first: usize,
    ) -> Self {
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
    pub(crate) fn scrollbar(&self) -> Option<RectPx> {
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
    pub(crate) fn thumb(&self) -> Option<RectPx> {
        let bar = self.scrollbar()?;
        let model = ScrollModel::combo_at(
            self.visible_rows as i32,
            DropdownMetrics::scaled(self.scale),
        );
        let height = model.thumb_height(self.visible_rows, self.total, bar.h);
        let y = model.thumb_y(bar, height, self.first, self.total - self.visible_rows);
        Some(RectPx::new(bar.x + 1, y, bar.w - 2, height))
    }
    pub(crate) fn scroll_geometry(&self) -> ShellListGeometry {
        let metrics = DropdownMetrics::scaled(self.scale);
        ShellListGeometry {
            outer: self.rect,
            content: self.content(),
            scrollbar: self.scrollbar(),
            thumb: self.thumb(),
            visible_rows: self.visible_rows,
            max_top: self.total - self.visible_rows,
            metrics: ListMetrics {
                row_h: metrics.row_h,
                scrollbar_w: metrics.scrollbar_w,
                arrow_h: metrics.button_h,
                min_thumb_h: metrics.min_thumb_h,
            },
        }
    }
    pub(crate) fn scroll_part_at(&self, x: i32, y: i32) -> Option<ListScrollPart> {
        self.scroll_geometry().scroll_part_at(x, y)
    }
    pub(crate) fn content(&self) -> RectPx {
        RectPx::new(
            self.rect.x,
            self.rect.y,
            self.rect.w - self.scrollbar().map_or(0, |bar| bar.w),
            self.rect.h,
        )
    }
    pub(crate) fn row_rect(&self, index: usize) -> Option<RectPx> {
        if index < self.first || index >= self.first + self.visible_rows {
            return None;
        }
        Some(combo_dropdown_row_rect(
            self.content(),
            self.scale,
            index - self.first,
        ))
    }
    pub(crate) fn row_at(&self, x: i32, y: i32) -> Option<usize> {
        self.content()
            .contains(x, y)
            .then(|| self.first + ((y - self.rect.y) / self.row_height) as usize)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ComboPopupInteraction<Id> {
    pub(crate) control: Option<Id>,
    pub(crate) top_index: usize,
    pub(crate) hot_row: Option<usize>,
    pub(crate) scroll: ListScrollInteraction,
}
impl<Id> Default for ComboPopupInteraction<Id> {
    fn default() -> Self {
        Self {
            control: None,
            top_index: 0,
            hot_row: None,
            scroll: ListScrollInteraction::default(),
        }
    }
}
impl<Id: Copy> ComboPopupInteraction<Id> {
    /// 6181DA initializes top0; the current selection supplies the initial hot
    /// row, even when that row lies below the first visible page.
    pub(crate) fn open(&mut self, id: Id, selected: Option<usize>) {
        self.control = Some(id);
        self.top_index = 0;
        self.hot_row = selected;
        self.scroll.cancel();
    }
    pub(crate) fn close(&mut self) {
        self.control = None;
        self.scroll.cancel();
    }
    pub(crate) fn cancel_pointer(&mut self) {
        self.scroll.cancel();
    }
    pub(crate) fn pointer_moved(&mut self, popup: ComboPopup, x: i32, y: i32) {
        // 60E262 retains the previous hot row after leaving the row content.
        if let Some(row) = popup.row_at(x, y) {
            self.hot_row = Some(row);
        }
        self.scroll
            .pointer_moved(popup.scroll_geometry(), &mut self.top_index, x, y);
    }
}
