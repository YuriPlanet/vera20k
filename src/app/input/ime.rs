//! OS input-method admission and committed-text routing.
//!
//! Winit reports committed composition text separately from physical key
//! events. Route it through the same per-control insertion paths as ordinary
//! text so native limits, filters and notifications remain authoritative.

use crate::app::{App, AppState};
use crate::ui::game_screen::GameScreen;
use winit::dpi::{PhysicalPosition, PhysicalSize, Position, Size};

/// Identifies the focused edit, so switching between two edits is visible as
/// a change even though both keep IME enabled.
///
/// The address of the edit's focus flag is stable for as long as that control
/// exists and needs no per-state label field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FocusedEdit(usize);

/// The focused edit plus the text the OS keyboard should start from.
struct ActiveEdit {
    identity: FocusedEdit,
    text: String,
}

fn flag(edit: &impl EditFocus) -> Option<ActiveEdit> {
    edit.focus_flag().then(|| ActiveEdit {
        identity: FocusedEdit(edit as *const _ as usize),
        text: edit.text(),
    })
}

/// The edit that accepts text and currently owns focus, if any.
fn focused_edit(state: &AppState) -> Option<ActiveEdit> {
    if crate::app::frontend::shell_transition::blocks_shell_input(state) {
        return None;
    }
    if state.frontend.screen == GameScreen::InGame
        && state.match_state.match_presentation.message_list.editing()
    {
        return Some(ActiveEdit {
            identity: FocusedEdit(
                &state.match_state.match_presentation.message_list as *const _ as usize,
            ),
            text: String::new(),
        });
    }
    if crate::app::input::coop_score::active(state) {
        return state
            .frontend
            .coop_score_page
            .as_ref()
            .and_then(|page| flag(&page.ui.chat.edit));
    }
    if let Some(dialog) = state.frontend.network_options.as_ref() {
        if dialog.pending_message().is_some() {
            return None;
        }
        return flag(&dialog.socket)
            .or_else(|| flag(&dialog.destination))
            .or_else(|| flag(&dialog.port));
    }
    if state.frontend.screen == GameScreen::MainMenu && state.frontend.shell_route.lan_lobby() {
        let runtime = state.frontend.lan.as_ref()?;
        if runtime.modal.is_some() || !runtime.lobby.enabled {
            return None;
        }
        return flag(&runtime.lobby.name).or_else(|| flag(&runtime.lobby.chat.edit));
    }
    if state.frontend.screen == GameScreen::MainMenu
        && state.frontend.shell_route.network_setup()
        && state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .is_none()
    {
        let ui = state.frontend.multiplayer_setup.as_ref()?;
        if ui.focused_control != crate::ui::multiplayer_setup::CHAT {
            return None;
        }
        return flag(&ui.chat.edit);
    }
    if App::native_skirmish_shell_active(state) {
        if let Some(browser) = state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
            .as_ref()
        {
            if browser.prompt.is_some() {
                return None;
            }
            return flag(&browser.description_edit);
        }
        if state.frontend.skirmish_shell_state.player_name_edit.focused {
            return flag(&state.frontend.skirmish_shell_state.player_name_edit);
        }
    }
    if state.frontend.screen == GameScreen::InGame
        && matches!(
            state.match_state.match_presentation.in_game_menu,
            crate::ui::pause_menu::InGameMenuState::SavedGame(_)
        )
    {
        let browser = state
            .match_state
            .match_presentation
            .saved_game_browser
            .as_ref()?;
        if browser.prompt.is_some() {
            return None;
        }
        return flag(&browser.description_edit);
    }
    if crate::app::shell_wol::text_input_active(state) {
        return Some(ActiveEdit {
            identity: FocusedEdit(&state.frontend.wol as *const _ as usize),
            text: String::new(),
        });
    }
    None
}

/// Empty the focused edit before the ordinary insertion path runs, so the
/// keyboard's fully edited value replaces the old text.
///
/// Only the fields the software keyboard is opened for are listed. These edits
/// have no selection range, so they are cleared outright; an edit that does
/// support selection keeps that mechanism instead.
#[cfg(target_os = "horizon")]
fn select_focused_edit_for_replacement(state: &mut AppState) {
    fn clear(edit: &mut crate::ui::skirmish_shell::SavedSeedDescriptionEdit) {
        if edit.focused {
            edit.set_units_limited(std::iter::empty::<u16>(), 0);
        }
    }
    if App::native_skirmish_shell_active(state) {
        let shell = &mut state.frontend.skirmish_shell_state;
        if let Some(browser) = shell.saved_seed_browser.as_mut() {
            if browser.prompt.is_none() {
                clear(&mut browser.description_edit);
                if browser.description_edit.focused {
                    return;
                }
            }
        }
        if shell.player_name_edit.focused {
            shell.player_name_edit.focus_select_all();
            return;
        }
    }
    if let Some(dialog) = state.frontend.network_options.as_mut() {
        if dialog.pending_message().is_none() {
            for edit in [&mut dialog.socket, &mut dialog.destination, &mut dialog.port] {
                clear(edit);
            }
        }
    }
}

