//! The Quick Match and Buddy List runners' entries into the shared lobby
//! records: a new runner, its joins, creates, boxes and leaves, the buddy
//! dialog's open and resume.

use super::created_channel;
use crate::app::shell_wol::WolPage;
use crate::app::shell_wol::service::WolServiceRequest;
use crate::app::{App, AppState};
use crate::ui::wol_lobby::*;
use crate::util::time::Instant;

use super::{
    AfterLeave, BoxThen, GAMES_PERIOD, LEAVE_WAIT, LIST_WAIT, LOBBIES_PERIOD, QuickMatchBoxThen,
    Runner, Step, WolLobby, admission_context, close_runner, continue_join, current_channel,
    ex_info, join_channel, join_failed_locally, leave_then_create, lobby_mut, nickname,
    options_screen, request, show_box, show_wait,
};

pub(super) fn new_runner(
    runner: Runner,
    observer: bool,
    postmatch: bool,
    now: Instant,
    modern: bool,
) -> Box<WolLobby> {
    Box::new(WolLobby {
        runner,
        ui: WolLobbyState::new(observer, modern),
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
        joining: None,
        matches: Vec::new(),
        watch_list_stale: false,
        visit: None,
    })
}

/// A runner with no `0x113` and the chat-lobby list behind
/// `TXT_REQ_CHANLIST` (`0x0077EF90(0, 0)`, the 30 s wait `0x0077F120`): the
/// Buddy List runner's entry (`0x0079F140`, steps 1-2) and the Quick Match
/// runner's lobby list (`0x0079FCF0..0x0079FD35`), whose entry cleared the
/// observer preference (`0x0079FC40..0x0079FC5D`).
pub(in crate::app::shell_wol) fn open_runner(state: &mut AppState, runner: Runner) {
    let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.in_game = false;
    visit.page = WolPage::Lobby;
    visit.lobby = Some(new_runner(
        runner,
        observer,
        false,
        Instant::now(),
        !visit.legacy_ui,
    ));
    if state.match_state.session.network_return_context().is_none() {
        state
            .match_state
            .session
            .enter_network(crate::app::match_runtime::session::NetworkReturnContext::Wol);
    }
    super::leaf::free_art(state, crate::ui::wol_login::DIALOG);
    state.frontend.shell_route = crate::app::shell_route::ShellRoute::Wol;
    let _ = App::ensure_skirmish_shell_chrome(state);
    show_wait(state, "TXT_REQ_CHANLIST");
    request(state, WolServiceRequest::ListChannels { kind: 0, flags: 0 });
}

/// `0x00788800` for the Quick Match lobby (stage 2): a lobby already
/// joined or pending answers 0 without a request.
pub(in crate::app::shell_wol) fn quick_match_join(
    state: &mut AppState,
    channel: WolChannel,
    key: &str,
) {
    join_channel(state, channel, key, false, false);
}

/// `0x00789960(record, DL=0)` for the host's game the guest found: the
/// admission `0x0079BA60` with Quick Match's ranked test, then the join.
/// RESIDUAL: `DL=0`'s internals are not traced; the Quick Match game has
/// no key and no resolution bits, so neither popup can follow.
pub(in crate::app::shell_wol) fn quick_match_join_game(state: &mut AppState, channel: WolChannel) {
    let context = admission_context(state);
    if let Err(reason) = admit(&channel, false, context) {
        // VERA: the log keeps what was compared; gamemd records nothing.
        log::warn!(
            "WOL Quick Match: {} refused the join ({}); local rules hash {:?}, host exInfo {:?}",
            channel.name,
            reason.unwrap_or("no message"),
            context.local_hash,
            channel.ex_info,
        );
        super::quick_match::game_refused(state, reason);
        return;
    }
    join_channel(state, channel, "", true, false);
}

/// `0x00789B60`'s Quick Match branch (`0x00789BA8..0x00789D60`,
/// `0x00789EC4..0x00789EEE`): no `0xC3` and no key; the leave and the
/// create then run as for Create.
pub(in crate::app::shell_wol) fn quick_match_create(state: &mut AppState) {
    let nickname = nickname(state);
    let tournament = i32::from(!super::quick_match::quick_coop(state));
    let (width, _) = options_screen(state);
    let ex_info = ex_info(state);
    let globals = &mut state.frontend.wol_lobby_globals;
    if width >= 1024 {
        globals.limit_resolution = false;
    }
    let channel = quick_match_channel(&nickname, tournament, globals.lobby_number, ex_info);
    leave_then_create(state, channel, String::new(), false, true);
}

