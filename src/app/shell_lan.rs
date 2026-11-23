//! LAN BB app boundary: original lobby owner005DC350, proc005DDBD0,
//! join validation005DF410 and receive005E00B0. Transport supplies discovery
//! and replies here; only MatchSession owns admitted participants/setup.

use super::{App, AppState};
use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::match_runtime::session::{
    LanRoomRejoin, MatchSession, NetworkReturnContext, NetworkRoomReturn,
};
use crate::app::shell_route::ShellRoute;
use crate::net::lan_messages::setup;
use crate::skirmish_launch::SkirmishHumanSlot;
use crate::ui::lan_shell::*;
use crate::ui::room_limits::RoomLimits;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::page::{self, PageCue, PageEvent};
use crate::util::time::{Duration, Instant};
use vera20k_gui::shell::list::{ListMetrics, ShellListGeometry};
use vera20k_gui::{NodeKind, NodeSpec, Prop, Value};
use winit::event::{KeyEvent, MouseButton};
use winit::keyboard::{KeyCode, PhysicalKey};

pub(crate) const MODAL: DialogResourceId = DialogResourceId(0xCE);
pub(crate) const OK: u16 = 1;
pub(crate) const CANCEL: u16 = 2;
/// VERA's Create Game box's Max players and Allow spectators (its own
/// controls, no resource's).
pub(crate) const ROOM_PLAYERS: u16 = 0x7F20;
pub(crate) const ROOM_SPECTATORS: u16 = 0x7F21;
/// The Max players caption beside it.
pub(crate) const ROOM_PLAYERS_CAPTION: u16 = 0x7F22;
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
// 5EE100's table at 0x831704, in its order: the roster sentinels. 5DF410
// calls it at 0x5DF6B4, after the seven labels above, and refuses a join name
// equal to one of them (strcmp, exact case).
const RESERVED_LITERALS: [&str; 7] = [
    setup::OPEN,
    setup::CLOSED,
    setup::WAITING,
    setup::EASY_AI,
    setup::MEDIUM_AI,
    setup::HARD_AI,
    setup::OPEN_OBSERVER,
];

/// Why a returning guest's wait for its host's room ended without a join.
#[derive(Debug, PartialEq, Eq)]
enum RejoinEnd {
    /// The room was not listed open within the wait.
    HostDidNotReturn(String),
    /// The room is back, but Join's name checks refuse the name: their key.
    NameRefused(&'static str),
}

/// Semantic UI requests consumed by the LAN transport adapter. Controlled
/// captures use the same boundary; no request manufactures a successful reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LanServiceRequest {
    Refresh,
    SelectGame {
        game_id: Option<u64>,
        change: LanSelectionChange,
    },
    NameChanged,
    Chat {
        text: Vec<u16>,
    },
    Join {
        game_id: u64,
        observe: bool,
    },
    CancelJoin {
        game_id: u64,
    },
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
    JoinDenied {
        game_id: u64,
        reason: u32,
    },
    JoinCanceled {
        game_id: u64,
    },
    JoinAccepted(LanJoinAcceptance),
}

#[derive(Debug, Clone)]
struct PendingJoin {
    game_id: u64,
    observe: bool,
    expires: Instant,
    canceling: bool,
}

pub(crate) struct LanShellRuntime {
    pub(crate) lobby: LanLobbyState,
    /// The single-OK box's label, a CSF or `VERA:` one ([`crate::i18n::label`]).
    pub(crate) modal: Option<&'static str>,
    /// VERA: the box's text when it says more than its label: why the host
    /// refused a join whose rules differ ([`agreement::rules_differ_text`]).
    ///
    /// [`agreement::rules_differ_text`]: crate::app::shell_network_setup::agreement::rules_differ_text
    pub(crate) modal_body: Option<String>,
    /// VERA's Create Game box is up; its widgets hold the room's limits.
    pub(crate) creating: bool,
    pending_join: Option<PendingJoin>,
    requests: Vec<LanServiceRequest>,
}

impl LanShellRuntime {
    fn new(name: &str) -> Self {
        Self {
            lobby: LanLobbyState::new(name),
            modal: None,
            modal_body: None,
            creating: false,
            pending_join: None,
            requests: vec![LanServiceRequest::Refresh],
        }
    }

    pub(crate) fn take_service_requests(&mut self) -> Vec<LanServiceRequest> {
        std::mem::take(&mut self.requests)
    }

