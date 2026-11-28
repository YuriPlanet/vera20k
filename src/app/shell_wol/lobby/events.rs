//! Lobby events of the service: the [`WolServiceEvent`] dispatch, chat
//! lines and pages received, locate and page results, net status, kicks and
//! channel bans.

use crate::app::shell_network_setup as setup;
use crate::app::shell_route::ShellRoute;
use crate::app::shell_wol::service::{self, WolServiceEvent, WolServiceRequest};
use crate::app::{App, AppState};
use crate::ui::lan_shell::LanChatLine;
use crate::ui::wol_chat;
use crate::ui::wol_lobby::*;

use super::{
    BoxThen, Runner, Step, awaiting_room, base_names, close_runner, close_wait, created,
    host_changed, join_picked_lobby, joined, leave_finished, lobby, lobby_dialog, lobby_mut,
    message_line, modern_lobby, nickname, put_box, refill_games, refill_lobbies, refill_players,
    request, request_squad, room_not_returned, room_reopened, setup_channel_left, show_box,
    show_wait, sort_lobby, units, visit_joined, visit_room_left, visit_settles, visit_user_joined,
    visit_user_left, visiting, watch_refused,
};
use crate::app::PlatformState;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::shell_wol::WolVisit;
use crate::audio::sfx::SfxPlayer;
use crate::util::config::GameConfig;

