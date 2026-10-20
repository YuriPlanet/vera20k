//! The page dialogs over the WOL shell: Page `0xCF`, opened from `0x113`,
//! C2 and C9, and Page Received `0x11E`, which the page sink opens over the
//! empty backdrop. The dialog states live on the visit; this module routes
//! their input, sends what they ask and writes their transcripts.

use std::time::Instant;

use winit::keyboard::KeyCode;

use super::service::WolServiceRequest;
use crate::app::{App, AppState};
use crate::ui::lan_shell::{LanChatLine, LanEditKey, LanListState, append_list_message};
use crate::ui::main_menu_dialogs::options::LauncherCue;
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::list::{ListMetrics, ShellListGeometry};
use crate::ui::shell::modal::{
    WolPageLayout, WolPageReceivedLayout, wol_page_layout, wol_page_received_layout,
};
use crate::ui::wol_chat::{self, WolChatLine};
use crate::ui::wol_page::*;

/// `[0x7EEF2C]`: `0x11E`'s own echo.
const REPLY_COLOR: u32 = 0xF0F0;

/// The shell's window, or in a match the frame the battlefield draws into
/// (the in-match `0xCF` centres over it, at its source size).
fn screen(state: &AppState) -> (i32, i32) {
    if state.frontend.screen == crate::app::GameScreen::InGame {
        (state.render_width() as i32, state.render_height() as i32)
    } else {
        (
            state.renderer.gpu.config.width as i32,
            state.renderer.gpu.config.height as i32,
        )
    }
}

pub(crate) fn page_layout(state: &AppState) -> WolPageLayout {
    let (width, height) = screen(state);
    wol_page_layout(width, height)
}

pub(crate) fn received_layout(state: &AppState) -> WolPageReceivedLayout {
    let (width, height) = screen(state);
    wol_page_received_layout(width, height)
}

/// The popups paint and hit-test at the retail scale.
fn list_geometry(rect: RectPx, list: &LanListState, count: usize) -> ShellListGeometry {
    ShellListGeometry::new(
        rect,
        count,
        list.top_index,
        ListMetrics::scaled(ShellScale::RETAIL),
    )
}

pub(crate) fn page_geometry(layout: &WolPageLayout, page: &WolPageDialog) -> ShellListGeometry {
    list_geometry(layout.transcript, &page.list, page.lines.len())
}

pub(crate) fn received_geometry(
    layout: &WolPageReceivedLayout,
    received: &WolPageReceived,
) -> ShellListGeometry {
    list_geometry(layout.transcript, &received.list, received.lines.len())
}

pub(crate) fn page_dialog(state: &AppState) -> Option<&WolPageDialog> {
    state.frontend.wol.as_ref()?.page_dialog.as_deref()
}

pub(crate) fn page_received(state: &AppState) -> Option<&WolPageReceived> {
    state.frontend.wol.as_ref()?.page_received.as_deref()
}

/// `0xCF` or `0x11E` takes the input.
pub(crate) fn open(state: &AppState) -> bool {
    page_dialog(state).is_some() || page_received(state).is_some()
}

/// `0x0078BA3E`, `0x00790C8D`, `0x0079235F` and, in a match, `0x0077D840`:
/// `UI__BeginDialog(0xCF)` and its modal loop. The input lock `[0xA8ED9C]`
/// and the keyboard clears around it have nothing to hold here: the dialog
/// takes every key while it exists.
pub(crate) fn open_page_dialog(state: &mut AppState) {
    let Some(visit) = state.frontend.wol.as_ref() else {
        return;
    };
    let last_pager = visit.last_pager.clone();
    let now = reply_window_now(crate::app::match_runtime::frame_pacer::wall_clock_ms());
    let globals = &mut state.frontend.wol_lobby_globals;
    let (page, actions) = WolPageDialog::open(
        &mut globals.page_reply_window,
        now,
        last_pager.as_deref(),
        &mut globals.page_last_control,
    );
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.page_dialog = Some(Box::new(page));
    }
    apply(state, actions);
}

/// State 7 (`State_Machine 0x0048CC2C`): the PageUser event's dialog over
/// the running match. The state machine's admission runs first; Cancel
/// ends the state (`0x0048CC31`).
pub(crate) fn open_in_match(state: &mut AppState) {
    if state.frontend.wol.is_none() {
        return;
    }
    App::enter_in_game_menu_state(state, InGameMenuState::Page);
    if state.match_state.match_presentation.in_game_menu == InGameMenuState::Page {
        // KeyboardClear 0x0054F720 at 0x0077D861 (and 0x0077D895 on close).
        state.match_state.input.clear_command_latches();
        open_page_dialog(state);
    }
}

