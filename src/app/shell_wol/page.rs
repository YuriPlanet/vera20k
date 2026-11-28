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
use crate::app::App;
use crate::app::PlatformState;
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::shell_wol::WolVisit;
use crate::audio::sfx::SfxPlayer;
use crate::ui::lan_shell::{LanChatLine, append_wrapped};
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::modal::{
    WolPageLayout, WolPageReceivedLayout, wol_page_layout, wol_page_received_layout,
};
use crate::ui::shell::page::{PageCue, PageEvent};
use crate::ui::wol_chat::{self, WolChatLine};
use crate::ui::wol_page::*;
use vera20k_gui::{NodeId, Prop, Value};
use winit::window::Window;

/// `[0x7EEF2C]`: `0x11E`'s own echo.
const REPLY_COLOR: u32 = 0xF0F0;

pub(crate) fn page_layout(renderer: &RendererState) -> WolPageLayout {
    let config = &renderer.gpu.config;
    wol_page_layout(config.width as i32, config.height as i32)
}

pub(crate) fn received_layout(renderer: &RendererState) -> WolPageReceivedLayout {
    let config = &renderer.gpu.config;
    wol_page_received_layout(config.width as i32, config.height as i32)
}

pub(crate) fn page_dialog(wol: Option<&WolVisit>) -> Option<&WolPageDialog> {
    wol?.page_dialog.as_deref()
}

pub(crate) fn page_received(wol: Option<&WolVisit>) -> Option<&WolPageReceived> {
    wol?.page_received.as_deref()
}

/// `0xCF` or `0x11E` takes the input.
pub(crate) fn open(wol: Option<&WolVisit>) -> bool {
    top(wol).is_some()
}

/// Android: the dialog whose edits take keys, for `app::input::ime::gui_armed`.
#[cfg(target_os = "android")]
pub(crate) fn armed_dialog(wol: Option<&WolVisit>) -> Option<u16> {
    top(wol).map(|dialog| dialog.0)
}

/// The open dialog that takes the input, `0xCF` first.
fn top(wol: Option<&WolVisit>) -> Option<DialogResourceId> {
    if page_dialog(wol).is_some() {
        Some(PAGE_DIALOG)
    } else {
        page_received(wol).map(|_| RECEIVED_DIALOG)
    }
}

