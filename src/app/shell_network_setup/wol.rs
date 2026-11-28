//! WOL-only parts of the setup dialogs C2 (0078F7F0) and C9 (00791B10).
//! Everything shared with BC/BD stays in the parent owner; this module holds
//! the StartGame/OnGameStart handshake, the WOL chat line and the readiness
//! clearing native WOL performs on option edits.

use super::actions::queue;
use super::chat_notice as message;
use super::*;
use crate::app::match_runtime::sim_host::SimHost;
use crate::app::renderer_state::RendererState;
use crate::audio::sfx::SfxPlayer;
use crate::render::gpu::GpuContext;
use crate::skirmish_launch::LaunchTeam;
use crate::ui::lan_shell::LanChatLine;
use crate::ui::wol_chat;

/// One server user record of the game channel at OnGameStart. `flags` bit0
/// marks the channel owner (`0x794F6F..`: the first such row decides).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WolStartUser {
    pub(crate) name: String,
    pub(crate) flags: u32,
}

/// The WOL host publishes whatever `g_RngSeed` (`0xA8ED94`) holds: no WOL
/// writer exists (C2 draws nothing, `0x797276` is unreferenced debug code).
/// That is the seed of the last `Init_Random_Number_System` (`0052FC20`):
/// the resident match's seed, else the process start seed.
pub(crate) fn resident_seed(startup_rng_seed: u32, sim_runtime: &SimHost) -> u32 {
    sim_runtime.runtime().map_or(startup_rng_seed, |runtime| {
        runtime.simulation.session().seed() as u32
    })
}

impl App {
    /// The chat join sink `0x7A5890` creates C9 (`0x7A6256`) for a joined
    /// game channel with the local node's choices unset. Like
    /// BD, the host's options arrive later; unlike BD, C9 never disables
    /// Accept and the sink's `005E9850` enables it.
    pub(crate) fn open_wol_joined_setup(
        state: &mut AppState,
        mut local: crate::skirmish_launch::SkirmishHumanSlot,
        observer_row: Option<u8>,
    ) {
        local.slot = unset_choices();
        if App::open_joined_setup(state, local, observer_row, SetupFlavor::Wol)
            && let Some(protocol) = state.frontend.network_setup_protocol.as_mut()
        {
            protocol.accept_eligible = true;
        }
    }
}

/// A node's choices before the host's options: country, color, start and
/// team all -2 (Random), as the join sink `0x7A5890` builds the local node
/// and the user-list sink `0x7A7558..0x7A7786` every other one.
pub(crate) fn unset_choices() -> crate::skirmish_launch::SkirmishLocalSlot {
    crate::skirmish_launch::SkirmishLocalSlot {
        country: crate::skirmish_launch::LaunchCountry::from_country_index(0),
        country_random: true,
        color_index: 0,
        color_random: true,
        start_position: crate::skirmish_launch::LaunchStartPosition::Auto,
        team: crate::skirmish_launch::LaunchTeam::None,
    }
}

/// The guest's user-list sink `0x7A7110` after its own game join: each
/// other user becomes the next node with unset choices
/// (`0x7A7558..0x7A7786`) but no row; rows are bound only by the host's
/// `'L'` (`0x5E8EA0`), here [`service::NetworkSetupEvent::HostSetup`]. The
/// first node added while only the local one exists is the host
/// `[0xA8D610]` (`0x7A7627..0x7A763D`). Without an operator in the list
/// this returns false and the caller runs the local leave
/// (`0x7A782A..0x7A788D`); otherwise `0x5E9850` clears the acceptances.
pub(crate) fn guest_users_listed(
    protocol: &mut Option<NetworkSetupProtocol>,
    session: &mut MatchSession,
    users: &[(String, bool)],
) -> bool {
    for (name, _) in users {
        let human = crate::skirmish_launch::SkirmishHumanSlot {
            raw_slot_index: None,
            name: name.clone(),
            slot: unset_choices(),
            role: Default::default(),
        };
        if let Err(error) = session.add_network_participant(human) {
            log::warn!("WOL user list: {name} has no node: {error:#}");
            continue;
        }
        if let Some(protocol) = protocol.as_mut()
            && protocol.wol_host.is_none()
        {
            protocol.wol_host = Some(name.clone());
        }
    }
    if !users.iter().any(|(_, operator)| *operator) {
        return false;
    }
    if let Err(error) = clear_guest_acceptance(protocol, session) {
        log::warn!("WOL user list: {error:#}");
    }
    true
}

/// The guest's first `R` to the host after the user list
/// (`0x7A7981..0x7A7AFD`): the MultiPlayer Country and Color the persona
/// just set, start and team -2; the observer preference sends the
/// observer's.
pub(crate) fn send_first_choices(frontend: &mut FrontendState, session: &MatchSession) {
    let local = frontend.multiplayer_shell_runtime.local_participant();
    let Some(row) = session
        .network_setup()
        .and_then(|launch| launch.local_player().raw_slot_index)
    else {
        return;
    };
    let observer = local.is_observer();
    let mut slot = local.slot;
    slot.start_position = crate::skirmish_launch::LaunchStartPosition::Auto;
    slot.team = crate::skirmish_launch::LaunchTeam::None;
    queue(
        &mut frontend.network_setup_protocol,
        NetworkSetupRequest::PlayerChoices {
            row,
            slot,
            observer,
        },
    );
}

