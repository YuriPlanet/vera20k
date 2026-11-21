//! Shell dialog layout pass.
//!
//! Implements contract C7: convert each control's resource rect from DLUs to
//! pixels once, then re-anchor it by its per-control `AnchorRule` for include-set
//! dialogs. Modal-centered dialogs skip the re-anchor (the caller centers them).
//! Render-agnostic; consumes only the descriptor + shared geometry primitives.

use super::descriptor::AnchorRule;
use super::geom::{self, RectPx, RightPanelRects};

/// Centering base width — the logical shell is authored at 800x600 and
/// horizontally compensated on wider screens. Matches the per-shell helpers.
const SHELL_BASE_W: i32 = 800;
/// Centering base height — the logical shell is authored at 800x600.
const SHELL_BASE_H: i32 = 600;

/// Status line `0x695` of the right-panel shells outside a suspended game
/// (`0x0060B550`): the window at `x = dx + 10`, `y = H - h - dy - 1`, where
/// `dx`/`dy` are the non-negative halves of the screen beyond 800x600. The
/// runtime window carries the same one-pixel size correction as the heading
/// and the monitor (456x21 for the 303x12 DLU template at 800x600, so the
/// retail text ink starts at y 581).
pub fn status_line_rect(dlu: RectPx, screen_w: i32, screen_h: i32) -> RectPx {
    let converted = geom::dlu_rect(dlu.x, dlu.y, dlu.w, dlu.h);
    let (w, h) = (converted.w + 1, converted.h + 1);
    let dx = geom::center_offset(screen_w, SHELL_BASE_W);
    let dy = geom::center_offset(screen_h, SHELL_BASE_H);
    RectPx::new(dx + 10, screen_h - h - dy - 1, w, h)
}

/// The status line's text box, VERA's own: the window above
/// ([`status_line_rect`], 456x21 at 800x600 on most pages) runs on to x 628,
/// the right end of the bar's dark interior, at the window's scale, as
/// translations run longer than the English it was sized for. A window that
/// already reaches there, such as `0x102`'s, keeps its width. Retail texts
/// fit the window and draw as before.
pub fn status_text_rect(window: RectPx) -> RectPx {
    // The window's height and the text box's width at 800x600.
    const WINDOW_H: i32 = 21;
    const TEXT_W: i32 = 618;
    RectPx {
        w: window.w.max(TEXT_W * window.h / WINDOW_H),
        ..window
    }
}

/// Places a dialog's controls on one screen by their resource template and
/// re-anchor rule. On a scaled mapping the
/// controls are placed on the 800x600 dialog and scaled with it, so a page's
/// retail and window-filling layouts come from one table.
#[derive(Debug, Clone, Copy)]
pub struct Placer {
    screen_w: i32,
    screen_h: i32,
    panel: RightPanelRects,
    scale: geom::ShellScale,
}

impl Placer {
    /// The retail placement on a `screen_w`x`screen_h` screen.
    pub fn new(screen_w: i32, screen_h: i32) -> Self {
        Self::on(geom::RETAIL_SCALE, screen_w, screen_h)
    }

    /// The placement on `scale`'s mapping: the retail screen for the retail
    /// mapping, the scaled 800x600 dialog otherwise.
    pub fn on(scale: geom::ShellScale, screen_w: i32, screen_h: i32) -> Self {
        let (screen_w, screen_h) = if scale.is_unscaled() {
            (screen_w, screen_h)
        } else {
            (SHELL_BASE_W, SHELL_BASE_H)
        };
        Self {
            screen_w,
            screen_h,
            panel: geom::right_panel_rects(screen_w, screen_h),
            scale,
        }
    }

    /// The control with resource template `(x, y, w, h)` DLU under `rule`.
    pub fn at(&self, rule: AnchorRule, x: i32, y: i32, w: i32, h: i32) -> RectPx {
        let placed = apply_anchor(
            rule,
            RectPx::new(x, y, w, h),
            self.screen_w,
            self.screen_h,
            self.panel,
        );
        self.scale.scale_rect(placed)
    }

    /// A control whose rule reads no resource template (the column's final
    /// tile, [`AnchorRule::ButtonColumnLast`]).
    pub fn rule(&self, rule: AnchorRule) -> RectPx {
        self.at(rule, 0, 0, 0, 0)
    }

    /// An already-placed 800x600 pixel rect `(x, y, w, h)` on this mapping.
    pub fn px(&self, x: i32, y: i32, w: i32, h: i32) -> RectPx {
        self.scale.scale_rect(RectPx::new(x, y, w, h))
    }
}

