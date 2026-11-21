//! Dialog `0x113`'s geometry and input: the popups' layouts and controls,
//! key presses, the pointer and mouse presses with their sounds.

use crate::app::frontend::shell_transition;
use crate::app::{App, AppState};
use crate::ui::lan_shell::LanEditKey;
use crate::ui::main_menu_dialogs::options::LauncherCue;
use crate::ui::room_limits::RoomLimitsPress;
use crate::ui::shell::geom::ShellScale;
use crate::ui::wol_lobby::*;
use crate::util::time::Instant;
use winit::keyboard::KeyCode;

use super::{WolLobby, chat_layout, dispatch, layout, lobby, lobby_mut, modern_lobby};

pub(crate) fn update_geometry(state: &mut AppState) {
    let layout = layout(state);
    // The chat keeps its own view's log and edit while the games cover it,
    // so its scroll and the edit's text survive the swap.
    let chat = chat_layout(state);
    let create_rects = create_layout(state);
    let password_rect = password_layout(state).password;
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
    let scale = layout.page.scale;
    for (list, rect, count) in [
        (&mut ui.lobby_list, layout.lobbies, ui.lobby_rows.len()),
        (&mut ui.game_list, layout.games, ui.game_rows.len()),
        (&mut ui.player_list, layout.players, ui.player_rows.len()),
        (&mut ui.chat.list, chat.chat_log, ui.chat.lines.len()),
    ] {
        list.reconcile_geometry(rect, count, scale);
    }
    ui.chat
        .edit
        .scroll_to_caret((scale.base_rect(chat.chat).w - 3).max(0) as u32, |text| {
            font.text_width(text)
        });
    if let Some(popup) = ui.popup.as_mut() {
        // Text widths are in the box's authored pixels.
        let available =
            |rect: crate::ui::shell::geom::RectPx| (scale.base_rect(rect).w - 3).max(0) as u32;
        match &mut popup.kind {
            WolPopupKind::Create(create) => {
                create
                    .password
                    .scroll_to_caret(available(create_rects.password), |text| {
                        font.text_width(text)
                    });
                if let (Some(name), Some((_, rect))) = (create.name.as_mut(), create_rects.name) {
                    name.scroll_to_caret(available(rect), |text| font.text_width(text));
                }
            }
            WolPopupKind::Password(edit) => {
                edit.scroll_to_caret(available(password_rect), |text| font.text_width(text))
            }
        }
    }
}

/// The chat NewEdit while it takes keys, for the input method
/// (`app::input::ime`): `handle_wol_key` reaches [`key_press`] with the lobby
/// page up, no page dialog, no popup and the chat showing.
pub(crate) fn ime_chat_rect(state: &AppState) -> Option<crate::ui::shell::geom::RectPx> {
    let routed = App::wol_active(state)
        && !shell_transition::blocks_shell_input(state)
        && !App::wol_quick_match_verify(state)
        && !super::page::open(state)
        && App::wol_page_is_lobby(state);
    lobby(state)
        .filter(|lobby| {
            routed
                && lobby.interactive()
                && lobby.ui.popup.is_none()
                && lobby.ui.card.is_none()
                && lobby.ui.shows_chat()
        })
        .map(|_| layout(state).chat)
}

pub(crate) fn key_press(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
    if shell_transition::blocks_shell_input(state)
        || !lobby(state).is_some_and(WolLobby::interactive)
        || super::player_card::key_press(state, code)
    {
        return;
    }
    let key = match code {
        Some(KeyCode::Backspace) => Some(LanEditKey::Backspace),
        Some(KeyCode::Delete) => Some(LanEditKey::Delete),
        Some(KeyCode::ArrowLeft) => Some(LanEditKey::Left),
        Some(KeyCode::ArrowRight) => Some(LanEditKey::Right),
        Some(KeyCode::Home) => Some(LanEditKey::Home),
        Some(KeyCode::End) => Some(LanEditKey::End),
        Some(KeyCode::Tab) => Some(LanEditKey::Tab),
        Some(KeyCode::Enter | KeyCode::NumpadEnter) => Some(LanEditKey::Enter),
        // The NewEdits consume Escape; 0x113 has no IDCANCEL case.
        Some(KeyCode::Escape) => return,
        _ => None,
    };
    let lobby = lobby_mut(state).expect("checked");
    let action = match key {
        Some(key) => lobby.ui.edit_key(key),
        None => {
            lobby.ui.insert_text(text.unwrap_or_default());
            None
        }
    };
    update_geometry(state);
    if let Some(action) = action {
        dispatch(state, action);
    }
    state.platform.window.request_redraw();
}