    /// Join the returning guest's host room the way Join would, once it is
    /// listed open, unless Join's name checks refuse the name (`name_error`).
    /// Returns why the wait ended without a join.
    fn rejoin_host(
        &mut self,
        session: &mut MatchSession,
        observe: bool,
        name_error: Option<&'static str>,
        now: Instant,
    ) -> Option<RejoinEnd> {
        let available = self.pending_join.is_none() && !self.creating && self.modal.is_none();
        let game_id = match session.poll_lan_room_rejoin(now, |id| {
            available
                && self
                    .lobby
                    .games
                    .iter()
                    .any(|game| game.open && game.id == id)
        })? {
            LanRoomRejoin::Join(game_id) => game_id,
            LanRoomRejoin::Expired(host) => return Some(RejoinEnd::HostDidNotReturn(host)),
        };
        if let Some(key) = name_error {
            return Some(RejoinEnd::NameRefused(key));
        }
        self.pending_join = Some(PendingJoin {
            game_id,
            observe,
            expires: now + JOIN_TIMEOUT,
            canceling: false,
        });
        self.lobby.selection_enabled = false;
        self.requests
            .push(LanServiceRequest::Join { game_id, observe });
        None
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
    shell_layout::on(state, LanLobbyLayout::new)
}

pub(crate) fn create_box_layout(state: &AppState) -> crate::ui::shell::modal::LanCreateGameLayout {
    shell_layout::on(state, crate::ui::shell::modal::lan_create_game_layout_on)
}

impl App {
    /// MainMenu52E418 and setup-return5DC624/5DC964 construct a fresh BB.
    /// The retained preference owner supplies the edit; admission stays empty.
    pub(crate) fn open_lan_lobby(state: &mut AppState) {
        crate::app::shell_main_menu::clear_typed_codes_for_network_session(state);
        if !Self::prepare_multiplayer_settings(
            state,
            crate::app::match_runtime::session::SessionMode::Lan,
        ) {
            return;
        }
        if !Self::ensure_skirmish_shell_chrome(state) {
            return;
        }
        state
            .match_state
            .session
            .enter_network(NetworkReturnContext::LanLobby);
        let owner = &mut state.frontend.multiplayer_shell_runtime;
        let local = owner.local_participant();
        // The name box shows the name as typed; the lobby row shows it as
        // every other player sees it.
        let name = owner.open_lan_name_box();
        let mut runtime = LanShellRuntime::new(&name);
        runtime
            .lobby
            .replace_players(vec![local_lobby_row(&local.name)]);
        state.frontend.lan = Some(runtime);
        // Android routes a socket by the process's network when the socket is
        // made: bind the process to Wi-Fi before the visit opens its sockets,
        // and watch that network while the lobby is open.
        #[cfg(target_os = "android")]
        crate::util::lan_network::acquire();
        crate::app::lan_service::open(state);
        App::destroy_current_shell_dialog(state);
        let route = ShellRoute::LanLobby;
        App::show_shell_page(state, route, PAGE.dialog);
        state.frontend.shell_first_paint_slide = None;
    }

    /// Called only after BB's ordinary teardown slide finishes.
    pub(crate) fn commit_lan_back(state: &mut AppState) {
        state
            .frontend
            .multiplayer_shell_runtime
            .persist_lan_preferences();
        state.frontend.lan = None;
        crate::app::lan_service::close(state);
        #[cfg(target_os = "android")]
        crate::util::lan_network::release();
        state.match_state.session.leave_network();
        Self::close_single_player_shell(state);
        state
            .frontend
            .shell_controller
            .reset_to(DialogResourceId(0xE2));
        state.frontend.shell_first_paint_slide = None;
    }

    /// After a match: a new BB as always, which the host of VERA's room
    /// leaves at once for the same room again, and where its guests wait to
    /// join it.
    pub(crate) fn reopen_lan_room(state: &mut AppState) {
        let rematch = state.match_state.session.take_network_room_return();
        Self::open_lan_lobby(state);
        if state.frontend.lan.is_none() {
            return;
        }
        match rematch {
            NetworkRoomReturn::LanHost if !crate::app::lan_service::unavailable(state) => {
                let observe = state
                    .frontend
                    .multiplayer_shell_runtime
                    .lan_create_observer();
                Self::commit_lan_create(state, observe);
            }
            NetworkRoomReturn::LanGuest { host, game_id, .. } => {
                let text = crate::i18n::tr_fill("VERA:LanWaitingForHost", &[("host", &host)]);
                state
                    .match_state
                    .session
                    .wait_for_lan_room(host, game_id, Instant::now());
                append_chat(
                    state,
                    LanChatLine {
                        text: text.encode_utf16().collect(),
                        color: u32::MAX,
                    },
                );
            }
            _ => {}
        }
    }

    pub(crate) fn commit_lan_create(state: &mut AppState, observe: bool) {
        crate::app::lan_service::force_presence(state);
        state.frontend.lan = None;
        Self::open_host_setup(state, observe);
    }
}

pub(crate) fn local_lobby_row(name: &str) -> LanPlayerRow {
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
            state
                .csf_label(key, key)
                .encode_utf16()
                .map(|unit| unit as u8)
                .take_while(|byte| *byte != 0)
                .map(char::from)
                .collect()
        })
        .collect()
}

// Create never reaches 5EE100 (its one caller is 5DF410's join), so a host may
// take a sentinel name, as in gamemd. Guests then read the host's roster row
// as that kind (5EDFC0; options.rs parsed_roster, which leaves @Waiting@ to
// its name lookup and so keeps that host). Rare, since the host must type the
// sentinel; whether such a room's start still agrees is not traced.
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
    let selected = lobby.selected_game;
    if selected < 1 {
        Some("TXT_MUST_SELECT_GAME")
    } else if lobby.game_count() < 2 || selected >= lobby.game_count() {
        Some("TXT_NOTHING_TO_JOIN")
    } else if let Some(key) = join_name_error(name, reserved) {
        Some(key)
    } else if lobby.selected_game().is_some_and(|game| !game.open) {
        Some("TXT_GAME_IS_CLOSED")
    } else {
        None
    }
}

/// 5DF410's checks of the name alone, between the selection and closed-game
/// checks: blank, one of the labels or one of 5EE100's sentinels.
fn join_name_error(name: &str, reserved: &[String]) -> Option<&'static str> {
    if name.is_empty() {
        Some("TXT_NAME_ERROR")
    } else if RESERVED_LITERALS.contains(&name) || reserved.iter().any(|value| value == name) {
        Some("ERROR:NameIsReservedWord")
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
        // VERA direct-client admission extends native version checks with the
        // actual source build and enabled gameplay feature identity.
        8 => Some("VERA:LanBuildDiffers"),
        // VERA's rooms: the host kicked this player within five minutes;
        // the online join ban's text (`0x7A5890`'s 0x80040072).
        9 => Some("TXT_JOINBAN"),
        _ => None,
    }
}