/// A dialog's placed controls: each control's pixel rect by resource id, in
/// table order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RectTable(Vec<(u16, RectPx)>);

impl RectTable {
    /// Control `id`'s rect.
    pub fn get(&self, id: u16) -> Option<RectPx> {
        self.0.iter().find(|(c, _)| *c == id).map(|(_, rect)| *rect)
    }

    /// Puts control `id` on `rect`, replacing its earlier rect.
    pub fn insert(&mut self, id: u16, rect: RectPx) {
        match self.0.iter_mut().find(|(c, _)| *c == id) {
            Some(entry) => entry.1 = rect,
            None => self.0.push((id, rect)),
        }
    }

    /// Drops control `id`, which then shows nowhere.
    pub fn remove(&mut self, id: u16) {
        self.0.retain(|(c, _)| *c != id);
    }

    pub fn iter(&self) -> impl Iterator<Item = (u16, RectPx)> + '_ {
        self.0.iter().copied()
    }
}

impl FromIterator<(u16, RectPx)> for RectTable {
    fn from_iter<I: IntoIterator<Item = (u16, RectPx)>>(iter: I) -> Self {
        let mut table = Self::default();
        for (id, rect) in iter {
            table.insert(id, rect);
        }
        table
    }
}

impl geom::ScaledRects for RectTable {
    fn scaled_rects(&self, scale: geom::ShellScale) -> Self {
        Self(
            self.0
                .iter()
                .map(|(id, rect)| (*id, scale.scale_rect(*rect)))
                .collect(),
        )
    }
}

/// Resolve one control's pixel rect by re-anchor rule. `dlu` is the raw DLU
/// resource rect; the snapped owner-draw rule consumes only `dlu.y`, while the
/// bottom-row rule is derived entirely from the resolved panel geometry.
fn apply_anchor(
    rule: AnchorRule,
    dlu: RectPx,
    screen_w: i32,
    screen_h: i32,
    panel: RightPanelRects,
) -> RectPx {
    match rule {
        AnchorRule::OwnerDrawButtonSnap { cell_w } => {
            geom::snap_button_round_half_up(dlu.y, panel, cell_w)
        }
        AnchorRule::OwnerDrawButtonBottomRow { cell_w } => {
            let y = panel.bottom.y - geom::RIGHT_PANEL_TILE_H;
            let x = panel.top.x + (geom::RIGHT_PANEL_WIDTH - cell_w);
            RectPx::new(x, y, cell_w, geom::SDBTNANM_CELL_H)
        }
        AnchorRule::Resource => geom::dlu_rect(dlu.x, dlu.y, dlu.w, dlu.h),
        AnchorRule::ChildWindow => geom::child_window(dlu.x, dlu.y, dlu.w, dlu.h),
        AnchorRule::ButtonColumn { cell_w } => geom::snap_button_biased_truncate(
            screen_w,
            screen_h,
            geom::dlu_rect(dlu.x, dlu.y, dlu.w, dlu.h),
            panel,
            cell_w,
        ),
        AnchorRule::ButtonColumnLast { cell_w } => RectPx::new(
            screen_w - geom::center_offset(screen_w, SHELL_BASE_W) - cell_w,
            panel.tile.y + (panel.tile_count - 1).max(0) * geom::RIGHT_PANEL_TILE_H,
            cell_w,
            geom::SDBTNANM_CELL_H,
        ),
        AnchorRule::StatusLine => status_line_rect(dlu, screen_w, screen_h),
        AnchorRule::RightAnchor => right_anchor(
            screen_w,
            screen_h,
            geom::dlu_rect(dlu.x, dlu.y, dlu.w, dlu.h),
            None,
        ),
        AnchorRule::RightAnchorRuntimeAdjust {
            resource_dw,
            resource_dh,
            inset,
            dy,
            dh,
        } => {
            let resource = geom::dlu_rect(dlu.x, dlu.y, dlu.w, dlu.h);
            let adjusted = RectPx::new(
                resource.x,
                resource.y,
                resource.w + resource_dw,
                resource.h + resource_dh,
            );
            let a = right_anchor(screen_w, screen_h, adjusted, inset);
            RectPx::new(a.x, a.y + dy, a.w, a.h + dh)
        }
    }
}

