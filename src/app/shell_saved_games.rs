//! B5's saved-game children. Original558DD0 owns the shared modal transaction;
//! the VERA repository and PreparedLoad remain the save/restore authorities.

use super::{App, AppState};
use crate::map::rmg::SeedDescription;
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::saved_file_input::{self, BrowserInputResult};
use crate::ui::skirmish_shell::seed_list::SeedListGeometry;
use crate::ui::skirmish_shell::{
    SavedSeedLayout, SavedSeedMode, SavedSeedOutcome, SavedSeedPrompt, SavedSeedPromptPurpose,
};
use std::path::PathBuf;
use winit::keyboard::KeyCode;

impl App {
    fn saved_game_layout(state: &AppState, mode: SavedSeedMode) -> Option<SavedSeedLayout> {
        let (shell, size) =
            crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(state)?;
        Some(crate::ui::shell::saved_games::saved_game_layout(
            mode,
            state.renderer.gpu.config.width as i32,
            state.renderer.gpu.config.height as i32,
            shell,
            size,
        ))
    }

    pub(crate) fn open_saved_game_browser(state: &mut AppState, mode: SavedSeedMode) {
        let Some(layout) = Self::saved_game_layout(state, mode) else {
            return;
        };
        let browser = Self::open_saved_game_rows(
            state,
            mode,
            SeedListGeometry::new(layout.list, 0, 0).visible_rows,
        );
        // The child replaces its hidden parent and owns all subsequent input.
        state.match_state.match_presentation.show_save_load_panel = false;
        state.match_state.match_presentation.saved_game_browser = Some(browser);
        Self::enter_in_game_menu_state(state, InGameMenuState::SavedGame(mode));
    }

    fn close_saved_game_browser(state: &mut AppState) {
        state.match_state.match_presentation.saved_game_browser = None;
        Self::enter_in_game_menu_state(state, InGameMenuState::Menu);
    }

    fn show_saved_game_prompt(
        state: &mut AppState,
        purpose: SavedSeedPromptPurpose<PathBuf>,
        body: String,
        confirm: bool,
    ) {
        let affirmative = Self::csf_label(
            state,
            if confirm { "TXT_YES" } else { "TXT_OK" },
            if confirm { "Yes" } else { "OK" },
        );
        let negative = confirm.then(|| Self::csf_label(state, "TXT_NO", "No"));
        if let Some(browser) = state
            .match_state
            .match_presentation
            .saved_game_browser
            .as_mut()
        {
            browser.description_edit.focused = false;
            browser.pressed_control = None;
            browser.scroll_repeat_at = None;
            browser.prompt = Some(SavedSeedPrompt {
                purpose,
                body,
                affirmative,
                negative,
            });
        }
    }

    fn finish_saved_game_write(
        state: &mut AppState,
        path: Option<PathBuf>,
        description: SeedDescription,
    ) {
        match crate::app::persistence::commands::save_game(
            state,
            &description.display_text(),
            path.as_deref(),
        ) {
            Ok(_) => {
                let body = Self::csf_label(state, "TXT_GAME_WAS_SAVED", "The game was saved.");
                Self::show_saved_game_prompt(state, SavedSeedPromptPurpose::Saved, body, false);
            }
            Err(error) => {
                log::warn!("Save browser: {error}");
                let body = Self::csf_label(state, "TXT_ERROR_SAVING_GAME", "Error saving game.");
                Self::show_saved_game_prompt(state, SavedSeedPromptPurpose::Error, body, false);
            }
        }
    }

