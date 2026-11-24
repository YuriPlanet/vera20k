//! B5's saved-game children. Original558DD0 owns the shared modal transaction;
//! the VERA repository and PreparedLoad remain the save/restore authorities.

use super::shell_saved_seeds::saved_browser_rows;
use super::{App, AppState};
use crate::map::rmg::SeedDescription;
use crate::ui::game_screen::GameScreen;
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::saved_file_input::{self, BrowserInputResult};
use crate::ui::skirmish_shell::{
    SavedSeedBrowserState, SavedSeedLayout, SavedSeedMode, SavedSeedOutcome, SavedSeedPromptPurpose,
};
use std::path::PathBuf;
use winit::keyboard::KeyCode;

impl App {
    pub(super) fn saved_game_layout(
        state: &AppState,
        mode: SavedSeedMode,
    ) -> Option<SavedSeedLayout> {
        let shell =
            crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(state)?;
        Some(crate::ui::shell::saved_games::saved_game_layout(
            mode,
            shell.width,
            shell.height,
            shell.layout,
            shell.button_size,
        ))
    }

    pub(crate) fn open_saved_game_browser(state: &mut AppState, mode: SavedSeedMode) {
        let Some(layout) = Self::saved_game_layout(state, mode) else {
            return;
        };
        let browser = Self::open_saved_game_rows(state, mode, saved_browser_rows(&layout), None);
        // The child replaces its hidden parent and owns all subsequent input.
        state.match_state.match_presentation.saved_game_browser = Some(browser);
        Self::enter_in_game_menu_state(state, InGameMenuState::SavedGame(mode));
    }

    fn saved_game_browser_slot(
        state: &mut AppState,
    ) -> &mut Option<SavedSeedBrowserState<PathBuf>> {
        if state.frontend.screen == GameScreen::MainMenu {
            &mut state.frontend.load_saved_game
        } else {
            &mut state.match_state.match_presentation.saved_game_browser
        }
    }

    fn saved_game_browser_rows(state: &AppState, mode: SavedSeedMode) -> usize {
        if state.frontend.screen == GameScreen::MainMenu {
            let layout =
                crate::app::frontend::shell_layout::load_saved_game_layout_for_mode(state, mode);
            saved_browser_rows(&layout.browser)
        } else {
            Self::saved_game_layout(state, mode)
                .as_ref()
                .map_or(1, saved_browser_rows)
        }
    }

    fn replace_saved_game_browser(state: &mut AppState, browser: SavedSeedBrowserState<PathBuf>) {
        if state.frontend.screen == GameScreen::MainMenu {
            let title = state.csf_text(browser.game_title_label().0);
            state
                .frontend
                .shell_page_title
                .set_text(&title, crate::util::time::Instant::now());
        }
        *Self::saved_game_browser_slot(state) = Some(browser);
        if state.frontend.screen == GameScreen::MainMenu {
            Self::handle_load_saved_game_mouse_move(state);
        }
    }

    fn return_to_saved_game_list(state: &mut AppState, selection: PathBuf) {
        let rows = Self::saved_game_browser_rows(state, SavedSeedMode::Load);
        let mut browser = Self::open_saved_game_rows(state, SavedSeedMode::Load, rows, None);
        browser.select_file(&selection, rows);
        Self::replace_saved_game_browser(state, browser);
    }

    fn close_saved_game_browser(state: &mut AppState) {
        if let Some(source) = Self::saved_game_browser_slot(state)
            .as_ref()
            .and_then(|b| b.conversion_source())
            .cloned()
        {
            Self::return_to_saved_game_list(state, source);
        } else if state.frontend.screen == GameScreen::MainMenu {
            let slide = crate::app::frontend::shell_transition::ShellSlideKind::LoadSavedGame;
            Self::leave_shell_dialog(state, slide, App::commit_load_saved_game_back);
        } else {
            state.match_state.match_presentation.saved_game_browser = None;
            Self::enter_in_game_menu_state(state, InGameMenuState::Menu);
        }
    }

    pub(super) fn show_saved_game_prompt(
        state: &mut AppState,
        purpose: SavedSeedPromptPurpose<PathBuf>,
        body: String,
        confirm: bool,
    ) {
        let prompt = Self::saved_browser_prompt(state, purpose, body, confirm);
        if let Some(browser) = Self::saved_game_browser_slot(state).as_mut() {
            browser.show_prompt(prompt);
        }
    }

