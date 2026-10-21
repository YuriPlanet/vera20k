//! LAN BB app boundary: original lobby owner005DC350, proc005DDBD0,
//! join validation005DF410 and receive005E00B0. Transport supplies discovery
//! and replies here; only MatchSession owns admitted participants/setup.

use super::{App, AppState};
use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::match_runtime::session::NetworkReturnContext;
use crate::skirmish_launch::SkirmishHumanSlot;
use crate::ui::lan_shell::*;
use crate::ui::main_menu_dialogs::options::LauncherCue;
use crate::ui::shell::descriptor::DialogId;
use std::time::{Duration, Instant};
use winit::event::{KeyEvent, MouseButton};
use winit::keyboard::{KeyCode, PhysicalKey};

const MODAL: DialogId = DialogId(0xCE);
const OK: u16 = 1;
// Native CDTimerClass uses timeGetTime()/16, including the 600-bucket join
// timer AC0230/AC0238 armed by005DF410. It is independent of simulation ticks.
const JOIN_TIMEOUT: Duration = Duration::from_millis(600 * 16);
const RESERVED_LABELS: [&str; 7] = [
    "GUI:Open",
    "GUI:Closed",
    "GUI:AIEasy",
    "GUI:AINormal",
    "GUI:AIHard",
    "GUI:Waiting",
    "GUI:OpenObserver",
];
const RESERVED_LITERALS: [&str; 7] = [
    "<Open>",
    "<Closed>",
    "<Waiting>",
    "<EasyAI>",
    "<MediumAI>",
    "<HardAI>",
    "<OpenObserver>",
];

