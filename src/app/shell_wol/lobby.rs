//! The custom lobby runner `0x00787770` and dialog `0x113`'s app adapter.
//!
//! The runner creates `0x113` hidden behind the wait box, prints the MOTD,
//! requests the chat-lobby list, joins a lobby picked by `0x00788180` and
//! only then shows the dialog. While it shows, its proc `0x0078AC10` drives
//! the lists, chat, sorts, Create (`0x00789B60`) and Join (`0x00789960`
//! with admission `0x0079BA60` and the join `0x00788800`), and the chat
//! sink's OnJoin (`0x007A5E40`) answers joins. Every service exchange is a
//! [`WolServiceRequest`]/[`WolServiceEvent`]; nothing here fabricates a
//! result. No RNG is drawn on these paths.
//!
//! Records the WOL globals hold (`0xB76AE8` games, `0xB73E18` lobbies,
//! `0xB77908` users, `0xB76B44` current and `0xB73BE8` pending channel) live
//! in [`WolLobby`]; the dialog's children in [`WolLobbyState`]. The process
//! globals the runner keeps across visits are [`WolLobbyGlobals`].

use std::time::{Duration, Instant};

use super::WolPage;
use super::service::{self, WolServiceEvent, WolServiceRequest};
use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::{App, AppState};
use crate::ui::lan_shell::{LanChatLine, LanEditKey};
use crate::ui::main_menu_dialogs::options::LauncherCue;
use crate::ui::wol_chat::{self, WolChatFilter, WolChatLine, WolLobbyChatLine};
use crate::ui::wol_lobby::*;
use crate::ui::wol_shell::{BOX_NO, BOX_OK, BOX_YES, WAIT_CHECK_STATUS, WAIT_DISCONNECT};
use crate::ui::wol_shell::{WolMessageBox, WolWaitBox};
use winit::keyboard::KeyCode;

/// The list wait `0x0077F120` gives up after 30 s.
const LIST_WAIT: Duration = Duration::from_millis(30_000);
/// `0x00788800`/`0x00789B60`: at most 150 x 100 ms for the leave.
const LEAVE_WAIT: Duration = Duration::from_millis(15_000);
/// `SetTimer(0, 30000)`: the game list; `SetTimer(1, 53000)`: the lobbies.
const GAMES_PERIOD: Duration = Duration::from_millis(30_000);
const LOBBIES_PERIOD: Duration = Duration::from_millis(53_000);
/// Refresh `0x55D` disables itself for `SetTimer(0x55D, 3000)`.
const REFRESH_HOLD: Duration = Duration::from_millis(3000);
/// `0x00789B60`: `[0xB77DC4] = 7` players.
const CREATE_MAX_USERS: i32 = 7;
/// The create result `0x800401F8` shows `TXT_BAD_CHANKEY`.
const BAD_CHANNEL_KEY: u32 = 0x8004_01F8;

/// Globals the lobby keeps for the process (BSS 0 at start; the WOL reset
/// `0x00780110` leaves them alone).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct WolLobbyGlobals {
    /// `[0xB77DD4]`: the chat lobby number the picker rejoins.
    pub(crate) lobby_number: i32,
    /// `[0xB77E44]` and `[0xB77E48]`.
    pub(crate) game_sort: GameSort,
    pub(crate) player_sort: PlayerSort,
    /// `[0xA8B540]`: Create's Limit Screen Resolution.
    pub(crate) limit_resolution: bool,
}

/// The decoded result of a joined channel: its record and its users.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WolJoined {
    pub(crate) channel: WolChannel,
    pub(crate) members: Vec<WolMember>,
}

#[derive(Debug, Clone)]
enum Step {
    /// The runner's first list (`ListChannels(0, 0)`) behind the wait box.
    ListingLobbies {
        deadline: Instant,
    },
    /// A chat-lobby join; `entry` shows the dialog afterwards.
    JoiningLobby {
        entry: bool,
    },
    Ready,
    /// A leave before a join or create (`WOL:LeavingChannel`).
    Leaving {
        deadline: Instant,
        then: AfterLeave,
    },
    Creating {
        observe: bool,
    },
    JoiningGame,
}

#[derive(Debug, Clone)]
enum AfterLeave {
    Join {
        channel: WolChannel,
        key: String,
        game: bool,
        entry: bool,
    },
    Create {
        channel: WolChannel,
        key: String,
        observe: bool,
    },
}

/// What closing a lobby box continues with.
#[derive(Debug, Clone)]
enum BoxThen {
    /// `0x007A5E40`'s failure tail; `close` when it set event 5.
    JoinFailed { close: bool },
    /// Create's boxes: the lobby back with Join disabled.
    CreateFailed,
    /// `0x00789960`'s `WOL:ResolutionLimited*` Yes/No.
    Resolution { channel: WolChannel },
    /// The runner gives up and returns 0: the welcome.
    Close,
}

/// One runner of the custom lobby.
#[derive(Debug)]
pub(crate) struct WolLobby {
    pub(crate) ui: WolLobbyState,
    games: Vec<WolChannel>,
    lobbies: Vec<WolChannel>,
    members: Vec<WolMember>,
    current: Option<WolChannel>,
    pending: Option<WolChannel>,
    /// `0x113` shows (it is created hidden).
    pub(crate) shown: bool,
    /// `Shell__DrawEmptyBackdrop` painted over the dialog for a popup or a
    /// Create/Join wait.
    pub(crate) covered: bool,
    pub(crate) wait: Option<WolWaitBox>,
    message: Option<(WolMessageBox, BoxThen)>,
    step: Step,
    postmatch: bool,
    games_at: Instant,
    lobbies_at: Instant,
    refresh_at: Option<Instant>,
    /// A game join's result that arrived while the dialog slid out.
    deferred: Option<Result<WolJoined, u32>>,
}

impl WolLobby {
    pub(crate) fn message(&self) -> Option<&WolMessageBox> {
        self.message.as_ref().map(|(message, _)| message)
    }

    /// The dialog takes input: shown, no box, no wait.
    pub(crate) fn interactive(&self) -> bool {
        self.shown && self.message.is_none() && self.wait.is_none()
    }
}

pub(crate) fn layout(state: &AppState) -> WolLobbyLayout {
    let (width, height) = shell_layout::shell_screen_size(state);
    if shell_layout::fills_window(state) {
        WolLobbyLayout::responsive(width as i32, height as i32)
    } else {
        WolLobbyLayout::new(width as i32, height as i32)
    }
}

fn lobby(state: &AppState) -> Option<&WolLobby> {
    state.frontend.wol.as_ref()?.lobby.as_deref()
}

/// This user is in a channel (`[0xB77DD0]`): a leave will see its OnPart.
pub(super) fn in_channel(state: &AppState) -> bool {
    lobby(state).is_some_and(|lobby| lobby.current.is_some())
}

fn lobby_mut(state: &mut AppState) -> Option<&mut WolLobby> {
    state.frontend.wol.as_mut()?.lobby.as_deref_mut()
}

fn request(state: &mut AppState, request: WolServiceRequest) {
    service::request(state, request);
}

fn csf(state: &AppState, key: &str) -> String {
    App::csf_label(state, key, key)
}

fn units(state: &AppState, key: &str) -> Vec<u16> {
    csf(state, key).encode_utf16().collect()
}

/// `0x005E2700` with color -1 into the lobby's transcript.
fn message_line(state: &mut AppState, text: String) {
    crate::app::shell_lan::append_chat(
        state,
        LanChatLine {
            text: text.encode_utf16().collect(),
            color: u32::MAX,
        },
    );
}

