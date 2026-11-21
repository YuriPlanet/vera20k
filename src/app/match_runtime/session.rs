//! Live front-end session owner: gamemd's `SessionClass` instance at
//! `0x00A8B238`, whose first field is the `GameMode` discriminator every
//! `GameMode == 0` / `!= 0` branch and the modal pump read.
//!
//! Native writers of `GameMode`:
//! - process start `0x0077B532` (and the sibling exits of `FUN_0077B2A0`): 0.
//! - main-menu shell `FUN_0052D9A0`: `0x0052DD7F` Network -> 3 (LAN),
//!   `0x0052DD61` Internet -> 4 (WOL), `0x0052E10F` Skirmish -> 5 before
//!   `SessionClass__ReadMultiPlayerSettings @ 0x006980C0`, `0x0052E175`
//!   Skirmish Back -> 0 (campaign).
//! - `Load_Game @ 0x0067E440` (called from the load dialog at `0x00559DC5`):
//!   `0x0067E4E8` restores the value the save header carries at `+0x3F0`
//!   (`0x006812D0`).
//! - `Main_Game @ 0x0048CCC0`: `0x0048CF94` resets it to 0 on the flagged
//!   return path; VERA's shell resume route (`ShellRoute`) carries that
//!   decision instead.
//!
//! One value, one holder at a time. An installed world carries `GameMode` in
//! its saved, hashed ScenarioSession (the simulation reads it there, and
//! `Load_Game`'s restore arrives with it), so the world holds it from install
//! until the shell writes it again. This owner holds it only before the
//! first install, after a shell write, or once a world is dropped without a
//! successor; [`super::state::MatchState::session_mode`] reads whichever
//! holds it. A network visit's mode follows from the service it visits
//! (`NetworkReturnContext`), so the visit stores no copy either.

use crate::skirmish_launch::{
    LaunchTeam, NETWORK_PLAYER_SLOT_COUNT, SkirmishLaunchSession, SkirmishLocalSlot,
};
use std::net::SocketAddrV4;
use std::sync::Arc;

pub(crate) use crate::sim::scenario_session::ScenarioGameMode as SessionMode;

mod connections;
mod cooperative_round;
mod network_map;
mod room;
mod setup_rows;
mod stage_result;
mod start;
pub(crate) use cooperative_round::{
    CooperativeRoundEvent, CooperativeRoundOptions, CooperativeRoundState, CooperativeRoundStep,
};
pub(crate) use stage_result::{CampaignResult, StageHandoff, StageProgress};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkReturnContext {
    LanLobby,
    /// The WOL custom lobby `0x113` (Custom Match, mode 4).
    Wol,
}

/// The post-match disposition admitted with the match. The shell and lobby
/// protocol consume or project it as the finished match returns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum NetworkRoomReturn {
    #[default]
    Lobby,
    LanHost,
    LanGuest {
        host: String,
        game_id: u64,
        until: Option<crate::util::time::Instant>,
    },
    Wol {
        cooperative: Option<CooperativeResume>,
        campaign: Option<Box<crate::app::shell_network_setup::campaign_room::CampaignRoom>>,
    },
}

/// The Cooperative stage selected by BC6's Continue for a kept WOL room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CooperativeResume {
    pub(crate) mode_id: i32,
    pub(crate) scenario: usize,
}

pub(crate) enum LanRoomRejoin {
    Join(u64),
    Expired(String),
}

impl NetworkReturnContext {
    /// The `GameMode` the main menu writes on entering this service:
    /// `0x0052DD7F` Network -> 3 (LAN), `0x0052DD61` Internet -> 4 (WOL).
    pub(crate) const fn session_mode(self) -> SessionMode {
        match self {
            Self::LanLobby => SessionMode::Lan,
            Self::Wol => SessionMode::Wol,
        }
    }
}

/// Which side holds `GameMode` now (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModeHolder {
    Shell(SessionMode),
    /// The installed world's ScenarioSession.
    World,
}

/// Also each row of the online options' `vacancies` (docs/lobby-protocol.md),
/// whose lobby refuses any other value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NetworkVacancy {
    Open,
    Closed,
    #[serde(rename = "observer")]
    OpenObserver,
    ///5E8EA0's AC11B0: a roster name whose type3 node has not arrived.
    Waiting,
}

/// What the host's Teams box gives a seated row in a VERA room
/// ([`MatchSession::set_network_seat_picks`]): Team Starts' start or Team
/// Colors' colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SeatPick {
    Start(crate::skirmish_launch::LaunchStartPosition),
    Color(u8),
}

/// A received5E8EA0 row binding. Human indices refer to the already-admitted
/// source vector; a display packet never creates a participant or endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkRosterEntry {
    Human(usize),
    Vacancy(NetworkVacancy),
    Ai(crate::skirmish_launch::AiDifficulty),
}

/// Session's five raw AI-array words (A8B27C/29C/2BC/2DC/2FC).
/// Active rows are derived from `launch.opponents`; only inactive rows retain
/// these words, independently of the visible human or vacant-row controls.
/// A LAN host's full options carry every row's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct NetworkAiOptions {
    pub difficulty: i32,
    pub country: i32,
    pub color: i32,
    pub start: i32,
    pub team: i32,
}

impl NetworkAiOptions {
    /// An active row's words, as gamemd's options string (`0x005DBB60`)
    /// writes them.
    pub(crate) fn of(ai: &crate::skirmish_launch::SkirmishAiSlot) -> Self {
        Self {
            difficulty: ai.difficulty.as_i32(),
            country: ai.country_shell_value(),
            color: ai.color_shell_value(),
            start: ai.start_position.shell_value(),
            team: ai.team.shell_value(),
        }
    }