/// Lobby events of the service. Events nothing waits for are stale.
pub(in crate::app::shell_wol) fn service_event(state: &mut AppState, event: WolServiceEvent) {
    if lobby(state.frontend.wol.as_ref()).is_none() {
        log::debug!("WOL lobby event without a lobby: {event:?}");
        return;
    }
    match event {
        WolServiceEvent::Channels { kind, channels } => {
            let lobby = lobby_mut(state.frontend.wol.as_mut()).expect("checked");
            if kind == GAME_CHANNEL && lobby.runner == Runner::QuickMatch {
                lobby.games = channels.clone();
                let step = super::quick_match::games_listed(state, &channels);
                if let Some(step) = step {
                    App::apply_step(state, step);
                }
                return;
            }
            if kind == GAME_CHANNEL {
                lobby.games = channels;
                refill_games(&mut state.frontend, &state.process_assets);
                // The channel-list sink rebuilds an open 0x116's rows.
                super::buddy::buddies_changed(
                    &mut state.frontend,
                    &state.platform.window,
                    &state.process_assets,
                    &state.renderer,
                    false,
                );
                let lobby = lobby_mut(state.frontend.wol.as_mut()).expect("checked");
                if let Step::FindingGame { name, .. } = &lobby.step {
                    let name = name.clone();
                    lobby.step = Step::Ready;
                    close_wait(&mut state.frontend);
                    let step = super::buddy::game_listed(
                        &mut state.frontend,
                        &state.platform.window,
                        &mut state.match_state,
                        &state.process_assets,
                        &state.renderer,
                        &state.persistence.options_profile,
                        &name,
                        true,
                    );
                    if let Some(step) = step {
                        App::apply_step(state, step);
                    }
                }
                return;
            }
            lobby.set_lobbies(channels);
            refill_lobbies(state.frontend.wol.as_mut());
            let lobby = lobby_mut(state.frontend.wol.as_mut()).expect("checked");
            if let (Step::ListingLobbies { .. }, Runner::QuickMatch) = (&lobby.step, lobby.runner) {
                lobby.step = Step::Ready;
                close_wait(&mut state.frontend);
                super::quick_match::lobbies_listed(state);
            } else if let (Step::ListingLobbies { .. }, Runner::Buddy) = (&lobby.step, lobby.runner)
            {
                lobby.step = Step::Ready;
                close_wait(&mut state.frontend);
                super::buddy::lobbies_listed(
                    &mut state.frontend,
                    &state.platform.window,
                    &state.process_assets,
                    &state.renderer,
                    &state.persistence.options_profile,
                );
            } else if let Step::ListingLobbies { .. } = lobby.step {
                show_wait(&mut state.frontend, &state.process_assets, "TXT_JOINLOB");
                let skip_full = state
                    .frontend
                    .wol
                    .as_ref()
                    .is_some_and(|visit| visit.lobby_joined_before)
                    && !lobby_postmatch(state.frontend.wol.as_ref());
                let joined = join_picked_lobby(state, skip_full, true);
                if let Some(visit) = state.frontend.wol.as_mut() {
                    visit.lobby_joined_before = true;
                }
                if !joined {
                    close_runner(state);
                }
            }
        }
        WolServiceEvent::VisitJoined(joined) => visit_joined(
            &mut state.frontend,
            &state.match_state,
            &state.process_assets,
            &state.renderer,
            joined,
        ),
        WolServiceEvent::VisitUserJoined(member) => visit_user_joined(&mut state.frontend, member),
        WolServiceEvent::VisitUserLeft(name) => visit_user_left(&mut state.frontend, name),
        WolServiceEvent::Joined(result) => joined(state, result),
        WolServiceEvent::UserJoined(member) => {
            let lobby = lobby_mut(state.frontend.wol.as_mut()).expect("checked");
            if lobby.current.is_some() {
                let name = member.name.clone();
                // 0x7A62B7..0x7A62EE: a joiner in a squad whose tag is not
                // known yet.
                if member.squad != 0 && member.clan_tag.is_empty() {
                    request_squad(state.frontend.wol.as_mut(), member.squad);
                }
                // 0x7A6542..0x7A662D: the lowered name's ladder record and
                // high score, each unless its table holds it, before the
                // insert. VERA: the modern lobby asks for both always, as
                // for its listed users (request_listed).
                // RESIDUAL: native kicks a game-channel joiner while a start
                // is pending ([0xB77DEC]/[0xB77E28]/[0xB77E27]) before these
                // requests and the insert; VERA kicks in setup::user_joined.
                let key = name.to_ascii_lowercase();
                let modern = modern_lobby(state.frontend.wol.as_ref());
                let globals = &state.frontend.wol_lobby_globals;
                let ladder = modern || !globals.ladder.contains_key(&key);
                let high_score = modern || !globals.high_scores.contains_key(&key);
                if ladder {
                    request(
                        state.frontend.wol.as_mut(),
                        WolServiceRequest::LadderRecords(vec![key.clone()]),
                    );
                }
                if high_score {
                    request(
                        state.frontend.wol.as_mut(),
                        WolServiceRequest::HighScores(vec![key]),
                    );
                }
                sort_lobby(&mut state.frontend, |lobby, sort, rank| {
                    lobby.user_joined(member, sort, rank)
                });
                refill_players(&mut state.frontend);
                if in_setup_channel(&state.frontend)
                    && let Err(error) = setup::user_joined(state, name)
                {
                    log::warn!("WOL game-channel join: {error:#}");
                }
            }
        }
        WolServiceEvent::UserLeft(name) => {
            let lobby = lobby_mut(state.frontend.wol.as_mut()).expect("checked");
            let operator = lobby
                .members
                .iter()
                .any(|member| member.name == name && member.flags & MEMBER_OPERATOR != 0);
            lobby.members.retain(|member| member.name != name);
            refill_players(&mut state.frontend);
            if in_setup_channel(&state.frontend)
                && let Err(error) = setup::user_left(state, &name, operator)
            {
                log::warn!("WOL game-channel leave: {error:#}");
            }
        }
        WolServiceEvent::RoomReopened(joined) => room_reopened(state, joined),
        WolServiceEvent::HostChanged { name, channel } => host_changed(state, name, channel),
        WolServiceEvent::ChannelLeft if visiting(state.frontend.wol.as_ref()) => {
            visit_room_left(state)
        }
        WolServiceEvent::ChannelLeft if awaiting_room(&state.match_state.session) => {
            room_not_returned(
                &mut state.frontend,
                &mut state.match_state,
                &state.process_assets,
                &state.renderer,
                true,
            )
        }
        WolServiceEvent::ChannelLeft if in_setup_channel(&state.frontend) => {
            setup_channel_left(state)
        }
        WolServiceEvent::ChannelLeft => leave_finished(state, true),
        WolServiceEvent::PlayerChoices {
            from,
            slot,
            observer,
        } => {
            // 0x7A82F8..0x7A833C: only from a user of this channel.
            let member = lobby_mut(state.frontend.wol.as_mut())
                .expect("checked")
                .members
                .iter()
                .any(|member| member.name.eq_ignore_ascii_case(&from));
            if member
                && in_setup_channel(&state.frontend)
                && let Err(error) = setup::player_choices(state, &from, slot, observer)
            {
                log::warn!("WOL choices from {from}: {error:#}");
            }
        }
        WolServiceEvent::PlayerReady { from, value } => {
            if in_setup_channel(&state.frontend)
                && let Err(error) = setup::player_ready(state, &from, value)
            {
                log::warn!("WOL readiness from {from}: {error:#}");
            }
        }
        WolServiceEvent::MapStatus { from, status } => {
            if in_setup_channel(&state.frontend)
                && let Err(error) = setup::map_status(state, &from, status)
            {
                log::warn!("WOL map status from {from}: {error:#}");
            }
        }
        WolServiceEvent::Created(result) => created(state, result),
        WolServiceEvent::Matches(matches) => {
            lobby_mut(state.frontend.wol.as_mut())
                .expect("checked")
                .matches = matches;
            refill_games(&mut state.frontend, &state.process_assets);
        }
        WolServiceEvent::WatchRefused(code) => {
            watch_refused(&mut state.frontend, &state.process_assets, code)
        }
        other => log::debug!("WOL event not for the lobby: {other:?}"),
    }
}

