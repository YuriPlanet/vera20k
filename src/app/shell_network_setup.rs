//! Network setup lifetime and service handoff: LAN BC005DE520 /
//! BD005E1CA0 and WOL C20078F7F0 / C900791B10. MatchSession owns accepted
//! setup/participants/seed. This adapter owns outstanding shell requests and
//! the dialog's temporary input/prompt state. WOL-only behaviour lives in
//! `wol`; every shared helper branches on GameMode like the native ones.

mod actions;
mod service;
pub(crate) use service::{NetworkSetupEvent, poll, service_event};
mod view;
mod wol;
pub(crate) use actions::{apply_action, commit_map_selection, commit_start, open_choose_map};
pub(crate) use view::view;
pub(crate) use wol::WolStartUser;

use super::{App, AppState};
use crate::app::frontend::shell_transition;
use crate::app::frontend::skirmish_session::SetupProfile;
use crate::app::match_runtime::session::NetworkVacancy;
use crate::app::match_runtime::session::SessionMode;
use crate::app::shell_lan::LanJoinAcceptance;
use crate::skirmish_launch::{SkirmishHumanSlot, SkirmishLaunchSession};
use crate::ui::multiplayer_setup::{MultiplayerSetupState, SetupDialog, SetupFlavor};
use crate::ui::wol_chat::WolChatLine;
use std::time::Instant;

/// Requests are service-boundary observations. They never create a peer,
/// acknowledge readiness or complete GO without an admitted response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NetworkSetupRequest {
    PublishOptions,
    PublishRoster,
    PublishCooperativeAssignments(Vec<[[i32; 2]; 8]>),
    MapStatus(u8),
    PrepareMapForHandoff {
        received_budget: i32,
    },
    PlayerChoices {
        row: u8,
        slot: crate::skirmish_launch::SkirmishLocalSlot,
        observer: bool,
    },
    Ready(bool),
    RequestAcceptance,
    Kick {
        name: String,
    },
    Leave,
    Chat(Vec<u16>),
    Go {
        agreed_seed: u32,
        max_ahead_frames: i32,
    },
    /// WOL chat through API+0x3C/+0xE8/+0xF8/+0x60 (`0077E1D0`, `0077E300`,
    /// `0078A6B0`, `0078A8F0`, `0078A470`). Locally echoed lines are not
    /// requests. Reply targets are resolved by the service from `00B77E30`.
    WolChat(WolChatLine),
    WolEmote(Vec<u16>),
    /// C2 commit `0x790BB9`: API+0x48 StartGame with every channel user.
    StartGame {
        users: Vec<String>,
    },
    /// `0078F000`: channel mode/topic/limit advertisement of this game.
    Advertise,
    /// C2 0x497 and timer 0x14AF8: one ping request per channel user
    /// (API2+0x18, 1000 ms timeout).
    RequestPings,
    /// Host timer 0x14AF8 `007A0F40`: broadcast `"P<n>"` and the name,ping list.
    BroadcastPings,
    /// Host join sink `007A6E7D`: tell a new user which row is the observer
    /// row (`"O<digit>"`, 8 when there is none).
    ObserverSlot {
        name: String,
        row: u8,
    },
}

pub(crate) struct NetworkSetupProtocol {
    locked: bool,
    /// WOL `00B77DEC`/`00B77E28` after the host's StartGame request, until
    /// the server's OnGameStart answers or rejects it.
    wol_start_pending: bool,
    /// WOL host `SetTimer(0x14AF8, 5000)` (`0x7914FF`).
    ping_deadline: Option<Instant>,
    /// WOL `00B76440`: last ping per participant name (`007A0EA0`).
    pings: Vec<(String, i32)>,
    start_requested: bool,
    accept_eligible: bool,
    go_deadline: Option<Instant>,
    map_transfer_pending: bool,
    transfer_progress: Option<(u32, u32)>,
    return_after_notice: bool,
    requests: Vec<NetworkSetupRequest>,
}

impl NetworkSetupProtocol {
    fn new(dialog: SetupDialog) -> Self {
        let host = dialog.host;
        let wol_host = dialog.wol() && host;
        Self {
            locked: false,
            wol_start_pending: false,
            ping_deadline: wol_host.then(|| Instant::now() + wol::PING_PERIOD),
            pings: Vec::new(),
            start_requested: false,
            accept_eligible: false,
            go_deadline: None,
            map_transfer_pending: false,
            transfer_progress: None,
            return_after_notice: false,
            requests: if wol_host {
                // C2 0x497: advertisement 78F000, then one ping per user.
                vec![
                    NetworkSetupRequest::PublishOptions,
                    NetworkSetupRequest::Advertise,
                    NetworkSetupRequest::RequestPings,
                ]
            } else if host {
                vec![NetworkSetupRequest::PublishOptions]
            } else {
                Vec::new()
            },
        }
    }

