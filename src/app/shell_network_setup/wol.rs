//! WOL-only parts of the setup dialogs C2 (0078F7F0) and C9 (00791B10).
//! Everything shared with BC/BD stays in the parent owner; this module holds
//! the StartGame/OnGameStart handshake, the WOL chat line, the host ping
//! timer and the readiness clearing native WOL performs on option edits.

use super::chat_notice as message;
use super::*;
use crate::ui::lan_shell::LanChatLine;
use crate::ui::wol_chat;
use std::time::Duration;

/// `SetTimer(hDlg, 0x14AF8, 5000)` at `0x7914FF`.
pub(super) const PING_PERIOD: Duration = Duration::from_millis(5000);

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
pub(crate) fn resident_seed(state: &AppState) -> u32 {
    state
        .match_state
        .sim_runtime
        .as_ref()
        .map_or(state.frontend.startup_rng_seed, |runtime| {
            runtime.simulation.session().seed() as u32
        })
}

impl App {
    /// The chat join sink `007A6100` creates C9 (`0x7A6256`) for a joined
    /// game channel with the local node's choices unset (`0x7A5890`). Like
    /// BD, the host's options arrive later; unlike BD, C9 never disables
    /// Accept and the sink's `005E9850` enables it.
    pub(crate) fn open_wol_joined_setup(
        state: &mut AppState,
        mut local: crate::skirmish_launch::SkirmishHumanSlot,
        observer_row: Option<u8>,
    ) {
        local.slot = unset_choices();
        let Some(mode) = state.frontend.skirmish_modes.first().cloned() else {
            return;
        };
        let mut launch = SkirmishLaunchSession {
            mode: crate::skirmish_launch::SkirmishLaunchMode::from_game_mode(&mode),
            selected_map_file: None,
            humans: vec![local],
            local_player_index: 0,
            opponents: Vec::new(),
            ai_player_count: 0,
            pre_fill_house_roster:
                crate::skirmish_launch::PreFillHouseRoster::from_compact_skirmish(0),
            options: state
                .frontend
                .skirmish_shell_state
                .launch_options_base
                .clone(),
        };
        launch.rebuild_house_roster();
        let dialog = SetupDialog {
            host: false,
            flavor: SetupFlavor::Wol,
        };
        if let Err(error) = install_setup(
            state,
            launch,
            dialog,
            [NetworkVacancy::Open; crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT],
            observer_row,
        ) {
            log::error!("Could not admit WOL join: {error:#}");
            open_service_lobby(state, dialog);
            return;
        }
        if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
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
pub(crate) fn guest_users_listed(state: &mut AppState, users: &[(String, bool)]) -> bool {
    for (name, _) in users {
        let human = crate::skirmish_launch::SkirmishHumanSlot {
            raw_slot_index: None,
            name: name.clone(),
            slot: unset_choices(),
            role: Default::default(),
        };
        if let Err(error) = state.match_state.session.add_network_participant(human) {
            log::warn!("WOL user list: {name} has no node: {error:#}");
            continue;
        }
        if let Some(protocol) = state.frontend.network_setup_protocol.as_mut()
            && protocol.wol_host.is_none()
        {
            protocol.wol_host = Some(name.clone());
        }
    }
    if !users.iter().any(|(_, operator)| *operator) {
        return false;
    }
    if let Err(error) = clear_guest_acceptance(state) {
        log::warn!("WOL user list: {error:#}");
    }
    true
}

/// The guest's first `R` to the host after the user list
/// (`0x7A7981..0x7A7AFD`): the MultiPlayer Country and Color the persona
/// just set, start and team -2; the observer preference sends the
/// observer's.
pub(crate) fn send_first_choices(state: &mut AppState) {
    let local = state.frontend.multiplayer_shell_runtime.local_participant();
    let Some(row) = state
        .match_state
        .session
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
        state,
        NetworkSetupRequest::PlayerChoices {
            row,
            slot,
            observer,
        },
    );
}

/// The join sink `0x7A6100` for another user in this game channel. While a
/// start is pending (`[0xB77DEC]`, `[0xB77E28]` or `[0xB77E27]`) the joiner
/// is kicked (API+0x54) after the PlayerJoined sound and gets no node
/// (`0x7A631E..0x7A6348`). A guest appends an unbound node. The host binds
/// it through `0x5E9240`: a player on the first Open row, an observer on
/// the first open observer row when no Open player row is left
/// (`0x5E9510`); with neither (`0x5E9560`) the joiner is kicked
/// (`0x7A6BC7`).
/// RESIDUAL: the host's node takes the mode's default country (mode
/// vt+0x28) natively; it is unset here.
pub(crate) fn user_joined(state: &mut AppState, name: String) -> anyhow::Result<()> {
    let session = &state.match_state.session;
    let pending = start_pending(state)
        || state
            .frontend
            .network_setup_protocol
            .as_ref()
            .is_some_and(|protocol| protocol.locked);
    let row = if !session.host() {
        Some(None)
    } else if session.network_no_open_player_row() {
        session.network_free_row(true).map(Some)
    } else {
        session.network_free_row(false).map(Some)
    };
    let observer = session.host() && session.network_no_open_player_row();
    let Some(row) = row.filter(|_| !pending) else {
        let sound = state
            .rules()
            .and_then(|rules| rules.general.player_joined_sound())
            .map(str::to_owned);
        App::play_shell_ui_sound_by_id(state, sound.as_deref());
        queue(state, NetworkSetupRequest::Kick { name });
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
    play_player_left(state);
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
    clear_guest_acceptance(state)?;
    let host = state.match_state.session.host();
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.record_remote_user_left(host, operator);
    }
    Ok(())
}

/// The private-options sink `0x7A8260`'s gates for a guest's `R`: nothing
/// while a WOL match runs (`[0xB77DC1]`) or a start is requested
/// (`[0xB77E28]`); the sender must be a channel user (the caller's check)
/// and own a node here. Only a host receives it.
pub(crate) fn player_choices(
    state: &mut AppState,
    name: &str,
    choices: crate::skirmish_launch::SkirmishLocalSlot,
    observer: bool,
) -> anyhow::Result<()> {
    if crate::app::shell_wol::in_match(state)
        || start_pending(state)
        || !state.match_state.session.host()
    {
        return Ok(());
    }
    let Some(index) = participant(state, name) else {
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

/// A user's `A<n>` (`0x5E3D10` arm `'A'`, `0x5E4388` → `0x77E900`): the
/// parse drops it while a WOL match runs or a start is requested
/// (`0x5E3E73..0x5E3E90`); the user's node takes the value, and its
/// accept icon shows only for 1 (`0x5EA3E0`).
pub(crate) fn player_ready(state: &mut AppState, name: &str, value: i32) -> anyhow::Result<()> {
    if crate::app::shell_wol::in_match(state) || start_pending(state) {
        return Ok(());
    }
    let result = ready_arm(state, name, value);
    if super::quick_match(state) {
        quick_match_start(state, name);
    } else {
        guest_sink_tail(state);
    }
    result
}

/// `0x0077E900`'s Quick Match tail (`0x0077EA53..0x0077EB72`): any `A<n>`
/// (`n` untested) from the node at index 1 while the channel has exactly
/// two users, on the host before its StartGame: every channel user, in
/// reverse channel order, to API+0x48, stage 9 (90 s) and `[0xB77DEC]`.
fn quick_match_start(state: &mut AppState, name: &str) {
    let session = &state.match_state.session;
    let second = session
        .network_setup()
        .and_then(|launch| launch.humans.get(1))
        .is_some_and(|human| human.name.eq_ignore_ascii_case(name));
    let users = crate::app::shell_wol::lobby::member_names(state);
    if !session.host() || !second || users.len() != 2 {
        return;
    }
    let Some(protocol) = state.frontend.network_setup_protocol.as_mut() else {
        return;
    };
    if protocol.wol_start_pending {
        return;
    }
    protocol.wol_start_pending = true;
    protocol.requests.push(NetworkSetupRequest::StartGame {
        users: users.into_iter().rev().collect(),
    });
    crate::app::shell_wol::quick_match::start_requested(state);
}

/// The Quick Match guest's options sink `+0x58` after its parse
/// (`0x007A7F72..0x007A8014`): its own node with a side and a colour, and
/// exactly two channel users, accept at once: `"A1"` channel-wide
/// (API+0x3C). The text is the host's digit-led options, and the parse
/// already dropped it while a match runs or a start is pending.
/// RESIDUAL: the channel-owner gate (`0x007931A0`, the first operator is
/// not this user) holds for every guest the matchbot names.
pub(super) fn quick_match_accept(state: &mut AppState) {
    let assigned = state
        .match_state
        .session
        .network_setup()
        .is_some_and(|launch| {
            let slot = &launch.local_player().slot;
            !slot.country_random && !slot.color_random
        });
    if !assigned || crate::app::shell_wol::lobby::member_names(state).len() != 2 {
        return;
    }
    queue(state, NetworkSetupRequest::Ready(true));
}

fn ready_arm(state: &mut AppState, name: &str, value: i32) -> anyhow::Result<()> {
    let Some(index) = participant(state, name) else {
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
    if crate::app::shell_wol::in_match(state) || start_pending(state) {
        return Ok(());
    }
    let result = match participant(state, name) {
        Some(index) => service::service_event(
            state,
            service::NetworkSetupEvent::MapStatus { index, status },
        ),
        None => Ok(()),
    };
    guest_sink_tail(state);
    result
}

/// After the arm, a guest's `+0x58` runs its preview tail on every public
/// text that passed its gates, whoever sent it (`0x007A7F14..0x007A8213`).
fn guest_sink_tail(state: &mut AppState) {
    if !state.match_state.session.host() {
        options_preview(state, false);
    }
}

/// The guest's options sink `+0x58` (`0x007A7EA0`) after its parse, for
/// each public options text: the host's options, and any user's `A` or
/// `K`. A preview hides `0x71F` and shows `0x468`
/// (`0x007A8017..0x007A8025`). Without one it asks the first channel user
/// for one (`P%s`) when the scenario is new; after 900 buckets with no
/// download running it asks again on each options text, unrestamped,
/// unless the map verifies (`0x007A802B..0x007A80D7`). A verified mixfile
/// map has its own preview and returned above (`0x007A80EF..0x007A8143`).
/// Then, unless the map is `RandMap.Sed` (`0x007A80DD..0x007A80E9`), an
/// accepted user in a tournament or clan room takes its acceptance back:
/// `A0`, and Accept disabled (`0x007A8145..0x007A8196`). The 900-bucket branch reads
/// the verification of the last map change; native verifies again
/// (`0x007A8090..0x007A80AF`), which differs only when a map arrives
/// without a change, and WOL transfers no map here.
pub(super) fn options_preview(state: &mut AppState, new_scenario: bool) {
    let now = crate::ui::wol_page::reply_window_now(
        crate::app::match_runtime::frame_pacer::wall_clock_ms(),
    ) as u32;
    let globals = &mut state.frontend.wol_lobby_globals;
    let stamp = *globals.preview_stamp.get_or_insert(now);
    let file = state
        .match_state
        .session
        .network_setup()
        .and_then(|launch| launch.selected_map_file.as_deref());
    let map_named = file.is_some_and(|file| !file.is_empty());
    let random = file
        .is_some_and(|file| file.eq_ignore_ascii_case(crate::map::skirmish_scenarios::RANDMAP_SED));
    let room = state.match_state.session.network_room();
    let local = state
        .match_state
        .session
        .network_setup()
        .map(|launch| launch.local_player_index);
    let unaccept = !random
        && room.tournament_or_clan()
        && local.is_some_and(|index| state.match_state.session.network_player_ready(index));
    let protocol = state
        .frontend
        .network_setup_protocol
        .as_mut()
        .expect("setup owns protocol");
    state.platform.window.request_redraw();
    if protocol.received_preview.is_some() || protocol.local_preview() {
        protocol.wol_no_preview = false;
        protocol.wol_preview_hidden = false;
        return;
    }
    let mut request = false;
    if new_scenario {
        request = true;
        state.frontend.wol_lobby_globals.preview_stamp = Some(now);
    } else if now.wrapping_sub(stamp) as i32 > 900 && !protocol.awaiting_preview_file {
        if !map_named || protocol.local_map_verified {
            state.frontend.wol_lobby_globals.preview_stamp = Some(now);
        } else {
            request = true;
        }
    }
    if unaccept {
        protocol.accept_eligible = false;
        protocol.requests.push(NetworkSetupRequest::Ready(false));
    }
    if request {
        protocol.requests.push(NetworkSetupRequest::RequestPreview);
    }
    if unaccept
        && let Some(index) = local
        && let Err(error) = state
            .match_state
            .session
            .set_network_player_ready(index, false)
    {
        log::warn!("WOL room unaccept: {error:#}");
    }
}

/// The host's `+0x54` arm `'P'` (`0x007A8BF3..0x007A8D85`): a channel
/// guest asks for the preview. The host that created the game answers
/// through `0x005E7EB0` while no match runs and no start is pending; the
/// scenario name after `P` is not read.
/// INFERRED: a `P` during the offer's ack wait is dropped, as LAN's wait
/// loop drops `0x1B`.
pub(crate) fn preview_request(state: &mut AppState) -> anyhow::Result<()> {
    if !state.match_state.session.host()
        || crate::app::shell_wol::in_match(state)
        || start_pending(state)
        || state
            .match_state
            .session
            .network_setup()
            .is_none_or(|launch| launch.humans.len() < 2)
        || service::preview_offer_pending(state)
    {
        return Ok(());
    }
    service::send_preview(state)
}

fn participant(state: &AppState, name: &str) -> Option<usize> {
    state
        .match_state
        .session
        .network_setup()
        .and_then(|launch| {
            // strcmpi 0x7C8D20 (0x7A8416).
            launch.humans.iter().position(|human| {
                crate::util::native_string::crt_strcmpi(&human.name, name).is_eq()
            })
        })
}

/// The kick sink `0x7AB670` for another user in this game channel: its
/// `TXT_USER_KICKED` line in C2/C9 unless `0x5E9510` finds no Open player
/// row (`0x7AB860..0x7AB8AD`), then its leave.
pub(crate) fn user_kicked(state: &mut AppState, name: &str, line: String) -> anyhow::Result<()> {
    if !state.match_state.session.network_no_open_player_row() {
        crate::app::shell_lan::append_chat(
            state,
            LanChatLine {
                text: line.encode_utf16().collect(),
                color: u32::MAX,
            },
        );
    }
    user_left(state, name, false)
}

/// The kick sink for this user: `TXT_CHANNEL_FULL` when the session is
/// over-full (`0x5E95E0`), else `TXT_YOURE_KICKED`.
pub(crate) fn kicked_key(state: &AppState) -> &'static str {
    if state.match_state.session.network_over_capacity() {
        "TXT_CHANNEL_FULL"
    } else {
        "TXT_YOURE_KICKED"
    }
}

/// `0x785740`'s local tail for C2/C9 after the lobby cleared the channel:
/// PlayerLeft (`0x785764`), then the dialog closes (`CloseAndPop`) and the
/// lobby rejoins (`0x788A30`). No box.
pub(crate) fn local_left(state: &mut AppState) {
    play_player_left(state);
    if super::quick_match(state) {
        // No dialog: the nodes go with the setup; the runner's stage
        // timeout moves on.
        super::abandon_for_wol_runner(state);
        return;
    }
    let dialog = super::dialog(state);
    App::leave_shell_dialog(
        state,
        shell_transition::ShellExitThen::NetworkSetupBack(dialog),
    );
}

/// `0x750920(Rules+0x6BC)` while in a channel (`[0xB76B10]` != 0).
pub(crate) fn play_player_left(state: &mut AppState) {
    let sound = state
        .rules()
        .and_then(|rules| rules.general.player_left_sound())
        .map(str::to_owned);
    App::play_shell_ui_sound_by_id(state, sound.as_deref());
}

fn queue(state: &mut AppState, request: NetworkSetupRequest) {
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.requests.push(request);
    }
}

pub(super) fn start_pending(state: &AppState) -> bool {
    state
        .frontend
        .network_setup_protocol
        .as_ref()
        .is_some_and(|protocol| protocol.wol_start_pending)
}

/// `005E9850`: clear every acceptance but the host's and enable Accept on C9.
pub(super) fn clear_guest_acceptance(state: &mut AppState) -> anyhow::Result<()> {
    state.match_state.session.clear_guest_readiness()?;
    let host = state.match_state.session.host();
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut()
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
    state: &mut AppState,
    users: Vec<WolStartUser>,
    pings: Vec<i32>,
) -> anyhow::Result<()> {
    // 0x7A7B95: once a start has matched ([0xB77E27]), OnGameStart does
    // nothing more.
    if verify(state).is_some() {
        return Ok(());
    }
    // 0x7A7B82..0x7A7B9E: the page reply window restarts with no time.
    let now = crate::ui::wol_page::reply_window_now(
        crate::app::match_runtime::frame_pacer::wall_clock_ms(),
    );
    state
        .frontend
        .wol_lobby_globals
        .page_reply_window
        .arm(now, 0);
    let launch = state
        .match_state
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
        if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
            protocol.wol_start_pending = false;
        }
        return Ok(());
    }
    // 0x7A7CC3..0x7A7CE9: modes 1-3 close 0x117; 0x7A7CEE..0x7A7CFC: an
    // open 0xCF closes.
    crate::app::shell_wol::quick_match::game_started(state);
    crate::app::shell_wol::page::close_page_dialog(state);
    // 0x794F6F..: the first user row with flag bit0 is the channel owner.
    let channel_owner = users
        .iter()
        .find(|user| user.flags & 1 != 0)
        .is_some_and(|user| user.name.eq_ignore_ascii_case(&local));
    state
        .match_state
        .session
        .prepare_wol_timing(pings, channel_owner)?;
    // [0xB779C4] (0x7A7D0E..0x7A7D49) and Start Game Now's clan lists
    // (0x795250..0x79546E) for the match.
    let terms = crate::app::shell_wol::lobby::start_terms(state);
    state.match_state.session.set_wol_start(terms);
    // C2/C9 stay up: Start Game Now (0x794BA0) pushes the verify 0x130
    // over them with the nodes in [0xA8DA78] order (0x794C5B).
    let names = launch_names(state);
    let protocol = state
        .frontend
        .network_setup_protocol
        .as_mut()
        .expect("setup owns protocol");
    protocol.locked = true;
    // 0x7A7E03: [0xB77E28] = 2 until the abort or the match, so packets,
    // joins and the setup's own controls stay dropped under 0x130.
    protocol.wol_start_pending = true;
    protocol.ping_deadline = None;
    protocol.wol_verify = Some(crate::ui::wol_verify::WolVerifyDialog::open(names));
    Ok(())
}

