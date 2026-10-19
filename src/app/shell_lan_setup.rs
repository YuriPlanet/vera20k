//! LAN setup lifetime (BC005DE520 / BD005E1CA0) and service handoff.
//! MatchSession owns accepted setup/participants/seed. This adapter owns
//! outstanding shell requests and the dialog's temporary input/prompt state.

mod actions;
mod service;
pub(crate) use service::{LanSetupEvent, poll, service_event};
mod view;
pub(crate) use actions::{apply_action, commit_map_selection, commit_start, open_choose_map};
pub(crate) use view::view;

use super::{App, AppState};
use crate::app::frontend::shell_transition;
use crate::app::match_runtime::session::NetworkVacancy;
use crate::app::shell_lan::LanJoinAcceptance;
use crate::skirmish_launch::{SkirmishHumanSlot, SkirmishLaunchSession};
use crate::ui::multiplayer_setup::{GUEST_PAGE, HOST_PAGE, MultiplayerSetupState};
use std::time::Instant;

/// Requests are service-boundary observations. They never create a peer,
/// acknowledge readiness or complete GO without an admitted response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LanSetupRequest {
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
}

pub(crate) struct LanSetupProtocol {
    locked: bool,
    start_requested: bool,
    accept_eligible: bool,
    go_deadline: Option<Instant>,
    map_transfer_pending: bool,
    transfer_progress: Option<(u32, u32)>,
    return_after_notice: bool,
    requests: Vec<LanSetupRequest>,
}

impl LanSetupProtocol {
    fn new(host: bool) -> Self {
        Self {
            locked: false,
            start_requested: false,
            accept_eligible: false,
            go_deadline: None,
            map_transfer_pending: false,
            transfer_progress: None,
            return_after_notice: false,
            requests: if host {
                vec![LanSetupRequest::PublishOptions]
            } else {
                Vec::new()
            },
        }
    }

    pub(crate) fn take_service_requests(&mut self) -> Vec<LanSetupRequest> {
        std::mem::take(&mut self.requests)
    }

    /// Presentation of the active service operation, never map ownership or
    /// transfer transport. Completion is a separate admitted service result.
    pub(crate) fn transfer_progress(&self) -> Option<(u32, u32)> {
        self.transfer_progress
    }
}

impl App {
    /// BC creation: restore the separate LAN setup and consume Main -> CRT
    /// seed initialization at005DEC3B..5C. Assignment still uses the retained
    /// Scenario cursor; gameplay reseeds both streams later at0052E619.
    pub(crate) fn open_lan_setup(state: &mut AppState, observe: bool) {
        let result = state.frontend.multiplayer_shell_runtime.new_lan_setup(
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
                log::error!("Could not restore LAN setup: {error:#}");
                Self::open_lan_lobby(state);
                return;
            }
        };
        if let Err(error) = install_setup(
            state,
            launch,
            true,
            vacancies,
            Some(if observe { 0 } else { 7 }),
        ) {
            log::error!("Could not admit local LAN setup: {error:#}");
            Self::open_lan_lobby(state);
            return;
        }
        let main = crate::app::state::process_main_draws(
            state.match_state.sim_runtime.as_mut(),
            &mut state.frontend.frontend_main_rng,
        );
        let agreed_seed = initialize_host_seed(&main, &mut state.frontend.legacy_crt_rng);
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
            false,
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
    host: bool,
    vacancies: [NetworkVacancy; 8],
    observer_row: Option<u8>,
) -> anyhow::Result<()> {
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
    state.frontend.lan_setup_protocol = Some(LanSetupProtocol::new(host));
    state.frontend.shell_route = crate::app::shell_route::ShellRoute::LanSetup;
    state.frontend.shell_controller.reset_to(
        if host {
            HOST_PAGE.dialog
        } else {
            GUEST_PAGE.dialog
        },
        false,
    );
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
    let session = &state.match_state.session;
    if session.host()
        && let Some(launch) = session.network_setup()
    {
        let vacancies = std::array::from_fn(|row| session.network_vacancy(row as u8));
        let choices = std::array::from_fn(|row| session.network_row_choices(row as u8));
        state.frontend.multiplayer_shell_runtime.remember_lan_setup(
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
        state.frontend.multiplayer_shell_runtime.persist_lan_setup();
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

/// BC/BD Back returns to a newly constructed BB, leaving the process
/// preference/RNG owners intact and clearing the admitted setup through owner.
pub(crate) fn commit_back(state: &mut AppState) {
    if state
        .frontend
        .lan_setup_protocol
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
    state.frontend.lan_setup_protocol = None;
    state.frontend.skirmish_shell_state.choose_map_modal = None;
    state.frontend.skirmish_preview_texture = None;
    state.frontend.loading_screen_atlas = None;
    state.match_state.session.return_to_network_lobby();
    App::open_lan_lobby(state);
}

/// The shared CE owner admitted its OK/default key.
pub(crate) fn acknowledge_notice(state: &mut AppState) {
    if let Some(ui) = state.frontend.multiplayer_setup.as_mut() {
        ui.notice = None;
    }
    state.frontend.shell_controller.pop();
    if state
        .frontend
        .lan_setup_protocol
        .as_ref()
        .is_some_and(|protocol| protocol.return_after_notice)
    {
        App::leave_shell_dialog(
            state,
            shell_transition::ShellExitThen::LanSetupBack {
                host: state.match_state.session.host(),
            },
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