/// The join sink `0x7A5890` for another user in this game channel. While a
/// start is pending (`[0xB77DEC]`, `[0xB77E28]` or `[0xB77E27]`) the joiner
/// is kicked (API+0x54) after the PlayerJoined sound and gets no node
/// (`0x7A631E..0x7A6348`). A guest appends an unbound node. The host binds
/// it through `0x5E9240`: a player on the first Open row, an observer on
/// the first open observer row when no Open player row is left
/// (`0x5E9510`); with neither (`0x5E9560`) the joiner is kicked
/// (`0x7A6BC7`). VERA's room first frees the last AI's seat when every seat
/// is taken, so it stays open until its host's Max players, and one with
/// Teams set lets the newcomer watch first (`MatchSession::network_join_row`).
/// RESIDUAL: the host's node takes the mode's default country (mode
/// vt+0x28) natively; it is unset here.
pub(crate) fn user_joined(state: &mut AppState, name: String) -> anyhow::Result<()> {
    let pending = super::start_phase(state.frontend.network_setup_protocol.as_ref()).in_progress()
        || state
            .frontend
            .network_setup_protocol
            .as_ref()
            .is_some_and(|protocol| protocol.locked);
    if !pending {
        state.match_state.session.free_network_seat_for_newcomer()?;
    }
    let session = &state.match_state.session;
    let place = if !session.host() {
        Some((None, false))
    } else if session.network_several_observers() {
        // VERA's room, to its host's Max players.
        session
            .network_join_row(false)
            .map(|(row, observer)| (Some(row), observer))
    } else if session.network_no_open_player_row() {
        session.network_free_row(true).map(|row| (Some(row), true))
    } else {
        session
            .network_free_row(false)
            .map(|row| (Some(row), false))
    };
    let Some((row, observer)) = place.filter(|_| !pending) else {
        App::play_shell_rules_sound(
            &state.frontend,
            &state.match_state,
            &state.process_assets,
            &mut state.audio.sfx_player,
            |rules| rules.general.player_joined_sound(),
        );
        queue(
            &mut state.frontend.network_setup_protocol,
            NetworkSetupRequest::Kick {
                name,
                keep_out: false,
            },
        );
        return Ok(());
    };
    let mut human = crate::skirmish_launch::SkirmishHumanSlot {
        raw_slot_index: row,
        name,
        slot: unset_choices(),
        role: Default::default(),
    };
    human.set_observer(observer);
    service::service_event(state, service::NetworkSetupEvent::ParticipantJoined(human))
}

/// `0x785740` for another user leaving this game channel: PlayerLeft
/// (`0x785764..0x785788`); after OnGameStart nothing more. Its node goes;
/// the ready records clear and C9's Accept enables (`0x785C64..0x785CF5`);
/// a guest whose operator left leaves the channel itself
/// (`0x785CFA..0x785D1C`); the host clears readiness, sends the slot-name
/// payload (`0x77DB90`) and publishes options (`0x5E32D0(1, 1)`) before the
/// repaint (`0x783C80`, `0x785D1F..0x785E81`). It does not advertise the
/// channel (`0x78F000`). The WOL adapter's one options snapshot includes the
/// complete launch roster, so it carries both native payloads without a
/// duplicate relay update.
pub(crate) fn user_left(state: &mut AppState, name: &str, operator: bool) -> anyhow::Result<()> {
    // VERA's room tells who left, unless the host left (the room closes
    // with its own line).
    departed(state, name, operator, !operator)
}

fn departed(state: &mut AppState, name: &str, operator: bool, tell: bool) -> anyhow::Result<()> {
    play_player_left(
        &state.frontend,
        &state.match_state,
        &state.process_assets,
        &mut state.audio.sfx_player,
    );
    if state
        .frontend
        .network_setup_protocol
        .as_ref()
        .is_some_and(|protocol| protocol.locked)
    {
        return Ok(());
    }
    let Some(index) = state
        .match_state
        .session
        .network_setup()
        .and_then(|launch| launch.humans.iter().position(|human| human.name == name))
    else {
        return Ok(());
    };
    service::service_event(state, service::NetworkSetupEvent::ParticipantLeft { index })?;
    if tell {
        let dialog = super::dialog(&state.frontend, &state.match_state);
        service::tell_room(
            &state.renderer,
            &mut state.frontend,
            &state.match_state,
            &state.process_assets,
            dialog,
            "VERA:PlayerLeftRoom",
            name,
        );
    }
    clear_guest_acceptance(
        &mut state.frontend.network_setup_protocol,
        &mut state.match_state.session,
    )?;
    let host = state.match_state.session.host();
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.record_remote_user_left(host, operator);
    }
    Ok(())
}