    fn resolve_saved_game_prompt(state: &mut AppState, affirmative: bool) {
        let prompt = state
            .match_state
            .match_presentation
            .saved_game_browser
            .as_mut()
            .and_then(|browser| {
                browser.pressed_control = None;
                browser.prompt.take()
            });
        let Some(prompt) = prompt else {
            return;
        };
        match prompt.purpose {
            SavedSeedPromptPurpose::EmptyDescription => {
                if let Some(browser) = state
                    .match_state
                    .match_presentation
                    .saved_game_browser
                    .as_mut()
                {
                    browser.description_edit.focused = true;
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
                state.persistence.invalidate_save_list();
                let rows = Self::saved_game_layout(state, SavedSeedMode::Delete)
                    .map(|layout| SeedListGeometry::new(layout.list, 0, 0).visible_rows)
                    .unwrap_or(1);
                let empty = state
                    .match_state
                    .match_presentation
                    .saved_game_browser
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
                    let body = Self::csf_label(
                        state,
                        "TXT_MUSTENTER_DESCRIPTION",
                        "You must enter a description!",
                    );
                    Self::show_saved_game_prompt(
                        state,
                        SavedSeedPromptPurpose::EmptyDescription,
                        body,
                        false,
                    );
                } else if let Some(file_name) =
                    file_name.filter(|path| state.persistence.repository.exists(path))
                {
                    let body =
                        Self::csf_label(state, "TXT_CONFIRM_SAVE", "Overwrite existing save game?");
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
                match crate::app::persistence::commands::try_load_save_file(state, &path) {
                    Ok(()) => {
                        state.match_state.match_presentation.saved_game_browser = None;
                        Self::enter_in_game_menu_state(state, InGameMenuState::Closed);
                    }
                    Err(error) => {
                        log::warn!("Load browser: {}: {error}", path.display());
                        let body =
                            Self::csf_label(state, "TXT_ERROR_LOADING_GAME", "Error loading game.");
                        Self::show_saved_game_prompt(
                            state,
                            SavedSeedPromptPurpose::Error,
                            body,
                            false,
                        );
                    }
                }
            }
            SavedSeedOutcome::Delete(file_name) => {
                let prefix = Self::csf_label(state, "TXT_DELETE_FILE_QUERY", "Delete this file?");
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

    fn apply_saved_game_input(state: &mut AppState, result: BrowserInputResult<PathBuf>) {
        match result {
            BrowserInputResult::None => {}
            BrowserInputResult::Outcome(outcome) => Self::apply_saved_game_outcome(state, outcome),
            BrowserInputResult::PromptAnswer(answer) => {
                Self::resolve_saved_game_prompt(state, answer)
            }
            BrowserInputResult::ButtonPressed => {
                Self::play_skirmish_shell_generic_click_sound(state)
            }
        }
    }

    pub(crate) fn saved_game_key(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
        if let Some(browser) = state
            .match_state
            .match_presentation
            .saved_game_browser
            .as_mut()
        {
            let result = saved_file_input::key(browser, code, text);
            Self::apply_saved_game_input(state, result);
        }
    }

    pub(crate) fn saved_game_mouse(state: &mut AppState, pressed: bool) {
        let Some(mode) = state
            .match_state
            .match_presentation
            .saved_game_browser
            .as_ref()
            .map(|browser| browser.mode)
        else {
            return;
        };
        let Some(layout) = Self::saved_game_layout(state, mode) else {
            return;
        };
        let extent = (
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        );
        let (x, y) = state.window_cursor_position();
        let pointer = (x.round() as i32, y.round() as i32);
        let browser = state
            .match_state
            .match_presentation
            .saved_game_browser
            .as_mut()
            .unwrap();
        let result = if pressed {
            saved_file_input::mouse_down(
                browser,
                &layout,
                extent,
                pointer,
                std::time::Instant::now(),
                saved_file_input::host_double_click_limits(),
            )
        } else {
            saved_file_input::mouse_up(browser, &layout, extent, pointer)
        };
        Self::apply_saved_game_input(state, result);
    }

    pub(crate) fn update_saved_game_browser(state: &mut AppState, pointer_moved: bool) {
        let Some(mode) = state
            .match_state
            .match_presentation
            .saved_game_browser
            .as_ref()
            .map(|browser| browser.mode)
        else {
            return;
        };
        let Some(layout) = Self::saved_game_layout(state, mode) else {
            return;
        };
        let (_, y) = state.window_cursor_position();
        let browser = state
            .match_state
            .match_presentation
            .saved_game_browser
            .as_mut()
            .unwrap();
        if !saved_file_input::update_scroll(
            browser,
            &layout,
            y.round() as i32,
            pointer_moved,
            std::time::Instant::now(),
        ) {
            return;
        }
        if let Some(rect) = layout.name_edit {
            let available =
                (crate::ui::skirmish_shell::player_name_edit_text_rect(rect).w - 2).max(0) as u32;
            let edit = &mut browser.description_edit;
            edit.first_visible_unit = edit.first_visible_unit.min(edit.caret);
            while edit.first_visible_unit < edit.caret {
                let text =
                    String::from_utf16_lossy(&edit.units[edit.first_visible_unit..edit.caret]);
                if state.renderer.bit_font.text_width(&text) <= available {
                    break;
                }
                edit.first_visible_unit += 1;
            }
        }
    }
}