/// Semantic service requests, not serialized packets or an online backend.
/// A controlled service drains these and returns the same events as a future
/// transport adapter. No request ever manufactures a successful response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LanServiceRequest {
    Refresh,
    SelectGame { game_id: Option<u64>, changed: bool },
    NameChanged(String),
    Chat { text: Vec<u16>, color: u32 },
    Join { game_id: u64, observe: bool },
    CancelJoin { game_id: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LanJoinAcceptance {
    pub(crate) game_id: u64,
    pub(crate) local: SkirmishHumanSlot,
    pub(crate) host: SkirmishHumanSlot,
    pub(crate) observer_row: Option<u8>,
}

pub(crate) enum LanServiceEvent {
    Games(Vec<LanGameRow>),
    /// Projection of discovery, never the admitted MatchSession roster.
    Players {
        game_id: Option<u64>,
        players: Vec<LanPlayerRow>,
    },
    Chat(LanChatLine),
    JoinDenied {
        game_id: u64,
        reason: u32,
    },
    JoinAccepted(LanJoinAcceptance),
}

#[derive(Debug, Clone)]
struct PendingJoin {
    game_id: u64,
    observe: bool,
    expires: Instant,
}

pub(crate) struct LanShellRuntime {
    pub(crate) lobby: LanLobbyState,
    pub(crate) modal: Option<&'static str>,
    pending_join: Option<PendingJoin>,
    requests: Vec<LanServiceRequest>,
}

impl LanShellRuntime {
    fn new(name: &str, observer: bool) -> Self {
        Self {
            lobby: LanLobbyState::new(name, observer),
            modal: None,
            pending_join: None,
            requests: vec![LanServiceRequest::Refresh],
        }
    }

    pub(crate) fn take_service_requests(&mut self) -> Vec<LanServiceRequest> {
        std::mem::take(&mut self.requests)
    }

    pub(crate) fn pending_game(&self) -> Option<u64> {
        self.pending_join.as_ref().map(|request| request.game_id)
    }

    fn release_request(&mut self) {
        self.pending_join = None;
        self.lobby.selection_enabled = true;
        self.lobby.set_enabled(true);
        self.requests.push(LanServiceRequest::Refresh);
    }
}

pub(crate) fn layout(state: &AppState) -> LanLobbyLayout {
    let (width, height) = shell_layout::shell_screen_size(state);
    if shell_layout::fills_window(state) {
        LanLobbyLayout::responsive(width as i32, height as i32)
    } else {
        LanLobbyLayout::new(width as i32, height as i32)
    }
}

impl App {
    /// MainMenu52E418 and setup-return5DC624/5DC964 construct a fresh BB.
    /// The retained preference owner supplies the edit; admission stays empty.
    pub(crate) fn open_lan_lobby(state: &mut AppState) {
        Self::prepare_multiplayer_settings(
            state,
            crate::app::shell_main_menu::MultiplayerSettingsContext::Shell,
        );
        if !Self::ensure_skirmish_shell_chrome(state) {
            return;
        }
        state
            .match_state
            .session
            .enter_network(NetworkReturnContext::LanLobby);
        let local = state.frontend.multiplayer_shell_runtime.local_participant();
        let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
        let mut runtime = LanShellRuntime::new(&local.name, observer);
        runtime
            .lobby
            .replace_players(vec![local_lobby_row(&local.name)]);
        state.frontend.lan = Some(runtime);
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        shell_transition::invalidate_main_menu_dialog_instance(state);
        state.frontend.shell_route = crate::app::shell_route::ShellRoute::LanLobby;
        state.frontend.shell_controller.reset_to(PAGE.dialog, false);
        state.frontend.shell_first_paint_slide = None;
        state.platform.window.request_redraw();
    }

    /// Called only after BB's ordinary teardown slide finishes.
    pub(crate) fn commit_lan_back(state: &mut AppState) {
        state
            .frontend
            .multiplayer_shell_runtime
            .persist_lan_preferences();
        state.frontend.lan = None;
        state.match_state.session.leave_network();
        Self::close_single_player_shell(state);
        state
            .frontend
            .shell_controller
            .reset_to(DialogId(0xE2), false);
        state.frontend.shell_first_paint_slide = None;
        state.platform.window.request_redraw();
    }

    pub(crate) fn commit_lan_create(state: &mut AppState, observe: bool) {
        state.frontend.lan = None;
        Self::open_host_setup(state, observe);
    }
}

fn local_lobby_row(name: &str) -> LanPlayerRow {
    LanPlayerRow {
        name: name.to_owned(),
        color: None,
        side_icon: None,
        host: false,
        accepted: false,
    }
}

// Original5E87A0 populates AC117C..AC1194 in this order. The join caller
// compares low-byte narrow labels exactly; case-insensitive matching is only
// used for Create's duplicate game-name test.
fn reserved_names(state: &AppState) -> Vec<String> {
    RESERVED_LABELS
        .iter()
        .map(|key| {
            App::csf_label(state, key, key)
                .encode_utf16()
                .map(|unit| unit as u8)
                .take_while(|byte| *byte != 0)
                .map(char::from)
                .collect()
        })
        .collect()
}

fn create_error(lobby: &LanLobbyState, name: &str) -> Option<&'static str> {
    if name.is_empty() {
        Some("TXT_NAME_BLANK")
    } else if lobby
        .games
        .iter()
        .any(|game| game.name.eq_ignore_ascii_case(name))
    {
        Some("TXT_GAMENAME_MUSTBE_UNIQUE")
    } else {
        None
    }
}

fn join_error(lobby: &LanLobbyState, name: &str, reserved: &[String]) -> Option<&'static str> {
    let selected = lobby.game_list.selected.unwrap_or(0);
    if selected < 1 {
        Some("TXT_MUST_SELECT_GAME")
    } else if lobby.game_count() < 2 || selected >= lobby.game_count() {
        Some("TXT_NOTHING_TO_JOIN")
    } else if name.is_empty() {
        Some("TXT_NAME_ERROR")
    } else if RESERVED_LITERALS.contains(&name) || reserved.iter().any(|value| value == name) {
        Some("ERROR:NameIsReservedWord")
    } else if lobby.selected_game().is_some_and(|game| !game.open) {
        Some("TXT_GAME_IS_CLOSED")
    } else {
        None
    }
}