    fn finish_saved_game_write(
        state: &mut AppState,
        path: Option<PathBuf>,
        description: SeedDescription,
    ) {
        if let Some(source) = Self::saved_game_browser_slot(state)
            .as_ref()
            .and_then(|b| b.conversion_source())
            .cloned()
        {
            match state.persistence.repository.migrate_to_slot(
                &source,
                &description.display_text(),
                path.as_deref(),
            ) {
                Ok(output) => Self::return_to_saved_game_list(state, output),
                Err(error) => {
                    log::warn!("Convert save {}: {error}", source.display());
                    Self::show_saved_game_prompt(
                        state,
                        SavedSeedPromptPurpose::Error,
                        crate::i18n::tr(error.message_key()),
                        false,
                    );
                }
            }
            return;
        }
        match crate::app::persistence::commands::save_game(
            state,
            &description.display_text(),
            path.as_deref(),
        ) {
            Ok(_) => {
                let body = state.csf_label("TXT_GAME_WAS_SAVED", "The game was saved.");
                Self::show_saved_game_prompt(state, SavedSeedPromptPurpose::Saved, body, false);
            }
            Err(error) => {
                log::warn!("Save browser: {error}");
                let body = state.csf_label("TXT_ERROR_SAVING_GAME", "Error saving game.");
                Self::show_saved_game_prompt(state, SavedSeedPromptPurpose::Error, body, false);
            }
        }
    }

    fn resolve_saved_game_prompt(state: &mut AppState, affirmative: bool) {
        let Some(prompt) = Self::saved_game_browser_slot(state)
            .as_mut()
            .and_then(|browser| browser.take_prompt())
        else {
            return;
        };
        match prompt.purpose {
            SavedSeedPromptPurpose::EmptyDescription => {
                if let Some(browser) = Self::saved_game_browser_slot(state).as_mut() {
                    browser.focus_edit();
                }
            }
            SavedSeedPromptPurpose::Saved => Self::close_saved_game_browser(state),
            SavedSeedPromptPurpose::Overwrite {
                file_name,
                description,
            } if affirmative => Self::finish_saved_game_write(state, Some(file_name), description),
            SavedSeedPromptPurpose::Delete { file_name } if affirmative => {
                // Original559140 ignores DeleteFileA's result and removes the
                // row. Re-entry re-enumerates the actual repository contents.
                if let Err(error) = state.persistence.repository.delete(&file_name) {
                    log::warn!("Delete browser: {}: {error}", file_name.display());
                }
                let layout = Self::saved_game_layout(state, SavedSeedMode::Delete);
                let rows = layout.as_ref().map_or(1, saved_browser_rows);
                let empty = Self::saved_game_browser_slot(state)
                    .as_mut()
                    .is_some_and(|browser| {
                        browser.remove_entry(&file_name, rows);
                        browser.entries.is_empty()
                    });
                if empty {
                    Self::close_saved_game_browser(state);
                }
            }
            _ => {}
        }
    }

    fn apply_saved_game_outcome(state: &mut AppState, outcome: SavedSeedOutcome<PathBuf>) {
        match outcome {
            SavedSeedOutcome::Close => Self::close_saved_game_browser(state),
            SavedSeedOutcome::Save {
                file_name,
                description,
            } => {
                if description.is_empty() {
                    let body = state
                        .csf_label("TXT_MUSTENTER_DESCRIPTION", "You must enter a description!");
                    Self::show_saved_game_prompt(
                        state,
                        SavedSeedPromptPurpose::EmptyDescription,
                        body,
                        false,
                    );
                } else if let Some(file_name) = file_name.filter(|path| {
                    Self::saved_game_browser_slot(state)
                        .as_ref()
                        .is_some_and(|b| b.is_naming_conversion())
                        || state.persistence.repository.exists(path)
                }) {
                    let body = state.csf_label("TXT_CONFIRM_SAVE", "Overwrite existing save game?");
                    Self::show_saved_game_prompt(
                        state,
                        SavedSeedPromptPurpose::Overwrite {
                            file_name,
                            description,
                        },
                        body,
                        true,
                    );
                } else {
                    Self::finish_saved_game_write(state, None, description);
                }
            }
            SavedSeedOutcome::Load(path) => {
                let loadable = Self::saved_game_browser_slot(state)
                    .as_ref()
                    .and_then(|browser| browser.selected_entry())
                    .is_none_or(|row| row.loadable);
                if loadable {
                    crate::app::persistence::commands::request_saved_game_load(state, path);
                } else {
                    Self::show_saved_game_prompt(
                        state,
                        SavedSeedPromptPurpose::Error,
                        crate::i18n::tr("VERA:SaveFromOtherVersionText"),
                        false,
                    );
                }
            }
            SavedSeedOutcome::BeginConvert(source) => {
                let rows = Self::saved_game_browser_rows(state, SavedSeedMode::Save);
                let browser =
                    Self::open_saved_game_rows(state, SavedSeedMode::Save, rows, Some(source));
                Self::replace_saved_game_browser(state, browser);
            }
            SavedSeedOutcome::Delete(file_name) => {
                let prefix = state.csf_label("TXT_DELETE_FILE_QUERY", "Delete this file?");
                let description = state
                    .match_state
                    .match_presentation
                    .saved_game_browser
                    .as_ref()
                    .and_then(|browser| browser.selected_entry())
                    .map(|entry| entry.description.display_text())
                    .unwrap_or_default();
                Self::show_saved_game_prompt(
                    state,
                    SavedSeedPromptPurpose::Delete { file_name },
                    format!("{prefix}\n\n{description}"),
                    true,
                );
            }
        }
    }