/// The runner is in a game channel with C2/C9 open over the hidden `0x113`,
/// or with Quick Match's setup under `0x117`.
pub(super) fn in_setup_channel(frontend: &FrontendState) -> bool {
    // A visitor's room goes on while the lobby page shows.
    (frontend.shell_route == ShellRoute::NetworkSetup
        || setup::quick_match(frontend.network_setup_protocol.as_ref())
        || visiting(frontend.wol.as_ref()))
        && lobby(frontend.wol.as_ref())
            .and_then(|lobby| lobby.current.as_ref())
            .is_some_and(|current| current.kind == GAME_CHANNEL)
}

fn lobby_postmatch(wol: Option<&WolVisit>) -> bool {
    lobby(wol).is_some_and(|lobby| lobby.postmatch)
}

fn sender(member: &WolMember) -> wol_chat::WolSender<'_> {
    wol_chat::WolSender {
        operator: member.flags & MEMBER_OPERATOR != 0,
        name: &member.name,
        clan: &member.clan_name,
    }
}

/// The two gates every chat, action and page sink runs: the language gate
/// (`0x0077D2E0`, inlined) and `0x0077D4C0`. The runner holds `0x113` in
/// the shell stack, also while C2/C9 run over it, until it hands over to a
/// match (it closes every dialog first); C2/C9 run when the setup route is
/// up. The local record is found by the login nickname
/// (`[0xB73E40]`) among the channel's users.
/// `room`: the line is the game room's, shown or a visitor's hidden one.
fn sender_passes(frontend: &FrontendState, from: &WolMember, text: &[u16], room: bool) -> bool {
    let visit = frontend.wol.as_ref();
    let filters = wol_chat::WolDisplayFilters::new(
        &frontend.wol_preferences,
        visit.is_some_and(|visit| visit.modern_ui()),
    );
    let sender = sender(from);
    let setup_open = room;
    let local = nickname(&frontend.wol_account);
    let in_game = visit.is_none_or(|visit| visit.in_game);
    let lobby = lobby_dialog(frontend.wol.as_ref()).filter(|_| !in_game);
    let local_clan = lobby
        .and_then(|lobby| {
            lobby
                .members
                .iter()
                .find(|member| member.name.eq_ignore_ascii_case(&local))
        })
        .map(|member| member.clan_name.as_str());
    wol_chat::language_gate(&sender, setup_open, text, filters)
        && wol_chat::buddy_clan_gate(
            &sender,
            local_clan,
            frontend.wol_buddies.names(),
            filters,
            lobby.is_some(),
        )
}