/// The popup control under the pointer.
fn popup_control(state: &AppState, popup: &WolLobbyPopup) -> Option<u16> {
    let (x, y) = state.match_state.input.cursor_px();
    let controls = match popup.kind {
        WolPopupKind::Create(_) => {
            let layout = create_layout(state);
            [
                Some((POPUP_OK, layout.ok)),
                Some((POPUP_CANCEL, layout.cancel)),
                layout.clan.map(|rect| (CREATE_CLAN, rect)),
                Some((CREATE_LIMIT, layout.limit)),
                layout.observe.map(|rect| (CREATE_OBSERVE, rect)),
                Some((POPUP_PASSWORD, layout.password)),
                layout.name.map(|(_, rect)| (CREATE_NAME, rect)),
                layout.ranked.map(|rect| (CREATE_RANKED, rect)),
                layout.watching.map(|rect| (CREATE_WATCHING, rect)),
            ]
            .into_iter()
            .flatten()
            .map(|(id, rect)| {
                if layout.name.is_some() && !matches!(id, POPUP_OK | POPUP_CANCEL) {
                    let label = layout.setting_label(rect);
                    (
                        id,
                        crate::ui::shell::geom::RectPx::new(
                            label.x,
                            rect.y,
                            rect.x + rect.w - label.x,
                            rect.h,
                        ),
                    )
                } else {
                    (id, rect)
                }
            })
            .collect()
        }
        WolPopupKind::Password(_) => {
            let layout = password_layout(state);
            vec![
                (POPUP_OK, layout.ok),
                (POPUP_CANCEL, layout.cancel),
                (POPUP_PASSWORD, layout.password),
            ]
        }
    };
    controls
        .into_iter()
        .find(|(_, rect)| rect.contains(x, y))
        .map(|(id, _)| id)
}

/// The popups follow the lobby page's shell mapping, so they grow with it.
pub(crate) fn popup_scale(state: &AppState) -> ShellScale {
    layout(state).page.scale
}

fn popup_screen(state: &AppState) -> (i32, i32) {
    (
        state.renderer.gpu.config.width as i32,
        state.renderer.gpu.config.height as i32,
    )
}

pub(crate) fn create_layout(state: &AppState) -> crate::ui::shell::modal::WolCreateGameLayout {
    let (width, height) = popup_screen(state);
    crate::ui::shell::modal::wol_create_game_layout_on(
        width,
        height,
        modern_lobby(state),
        popup_scale(state),
    )
}

fn password_layout(state: &AppState) -> crate::ui::shell::modal::WolChannelPasswordLayout {
    let (width, height) = popup_screen(state);
    crate::ui::shell::modal::wol_channel_password_layout_on(width, height, popup_scale(state))
}

/// Where VERA's online Create Game has its room's limits.
fn popup_room_layout(state: &AppState) -> Option<crate::ui::room_limits::RoomLimitsLayout> {
    create_layout(state).room
}

