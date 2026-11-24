//! The page dialogs over the WOL shell: Page `0xCF`, opened from `0x113`,
//! C2 and C9, and Page Received `0x11E`, which the page sink opens over the
//! empty backdrop. Their rows and statics live on the visit and their
//! widgets in the shell stack ([`PAGE_DIALOG`], [`RECEIVED_DIALOG`]); this
//! module routes their input, sends what they ask and writes their
//! transcripts.

use crate::app::shell_route::ShellRoute;
use winit::keyboard::KeyCode;

use super::leaf::{self, Pointer};
use super::service::WolServiceRequest;
use crate::app::{App, AppState};
use crate::ui::lan_shell::{LanChatLine, append_wrapped};
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::modal::{
    WolPageLayout, WolPageReceivedLayout, wol_page_layout, wol_page_layout_in,
    wol_page_received_layout, wol_page_received_layout_in,
};
use crate::ui::shell::page::{PageCue, PageEvent};
use crate::ui::wol_chat::{self, WolChatLine};
use crate::ui::wol_page::*;
use vera20k_gui::{NodeId, Prop, Value};

/// `[0x7EEF2C]`: `0x11E`'s own echo.
const REPLY_COLOR: u32 = 0xF0F0;

/// In a match, the frame the battlefield draws into: the in-match `0xCF`
/// centres over it, at its source size. Elsewhere the boxes centre like the
/// shell's other menu boxes, over the window.
fn match_frame(state: &AppState) -> Result<RectPx, (i32, i32)> {
    let config = &state.renderer.gpu.config;
    match state.frontend.screen == crate::app::GameScreen::InGame {
        true => Ok(RectPx::new(
            0,
            0,
            state.render_width() as i32,
            state.render_height() as i32,
        )),
        false => Err((config.width as i32, config.height as i32)),
    }
}

pub(crate) fn page_layout(state: &AppState) -> WolPageLayout {
    match_frame(state).map_or_else(|(w, h)| wol_page_layout(w, h), wol_page_layout_in)
}

pub(crate) fn received_layout(state: &AppState) -> WolPageReceivedLayout {
    match_frame(state).map_or_else(
        |(w, h)| wol_page_received_layout(w, h),
        wol_page_received_layout_in,
    )
}

pub(crate) fn page_dialog(state: &AppState) -> Option<&WolPageDialog> {
    state.frontend.wol.as_ref()?.page_dialog.as_deref()
}

pub(crate) fn page_received(state: &AppState) -> Option<&WolPageReceived> {
    state.frontend.wol.as_ref()?.page_received.as_deref()
}

/// `0xCF` or `0x11E` takes the input.
pub(crate) fn open(state: &AppState) -> bool {
    top(state).is_some()
}

/// The open dialog that takes the input, `0xCF` first.
fn top(state: &AppState) -> Option<DialogResourceId> {
    if page_dialog(state).is_some() {
        Some(PAGE_DIALOG)
    } else {
        page_received(state).map(|_| RECEIVED_DIALOG)
    }
}

/// `dialog`'s widgets, where they sit and its transcript's id.
fn widgets(
    state: &AppState,
    dialog: DialogResourceId,
) -> (fn() -> vera20k_gui::NodeSpec, Rects, u16) {
    if dialog == PAGE_DIALOG {
        let layout = page_layout(state);
        (
            page_spec,
            Box::new(move |id| layout.rect_of(id)),
            PAGE_TRANSCRIPT,
        )
    } else {
        let layout = received_layout(state);
        (
            received_spec,
            Box::new(move |id| layout.rect_of(id)),
            RECEIVED_TRANSCRIPT,
        )
    }
}

type Rects = Box<dyn Fn(u16) -> Option<RectPx>>;

/// `dialog`'s rows.
fn lines(visit: &mut super::WolVisit, dialog: DialogResourceId) -> Option<&mut Vec<LanChatLine>> {
    match dialog {
        PAGE_DIALOG => visit.page_dialog.as_deref_mut().map(|page| &mut page.lines),
        _ => visit
            .page_received
            .as_deref_mut()
            .map(|received| &mut received.lines),
    }
}

/// Builds and places `dialog`'s widgets at the popups' retail pixels and
/// gives its list the rows; the root.
fn sync(state: &mut AppState, dialog: DialogResourceId) -> Option<NodeId> {
    let (spec, rect_of, list) = widgets(state, dialog);
    let root = leaf::place(
        state,
        dialog,
        crate::ui::shell::geom::RETAIL_SCALE,
        spec,
        rect_of,
    )?;
    let frontend = &mut state.frontend;
    let rows = frontend
        .wol
        .as_mut()
        .and_then(|visit| lines(visit, dialog))
        .map_or(0, |l| l.len());
    let rows = Value::Int(rows as i32);
    frontend
        .shell_controller
        .set_prop(dialog, list, Prop::RowCount, rows);
    Some(root)
}

