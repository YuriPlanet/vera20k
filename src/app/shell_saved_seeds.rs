//! Saved-seed browser input and persistence transactions.
//! Native driver 0x00558DD0 owns modal lifetime; MapSeed slots 0x00597760,
//! 0x00597A30 and 0x00597D50 own persistence.

use crate::app::frontend::shell_layout;
use super::shell_random_map::{RANDOM_MAP_DESCRIPTION_FALLBACK, RANDOM_MAP_DESCRIPTION_KEY};
use super::*;
use crate::map::rmg::{SeedDescription, saved_seeds};
use crate::ui::shell::list::{ListMetrics, ShellListGeometry};
use crate::ui::skirmish_shell::{
    SavedSeedLayout, SavedSeedOutcome, SavedSeedPrompt, SavedSeedPromptPurpose,
};
use crate::util::native_file_name::{self, NativeFileName};

use crate::ui::shell::saved_file_input::{self, BrowserInputResult};

/// The rows a saved-file browser's list shows on `layout`.
pub(super) fn saved_browser_rows(layout: &SavedSeedLayout) -> usize {
    ShellListGeometry::new(layout.list, 0, 0, ListMetrics::scaled(layout.scale)).visible_rows
}

impl App {
    fn saved_seed_browser(state: &mut AppState) -> Option<&mut SavedSeedBrowserState> {
        state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
            .as_mut()
    }

    pub(super) fn open_saved_seed_browser(state: &mut AppState, mode: SavedSeedMode) {
        let entries = Self::random_map_dir(state)
            .map(|dir| saved_seeds::list_saved_seeds(&dir))
            .unwrap_or_default();
        let shell = &state.frontend.skirmish_shell_state;
        let modal = shell.random_map_setup_modal.as_ref();
        let current = modal
            .map(|modal| modal.options.description.clone())
            .unwrap_or_default();
        let empty = state.csf_label("TXT_EMPTY_SLOT", "[EMPTY SLOT]");
        let rows = saved_browser_rows(&shell_layout::saved_seed_layout(state, mode));
        let time = saved_seeds::new_slot_file_time(crate::util::time::SystemTime::now());
        let browser = SavedSeedBrowserState::open(mode, entries, current, empty.into(), time, rows);
        state.frontend.skirmish_shell_state.saved_seed_browser = Some(browser);
        state.frontend.skirmish_shell_state.player_name_edit.focused = false;
    }

    fn close_saved_seed_browser(state: &mut AppState) {
        state.frontend.skirmish_shell_state.saved_seed_browser = None;
        // Re-entry sees physical disk state, including failed native-style deletes.
        let available =
            Self::random_map_dir(state).is_some_and(|dir| saved_seeds::saved_seeds_available(&dir));
        if let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_mut()
        {
            modal.saved_seed_buttons_enabled = available;
            // `0x105` is uncovered, not shown again: its statics repaint.
            modal.statics.uncovered();
        }
    }

    /// The saved-file browsers' message box: Yes/No to confirm, else OK.
    pub(super) fn saved_browser_prompt<I>(
        state: &AppState,
        purpose: SavedSeedPromptPurpose<I>,
        body: String,
        confirm: bool,
    ) -> SavedSeedPrompt<I> {
        let affirmative = if confirm {
            state.csf_label("TXT_YES", "Yes")
        } else {
            state.csf_label("TXT_OK", "OK")
        };
        let negative = confirm.then(|| state.csf_label("TXT_NO", "No"));
        SavedSeedPrompt {
            purpose,
            body,
            affirmative,
            negative,
        }
    }

    fn show_saved_seed_prompt(
        state: &mut AppState,
        purpose: SavedSeedPromptPurpose,
        body: String,
        confirm: bool,
    ) {
        let prompt = Self::saved_browser_prompt(state, purpose, body, confirm);
        if let Some(browser) = Self::saved_seed_browser(state) {
            browser.show_prompt(prompt);
        }
    }