fn echo(state: &mut AppState, text: Vec<u16>, color: u32) {
    crate::app::shell_lan::append_chat(state, LanChatLine { text, color });
}

/// The login nickname `[0xB73E40]` (the MultiPlayer handle).
fn nickname(state: &AppState) -> String {
    state.frontend.wol_account.nickname.clone()
}

fn admission_context() -> AdmissionContext {
    AdmissionContext {
        // Custom Match is mode 4, never 1/2/3.
        quick_match: false,
        clan_id: 0,
        local_hash: None,
    }
}

/// `[0xA8EB84]`: Options' ScreenWidth, the running shell width while the
/// profile holds none.
fn options_screen(state: &AppState) -> (i32, i32) {
    let profile = &state.persistence.options_profile;
    if profile.screen_width > 0 && profile.screen_height > 0 {
        (profile.screen_width, profile.screen_height)
    } else {
        let (w, h) = shell_layout::shell_screen_size(state);
        (w as i32, h as i32)
    }
}

fn base_names(state: &AppState) -> [String; 8] {
    std::array::from_fn(|index| csf(state, &format!("TXT_LOB_{}", index + 1)))
}

/// `0x00780110`'s view of the runner's wait box `0x792BE0(0)`; an open one
/// only takes the new text.
fn show_wait(state: &mut AppState, key: &str) {
    let fresh = service::wait_box(state, key);
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    match lobby.wait.as_mut() {
        Some(open) => open.text = fresh.text,
        None => lobby.wait = Some(fresh),
    }
    lobby.ui.cancel_pointer();
    state
        .frontend
        .shell_controller
        .reset_to(crate::ui::wol_shell::WAIT_DIALOG, false);
}

/// `0x00792BE0(0)`: only an open box takes the text.
fn retext_wait(state: &mut AppState, key: &str) {
    let text = csf(state, key);
    if let Some(wait) = lobby_mut(state).and_then(|lobby| lobby.wait.as_mut()) {
        wait.text = text;
    }
}

fn close_wait(state: &mut AppState) {
    if let Some(lobby) = lobby_mut(state) {
        lobby.wait = None;
    }
    state
        .frontend
        .shell_controller
        .reset_to(super::WOL_LOBBY_DIALOG, false);
}

fn show_box(state: &mut AppState, key: &str, yes_no: bool, then: BoxThen) {
    let message = service::message_box(state, key, yes_no);
    let dialog = message.dialog();
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    lobby.message = Some((message, then));
    lobby.ui.cancel_pointer();
    state.frontend.shell_controller.reset_to(dialog, false);
}

/// The runner `0x00787770` entered after the login (`postmatch` false) or
/// after a match (true). A hidden lobby kept while C2/C9 ran is shown again
/// instead: setup Back left its game channel and set `[0xB77DD8]`, so the
/// runner rejoins its chat lobby.
pub(crate) fn open(state: &mut AppState, postmatch: bool) {
    let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
    let motd = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.motd.take());
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    if state.match_state.session.network_return_context().is_none() {
        state.match_state.session.enter_network(
            crate::app::match_runtime::session::SessionMode::Wol,
            crate::app::match_runtime::session::NetworkReturnContext::Wol,
        );
    }
    visit.page = WolPage::Lobby;
    state.frontend.wol_login_art = None;
    state.frontend.shell_route = crate::app::shell_route::ShellRoute::WolLobby;
    state
        .frontend
        .shell_controller
        .reset_to(super::WOL_LOBBY_DIALOG, false);
    let now = Instant::now();
    let resume = !postmatch && visit.lobby.is_some();
    if resume {
        let lobby = visit.lobby.as_deref_mut().expect("checked");
        lobby.current = None;
        lobby.members.clear();
        lobby.shown = true;
        lobby.covered = false;
        lobby.ui.observer = observer;
        lobby.step = Step::Ready;
        refill_players(state);
        switch_lobby(state);
        state.platform.window.request_redraw();
        return;
    }
    visit.lobby = Some(Box::new(WolLobby {
        ui: WolLobbyState::new(observer),
        games: Vec::new(),
        lobbies: Vec::new(),
        members: Vec::new(),
        current: None,
        pending: None,
        shown: false,
        covered: false,
        wait: None,
        message: None,
        step: Step::ListingLobbies {
            deadline: now + LIST_WAIT,
        },
        postmatch,
        games_at: now + GAMES_PERIOD,
        lobbies_at: now + LOBBIES_PERIOD,
        refresh_at: None,
        deferred: None,
    }));
    let _ = App::ensure_skirmish_shell_chrome(state);
    open_art(state);
    // The MOTD [0xB73AE0]: one line per strtok("\n") token, a blank row,
    // and the global is cleared.
    if let Some(motd) = motd.filter(|motd| !motd.is_empty()) {
        for line in crate::rules::ini_value::strtok(&motd, &['\n']) {
            message_line(state, line.to_owned());
        }
        message_line(state, "\n".to_owned());
    }
    show_wait(state, "GUI:LoginComplete");
    show_wait(state, "TXT_REQ_CHANLIST");
    request(state, WolServiceRequest::ListChannels { kind: 0, flags: 0 });
    state.platform.window.request_redraw();
}

/// `0x0072CD60`: MnScrnLCustomMatchLobby.shp through its own palette (800
/// wide only), with the list and button PCX images.
pub(crate) fn open_art(state: &mut AppState) -> bool {
    if state.frontend.wol_lobby_art.is_some() {
        return true;
    }
    let Some(assets) = state.process_assets.manager() else {
        return false;
    };
    state.frontend.wol_lobby_art = crate::render::main_menu_shell_chrome::build_wol_page_art(
        &state.renderer.gpu,
        &state.renderer.batch_renderer,
        assets,
        shell_layout::art_key_screen_width(state),
        ("MnScrnLCustomMatchLobby.shp", "MnScrnLCustomMatchLobby.pal"),
        &PCX_FILES,
    );
    state.frontend.wol_lobby_art.is_some()
}

fn refill_games(state: &mut AppState) {
    let sort = state.frontend.wol_lobby_globals.game_sort;
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let rows = game_rows(&lobby.games, sort, lobby.ui.observer);
    lobby.ui.replace_games(rows);
}

fn refill_lobbies(state: &mut AppState) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let current = lobby.current.as_ref().map(|channel| channel.name.clone());
    lobby
        .ui
        .replace_lobbies(lobby_rows(&lobby.lobbies), current.as_deref());
}

fn refill_players(state: &mut AppState) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    // 0x00783C80 fills the chat lobby's users; in a game channel it shows
    // nothing the custom lobby can see.
    let rows = match &lobby.current {
        Some(channel) if channel.kind == 0 => lobby
            .members
            .iter()
            .map(WolPlayerRow::from_member)
            .collect(),
        _ => Vec::new(),
    };
    lobby.ui.replace_players(rows);
}

/// `0x0079BA60` for the selected game row (`show` prints its refusal).
fn admit_row(state: &mut AppState, row: usize, observer: bool, show: bool) -> bool {
    let Some(lobby) = lobby(state) else {
        return false;
    };
    let Some(channel) = lobby
        .ui
        .game_rows
        .get(row)
        .and_then(|row| lobby.games.get(row.record))
    else {
        return false;
    };
    match admit(channel, observer, admission_context()) {
        Ok(()) => true,
        Err(key) => {
            if let (true, Some(key)) = (show, key) {
                let text = csf(state, key);
                message_line(state, text);
            }
            false
        }
    }
}