/// A fresh `dialog` over the shell stack, as an overlay: the screens under
/// it may reset or box their own dialogs (a lobby wait box) and it stays on
/// top with its widgets until it closes.
fn push_fresh(state: &mut AppState, dialog: DialogResourceId) {
    let controller = &mut state.frontend.shell_controller;
    controller.close(dialog);
    controller.push_overlay(dialog);
    sync(state, dialog);
}

/// Closes the overlays of dialogs the visit no longer has (a logout drops
/// `0x11E`, leaving WOL drops both).
pub(crate) fn close_stale(state: &mut AppState) {
    let (page, received) = (page_dialog(state).is_none(), page_received(state).is_none());
    let controller = &mut state.frontend.shell_controller;
    for (gone, dialog) in [(page, PAGE_DIALOG), (received, RECEIVED_DIALOG)] {
        if gone {
            controller.close(dialog);
        }
    }
}

/// `0xCF` built over the stack and opened by `open` on its widgets with
/// the page globals.
fn begin(
    state: &mut AppState,
    open: impl FnOnce(
        &mut DialogController,
        &mut super::lobby::WolLobbyGlobals,
    ) -> (WolPageDialog, Vec<WolPageAction>),
) {
    if state.frontend.wol.is_none() {
        return;
    }
    push_fresh(state, PAGE_DIALOG);
    let frontend = &mut state.frontend;
    let (page, actions) = open(
        &mut frontend.shell_controller,
        &mut frontend.wol_lobby_globals,
    );
    if let Some(visit) = frontend.wol.as_mut() {
        visit.page_dialog = Some(Box::new(page));
    }
    apply(state, actions);
}

/// `0x0078BA3E`, `0x00790C8D`, `0x0079235F` and, in a match, `0x0077D840`:
/// `UI__BeginDialog(0xCF)` and its modal loop. The input lock `[0xA8ED9C]`
/// and the keyboard clears around it have nothing to hold here: the dialog
/// takes every key while it exists.
pub(crate) fn open_page_dialog(state: &mut AppState) {
    let last_pager = state
        .frontend
        .wol
        .as_ref()
        .and_then(|visit| visit.last_pager.clone());
    let now = reply_window_now(crate::app::match_runtime::frame_pacer::wall_clock_ms());
    begin(state, |c, globals| {
        WolPageDialog::open(
            c,
            &mut globals.page_reply_window,
            now,
            last_pager.as_deref(),
            &mut globals.page_last_control,
        )
    });
}

/// VERA's player card's Message: `0xCF` opened on its player, addressed as
/// `0x497` addresses the last pager. The reply window stays for a later open.
pub(crate) fn open_page_dialog_to(state: &mut AppState, name: &str) {
    begin(state, |c, globals| {
        let mut page = WolPageDialog::default();
        let actions = page.address(c, name, &mut globals.page_last_control);
        (page, actions)
    });
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
    close_stale(state);
    if state.match_state.match_presentation.in_game_menu == InGameMenuState::Page {
        state.match_state.input.clear_command_latches();
        App::enter_in_game_menu_state(state, InGameMenuState::Closed);
    }
}

/// `0x11E` closes.
pub(super) fn close_received(state: &mut AppState) {
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.page_received = None;
    }
    close_stale(state);
}

/// The observer gate `0x0078D52A..0x0078D566`: once the local player is
/// out (Session+0x300, `[0xA8B538]`), a name `_stricmp`-equal to any
/// connection node's refuses the page. Outside a match there are no nodes.
fn observer_cannot_page(state: &AppState, name: &str) -> bool {
    let out = state
        .match_state
        .sim_runtime
        .runtime()
        .is_some_and(|runtime| runtime.simulation.local_player_defeated());
    let session = &state.match_state.session;
    let nodes: Vec<String> = crate::app::match_runtime::network_match::nodes(session)
        .into_iter()
        .map(|node| crate::app::match_runtime::network_match::participant_name(session, node))
        .collect();
    observer_refuses(out, &nodes, name)
}

