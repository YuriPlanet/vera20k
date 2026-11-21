//! VERA's Skirmish Setups page: the player saves the page under a name, with
//! its map and game mode, and loads or deletes a saved setup later. gamemd's
//! `0x102` has no counterpart. The page is the saved-file browser's Save
//! dialog (driver `0x00558DD0`: list, name field, message boxes) with Load and
//! Delete under its Save; `MultiplayerShellRuntime` owns the setups.

use super::shell_saved_seeds::saved_browser_rows;
use super::*;
use crate::app::frontend::shell_layout::setups_layout;
use crate::map::rmg::{SeedDescription, saved_seeds};
use crate::ui::shell::saved_file_input::{self, BrowserInputResult};
use crate::ui::skirmish_shell::{
    SavedSeedBrowserRow, SavedSeedBrowserState, SavedSeedOutcome, SavedSeedPromptPurpose,
};

type SetupOutcome = SavedSeedOutcome<String>;
type SetupPurpose = SavedSeedPromptPurpose<String>;

fn description(text: &str) -> SeedDescription {
    SeedDescription::from_units(text.encode_utf16())
}

impl App {
    /// The Setups button: open the page on the saved setups, newest first,
    /// with a new row whose name is the selected map's.
    pub(super) fn open_skirmish_setups(state: &mut AppState) {
        let rows = state
            .frontend
            .multiplayer_shell_runtime
            .named_setups()
            .map(|(name, saved)| SavedSeedBrowserRow {
                file_name: Some(name.to_owned()),
                description: description(name),
                last_write_time: saved,
                visible: true,
                loadable: true,
                migratable: false,
            })
            .collect();
        let maps = state.frontend.scenario_catalog.shell_maps();
        let map = maps.get(state.frontend.skirmish_shell_state.selected_map_idx);
        let map_name = map.map(|map| map.display_name.trim()).unwrap_or_default();
        let empty = state.csf_label("TXT_EMPTY_SLOT", "[EMPTY SLOT]");
        let visible_rows = saved_browser_rows(&setups_layout(state));
        let mut browser = SavedSeedBrowserState::open_rows(
            SavedSeedMode::Save,
            rows,
            description(map_name),
            description(&empty),
            saved_seeds::new_slot_file_time(crate::util::time::SystemTime::now()),
            visible_rows,
        );
        // VERA: Back takes Escape (the base browser ignores IDCANCEL); a
        // message box over the page takes it first.
        use crate::ui::skirmish_shell::saved_seed_browser::{BACK, BROWSER_DIALOG};
        let cancel = vera20k_gui::Value::Bool(true);
        browser
            .ui
            .set_prop(BROWSER_DIALOG, BACK, vera20k_gui::Prop::Cancel, cancel);
        Self::blur_player_name(state);
        state.frontend.skirmish_shell_state.setups_browser = Some(browser);
    }

    fn close_skirmish_setups(state: &mut AppState) {
        state.frontend.skirmish_shell_state.setups_browser = None;
    }

    fn show_setups_prompt(
        state: &mut AppState,
        purpose: SetupPurpose,
        body: String,
        confirm: bool,
    ) {
        let prompt = Self::saved_browser_prompt(state, purpose, body, confirm);
        if let Some(browser) = state.frontend.skirmish_shell_state.setups_browser.as_mut() {
            browser.show_prompt(prompt);
        }
    }

    fn save_skirmish_setup(state: &mut AppState, name: &str) {
        let frontend = &mut state.frontend;
        frontend.multiplayer_shell_runtime.save_named_setup(
            name,
            saved_seeds::new_slot_file_time(crate::util::time::SystemTime::now()),
            &frontend.skirmish_shell_state,
            frontend.scenario_catalog.shell_maps(),
        );
        let body = crate::i18n::tr_fill("VERA:SetupSaved", &[("name", &name)]);
        Self::show_setups_prompt(state, SetupPurpose::Saved, body, false);
    }

    fn load_skirmish_setup(state: &mut AppState, name: &str) {
        let frontend = &mut state.frontend;
        let loaded = frontend.multiplayer_shell_runtime.load_named_setup(
            name,
            &mut frontend.skirmish_shell_state,
            &frontend.scenario_catalog,
            &frontend.skirmish_modes,
        );
        match loaded.and_then(|index| frontend.scenario_catalog.shell_maps().get(index)) {
            Some(map) => {
                let file_name = map.file_name.clone();
                Self::note_selected_shell_map(state, &file_name);
                Self::close_skirmish_setups(state);
            }
            None => {
                let body = crate::i18n::tr_fill("VERA:SetupMapMissing", &[("name", &name)]);
                Self::show_setups_prompt(state, SetupPurpose::Error, body, false);
            }
        }
    }