fn selected_channel(lobby: &WolLobby) -> Option<WolChannel> {
    lobby
        .ui
        .selected_game()
        .and_then(|row| lobby.games.get(row.record))
        .cloned()
}

/// The runner's lobby switch (`[0xB77DD8]`): controls off, the picker
/// without skipping crowded lobbies, the join; no lobby closes the runner.
fn switch_lobby(state: &mut AppState) {
    if let Some(lobby) = lobby_mut(state) {
        lobby.ui.begin_lobby_switch();
    }
    if !join_picked_lobby(state, false, false) {
        if let Some(lobby) = lobby_mut(state) {
            lobby.ui.set_enabled(BACK, true);
        }
        close_runner(state);
    }
}

/// `0x00788A30`: pick (`0x00788180`) and join with the lobby key. False
/// when no chat lobby exists.
fn join_picked_lobby(state: &mut AppState, skip_full: bool, entry: bool) -> bool {
    let number = state.frontend.wol_lobby_globals.lobby_number;
    let Some(lobby) = lobby(state) else {
        return false;
    };
    let Some((record, number)) = pick_lobby(&lobby.lobbies, number, skip_full) else {
        return false;
    };
    let channel = lobby.lobbies[record].clone();
    state.frontend.wol_lobby_globals.lobby_number = number;
    join_channel(state, channel, LOBBY_KEY, false, entry);
    // 0x00788A30 refills the game list after its join.
    refill_games(state);
    true
}

/// `0x00788800`: leave the current channel first (`WOL:LeavingChannel`
/// through `0x00792BE0(1)`, up to 150 x 100 ms), then copy the key
/// (`+0xD7`), remember the pending channel `[0xB73BE8]` and request the join
/// behind the box retexted `TXT_JOINLOB`. A channel already joined or
/// pending is not joined again (it returns 0 without a request).
fn join_channel(state: &mut AppState, channel: WolChannel, key: &str, game: bool, entry: bool) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let named = |known: &Option<WolChannel>| {
        known
            .as_ref()
            .is_some_and(|known| known.name == channel.name)
    };
    if named(&lobby.current) || named(&lobby.pending) {
        // The game list never holds the current chat lobby and the commands
        // are off while a join is pending: unreachable for a game.
        log::debug!(
            "WOL join of {} skipped: already joined or pending",
            channel.name
        );
        if game {
            join_failed_locally(state, true);
        }
        return;
    }
    if lobby.current.is_some() {
        lobby.step = Step::Leaving {
            deadline: Instant::now() + LEAVE_WAIT,
            then: AfterLeave::Join {
                channel,
                key: key.to_owned(),
                game,
                entry,
            },
        };
        request(state, WolServiceRequest::LeaveChannel);
        show_wait(state, "WOL:LeavingChannel");
        return;
    }
    send_join(state, channel, key.to_owned(), game, entry);
}

fn send_join(state: &mut AppState, channel: WolChannel, key: String, game: bool, entry: bool) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    lobby.pending = Some(channel.clone());
    lobby.step = if game {
        Step::JoiningGame
    } else {
        Step::JoiningLobby { entry }
    };
    show_wait(state, "TXT_JOINLOB");
    request(state, WolServiceRequest::JoinChannel { channel, key });
    if game {
        // 0x00789960 returned true: 0x0078C281 slides the dialog out
        // (0x006071E0), draws the empty backdrop and hides it; C9 comes from
        // the join sink afterwards.
        App::leave_shell_dialog(state, shell_transition::ShellExitThen::WolLobbyHide);
    }
}

/// The leave finished (the sink cleared `[0xB77DD0]`) or its 15 s ran out.
fn leave_finished(state: &mut AppState, left: bool) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    if left {
        lobby.current = None;
        lobby.members.clear();
    }
    let Step::Leaving { then, .. } = std::mem::replace(&mut lobby.step, Step::Ready) else {
        if left {
            refill_players(state);
            refill_lobbies(state);
        }
        return;
    };
    refill_players(state);
    refill_lobbies(state);
    match then {
        AfterLeave::Join {
            channel,
            key,
            game,
            entry,
        } => {
            if left {
                send_join(state, channel, key, game, entry);
            } else {
                // Still in the channel: 0x00788800 returns 1. The box keeps
                // flag 1 ([0xB77DC8] only loses 0x40), so it stays open.
                join_failed_locally(state, game);
            }
        }
        // 0x00789B60 closes the box (0x00792EC0) and creates whether or
        // not the leave finished.
        AfterLeave::Create {
            channel,
            key,
            observe,
        } => {
            close_wait(state);
            send_create(state, channel, key, observe);
        }
    }
}

/// `0x00789960` returned false: every command back, Join as admitted.
fn join_failed_locally(state: &mut AppState, game: bool) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    lobby.covered = false;
    lobby.step = Step::Ready;
    if game {
        lobby.ui.enable_commands(true);
    } else {
        lobby.ui.lobby_switched(false);
    }
}

/// The runner returns 0 (`LAB_00787F98`): the dialog closes without a
/// slide and `WOL_Main` shows the welcome (mode 0).
fn close_runner(state: &mut AppState) {
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.lobby = None;
    }
    state.frontend.wol_lobby_art = None;
    state.match_state.session.leave_network();
    App::reopen_wol_welcome(state);
}

/// Back's teardown has run: event 6 leaves the channel (API+0x24) and the
/// runner returns 0.
pub(crate) fn commit_back(state: &mut AppState) {
    request(state, WolServiceRequest::LeaveChannel);
    close_runner(state);
}

/// The Join slide has run: the empty backdrop shows under the wait, and
/// `0x0078C2A0` enables the command column again. A join result that came
/// during the slide is delivered now (the native slide pumps no messages).
pub(crate) fn commit_hide(state: &mut AppState) {
    let deferred = lobby_mut(state).and_then(|lobby| {
        lobby.shown = false;
        lobby.covered = false;
        lobby.ui.enable_commands(true);
        lobby.deferred.take()
    });
    if let Some(result) = deferred {
        joined(state, result);
    }
}

/// Create's slide has run: `UI__BeginDialog(0xC2)` over a hidden `0x113`.
pub(crate) fn commit_create(state: &mut AppState, observe: bool) {
    commit_hide(state);
    App::open_host_setup(state, observe);
}

/// API+0x1C, then `GUI:CreatingChannel` in a box created with flag 1 whose
/// modal loop waits for the result. The session preparation `0x00789B60`
/// runs before it (GameMode options, the host node, `[0x8316A4]`) belongs
/// to the setup owner that creates C2 ([`App::open_host_setup`]).
fn send_create(state: &mut AppState, channel: WolChannel, key: String, observe: bool) {
    if let Some(lobby) = lobby_mut(state) {
        lobby.step = Step::Creating { observe };
    }
    request(state, WolServiceRequest::CreateChannel { channel, key });
    show_wait(state, "GUI:CreatingChannel");
}

/// `0x00789B60`'s record (`0x10C` bytes): type `0x29`, `"%s's_game"` of the
/// nickname, 7 users, the 0x629 byte (absent, 0) as tournament, the clan
/// and resolution bits plus the chat-lobby number `[0xB77DD4]` as game
/// flags. `width` is Options' ScreenWidth `[0xA8EB84]`. The exInfo
/// (`0x0077E7A0`) needs the INI hashes VERA has no owner of and is left
/// empty; `+0x04 = 1` has no field here.
fn created_channel(
    nickname: &str,
    clan: bool,
    limit: bool,
    width: i32,
    lobby_number: i32,
) -> WolChannel {
    let mut game_flags = 0u32;
    if clan {
        game_flags |= GAME_CLAN;
    }
    if limit {
        game_flags |= if width == 800 {
            GAME_LIMIT_800
        } else {
            GAME_LIMIT_640
        };
    }
    WolChannel {
        kind: GAME_CHANNEL,
        max_users: CREATE_MAX_USERS,
        name: format!("{nickname}'s_game"),
        game_flags: game_flags.wrapping_add(lobby_number as u32),
        ..Default::default()
    }
}