fn show_modal(state: &mut AppState, key: &'static str) {
    let runtime = state.frontend.lan.as_mut().expect("active BB");
    runtime.modal = Some(key);
    runtime.modal_body = None;
    push_box(state, box_spec(&[OK]));
}

/// A box's root with its buttons.
/// OK takes Enter, and Escape when the box has no Cancel.
fn box_spec(buttons: &[u16]) -> NodeSpec {
    let label = |id| if id == CANCEL { "GUI:Cancel" } else { "GUI:OK" };
    let cancel = if buttons.contains(&CANCEL) {
        CANCEL
    } else {
        OK
    };
    let button = |id: &u16| {
        NodeSpec::new(NodeKind::Button)
            .id((*id).into())
            .label(label(*id))
            .with(Prop::Default, Value::Bool(*id == OK))
            .with(Prop::Cancel, Value::Bool(*id == cancel))
    };
    NodeSpec::new(NodeKind::Dialog).children(buttons.iter().map(button))
}

/// Pushes the box `0xCE` with its widgets over BB, which lets go of the
/// pointer.
fn push_box(state: &mut AppState, spec: NodeSpec) {
    let controller = &mut state.frontend.shell_controller;
    controller.gui_mut().cancel_pointer();
    controller.push_modal(MODAL);
    if let Some(root) = controller.root_in(MODAL) {
        page::ensure_built(controller.gui_mut(), root, || spec);
    }
    sync_gui(state);
}

/// Select the active LAN dialog's transcript; the shared chat owner retains
/// all wrapping, row retention and scroll behavior for BB, BC and BD.
pub(crate) fn append_chat(state: &mut AppState, line: LanChatLine) {
    append_chat_sent_at(state, line, None);
}

/// [`append_chat`] for a line the WOL service sent at `sent_at` (Unix
/// seconds): VERA20K Online's chat lobby shows that time, not the current one.
pub(crate) fn append_chat_sent_at(state: &mut AppState, line: LanChatLine, sent_at: Option<u64>) {
    let route = state.frontend.shell_route;
    // WOL Quick Match's 0x117 is the top dialog with a 0x53F.
    if route == ShellRoute::Wol && crate::app::shell_wol::quick_match::page_open(state) {
        crate::app::shell_wol::quick_match::append_line(state, line);
        return;
    }
    let setup = route == ShellRoute::NetworkSetup;
    append_chat_to(
        state,
        setup,
        !setup && route == ShellRoute::WolLobby,
        line,
        sent_at,
    );
}

/// [`append_chat`] for a line of the game room's: VERA keeps a lobby
/// visitor's room going hidden (docs/lobby-protocol.md "Visiting the lobby"),
/// and the line goes to its transcript, not to the lobby page on screen.
pub(crate) fn append_room_chat(state: &mut AppState, line: LanChatLine) {
    append_room_chat_sent_at(state, line, None);
}

/// [`append_room_chat`] for a line the WOL service sent at `sent_at`.
pub(crate) fn append_room_chat_sent_at(
    state: &mut AppState,
    line: LanChatLine,
    sent_at: Option<u64>,
) {
    if crate::app::shell_wol::lobby::visiting(state) {
        append_chat_to(state, true, false, line, sent_at);
    } else {
        append_chat_sent_at(state, line, sent_at);
    }
}

/// The WOL lobby `0x113`'s transcript even while C2/C9 cover it: native
/// keeps it in the shell stack, hidden, and its sinks still write to it.
/// `sent_at` as for [`append_chat_sent_at`].
pub(crate) fn append_wol_lobby_chat(state: &mut AppState, line: LanChatLine, sent_at: Option<u64>) {
    append_chat_to(state, false, true, line, sent_at);
}

