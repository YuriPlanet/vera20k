//! Network setup lifetime and service handoff: LAN BC005DE520 /
//! BD005E1CA0 and WOL C20078F7F0 / C900791B10. MatchSession owns accepted
//! setup/participants/seed. This adapter owns outstanding shell requests and
//! the dialog's temporary input/prompt state. WOL-only behaviour lives in
//! `wol`; every shared helper branches on GameMode like the native ones.

mod actions;
pub(crate) mod agreement;
mod cooperative;
pub(crate) use cooperative::chooser_rows as cooperative_chooser_rows;
pub(crate) mod maps;
mod service;
pub(crate) use service::{
    HostOptions, NetworkSetupEvent, accept_network_map_source, accept_network_random_map_preview,
    fail_go_handoff, invalidate_network_agreement, poll, prepare_lan_guest_handoff, service_event,
    set_network_map_progress,
};
pub(crate) mod view;
mod wol;
pub(crate) use actions::{
    apply_action, begin_external_lan_go, commit_map_selection, commit_start, open_choose_map,
};
pub(crate) use view::view;
pub(crate) use wol::{
    WolStartUser, guest_users_listed, kicked_key, local_left, map_status, play_player_left,
    player_choices, player_ready, preview_request, resident_seed, send_first_choices, user_joined,
    user_kicked, user_left, verify, verify_cursor_moved, verify_layout, verify_mouse,
    verify_poll_scroll,
};

use super::{App, AppState};
use crate::app::frontend::shell_transition;
use crate::app::frontend::skirmish_session::SetupProfile;
use crate::app::match_runtime::session::NetworkVacancy;
use crate::app::match_runtime::session::SessionMode;
use crate::app::shell_lan::LanJoinAcceptance;
use crate::skirmish_launch::{SkirmishHumanSlot, SkirmishLaunchSession};
use crate::ui::multiplayer_setup::{MultiplayerSetupState, SetupDialog, SetupFlavor};
use crate::ui::wol_chat::WolChatLine;
use crate::util::time::Instant;

/// Requests are service-boundary observations. They never create a peer,
/// acknowledge readiness or complete GO without an admitted response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NetworkSetupRequest {
    PublishOptions,
    PublishRoster,
    PublishCooperativeAssignments(Vec<[[i32; 2]; 8]>),
    /// Cooperative's campaign record, the `C0` text type `0x17` carries to
    /// every guest (`0x0049D680` through `0x005E3720`).
    PublishCooperativeCampaign(String),
    MapStatus(u8),
    PrepareMapForHandoff {
        received_budget: i32,
    },
    /// The guest's preview request: LAN type `0x1B` (`0x005E7586..0x005E759D`),
    /// sent once per material map change it cannot verify; WOL `P<scenario
    /// file>` privately to the first channel user (`0x007A81B8..0x007A8213`).
    RequestPreview,
    /// The host's `'W'` start staging, type `0x17` to every guest
    /// (`0x005E3720`).
    PublishStartStaging(String),
    /// The guest's second `0x10` at GO, for a random map's `RandMap.img`.
    RequestRandomMapImage,
    /// The host's preview offer, type `0x19` to every guest (`0x005E8133`).
    OfferPreview,
    /// The host's `SendFileToClients("Preview.bin")` once every guest has
    /// acknowledged the offer (`0x005E8471..0x005E847E`).
    SendPreviewFile,
    /// The guest's answer to an offer, type `0x1A` (`0x005E7920`).
    AcknowledgePreview,
    /// The guest's `0x10` for the offered `Preview.bin` (`0x00693B70`).
    RequestPreviewFile,
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

/// The online lobby first publishes final assignments, then submits Start only
/// after every player has accepted that same revision. The transport owns the
/// outstanding request; this owner retains only the setup input phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OnlineStartPhase {
    Idle,
    Preparing,
    Submitted,
}