/// `0x00789B60` after `0xC3` OK.
fn create_confirmed(
    state: &mut AppState,
    password: String,
    clan: bool,
    limit: bool,
    observe: bool,
) {
    let nickname = nickname(state);
    let (width, _) = options_screen(state);
    let globals = &mut state.frontend.wol_lobby_globals;
    globals.limit_resolution = limit;
    let channel = created_channel(&nickname, clan, limit, width, globals.lobby_number);
    // The key goes to +0xD7 with spaces made '_'; the popup's observe byte
    // becomes the observer preference [0xAC10C8].
    let key = password.replace(' ', "_");
    state
        .frontend
        .multiplayer_shell_runtime
        .set_lan_observer(observe);
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    lobby.ui.popup = None;
    lobby.ui.observer = observe;
    // API+0x24 runs unconditionally, and once more with the wait (a
    // `0x792BE0(0)` retext: no box is created) while still joined.
    let joined = lobby.current.is_some();
    if joined {
        lobby.step = Step::Leaving {
            deadline: Instant::now() + LEAVE_WAIT,
            then: AfterLeave::Create {
                channel: channel.clone(),
                key: key.clone(),
                observe,
            },
        };
    }
    request(state, WolServiceRequest::LeaveChannel);
    if joined {
        request(state, WolServiceRequest::LeaveChannel);
        retext_wait(state, "WOL:LeavingChannel");
    } else {
        send_create(state, channel, key, observe);
    }
}

/// Create ended without C2: the commands back with Join disabled
/// (`0x0078AC10`'s 0x62B tail), focus on the chat edit.
fn create_ended(state: &mut AppState) {
    if let Some(lobby) = lobby_mut(state) {
        lobby.covered = false;
        lobby.step = Step::Ready;
        lobby.ui.enable_commands(false);
    }
    state
        .frontend
        .shell_controller
        .reset_to(super::WOL_LOBBY_DIALOG, false);
}

/// The Join button (`show` when the row differs from `[0x84A23C]`) and a
/// double-click both reach `0x00789960` after admission.
fn begin_join(state: &mut AppState, row: usize, show: bool) {
    let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
    let admitted = admit_row(state, row, observer, show);
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    lobby.ui.join_row = Some(row);
    lobby.ui.set_enabled(JOIN, admitted);
    if !admitted {
        return;
    }
    let Some(channel) = selected_channel(lobby) else {
        return;
    };
    lobby.ui.disable_commands();
    continue_join(state, channel);
}

/// `0x00789960` past admission: the resolution limit, then the password.
fn continue_join(state: &mut AppState, channel: WolChannel) {
    let limit = channel.screen_limit();
    if limit < options_screen(state).0 {
        let key = if channel.game_flags & GAME_LIMIT_800 != 0 {
            "WOL:ResolutionLimited800"
        } else {
            "WOL:ResolutionLimited640"
        };
        show_box(state, key, true, BoxThen::Resolution { channel });
        return;
    }
    if channel.keyed() {
        // Shell__DrawEmptyBackdrop, then 0xE1.
        if let Some(lobby) = lobby_mut(state) {
            lobby.covered = true;
            lobby.ui.popup = Some(WolLobbyPopup::password());
        }
        state
            .frontend
            .shell_controller
            .reset_to(PASSWORD_DIALOG, false);
        return;
    }
    join_channel(state, channel, "", true, false);
}

fn answer_resolution(state: &mut AppState, channel: WolChannel, yes: bool) {
    let limit = channel.screen_limit();
    if yes {
        // OptionsClass::WriteToINI 0x5FAD10 with the limited size.
        let (width, height) = limited_screen(limit);
        let profile = &mut state.persistence.options_profile;
        profile.screen_width = width;
        profile.screen_height = height;
        crate::app::persistence::options::persist_options_profile(state);
    }
    if limit < options_screen(state).0 {
        join_failed_locally(state, true);
        return;
    }
    if channel.keyed() {
        continue_join(state, channel);
    } else {
        join_channel(state, channel, "", true, false);
    }
}

fn popup_action(state: &mut AppState, action: WolPopupAction) {
    // The popup's destruction repaints the dialog under it.
    if let Some(lobby) = lobby_mut(state) {
        lobby.covered = false;
    }
    match action {
        WolPopupAction::CreateConfirmed {
            password,
            clan,
            limit,
            observe,
        } => create_confirmed(state, password, clan, limit, observe),
        WolPopupAction::CreateCancelled => {
            if let Some(lobby) = lobby_mut(state) {
                lobby.ui.popup = None;
            }
            create_ended(state);
        }
        WolPopupAction::PasswordConfirmed(password) => {
            let channel = lobby_mut(state).and_then(|lobby| {
                lobby.ui.popup = None;
                selected_channel(lobby)
            });
            state
                .frontend
                .shell_controller
                .reset_to(super::WOL_LOBBY_DIALOG, false);
            match channel {
                // Spaces become '_' (0x00789AB6).
                Some(channel) => {
                    join_channel(state, channel, &password.replace(' ', "_"), true, false)
                }
                None => join_failed_locally(state, true),
            }
        }
        WolPopupAction::PasswordCancelled => {
            if let Some(lobby) = lobby_mut(state) {
                lobby.ui.popup = None;
            }
            state
                .frontend
                .shell_controller
                .reset_to(super::WOL_LOBBY_DIALOG, false);
            join_failed_locally(state, true);
        }
    }
}

/// `0x0078AC10`'s 0x540 Enter after `0x005EDD00`.
fn send_chat(state: &mut AppState, text: Vec<u16>) {
    let commands = wol_chat::WolChatCommands {
        page: units(state, "TXT_PAGE_COMMAND"),
        reply: units(state, "TXT_REPLY_COMMAND"),
        reply_short: units(state, "TXT_REPLY_COMMAND_SHORT"),
        online: units(state, "TXT_ONLINE_COMMAND"),
    };
    let filters = wol_chat::WolChatFilterCommands {
        buddy: units(state, "TXT_BUDDY_FILTER"),
        clan: units(state, "TXT_CLAN_FILTER"),
    };
    let last_pager = super::last_pager(state).map(str::to_owned);
    let nickname = nickname(state);
    let line = match wol_chat::parse_lobby(&text, &commands, &filters, last_pager.is_some()) {
        WolLobbyChatLine::Filter(filter) => {
            // 0x78B3F4/0x78B4DF flip the live DisplayBuddies/DisplayClan
            // bytes without saving; the next WOL entry re-reads RA2MD.INI.
            let live = &mut state.frontend.wol_preferences;
            let on = match filter {
                WolChatFilter::Buddy => {
                    live.display_buddies = !live.display_buddies;
                    live.display_buddies
                }
                WolChatFilter::Clan => {
                    live.display_clan = !live.display_clan;
                    live.display_clan
                }
            };
            let notice = units(state, filter.notice(on));
            echo(state, notice, wol_chat::EMOTE_COLOR);
            return;
        }
        WolLobbyChatLine::Line(line) => line,
    };
    let to = lobby(state)
        .map(|lobby| lobby.ui.private_targets())
        .unwrap_or_default();
    let in_channel = lobby(state).is_some_and(|lobby| lobby.current.is_some());
    let echo_line = match &line {
        WolChatLine::Notice(key) => {
            let text = csf(state, key);
            message_line(state, text);
            return;
        }
        WolChatLine::Locate { .. } => None,
        WolChatLine::Page { name, text } => Some((
            wol_chat::page_echo(&nickname, name, text),
            wol_chat::PRIVATE_COLOR,
        )),
        WolChatLine::Reply { text } => {
            let target: Vec<u16> = last_pager.unwrap_or_default().encode_utf16().collect();
            Some((
                wol_chat::page_echo(&nickname, &target, text),
                wol_chat::PRIVATE_COLOR,
            ))
        }
        WolChatLine::Say(text) => {
            if !in_channel {
                let text = csf(state, "TXT_NOT_IN_CHAN");
                message_line(state, text);
                return;
            }
            // 0x0077E1D0 sends privately to the selected users.
            Some((
                wol_chat::say_echo(&nickname, text),
                if to.is_empty() {
                    wol_chat::SAY_COLOR
                } else {
                    wol_chat::PRIVATE_COLOR
                },
            ))
        }
    };
    let to = if matches!(line, WolChatLine::Say(_)) {
        to
    } else {
        Vec::new()
    };
    request(state, WolServiceRequest::Chat { line, to });
    if let Some((text, color)) = echo_line {
        echo(state, text, color);
    }
}