/// The private-options sink `0x7A8260`'s gates for a guest's `R`: nothing
/// while a WOL match runs (`[0xB77DC1]`) or a start is requested
/// (`[0xB77E28]`), and nothing in Quick Match (`[0xA8B244]` 1-3 take only
/// `Z`), where the matchbot's options seat both players; the sender must be
/// a channel user (the caller's check) and own a node here. Only a host
/// receives it. VERA's online host takes it while its guests accept the
/// final settings: the lobby has un-readied the sender, so the change
/// republishes and cancels that start.
pub(crate) fn player_choices(
    state: &mut AppState,
    name: &str,
    choices: crate::skirmish_launch::SkirmishLocalSlot,
    observer: bool,
) -> anyhow::Result<()> {
    if crate::app::shell_wol::in_match(state.frontend.wol.as_ref())
        || super::start_phase(state.frontend.network_setup_protocol.as_ref()).submitted()
        || super::quick_match(state.frontend.network_setup_protocol.as_ref())
        || !state.match_state.session.host()
    {
        return Ok(());
    }
    let Some(index) = participant(&state.match_state.session, name) else {
        return Ok(());
    };
    service::service_event(
        state,
        service::NetworkSetupEvent::PlayerChoices {
            index,
            choices,
            observer,
        },
    )
}

/// The VERA lobby's `balance` event, its answer to the host's `/balance`
/// (no native counterpart): a team for each named player. It has
/// `player_choices`' gates but waits, as the host's own edits do, from its
/// Start click, and acts only in a mode that lets players ally outside a clan
/// room, where the team combos show.
pub(crate) fn balance_teams(state: &mut AppState, teams: &[(String, u8)]) -> anyhow::Result<()> {
    if crate::app::shell_wol::in_match(state.frontend.wol.as_ref())
        || super::start_phase(state.frontend.network_setup_protocol.as_ref()).in_progress()
        || super::quick_match(state.frontend.network_setup_protocol.as_ref())
        || !state.match_state.session.host()
    {
        return Ok(());
    }
    let session = &state.match_state.session;
    let Some(launch) = session.network_setup() else {
        return Ok(());
    };
    if !launch.mode.allies_allowed || session.network_room().clan {
        return Ok(());
    }
    // Without Teams, the Team list's A to D (`team_choices`).
    let count = session
        .network_teams()
        .unwrap_or(crate::skirmish_launch::NATIVE_TEAMS);
    let moves = team_moves(launch, teams, count);
    if moves.is_empty() {
        return Ok(());
    }
    service::service_event(state, service::NetworkSetupEvent::Teams(moves))
}

/// The participants `teams` move to another team: playing humans with a row,
/// on one of the room's `count` teams.
fn team_moves(
    launch: &SkirmishLaunchSession,
    teams: &[(String, u8)],
    count: u8,
) -> Vec<(usize, u8)> {
    teams
        .iter()
        .filter_map(|(name, team)| {
            let index = participant_in(launch, name)?;
            let human = &launch.humans[index];
            (*team < count
                && !human.is_observer()
                && human.raw_slot_index.is_some()
                && human.slot.team != LaunchTeam::Team(*team))
            .then_some((index, *team))
        })
        .collect()
}

/// A user's `A<n>` (`0x5E3D10` arm `'A'`, `0x5E4388` → `0x77E900`): the
/// parse drops it while a WOL match runs or a start is requested
/// (`0x5E3E73..0x5E3E90`); the user's node takes the value, and its
/// accept icon shows only for 1 (`0x5EA3E0`).
pub(crate) fn player_ready(state: &mut AppState, name: &str, value: i32) -> anyhow::Result<()> {
    if crate::app::shell_wol::in_match(state.frontend.wol.as_ref())
        || super::start_phase(state.frontend.network_setup_protocol.as_ref()).submitted()
    {
        return Ok(());
    }
    let result = ready_arm(state, name, value);
    if super::quick_match(state.frontend.network_setup_protocol.as_ref()) {
        quick_match_start(&mut state.frontend, &state.match_state.session, name);
    } else {
        guest_sink_tail(
            &mut state.frontend.network_setup_protocol,
            &mut state.match_state.session,
        );
    }
    result
}

/// `0x0077E900`'s Quick Match tail (`0x0077EA53..0x0077EB72`): any `A<n>`
/// (`n` untested) from the node at index 1 while the channel has exactly
/// two users, on the host before its StartGame: every channel user, in
/// reverse channel order, to API+0x48, stage 9 (90 s) and `[0xB77DEC]`.
fn quick_match_start(frontend: &mut FrontendState, session: &MatchSession, name: &str) {
    let second = session
        .network_setup()
        .and_then(|launch| launch.humans.get(1))
        .is_some_and(|human| human.name.eq_ignore_ascii_case(name));
    if !session.host()
        || !second
        || crate::app::shell_wol::lobby::member_count(frontend.wol.as_ref()) != 2
    {
        return;
    }
    let users = crate::app::shell_wol::lobby::member_names(frontend.wol.as_ref())
        .rev()
        .map(str::to_owned)
        .collect();
    if frontend
        .network_setup_protocol
        .as_mut()
        .is_some_and(|protocol| protocol.submit_start(users))
    {
        crate::app::shell_wol::quick_match::start_requested(frontend.wol.as_mut());
    }
}