/// `0xCF` closes; in a match its state ends with it.
pub(crate) fn close_page_dialog(state: &mut AppState) {
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.page_dialog = None;
    }
    if state.match_state.match_presentation.in_game_menu == InGameMenuState::Page {
        state.match_state.input.clear_command_latches();
        App::enter_in_game_menu_state(state, InGameMenuState::Closed);
    }
}

/// The observer gate `0x0078D52A..0x0078D566`: once the local player is
/// out (Session+0x300, `[0xA8B538]`), a name `_stricmp`-equal to any
/// connection node's refuses the page. Outside a match there are no nodes.
fn observer_cannot_page(state: &AppState, name: &str) -> bool {
    let out = state
        .match_state
        .sim_runtime
        .as_ref()
        .is_some_and(|runtime| runtime.simulation.local_player_defeated());
    let session = &state.match_state.session;
    let nodes: Vec<String> = crate::app::match_runtime::network_match::nodes(session)
        .into_iter()
        .map(|node| crate::app::match_runtime::network_match::participant_name(session, node))
        .collect();
    observer_refuses(out, &nodes, name)
}

/// `0x005E2820` into one of the dialogs' lists, wrapped to its width.
fn append_line(
    font: &crate::render::bit_font::BitFont,
    lines: &mut Vec<LanChatLine>,
    list: &mut LanListState,
    line: LanChatLine,
    width: i32,
    visible: usize,
) {
    append_list_message(lines, list, line, width, visible, |units| {
        font.text_width(&String::from_utf16_lossy(units)) as i32
    });
}

/// `0x005E2790` with `0xCF` current: the line goes to its list `0x5B5`.
/// Returns whether `0xCF` took it.
pub(crate) fn append_page_line(state: &mut AppState, line: LanChatLine) -> bool {
    let layout = page_layout(state);
    let font = &state.renderer.bit_font;
    let Some(page) = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.page_dialog.as_deref_mut())
    else {
        return false;
    };
    let visible = page_geometry(&layout, page).visible_rows;
    let width = layout.transcript.w;
    append_line(font, &mut page.lines, &mut page.list, line, width, visible);
    true
}

/// `0x005E2700` into `0x11E`'s `0x53F`.
pub(super) fn append_received_line(state: &mut AppState, line: LanChatLine) {
    let layout = received_layout(state);
    let font = &state.renderer.bit_font;
    let Some(received) = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.page_received.as_deref_mut())
    else {
        return;
    };
    let visible = received_geometry(&layout, received).visible_rows;
    let width = layout.transcript.w;
    append_line(
        font,
        &mut received.lines,
        &mut received.list,
        line,
        width,
        visible,
    );
}

/// The current window's `0x53F`, else its `0x5B5` (the find sink's tail
/// `0x007AA5D5`, the page-result sink's failure branch `0x007A9800`):
/// `0xCF`'s list, `0x11E`'s, or the lobby's or C2/C9's transcript; with
/// neither, in a match (Session+0x30D8, `0x0069BBE0`), a Grey MessageList
/// row. Returns false when nothing took the line.
pub(super) fn current_window_line(state: &mut AppState, line: LanChatLine) -> bool {
    if page_dialog(state).is_some() {
        return append_page_line(state, line);
    }
    if page_received(state).is_some() {
        append_received_line(state, line);
        return true;
    }
    let Some(in_game) = state.frontend.wol.as_ref().map(|visit| visit.in_game) else {
        return false;
    };
    if in_game {
        let text = String::from_utf16_lossy(&line.text);
        crate::app::input::messages::post_wol_result_row(state, &text);
        return true;
    }
    if state.frontend.shell_route.network_setup() || App::wol_page_is_lobby(state) {
        crate::app::shell_lan::append_chat(state, line);
        return true;
    }
    false
}

/// The page sinks' last resort (`0x007A945A..0x007A94E8`): no dialog in
/// the stack has a `0x53F`, so `0x11E` opens modeless over the empty
/// backdrop (`0x0052FEC0(1)`) unless it exists, takes the line and names
/// its pager in `0x718`.
/// RESIDUAL: native keeps the page below (`0x10E`) live under the modeless
/// dialog, its windows hit-testable and repainting on hover; here `0x11E`
/// takes every input until Back and the page repaints after it.
pub(super) fn page_received_line(state: &mut AppState, line: LanChatLine) {
    let fallback = state.frontend.wol_buddies.selected.clone();
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    if visit.page_received.is_none() {
        visit.page_received = Some(Box::new(WolPageReceived::new(&fallback, false)));
    }
    append_received_line(state, line);
    if let Some(received) = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.page_received.as_mut())
    {
        received.reply_to = received.pager_from_list();
    }
    state.platform.window.request_redraw();
}