/// `0x3ED`: outside a channel `TXT_NOT_IN_CHAN`, an empty edit
/// `TXT_ENTER_MESSAGE`, else `0x0077E300` (private to the selected users).
fn send_emote(state: &mut AppState, text: Vec<u16>) {
    let Some(lobby) = lobby(state) else {
        return;
    };
    let to = lobby.ui.private_targets();
    if lobby.current.is_none() {
        let text = csf(state, "TXT_NOT_IN_CHAN");
        message_line(state, text);
        return;
    }
    if text.is_empty() {
        let text = csf(state, "TXT_ENTER_MESSAGE");
        message_line(state, text);
        return;
    }
    let nickname = nickname(state);
    let color = if to.is_empty() {
        wol_chat::EMOTE_COLOR
    } else {
        wol_chat::PRIVATE_EMOTE_COLOR
    };
    echo(state, wol_chat::emote_echo(&nickname, &text), color);
    request(state, WolServiceRequest::Emote { text, to });
}

fn dispatch(state: &mut AppState, action: WolLobbyAction) {
    match action {
        WolLobbyAction::Create => {
            let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
            let (width, _) = options_screen(state);
            let globals = &mut state.frontend.wol_lobby_globals;
            if width > 0x3FF {
                globals.limit_resolution = false;
            }
            let limit = globals.limit_resolution;
            let Some(lobby) = lobby_mut(state) else {
                return;
            };
            lobby.ui.disable_commands();
            lobby.ui.game_list.selected = None;
            lobby.covered = true;
            // +0x13 starts as [0xAC10C8] == 1.
            lobby.ui.popup = Some(WolLobbyPopup::create(width, limit, 0, observer));
            state
                .frontend
                .shell_controller
                .reset_to(CREATE_DIALOG, false);
        }
        WolLobbyAction::Join => {
            let Some(lobby) = lobby(state) else {
                return;
            };
            let Some(row) = lobby.ui.game_list.selected else {
                return;
            };
            let show = lobby.ui.join_row != Some(row);
            begin_join(state, row, show);
        }
        WolLobbyAction::Back => {
            App::leave_shell_dialog(state, shell_transition::ShellExitThen::WolLobbyBack);
        }
        WolLobbyAction::Refresh => {
            if let Some(lobby) = lobby_mut(state) {
                lobby.ui.set_enabled(REFRESH, false);
                lobby.refresh_at = Some(Instant::now() + REFRESH_HOLD);
            }
            request(
                state,
                WolServiceRequest::ListChannels {
                    kind: GAME_CHANNEL,
                    flags: 1,
                },
            );
        }
        WolLobbyAction::GameSort(control) => {
            let globals = &mut state.frontend.wol_lobby_globals;
            if let Some(sort) = toggle_game_sort(control, globals.game_sort) {
                globals.game_sort = sort;
                refill_games(state);
            }
        }
        WolLobbyAction::PlayerSort(control) => {
            let globals = &mut state.frontend.wol_lobby_globals;
            if let Some(sort) = toggle_player_sort(control, globals.player_sort) {
                globals.player_sort = sort;
                if let Some(lobby) = lobby_mut(state) {
                    resort_members(&mut lobby.members, sort);
                }
                refill_players(state);
            }
        }
        WolLobbyAction::SelectGame { row, double } => {
            let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
            let Some(lobby) = lobby(state) else {
                return;
            };
            let name = lobby.ui.game_rows.get(row).map(|row| row.name.clone());
            let show = double || name != lobby.ui.admitted_name;
            let admitted = admit_row(state, row, observer, show);
            let Some(lobby) = lobby_mut(state) else {
                return;
            };
            lobby.ui.admitted_name = name;
            if double && admitted {
                begin_join(state, row, false);
            } else {
                lobby.ui.set_enabled(JOIN, admitted);
            }
        }
        WolLobbyAction::SelectLobby(row) => {
            let Some(lobby) = lobby(state) else {
                return;
            };
            let Some(record) = lobby.ui.lobby_rows.get(row) else {
                return;
            };
            let named = |known: &Option<WolChannel>| {
                known
                    .as_ref()
                    .is_some_and(|known| known.name == record.name)
            };
            if named(&lobby.current) || named(&lobby.pending) {
                return;
            }
            state.frontend.wol_lobby_globals.lobby_number = record.number;
            switch_lobby(state);
        }
        WolLobbyAction::Chat(text) => send_chat(state, text),
        WolLobbyAction::Emote(text) => send_emote(state, text),
        WolLobbyAction::ObserverChanged(observer) => state
            .frontend
            .multiplayer_shell_runtime
            .set_lan_observer(observer),
        // 0x0078B829: 0x113 slides out over the empty backdrop before 0x2BC.
        WolLobbyAction::Options => {
            App::leave_shell_dialog(state, shell_transition::ShellExitThen::WolOptionsOpen);
        }
        WolLobbyAction::Unported(control) => {
            // Buddy List (mode 5), Clan/Tournament/News (0x00773E90 web
            // pages) and Page (0xCF) are later chains.
            log::warn!("WOL lobby control {control:#x} is not implemented; 0x113 stays");
        }
        WolLobbyAction::Popup(action) => popup_action(state, action),
    }
    state.platform.window.request_redraw();
}

/// A lobby box or wait-box button released.
pub(super) fn box_command(state: &mut AppState, control: u16) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    if let Some((_, then)) = lobby
        .message
        .take_if(|_| matches!(control, BOX_OK | BOX_YES | BOX_NO))
    {
        state
            .frontend
            .shell_controller
            .reset_to(super::WOL_LOBBY_DIALOG, false);
        match then {
            BoxThen::JoinFailed { close } => join_failed_tail(state, close),
            BoxThen::CreateFailed => create_ended(state),
            BoxThen::Resolution { channel } => {
                answer_resolution(state, channel, control == BOX_YES)
            }
            BoxThen::Close => close_runner(state),
        }
        return;
    }
    match control {
        // Event 0 ends the runner's wait: LAB_00787F98.
        WAIT_DISCONNECT if lobby.wait.as_ref().is_some_and(|wait| wait.shown) => {
            request(state, WolServiceRequest::CancelWait);
            close_runner(state);
        }
        WAIT_CHECK_STATUS => {
            log::warn!("WOL Check Status needs the embedded browser; not opened");
        }
        _ => {}
    }
}

