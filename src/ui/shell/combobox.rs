//! Shared ComboDropWin (0060E0C0) popup presentation and pointer capture.
//! The item's identity and selected value remain with each control's owner.

use super::geom::{RectPx, ShellScale};
use super::list::{ListMetrics, ListScrollInteraction, ListScrollPart, ShellListGeometry};
use crate::ui::skirmish_shell::{DropdownMetrics, ScrollModel, combo_arrow_hit_rect};
use crate::util::time::Instant;

/// What one mouse press does to an owner-draw combo or its open list.
///
/// `OwnerDraw_ComboBox @ 0x00617250` plays the open cue on any press of an
/// enabled face (`0x006184A2..0x006184BA`) and opens the list only from its
/// arrow zone ([`combo_arrow_hit_rect`], `0x006184BF..0x006184D7`). The open
/// list's drop window (`ComboDropWin @ 0x0060D540`) captures the mouse: its
/// scrollbar takes a press silently, and a pick or a press anywhere else
/// closes it with the close cue (`0x0060E4E9..0x0060E500`) and goes no further.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComboPress {
    /// The open list's scrollbar part under the press.
    Scroll(ListScrollPart),
    /// A row of the open list; the list closes.
    Pick(usize),
    /// Anywhere else while the list is open; the list closes.
    Dismiss,
    /// The face outside the arrow zone; only the open cue plays.
    Face,
    /// The arrow zone; the list opens.
    Open,
}

/// The two combo cues, `GUIComboOpenSound` and `GUIComboCloseSound`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComboCue {
    Open,
    Close,
}

impl ComboPress {
    /// A press on the collapsed face of an enabled combo whose control rect is
    /// `rect`.
    pub(crate) fn on_face(rect: RectPx, scale: ShellScale, x: i32, y: i32) -> Self {
        if combo_arrow_hit_rect(rect, scale).contains(x, y) {
            Self::Open
        } else {
            Self::Face
        }
    }

    pub(crate) fn cue(self) -> Option<ComboCue> {
        match self {
            Self::Scroll(_) => None,
            Self::Pick(_) | Self::Dismiss => Some(ComboCue::Close),
            Self::Face | Self::Open => Some(ComboCue::Open),
        }
    }
}

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
    /// The same list on rows `row_height` tall, for a control drawn smaller
    /// than the shell's own (VERA's roster).
    pub(crate) fn with_row_height(mut self, row_height: i32) -> Self {
        self.row_height = row_height.max(1);
        self.rect.h = self.visible_rows as i32 * self.row_height;
        self
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
                row_h: self.row_height,
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
        let content = self.content();
        Some(RectPx::new(
            content.x,
            content.y + (index - self.first) as i32 * self.row_height,
            content.w,
            self.row_height,
        ))
    }
    pub(crate) fn row_at(&self, x: i32, y: i32) -> Option<usize> {
        self.content()
            .contains(x, y)
            .then(|| self.first + ((y - self.rect.y) / self.row_height) as usize)
    }
    /// A press while this list is open: [`ComboPress::Scroll`],
    /// [`ComboPress::Pick`] or [`ComboPress::Dismiss`].
    pub(crate) fn press(&self, x: i32, y: i32) -> ComboPress {
        if let Some(part) = self.scroll_part_at(x, y) {
            return ComboPress::Scroll(part);
        }
        self.row_at(x, y)
            .map_or(ComboPress::Dismiss, ComboPress::Pick)
    }
    pub(crate) fn track_hot_row(&self, hot_row: &mut Option<usize>, x: i32, y: i32) {
        // 60E262 retains the previous hot row after leaving the row content.
        if let Some(row) = self.row_at(x, y) {
            *hot_row = Some(row);
        }
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
    /// A press while this interaction's list `popup` is open. The scrollbar
    /// takes it; a pick or a dismiss closes the list.
    pub(crate) fn press_list(
        &mut self,
        popup: ComboPopup,
        x: i32,
        y: i32,
        now: Instant,
    ) -> ComboPress {
        let press = popup.press(x, y);
        match press {
            ComboPress::Scroll(part) => {
                self.scroll
                    .press(part, popup.scroll_geometry(), &mut self.top_index, y, now)
            }
            _ => self.close(),
        }
        press
    }
    /// A press on the face of enabled combo `id`, whose control rect is
    /// `rect`; the arrow zone opens its list at `selected`.
    pub(crate) fn press_face(
        &mut self,
        id: Id,
        rect: RectPx,
        scale: ShellScale,
        selected: Option<usize>,
        x: i32,
        y: i32,
    ) -> ComboPress {
        let press = ComboPress::on_face(rect, scale, x, y);
        if press == ComboPress::Open {
            self.open(id, selected);
        }
        press
    }
    pub(crate) fn pointer_moved(&mut self, popup: ComboPopup, x: i32, y: i32) {
        popup.track_hot_row(&mut self.hot_row, x, y);
        self.scroll
            .pointer_moved(popup.scroll_geometry(), &mut self.top_index, x, y);
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    #[test]
    fn face_press_opens_only_right_of_client_right_minus_0x14() {
        // 0x006184CD..0x006184D7: `x > right - 0x14`, `right` exclusive.
        let rect = RectPx::new(10, 20, 120, 21);
        let right = rect.x + rect.w;
        let press = |x| ComboPress::on_face(rect, ShellScale::RETAIL, x, 25);
        assert_eq!(press(right - 0x14), ComboPress::Face);
        assert_eq!(press(right - 0x13), ComboPress::Open);
        assert_eq!(press(right - 1), ComboPress::Open);
        assert_eq!(press(right - 0x13).cue(), Some(ComboCue::Open));
        assert_eq!(press(rect.x).cue(), Some(ComboCue::Open));
    }

    #[test]
    fn open_list_press_picks_or_dismisses_and_closes_with_the_close_cue() {
        let popup = ComboPopup::new(RectPx::new(0, 0, 100, 21), ShellScale::RETAIL, 3, 0, 0);
        let row = popup.row_rect(1).unwrap();
        let mut combo = ComboPopupInteraction::default();
        combo.open(7u16, Some(0));
        let pick = combo.press_list(popup, row.x + 1, row.y + 1, Instant::now());
        assert_eq!((pick, combo.control), (ComboPress::Pick(1), None));
        combo.open(7, Some(0));
        let below = popup.rect.y + popup.rect.h + 5;
        let dismiss = combo.press_list(popup, row.x + 1, below, Instant::now());
        assert_eq!((dismiss, combo.control), (ComboPress::Dismiss, None));
        assert_eq!(pick.cue(), Some(ComboCue::Close));
        assert_eq!(dismiss.cue(), Some(ComboCue::Close));
    }
}
