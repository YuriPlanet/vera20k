//! Dialog 0xE2 shell layout recovered from gamemd.exe.

use super::state::{HAS_EDITION_SWITCH, MAIN_MENU_DIALOG, MainMenuControlId, SWITCH_EDITION};
use crate::ui::shell::descriptor::{AnchorRule, HEADING_ANCHOR, MONITOR_ANCHOR};
use crate::ui::shell::geom::SDBTNANM_CELL_W_NARROW;
pub use crate::ui::shell::geom::{RIGHT_PANEL_TILE_H, RIGHT_PANEL_WIDTH};
pub use crate::ui::shell::geom::{RectPx, RightPanelRects};
pub use crate::ui::shell::geom::{SHELL_BASE_H, SHELL_BASE_W};
use crate::ui::shell::geom::{ShellScale, dlu_rect, lower_strip_rect, right_panel_rects};
use crate::ui::shell::layout::Placer;
use crate::ui::shell::menu_page::{
    MenuPageButtonRect, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec,
};

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
    /// How far these rects sit from the 800x600 art they were authored against.
    /// `RETAIL` on the retail layout at every screen size.
    pub scale: ShellScale,
    pub screen: RectPx,
    pub movie_base: MainMenuMovieBase,
    pub movie: RectPx,
    pub title: RectPx,
    /// Bottom-right version static. Retail text is `"<GUI:Version> <VERSION.TXT>"`;
    /// VERA shows its own release there (`BUILD_STAMP`).
    /// Sits inside the SDBTM cap, sidebar-inset from the right edge.
    pub version_line: RectPx,
    /// Bottom-left hover tooltip/status static. Receives the CSF tooltip for the
    /// control under the cursor and is otherwise blank.
    pub tooltip_line: RectPx,
    /// Static `0x71C` window rect (the animated SDWRNANM monitor).
    pub warning_monitor: RectPx,
    /// Top to bottom; Exit is last.
    pub buttons: Vec<MainMenuButtonRect>,
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
/// X axis, shifted left by `max(0, (screen_w - 800) / 2)` on widescreens. Y
/// anchors the control's bottom edge to the bottom edge of the right-panel
/// lower cap. The runtime window is one pixel wider and taller than the DLU
/// conversion, as for the heading, the monitor and the status line.
fn version_line_rect(screen_w: i32, right_panel: RightPanelRects) -> RectPx {
    let base = dlu_rect(425, 357, 108, 10);
    let ctrl_w = base.w + 1;
    let ctrl_h = base.h + 1;
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

/// RT_DIALOG `0xE2`'s owner-draw buttons, five stacked and Exit on the bottom
/// row, with their captions, status help and the results the proc writes.
pub const MAIN_MENU_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: MAIN_MENU_DIALOG,
    title_key: "GUI:MainMenu",
    stacked: &[
        MenuPageButtonSpec::new(
            0x0683,
            125,
            "GUI:SinglePlayer",
            "STT:MainButtonSinglePlayer",
        )
        .returning(1),
        MenuPageButtonSpec::new(0x0684, 152, "GUI:WWOnline", "STT:MainButtonWWOnline").returning(2),
        MenuPageButtonSpec::new(0x0578, 179, "GUI:Network", "STT:MainButtonNetwork").returning(3),
        MenuPageButtonSpec::new(0x0686, 206, "GUI:MoviesAndCredits", "STT:MainButtonMovies")
            .returning(4),
        MenuPageButtonSpec::new(0x055C, 233, "GUI:Options", "STT:MainButtonOptions").returning(5),
    ],
    back: MenuPageButtonSpec::new(0x03EE, 330, "GUI:ExitGame", "STT:MainButtonExitGamemd")
        .returning(6),
};

fn edition_button(ra2: bool) -> MenuPageButtonSpec {
    let key = if ra2 {
        "VERA:SwitchToYR"
    } else {
        "VERA:SwitchToRA2"
    };
    MenuPageButtonSpec::new(SWITCH_EDITION, 0, key, "VERA:SwitchEditionHelp")
}

pub(crate) fn nodes(ra2: bool) -> vera20k_gui::NodeSpec {
    use vera20k_gui::{NodeKind, NodeSpec};
    NodeSpec::new(NodeKind::Dialog).children(
        MAIN_MENU_PAGE
            .stacked
            .iter()
            .copied()
            .chain(HAS_EDITION_SWITCH.then(|| edition_button(ra2)))
            .chain([MAIN_MENU_PAGE.back])
            .map(|button| button.node()),
    )
}