/// `0x007A5E40`'s failure tail: rejoin flag from the pending channel, the
/// pending and current channels cleared, the wait closed, the lists
/// refilled and `0x113` shown; event 5 then ends the runner.
fn join_failed_tail(state: &mut AppState, close: bool) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let rejoin = lobby.pending.take().is_some();
    lobby.current = None;
    lobby.members.clear();
    let entry = matches!(lobby.step, Step::JoiningLobby { entry: true });
    lobby.step = Step::Ready;
    lobby.shown = true;
    lobby.covered = false;
    close_wait(state);
    refill_games(state);
    refill_players(state);
    if close {
        close_runner(state);
    } else if entry {
        // The entry wait takes this event as success; [0xB77DD8] is reset.
        entry_shown(state);
    } else if rejoin {
        switch_lobby(state);
    }
}

/// The runner after its first join: `[0xB77DD8] = 0`, message `0x593`
/// (both lists requested), the wait closed and `0x113` shown.
fn entry_shown(state: &mut AppState) {
    if let Some(lobby) = lobby_mut(state) {
        lobby.shown = true;
        lobby.step = Step::Ready;
        let now = Instant::now();
        lobby.games_at = now + GAMES_PERIOD;
        lobby.lobbies_at = now + LOBBIES_PERIOD;
    }
    close_wait(state);
    request(state, WolServiceRequest::ListChannels { kind: 0, flags: 0 });
    request(
        state,
        WolServiceRequest::ListChannels {
            kind: GAME_CHANNEL,
            flags: 1,
        },
    );
}

/// `0x007A5E40` (CChatEventSink vt+0x28) for this user, or a result.
fn joined(state: &mut AppState, result: Result<WolJoined, u32>) {
    let Some(runner) = lobby_mut(state) else {
        return;
    };
    if runner.pending.is_none() {
        log::debug!("stale WOL join result");
        return;
    }
    if matches!(runner.step, Step::JoiningGame) && runner.shown {
        runner.deferred = Some(result);
        return;
    }
    let joined = match result {
        Ok(joined) => joined,
        Err(code) => {
            join_error(state, code);
            return;
        }
    };
    let sort = state.frontend.wol_lobby_globals.player_sort;
    let runner = lobby_mut(state).expect("checked");
    runner.pending = None;
    let entry = matches!(runner.step, Step::JoiningLobby { entry: true });
    runner.step = Step::Ready;
    runner.members.clear();
    for member in joined.members {
        insert_member(&mut runner.members, member, sort);
    }
    runner.current = Some(joined.channel.clone());
    if joined.channel.kind == GAME_CHANNEL {
        // The guest dialog C9 over a hidden 0x113; the observer preference
        // [0xAC10C8] makes the local node an observer.
        close_wait(state);
        let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
        let mut local = state.frontend.multiplayer_shell_runtime.local_participant();
        if observer {
            local.role = crate::skirmish_launch::LaunchHumanRole::Observer;
        }
        // [0x8316A4] = 7 until the host's "O<n>" arrives.
        App::open_wol_joined_setup(state, local, Some(7));
        return;
    }
    // A chat lobby: its number, the TXT_JOINED_S line, the controls back
    // with Join as the selected game admits for either role.
    let number = joined.channel.lobby_number();
    if let Some(number) = number {
        state.frontend.wol_lobby_globals.lobby_number = number;
    }
    let name = match number {
        Some(number) => WolLobbyRow::joined_name(number, &base_names(state)),
        None => joined.channel.name.clone(),
    };
    let text = crate::assets::csf_file::format_csf(
        &csf(state, "TXT_JOINED_S"),
        &[crate::assets::csf_file::CsfArg::Str(&name)],
    );
    message_line(state, text);
    let selected = lobby(state).and_then(|lobby| lobby.ui.game_list.selected);
    let join = selected.is_some_and(|row| {
        admit_row(state, row, false, false) || admit_row(state, row, true, false)
    });
    if let Some(lobby) = lobby_mut(state) {
        lobby.ui.lobby_switched(join);
    }
    refill_lobbies(state);
    refill_players(state);
    if entry {
        entry_shown(state);
    } else {
        close_wait(state);
    }
}

/// `0x007A5E40`'s HRESULT branches in Custom Match.
/// How `0x007A5E40` answers a failed join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JoinErrorReply {
    /// `0x8004006C`: nothing happens.
    Ignore,
    /// A box, then the failure tail; `close` sets event 5 (the runner
    /// returns 0). `forget` drops the pending channel's records first;
    /// `next_lobby` increments `[0xB77DD4]`.
    Box {
        key: &'static str,
        close: bool,
        forget: bool,
        next_lobby: bool,
    },
    /// `TXT_CANT_JOINCHAN` (`%X` of the code) through `0x005E2700`, then
    /// the tail with event 5.
    Chat,
}

fn join_error_reply(code: u32, pending: bool) -> JoinErrorReply {
    let reply = |key, close, forget, next_lobby| JoinErrorReply::Box {
        key,
        close,
        forget,
        next_lobby,
    };
    match code {
        0x8004_006C => JoinErrorReply::Ignore,
        0x8004_0070 => reply("TXT_GAME_CLOSED", false, true, false),
        0x8004_0071 => reply("TXT_BADPASS", false, false, false),
        0x8004_006E => reply("TXT_CHANNEL_FULL", !pending, false, false),
        0x8004_0072 => reply("TXT_JOINBAN", true, false, true),
        _ => JoinErrorReply::Chat,
    }
}

fn join_error(state: &mut AppState, code: u32) {
    let pending = lobby(state).is_some_and(|lobby| lobby.pending.is_some());
    match join_error_reply(code, pending) {
        JoinErrorReply::Ignore => {}
        JoinErrorReply::Box {
            key,
            close,
            forget,
            next_lobby,
        } => {
            if let Some(lobby) = lobby_mut(state)
                && forget
                && let Some(name) = lobby.pending.as_ref().map(|channel| channel.name.clone())
            {
                lobby.lobbies.retain(|channel| channel.name != name);
                lobby.games.retain(|channel| channel.name != name);
            }
            if next_lobby {
                let globals = &mut state.frontend.wol_lobby_globals;
                globals.lobby_number = globals.lobby_number.wrapping_add(1);
            }
            show_box(state, key, false, BoxThen::JoinFailed { close });
        }
        JoinErrorReply::Chat => {
            let text = csf(state, "TXT_CANT_JOINCHAN").replacen("%X", &format!("{code:X}"), 1);
            message_line(state, text);
            join_failed_tail(state, true);
        }
    }
}

fn created(state: &mut AppState, result: Result<Vec<WolMember>, u32>) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let Step::Creating { observe } = lobby.step else {
        log::debug!("stale WOL create result");
        return;
    };
    match result {
        Err(code) => {
            close_wait(state);
            let key = if code == BAD_CHANNEL_KEY {
                "TXT_BAD_CHANKEY"
            } else {
                "TXT_CANTCREATE_CHAN"
            };
            show_box(state, key, false, BoxThen::CreateFailed);
        }
        Ok(members) => {
            close_wait(state);
            let nickname = nickname(state);
            let found = members.iter().any(|member| {
                crate::util::native_string::crt_strcmpi(&member.name, &nickname).is_eq()
            });
            if !found {
                // Not among the users: 0x00789B60 returns 0 silently.
                create_ended(state);
                return;
            }
            let lobby = lobby_mut(state).expect("checked");
            lobby.members = members;
            lobby.step = Step::Ready;
            App::leave_shell_dialog(
                state,
                shell_transition::ShellExitThen::WolCreateAccepted { observe },
            );
        }
    }
}