/// One chat or action line of [`WolServiceEvent::Message`].
pub(in crate::app::shell_wol) struct ReceivedMessage {
    pub(in crate::app::shell_wol) from: WolMember,
    pub(in crate::app::shell_wol) text: Vec<u16>,
    /// VERA: the lobby's own line, `text` from its phrases in the interface
    /// language.
    pub(in crate::app::shell_wol) localized: bool,
    /// VERA: the lobby's warning that a player's game differs from the
    /// host's, shown red (`wol_chat::WARNING_COLOR`).
    pub(in crate::app::shell_wol) warning: bool,
    pub(in crate::app::shell_wol) private: bool,
    pub(in crate::app::shell_wol) action: bool,
    pub(in crate::app::shell_wol) ansi: bool,
    pub(in crate::app::shell_wol) options: Option<Box<setup::HostOptions>>,
    pub(in crate::app::shell_wol) sent_at: Option<u64>,
    /// VERA: said in the public lobby this player visits from their room.
    pub(in crate::app::shell_wol) visit: bool,
}

/// The chat sinks (`0x007A8DA0`, `0x007A8FC0`, `0x007AB170`, `0x007AAFB0`
/// and their UTF-16 forms): nothing while a match runs (`[0xB77DC1]`);
/// then the gates; a line under this user's own name (`strcmpi` with
/// `[0xA8B380]`) is not shown, the send already echoed it. The line goes to
/// the current dialog's transcript (`0x005E2700`), the lobby's, C2/C9's or
/// `0x117`'s. In modes 1-3 the two ANSI message sinks route to the Quick
/// Match runner instead (`0x007A8DBB..0x007A8DEB`, `0x007A8FDB..0x007A9024`);
/// the UTF-16 and action sinks have no mode test.
pub(in crate::app::shell_wol) fn message_received(
    frontend: &mut FrontendState,
    game_config: Option<&GameConfig>,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    options_profile: &RetailOptionsProfile,
    message: ReceivedMessage,
) {
    let ReceivedMessage {
        from,
        text,
        localized,
        warning,
        private,
        action,
        ansi,
        options,
        sent_at,
        visit,
    } = message;
    // A visitor's room goes on hidden: its lines go to it, and the visited
    // lobby's to the lobby page.
    if visit && !visiting(frontend.wol.as_ref()) {
        return;
    }
    let room = !visit
        && (frontend.shell_route == ShellRoute::NetworkSetup || visiting(frontend.wol.as_ref()));
    let in_game = frontend.wol.as_ref().is_none_or(|visit| visit.in_game);
    if ansi && !action && !in_game && super::quick_match::mode_active(frontend.wol.as_ref()) {
        let text = String::from_utf16_lossy(&text);
        super::quick_match::ansi_message(
            frontend,
            game_config,
            match_state,
            process_assets,
            renderer,
            options_profile,
            &from,
            &text,
            private,
            options,
        );
        return;
    }
    if frontend.wol.as_ref().is_none_or(|visit| visit.in_game)
        || !sender_passes(frontend, &from, &text, room)
        || from
            .name
            .eq_ignore_ascii_case(&nickname(&frontend.wol_account))
    {
        return;
    }
    let (line, color) = wol_chat::received_line(&sender(&from), &text, private, action);
    let color = if warning {
        wol_chat::WARNING_COLOR
    } else {
        color
    };
    let line = LanChatLine { text: line, color };
    if visit {
        crate::app::shell_lan::append_wol_lobby_chat(
            renderer,
            frontend,
            match_state,
            process_assets,
            line,
            sent_at,
        );
    } else {
        crate::app::shell_lan::append_room_chat_sent_at(
            renderer,
            frontend,
            match_state,
            process_assets,
            line,
            sent_at,
        );
    }
    let (prefix, _) = wol_chat::received_line(&sender(&from), &[], private, action);
    super::translation::received(
        frontend.wol.as_mut(),
        options_profile,
        &text,
        localized,
        super::translation::TranslatedLine::Wol {
            prefix,
            color,
            visit,
        },
    );
}