/// Right-panel static anchor (`0x0060B1D0`, no suspended game): the inset
/// override, else `(168 - w) / 2`, from the right edge, less the horizontal
/// half of the screen beyond 800; `y` is the resource `y` plus the vertical
/// half beyond 600. Both halves clamp at 0. `rect` is the converted client
/// rect.
fn right_anchor(screen_w: i32, screen_h: i32, rect: RectPx, inset: Option<i32>) -> RectPx {
    let inset = inset.unwrap_or((geom::RIGHT_PANEL_WIDTH - rect.w) / 2);
    let delta_x = geom::center_offset(screen_w, SHELL_BASE_W);
    let delta_y = geom::center_offset(screen_h, SHELL_BASE_H);
    RectPx::new(
        screen_w - inset - rect.w - delta_x,
        rect.y + delta_y,
        rect.w,
        rect.h,
    )
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::shell::descriptor::{HEADING_ANCHOR, MONITOR_ANCHOR};
    use crate::ui::shell::geom::SDBTNANM_CELL_W_NARROW;

    const SNAP: AnchorRule = AnchorRule::OwnerDrawButtonSnap {
        cell_w: SDBTNANM_CELL_W_NARROW,
    };
    const BOTTOM_ROW: AnchorRule = AnchorRule::OwnerDrawButtonBottomRow {
        cell_w: SDBTNANM_CELL_W_NARROW,
    };

    /// `0x102`'s status line (410x12 DLU) is wider than the family's and
    /// already reaches the bar's end, so its text box stays on the screen.
    #[test]
    fn a_status_line_that_reaches_the_bar_end_keeps_its_width() {
        let family = status_line_rect(RectPx::new(2, 355, 303, 12), 800, 600);
        let skirmish = status_line_rect(RectPx::new(2, 355, 410, 12), 800, 600);
        for window in [family, skirmish] {
            let text_box = status_text_rect(window);
            assert_eq!(text_box.x + text_box.w, 628, "{window:?}");
        }
    }

    /// All four anchor rules reproduce the 0xE2 helper outputs at 800x600.
    #[test]
    fn anchor_rules_reproduce_main_menu_rects_800x600() {
        let placer = Placer::new(800, 600);
        // Snap button: flush-right 156-cell, first row at y=199.
        assert_eq!(
            placer.at(SNAP, 425, 125, 108, 23),
            RectPx::new(644, 199, 156, 42)
        );
        // Exit: same column, in the final tile immediately above the bottom cap.
        assert_eq!(placer.rule(BOTTOM_ROW), RectPx::new(644, 535, 156, 42));
        // Title: compatibility +1w/+1h, right-anchor (635,2,163,17),
        // then the title finalizer adds +7y/+1h.
        assert_eq!(
            placer.at(HEADING_ANCHOR, 425, 1, 108, 10),
            RectPx::new(635, 9, 163, 18)
        );
        // 0x71C monitor window: +1w/+1h, x = 800 - 37 - 93.
        assert_eq!(
            placer.at(MONITOR_ANCHOR, 447, 29, 61, 33),
            RectPx::new(670, 47, 93, 55)
        );
    }

    #[test]
    fn right_panel_statics_take_the_vertical_half_beyond_600() {
        // 0x0060B1D0: y = resource y + max(0, (H - 600) / 2), independent of
        // the panel art, which only moves from height 768.
        for (screen_h, heading_y, monitor_y) in [(480, 9, 47), (700, 59, 97), (720, 69, 107)] {
            let placer = Placer::new(800, screen_h);
            let heading = placer.at(HEADING_ANCHOR, 425, 1, 108, 10);
            let monitor = placer.at(MONITOR_ANCHOR, 447, 29, 61, 33);
            assert_eq!(heading, RectPx::new(635, heading_y, 163, 18), "{screen_h}");
            assert_eq!(monitor, RectPx::new(670, monitor_y, 93, 55), "{screen_h}");
        }
    }

    /// Re-anchor is oversized-screen aware: 1024x768 reproduces the 0xE2 cells.
    #[test]
    fn snap_and_bottom_row_track_supported_screen_sizes() {
        assert_eq!(
            Placer::new(640, 480).rule(BOTTOM_ROW),
            RectPx::new(484, 409, 156, 42)
        );
        let at_1024 = Placer::new(1024, 768);
        assert_eq!(
            at_1024.at(SNAP, 425, 125, 108, 23),
            RectPx::new(756, 283, 156, 42)
        );
        assert_eq!(at_1024.rule(BOTTOM_ROW), RectPx::new(756, 619, 156, 42));
    }
}
