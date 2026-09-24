//! Dialog 0xE2 shell layout recovered from gamemd.exe.

use super::state::MainMenuControlId;
use crate::ui::shell::descriptor::{
    AnchorRule, BgKind, ControlDescriptor, ControlKind, DialogDescriptor, DialogId, HEADING_ANCHOR,
    MONITOR_ANCHOR, RepositionPolicy,
};
use crate::ui::shell::geom::SDBTNANM_CELL_W_NARROW;
pub use crate::ui::shell::geom::{RIGHT_PANEL_TILE_H, RIGHT_PANEL_WIDTH};
pub use crate::ui::shell::geom::{RectPx, RightPanelRects};
use crate::ui::shell::geom::{dlu_rect, lower_strip_rect, right_panel_rects};
use crate::ui::shell::layout::{LaidOutControl, layout_pass};
use crate::ui::shell::warning_monitor::WARNING_MONITOR_CONTROL;

pub const SHELL_BASE_W: i32 = 800;
pub const SHELL_BASE_H: i32 = 600;
pub const RA2TS_L_W: i32 = 632;
pub const RA2TS_L_H: i32 = 570;
pub const RA2TS_S_W: i32 = 472;
pub const RA2TS_S_H: i32 = 450;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuMovieBase {
    Ra2tsS,
    Ra2tsL,
}