    pub(crate) fn ping(&self, name: &str) -> Option<i32> {
        self.pings
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, ping)| *ping)
    }

    pub(crate) fn take_service_requests(&mut self) -> Vec<NetworkSetupRequest> {
        std::mem::take(&mut self.requests)
    }

    /// Presentation of the active service operation, never map ownership or
    /// transfer transport. Completion is a separate admitted service result.
    pub(crate) fn transfer_progress(&self) -> Option<(u32, u32)> {
        self.transfer_progress
    }
}

/// The setup dialog the admitted session shows: GameMode4 is WOL C2/C9.
/// A white (0xFFFFFFFF) CSF line in the setup chat log: Start refusals and
/// WOL chat notices.
fn chat_notice(state: &mut AppState, key: &str, args: &[crate::assets::csf_file::CsfArg]) {
    let text = crate::assets::csf_file::format_csf(&App::csf_label(state, key, key), args);
    crate::app::shell_lan::append_chat(
        state,
        crate::ui::lan_shell::LanChatLine {
            text: text.encode_utf16().collect(),
            color: u32::MAX,
        },
    );
}

pub(crate) fn dialog(state: &AppState) -> SetupDialog {
    let session = &state.match_state.session;
    SetupDialog {
        host: session.host(),
        flavor: if session.mode() == SessionMode::Wol {
            SetupFlavor::Wol
        } else {
            SetupFlavor::Lan
        },
    }
}

fn profile(dialog: SetupDialog) -> SetupProfile {
    if dialog.wol() {
        SetupProfile::Wol
    } else {
        SetupProfile::Lan
    }
}

/// Setup Back/failure returns to the service lobby that created it: a fresh
/// LAN BB, or the WOL lobby 0x113 with its channel list re-requested.
fn open_service_lobby(state: &mut AppState, dialog: SetupDialog) {
    if dialog.wol() {
        crate::app::shell_wol::open_lobby(state, false);
    } else {
        App::open_lan_lobby(state);
    }
}

impl App {
    /// BC/C2 creation: restore the service's own setup profile. LAN BC then
    /// consumes Main -> CRT seed initialization at005DEC3B..5C; C2 draws no
    /// RNG (no 65C7E0/7CB49D/7CB4AA callee in 0078F7F0). Assignment still
    /// uses the retained Scenario cursor; gameplay reseeds both streams later
    /// at0052E619.
    pub(crate) fn open_host_setup(state: &mut AppState, observe: bool) {
        let dialog = SetupDialog {
            host: true,
            ..dialog(state)
        };
        let result = state.frontend.multiplayer_shell_runtime.new_network_setup(
            profile(dialog),
            &state.frontend.scenario_catalog,
            &state.frontend.skirmish_modes,
            state
                .frontend
                .skirmish_shell_state
                .launch_options_base
                .clone(),
            state.frontend.skirmish_shell_state.trackbar_bounds,
            observe,
        );
        let (launch, vacancies) = match result {
            Ok(value) => value,
            Err(error) => {
                log::error!("Could not restore network setup: {error:#}");
                open_service_lobby(state, dialog);
                return;
            }
        };
        if let Err(error) = install_setup(
            state,
            launch,
            dialog,
            vacancies,
            Some(if observe { 0 } else { 7 }),
        ) {
            log::error!("Could not admit local network setup: {error:#}");
            open_service_lobby(state, dialog);
            return;
        }
        let agreed_seed = if dialog.wol() {
            wol::resident_seed(state)
        } else {
            let main = crate::app::state::process_main_draws(
                state.match_state.sim_runtime.as_mut(),
                &mut state.frontend.frontend_main_rng,
            );
            initialize_host_seed(&main, &mut state.frontend.legacy_crt_rng)
        };
        state
            .match_state
            .session
            .set_network_start_seed(agreed_seed)
            .expect("BC owns an editable admitted setup");
        state
            .match_state
            .session
            .set_network_player_ready(0, true)
            .expect("BC owns the local host node");
    }