    pub(crate) fn finish_saved_game_load(
        state: &mut AppState,
        path: &std::path::Path,
        result: Result<(), crate::app::persistence::PreparedLoadError>,
    ) {
        match result {
            Ok(()) => {
                state.match_state.match_presentation.saved_game_browser = None;
                Self::enter_in_game_menu_state(state, InGameMenuState::Closed);
            }
            Err(error) => {
                log::warn!("Load browser: {}: {error}", path.display());
                let body = state.csf_label("TXT_ERROR_LOADING_GAME", "Error loading game.");
                Self::show_saved_game_prompt(state, SavedSeedPromptPurpose::Error, body, false);
            }
        }
    }

    pub(super) fn apply_saved_game_input(
        state: &mut AppState,
        results: Vec<BrowserInputResult<PathBuf>>,
    ) {
        for result in results {
            match result {
                BrowserInputResult::Outcome(outcome) => {
                    Self::apply_saved_game_outcome(state, outcome)
                }
                BrowserInputResult::PromptAnswer(answer) => {
                    Self::resolve_saved_game_prompt(state, answer);
                    if state.frontend.screen == GameScreen::MainMenu {
                        Self::handle_load_saved_game_mouse_move(state);
                    }
                }
                // B7/2B4/2B5's action and Back are owner-draw (0x0061376B).
                BrowserInputResult::Cue(cue) => Self::play_page_cue(state, cue),
            }
        }
    }

    pub(crate) fn saved_game_key(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
        if let Some(browser) = Self::saved_game_browser_slot(state).as_mut() {
            let results = saved_file_input::key(browser, code, text);
            Self::apply_saved_game_input(state, results);
        }
    }

    /// One pointer message on the browser (`down`: `None` moves).
    fn saved_game_pointer(state: &mut AppState, down: Option<bool>) {
        let Some(mode) = state
            .match_state
            .match_presentation
            .saved_game_browser
            .as_ref()
            .map(|b| b.mode)
        else {
            return;
        };
        let Some(layout) = Self::saved_game_layout(state, mode) else {
            return;
        };
        let extent = (layout.screen.w as u32, layout.screen.h as u32);
        let pointer = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
        if let Some(browser) = Self::saved_game_browser_slot(state).as_mut() {
            let results = saved_file_input::pointer(browser, &layout, extent, pointer, down);
            Self::apply_saved_game_input(state, results);
        }
    }

    pub(crate) fn saved_game_mouse(state: &mut AppState, pressed: bool) {
        Self::saved_game_pointer(state, Some(pressed));
    }

    /// Each frame, and on a pointer move: the scroll repeat, a held thumb
    /// and the edit's scroll.
    pub(crate) fn update_saved_game_browser(state: &mut AppState, pointer_moved: bool) {
        if pointer_moved {
            Self::saved_game_pointer(state, None);
        }
        let Some(mode) = state
            .match_state
            .match_presentation
            .saved_game_browser
            .as_ref()
            .map(|b| b.mode)
        else {
            return;
        };
        let Some(layout) = Self::saved_game_layout(state, mode) else {
            return;
        };
        let extent = (layout.screen.w as u32, layout.screen.h as u32);
        let font = &state.renderer.bit_font;
        if let Some(browser) = state
            .match_state
            .match_presentation
            .saved_game_browser
            .as_mut()
        {
            saved_file_input::update(browser, &layout, extent, |text| font.text_width(text));
        }
    }
}