/// The find sink `0x007A9970` for a locate this user sent (`[0xB76E28]`),
/// outside a buddy-status locate and an open `0xCF`:
/// `TXT_USER_FOUND` with a chat lobby's room text (type 0 named
/// `Lob_41_<n>`, as `TXT_JOINED_S` shows it) or the channel's own name;
/// otherwise the code's `TXT_USER_*` text. The line goes, white, to the
/// current dialog's transcript and the sink returns; only the path without
/// a transcript clears the name (`0x007AA685`).
/// RESIDUAL: in a match the line is a MessageList row (WOL in-match chain).
pub(in crate::app::shell_wol) fn located(
    frontend: &mut FrontendState,
    platform: &PlatformState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
    result: Result<WolChannel, u32>,
) {
    // 0x007A9986: a pending buddy locate takes any answer first.
    if super::buddies::located(
        frontend,
        &platform.window,
        process_assets,
        renderer,
        &result,
    ) {
        return;
    }
    // 0x007A997A: with 0xCF in the stack its status takes the answer.
    if super::page::located(
        frontend.wol.as_mut(),
        &platform.window,
        process_assets,
        &result,
    ) {
        return;
    }
    let Some(visit) = frontend.wol.as_ref() else {
        return;
    };
    let name = visit.locate_name.clone().unwrap_or_default();
    use crate::assets::csf_file::{CsfArg, format_csf};
    let text = match result {
        Ok(channel) => {
            let room = match channel.lobby_number().filter(|_| channel.kind == 0) {
                Some(number) => WolLobbyRow::joined_name(
                    number,
                    &base_names(frontend.wol.as_ref(), process_assets),
                ),
                None => channel.name.clone(),
            };
            format_csf(
                &process_assets.csf_text("TXT_USER_FOUND"),
                &[CsfArg::Str(&name), CsfArg::Str(&room)],
            )
        }
        Err(code) => {
            let key = match code {
                0x0004_0131 => "TXT_USER_NOT_IN_CHANNEL",
                0x0004_0132 => "TXT_USER_FIND_DISABLED",
                _ => "TXT_USER_NOT_ONLINE",
            };
            format_csf(&process_assets.csf_text(key), &[CsfArg::Str(&name)])
        }
    };
    // 0x007AA5D5..0x007AA685: colour [0x7EEF28] = -1; `[0xB76E28]` is
    // cleared either way.
    let line = LanChatLine {
        text: text.encode_utf16().collect(),
        color: u32::MAX,
    };
    if !super::page::current_window_line(
        frontend,
        platform,
        match_state,
        process_assets,
        renderer,
        diag,
        sfx_player,
        persistence,
        line,
    ) {
        log::info!("WOL find result outside a transcript: {text}");
    }
    if let Some(visit) = frontend.wol.as_mut() {
        visit.locate_name = None;
    }
}

/// The page-send result sink `0x007A9800`: success writes `TXT_PAGESENT`
/// through `0x005E2790`, so only an open `0xCF` shows it. A failure goes
/// to the current window only: its `0x53F` (`0x113`, C2/C9, `0x11E`), else
/// its `0x5B5` (`0xCF`).
/// RESIDUAL: with neither, a match posts a MessageList row (in-match chain).
pub(in crate::app::shell_wol) fn page_result(
    frontend: &mut FrontendState,
    platform: &PlatformState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
    result: Result<(), u32>,
) {
    let key = match result {
        Ok(()) => "TXT_PAGESENT",
        Err(0x0004_0134) => "TXT_PAGEDISABLED",
        Err(0x0004_0133) => "TXT_PAGEUSERGONE",
        Err(_) => "TXT_PAGEFAILED",
    };
    let line = LanChatLine {
        text: units(process_assets, key),
        color: u32::MAX,
    };
    if result.is_ok() {
        super::page::append_page_line(frontend, renderer, line);
    } else {
        super::page::current_window_line(
            frontend,
            platform,
            match_state,
            process_assets,
            renderer,
            diag,
            sfx_player,
            persistence,
            line,
        );
    }
}