/// The Quick Match guest's options sink `+0x58` after its parse
/// (`0x007A7F72..0x007A8014`): its own node with a side and a colour, and
/// exactly two channel users, accept at once: `"A1"` channel-wide
/// (API+0x3C). The text is the host's digit-led options, and the parse
/// already dropped it while a match runs or a start is pending. VERA: the
/// online lobby's verified map runs it again (`accept_network_map_source`).
/// RESIDUAL: the channel-owner gate (`0x007931A0`, the first operator is
/// not this user) holds for every guest the matchbot names.
pub(super) fn quick_match_accept(frontend: &mut FrontendState, session: &MatchSession) {
    let assigned = session.network_setup().is_some_and(|launch| {
        let slot = &launch.local_player().slot;
        !slot.country_random && !slot.color_random
    });
    if !assigned || crate::app::shell_wol::lobby::member_count(frontend.wol.as_ref()) != 2 {
        return;
    }
    queue(
        &mut frontend.network_setup_protocol,
        NetworkSetupRequest::Ready(true),
    );
}

fn ready_arm(state: &mut AppState, name: &str, value: i32) -> anyhow::Result<()> {
    let Some(index) = participant(&state.match_state.session, name) else {
        return Ok(());
    };
    // RESIDUAL: 0x77E900's own-name A0 re-enables C9 (0x5EDDE0); this
    // user's own readiness is local.
    if state
        .match_state
        .session
        .network_setup()
        .is_some_and(|launch| launch.local_player_index == index)
    {
        return Ok(());
    }
    service::service_event(
        state,
        service::NetworkSetupEvent::PlayerReady {
            index,
            accepted: value == 1,
        },
    )
}

/// `0x005E3D10`'s `K` arm, for a channel user's map status while C2 or C9
/// is open. Like `A`, it is dropped while a match runs or a start is
/// requested (`0x005E3E73..0x005E3E90`).
pub(crate) fn map_status(state: &mut AppState, name: &str, status: u8) -> anyhow::Result<()> {
    if crate::app::shell_wol::in_match(state.frontend.wol.as_ref())
        || super::start_phase(state.frontend.network_setup_protocol.as_ref()).submitted()
    {
        return Ok(());
    }
    let result = match participant(&state.match_state.session, name) {
        Some(index) => service::service_event(
            state,
            service::NetworkSetupEvent::MapStatus { index, status },
        ),
        None => Ok(()),
    };
    guest_sink_tail(
        &mut state.frontend.network_setup_protocol,
        &mut state.match_state.session,
    );
    result
}

/// After the arm, a guest's `+0x58` runs its preview tail on every public
/// text that passed its gates, whoever sent it (`0x007A7F14..0x007A8213`).
fn guest_sink_tail(protocol: &mut Option<NetworkSetupProtocol>, session: &mut MatchSession) {
    if !session.host() {
        options_preview(protocol, session);
    }
}

/// The guest's options sink `+0x58` (`0x007A7EA0`) after its parse, for
/// each public options text: the host's options, and any user's `A` or
/// `K`. A guest with a preview, received or from its own verified mixfile
/// map, returns (`0x007A8017..0x007A8025`, `0x007A80EF..0x007A8143`).
/// Otherwise, unless the map is `RandMap.Sed` (`0x007A80DD..0x007A80E9`), an
/// accepted user in a tournament or clan room takes its acceptance back:
/// `A0`, and Accept disabled (`0x007A8145..0x007A8196`).
/// RESIDUAL: the sink's `P%s` preview request to the first channel user and
/// its 900-bucket repeat (`0x007A802B..0x007A80D7`) are not sent, and the
/// host's `Z3` and `P` arms are not ported: the online lobby moves previews
/// as map transfers (docs/lobby-protocol.md).
pub(super) fn options_preview(
    protocol: &mut Option<NetworkSetupProtocol>,
    session: &mut MatchSession,
) {
    let random = session
        .network_setup()
        .and_then(|launch| launch.selected_map_file.as_deref())
        .is_some_and(|file| file.eq_ignore_ascii_case(crate::map::skirmish_scenarios::RANDMAP_SED));
    let room = session.network_room();
    let local = session
        .network_setup()
        .map(|launch| launch.local_player_index);
    let unaccept = !random
        && room.tournament_or_clan()
        && local.is_some_and(|index| session.network_player_ready(index));
    let protocol = protocol.as_mut().expect("setup owns protocol");
    if protocol.received_preview.is_some() || protocol.local_preview() || !unaccept {
        return;
    }
    protocol.accept_eligible = false;
    protocol.requests.push(NetworkSetupRequest::Ready(false));
    if let Some(index) = local
        && let Err(error) = session.set_network_player_ready(index, false)
    {
        log::warn!("WOL room unaccept: {error:#}");
    }
}