    /// Fresh BC: BSS difficulty0, cleared country/color, and5DEC98..BD's
    /// explicit -2 start/team fills. Later row transitions retain their own
    /// -1 clears and5EE510's final team write instead of recreating this state.
    pub(crate) const INITIAL_INACTIVE: Self = Self {
        difficulty: 0,
        country: -1,
        color: -1,
        start: -2,
        team: -2,
    };
    ///5ED0CF..5ED0EB /5ED17D..5ED199 clear all five words to -1.
    const CLEARED: Self = Self {
        difficulty: -1,
        country: -1,
        color: -1,
        start: -1,
        team: -1,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkRowKind {
    Open,
    Closed,
    OpenObserver,
    Ai(crate::skirmish_launch::AiDifficulty),
}

/// What a peer's `R` changed on the host (`0x007A8260`): the rows, which
/// republishes the slot names (`0x0077DB90`) and the advertisement
/// (`0x0078F000`), and the options (`0x005E32D0`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PeerChoicesChange {
    pub roster: bool,
    pub options: bool,
}

/// Decoded full-options fields5E4F97..5E513F. Filename remains solely in
/// the launch; player_capacity is rawA8BB0C, not effective831700 row capacity.
/// Online it is the options' `map_metadata` (docs/lobby-protocol.md), whose
/// lobby refuses any other key.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct NetworkMapMetadata {
    pub display_name: String,
    pub player_capacity: i32,
    pub official: bool,
    pub byte_length: i32,
    pub digest: String,
    /// Exact physical bytes for VERA online agreement and map transfer. Native
    /// [Digest] text above remains a separate compatibility field.
    pub source_sha256: String,
    pub effective_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkMapAdmission {
    Unavailable,
    Present,
    NeedsTransfer,
}
impl NetworkMapAdmission {
    pub const fn status(self) -> u8 {
        match self {
            Self::Unavailable => 0,
            Self::Present => 1,
            Self::NeedsTransfer => 2,
        }
    }
    pub const fn accept_eligible(self) -> bool {
        !matches!(self, Self::Unavailable)
    }
}
impl NetworkMapMetadata {
    ///5E5500..5844: custom missing maps permit Accept and request transfer;
    /// RandMap.Sed bypasses physical availability.
    pub fn admission(&self, file: &str, local_present: bool) -> NetworkMapAdmission {
        if local_present || file.eq_ignore_ascii_case(crate::map::skirmish_scenarios::RANDMAP_SED) {
            NetworkMapAdmission::Present
        } else if self.official {
            NetworkMapAdmission::Unavailable
        } else {
            NetworkMapAdmission::NeedsTransfer
        }
    }
}

/// Independent native node+77 readiness and node+7F map admission. These
/// flags belong to the accepted roster's indices, never to display names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParticipantAdmission {
    ready: bool,
    map_status: u8,
    /// VERA: the K this participant last sent for the current map. A
    /// revision clears `map_status`, not this; another map or mode does.
    reported_map_status: Option<u8>,
}

/// A viewer's transport identity. It cannot be translated into a command
/// sender, setup seat, or House index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ConnectionWatcher {
    endpoint: SocketAddrV4,
    local: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
struct ConnectionRoster {
    /// Indices into the immutable launch's native nodes. This also keeps the
    /// classic Observer House nodes that take part in native lockstep.
    playing_sources: Vec<usize>,
    watching_sources: Vec<ConnectionWatcher>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NetworkPhase {
    Lobby {
        room_return: NetworkRoomReturn,
    },
    Setup {
        launch: SkirmishLaunchSession,
        watching_sources: Vec<ConnectionWatcher>,
        admission: Vec<ParticipantAdmission>,
        vacancies: [NetworkVacancy; NETWORK_PLAYER_SLOT_COUNT],
        /// Authoritative only while the corresponding row is vacant. Entering
        /// a human/AI row makes its canonical participant choices authoritative.
        inactive_choices: [SkirmishLocalSlot; NETWORK_PLAYER_SLOT_COUNT],
        inactive_ai_options: [NetworkAiOptions; NETWORK_PLAYER_SLOT_COUNT],
        /// A8DA90 AI display pointers. Roster packets change these without
        /// changing Session's AI arrays; global packets may arrive reordered.
        /// A WOL host's options are its roster (`'L'`, `0x5E8EA0`), so there
        /// they set these from the AI arrays on the rows they send Closed.
        display_ai: [Option<crate::skirmish_launch::AiDifficulty>; NETWORK_PLAYER_SLOT_COUNT],
        /// Native831700/8316A4: prior capacity and independent reserved observer row.
        capacity: i32,
        observer_row: Option<u8>,
        /// VERA's own rooms (`SetupDialog::modern`): the rows past the
        /// players are spectator rows, Closed unless open for or held by an
        /// observer. Native diagnostics keep the one reserved row above.
        several_observers: bool,
        /// The host of a VERA room keeps one spectator row open after its
        /// spectators, AI playing or not: unlike gamemd's room, where a game
        /// with observers has no AI (`0x5EC9F0`'s TXT_NO_AI_WITH_OBSERVER).
        open_spectators: bool,
        /// The most people a VERA room holds, players and spectators: its
        /// host's Create Game Max players, then the team bar's Room size.
        people_limit: u8,
        /// A VERA room queues the spectators who ask to play while no seat
        /// is free (`launch.options.queue`), unless the online lobby
        /// refused the queue.
        join_queue: bool,
        /// A kept WOL co-op room is seating the same partner again.
        cooperative_rematch_seat: bool,
    },
    Frozen {
        startup: Arc<crate::app::match_bootstrap::PreparedNetworkStartup>,
        connections: ConnectionRoster,
        room_return: NetworkRoomReturn,
        campaign_abort_pending: bool,
    },
    CampaignResult {
        startup: Arc<crate::app::match_bootstrap::PreparedNetworkStartup>,
        connections: ConnectionRoster,
        result: Box<CampaignResult>,
        room_return: NetworkRoomReturn,
    },
    /// The co-op result dialog BC6 between missions: the finished start,
    /// the round's own roster (the next mission's, which the master's
    /// `+0x68`/`0x0069B8C0` or a received `0x32` changes) and its readiness.
    CooperativeRound {
        previous: Arc<crate::app::match_bootstrap::PreparedNetworkStartup>,
        connections: ConnectionRoster,
        launch: SkirmishLaunchSession,
        round: CooperativeRoundState,
        room_return: NetworkRoomReturn,
    },
}

impl NetworkPhase {
    fn lobby() -> Self {
        Self::Lobby {
            room_return: NetworkRoomReturn::Lobby,
        }
    }