///005E0Bxx: rejection always emits TXT_REQUEST_DENIED, then these reasons
/// additionally open the shared single-OK message box while servicing LAN.
fn denial_modal(reason: u32, observer: bool) -> Option<&'static str> {
    match reason {
        0 => Some("TXT_NAME_MUSTBE_UNIQUE"),
        1 => Some(if observer {
            "TXT_OBSERVER_FULL"
        } else {
            "TXT_GAME_FULL"
        }),
        2 => Some("TXT_YOURGAME_OUTDATED"),
        3 => Some("TXT_DESTGAME_OUTDATED"),
        5 => Some("TXT_GAME_CANCELLED"),
        6 => Some("TXT_MISMATCH"),
        7 => Some("TXT_SERIAL_DUP"),
        _ => None,
    }
}

fn show_modal(state: &mut AppState, key: &'static str) {
    let runtime = state.frontend.lan.as_mut().expect("active BB");
    runtime.modal = Some(key);
    runtime.lobby.cancel_pointer();
    state.frontend.shell_controller.push(MODAL, true);
}

/// Select the active LAN dialog's transcript; the shared chat owner retains
/// all wrapping, row retention and scroll behavior for BB, BC and BD.
pub(crate) fn append_chat(state: &mut AppState, line: LanChatLine) {
    let route = state.frontend.shell_route;
    let setup = route == crate::app::shell_route::ShellRoute::NetworkSetup;
    append_chat_to(state, setup, !setup && route.wol_lobby(), line);
}

/// The WOL lobby `0x113`'s transcript even while C2/C9 cover it: native
/// keeps it in the shell stack, hidden, and its sinks still write to it.
pub(crate) fn append_wol_lobby_chat(state: &mut AppState, line: LanChatLine) {
    append_chat_to(state, false, true, line);
}

fn append_chat_to(state: &mut AppState, setup: bool, wol_lobby: bool, line: LanChatLine) {
    let (rect, scale) = if setup {
        let layout = crate::app::input::multiplayer_setup::layout(state);
        (layout.chat_log, layout.page.scale)
    } else if wol_lobby {
        let layout = crate::app::shell_wol::lobby::layout(state);
        (layout.chat_log, layout.page.scale)
    } else {
        let layout = layout(state);
        (layout.chat_log, layout.page.scale)
    };
    let chat = if setup {
        state
            .frontend
            .multiplayer_setup
            .as_mut()
            .map(|ui| &mut ui.chat)
    } else if wol_lobby {
        state
            .frontend
            .wol
            .as_mut()
            .and_then(|visit| visit.lobby_mut())
            .map(|lobby| &mut lobby.ui.chat)
    } else {
        state
            .frontend
            .lan
            .as_mut()
            .map(|runtime| &mut runtime.lobby.chat)
    };
    if let Some(chat) = chat {
        let visible = chat
            .list
            .geometry(rect, chat.lines.len(), scale)
            .visible_rows;
        chat.append_message(line, scale.base_rect(rect).w, visible, |units| {
            state
                .renderer
                .bit_font
                .text_width(&String::from_utf16_lossy(units)) as i32
        });
    }
}

pub(crate) fn report_error(state: &mut AppState, key: &str) {
    let text = App::csf_label(state, key, key).encode_utf16().collect();
    //5E2700 uses -1 for the ordinary yellow message color.
    append_chat(
        state,
        LanChatLine {
            text,
            color: u32::MAX,
        },
    );
    let sound = state
        .rules()
        .and_then(|rules| rules.general.system_error_sound())
        .map(str::to_owned);
    App::play_shell_ui_sound_by_id(state, sound.as_deref());
}

