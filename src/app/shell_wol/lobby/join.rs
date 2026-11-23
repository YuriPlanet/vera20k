//! Join results: the chat sink's OnJoin (`0x007A5890`) with its failure
//! tail and error replies, the runner's first join, a guest entering a game,
//! the join and watch error codes, and the create result.

use crate::app::frontend::shell_transition;
use crate::app::shell_network_setup as setup;
use crate::app::shell_wol::service::WolServiceRequest;
use crate::app::{App, AppState};
use crate::ui::wol_lobby::*;
use crate::util::time::Instant;

use super::{
    BAD_CHANNEL_KEY, BoxThen, GAMES_PERIOD, LOBBIES_PERIOD, QuickMatchBoxThen, Runner, Step,
    WolJoined, base_names, buddy_runner, close_runner, close_wait, commit_create, create_ended,
    joined_game_line, lobby, lobby_mut, message_line, modern_lobby, nickname, open_game_room,
    quick_match_runner, refill_games, refill_lobbies, refill_players, replace_box_body, request,
    request_listed, selected_admits, setup_channel_left, show_box, sort_lobby, switch_lobby,
    switching_room, take_visit,
};

/// `0x007A5890`'s failure tail: rejoin flag from the pending channel, the
/// pending and current channels cleared, the wait closed, the lists
/// refilled and `0x113` shown; event 5 then ends the runner.
pub(super) fn join_failed_tail(state: &mut AppState, close: bool) {
    if quick_match_runner(state) {
        // Event 5 acts only at stage 0, so `close` is the stage timeout's
        // business; a failed game join returns 0 from 0x00789960.
        let game = lobby(state).is_some_and(|lobby| matches!(lobby.step, Step::JoiningGame { .. }));
        if let Some(lobby) = lobby_mut(state) {
            lobby.pending = None;
            lobby.current = None;
            lobby.members.clear();
            lobby.step = Step::Ready;
        }
        close_wait(state);
        if game {
            super::quick_match::game_joined(state, false);
        }
        return;
    }
    if buddy_runner(state) {
        // The Join wait takes event 5 or 0xA as a stop: back to 0x116.
        if let Some(lobby) = lobby_mut(state) {
            lobby.pending = None;
            lobby.current = None;
            lobby.members.clear();
        }
        super::buddy::stopped(state);
        return;
    }
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let rejoin = lobby.pending.take().is_some();
    if matches!(lobby.step, Step::SwitchingRoom { .. }) {
        // The server refused the destination before changing membership.
        lobby.step = Step::Ready;
        lobby.shown = true;
        lobby.covered = false;
        close_wait(state);
        let join = selected_admits(state);
        if let Some(lobby) = lobby_mut(state) {
            lobby.ui.set_commands(true, join, false);
        }
        return;
    }
    lobby.current = None;
    lobby.members.clear();
    let entry = matches!(lobby.step, Step::JoiningLobby { entry: true, .. });
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
    request_games(state);
}

/// `0x0077EF90(0x29, 1)`: the game list with automatic pings. VERA's modern
/// lobby asks for the matches to watch with it.
pub(super) fn request_games(state: &mut AppState) {
    request(
        state,
        WolServiceRequest::ListChannels {
            kind: GAME_CHANNEL,
            flags: 1,
        },
    );
    if modern_lobby(state) {
        request(state, WolServiceRequest::ListMatches);
    }
}