/// A short retail surface cannot hold all seven desktop controls.
pub(crate) fn retail_column_fits(screen_w: i32, screen_h: i32) -> bool {
    !HAS_EDITION_SWITCH
        || right_panel_rects(screen_w, screen_h).tile_count
            >= MAIN_MENU_PAGE.stacked.len() as i32 + 2
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

    // Owner-draw buttons + the 0x694 heading + 0x71C monitor static: DLU->pixel
    // once, then the include-set re-anchor (contract C7). The owner-draw
    // buttons read only their DLU top; the bottom-anchored version/tooltip
    // statics keep their bespoke helpers above.
    let placer = Placer::new(screen_w, screen_h);
    let button = |button: &MenuPageButtonSpec, rule| MainMenuButtonRect {
        id: MainMenuControlId::from_resource_id(button.id)
            .expect("0xE2 buttons are main-menu controls"),
        rect: placer.at(rule, 425, button.dlu_top, 108, 23),
    };
    let snap = AnchorRule::OwnerDrawButtonSnap {
        cell_w: BUTTON_CELL_W,
    };
    let bottom_row = AnchorRule::OwnerDrawButtonBottomRow {
        cell_w: BUTTON_CELL_W,
    };
    let exit = button(&MAIN_MENU_PAGE.back, bottom_row);
    let mut buttons: Vec<_> = MAIN_MENU_PAGE
        .stacked
        .iter()
        .map(|b| button(b, snap))
        .collect();
    if HAS_EDITION_SWITCH && retail_column_fits(screen_w, screen_h) {
        let mut rect = exit.rect;
        rect.y -= crate::ui::shell::geom::RIGHT_PANEL_TILE_H;
        buttons.push(MainMenuButtonRect {
            id: MainMenuControlId::SwitchEdition,
            rect,
        });
    }
    buttons.push(exit);

    MainMenuShellLayout {
        scale: crate::ui::shell::geom::RETAIL_SCALE,
        screen: RectPx::new(0, 0, screen_w, screen_h),
        movie_base,
        movie: RectPx::new(movie_x, movie_y, movie_w, movie_h),
        // 0x694 heading: runtime +1w/+1h, right-anchor, then +7y/+1h.
        title: placer.at(HEADING_ANCHOR, 425, 1, 108, 10),
        version_line,
        tooltip_line,
        warning_monitor: placer.at(MONITOR_ANCHOR, 447, 29, 61, 33),
        buttons,
        right_panel,
        lower_strip,
    }
}

/// The RA2TS source rect inside the 800x600 logical shell: the large loop's
/// native 632x570 canvas, flush to the panel/strip inner edges (verified by the
/// 800x600 pixel guard in `compute_layout`).
pub const RA2TS_BASE_MOVIE: RectPx = RectPx::new(0, 0, RA2TS_L_W, RA2TS_L_H);

/// The movie loop a responsive shell plays. The retail 640-wide asset switch
/// only made sense for the unscaled layout; a stretched area always uses the
/// large loop.
pub fn movie_base_for_responsive_width(_screen_w: u32) -> MainMenuMovieBase {
    MainMenuMovieBase::Ra2tsL
}

// The 800x600 layout plays the large loop, as a stretched area always does
// ([`movie_base_for_responsive_width`]).
vera20k_gui::scaled_rects!(MainMenuButtonRect { rect; keep: id });
vera20k_gui::scaled_rects!(MainMenuShellLayout {
    [scale, screen]
    movie, title, version_line, tooltip_line, warning_monitor, buttons, right_panel, lower_strip;
    keep: movie_base
});