pub(crate) fn dispatch(state: &mut AppState, action: LanLobbyAction) {
    let name = state
        .frontend
        .multiplayer_shell_runtime
        .local_participant()
        .name;
    match action {
        LanLobbyAction::Create { observe: _ } => {
            let observe = state
                .frontend
                .multiplayer_shell_runtime
                .lan_create_observer();
            let error = create_error(
                &state.frontend.lan.as_ref().expect("active BB").lobby,
                &name,
            );
            if let Some(key) = error {
                state.frontend.lan.as_mut().unwrap().lobby.set_enabled(true);
                show_modal(state, key);
            } else {
                App::leave_shell_dialog(
                    state,
                    shell_transition::ShellExitThen::LanCreate { observe },
                );
            }
        }
        LanLobbyAction::Join { game_id, observe } => {
            let reserved = reserved_names(state);
            let error = join_error(
                &state.frontend.lan.as_ref().unwrap().lobby,
                &name,
                &reserved,
            );
            if let Some(key) = error {
                state.frontend.lan.as_mut().unwrap().lobby.set_enabled(true);
                report_error(state, key);
            } else if let Some(game_id) = game_id {
                let runtime = state.frontend.lan.as_mut().unwrap();
                runtime.pending_join = Some(PendingJoin {
                    game_id,
                    observe,
                    expires: Instant::now() + JOIN_TIMEOUT,
                });
                runtime.lobby.selection_enabled = false;
                runtime
                    .requests
                    .push(LanServiceRequest::Join { game_id, observe });
            }
        }
        LanLobbyAction::Back => {
            App::leave_shell_dialog(state, shell_transition::ShellExitThen::LanBack)
        }
        LanLobbyAction::ObserverChanged(value) => state
            .frontend
            .multiplayer_shell_runtime
            .set_lan_observer(value),
        LanLobbyAction::NameChanged(value) => {
            state.frontend.multiplayer_shell_runtime.set_handle(&value);
            let runtime = state.frontend.lan.as_mut().unwrap();
            runtime
                .requests
                .push(LanServiceRequest::NameChanged(value.clone()));
            // Local discovery row is a projection. It is always first only
            // when looking at the Lobby sentinel, never a selected game's roster.
            if runtime.lobby.game_list.selected == Some(0) {
                if let Some(local) = runtime.lobby.players.first_mut() {
                    local.name = value;
                } else {
                    runtime.lobby.players.push(local_lobby_row(&value));
                }
            }
        }
        LanLobbyAction::SendChat(message) => {
            let mut display: Vec<u16> = format!("[{name}] ").encode_utf16().collect();
            display.extend_from_slice(&message);
            append_chat(
                state,
                LanChatLine {
                    text: display,
                    //5DDEB9..5DDECA: echo uses7EEF44, outgoing uses7EEF2C.
                    color: 0x0040_FFFF,
                },
            );
            state
                .frontend
                .lan
                .as_mut()
                .unwrap()
                .requests
                .push(LanServiceRequest::Chat {
                    text: message,
                    color: 0x0000_F0F0,
                });
        }
        LanLobbyAction::SelectGame { game_id, changed } => {
            let runtime = state.frontend.lan.as_mut().unwrap();
            if game_id.is_none() {
                runtime.lobby.replace_players(vec![local_lobby_row(&name)]);
            }
            runtime
                .requests
                .push(LanServiceRequest::SelectGame { game_id, changed });
        }
    }
    state.platform.window.request_redraw();
}