/// `0x007A5890` (CChatEventSink vt+0x28) for this user, or a result.
pub(super) fn joined(state: &mut AppState, result: Result<WolJoined, u32>) {
    let online = state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.provider == super::WolProvider::Online);
    let Some(runner) = lobby_mut(state) else {
        return;
    };
    if runner.pending.is_none()
        || (online
            && (!matches!(
                runner.step,
                Step::JoiningGame { .. } | Step::JoiningLobby { .. } | Step::SwitchingRoom { .. }
            ) || runner.step.online_request().is_none()))
    {
        log::debug!("stale WOL join result");
        return;
    }
    if matches!(runner.step, Step::SwitchingRoom { .. })
        && let Ok(joined) = &result
        && runner
            .pending
            .as_ref()
            .is_none_or(|target| target.name != joined.channel.name)
    {
        return;
    }
    if matches!(runner.step, Step::JoiningGame { .. }) && runner.shown {
        runner.deferred = Some(result);
        return;
    }
    if let Some(request) = runner.step.online_request_mut() {
        *request = None;
    }
    let joined = match result {
        Ok(joined) => joined,
        Err(code) => {
            join_error(state, code);
            return;
        }
    };
    if switching_room(state) {
        take_visit(state);
        lobby_mut(state).expect("checked").shown = false;
    }
    let runner = lobby_mut(state).expect("checked");
    runner.pending = None;
    let entry = matches!(runner.step, Step::JoiningLobby { entry: true, .. });
    runner.step = Step::Ready;
    let listed = joined.members.clone();
    let channel = joined.channel.clone();
    sort_lobby(state, |lobby, sort, rank| {
        lobby.users_listed(channel, joined.members, sort, rank);
    });
    request_listed(state, &listed);
    if joined.channel.kind == GAME_CHANNEL {
        close_wait(state);
        // As a guest, unless the lobby made this player the room's host
        // while the join slid in ([`host_changed`]).
        let observe = state.frontend.multiplayer_shell_runtime.lan_observer();
        open_game_room(state, &listed, observe, None);
        return;
    }
    // A chat lobby: its number (0x7A5EFD, any type-0 name), the
    // TXT_JOINED_S line (not in the modern lobby, whose operator writes
    // its sign-in text), the controls back with Join as the selected
    // game admits for either role.
    state.frontend.wol_lobby_globals.lobby_number = joined.channel.joined_lobby_number();
    if quick_match_runner(state) {
        // 0x7A5E86..0x7A5EC8: no TXT_JOINED_S in modes 1-3; the wait closes
        // and the lobby's user list sends the request (0x7A7B0D..0x7A7B3B).
        close_wait(state);
        super::quick_match::lobby_users_listed(state);
        return;
    }
    if !modern_lobby(state) {
        let name = match joined.channel.lobby_number() {
            Some(number) => WolLobbyRow::joined_name(number, &base_names(state)),
            None => joined.channel.name.clone(),
        };
        let text = crate::assets::csf_file::format_csf(
            &state.csf_text("TXT_JOINED_S"),
            &[crate::assets::csf_file::CsfArg::Str(&name)],
        );
        message_line(state, text);
    }
    let join = selected_admits(state);
    if let Some(lobby) = lobby_mut(state) {
        lobby.ui.set_commands(true, join, true);
    }
    refill_lobbies(state);
    refill_players(state);
    if entry {
        entry_shown(state);
    } else {
        close_wait(state);
    }
}

/// A joined game channel: the guest dialog C9 over a hidden 0x113; the
/// observer preference [0xAC10C8] makes the local node an observer.
pub(super) fn enter_game_as_guest(state: &mut AppState) {
    let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
    let mut local = state.frontend.multiplayer_shell_runtime.local_participant();
    if observer {
        local.role = crate::skirmish_launch::LaunchHumanRole::Observer;
    }
    // [0x8316A4] = 7 until the host's "O<n>" arrives.
    App::open_wol_joined_setup(state, local, Some(7));
    // The user-list sink 0x7A7110: the others as nodes, then the
    // persona's Country and Color as the MultiPlayer picks
    // (0x69B760/0x69B7E0) and the first R.
    let me = nickname(state);
    let users: Vec<(String, bool)> = lobby(state)
        .map(|lobby| {
            lobby
                .members
                .iter()
                .filter(|member| !member.name.eq_ignore_ascii_case(&me))
                .map(|member| (member.name.clone(), member.flags & MEMBER_OPERATOR != 0))
                .collect()
        })
        .unwrap_or_default();
    let modern = modern_lobby(state) && !quick_match_runner(state);
    if setup::guest_users_listed(state, &users) {
        super::persona::take_side_and_color(state, &me);
        setup::send_first_choices(state);
        if quick_match_runner(state) {
            super::quick_match::game_joined(state, true);
        }
        let room = lobby(state)
            .and_then(|lobby| lobby.current.as_ref())
            .map(|channel| channel.name.clone())
            .unwrap_or_default();
        if modern && let Some(line) = joined_game_line(&users, &room) {
            message_line(state, line);
        }
    } else {
        // 0x7A782A..0x7A788D: no operator, the local tail 0x785740.
        setup_channel_left(state);
    }
}

/// How `0x007A5890` answers a failed join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum JoinErrorReply {
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

/// VERA's lobby refused a join because the host kicked this player within
/// five minutes: 0x80040072 with the HRESULT customer bit, so it shows the
/// join ban's text without the channel ban's exit from the lobby.
pub(crate) const KEPT_OUT_JOIN_ERROR: u32 = 0xA004_0072;

/// VERA's lobby refused a Create because another open room has that name.
pub(crate) const ROOM_NAME_TAKEN: u32 = 0xA004_0073;
const ROOM_NAME_TAKEN_TEXT: &str = "VERA:RoomNameTaken";

