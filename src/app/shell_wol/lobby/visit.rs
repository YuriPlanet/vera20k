//! The room's Lobby visit (docs/lobby-protocol.md "Visiting the lobby"): a
//! player in a game room also joins the public lobby to look round it, and
//! Back to Room returns them. The visited lobby's joins, leaves and lines
//! settle here.

use crate::app::AppState;
use crate::app::shell_wol::WolPage;
use crate::app::shell_wol::service::WolServiceRequest;
use crate::ui::wol_lobby::*;

use super::{
    LobbyVisit, Step, WolJoined, admit_row, current_channel, in_setup_channel, leave_finished,
    lobby, lobby_mut, message_line, quick_match_runner, refill_players, request, request_games,
    request_listed, sort_lobby, switching_room,
};

/// VERA: the room's Lobby (docs/lobby-protocol.md "Visiting the lobby"):
/// this player also joins the public lobby to look round it. Their room
/// goes on hidden, its events still reaching it, until Back to Room.
pub(crate) fn visit_lobby(state: &mut AppState) {
    if quick_match_runner(state) || !in_setup_channel(state) {
        return;
    }
    let Some(runner) = lobby_mut(state) else {
        return;
    };
    if !matches!(runner.step, Step::Ready) || runner.visit.is_some() {
        return;
    }
    runner.step = Step::Visiting { request: None };
    request(state, WolServiceRequest::VisitLobby);
}

/// Whether this player looks round the public lobby while their game room
/// goes on hidden ([`visit_lobby`]).
pub(crate) fn visiting(state: &AppState) -> bool {
    lobby(state).is_some_and(|lobby| lobby.visit.is_some())
}

/// VERA online: where the lobby can deliver a page to `name`: the name as
/// the public lobby this player visits lists it (case-insensitive, `true`),
/// else as the current channel does. A roommate in both visits too and sees
/// the lobby page. `None` when neither lists it, and in a match.
pub(in crate::app::shell_wol) fn page_target<'a>(
    visit: &'a super::WolVisit,
    name: &str,
) -> Option<(&'a str, bool)> {
    let lobby = visit.lobby().filter(|_| !visit.in_game)?;
    let find = |members: &'a [WolMember]| {
        members
            .iter()
            .find(|member| member.name.eq_ignore_ascii_case(name))
            .map(|member| member.name.as_str())
    };
    let visited = lobby.visit.as_ref().and_then(|other| find(&other.members));
    visited.map(|name| (name, true)).or_else(|| {
        lobby.current.as_ref()?;
        find(&lobby.members).map(|name| (name, false))
    })
}

/// The lobby let this player into the public lobby as a visitor: its page
/// shows over the hidden room, with Back to Room. A repeat answer lists the
/// lobby again; one this game no longer waits for ends the visit.
pub(super) fn visit_joined(state: &mut AppState, joined: WolJoined) {
    let Some(runner) = lobby_mut(state) else {
        return;
    };
    let first = runner.visit.is_none();
    if first && !matches!(runner.step, Step::Visiting { .. }) {
        request(state, WolServiceRequest::EndVisit);
        return;
    }
    if first {
        runner.step = Step::Ready;
        runner.pending = None;
    }
    let listed = joined.members.clone();
    sort_lobby(state, |lobby, sort, rank| {
        let mut members = Vec::new();
        for member in joined.members {
            insert_member(&mut members, member, sort, rank);
        }
        lobby.visit = Some(LobbyVisit {
            channel: joined.channel,
            members,
        });
        lobby.ui.set_visiting(true);
    });
    request_listed(state, &listed);
    show_visit(state);
    if first {
        let room = current_channel(state)
            .map(|room| room.name)
            .unwrap_or_default();
        message_line(
            state,
            crate::i18n::tr_fill("VERA:VisitingLobby", &[("room", &room)]),
        );
    }
}

/// The lobby page over the visitor's hidden room.
fn show_visit(state: &mut AppState) {
    crate::app::shell_network_setup::hide_room(state);
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.page = WolPage::Lobby;
    }
    state
        .frontend
        .shell_controller
        .reset_to(super::WOL_LOBBY_DIALOG);
    let join = selected_admits(state);
    if let Some(lobby) = lobby_mut(state) {
        lobby.shown = true;
        lobby.covered = false;
        lobby.ui.set_commands(true, join, false);
    }
    refill_players(state);
    request_games(state);
}