/// Lobby events of the service. Events nothing waits for are stale.
pub(super) fn service_event(state: &mut AppState, event: WolServiceEvent) {
    if lobby(state).is_none() {
        log::debug!("WOL lobby event without a lobby: {event:?}");
        return;
    }
    match event {
        WolServiceEvent::Channels { kind, channels } => {
            let lobby = lobby_mut(state).expect("checked");
            if kind == GAME_CHANNEL {
                lobby.games = channels;
                refill_games(state);
                return;
            }
            lobby.lobbies = channels;
            refill_lobbies(state);
            let lobby = lobby_mut(state).expect("checked");
            if let Step::ListingLobbies { .. } = lobby.step {
                show_wait(state, "TXT_JOINLOB");
                let skip_full = state
                    .frontend
                    .wol
                    .as_ref()
                    .is_some_and(|visit| visit.lobby_joined_before)
                    && !lobby_postmatch(state);
                let joined = join_picked_lobby(state, skip_full, true);
                if let Some(visit) = state.frontend.wol.as_mut() {
                    visit.lobby_joined_before = true;
                }
                if !joined {
                    close_runner(state);
                }
            }
        }
        WolServiceEvent::Joined(result) => joined(state, result),
        WolServiceEvent::UserJoined(member) => {
            let sort = state.frontend.wol_lobby_globals.player_sort;
            let lobby = lobby_mut(state).expect("checked");
            if lobby.current.is_some() {
                insert_member(&mut lobby.members, member, sort);
                refill_players(state);
            }
        }
        WolServiceEvent::UserLeft(name) => {
            let lobby = lobby_mut(state).expect("checked");
            lobby.members.retain(|member| member.name != name);
            refill_players(state);
        }
        WolServiceEvent::ChannelLeft => leave_finished(state, true),
        WolServiceEvent::Created(result) => created(state, result),
        other => log::debug!("WOL event not for the lobby: {other:?}"),
    }
    state.platform.window.request_redraw();
}

fn lobby_postmatch(state: &AppState) -> bool {
    lobby(state).is_some_and(|lobby| lobby.postmatch)
}

/// The page sink (CChatEventSink+0x70, `0x007A9220`): the sender becomes
/// `[0xB77E30]`; a dialog with a transcript shows `(%hs) %s` in white and
/// `[AudioVisual] IncomingMessage` plays. Elsewhere the page dialog `0x11E`
/// would open, a later chain; no auto-reply text exists without wolinfo.
pub(super) fn page_received(state: &mut AppState, from: String, text: Vec<u16>) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.last_pager = Some(from.clone());
    let route = state.frontend.shell_route;
    if route.wol_lobby() || route.network_setup() {
        let mut line: Vec<u16> = format!("({from}) ").encode_utf16().collect();
        line.extend_from_slice(&text);
        echo(state, line, 0xFF_FFFF);
    } else {
        log::warn!("WOL page outside a transcript needs dialog 0x11E; not shown");
    }
    let sound = state
        .rules()
        .and_then(|rules| rules.general.incoming_message_sound())
        .map(str::to_owned);
    App::play_shell_ui_sound_by_id(state, sound.as_deref());
}

pub(crate) fn update_geometry(state: &mut AppState) {
    let layout = layout(state);
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
        (&mut ui.chat.list, layout.chat_log, ui.chat.lines.len()),
    ] {
        list.reconcile_geometry(rect, count, scale);
    }
    ui.chat
        .edit
        .scroll_to_caret((scale.base_rect(layout.chat).w - 3).max(0) as u32, |text| {
            font.text_width(text)
        });
    if let Some(popup) = ui.popup.as_mut() {
        let (width, height) = (
            state.renderer.gpu.config.width as i32,
            state.renderer.gpu.config.height as i32,
        );
        let rect = match popup.kind {
            WolPopupKind::Create(_) => {
                crate::ui::shell::modal::wol_create_game_layout(width, height).password
            }
            WolPopupKind::Password(_) => {
                crate::ui::shell::modal::wol_channel_password_layout(width, height).password
            }
        };
        let available = (rect.w - 3).max(0) as u32;
        match &mut popup.kind {
            WolPopupKind::Create(create) => create
                .password
                .scroll_to_caret(available, |text| font.text_width(text)),
            WolPopupKind::Password(edit) => {
                edit.scroll_to_caret(available, |text| font.text_width(text))
            }
        }
    }
}