pub(crate) struct NetworkSetupProtocol {
    locked: bool,
    /// WOL Quick Match (`[0xA8B244]` 1): the game channel's nodes and start
    /// run without C2/C9 (`0x007A6208..0x007A6229`, `0x0078A36C..0x0078A386`)
    /// under the runner's `0x117`.
    quick_match: bool,
    /// An external launch (docs/spawn.md): a launcher formed the room, so
    /// like Quick Match no setup dialog exists. The relay start handshake
    /// runs unchanged and Negotiated(0) starts loading directly; a direct
    /// (LAN) launch runs the LAN GO and its completion starts loading.
    external: bool,
    /// WOL `00B77DEC`/`00B77E28` after the host's StartGame request, until
    /// the server's OnGameStart rejects it; a matching OnGameStart holds it
    /// (`[0xB77E28] = 2`) until the abort or the match.
    wol_start_pending: bool,
    online_start_preparing: bool,
    /// WOL host `SetTimer(0x14AF8, 5000)` (`0x7914FF`).
    ping_deadline: Option<Instant>,
    /// WOL `00B76440`: last ping per participant name (`007A0EA0`).
    pings: Vec<(String, i32)>,
    /// WOL guest `[0xA8D610]`: the first other user the user-list sink
    /// added (`0x7A7627..0x7A763D`); its row shows `wolhost.pcx`.
    wol_host: Option<String>,
    /// WOL Start Game Now's connection verify `0x130` over C2/C9, from
    /// OnGameStart until the negotiation ends.
    wol_verify: Option<crate::ui::wol_verify::WolVerifyDialog>,
    start_requested: bool,
    accept_eligible: bool,
    /// LAN5DD5E5/5DD690/5DD950: send admission, drain and reply phases.
    lan_go: Option<LanGoWait>,
    map_transfer_pending: bool,
    /// A random map's `RandMap.Sed` has arrived; its `RandMap.img` is the
    /// second receive (`0x005DDA5D..0x005DDA68`).
    awaiting_random_map_image: bool,
    transfer_progress: Option<(u32, u32)>,
    return_after_notice: bool,
    /// The host's offer: the guests that answered and the 1800-bucket wait
    /// (`0x005E81D0..0x005E8326`).
    preview_offer: Option<PreviewOffer>,
    /// A guest's offered `Preview.bin` is being received.
    awaiting_preview_file: bool,
    /// The guest verified the host's map locally at its last change
    /// (`0x006951F0`).
    local_map_verified: bool,
    /// WOL C9: the verified map is a loose file on disk (`RawFileClass`
    /// exists, `0x005E7604..0x005E76E8`), so no local preview loads.
    local_map_loose: bool,
    /// WOL C9's `0x71F` "Preview unavailable" is shown, and `0x468` hidden,
    /// by the host's public `Z3` (`0x005E3D4E..0x005E3DC2`).
    wol_no_preview: bool,
    wol_preview_hidden: bool,
    /// The guest's `[0xAC1154]` from the host's `Preview.bin`, numbered so
    /// the renderer rebuilds its texture once per arrival.
    received_preview: Option<(u32, crate::map::preview::DecodedPreview)>,
    /// Derived from the same admitted source as the received image.
    received_preview_bounds: Option<crate::map::preview::PreviewSourceBounds>,
    received_preview_count: u32,
    requests: Vec<NetworkSetupRequest>,
}

/// The native blocking GO loops decide which global packets are consumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LanGoReceiveMode {
    Normal,
    Defer,
    RepliesOnly,
}

#[derive(Debug)]
enum LanGoWait {
    HostSending {
        pending: Vec<usize>,
    },
    HostDrain {
        pending: Vec<usize>,
        timer: crate::app::match_runtime::session::WallBucketTimer,
    },
    HostReplies {
        pending: Vec<usize>,
        timer: crate::app::match_runtime::session::WallBucketTimer,
    },
    GuestPreparing,
    GuestSending,
    GuestDrain,
    Complete,
    HandingOff,
    Failed,
}

impl LanGoWait {
    fn receive_mode(&self) -> LanGoReceiveMode {
        match self {
            Self::HostReplies { .. } => LanGoReceiveMode::RepliesOnly,
            _ => LanGoReceiveMode::Defer,
        }
    }

    fn packets_queued(self, now: u32) -> anyhow::Result<Self> {
        use crate::app::match_runtime::session::WallBucketTimer;
        match self {
            Self::HostSending { pending } => Ok(Self::HostDrain {
                pending,
                timer: WallBucketTimer::start(now, 1200),
            }),
            Self::GuestSending => Ok(Self::GuestDrain),
            _ => anyhow::bail!("GO packets were queued outside their send phase"),
        }
    }

