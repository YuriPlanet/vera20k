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
pub(super) fn resident_seed(state: &AppState) -> u32 {
    state
        .match_state
        .sim_runtime
        .as_ref()
        .map_or(state.frontend.startup_rng_seed, |runtime| {
            runtime.simulation.session.seed() as u32
        })
}

impl App {
    /// The chat join sink `007A6100` creates C9 (`0x7A6256`) for a joined
    /// game channel. Like BD, the host's options arrive later; unlike BD,
    /// C9 never disables Accept and the sink's `005E9850` enables it.
    pub(crate) fn open_wol_joined_setup(
        state: &mut AppState,
        local: crate::skirmish_launch::SkirmishHumanSlot,
        observer_row: Option<u8>,
    ) {
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
            [NetworkVacancy::Open; 8],
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
    state.match_state.session.clear_wol_guest_readiness()?;
    let host = state.match_state.session.host();
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut()
        && !host
    {
        protocol.accept_eligible = true;
    }
    Ok(())
}

/// `CChatEventSink+0x5C` OnGameStart (`0x7A7C80`) for host and guest. A
/// user list that differs from the local one clears the pending start (the
/// host may Start again); a match closes the setup and runs Start Game Now
/// (`0x794BA0`), whose timing block decides MaxAhead.
pub(super) fn game_started(
    state: &mut AppState,
    users: Vec<WolStartUser>,
    pings: [i32; 8],
) -> anyhow::Result<()> {
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
    // 0x794F6F..: the first user row with flag bit0 is the channel owner.
    let channel_owner = users
        .iter()
        .find(|user| user.flags & 1 != 0)
        .is_some_and(|user| user.name.eq_ignore_ascii_case(&local));
    state
        .match_state
        .session
        .prepare_wol_timing(pings, channel_owner)?;
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
    protocol.locked = true;
    protocol.wol_start_pending = false;
    protocol.ping_deadline = None;
    // Start Game Now leaves the game channel (API+0x24 at 0x7955F1).
    protocol.requests.push(NetworkSetupRequest::Leave);
    state.frontend.skirmish_preview_texture = None;
    App::leave_shell_dialog(
        state,
        shell_transition::ShellExitThen::NetworkStart {
            dialog: super::dialog(state),
            agreed_seed: seed,
        },
    );
    Ok(())
}

/// C2/C9 0x540 Enter (notify 0x501): commands first, then a plain line.
pub(super) fn chat(state: &mut AppState, text: Vec<u16>) {
    // 5EDD00's TrimRight("\n\r") result is what WOL sends.
    let text = wol_chat::trim_line_end(&text).to_vec();
    if text.is_empty() {
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