    fn room_return_mut(&mut self) -> Option<&mut NetworkRoomReturn> {
        match self {
            Self::Lobby { room_return }
            | Self::Frozen { room_return, .. }
            | Self::CampaignResult { room_return, .. }
            | Self::CooperativeRound { room_return, .. } => Some(room_return),
            Self::Setup { .. } => None,
        }
    }
}
/// A packet the battle or the co-op round asks the LAN service to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NetworkSendRequest {
    /// The kick proposal packet `0x1C` (0x0064AAE0) to every other node.
    ProposeKick { target_source: usize },
    /// The in-battle chat packet `0x0C` (`0x0055EDBF..0x0055F04E`) to these
    /// participants. The packet's name and colour are the local player's.
    Chat {
        recipients: Vec<usize>,
        text: String,
    },
    /// The taunt packet `0x1D` (`0x00536350`) to these participants; `byte`
    /// is `(country << 4) | n` at packet `+0x30`.
    Taunt { recipients: Vec<usize>, byte: u8 },
    /// A beacon record `0x20`/`0x21` to every other node
    /// (`0x00430E0C..0x00430E4E`, `0x004313C2..0x004313F5`).
    Beacon(crate::net::lan_messages::ingame::BeaconRecord),
    /// A beacon label `0x22` to every other node (`0x0043165E..0x00431678`).
    BeaconText { house: i8, slot: i8, text: Vec<u16> },
    /// BC6's `0x26` to every node.
    CooperativeReady,
    /// BC6's `0x24` (Quit) to every node.
    CooperativeLeave,
    /// A bounded co-op handoff failed. Use the existing scoped LAN departure
    /// so a peer which already started loading retires that same admission.
    CooperativeAbort,
    /// The master's `0x34`.
    CooperativeBegin,
    /// The master's `0x32` to every bound human slot.
    CooperativeOptions(CooperativeRoundOptions),
    /// A guest's `0x23`.
    CooperativeAck,
    /// The master's `0x31`.
    CooperativeGo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NetworkSession {
    campaign_room: Option<crate::app::shell_network_setup::campaign_room::CampaignRoom>,
    /// The WOL game channel's kind (`0x0077D940`/`0x0077D970`), fixed for
    /// the setup it hosts.
    room: NetworkRoom,
    /// The WOL start's tournament value and clan lists, taken at
    /// OnGameStart/Start Game Now for the accepted start.
    wol_start: WolStartTerms,
    send_requests: Vec<NetworkSendRequest>,
    /// 69AE90's effective display percents waiting for the LAN service;
    /// random-map milestones are already halved.
    loading_reports: Vec<i32>,
    /// Node addresses belong to the accepted participants, not discovery UI
    /// rows. Native node+28 is carried into697B70's House/connection binding.
    /// The IPv4 LAN adaptation uses the actual global-envelope source endpoint.
    endpoints: std::collections::BTreeMap<String, std::net::SocketAddrV4>,
    map_metadata: Option<NetworkMapMetadata>,
    /// Verified local content for the current advertised map. The lobby keeps
    /// the host's filename; only the frozen local load consumes this source.
    local_map_source: Option<crate::map::source::LoadedMapSource>,
    local_map_effective_digest: Option<String>,
    local_random_map_preview: Option<crate::map::source::LoadedMapSource>,
    proposed_seed: Option<u32>,
    max_ahead_frames: Option<i32>,
    return_context: NetworkReturnContext,
    phase: NetworkPhase,
}

/// The live session: who holds `GameMode`, human participant table, the
/// local participant's index and whether this client owns the match setup
/// (seed and player list).
///
/// Process start matches native: `GameMode` 0 with an empty participant list
/// (`0x0077B532`). The shell writes it at offline launch and network entry or
/// exit; a world takes it at install or saved-game restore. It persists after
/// a match exactly as the native global does: the finished world keeps
/// holding it until the shell writes it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MatchSession {
    mode: ModeHolder,
    offline_player_name: Option<String>,
    network: Option<NetworkSession>,
    host: bool,
    /// Process byte B07788, set by the first ReadMultiPlayerSettings call.
    /// It survives dialog, match and saved-game transitions.
    multiplayer_settings_initialized: bool,
    /// How many colours the loaded rules list for a lobby, refreshed before
    /// each setup dialog. A room with no more players than this keeps every
    /// colour to one player.
    lobby_color_count: usize,
    /// LatencyFudge `0xA8DB9C`, a zero-initialized process global. LAN
    /// start (`0x005DD2E4`/`0x005DD4A4`) writes 0, WOL Start Game Now
    /// (`0x007950C9..0x0079510B`) 1 or 2, and EVENT_LATENCYFUDGE
    /// (`0x004C7F4F`) the event value. FF reads it at init and Resume;
    /// MaxAhead pacing is transport.
    latency_fudge: i32,
    /// FF Resume's two event limiters (`0xA8EE40` latency, `0xA8EE30`
    /// speed): function statics that outlive every menu and match.
    pause_event_limiters: PauseEventLimiters,
}

/// One native wall-clock countdown `{start, clock, duration}` in 16 ms
/// buckets: `0x004F1B00` starts it, `0x004F1B20` reports the remainder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct WallBucketTimer {
    start: Option<u32>,
    duration: u32,
}

impl WallBucketTimer {
    pub(crate) fn start(now_bucket: u32, duration: u32) -> Self {
        Self {
            start: Some(now_bucket),
            duration,
        }
    }

    /// Diagnostic projection of the actual countdown, including recovery
    /// restarts. The native expiry comparison remains in remaining().
    pub(crate) fn deadline_bucket(self) -> Option<u32> {
        self.start.map(|start| start.wrapping_add(self.duration))
    }

    /// `0x004F1B20`: a never-started timer returns its duration; otherwise
    /// `duration - elapsed` while elapsed (wrapping) is below it, else 0.
    pub(crate) fn remaining(self, now_bucket: u32) -> u32 {
        match self.start {
            None => self.duration,
            Some(start) => {
                let elapsed = now_bucket.wrapping_sub(start) as i32;
                if elapsed < self.duration as i32 {
                    (self.duration as i32 - elapsed) as u32
                } else {
                    0
                }
            }
        }
    }
}

/// The FF Resume rate limiters. Both statics are constructed on the
/// callback's first WM_COMMAND (`0x004F1290..0x004F12D8`) as expired
/// zero-duration timers; each accepted event restarts its own for 900.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct PauseEventLimiters {
    pub(crate) latency: WallBucketTimer,
    pub(crate) speed: WallBucketTimer,
}

