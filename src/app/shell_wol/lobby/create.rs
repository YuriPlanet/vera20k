//! Create (`0x00789B60`): the create dialog's commit, the created channel
//! record and its ExInfo, a room's republished limits, and the leave before
//! a create.

use crate::app::shell_wol::service::WolServiceRequest;
use crate::app::{App, AppState};
use crate::ui::wol_lobby::*;
use crate::util::time::Instant;

use super::{
    AfterLeave, CREATE_MAX_USERS, HOST_ROOM_LINE, LEAVE_WAIT, Step, buddy_runner, commit_hide,
    lobby_mut, local_ini_hash, message_line, modern_lobby, nickname, options_screen, request,
    retext_wait, show_wait,
};

/// Create's slide has run: `UI__BeginDialog(0xC2)` over a hidden `0x113`.
pub(crate) fn commit_create(state: &mut AppState, observe: bool) {
    commit_hide(state);
    if App::open_host_setup(state, observe) && modern_lobby(state) {
        message_line(state, crate::i18n::tr(HOST_ROOM_LINE));
    }
}

/// API+0x1C, then `GUI:CreatingChannel` in a box created with flag 1 whose
/// modal loop waits for the result. The session preparation `0x00789B60`
/// runs before it (GameMode options, the host node, `[0x8316A4]`) belongs
/// to the setup owner that creates C2 ([`App::open_host_setup`]).
pub(super) fn send_create(
    state: &mut AppState,
    channel: WolChannel,
    key: String,
    observe: bool,
    ranked: bool,
) {
    // 0x00789F55..0x00789FB4: with registry Options bit 1 clear (VERA reads
    // no registry), the nickname's persona Country and Color become the
    // MultiPlayer picks the host node then takes (0x0069B700/0x0069B720).
    let nickname = nickname(state);
    super::persona::take_side_and_color(state, &nickname);
    if let Some(lobby) = lobby_mut(state) {
        lobby.step = Step::Creating {
            observe,
            channel: channel.clone(),
            request: None,
        };
    }
    request(
        state,
        WolServiceRequest::CreateChannel {
            channel,
            key,
            ranked,
        },
    );
    show_wait(state, "GUI:CreatingChannel");
}

/// `0x00789B60`'s record (`0x10C` bytes): type `0x29`, `"%s's_game"` of the
/// nickname, [`CREATE_MAX_USERS`] users, the 0x629 byte (absent, 0) as
/// tournament, the clan and resolution bits plus the chat-lobby number
/// `[0xB77DD4]` as game flags. `width` is Options' ScreenWidth
/// `[0xA8EB84]`; `ex_info` is `0x0077E7A0`'s text (`0x00789D05`).
/// `+0x04 = 1` has no field here.
pub(super) fn created_channel(
    nickname: &str,
    clan: bool,
    limit: bool,
    width: i32,
    lobby_number: i32,
    ex_info: String,
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
        ex_info,
        ..Default::default()
    }
}

/// `0x0077E7A0`: the hash, then `0x005E94D0`/`0x005E9580`/`!0x005E9560`
/// over the slot array `[0xA8DA90]`, and the scenario `[0xA8B8E0]` (the
/// record at `[0xA8B254]`, as `SelectScenario` `0x005E7BF0` copies it). At
/// Create the WOL entry (`0x00786390` -> `0x005E8B60`) has filled every slot
/// with the Open or Closed sentinel, and no setup exists yet: no AI, no
/// observer and no open observer slot.
/// RESIDUAL: a setup earlier in the visit leaves its own slots, which
/// native's Create publishes instead.
pub(super) fn ex_info(state: &AppState) -> String {
    let index = state.frontend.multiplayer_shell_runtime.scenario_index();
    let scenario = state
        .frontend
        .scenario_catalog
        .records()
        .get(index)
        .map(|record| record.file_name.clone())
        .unwrap_or_default();
    let (ai_players, observer_taken, observer_open) = slot_ex_info(&state.match_state.session);
    ExInfo {
        version: EX_INFO_VERSION,
        hash: local_ini_hash(state).unwrap_or(0),
        ai_players,
        observer_taken,
        observer_open,
        scenario,
    }
    .write()
}

/// The slot words of [`ex_info`]: the AI rows (`0x005E94D0`), a human
/// observer on the observer row `[0x8316A4]` (`0x005E9580`) and an open
/// observer row (`!0x005E9560`). A VERA room with several observers counts
/// its observers and its open observer rows instead.
pub(super) fn slot_ex_info(
    session: &crate::app::match_runtime::session::MatchSession,
) -> (i32, i32, i32) {
    use crate::app::match_runtime::session::NetworkVacancy;
    let Some(launch) = session.network_setup() else {
        return (0, 0, 0);
    };
    let rows = 0..crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as u8;
    let ai = rows
        .clone()
        .filter(|row| session.network_display_ai(*row).is_some())
        .count();
    let open = rows
        .clone()
        .filter(|row| session.network_vacancy(*row) == Some(NetworkVacancy::OpenObserver))
        .count();
    let observers = launch.humans.iter().filter(|human| human.is_observer());
    if session.network_several_observers() {
        return (ai as i32, observers.count() as i32, open as i32);
    }
    let observer_row = session.network_observer_row();
    let taken = observer_row.is_some()
        && observers
            .clone()
            .any(|human| human.raw_slot_index == observer_row);
    (ai as i32, i32::from(taken), i32::from(open != 0))
}