pub(in crate::app::shell_wol) fn cursor_moved(state: &mut AppState) {
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let popup_over = lobby(state)
        .and_then(|lobby| lobby.ui.popup.as_ref())
        .map(|popup| popup_control(state, popup));
    let room = popup_room_layout(state);
    let popup_scale = popup_scale(state);
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    if let (Some(over), Some(popup)) = (popup_over, lobby.ui.popup.as_mut()) {
        popup.buttons.hovered = over;
        if let (Some(picker), Some(room)) = (popup.room_mut(), room) {
            picker.drag(&room, popup_scale, x);
        }
        state.platform.window.request_redraw();
        return;
    }
    if super::player_card::cursor_moved(state) {
        return;
    }
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let ui = &mut lobby.ui;
    ui.buttons.hovered = layout.control_at(x, y);
    let scale = layout.page.scale;
    for (list, rect, count) in [
        (&mut ui.lobby_list, layout.lobbies, ui.lobby_rows.len()),
        (&mut ui.game_list, layout.games, ui.game_rows.len()),
        (&mut ui.player_list, layout.players, ui.player_rows.len()),
        (&mut ui.chat.list, layout.chat_log, ui.chat.lines.len()),
    ] {
        let geometry = list.geometry(rect, count, scale);
        list.hot_row = geometry.row_at(count, list.top_index, x, y);
        list.scroll
            .pointer_moved(geometry, &mut list.top_index, x, y);
    }
    state.frontend.shell_status_line.repaint();
    state.platform.window.request_redraw();
}

enum Press {
    Nothing,
    Button,
    Cue(LauncherCue),
}