impl PauseEventLimiters {
    pub(crate) const COOLDOWN_BUCKETS: u32 = 900;
}

impl Default for MatchSession {
    fn default() -> Self {
        Self {
            mode: ModeHolder::Shell(SessionMode::Campaign),
            offline_player_name: None,
            network: None,
            host: false,
            multiplayer_settings_initialized: false,
            lobby_color_count: crate::skirmish_launch::HOUSE_COLOR_COUNT,
            latency_fudge: 0,
            pause_event_limiters: PauseEventLimiters::default(),
        }
    }
}

impl MatchSession {
    /// SessionClass__ReadMultiPlayerSettings 698161..6981DA consumes a Main
    /// RandomRanged(0,32768) before reading PortPool, even with an explicit
    /// PortPool key. Keep this shared stream effect at its process lifetime.
    /// Port allocation and firewall detection consume the resulting default
    /// only in transport services, outside the shell implementation.
    pub(crate) fn set_lobby_colors(
        &mut self,
        colors: crate::rules::skirmish_colors::SkirmishColorMode,
    ) {
        self.lobby_color_count = colors.lobby_color_count();
    }

    pub(crate) fn prepare_multiplayer_settings(&mut self, main: &crate::sim::rng::MainRngDraws) {
        if !self.multiplayer_settings_initialized {
            let _port_pool_default = 4096 + main.ranged(0, 32768);
            self.multiplayer_settings_initialized = true;
        }
    }

    /// The shell's launch write: campaign (`0x0052E175`) or skirmish
    /// (`0x0052E10F`). An offline session has at most the local human and is
    /// its own host. A campaign has no participant table.
    pub(crate) fn begin_offline(&mut self, mode: SessionMode, local_player_name: Option<String>) {
        debug_assert!(
            !mode.is_network(),
            "offline launch cannot start a network session"
        );
        self.begin_local_loading(mode, local_player_name);
    }

    /// A replay retains its recorded mode while the shell constructs a world.
    /// A recorded network mode admits no live participant table or connection;
    /// the installed world takes ownership through `hand_mode_to_world`.
    pub(crate) fn begin_replay(&mut self, mode: SessionMode, local_player_name: String) {
        self.begin_local_loading(mode, Some(local_player_name));
    }

    fn begin_local_loading(&mut self, mode: SessionMode, local_player_name: Option<String>) {
        self.mode = ModeHolder::Shell(mode);
        self.offline_player_name = local_player_name;
        self.network = None;
        self.host = true;
    }

    /// Load_Game67E4E8 restores the exact mode from the admitted saved session,
    /// whose restored world now holds it. An existing service visit of that
    /// mode may retain its return context; loading never fabricates a new
    /// network connection or participant admission.
    pub(crate) fn restore_from_save(&mut self, mode: SessionMode, local_player_name: Option<&str>) {
        if !mode.is_network() {
            self.begin_offline(mode, local_player_name.map(str::to_owned));
        } else {
            if self.network_mode() != Some(mode) {
                self.network = None;
            } else {
                self.return_to_network_lobby();
            }
            self.offline_player_name = local_player_name.map(str::to_owned);
        }
        self.mode = ModeHolder::World;
    }

    /// `GameMode` while the shell holds it; `None` while the installed world
    /// does ([`super::state::MatchState::session_mode`] reads either).
    pub(crate) fn shell_mode(&self) -> Option<SessionMode> {
        match self.mode {
            ModeHolder::Shell(mode) => Some(mode),
            ModeHolder::World => None,
        }
    }

    /// A fresh world built with the shell's `GameMode` takes it over at
    /// install.
    pub(crate) fn hand_mode_to_world(&mut self, world_mode: SessionMode) {
        if let ModeHolder::Shell(mode) = self.mode {
            debug_assert_eq!(
                mode, world_mode,
                "the loader builds the world with the shell's GameMode"
            );
        }
        self.mode = ModeHolder::World;
    }

    /// A world dropped without a successor hands `GameMode` back.
    pub(crate) fn retake_mode_from_world(&mut self, world_mode: SessionMode) {
        if self.mode == ModeHolder::World {
            self.mode = ModeHolder::Shell(world_mode);
        }
    }

    /// The mode of the active service visit, if any.
    fn network_mode(&self) -> Option<SessionMode> {
        self.network_return_context()
            .map(NetworkReturnContext::session_mode)
    }

    /// The local participant's handle; `None` for launches without one.
    pub(crate) fn local_player_name(&self) -> Option<&str> {
        if let Some(launch) = self.network_setup() {
            Some(launch.local_player().name.as_str())
        } else {
            self.offline_player_name.as_deref()
        }
    }

    #[cfg(test)]
    pub(crate) fn participant_count(&self) -> usize {
        self.network_setup()
            .map_or(usize::from(self.offline_player_name.is_some()), |launch| {
                launch.humans.len()
            })
    }

    /// Main menu writes GameMode3/4 before entering the service-dependent shell.
    pub(crate) fn enter_network(&mut self, return_context: NetworkReturnContext) {
        self.mode = ModeHolder::Shell(return_context.session_mode());
        self.host = false;
        self.offline_player_name = None;
        self.network = Some(NetworkSession {
            campaign_room: None,
            room: NetworkRoom::default(),
            wol_start: WolStartTerms::default(),
            send_requests: Vec::new(),
            loading_reports: Vec::new(),
            endpoints: Default::default(),
            map_metadata: None,
            local_map_source: None,
            local_map_effective_digest: None,
            local_random_map_preview: None,
            proposed_seed: None,
            max_ahead_frames: None,
            return_context,
            phase: NetworkPhase::lobby(),
        });
    }