impl MainMenuShellLayout {
    /// The layout as the family pages' shared paint reads it; status line
    /// `0x695` is the tooltip line.
    pub fn page(&self) -> MenuPageLayout {
        MenuPageLayout {
            scale: self.scale,
            screen: self.screen,
            movie_base: self.movie_base,
            movie: self.movie,
            title: self.title,
            warning_monitor: self.warning_monitor,
            status_help: self.tooltip_line,
            buttons: self
                .buttons
                .iter()
                .map(|b| MenuPageButtonRect {
                    id: b.id.resource_id(),
                    rect: b.rect,
                })
                .collect(),
            right_panel: self.right_panel,
            lower_strip: self.lower_strip,
        }
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::shell::geom::fit_window;

    #[test]
    fn buttons_grid_snap_and_exit_special_case_800x600() {
        let layout = compute_layout(800, 600);
        // SP/WW/Net/Movies/Options snap to 42-px SDBTNANM rows from y=199.
        let expected_y = [199, 241, 283, 325, 367];
        for (button, y) in layout.buttons[..5].iter().zip(expected_y) {
            assert_eq!(button.rect, RectPx::new(644, y, 156, 42));
        }
        // Exit (0x3ee): same flush-right cell in the final row above SDBTM.
        assert_eq!(
            layout.buttons.last().unwrap().rect,
            RectPx::new(644, 535, 156, 42)
        );
    }

    #[test]
    fn the_edition_switch_precedes_exit_without_overlapping_at_every_size() {
        use MainMenuControlId::*;
        let ids = |layout: MainMenuShellLayout| {
            layout
                .buttons
                .iter()
                .map(|button| button.id)
                .collect::<Vec<_>>()
        };
        let mut expected = vec![
            SinglePlayer0x683,
            WwOnline0x684,
            Network0x578,
            MoviesAndCredits0x686,
            Options0x55c,
            ExitGame0x3ee,
        ];
        if HAS_EDITION_SWITCH {
            expected.insert(expected.len() - 1, SwitchEdition);
        }
        let compact = if retail_column_fits(640, 480) {
            compute_layout(640, 480)
        } else {
            fit_window(640, 480, compute_layout)
        };
        for layout in [
            compact,
            compute_layout(800, 600),
            fit_window(1920, 1080, compute_layout),
        ] {
            assert!(
                layout
                    .buttons
                    .windows(2)
                    .all(|pair| pair[0].rect.y + pair[0].rect.h <= pair[1].rect.y)
            );
            assert_eq!(ids(layout), expected);
        }
        assert_eq!(edition_button(false).csf_key, "VERA:SwitchToRA2");
        assert_eq!(edition_button(true).csf_key, "VERA:SwitchToYR");
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
        // DLU (425, 357, 108, 10) at 800x600 → pixel (638, 580, 162, 16) raw,
        // one pixel larger at runtime (163x17). Sidebar inset = (168 - 163) / 2
        // = 2 → X = 800 - 2 - 163 = 635. Bottom-cap anchor: right_panel.bottom
        // = (632, 577, 168, 23); bottom_edge = 600. Y = 600 - 17 = 583.
        let layout = compute_layout(800, 600);
        assert_eq!(layout.version_line, RectPx::new(635, 583, 163, 17));
    }

    #[test]
    fn key_rects_match_640x480_movie_choice() {
        let layout = compute_layout(640, 480);
        assert_eq!(layout.movie_base, MainMenuMovieBase::Ra2tsS);
        assert_eq!(layout.movie, RectPx::new(0, 0, 472, 450));
        assert_eq!(
            layout.buttons.last().unwrap().rect,
            RectPx::new(484, 409, 156, 42)
        );
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
    fn responsive_layout_fits_the_base_shell_centred_in_the_window() {
        let layout = fit_window(1600, 900, compute_layout);
        assert_eq!(layout.screen, RectPx::new(0, 0, 1600, 900));
        assert_eq!(layout.movie_base, MainMenuMovieBase::Ra2tsL);
        // The height limits the factor to 1.5: the 1200x900 shell sits 200 px
        // in from either side. Base movie (0,0,632,570) -> (200,0,948,855).
        assert_eq!(layout.movie, RectPx::new(200, 0, 948, 855));
        // Base SP cell (644,199,156,42) scaled 1.5x (corner-rounded) ->
        // (1166,299,234,63); base Exit (644,535,156,42) -> (1166,803,234,63).
        assert_eq!(layout.buttons[0].rect, RectPx::new(1166, 299, 234, 63));
        assert_eq!(
            layout.buttons.last().unwrap().rect,
            RectPx::new(1166, 803, 234, 63)
        );
    }

    /// At 1.28x (1366x768) each scaled panel row rounds like the button
    /// authored on it. Stepping the rounded 53-px tile put row 8 six pixels
    /// above Exit, so slide frames and tiles jumped against the buttons.
    #[test]
    fn scaled_panel_rows_land_on_their_buttons() {
        let base = compute_layout(800, 600);
        let layout = fit_window(1366, 768, compute_layout);
        let panel = layout.right_panel;
        for (authored, button) in base.buttons.iter().zip(&layout.buttons) {
            let row = (authored.rect.y - base.right_panel.tile.y) / 42;
            assert_eq!(panel.row_cell(row, 156, 42), button.rect, "row {row}");
        }
        for row in 0..panel.tile_count {
            let rect = panel.tile_row(row);
            assert_eq!(rect.y + rect.h, panel.tile_row(row + 1).y, "row {row}");
        }
        assert_eq!(panel.tile_row(panel.tile_count).y, panel.bottom.y);
    }

    #[test]
    fn responsive_layout_keeps_large_movie_when_shrunk_to_640x480() {
        // The stretched shell always plays the large loop; the retail 640
        // asset switch belongs to the unscaled layout.
        let layout = fit_window(640, 480, compute_layout);
        assert_eq!(layout.movie_base, MainMenuMovieBase::Ra2tsL);
        assert_eq!(layout.movie, RectPx::new(0, 0, 506, 456));
        // Base SP cell (644,199,156,42) scaled 0.8x/0.8x → (515, 159, 125, 34).
        assert_eq!(layout.buttons[0].rect, RectPx::new(515, 159, 125, 34));
    }
    #[test]
    fn production_main_menu_title_uses_exact_runtime_rect_at_required_resolutions() {
        for (screen_w, screen_h, expected) in [
            (640, 480, RectPx::new(475, 9, 163, 18)),
            (800, 600, RectPx::new(635, 9, 163, 18)),
            (1024, 768, RectPx::new(747, 93, 163, 18)),
        ] {
            let actual = compute_layout(screen_w, screen_h).title;
            assert_eq!(actual, expected, "{screen_w}x{screen_h}");
        }
    }
}
