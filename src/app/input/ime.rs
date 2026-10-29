//! OS input-method admission and committed-text routing.
//!
//! Winit reports committed composition text separately from physical key
//! events. Route it through the same per-control insertion paths as ordinary
//! text so native limits, filters and notifications remain authoritative.

use crate::app::{App, AppState};
use crate::ui::game_screen::GameScreen;
use winit::dpi::{PhysicalPosition, PhysicalSize, Position, Size};

/// Whether an edit that accepts text currently owns focus.
pub(crate) fn text_input_active(state: &AppState) -> bool {
    if crate::app::frontend::shell_transition::blocks_shell_input(state) {
        return false;
    }
    if state.frontend.screen == GameScreen::InGame
        && state.match_state.match_presentation.message_list.editing()
    {
        return true;
    }
    if crate::app::input::coop_score::active(state) {
        return state
            .frontend
            .coop_score_page
            .as_ref()
            .is_some_and(|page| page.ui.chat.edit.focused);
    }
    if let Some(dialog) = state.frontend.network_options.as_ref() {
        return dialog.pending_message().is_none()
            && (dialog.socket.focused || dialog.destination.focused || dialog.port.focused);
    }
    if state.frontend.screen == GameScreen::MainMenu && state.frontend.shell_route.lan_lobby() {
        return state.frontend.lan.as_ref().is_some_and(|runtime| {
            runtime.modal.is_none()
                && runtime.lobby.enabled
                && (runtime.lobby.name.focused || runtime.lobby.chat.edit.focused)
        });
    }
    if state.frontend.screen == GameScreen::MainMenu
        && state.frontend.shell_route.network_setup()
        && state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .is_none()
    {
        return state.frontend.multiplayer_setup.as_ref().is_some_and(|ui| {
            ui.focused_control == crate::ui::multiplayer_setup::CHAT && ui.chat.edit.focused
        });
    }
    if App::native_skirmish_shell_active(state) {
        if let Some(browser) = state
            .frontend
            .skirmish_shell_state
            .saved_seed_browser
            .as_ref()
        {
            return browser.prompt.is_none() && browser.description_edit.focused;
        }
        if state.frontend.skirmish_shell_state.player_name_edit.focused {
            return true;
        }
    }
    if state.frontend.screen == GameScreen::InGame
        && matches!(
            state.match_state.match_presentation.in_game_menu,
            crate::ui::pause_menu::InGameMenuState::SavedGame(_)
        )
    {
        return state
            .match_state
            .match_presentation
            .saved_game_browser
            .as_ref()
            .is_some_and(|browser| browser.prompt.is_none() && browser.description_edit.focused);
    }
    if crate::app::shell_wol::text_input_active(state) {
        return true;
    }
    false
}

/// Insert one committed string into whichever edit owned focus when the IME
/// produced it. Returns true when an owner consumed the event.
pub(crate) fn commit(state: &mut AppState, text: &str) -> bool {
    if text.is_empty() || !text_input_active(state) {
        return false;
    }

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
    let active = state.platform.window_active && text_input_active(state);
    if active != state.platform.ime_allowed {
        state.platform.ime_allowed = active;
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
