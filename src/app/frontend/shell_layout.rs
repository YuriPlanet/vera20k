//! The one place the shell family picks its layout: gamemd's centered,
//! unscaled 800x600 dialog, or the same dialog scaled to fit the live window
//! at its own shape ([`fills_window`]).
//!
//! Every shell route that both paints and hit-tests must take its rects from
//! here; a render path computing one layout and an input handler the other
//! misaligns clicks by the scale factor.

use crate::app::AppState;
use crate::ui::main_menu_shell::{MainMenuShellLayout, compute_layout, compute_responsive_layout};
use crate::ui::shell::menu_page::{self, MenuPageLayout, MenuPageSpec};
use crate::ui::skirmish_shell::{
    ChooseMapModalLayout, RandomMapSetupLayout, SavedSeedLayout, SavedSeedMode, SkirmishShellLayout,
};

/// The window pixel space the shell paints and hit-tests in. The GPU surface
/// size, not the monitor's: a resized window repaints at the new size.
pub(crate) fn shell_screen_size(state: &AppState) -> (u32, u32) {
    (
        state.renderer.gpu.config.width,
        state.renderer.gpu.config.height,
    )
}

/// Whether the shell family fits the live window instead of painting gamemd's
/// centered, unscaled dialog.
pub(crate) fn fills_window(state: &AppState) -> bool {
    window_takes_fitted_shell(shell_screen_size(state))
}

/// Only the two sizes gamemd's shell runs at keep the retail layout: 800x600,
/// and 640x480 with its own 640-wide art (`AllowModeToggle`). Any other window
/// gets the 800x600 dialog scaled to fit at its own shape, centred. The startup
/// splash asks by surface size, before the app state exists.
pub(crate) fn window_takes_fitted_shell(size: (u32, u32)) -> bool {
    !matches!(size, (800, 600) | (640, 480))
}

/// How much a menu's software cursor grows: with a fitted menu's art, by the
/// same factor. An active-game shell draws its cursor in the match frame,
/// which the match zoom scales instead.
pub(crate) fn menu_cursor_scale(state: &AppState) -> f32 {
    if state.frontend.screen == crate::ui::game_screen::GameScreen::InGame || !fills_window(state) {
        return 1.0;
    }
    let (width, height) = shell_screen_size(state);
    crate::ui::shell::geom::ShellScale::from_screen(width as i32, height as i32).factor()
}

/// Main menu dialog `0xE2`.
pub(crate) fn main_menu_layout(state: &AppState) -> MainMenuShellLayout {
    let (width, height) = shell_screen_size(state);
    if fills_window(state) {
        compute_responsive_layout(width, height)
    } else {
        compute_layout(width, height)
    }
}

/// Count-rule message boxes over the front-end shell use the same mapping for
/// their art, labels and controller hit rectangles as the page beneath them.
pub(crate) fn shell_message_box_layout(
    state: &AppState,
    kind: crate::ui::shell::modal::ModalKind,
) -> crate::ui::shell::modal::MessageBoxLayout {
    message_box_layout_for_screen(kind, shell_screen_size(state))
}

fn message_box_layout_for_screen(
    kind: crate::ui::shell::modal::ModalKind,
    size: (u32, u32),
) -> crate::ui::shell::modal::MessageBoxLayout {
    let scale = shell_scale_for_screen(size);
    crate::ui::shell::modal::message_box_layout_on(kind, size.0 as i32, size.1 as i32, scale)
}

pub(crate) fn shell_scale_for_screen(size: (u32, u32)) -> crate::ui::shell::geom::ShellScale {
    use crate::ui::shell::geom::ShellScale;
    if window_takes_fitted_shell(size) {
        ShellScale::from_screen(size.0 as i32, size.1 as i32)
    } else {
        ShellScale::RETAIL
    }
}

/// The shared right-panel menu pages (`0x100` Single Player, `0x101` Movies &
/// Credits) and any other [`MenuPageSpec`]-driven page.
pub(crate) fn menu_page_layout(state: &AppState, spec: &MenuPageSpec) -> MenuPageLayout {
    let (width, height) = shell_screen_size(state);
    if fills_window(state) {
        menu_page::compute_responsive_layout(spec, width, height)
    } else {
        menu_page::compute_layout(spec, width, height)
    }
}

