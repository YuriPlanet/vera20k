//! Shared layout for the RA2TS-bearing right-panel menu pages.
//!
//! Single Player `0x100` and Movies & Credits `0x101` are the same dialog
//! shape in gamemd.exe: a `0x71A` RA2TS movie static, heading `0x694`, status
//! help `0x695`, three stacked owner-draw buttons at DLU tops 122/149/176 and a
//! Back button on the final right-panel tile. Only the control identities,
//! labels and dialog-proc results differ, so each page is a `MenuPageSpec`
//! table over this one layout.

use super::descriptor::{DialogId, HEADING_ANCHOR, MONITOR_ANCHOR};
use super::geom::{
    RectPx, RightPanelRects, SDBTNANM_CELL_H, SDBTNANM_CELL_W_NARROW, center_offset, dlu_rect,
    lower_strip_rect, right_panel_rects, snap_button_biased_truncate,
};
use super::layout::anchor_rect;
use crate::ui::main_menu_shell::{MainMenuMovieBase, movie_base_for_screen_width};

const SHELL_BASE_W: i32 = 800;
const SHELL_BASE_H: i32 = 600;
const RIGHT_PANEL_TILE_H: i32 = super::geom::RIGHT_PANEL_TILE_H;
const STATUS_HELP_W: i32 = 456;
const STATUS_HELP_H: i32 = 21;
const STATUS_HELP_BOTTOM_INSET: i32 = 1;
const RA2TS_L_W: i32 = 632;
const RA2TS_L_H: i32 = 570;
const RA2TS_S_W: i32 = 472;
const RA2TS_S_H: i32 = 450;

/// One owner-draw button of a menu page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuPageButtonSpec {
    /// Win32 control resource id.
    pub id: u16,
    /// Resource-template DLU top. Stacked buttons snap from it; the Back
    /// button occupies the final tile and ignores it.
    pub dlu_top: i32,
    /// Caption CSF key from the dialog template.
    pub csf_key: &'static str,
    /// Status-help CSF key written to static `0x695` on hover.
    pub tooltip_key: &'static str,
    /// Constant value the dialog proc writes through `GetWindowLong(hwnd, 8)`,
    /// or `None` when it writes run-time data instead.
    pub result: Option<i32>,
}

/// Static description of one right-panel menu page dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuPageSpec {
    pub dialog: DialogId,
    /// Heading static `0x694` caption.
    pub title_key: &'static str,
    /// Stacked right-panel buttons in resource order.
    pub stacked: &'static [MenuPageButtonSpec],
    /// Bottom-row Back button (`0x686` in both pages).
    pub back: MenuPageButtonSpec,
}

impl MenuPageSpec {
    /// Every owner-draw button, stacked first, then Back.
    pub fn buttons(&self) -> impl Iterator<Item = &MenuPageButtonSpec> {
        self.stacked.iter().chain(std::iter::once(&self.back))
    }

    pub fn button(&self, id: u16) -> Option<&MenuPageButtonSpec> {
        self.buttons().find(|button| button.id == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuPageButtonRect {
    pub id: u16,
    pub rect: RectPx,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuPageLayout {
    pub screen: RectPx,
    pub movie_base: MainMenuMovieBase,
    pub movie: RectPx,
    /// Heading static `0x694` paint rect.
    pub title: RectPx,
    /// Static `0x71C` window rect (the SDWRNANM monitor).
    pub warning_monitor: RectPx,
    pub status_help: RectPx,
    /// Stacked buttons in spec order, then Back.
    pub buttons: Vec<MenuPageButtonRect>,
    pub right_panel: RightPanelRects,
    pub lower_strip: RectPx,
}

fn movie_origin(screen_w: i32, screen_h: i32) -> (i32, i32) {
    (
        center_offset(screen_w, SHELL_BASE_W),
        center_offset(screen_h, SHELL_BASE_H),
    )
}

fn status_help_rect(screen_w: i32, screen_h: i32) -> RectPx {
    let offset_x = center_offset(screen_w, SHELL_BASE_W);
    let offset_y = center_offset(screen_h, SHELL_BASE_H);
    RectPx::new(
        offset_x + 10,
        screen_h - offset_y - STATUS_HELP_H - STATUS_HELP_BOTTOM_INSET,
        STATUS_HELP_W,
        STATUS_HELP_H,
    )
}

fn back_rect(screen_w: i32, panel: RightPanelRects) -> RectPx {
    let offset_x = center_offset(screen_w, SHELL_BASE_W);
    RectPx::new(
        screen_w - offset_x - SDBTNANM_CELL_W_NARROW,
        panel.tile.y + (panel.tile_count - 1).max(0) * RIGHT_PANEL_TILE_H,
        SDBTNANM_CELL_W_NARROW,
        SDBTNANM_CELL_H,
    )
}

pub fn compute_layout(spec: &MenuPageSpec, screen_w: u32, screen_h: u32) -> MenuPageLayout {
    let screen_w = screen_w as i32;
    let screen_h = screen_h as i32;
    let movie_base = movie_base_for_screen_width(screen_w as u32);
    let (movie_x, movie_y) = movie_origin(screen_w, screen_h);
    let (movie_w, movie_h) = match movie_base {
        MainMenuMovieBase::Ra2tsS => (RA2TS_S_W, RA2TS_S_H),
        MainMenuMovieBase::Ra2tsL => (RA2TS_L_W, RA2TS_L_H),
    };
    let panel = right_panel_rects(screen_w, screen_h);
    // Both statics sit at the same template rects as in main menu 0xE2 (the
    // right-panel anchor reads only their size, not the DLU x).
    let title = anchor_rect(
        HEADING_ANCHOR,
        RectPx::new(425, 1, 108, 10),
        screen_w,
        screen_h,
    );
    let warning_monitor = anchor_rect(
        MONITOR_ANCHOR,
        RectPx::new(447, 29, 61, 33),
        screen_w,
        screen_h,
    );
    let mut buttons: Vec<MenuPageButtonRect> = spec
        .stacked
        .iter()
        .map(|button| MenuPageButtonRect {
            id: button.id,
            rect: snap_button_biased_truncate(
                screen_w,
                screen_h,
                dlu_rect(425, button.dlu_top, 108, 23),
                panel,
                SDBTNANM_CELL_W_NARROW,
            ),
        })
        .collect();
    buttons.push(MenuPageButtonRect {
        id: spec.back.id,
        rect: back_rect(screen_w, panel),
    });

    MenuPageLayout {
        screen: RectPx::new(0, 0, screen_w, screen_h),
        movie_base,
        movie: RectPx::new(movie_x, movie_y, movie_w, movie_h),
        title,
        warning_monitor,
        status_help: status_help_rect(screen_w, screen_h),
        buttons,
        right_panel: panel,
        lower_strip: lower_strip_rect(screen_w, screen_h),
    }
}

impl MenuPageLayout {
    pub fn button_rect(&self, id: u16) -> Option<RectPx> {
        self.buttons
            .iter()
            .find(|button| button.id == id)
            .map(|button| button.rect)
    }
}