/// `0x00789B60`'s record in Quick Match: Create's record and exInfo
/// (`0x0077E7A0` at `0x00789D05` runs before the mode branch) with two users
/// (`[0xB77DC4] = 2`, `0x00789BE0`), tournament 1 (Quick Co-op 0,
/// `0x00789EC9`) and game flags `0x800` plus `[0xB77DD4]`: no clan or
/// resolution bits.
pub(super) fn quick_match_channel(
    nickname: &str,
    tournament: i32,
    lobby_number: i32,
    ex_info: String,
) -> WolChannel {
    WolChannel {
        max_users: 2,
        tournament,
        game_flags: GAME_QUICK_MATCH.wrapping_add(lobby_number as u32),
        ..created_channel(nickname, false, false, 0, lobby_number, ex_info)
    }
}

/// A box of the Quick Match runner over `0x117` or the backdrop.
pub(in crate::app::shell_wol) fn quick_match_box(
    state: &mut AppState,
    key: &str,
    then: QuickMatchBoxThen,
) {
    show_box(state, key, false, BoxThen::QuickMatch(then));
}

/// The runner's exit leave (`0x007A03A0..0x007A0410`): a channel this user
/// is in is left behind `WOL:LeavingChannel` (at most 150 x 100 ms);
/// then the runner returns 0.
pub(in crate::app::shell_wol) fn quick_match_leave_and_return(state: &mut AppState) {
    let Some(lobby) = lobby_mut(state) else {
        close_runner(state);
        return;
    };
    lobby.pending = None;
    if lobby.current.is_none() {
        close_runner(state);
        return;
    }
    lobby.step = Step::Leaving {
        deadline: Instant::now() + LEAVE_WAIT,
        then: AfterLeave::Return,
    };
    request(state, WolServiceRequest::LeaveChannel);
    show_wait(state, "WOL:LeavingChannel");
}

/// The Buddy List runner's dialog loop takes over: its records stay, the
/// runner's own boxes are closed and the backdrop shows under `0x116`.
pub(in crate::app::shell_wol) fn buddy_dialog_opens(state: &mut AppState) {
    if let Some(lobby) = lobby_mut(state) {
        lobby.wait = None;
        lobby.message = None;
        lobby.step = Step::Ready;
        lobby.covered = false;
        lobby.ui.close_popup();
        lobby.joining = None;
    }
}

/// The Buddy List runner's dialog loop after a match or a stopped
/// Create/Join (C2/C9 Back left the game channel).
pub(in crate::app::shell_wol) fn buddy_runner_resumes(state: &mut AppState) {
    if let Some(lobby) = lobby_mut(state) {
        // A game channel C2 Back is still leaving stays until its OnPart.
        if lobby
            .current
            .as_ref()
            .is_none_or(|current| current.kind != GAME_CHANNEL)
        {
            lobby.current = None;
            lobby.members.clear();
        }
        lobby.pending = None;
        lobby.shown = false;
    }
}

/// C2 Back's own leave (API+0x24 at `0x007900B8`), after the dialog closed.
pub(crate) fn host_setup_left(state: &mut AppState) {
    if current_channel(state).is_some_and(|current| current.kind == GAME_CHANNEL) {
        request(state, WolServiceRequest::LeaveChannel);
    }
}

/// Join from `0x116` for a buddy in a listed game: `0x00789960` with the
/// record, over the empty backdrop. Its own admission `0x0079BA60` runs
/// first: `0x116`'s observer box does not recompute Join's enable, so the
/// game may refuse here; then `0x00789960` returns false and the loop goes
/// on. RESIDUAL: its refusal line (`show` = 1) goes to a list that is not
/// traced while `0x116` is down; none is printed.
pub(in crate::app::shell_wol) fn buddy_join(state: &mut AppState, channel: WolChannel) {
    if let Some(lobby) = lobby_mut(state) {
        lobby.step = Step::Ready;
        lobby.shown = false;
    }
    let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
    if admit(&channel, observer, admission_context(state)).is_err() {
        join_failed_locally(state, true);
        return;
    }
    continue_join(state, channel);
}

/// Join from `0x116` for a game the list lacks: type `0x29` again behind
/// `TXT_REQ_CHANLIST` (`0x0077EF90(0x29, 0)`, `0x0077F120`), then a
/// second search.
pub(in crate::app::shell_wol) fn buddy_find_game(state: &mut AppState, name: String) {
    if let Some(lobby) = lobby_mut(state) {
        lobby.step = Step::FindingGame {
            name,
            deadline: Instant::now() + LIST_WAIT,
        };
    }
    show_wait(state, "TXT_REQ_CHANLIST");
    request(
        state,
        WolServiceRequest::ListChannels {
            kind: GAME_CHANNEL,
            flags: 0,
        },
    );
}
