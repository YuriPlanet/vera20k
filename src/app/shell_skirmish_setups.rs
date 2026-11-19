//! VERA's Skirmish Setups page: the player saves the page under a name, with
//! its map and game mode, and loads or deletes a saved setup later. gamemd's
//! `0x102` has no counterpart. The page is the saved-file browser's Save
//! dialog (driver `0x00558DD0`: list, name field, message boxes) with Load and
//! Delete under its Save; `MultiplayerShellRuntime` owns the setups.

use super::*;
use crate::map::rmg::{SeedDescription, saved_seeds};
use crate::ui::shell::list::ListMetrics;
use crate::ui::shell::saved_file_input::{self, BrowserInputResult};
use crate::ui::skirmish_shell::seed_list::SeedListGeometry;
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
            })
            .collect();
        let map_name = state
            .frontend
            .scenario_catalog
            .shell_maps()
            .get(state.frontend.skirmish_shell_state.selected_map_idx)
            .map(|map| map.display_name.trim().to_owned())
            .unwrap_or_default();
        let empty = Self::csf_label(state, "TXT_EMPTY_SLOT", "[EMPTY SLOT]");
        let layout = crate::app::frontend::shell_layout::setups_layout(state);
        let visible_rows =
            SeedListGeometry::new(layout.list, 0, 0, ListMetrics::scaled(layout.scale))
                .visible_rows;
        let browser = SavedSeedBrowserState::open_rows(
            SavedSeedMode::Save,
            rows,
            description(&map_name),
            description(&empty),
            saved_seeds::new_slot_file_time(crate::util::time::SystemTime::now()),
            visible_rows,
        );
        let shell = &mut state.frontend.skirmish_shell_state;
        shell.player_name_edit.focused = false;
        shell.open_combo_dropdown = None;
        shell.pressed_owner_draw_button = None;
        shell.setups_browser = Some(browser);
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
            SetupOutcome::Close => Self::close_skirmish_setups(state),
            SetupOutcome::Save { description, .. } => {
                let name = String::from_utf16_lossy(description.units());
                if name.trim().is_empty() {
                    let body = Self::csf_label(
                        state,
                        "TXT_MUSTENTER_DESCRIPTION",
                        "You must enter a description!",
                    );
                    Self::show_setups_prompt(state, SetupPurpose::EmptyDescription, body, false);
                } else if state
                    .frontend
                    .multiplayer_shell_runtime
                    .has_named_setup(&name)
                {
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
                Self::show_setups_prompt(
                    state,
                    SetupPurpose::Delete { file_name: name },
                    body,
                    true,
                );
            }
        }
    }

    fn resolve_skirmish_setups_prompt(state: &mut AppState, affirmative: bool) {
        let Some(browser) = state.frontend.skirmish_shell_state.setups_browser.as_mut() else {
            return;
        };
        browser.pressed_control = None;
        let Some(prompt) = browser.prompt.take() else {
            return;
        };
        match prompt.purpose {
            SetupPurpose::EmptyDescription => browser.description_edit.focused = true,
            SetupPurpose::Saved => Self::close_skirmish_setups(state),
            SetupPurpose::Overwrite { file_name, .. } if affirmative => {
                Self::save_skirmish_setup(state, &file_name);
            }
            SetupPurpose::Delete { file_name } if affirmative => {
                let layout = crate::app::frontend::shell_layout::setups_layout(state);
                let visible_rows =
                    SeedListGeometry::new(layout.list, 0, 0, ListMetrics::scaled(layout.scale))
                        .visible_rows;
                state
                    .frontend
                    .multiplayer_shell_runtime
                    .delete_named_setup(&file_name);
                if let Some(browser) = state.frontend.skirmish_shell_state.setups_browser.as_mut() {
                    browser.remove_entry(&file_name, visible_rows);
                }
            }
            _ => {}
        }
    }

    fn apply_skirmish_setups_input(state: &mut AppState, result: BrowserInputResult<String>) {
        match result {
            BrowserInputResult::None => {}
            BrowserInputResult::Outcome(outcome) => {
                Self::apply_skirmish_setups_outcome(state, outcome)
            }
            BrowserInputResult::PromptAnswer(answer) => {
                Self::resolve_skirmish_setups_prompt(state, answer)
            }
            BrowserInputResult::ButtonPressed | BrowserInputResult::PromptButtonPressed => {
                Self::play_main_menu_button_sound(state)
            }
            BrowserInputResult::RowClicked => Self::play_skirmish_shell_generic_click_sound(state),
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
        // Esc leaves the page when no message box is up.
        if browser.prompt.is_none() && code == Some(KeyCode::Escape) {
            Self::close_skirmish_setups(state);
            return;
        }
        let result = saved_file_input::key(browser, code, text);
        Self::apply_skirmish_setups_input(state, result);
        Self::sync_skirmish_setups_edit_scroll(state);
    }

    fn sync_skirmish_setups_edit_scroll(state: &mut AppState) {
        let layout = crate::app::frontend::shell_layout::setups_layout(state);
        let Some(rect) = layout.name_edit else {
            return;
        };
        // As the seed browser's: the fit width is in the font's authored units.
        let available = (crate::ui::skirmish_shell::player_name_edit_text_rect(
            layout.scale.base_rect(rect),
            crate::ui::shell::geom::ShellScale::RETAIL,
        )
        .w - 2)
            .max(0) as u32;
        let Some(browser) = state.frontend.skirmish_shell_state.setups_browser.as_mut() else {
            return;
        };
        browser
            .description_edit
            .scroll_to_caret(available, |text| state.renderer.bit_font.text_width(text));
    }

    pub(super) fn update_skirmish_setups_scroll(state: &mut AppState, pointer_moved: bool) {
        if state.frontend.skirmish_shell_state.setups_browser.is_none() {
            return;
        }
        let layout = crate::app::frontend::shell_layout::setups_layout(state);
        let y = state.match_state.input.cursor_y.round() as i32;
        let Some(browser) = state.frontend.skirmish_shell_state.setups_browser.as_mut() else {
            return;
        };
        if saved_file_input::update_scroll(
            browser,
            &layout,
            y,
            pointer_moved,
            crate::util::time::Instant::now(),
        ) {
            Self::sync_skirmish_setups_edit_scroll(state);
        }
    }

    pub(super) fn handle_skirmish_setups_mouse_down(state: &mut AppState) {
        let layout = crate::app::frontend::shell_layout::setups_layout(state);
        let extent = (state.render_width(), state.render_height());
        let pointer = state.match_state.input.cursor_px();
        let Some(browser) = state.frontend.skirmish_shell_state.setups_browser.as_mut() else {
            return;
        };
        let result = saved_file_input::mouse_down(
            browser,
            &layout,
            extent,
            pointer,
            crate::util::time::Instant::now(),
            saved_file_input::host_double_click_limits(),
        );
        Self::apply_skirmish_setups_input(state, result);
        Self::sync_skirmish_setups_edit_scroll(state);
    }

    pub(super) fn handle_skirmish_setups_mouse_up(state: &mut AppState) {
        let layout = crate::app::frontend::shell_layout::setups_layout(state);
        let extent = (state.render_width(), state.render_height());
        let pointer = state.match_state.input.cursor_px();
        let Some(browser) = state.frontend.skirmish_shell_state.setups_browser.as_mut() else {
            return;
        };
        let result = saved_file_input::mouse_up(browser, &layout, extent, pointer);
        Self::apply_skirmish_setups_input(state, result);
    }
}