pub(super) fn key_press(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
    if shell_transition::blocks_shell_input(state)
        || !lobby(state).is_some_and(WolLobby::interactive)
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
    let (width, height) = (
        state.renderer.gpu.config.width as i32,
        state.renderer.gpu.config.height as i32,
    );
    let controls = match popup.kind {
        WolPopupKind::Create(_) => {
            let layout = crate::ui::shell::modal::wol_create_game_layout(width, height);
            vec![
                (POPUP_OK, layout.ok),
                (POPUP_CANCEL, layout.cancel),
                (CREATE_CLAN, layout.clan),
                (CREATE_LIMIT, layout.limit),
                (CREATE_OBSERVE, layout.observe),
                (POPUP_PASSWORD, layout.password),
            ]
        }
        WolPopupKind::Password(_) => {
            let layout = crate::ui::shell::modal::wol_channel_password_layout(width, height);
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

pub(super) fn cursor_moved(state: &mut AppState) {
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let popup_over = lobby(state)
        .and_then(|lobby| lobby.ui.popup.as_ref())
        .map(|popup| popup_control(state, popup));
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    if let (Some(over), Some(popup)) = (popup_over, lobby.ui.popup.as_mut()) {
        popup.buttons.hovered = over;
        state.platform.window.request_redraw();
        return;
    }
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

pub(super) fn mouse(state: &mut AppState, pressed: bool) {
    if shell_transition::blocks_shell_input(state)
        || !lobby(state).is_some_and(WolLobby::interactive)
    {
        return;
    }
    update_geometry(state);
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let scale = layout.page.scale;
    if let Some(popup) = lobby(state).and_then(|lobby| lobby.ui.popup.clone()) {
        let over = popup_control(state, &popup);
        let (width, height) = (
            state.renderer.gpu.config.width as i32,
            state.renderer.gpu.config.height as i32,
        );
        let lobby = lobby_mut(state).expect("checked");
        let popup = lobby.ui.popup.as_mut().expect("checked");
        if !pressed {
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
        let press = match over {
            Some(control @ (CREATE_CLAN | CREATE_LIMIT | CREATE_OBSERVE)) => {
                let rect = crate::ui::shell::modal::wol_create_game_layout(width, height);
                let window = match control {
                    CREATE_CLAN => rect.clan,
                    CREATE_LIMIT => rect.limit,
                    _ => rect.observe,
                };
                if crate::ui::skirmish_shell::checkbox_icon_rect(
                    window,
                    crate::ui::shell::geom::ShellScale::RETAIL,
                )
                .contains(x, y)
                    && popup.toggle(control)
                {
                    Press::Cue(LauncherCue::Checkbox)
                } else {
                    Press::Nothing
                }
            }
            Some(control @ (POPUP_OK | POPUP_CANCEL)) if popup.enabled(control) => {
                popup.buttons.press(Some(control));
                Press::Button
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
                            ui.toggle_player(row);
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

/// The runner's timeouts, the wait box reveal, the dialog's timers (only
/// while it is the current window) and the shared scrollbar repeats.
pub(super) fn poll(state: &mut AppState) -> Option<Instant> {
    let now = Instant::now();
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let mut redraw = false;
    let lobby = lobby_mut(state)?;
    redraw |= lobby.wait.as_mut().is_some_and(|wait| wait.poll(now));
    match lobby.step {
        Step::ListingLobbies { deadline } if now >= deadline => {
            // 0x0077F120 gave up: the runner's box, then the welcome.
            lobby.step = Step::Ready;
            close_wait(state);
            show_box(state, "TXT_CANT_CONNECT", false, BoxThen::Close);
            state.platform.window.request_redraw();
            return None;
        }
        Step::Leaving { deadline, .. } if now >= deadline => {
            leave_finished(state, false);
            state.platform.window.request_redraw();
            return None;
        }
        _ => {}
    }
    let mut deadlines: Vec<Instant> = Vec::new();
    deadlines.extend(
        lobby
            .wait
            .as_ref()
            .filter(|wait| !wait.shown)
            .map(WolWaitBox::reveal_at),
    );
    if let Step::ListingLobbies { deadline } | Step::Leaving { deadline, .. } = lobby.step {
        deadlines.push(deadline);
    }
    if let Some(at) = lobby.refresh_at {
        if now >= at {
            lobby.refresh_at = None;
            lobby.ui.set_enabled(REFRESH, true);
            redraw = true;
        } else {
            deadlines.push(at);
        }
    }
    // The timers run only while 0x113 is the current window.
    let current_window = lobby.interactive() && lobby.ui.popup.is_none();
    let mut requests = Vec::new();
    if current_window {
        if now >= lobby.games_at {
            lobby.games_at = now + GAMES_PERIOD;
            requests.push(WolServiceRequest::ListChannels {
                kind: GAME_CHANNEL,
                flags: 1,
            });
        }
        if now >= lobby.lobbies_at {
            lobby.lobbies_at = now + LOBBIES_PERIOD;
            requests.push(WolServiceRequest::ListChannels { kind: 0, flags: 0 });
        }
        deadlines.push(lobby.games_at);
        deadlines.push(lobby.lobbies_at);
        let ui = &mut lobby.ui;
        let scale = layout.page.scale;
        for (list, rect, count) in [
            (&mut ui.lobby_list, layout.lobbies, ui.lobby_rows.len()),
            (&mut ui.game_list, layout.games, ui.game_rows.len()),
            (&mut ui.player_list, layout.players, ui.player_rows.len()),
            (&mut ui.chat.list, layout.chat_log, ui.chat.lines.len()),
        ] {
            let geometry = list.geometry(rect, count, scale);
            redraw |= list.scroll.poll(geometry, &mut list.top_index, x, y, now);
            deadlines.extend(list.scroll.repeat_at());
        }
    }
    for request in requests {
        self::request(state, request);
    }
    if redraw {
        state.platform.window.request_redraw();
    }
    deadlines.into_iter().min()
}

/// The status help of what the pointer is over: the table's key, or a
/// list's `0x4E9` answer (`0x00780E30` for a game, `0x00781090` for a user).
pub(crate) fn status_help(state: &AppState) -> String {
    let Some(lobby) = lobby(state) else {
        return String::new();
    };
    if let Some(popup) = lobby.ui.popup.as_ref() {
        return match popup.kind {
            WolPopupKind::Create(_) => popup.buttons.hovered.and_then(create_tooltip_key),
            WolPopupKind::Password(_) => None,
        }
        .map(|key| csf(state, key))
        .unwrap_or_default();
    }
    let ui = &lobby.ui;
    match ui.buttons.hovered {
        Some(GAMES) => match ui.game_list.hot_row.and_then(|row| ui.game_rows.get(row)) {
            None => csf(state, "STT:LobbyListGames"),
            Some(row) => game_help(state, row),
        },
        Some(PLAYERS) => match ui
            .player_list
            .hot_row
            .and_then(|row| ui.player_rows.get(row))
        {
            None => csf(state, "STT:LobbyListUsers"),
            Some(row) => user_help(state, &row.name),
        },
        Some(control) => tooltip_key(control)
            .map(|key| csf(state, key))
            .unwrap_or_default(),
        None => String::new(),
    }
}

/// `0x00780A90` without ladder records: `"nick : Unranked"`.
fn user_help(state: &AppState, name: &str) -> String {
    format!("{name} : {}", csf(state, "TXT_UNRANKED"))
}

/// `0x00780E30`: the host (the name up to `'`, at most 10 characters), the
/// ping and the map; the map name lookup `0x00780D90` is not ported.
fn game_help(state: &AppState, row: &WolGameRow) -> String {
    let host: String = row
        .name
        .split('\'')
        .next()
        .unwrap_or_default()
        .chars()
        .take(10)
        .collect();
    let ping = if row.ping == -1 {
        csf(state, "TXT_UNKNOWN_PING")
    } else {
        crate::assets::csf_file::format_csf(
            &csf(state, "MSG:PingInfo"),
            &[crate::assets::csf_file::CsfArg::Int(i64::from(row.ping))],
        )
    };
    format!("{}, {ping}", user_help(state, &host))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_errors_follow_the_on_join_sink() {
        assert_eq!(join_error_reply(0x8004_006C, true), JoinErrorReply::Ignore);
        assert_eq!(
            join_error_reply(0x8004_0070, true),
            JoinErrorReply::Box {
                key: "TXT_GAME_CLOSED",
                close: false,
                forget: true,
                next_lobby: false
            }
        );
        assert_eq!(
            join_error_reply(0x8004_0071, true),
            JoinErrorReply::Box {
                key: "TXT_BADPASS",
                close: false,
                forget: false,
                next_lobby: false
            }
        );
        // A full channel ends the runner only when nothing was pending.
        for (pending, close) in [(true, false), (false, true)] {
            assert_eq!(
                join_error_reply(0x8004_006E, pending),
                JoinErrorReply::Box {
                    key: "TXT_CHANNEL_FULL",
                    close,
                    forget: false,
                    next_lobby: false
                }
            );
        }
        assert_eq!(
            join_error_reply(0x8004_0072, true),
            JoinErrorReply::Box {
                key: "TXT_JOINBAN",
                close: true,
                forget: false,
                next_lobby: true
            }
        );
        assert_eq!(join_error_reply(0x8000_4005, true), JoinErrorReply::Chat);
        assert_eq!(join_error_reply(0x8004_006D, false), JoinErrorReply::Chat);
    }

    #[test]
    fn the_created_record_carries_name_users_and_flags() {
        let channel = created_channel("Tester", false, false, 1024, 3);
        assert_eq!(channel.name, "Tester's_game");
        assert_eq!((channel.kind, channel.max_users), (GAME_CHANNEL, 7));
        assert_eq!((channel.game_flags, channel.flags), (3, 0));
        assert!(channel.ex_info.is_empty());
        let limited = created_channel("Tester", true, true, 800, 0);
        assert_eq!(limited.game_flags, GAME_CLAN | GAME_LIMIT_800);
        let limited = created_channel("Tester", false, true, 640, 1);
        assert_eq!(limited.game_flags, GAME_LIMIT_640 + 1);
        // A limited game's screen admission matches its bit.
        assert_eq!(
            created_channel("a", false, true, 800, 0).screen_limit(),
            800
        );
        assert_eq!(
            created_channel("a", false, true, 640, 0).screen_limit(),
            640
        );
    }
}