    fn observe(self, now: u32, queued: usize, service_ok: bool) -> Self {
        use crate::app::match_runtime::session::WallBucketTimer;
        match self {
            Self::HostDrain { pending, timer } => {
                // Original5DD629 JNZ exits while time remains. A fresh timer
                // therefore skips this drain; an expired one keeps servicing
                // until the queue empties. Do not turn it into a timeout.
                if queued == 0 || timer.remaining(now) != 0 {
                    if pending.is_empty() {
                        Self::Complete
                    } else {
                        Self::HostReplies {
                            pending,
                            timer: WallBucketTimer::start(now, 600),
                        }
                    }
                } else {
                    Self::HostDrain { pending, timer }
                }
            }
            Self::HostReplies { pending, timer } => {
                if pending.is_empty() || timer.remaining(now) == 0 {
                    Self::Complete
                } else {
                    Self::HostReplies { pending, timer }
                }
            }
            // 5DD950..97D exits on queue empty or a false541820 result.
            Self::GuestDrain if queued == 0 || !service_ok => Self::Complete,
            phase => phase,
        }
    }

    fn acknowledge(&mut self, source: usize) {
        if let Self::HostReplies { pending, .. } = self {
            pending.retain(|candidate| *candidate != source);
            if pending.is_empty() {
                *self = Self::Complete;
            }
        }
    }
}

/// Called before every global dequeue, including after a GO reply changed phase.
pub(crate) fn lan_go_receive_mode(state: &AppState) -> LanGoReceiveMode {
    state
        .frontend
        .network_setup_protocol
        .as_ref()
        .and_then(|protocol| protocol.lan_go.as_ref())
        .map_or(LanGoReceiveMode::Normal, LanGoWait::receive_mode)
}

/// Call only after the complete GO broadcast or the guest's Type13 was admitted.
pub(crate) fn mark_lan_go_packets_queued(state: &mut AppState, now: u32) -> anyhow::Result<()> {
    let protocol = state
        .frontend
        .network_setup_protocol
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("GO send has no setup owner"))?;
    anyhow::ensure!(
        matches!(
            protocol.lan_go,
            Some(LanGoWait::HostSending { .. } | LanGoWait::GuestSending)
        ),
        "GO packets were queued outside their send phase"
    );
    let phase = protocol
        .lan_go
        .take()
        .ok_or_else(|| anyhow::anyhow!("no GO send is pending"))?;
    protocol.lan_go = Some(phase.packets_queued(now)?);
    Ok(())
}

/// The transport supplies observations, while this owner decides progression.
/// A true result requires current guest timing followed by GoCompleted.
pub(crate) fn observe_lan_go_transport(
    state: &mut AppState,
    now: u32,
    queued: usize,
    service_ok: bool,
) -> bool {
    let Some(protocol) = state.frontend.network_setup_protocol.as_mut() else {
        return false;
    };
    if let Some(phase) = protocol.lan_go.take() {
        protocol.lan_go = Some(phase.observe(now, queued, service_ok));
    }
    matches!(protocol.lan_go, Some(LanGoWait::Complete))
}

/// The host's offer as sent: the guests it went to (the node count is fixed
/// at the send, `0xA8DA84`, and acks match by sender, `0x005E82D0`), and the
/// preview the `'W'` described.
struct PreviewOffer {
    guests: Vec<String>,
    acknowledged: Vec<String>,
    preview: crate::map::preview::DecodedPreview,
    deadline: Instant,
}

impl NetworkSetupProtocol {
    fn new(dialog: SetupDialog, quick_match: bool) -> Self {
        let host = dialog.host;
        // C2's 0x497 (the advertisement, the pings and their timer) never
        // runs without C2.
        let wol_host = dialog.wol() && host && !quick_match;
        Self {
            locked: false,
            quick_match,
            external: false,
            wol_start_pending: false,
            online_start_preparing: false,
            ping_deadline: wol_host.then(|| Instant::now() + wol::PING_PERIOD),
            pings: Vec::new(),
            wol_host: None,
            wol_verify: None,
            start_requested: false,
            accept_eligible: false,
            lan_go: None,
            map_transfer_pending: false,
            awaiting_random_map_image: false,
            transfer_progress: None,
            return_after_notice: false,
            preview_offer: None,
            awaiting_preview_file: false,
            local_map_verified: false,
            local_map_loose: false,
            wol_no_preview: false,
            wol_preview_hidden: false,
            received_preview: None,
            received_preview_bounds: None,
            received_preview_count: 0,
            requests: if wol_host {
                // C2 0x497: advertisement 78F000, then one ping per user.
                vec![
                    NetworkSetupRequest::PublishOptions,
                    NetworkSetupRequest::Advertise,
                    NetworkSetupRequest::RequestPings,
                ]
            } else if host && !quick_match {
                vec![NetworkSetupRequest::PublishOptions]
            } else {
                Vec::new()
            },
        }
    }

