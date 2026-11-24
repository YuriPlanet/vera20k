//! Single Player's Load Saved Game: dialog `0xB7` in Load mode, opened by
//! `Main__PrepareSession` state 9 (`0x0052E0BE`) after `0x100`'s teardown.
//! The shared saved-game loop `0x00558DD0` owns it; outside a suspended game
//! it is a family page (right panel, heading, status line, slides). Back
//! (result 2) tears it down and state 1 recreates Single Player.

use std::path::{Path, PathBuf};

use winit::keyboard::KeyCode;

use super::{App, AppState};
use crate::app::frontend::shell_layout::load_saved_game_layout;
use crate::app::shell_route::ShellRoute;
use crate::ui::shell::saved_file_input;
use crate::ui::shell::saved_games::LOAD_SAVED_GAME_PAGE;
use crate::ui::skirmish_shell::{SavedSeedBrowserState, SavedSeedMode, SavedSeedPromptPurpose};

impl App {
    /// State 9: `0x00558F2E` creates `0xB7` with the rows `0x005596A0`
    /// scanned, and `0x00558FC8..0x00558FE5` enables Load from the row count.
    pub(super) fn open_load_saved_game_page(state: &mut AppState) {
        Self::destroy_current_shell_dialog(state);
        // The error message box paints from the skirmish chrome atlas.
        let _ = Self::ensure_skirmish_shell_chrome(state);
        let rows =
            super::shell_saved_seeds::saved_browser_rows(&load_saved_game_layout(state).browser);
        let browser = Self::open_saved_game_rows(state, SavedSeedMode::Load, rows, None);
        state.frontend.load_saved_game = Some(browser);
        // The status help is the cursor's from the first frame.
        Self::load_saved_game_pointer(state, None);
        let dialog = LOAD_SAVED_GAME_PAGE.dialog;
        Self::show_shell_page(state, ShellRoute::LoadSavedGame, dialog);
    }

    /// Back's teardown has run: state 9 returns false and state 1 recreates
    /// Single Player (`0x0052E0F9`).
    pub(super) fn commit_load_saved_game_back(state: &mut AppState) {
        state.frontend.load_saved_game = None;
        Self::open_single_player_shell(state);
    }

    /// One pointer message on the page (`down`: `None` moves).
    fn load_saved_game_pointer(state: &mut AppState, down: Option<bool>) {
        let layout = load_saved_game_layout(state);
        let extent = (
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        );
        let pointer = state.match_state.input.cursor_px();
        if let Some(browser) = state.frontend.load_saved_game.as_mut() {
            let results =
                saved_file_input::pointer(browser, &layout.browser, extent, pointer, down);
            Self::apply_saved_game_input(state, results);
        }
    }

    pub(super) fn handle_load_saved_game_mouse(state: &mut AppState, pressed: bool) {
        Self::load_saved_game_pointer(state, Some(pressed));
    }

    pub(super) fn handle_load_saved_game_mouse_move(state: &mut AppState) {
        Self::load_saved_game_pointer(state, None);
        // Every hover message repaints the status line (0x00615EF7).
        state.frontend.shell_status_line.repaint();
    }

    /// The name edit and its error box get Escape before the shell menu.
    pub(super) fn load_saved_game_leaves_escape(state: &AppState) -> bool {
        state
            .frontend
            .load_saved_game
            .as_ref()
            .is_some_and(|browser| browser.prompt.is_none() && !browser.is_naming_conversion())
    }

    pub(super) fn handle_load_saved_game_key(
        state: &mut AppState,
        code: Option<KeyCode>,
        text: Option<&str>,
    ) {
        if let Some(browser) = state.frontend.load_saved_game.as_mut() {
            let results = saved_file_input::key(browser, code, text);
            Self::apply_saved_game_input(state, results);
        }
    }

    /// Scrollbar arrow repeat (`0x0061C690`), each frame the page shows.
    pub(super) fn update_load_saved_game_scroll(state: &mut AppState) {
        let layout = load_saved_game_layout(state);
        let extent = (
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        );
        let font = &state.renderer.bit_font;
        if let Some(browser) = state.frontend.load_saved_game.as_mut() {
            saved_file_input::update(browser, &layout.browser, extent, |text| {
                font.text_width(text)
            });
        }
    }

    /// 559D60 shares Load67E440 with the in-game browser. Failure keeps the
    /// current browser rows and selection.
    pub(crate) fn finish_load_saved_game_from_menu(
        state: &mut AppState,
        path: &Path,
        result: Result<(), crate::app::persistence::PreparedLoadError>,
    ) {
        let Err(error) = result else {
            state.frontend.load_saved_game = None;
            return;
        };
        log::warn!("Load Saved Game: {}: {error}", path.display());
        let body = state.csf_label("TXT_ERROR_LOADING_GAME", "Error loading game.");
        Self::show_saved_game_prompt(state, SavedSeedPromptPurpose::Error, body, false);
    }