pub(crate) trait EditFocus {
    fn focus_flag(&self) -> bool;
    /// The edit's current content, so the OS keyboard can start from it
    /// instead of appending to whatever the player typed last time.
    fn text(&self) -> String;
}

impl EditFocus for crate::ui::skirmish_shell::PlayerNameEditState {
    fn focus_flag(&self) -> bool {
        self.focused
    }
    fn text(&self) -> String {
        self.text.clone()
    }
}

impl EditFocus for crate::ui::skirmish_shell::SavedSeedDescriptionEdit {
    fn focus_flag(&self) -> bool {
        self.focused
    }
    fn text(&self) -> String {
        self.display_text()
    }
}

/// Whether an edit that accepts text currently owns focus.
pub(crate) fn text_input_active(state: &AppState) -> bool {
    focused_edit(state).is_some()
}

/// Insert one committed string into whichever edit owned focus when the IME
/// produced it. Returns true when an owner consumed the event.
pub(crate) fn commit(state: &mut AppState, text: &str) -> bool {
    if text.is_empty() || !text_input_active(state) {
        return false;
    }

    // Horizon: the software keyboard opens pre-filled with the edit's content
    // and returns the whole edited value. Select everything before handing it
    // to the ordinary insertion path, which then consumes the selection and
    // replaces the old text instead of appending to it. Desktop keeps plain
    // per-character insertion because it has physical keys.
    #[cfg(target_os = "horizon")]
    select_focused_edit_for_replacement(state);

    if state.frontend.screen == GameScreen::InGame
        && state.match_state.match_presentation.message_list.editing()
    {
        crate::app::input::chat::commit_text(state, text);
        state.platform.window.request_redraw();
        return true;
    }
    if crate::app::input::coop_score::active(state) {
        crate::app::input::coop_score::key_press(state, None, Some(text));
        return true;
    }
    if state.frontend.network_options.is_some() {
        crate::app::input::network_options::key_press(state, None, Some(text));
        return true;
    }
    if state.frontend.screen == GameScreen::MainMenu && state.frontend.shell_route.lan_lobby() {
        crate::app::shell_lan::key_press(state, None, Some(text));
        return true;
    }
    if state.frontend.screen == GameScreen::MainMenu
        && state.frontend.shell_route.network_setup()
        && state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .is_none()
    {
        crate::app::input::multiplayer_setup::key_press(state, None, Some(text));
        return true;
    }
    if App::native_skirmish_shell_active(state) {
        if state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
            .is_some()
        {
            App::handle_saved_seed_browser_key(state, None, Some(text));
            state.platform.window.request_redraw();
            return true;
        }
        if state.frontend.skirmish_shell_state.player_name_edit.focused {
            return App::handle_skirmish_shell_text_input(state, text);
        }
    }
    if state.frontend.screen == GameScreen::InGame
        && matches!(
            state.match_state.match_presentation.in_game_menu,
            crate::ui::pause_menu::InGameMenuState::SavedGame(_)
        )
    {
        App::saved_game_key(state, None, Some(text));
        return true;
    }
    if crate::app::shell_wol::commit_text(state, text) {
        return true;
    }
    false
}

/// Keep the platform IME context aligned with edit focus. Desktop backends use
/// this to start/stop composition; Horizon uses the same signal to open its
/// software keyboard once per focus acquisition.
pub(crate) fn sync_window(state: &mut AppState) {
    let focused = if state.platform.window_active {
        focused_edit(state)
    } else {
        None
    };
    let active = focused.is_some();
    // Reopening when a *different* edit takes focus matters as much as the
    // first one: the software keyboard is a one-shot modal, so a player who
    // taps a second text field needs it raised again even though IME stayed
    // enabled the whole time.
    let identity = focused.as_ref().map(|edit| edit.identity);
    let changed =
        active != state.platform.ime_allowed || identity != state.platform.ime_focus;
    if changed {
        state.platform.ime_allowed = active;
        state.platform.ime_focus = identity;
        // A focused edit opens the keyboard pre-filled with its own content,
        // so confirming replaces the value instead of appending to it. The
        // setter is a Horizon-backend extension, absent from desktop winit.
        #[cfg(target_os = "horizon")]
        state.platform.window.set_ime_initial_text(
            focused.as_ref().map_or("", |edit| edit.text.as_str()),
        );
        state.platform.window.set_ime_allowed(active);
        if active {
            state.platform.ime_cursor = None;
        }
    }
    if !active {
        return;
    }
    let (x, y) = state.window_cursor_position();
    let cursor = (x.round() as i32, y.round() as i32);
    if state.platform.ime_cursor != Some(cursor) {
        state.platform.ime_cursor = Some(cursor);
        state.platform.window.set_ime_cursor_area(
            Position::Physical(PhysicalPosition::new(cursor.0, cursor.1)),
            Size::Physical(PhysicalSize::new(1, 1)),
        );
    }
}