/// `0x005E2820` into `dialog`'s list, wrapped to its width; the view
/// follows the rows when it showed the last one. Returns whether the
/// dialog took the line.
fn append_line(state: &mut AppState, dialog: DialogResourceId, line: LanChatLine) -> bool {
    let (_, rect_of, id) = widgets(state, dialog);
    let rect = rect_of(id).unwrap_or_default();
    let font = &state.renderer.bit_font;
    let frontend = &mut state.frontend;
    let Some(lines) = frontend.wol.as_mut().and_then(|visit| lines(visit, dialog)) else {
        return false;
    };
    let dropped = append_wrapped(lines, line, rect.w, |units| {
        font.text_width(&String::from_utf16_lossy(units)) as i32
    });
    let c = &mut frontend.shell_controller;
    if let Some(list) = c.control(dialog, id) {
        // A list placed after its rows came starts at its first row.
        if c.prop(dialog, id, Prop::Top).is_none() {
            c.set_prop(dialog, id, Prop::Top, Value::Int(0));
        }
        let _ = c.gui_mut().log_rows(list, lines.len(), dropped);
    }
    true
}

/// `0x005E2790` with `0xCF` current: the line goes to its list `0x5B5`.
/// Returns whether `0xCF` took it.
pub(crate) fn append_page_line(state: &mut AppState, line: LanChatLine) -> bool {
    append_line(state, PAGE_DIALOG, line)
}

/// `0x005E2700` into `0x11E`'s `0x53F`.
pub(super) fn append_received_line(state: &mut AppState, line: LanChatLine) {
    append_line(state, RECEIVED_DIALOG, line);
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
    if state.frontend.shell_route == ShellRoute::NetworkSetup || App::wol_page_is_lobby(state) {
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
    if page_received(state).is_none() {
        open_received(state, false);
    }
    append_received_line(state, line);
    name_pager(state);
}

/// `0x116`'s Page (`0x0079EA23..0x0079EA75`): `0x11E` created over the
/// Buddy List and run modally (`0x007759E0` with the pump `0x0077F900`).
/// Its list is empty, so its reply-to is the selected buddy.
pub(crate) fn open_received_modal(state: &mut AppState) {
    open_received(state, true);
    name_pager(state);
}

/// `0x718` names the pager.
fn name_pager(state: &mut AppState) {
    with_received(state, |received, c| {
        received.reply_to = received.pager_from_list(c)
    });
    state.platform.window.request_redraw();
}

/// `0x11E` over the stack, replying by default to `[0xB76E24]`.
fn open_received(state: &mut AppState, modal: bool) {
    let fallback = state.frontend.wol_buddies.selected.clone();
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.page_received = Some(Box::new(WolPageReceived::new(&fallback, modal)));
    push_fresh(state, RECEIVED_DIALOG);
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
    let key = match result {
        Ok(channel) => {
            let text = match channel.lobby_number().filter(|_| channel.kind == 0) {
                Some(number) => crate::ui::wol_lobby::WolLobbyRow::joined_name(
                    number,
                    &super::lobby::base_names(state),
                ),
                None => channel.name.clone(),
            };
            return set_status(state, text);
        }
        Err(0x0004_0131) => "TXT_FINDPAGE_NOT_IN_CHANNEL",
        Err(0x0004_0132) => "TXT_FINDPAGE_FIND_DISABLED",
        Err(_) => "TXT_FINDPAGE_NOT_ONLINE",
    };
    set_status(state, state.csf_label(key, key))
}

fn set_status(state: &mut AppState, text: String) -> bool {
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

fn request(state: &mut AppState, line: WolChatLine) {
    super::service::request(
        state,
        WolServiceRequest::Chat {
            line,
            to: Vec::new(),
        },
    );
}

fn locate(state: &mut AppState, name: String) {
    // API+0x60 without [0xB76E28]: the 0xCF status shows the answer.
    if state.frontend.wol.as_ref().is_some_and(|visit| visit.api) {
        request(
            state,
            WolChatLine::Locate {
                name: name.encode_utf16().collect(),
            },
        );
    }
}

/// API+0xF8 with the record and text, and `u"[%hs] %s"` (`0x0084B94C`)
/// with the login nickname `[0xB73E40]`, the echo, into `dialog`'s list.
fn send_page(
    state: &mut AppState,
    dialog: DialogResourceId,
    name: &str,
    text: Vec<u16>,
    color: u32,
) {
    let mut echo: Vec<u16> = format!("[{}] ", state.frontend.wol_account.nickname)
        .encode_utf16()
        .collect();
    echo.extend_from_slice(&text);
    request(
        state,
        WolChatLine::Page {
            name: name.encode_utf16().collect(),
            text,
        },
    );
    append_line(state, dialog, LanChatLine { text: echo, color });
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
                send_page(state, PAGE_DIALOG, &name, text, wol_chat::SAY_COLOR);
            }
            WolPageAction::Notice(key) => notice(state, key),
            WolPageAction::Close => close_page_dialog(state),
        }
    }
    state.platform.window.request_redraw();
}