    /// Adopt a service-admitted setup. Native LAN own-node creation precedes
    /// remote-node events (5E00B0); UI requests alone do not create participants.
    /// With `several_observers` the rows past the native eight start Closed.
    pub(crate) fn accept_network_setup(
        &mut self,
        mut launch: SkirmishLaunchSession,
        host: bool,
        observer_row: Option<u8>,
        several_observers: bool,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.network.is_some(), "no network visit is active");
        anyhow::ensure!(
            observer_row.is_none_or(|row| usize::from(row) < NETWORK_PLAYER_SLOT_COUNT),
            "observer reservation is outside setup rows"
        );
        anyhow::ensure!(
            !launch.humans.is_empty() && launch.local_player_index < launch.humans.len(),
            "network setup has no local participant"
        );
        anyhow::ensure!(
            launch.humans.len() + launch.opponents.len() <= NETWORK_PLAYER_SLOT_COUNT,
            "network setup exceeds the participant slots"
        );
        validate_network_rows(&launch)?;
        launch
            .opponents
            .sort_by_key(|opponent| opponent.raw_slot_index);
        launch.use_live_house_rows();
        let network = self.network.as_mut().expect("network mode owns a visit");
        anyhow::ensure!(
            !matches!(network.phase, NetworkPhase::Frozen { .. }),
            "accepted start already froze the setup"
        );
        let admission = (0..launch.humans.len())
            .map(|index| ParticipantAdmission {
                ready: false,
                map_status: u8::from(index == launch.local_player_index),
                reported_map_status: None,
            })
            .collect();
        let inactive_choices =
            std::array::from_fn(|_| native_network_row_defaults(&launch.mode, false));
        let display_ai = display_ai_rows(&launch);
        network.phase = NetworkPhase::Setup {
            display_ai,
            watching_sources: Vec::new(),
            capacity: 8,
            observer_row,
            several_observers,
            open_spectators: false,
            people_limit: NETWORK_PLAYER_SLOT_COUNT as u8,
            join_queue: several_observers,
            cooperative_rematch_seat: false,
            launch,
            admission,
            vacancies: [NetworkVacancy::Open; NETWORK_PLAYER_SLOT_COUNT],
            inactive_choices,
            inactive_ai_options: [NetworkAiOptions::INITIAL_INACTIVE; NETWORK_PLAYER_SLOT_COUNT],
        };
        self.host = host;
        // A VERA room's rows past its players are Closed spectator rows.
        let players = usize::from(self.network_player_row_limit());
        if let Some(NetworkPhase::Setup {
            vacancies,
            several_observers: true,
            ..
        }) = self.network.as_mut().map(|network| &mut network.phase)
        {
            vacancies[players..].fill(NetworkVacancy::Closed);
        }
        Ok(())
    }

    /// Bind the source of an admitted type3/type6 node. Discovery alone
    /// cannot create an endpoint for an unaccepted participant. Frozen starts
    /// retain the same identity/address binding until their cleanup.
    pub(crate) fn bind_network_endpoint(
        &mut self,
        name: &str,
        endpoint: std::net::SocketAddrV4,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.network_setup()
                .is_some_and(|launch| launch.humans.iter().any(|human| human.name == name)),
            "endpoint has no admitted network participant"
        );
        anyhow::ensure!(
            endpoint.port() != 0
                && !endpoint.ip().is_unspecified()
                && !endpoint.ip().is_broadcast()
                && !endpoint.ip().is_multicast(),
            "participant endpoint is not a direct IPv4 address"
        );
        let network = self.network.as_mut().expect("admitted network participant");
        anyhow::ensure!(
            !matches!(network.phase, NetworkPhase::Frozen { .. }),
            "cannot replace endpoint of an accepted start"
        );
        anyhow::ensure!(
            !network
                .endpoints
                .iter()
                .any(|(other, address)| other != name && *address == endpoint),
            "endpoint already belongs to another participant"
        );
        network.endpoints.insert(name.to_owned(), endpoint);
        Ok(())
    }

    pub(crate) fn network_endpoint(&self, source_index: usize) -> Option<std::net::SocketAddrV4> {
        let name = &self.network_setup()?.humans.get(source_index)?.name;
        self.network.as_ref()?.endpoints.get(name).copied()
    }

    pub(crate) fn network_endpoint_source(
        &self,
        endpoint: std::net::SocketAddrV4,
    ) -> Option<usize> {
        self.network_setup()?
            .humans
            .iter()
            .enumerate()
            .find_map(|(index, _)| {
                (self.network_endpoint(index) == Some(endpoint)).then_some(index)
            })
    }

    /// VERA: whether the network match in setup or started seats a house
    /// the computer runs, for the AI INI its rules agreement compares
    /// (`shell_network_setup::agreement::local`): a campaign room always,
    /// else its launch says (`SkirmishLaunchSession::seats_computers`). Its
    /// load answers alike (`LoadingStartup::ai_file`).
    pub(crate) fn network_seats_computers(&self) -> bool {
        self.campaign_room().is_some()
            || self
                .network_setup()
                .is_none_or(SkirmishLaunchSession::seats_computers)
    }

    pub(crate) fn network_setup(&self) -> Option<&SkirmishLaunchSession> {
        match &self.network.as_ref()?.phase {
            NetworkPhase::Lobby { .. } => None,
            NetworkPhase::Setup { launch, .. } => Some(launch),
            NetworkPhase::Frozen { startup, .. } | NetworkPhase::CampaignResult { startup, .. } => {
                Some(startup.launch_session())
            }
            NetworkPhase::CooperativeRound { launch, .. } => Some(launch),
        }
    }

    /// VERA: the Options switches a network match plays and its start
    /// agreement digests. A co-op campaign plays the original game's; a room
    /// plays its setup's `HostRules`: its host's, none in Quick Match
    /// (`live_service` drops what its host sends), each player's own in a
    /// launcher's room. None before a setup; Err for a host's retired rules.
    pub(crate) fn network_personal_rules(
        &self,
    ) -> Option<Result<crate::rules::personal_rules::PersonalRules, String>> {
        if self.campaign_room().is_some() {
            return Some(Ok(Default::default()));
        }
        Some(self.network_setup()?.options.host_rules.personal_rules())
    }

    pub(crate) fn network_authored_start(
        &self,
    ) -> Option<&crate::app::match_bootstrap::PreparedCampaignStart> {
        match &self.network.as_ref()?.phase {
            NetworkPhase::Frozen { startup, .. } | NetworkPhase::CampaignResult { startup, .. } => {
                startup.authored_campaign()
            }
            _ => None,
        }
    }

    fn editable_network_setup(&mut self) -> anyhow::Result<&mut SkirmishLaunchSession> {
        match self.network.as_mut().map(|network| &mut network.phase) {
            Some(
                NetworkPhase::Setup { launch, .. } | NetworkPhase::CooperativeRound { launch, .. },
            ) => Ok(launch),
            _ => anyhow::bail!("network setup is not editable"),
        }
    }

    /// VERA online transport adaptation: every client numbers the authenticated
    /// room roster identically before installing host options or relay peers.
    /// Native WOL creates the local node first (7A7627), which is a different
    /// order on each machine. Identity and admission stay owned here; neither
    /// an options packet nor a relay grant may add an unadmitted participant.
    pub(crate) fn canonicalize_online_roster(&mut self, names: &[String]) -> anyhow::Result<()> {
        let Some(NetworkSession {
            return_context: NetworkReturnContext::Wol,
            phase: NetworkPhase::Setup {
                launch, admission, ..
            },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("online roster requires an editable WOL setup");
        };
        anyhow::ensure!(
            names.len() == launch.humans.len(),
            "online roster membership changed"
        );
        let mut order = Vec::with_capacity(names.len());
        for name in names {
            let index = launch
                .humans
                .iter()
                .position(|human| &human.name == name)
                .ok_or_else(|| anyhow::anyhow!("online roster has an unadmitted participant"))?;
            anyhow::ensure!(
                !order.contains(&index),
                "online roster repeats a participant"
            );
            order.push(index);
        }
        let local_index = order
            .iter()
            .position(|index| *index == launch.local_player_index)
            .ok_or_else(|| anyhow::anyhow!("online roster has no local participant"))?;
        let humans = order
            .iter()
            .map(|index| launch.humans[*index].clone())
            .collect();
        let reordered_admission = order.iter().map(|index| admission[*index]).collect();
        launch.humans = humans;
        launch.local_player_index = local_index;
        *admission = reordered_admission;
        Ok(())
    }

    /// Type3 roster admission appends a remote node. It does not reorder the
    /// accepted source vector or clear unrelated participant acknowledgements.
    pub(crate) fn add_network_participant(
        &mut self,
        human: crate::skirmish_launch::SkirmishHumanSlot,
    ) -> anyhow::Result<usize> {
        let host = self.host;
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    admission,
                    display_ai,
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        anyhow::ensure!(
            launch.humans.len() + if host { launch.opponents.len() } else { 0 }
                < NETWORK_PLAYER_SLOT_COUNT,
            "network setup has no free slot"
        );
        // A WOL guest's node has no row until the host's 'L' binds one.
        anyhow::ensure!(
            human.raw_slot_index.is_none_or(|row| {
                usize::from(row) < NETWORK_PLAYER_SLOT_COUNT
                    && !launch
                        .humans
                        .iter()
                        .any(|entry| entry.raw_slot_index == Some(row))
            }),
            "participant row is occupied or out of range"
        );
        if let Some(row) = human.raw_slot_index {
            display_ai[usize::from(row)] = None;
        }
        let index = launch.humans.len();
        launch.humans.push(human);
        admission.push(ParticipantAdmission {
            ready: false,
            map_status: 0,
            reported_map_status: None,
        });
        self.keep_spectator_row()?;
        // A newcomer who gets a seat in a VERA room with teams joins the
        // team with the fewest players.
        if self.host && self.network_team_room() {
            self.editable_network_setup()?.humans[index].slot.team = LaunchTeam::None;
            self.keep_players_on_teams()?;
        }
        Ok(index)
    }

    pub(crate) fn remove_network_participant(&mut self, index: usize) -> anyhow::Result<()> {
        let Some(NetworkSession {
            phase: NetworkPhase::Setup {
                launch, admission, ..
            },
            endpoints,
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        anyhow::ensure!(
            index < launch.humans.len() && index != launch.local_player_index,
            "remote removal requires a remote participant"
        );
        let removed = launch.humans.remove(index);
        endpoints.remove(&removed.name);
        launch.options.queue.retain(|name| *name != removed.name);
        admission.remove(index);
        if index < launch.local_player_index {
            launch.local_player_index -= 1;
        }
        self.keep_spectator_row()
    }

    pub(crate) fn network_return_context(&self) -> Option<NetworkReturnContext> {
        self.network.as_ref().map(|network| network.return_context)
    }

    pub(crate) fn leave_network(&mut self) {
        self.network = None;
        self.mode = ModeHolder::Shell(SessionMode::Campaign);
        self.host = false;
        self.offline_player_name = None;
    }

    /// Whether this client owns the match setup: it reads the seed and fixes
    /// the participant list. Every offline session is its own host.
    pub(crate) fn host(&self) -> bool {
        self.host
    }
}

/// What a WOL start hands the match: OnGameStart's `[0xB779C4]` (the
/// channel's tournament value, 2 in a clan room) and Start Game Now's two
/// clan name lists `[0xB77A04]`/`[0xB76568]`; `quick_coop` is the WOL mode
/// `[0xA8B244] == 2`, which scenario load reads at `0x0068755E`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WolStartTerms {
    pub(crate) tournament: i32,
    pub(crate) squad_lists: [Vec<String>; 2],
    pub(crate) quick_coop: bool,
}

/// A WOL game channel's kind from its record: tournament `+0x14` and the
/// clan bit `+0x20 & 0x100` of the current channel `[0xB76B10]`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct NetworkRoom {
    pub(crate) tournament: bool,
    pub(crate) clan: bool,
}

