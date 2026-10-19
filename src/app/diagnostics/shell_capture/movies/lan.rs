//! Production BB/BC/BD checkpoints. These WRITES LAN/MultiPlayer preferences:
//! use an isolated RA2_DIR. Inputs enter the existing UI/service adapters;
//! MatchSession alone owns accepted participants, setup and loading admission.

use super::*;
use crate::app::input::multiplayer_setup as setup_input;
use crate::app::match_runtime::session::{
    NetworkLoadingRequest, NetworkMapMetadata, NetworkVacancy, SessionMode,
};
use crate::app::shell_lan::{self as lobby, LanJoinAcceptance, LanServiceEvent, LanServiceRequest};
use crate::app::shell_lan_setup::{self as setup, LanSetupEvent, LanSetupRequest};
use crate::skirmish_launch::*;
use crate::ui::lan_shell::{LanChatLine, LanGameRow, LanPlayerRow};
use crate::ui::shell::geom::RectPx;
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

const TRANSFER_FILE: &str = "CONTROLLED.MAP";
const TRANSFER_ROOT_MARKER: &str = ".vera20k-lan-capture-root";

/// Diagnostic file lease, never a map/catalog authority. Existing files are
/// refused; cleanup removes only the unchanged file this capture created.
struct OwnedTransferFile {
    path: PathBuf,
    length: usize,
    sha256: String,
}
impl OwnedTransferFile {
    fn remove_unchanged(&self) -> Result<()> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "owned transfer path replaced; preserving it"
        );
        let bytes = fs::read(&self.path)?;
        ensure!(
            bytes.len() == self.length && crate::util::sha256::sha256_hex(&bytes) == self.sha256,
            "owned transfer bytes changed; preserving {}",
            self.path.display()
        );
        fs::remove_file(&self.path).context("remove unchanged capture transfer file")
    }
}
impl Drop for OwnedTransferFile {
    fn drop(&mut self) {
        if let Err(error) = self.remove_unchanged() {
            log::warn!("LAN capture file cleanup: {error:#}");
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanTarget {
    Lobby,
    Chat,
    Discovery,
    Back,
    Host,
    HostSettings,
    SoloStart,
    ChooseMap,
    ChooseMapCancel,
    HostBack,
    HostMissingMap,
    HostReadiness,
    HostGo,
    GuestUnconfigured,
    Guest,
    Accepted,
    HostChange,
    ColorChange,
    Kick,
    HostLeft,
    NoMap,
    TransferEligible,
    TransferProgress,
    TransferFailure,
    TransferFailureReturn,
    GuestGo,
    TransferGo,
    HostAbortReturn,
    GuestAbortReturn,
    RemoteLoadingProgress,
    HostObserver,
    HostObserverGo,
    JoinDenied,
    JoinDeniedReturn,
    JoinTimeoutReturn,
    ParticipantLeft,
    StartPrompt,
    StartPromptAccepted,
    LoadingTimeoutReturn,
}

pub(crate) const LAN_CHECKPOINTS: &[(&str, LanTarget)] = &[
    ("lan-0xbb-steady", LanTarget::Lobby),
    ("lan-0xbb-chat", LanTarget::Chat),
    ("lan-0xbb-discovery", LanTarget::Discovery),
    ("lan-0xbb-join-denied", LanTarget::JoinDenied),
    ("lan-0xbb-join-denied-return", LanTarget::JoinDeniedReturn),
    ("lan-0xbb-join-timeout-return", LanTarget::JoinTimeoutReturn),
    ("lan-0xbb-back", LanTarget::Back),
    ("lan-0xbc-steady", LanTarget::Host),
    ("lan-0xbc-participant-left", LanTarget::ParticipantLeft),
    ("lan-0xbc-settings", LanTarget::HostSettings),
    ("lan-0xbc-solo-start", LanTarget::SoloStart),
    ("lan-0x6b-steady", LanTarget::ChooseMap),
    ("lan-0x6b-cancel", LanTarget::ChooseMapCancel),
    ("lan-0xbc-back", LanTarget::HostBack),
    ("lan-0xbc-missing-map", LanTarget::HostMissingMap),
    ("lan-0xbc-readiness", LanTarget::HostReadiness),
    ("lan-host-loading-first", LanTarget::HostGo),
    ("lan-0xbd-unconfigured", LanTarget::GuestUnconfigured),
    ("lan-0xbd-steady", LanTarget::Guest),
    ("lan-0xbd-accepted", LanTarget::Accepted),
    ("lan-0xbd-start-prompt", LanTarget::StartPrompt),
    (
        "lan-0xbd-start-prompt-accepted",
        LanTarget::StartPromptAccepted,
    ),
    ("lan-0xbd-host-change", LanTarget::HostChange),
    ("lan-0xbd-color-change", LanTarget::ColorChange),
    ("lan-0xbd-kick", LanTarget::Kick),
    ("lan-0xbd-host-left", LanTarget::HostLeft),
    ("lan-0xbd-no-map", LanTarget::NoMap),
    ("lan-0xbd-transfer-eligible", LanTarget::TransferEligible),
    ("lan-map-transfer-progress", LanTarget::TransferProgress),
    ("lan-map-transfer-failure", LanTarget::TransferFailure),
    (
        "lan-map-transfer-failure-return",
        LanTarget::TransferFailureReturn,
    ),
    ("lan-guest-loading-first", LanTarget::GuestGo),
    ("lan-transfer-loading-first", LanTarget::TransferGo),
    ("lan-host-abort-return", LanTarget::HostAbortReturn),
    (
        "lan-loading-timeout-return",
        LanTarget::LoadingTimeoutReturn,
    ),
    ("lan-0xbc-host-observer", LanTarget::HostObserver),
    ("lan-observer-loading-first", LanTarget::HostObserverGo),
    ("lan-guest-abort-return", LanTarget::GuestAbortReturn),
    (
        "lan-remote-loading-progress",
        LanTarget::RemoteLoadingProgress,
    ),
];
impl LanTarget {
    fn host_observer(self) -> bool {
        matches!(
            self,
            Self::HostObserver | Self::HostObserverGo | Self::ParticipantLeft
        )
    }
    fn missing_map(self) -> bool {
        matches!(
            self,
            Self::NoMap
                | Self::TransferEligible
                | Self::TransferProgress
                | Self::TransferFailure
                | Self::TransferFailureReturn
                | Self::TransferGo
        )
    }
    fn guest(self) -> bool {
        matches!(
            self,
            Self::GuestUnconfigured
                | Self::Guest
                | Self::Accepted
                | Self::StartPrompt
                | Self::StartPromptAccepted
                | Self::HostChange
                | Self::ColorChange
                | Self::Kick
                | Self::HostLeft
                | Self::NoMap
                | Self::TransferEligible
                | Self::TransferProgress
                | Self::TransferFailure
                | Self::TransferFailureReturn
                | Self::GuestGo
                | Self::TransferGo
                | Self::GuestAbortReturn
                | Self::RemoteLoadingProgress
                | Self::JoinDenied
                | Self::JoinDeniedReturn
                | Self::JoinTimeoutReturn
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LanPhase {
    Main,
    Lobby,
    Join,
    JoinDenied,
    JoinTimeout,
    JoinRecovered,
    Host,
    SettingsControl(u8),
    SettingsPopup {
        step: u8,
        control: u16,
        index: usize,
        value: i32,
    },
    Guest,
    Configured,
    Prompt(Instant),
    Accepted,
    HostReady,
    Chooser,
    ChooserReturn,
    Returned,
    MainReturn,
    Transfer,
    Notice,
    Loading,
    RemoteProgress,
    AbortInput,
    Leaving,
    Settle(u32),
}

pub(super) struct LanCapture {
    target: LanTarget,
    phase: LanPhase,
    route: Vec<Value>,
    inputs: Value,
    admitted: Option<Value>,
    abort_input: Option<abort_return::AbortReturnInput>,
    observed_exit_cascade: bool,
    observed_runtime: Option<Value>,
    transfer_file: Option<OwnedTransferFile>,
    delivery: Option<Value>,
    join_requested_at: Option<Instant>,
    loading_wait_reported_at: Option<Instant>,
    /// Offline 0x102 mode/map before LAN Choose Map; the LAN chooser must not
    /// write the Skirmish dialog's retained selection.
    offline_selection: Option<(i32, usize)>,
}
impl LanCapture {
    pub(super) fn new(target: LanTarget) -> Self {
        Self {
            target,
            phase: LanPhase::Main,
            route: Vec::new(),
            admitted: None,
            abort_input: None,
            observed_exit_cascade: false,
            observed_runtime: None,
            transfer_file: None,
            delivery: None,
            join_requested_at: None,
            loading_wait_reported_at: None,
            offline_selection: None,
            inputs: super::native_corpus("tools/storage_oracle/lan_shell_controls.json")
                .expect("native LAN corpus"),
        }
    }
    pub(super) fn timeout(&self) -> Duration {
        // Original peer wait is3600 wall-clock buckets (57.6s). Only this
        // diagnostic budget grows; loading retains its production timer.
        Duration::from_secs(if self.target == LanTarget::LoadingTimeoutReturn {
            120
        } else {
            60
        })
    }
    fn event(&mut self, frame: u32, action: &str) {
        log::debug!("LAN capture {:?} frame {frame}: {action}", self.target);
        self.route.push(json!({"frame":frame,"action":action}));
    }
    fn settle(&mut self, state: &mut AppState) {
        MoviesCapture::restore_neutral_pointer(state);
        if state.frontend.shell_route.lan_lobby() {
            lobby::cursor_moved(state);
        } else if state.frontend.shell_route.lan_setup() {
            setup_input::cursor_moved(state);
        }
        self.phase = LanPhase::Settle(SETTLE_FRAMES);
    }
    fn human(&self, host: bool) -> Result<SkirmishHumanSlot> {
        Ok(SkirmishHumanSlot {
            raw_slot_index: Some(if host { 0 } else { 1 }),
            name: self.inputs[if host { "host_name" } else { "peer_name" }]
                .as_str()
                .context("native name")?
                .into(),
            role: if host && self.target.host_observer() {
                LaunchHumanRole::Observer
            } else {
                LaunchHumanRole::default()
            },
            slot: SkirmishLocalSlot {
                country: LaunchCountry::America,
                country_random: false,
                color_index: self.inputs[if host { "host_color" } else { "peer_color" }]
                    .as_u64()
                    .context("native color")? as u8,
                color_random: false,
                start_position: LaunchStartPosition::Auto,
                team: LaunchTeam::None,
            },
        })
    }
    fn drain_requests(&mut self, state: &mut AppState) -> Vec<LanSetupRequest> {
        let requests = state
            .frontend
            .lan_setup_protocol
            .as_mut()
            .map(|p| p.take_service_requests())
            .unwrap_or_default();
        self.route.push(
            json!({"setup_requests":requests.iter().map(|r|format!("{r:?}")).collect::<Vec<_>>()}),
        );
        requests
    }
    fn host_options(&self, state: &AppState) -> Result<SkirmishLaunchSession> {
        let mut launch = state
            .match_state
            .session
            .network_setup()
            .context("admitted guest setup")?
            .clone();
        let values = &self.inputs["host_options"];
        let mode_id = values["mode_id"].as_i64().context("native mode")? as i32;
        let mode = state
            .frontend
            .skirmish_modes
            .iter()
            .find(|m| m.id == mode_id)
            .context("retail native mode")?;
        launch.mode = SkirmishLaunchMode::from_game_mode(mode);
        let file = if self.target.missing_map() {
            self.assert_map_absent(state)?;
            TRANSFER_FILE
        } else {
            let file = values["map"].as_str().context("native map")?;
            ensure!(
                state
                    .frontend
                    .scenario_catalog
                    .shell_maps()
                    .iter()
                    .any(|m| m.file_name.eq_ignore_ascii_case(file)),
                "native map missing from production catalog"
            );
            file
        };
        launch.selected_map_file = Some(file.into());
        // Packet7/5E00B0 constructs the guest's local node first; roster
        // packets append the host later. Preserve that admitted source order
        // while mapping full-options values by identity (5E3D10).
        self.assert_guest_order(state, true)?;
        for human in &mut launch.humans {
            let values = self.human(human.name == self.human(true)?.name)?;
            human.slot = values.slot;
        }
        launch.opponents.clear();
        launch.ai_player_count = 0;
        launch.options.starting_credits =
            values["credits"].as_i64().context("native credits")? as i32;
        launch.options.game_speed = values["speed"].as_i64().context("native speed")? as i32;
        launch.options.unit_count = values["units"].as_i64().context("native units")? as i32;
        launch.options.short_game = values["short_game"].as_bool().context("native shortgame")?;
        launch.options.super_weapons = values["super_weapons"].as_bool().context("native SW")?;
        launch.options.build_off_ally = values["build_off_ally"]
            .as_bool()
            .context("native allies")?;
        launch.options.mcv_redeploy = values["mcv_redeploy"].as_bool().context("native MCV")?;
        launch.options.crates = values["crates"].as_bool().context("native crates")?;
        launch.rebuild_house_roster();
        Ok(launch)
    }
    fn publish_setup(&self, state: &mut AppState, launch: SkirmishLaunchSession) -> Result<()> {
        let mut vacancies = [NetworkVacancy::Open; 8];
        vacancies[7] = NetworkVacancy::Closed;
        setup::service_event(
            state,
            LanSetupEvent::HostSetup {
                launch,
                vacancies,
                map_metadata: self.map_metadata()?,
                // Packet7+57 -> observer reservation8316A4/5E08C8.
                observer_row: Some(
                    self.inputs["host_options"]["observer_row"]
                        .as_u64()
                        .context("native observer row")? as u8,
                ),
                cooperative_difficulty: 0,
                seed: Some(self.seed()?),
            },
        )?;
        self.assert_guest_order(state, true)
    }
    fn assert_guest_order(&self, state: &AppState, host_admitted: bool) -> Result<()> {
        let launch = state.match_state.session.network_setup().context("BD")?;
        ensure!(
            launch.local_player_index == 0,
            "native guest local node must remain source index0"
        );
        ensure!(
            launch.humans.len() == if host_admitted { 2 } else { 1 },
            "unexpected admitted guest roster"
        );
        let local = self.human(false)?;
        ensure!(
            launch.humans[0].name == local.name
                && launch.humans[0].raw_slot_index == local.raw_slot_index,
            "packet7 must create only the local node, independently of visible row1"
        );
        if host_admitted {
            let host = self.human(true)?;
            ensure!(
                launch.humans[1].name == host.name
                    && launch.humans[1].raw_slot_index == host.raw_slot_index,
                "later host node must append at source index1 with visible row0"
            );
        }
        Ok(())
    }
    fn apply_timing(&mut self, state: &mut AppState, host: bool) -> Result<()> {
        let corpus: Value = super::native_corpus("tools/storage_oracle/lan_lobby.json")?;
        let row = corpus["timing"]
            .as_array()
            .context("native timing corpus")?
            .iter()
            .find(|row| {
                row["name"] == "p2_zero" && row["role"] == if host { "host" } else { "guest" }
            })
            .context("native zero-response timing control")?;
        let response = row["responses"][0].as_u64().context("native response")? as u32;
        let protocol = row["protocol"].as_i64().context("native protocol")? as i32;
        setup::service_event(
            state,
            LanSetupEvent::Timing {
                protocol,
                response_times: [response, response],
            },
        )?;
        ensure!(
            state.match_state.session.network_max_ahead_frames()
                == Some(row["max_ahead"].as_i64().context("native timing output")? as i32),
            "native timing control diverged"
        );
        self.route.push(json!({"timing_input": {"native_case":"p2_zero", "host":host, "protocol":protocol, "responses":[response,response]}, "final_max_ahead":state.match_state.session.network_max_ahead_frames()}));
        Ok(())
    }

    fn map_metadata(&self) -> Result<NetworkMapMetadata> {
        let values = &self.inputs["host_options"];
        Ok(NetworkMapMetadata {
            display_name: values["display_name"]
                .as_str()
                .context("native decoded map label")?
                .into(),
            player_capacity: values["capacity"]
                .as_i64()
                .context("native raw map capacity")? as i32,
            official: !self.target.missing_map() || self.target == LanTarget::NoMap,
            byte_length: values["map_bytes"]
                .as_i64()
                .context("native advertised bytes")? as i32,
            digest: values["digest"]
                .as_str()
                .context("native advertised digest")?
                .into(),
        })
    }
    fn assert_map_absent(&self, state: &AppState) -> Result<PathBuf> {
        let assets = state
            .process_assets
            .manager()
            .context("retained assets for missing-map control")?;
        let root = fs::canonicalize(assets.ra2_dir())?;
        ensure!(
            !fs::read_dir(&root)?.any(|entry| entry.is_ok_and(|entry| entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(TRANSFER_FILE))),
            "missing-map control refuses existing {TRANSFER_FILE}"
        );
        ensure!(
            assets.resolve_ref(TRANSFER_FILE).is_none(),
            "missing-map control already resolves through VFS"
        );
        ensure!(
            !state
                .frontend
                .scenario_catalog
                .shell_maps()
                .iter()
                .any(|map| map.file_name.eq_ignore_ascii_case(TRANSFER_FILE)),
            "missing-map control already exists in catalog"
        );
        Ok(root)
    }
    fn deliver_map_file(&mut self, state: &AppState) -> Result<()> {
        let root = self.assert_map_absent(state)?;
        let marker = fs::symlink_metadata(root.join(TRANSFER_ROOT_MARKER))
            .context("map delivery requires an explicitly marked isolated RA2_DIR")?;
        ensure!(
            marker.is_file()
                && !marker.file_type().is_symlink()
                && fs::read(root.join(TRANSFER_ROOT_MARKER))?
                    == b"VERA20k isolated shell capture fixture\n",
            "isolated capture marker has wrong identity"
        );
        let assets = state
            .process_assets
            .manager()
            .context("retained delivery asset source")?;
        let original = self.inputs["host_options"]["map"]
            .as_str()
            .context("native delivery source")?;
        let source = assets
            .resolve_ref(original)
            .context("original retail map bytes unavailable")?;
        let length = self.inputs["host_options"]["map_bytes"]
            .as_u64()
            .context("native map bytes")? as usize;
        let sha256 = self.inputs["transfer"]["source_sha256"]
            .as_str()
            .context("native delivered map SHA")?;
        ensure!(
            source.bytes.len() == length && crate::util::sha256::sha256_hex(source.bytes) == sha256,
            "retail delivery bytes differ from native successful transfer"
        );
        let path = root.join(TRANSFER_FILE);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        self.transfer_file = Some(OwnedTransferFile {
            path: path.clone(),
            length,
            sha256: sha256.into(),
        });
        file.write_all(source.bytes)?;
        file.flush()?;
        drop(file);
        self.delivery = Some(
            json!({"path":path,"payload_len":length,"source_sha256":sha256,
            "retail_source_archive":source.source_archive,"retail_entry_id":source.entry_id,
            "boundary":"declared successful file-delivery service input; transport excluded"}),
        );
        Ok(())
    }
    fn transferred_loader_source<'a>(
        &self,
        state: &'a AppState,
    ) -> Result<Option<&'a crate::map::source::LoadedMapSource>> {
        if self.target != LanTarget::TransferGo {
            return Ok(None);
        }
        let owned = self
            .transfer_file
            .as_ref()
            .context("transfer completion never delivered its file")?;
        ensure!(
            crate::app::loading::pump::loading_map_name(state) == Some(TRANSFER_FILE),
            "loader selected a different map token"
        );
        let source = crate::app::loading::pump::loading_map_source(state)
            .context("production loader has not parsed transferred file")?;
        let crate::map::source::LoadedMapSource::Loose {
            path,
            payload_len,
            source_sha256,
        } = source
        else {
            bail!("transferred map was not parsed from the delivered loose file")
        };
        ensure!(
            fs::canonicalize(path)? == owned.path
                && *payload_len == owned.length
                && *source_sha256 == owned.sha256,
            "production parser consumed different transferred bytes"
        );
        ensure!(
            !state
                .frontend
                .scenario_catalog
                .shell_maps()
                .iter()
                .any(|map| map.file_name.eq_ignore_ascii_case(TRANSFER_FILE)),
            "first transferred load invented a discovery catalog entry"
        );
        Ok(Some(source))
    }
    pub(super) fn cleanup(&mut self) -> Result<()> {
        if let Some(file) = self.transfer_file.as_ref() {
            file.remove_unchanged()?;
        }
        self.transfer_file = None;
        Ok(())
    }

    fn timing(&self) -> Result<i32> {
        Ok(self.inputs["max_ahead_frames"]
            .as_i64()
            .context("native GO timing")? as i32)
    }
    fn seed(&self) -> Result<u32> {
        Ok(self.inputs["host_options"]["seed"]
            .as_u64()
            .context("native seed")? as u32)
    }

    pub(super) fn after_present(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<()> {
        if frame % 120 == 0 {
            log::debug!(
                "LAN capture {:?} frame {frame}: phase={:?} rendered={rendered:?} route={:?} slide={:?} wave={} title_terminal={} main={:?}",
                self.target,
                self.phase,
                state.frontend.shell_route,
                state.frontend.shell_slide_active_shell,
                state.frontend.shell_first_paint_slide.is_some(),
                state.frontend.shell_page_title.is_terminal(),
                MainMenuCaptureSnapshot::from_state(state),
            );
        }
        ensure!(
            state.frontend.main_menu_shell_error.is_none(),
            "LAN capture encountered shell fallback"
        );
        if self.abort_return()
            && self.target != LanTarget::LoadingTimeoutReturn
            && self.phase == LanPhase::Loading
            && state.frontend.screen == GameScreen::Loading
        {
            let requests = state.match_state.session.take_network_loading_requests();
            if requests
                .iter()
                .any(|request| matches!(request, NetworkLoadingRequest::ReportProgress(100)))
            {
                let local = state
                    .match_state
                    .session
                    .network_setup()
                    .context("frozen loading roster")?
                    .local_player_index;
                let peers = state
                    .match_state
                    .session
                    .network_connected_source_indices()
                    .iter()
                    .copied()
                    .filter(|index| *index != local)
                    .collect::<Vec<_>>();
                for index in &peers {
                    ensure!(
                        crate::app::loading::pump::accept_network_loading_progress(
                            state, *index, 100,
                        ),
                        "controlled peer completion was refused"
                    );
                }
                self.route.push(json!({"frame":frame,
                    "action":"controlled remote loading completion after actual local report",
                    "sources":peers,"percent":100,"requests":requests.iter().map(|r|format!("{r:?}")).collect::<Vec<_>>()}));
            }
        }
        if self.target == LanTarget::LoadingTimeoutReturn
            && self.phase == LanPhase::Loading
            && state.frontend.screen == GameScreen::Loading
        {
            let requests = state.match_state.session.take_network_loading_requests();
            if self.loading_wait_reported_at.is_none()
                && requests
                    .iter()
                    .any(|request| matches!(request, NetworkLoadingRequest::ReportProgress(100)))
            {
                self.loading_wait_reported_at = Some(Instant::now());
                self.event(
                    frame,
                    "actual local100 reported; remote completion deliberately withheld",
                );
            }
            ensure!(
                !requests
                    .iter()
                    .any(|request| matches!(request, NetworkLoadingRequest::Disconnect { .. })),
                "timeout disconnect preceded installed runtime"
            );
        }
        match self.phase {
            LanPhase::Main
                if rendered == PresentedShell::MainMenu
                    && steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(
                        state,
                    ))? =>
            {
                App::leave_shell_dialog(
                    state,
                    crate::app::frontend::shell_transition::ShellExitThen::MainMenu(
                        crate::ui::main_menu_shell::MainMenuShellAction::Network,
                    ),
                );
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "Network must tear down MainMenu"
                );
                self.event(frame, "MainMenu Network");
                self.phase = LanPhase::Lobby;
            }
            LanPhase::Lobby if rendered == PresentedShell::LanLobby && lobby_settled(state) => {
                ensure!(
                    state.match_state.session.mode() == SessionMode::Lan
                        && state.match_state.session.network_setup().is_none(),
                    "BB acquired a competing setup"
                );
                let name = self.human(!self.target.guest())?.name;
                let rect = lobby::layout(state).name;
                lobby_click(state, rect);
                lobby::key_press(state, Some(KeyCode::Home), None);
                for _ in 0..32 {
                    lobby::key_press(state, Some(KeyCode::Delete), None);
                }
                lobby::key_press(state, None, Some(&name));
                self.event(frame, "BB native-control name input");
                // Set the real BB checkbox through its native icon hit owner.
                // This also makes repeated captures independent of saved preference.
                let want_observer = self.target.host_observer();
                let current = state.frontend.lan.as_ref().context("BB")?.lobby.observer;
                if current != want_observer
                    || (want_observer
                        && !state
                            .frontend
                            .multiplayer_shell_runtime
                            .lan_create_observer())
                {
                    let rect = lobby::layout(state).observer_toggle;
                    lobby_click(state, rect);
                    if want_observer
                        && !state
                            .frontend
                            .multiplayer_shell_runtime
                            .lan_create_observer()
                    {
                        let rect = lobby::layout(state).observer_toggle;
                        lobby_click(state, rect);
                    }
                }

                match self.target {
                    LanTarget::Lobby => self.settle(state),
                    LanTarget::Back => {
                        lobby_button(state, crate::ui::lan_shell::BACK)?;
                        // The real click completes before restoring the sealed
                        // cursor required by the returning MainMenu snapshot.
                        MoviesCapture::restore_neutral_pointer(state);
                        self.phase = LanPhase::MainReturn;
                    }
                    LanTarget::Chat => {
                        let rect = lobby::layout(state).chat;
                        lobby_click(state, rect);
                        lobby::key_press(state, None, Some("Hello"));
                        lobby::key_press(state, Some(KeyCode::Enter), None);
                        let requests = state
                            .frontend
                            .lan
                            .as_mut()
                            .context("BB")?
                            .take_service_requests();
                        ensure!(requests.iter().any(|r|matches!(r,LanServiceRequest::Chat{text,..} if text==&"Hello\r\n".encode_utf16().collect::<Vec<_>>())),"native chat Enter payload missing");
                        let incoming = "[ControlledPeer] Hello back"
                            .encode_utf16()
                            .collect::<Vec<_>>();
                        lobby::service_event(
                            state,
                            LanServiceEvent::Chat(LanChatLine {
                                text: incoming.clone(),
                                color: 0x0000_F0F0,
                            }),
                        );
                        ensure!(
                            state
                                .frontend
                                .lan
                                .as_ref()
                                .context("BB")?
                                .lobby
                                .chat
                                .lines
                                .iter()
                                .any(|line| line.text == incoming),
                            "incoming chat did not reach shared transcript"
                        );
                        self.settle(state);
                    }
                    target if target.guest() || target == LanTarget::Discovery => {
                        lobby::service_event(
                            state,
                            LanServiceEvent::Games(vec![LanGameRow {
                                id: 1,
                                name: self.human(true)?.name,
                                open: true,
                            }]),
                        );
                        let rect = lobby::layout(state).games;
                        lobby_click(state, RectPx::new(rect.x + 8, rect.y + 26, 4, 4));
                        lobby::service_event(
                            state,
                            LanServiceEvent::Players {
                                game_id: Some(1),
                                players: vec![LanPlayerRow {
                                    name: self.human(true)?.name,
                                    color: None,
                                    side_icon: None,
                                    host: true,
                                    accepted: true,
                                }],
                            },
                        );
                        ensure!(
                            state
                                .frontend
                                .lan
                                .as_ref()
                                .and_then(|r| r.lobby.selected_game())
                                .is_some_and(|g| g.id == 1),
                            "real game row selection failed"
                        );
                        if target == LanTarget::Discovery {
                            self.settle(state);
                        } else {
                            self.join_requested_at = Some(Instant::now());
                            lobby_button(state, crate::ui::lan_shell::JOIN)?;
                            self.phase = LanPhase::Join;
                        }
                    }
                    _ => {
                        lobby_button(state, crate::ui::lan_shell::CREATE)?;
                        self.phase = LanPhase::Host;
                    }
                }
            }
            LanPhase::Join => {
                let runtime = state.frontend.lan.as_mut().context("pending join BB")?;
                ensure!(
                    runtime.pending_game() == Some(1),
                    "Join was not admitted through native validation"
                );
                ensure!(
                    runtime
                        .take_service_requests()
                        .iter()
                        .any(|r| matches!(r, LanServiceRequest::Join { game_id: 1, .. })),
                    "no real join request"
                );
                if self.target == LanTarget::JoinTimeoutReturn {
                    // The ordinary poll/deadline owner runs its actual600*16ms
                    // timer. This driver neither polls it nor advances its clock.
                    self.phase = LanPhase::JoinTimeout;
                    return Ok(());
                }
                if matches!(
                    self.target,
                    LanTarget::JoinDenied | LanTarget::JoinDeniedReturn
                ) {
                    lobby::service_event(
                        state,
                        LanServiceEvent::JoinDenied {
                            game_id: 1,
                            reason: 1,
                        },
                    );
                    self.event(frame, "controlled type8 reason1 after real Join");
                    self.phase = LanPhase::JoinDenied;
                    return Ok(());
                }
                lobby::service_event(
                    state,
                    LanServiceEvent::JoinAccepted(LanJoinAcceptance {
                        game_id: 1,
                        local: self.human(false)?,
                        host: self.human(true)?,
                        observer_row: Some(
                            self.inputs["host_options"]["observer_row"]
                                .as_u64()
                                .context("native packet7 observer row")?
                                as u8,
                        ),
                    }),
                );
                self.event(frame, "controlled type7 acceptance after real Join");
                self.phase = LanPhase::Guest;
            }
            LanPhase::JoinDenied if rendered == PresentedShell::LanLobby => {
                let runtime = state.frontend.lan.as_ref().context("denied BB")?;
                ensure!(
                    runtime.pending_game().is_none()
                        && runtime.modal == Some("TXT_GAME_FULL")
                        && runtime.lobby.enabled
                        && runtime.lobby.selection_enabled,
                    "native join denial did not release pending request and open CE"
                );
                if self.target == LanTarget::JoinDenied {
                    self.settle(state);
                } else {
                    lobby::key_press(state, Some(KeyCode::Enter), None);
                    self.phase = LanPhase::JoinRecovered;
                }
            }
            LanPhase::JoinTimeout if rendered == PresentedShell::LanLobby => {
                let runtime = state.frontend.lan.as_ref().context("timed join BB")?;
                if runtime.pending_game().is_none() {
                    let elapsed = self
                        .join_requested_at
                        .context("join issue observation")?
                        .elapsed();
                    ensure!(
                        elapsed >= Duration::from_millis(600 * 16),
                        "join timer returned before its native interval"
                    );
                    ensure!(runtime.modal.is_none(), "join timeout invented a modal");
                    self.route.push(json!({"frame":frame,"action":"actual join timeout elapsed","elapsed_ms":elapsed.as_millis()}));
                    self.phase = LanPhase::JoinRecovered;
                }
            }
            LanPhase::JoinRecovered
                if rendered == PresentedShell::LanLobby && lobby_settled(state) =>
            {
                let runtime = state.frontend.lan.as_mut().context("recovered BB")?;
                ensure!(
                    runtime.pending_game().is_none()
                        && runtime.modal.is_none()
                        && runtime.lobby.enabled
                        && runtime.lobby.selection_enabled
                        && !runtime.lobby.chat.lines.is_empty()
                        && runtime
                            .lobby
                            .selected_game()
                            .is_some_and(|game| game.id == 1),
                    "join return failed to restore the same active lobby"
                );
                let requests = runtime.take_service_requests();
                ensure!(
                    requests.contains(&LanServiceRequest::Refresh),
                    "join return omitted refresh request"
                );
                if self.target == LanTarget::JoinTimeoutReturn {
                    ensure!(
                        requests.contains(&LanServiceRequest::CancelJoin { game_id: 1 }),
                        "timeout omitted CancelJoin"
                    );
                }
                ensure!(
                    state.match_state.session.network_setup().is_none()
                        && state.frontend.multiplayer_setup.is_none(),
                    "failed join created an admitted setup"
                );
                self.route.push(json!({"frame":frame,"action":"join returned to enabled BB", "requests":requests.iter().map(|r|format!("{r:?}")).collect::<Vec<_>>()}));
                self.settle(state);
            }
            LanPhase::Host
                if rendered == PresentedShell::LanSetup && setup_settled(state, true) =>
            {
                self.event(frame, "BC steady");
                if self.target.host_observer() {
                    let launch = state
                        .match_state
                        .session
                        .network_setup()
                        .context("observer BC")?;
                    ensure!(
                        launch.local_player_index == 0
                            && launch.humans[0].raw_slot_index == Some(0)
                            && launch.humans[0].is_observer()
                            && state.match_state.session.network_observer_row() == Some(0),
                        "real BB observer preference did not admit host observer source0/raw0/O0"
                    );
                }

                match self.target {
                    LanTarget::Host | LanTarget::HostObserver => self.settle(state),
                    LanTarget::HostBack => {
                        setup_button(state, crate::ui::multiplayer_setup::BACK)?;
                        self.phase = LanPhase::Returned;
                    }
                    LanTarget::ChooseMap | LanTarget::ChooseMapCancel => {
                        self.offline_selection = Some(offline_selection(state));
                        setup_button(state, crate::ui::multiplayer_setup::CHOOSE_MAP)?;
                        self.phase = LanPhase::Chooser;
                    }
                    LanTarget::HostSettings => {
                        self.drain_requests(state);
                        let before = state
                            .match_state
                            .session
                            .network_setup()
                            .context("BC")?
                            .options
                            .short_game;
                        let layout = setup_input::layout(state);
                        let icon = crate::ui::skirmish_shell::checkbox_icon_rect(
                            layout.checkboxes[0],
                            layout.page.scale,
                        );
                        setup_click(state, icon);
                        ensure!(
                            state
                                .match_state
                                .session
                                .network_setup()
                                .context("BC")?
                                .options
                                .short_game
                                != before,
                            "real checkbox did not change owner"
                        );
                        ensure!(
                            self.drain_requests(state)
                                .contains(&LanSetupRequest::PublishOptions),
                            "checkbox omitted host options publication"
                        );
                        self.event(frame, "BC Short Game checkbox icon clicked");
                        self.phase = LanPhase::SettingsControl(0);
                    }
                    LanTarget::SoloStart => {
                        setup_button(state, crate::ui::multiplayer_setup::START)?;
                        ensure!(
                            state
                                .frontend
                                .multiplayer_setup
                                .as_ref()
                                .context("BC")?
                                .notice
                                .is_none(),
                            "solo start is chat, not modal"
                        );
                        ensure!(
                            !state
                                .frontend
                                .multiplayer_setup
                                .as_ref()
                                .context("BC")?
                                .chat
                                .lines
                                .is_empty(),
                            "solo start lacks native chat warning"
                        );
                        self.settle(state);
                    }
                    _ => {
                        setup::service_event(
                            state,
                            LanSetupEvent::ParticipantJoined(self.human(false)?),
                        )?;
                        if matches!(
                            self.target,
                            LanTarget::HostObserverGo | LanTarget::ParticipantLeft
                        ) {
                            let mut second = self.human(false)?;
                            second.name = self.inputs["observer_peer"]["name"]
                                .as_str()
                                .context("declared second peer")?
                                .into();
                            second.raw_slot_index = Some(2);
                            second.slot.color_index = self.inputs["observer_peer"]["color"]
                                .as_u64()
                                .context("declared peer color")?
                                as u8;
                            setup::service_event(state, LanSetupEvent::ParticipantJoined(second))?;
                            setup::service_event(
                                state,
                                LanSetupEvent::MapStatus {
                                    index: 2,
                                    status: 1,
                                },
                            )?;
                            let launch = state
                                .match_state
                                .session
                                .network_setup()
                                .context("observer roster")?;
                            ensure!(
                                launch.humans.len() == 3
                                    && launch
                                        .humans
                                        .iter()
                                        .filter(|human| !human.is_observer())
                                        .count()
                                        == 2
                                    && launch
                                        .humans
                                        .iter()
                                        .map(|human| human.raw_slot_index)
                                        .collect::<Vec<_>>()
                                        == vec![Some(0), Some(1), Some(2)],
                                "observer source roster changed declared bindings"
                            );
                        }
                        if self.target == LanTarget::ParticipantLeft {
                            // Original5E10F2 removes the admitted source node;
                            // its raw setup row does not renumber surviving peers.
                            setup::service_event(
                                state,
                                LanSetupEvent::PlayerReady {
                                    index: 2,
                                    accepted: true,
                                },
                            )?;
                            let removed = self.human(false)?.name;
                            let before = state
                                .match_state
                                .session
                                .network_setup()
                                .context("three-node BC")?
                                .humans[2]
                                .clone();
                            setup::service_event(
                                state,
                                LanSetupEvent::ParticipantLeft { index: 1 },
                            )?;
                            let launch = state
                                .match_state
                                .session
                                .network_setup()
                                .context("BC retained")?;
                            ensure!(
                                launch.humans.len() == 2
                                    && launch.local_player_index == 0
                                    && launch.humans[1] == before
                                    && launch.house_roster_is_consistent(),
                                "departure did not compact source nodes without changing the survivor"
                            );
                            ensure!(
                                state.match_state.session.network_player_ready(1)
                                    && state.match_state.session.network_map_status(1) == Some(1),
                                "departure lost surviving peer admission state"
                            );
                            let view =
                                setup::view(state).context("BC projection after departure")?;
                            ensure!(
                                view.host
                                    // Visible vacant rows retain their country flag;
                                    // only participant status must disappear.
                                    && view.row_icons[1].as_ref().is_none_or(|icon| {
                                        !icon.host && !icon.accepted
                                    })
                                    && view.row_icons[2].as_ref().is_some_and(|icon| {
                                        !icon.host && icon.accepted
                                    })
                                    && view.combos.iter().all(|combo| combo
                                        .choices
                                        .iter()
                                        .all(|choice| choice.text != removed)),
                                "removed peer remains in the rendered BC projection"
                            );
                            ensure!(
                                state.frontend.shell_route.lan_setup()
                                    && state
                                        .frontend
                                        .multiplayer_setup
                                        .as_ref()
                                        .context("BC")?
                                        .notice
                                        .is_none(),
                                "nonhost departure left setup or invented a modal"
                            );
                            self.route.push(json!({"frame":frame,"action":"nonhost participant left",
                                "removed_source":1,"survivor_source":1,"survivor_raw_row":2,
                                "player_left_sound":state.rules().and_then(|rules|rules.general.player_left_sound())}));
                            self.settle(state);
                            return Ok(());
                        }
                        if self.target != LanTarget::HostMissingMap {
                            setup::service_event(
                                state,
                                LanSetupEvent::MapStatus {
                                    index: 1,
                                    status: 1,
                                },
                            )?;
                        }
                        setup_button(state, crate::ui::multiplayer_setup::START)?;
                        if self.target == LanTarget::HostMissingMap {
                            self.settle(state);
                        } else {
                            ensure!(
                                self.drain_requests(state)
                                    .contains(&LanSetupRequest::RequestAcceptance),
                                "host did not request readiness"
                            );
                            if self.target == LanTarget::HostReadiness {
                                self.settle(state);
                            } else {
                                self.phase = LanPhase::HostReady;
                            }
                        }
                    }
                }
            }
            LanPhase::HostReady => {
                self.apply_timing(state, true)?;
                setup::service_event(
                    state,
                    LanSetupEvent::PlayerReady {
                        index: 1,
                        accepted: true,
                    },
                )?;
                if self.target == LanTarget::HostObserverGo {
                    setup::service_event(
                        state,
                        LanSetupEvent::PlayerReady {
                            index: 2,
                            accepted: true,
                        },
                    )?;
                }
                ensure!(
                    state.frontend.screen == GameScreen::MainMenu,
                    "A1 must not auto start"
                );
                setup_button(state, crate::ui::multiplayer_setup::START)?;
                ensure!(
                    self.drain_requests(state)
                        .iter()
                        .any(|r| matches!(r, LanSetupRequest::Go { .. })),
                    "real second Start did not request GO"
                );
                if self.target == LanTarget::HostObserverGo {
                    let corpus: Value =
                        super::native_corpus("tools/storage_oracle/lan_lobby.json")?;
                    let control = corpus["assignments"]
                        .as_array()
                        .context("native assignment controls")?
                        .iter()
                        .find(|row| row["name"] == "observer_between_random_humans")
                        .context("native observer assignment")?;
                    let index = control["initial"]["humans"]
                        .as_array()
                        .context("native human inputs")?
                        .iter()
                        .position(|row| row[0] == -3)
                        .context("native observer node")?;
                    let color = control["result"]["humans"][index][2]
                        .as_u64()
                        .context("native observer color")? as u8;
                    ensure!(
                        state
                            .match_state
                            .session
                            .network_setup()
                            .context("resolved observer setup")?
                            .local_slot()
                            .color_index
                            == color,
                        "host observer assignment differs from executed native special color"
                    );
                }
                self.admitted = Some(session_receipt(state));
                setup::service_event(state, LanSetupEvent::GoCompleted)?;
                self.phase = LanPhase::Loading;
            }
            LanPhase::SettingsControl(step)
                if rendered == PresentedShell::LanSetup && setup_settled(state, true) =>
            {
                use crate::ui::multiplayer_setup::{CREDITS, SPEED, SetupColumn, UNITS};
                let view = setup::view(state).context("BC settings view")?;
                let layout = setup_input::layout(state);
                ensure!(!view.locked, "settings driver encountered locked BC");
                if step <= 4 {
                    let candidate = if step < 4 {
                        let column = [
                            SetupColumn::Country,
                            SetupColumn::Color,
                            SetupColumn::Start,
                            SetupColumn::Team,
                        ][usize::from(step)];
                        let row = state
                            .match_state
                            .session
                            .network_setup()
                            .context("BC choices")?
                            .local_player()
                            .raw_slot_index
                            .context("local setup row")?;
                        let control = column.control(usize::from(row));
                        let combo = view.combo(control).context("local combo")?;
                        ensure!(
                            combo.enabled && combo.visible,
                            "local {column:?} is disabled"
                        );
                        let (index, choice) = combo
                            .choices
                            .iter()
                            .enumerate()
                            .take(column.visible_rows())
                            .find(|(index, choice)| {
                                choice.value >= 0 && Some(*index) != combo.selected
                            })
                            .context("different concrete choice on first popup page")?;
                        Some((control, index, choice.value))
                    } else {
                        // Pick only a native-enabled Open row that actually offers Easy AI.
                        // No slot is manufactured by the diagnostic driver.
                        (0..8u8).find_map(|row| {
                            (state.match_state.session.network_vacancy(row)
                                == Some(NetworkVacancy::Open))
                            .then(|| view.combo(SetupColumn::Player.control(usize::from(row))))
                            .flatten()
                            .filter(|combo| combo.visible && combo.enabled)
                            .and_then(|combo| {
                                combo
                                    .choices
                                    .iter()
                                    .enumerate()
                                    .take(SetupColumn::Player.visible_rows())
                                    .find(|(_, choice)| choice.value == 4)
                                    .map(|(index, choice)| (combo.id, index, choice.value))
                            })
                        })
                    };
                    if let Some((control, index, value)) = candidate {
                        self.drain_requests(state);
                        let face = layout.control_rect(control).context("combo face")?;
                        let arrow = crate::ui::skirmish_shell::combo_arrow_hit_rect(
                            face,
                            layout.page.scale,
                        );
                        setup_click(state, arrow);
                        ensure!(
                            state
                                .frontend
                                .multiplayer_setup
                                .as_ref()
                                .context("BC popup")?
                                .popup
                                .control
                                == Some(control),
                            "actual combo arrow did not open its popup"
                        );
                        self.route
                            .push(json!({"frame":frame,"action":"BC combo arrow clicked",
                            "control":control,"requested_item_data":value}));
                        // Let the real popup render before its row receives the next click.
                        self.phase = LanPhase::SettingsPopup {
                            step,
                            control,
                            index,
                            value,
                        };
                    } else {
                        self.event(frame, "BC has no legal Open row offering Easy AI");
                        self.phase = LanPhase::SettingsControl(step + 1);
                    }
                } else if step < 8 {
                    let control = [SPEED, CREDITS, UNITS][usize::from(step - 5)];
                    let bar = view
                        .trackbars
                        .iter()
                        .find(|bar| bar.id == control && bar.enabled && bar.visible)
                        .context("enabled BC trackbar")?;
                    let rect = layout.control_rect(control).context("trackbar window")?;
                    let y = rect.y + rect.h / 2;
                    // Ask the shared native hit owner for a non-thumb jump. The
                    // actual mouse handler performs the change and publication.
                    let (x, expected) = (rect.x..rect.x + rect.w)
                        .find_map(|x| match bar.press(x, y, rect, layout.page.scale) {
                            crate::ui::shell::trackbar::TrackbarPress::Jump(value)
                                if value != bar.value =>
                            {
                                Some((x, value))
                            }
                            _ => None,
                        })
                        .context("trackbar has no different admitted jump")?;
                    let before = bar.value;
                    self.drain_requests(state);
                    setup_click(state, RectPx::new(x, y, 1, 1));
                    let options = &state
                        .match_state
                        .session
                        .network_setup()
                        .context("trackbar session")?
                        .options;
                    let actual = match control {
                        SPEED => crate::ui::skirmish_shell::game_speed_visual_position(
                            options.game_speed,
                        ),
                        CREDITS => options.starting_credits,
                        UNITS => options.unit_count,
                        _ => unreachable!(),
                    };
                    ensure!(
                        actual == expected && actual != before,
                        "trackbar did not change canonical options"
                    );
                    ensure!(
                        state
                            .frontend
                            .multiplayer_setup
                            .as_ref()
                            .context("BC trackbar release")?
                            .trackbar_hold
                            .is_none(),
                        "trackbar capture survived mouse release"
                    );
                    ensure!(
                        self.drain_requests(state)
                            .contains(&LanSetupRequest::PublishOptions),
                        "trackbar omitted options publication"
                    );
                    self.route
                        .push(json!({"frame":frame,"action":"BC trackbar clicked",
                        "control":control,"before":before,"after":actual,"point":[x,y]}));
                    self.phase = LanPhase::SettingsControl(step + 1);
                } else {
                    self.drain_requests(state);
                    setup_click(state, layout.chat);
                    setup_input::key_press(state, None, Some("Setup ready"));
                    setup_input::key_press(state, Some(KeyCode::Enter), None);
                    let expected = "Setup ready\r\n".encode_utf16().collect::<Vec<_>>();
                    ensure!(
                        self.drain_requests(state)
                            .contains(&LanSetupRequest::Chat(expected)),
                        "BC chat Enter omitted native CRLF service payload"
                    );
                    let chat = &state
                        .frontend
                        .multiplayer_setup
                        .as_ref()
                        .context("BC chat")?
                        .chat;
                    ensure!(
                        chat.edit.units.is_empty(),
                        "chat Enter did not clear NewEdit"
                    );
                    ensure!(
                        chat.lines.iter().any(|line| {
                            String::from_utf16_lossy(&line.text).contains("Setup ready")
                        }),
                        "BC chat did not reach its local transcript"
                    );
                    self.event(frame, "BC chat typed and Enter submitted");
                    self.settle(state);
                }
            }
            LanPhase::SettingsPopup {
                step,
                control,
                index,
                value,
            } if rendered == PresentedShell::LanSetup => {
                use crate::ui::multiplayer_setup::SetupColumn;
                let view = setup::view(state).context("presented settings popup")?;
                let layout = setup_input::layout(state);
                let ui = state.frontend.multiplayer_setup.as_ref().context("BC")?;
                ensure!(
                    ui.popup.control == Some(control),
                    "settings popup changed before presentation"
                );
                let popup = ui
                    .popup_geometry(&view, &layout)
                    .context("presented popup geometry")?;
                ensure!(
                    view.combo(control)
                        .and_then(|combo| combo.choices.get(index))
                        .map(|choice| choice.value)
                        == Some(value),
                    "popup item identity changed before click"
                );
                let row_rect = popup.row_rect(index).context("visible popup choice")?;
                setup_click(state, row_rect);
                ensure!(
                    state
                        .frontend
                        .multiplayer_setup
                        .as_ref()
                        .context("BC")?
                        .popup
                        .control
                        .is_none(),
                    "choice click did not close popup"
                );
                let (column, row) =
                    SetupColumn::decode(control).context("setup control identity")?;
                let launch = state
                    .match_state
                    .session
                    .network_setup()
                    .context("selected setup")?;
                if column == SetupColumn::Player {
                    ensure!(
                        launch
                            .opponents
                            .iter()
                            .any(|ai| ai.raw_slot_index == row as u8
                                && ai.difficulty == AiDifficulty::Easy),
                        "Open row did not become canonical Easy AI"
                    );
                    ensure!(
                        state
                            .match_state
                            .session
                            .network_vacancy(row as u8)
                            .is_none(),
                        "AI row remained vacant"
                    );
                } else {
                    let slot = state
                        .match_state
                        .session
                        .network_row_choices(row as u8)
                        .context("canonical selected row")?;
                    let matches = match column {
                        SetupColumn::Country => {
                            !slot.country_random && i32::from(slot.country.raw_index()) == value
                        }
                        SetupColumn::Color => {
                            !slot.color_random && i32::from(slot.color_index) == value
                        }
                        SetupColumn::Start => {
                            slot.start_position == LaunchStartPosition::Position(value as u8)
                        }
                        SetupColumn::Team => slot.team == LaunchTeam::Team(value as u8),
                        SetupColumn::Player => unreachable!(),
                    };
                    ensure!(
                        matches,
                        "actual popup selection did not update canonical {column:?}"
                    );
                }
                self.route.push(
                    json!({"frame":frame,"action":"presented BC popup choice clicked",
                    "control":control,"column":format!("{column:?}"),"row":row,"item_data":value,
                    "canonical_setup":launch}),
                );
                ensure!(
                    self.drain_requests(state)
                        .contains(&LanSetupRequest::PublishOptions),
                    "host combo omitted options publication"
                );
                self.phase = LanPhase::SettingsControl(step + 1);
            }
            LanPhase::Guest
                if rendered == PresentedShell::LanSetup && setup_settled(state, false) =>
            {
                self.assert_guest_order(state, false)?;
                ensure!(
                    state
                        .match_state
                        .session
                        .network_setup()
                        .context("BD")?
                        .selected_map_file
                        .is_none(),
                    "type7 invented a configured map"
                );
                if self.target == LanTarget::GuestUnconfigured {
                    self.settle(state);
                } else {
                    setup::service_event(
                        state,
                        LanSetupEvent::ParticipantJoined(self.human(true)?),
                    )?;
                    self.assert_guest_order(state, true)?;
                    let launch = self.host_options(state)?;
                    self.publish_setup(state, launch)?;
                    let status = if self.target == LanTarget::NoMap {
                        0
                    } else if matches!(
                        self.target,
                        LanTarget::TransferEligible
                            | LanTarget::TransferProgress
                            | LanTarget::TransferFailure
                            | LanTarget::TransferFailureReturn
                            | LanTarget::TransferGo
                    ) {
                        2
                    } else {
                        1
                    };
                    ensure!(
                        state.match_state.session.network_map_status(0) == Some(status),
                        "production map availability did not produce expected K{status}"
                    );
                    ensure!(
                        self.drain_requests(state)
                            .contains(&LanSetupRequest::MapStatus(status)),
                        "production map availability did not publish K{status}"
                    );
                    ensure!(
                        setup::view(state)
                            .context("configured guest view")?
                            .accept_enabled
                            == (status != 0),
                        "map availability and Accept disagree"
                    );
                    self.event(
                        frame,
                        "controlled host metadata; actual production local map availability",
                    );
                    self.phase = LanPhase::Configured;
                }
            }
            LanPhase::Configured if setup_settled(state, false) => match self.target {
                LanTarget::Guest | LanTarget::NoMap | LanTarget::TransferEligible => {
                    self.settle(state)
                }
                LanTarget::StartPrompt | LanTarget::StartPromptAccepted => {
                    self.drain_requests(state);
                    setup::service_event(state, LanSetupEvent::StartPrompt)?;
                    let view = setup::view(state).context("prompted BD")?;
                    ensure!(
                        view.start_requested
                            && view.accept_enabled
                            && !state.match_state.session.network_player_ready(0),
                        "host Start prompt changed readiness or omitted the eligible button prompt"
                    );
                    let expected =
                        App::csf_label(state, "GUI:HostGameStartJoiner", "GUI:HostGameStartJoiner");
                    let actual = state
                        .frontend
                        .multiplayer_setup
                        .as_ref()
                        .context("BD")?
                        .chat
                        .lines
                        .iter()
                        .flat_map(|line| line.text.iter().copied())
                        .collect::<Vec<_>>();
                    ensure!(
                        String::from_utf16_lossy(&actual)
                            .chars()
                            .filter(|c| !c.is_whitespace())
                            .collect::<String>()
                            .contains(
                                &expected
                                    .chars()
                                    .filter(|c| !c.is_whitespace())
                                    .collect::<String>()
                            ),
                        "host Start prompt omitted localized chat"
                    );
                    ensure!(
                        self.drain_requests(state).is_empty(),
                        "host prompt emitted readiness before Accept"
                    );
                    self.phase = LanPhase::Prompt(Instant::now());
                }
                LanTarget::Kick | LanTarget::HostLeft => {
                    setup::service_event(
                        state,
                        if self.target == LanTarget::Kick {
                            LanSetupEvent::Kicked
                        } else {
                            LanSetupEvent::HostLeft
                        },
                    )?;
                    self.phase = LanPhase::Returned;
                }
                _ => {
                    self.drain_requests(state);
                    setup_button(state, crate::ui::multiplayer_setup::ACCEPT)?;
                    ensure!(
                        self.drain_requests(state)
                            .contains(&LanSetupRequest::Ready(true)),
                        "real Accept did not emit A1"
                    );
                    self.phase = LanPhase::Accepted;
                }
            },
            LanPhase::Prompt(started)
                if rendered == PresentedShell::LanSetup
                    && setup_settled(state, false)
                    && started.elapsed() >= Duration::from_millis(1100) =>
            {
                // Let the production renderer advance its own one-second
                // native Accept flash timer; never set its private phase.
                let view = setup::view(state).context("presented prompt")?;
                ensure!(
                    view.start_requested && view.accept_enabled,
                    "prompt disappeared before presentation"
                );
                self.event(frame, "prompted BD presented with real flash timer");
                if self.target == LanTarget::StartPrompt {
                    self.settle(state);
                } else {
                    setup_button(state, crate::ui::multiplayer_setup::ACCEPT)?;
                    let requests = self.drain_requests(state);
                    ensure!(
                        requests == vec![LanSetupRequest::Ready(true)],
                        "prompted real Accept must emit exactly one A1"
                    );
                    let view = setup::view(state).context("accepted BD")?;
                    ensure!(
                        !view.start_requested && !view.accept_enabled,
                        "Accept retained prompt or remained eligible"
                    );
                    self.phase = LanPhase::Accepted;
                }
            }
            LanPhase::Accepted => {
                ensure!(
                    state.match_state.session.network_player_ready(0),
                    "A1 not owned by session"
                );
                match self.target {
                    LanTarget::Accepted | LanTarget::StartPromptAccepted => self.settle(state),
                    LanTarget::ColorChange => {
                        let mut launch = self.host_options(state)?;
                        launch.humans[1].slot.color_index = 3;
                        self.publish_setup(state, launch)?;
                        ensure!(
                            state.match_state.session.network_player_ready(0),
                            "color-only update cleared A1"
                        );
                        ensure!(
                            !self
                                .drain_requests(state)
                                .contains(&LanSetupRequest::Ready(false)),
                            "color-only update emitted A0"
                        );
                        self.settle(state);
                    }
                    LanTarget::HostChange => {
                        let mut launch = self.host_options(state)?;
                        launch.options.starting_credits = self.inputs["changed_credits"]
                            .as_i64()
                            .context("declared credits")?
                            as i32;
                        self.publish_setup(state, launch)?;
                        ensure!(
                            !state.match_state.session.network_player_ready(0),
                            "material change failed to clear readiness"
                        );
                        ensure!(
                            self.drain_requests(state)
                                .iter()
                                .filter(|request| **request == LanSetupRequest::Ready(false))
                                .count()
                                == 1,
                            "material change must emit exactly one A0"
                        );
                        self.settle(state);
                    }
                    _ => {
                        setup::service_event(
                            state,
                            LanSetupEvent::HostGo {
                                agreed_seed: self.seed()?,
                                max_ahead_frames: self.timing()?,
                            },
                        )?;
                        ensure!(
                            self.drain_requests(state).contains(
                                &LanSetupRequest::PrepareMapForHandoff {
                                    received_budget: self.timing()?
                                }
                            ),
                            "GO bypassed map service"
                        );
                        self.admitted = Some(session_receipt(state));
                        self.apply_timing(state, false)?;
                        if matches!(
                            self.target,
                            LanTarget::GuestGo
                                | LanTarget::GuestAbortReturn
                                | LanTarget::RemoteLoadingProgress
                        ) {
                            setup::service_event(state, LanSetupEvent::GoCompleted)?;
                            self.phase = LanPhase::Loading;
                        } else {
                            setup::service_event(state, LanSetupEvent::MapTransferStarted)?;
                            self.phase = LanPhase::Transfer;
                        }
                    }
                }
            }
            LanPhase::Transfer => match self.target {
                LanTarget::TransferProgress => {
                    setup::service_event(
                        state,
                        LanSetupEvent::MapTransferProgress {
                            completed: 219,
                            total: 436,
                        },
                    )?;
                    ensure!(
                        state
                            .frontend
                            .lan_setup_protocol
                            .as_ref()
                            .context("transfer owner")?
                            .transfer_progress()
                            == Some((219, 436)),
                        "transfer progress not retained"
                    );
                    self.settle(state);
                }
                LanTarget::TransferGo => {
                    self.deliver_map_file(state)?;
                    setup::service_event(
                        state,
                        LanSetupEvent::MapTransferFinished {
                            transferred_file: Some(TRANSFER_FILE.into()),
                        },
                    )?;
                    self.phase = LanPhase::Loading;
                }
                _ => {
                    setup::service_event(
                        state,
                        LanSetupEvent::MapTransferFinished {
                            transferred_file: None,
                        },
                    )?;
                    self.phase = LanPhase::Notice;
                }
            },
            LanPhase::Notice if rendered == PresentedShell::LanSetup => {
                ensure!(
                    state
                        .frontend
                        .multiplayer_setup
                        .as_ref()
                        .context("BD")?
                        .notice
                        .is_some(),
                    "failed transfer lacks CE notice"
                );
                if self.target == LanTarget::TransferFailureReturn {
                    setup_input::key_press(state, Some(KeyCode::Enter), None);
                    self.phase = LanPhase::Returned;
                } else {
                    self.settle(state);
                }
            }
            LanPhase::Chooser
                if rendered == PresentedShell::LanSetup
                    && MoviesCapture::slide_settled(state, ShellSlideKind::ChooseMap)
                    && state
                        .frontend
                        .skirmish_shell_state
                        .choose_map_modal
                        .as_ref()
                        .is_some_and(|modal| modal.statics.is_terminal()) =>
            {
                ensure!(
                    state
                        .frontend
                        .skirmish_shell_state
                        .choose_map_modal
                        .is_some(),
                    "chooser not active"
                );
                ensure!(
                    self.offline_selection == Some(offline_selection(state)),
                    "LAN Choose Map wrote the offline Skirmish selection"
                );
                if self.target == LanTarget::ChooseMap {
                    self.settle(state);
                } else {
                    state.match_state.input.cursor_x = 720.;
                    state.match_state.input.cursor_y = 556.;
                    ensure!(
                        App::handle_choose_map_modal_mouse_down(state),
                        "chooser Cancel press rejected"
                    );
                    App::handle_choose_map_modal_mouse_up(state);
                    self.phase = LanPhase::ChooserReturn;
                }
            }
            LanPhase::ChooserReturn
                if rendered == PresentedShell::LanSetup && setup_settled(state, true) =>
            {
                ensure!(
                    state
                        .frontend
                        .skirmish_shell_state
                        .choose_map_modal
                        .is_none(),
                    "chooser survived Cancel"
                );
                ensure!(
                    self.offline_selection == Some(offline_selection(state)),
                    "LAN Choose Map Cancel wrote the offline Skirmish selection"
                );
                self.settle(state);
            }
            LanPhase::Returned if rendered == PresentedShell::LanLobby && lobby_settled(state) => {
                ensure!(
                    state.frontend.multiplayer_setup.is_none()
                        && state.frontend.lan_setup_protocol.is_none()
                        && state.match_state.session.network_setup().is_none(),
                    "setup owner survived return"
                );
                let bb = &state.frontend.lan.as_ref().context("returned BB")?.lobby;
                ensure!(
                    bb.games.is_empty() && bb.chat.lines.is_empty(),
                    "returned BB retained old projection"
                );
                self.settle(state);
            }
            LanPhase::MainReturn
                if rendered == PresentedShell::MainMenu
                    && steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(
                        state,
                    ))? =>
            {
                ensure!(
                    state.frontend.lan.is_none()
                        && state.match_state.session.mode() != SessionMode::Lan,
                    "LAN survived MainMenu return"
                );
                self.settle(state);
            }
            LanPhase::Loading
                if state.frontend.screen == GameScreen::Loading
                    && self.target == LanTarget::RemoteLoadingProgress
                    && crate::app::loading::pump::next_native_loading_frame(state)
                        == Some(crate::app::loading::pump::NextLoadingFrame::First) =>
            {
                // The original renderer has now created its immutable source
                // rows. Supply only the remote service milestone, never local progress.
                ensure!(
                    crate::app::loading::pump::accept_network_loading_progress(state, 1, 50),
                    "remote loading service input refused"
                );
                ensure!(
                    !crate::app::loading::pump::accept_network_loading_progress(state, 0, 50),
                    "local loading progress accepted remote input"
                );
                self.event(
                    frame,
                    "controlled remote source1 progress50; local source0 untouched",
                );
                self.phase = LanPhase::RemoteProgress;
            }
            LanPhase::Loading
                if state.frontend.screen == GameScreen::InGame && self.abort_return() =>
            {
                ensure!(
                    state.match_state.session.mode() == SessionMode::Lan,
                    "installed LAN became offline"
                );
                if self.target == LanTarget::LoadingTimeoutReturn {
                    let elapsed = self
                        .loading_wait_reported_at
                        .context("actual local100 report before timeout")?
                        .elapsed();
                    // This receipt observes after presentation, not at the
                    // timer's bucket anchor; bound it below57s without changing
                    // the actual3600-bucket production timeout.
                    ensure!(
                        elapsed >= Duration::from_secs(57),
                        "peer wait returned before the native initial timeout"
                    );
                    let requests = state.match_state.session.take_network_loading_requests();
                    let launch = state
                        .match_state
                        .session
                        .network_setup()
                        .context("frozen launch after timeout")?;
                    let local = launch.local_player_index;
                    ensure!(
                        launch.humans.len() == 2
                            && state.match_state.session.network_connected_source_indices()
                                == [local],
                        "timeout changed historical roster or retained disconnected live membership"
                    );
                    let peer = (0..launch.humans.len())
                        .find(|index| *index != local)
                        .context("remote source")?;
                    let disconnects = requests
                        .iter()
                        .filter_map(|request| match request {
                            NetworkLoadingRequest::Disconnect {
                                source_index,
                                reason,
                            } => Some((*source_index, *reason)),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    ensure!(
                        disconnects == vec![(peer, 1)],
                        "timeout omitted or duplicated actual reason1 Disconnect"
                    );
                    let slots = crate::sim::scenario_bootstrap::normalized_launch_slots(launch);
                    let peer_slot = slots
                        .iter()
                        .find(|slot| slot.source_human_index == Some(peer))
                        .context("canonical source-to-House binding")?;
                    let local_slot = slots
                        .iter()
                        .find(|slot| slot.source_human_index == Some(local))
                        .context("canonical local binding")?;
                    let sim = &state
                        .match_state
                        .sim_runtime
                        .as_ref()
                        .context("installed timeout runtime")?
                        .simulation;
                    let owner = sim
                        .interner
                        .get(&peer_slot.owner_name)
                        .context("constructed remote House")?;
                    let local_owner = sim
                        .interner
                        .get(&local_slot.owner_name)
                        .context("constructed local House")?;
                    ensure!(
                        sim.session.tick == 0
                            && sim.session.binary_frame == 0
                            && sim
                                .houses
                                .get(&owner)
                                .is_some_and(|house| house.is_to_die())
                            && sim
                                .houses
                                .get(&local_owner)
                                .is_some_and(|house| !house.is_to_die()),
                        "timeout did not flag only the mapped remote House before first simulation tick"
                    );
                    let template =
                        App::csf_label(state, "TXT_CONNECTION_LOST", "TXT_CONNECTION_LOST");
                    let text = crate::assets::csf_file::format_csf(
                        &template,
                        &[crate::assets::csf_file::CsfArg::Str(
                            &launch.humans[peer].name,
                        )],
                    );
                    let actual = state
                        .match_state
                        .match_presentation
                        .message_list
                        .messages()
                        .iter()
                        .map(|message| message.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    ensure!(
                        actual
                            .split_whitespace()
                            .collect::<String>()
                            .contains(&text.split_whitespace().collect::<String>()),
                        "production connection-lost message missing"
                    );
                    self.route.push(json!({"frame":frame,"action":"native peer wait expired and runtime installed",
                        "elapsed_since_observed_local100_ms":elapsed.as_millis(),"historical_sources":launch.humans.len(),
                        "connected_sources":state.match_state.session.network_connected_source_indices(),
                        "disconnected_source":peer,"mapped_house":peer_slot.owner_name,"pending_flag_to_die":true,
                        "requests":requests.iter().map(|request|format!("{request:?}")).collect::<Vec<_>>(),
                        "message_rows":state.match_state.match_presentation.message_list.messages().iter().map(|message|&message.text).collect::<Vec<_>>()}));
                }
                let sim = &state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .context("installed LAN runtime")?
                    .simulation;
                ensure!(
                    sim.session.started() && sim.session.game_mode_nonzero(),
                    "LAN runtime was not admitted"
                );
                self.observed_runtime = Some(json!({"mode":"Lan", "tick":sim.session.tick,
                    "current_house":sim.session.current_house().map(|id|sim.interner.resolve(id)),
                    "map":sim.session.map_name,"host":state.match_state.session.host()}));
                self.abort_input = Some(abort_return::AbortReturnInput::begin(
                    state,
                    &mut self.route,
                    frame,
                )?);
                self.phase = LanPhase::AbortInput;
            }
            LanPhase::AbortInput => {
                if self
                    .abort_input
                    .as_mut()
                    .context("LAN abort input")?
                    .after_present(state, &mut self.route, frame)?
                {
                    self.phase = LanPhase::Leaving;
                }
            }
            LanPhase::Leaving => {
                if state.match_state.scenario_exit.is_some() && !self.observed_exit_cascade {
                    let sim = &state
                        .match_state
                        .sim_runtime
                        .as_ref()
                        .context("EXIT runtime")?
                        .simulation;
                    ensure!(
                        sim.quit_requested
                            && state.match_state.scenario_outcome.is_none()
                            && !sim
                                .pending_command_snapshot()
                                .iter()
                                .any(|command| matches!(
                                    command.payload,
                                    crate::sim::command::Command::ExitMatch
                                )),
                        "LAN return began before local queued EXIT consumed"
                    );
                    self.route.push(
                        json!({"frame":frame,"action":"EXIT consumed","tick":sim.session.tick}),
                    );
                    self.observed_exit_cascade = true;
                }
                if rendered == PresentedShell::LanLobby && lobby_settled(state) {
                    ensure!(
                        self.observed_exit_cascade,
                        "LAN returned without executed EXIT cascade"
                    );
                    ensure!(
                        state.match_state.session.mode() == SessionMode::Lan
                            && state.frontend.loading_session.is_none()
                            && state.match_state.scenario_exit.is_none()
                            && state.match_state.match_presentation.in_game_menu
                                == crate::ui::pause_menu::InGameMenuState::Closed,
                        "LAN abort cleanup incomplete"
                    );
                    // Reuse the same fresh-BB cleanup checks as other session returns.
                    self.phase = LanPhase::Returned;
                }
            }
            LanPhase::Settle(n) if n > 0 => self.phase = LanPhase::Settle(n - 1),
            _ => {}
        }
        Ok(())
    }
    fn abort_return(&self) -> bool {
        matches!(
            self.target,
            LanTarget::HostAbortReturn
                | LanTarget::GuestAbortReturn
                | LanTarget::LoadingTimeoutReturn
        )
    }
    pub(super) fn owns_simulation_steps(&self, state: &AppState) -> bool {
        self.abort_return()
            && self.phase == LanPhase::Loading
            && state.frontend.screen == GameScreen::InGame
    }
    pub(super) fn ready(&self, state: &AppState) -> Result<bool> {
        if self.phase == LanPhase::RemoteProgress {
            ensure!(
                state.frontend.screen == GameScreen::Loading,
                "remote-progress checkpoint missed loading"
            );
            return Ok(true);
        }

        if self.phase == LanPhase::Loading {
            if self.abort_return() || self.target == LanTarget::RemoteLoadingProgress {
                return Ok(false);
            }
            ensure!(
                state.match_state.session.mode() == SessionMode::Lan,
                "network handoff became offline"
            );
            return Ok(state.frontend.screen == GameScreen::Loading
                && crate::app::loading::pump::next_native_loading_frame(state)
                    == Some(crate::app::loading::pump::NextLoadingFrame::First));
        }
        Ok(self.phase == LanPhase::Settle(0)
            && state.frontend.shell_exit.is_none()
            && state.frontend.shell_first_paint_slide.is_none())
    }
    pub(super) fn receipt(&self, state: &AppState) -> Result<Value> {
        let loader_source = self.transferred_loader_source(state)?;
        let presentation = crate::app::loading::pump::network_loading_presentation(state);
        if self.target == LanTarget::HostObserverGo {
            use crate::render::loading_screen_chrome::LoadingArtVariant;
            let loading = presentation
                .as_ref()
                .context("actual network loading presentation")?;
            ensure!(
                loading.atlas_ready
                    && loading.art == Some(LoadingArtVariant::Observer)
                    && loading.local_player_index == 0
                    && loading.participants.len() == 3
                    && loading.participants[0].art == Some(LoadingArtVariant::Observer)
                    && loading.participants[1..].iter().all(
                        |row| row.art.is_some() && row.art != Some(LoadingArtVariant::Observer)
                    ),
                "observer loading art or source order differs from admitted roster"
            );
            ensure!(
                loading.participants[0].label == self.human(true)?.name
                    && loading.participants[1].label == self.human(false)?.name
                    && loading.participants[2].label
                        == self.inputs["observer_peer"]["name"]
                            .as_str()
                            .context("second peer")?,
                "loading painter reordered observer source nodes"
            );
        }
        let presentation = presentation.map(|loading|json!({"local_player_index":loading.local_player_index,
            "art":format!("{:?}",loading.art),"atlas_ready":loading.atlas_ready,
            "participants":loading.participants.iter().map(|row|json!({"label":row.label,"art":format!("{:?}",row.art)})).collect::<Vec<_>>()}));
        Ok(
            json!({"target":format!("{:?}",self.target),"route":self.route,"native_sha256":self.inputs["native_sha256"],
            "service_boundary":"controlled decoded inputs; no transport/backend claim","session":session_receipt(state),"before_handoff":self.admitted,"installed_runtime":self.observed_runtime,"observed_exit_cascade":self.observed_exit_cascade,"file_delivery":self.delivery,"parsed_transfer_source":loader_source,"loading_presentation":presentation}),
        )
    }
}
fn session_receipt(state: &AppState) -> Value {
    json!({"mode":format!("{:?}",state.match_state.session.mode()),"host":state.match_state.session.host(),
        "setup":state.match_state.session.network_setup(),"seed":state.match_state.session.network_start_seed(),
        "max_ahead_frames":state.match_state.session.network_max_ahead_frames(),
        "setup_capacity":state.match_state.session.network_setup_capacity(),"observer_row":state.match_state.session.network_observer_row(),
        "screen":format!("{:?}",state.frontend.screen)})
}
fn lobby_settled(state: &AppState) -> bool {
    MoviesCapture::slide_settled(state, ShellSlideKind::LanLobby)
        && state.frontend.shell_page_title.is_terminal()
}
fn setup_settled(state: &AppState, host: bool) -> bool {
    MoviesCapture::slide_settled(state, ShellSlideKind::LanSetup { host })
        && state
            .frontend
            .multiplayer_setup
            .as_ref()
            .is_some_and(|ui| ui.statics.is_terminal())
}
fn lobby_click(state: &mut AppState, rect: RectPx) {
    set_window_pointer_to_rect_center(state, rect);
    lobby::cursor_moved(state);
    lobby::mouse(state, MouseButton::Left, true);
    lobby::mouse(state, MouseButton::Left, false);
}
fn setup_click(state: &mut AppState, rect: RectPx) {
    set_window_pointer_to_rect_center(state, rect);
    setup_input::cursor_moved(state);
    setup_input::mouse(state, MouseButton::Left, true);
    setup_input::mouse(state, MouseButton::Left, false);
}
fn lobby_button(state: &mut AppState, id: u16) -> Result<()> {
    let rect = lobby::layout(state)
        .page
        .buttons
        .into_iter()
        .find(|b| b.id == id)
        .context("BB button")?
        .rect;
    lobby_click(state, rect);
    Ok(())
}
fn setup_button(state: &mut AppState, id: u16) -> Result<()> {
    let rect = setup_input::layout(state)
        .page
        .buttons
        .into_iter()
        .find(|b| b.id == id)
        .context("setup button")?
        .rect;
    setup_click(state, rect);
    Ok(())
}

fn offline_selection(state: &AppState) -> (i32, usize) {
    let dialog = &state.frontend.skirmish_shell_state;
    (dialog.selected_mode_id, dialog.selected_map_idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controlled_setup_values_are_bound_to_original_host_output() {
        let capture = LanCapture::new(LanTarget::Guest);
        assert_eq!(capture.inputs["transfer"]["file"], TRANSFER_FILE);
        let packet = |name: &str| {
            crate::util::base64::base64_decode(
                capture.inputs["packets_base64"][name]
                    .as_str()
                    .expect("captured packet"),
            )
            .expect("captured bytes")
        };
        for (name, kind) in [
            ("acceptance", 7u32),
            ("host_details", 3),
            ("roster", 22),
            ("options", 23),
            ("resolved_roster", 22),
            ("go", 11),
        ] {
            let bytes = packet(name);
            assert_eq!(bytes.len(), 455);
            assert_eq!(u32::from_le_bytes(bytes[..4].try_into().unwrap()), kind);
        }
        let options = packet("options");
        let fields: Vec<_> = options[47..].split(|byte| *byte == b',').collect();
        for (key, index) in [
            ("seed", 0),
            ("speed", 2),
            ("credits", 3),
            ("units", 4),
            ("capacity", 13),
            ("map_bytes", 15),
        ] {
            assert_eq!(
                capture.inputs["host_options"][key].to_string().as_bytes(),
                fields[index]
            );
        }
        assert_eq!(
            capture.inputs["host_options"]["map"]
                .as_str()
                .unwrap()
                .as_bytes(),
            fields[16]
        );
        assert_eq!(
            capture.inputs["host_options"]["digest"]
                .as_str()
                .unwrap()
                .as_bytes(),
            fields[17].split(|byte| *byte == b':').next().unwrap()
        );
        assert_eq!(
            capture.timing().unwrap(),
            i32::from_le_bytes(packet("go")[47..51].try_into().unwrap())
        );
        assert_eq!(
            capture.inputs["host_options"]["observer_row"]
                .as_u64()
                .unwrap(),
            u32::from_le_bytes(packet("acceptance")[0x57..0x5b].try_into().unwrap()) as u64
        );
    }
    #[test]
    fn transfer_cleanup_preserves_changed_file_and_removes_unchanged_owned_file() {
        let root = std::env::temp_dir().join(format!(
            "vera20k-lan-transfer-cleanup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join(TRANSFER_FILE);
        let bytes = b"declared delivered fixture";
        let guard = OwnedTransferFile {
            path: path.clone(),
            length: bytes.len(),
            sha256: crate::util::sha256::sha256_hex(bytes),
        };
        fs::write(&path, bytes).unwrap();
        guard.remove_unchanged().unwrap();
        assert!(!path.exists());
        fs::write(&path, b"changed by another owner").unwrap();
        assert!(guard.remove_unchanged().is_err());
        drop(guard);
        assert_eq!(fs::read(&path).unwrap(), b"changed by another owner");
        fs::remove_file(&path).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