fn participant(session: &MatchSession, name: &str) -> Option<usize> {
    session
        .network_setup()
        .and_then(|launch| participant_in(launch, name))
}

fn participant_in(launch: &SkirmishLaunchSession, name: &str) -> Option<usize> {
    // strcmpi 0x7C8D20 (0x7A8416).
    launch
        .humans
        .iter()
        .position(|human| crate::util::native_string::crt_strcmpi(&human.name, name).is_eq())
}

/// The kick sink `0x7AB670` for another user in this game channel: its
/// `TXT_USER_KICKED` line in C2/C9 unless `0x5E9510` finds no Open player
/// row (`0x7AB860..0x7AB8AD`), then its leave. VERA's room tells the leave
/// only where that line was skipped.
pub(crate) fn user_kicked(state: &mut AppState, name: &str, line: String) -> anyhow::Result<()> {
    let full = state.match_state.session.network_no_open_player_row();
    if !full {
        crate::app::shell_lan::append_room_chat(
            &state.renderer,
            &mut state.frontend,
            &state.match_state,
            &state.process_assets,
            LanChatLine {
                text: line.encode_utf16().collect(),
                color: u32::MAX,
            },
        );
    }
    departed(state, name, false, full)
}

/// The kick sink for this user: `TXT_CHANNEL_FULL` when the session is
/// over-full (`0x5E95E0`), else `TXT_YOURE_KICKED`.
pub(crate) fn kicked_key(session: &MatchSession) -> &'static str {
    if session.network_over_capacity() {
        "TXT_CHANNEL_FULL"
    } else {
        "TXT_YOURE_KICKED"
    }
}

/// `0x785740`'s local tail for C2/C9 after the lobby cleared the channel:
/// PlayerLeft (`0x785764`), then the dialog closes (`CloseAndPop`) and the
/// lobby rejoins (`0x788A30`). No box.
pub(crate) fn local_left(state: &mut AppState) {
    play_player_left(
        &state.frontend,
        &state.match_state,
        &state.process_assets,
        &mut state.audio.sfx_player,
    );
    if super::quick_match(state.frontend.network_setup_protocol.as_ref()) {
        // No dialog: the nodes go with the setup; the runner's stage
        // timeout moves on.
        super::abandon_for_wol_runner(&mut state.frontend, &mut state.match_state);
        return;
    }
    let dialog = super::dialog(&state.frontend, &state.match_state);
    App::leave_shell_dialog(
        state,
        crate::app::frontend::shell_transition::ShellSlideKind::NetworkSetup(dialog),
        crate::app::shell_network_setup::commit_back,
    );
}

/// `0x750920(Rules+0x6BC)` while in a channel (`[0xB76B10]` != 0).
pub(crate) fn play_player_left(
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
) {
    App::play_shell_rules_sound(frontend, match_state, process_assets, sfx_player, |rules| {
        rules.general.player_left_sound()
    });
}

/// `005E9850`: clear every acceptance but the host's and enable Accept on C9.
pub(super) fn clear_guest_acceptance(
    protocol: &mut Option<NetworkSetupProtocol>,
    session: &mut MatchSession,
) -> anyhow::Result<()> {
    session.clear_guest_readiness()?;
    let host = session.host();
    if let Some(protocol) = protocol.as_mut()
        && !host
    {
        protocol.accept_eligible = true;
    }
    Ok(())
}