/// Whether the selected game row admits Join, as a player or a spectator.
pub(super) fn selected_admits(state: &mut AppState) -> bool {
    lobby(state)
        .and_then(|lobby| lobby.ui.selected_row(GAMES))
        .is_some_and(|row| {
            admit_row(state, row, false, false) || admit_row(state, row, true, false)
        })
}

/// Back to Room: the visitor leaves the public lobby, and their room, which
/// went on meanwhile, shows as it is now.
pub(super) fn back_to_room(state: &mut AppState) {
    if visit_ends(state) {
        request(state, WolServiceRequest::EndVisit);
    }
}

/// Ends a visit here, for Back to Room or the room's start (the lobby ends
/// it for a start itself): the room shows again. False without a visit.
pub(in crate::app::shell_wol) fn visit_ends(state: &mut AppState) -> bool {
    let Some(runner) = lobby_mut(state) else {
        return false;
    };
    if runner.visit.take().is_none() {
        return false;
    }
    runner.ui.set_visiting(false);
    if matches!(runner.step, Step::SwitchingRoom { .. }) {
        // The original room started before the destination admitted us.
        runner.step = Step::Ready;
        runner.pending = None;
        runner.wait = None;
        runner.message = None;
    }
    runner.shown = false;
    runner.covered = false;
    refill_players(state);
    crate::app::shell_network_setup::show_room(state);
    true
}

/// A refused or lost visit: the player stays in their room.
pub(super) fn visit_failed(state: &mut AppState) {
    let Some(runner) = lobby_mut(state) else {
        return;
    };
    runner.step = Step::Ready;
    message_line(state, crate::i18n::tr("VERA:VisitFailed"));
}

/// Ends the visit with its room, as leaving it or the runner's close does:
/// the hidden room is dropped through its owner. The lobby visited, if
/// there was a visit.
pub(super) fn take_visit(state: &mut AppState) -> Option<LobbyVisit> {
    let runner = lobby_mut(state)?;
    let visit = runner.visit.take()?;
    runner.ui.set_visiting(false);
    if state.frontend.network_setup_protocol.is_some() {
        crate::app::shell_network_setup::abandon_for_wol_runner(state);
    }
    Some(visit)
}

/// The visitor's room ended or the service committed their switch to
/// another one. Keep the pending destination until its joined event.
pub(super) fn visit_room_left(state: &mut AppState) {
    if lobby(state).is_some_and(|lobby| matches!(lobby.step, Step::Leaving { .. })) {
        take_visit(state);
        leave_finished(state, true);
        return;
    }
    let room = current_channel(state)
        .map(|room| room.name)
        .unwrap_or_default();
    // A committed switch and an independent room closure both emit
    // channel_left. Until the destination answers, only the departure is known.
    let key = if switching_room(state) {
        "VERA:NoLongerInRoom"
    } else {
        "VERA:VisitedRoomClosed"
    };
    visit_settles(state, crate::i18n::tr_fill(key, &[("room", &room)]));
}

/// The visitor's room is gone for them, kicked or closed: the lobby made
/// the public lobby they visit their channel, and its page stays, saying
/// why with `line`.
pub(super) fn visit_settles(state: &mut AppState, line: String) {
    let Some(visit) = take_visit(state) else {
        return;
    };
    state.frontend.wol_lobby_globals.lobby_number = visit.channel.joined_lobby_number();
    let join = selected_admits(state);
    if let Some(lobby) = lobby_mut(state) {
        lobby.current = Some(visit.channel);
        lobby.members = visit.members;
        lobby.ui.set_commands(true, join, false);
    }
    refill_players(state);
    message_line(state, line);
}

/// Another user joined the lobby this player visits: its list takes them,
/// with their ladder record and high score, as the modern lobby's users.
pub(super) fn visit_user_joined(state: &mut AppState, member: WolMember) {
    if !visiting(state) {
        return;
    }
    let key = member.name.to_ascii_lowercase();
    request(state, WolServiceRequest::LadderRecords(vec![key.clone()]));
    request(state, WolServiceRequest::HighScores(vec![key]));
    sort_lobby(state, |lobby, sort, rank| {
        if let Some(visit) = lobby.visit.as_mut() {
            insert_member(&mut visit.members, member, sort, rank);
        }
    });
    refill_players(state);
}

/// Another user left the lobby this player visits.
pub(super) fn visit_user_left(state: &mut AppState, name: String) {
    if let Some(visit) = lobby_mut(state).and_then(|lobby| lobby.visit.as_mut()) {
        visit.members.retain(|member| member.name != name);
    }
    refill_players(state);
}