/// `0x116`'s Page (`0x0079EA23..0x0079EA75`): `0x11E` created over the
/// Buddy List and run modally (`0x007759E0` with the pump `0x0077F900`).
/// Its list is empty, so its reply-to is the selected buddy.
pub(crate) fn open_received_modal(state: &mut AppState) {
    let fallback = state.frontend.wol_buddies.selected.clone();
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    let mut received = WolPageReceived::new(&fallback, true);
    received.reply_to = received.pager_from_list();
    visit.page_received = Some(Box::new(received));
    state.platform.window.request_redraw();
}

/// The find sink with `0xCF` current (`0x007AA25D`): the status `0x5C8`
/// takes the room or a `TXT_FINDPAGE_*` text. Returns whether `0xCF` took
/// the result.
pub(super) fn located(
    state: &mut AppState,
    result: &Result<crate::ui::wol_lobby::WolChannel, u32>,
) -> bool {
    if page_dialog(state).is_none() {
        return false;
    }
    let text = match result {
        Ok(channel) => match channel.lobby_number().filter(|_| channel.kind == 0) {
            Some(number) => crate::ui::wol_lobby::WolLobbyRow::joined_name(
                number,
                &super::lobby::base_names(state),
            ),
            None => channel.name.clone(),
        },
        Err(0x0004_0131) => App::csf_label(
            state,
            "TXT_FINDPAGE_NOT_IN_CHANNEL",
            "TXT_FINDPAGE_NOT_IN_CHANNEL",
        ),
        Err(0x0004_0132) => App::csf_label(
            state,
            "TXT_FINDPAGE_FIND_DISABLED",
            "TXT_FINDPAGE_FIND_DISABLED",
        ),
        Err(_) => App::csf_label(state, "TXT_FINDPAGE_NOT_ONLINE", "TXT_FINDPAGE_NOT_ONLINE"),
    };
    if let Some(page) = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.page_dialog.as_mut())
    {
        page.status = text;
    }
    state.platform.window.request_redraw();
    true
}

fn request(state: &mut AppState, request: WolServiceRequest) {
    super::service::request(state, request);
}

fn locate(state: &mut AppState, name: String) {
    // API+0x60 without [0xB76E28]: the 0xCF status shows the answer.
    if state.frontend.wol.as_ref().is_some_and(|visit| visit.api) {
        request(
            state,
            WolServiceRequest::Chat {
                line: WolChatLine::Locate {
                    name: name.encode_utf16().collect(),
                },
                to: Vec::new(),
            },
        );
    }
}

/// API+0xF8 with the record and text.
fn send_page(state: &mut AppState, name: &str, text: &[u16]) {
    request(
        state,
        WolServiceRequest::Chat {
            line: WolChatLine::Page {
                name: name.encode_utf16().collect(),
                text: text.to_vec(),
            },
            to: Vec::new(),
        },
    );
}

/// `u"[%hs] %s"` (`0x0084B94C`) with the login nickname `[0xB73E40]`.
fn echo_line(state: &AppState, text: &[u16], color: u32) -> LanChatLine {
    let mut line: Vec<u16> = format!("[{}] ", state.frontend.wol_account.nickname)
        .encode_utf16()
        .collect();
    line.extend_from_slice(text);
    LanChatLine { text: line, color }
}

fn apply(state: &mut AppState, actions: Vec<WolPageAction>) {
    for action in actions {
        match action {
            WolPageAction::Locate(name) => locate(state, name),
            WolPageAction::Send { name, text } => {
                // 0x0078D48F: no WOLAPI object, nothing past the clear.
                if !state.frontend.wol.as_ref().is_some_and(|visit| visit.api) {
                    continue;
                }
                if observer_cannot_page(state, &name) {
                    notice(state, "TXT_OBSERVER_CANT_PAGE");
                    continue;
                }
                send_page(state, &name, &text);
                let line = echo_line(state, &text, wol_chat::SAY_COLOR);
                append_page_line(state, line);
            }
            WolPageAction::Notice(key) => notice(state, key),
            WolPageAction::Close => close_page_dialog(state),
        }
    }
    state.platform.window.request_redraw();
}

