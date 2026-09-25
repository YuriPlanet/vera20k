//! Single Player's Load Saved Game: dialog `0xB7` in Load mode, opened by
//! `Main__PrepareSession` state 9 (`0x0052E0BE`) after `0x100`'s teardown.
//! The shared saved-game loop `0x00558DD0` owns it; outside a suspended game
//! it is a family page (right panel, heading, status line, slides). Back
//! (result 2) tears it down and state 1 recreates Single Player.

use std::path::PathBuf;

use winit::keyboard::KeyCode;

use super::{App, AppState};
use crate::app::shell_route::ShellRoute;
use crate::ui::shell::saved_file_input::{self, BrowserInputResult};
use crate::ui::shell::saved_games::{
    LOAD_SAVED_GAME_PAGE, MainMenuSavedGameLayout, main_menu_saved_game_layout,
};
use crate::ui::skirmish_shell::seed_list::SeedListGeometry;
use crate::ui::skirmish_shell::{
    SavedSeedBrowserState, SavedSeedMode, SavedSeedOutcome, SavedSeedPrompt, SavedSeedPromptPurpose,
};

impl App {
    /// State 9: `0x00558F2E` creates `0xB7` with the rows `0x005596A0`
    /// scanned, and `0x00558FC8..0x00558FE5` enables Load from the row count.
    pub(super) fn open_load_saved_game_page(state: &mut AppState) {
        Self::enter_shell_window_mode(state);
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
        // The error message box paints from the skirmish chrome atlas.
        let _ = Self::ensure_skirmish_shell_chrome(state);
        let layout = Self::load_saved_game_layout(state);
        let rows = SeedListGeometry::new(layout.browser.list, 0, 0).visible_rows;
        let browser = Self::open_saved_game_rows(state, SavedSeedMode::Load, rows);
        state.frontend.load_saved_game = Some(browser);
        state.frontend.shell_route = ShellRoute::LoadSavedGame;
        state
            .frontend
            .shell_controller
            .reset_to(LOAD_SAVED_GAME_PAGE.dialog, false);
    }

    /// Back's teardown has run: state 9 returns false and state 1 recreates
    /// Single Player (`0x0052E0F9`).
    pub(super) fn commit_load_saved_game_back(state: &mut AppState) {
        state.frontend.load_saved_game = None;
        Self::open_single_player_shell(state);
    }

    pub(super) fn load_saved_game_active(state: &AppState) -> bool {
        state.frontend.screen == crate::ui::game_screen::GameScreen::MainMenu
            && state.frontend.shell_route.load_saved_game()
            && state.frontend.load_saved_game.is_some()
    }

    pub(crate) fn load_saved_game_layout(state: &AppState) -> MainMenuSavedGameLayout {
        main_menu_saved_game_layout(
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        )
    }

    fn load_saved_game_pointer(state: &AppState) -> (i32, i32) {
        (
            state.match_state.input.cursor_x.round() as i32,
            state.match_state.input.cursor_y.round() as i32,
        )
    }

    pub(super) fn handle_load_saved_game_mouse(state: &mut AppState, pressed: bool) {
        let layout = Self::load_saved_game_layout(state);
        let extent = (
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        );
        let pointer = Self::load_saved_game_pointer(state);
        let Some(browser) = state.frontend.load_saved_game.as_mut() else {
            return;
        };
        let result = if pressed {
            saved_file_input::mouse_down(
                browser,
                &layout.browser,
                extent,
                pointer,
                std::time::Instant::now(),
                saved_file_input::host_double_click_limits(),
            )
        } else {
            saved_file_input::mouse_up(browser, &layout.browser, extent, pointer)
        };
        Self::apply_load_saved_game_input(state, result);
    }

    pub(super) fn handle_load_saved_game_mouse_move(state: &mut AppState) {
        Self::update_load_saved_game_scroll(state, true);
        // Every hover message repaints the status line (0x00615EF7).
        state.frontend.shell_status_line.hover_repaint();
        state.platform.window.request_redraw();
    }

    /// Enter and Escape reach the proc as IDOK/IDCANCEL, which it ignores;
    /// they answer the error message box while it is up.
    pub(super) fn handle_load_saved_game_key(state: &mut AppState, code: KeyCode) {
        if let Some(browser) = state.frontend.load_saved_game.as_mut() {
            let result = saved_file_input::key(browser, Some(code), None);
            Self::apply_load_saved_game_input(state, result);
        }
    }