    fn finish_saved_seed_write(
        state: &mut AppState,
        file_name: NativeFileName,
        description: SeedDescription,
    ) {
        let Some(dir) = Self::random_map_dir(state) else {
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
        if let Err(error) = crate::util::file_system::create_dir_all(&dir).and_then(|()| {
            saved_seeds::write_browser_seed(&dir, &file_name, &modal.options, modal.ares)
        }) {
            log::warn!(
                "saved seed: native-style save acknowledgment despite I/O failure for {file_name}: {error}"
            );
        }
        let body = state.csf_label("GUI:MapSaved", "Map Saved");
        Self::show_saved_seed_prompt(state, SavedSeedPromptPurpose::Saved, body, false);
    }

    fn resolve_saved_seed_prompt(state: &mut AppState, affirmative: bool) {
        let Some(prompt) =
            Self::saved_seed_browser(state).and_then(|browser| browser.take_prompt())
        else {
            return;
        };
        match prompt.purpose {
            // 0x00559269 explicitly restores the edit focus after this warning.
            SavedSeedPromptPurpose::EmptyDescription => {
                if let Some(browser) = Self::saved_seed_browser(state) {
                    browser.focus_edit();
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
                if let Some(dir) = Self::random_map_dir(state)
                    && let Err(error) = native_file_name::delete(&dir, &file_name)
                {
                    log::warn!("saved seed: could not delete {file_name}: {error}");
                }
                let layout = shell_layout::saved_seed_layout(state, SavedSeedMode::Delete);
                let rows = saved_browser_rows(&layout);
                // driver 0x00558DD0 removes the row regardless of DeleteFileA's result.
                let empty = Self::saved_seed_browser(state).is_some_and(|browser| {
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
        let Some(dir) = Self::random_map_dir(state) else {
            return;
        };
        match outcome {
            SavedSeedOutcome::Close => Self::close_saved_seed_browser(state),
            SavedSeedOutcome::Load(file_name) => {
                let default =
                    state.csf_label(RANDOM_MAP_DESCRIPTION_KEY, RANDOM_MAP_DESCRIPTION_FALLBACK);
                let shell = &mut state.frontend.skirmish_shell_state;
                let Some(modal) = shell.random_map_setup_modal.as_mut() else {
                    return;
                };
                match saved_seeds::read_browser_seed(
                    &dir,
                    &file_name,
                    &modal.options,
                    &default,
                    modal.ares,
                ) {
                    Ok(mut options) => {
                        // Load 0x00596963 posts Generate with its reroll latch clear.
                        // Ordinary valid control readback preserves loaded derived
                        // values, couples height to width, and stamps the default.
                        options.height = options.width;
                        options.description = default.into();
                        modal.adopt_saved_seed(options);
                        let options = modal.options.clone();
                        modal.begin_generate();
                        Self::close_saved_seed_browser(state);
                        if !Self::start_random_map_generation(state, &options, false)
                            && let Some(modal) = state
                                .frontend
                                .skirmish_shell_state
                                .random_map_setup_modal
                                .as_mut()
                        {
                            modal.fail_generate();
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
                    let body = state
                        .csf_label("TXT_MUSTENTER_DESCRIPTION", "You must enter a description!");
                    let purpose = SavedSeedPromptPurpose::EmptyDescription;
                    Self::show_saved_seed_prompt(state, purpose, body, false);
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
                    let body = state.csf_label("TXT_CONFIRM_SAVE", "Overwrite existing save game?");
                    let purpose = SavedSeedPromptPurpose::Overwrite {
                        file_name,
                        description,
                    };
                    Self::show_saved_seed_prompt(state, purpose, body, true);
                } else {
                    Self::finish_saved_seed_write(state, file_name, description);
                }
            }
            SavedSeedOutcome::Delete(file_name) => {
                let prefix = state.csf_label("TXT_DELETE_FILE_QUERY", "Delete this file?");
                let browser = state
                    .frontend
                    .skirmish_shell_state
                    .saved_seed_browser
                    .as_ref();
                let row = browser.and_then(|browser| browser.selected_entry());
                let description = row
                    .map(|row| row.description.display_text())
                    .unwrap_or_default();
                let body = format!("{prefix}\n\n{description}");
                Self::show_saved_seed_prompt(
                    state,
                    SavedSeedPromptPurpose::Delete { file_name },
                    body,
                    true,
                );
            }
        }
    }

    fn apply_saved_seed_input(
        state: &mut AppState,
        results: Vec<BrowserInputResult<NativeFileName>>,
    ) {
        for result in results {
            match result {
                BrowserInputResult::Outcome(outcome) => {
                    Self::apply_saved_seed_outcome(state, outcome)
                }
                BrowserInputResult::PromptAnswer(answer) => {
                    Self::resolve_saved_seed_prompt(state, answer)
                }
                BrowserInputResult::Cue(cue) => Self::play_page_cue(state, cue),
            }
        }
    }

    pub(super) fn handle_saved_seed_browser_key(
        state: &mut AppState,
        code: Option<KeyCode>,
        text: Option<&str>,
    ) {
        if let Some(browser) = Self::saved_seed_browser(state) {
            let results = saved_file_input::key(browser, code, text);
            Self::apply_saved_seed_input(state, results);
        }
    }

    /// One pointer message on the browser (`down`: `None` moves); false
    /// when none is open.
    pub(super) fn saved_seed_browser_pointer(state: &mut AppState, down: Option<bool>) -> bool {
        let Some(mode) = state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
            .as_ref()
            .map(|b| b.mode)
        else {
            return false;
        };
        let layout = shell_layout::saved_seed_layout(state, mode);
        let extent = (state.render_width(), state.render_height());
        let pointer = state.match_state.input.cursor_px();
        if let Some(browser) = Self::saved_seed_browser(state) {
            let results = saved_file_input::pointer(browser, &layout, extent, pointer, down);
            Self::apply_saved_seed_input(state, results);
        }
        true
    }

    /// Each frame, and on a pointer move: the scroll repeat, a held thumb
    /// and the edit's scroll.
    pub(super) fn update_saved_seed_browser_scroll(state: &mut AppState, pointer_moved: bool) {
        if pointer_moved {
            Self::saved_seed_browser_pointer(state, None);
        }
        let Some(mode) = state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
            .as_ref()
            .map(|b| b.mode)
        else {
            return;
        };
        let layout = shell_layout::saved_seed_layout(state, mode);
        let extent = (state.render_width(), state.render_height());
        let font = &state.renderer.bit_font;
        if let Some(browser) = state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
            .as_mut()
        {
            saved_file_input::update(browser, &layout, extent, |text| font.text_width(text));
        }
    }
}