/// A white (-1) CSF line through `0x005E2790`.
fn notice(state: &mut AppState, key: &str) {
    let text = App::csf_label(state, key, key).encode_utf16().collect();
    append_page_line(
        state,
        LanChatLine {
            text,
            color: u32::MAX,
        },
    );
}

fn apply_received(state: &mut AppState, action: Option<WolPageReceivedAction>) {
    match action {
        Some(WolPageReceivedAction::Send { name, text }) => {
            send_page(state, &name, &text);
            let line = echo_line(state, &text, REPLY_COLOR);
            append_received_line(state, line);
        }
        Some(WolPageReceivedAction::Close) => {
            if let Some(visit) = state.frontend.wol.as_mut() {
                visit.page_received = None;
            }
        }
        None => {}
    }
    state.platform.window.request_redraw();
}

/// `[0xB77E8C]`, process global.
fn with_last<R>(
    state: &mut AppState,
    f: impl FnOnce(&mut WolPageDialog, &mut u16) -> R,
) -> Option<R> {
    let mut last = state.frontend.wol_lobby_globals.page_last_control;
    let page = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.page_dialog.as_deref_mut())?;
    let result = f(page, &mut last);
    state.frontend.wol_lobby_globals.page_last_control = last;
    Some(result)
}

fn page_control_at(layout: &WolPageLayout, x: i32, y: i32) -> Option<u16> {
    [
        (SEND, layout.send),
        (CANCEL, layout.cancel),
        (NAME, layout.name),
        (TEXT, layout.text),
        (PAGE_TRANSCRIPT, layout.transcript),
    ]
    .into_iter()
    .find(|(_, rect)| rect.contains(x, y))
    .map(|(id, _)| id)
}

fn received_control_at(layout: &WolPageReceivedLayout, x: i32, y: i32) -> Option<u16> {
    [
        (SEND, layout.send),
        (BACK, layout.back),
        (TEXT, layout.reply),
        (RECEIVED_TRANSCRIPT, layout.transcript),
    ]
    .into_iter()
    .find(|(_, rect)| rect.contains(x, y))
    .map(|(id, _)| id)
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    let (x, y) = state.match_state.input.cursor_px();
    if page_dialog(state).is_some() {
        let layout = page_layout(state);
        let over = page_control_at(&layout, x, y).filter(|id| matches!(*id, SEND | CANCEL));
        if let Some(page) = state
            .frontend
            .wol
            .as_mut()
            .and_then(|visit| visit.page_dialog.as_deref_mut())
        {
            page.buttons.hovered = over;
            let geometry = page_geometry(&layout, page);
            page.list
                .scroll
                .pointer_moved(geometry, &mut page.list.top_index, x, y);
        }
    } else if page_received(state).is_some() {
        let layout = received_layout(state);
        let over = received_control_at(&layout, x, y).filter(|id| matches!(*id, SEND | BACK));
        if let Some(received) = state
            .frontend
            .wol
            .as_mut()
            .and_then(|visit| visit.page_received.as_deref_mut())
        {
            received.buttons.hovered = over;
            let geometry = received_geometry(&layout, received);
            received.list.hot_row =
                geometry.row_at(received.lines.len(), received.list.top_index, x, y);
            received
                .list
                .scroll
                .pointer_moved(geometry, &mut received.list.top_index, x, y);
        }
    }
    state.platform.window.request_redraw();
}

