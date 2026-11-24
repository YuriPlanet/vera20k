//! Dialog 0x100 Single Player page: resource-order controls over the shared
//! right-panel menu page layout (`ui::shell::menu_page`).

use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};

/// RT_DIALOG `0x100` buttons with the results written by dialog proc
/// `0x0052D640` (`SinglePlayerDialog0x100__WndProc`). Control `0x68A` (result
/// 10) has no template child, so it is not reachable.
pub const SINGLE_PLAYER_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogResourceId(0x0100),
    title_key: "GUI:SinglePlayerMenu",
    stacked: &[
        MenuPageButtonSpec::new(
            0x0688,
            122,
            "GUI:NewCampaign",
            "STT:SingleButtonNewCampaign",
        )
        .returning(8),
        MenuPageButtonSpec::new(
            0x0689,
            149,
            "GUI:LoadSavedGame",
            "STT:SingleButtonLoadSavedGame",
        )
        .returning(9),
        MenuPageButtonSpec::new(0x0579, 176, "GUI:Skirmish", "STT:SingleButtonSkirmish")
            .returning(0x0B),
    ],
    back: MenuPageButtonSpec::new(0x0686, 346, "GUI:MainMenu", "STT:SingleButtonBack")
        .returning(0x12),
};

pub fn compute_layout(screen_w: u32, screen_h: u32) -> MenuPageLayout {
    menu_page::compute_layout(&SINGLE_PLAYER_PAGE, screen_w, screen_h)
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::main_menu_shell::MainMenuMovieBase;
    use crate::ui::shell::geom::RectPx;

    fn rect(layout: &MenuPageLayout, id: u16) -> RectPx {
        layout.button_rect(id).expect("page button")
    }

    #[test]
    fn key_rects_match_dialog_0x100_rows_at_800x600() {
        let layout = compute_layout(800, 600);
        assert_eq!(layout.movie_base, MainMenuMovieBase::Ra2tsL);
        assert_eq!(layout.movie, RectPx::new(0, 0, 632, 570));
        // Same heading rect as 0xE2: +1 window, then +7y/+1h (0x60BD17).
        assert_eq!(layout.title, RectPx::new(635, 9, 163, 18));
        // 0x71C window; the 92x53 SDWRNANM frame centers at (670, 48).
        assert_eq!(layout.warning_monitor, RectPx::new(670, 47, 93, 55));
        assert_eq!(rect(&layout, 0x0688), RectPx::new(644, 199, 156, 42));
        assert_eq!(rect(&layout, 0x0689), RectPx::new(644, 241, 156, 42));
        assert_eq!(rect(&layout, 0x0579), RectPx::new(644, 283, 156, 42));
        assert_eq!(rect(&layout, 0x0686), RectPx::new(644, 535, 156, 42));
        assert_eq!(layout.status_help, RectPx::new(10, 578, 456, 21));
    }

    #[test]
    fn small_screen_keeps_native_button_and_status_extents() {
        let layout = compute_layout(640, 480);
        assert_eq!(rect(&layout, 0x0688), RectPx::new(484, 199, 156, 42));
        assert_eq!(rect(&layout, 0x0686), RectPx::new(484, 409, 156, 42));
        assert_eq!(layout.status_help, RectPx::new(10, 458, 456, 21));
    }
}