    /// Packet7 closes BB before full host options arrive. Retained options
    /// supply the initial controls, but map/mode are unconfigured in BD and
    /// Accept remains disabled until a subsequent map-availability response.
    pub(crate) fn open_lan_joined_setup(state: &mut AppState, acceptance: LanJoinAcceptance) {
        let mode = state.frontend.skirmish_modes.first().cloned();
        let Some(mode) = mode else {
            return;
        };
        let mut launch = SkirmishLaunchSession {
            mode: crate::skirmish_launch::SkirmishLaunchMode::from_game_mode(&mode),
            selected_map_file: None,
            humans: vec![acceptance.local],
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
        // The packet7 peer's identity belongs to the service envelope until
        // the original type3 roster admission; it is not a fabricated node.
        if let Err(error) = install_setup(
            state,
            launch,
            SetupDialog {
                host: false,
                flavor: SetupFlavor::Lan,
            },
            [NetworkVacancy::Open; 8],
            acceptance.observer_row,
        ) {
            log::error!("Could not admit LAN join: {error:#}");
            Self::open_lan_lobby(state);
        }
    }
}

/// BC005DEC3B..61, consuming the shared Main and thread-local CRT owners.
/// Source: executed host_seed controls in tools/storage_oracle/lan_lobby.json.
fn initialize_host_seed(
    main: &crate::sim::rng::MainRngDraws,
    crt: &mut crate::util::legacy_crt_rng::LegacyCrtRng,
) -> u32 {
    crt.reseed(main.ranged(1, 32767) as u32);
    u32::from(crt.draw15())
}

fn install_setup(
    state: &mut AppState,
    launch: SkirmishLaunchSession,
    dialog: SetupDialog,
    vacancies: [NetworkVacancy; 8],
    observer_row: Option<u8>,
) -> anyhow::Result<()> {
    let host = dialog.host;
    if !host {
        // Type7 uses setters with drawflag0. BC New only reads the retained
        // preferences, including concrete values behind Random markers.
        state
            .frontend
            .multiplayer_shell_runtime
            .update_local_preferences(launch.local_player(), launch.mode.id);
    }
    state
        .match_state
        .session
        .accept_network_setup(launch, host, observer_row)?;
    if host {
        for (row, vacancy) in vacancies.into_iter().enumerate() {
            if state
                .match_state
                .session
                .network_vacancy(row as u8)
                .is_some()
            {
                state
                    .match_state
                    .session
                    .set_network_vacancy(row as u8, vacancy)?;
            }
        }
    }
    if host {
        let launch = state
            .match_state
            .session
            .network_setup()
            .expect("admitted host");
        if let Some(map) = state
            .frontend
            .scenario_catalog
            .shell_maps()
            .iter()
            .find(|map| {
                launch
                    .selected_map_file
                    .as_deref()
                    .is_some_and(|file| file.eq_ignore_ascii_case(&map.file_name))
            })
        {
            let removed = state.match_state.session.select_network_map(
                launch.mode.clone(),
                map.file_name.clone(),
                map.player_capacity,
            )?;
            debug_assert!(removed.is_empty(), "new host has no remote peers");
        }
        if let Some(file) = state
            .match_state
            .session
            .network_setup()
            .and_then(|launch| launch.selected_map_file.clone())
        {
            let metadata = local_map_metadata(state, &file)?;
            state
                .match_state
                .session
                .accept_network_map_metadata(metadata, None, true)?;
        }
    }
    state.frontend.lan = None;
    state.frontend.multiplayer_setup = Some(MultiplayerSetupState::default());
    state.frontend.network_setup_protocol = Some(NetworkSetupProtocol::new(dialog));
    state.frontend.shell_route = crate::app::shell_route::ShellRoute::NetworkSetup;
    state
        .frontend
        .shell_controller
        .reset_to(dialog.page().dialog, false);
    state.frontend.skirmish_shell_state.choose_map_modal = None;
    state.frontend.skirmish_preview_texture = None;
    shell_transition::invalidate_main_menu_dialog_instance(state);
    state.platform.window.request_redraw();
    Ok(())
}

pub(crate) fn show_statics(state: &mut AppState) {
    let Some(view) = view(state) else {
        return;
    };
    let heading = App::csf_label(state, view.page().title_key, view.page().title_key);
    if let Some(ui) = state.frontend.multiplayer_setup.as_mut() {
        ui.statics
            .show(&heading, &view.mode_label, &view.map_label, Instant::now());
        ui.shown = true;
    }
}

fn remember_setup(state: &mut AppState) {
    let profile = profile(dialog(state));
    let session = &state.match_state.session;
    if session.host()
        && let Some(launch) = session.network_setup()
    {
        let vacancies = std::array::from_fn(|row| session.network_vacancy(row as u8));
        let choices = std::array::from_fn(|row| session.network_row_choices(row as u8));
        state
            .frontend
            .multiplayer_shell_runtime
            .remember_network_setup(
                profile,
                launch,
                vacancies,
                choices,
                &state.frontend.scenario_catalog,
            );
    }
    persist_setup_preferences(state);
}

fn persist_setup_preferences(state: &mut AppState) {
    if state.match_state.session.host() {
        let profile = profile(dialog(state));
        state
            .frontend
            .multiplayer_shell_runtime
            .persist_network_setup(profile);
    } else {
        state
            .frontend
            .multiplayer_shell_runtime
            .persist_local_preferences();
    }
}

/// The native map description can come from the host when no local scenario
/// record qualifies. It belongs to the accepted session, not a phantom entry
/// in the discovery catalog (ParsePacket5E51C4..5E5409).
fn map_display_name(state: &AppState) -> Option<&str> {
    let session = &state.match_state.session;
    let file = session.network_setup()?.selected_map_file.as_deref()?;
    session
        .network_map_metadata()
        .map(|metadata| metadata.display_name.as_str())
        .or_else(|| {
            state
                .frontend
                .scenario_catalog
                .shell_maps()
                .iter()
                .find(|map| map.file_name.eq_ignore_ascii_case(file))
                .map(|map| map.display_name.as_str())
        })
}

fn local_map_metadata(
    state: &AppState,
    file: &str,
) -> anyhow::Result<crate::app::match_runtime::session::NetworkMapMetadata> {
    let config = state
        .platform
        .game_config
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("network map requires a configured retail root"))?;
    let assets = state
        .process_assets
        .manager()
        .ok_or_else(|| anyhow::anyhow!("network map requires the active retail sources"))?;
    state
        .frontend
        .scenario_catalog
        .network_map_metadata(file, &config.paths.ra2_dir, assets)
}