/// The NetStatus sink `0x007AA6D0` outside the server-list wait: only
/// [`service::NET_STATUS_LOST`] while connected (`[0xB77DC0]` clear) does
/// anything. In a match it posts one `TXT_WOL_CONNLOST` row (`[0xB77DC2]`
/// once) and clears the channel records, leaving `[0xB77DC0]` alone; the
/// runner logs in again on its next entry. Outside a match, with `0x113`
/// in the stack, `TXT_YOURE_DISCON` shows from inside the sink; then
/// `[0xB77DC0]` and `[0xB77DC2]` are set, the channel records cleared and
/// event 0 ends the runner, whose `[0xB77AD0]` box is empty (the welcome).
/// RESIDUAL: native shows that box over a running C2/C9; here the setup is
/// dropped first and the box shows over `0x113`. A native slide pumps no
/// messages, so the sink never runs mid-slide; here a status arriving during
/// a C2/C9 Back slide is handled at once and the slide's end then starts a
/// re-login over the box.
pub(in crate::app::shell_wol) fn net_status(state: &mut AppState, status: u32) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    if !visit.connected || status != service::NET_STATUS_LOST {
        return;
    }
    if visit.in_game {
        if visit.disconnected {
            return;
        }
        visit.disconnected = true;
        if let Some(lobby) = visit.lobby_mut() {
            lobby.current = None;
            lobby.pending = None;
            lobby.members.clear();
        }
        crate::app::input::messages::post_wol_connection_lost(
            &state.platform,
            &state.renderer,
            &state.diag,
            &state.frontend,
            &mut state.match_state,
            &state.process_assets,
            &mut state.audio.sfx_player,
            &state.persistence,
        );
        return;
    }
    visit.connected = false;
    visit.disconnected = true;
    let Some(lobby) = visit.lobby_mut() else {
        return;
    };
    lobby.current = None;
    lobby.pending = None;
    lobby.members.clear();
    if let (Runner::Buddy | Runner::QuickMatch, Step::ListingLobbies { .. }) =
        (lobby.runner, &lobby.step)
    {
        // The entry's list wait 0x0077F120 fails without a box: mode 0,
        // the welcome.
        close_runner(state);
        return;
    }
    if lobby.runner == Runner::QuickMatch {
        // 0x7AA7E3: TXT_YOURE_DISCON needs 0x113 open; the Quick Match
        // runner only takes event 0, and its exit has no channel to leave.
        super::quick_match::exit(state);
        return;
    }
    if lobby.runner == Runner::Buddy {
        // The Buddy List runner's pump takes event 0: 0x116 ends without a
        // slide and TXT_YOURE_DISCON shows before the welcome. Over C2/C9
        // the setup is dropped first, as for the lobby.
        if state.frontend.shell_route == ShellRoute::NetworkSetup {
            crate::app::shell_network_setup::abandon_for_wol_runner(
                &mut state.frontend,
                &mut state.match_state,
            );
        }
        super::buddy::dialog_ended(&mut state.frontend);
        show_box(
            &mut state.frontend,
            &state.process_assets,
            "TXT_YOURE_DISCON",
            false,
            BoxThen::Close,
        );
        return;
    }
    if state.frontend.shell_route == ShellRoute::NetworkSetup {
        crate::app::shell_network_setup::abandon_for_wol_runner(
            &mut state.frontend,
            &mut state.match_state,
        );
        state.frontend.shell_route = ShellRoute::WolLobby;
        if let Some(lobby) = lobby_mut(state.frontend.wol.as_mut()) {
            lobby.shown = true;
            lobby.covered = false;
        }
        super::leaf::open_art(
            &mut state.frontend,
            &state.process_assets,
            &state.renderer,
            DIALOG,
        );
    }
    // The records are zeroed without a list refill; the box shows over the
    // lists as they were, and the runner then closes.
    show_box(
        &mut state.frontend,
        &state.process_assets,
        "TXT_YOURE_DISCON",
        false,
        BoxThen::Close,
    );
}