/// `0x0078F000`'s channel limit (`"mode #%s +l %d"`, API+0x9C): the host,
/// then over the setup's rows (`0x005E64C0`, one more with the observer
/// preference `[0xAC10C8]` at 1) every human and every Open or open
/// observer row, and past them every human and open observer row.
pub(super) fn republished_limit(
    session: &crate::app::match_runtime::session::MatchSession,
    observer_preference: bool,
) -> Option<i32> {
    use crate::app::match_runtime::session::NetworkVacancy;
    let launch = session.network_setup()?;
    let rows = session.network_setup_capacity()? + i32::from(observer_preference);
    let mut limit = 1;
    for row in 1..crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as u8 {
        let human = launch
            .humans
            .iter()
            .any(|human| human.raw_slot_index == Some(row));
        let counted = match session.network_vacancy(row) {
            Some(NetworkVacancy::OpenObserver) => true,
            Some(NetworkVacancy::Open) => i32::from(row) < rows,
            _ => human,
        };
        limit += i32::from(counted);
    }
    Some(limit)
}

/// `0x0078F000`, the host's republish after its setup changed: the channel
/// limit and a fresh exInfo (API+0xA8) replace the Create record's. VERA's
/// room gives the most people it takes instead
/// (`MatchSession::network_people_capacity`), which VERA's lobby lists.
/// RESIDUAL: API+0xFC's count (the limit plus the AI rows,
/// `0x0078F1D9..0x0078F1E5`) has no field in VERA's channel record, so a
/// native room's list adds its AI to the count but not to the capacity.
pub(crate) fn republish_channel(state: &mut AppState) -> Option<WolChannel> {
    let observer = state
        .frontend
        .multiplayer_shell_runtime
        .lan_create_observer();
    let session = &state.match_state.session;
    let max_users = session
        .network_people_capacity()
        .or_else(|| republished_limit(session, observer))?;
    let ex_info = ex_info(state);
    let current = lobby_mut(state)?.current.as_mut()?;
    current.max_users = max_users;
    current.ex_info = ex_info;
    Some(current.clone())
}

/// `0x00789B60` after `0xC3` OK.
pub(super) fn create_confirmed(
    state: &mut AppState,
    password: String,
    name: Option<String>,
    clan: bool,
    limit: bool,
    observe: bool,
    ranked: bool,
) {
    let nickname = nickname(state);
    let (width, _) = options_screen(state);
    let globals = &mut state.frontend.wol_lobby_globals;
    globals.limit_resolution = limit;
    let lobby_number = globals.lobby_number;
    let ex_info = ex_info(state);
    let mut channel = created_channel(&nickname, clan, limit, width, lobby_number, ex_info);
    // VERA's online lobby names the room as its host typed.
    if let Some(name) = name {
        channel.name = name;
    }
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
    lobby.ui.close_popup();
    lobby.ui.set_observer(observe);
    leave_then_create(state, channel, key, observe, ranked);
}

/// `0x00789B60`'s leave and create: API+0x24 runs unconditionally, and
/// once more with the wait (a `0x792BE0(0)` retext: no box is created)
/// while still joined.
/// `ranked` false asks VERA's lobby to keep the room's games unranked.
pub(super) fn leave_then_create(
    state: &mut AppState,
    channel: WolChannel,
    key: String,
    observe: bool,
    ranked: bool,
) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let joined = lobby.current.is_some();
    if joined {
        lobby.step = Step::Leaving {
            deadline: Instant::now() + LEAVE_WAIT,
            then: AfterLeave::Create {
                channel: channel.clone(),
                key: key.clone(),
                observe,
                ranked,
            },
        };
    }
    request(state, WolServiceRequest::LeaveChannel);
    if joined {
        request(state, WolServiceRequest::LeaveChannel);
        retext_wait(state, "WOL:LeavingChannel");
    } else {
        send_create(state, channel, key, observe, ranked);
    }
}

/// Create ended without C2: the commands back with Join disabled
/// (`0x0078AC10`'s 0x62B tail), focus on the chat edit. The Buddy List
/// runner takes its stop path back to `0x116`.
pub(super) fn create_ended(state: &mut AppState) {
    if buddy_runner(state) {
        super::buddy::stopped(state);
        return;
    }
    if let Some(lobby) = lobby_mut(state) {
        lobby.covered = false;
        lobby.step = Step::Ready;
        lobby.ui.set_commands(true, false, false);
    }
    state
        .frontend
        .shell_controller
        .reset_to(super::WOL_LOBBY_DIALOG);
}