/// The skirmish dialog `0x102`, the page behind its modals.
pub(crate) fn skirmish_layout(state: &AppState) -> SkirmishShellLayout {
    let (width, height) = (state.render_width(), state.render_height());
    if fills_window(state) {
        crate::ui::skirmish_shell::compute_responsive_layout(width, height)
    } else {
        crate::ui::skirmish_shell::compute_layout(width, height)
    }
}

/// Choose Map `0x6B`, which replaces `0x102` while it is open.
pub(crate) fn choose_map_layout(state: &AppState) -> ChooseMapModalLayout {
    let (width, height) = (state.render_width(), state.render_height());
    if fills_window(state) {
        crate::ui::skirmish_shell::compute_responsive_choose_map_modal_layout(width, height)
    } else {
        crate::ui::skirmish_shell::compute_choose_map_modal_layout(width, height)
    }
}

/// Random-map setup `0x105`, which replaces `0x6B` while it is open.
pub(crate) fn random_map_setup_layout(state: &AppState) -> RandomMapSetupLayout {
    let (width, height) = (state.render_width(), state.render_height());
    if fills_window(state) {
        crate::ui::skirmish_shell::compute_responsive_random_map_setup_layout(width, height)
    } else {
        crate::ui::skirmish_shell::compute_random_map_setup_layout(width, height)
    }
}

/// The saved-seed browser (Load / Save / Delete), which covers `0x105`.
pub(crate) fn saved_seed_layout(state: &AppState, mode: SavedSeedMode) -> SavedSeedLayout {
    let (width, height) = (state.render_width(), state.render_height());
    if fills_window(state) {
        crate::ui::skirmish_shell::compute_responsive_saved_seed_layout(mode, width, height)
    } else {
        crate::ui::skirmish_shell::compute_saved_seed_layout(mode, width, height)
    }
}

/// Campaign selection `0x94`, the page behind Single Player's New Campaign.
pub(crate) fn campaign_layout(state: &AppState) -> crate::ui::campaign_shell::CampaignLayout {
    let (width, height) = shell_screen_size(state);
    if fills_window(state) {
        crate::ui::campaign_shell::compute_responsive_layout(width, height)
    } else {
        crate::ui::campaign_shell::compute_layout(width, height)
    }
}

/// Single Player's Load Saved Game `0xB7`.
pub(crate) fn load_saved_game_layout(
    state: &AppState,
) -> crate::ui::shell::saved_games::MainMenuSavedGameLayout {
    let (width, height) = shell_screen_size(state);
    if fills_window(state) {
        crate::ui::shell::saved_games::main_menu_saved_game_responsive_layout(width, height)
    } else {
        crate::ui::shell::saved_games::main_menu_saved_game_layout(width, height)
    }
}

/// Movie list `0x129`, the page behind Movies & Credits' Play Movies.
pub(crate) fn movie_list_layout(
    state: &AppState,
) -> crate::ui::movies_credits_shell::MovieListLayout {
    let (width, height) = shell_screen_size(state);
    if fills_window(state) {
        crate::ui::movies_credits_shell::compute_responsive_movie_list_layout(width, height)
    } else {
        crate::ui::movies_credits_shell::compute_movie_list_layout(width, height)
    }
}

/// End-of-match score screen `0x108`.
pub(crate) fn score_layout(state: &AppState) -> crate::ui::score_shell::ScoreLayout {
    let (width, height) = shell_screen_size(state);
    if fills_window(state) {
        crate::ui::score_shell::compute_responsive_layout(width, height)
    } else {
        crate::ui::score_shell::compute_layout(width, height)
    }
}