/// `CChatEventSink+0x5C` OnGameStart (`0x7A7B60`) for host and guest. A
/// user list that differs from the local one clears the pending start (the
/// host may Start again); a match closes the setup and runs Start Game Now
/// (`0x794BA0`), whose timing block decides MaxAhead.
pub(super) fn game_started(
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    users: Vec<WolStartUser>,
    pings: Vec<i32>,
) -> anyhow::Result<()> {
    // 0x7A7B95: once a start has matched ([0xB77E27]), OnGameStart does
    // nothing more.
    if verify(frontend.network_setup_protocol.as_ref()).is_some() {
        return Ok(());
    }
    let launch = match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no admitted setup"))?;
    let local = launch.local_player().name.clone();
    // 0x7A7C80 lowercases both names (Wstring::toLower) before comparing.
    let matches = users.len() == launch.humans.len()
        && launch.humans.iter().all(|human| {
            users
                .iter()
                .any(|user| user.name.eq_ignore_ascii_case(&human.name))
        });
    if !matches {
        if let Some(protocol) = frontend.network_setup_protocol.as_mut() {
            protocol.set_start_phase(super::StartPhase::Idle);
        }
        return Ok(());
    }
    // 0x7A7CC3..0x7A7CE9: modes 1-3 close 0x117; 0x7A7CEE..0x7A7CFC: an
    // open 0xCF closes.
    crate::app::shell_wol::quick_match::game_started(frontend);
    crate::app::shell_wol::page::close_page_dialog(frontend);
    // 0x794F6F..: the first user row with flag bit0 is the channel owner.
    let channel_owner = users
        .iter()
        .find(|user| user.flags & 1 != 0)
        .is_some_and(|user| user.name.eq_ignore_ascii_case(&local));
    match_state
        .session
        .prepare_wol_timing(pings, channel_owner)?;
    // [0xB779C4] (0x7A7D0E..0x7A7D49) and Start Game Now's clan lists
    // (0x795250..0x79546E) for the match.
    let terms = crate::app::shell_wol::lobby::start_terms(frontend.wol.as_ref());
    match_state.session.set_wol_start(terms);
    // C2/C9 stay up: Start Game Now (0x794BA0) pushes the verify 0x130
    // over them with the nodes in [0xA8DA78] order (0x794C5B).
    let names = launch_names(&match_state.session);
    let protocol = frontend
        .network_setup_protocol
        .as_mut()
        .expect("setup owns protocol");
    protocol.locked = true;
    // 0x7A7E03: [0xB77E28] = 2 until the abort or the match, so packets,
    // joins and the setup's own controls stay dropped under 0x130.
    protocol.set_start_phase(super::StartPhase::Submitted);
    protocol.wol_verify = Some(crate::ui::wol_verify::WolVerifyDialog::open(names));
    Ok(())
}

/// The local node leads `[0xA8DA78]`: the host's own, and a guest's join
/// (`0x007A61F0..0x007A620D`); `0x00792F10` skips it and addresses nodes 1
/// up.
/// RESIDUAL: a guest's peers follow the roster order; the native order of
/// a guest's other nodes is not traced.
fn launch_names(session: &MatchSession) -> Vec<String> {
    let Some(launch) = session.network_setup() else {
        return Vec::new();
    };
    let local = launch.local_player_index;
    std::iter::once(&launch.humans[local])
        .chain(
            launch
                .humans
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != local)
                .map(|(_, human)| human),
        )
        .map(|human| human.name.clone())
        .collect()
}

/// Start Game Now's verify `0x130`, while it is up.
pub(crate) fn verify(
    protocol: Option<&NetworkSetupProtocol>,
) -> Option<&crate::ui::wol_verify::WolVerifyDialog> {
    protocol?.wol_verify.as_ref()
}

fn verify_mut(
    protocol: &mut Option<NetworkSetupProtocol>,
) -> anyhow::Result<&mut crate::ui::wol_verify::WolVerifyDialog> {
    protocol
        .as_mut()
        .and_then(|protocol| protocol.wol_verify.as_mut())
        .ok_or_else(|| anyhow::anyhow!("no connection verify is up"))
}

/// Node `node`'s box checks (`0x00794809`, `0x00793D20`).
pub(super) fn verify_connected(
    protocol: &mut Option<NetworkSetupProtocol>,
    node: usize,
) -> anyhow::Result<()> {
    verify_mut(protocol)?.connect(node);
    Ok(())
}

/// `0x00794D0A..`: the negotiation's result. 0 closes `0x130`
/// (`0x00794F30..0x00794F44`), leaves the game channel (API+0x24 at
/// `0x007955F1`) and starts the match; 2 opens `0xD0` with
/// `TXT_SCENARIOS_DO_NOT_MATCH` over `0x130` first; 1 and 2 abort.
/// VERA's own room (not the original-style one) stays joined instead, so
/// its players come back to it after the match.
pub(super) fn negotiated(state: &mut AppState, result: u8) -> anyhow::Result<()> {
    let verify = verify_mut(&mut state.frontend.network_setup_protocol)?;
    anyhow::ensure!(verify.mismatch.is_none(), "the mismatch box is up");
    match result {
        0 => {
            let seed = state
                .match_state
                .session
                .network_start_seed()
                .ok_or_else(|| anyhow::anyhow!("WOL start has no admitted seed"))?;
            // VERA: a guest's match, or a Quick Match the matchbot paired,
            // starts without a click. Call back a player in another window
            // now: a hidden window's slide waits for its frames.
            state.platform.request_attention(true);
            let protocol = state
                .frontend
                .network_setup_protocol
                .as_mut()
                .expect("setup owns protocol");
            protocol.wol_verify = None;
            if protocol.external {
                // A watch stays in the lobby. Its service owns the grant and
                // must receive a refused start so it can release the place.
                if !state.frontend.external_launch {
                    return super::actions::try_commit_start(state, seed);
                }
                super::commit_start(state, seed);
                return Ok(());
            }
            let quick_match = super::quick_match(state.frontend.network_setup_protocol.as_ref());
            let back_to_room =
                super::dialog(&state.frontend, &state.match_state).modern && !quick_match;
            let protocol = state
                .frontend
                .network_setup_protocol
                .as_mut()
                .expect("setup owns protocol");
            if !back_to_room {
                protocol.requests.push(NetworkSetupRequest::Leave);
            }
            state.frontend.skirmish_preview_texture = None;
            if quick_match {
                // 0x130 closes without a slide and no setup dialog exists.
                crate::app::shell_wol::quick_match::start_leave(state.frontend.wol.as_mut());
                super::commit_start(state, seed);
                return Ok(());
            }
            App::leave_shell_dialog_for_network_start(
                state,
                super::dialog(&state.frontend, &state.match_state),
                seed,
            );
        }
        1 => crate::app::shell_wol::start_aborted(state),
        2 => {
            let message = crate::app::shell_wol::message_box(
                &state.process_assets,
                "TXT_SCENARIOS_DO_NOT_MATCH",
            );
            verify_mut(&mut state.frontend.network_setup_protocol)?.mismatch = Some(message);
        }
        other => anyhow::bail!("unknown negotiation result {other}"),
    }
    Ok(())
}

