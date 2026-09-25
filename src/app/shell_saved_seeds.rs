//! Saved-seed browser input and persistence transactions.
//! Native driver 0x00558DD0 owns modal lifetime; MapSeed slots 0x00597760,
//! 0x00597A30 and 0x00597D50 own persistence.

use super::shell_random_map::{RANDOM_MAP_DESCRIPTION_FALLBACK, RANDOM_MAP_DESCRIPTION_KEY};
use super::*;
use crate::map::rmg::{SeedDescription, saved_seeds};
use crate::ui::skirmish_shell::seed_list::SeedListGeometry;
use crate::ui::skirmish_shell::{SavedSeedOutcome, SavedSeedPrompt, SavedSeedPromptPurpose};
use crate::util::native_file_name::{self, NativeFileName};

use crate::ui::shell::saved_file_input::{self, BrowserInputResult};

impl App {
    pub(super) fn saved_seed_dir(state: &AppState) -> Option<std::path::PathBuf> {
        state
            .platform
            .game_config
            .as_ref()
            .map(|config| config.paths.ra2_dir.clone())
    }

    fn skirmish_saved_seed_layout(
        state: &AppState,
        mode: SavedSeedMode,
    ) -> crate::ui::skirmish_shell::SavedSeedLayout {
        crate::ui::skirmish_shell::compute_saved_seed_layout(
            mode,
            state.render_width(),
            state.render_height(),
        )
    }

    pub(super) fn open_saved_seed_browser(state: &mut AppState, mode: SavedSeedMode) {
        let entries = Self::saved_seed_dir(state)
            .map(|dir| saved_seeds::list_saved_seeds(&dir))
            .unwrap_or_default();
        let current = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_ref()
            .map(|modal| modal.options.description.clone())
            .unwrap_or_default();
        let empty = Self::csf_label(state, "TXT_EMPTY_SLOT", "[EMPTY SLOT]");
        let layout = Self::skirmish_saved_seed_layout(state, mode);
        let rows = SeedListGeometry::new(layout.list, 0, 0).visible_rows;
        state.frontend.skirmish_shell_state.saved_seed_browser = Some(SavedSeedBrowserState::open(
            mode,
            entries,
            current,
            empty.into(),
            saved_seeds::new_slot_file_time(std::time::SystemTime::now()),
            rows,
        ));
        state.frontend.skirmish_shell_state.player_name_edit.focused = false;
    }

    fn close_saved_seed_browser(state: &mut AppState) {
        state.frontend.skirmish_shell_state.saved_seed_browser = None;
        // Re-entry sees physical disk state, including failed native-style deletes.
        let available =
            Self::saved_seed_dir(state).is_some_and(|dir| saved_seeds::saved_seeds_available(&dir));
        if let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_mut()
        {
            modal.saved_seed_buttons_enabled = available;
        }
    }