/// The kick sink `0x007AB670` in a chat lobby; nothing while a match runs.
/// This user: `TXT_YOURE_KICKED` with the channel's name and the kicker in
/// a box, whose close moves the lobby pick on (`[0xB77DD4]`) and runs the
/// leave tail `0x00785740`. Another user: `TXT_USER_KICKED` in the
/// transcript, then its leave from the users. In a game channel this user's
/// line (`TXT_CHANNEL_FULL` when the session is over-full) goes to the
/// hidden `0x113`'s transcript (`0x7AB824..0x7AB850`) and the local leave
/// closes C2/C9; another user's line goes to C2/C9.
/// RESIDUAL: in a chat lobby another user's line is skipped natively when
/// `0x005E9510` finds no open slot among the session nodes, which there
/// depends on what the last setup left; it is always shown here.
pub(in crate::app::shell_wol) fn kicked(
    state: &mut AppState,
    channel: WolChannel,
    user: WolMember,
    by: WolMember,
) {
    if state
        .frontend
        .wol
        .as_ref()
        .is_none_or(|visit| visit.in_game)
    {
        return;
    }
    let Some(lobby) = lobby(state.frontend.wol.as_ref()) else {
        return;
    };
    use crate::assets::csf_file::{CsfArg, format_csf};
    if lobby
        .current
        .as_ref()
        .is_some_and(|current| current.kind != 0)
    {
        // VERA: a kicked visitor stays in the lobby they visit; the lobby
        // made it their channel.
        if user.flags & MEMBER_LOCAL != 0 && visiting(state.frontend.wol.as_ref()) {
            let text = format_csf(
                &state
                    .process_assets
                    .csf_text(setup::kicked_key(&state.match_state.session)),
                &[CsfArg::Str(&channel.name), CsfArg::Str(&by.name)],
            );
            visit_settles(
                &mut state.frontend,
                &mut state.match_state,
                &state.process_assets,
                &state.renderer,
                text,
            );
            return;
        }
        if state.frontend.shell_route != ShellRoute::NetworkSetup
            && !visiting(state.frontend.wol.as_ref())
        {
            return;
        }
        if user.flags & MEMBER_LOCAL != 0 {
            let key = setup::kicked_key(&state.match_state.session);
            let text = format_csf(
                &state.process_assets.csf_text(key),
                &[CsfArg::Str(&channel.name), CsfArg::Str(&by.name)],
            );
            crate::app::shell_lan::append_wol_lobby_chat(
                &state.renderer,
                &mut state.frontend,
                &state.match_state,
                &state.process_assets,
                LanChatLine {
                    text: text.encode_utf16().collect(),
                    color: u32::MAX,
                },
                None,
            );
            setup_channel_left(state);
            return;
        }
        let text = format_csf(
            &state.process_assets.csf_text("TXT_USER_KICKED"),
            &[
                CsfArg::Str(&user.name),
                CsfArg::Str(&channel.name),
                CsfArg::Str(&by.name),
            ],
        );
        if let Some(lobby) = lobby_mut(state.frontend.wol.as_mut()) {
            lobby.members.retain(|member| member.name != user.name);
        }
        if let Err(error) = setup::user_kicked(state, &user.name, text) {
            log::warn!("WOL game-channel kick: {error:#}");
        }
        return;
    }
    if user.flags & MEMBER_LOCAL != 0 {
        let text = format_csf(
            &state.process_assets.csf_text("TXT_YOURE_KICKED"),
            &[CsfArg::Str(&channel.name), CsfArg::Str(&by.name)],
        );
        let message = service::message_box(&state.process_assets, text, false);
        put_box(&mut state.frontend, message, BoxThen::Kicked);
    } else {
        let text = format_csf(
            &state.process_assets.csf_text("TXT_USER_KICKED"),
            &[
                CsfArg::Str(&user.name),
                CsfArg::Str(&channel.name),
                CsfArg::Str(&by.name),
            ],
        );
        message_line(
            &mut state.frontend,
            &state.match_state,
            &state.process_assets,
            &state.renderer,
            text,
        );
        if let Some(lobby) = lobby_mut(state.frontend.wol.as_mut()) {
            lobby.members.retain(|member| member.name != user.name);
        }
        refill_players(&mut state.frontend);
    }
}

