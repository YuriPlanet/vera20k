//! Dialog `0x113`'s geometry and input: its children and the popups on
//! their layouts, key presses and the pointer, all through the lobby's
//! widget tree, with the effects they ask of the app.

use crate::app::frontend::shell_transition;
use crate::app::{App, AppState};
use crate::ui::shell::geom::ShellScale;
use crate::ui::wol_lobby::*;
use winit::keyboard::KeyCode;

use super::{WolLobby, dispatch, layout, lobby, lobby_mut, modern_lobby};
use crate::render::gpu::GpuContext;

/// Puts `0x113`'s children and the open popup's on their layouts, the
/// lists in range and the edits' carets in view (text widths in the box's
/// authored pixels).
pub(crate) fn sync(state: &mut AppState) {
    let layout = layout(state);
    let create = create_layout(state);
    let password = password_layout(state);
    let filter = state.frontend.wol_lobby_globals.room_filter;
    let font = &state.renderer.bit_font;
    let Some(lobby) = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.lobby.as_deref_mut())
    else {
        return;
    };
    let ui = &mut lobby.ui;
    ui.sync(&layout);
    ui.set_filter(filter);
    let create_popup = ui
        .popup
        .as_ref()
        .is_some_and(|popup| popup.create.is_some());
    ui.sync_popup(|id| match create_popup {
        true => create_rect(&create, id),
        false => password.rect_of(id),
    });
    ui.scroll_edits(&|text| font.text_width(text));
    super::player_card::sync(state);
}

/// The NewEdit that takes keys, for the input method (`app::input::ime`):
/// `handle_wol_key` reaches [`key_press`] with the lobby page up, no page
/// dialog and no popup. It is the chat edit while the chat shows, else VERA's
/// room search.
pub(crate) fn ime_edit_rect(state: &AppState) -> Option<crate::ui::shell::geom::RectPx> {
    let routed = crate::app::input::precedence::route_page(&state.frontend)
        == crate::app::input::precedence::ScreenPage::Wol
        && !shell_transition::blocks_shell_input(state)
        && !App::wol_quick_match_verify(state)
        && !super::page::open(state.frontend.wol.as_ref())
        && App::wol_page_is_lobby(state.frontend.wol.as_ref());
    lobby(state.frontend.wol.as_ref())
        .filter(|lobby| {
            routed && lobby.interactive() && lobby.ui.popup.is_none() && lobby.ui.card.is_none()
        })
        .map(|lobby| {
            let edit = if lobby.ui.shows_chat() { CHAT } else { SEARCH };
            layout(state).control_rect(edit).unwrap_or_default()
        })
}

/// What the lobby's widgets did: their cues, a player card opened and the
/// dialog's actions, in order.
fn apply(state: &mut AppState, effects: Vec<WolLobbyEffect>) {
    for effect in effects {
        match effect {
            WolLobbyEffect::Cue(cue) => App::play_page_cue(
                &state.frontend,
                &state.match_state,
                &state.process_assets,
                &mut state.audio.sfx_player,
                cue,
            ),
            WolLobbyEffect::Card(name) => super::player_card::opened(
                state.frontend.wol.as_mut(),
                &state.platform.window,
                &name,
            ),
            WolLobbyEffect::CardButton(id) => super::player_card::button(state, id),
            WolLobbyEffect::CardCountry(row) => super::player_card::country_picked(
                &mut state.frontend,
                state.platform.game_config.as_ref(),
                &mut state.persistence.options_profile,
                row,
            ),
            WolLobbyEffect::CardDismissed => {
                super::player_card::dismissed(&mut state.frontend, &state.platform.window)
            }
            WolLobbyEffect::Action(action) => dispatch(state, action),
        }
    }
    state.frontend.shell_status_line.repaint();
}

pub(crate) fn key_press(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
    use vera20k_gui::{InputEvent, Key};
    if shell_transition::blocks_shell_input(state)
        || !lobby(state.frontend.wol.as_ref()).is_some_and(WolLobby::interactive)
        || super::player_card::key_press(state.frontend.wol.as_ref(), code)
    {
        return;
    }
    // The edits take no vertical keys.
    let shift = state.platform.live_modifiers.shift_key();
    let (key, text) = match crate::ui::shell::controller::gui_input(code, shift, text) {
        Some(InputEvent::Key(Key::Up | Key::Down | Key::PageUp | Key::PageDown)) | None => {
            (None, None)
        }
        Some(InputEvent::Key(key)) => (Some(key), None),
        Some(InputEvent::Text(text)) => (None, Some(text)),
        Some(_) => (None, None),
    };
    sync(state);
    let effects = lobby_mut(state.frontend.wol.as_mut())
        .expect("checked")
        .ui
        .key(key, text.as_deref());
    sync(state);
    apply(state, effects);
}

/// The popups follow the lobby page's shell mapping, so they grow with it.
pub(crate) fn popup_scale(state: &AppState) -> ShellScale {
    layout(state).page.scale
}

fn popup_screen(gpu: &GpuContext) -> (i32, i32) {
    (gpu.config.width as i32, gpu.config.height as i32)
}

pub(crate) fn create_layout(state: &AppState) -> crate::ui::shell::modal::WolCreateGameLayout {
    let (width, height) = popup_screen(&state.renderer.gpu);
    crate::ui::shell::modal::wol_create_game_layout_on(
        width,
        height,
        modern_lobby(state.frontend.wol.as_ref()),
        popup_scale(state),
    )
}

pub(crate) fn password_layout(state: &AppState) -> crate::ui::shell::modal::WolPageLayout {
    let (width, height) = popup_screen(&state.renderer.gpu);
    crate::ui::shell::modal::wol_channel_password_layout_on(width, height, popup_scale(state))
}

/// The pointer moved, pressed or released over the popup, else the open
/// player card, else `0x113`'s children.
fn pointer(state: &mut AppState, press: Option<bool>) {
    sync(state);
    let (x, y) = state.match_state.input.cursor_px();
    let Some(lobby) = lobby_mut(state.frontend.wol.as_mut()) else {
        return;
    };
    let effects = lobby.ui.pointer(x, y, press);
    apply(state, effects);
}

pub(in crate::app::shell_wol) fn cursor_moved(state: &mut AppState) {
    pointer(state, None);
}

pub(in crate::app::shell_wol) fn mouse(state: &mut AppState, pressed: bool) {
    if shell_transition::blocks_shell_input(state)
        || !lobby(state.frontend.wol.as_ref()).is_some_and(WolLobby::interactive)
    {
        return;
    }
    pointer(state, Some(pressed));
}