/// Only declared service events mutate discovery/request state. Accepted data
/// is moved into the transition and then MatchSession, never cached as a second
/// participant table. Stale replies cannot accept a different pending game.
pub(crate) fn service_event(state: &mut AppState, event: LanServiceEvent) {
    if state.frontend.shell_route != crate::app::shell_route::ShellRoute::LanLobby {
        return;
    }
    let Some(runtime) = state.frontend.lan.as_mut() else {
        return;
    };
    match event {
        LanServiceEvent::Games(games) => {
            let notices = discovery_notices(&runtime.lobby.games, &games);
            let selection = runtime.lobby.replace_games(games);
            for (name, open, new_record) in notices {
                discovery_notice(state, &name, open, new_record);
            }
            if let Some(action) = selection {
                dispatch(state, action);
            }
        }
        LanServiceEvent::Players { game_id, players } => {
            if runtime.lobby.selected_game().map(|game| game.id) == game_id {
                runtime.lobby.replace_players(players);
            }
        }
        LanServiceEvent::Chat(line) => append_chat(state, line),
        LanServiceEvent::JoinDenied { game_id, reason } => {
            let Some(pending) = runtime
                .pending_join
                .as_ref()
                .filter(|pending| pending.game_id == game_id)
            else {
                return;
            };
            let message = denial_modal(reason, pending.observe);
            runtime.release_request();
            report_error(state, "TXT_REQUEST_DENIED");
            if let Some(message) = message {
                show_modal(state, message);
            }
        }
        LanServiceEvent::JoinAccepted(accepted) => {
            if runtime.pending_game() != Some(accepted.game_id) {
                return;
            }
            runtime.pending_join = None;
            App::leave_shell_dialog(
                state,
                shell_transition::ShellExitThen::LanJoinAccepted(accepted),
            );
        }
    }
    state.platform.window.request_redraw();
}

/// Type1 discovery5E0218..5E04E0. The existing discovery projection owns the
/// previous open flag. Missing/unchanged/initially-closed rows are silent.
fn discovery_notices(old: &[LanGameRow], incoming: &[LanGameRow]) -> Vec<(String, bool, bool)> {
    incoming
        .iter()
        .filter_map(|game| match old.iter().find(|old| old.id == game.id) {
            Some(old) if old.open != game.open => Some((game.name.clone(), game.open, false)),
            None if game.open => Some((game.name.clone(), true, true)),
            _ => None,
        })
        .collect()
}

fn discovery_notice(state: &mut AppState, name: &str, open: bool, new_record: bool) {
    let key = if open {
        "TXT_S_FORMED_NEW_GAME"
    } else {
        "TXT_GAME_NOW_IN_PROGRESS"
    };
    let text = crate::assets::csf_file::format_csf(
        &App::csf_label(state, key, key),
        &[crate::assets::csf_file::CsfArg::Str(name)],
    );
    let sound = state
        .rules()
        .and_then(|rules| {
            if open {
                rules.general.game_forming_sound()
            } else {
                rules.general.game_closed_sound()
            }
        })
        .map(str::to_owned);
    // Existing-row transitions play5E030A before chat5E031E; new open ads
    // append5E04C1 before playing5E04E0. Both run only in membership0/1 (BB).
    if !new_record {
        App::play_shell_ui_sound_by_id(state, sound.as_deref());
    }
    append_chat(
        state,
        LanChatLine {
            text: text.encode_utf16().collect(),
            color: u32::MAX,
        },
    );
    if new_record {
        App::play_shell_ui_sound_by_id(state, sound.as_deref());
    }
}

fn acknowledge(state: &mut AppState) {
    if let Some(runtime) = state.frontend.lan.as_mut() {
        runtime.modal = None;
        runtime.lobby.cancel_pointer();
    }
    state.frontend.shell_controller.pop();
    state.platform.window.request_redraw();
}

pub(crate) fn key(state: &mut AppState, event: &KeyEvent) {
    if event.state.is_pressed() && !shell_transition::blocks_shell_input(state) {
        let code = match event.physical_key {
            PhysicalKey::Code(code) => Some(code),
            _ => None,
        };
        key_press(state, code, event.text.as_deref());
    }
}