impl MainMenuMovieBase {
    pub const fn asset_name(self) -> &'static str {
        match self {
            Self::Ra2tsS => "ra2ts_s.bik",
            Self::Ra2tsL => "ra2ts_l.bik",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MainMenuButtonRect {
    pub id: MainMenuControlId,
    pub rect: RectPx,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainMenuShellLayout {
    pub screen: RectPx,
    pub movie_base: MainMenuMovieBase,
    pub movie: RectPx,
    pub title: RectPx,
    /// Bottom-right version/status static. Text is `"<GUI:Version> <VERSION.TXT>"`.
    /// Sits inside the SDBTM cap, sidebar-inset from the right edge.
    pub version_line: RectPx,
    /// Bottom-left hover tooltip/status static. Receives the CSF tooltip for the
    /// control under the cursor and is otherwise blank.
    pub tooltip_line: RectPx,
    /// Static `0x71C` window rect (the animated SDWRNANM monitor).
    pub warning_monitor: RectPx,
    pub buttons: [MainMenuButtonRect; 6],
    pub right_panel: RightPanelRects,
    pub lower_strip: RectPx,
}

fn movie_origin(screen_w: i32, screen_h: i32) -> (i32, i32) {
    let x = if screen_w <= SHELL_BASE_W {
        0
    } else {
        (screen_w - SHELL_BASE_W) / 2
    };
    let y = if screen_h <= SHELL_BASE_H {
        0
    } else {
        (screen_h - SHELL_BASE_H) / 2
    };
    (x, y)
}

pub fn movie_base_for_screen_width(screen_w: u32) -> MainMenuMovieBase {
    if screen_w == 640 {
        MainMenuMovieBase::Ra2tsS
    } else {
        MainMenuMovieBase::Ra2tsL
    }
}

/// Status line `0x695` window (template `(2, 355, 303, 12)` DLU).
fn tooltip_line_rect(screen_w: i32, screen_h: i32) -> RectPx {
    crate::ui::shell::layout::status_line_rect(RectPx::new(2, 355, 303, 12), screen_w, screen_h)
}

/// Bottom-right version-line rect anchored to the SDBTM lower-cap bottom edge.
///
/// The retail layout pass uses a sidebar inset of `(168 - ctrl_w) / 2` on the
/// X axis (3 px at the standard 162 px control width), shifted left by
/// `max(0, (screen_w - 800) / 2)` on widescreens. Y anchors the control's
/// bottom edge to the bottom edge of the right-panel lower cap.
fn version_line_rect(screen_w: i32, right_panel: RightPanelRects) -> RectPx {
    let base = dlu_rect(425, 357, 108, 10);
    let ctrl_w = base.w;
    let ctrl_h = base.h;
    let inset = (RIGHT_PANEL_WIDTH - ctrl_w) / 2;
    let delta_x = if screen_w > SHELL_BASE_W {
        (screen_w - SHELL_BASE_W) / 2
    } else {
        0
    };
    let x = screen_w - inset - ctrl_w - delta_x;
    let cap_bottom = right_panel.bottom.y + right_panel.bottom.h;
    let y = cap_bottom - ctrl_h;
    RectPx::new(x, y, ctrl_w, ctrl_h)
}

/// Owner-draw button-cell width for the 0xE2 column: the 156-wide SDBTNANM cell
/// flush at the panel right edge (distinct from the 168-wide single-player cell).
const BUTTON_CELL_W: i32 = SDBTNANM_CELL_W_NARROW;

/// Build a 0xE2 owner-draw button control descriptor. The anchor consumes only
/// the DLU top; the x/w/h carry the 162x37 template client rect the SDBTNANM
/// cell replaces.
fn button_ctrl(
    id: u16,
    dlu_y: i32,
    anchor: AnchorRule,
    csf_key: &'static str,
    tooltip_key: &'static str,
) -> ControlDescriptor {
    ControlDescriptor {
        id,
        kind: ControlKind::Button,
        dlu_rect: RectPx::new(425, dlu_y, 108, 23),
        anchor,
        csf_key: Some(csf_key),
        tooltip_key: Some(tooltip_key),
        group: 0,
        enabled: true,
        visible: true,
    }
}

/// Dialog 0xE2 descriptor: the six owner-draw buttons (five stacked + Exit) plus
/// the 0x694 heading and the 0x71C warning monitor. The bottom-anchored
/// version/tooltip statics keep their bespoke shell helpers for now (no verified
/// resource id; one-of-a-kind anchors), so they are computed outside this table.
fn dialog_descriptor() -> DialogDescriptor {
    let snap = AnchorRule::OwnerDrawButtonSnap {
        cell_w: BUTTON_CELL_W,
    };
    DialogDescriptor {
        id: DialogId(0x00E2),
        bg_kind: BgKind::RightPanelShell,
        slide_eligible: true,
        reposition_policy: RepositionPolicy::IncludeSetReanchor,
        controls: vec![
            button_ctrl(
                0x0683,
                125,
                snap,
                "GUI:SinglePlayer",
                "STT:MainButtonSinglePlayer",
            ),
            button_ctrl(0x0684, 152, snap, "GUI:WWOnline", "STT:MainButtonWWOnline"),
            button_ctrl(0x0578, 179, snap, "GUI:Network", "STT:MainButtonNetwork"),
            button_ctrl(
                0x0686,
                206,
                snap,
                "GUI:MoviesAndCredits",
                "STT:MainButtonMovies",
            ),
            button_ctrl(0x055C, 233, snap, "GUI:Options", "STT:MainButtonOptions"),
            button_ctrl(
                0x03EE,
                330,
                AnchorRule::OwnerDrawButtonBottomRow {
                    cell_w: BUTTON_CELL_W,
                },
                "GUI:ExitGame",
                "STT:MainButtonExitGamemd",
            ),
            // 0x694 heading: runtime +1w/+1h, right-anchor, then +7y/+1h.
            ControlDescriptor {
                id: 0x0694,
                kind: ControlKind::Static,
                dlu_rect: RectPx::new(425, 1, 108, 10),
                anchor: HEADING_ANCHOR,
                csf_key: None,
                tooltip_key: None,
                group: 0,
                enabled: true,
                visible: true,
            },
            // 0x71C SDWRNANM warning monitor (kind 4).
            ControlDescriptor {
                id: WARNING_MONITOR_CONTROL,
                kind: ControlKind::Static,
                dlu_rect: RectPx::new(447, 29, 61, 33),
                anchor: MONITOR_ANCHOR,
                csf_key: None,
                tooltip_key: None,
                group: 0,
                enabled: true,
                visible: true,
            },
        ],
    }
}

/// Look up a laid-out control's pixel rect by resource id. The id set is a
/// compile-time-constant table, so a miss is a programming error.
fn rect_for(laid: &[LaidOutControl], id: u16) -> RectPx {
    laid.iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("0xE2 descriptor missing control id {id:#06x}"))
        .rect
}

pub fn compute_layout(screen_w: u32, screen_h: u32) -> MainMenuShellLayout {
    let screen_w = screen_w as i32;
    let screen_h = screen_h as i32;
    let movie_base = movie_base_for_screen_width(screen_w as u32);
    let (movie_x, movie_y) = movie_origin(screen_w, screen_h);
    let (movie_w, movie_h) = match movie_base {
        MainMenuMovieBase::Ra2tsS => (RA2TS_S_W, RA2TS_S_H),
        MainMenuMovieBase::Ra2tsL => (RA2TS_L_W, RA2TS_L_H),
    };
    let right_panel = right_panel_rects(screen_w, screen_h);
    let lower_strip = lower_strip_rect(screen_w, screen_h);
    let version_line = version_line_rect(screen_w, right_panel);
    let tooltip_line = tooltip_line_rect(screen_w, screen_h);

    // Owner-draw buttons + the 0x694 heading + 0x71C monitor static are laid out
    // by the shared shell pass (contract C7: DLU->pixel once, then include-set
    // re-anchor). The bottom-anchored version/tooltip statics keep their bespoke
    // helpers above.
    let laid = layout_pass(&dialog_descriptor(), screen_w, screen_h);

    MainMenuShellLayout {
        screen: RectPx::new(0, 0, screen_w, screen_h),
        movie_base,
        movie: RectPx::new(movie_x, movie_y, movie_w, movie_h),
        title: rect_for(&laid, 0x0694),
        version_line,
        tooltip_line,
        warning_monitor: rect_for(&laid, WARNING_MONITOR_CONTROL),
        buttons: [
            MainMenuButtonRect {
                id: MainMenuControlId::SinglePlayer0x683,
                rect: rect_for(&laid, 0x0683),
            },
            MainMenuButtonRect {
                id: MainMenuControlId::WwOnline0x684,
                rect: rect_for(&laid, 0x0684),
            },
            MainMenuButtonRect {
                id: MainMenuControlId::Network0x578,
                rect: rect_for(&laid, 0x0578),
            },
            MainMenuButtonRect {
                id: MainMenuControlId::MoviesAndCredits0x686,
                rect: rect_for(&laid, 0x0686),
            },
            MainMenuButtonRect {
                id: MainMenuControlId::Options0x55c,
                rect: rect_for(&laid, 0x055C),
            },
            MainMenuButtonRect {
                id: MainMenuControlId::ExitGame0x3ee,
                rect: rect_for(&laid, 0x03EE),
            },
        ],
        right_panel,
        lower_strip,
    }
}

fn scale_rect(rect: RectPx, scale_x: f32, scale_y: f32) -> RectPx {
    let x0 = (rect.x as f32 * scale_x).round() as i32;
    let y0 = (rect.y as f32 * scale_y).round() as i32;
    let x1 = ((rect.x + rect.w) as f32 * scale_x).round() as i32;
    let y1 = ((rect.y + rect.h) as f32 * scale_y).round() as i32;
    RectPx::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
}

/// Compute the modern responsive shell layout.
///
/// The verified 800x600 dialog is kept as the logical coordinate space, then
/// stretched independently on X/Y to fill the swapchain. This intentionally
/// drifts from retail pixel parity, but keeps render and input coordinates in
/// the same window-pixel space.
pub fn compute_responsive_layout(screen_w: u32, screen_h: u32) -> MainMenuShellLayout {
    let base = compute_layout(SHELL_BASE_W as u32, SHELL_BASE_H as u32);
    let scale_x = screen_w as f32 / SHELL_BASE_W as f32;
    let scale_y = screen_h as f32 / SHELL_BASE_H as f32;
    let mut buttons = base.buttons;
    for button in &mut buttons {
        button.rect = scale_rect(button.rect, scale_x, scale_y);
    }
    let right_panel = RightPanelRects {
        top: scale_rect(base.right_panel.top, scale_x, scale_y),
        tile: scale_rect(base.right_panel.tile, scale_x, scale_y),
        tile_count: base.right_panel.tile_count,
        bottom: scale_rect(base.right_panel.bottom, scale_x, scale_y),
    };
    let lower_strip = scale_rect(base.lower_strip, scale_x, scale_y);

    MainMenuShellLayout {
        screen: RectPx::new(0, 0, screen_w as i32, screen_h as i32),
        movie_base: movie_base_for_screen_width(screen_w),
        movie: scale_rect(base.movie, scale_x, scale_y),
        title: scale_rect(base.title, scale_x, scale_y),
        version_line: scale_rect(base.version_line, scale_x, scale_y),
        tooltip_line: scale_rect(base.tooltip_line, scale_x, scale_y),
        warning_monitor: scale_rect(base.warning_monitor, scale_x, scale_y),
        buttons,
        right_panel,
        lower_strip,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_rects_match_800x600() {
        // All six buttons are SDBTNANM cells: 156x42, flush-right at x=644
        // (632 panel left + 168 - 156). The five stacked buttons are grid-snapped
        // Y; Exit occupies the final complete tile above the bottom cap.
        let layout = compute_layout(800, 600);
        assert_eq!(layout.movie_base, MainMenuMovieBase::Ra2tsL);
        assert_eq!(layout.movie, RectPx::new(0, 0, 632, 570));
        assert_eq!(layout.buttons[0].rect, RectPx::new(644, 199, 156, 42)); // SP
        assert_eq!(layout.buttons[5].rect, RectPx::new(644, 535, 156, 42)); // Exit
    }

    #[test]
    fn buttons_grid_snap_and_exit_special_case_800x600() {
        let layout = compute_layout(800, 600);
        // SP/WW/Net/Movies/Options snap to 42-px SDBTNANM rows from y=199.
        let expected_y = [199, 241, 283, 325, 367];
        for (button, y) in layout.buttons[..5].iter().zip(expected_y) {
            assert_eq!(button.rect, RectPx::new(644, y, 156, 42));
        }
        // Exit (0x3ee): same flush-right cell in the final row above SDBTM.
        assert_eq!(layout.buttons[5].rect, RectPx::new(644, 535, 156, 42));
    }

    #[test]
    fn title_rect_matches_enrolled_runtime_at_required_resolutions() {
        // Resource 162x16 -> compatibility 163x17 -> right-anchor ->
        // finalizer +7y/+1h. Only 800x600 has a sealed native pixel guard.
        assert_eq!(compute_layout(640, 480).title, RectPx::new(475, 9, 163, 18));
        assert_eq!(compute_layout(800, 600).title, RectPx::new(635, 9, 163, 18));
        assert_eq!(
            compute_layout(1024, 768).title,
            RectPx::new(747, 93, 163, 18)
        );
    }

    #[test]
    fn tooltip_line_anchors_bottom_left_with_10_px_inset() {
        // DLU (2, 355, 303, 12) at 800x600 → pixel (3, 577, 455, 20), one
        // pixel larger at runtime (456x21). 0x0060B550: X = 0 + 10 = 10,
        // Y = 600 - 21 - 0 - 1 = 578.
        let layout = compute_layout(800, 600);
        assert_eq!(layout.tooltip_line, RectPx::new(10, 578, 456, 21));
    }

    #[test]
    fn version_line_uses_sidebar_inset_and_bottom_cap_anchor() {
        // DLU (425, 357, 108, 10) at 800x600 → pixel (638, 580, 162, 16) raw.
        // Sidebar inset = (168 - 162) / 2 = 3 → final X = 800 - 3 - 162 = 635.
        // Bottom-cap anchor: right_panel.bottom = (632, 577, 168, 23);
        // bottom_edge = 600. Y = 600 - 16 = 584.
        let layout = compute_layout(800, 600);
        assert_eq!(layout.version_line, RectPx::new(635, 584, 162, 16));
    }

    #[test]
    fn key_rects_match_640x480_movie_choice() {
        let layout = compute_layout(640, 480);
        assert_eq!(layout.movie_base, MainMenuMovieBase::Ra2tsS);
        assert_eq!(layout.movie, RectPx::new(0, 0, 472, 450));
        assert_eq!(layout.buttons[5].rect, RectPx::new(484, 409, 156, 42));
    }

    #[test]
    fn large_screen_offsets_movie_without_scaling() {
        let layout = compute_layout(1024, 768);
        assert_eq!(layout.movie_base, MainMenuMovieBase::Ra2tsL);
        assert_eq!(layout.movie, RectPx::new(112, 84, 632, 570));
        assert_eq!(layout.right_panel.top, RectPx::new(744, 84, 168, 199));
        assert_eq!(layout.right_panel.tile, RectPx::new(744, 283, 168, 42));
        assert_eq!(layout.right_panel.bottom, RectPx::new(744, 661, 168, 23));
        assert_eq!(layout.lower_strip, RectPx::new(112, 652, 632, 32));
        assert_eq!(layout.title, RectPx::new(747, 93, 163, 18));
    }

    #[test]
    fn large_screen_buttons_sdbtnanm_cells_and_exit() {
        // 1024x768: left_margin=112, top_margin=84, panel.top.x=744 -> cells at
        // x=756. Grid anchor panel_y = 84 + 199 = 283; rows step 42.
        let layout = compute_layout(1024, 768);
        let expected_y = [283, 325, 367, 409, 451];
        for (button, y) in layout.buttons[..5].iter().zip(expected_y) {
            assert_eq!(button.rect, RectPx::new(756, y, 156, 42));
        }
        // Exit: same flush-right cell x=756 in the final row above SDBTM.
        assert_eq!(layout.buttons[5].rect, RectPx::new(756, 619, 156, 42));
    }

    #[test]
    fn responsive_layout_fills_window_by_scaling_base_shell() {
        let layout = compute_responsive_layout(1600, 900);
        assert_eq!(layout.screen, RectPx::new(0, 0, 1600, 900));
        assert_eq!(layout.movie_base, MainMenuMovieBase::Ra2tsL);
        assert_eq!(layout.movie, RectPx::new(0, 0, 1264, 855));
        // Base SP cell (644,199,156,42) scaled 2x/1.5x (corner-rounded) ->
        // (1288,299,312,63); base Exit (644,535,156,42) -> (1288,803,312,63).
        assert_eq!(layout.buttons[0].rect, RectPx::new(1288, 299, 312, 63));
        assert_eq!(layout.buttons[5].rect, RectPx::new(1288, 803, 312, 63));
    }

    #[test]
    fn responsive_layout_keeps_640_movie_asset_rule() {
        let layout = compute_responsive_layout(640, 480);
        assert_eq!(layout.movie_base, MainMenuMovieBase::Ra2tsS);
        assert_eq!(layout.movie, RectPx::new(0, 0, 506, 456));
        // Base SP cell (644,199,156,42) scaled 0.8x/0.8x → (515, 159, 125, 34).
        assert_eq!(layout.buttons[0].rect, RectPx::new(515, 159, 125, 34));
    }
}