fn append_chat_to(
    state: &mut AppState,
    setup: bool,
    wol_lobby: bool,
    mut line: LanChatLine,
    sent_at: Option<u64>,
) {
    // Only the modern chat lobby's lines show a time (the native shell UI
    // stays as gamemd draws it).
    if wol_lobby
        && state
            .frontend
            .wol
            .as_ref()
            .is_some_and(|visit| visit.modern_ui())
    {
        let now = crate::util::time::SystemTime::now()
            .duration_since(crate::util::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        line.text = crate::ui::wol_chat::timed_line(line.text, sent_at, now);
    }
    let (rect, scale, text) = if setup {
        let layout = crate::app::input::multiplayer_setup::layout(state);
        (layout.chat_log, layout.page.scale, layout.chat_text_scale())
    } else if wol_lobby {
        // The online page's chat may be hidden behind its games: wrap to
        // the log of its chat view.
        let layout = crate::app::shell_wol::lobby::chat_layout(state);
        let chat_log = layout.control_rect(crate::ui::wol_lobby::CHAT_LOG);
        (chat_log.unwrap_or_default(), layout.page.scale, 1.0)
    } else {
        let layout = layout(state);
        (
            layout.rect(CHAT_LOG).unwrap_or_default(),
            layout.page.scale,
            1.0,
        )
    };
    let width = crate::ui::lan_shell::chat_wrap_width(rect, scale, text);
    let setup_page = setup.then(|| crate::app::shell_network_setup::dialog(state).page().dialog);
    let font = &state.renderer.bit_font;
    let measure = |units: &[u16]| font.text_width(&String::from_utf16_lossy(units)) as i32;
    let frontend = &mut state.frontend;
    let controller = &mut frontend.shell_controller;
    // The transcript and its log's list, which keeps the view.
    let (chat, gui, list) = if let Some(page) = setup_page {
        let list = controller.control(page, crate::ui::multiplayer_setup::CHAT_LOG);
        let Some(ui) = frontend.multiplayer_setup.as_mut() else {
            return;
        };
        // A room hidden behind the lobby keeps its log's view parked.
        if list.is_none()
            && let Some(parked) = ui.parked_chat.as_mut()
        {
            let metrics = ListMetrics::for_text(scale, text);
            let visible = ShellListGeometry::new(rect, 0, 0, metrics).visible_rows;
            let log = |rows, dropped| parked.log(rows, dropped, visible);
            ui.chat.append_message(line, width, measure, log);
            return;
        }
        (&mut ui.chat, controller.gui_mut(), list)
    } else if wol_lobby {
        if let Some(lobby) = frontend.wol.as_mut().and_then(|visit| visit.lobby_mut()) {
            lobby.ui.append_chat(line, width, measure);
            // The view button counts it while the games show.
            lobby.ui.chat_arrived();
        }
        return;
    } else {
        let list = controller.control(PAGE.dialog, CHAT_LOG);
        let Some(runtime) = frontend.lan.as_mut() else {
            return;
        };
        (&mut runtime.lobby.chat, controller.gui_mut(), list)
    };
    chat.append_message(line, width, measure, log_to(gui, list));
}

/// Append the line `key` (a CSF or `VERA:` label, [`crate::i18n::label`]) to
/// the active lobby's or room's chat ([`append_chat`]), with the system error
/// sound.
pub(crate) fn report_error(state: &mut AppState, key: &str) {
    let text = state.csf_label(key, key).encode_utf16().collect();
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
    if matches!(
        action,
        LanLobbyAction::Create { .. } | LanLobbyAction::Join { .. }
    ) && crate::app::lan_service::unavailable(state)
    {
        // Without its sockets the room would be one nobody sees: try them
        // again, and say why when they are still taken.
        crate::app::lan_service::open(state);
        if crate::app::lan_service::unavailable(state) {
            if let Some(runtime) = state.frontend.lan.as_mut() {
                runtime.lobby.set_enabled(true);
            }
            return;
        }
    }
    match action {
        // VERA's room limits are modal within BB (0x005DDBD0). Stock BB's
        // Create command goes straight to room setup.
        LanLobbyAction::Create { observe: _ } if !state.frontend.native_shell_ui => {
            let limits = state.frontend.room_limits;
            state.frontend.lan.as_mut().expect("active BB").creating = true;
            let choices = crate::ui::room_limits::widgets(limits, ROOM_PLAYERS, ROOM_SPECTATORS);
            let align = crate::ui::shell::geom::ShellAlign::H_RIGHT
                | crate::ui::shell::geom::ShellAlign::V_CENTER;
            let caption = crate::ui::wol_page::static_text(
                ROOM_PLAYERS_CAPTION,
                Some("VERA:MaxPlayers"),
                align,
            );
            push_box(
                state,
                box_spec(&[OK, CANCEL]).children(choices).child(caption),
            );
        }
        LanLobbyAction::Create { observe: _ } => create(state, &name),
        LanLobbyAction::Join { game_id, observe } => {
            state.match_state.session.take_network_room_return();
            let reserved = reserved_names(state);
            let error = join_error(
                &state.frontend.lan.as_ref().expect("active BB").lobby,
                &name,
                &reserved,
            );
            if let Some(key) = error {
                state
                    .frontend
                    .lan
                    .as_mut()
                    .expect("active BB")
                    .lobby
                    .set_enabled(true);
                report_error(state, key);
            } else if let Some(game_id) = game_id {
                let runtime = state.frontend.lan.as_mut().expect("active BB");
                runtime.pending_join = Some(PendingJoin {
                    game_id,
                    observe,
                    expires: Instant::now() + JOIN_TIMEOUT,
                    canceling: false,
                });
                runtime.lobby.selection_enabled = false;
                runtime
                    .requests
                    .push(LanServiceRequest::Join { game_id, observe });
            }
        }
        LanLobbyAction::Back => App::leave_shell_dialog(
            state,
            shell_transition::ShellSlideKind::LanLobby,
            App::commit_lan_back,
        ),
        LanLobbyAction::ObserverChanged(value) => state
            .frontend
            .multiplayer_shell_runtime
            .set_lan_observer(value),
        LanLobbyAction::NameChanged(value) => {
            state
                .frontend
                .multiplayer_shell_runtime
                .set_lan_name(&value);
            // The row shows the name as sent, as the service's rebuild does.
            let name = state
                .frontend
                .multiplayer_shell_runtime
                .local_participant()
                .name;
            let runtime = state.frontend.lan.as_mut().expect("active BB");
            runtime.requests.push(LanServiceRequest::NameChanged);
            // Local discovery row is a projection. It is always first only
            // when looking at the Lobby sentinel, never a selected game's roster.
            // 0x541's handler rebuilds the roster (0x005DAFE0), so a selected
            // own row loses its selection with its old name.
            if runtime.lobby.selected_game == 0 {
                let mut players = runtime.lobby.players.clone();
                match players.first_mut() {
                    Some(local) => local.name = name,
                    None => players.push(local_lobby_row(&name)),
                }
                runtime.lobby.replace_players(players);
            }
        }
        LanLobbyAction::SendChat(message) => {
            let mut display: Vec<u16> = format!("[{name}] ").encode_utf16().collect();
            display.extend_from_slice(&message);
            append_chat(
                state,
                LanChatLine {
                    text: display,
                    // `0x005DDEB9..0x005DDECA`: the echo uses `[0x7EEF44]`;
                    // the packet's `[0x7EEF2C]` is `lan_service::chat_packet`'s.
                    color: 0x0040_FFFF,
                },
            );
            state
                .frontend
                .lan
                .as_mut()
                .expect("active BB")
                .requests
                .push(LanServiceRequest::Chat { text: message });
        }
        LanLobbyAction::SelectGame { game_id, change } => {
            let runtime = state.frontend.lan.as_mut().expect("active BB");
            if game_id.is_none() {
                runtime.lobby.replace_players(vec![local_lobby_row(&name)]);
            }
            runtime
                .requests
                .push(LanServiceRequest::SelectGame { game_id, change });
        }
    }
}

/// Only declared service events mutate discovery/request state. Accepted data
/// is moved into the transition and then MatchSession, never cached as a second
/// participant table. Stale replies cannot accept a different pending game.
pub(crate) fn service_event(state: &mut AppState, event: LanServiceEvent) {
    if state.frontend.shell_route != ShellRoute::LanLobby {
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
            rejoin_host(state);
        }
        LanServiceEvent::Players { game_id, players } => {
            if runtime.lobby.selected_game().map(|game| game.id) == game_id {
                runtime.lobby.replace_players(players);
            }
        }
        LanServiceEvent::JoinDenied { game_id, reason } => {
            let Some(pending) = runtime
                .pending_join
                .as_ref()
                .filter(|pending| pending.game_id == game_id && !pending.canceling)
            else {
                return;
            };
            let message = denial_modal(reason, pending.observe);
            runtime.release_request();
            report_error(state, "TXT_REQUEST_DENIED");
            if let Some(message) = message {
                show_modal(state, message);
            }
            // VERA: the host compared the rules hash, and retail's text for
            // it sends the player to Westwood. Say what this game changes.
            if reason == 6 {
                let body = crate::app::shell_network_setup::agreement::rules_differ_text(state);
                if let Some(runtime) = state.frontend.lan.as_mut() {
                    runtime.modal_body = Some(body);
                }
            }
        }
        LanServiceEvent::JoinAccepted(accepted) => {
            if !runtime
                .pending_join
                .as_ref()
                .is_some_and(|pending| pending.game_id == accepted.game_id && !pending.canceling)
            {
                return;
            }
            runtime.pending_join = None;
            App::leave_shell_dialog(
                state,
                shell_transition::ShellSlideKind::LanLobby,
                move |state| {
                    App::open_joined_setup(
                        state,
                        accepted.local,
                        accepted.observer_row,
                        crate::ui::multiplayer_setup::SetupFlavor::Lan,
                    );
                },
            );
        }
        LanServiceEvent::JoinCanceled { game_id } => {
            if runtime
                .pending_join
                .as_ref()
                .is_some_and(|pending| pending.game_id == game_id && pending.canceling)
            {
                runtime.release_request();
            }
        }
    }
}