    pub(crate) fn set_online_start_phase(&mut self, phase: OnlineStartPhase) {
        self.online_start_preparing = phase == OnlineStartPhase::Preparing;
        self.wol_start_pending = phase == OnlineStartPhase::Submitted;
        if phase == OnlineStartPhase::Idle {
            self.start_requested = false;
        }
    }

    pub(crate) fn start_in_progress(&self) -> bool {
        self.wol_start_pending || self.online_start_preparing
    }

    pub(crate) fn lan_go_send_pending(&self) -> bool {
        matches!(
            self.lan_go,
            Some(
                LanGoWait::HostSending { .. } | LanGoWait::GuestPreparing | LanGoWait::GuestSending
            )
        )
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

    fn record_remote_user_left(&mut self, host: bool, operator: bool) {
        if host {
            self.requests.push(NetworkSetupRequest::PublishOptions);
        } else if operator {
            self.requests.push(NetworkSetupRequest::Leave);
        }
    }

    pub(crate) fn editable(&self) -> bool {
        !self.locked
    }

    /// Publish the current accepted choices followed by their display rows.
    pub(crate) fn republish_setup(&mut self) {
        self.requests.extend([
            NetworkSetupRequest::PublishOptions,
            NetworkSetupRequest::PublishRoster,
        ]);
    }

    /// Presentation of the active service operation, never map ownership or
    /// transfer transport. Completion is a separate admitted service result.
    pub(crate) fn transfer_progress(&self) -> Option<(u32, u32)> {
        self.transfer_progress
    }

    pub(crate) fn blocking_map_transfer(&self) -> bool {
        self.map_transfer_pending
    }

    /// The guest's preview when its map did not verify: the host's
    /// `Preview.bin`, with its arrival number.
    pub(crate) fn received_preview(&self) -> Option<(u32, &crate::map::preview::DecodedPreview)> {
        self.received_preview
            .as_ref()
            .map(|(count, preview)| (*count, preview))
    }

    /// The guest's `[0xAC1154]` comes from its own copy of the map
    /// (`0x005E74E0`).
    pub(crate) fn local_preview(&self) -> bool {
        self.local_map_verified && !self.local_map_loose
    }

    /// C9's `0x71F` shows, and whether `0x468` is hidden.
    pub(crate) fn wol_no_preview(&self) -> (bool, bool) {
        (self.wol_no_preview, self.wol_preview_hidden)
    }

    /// Physical resources were verified by the shared map/source owners.
    /// Keep any earlier qualified local preview; a received source without
    /// a chooser record instead uses the already-decoded received surface.
    fn accept_verified_map_source(&mut self) {
        self.transfer_progress = None;
        if !self.local_map_verified {
            self.local_map_loose = true;
        }
        self.local_map_verified = true;
        self.drop_preview_offer();
        self.awaiting_preview_file = false;
    }

    pub(crate) fn received_preview_bounds(
        &self,
    ) -> Option<&crate::map::preview::PreviewSourceBounds> {
        self.received_preview_bounds.as_ref()
    }

    fn accept_preview_bounds(&mut self, bounds: Option<crate::map::preview::PreviewSourceBounds>) {
        self.received_preview_bounds = bounds;
    }

    /// Only an admitted map-identity change invalidates the derived image and
    /// marker bounds (5E74E0..5E7513). Reusing the same source after an upload
    /// completed must not depend on another physical transfer to restore them.
    fn invalidate_received_preview(&mut self, map_changed: bool) {
        if !map_changed {
            return;
        }
        self.received_preview = None;
        self.received_preview_bounds = None;
        self.awaiting_preview_file = false;
    }

    fn drop_preview_offer(&mut self) {
        self.preview_offer = None;
    }
}

/// The admitted setup is a WOL Quick Match game channel's, with no dialog.
pub(crate) fn quick_match(state: &AppState) -> bool {
    state
        .frontend
        .network_setup_protocol
        .as_ref()
        .is_some_and(|protocol| protocol.quick_match)
}

/// The admitted setup belongs to an external launch, with no dialog.
pub(crate) fn external(state: &AppState) -> bool {
    state
        .frontend
        .network_setup_protocol
        .as_ref()
        .is_some_and(|protocol| protocol.external)
}

/// The network visit an external launch enters: GameMode 4 (relay) or 3
/// (direct LAN) with the launcher's canonical session admitted as this
/// client's setup. Source 0 is the room owner, as in the lobby's
/// `game_started` users, and the direct host.
pub(crate) fn accept_external_session(
    session: &mut crate::app::match_runtime::session::MatchSession,
    launch: SkirmishLaunchSession,
    context: crate::app::match_runtime::session::NetworkReturnContext,
) -> anyhow::Result<()> {
    let host = launch.local_player_index == 0;
    session.enter_network(context);
    session.accept_network_setup(launch, host, None)
}

/// Install an external launch's setup without a dialog, its online start
/// requests or pings: the launcher already agreed the room.
pub(crate) fn install_external_setup(
    state: &mut AppState,
    launch: SkirmishLaunchSession,
    context: crate::app::match_runtime::session::NetworkReturnContext,
) -> anyhow::Result<()> {
    // The parser resolved the launcher's map beside spawn.ini to a local
    // path. Participants keep it in different directories, and the world
    // hashes the map name, so load that file as this client's physical
    // source under its shared logical name, as a LAN guest's download.
    let mut launch = launch;
    let path = launch
        .selected_map_file
        .clone()
        .ok_or_else(|| anyhow::anyhow!("external launch has no map"))?;
    let logical = std::path::Path::new(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("external map has no file name"))?
        .to_owned();
    crate::map::source::validate_network_map_name(&logical)?;
    let loaded = {
        let root = &state
            .platform
            .game_config
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no asset directory"))?
            .paths
            .ra2_dir;
        let assets = state
            .process_assets
            .manager()
            .ok_or_else(|| anyhow::anyhow!("retail assets are unavailable"))?;
        crate::map::source::load_network_map_ini(root, &path, assets)?
    };
    let effective = crate::map::source::effective_network_map_digest(&loaded.ini, false);
    launch.selected_map_file = Some(logical);
    accept_external_session(&mut state.match_state.session, launch, context)?;
    state
        .match_state
        .session
        .accept_external_network_map_source(loaded.source, effective)?;
    let mut protocol = NetworkSetupProtocol::new(dialog(state), false);
    protocol.external = true;
    protocol.ping_deadline = None;
    protocol.requests.clear();
    state.frontend.network_setup_protocol = Some(protocol);
    Ok(())
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
        flavor: if state.match_state.session_mode() == SessionMode::Wol {
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
        // VERA's compact online flow opens a mode that offers AI opponents.
        // Native diagnostic visits retain their saved mode selection.
        let prefer_computer_mode = dialog.wol()
            && !observe
            && state
                .frontend
                .wol
                .as_ref()
                .is_some_and(|visit| visit.modern_ui());
        let result = state.frontend.multiplayer_shell_runtime.new_network_setup(
            profile(dialog),
            prefer_computer_mode,
            &state.frontend.scenario_catalog,
            &state.frontend.skirmish_modes,
            state
                .frontend
                .skirmish_shell_state
                .launch_options_base
                .clone(),
            state.frontend.skirmish_shell_state.trackbar_bounds,
            observe,
            if dialog.wol() {
                crate::app::shell_wol::lobby::room(state)
            } else {
                Default::default()
            },
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
        // The WOL C2's own +20/+18 calls (0x791293, 0x7912A3) belong to a
        // later chain; LAN BC runs them here.
        if !dialog.wol()
            && let Err(error) = cooperative::host_opened(state)
        {
            log::error!("Could not open the Cooperative campaign: {error:#}");
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
            [NetworkVacancy::Open; crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT],
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
    vacancies: [NetworkVacancy; crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT],
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
    let quick_match = dialog.wol() && crate::app::shell_wol::quick_match::mode_active(state);
    if dialog.wol() {
        let room = crate::app::shell_wol::lobby::room(state);
        state.match_state.session.set_network_room(room);
        // C2's lockout in a tournament or clan room, after the profile
        // restore (0x79132A..0x7913D1): Short Game and Bases on; the AI
        // count is already 0 on WOL.
        if host && room.tournament_or_clan() && !quick_match {
            let mut options = state
                .match_state
                .session
                .network_setup()
                .expect("admitted setup")
                .options
                .clone();
            options.short_game = true;
            options.bases = true;
            state.match_state.session.set_network_options(options)?;
        }
    }
    if host {
        for (row, vacancy) in vacancies.iter().copied().enumerate().take(8) {
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
        // VERA profile rows8..29 follow capacity admission: expanding the
        // initial eight rows opens the new suffix. Restore saved Closed rows
        // through the same row-kind owner afterward; Open rows and any removed
        // out-of-capacity AI keep the map owner's result. Native rows above
        // were restored in their original order (FUN_005ED5A0).
        for (row, vacancy) in vacancies.into_iter().enumerate().skip(8) {
            if vacancy == NetworkVacancy::Closed
                && state
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
    let mut setup_ui = MultiplayerSetupState::default();
    if let Some(row) = state
        .match_state
        .session
        .network_setup()
        .and_then(|launch| launch.local_player().raw_slot_index)
    {
        setup_ui.show_row(usize::from(row));
    }
    state.frontend.multiplayer_setup = Some(setup_ui);
    state.frontend.network_setup_protocol = Some(NetworkSetupProtocol::new(dialog, quick_match));
    if !quick_match && state.match_state.session.host() {
        note_scenario(state);
    }
    if quick_match {
        // 0x117 stays up and takes the lines; no dialog opens.
        state.platform.window.request_redraw();
        return Ok(());
    }
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

/// `[0xA8B254]` after the host's map choice (BC open `0x005ED668`, Use Map
/// `0x005E7370`, C2 `0x00790DFE`): the selected map's record, or 0 when it
/// has none (`0x005ED660`). Quick Match's setup has no dialog and never
/// writes it, nor does an options parse.
/// RESIDUAL: a guest's dialog init writes it too (C9 `0x00792978` with the
/// mode's `+0x44` default, the LAN lobby `0x005DCB3B`/`0x005DDA8C`); VERA's
/// guests keep the previous value. Only a guest's `+0x20` before the host's
/// C0 reads it.
pub(super) fn note_scenario(state: &mut AppState) {
    let index = state
        .match_state
        .session
        .network_setup()
        .and_then(|launch| launch.selected_map_file.as_deref())
        .and_then(|file| {
            state
                .frontend
                .scenario_catalog
                .records()
                .iter()
                .position(|record| record.file_name.eq_ignore_ascii_case(file))
        })
        .unwrap_or(0);
    state
        .frontend
        .multiplayer_shell_runtime
        .set_scenario_index(index);
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

pub(crate) fn remember_setup(state: &mut AppState) {
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

/// The network setup's scenario description, Session+0xEA (`0x00A8B322`).
/// It can come from the host when no local scenario record qualifies. It
/// belongs to the accepted session, not a phantom entry in the discovery
/// catalog (ParsePacket5E51C4..5E5409).
pub(crate) fn map_display_name(state: &AppState) -> Option<&str> {
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

pub(crate) fn local_map_metadata(
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
    if dialog.wol() && dialog.host {
        crate::app::shell_wol::lobby::host_setup_left(state);
    }
    open_service_lobby(state, dialog);
}

/// PrepareSession after a network match. LAN builds a new BB; WOL enters
/// `WOL_Main` again, whose lobby runner runs with postmatch set
/// (`0x77B44B`) and rejoins the previous chat lobby.
pub(crate) fn return_after_match(state: &mut AppState) {
    let dialog = leave_setup(state);
    if dialog.wol() {
        crate::app::shell_wol::reenter_after_match(state);
    } else {
        crate::app::lan_service::restart_lobby_visit(state);
        App::open_lan_lobby(state);
    }
}

/// Type8's synchronous native departure drain blocks further setup input.
/// The LAN service resumes the existing Kicked transition when it finishes.
pub(crate) fn lock_lan_departure(state: &mut AppState) {
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.locked = true;
    }
}

/// The WOL runner closes every dialog (`LAB_00787F98`) while C2/C9 runs:
/// the setup is dropped through its owner without reopening a lobby.
pub(crate) fn abandon_for_wol_runner(state: &mut AppState) {
    leave_setup(state);
}

fn leave_setup(state: &mut AppState) -> SetupDialog {
    let dialog = dialog(state);
    let unlocked = state
        .frontend
        .network_setup_protocol
        .as_ref()
        .is_some_and(|p| !p.locked);
    if dialog.wol() && crate::app::shell_wol::quick_match::mode_active(state) {
        // C2/C9's teardowns write the settings (0x6990A0, 0x5ED400); the
        // Quick Match path has neither and writes none.
    } else if unlocked && dialog.wol() && !dialog.host {
        // C9 (0x791B10) and the local-leave tail 0x785740 write no
        // settings: no 0x6990A0 or 0x5ED400 on Back, a kick or the host
        // leaving.
    } else if unlocked {
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
    if !dialog.wol() {
        crate::app::lan_service::leave_setup(state);
    }
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
    fn remote_user_departure_queues_native_wol_requests_in_protocol_order() {
        let mut host = NetworkSetupProtocol::new(
            SetupDialog {
                host: true,
                flavor: SetupFlavor::Wol,
            },
            false,
        );
        assert_eq!(
            host.take_service_requests(),
            vec![
                NetworkSetupRequest::PublishOptions,
                NetworkSetupRequest::Advertise,
                NetworkSetupRequest::RequestPings,
            ]
        );
        host.record_remote_user_left(true, false);
        assert_eq!(
            host.take_service_requests(),
            vec![NetworkSetupRequest::PublishOptions]
        );

        let mut guest = NetworkSetupProtocol::new(
            SetupDialog {
                host: false,
                flavor: SetupFlavor::Wol,
            },
            false,
        );
        guest.record_remote_user_left(false, true);
        assert_eq!(
            guest.take_service_requests(),
            vec![NetworkSetupRequest::Leave]
        );
        guest.record_remote_user_left(false, false);
        assert!(guest.take_service_requests().is_empty());
    }

    #[test]
    fn lan_go_drain_gates_match_executed_original_branches() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/lan_messages.json",
        ))
        .unwrap();
        for case in corpus["go_waits"]["host"].as_array().unwrap() {
            let start = case["start"].as_u64().unwrap() as u32;
            let now = case["now"].as_u64().unwrap() as u32;
            let queued = case["queued"].as_u64().unwrap() as usize;
            let phase = LanGoWait::HostSending { pending: vec![1] }
                .packets_queued(start)
                .unwrap()
                .observe(now, queued, true);
            let expected = if case["service_calls"].as_u64().unwrap() == 0 {
                LanGoReceiveMode::RepliesOnly
            } else {
                LanGoReceiveMode::Defer
            };
            assert_eq!(phase.receive_mode(), expected, "{case}");
        }
        for case in corpus["go_waits"]["guest"].as_array().unwrap() {
            let phase = LanGoWait::GuestSending
                .packets_queued(100)
                .unwrap()
                .observe(
                    100,
                    case["queued"].as_u64().unwrap() as usize,
                    case["service_ok"].as_bool().unwrap(),
                );
            assert_eq!(
                matches!(phase, LanGoWait::Complete),
                case["service_calls"].as_u64().unwrap() == 0,
                "{case}"
            );
        }
    }

    #[test]
    fn lan_go_admission_reply_and_timeout_phases_remain_separate() {
        let mut phase = LanGoWait::HostSending {
            pending: vec![1, 2],
        };
        phase.acknowledge(1);
        assert_eq!(phase.receive_mode(), LanGoReceiveMode::Defer);
        let mut phase = phase.packets_queued(100).unwrap().observe(100, 3, true);
        phase.acknowledge(9);
        phase.acknowledge(1);
        phase.acknowledge(1);
        assert_eq!(phase.receive_mode(), LanGoReceiveMode::RepliesOnly);
        let phase = phase.observe(699, 3, true);
        assert_eq!(phase.receive_mode(), LanGoReceiveMode::RepliesOnly);
        assert!(matches!(phase.observe(700, 3, true), LanGoWait::Complete));

        let unsent = LanGoWait::GuestSending.observe(100, 0, true);
        assert!(matches!(unsent, LanGoWait::GuestSending));
        let sent = unsent.packets_queued(100).unwrap();
        let waiting = sent.observe(101, 1, true);
        assert!(matches!(waiting, LanGoWait::GuestDrain));
        assert!(matches!(waiting.observe(102, 0, true), LanGoWait::Complete));
    }

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