/// Setup Back returns to a newly constructed BB (LAN) or the WOL lobby
/// 0x113, leaving the process preference/RNG owners and the WOL login intact
/// and clearing the admitted setup through its owner.
pub(crate) fn commit_back(state: &mut AppState) {
    let dialog = leave_setup(state);
    open_service_lobby(state, dialog);
}

/// PrepareSession after a network match. LAN builds a new BB; WOL re-runs
/// the lobby runner with postmatch set (`WOL__RunMain 0x77B44B`), which
/// rejoins the previous chat lobby.
pub(crate) fn return_after_match(state: &mut AppState) {
    let dialog = leave_setup(state);
    if dialog.wol() {
        crate::app::shell_wol::open_lobby(state, true);
    } else {
        App::open_lan_lobby(state);
    }
}

fn leave_setup(state: &mut AppState) -> SetupDialog {
    let dialog = dialog(state);
    if state
        .frontend
        .network_setup_protocol
        .as_ref()
        .is_some_and(|p| !p.locked)
    {
        remember_setup(state);
    } else {
        // A completed/failed handoff already snapshotted raw choices before
        // assignment. Never replace Random markers with resolved launch data.
        persist_setup_preferences(state);
    }
    state.frontend.multiplayer_setup = None;
    state.frontend.network_setup_protocol = None;
    state.frontend.skirmish_preview_texture = None;
    state.frontend.skirmish_shell_state.choose_map_modal = None;
    state.frontend.loading_screen_atlas = None;
    state.match_state.session.return_to_network_lobby();
    dialog
}

/// The shared CE owner admitted its OK/default key.
pub(crate) fn acknowledge_notice(state: &mut AppState) {
    if let Some(ui) = state.frontend.multiplayer_setup.as_mut() {
        ui.notice = None;
    }
    state.frontend.shell_controller.pop();
    if state
        .frontend
        .network_setup_protocol
        .as_ref()
        .is_some_and(|protocol| protocol.return_after_notice)
    {
        App::leave_shell_dialog(
            state,
            shell_transition::ShellExitThen::NetworkSetupBack(dialog(state)),
        );
    }
    state.platform.window.request_redraw();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_host_entries_match_native_main_to_crt_seed_history() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/lan_lobby.json",
        ))
        .unwrap();
        for case in corpus["host_seed"].as_array().unwrap() {
            let main = crate::sim::rng::MainRng::new(case["main_seed"].as_u64().unwrap());
            let mut crt = crate::util::legacy_crt_rng::LegacyCrtRng::default();
            for entry in case["entries"].as_array().unwrap() {
                assert_eq!(
                    main.native_state_hex(),
                    entry["main_before"].as_str().unwrap()
                );
                let seed = initialize_host_seed(&main.draws(), &mut crt);
                assert_eq!(u64::from(seed), entry["transmitted_seed"].as_u64().unwrap());
                assert_eq!(
                    main.native_state_hex(),
                    entry["main_after"].as_str().unwrap()
                );
            }
        }
    }
}