/// A white (-1) CSF line through `0x005E2790`.
fn notice(state: &mut AppState, key: &str) {
    let text = state.csf_label(key, key).encode_utf16().collect();
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
            send_page(state, RECEIVED_DIALOG, &name, text, REPLY_COLOR)
        }
        Some(WolPageReceivedAction::Close) => close_received(state),
        None => {}
    }
    state.platform.window.request_redraw();
}

/// `0xCF` on its widgets with `[0xB77E8C]`, process global.
fn with_last<R>(
    state: &mut AppState,
    f: impl FnOnce(&mut WolPageDialog, &mut DialogController, &mut u16) -> R,
) -> Option<R> {
    let frontend = &mut state.frontend;
    let page = frontend.wol.as_mut()?.page_dialog.as_deref_mut()?;
    Some(f(
        page,
        &mut frontend.shell_controller,
        &mut frontend.wol_lobby_globals.page_last_control,
    ))
}

/// `0x11E` on its widgets.
fn with_received<R>(
    state: &mut AppState,
    f: impl FnOnce(&mut WolPageReceived, &mut DialogController) -> R,
) -> Option<R> {
    let frontend = &mut state.frontend;
    let received = frontend.wol.as_mut()?.page_received.as_deref_mut()?;
    Some(f(received, &mut frontend.shell_controller))
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    if let Some(dialog) = top(state)
        && sync(state, dialog).is_some()
    {
        // The boxes' hit tests leave the status line under them alone.
        leaf::pointer_quiet(state, Pointer::Move);
    }
}

/// A press on an edit takes the focus with its notifications; a press on
/// `0xCF`'s transcript row hands it back; a released button acts. A row of
/// `0x11E`'s selectable owner-draw list clicks like the lobby's.
pub(crate) fn mouse(state: &mut AppState, pressed: bool) {
    let Some(dialog) = top(state) else {
        return;
    };
    if sync(state, dialog).is_none() {
        return;
    }
    let before = leaf::focused(state);
    let events = leaf::pointer_quiet(state, if pressed { Pointer::Down } else { Pointer::Up });
    if dialog == PAGE_DIALOG {
        let after = leaf::focused(state);
        let actions = with_last(state, |page, c, last| {
            let mut actions = page.focus_moved(c, before, after, last);
            for event in events {
                match event {
                    PageEvent::Selected(PAGE_TRANSCRIPT, _) => {
                        actions.extend(page.press_transcript_row(c, last))
                    }
                    PageEvent::Activated(id) => actions.extend(page.activate(c, id, last)),
                    _ => {}
                }
            }
            actions
        });
        apply(state, actions.unwrap_or_default());
        return;
    }
    for event in events {
        let action = with_received(state, |received, c| match event {
            PageEvent::Selected(RECEIVED_TRANSCRIPT, row) => {
                received.select(c, row as usize);
                None
            }
            PageEvent::Activated(id) => received.activate(c, id),
            _ => None,
        });
        if matches!(event, PageEvent::Selected(..)) {
            App::play_page_cue(state, PageCue::ListClick);
        }
        apply_received(state, action.flatten());
    }
}

pub(crate) fn key_press(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
    let shift = state.platform.live_modifiers.shift_key();
    let event = crate::ui::shell::controller::gui_input(code, shift, text);
    let (Some(dialog), Some(event)) = (top(state), event) else {
        return;
    };
    if sync(state, dialog).is_none() {
        return;
    }
    if dialog == RECEIVED_DIALOG {
        let action = with_received(state, |received, c| received.input(c, event));
        return apply_received(state, action.flatten());
    }
    let actions = with_last(state, |page, c, last| page.input(c, event, last));
    apply(state, actions.unwrap_or_default());
    // 0xCF's focused NewEdit keeps its caret in view: 0x00623880 paints
    // from the first visible unit, at the popup's retail scale.
    if let Some(node) =
        leaf::root(state, PAGE_DIALOG).and(state.frontend.shell_controller.gui().focus())
    {
        leaf::scroll_edit(state, node);
    }
}

/// The transcripts' scrollbar repeats.
pub(super) fn poll_scroll_repeat(state: &mut AppState) -> Option<crate::util::time::Instant> {
    close_stale(state);
    leaf::poll(state, open(state))
}