/// Launcher Options `0xD5`, showing `page`.
pub(crate) fn launcher_options_layout(
    state: &AppState,
    page: crate::ui::main_menu_dialogs::options::OptionsPage,
) -> crate::ui::main_menu_dialogs::options::shell::LauncherOptionsLayout {
    use crate::ui::main_menu_dialogs::options::shell::LauncherOptionsLayout;
    let (width, height) = (state.render_width() as i32, state.render_height() as i32);
    let layout = if fills_window(state) {
        LauncherOptionsLayout::responsive(width, height, page)
    } else {
        LauncherOptionsLayout::new(width, height, page)
    };
    let battle = crate::app::App::match_zoom_geometry(state);
    layout.with_battle_frame((battle.src_width, battle.src_height))
}

/// The width a page's art set is keyed on. A fitted page maps from the
/// authored 800x600, so it takes the art it would show there: the large score
/// SHP (gamemd swaps to the small one only at exactly 640) and the Westwood
/// Online background (loaded only at 800 wide by `0x0072C7E0`).
pub(crate) fn art_key_screen_width(state: &AppState) -> u32 {
    if fills_window(state) {
        crate::ui::shell::geom::SHELL_BASE_W as u32
    } else {
        state.renderer.gpu.config.width
    }
}

/// Westwood Online welcome `0x10E`.
pub(crate) fn wol_welcome_layout(state: &AppState) -> crate::ui::wol_shell::WolWelcomeLayout {
    let (width, height) = shell_screen_size(state);
    let spec = state
        .frontend
        .wol
        .as_ref()
        .map_or(&crate::ui::wol_shell::WOL_WELCOME_PAGE, |visit| {
            visit.welcome_page_spec()
        });
    if fills_window(state) {
        crate::ui::wol_shell::compute_responsive_layout(width, height, spec)
    } else {
        crate::ui::wol_shell::compute_layout(width, height, spec)
    }
}

/// Keyboard `0xA3`. Only the front-end family page stretches: the same dialog
/// opened from the in-game menu sits inside the tactical shell's own frame, so it
/// keeps the centered layout and the pixel space its frame was measured in.
pub(crate) fn keyboard_layout(
    state: &AppState,
    width: i32,
    height: i32,
    shell: Option<(crate::ui::shell::in_game_shell::InGameShellLayout, [i32; 2])>,
) -> crate::ui::shell::keyboard::KeyboardLayout {
    match (fills_window(state), shell) {
        (true, None) => crate::ui::shell::keyboard::KeyboardLayout::responsive(width, height),
        _ => crate::ui::shell::keyboard::KeyboardLayout::new(width, height, shell),
    }
}

#[cfg(test)]
mod tests {
    use super::window_takes_fitted_shell;

    #[test]
    fn exit_message_box_follows_fitted_menu_and_keeps_button_hits() {
        use crate::ui::shell::geom::RectPx;
        use crate::ui::shell::modal::ModalKind;
        let layout = super::message_box_layout_for_screen(ModalKind::Confirm, (2560, 1600));
        assert_eq!(layout.dialog, RectPx::new(680, 365, 1202, 870));
        assert_eq!(layout.ok, RectPx::new(1509, 1037, 336, 67));
        let cancel = layout.cancel.unwrap();
        assert_eq!(cancel, RectPx::new(1509, 1123, 336, 66));
        assert!(layout.ok.contains(1673, 1071));
        assert!(cancel.contains(1673, 1156));
        assert!(!layout.ok.contains(1673, 1156));
        assert!(!cancel.contains(1673, 1071));
        for size in [(800, 600), (640, 480)] {
            let layout = super::message_box_layout_for_screen(ModalKind::Confirm, size);
            assert_eq!((layout.dialog.w, layout.dialog.h), (451, 326));
            assert!(layout.scale.is_retail());
        }
    }

    #[test]
    fn only_the_retail_shell_sizes_keep_the_retail_layout() {
        assert!(!window_takes_fitted_shell((800, 600)));
        assert!(!window_takes_fitted_shell((640, 480)));
        for size in [
            (1024, 768),
            (1600, 1200),
            (3440, 1440),
            (800, 601),
            (640, 600),
        ] {
            assert!(window_takes_fitted_shell(size), "{size:?}");
        }
    }
}