/// `dialog`'s widgets, where they sit and its transcript's id.
fn widgets(
    renderer: &RendererState,
    dialog: DialogResourceId,
) -> (fn() -> vera20k_gui::NodeSpec, Rects, u16) {
    if dialog == PAGE_DIALOG {
        let layout = page_layout(renderer);
        (
            page_spec,
            Box::new(move |id| layout.rect_of(id)),
            PAGE_TRANSCRIPT,
        )
    } else {
        let layout = received_layout(renderer);
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
fn sync(
    frontend: &mut FrontendState,
    renderer: &RendererState,
    dialog: DialogResourceId,
) -> Option<NodeId> {
    let (spec, rect_of, list) = widgets(renderer, dialog);
    let root = leaf::place(
        &mut frontend.shell_controller,
        dialog,
        crate::ui::shell::geom::RETAIL_SCALE,
        spec,
        rect_of,
    )?;
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
fn push_fresh(frontend: &mut FrontendState, renderer: &RendererState, dialog: DialogResourceId) {
    let controller = &mut frontend.shell_controller;
    controller.close(dialog);
    controller.push_overlay(dialog);
    sync(frontend, renderer, dialog);
}

/// Closes the overlays of dialogs the visit no longer has (a logout drops
/// `0x11E`, leaving WOL drops both).
pub(crate) fn close_stale(frontend: &mut FrontendState) {
    let (page, received) = (
        page_dialog(frontend.wol.as_ref()).is_none(),
        page_received(frontend.wol.as_ref()).is_none(),
    );
    let controller = &mut frontend.shell_controller;
    for (gone, dialog) in [(page, PAGE_DIALOG), (received, RECEIVED_DIALOG)] {
        if gone {
            controller.close(dialog);
        }
    }
}

/// Open the lobby or room pager (0x0078BA3E, 0x00790C8D and 0x0079235F).
/// The player card opens it already addressed to the selected player.
pub(crate) fn open_page_dialog(
    frontend: &mut FrontendState,
    platform: &mut PlatformState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    recipient: Option<&str>,
) {
    if frontend.wol.is_none() {
        return;
    }
    push_fresh(frontend, renderer, PAGE_DIALOG);
    let mut page = WolPageDialog::default();
    let actions = recipient.map_or_else(Vec::new, |name| {
        page.address(
            &mut frontend.shell_controller,
            name,
            &mut frontend.wol_lobby_globals.page_last_control,
        )
    });
    if let Some(visit) = frontend.wol.as_mut() {
        visit.page_dialog = Some(Box::new(page));
    }
    apply(
        frontend,
        platform,
        match_state,
        process_assets,
        renderer,
        actions,
    );
}

/// Close the shell pager before returning to the lobby or entering a match.
pub(crate) fn close_page_dialog(frontend: &mut FrontendState) {
    if let Some(visit) = frontend.wol.as_mut() {
        visit.page_dialog = None;
    }
    close_stale(frontend);
}

pub(super) fn close_received(frontend: &mut FrontendState) {
    if let Some(visit) = frontend.wol.as_mut() {
        visit.page_received = None;
    }
    close_stale(frontend);
}

/// The observer gate `0x0078D52A..0x0078D566`: once the local player is
/// out (Session+0x300, `[0xA8B538]`), a name `_stricmp`-equal to any
/// connection node's refuses the page. Outside a match there are no nodes.
fn observer_cannot_page(match_state: &MatchState, name: &str) -> bool {
    let out = match_state
        .sim_runtime
        .runtime()
        .is_some_and(|runtime| runtime.simulation.local_player_defeated());
    let session = &match_state.session;
    let nodes: Vec<String> = crate::app::match_runtime::network_match::nodes(session)
        .into_iter()
        .map(|node| crate::app::match_runtime::network_match::participant_name(session, node))
        .collect();
    observer_refuses(out, &nodes, name)
}

/// `0x005E2820` into `dialog`'s list, wrapped to its width; the view
/// follows the rows when it showed the last one. Returns whether the
/// dialog took the line.
fn append_line(
    frontend: &mut FrontendState,
    renderer: &RendererState,
    dialog: DialogResourceId,
    line: LanChatLine,
) -> bool {
    let (_, rect_of, id) = widgets(renderer, dialog);
    let rect = rect_of(id).unwrap_or_default();
    let font = &renderer.bit_font;
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
pub(crate) fn append_page_line(
    frontend: &mut FrontendState,
    renderer: &RendererState,
    line: LanChatLine,
) -> bool {
    append_line(frontend, renderer, PAGE_DIALOG, line)
}

/// `0x005E2700` into `0x11E`'s `0x53F`.
pub(super) fn append_received_line(
    frontend: &mut FrontendState,
    renderer: &RendererState,
    line: LanChatLine,
) {
    append_line(frontend, renderer, RECEIVED_DIALOG, line);
}

/// The current window's `0x53F`, else its `0x5B5` (the find sink's tail
/// `0x007AA5D5`, the page-result sink's failure branch `0x007A9800`):
/// `0xCF`'s list, `0x11E`'s, or the lobby's or C2/C9's transcript; with
/// neither, in a match (Session+0x30D8, `0x0069BBE0`), a Grey MessageList
/// row. Returns false when nothing took the line.
pub(super) fn current_window_line(
    frontend: &mut FrontendState,
    platform: &PlatformState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
    line: LanChatLine,
) -> bool {
    if page_dialog(frontend.wol.as_ref()).is_some() {
        return append_page_line(frontend, renderer, line);
    }
    if page_received(frontend.wol.as_ref()).is_some() {
        append_received_line(frontend, renderer, line);
        return true;
    }
    let Some(in_game) = frontend.wol.as_ref().map(|visit| visit.in_game) else {
        return false;
    };
    if in_game {
        let text = String::from_utf16_lossy(&line.text);
        crate::app::input::messages::post_wol_row(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            sfx_player,
            persistence,
            &text,
        );
        return true;
    }
    if frontend.shell_route == ShellRoute::NetworkSetup
        || App::wol_page_is_lobby(frontend.wol.as_ref())
    {
        crate::app::shell_lan::append_chat(renderer, frontend, match_state, process_assets, line);
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
pub(super) fn page_received_line(
    frontend: &mut FrontendState,
    window: &Window,
    renderer: &RendererState,
    line: LanChatLine,
) {
    if page_received(frontend.wol.as_ref()).is_none() {
        open_received(frontend, renderer, false);
    }
    append_received_line(frontend, renderer, line);
    name_pager(frontend, window);
}

/// `0x116`'s Page (`0x0079EA23..0x0079EA75`): `0x11E` created over the
/// Buddy List and run modally (`0x007759E0` with the pump `0x0077F900`).
/// Its list is empty, so its reply-to is the selected buddy.
pub(crate) fn open_received_modal(
    frontend: &mut FrontendState,
    window: &Window,
    renderer: &RendererState,
) {
    open_received(frontend, renderer, true);
    name_pager(frontend, window);
}

/// `0x718` names the pager.
fn name_pager(frontend: &mut FrontendState, window: &Window) {
    with_received(frontend, |received, c| {
        received.reply_to = received.pager_from_list(c)
    });
    window.request_redraw();
}

/// `0x11E` over the stack, replying by default to `[0xB76E24]`.
fn open_received(frontend: &mut FrontendState, renderer: &RendererState, modal: bool) {
    let fallback = frontend.wol_buddies.selected.clone();
    let Some(visit) = frontend.wol.as_mut() else {
        return;
    };
    visit.page_received = Some(Box::new(WolPageReceived::new(&fallback, modal)));
    push_fresh(frontend, renderer, RECEIVED_DIALOG);
}

/// The find sink with `0xCF` current (`0x007AA25D`): the status `0x5C8`
/// takes the room or a `TXT_FINDPAGE_*` text. Returns whether `0xCF` took
/// the result.
pub(super) fn located(
    mut wol: Option<&mut WolVisit>,
    window: &Window,
    process_assets: &ProcessAssets,
    result: &Result<crate::ui::wol_lobby::WolChannel, u32>,
) -> bool {
    if page_dialog(wol.as_deref()).is_none() {
        return false;
    }
    let key = match result {
        Ok(channel) => {
            let text = match channel.lobby_number().filter(|_| channel.kind == 0) {
                Some(number) => crate::ui::wol_lobby::WolLobbyRow::joined_name(
                    number,
                    &super::lobby::base_names(wol.as_deref(), process_assets),
                ),
                None => channel.name.clone(),
            };
            return set_status(wol.as_deref_mut(), window, text);
        }
        Err(0x0004_0131) => "TXT_FINDPAGE_NOT_IN_CHANNEL",
        Err(0x0004_0132) => "TXT_FINDPAGE_FIND_DISABLED",
        Err(_) => "TXT_FINDPAGE_NOT_ONLINE",
    };
    set_status(
        wol.as_deref_mut(),
        window,
        process_assets.csf_label(key, key),
    )
}

fn set_status(wol: Option<&mut WolVisit>, window: &Window, text: String) -> bool {
    if let Some(page) = wol.and_then(|visit| visit.page_dialog.as_mut()) {
        page.status = text;
    }
    window.request_redraw();
    true
}

fn request(wol: Option<&mut WolVisit>, line: WolChatLine) {
    super::service::request(
        wol,
        WolServiceRequest::Chat {
            line,
            to: Vec::new(),
        },
    );
}

fn locate(mut wol: Option<&mut WolVisit>, name: String) {
    // API+0x60 without [0xB76E28]: the 0xCF status shows the answer.
    if wol.as_deref().is_some_and(|visit| visit.api) {
        request(
            wol.as_deref_mut(),
            WolChatLine::Locate {
                name: name.encode_utf16().collect(),
            },
        );
    }
}

/// API+0xF8 with the record and text, and `u"[%hs] %s"` (`0x0084B94C`)
/// with the login nickname `[0xB73E40]`, the echo, into `dialog`'s list.
fn send_page(
    frontend: &mut FrontendState,
    renderer: &RendererState,
    dialog: DialogResourceId,
    name: &str,
    text: Vec<u16>,
    color: u32,
) {
    let mut echo: Vec<u16> = format!("[{}] ", frontend.wol_account.nickname)
        .encode_utf16()
        .collect();
    echo.extend_from_slice(&text);
    request(
        frontend.wol.as_mut(),
        WolChatLine::Page {
            name: name.encode_utf16().collect(),
            text,
        },
    );
    append_line(
        frontend,
        renderer,
        dialog,
        LanChatLine { text: echo, color },
    );
}

fn apply(
    frontend: &mut FrontendState,
    platform: &mut PlatformState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    actions: Vec<WolPageAction>,
) {
    for action in actions {
        match action {
            WolPageAction::Locate(name) => locate(frontend.wol.as_mut(), name),
            WolPageAction::Send { name, text } => {
                // 0x0078D48F: no WOLAPI object, nothing past the clear.
                if !frontend.wol.as_ref().is_some_and(|visit| visit.api) {
                    continue;
                }
                if observer_cannot_page(match_state, &name) {
                    notice(frontend, renderer, process_assets, "TXT_OBSERVER_CANT_PAGE");
                    continue;
                }
                send_page(
                    frontend,
                    renderer,
                    PAGE_DIALOG,
                    &name,
                    text,
                    wol_chat::SAY_COLOR,
                );
            }
            WolPageAction::Notice(key) => notice(frontend, renderer, process_assets, key),
            WolPageAction::Close => close_page_dialog(frontend),
        }
    }
    platform.window.request_redraw();
}

/// A white (-1) CSF line through `0x005E2790`.
fn notice(
    frontend: &mut FrontendState,
    renderer: &RendererState,
    process_assets: &ProcessAssets,
    key: &str,
) {
    let text = process_assets.csf_label(key, key).encode_utf16().collect();
    append_page_line(
        frontend,
        renderer,
        LanChatLine {
            text,
            color: u32::MAX,
        },
    );
}

fn apply_received(
    frontend: &mut FrontendState,
    window: &Window,
    renderer: &RendererState,
    action: Option<WolPageReceivedAction>,
) {
    match action {
        Some(WolPageReceivedAction::Send { name, text }) => send_page(
            frontend,
            renderer,
            RECEIVED_DIALOG,
            &name,
            text,
            REPLY_COLOR,
        ),
        Some(WolPageReceivedAction::Close) => close_received(frontend),
        None => {}
    }
    window.request_redraw();
}

/// `0xCF` on its widgets with `[0xB77E8C]`, process global.
fn with_last<R>(
    frontend: &mut FrontendState,
    f: impl FnOnce(&mut WolPageDialog, &mut DialogController, &mut u16) -> R,
) -> Option<R> {
    let page = frontend.wol.as_mut()?.page_dialog.as_deref_mut()?;
    Some(f(
        page,
        &mut frontend.shell_controller,
        &mut frontend.wol_lobby_globals.page_last_control,
    ))
}

/// `0x11E` on its widgets.
fn with_received<R>(
    frontend: &mut FrontendState,
    f: impl FnOnce(&mut WolPageReceived, &mut DialogController) -> R,
) -> Option<R> {
    let received = frontend.wol.as_mut()?.page_received.as_deref_mut()?;
    Some(f(received, &mut frontend.shell_controller))
}

pub(crate) fn cursor_moved(
    frontend: &mut FrontendState,
    window: &Window,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    sfx_player: &mut Option<SfxPlayer>,
) {
    if let Some(dialog) = top(frontend.wol.as_ref())
        && sync(frontend, renderer, dialog).is_some()
    {
        // The boxes' hit tests leave the status line under them alone.
        leaf::pointer_quiet(
            frontend,
            window,
            match_state,
            process_assets,
            sfx_player,
            Pointer::Move,
        );
    }
}

/// A press on an edit takes the focus with its notifications; a press on
/// `0xCF`'s transcript row hands it back; a released button acts. A row of
/// `0x11E`'s selectable owner-draw list clicks like the lobby's.
pub(crate) fn mouse(
    frontend: &mut FrontendState,
    platform: &mut PlatformState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    audio: &mut AppAudioRuntime,
    pressed: bool,
) {
    let Some(dialog) = top(frontend.wol.as_ref()) else {
        return;
    };
    if sync(frontend, renderer, dialog).is_none() {
        return;
    }
    let before = leaf::focused(&frontend.shell_controller);
    let events = leaf::pointer_quiet(
        frontend,
        &platform.window,
        match_state,
        process_assets,
        &mut audio.sfx_player,
        if pressed { Pointer::Down } else { Pointer::Up },
    );
    // Android: a press that lands on one of the page's edits arms the keyboard
    // for this dialog, the tree those edits live in.
    #[cfg(target_os = "android")]
    if pressed && frontend.shell_controller.gui().pressed_edit() {
        crate::util::android_ime::arm(dialog.0);
        crate::util::android_ime::request_show();
    }
    if dialog == PAGE_DIALOG {
        let after = leaf::focused(&frontend.shell_controller);
        let actions = with_last(frontend, |page, c, last| {
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
        apply(
            frontend,
            platform,
            match_state,
            process_assets,
            renderer,
            actions.unwrap_or_default(),
        );
        return;
    }
    for event in events {
        let action = with_received(frontend, |received, c| match event {
            PageEvent::Selected(RECEIVED_TRANSCRIPT, row) => {
                received.select(c, row as usize);
                None
            }
            PageEvent::Activated(id) => received.activate(c, id),
            _ => None,
        });
        if matches!(event, PageEvent::Selected(..)) {
            App::play_page_cue(
                frontend,
                match_state,
                process_assets,
                &mut audio.sfx_player,
                PageCue::ListClick,
            );
        }
        apply_received(frontend, &platform.window, renderer, action.flatten());
    }
}

pub(crate) fn key_press(
    frontend: &mut FrontendState,
    platform: &mut PlatformState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    code: Option<KeyCode>,
    text: Option<&str>,
) {
    let shift = platform.live_modifiers.shift_key();
    let event = crate::ui::shell::controller::gui_input(code, shift, text);
    let (Some(dialog), Some(event)) = (top(frontend.wol.as_ref()), event) else {
        return;
    };
    if sync(frontend, renderer, dialog).is_none() {
        return;
    }
    if dialog == RECEIVED_DIALOG {
        let action = with_received(frontend, |received, c| received.input(c, event));
        return apply_received(frontend, &platform.window, renderer, action.flatten());
    }
    let actions = with_last(frontend, |page, c, last| page.input(c, event, last));
    apply(
        frontend,
        platform,
        match_state,
        process_assets,
        renderer,
        actions.unwrap_or_default(),
    );
    // 0xCF's focused NewEdit keeps its caret in view: 0x00623880 paints
    // from the first visible unit, at the popup's retail scale.
    if let Some(node) = leaf::root(&frontend.shell_controller, PAGE_DIALOG)
        .and(frontend.shell_controller.gui().focus())
    {
        leaf::scroll_edit(&mut frontend.shell_controller, &renderer.bit_font, node);
    }
}

/// The transcripts' scrollbar repeats.
pub(super) fn poll_scroll_repeat(
    frontend: &mut FrontendState,
    window: &Window,
) -> Option<crate::util::time::Instant> {
    close_stale(frontend);
    leaf::poll(
        &mut frontend.shell_controller,
        window,
        open(frontend.wol.as_ref()),
    )
}