impl NetworkRoom {
    /// `0x0077D940`: a tournament or clan room (GameMode 4 only).
    pub(crate) const fn tournament_or_clan(self) -> bool {
        self.tournament || self.clan
    }
}

/// `0x005E6440` (and its inline copies): LAN 8; WOL 6, or 2 in a
/// tournament room that is not a clan room.
fn row_limit(session_mode: SessionMode, room: NetworkRoom) -> i32 {
    match session_mode {
        SessionMode::Wol if room.tournament && !room.clan => 2,
        SessionMode::Wol => 6,
        _ => 8,
    }
}

/// `0x005EC3A0` (`0x005EC3C4..0x005EC467`): map capacity (`0x005E6520`),
/// the mode's vtable+98 limit, then in a WOL clan room an even count, in a
/// tournament room 2. The row limit applies unless the mode lists in
/// Skirmish (vtable+40) outside a tournament or clan room.
fn row_capacity(
    session_mode: SessionMode,
    room: NetworkRoom,
    mode: &crate::skirmish_launch::SkirmishLaunchMode,
    capacity: i32,
) -> i32 {
    let extended = capacity > 8 && !(session_mode == SessionMode::Wol && room.tournament_or_clan());
    let mut capacity = capacity.min(mode.class.policy().setup_player_limit.map_or(
        if extended {
            NETWORK_PLAYER_SLOT_COUNT as i32
        } else {
            8
        },
        |limit| limit as i32,
    ));
    if session_mode == SessionMode::Wol {
        if room.clan {
            capacity &= !1;
        } else if room.tournament {
            capacity = 2;
        }
    }
    if mode.class.policy().lists_in_skirmish
        && !(session_mode == SessionMode::Wol && room.tournament_or_clan())
    {
        capacity
    } else {
        capacity.min(if extended {
            NETWORK_PLAYER_SLOT_COUNT as i32
        } else {
            row_limit(session_mode, room)
        })
    }
}