pub(crate) fn key_press(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
    let Some(runtime) = state.frontend.lan.as_mut() else {
        return;
    };
    if runtime.modal.is_some() {
        if matches!(
            code,
            Some(KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Escape)
        ) {
            acknowledge(state);
        }
        return;
    }
    let edit = match code {
        Some(KeyCode::Backspace) => Some(LanEditKey::Backspace),
        Some(KeyCode::Delete) => Some(LanEditKey::Delete),
        Some(KeyCode::ArrowLeft) => Some(LanEditKey::Left),
        Some(KeyCode::ArrowRight) => Some(LanEditKey::Right),
        Some(KeyCode::Home) => Some(LanEditKey::Home),
        Some(KeyCode::End) => Some(LanEditKey::End),
        Some(KeyCode::Tab) => Some(LanEditKey::Tab),
        Some(KeyCode::Enter | KeyCode::NumpadEnter) => Some(LanEditKey::Enter),
        //614B30 consumes Escape in either NewEdit; BB command2 is not sent.
        Some(KeyCode::Escape) => return,
        _ => None,
    };
    let actions = if let Some(edit) = edit {
        runtime.lobby.edit_key(edit).into_iter().collect()
    } else {
        runtime.lobby.insert_text(text.unwrap_or_default())
    };
    for action in actions {
        dispatch(state, action);
    }
    state.platform.window.request_redraw();
}

fn pointer_control(state: &AppState) -> Option<u16> {
    let (x, y) = state.match_state.input.cursor_px();
    if state.frontend.lan.as_ref()?.modal.is_some() {
        return crate::ui::shell::modal::body_ok_layout(
            state.render_width() as i32,
            state.render_height() as i32,
        )
        .ok
        .contains(x, y)
        .then_some(OK);
    }
    layout(state).control_at(x, y)
}

pub(crate) fn update_geometry(state: &mut AppState) {
    let layout = layout(state);
    if let Some(runtime) = state.frontend.lan.as_mut() {
        let lobby = &mut runtime.lobby;
        for (list, rect, count) in [
            (&mut lobby.game_list, layout.games, lobby.games.len() + 1),
            (&mut lobby.player_list, layout.players, lobby.players.len()),
            (
                &mut lobby.chat.list,
                layout.chat_log,
                lobby.chat.lines.len(),
            ),
        ] {
            list.reconcile_geometry(rect, count, layout.page.scale);
        }
        for (edit, rect) in [
            (&mut lobby.name, layout.name),
            (&mut lobby.chat.edit, layout.chat),
        ] {
            edit.scroll_to_caret(
                (layout.page.scale.base_rect(rect).w - 3).max(0) as u32,
                |text| state.renderer.bit_font.text_width(text),
            );
        }
    }
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    let control = pointer_control(state);
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    if let Some(runtime) = state.frontend.lan.as_mut() {
        let lobby = &mut runtime.lobby;
        lobby.buttons.hovered = control;
        if runtime.modal.is_none() && lobby.enabled {
            for (list, rect, count) in [
                (&mut lobby.game_list, layout.games, lobby.games.len() + 1),
                (&mut lobby.player_list, layout.players, lobby.players.len()),
                (
                    &mut lobby.chat.list,
                    layout.chat_log,
                    lobby.chat.lines.len(),
                ),
            ] {
                let geometry = list.geometry(rect, count, layout.page.scale);
                list.hot_row = geometry.row_at(count, list.top_index, x, y);
                list.scroll
                    .pointer_moved(geometry, &mut list.top_index, x, y);
            }
        }
    }
    state.platform.window.request_redraw();
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button != MouseButton::Left || shell_transition::blocks_shell_input(state) {
        return;
    }
    update_geometry(state);
    let control = pointer_control(state);
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let Some(runtime) = state.frontend.lan.as_mut() else {
        return;
    };
    if runtime.modal.is_some() {
        if pressed {
            runtime.lobby.buttons.press(control);
            if control == Some(OK) {
                App::play_main_menu_button_sound(state);
            }
        } else if runtime.lobby.buttons.release(control) == Some(OK) {
            acknowledge(state);
        }
        state.platform.window.request_redraw();
        return;
    }
    let lobby = &mut runtime.lobby;
    if !lobby.enabled {
        return;
    }
    if pressed {
        for (list, rect, count) in [
            (&mut lobby.game_list, layout.games, lobby.games.len() + 1),
            (&mut lobby.player_list, layout.players, lobby.players.len()),
            (
                &mut lobby.chat.list,
                layout.chat_log,
                lobby.chat.lines.len(),
            ),
        ] {
            let geometry = list.geometry(rect, count, layout.page.scale);
            if let Some(part) = geometry.scroll_part_at(x, y) {
                list.scroll
                    .press(part, geometry, &mut list.top_index, y, Instant::now());
                state.platform.window.request_redraw();
                return;
            }
        }
        if control == Some(GAMES) {
            let geometry =
                lobby
                    .game_list
                    .geometry(layout.games, lobby.game_count(), layout.page.scale);
            if let Some(row) = geometry.row_at(lobby.game_count(), lobby.game_list.top_index, x, y)
            {
                let double = crate::ui::shell::list::is_double_click(
                    &mut lobby.game_list.last_press,
                    Instant::now(),
                    x,
                    y,
                    crate::ui::shell::saved_file_input::host_double_click_limits(),
                );
                let selection = lobby.select_game(row);
                let join = double.then(|| lobby.activate(JOIN)).flatten();
                if let Some(action) = selection {
                    dispatch(state, action);
                }
                if let Some(action) = join {
                    dispatch(state, action);
                }
                App::play_launcher_options_cue(state, LauncherCue::GenericClick);
            }
        } else if matches!(control, Some(OBSERVER_IMAGE | OBSERVER_TOGGLE)) {
            let icon_hit = control == Some(OBSERVER_IMAGE)
                || crate::ui::skirmish_shell::checkbox_icon_rect(
                    layout.observer_toggle,
                    layout.page.scale,
                )
                .contains(x, y);
            if icon_hit {
                let action = lobby.activate(control.unwrap());
                if let Some(action) = action {
                    dispatch(state, action);
                }
                App::play_launcher_options_cue(state, LauncherCue::Checkbox);
            }
        } else {
            if let Some(control) = control {
                lobby.focus(control);
            }
            lobby
                .buttons
                .press(control.filter(|control| matches!(*control, CREATE | JOIN | BACK)));
            if lobby.buttons.pressed.is_some() {
                App::play_main_menu_button_sound(state);
            }
        }
    } else {
        lobby.game_list.scroll.cancel();
        lobby.player_list.scroll.cancel();
        lobby.chat.list.scroll.cancel();
        let action = lobby
            .buttons
            .release(control)
            .and_then(|control| lobby.activate(control));
        if let Some(action) = action {
            dispatch(state, action);
        }
    }
    state.platform.window.request_redraw();
}