pub(crate) fn mouse(state: &mut AppState, pressed: bool) {
    let (x, y) = state.match_state.input.cursor_px();
    if page_dialog(state).is_some() {
        let layout = page_layout(state);
        let over = page_control_at(&layout, x, y);
        if pressed {
            let mut button = false;
            let actions = with_last(state, |page, last| {
                let geometry = page_geometry(&layout, page);
                match over {
                    Some(PAGE_TRANSCRIPT) => {
                        if let Some(part) = geometry.scroll_part_at(x, y) {
                            page.list.scroll.press(
                                part,
                                geometry,
                                &mut page.list.top_index,
                                y,
                                Instant::now(),
                            );
                            Vec::new()
                        } else if geometry
                            .row_at(page.lines.len(), page.list.top_index, x, y)
                            .is_some()
                        {
                            page.press_transcript_row(last)
                        } else {
                            Vec::new()
                        }
                    }
                    Some(SEND) if !page.send_enabled => Vec::new(),
                    Some(id @ (SEND | CANCEL)) => {
                        page.buttons.press(Some(id));
                        button = true;
                        Vec::new()
                    }
                    Some(id) => page.press(id, last),
                    None => {
                        page.buttons.press(None);
                        Vec::new()
                    }
                }
            })
            .unwrap_or_default();
            if button {
                App::play_main_menu_button_sound(state);
            }
            apply(state, actions);
        } else {
            let over = over.filter(|id| matches!(*id, SEND | CANCEL));
            let actions = with_last(state, |page, last| {
                page.list.scroll.cancel();
                match page.buttons.release(over) {
                    Some(id) => page.activate(id, last),
                    None => Vec::new(),
                }
            })
            .unwrap_or_default();
            apply(state, actions);
        }
        return;
    }
    let layout = received_layout(state);
    let over = received_control_at(&layout, x, y);
    let mut button = false;
    let mut row_click = false;
    let action = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.page_received.as_deref_mut())
        .and_then(|received| {
            let geometry = received_geometry(&layout, received);
            if !pressed {
                received.list.scroll.cancel();
                let over = over.filter(|id| matches!(*id, SEND | BACK));
                return received
                    .buttons
                    .release(over)
                    .and_then(|id| received.activate(id));
            }
            match over {
                Some(SEND) if !received.send_enabled => {}
                Some(id @ (SEND | BACK)) => {
                    received.buttons.press(Some(id));
                    button = true;
                }
                Some(RECEIVED_TRANSCRIPT) => {
                    if let Some(part) = geometry.scroll_part_at(x, y) {
                        received.list.scroll.press(
                            part,
                            geometry,
                            &mut received.list.top_index,
                            y,
                            Instant::now(),
                        );
                    } else if let Some(row) =
                        geometry.row_at(received.lines.len(), received.list.top_index, x, y)
                    {
                        // A selectable owner-draw list clicks like the lobby's.
                        received.select(row);
                        row_click = true;
                    }
                }
                Some(id) => received.press(id),
                None => received.buttons.press(None),
            }
            None
        });
    if button {
        App::play_main_menu_button_sound(state);
    }
    if row_click {
        App::play_launcher_options_cue(state, LauncherCue::GenericClick);
    }
    apply_received(state, action);
}

fn edit_key(code: Option<KeyCode>) -> Option<Option<LanEditKey>> {
    Some(match code {
        Some(KeyCode::Backspace) => Some(LanEditKey::Backspace),
        Some(KeyCode::Delete) => Some(LanEditKey::Delete),
        Some(KeyCode::ArrowLeft) => Some(LanEditKey::Left),
        Some(KeyCode::ArrowRight) => Some(LanEditKey::Right),
        Some(KeyCode::Home) => Some(LanEditKey::Home),
        Some(KeyCode::End) => Some(LanEditKey::End),
        Some(KeyCode::Tab) => Some(LanEditKey::Tab),
        Some(KeyCode::Enter | KeyCode::NumpadEnter) => Some(LanEditKey::Enter),
        // Neither loop runs IsDialogMessage: Escape does nothing.
        Some(KeyCode::Escape) => return None,
        _ => None,
    })
}

pub(crate) fn key_press(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
    let Some(key) = edit_key(code) else {
        return;
    };
    if page_dialog(state).is_some() {
        let actions = with_last(state, |page, last| match key {
            Some(key) => page.key(key, last),
            None => page.insert_text(text.unwrap_or_default(), last),
        })
        .unwrap_or_default();
        apply(state, actions);
        return;
    }
    let action = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.page_received.as_deref_mut())
        .and_then(|received| match key {
            Some(key) => received.key(key),
            None => {
                received.insert_text(text.unwrap_or_default());
                None
            }
        });
    apply_received(state, action);
}

/// The transcripts' scrollbar repeats.
pub(super) fn poll_scroll_repeat(state: &mut AppState) -> Option<Instant> {
    let (x, y) = state.match_state.input.cursor_px();
    let now = Instant::now();
    let (moved, repeat) = if page_dialog(state).is_some() {
        let layout = page_layout(state);
        let page = state
            .frontend
            .wol
            .as_mut()
            .and_then(|visit| visit.page_dialog.as_deref_mut())?;
        let geometry = page_geometry(&layout, page);
        let moved = page
            .list
            .scroll
            .poll(geometry, &mut page.list.top_index, x, y, now);
        (moved, page.list.scroll.repeat_at())
    } else {
        let layout = received_layout(state);
        let received = state
            .frontend
            .wol
            .as_mut()
            .and_then(|visit| visit.page_received.as_deref_mut())?;
        let geometry = received_geometry(&layout, received);
        let moved = received
            .list
            .scroll
            .poll(geometry, &mut received.list.top_index, x, y, now);
        (moved, received.list.scroll.repeat_at())
    };
    if moved {
        state.platform.window.request_redraw();
    }
    repeat
}