/// The ban sink `0x007AB3C0`: outside a match, `TXT_CHAN_BAN` or
/// `TXT_CHAN_UNBAN` with the name in the transcript.
pub(in crate::app::shell_wol) fn channel_ban(
    frontend: &mut FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    name: &str,
    banned: bool,
) {
    if frontend.wol.as_ref().is_none_or(|visit| visit.in_game) {
        return;
    }
    let key = if banned {
        "TXT_CHAN_BAN"
    } else {
        "TXT_CHAN_UNBAN"
    };
    let text = crate::assets::csf_file::format_csf(
        &process_assets.csf_text(key),
        &[crate::assets::csf_file::CsfArg::Str(name)],
    );
    message_line(frontend, match_state, process_assets, renderer, text);
}

/// The page sinks after their gates: the ANSI one (`0x007A9200`) keeps the
/// sender as the `/reply` target `[0xB77E30]`; the UTF-16 one (`0x007A9550`)
/// does not. Outside a match the line `u"(%hs) %s"` walks the shell stack
/// from the current window: `0xCF`'s list `0x5B5` takes it and the walk goes
/// on (`0x005E2790`); the first transcript `0x53F` (`0x113`, C2/C9 or an
/// open `0x11E`) takes it and the walk stops; with none, `0x11E` opens.
/// In a match (`[0xB77DC1]`, `0x007A92F9..0x007A93DC`) the line is a Grey
/// MessageList row. VERA keeps the match playable when a page arrives and
/// omits the retail reply hint. `[AudioVisual] IncomingMessage` plays last.
/// RESIDUAL: the auto-reply `[0xB77E40]`/`[0xB76230]` has no writer here.
pub(in crate::app::shell_wol) fn page_received(
    frontend: &mut FrontendState,
    platform: &PlatformState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
    from: WolMember,
    text: Vec<u16>,
    ansi: bool,
) {
    if frontend.wol.is_none()
        || !sender_passes(
            frontend,
            &from,
            &text,
            frontend.shell_route == ShellRoute::NetworkSetup,
        )
    {
        return;
    }
    let visit = frontend.wol.as_mut().expect("checked");
    if ansi {
        visit.last_pager = Some(from.name.clone());
    }
    let in_game = visit.in_game;
    let mut units: Vec<u16> = format!("({}) ", from.name).encode_utf16().collect();
    units.extend_from_slice(&text);
    let line = LanChatLine {
        text: units,
        color: wol_chat::PAGE_COLOR,
    };
    if in_game {
        let row = String::from_utf16_lossy(&line.text);
        crate::app::input::messages::post_wol_row(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            sfx_player,
            persistence,
            &row,
        );
    } else {
        super::page::append_page_line(frontend, renderer, line.clone());
        // The walk down the stack stops at the first `0x53F`: the lobby's
        // (also while Options `0x2BC` hides it), C2/C9's, or `0x11E`'s.
        let route = frontend.shell_route;
        let transcript = super::page::page_received(frontend.wol.as_ref()).is_none()
            && (route == ShellRoute::NetworkSetup
                || lobby_dialog(frontend.wol.as_ref()).is_some()
                || super::quick_match::page_open(frontend.wol.as_ref()));
        if transcript {
            crate::app::shell_lan::append_chat(
                renderer,
                frontend,
                match_state,
                process_assets,
                line,
            );
        } else {
            super::page::page_received_line(frontend, &platform.window, renderer, line);
        }
    }
    // 0x7A94ED..0x7A9510: with [0xB77E40] the pager gets the Quick Match
    // away text (API+0xEC).
    super::quick_match::page_reply(frontend.wol.as_mut(), &from.name);
    App::play_shell_rules_sound(frontend, match_state, process_assets, sfx_player, |rules| {
        rules.general.incoming_message_sound()
    });
}
