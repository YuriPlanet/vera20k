//! The one place the shell family picks its layout: gamemd's centered,
//! unscaled 800x600 dialog, or the same dialog scaled to fit the live window
//! at its own shape ([`fills_window`]).
//!
//! Every shell route that both paints and hit-tests must take its rects from
//! here; a render path computing one layout and an input handler the other
//! misaligns clicks by the scale factor.

use crate::app::AppState;
use crate::app::shell_route::ShellRoute;
use crate::app::shell_wol::WolPage;
use crate::ui::main_menu_shell::{MainMenuShellLayout, compute_layout};
use crate::ui::movies_credits_shell;
use crate::ui::shell::geom::{ScaledRects, ShellScale, fit_window};
use crate::ui::shell::menu_page::{self, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::saved_games;
use crate::ui::skirmish_shell::{
    self as skirmish, ChooseMapModalLayout, RandomMapSetupLayout, SavedSeedLayout, SavedSeedMode,
    SkirmishShellLayout,
};

/// The window pixel space the shell paints and hit-tests in. The GPU surface
/// size, not the monitor's: a resized window repaints at the new size.
pub(crate) fn shell_screen_size(state: &AppState) -> (u32, u32) {
    let config = &state.renderer.gpu.config;
    (config.width, config.height)
}

/// Whether the shell family fits the live window instead of painting gamemd's
/// centered, unscaled dialog.
pub(crate) fn fills_window(state: &AppState) -> bool {
    window_takes_fitted_shell(shell_screen_size(state))
}

/// Whether a window of `size` gets the fitted shell
/// ([`shell_scale_for_window`](crate::ui::shell::geom::shell_scale_for_window)).
/// The startup splash asks by surface size, before the app state exists.
pub(crate) fn window_takes_fitted_shell(size: (u32, u32)) -> bool {
    !shell_scale_for_screen(size).is_unscaled()
}

/// The fitted shell sets the cursor scale. VERA's visible online lobby and
/// rooms use a compact cursor. In-game shell cursors follow the match zoom.
pub(crate) fn menu_cursor_scale(state: &AppState) -> f32 {
    if state.frontend.screen == crate::ui::game_screen::GameScreen::InGame {
        return 1.0;
    }
    let fitted = shell_scale_for_screen(shell_screen_size(state)).factor();
    if crate::app::input::precedence::modal_page(state).is_some() {
        return fitted;
    }

    let route = state.frontend.shell_route;
    if route == ShellRoute::WolLobby
        && let Some(visit) = state.frontend.wol.as_ref()
    {
        // The visit keeps the lobby route while a separate page or dialog owns the screen.
        let visible = visit
            .lobby()
            .is_some_and(|lobby| lobby.ui.is_modern() && lobby.shown && !lobby.covered);
        let dialog = visit.message().is_some()
            || visit.wait_box().is_some_and(|wait| wait.shown)
            || crate::app::shell_wol::page::page_dialog(state).is_some()
            || crate::app::shell_wol::page::page_received(state).is_some();
        return wol_lobby_cursor_scale(fitted, &visit.page, visible, dialog);
    }
    if route == ShellRoute::NetworkSetup {
        let dialog = crate::app::shell_network_setup::dialog(state);
        if dialog.wol() && dialog.modern {
            return fitted * 0.5;
        }
    }
    fitted
}

fn wol_lobby_cursor_scale(fitted: f32, page: &WolPage, visible: bool, dialog: bool) -> f32 {
    if matches!(page, WolPage::Lobby) && visible && !dialog {
        fitted * 0.5
    } else {
        fitted
    }
}

/// `retail` laid out at the window's size, or in a fitted window on the
/// authored 800x600 dialog and scaled to fit it ([`fit_window`]).
pub(crate) fn pick<L: ScaledRects>(state: &AppState, retail: impl FnOnce(u32, u32) -> L) -> L {
    pick_at(fills_window(state), shell_screen_size(state), retail)
}

/// [`pick`] at `size` (the render size, for the skirmish family).
fn pick_at<L: ScaledRects>(
    fitted: bool,
    size: (u32, u32),
    retail: impl FnOnce(u32, u32) -> L,
) -> L {
    if fitted {
        fit_window(size.0, size.1, retail)
    } else {
        retail(size.0, size.1)
    }
}

/// The skirmish family's [`pick`], laid out at the render size.
fn pick_render<L: ScaledRects>(state: &AppState, retail: impl FnOnce(u32, u32) -> L) -> L {
    let size = (state.render_width(), state.render_height());
    pick_at(fills_window(state), size, retail)
}

/// A layout built on the window's size and its shell mapping
/// ([`shell_scale_for_screen`]).
pub(crate) fn on<L>(state: &AppState, layout: impl FnOnce(i32, i32, ShellScale) -> L) -> L {
    let size = shell_screen_size(state);
    layout(size.0 as i32, size.1 as i32, shell_scale_for_screen(size))
}

/// Main menu dialog `0xE2`.
pub(crate) fn main_menu_layout(state: &AppState) -> MainMenuShellLayout {
    pick(state, compute_layout)
}

/// A front-end box's rects on the window's mapping.
pub(crate) fn shell_box_parts(
    state: &AppState,
    shell_box: &crate::ui::shell::modal::ShellBox,
) -> crate::ui::shell::modal::BoxParts {
    on(state, |w, h, scale| shell_box.parts(w, h, scale))
}

/// The settings in VERA's Esc menu, on its message box's mapping.
pub(crate) fn shell_menu_settings_layout(
    size: (u32, u32),
    font: &crate::render::bit_font::BitFont,
) -> crate::ui::main_menu_dialogs::options::shell::LauncherOptionsLayout {
    crate::ui::main_menu_dialogs::options::shell::LauncherOptionsLayout::esc_menu(
        size.0 as i32,
        size.1 as i32,
        shell_scale_for_screen(size),
        |caption| font.text_width(caption),
    )
}

pub(crate) fn shell_scale_for_screen(size: (u32, u32)) -> ShellScale {
    crate::ui::shell::geom::shell_scale_for_window(size.0 as i32, size.1 as i32)
}

/// The shared right-panel menu pages (`0x100` Single Player, `0x101` Movies &
/// Credits) and any other [`MenuPageSpec`]-driven page.
pub(crate) fn menu_page_layout(state: &AppState, spec: &MenuPageSpec) -> MenuPageLayout {
    pick(state, |w, h| menu_page::compute_layout(spec, w, h))
}

/// The skirmish dialog `0x102`, the page behind its modals, for the selected
/// map's player rows: VERA's longer list past eight. The unscaled 640x480
/// page has no room for it under its options, so there a longer list takes
/// the scaled 800x600 page every other window size already gets.
pub(crate) fn skirmish_layout(state: &AppState) -> SkirmishShellLayout {
    use skirmish::{SKIRMISH_ROW_COUNT, compute_layout_for_rows, selected_player_rows};
    let (width, height) = (state.render_width(), state.render_height());
    let rows = selected_player_rows(
        &state.frontend.skirmish_shell_state,
        state.frontend.scenario_catalog.shell_maps(),
    );
    let layout = compute_layout_for_rows(width, height, rows);
    if fills_window(state)
        || (layout.shown_rows > SKIRMISH_ROW_COUNT
            && layout.authored_width() < crate::ui::shell::geom::SHELL_BASE_W)
    {
        fit_window(width, height, |w, h| compute_layout_for_rows(w, h, rows))
    } else {
        layout
    }
}

/// Choose Map `0x6B`, which replaces `0x102` while it is open.
pub(crate) fn choose_map_layout(state: &AppState) -> ChooseMapModalLayout {
    pick_render(state, skirmish::compute_choose_map_modal_layout)
}

/// Random-map setup `0x105`, which replaces `0x6B` while it is open.
pub(crate) fn random_map_setup_layout(state: &AppState) -> RandomMapSetupLayout {
    pick_render(state, skirmish::compute_random_map_setup_layout)
}

/// The saved-seed browser (Load / Save / Delete), which covers `0x105`.
pub(crate) fn saved_seed_layout(state: &AppState, mode: SavedSeedMode) -> SavedSeedLayout {
    pick_render(state, |w, h| {
        skirmish::compute_saved_seed_layout(mode, w, h)
    })
}

/// VERA's Skirmish Setups page, on the Save browser's rects.
pub(crate) fn setups_layout(state: &AppState) -> SavedSeedLayout {
    pick_render(state, skirmish::compute_setups_layout)
}

/// Campaign selection `0x94`, the page behind Single Player's New Campaign.
pub(crate) fn campaign_layout(state: &AppState) -> crate::ui::campaign_shell::CampaignLayout {
    pick(state, crate::ui::campaign_shell::compute_layout)
}

/// Single Player's Load Saved Game `0xB7`.
pub(crate) fn load_saved_game_layout(state: &AppState) -> saved_games::MainMenuSavedGameLayout {
    pick(state, saved_games::main_menu_saved_game_layout)
}

/// Movie list `0x129`, the page behind Movies & Credits' Play Movies.
pub(crate) fn movie_list_layout(state: &AppState) -> movies_credits_shell::MovieListLayout {
    pick(state, movies_credits_shell::compute_movie_list_layout)
}

/// VERA's Mods page, behind the Mods button of Options.
pub(crate) fn mods_layout(state: &AppState) -> crate::ui::mods_shell::ModsLayout {
    pick(state, crate::ui::mods_shell::compute_layout)
}

/// End-of-match score screen `0x108`.
pub(crate) fn score_layout(state: &AppState) -> crate::ui::score_shell::ScoreLayout {
    pick(state, crate::ui::score_shell::compute_layout)
}

/// Launcher Options `0xD5`, showing `category`'s page.
pub(crate) fn launcher_options_layout(
    state: &AppState,
    category: crate::ui::main_menu_dialogs::options::Category,
) -> crate::ui::main_menu_dialogs::options::shell::LauncherOptionsLayout {
    use crate::ui::main_menu_dialogs::options::shell::LauncherOptionsLayout;
    let (width, height) = (state.render_width() as i32, state.render_height() as i32);
    let layout = if fills_window(state) {
        LauncherOptionsLayout::responsive(width, height, category)
    } else {
        LauncherOptionsLayout::new(width, height, category)
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
    pick(state, crate::ui::wol_shell::compute_layout)
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

#[cfg(all(test, tests_app_frontend))]
mod tests {
    use super::window_takes_fitted_shell;

    #[test]
    fn exit_message_box_follows_fitted_menu_and_keeps_button_hits() {
        use crate::ui::shell::geom::RectPx;
        let quit = crate::ui::shell::modal::ShellBox::message(
            "Q".into(),
            "OK".into(),
            "Cancel".into(),
            String::new(),
        );
        let parts = |(w, h): (u32, u32)| {
            quit.parts(w as i32, h as i32, super::shell_scale_for_screen((w, h)))
        };
        let layout = parts((2560, 1600));
        assert_eq!(layout.dialog, RectPx::new(456, 323, 1202, 869));
        let (ok, cancel) = (layout.button(0), layout.button(1));
        assert_eq!(ok, RectPx::new(1285, 995, 336, 66));
        assert_eq!(cancel, RectPx::new(1285, 1080, 336, 67));
        assert!(ok.contains(1453, 1028) && cancel.contains(1453, 1113));
        assert!(!ok.contains(1453, 1113) && !cancel.contains(1453, 1028));
        for size in [(800, 600), (640, 480)] {
            let layout = parts(size);
            assert_eq!((layout.dialog.w, layout.dialog.h), (451, 326));
            assert!(layout.scale.is_unscaled());
        }
    }

    #[test]
    fn only_the_visible_online_lobby_uses_the_compact_cursor() {
        use super::{WolPage, shell_scale_for_screen, wol_lobby_cursor_scale};
        use crate::app::shell_wol::options::WolOptionsPage;

        let options = WolPage::Options(Box::new(WolOptionsPage {
            shown: Default::default(),
            sound: None,
        }));
        let sound = WolPage::Options(Box::new(WolOptionsPage {
            shown: Default::default(),
            sound: Some(([0; 3], true)),
        }));
        for (size, full, compact) in [((800, 600), 1.0, 0.5), ((1200, 900), 1.5, 0.75)] {
            let fitted = shell_scale_for_screen(size).factor();
            assert_eq!(
                wol_lobby_cursor_scale(fitted, &WolPage::Lobby, true, false),
                compact
            );
            assert_eq!(
                wol_lobby_cursor_scale(fitted, &WolPage::Lobby, true, true),
                full
            );
            assert_eq!(
                wol_lobby_cursor_scale(fitted, &WolPage::Lobby, false, false),
                full
            );
            assert_eq!(wol_lobby_cursor_scale(fitted, &options, true, false), full);
            assert_eq!(wol_lobby_cursor_scale(fitted, &sound, true, false), full);
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