/// VERA: a returning guest joins its host's room as soon as it is listed.
/// The name box stays live while it waits, so the name passes Join's checks
/// first.
fn rejoin_host(state: &mut AppState) {
    let observe = state.frontend.multiplayer_shell_runtime.lan_observer();
    let name = state
        .frontend
        .multiplayer_shell_runtime
        .local_participant()
        .name;
    let name_error = join_name_error(&name, &reserved_names(state));
    let Some(end) = state.frontend.lan.as_mut().and_then(|runtime| {
        runtime.rejoin_host(
            &mut state.match_state.session,
            observe,
            name_error,
            Instant::now(),
        )
    }) else {
        return;
    };
    let host = match end {
        RejoinEnd::HostDidNotReturn(host) => host,
        RejoinEnd::NameRefused(key) => return report_error(state, key),
    };
    let text = crate::i18n::tr_fill("VERA:LanHostDidNotReturn", &[("host", &host)]);
    append_chat(
        state,
        LanChatLine {
            text: text.encode_utf16().collect(),
            color: u32::MAX,
        },
    );
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
        &state.csf_label(key, key),
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

fn create(state: &mut AppState, name: &str) {
    // Opening the picker is reversible; retire an old room wait on confirmation.
    state.match_state.session.take_network_room_return();
    let observe = state
        .frontend
        .multiplayer_shell_runtime
        .lan_create_observer();
    let error = create_error(&state.frontend.lan.as_ref().expect("active BB").lobby, name);
    if let Some(key) = error {
        state
            .frontend
            .lan
            .as_mut()
            .expect("active BB")
            .lobby
            .set_enabled(true);
        show_modal(state, key);
    } else {
        App::leave_shell_dialog(
            state,
            shell_transition::ShellSlideKind::LanLobby,
            move |state| App::commit_lan_create(state, observe),
        );
    }
}

/// The Create Game box's OK keeps its limits for the room and creates it;
/// Cancel returns to the lobby.
fn close_create_box(state: &mut AppState, confirmed: bool) {
    let Some(runtime) = state
        .frontend
        .lan
        .as_mut()
        .filter(|runtime| runtime.creating)
    else {
        return;
    };
    runtime.creating = false;
    let limits = room_limits(state);
    state.frontend.shell_controller.pop();
    if confirmed {
        state.frontend.room_limits = limits;
        let name = state
            .frontend
            .multiplayer_shell_runtime
            .local_participant()
            .name;
        create(state, &name);
    } else if let Some(runtime) = state.frontend.lan.as_mut() {
        runtime.lobby.set_enabled(true);
    }
}

fn acknowledge(state: &mut AppState) {
    if let Some(runtime) = state.frontend.lan.as_mut() {
        runtime.modal = None;
        runtime.modal_body = None;
    }
    state.frontend.shell_controller.pop();
}

/// VERA's Esc menu (`app::shell_menu`): the create box and notices keep Esc.
pub(crate) fn leaves_escape(state: &AppState) -> bool {
    state
        .frontend
        .lan
        .as_ref()
        .is_some_and(|runtime| !runtime.creating && runtime.modal.is_none())
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

/// The chat NewEdit while it takes keys, for the input method
/// (`app::input::ime`). The name edit takes byte text only, so it keeps
/// plain key text.
pub(crate) fn ime_chat_rect(state: &AppState) -> Option<crate::ui::shell::geom::RectPx> {
    let routed = state.frontend.screen == crate::ui::game_screen::GameScreen::MainMenu
        && state.frontend.shell_route == ShellRoute::LanLobby
        && !shell_transition::blocks_shell_input(state);
    let chat = state.frontend.shell_controller.focused_control(PAGE.dialog) == Some(CHAT);
    state
        .frontend
        .lan
        .as_ref()
        .filter(|runtime| {
            routed && runtime.modal.is_none() && !runtime.creating && runtime.lobby.enabled && chat
        })
        .map(|_| layout(state).rect(CHAT).unwrap_or_default())
}

pub(crate) fn key_press(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
    let Some(runtime) = state.frontend.lan.as_mut() else {
        return;
    };
    // The Create box and the notice take the keys through their buttons.
    if runtime.creating || runtime.modal.is_some() {
        if let Some(key) = code.and_then(|code| crate::ui::shell::controller::gui_key(code, false))
        {
            sync_gui(state);
            state
                .frontend
                .shell_controller
                .on_input(vera20k_gui::InputEvent::Key(key));
            handle_events(state);
        }
        return;
    }
    //614B30 consumes Escape in either NewEdit; BB command2 is not sent.
    if code == Some(KeyCode::Escape) {
        return;
    }
    sync_gui(state);
    let Some(runtime) = state.frontend.lan.as_mut() else {
        return;
    };
    let controller = &mut state.frontend.shell_controller;
    let actions = crate::ui::lan_shell::key(controller, &mut runtime.lobby, code, text);
    for action in actions {
        dispatch(state, action);
    }
}

/// Builds BB's widgets once (the name edit with the lobby's name, the chat
/// edit focused) and puts them on the layout and on the lobby: the lists'
/// rows, selections and tops (the chat log's follows new lines), the edits'
/// view of their caret, the observer box and whether the page takes input;
/// and the box over it, if any, on its layout.
pub(crate) fn sync_gui(state: &mut AppState) {
    let layout = layout(state);
    let create = create_box_layout(state);
    let (width, height) = (state.render_width() as i32, state.render_height() as i32);
    let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
    let font = &state.renderer.bit_font;
    let Some(runtime) = state.frontend.lan.as_mut() else {
        return;
    };
    let controller = &mut state.frontend.shell_controller;
    let (Some(root), boxed) = (controller.root_in(PAGE.dialog), controller.root_in(MODAL)) else {
        return;
    };
    controller.set_scale_in(PAGE.dialog, layout.page.scale);
    if page::ensure_built(controller.gui_mut(), root, spec) {
        let name = runtime.lobby.notified_name.clone();
        controller.set_text(PAGE.dialog, NAME, &name);
        controller.focus_control(PAGE.dialog, Some(CHAT));
    }
    let gui = controller.gui_mut();
    page::sync_rects(gui, root, |id| layout.rect(id));
    let lobby = &mut runtime.lobby;
    let _ = gui.set(root, Prop::Enabled, Value::Bool(lobby.enabled));
    // The observer image `0x75F` toggles like its box, anywhere on it.
    for id in [OBSERVER_TOGGLE, OBSERVER_IMAGE] {
        page::set(gui, root, id, Prop::Checked, Value::Bool(observer));
    }
    let image = layout.rect(OBSERVER_IMAGE).unwrap_or_default();
    page::set(gui, root, OBSERVER_IMAGE, Prop::HitRect, Value::Rect(image));
    let metrics = ListMetrics::scaled(layout.page.scale);
    if let Some(log) = gui.find_id(root, CHAT_LOG.into()) {
        let _ = gui.log_rows(log, lobby.chat.lines.len(), 0);
        page::set(gui, root, CHAT_LOG, Prop::Selected, Value::Int(-1));
    }
    for (id, count, selected) in [
        (GAMES, lobby.game_count(), Some(lobby.selected_game)),
        (PLAYERS, lobby.players.len(), lobby.selected_player),
    ] {
        let rect = layout.rect(id).unwrap_or_default();
        let top = page::int(gui, root, id, Prop::Top).unwrap_or(0).max(0) as usize;
        // Discovery or a resized window may leave the top past the range.
        let top = top.min(ShellListGeometry::new(rect, count, 0, metrics).max_top);
        let selected = selected.map_or(-1, |row| row as i32);
        for (prop, value) in [
            (Prop::RowCount, count as i32),
            (Prop::Top, top as i32),
            (Prop::Selected, selected),
        ] {
            page::set(gui, root, id, prop, Value::Int(value));
        }
    }
    for id in [NAME, CHAT] {
        if let Some(edit) = gui.find_id(root, id.into()) {
            let _ = gui.keep_caret_in_view(edit, &|text| font.text_width(text));
        }
    }
    if let Some(boxed) = boxed {
        if runtime.creating {
            controller.set_scale_in(MODAL, create.scale);
        }
        let gui = controller.gui_mut();
        let ok = match runtime.creating {
            true => create.ok,
            false => crate::ui::shell::modal::menu_message_box(1, width, height).button(0),
        };
        page::sync_rects(gui, boxed, |id| match id {
            OK => Some(ok),
            CANCEL => Some(create.cancel),
            ROOM_PLAYERS => Some(create.room.players),
            ROOM_PLAYERS_CAPTION => Some(create.room.players_label),
            ROOM_SPECTATORS => Some(create.room.spectators),
            _ => None,
        });
    }
}

/// The limits VERA's Create Game box shows.
pub(crate) fn room_limits(state: &AppState) -> RoomLimits {
    let controller = &state.frontend.shell_controller;
    let (gui, root) = (controller.gui(), controller.root_in(MODAL));
    root.map_or(state.frontend.room_limits, |root| RoomLimits {
        players: page::int(gui, root, ROOM_PLAYERS, Prop::Pos).unwrap_or_default() as u8,
        spectators: page::checked(gui, root, ROOM_SPECTATORS),
    })
}

/// List `id`'s top row as its widget holds it.
pub(crate) fn list_top(state: &AppState, id: u16) -> usize {
    let controller = &state.frontend.shell_controller;
    let top = controller
        .root_in(PAGE.dialog)
        .and_then(|root| page::int(controller.gui(), root, id, Prop::Top));
    top.unwrap_or(0).max(0) as usize
}

/// Whether BB itself takes the pointer: no box covers it.
fn on_top(state: &AppState) -> bool {
    state.frontend.shell_controller.top_id() == Some(PAGE.dialog)
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    sync_gui(state);
    let (x, y) = state.match_state.input.cursor_px();
    state.frontend.shell_controller.on_pointer_move(x, y);
    handle_events(state);
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button != MouseButton::Left || shell_transition::blocks_shell_input(state) {
        return;
    }
    sync_gui(state);
    let (x, y) = state.match_state.input.cursor_px();
    let controller = &mut state.frontend.shell_controller;
    if !pressed {
        controller.on_pointer_up(x, y);
        return handle_events(state);
    }
    // A press on an edit focuses it; one on the observer box or image
    // toggles it.
    controller.on_pointer_down(x, y);
    handle_events(state);
}

/// What the lobby makes of control `id`'s activation.
fn lobby_command(state: &mut AppState, id: u16) {
    let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
    if let Some(action) = state
        .frontend
        .lan
        .as_mut()
        .and_then(|runtime| runtime.lobby.activate(id, observer))
    {
        dispatch(state, action);
    }
}

/// The widgets' events of the last input, in order: each control's cue,
/// then what it does.
fn handle_events(state: &mut AppState) {
    // The games and players lists play their own GenericClick.
    let events = App::shell_page_events(state, false);
    if events.is_empty() {
        return;
    }
    let creating = state
        .frontend
        .lan
        .as_ref()
        .is_some_and(|runtime| runtime.creating);
    for event in events {
        match event {
            PageEvent::Activated(OK | CANCEL) if creating => {
                close_create_box(state, event == PageEvent::Activated(OK))
            }
            PageEvent::Activated(OK) => acknowledge(state),
            PageEvent::Activated(id)
            | PageEvent::Toggled(id @ (OBSERVER_TOGGLE | OBSERVER_IMAGE), _) => {
                lobby_command(state, id)
            }
            // A second press in time joins the selected game.
            PageEvent::Selected(GAMES, row) | PageEvent::DoubleClicked(GAMES, row) => {
                let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
                let Some(runtime) = state.frontend.lan.as_mut() else {
                    continue;
                };
                let selection = runtime.lobby.select_game(row as usize);
                let join = matches!(event, PageEvent::DoubleClicked(..))
                    .then(|| runtime.lobby.activate(JOIN, observer))
                    .flatten();
                for action in selection.into_iter().chain(join) {
                    dispatch(state, action);
                }
                App::play_page_cue(state, PageCue::ListClick);
            }
            // A double-click's second press only notifies LBN_DBLCLK
            // (0x0061A904..0x0061A945), which this dialog ignores.
            // RESIDUAL: native also focuses the list (SetFocus, 0x0061A9CB),
            // which would stop chat typing; VERA keeps chat focus, as for the
            // games list.
            PageEvent::Selected(PLAYERS, row) => {
                if state
                    .frontend
                    .lan
                    .as_mut()
                    .is_some_and(|runtime| runtime.lobby.select_player(row as usize))
                {
                    App::play_page_cue(state, PageCue::ListClick);
                }
            }
            _ => {}
        }
    }
}

/// Service timeout and the lists' scrollbar repeats; the caller schedules
/// the earliest returned deadline alongside the existing shell timers.
pub(crate) fn poll(state: &mut AppState) -> Option<Instant> {
    let now = Instant::now();
    let runtime = state.frontend.lan.as_mut()?;
    if let Some(request) = runtime
        .pending_join
        .as_mut()
        .filter(|request| !request.canceling && now >= request.expires)
    {
        // Native5DF410 bounds the request by600 wall buckets. VERA also
        // retires a possibly committed UDP admission before enabling retry;
        // an old reply may not enter setup after the timeout was reported.
        request.canceling = true;
        let game_id = request.game_id;
        runtime
            .requests
            .push(LanServiceRequest::CancelJoin { game_id });
        // Native reports a timeout as a denial; VERA says nothing answered.
        report_error(
            state,
            if state.frontend.native_shell_ui {
                "TXT_REQUEST_DENIED"
            } else {
                "VERA:LanJoinNoAnswer"
            },
        );
    }
    let deadline = state
        .frontend
        .lan
        .as_ref()?
        .pending_join
        .as_ref()
        .filter(|request| !request.canceling)
        .map(|request| request.expires);
    if !on_top(state) {
        return deadline;
    }
    let repeat = state.frontend.shell_controller.poll();
    handle_events(state);
    deadline.into_iter().chain(repeat).min()
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    #[test]
    fn discovery_notifies_formation_and_open_flag_changes_without_repeating_advertisements() {
        let mut lobby = LanLobbyState::new("Tester");
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
        let mut lobby = LanLobbyState::new("@Open@");
        assert_eq!(create_error(&lobby, ""), Some("TXT_NAME_BLANK"));
        assert_eq!(create_error(&lobby, "@Open@"), None);
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
            join_error(&lobby, "@Open@", &[]),
            Some("ERROR:NameIsReservedWord")
        );
        assert_eq!(
            join_error(&lobby, "@open@", &[]),
            Some("TXT_GAME_IS_CLOSED")
        );
        lobby.games[0].open = true;
        assert_eq!(join_error(&lobby, "@open@", &[]), None);
        assert_eq!(join_error(&lobby, "<Open>", &[]), None);
    }
    #[test]
    fn a_returning_guest_joins_its_host_room_once_it_is_open() {
        let now = Instant::now();
        let mut runtime = LanShellRuntime::new("Guest");
        runtime.take_service_requests();
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::LanLobby);
        session.wait_for_lan_room("Host".into(), 7, now);
        let game = |id, name: &str, open| LanGameRow {
            id,
            name: name.into(),
            open,
        };
        runtime
            .lobby
            .replace_games(vec![game(8, "Host", true), game(7, "host", false)]);
        assert_eq!(runtime.rejoin_host(&mut session, false, None, now), None);
        assert_eq!(runtime.pending_game(), None);
        runtime
            .lobby
            .replace_games(vec![game(8, "Host", true), game(7, "host", true)]);
        runtime.rejoin_host(&mut session, true, None, now);
        assert_eq!(runtime.pending_game(), Some(7));
        assert!(matches!(
            runtime.take_service_requests().as_slice(),
            [LanServiceRequest::Join {
                game_id: 7,
                observe: true
            }]
        ));
        assert_eq!(session.take_network_room_return(), NetworkRoomReturn::Lobby);

        let mut late = LanShellRuntime::new("Guest");
        session.wait_for_lan_room("Host".into(), 7, now);
        let now = now + Duration::from_secs(120);
        late.lobby.replace_games(vec![game(7, "Host", true)]);
        assert_eq!(
            late.rejoin_host(&mut session, false, None, now),
            Some(RejoinEnd::HostDidNotReturn("Host".into()))
        );
        assert_eq!(late.pending_game(), None);
        assert_eq!(session.take_network_room_return(), NetworkRoomReturn::Lobby);

        // Renamed to a sentinel while it waited: told, as Join would be.
        let mut renamed = LanShellRuntime::new("@Open@");
        renamed.take_service_requests();
        session.wait_for_lan_room("Host".into(), 7, now);
        renamed.lobby.replace_games(vec![game(7, "Host", true)]);
        let refused = join_name_error("@Open@", &[]);
        assert_eq!(
            renamed.rejoin_host(&mut session, false, refused, now),
            Some(RejoinEnd::NameRefused("ERROR:NameIsReservedWord"))
        );
        assert_eq!(renamed.pending_game(), None);
        assert!(renamed.take_service_requests().is_empty());
        assert_eq!(session.take_network_room_return(), NetworkRoomReturn::Lobby);
    }

    #[test]
    fn pending_join_disables_admission_until_a_service_result_releases_it() {
        let mut runtime = LanShellRuntime::new("Tester");
        runtime.lobby.replace_games(vec![LanGameRow {
            id: 7,
            name: "Host".into(),
            open: true,
        }]);
        runtime.lobby.select_game(1);
        assert_eq!(
            runtime.lobby.activate(JOIN, false),
            Some(LanLobbyAction::Join {
                game_id: Some(7),
                observe: false
            })
        );
        runtime.pending_join = Some(PendingJoin {
            game_id: 7,
            observe: false,
            expires: Instant::now() + JOIN_TIMEOUT,
            canceling: false,
        });
        assert!(!runtime.lobby.enabled);
        assert_eq!(runtime.lobby.activate(CREATE, false), None);
        assert_eq!(runtime.lobby.select_game(0), None);
        assert_eq!(runtime.pending_game(), Some(7));
        runtime.release_request();
        assert_eq!(runtime.pending_game(), None);
        assert!(runtime.lobby.enabled);
        assert_eq!(
            runtime.lobby.activate(CREATE, false),
            Some(LanLobbyAction::Create { observe: false })
        );
    }

    #[test]
    fn native_rejection_reason_one_uses_observer_request_and_unknown_reason_has_no_modal() {
        assert_eq!(denial_modal(1, false), Some("TXT_GAME_FULL"));
        assert_eq!(denial_modal(1, true), Some("TXT_OBSERVER_FULL"));
        assert_eq!(denial_modal(4, false), None);
        assert_eq!(denial_modal(7, false), Some("TXT_SERIAL_DUP"));
        assert_eq!(denial_modal(9, false), Some("TXT_JOINBAN"));
    }

    /// With the game's string table loaded, VERA's own refusal still shows
    /// its sentence, not the table's MISSING:'…', and the game's labels keep
    /// the table's text.
    #[test]
    fn vera_refusal_shows_its_sentence_beside_the_string_table() {
        let csf = crate::assets::csf_file::test_csf(&[("TXT_REQUEST_DENIED", "Request denied")]);
        assert_eq!(
            crate::i18n::label(Some(&csf), denial_modal(8, false).unwrap(), ""),
            "VERA20k build or enabled gameplay features differ."
        );
        assert_eq!(
            crate::i18n::label(Some(&csf), "TXT_REQUEST_DENIED", ""),
            "Request denied"
        );
    }
}