/// VERA's lobby refused a `watch` (docs/lobby-protocol.md "Watching"): 409
/// `unknown_match` (the match ended, or this player may not watch it),
/// `match_active` (this player plays one) or `watchers_full`.
pub(crate) const WATCH_UNKNOWN_MATCH: u32 = 0xA004_0074;
pub(crate) const WATCH_MATCH_ACTIVE: u32 = 0xA004_0075;
pub(crate) const WATCHERS_FULL: u32 = 0xA004_0076;
/// The modern list's tag of a match to watch.
pub(super) const WATCH_LIST_TAG: &str = "VERA:WatchListTag";

/// `0x007A5890`'s HRESULT branches in Custom Match.
pub(super) fn join_error_reply(code: u32, pending: bool) -> JoinErrorReply {
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
        KEPT_OUT_JOIN_ERROR => reply("TXT_JOINBAN", false, false, false),
        _ => JoinErrorReply::Chat,
    }
}

fn join_error(state: &mut AppState, code: u32) {
    let pending = lobby(state).is_some_and(|lobby| lobby.pending.is_some());
    let reply = join_error_reply(code, pending);
    if quick_match_runner(state)
        && let JoinErrorReply::Box {
            key: key @ ("TXT_GAME_CLOSED" | "TXT_BADPASS" | "TXT_CHANNEL_FULL"),
            ..
        } = reply
    {
        // 0x7A5961..0x7A5D2A: modes 1-3 print these as lines (colour -1)
        // instead of the boxes; every failure sets event 5.
        let text = state.csf_text(key);
        message_line(state, text);
        join_failed_tail(state, true);
        return;
    }
    match reply {
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
            let text = state
                .csf_text("TXT_CANT_JOINCHAN")
                .replacen("%X", &format!("{code:X}"), 1);
            message_line(state, text);
            join_failed_tail(state, true);
        }
    }
}

pub(super) fn created(state: &mut AppState, result: Result<Vec<WolMember>, u32>) {
    let online = state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.provider == super::WolProvider::Online);
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let Step::Creating {
        observe, request, ..
    } = &lobby.step
    else {
        log::debug!("stale WOL create result");
        return;
    };
    let observe = *observe;
    if online && request.is_none() {
        return;
    }
    let quick_match = lobby.runner == Runner::QuickMatch;
    match result {
        Err(code) => {
            // The rejected create no longer owns a live admission. Keep the
            // normal error box/return tail, but ignore a late success callback.
            lobby.step = Step::Ready;
            close_wait(state);
            let key = if code == BAD_CHANNEL_KEY {
                "TXT_BAD_CHANKEY"
            } else {
                "TXT_CANTCREATE_CHAN"
            };
            let then = if quick_match {
                BoxThen::QuickMatch(QuickMatchBoxThen::CreateFailed)
            } else {
                BoxThen::CreateFailed
            };
            show_box(state, key, false, then);
            if code == ROOM_NAME_TAKEN {
                replace_box_body(state, &crate::i18n::tr(ROOM_NAME_TAKEN_TEXT));
            }
        }
        Ok(members) => {
            close_wait(state);
            let nickname = nickname(state);
            let found = members.iter().any(|member| {
                crate::util::native_string::crt_strcmpi(&member.name, &nickname).is_eq()
            });
            if !found {
                // Not among the users: 0x00789B60 returns 0 silently.
                if quick_match {
                    if let Some(lobby) = lobby_mut(state) {
                        lobby.step = Step::Ready;
                    }
                    super::quick_match::created(state, false);
                } else {
                    create_ended(state);
                }
                return;
            }
            let lobby = lobby_mut(state).expect("checked");
            // 0x7A57C7..0x7A57EB: the created channel is the current one
            // ([0xB76B10]) and this user its host ([0xB77E4C]).
            let listed = members.clone();
            // The create's step ends here and hands over its channel.
            let Step::Creating { channel, .. } = std::mem::replace(&mut lobby.step, Step::Ready)
            else {
                unreachable!("closing the wait box and reading the nickname keep the create step")
            };
            lobby.current = Some(channel);
            lobby.members = members;
            let buddy = lobby.runner == Runner::Buddy;
            request_listed(state, &listed);
            if quick_match {
                // 0x0078A36C..0x0078A386: modes 1-3 skip C2; the game
                // channel's nodes run without a dialog.
                let opened = App::open_host_setup(state, false);
                super::quick_match::created(state, opened);
            } else if buddy {
                // 0x116 has ended: C2 opens over the empty backdrop.
                commit_create(state, observe);
            } else {
                App::leave_shell_dialog(
                    state,
                    shell_transition::ShellSlideKind::WolLobby,
                    move |state| commit_create(state, observe),
                );
            }
        }
    }
}