/// Service timeout and shared scrollbar repeats; the caller schedules the
/// earliest returned deadline alongside the existing shell timers.
pub(crate) fn poll(state: &mut AppState) -> Option<Instant> {
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let now = Instant::now();
    let runtime = state.frontend.lan.as_mut()?;
    if runtime
        .pending_join
        .as_ref()
        .is_some_and(|request| now >= request.expires)
    {
        let game_id = runtime.pending_game().unwrap();
        runtime
            .requests
            .push(LanServiceRequest::CancelJoin { game_id });
        runtime.release_request();
        report_error(state, "TXT_REQUEST_DENIED");
    }
    let runtime = state.frontend.lan.as_mut()?;
    let mut deadline = runtime.pending_join.as_ref().map(|request| request.expires);
    let lobby = &mut runtime.lobby;
    if runtime.modal.is_none() && lobby.enabled {
        for (list, rect, count) in [
            (&mut lobby.game_list, layout.games, lobby.games.len() + 1),
            (&mut lobby.player_list, layout.players, lobby.players.len()),
            (
                &mut lobby.chat.list,
                layout.chat_log,
                lobby.chat.lines.len(),
            ),
        ] {
            let geometry = list.geometry(rect, count, layout.page.scale);
            if list.scroll.poll(geometry, &mut list.top_index, x, y, now) {
                state.platform.window.request_redraw();
            }
            if let Some(next) = list.scroll.repeat_at() {
                deadline = Some(deadline.map_or(next, |old| old.min(next)));
            }
        }
    }
    deadline
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_notifies_formation_and_open_flag_changes_without_repeating_advertisements() {
        let mut lobby = LanLobbyState::new("Tester", false);
        let open = LanGameRow {
            id: 7,
            name: "Host".into(),
            open: true,
        };
        let closed = LanGameRow {
            id: 8,
            name: "Already playing".into(),
            open: false,
        };
        let advertised = vec![open.clone(), closed];
        assert_eq!(
            discovery_notices(&lobby.games, &advertised),
            vec![("Host".into(), true, true)]
        );
        lobby.replace_games(advertised.clone());
        assert!(discovery_notices(&lobby.games, &advertised).is_empty());

        let mut started = open.clone();
        started.open = false;
        assert_eq!(
            discovery_notices(&lobby.games, &[started.clone()]),
            vec![("Host".into(), false, false)]
        );
        lobby.replace_games(vec![started]);
        assert_eq!(
            discovery_notices(&lobby.games, &[open]),
            vec![("Host".into(), true, false)]
        );
        assert!(discovery_notices(&lobby.games, &[]).is_empty());
    }

    #[test]
    fn native_create_and_join_have_different_name_rules_and_error_surfaces() {
        let mut lobby = LanLobbyState::new("<Open>", false);
        assert_eq!(create_error(&lobby, ""), Some("TXT_NAME_BLANK"));
        assert_eq!(create_error(&lobby, "<Open>"), None);
        assert_eq!(join_error(&lobby, "", &[]), Some("TXT_MUST_SELECT_GAME"));
        lobby.replace_games(vec![LanGameRow {
            id: 7,
            name: "Alice".into(),
            open: false,
        }]);
        lobby.select_game(1);
        assert_eq!(
            create_error(&lobby, "aLiCe"),
            Some("TXT_GAMENAME_MUSTBE_UNIQUE")
        );
        assert_eq!(join_error(&lobby, "", &[]), Some("TXT_NAME_ERROR"));
        assert_eq!(
            join_error(&lobby, "<Open>", &[]),
            Some("ERROR:NameIsReservedWord")
        );
        assert_eq!(
            join_error(&lobby, "<open>", &[]),
            Some("TXT_GAME_IS_CLOSED")
        );
        lobby.games[0].open = true;
        assert_eq!(join_error(&lobby, "<open>", &[]), None);
    }
    #[test]
    fn pending_join_disables_admission_until_a_service_result_releases_it() {
        let mut runtime = LanShellRuntime::new("Tester", false);
        runtime.lobby.replace_games(vec![LanGameRow {
            id: 7,
            name: "Host".into(),
            open: true,
        }]);
        runtime.lobby.select_game(1);
        assert_eq!(
            runtime.lobby.activate(JOIN),
            Some(LanLobbyAction::Join {
                game_id: Some(7),
                observe: false
            })
        );
        runtime.pending_join = Some(PendingJoin {
            game_id: 7,
            observe: false,
            expires: Instant::now() + JOIN_TIMEOUT,
        });
        assert!(!runtime.lobby.enabled);
        assert_eq!(runtime.lobby.activate(CREATE), None);
        assert_eq!(runtime.lobby.select_game(0), None);
        assert_eq!(runtime.pending_game(), Some(7));
        runtime.release_request();
        assert_eq!(runtime.pending_game(), None);
        assert!(runtime.lobby.enabled);
        assert_eq!(
            runtime.lobby.activate(CREATE),
            Some(LanLobbyAction::Create { observe: false })
        );
    }

    #[test]
    fn native_rejection_reason_one_uses_observer_request_and_unknown_reason_has_no_modal() {
        assert_eq!(denial_modal(1, false), Some("TXT_GAME_FULL"));
        assert_eq!(denial_modal(1, true), Some("TXT_OBSERVER_FULL"));
        assert_eq!(denial_modal(4, false), None);
        assert_eq!(denial_modal(7, false), Some("TXT_SERIAL_DUP"));
    }
}