    fn show_saved_seed_prompt(
        state: &mut AppState,
        purpose: SavedSeedPromptPurpose,
        body: String,
        confirm: bool,
    ) {
        let affirmative = if confirm {
            Self::csf_label(state, "TXT_YES", "Yes")
        } else {
            Self::csf_label(state, "TXT_OK", "OK")
        };
        let negative = confirm.then(|| Self::csf_label(state, "TXT_NO", "No"));
        if let Some(browser) = state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
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

    fn finish_saved_seed_write(
        state: &mut AppState,
        file_name: NativeFileName,
        description: SeedDescription,
    ) {
        let Some(dir) = Self::saved_seed_dir(state) else {
            return;
        };
        let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_mut()
        else {
            return;
        };
        // MapSeed Save 0x00597804 updates the working description before file I/O.
        // Its nonnull-filename path returns success even if the file could not open.
        modal.options.description = description;
        if let Err(error) = saved_seeds::write_browser_seed(&dir, &file_name, &modal.options) {
            log::warn!(
                "saved seed: native-style save acknowledgment despite I/O failure for {file_name}: {error}"
            );
        }
        let body = Self::csf_label(state, "GUI:MapSaved", "Map Saved");
        Self::show_saved_seed_prompt(state, SavedSeedPromptPurpose::Saved, body, false);
    }

    fn resolve_saved_seed_prompt(state: &mut AppState, affirmative: bool) {
        let prompt = state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
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
                // 0x00559269 explicitly restores the edit focus after this warning.
                if let Some(browser) = state
                    .frontend
                    .skirmish_shell_state
                    .saved_seed_browser
                    .as_mut()
                {
                    browser.description_edit.focused = true;
                }
            }
            SavedSeedPromptPurpose::Saved => Self::close_saved_seed_browser(state),
            SavedSeedPromptPurpose::Overwrite {
                file_name,
                description,
            } if affirmative => {
                Self::finish_saved_seed_write(state, file_name, description);
            }
            SavedSeedPromptPurpose::Delete { file_name } if affirmative => {
                if let Some(dir) = Self::saved_seed_dir(state) {
                    if let Err(error) = native_file_name::delete(&dir, &file_name) {
                        log::warn!("saved seed: could not delete {file_name}: {error}");
                    }
                }
                let layout = Self::skirmish_saved_seed_layout(state, SavedSeedMode::Delete);
                let rows = SeedListGeometry::new(layout.list, 0, 0).visible_rows;
                let empty = state
                    .frontend
                    .skirmish_shell_state
                    .saved_seed_browser
                    .as_mut()
                    .is_some_and(|browser| {
                        // driver 0x00558DD0 removes the row regardless of DeleteFileA's result.
                        browser.remove_entry(&file_name, rows);
                        browser.entries.is_empty()
                    });
                if empty {
                    Self::close_saved_seed_browser(state);
                }
            }
            _ => {} // Message-box dismissal focuses the parent dialog, not its old edit.
        }
    }

    fn apply_saved_seed_outcome(state: &mut AppState, outcome: SavedSeedOutcome) {
        let Some(dir) = Self::saved_seed_dir(state) else {
            return;
        };
        match outcome {
            SavedSeedOutcome::Close => Self::close_saved_seed_browser(state),
            SavedSeedOutcome::Load(file_name) => {
                let default = Self::csf_label(
                    state,
                    RANDOM_MAP_DESCRIPTION_KEY,
                    RANDOM_MAP_DESCRIPTION_FALLBACK,
                );
                let Some(modal) = state
                    .frontend
                    .skirmish_shell_state
                    .random_map_setup_modal
                    .as_mut()
                else {
                    return;
                };
                match saved_seeds::read_browser_seed(&dir, &file_name, &modal.options, &default) {
                    Ok(mut options) => {
                        // Load 0x00596963 posts Generate with its reroll latch clear.
                        // Ordinary valid control readback preserves loaded derived
                        // values, couples height to width, and stamps the default.
                        options.height = options.width;
                        options.description = default.into();
                        modal.options = options.clone();
                        modal.begin_generate();
                        Self::close_saved_seed_browser(state);
                        if !Self::start_random_map_generation(state, &options, false) {
                            if let Some(modal) = state
                                .frontend
                                .skirmish_shell_state
                                .random_map_setup_modal
                                .as_mut()
                            {
                                modal.fail_generate();
                            }
                        }
                    }
                    Err(error) => log::warn!("saved seed: could not read {file_name}: {error}"),
                }
            }
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
                    Self::show_saved_seed_prompt(
                        state,
                        SavedSeedPromptPurpose::EmptyDescription,
                        body,
                        false,
                    );
                    return;
                }
                let file_name = file_name.unwrap_or_else(|| {
                    let assets = state.process_assets.manager();
                    saved_seeds::allocate_seed_file_name(
                        &mut state.frontend.legacy_crt_rng,
                        |name| {
                            assets.is_some_and(|assets| assets.contains(name))
                                || native_file_name::available(&dir, &name.into())
                        },
                    )
                });
                if native_file_name::available(&dir, &file_name) {
                    let body =
                        Self::csf_label(state, "TXT_CONFIRM_SAVE", "Overwrite existing save game?");
                    Self::show_saved_seed_prompt(
                        state,
                        SavedSeedPromptPurpose::Overwrite {
                            file_name,
                            description,
                        },
                        body,
                        true,
                    );
                } else {
                    Self::finish_saved_seed_write(state, file_name, description);
                }
            }
            SavedSeedOutcome::Delete(file_name) => {
                let prefix = Self::csf_label(state, "TXT_DELETE_FILE_QUERY", "Delete this file?");
                let description = state
                    .frontend
                    .skirmish_shell_state
                    .saved_seed_browser
                    .as_ref()
                    .and_then(|browser| browser.selected_entry())
                    .map(|row| row.description.display_text())
                    .unwrap_or_default();
                Self::show_saved_seed_prompt(
                    state,
                    SavedSeedPromptPurpose::Delete { file_name },
                    format!("{prefix}\n\n{description}"),
                    true,
                );
            }
        }
    }

    fn apply_saved_seed_input(state: &mut AppState, result: BrowserInputResult<NativeFileName>) {
        match result {
            BrowserInputResult::None => {}
            BrowserInputResult::Outcome(outcome) => Self::apply_saved_seed_outcome(state, outcome),
            BrowserInputResult::PromptAnswer(answer) => {
                Self::resolve_saved_seed_prompt(state, answer)
            }
            BrowserInputResult::ButtonPressed => Self::play_main_menu_button_sound(state),
            BrowserInputResult::RowClicked => Self::play_skirmish_shell_generic_click_sound(state),
        }
    }

    pub(super) fn handle_saved_seed_browser_key(
        state: &mut AppState,
        code: Option<KeyCode>,
        text: Option<&str>,
    ) {
        let Some(browser) = state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
            .as_mut()
        else {
            return;
        };
        let result = saved_file_input::key(browser, code, text);
        Self::apply_saved_seed_input(state, result);
    }

    fn sync_saved_seed_edit_scroll(state: &mut AppState) {
        let layout = Self::skirmish_saved_seed_layout(state, SavedSeedMode::Save);
        let Some(rect) = layout.name_edit else {
            return;
        };
        let available =
            (crate::ui::skirmish_shell::player_name_edit_text_rect(rect).w - 2).max(0) as u32;
        let Some(browser) = state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
            .as_mut()
        else {
            return;
        };
        let edit = &mut browser.description_edit;
        edit.first_visible_unit = edit.first_visible_unit.min(edit.caret);
        while edit.first_visible_unit < edit.caret {
            let prefix = String::from_utf16_lossy(&edit.units[edit.first_visible_unit..edit.caret]);
            if state.renderer.bit_font.text_width(&prefix) <= available {
                break;
            }
            edit.first_visible_unit += 1;
        }
    }

    pub(super) fn update_saved_seed_browser_scroll(state: &mut AppState, pointer_moved: bool) {
        let Some(mode) = state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
            .as_ref()
            .map(|b| b.mode)
        else {
            return;
        };
        let layout = Self::skirmish_saved_seed_layout(state, mode);
        let y = state.match_state.input.cursor_y.round() as i32;
        let browser = state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
            .as_mut()
            .unwrap();
        if saved_file_input::update_scroll(
            browser,
            &layout,
            y,
            pointer_moved,
            std::time::Instant::now(),
        ) {
            Self::sync_saved_seed_edit_scroll(state);
        }
    }

    pub(super) fn handle_saved_seed_browser_mouse_down(state: &mut AppState) -> bool {
        let Some(browser) = state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
            .as_ref()
        else {
            return false;
        };
        let layout = Self::skirmish_saved_seed_layout(state, browser.mode);
        let extent = (state.render_width(), state.render_height());
        let pointer = (
            state.match_state.input.cursor_x.round() as i32,
            state.match_state.input.cursor_y.round() as i32,
        );
        let browser = state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
            .as_mut()
            .unwrap();
        let result = saved_file_input::mouse_down(
            browser,
            &layout,
            extent,
            pointer,
            std::time::Instant::now(),
            saved_file_input::host_double_click_limits(),
        );
        Self::apply_saved_seed_input(state, result);
        true
    }

    pub(super) fn handle_saved_seed_browser_mouse_up(state: &mut AppState) -> bool {
        let Some(browser) = state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
            .as_ref()
        else {
            return false;
        };
        let layout = Self::skirmish_saved_seed_layout(state, browser.mode);
        let extent = (state.render_width(), state.render_height());
        let pointer = (
            state.match_state.input.cursor_x.round() as i32,
            state.match_state.input.cursor_y.round() as i32,
        );
        let browser = state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
            .as_mut()
            .unwrap();
        let result = saved_file_input::mouse_up(browser, &layout, extent, pointer);
        Self::apply_saved_seed_input(state, result);
        true
    }
}