/// The mismatch box's OK: its modal run (`0x007759E0`) returns and the
/// abort goes on.
pub(crate) fn mismatch_ok(state: &mut AppState) {
    if verify(state.frontend.network_setup_protocol.as_ref())
        .is_some_and(|verify| verify.mismatch.is_some())
    {
        crate::app::shell_wol::start_aborted(state);
    }
}

fn screen(gpu: &GpuContext) -> (i32, i32) {
    (gpu.config.width as i32, gpu.config.height as i32)
}

/// `0x130`'s layout on the live screen.
pub(crate) fn verify_layout(
    gpu: &GpuContext,
    frontend: &FrontendState,
    match_state: &MatchState,
) -> crate::ui::shell::modal::WolVerifyLayout {
    use crate::app::frontend::shell_layout;
    use crate::ui::shell::geom::RETAIL_SCALE;

    let (width, height) = screen(gpu);
    let scale = if !setup_dialog(frontend, false, SetupFlavor::Wol).modern {
        RETAIL_SCALE
    } else if super::quick_match(frontend.network_setup_protocol.as_ref()) {
        // Quick Match has no C2/C9 room; its backdrop uses the fitted WOL shell.
        shell_layout::shell_scale_for_screen((width as u32, height as u32))
    } else {
        crate::app::input::multiplayer_setup::page_layout(gpu, frontend, match_state)
            .page
            .scale
    };
    crate::ui::shell::modal::wol_verify_layout(width, height, scale)
}

/// The 0xD0 mismatch is a separate retail message box; do not resize it here.
pub(crate) fn verify_mismatch_parts(
    gpu: &GpuContext,
    message: &crate::ui::shell::modal::ShellBox,
) -> crate::ui::shell::modal::BoxParts {
    let (width, height) = screen(gpu);
    message.parts(width, height, crate::ui::shell::geom::RETAIL_SCALE)
}

/// The pointer while `0x130` is up (`down`: `None` moves): the mismatch
/// box's OK takes it when the box is open; otherwise a box's 18x18 icon
/// (`0x006163A0` toggles on the press) or the extended roster's scrollbar.
pub(crate) fn verify_pointer(state: &mut AppState, down: Option<bool>) {
    use crate::ui::shell::page::PageEvent;
    let at = state.match_state.input.cursor_px();
    let ok = verify(state.frontend.network_setup_protocol.as_ref())
        .and_then(|verify| verify.mismatch.as_ref())
        .map(|message| verify_mismatch_parts(&state.renderer.gpu, message).button(0))
        .unwrap_or_default();
    let layout = verify_layout(&state.renderer.gpu, &state.frontend, &state.match_state);
    let Ok(verify) = verify_mut(&mut state.frontend.network_setup_protocol) else {
        return;
    };
    let events = verify.pointer(&layout, ok, at, down);
    let events = App::play_page_cues(
        &state.frontend,
        &state.match_state,
        &state.process_assets,
        &mut state.audio.sfx_player,
        events,
        false,
    );
    if events.contains(&PageEvent::Activated(crate::ui::wol_shell::BOX_OK)) {
        mismatch_ok(state);
    }
}

/// `0x130`'s widgets on its layout before they paint (input syncs them only
/// on a pointer message).
pub(crate) fn verify_sync(
    gpu: &GpuContext,
    frontend: &mut FrontendState,
    match_state: &MatchState,
) {
    let Some(shown) = verify(frontend.network_setup_protocol.as_ref()) else {
        return;
    };
    let parts = Some(shown)
        .and_then(|verify| verify.mismatch.as_ref())
        .map(|message| verify_mismatch_parts(gpu, message));
    let ok = parts
        .as_ref()
        .map(|parts| parts.button(0))
        .unwrap_or_default();
    let layout = verify_layout(gpu, frontend, match_state);
    if let Ok(verify) = verify_mut(&mut frontend.network_setup_protocol) {
        verify.sync_widgets(&layout, ok);
        verify.sync_mismatch(parts.as_ref());
    }
}

pub(crate) fn verify_poll_scroll(protocol: &mut Option<NetworkSetupProtocol>) -> Option<Instant> {
    verify_mut(protocol).ok()?.poll_scroll().1
}