    /// Status help of the control under the cursor (`0x006040B0`): the list,
    /// Load or Back, enabled or not.
    pub(crate) fn load_saved_game_status_key(state: &AppState) -> Option<String> {
        (state.frontend.load_saved_game.as_ref()?.hovered_help()).map(str::to_owned)
    }

    /// The description a new save of the running match gets by default.
    /// Pause Save4F1416 selects retained Scenario+13DA in GameMode0.
    pub(super) fn current_save_description(state: &AppState) -> String {
        if state.match_state.session_mode()
            == crate::app::match_runtime::session::SessionMode::Campaign
        {
            state.match_state.campaign_names.save_title()
        } else {
            state.csf_label("GUI:SkirmishGame", "Skirmish Game")
        }
    }

    /// Rows for a saved-game browser: every readable save with its header
    /// description, newest first (`0x005596A0`).
    pub(super) fn open_saved_game_rows(
        state: &AppState,
        mode: SavedSeedMode,
        visible_rows: usize,
        conversion_source: Option<PathBuf>,
    ) -> SavedSeedBrowserState<PathBuf> {
        let other_version = crate::i18n::tr("VERA:SaveFromOtherVersion");
        let mut description = if conversion_source.is_some() {
            String::new()
        } else {
            Self::current_save_description(state)
        };
        let entries = state
            .persistence
            .repository
            .browser_entries()
            .into_iter()
            .map(|(entry, last_write_time)| {
                if conversion_source.as_ref() == Some(&entry.path) {
                    description = entry.header.description.clone();
                }
                let mut row = saved_game_row(entry, last_write_time, mode, &other_version);
                // The conversion chooser lists old saves as read-only alongside writable slots.
                row.visible |= conversion_source.is_some();
                row
            })
            .collect();
        // Existing rows still keep their own saved descriptions.
        let empty = state.csf_label("TXT_EMPTY_SLOT", "[EMPTY SLOT]");
        let mut browser = SavedSeedBrowserState::open_rows(
            mode,
            entries,
            description.into(),
            empty.into(),
            crate::map::rmg::saved_seeds::new_slot_file_time(crate::util::time::SystemTime::now()),
            visible_rows,
        );
        if let Some(source) = conversion_source {
            browser.set_conversion_source(source, visible_rows);
        }
        browser
    }
}

/// One saved game's browser row. gamemd hides a save whose internal version
/// differs (`0x005599DF`), and its version never changes; every VERA build
/// that changes a saved type has a new layout id, so hiding would empty the
/// list after an update. VERA-only: Load and Delete keep such a row, marked
/// with `other_version` and not loadable. Save still hides it, so its marked
/// text never becomes a new save's description.
fn saved_game_row(
    entry: crate::app::persistence::SaveEntry,
    last_write_time: u64,
    mode: SavedSeedMode,
    other_version: &str,
) -> crate::ui::skirmish_shell::SavedSeedBrowserRow<PathBuf> {
    let loadable = entry.header.is_compatible();
    let migratable = mode == SavedSeedMode::Load
        && crate::app::persistence::SaveRepository::can_migrate(&entry.header);
    let description = if loadable {
        entry.header.description
    } else {
        format!("{} {other_version}", entry.header.description)
    };
    crate::ui::skirmish_shell::SavedSeedBrowserRow {
        visible: loadable || mode != SavedSeedMode::Save,
        loadable,
        migratable,
        file_name: Some(entry.path),
        description: description.into(),
        last_write_time,
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use crate::sim::snapshot::GameSnapshot;
    use crate::ui::skirmish_shell::SavedSeedBrowserState;

    #[test]
    fn a_save_from_another_build_stays_listed_for_load_and_delete_but_not_loadable() {
        let bytes = GameSnapshot::save_validated(
            &crate::sim::world::Simulation::new(),
            0,
            0,
            "Old campaign",
            0,
        );
        let mut header = GameSnapshot::read_header(&bytes).unwrap();
        header.layout = 0;
        let path = PathBuf::from("old.bin");
        let open = |mode| {
            let entry = crate::app::persistence::SaveEntry {
                path: path.clone(),
                header: header.clone(),
            };
            SavedSeedBrowserState::open_rows(
                mode,
                vec![saved_game_row(entry, 5, mode, "(other version)")],
                "Current".into(),
                "[EMPTY SLOT]".into(),
                9,
                4,
            )
        };
        for mode in [SavedSeedMode::Load, SavedSeedMode::Delete] {
            let browser = open(mode);
            let row = browser.selected_entry().expect("the old save is listed");
            assert_eq!(row.file_name.as_ref(), Some(&path));
            assert!(!row.loadable);
            assert!(!row.migratable);
            assert_eq!(
                row.description.display_text(),
                "Old campaign (other version)"
            );
        }
        assert!(
            open(SavedSeedMode::Save)
                .entries
                .iter()
                .all(|row| row.file_name.is_none()),
            "Save offers only the new slot"
        );
    }
}