/// The local node leads `[0xA8DA78]`: the host's own, and a guest's join
/// (`0x007A61F0..0x007A620D`); `0x00792F10` skips it and addresses nodes 1
/// up.
/// RESIDUAL: a guest's peers follow the roster order; the native order of
/// a guest's other nodes is not traced.
fn launch_names(state: &AppState) -> Vec<String> {
    let Some(launch) = state.match_state.session.network_setup() else {
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
pub(crate) fn verify(state: &AppState) -> Option<&crate::ui::wol_verify::WolVerifyDialog> {
    state
        .frontend
        .network_setup_protocol
        .as_ref()?
        .wol_verify
        .as_ref()
}

fn verify_mut(state: &mut AppState) -> anyhow::Result<&mut crate::ui::wol_verify::WolVerifyDialog> {
    state
        .frontend
        .network_setup_protocol
        .as_mut()
        .and_then(|protocol| protocol.wol_verify.as_mut())
        .ok_or_else(|| anyhow::anyhow!("no connection verify is up"))
}

/// Node `node`'s box checks (`0x00794809`, `0x00793D20`).
pub(super) fn verify_connected(state: &mut AppState, node: usize) -> anyhow::Result<()> {
    verify_mut(state)?.connect(node);
    Ok(())
}

/// `0x00794D0A..`: the negotiation's result. 0 closes `0x130`
/// (`0x00794F30..0x00794F44`), leaves the game channel (API+0x24 at
/// `0x007955F1`) and starts the match; 2 opens `0xD0` with
/// `TXT_SCENARIOS_DO_NOT_MATCH` over `0x130` first; 1 and 2 abort.
pub(super) fn negotiated(state: &mut AppState, result: u8) -> anyhow::Result<()> {
    let verify = verify_mut(state)?;
    anyhow::ensure!(verify.mismatch.is_none(), "the mismatch box is up");
    match result {
        0 => {
            let seed = state
                .match_state
                .session
                .network_start_seed()
                .ok_or_else(|| anyhow::anyhow!("WOL start has no admitted seed"))?;
            let protocol = state
                .frontend
                .network_setup_protocol
                .as_mut()
                .expect("setup owns protocol");
            protocol.wol_verify = None;
            protocol.requests.push(NetworkSetupRequest::Leave);
            state.frontend.skirmish_preview_texture = None;
            if super::quick_match(state) {
                // 0x130 closes without a slide and no setup dialog exists.
                crate::app::shell_wol::quick_match::start_leave(state);
                super::commit_start(state, seed);
                return Ok(());
            }
            App::leave_shell_dialog(
                state,
                shell_transition::ShellExitThen::NetworkStart {
                    dialog: super::dialog(state),
                    agreed_seed: seed,
                },
            );
        }
        1 => crate::app::shell_wol::start_aborted(state),
        2 => {
            let message = crate::app::shell_wol::message_box(state, "TXT_SCENARIOS_DO_NOT_MATCH");
            verify_mut(state)?.mismatch = Some(message);
        }
        other => anyhow::bail!("unknown negotiation result {other}"),
    }
    Ok(())
}

/// The mismatch box's OK: its modal run (`0x007759E0`) returns and the
/// abort goes on.
pub(crate) fn mismatch_ok(state: &mut AppState) {
    if verify(state).is_some_and(|verify| verify.mismatch.is_some()) {
        crate::app::shell_wol::start_aborted(state);
    }
}

fn screen(state: &AppState) -> (i32, i32) {
    (
        state.renderer.gpu.config.width as i32,
        state.renderer.gpu.config.height as i32,
    )
}

/// `0x130`'s layout on the live screen.
pub(crate) fn verify_layout(state: &AppState) -> crate::ui::shell::modal::WolVerifyLayout {
    let (width, height) = screen(state);
    crate::ui::shell::modal::wol_verify_layout(width, height)
}

/// The mismatch box's OK under the pointer.
fn mismatch_ok_at(state: &AppState, x: i32, y: i32) -> Option<u16> {
    let (width, height) = screen(state);
    crate::ui::shell::modal::body_ok_layout(width, height)
        .ok
        .contains(x, y)
        .then_some(crate::ui::wol_shell::BOX_OK)
}

/// The pointer while `0x130` is up: the mismatch box's OK takes it when
/// the box is open; otherwise a box's 18x18 icon (`0x006163A0` toggles on
/// the press).
pub(crate) fn verify_mouse(state: &mut AppState, pressed: bool) {
    let (x, y) = state.match_state.input.cursor_px();
    let Some(verify) = verify(state) else {
        return;
    };
    if verify.mismatch.is_some() {
        let over = mismatch_ok_at(state, x, y);
        let Ok(verify) = verify_mut(state) else {
            return;
        };
        if pressed {
            verify.ok.press(over);
            if over.is_some() {
                App::play_main_menu_button_sound(state);
            }
        } else if verify.ok.release(over).is_some() {
            mismatch_ok(state);
        }
        state.platform.window.request_redraw();
        return;
    }
    let layout = verify_layout(state);
    let node = verify_mut(state)
        .ok()
        .and_then(|verify| verify.pointer_button(&layout, x, y, pressed, Instant::now()));
    if node.is_some() {
        App::play_launcher_options_cue(
            state,
            crate::ui::main_menu_dialogs::options::LauncherCue::Checkbox,
        );
    }
    state.platform.window.request_redraw();
}

pub(crate) fn verify_cursor_moved(state: &mut AppState) {
    let (x, y) = state.match_state.input.cursor_px();
    let over = mismatch_ok_at(state, x, y);
    let layout = verify_layout(state);
    if let Ok(verify) = verify_mut(state) {
        if verify.mismatch.is_some() {
            verify.ok.hovered = over;
        } else {
            verify.pointer_moved(&layout, x, y);
        }
        state.platform.window.request_redraw();
    }
}

pub(crate) fn verify_poll_scroll(state: &mut AppState) -> Option<Instant> {
    let layout = verify_layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let verify = verify_mut(state).ok()?;
    if verify.mismatch.is_some() {
        return None;
    }
    let (changed, deadline) = verify.poll_scroll(&layout, x, y, Instant::now());
    if changed {
        state.platform.window.request_redraw();
    }
    deadline
}

/// C2/C9 0x540 Enter (notify 0x501): commands first, then a plain line.
pub(super) fn chat(state: &mut AppState, text: Vec<u16>) {
    // 5EDD00's TrimRight("\n\r") result is what WOL sends.
    let text = wol_chat::trim_line_end(&text).to_vec();
    if text.is_empty() || crate::app::shell_wol::translation::command(state, &text) {
        return;
    }
    let commands = wol_chat::WolChatCommands {
        page: csf_units(state, "TXT_PAGE_COMMAND"),
        reply: csf_units(state, "TXT_REPLY_COMMAND"),
        reply_short: csf_units(state, "TXT_REPLY_COMMAND_SHORT"),
        online: csf_units(state, "TXT_ONLINE_COMMAND"),
    };
    let last_pager = crate::app::shell_wol::last_pager(state).map(str::to_owned);
    let nickname = local_nickname(state);
    let line = wol_chat::parse(&text, &commands, last_pager.is_some());
    let echo = match &line {
        WolChatLine::Notice(key) => {
            message(state, key, &[]);
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
    crate::app::shell_wol::chat_sent(state, &line);
    queue(state, NetworkSetupRequest::WolChat(line));
    if let Some((text, color)) = echo {
        crate::app::shell_lan::append_chat(state, LanChatLine { text, color });
    }
}

/// C2/C9 0x3ED (`0x791110`): the edit is cleared first; outside a channel
/// `TXT_NOT_IN_CHAN`, an empty line `TXT_ENTER_MESSAGE`, else an action.
pub(super) fn emote(state: &mut AppState) {
    let Some(ui) = state.frontend.multiplayer_setup.as_mut() else {
        return;
    };
    let text = wol_chat::trim_line_end(&ui.chat.edit.units).to_vec();
    ui.chat.edit.set_units_limited([], 0);
    ui.focus_chat();
    if text.is_empty() {
        message(state, "TXT_ENTER_MESSAGE", &[]);
        return;
    }
    let nickname = local_nickname(state);
    crate::app::shell_lan::append_chat(
        state,
        LanChatLine {
            text: wol_chat::emote_echo(&nickname, &text),
            color: wol_chat::EMOTE_COLOR,
        },
    );
    queue(state, NetworkSetupRequest::WolEmote(text));
}

fn csf_units(state: &AppState, key: &str) -> Vec<u16> {
    state
        .process_assets
        .csf
        .as_ref()
        .and_then(|csf| csf.get(key))
        .unwrap_or_default()
        .encode_utf16()
        .collect()
}

/// `00B73E40` is the login nickname, which the login made the MultiPlayer
/// handle and therefore the local participant's name.
fn local_nickname(state: &AppState) -> String {
    state
        .match_state
        .session
        .local_player_name()
        .unwrap_or_default()
        .to_owned()
}

/// Host `WM_TIMER 0x14AF8`: broadcast the ping list (`007A0F40`) and ask
/// again, unless a start is pending.
pub(super) fn poll(state: &mut AppState) -> Option<Instant> {
    let protocol = state.frontend.network_setup_protocol.as_mut()?;
    let deadline = protocol.ping_deadline?;
    let now = Instant::now();
    if now < deadline {
        return Some(deadline);
    }
    let next = now + PING_PERIOD;
    protocol.ping_deadline = Some(next);
    protocol.requests.push(NetworkSetupRequest::BroadcastPings);
    if !protocol.wol_start_pending {
        protocol.requests.push(NetworkSetupRequest::RequestPings);
    }
    Some(next)
}

/// `007A0EA0`: a ping for a node name updates that node's icon.
pub(super) fn ping(state: &mut AppState, name: String, ping: i32) -> anyhow::Result<()> {
    let protocol = state
        .frontend
        .network_setup_protocol
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("no setup protocol"))?;
    match protocol.pings.iter_mut().find(|(known, _)| *known == name) {
        Some(entry) => entry.1 = ping,
        None => protocol.pings.push((name, ping)),
    }
    Ok(())
}