    fn apply_skirmish_setups_outcome(state: &mut AppState, outcome: SetupOutcome) {
        match outcome {
            SavedSeedOutcome::BeginConvert(_) => {}
            SetupOutcome::Close => Self::close_skirmish_setups(state),
            SetupOutcome::Save { description, .. } => {
                let name = String::from_utf16_lossy(description.units());
                if name.trim().is_empty() {
                    let body = state
                        .csf_label("TXT_MUSTENTER_DESCRIPTION", "You must enter a description!");
                    Self::show_setups_prompt(state, SetupPurpose::EmptyDescription, body, false);
                } else if (state.frontend.multiplayer_shell_runtime).has_named_setup(&name) {
                    let body = crate::i18n::tr_fill("VERA:SetupOverwrite", &[("name", &name)]);
                    let purpose = SetupPurpose::Overwrite {
                        file_name: name,
                        description,
                    };
                    Self::show_setups_prompt(state, purpose, body, true);
                } else {
                    Self::save_skirmish_setup(state, &name);
                }
            }
            SetupOutcome::Load(name) => Self::load_skirmish_setup(state, &name),
            SetupOutcome::Delete(name) => {
                let body = crate::i18n::tr_fill("VERA:SetupDelete", &[("name", &name)]);
                let purpose = SetupPurpose::Delete { file_name: name };
                Self::show_setups_prompt(state, purpose, body, true);
            }
        }
    }

    fn resolve_skirmish_setups_prompt(state: &mut AppState, affirmative: bool) {
        let Some(browser) = state.frontend.skirmish_shell_state.setups_browser.as_mut() else {
            return;
        };
        let Some(prompt) = browser.take_prompt() else {
            return;
        };
        match prompt.purpose {
            SetupPurpose::EmptyDescription => browser.focus_edit(),
            SetupPurpose::Saved => Self::close_skirmish_setups(state),
            SetupPurpose::Overwrite { file_name, .. } if affirmative => {
                Self::save_skirmish_setup(state, &file_name);
            }
            SetupPurpose::Delete { file_name } if affirmative => {
                let rows = saved_browser_rows(&setups_layout(state));
                (state.frontend.multiplayer_shell_runtime).delete_named_setup(&file_name);
                if let Some(browser) = state.frontend.skirmish_shell_state.setups_browser.as_mut() {
                    browser.remove_entry(&file_name, rows);
                }
            }
            _ => {}
        }
    }

    fn apply_skirmish_setups_input(state: &mut AppState, results: Vec<BrowserInputResult<String>>) {
        for result in results {
            match result {
                BrowserInputResult::Outcome(outcome) => {
                    Self::apply_skirmish_setups_outcome(state, outcome)
                }
                BrowserInputResult::PromptAnswer(answer) => {
                    Self::resolve_skirmish_setups_prompt(state, answer)
                }
                BrowserInputResult::Cue(cue) => Self::play_page_cue(state, cue),
            }
        }
        state.platform.window.request_redraw();
    }

    pub(super) fn handle_skirmish_setups_key(
        state: &mut AppState,
        code: Option<KeyCode>,
        text: Option<&str>,
    ) {
        let Some(browser) = state.frontend.skirmish_shell_state.setups_browser.as_mut() else {
            return;
        };
        // Esc leaves the page through Back when no message box is up.
        let results = saved_file_input::key(browser, code, text);
        Self::apply_skirmish_setups_input(state, results);
        Self::update_skirmish_setups_scroll(state, false);
    }

    /// Each frame, and on a pointer move: the scroll repeat, a held thumb
    /// and the edit's scroll.
    pub(super) fn update_skirmish_setups_scroll(state: &mut AppState, pointer_moved: bool) {
        if pointer_moved {
            Self::skirmish_setups_pointer(state, None);
        }
        let layout = setups_layout(state);
        let extent = (state.render_width(), state.render_height());
        let font = &state.renderer.bit_font;
        if let Some(browser) = state.frontend.skirmish_shell_state.setups_browser.as_mut() {
            saved_file_input::update(browser, &layout, extent, |text| font.text_width(text));
        }
    }

    pub(super) fn skirmish_setups_pointer(state: &mut AppState, down: Option<bool>) {
        let layout = setups_layout(state);
        let extent = (state.render_width(), state.render_height());
        let pointer = state.match_state.input.cursor_px();
        let Some(browser) = state.frontend.skirmish_shell_state.setups_browser.as_mut() else {
            return;
        };
        let results = saved_file_input::pointer(browser, &layout, extent, pointer, down);
        Self::apply_skirmish_setups_input(state, results);
    }
}
