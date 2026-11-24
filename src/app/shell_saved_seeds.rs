//! Saved-seed browser input and persistence transactions.
//! Native driver 0x00558DD0 owns modal lifetime; MapSeed slots 0x00597760,
//! 0x00597A30 and 0x00597D50 own persistence.

use super::shell_random_map::{
    RANDOM_MAP_DESCRIPTION_FALLBACK, RANDOM_MAP_DESCRIPTION_KEY, random_map_dialog,
};
use super::*;
use crate::app::frontend::shell_layout;
use crate::map::rmg::saved_seeds::{SavedSeedFile, SeedWrite};
use crate::map::rmg::{SeedDescription, saved_seeds};
use crate::ui::skirmish_shell::{
    SavedSeedLayout, SavedSeedOutcome, SavedSeedPrompt, SavedSeedPromptPurpose,
};
use vera20k_gui::shell::list::{ListMetrics, ShellListGeometry};

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

    /// The open browser's layout.
    fn saved_seed_layout(state: &AppState) -> Option<SavedSeedLayout> {
        let browser = state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
            .as_ref()?;
        Some(shell_layout::saved_seed_layout(state, browser.mode))
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
        Self::blur_player_name(state);
    }

    fn close_saved_seed_browser(state: &mut AppState) {
        state.frontend.skirmish_shell_state.saved_seed_browser = None;
        // Re-entry sees physical disk state, including failed native-style deletes.
        let available =
            Self::random_map_dir(state).is_some_and(|dir| saved_seeds::saved_seeds_available(&dir));
        if let Some(modal) = random_map_dialog(state) {
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
        file_name: SavedSeedFile,
        description: SeedDescription,
        action: SeedWrite,
    ) {
        let Some(dir) = Self::random_map_dir(state) else {
            Self::show_saved_seed_error(state, "VERA:RandomMapSaveFailed");
            return;
        };
        let Some(modal) = random_map_dialog(state) else {
            return;
        };
        // MapSeed Save 0x00597804 updates the working description before file I/O.
        modal.options.description = description;
        if let Err(error) =
            saved_seeds::write_browser_seed(&dir, &file_name, &modal.options, modal.ares, action)
        {
            log::warn!("saved seed: could not save {file_name}: {error}");
            Self::show_saved_seed_error(state, "VERA:RandomMapSaveFailed");
            return;
        }
        Self::refresh_saved_random_maps(state);
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
                Self::finish_saved_seed_write(state, file_name, description, SeedWrite::Overwrite);
            }
            SavedSeedPromptPurpose::Delete { file_name } if affirmative => {
                if let Some(dir) = Self::random_map_dir(state)
                    && let Err(error) = file_name.delete(&dir)
                {
                    log::warn!("saved seed: could not delete {file_name}: {error}");
                }
                // As in driver 0x00558DD0 the browser drops the row after the
                // attempt. The chooser reflects the files still on disk.
                Self::refresh_saved_random_maps(state);
                let layout = shell_layout::saved_seed_layout(state, SavedSeedMode::Delete);
                let rows = saved_browser_rows(&layout);
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

    fn show_saved_seed_error(state: &mut AppState, key: &str) {
        Self::show_saved_seed_prompt(
            state,
            SavedSeedPromptPurpose::Error,
            crate::i18n::tr(key),
            false,
        );
    }

    /// Load one saved seed. The chooser's Use Map generates it and then accepts
    /// it in the chosen mode; Load in the generator leaves its controls open.
    pub(super) fn load_random_map_seed(state: &mut AppState, file: &SavedSeedFile, use_map: bool) {
        let dir = Self::random_map_dir(state);
        let default = state.csf_label(RANDOM_MAP_DESCRIPTION_KEY, RANDOM_MAP_DESCRIPTION_FALLBACK);
        let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .random_map_setup_modal
            .as_mut()
        else {
            return;
        };
        let loaded = dir
            .ok_or_else(|| std::io::Error::other("no maps directory"))
            .and_then(|dir| {
                // The chooser plays the saved map independently of the editor:
                // a zero-valued Advanced slider is omitted from the seed file.
                let defaults = crate::map::rmg::RmgOptions::default();
                let current = if use_map { &defaults } else { &modal.options };
                saved_seeds::read_browser_seed(&dir, file, current, &default, modal.ares)
            });
        let mut options = match loaded {
            Ok(options) => options,
            Err(error) => {
                log::warn!("saved seed: could not read {file}: {error}");
                if use_map {
                    modal.extras_note = Some((
                        "VERA:RandomMapOpenFailed",
                        "Could not open this saved map. Go back and pick another.",
                    ));
                } else {
                    Self::show_saved_seed_error(state, "VERA:RandomMapOpenFailed");
                }
                return;
            }
        };
        if use_map {
            if options.num_players > modal.players_range.max
                && state.frontend.shell_route == crate::app::shell_route::ShellRoute::NetworkSetup
            {
                modal.extras_note = Some((
                    "VERA:RandomMapRoomLimit",
                    "This map has too many players for a network room.",
                ));
                return;
            }
        } else {
            // Load 0x00596963 posts Generate with its reroll latch clear.
            // Control readback couples height to width and stamps the default.
            options.height = options.width;
            options.description = default.into();
        }
        modal.extras_note = None;
        modal.adopt_saved_seed(options);
        let options = modal.options.clone();
        modal.begin_generate();
        if !use_map {
            Self::close_saved_seed_browser(state);
        }
        if !Self::start_random_map_generation(state, &options, use_map)
            && let Some(modal) = state
                .frontend
                .skirmish_shell_state
                .random_map_setup_modal
                .as_mut()
        {
            modal.fail_generate();
        }
    }

    fn apply_saved_seed_outcome(state: &mut AppState, outcome: SavedSeedOutcome) {
        match outcome {
            SavedSeedOutcome::Close => Self::close_saved_seed_browser(state),
            SavedSeedOutcome::BeginConvert(_) => {}
            SavedSeedOutcome::Load(file_name) => {
                Self::load_random_map_seed(state, &file_name, false)
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
                if let Some(file_name) = file_name {
                    let body = state.csf_label("TXT_CONFIRM_SAVE", "Overwrite existing save game?");
                    let purpose = SavedSeedPromptPurpose::Overwrite {
                        file_name,
                        description,
                    };
                    Self::show_saved_seed_prompt(state, purpose, body, true);
                    return;
                }
                let Some(dir) = Self::random_map_dir(state) else {
                    Self::show_saved_seed_error(state, "VERA:RandomMapSaveFailed");
                    return;
                };
                let assets = state.process_assets.manager();
                match saved_seeds::allocate_seed_file_name(
                    &dir,
                    &mut state.frontend.legacy_crt_rng,
                    |name| assets.is_some_and(|assets| assets.contains(name)),
                ) {
                    Ok(file_name) => {
                        Self::finish_saved_seed_write(state, file_name, description, SeedWrite::New)
                    }
                    Err(error) => {
                        log::warn!("saved seed: could not create a new slot: {error}");
                        Self::show_saved_seed_error(state, "VERA:RandomMapSaveFailed");
                    }
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
                let description = row.map(|row| row.description.display_text());
                let body = format!("{prefix}\n\n{}", description.unwrap_or_default());
                let purpose = SavedSeedPromptPurpose::Delete { file_name };
                Self::show_saved_seed_prompt(state, purpose, body, true);
            }
        }
    }

    fn apply_saved_seed_input(
        state: &mut AppState,
        results: Vec<BrowserInputResult<SavedSeedFile>>,
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
        let Some(layout) = Self::saved_seed_layout(state) else {
            return false;
        };
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
        let Some(layout) = Self::saved_seed_layout(state) else {
            return;
        };
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