/// `0x005EC4A3..0x005EC4EF`: the host's observer row adds a row when the
/// row limit was skipped or the capacity is under it (WOL limits under 6
/// count one more).
fn host_extra_row(
    session_mode: SessionMode,
    room: NetworkRoom,
    mode: &crate::skirmish_launch::SkirmishLaunchMode,
    capacity: i32,
) -> bool {
    let wol_room = session_mode == SessionMode::Wol && room.tournament_or_clan();
    if mode.class.policy().lists_in_skirmish && !wol_room {
        return true;
    }
    let limit = row_limit(session_mode, room);
    let limit = if session_mode == SessionMode::Wol && limit < 6 {
        limit + 1
    } else {
        limit
    };
    capacity < limit
}

/// Mode `+0x2C` (`0x5D5DC0`): the team a pick outside the Team list takes,
/// Team 0 when the mode must ally, else none.
fn mode_default_team(mode: &crate::skirmish_launch::SkirmishLaunchMode) -> LaunchTeam {
    if mode.must_ally {
        LaunchTeam::Team(0)
    } else {
        LaunchTeam::None
    }
}

#[derive(Clone, Copy)]
pub(crate) enum PeerChoicePolicy<'a> {
    Lan,
    /// `teams`: a VERA room's Teams count while it is set
    /// ([`MatchSession::network_teams`]); without it, gamemd's A to D.
    Wol {
        countries: Option<&'a [crate::skirmish_launch::LaunchCountry]>,
        teams: Option<u8>,
    },
}

/// Shared69B600/5E9720 collision checks; caller selects the native country/team policy.
/// A Quick Match host seats the matchbot's guest through it too.
/// `listed_colors` is how many colours the lobby lists.
pub(crate) fn apply_peer_choices(
    launch: &mut SkirmishLaunchSession,
    inactive_ai_options: &[NetworkAiOptions; NETWORK_PLAYER_SLOT_COUNT],
    index: usize,
    choices: SkirmishLocalSlot,
    observer: bool,
    policy: PeerChoicePolicy<'_>,
    listed_colors: usize,
) -> PeerChoicesChange {
    let mut change = PeerChoicesChange::default();
    let current = launch.humans[index].slot.clone();
    let native_country = |slot: &SkirmishLocalSlot| (!slot.country_random).then_some(slot.country);
    let countries = match policy {
        PeerChoicePolicy::Lan => None,
        PeerChoicePolicy::Wol { countries, .. } => countries,
    };
    let allowed = |country: Option<crate::skirmish_launch::LaunchCountry>| {
        countries.is_none_or(|list| country.is_none_or(|country| list.contains(&country)))
    };
    let mut slot = current.clone();
    // An observer's country is -3 on both sides.
    if observer {
    } else if allowed(native_country(&choices)) {
        if native_country(&choices) != native_country(&current) {
            slot.country = choices.country;
            slot.country_random = choices.country_random;
            change.options = true;
        }
    } else {
        if !allowed(native_country(&current)) {
            slot.country_random = true;
        }
        change.options = true;
    }
    let native_color = |slot: &SkirmishLocalSlot| (!slot.color_random).then_some(slot.color_index);
    if native_color(&choices) != native_color(&current) {
        // 0x69B600: every node of the session and every AI row.
        let taken = native_color(&choices).is_some_and(|color| {
            network_color_in_use(launch, inactive_ai_options, color, listed_colors)
        });
        slot.color_random = choices.color_random || taken;
        if !slot.color_random {
            slot.color_index = choices.color_index;
        }
        change.options = true;
    }
    if choices.start_position != current.start_position {
        // 0x5E9720: the nodes on a row and every AI row.
        let taken = network_start_in_use(launch, inactive_ai_options, choices.start_position);
        slot.start_position = if taken {
            crate::skirmish_launch::LaunchStartPosition::Auto
        } else {
            choices.start_position
        };
        change.options = true;
    }
    if choices.team != current.team {
        // Mode +0x34 (0x5D5DE0) admits only 0..3; +0x2C (0x5D5DC0) is
        // Team 0 under MustAlly, else none. A VERA room with Teams set admits
        // its own teams and leaves any other pick on no team, which
        // `keep_players_on_teams` seats on the smallest team.
        slot.team = match policy {
            PeerChoicePolicy::Lan => choices.team,
            PeerChoicePolicy::Wol {
                teams: Some(teams), ..
            } => match choices.team {
                LaunchTeam::Team(team) if team < teams => choices.team,
                _ => LaunchTeam::None,
            },
            PeerChoicePolicy::Wol { teams: None, .. } => match choices.team {
                LaunchTeam::Team(team) if team < crate::skirmish_launch::NATIVE_TEAMS => {
                    choices.team
                }
                _ => mode_default_team(&launch.mode),
            },
        };
        change.options = true;
    }
    launch.humans[index].slot = slot;
    change
}