    /// Scrollbar drag and arrow repeat (`0x0061C690`).
    pub(super) fn update_load_saved_game_scroll(state: &mut AppState, pointer_moved: bool) {
        if !Self::load_saved_game_active(state) {
            return;
        }
        let layout = Self::load_saved_game_layout(state);
        let (_, y) = Self::load_saved_game_pointer(state);
        if let Some(browser) = state.frontend.load_saved_game.as_mut()
            && saved_file_input::update_scroll(
                browser,
                &layout.browser,
                y,
                pointer_moved,
                std::time::Instant::now(),
            )
        {
            state.platform.window.request_redraw();
        }
    }

    fn apply_load_saved_game_input(state: &mut AppState, result: BrowserInputResult<PathBuf>) {
        match result {
            BrowserInputResult::None => {}
            // Owner-draw buttons play GUIMainButtonSound on the press
            // (0x00613667..0x00613771).
            BrowserInputResult::ButtonPressed => Self::play_main_menu_button_sound(state),
            BrowserInputResult::PromptAnswer(_) => {
                if let Some(browser) = state.frontend.load_saved_game.as_mut() {
                    browser.pressed_control = None;
                    browser.prompt = None;
                }
            }
            BrowserInputResult::Outcome(SavedSeedOutcome::Close) => Self::leave_shell_dialog(
                state,
                crate::app::frontend::shell_transition::ShellExitThen::LoadSavedGameBack,
            ),
            BrowserInputResult::Outcome(SavedSeedOutcome::Load(path)) => {
                Self::load_saved_game_from_menu(state, &path)
            }
            BrowserInputResult::Outcome(_) => {}
        }
    }

    /// Load (`0x00559051`): retail hides `0xB7`, shows the Loading panel
    /// `0xF0` and runs `Load_Game` `0x0067E440`, entering the game on success.
    /// VERA20k restores saves only into a running scenario, so a load from
    /// the main menu reports the failure message box
    /// (`0x00559539..0x00559599`) and keeps the dialog up; retail re-hides it
    /// after the box.
    fn load_saved_game_from_menu(state: &mut AppState, path: &std::path::Path) {
        log::warn!(
            "Load Saved Game: {}: saved games cannot be started from the main menu yet",
            path.display()
        );
        let body = Self::csf_label(state, "TXT_ERROR_LOADING_GAME", "Error loading game.");
        let affirmative = Self::csf_label(state, "TXT_OK", "OK");
        if let Some(browser) = state.frontend.load_saved_game.as_mut() {
            browser.pressed_control = None;
            browser.scroll_repeat_at = None;
            browser.prompt = Some(SavedSeedPrompt {
                purpose: SavedSeedPromptPurpose::Error,
                body,
                affirmative,
                negative: None,
            });
        }
    }

    /// Status help of the control under the cursor (`0x006040B0`): the list,
    /// Load or Back, enabled or not.
    pub(crate) fn load_saved_game_status_key(state: &AppState) -> Option<&'static str> {
        use crate::ui::skirmish_shell::SavedSeedControl;
        let browser = state.frontend.load_saved_game.as_ref()?;
        if browser.prompt.is_some() {
            return None;
        }
        let layout = Self::load_saved_game_layout(state);
        let (x, y) = Self::load_saved_game_pointer(state);
        match crate::ui::skirmish_shell::saved_seed_control_at(&layout.browser, x, y)? {
            SavedSeedControl::List => Some(crate::ui::shell::saved_games::LOAD_LIST_HELP_KEY),
            SavedSeedControl::Action => Some(LOAD_SAVED_GAME_PAGE.stacked[0].tooltip_key),
            SavedSeedControl::Back0x686 => Some(LOAD_SAVED_GAME_PAGE.back.tooltip_key),
            _ => None,
        }
    }

    /// Rows for a saved-game browser: every readable save with its header
    /// description, newest first (`0x005596A0`).
    pub(super) fn open_saved_game_rows(
        state: &AppState,
        mode: SavedSeedMode,
        visible_rows: usize,
    ) -> SavedSeedBrowserState<PathBuf> {
        let entries = state
            .persistence
            .repository
            .browser_entries()
            .into_iter()
            .map(
                |(entry, last_write_time)| crate::ui::skirmish_shell::SavedSeedBrowserRow {
                    file_name: Some(entry.path),
                    description: entry.header.description.into(),
                    last_write_time,
                    visible: true,
                },
            )
            .collect();
        // SetDefaults683A6D..683A9F keeps this localized default for mode5;
        // the campaign-only map Name/UIName override does not run here.
        let current = Self::csf_label(state, "GUI:SkirmishGame", "Skirmish Game");
        let empty = Self::csf_label(state, "TXT_EMPTY_SLOT", "[EMPTY SLOT]");
        SavedSeedBrowserState::open_rows(
            mode,
            entries,
            current.into(),
            empty.into(),
            crate::map::rmg::saved_seeds::new_slot_file_time(std::time::SystemTime::now()),
            visible_rows,
        )
    }
}