/// C2/C9 0x540 Enter (notify 0x501): commands first, then a plain line.
pub(super) fn chat(
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    text: Vec<u16>,
) {
    // 5EDD00's TrimRight("\n\r") result is what WOL sends.
    let text = wol_chat::trim_line_end(&text).to_vec();
    if text.is_empty() {
        return;
    }
    let commands = wol_chat::WolChatCommands {
        page: csf_units(process_assets, "TXT_PAGE_COMMAND"),
        reply: csf_units(process_assets, "TXT_REPLY_COMMAND"),
        reply_short: csf_units(process_assets, "TXT_REPLY_COMMAND_SHORT"),
        online: csf_units(process_assets, "TXT_ONLINE_COMMAND"),
    };
    let last_pager = crate::app::shell_wol::last_pager(frontend.wol.as_ref()).map(str::to_owned);
    let nickname = local_nickname(&match_state.session);
    let line = wol_chat::parse(&text, &commands, last_pager.is_some());
    let echo = match &line {
        WolChatLine::Notice(key) => {
            message(renderer, frontend, match_state, process_assets, key, &[]);
            return;
        }
        // 0078A470 prints nothing locally on success.
        WolChatLine::Locate { .. } => None,
        WolChatLine::Say(text) => Some((wol_chat::say_echo(&nickname, text), wol_chat::SAY_COLOR)),
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
    };
    crate::app::shell_wol::chat_sent(frontend.wol.as_mut(), &line);
    queue(
        &mut frontend.network_setup_protocol,
        NetworkSetupRequest::WolChat(line),
    );
    if let Some((text, color)) = echo {
        crate::app::shell_lan::append_room_chat(
            renderer,
            frontend,
            match_state,
            process_assets,
            LanChatLine { text, color },
        );
    }
}

/// C2/C9 0x3ED (`0x791110`): the edit is cleared first; outside a channel
/// `TXT_NOT_IN_CHAN`, an empty line `TXT_ENTER_MESSAGE`, else an action.
pub(super) fn emote(
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
) {
    if frontend.multiplayer_setup.is_none() {
        return;
    }
    let units: Vec<u16> = crate::app::input::multiplayer_setup::chat_text(frontend, match_state)
        .encode_utf16()
        .collect();
    let text = wol_chat::trim_line_end(&units).to_vec();
    let page = super::dialog(frontend, match_state).page().dialog;
    let chat = crate::ui::multiplayer_setup::CHAT;
    frontend.shell_controller.set_text(page, chat, "");
    if text.is_empty() {
        message(
            renderer,
            frontend,
            match_state,
            process_assets,
            "TXT_ENTER_MESSAGE",
            &[],
        );
        return;
    }
    let nickname = local_nickname(&match_state.session);
    crate::app::shell_lan::append_room_chat(
        renderer,
        frontend,
        match_state,
        process_assets,
        LanChatLine {
            text: wol_chat::emote_echo(&nickname, &text),
            color: wol_chat::EMOTE_COLOR,
        },
    );
    queue(
        &mut frontend.network_setup_protocol,
        NetworkSetupRequest::WolEmote(text),
    );
}

fn csf_units(process_assets: &ProcessAssets, key: &str) -> Vec<u16> {
    process_assets.csf_label(key, "").encode_utf16().collect()
}

/// `00B73E40` is the login nickname, which the login made the MultiPlayer
/// handle and therefore the local participant's name.
fn local_nickname(session: &MatchSession) -> String {
    session.local_player_name().unwrap_or_default().to_owned()
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    #[test]
    fn balance_moves_seated_players_only_onto_listed_teams() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let mut launch: SkirmishLaunchSession =
            serde_json::from_value(fixture["launch"].clone()).unwrap();
        let player = launch.humans[0].clone();
        launch.humans = ["Host", "Guest", "Watcher", "Gone"]
            .into_iter()
            .enumerate()
            .map(|(row, name)| {
                let mut human = player.clone();
                human.name = name.into();
                human.raw_slot_index = Some(row as u8);
                human.slot.team = LaunchTeam::Team(0);
                human
            })
            .collect();
        launch.humans[2].set_observer(true);
        launch.humans[3].raw_slot_index = None;
        let teams = [
            ("host", 1),
            ("Guest", 0),
            ("Watcher", 1),
            ("Gone", 1),
            ("Nobody", 1),
        ];
        let teams: Vec<_> = teams.map(|(name, team)| (name.to_owned(), team)).into();
        assert_eq!(team_moves(&launch, &teams, 4), [(0, 1)]);
        assert_eq!(team_moves(&launch, &[("Guest".into(), 4)], 4), []);
        // A room with Teams set takes its own teams only.
        assert_eq!(team_moves(&launch, &[("Guest".into(), 2)], 3), [(1, 2)]);
        assert_eq!(team_moves(&launch, &[("Guest".into(), 2)], 2), []);
    }
}