/// `listed_colors` is how many colours the lobby lists: a room with more
/// players than that has to share, so nothing counts as taken there.
fn network_color_in_use(
    launch: &SkirmishLaunchSession,
    inactive: &[NetworkAiOptions; NETWORK_PLAYER_SLOT_COUNT],
    color: u8,
    listed_colors: usize,
) -> bool {
    if launch.humans.len() + launch.opponents.len() > listed_colors {
        return false;
    }
    launch
        .humans
        .iter()
        .any(|human| !human.slot.color_random && human.slot.color_index == color)
        || launch
            .opponents
            .iter()
            .any(|ai| !ai.color_random && ai.color_index == color)
        || inactive.iter().enumerate().any(|(row, raw)| {
            raw.color == i32::from(color)
                && !launch
                    .opponents
                    .iter()
                    .any(|ai| usize::from(ai.raw_slot_index) == row)
        })
}

fn network_start_in_use(
    launch: &SkirmishLaunchSession,
    inactive: &[NetworkAiOptions; NETWORK_PLAYER_SLOT_COUNT],
    start: crate::skirmish_launch::LaunchStartPosition,
) -> bool {
    let crate::skirmish_launch::LaunchStartPosition::Position(position) = start else {
        return false;
    };
    // VERA shared control: when the mode policy enables shared starts, a player
    // may take a start other players hold to share their House.
    let shared = launch.mode.class.policy().shared_start_positions;
    !shared
        && launch
            .humans
            .iter()
            .any(|human| human.raw_slot_index.is_some() && human.slot.start_position == start)
        || launch.opponents.iter().any(|ai| ai.start_position == start)
        || inactive.iter().enumerate().any(|(row, raw)| {
            raw.start == i32::from(position)
                && !launch
                    .opponents
                    .iter()
                    .any(|ai| usize::from(ai.raw_slot_index) == row)
        })
}

fn native_network_row_defaults(
    mode: &crate::skirmish_launch::SkirmishLaunchMode,
    ai: bool,
) -> SkirmishLocalSlot {
    // Common mode+28/+2C/+30 defaults5D5DB0..5D5DD0, applied by5E9E70
    // and5EE510. Mode-specific country restrictions are admitted separately.
    SkirmishLocalSlot {
        country: crate::skirmish_launch::LaunchCountry::America,
        country_random: true,
        color_index: 0,
        color_random: true,
        start_position: crate::skirmish_launch::LaunchStartPosition::Auto,
        team: if ai && mode.allies_allowed {
            crate::skirmish_launch::LaunchTeam::Team(3)
        } else if !ai && mode.must_ally {
            crate::skirmish_launch::LaunchTeam::Team(0)
        } else {
            crate::skirmish_launch::LaunchTeam::None
        },
    }
}

/// The AI display rows a setup's AI arrays give: each AI's difficulty on its
/// row, unless a human holds that row, as the host's own rows show it
/// ([`MatchSession::add_network_participant`] clears a row a human takes).
fn display_ai_rows(
    launch: &SkirmishLaunchSession,
) -> [Option<crate::skirmish_launch::AiDifficulty>; NETWORK_PLAYER_SLOT_COUNT] {
    std::array::from_fn(|row| {
        let row = row as u8;
        if launch
            .humans
            .iter()
            .any(|human| human.raw_slot_index == Some(row))
        {
            return None;
        }
        launch
            .opponents
            .iter()
            .find(|ai| ai.raw_slot_index == row)
            .map(|ai| ai.difficulty)
    })
}

/// Each packet owns one domain. Until both arrive, a display human and a raw
/// AI slot may share a row; only final start admission requires disjoint rows.
fn validate_network_option_rows(launch: &SkirmishLaunchSession) -> anyhow::Result<()> {
    anyhow::ensure!(
        launch.local_player().raw_slot_index.is_some(),
        "local participant must have a visible setup row"
    );
    for rows in [
        launch
            .humans
            .iter()
            .filter_map(|human| human.raw_slot_index)
            .collect::<Vec<_>>(),
        launch
            .opponents
            .iter()
            .map(|ai| ai.raw_slot_index)
            .collect::<Vec<_>>(),
    ] {
        let mut occupied = [false; NETWORK_PLAYER_SLOT_COUNT];
        for row in rows {
            anyhow::ensure!(
                usize::from(row) < NETWORK_PLAYER_SLOT_COUNT && !occupied[usize::from(row)],
                "duplicate or invalid network row"
            );
            occupied[usize::from(row)] = true;
        }
    }
    Ok(())
}

fn validate_network_rows(launch: &SkirmishLaunchSession) -> anyhow::Result<()> {
    anyhow::ensure!(
        launch.local_player().raw_slot_index.is_some(),
        "local participant must have a visible setup row"
    );
    let mut occupied = [false; NETWORK_PLAYER_SLOT_COUNT];
    for row in launch
        .humans
        .iter()
        .filter_map(|human| human.raw_slot_index)
        .chain(launch.opponents.iter().map(|ai| ai.raw_slot_index))
    {
        anyhow::ensure!(
            usize::from(row) < NETWORK_PLAYER_SLOT_COUNT && !occupied[usize::from(row)],
            "network rows must be distinct and below30"
        );
        occupied[usize::from(row)] = true;
    }
    Ok(())
}

/// A start's admission, shared by the first start and a co-op
/// continuation: a consistent House roster, resolved assignments and an
/// exact map. A Cooperative human sits in one of BC6's eight raw slots
/// (`0x89E000[8]`), which the round after the mission indexes by row.
fn admit_network_start_roster(launch: &SkirmishLaunchSession) -> anyhow::Result<()> {
    validate_network_rows(launch)?;
    anyhow::ensure!(
        !launch.mode.class.uses_cooperative_setup()
            || launch
                .humans
                .iter()
                .filter_map(|human| human.raw_slot_index)
                .all(|row| usize::from(row) < crate::net::lan_messages::cooperative::ROWS),
        "a Cooperative human sits past the round's eight rows"
    );
    anyhow::ensure!(
        launch.house_roster_is_consistent(),
        "network House constructors disagree with the participant rows"
    );
    crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(launch.clone())
        .map_err(|error| anyhow::anyhow!("network assignments unresolved: {error}"))?;
    anyhow::ensure!(
        launch
            .selected_map_file
            .as_deref()
            .is_some_and(|file| !file.is_empty()
                && file.trim() == file
                && !file.eq_ignore_ascii_case("auto")),
        "network start requires an exact map selection"
    );
    Ok(())
}

#[cfg(all(test, tests_app_match))]
mod tests;