pub(in crate::app::shell_wol) fn mouse(state: &mut AppState, pressed: bool) {
    if shell_transition::blocks_shell_input(state)
        || !lobby(state).is_some_and(WolLobby::interactive)
    {
        return;
    }
    update_geometry(state);
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let scale = layout.page.scale;
    if let Some(popup) = lobby(state).and_then(|lobby| lobby.ui.popup.as_ref()) {
        let over = popup_control(state, popup);
        let rects = create_layout(state);
        let lobby = lobby_mut(state).expect("checked");
        let popup = lobby.ui.popup.as_mut().expect("checked");
        if !pressed {
            if let Some(picker) = popup.room_mut() {
                picker.release();
            }
            let action = popup
                .buttons
                .release(over)
                .and_then(|control| popup.activate(control));
            if let Some(action) = action {
                dispatch(state, WolLobbyAction::Popup(action));
            }
            state.platform.window.request_redraw();
            return;
        }
        let picked = popup
            .room_mut()
            .zip(rects.room)
            .map(|(picker, room)| picker.press(&room, scale, x, y));
        let press = match (picked, over) {
            (Some(RoomLimitsPress::Players), _) => Press::Nothing,
            (Some(RoomLimitsPress::Spectators), _) => Press::Cue(LauncherCue::Checkbox),
            (
                _,
                Some(
                    control @ (CREATE_CLAN | CREATE_LIMIT | CREATE_OBSERVE | CREATE_RANKED
                    | CREATE_WATCHING),
                ),
            ) => {
                let window = match control {
                    CREATE_CLAN => rects.clan,
                    CREATE_LIMIT => Some(rects.limit),
                    CREATE_RANKED => rects.ranked,
                    CREATE_WATCHING => rects.watching,
                    _ => rects.observe,
                };
                if window.is_some_and(|window| {
                    crate::ui::skirmish_shell::checkbox_icon_rect(window, scale).contains(x, y)
                }) && popup.toggle(control)
                {
                    Press::Cue(LauncherCue::Checkbox)
                } else {
                    Press::Nothing
                }
            }
            (_, Some(control @ (POPUP_OK | POPUP_CANCEL))) if popup.enabled(control) => {
                popup.buttons.press(Some(control));
                Press::Button
            }
            (_, Some(control @ (POPUP_PASSWORD | CREATE_NAME))) => {
                popup.focus(control);
                popup.buttons.press(None);
                Press::Nothing
            }
            _ => {
                popup.buttons.press(None);
                Press::Nothing
            }
        };
        play(state, press);
        state.platform.window.request_redraw();
        return;
    }
    if super::player_card::mouse(state, pressed) {
        return;
    }
    let over = layout.control_at(x, y);
    let lobby = lobby_mut(state).expect("checked");
    let ui = &mut lobby.ui;
    if !pressed {
        for list in [
            &mut ui.lobby_list,
            &mut ui.game_list,
            &mut ui.player_list,
            &mut ui.chat.list,
        ] {
            list.scroll.cancel();
        }
        let action = ui
            .buttons
            .release(over)
            .and_then(|control| ui.activate(control));
        if let Some(action) = action {
            dispatch(state, action);
        }
        state.platform.window.request_redraw();
        return;
    }
    let now = Instant::now();
    let mut action = None;
    let mut card = None;
    let mut press = Press::Nothing;
    let mut scrolled = false;
    let enabled = [LOBBIES, GAMES, PLAYERS, CHAT_LOG].map(|id| ui.enabled(id));
    for (enabled, list, rect, count) in [
        (
            enabled[0],
            &mut ui.lobby_list,
            layout.lobbies,
            ui.lobby_rows.len(),
        ),
        (
            enabled[1],
            &mut ui.game_list,
            layout.games,
            ui.game_rows.len(),
        ),
        (
            enabled[2],
            &mut ui.player_list,
            layout.players,
            ui.player_rows.len(),
        ),
        (
            enabled[3],
            &mut ui.chat.list,
            layout.chat_log,
            ui.chat.lines.len(),
        ),
    ] {
        if enabled {
            let geometry = list.geometry(rect, count, scale);
            if let Some(part) = geometry.scroll_part_at(x, y) {
                list.scroll
                    .press(part, geometry, &mut list.top_index, y, now);
                scrolled = true;
                break;
            }
        }
    }
    if !scrolled {
        match over {
            Some(control @ (LOBBIES | GAMES | PLAYERS)) => {
                let (rect, count, top) = match control {
                    LOBBIES => (layout.lobbies, ui.lobby_rows.len(), ui.lobby_list.top_index),
                    GAMES => (layout.games, ui.game_rows.len(), ui.game_list.top_index),
                    _ => (
                        layout.players,
                        ui.player_rows.len(),
                        ui.player_list.top_index,
                    ),
                };
                let geometry = crate::ui::shell::list::ShellListGeometry::new(
                    rect,
                    count,
                    top,
                    crate::ui::shell::list::ListMetrics::scaled(scale),
                );
                if let Some(row) = geometry.row_at(count, top, x, y) {
                    action = match control {
                        LOBBIES => ui.select_lobby(row),
                        GAMES => {
                            let double = crate::ui::shell::list::is_double_click(
                                &mut ui.game_list.last_press,
                                now,
                                x,
                                y,
                                crate::ui::shell::saved_file_input::host_double_click_limits(),
                            );
                            ui.select_game(row, double)
                        }
                        _ => {
                            card = ui.press_player(row);
                            None
                        }
                    };
                    press = Press::Cue(LauncherCue::GenericClick);
                }
            }
            Some(control @ (OBSERVER_IMAGE | OBSERVER_TOGGLE)) => {
                let icon_hit = control == OBSERVER_IMAGE
                    || crate::ui::skirmish_shell::checkbox_icon_rect(layout.observer_toggle, scale)
                        .contains(x, y);
                if icon_hit {
                    action = ui.activate(control);
                    press = Press::Cue(LauncherCue::Checkbox);
                }
            }
            Some(control) if control != CHAT && control != CHAT_LOG && ui.enabled(control) => {
                ui.buttons.press(Some(control));
                press = Press::Button;
            }
            _ => ui.buttons.press(None),
        }
    }
    play(state, press);
    if let Some(name) = card {
        super::player_card::opened(state, &name);
    }
    if let Some(action) = action {
        dispatch(state, action);
    }
    state.platform.window.request_redraw();
}

fn play(state: &mut AppState, press: Press) {
    match press {
        Press::Cue(cue) => App::play_launcher_options_cue(state, cue),
        // Owner-draw buttons play GUIMainButtonSound on the press.
        Press::Button => App::play_main_menu_button_sound(state),
        Press::Nothing => {}
    }
}
