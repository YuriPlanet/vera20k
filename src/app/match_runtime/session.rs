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
    LaunchTeam, NETWORK_PLAYER_SLOT_COUNT, SkirmishAiSlot, SkirmishLaunchOptions,
    SkirmishLaunchSession, SkirmishLocalSlot,
};
use std::sync::Arc;

pub(crate) use crate::sim::scenario_session::ScenarioGameMode as SessionMode;

mod cooperative_round;
pub(crate) use cooperative_round::{
    CooperativeRoundEvent, CooperativeRoundOptions, CooperativeRoundState, CooperativeRoundStep,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkReturnContext {
    LanLobby,
    /// The WOL custom lobby `0x113` (Custom Match, mode 4).
    Wol,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkVacancy {
    Open,
    Closed,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NetworkAiOptions {
    pub difficulty: i32,
    pub country: i32,
    pub color: i32,
    pub start: i32,
    pub team: i32,
}

impl NetworkAiOptions {
    /// An active row's words, as the options string writes them.
    pub(crate) fn of(ai: &crate::skirmish_launch::SkirmishAiSlot) -> Self {
        Self {
            difficulty: ai.difficulty.as_i32(),
            country: if ai.country_random {
                -2
            } else {
                i32::from(ai.country.raw_index())
            },
            color: if ai.color_random {
                -2
            } else {
                i32::from(ai.color_index)
            },
            start: match ai.start_position {
                crate::skirmish_launch::LaunchStartPosition::Auto => -2,
                crate::skirmish_launch::LaunchStartPosition::Position(index) => i32::from(index),
            },
            team: match ai.team {
                crate::skirmish_launch::LaunchTeam::None => -2,
                crate::skirmish_launch::LaunchTeam::Team(index) => i32::from(index),
            },
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
        if local_present || file.eq_ignore_ascii_case("RandMap.Sed") {
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NetworkPhase {
    Lobby,
    Setup {
        launch: SkirmishLaunchSession,
        admission: Vec<ParticipantAdmission>,
        vacancies: [NetworkVacancy; NETWORK_PLAYER_SLOT_COUNT],
        /// Authoritative only while the corresponding row is vacant. Entering
        /// a human/AI row makes its canonical participant choices authoritative.
        inactive_choices: [SkirmishLocalSlot; NETWORK_PLAYER_SLOT_COUNT],
        inactive_ai_options: [NetworkAiOptions; NETWORK_PLAYER_SLOT_COUNT],
        /// A8DA90 AI display pointers. Roster packets change these without
        /// changing Session's AI arrays; global packets may arrive reordered.
        display_ai: [Option<crate::skirmish_launch::AiDifficulty>; NETWORK_PLAYER_SLOT_COUNT],
        /// Native831700/8316A4: prior capacity and independent reserved observer row.
        capacity: i32,
        observer_row: Option<u8>,
        /// VERA's own rooms (`SetupDialog::modern`): the rows past the
        /// players are spectator rows, Closed unless open for or held by an
        /// observer. Native diagnostics keep the one reserved row above.
        several_observers: bool,
        /// The host of a VERA room keeps one spectator row open after its
        /// spectators while no AI plays: a game with observers has no AI
        /// (`0x5EC9F0`'s TXT_NO_AI_WITH_OBSERVER).
        open_spectators: bool,
        /// The most people a VERA room holds, players and spectators, its
        /// host's Create Game choice.
        people_limit: u8,
        /// A VERA room queues the spectators who ask to play while no seat
        /// is free (`launch.options.queue`), unless the online lobby
        /// refused the queue.
        join_queue: bool,
    },
    Frozen {
        startup: Arc<crate::app::match_bootstrap::PreparedNetworkStartup>,
        /// Live source membership indexes the immutable historical launch.
        connected: Vec<usize>,
    },
    /// The co-op result dialog BC6 between missions: the finished start,
    /// the round's own roster (the next mission's, which the master's
    /// `+0x68`/`0x0069B8C0` or a received `0x32` changes) and its readiness.
    CooperativeRound {
        previous: Arc<crate::app::match_bootstrap::PreparedNetworkStartup>,
        connected: Vec<usize>,
        launch: SkirmishLaunchSession,
        round: CooperativeRoundState,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NetworkLoadingRequest {
    ///69AE90's effective display percent; random-map milestones are already halved.
    ReportProgress(i32),
    Disconnect {
        source_index: usize,
        reason: i32,
    },
    /// The kick proposal packet `0x1C` (0x0064AAE0) to every other node.
    ProposeKick {
        target_source: usize,
    },
    /// The in-battle chat packet `0x0C` (`0x0055EDBF..0x0055F04E`) to these
    /// participants. The packet's name and colour are the local player's.
    Chat {
        recipients: Vec<usize>,
        text: String,
    },
    /// The taunt packet `0x1D` (`0x00536350`) to these participants; `byte`
    /// is `(country << 4) | n` at packet `+0x30`.
    Taunt {
        recipients: Vec<usize>,
        byte: u8,
    },
    /// A beacon record `0x20`/`0x21` to every other node
    /// (`0x00430E0C..0x00430E4E`, `0x004313C2..0x004313F5`).
    Beacon(crate::net::lan_messages::ingame::BeaconRecord),
    /// BC6's `0x26` to every node.
    CooperativeReady,
    /// BC6's `0x24` (Quit) to every node.
    CooperativeLeave,
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
    /// The WOL game channel's kind (`0x0077D940`/`0x0077D970`), fixed for
    /// the setup it hosts.
    room: NetworkRoom,
    /// The WOL start's tournament value and clan lists, taken at
    /// OnGameStart/Start Game Now for the accepted start.
    wol_start: WolStartTerms,
    loading_requests: Vec<NetworkLoadingRequest>,
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
/// local participant's index, whether this client owns the match setup (seed
/// and player list), and the retail sync-checksum history the multiplayer
/// queue fills.
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
            room: NetworkRoom::default(),
            wol_start: WolStartTerms::default(),
            loading_requests: Vec::new(),
            endpoints: Default::default(),
            map_metadata: None,
            local_map_source: None,
            local_map_effective_digest: None,
            local_random_map_preview: None,
            proposed_seed: None,
            max_ahead_frames: None,
            return_context,
            phase: NetworkPhase::Lobby,
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
        launch.rebuild_house_roster();
        let network = self.network.as_mut().expect("network mode owns a visit");
        anyhow::ensure!(
            !matches!(network.phase, NetworkPhase::Frozen { .. }),
            "accepted start already froze the setup"
        );
        let admission = (0..launch.humans.len())
            .map(|index| ParticipantAdmission {
                ready: false,
                map_status: u8::from(index == launch.local_player_index),
            })
            .collect();
        let inactive_choices =
            std::array::from_fn(|_| native_network_row_defaults(&launch.mode, false));
        let display_ai = std::array::from_fn(|row| {
            launch
                .opponents
                .iter()
                .find(|ai| usize::from(ai.raw_slot_index) == row)
                .map(|ai| ai.difficulty)
        });
        network.phase = NetworkPhase::Setup {
            display_ai,
            capacity: 8,
            observer_row,
            several_observers,
            open_spectators: false,
            people_limit: NETWORK_PLAYER_SLOT_COUNT as u8,
            join_queue: several_observers,
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

    pub(crate) fn network_setup(&self) -> Option<&SkirmishLaunchSession> {
        match &self.network.as_ref()?.phase {
            NetworkPhase::Lobby => None,
            NetworkPhase::Setup { launch, .. } => Some(launch),
            NetworkPhase::Frozen { startup, .. } => Some(startup.launch_session()),
            NetworkPhase::CooperativeRound { launch, .. } => Some(launch),
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
        launch.rebuild_house_roster();
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
        });
        launch.rebuild_house_roster();
        self.keep_spectator_row()?;
        // A newcomer who gets a seat in a VERA room with Teams set joins the
        // team with the fewest players.
        if self.host && self.network_teams().is_some() {
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
        launch.rebuild_house_roster();
        self.keep_spectator_row()
    }

    /// The WOL host's private-options `R` arm (`0x007A8260`, `0x7A83CC..
    /// 0x7A8BD2`) for the participant at `index`. A node without a row is
    /// ignored. An observer asking to play moves to the first Open row
    /// (`0x005E9240`), a player asking to observe moves to the observer row;
    /// with no such row (`0x005E9510` / `0x005E9560`) its own row is vacated
    /// instead (`0x005EC9F0` mode 0) and nothing else is checked. Otherwise
    /// each field is checked against the session: the country by the mode
    /// (`+0x1C`, here `countries` for Cooperative, else any; the current one,
    /// else the mode's `+0x28` Random), a colour or start already held
    /// becomes Random (`0x0069B600`, `0x005E9720`), a team outside 0..3
    /// takes the mode default (`+0x34`/`+0x2C`). In a VERA room with Teams
    /// set, a pick outside its teams is no team instead, and the room seats
    /// the player on its smallest team. No sound, text or reply.
    pub(crate) fn accept_network_peer_choices(
        &mut self,
        index: usize,
        choices: SkirmishLocalSlot,
        observer: bool,
        countries: Option<&[crate::skirmish_launch::LaunchCountry]>,
    ) -> anyhow::Result<PeerChoicesChange> {
        anyhow::ensure!(self.host, "only the host checks a peer's choices");
        let human = self
            .network_setup()
            .and_then(|launch| launch.humans.get(index))
            .ok_or_else(|| anyhow::anyhow!("unknown participant"))?;
        anyhow::ensure!(
            index != self.network_setup().expect("checked").local_player_index,
            "the host's own row is not a peer's"
        );
        let Some(row) = human.raw_slot_index else {
            return Ok(PeerChoicesChange::default());
        };
        let teams = self.network_teams();
        let mut change = self.accept_peer_choices(
            index,
            row,
            choices,
            observer,
            PeerChoicePolicy::Wol { countries, teams },
        )?;
        change.options |= self.keep_players_on_teams()?;
        Ok(change)
    }

    /// LAN Type18 receiver5E0D1C..5E0F20: writes country and team directly,
    /// with the same color/start collision checks as WOL. It does not perform
    /// WOL's observer row moves or mode country/team validation; VERA's rooms
    /// move a guest asking to observe (country -3) or to play as WOL does.
    pub(crate) fn accept_lan_peer_choices(
        &mut self,
        index: usize,
        choices: SkirmishLocalSlot,
        observer: bool,
    ) -> anyhow::Result<PeerChoicesChange> {
        anyhow::ensure!(self.host, "only the host checks LAN peer choices");
        let several = self.network_several_observers();
        let launch = self.editable_network_setup()?;
        anyhow::ensure!(
            index != launch.local_player_index,
            "the host's own row is not a peer's"
        );
        let human = launch
            .humans
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("unknown participant"))?;
        let Some(row) = human.raw_slot_index else {
            return Ok(PeerChoicesChange::default());
        };
        let observer = if several {
            observer
        } else {
            human.is_observer()
        };
        let mut change =
            self.accept_peer_choices(index, row, choices, observer, PeerChoicePolicy::Lan)?;
        change.options |= self.keep_players_on_teams()?;
        Ok(change)
    }

    fn accept_peer_choices(
        &mut self,
        index: usize,
        row: u8,
        choices: SkirmishLocalSlot,
        observer: bool,
        policy: PeerChoicePolicy<'_>,
    ) -> anyhow::Result<PeerChoicesChange> {
        let was_observer = self.network_setup().expect("checked").humans[index].is_observer();
        let observer_row = self.network_observer_row();
        let mut change = PeerChoicesChange::default();
        if was_observer && !observer {
            change.roster = true;
            change.options = true;
            if self.network_no_open_player_row() {
                // VERA's rooms keep the spectator watching, in the join
                // queue for the next free seat.
                if self.network_several_observers() {
                    self.queue_network_spectator(index)?;
                } else {
                    self.set_network_row_kind(row, NetworkRowKind::OpenObserver)?;
                }
                return Ok(change);
            }
            // 0x7A8838..0x7A88A4: country Random and a player role, the
            // observer row opened for observers, then 0x5E9240's row. The
            // country check still compares against the -3 read at 0x7A865F,
            // so the options always go out (0x7A8905..0x7A8915). In VERA's
            // rooms the observer's own row is the one it leaves.
            let left = if self.network_several_observers() {
                row
            } else {
                observer_row.unwrap_or(row)
            };
            self.set_network_row_kind(left, NetworkRowKind::OpenObserver)?;
            let free = self
                .network_free_row(false)
                .ok_or_else(|| anyhow::anyhow!("no Open row after 0x5E9510"))?;
            let human = &mut self.editable_network_setup()?.humans[index];
            human.set_observer(false);
            human.slot.country_random = true;
            human.raw_slot_index = Some(free);
            self.keep_spectator_row()?;
            self.leave_network_queue(index)?;
        } else if was_observer {
            // A spectator asking to watch leaves the join queue.
            change.options |= self.leave_network_queue(index)?;
        } else if observer {
            change.roster = true;
            change.options = true;
            let target = self.network_free_row(true);
            let several = self.network_several_observers();
            // VERA's rooms keep the player seated without an open
            // spectator row.
            if several && target.is_none() {
                return Ok(change);
            }
            self.set_network_row_kind(row, NetworkRowKind::Open)?;
            if let Some(target) = target {
                // 0x7A89F4..0x7A8A6F: the node goes to [0x8316A4]; in VERA's
                // rooms, to the open observer row 0x5E9240 found.
                let human = &mut self.editable_network_setup()?.humans[index];
                human.set_observer(true);
                human.raw_slot_index = Some(if several {
                    target
                } else {
                    observer_row.unwrap_or(target)
                });
            }
            self.editable_network_setup()?.rebuild_house_roster();
            self.keep_spectator_row()?;
            return Ok(change);
        }
        let applied = self.apply_peer_choice_fields(index, choices, observer, policy)?;
        change.options |= applied.options;
        Ok(change)
    }

    fn apply_peer_choice_fields(
        &mut self,
        index: usize,
        choices: SkirmishLocalSlot,
        observer: bool,
        policy: PeerChoicePolicy<'_>,
    ) -> anyhow::Result<PeerChoicesChange> {
        let listed_colors = self.lobby_color_count;
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    inactive_ai_options,
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        Ok(apply_peer_choices(
            launch,
            inactive_ai_options,
            index,
            choices,
            observer,
            policy,
            listed_colors,
        ))
    }

    pub(crate) fn network_vacancy(&self, row: u8) -> Option<NetworkVacancy> {
        let NetworkPhase::Setup {
            launch,
            vacancies,
            display_ai,
            ..
        } = &self.network.as_ref()?.phase
        else {
            return None;
        };
        if usize::from(row) >= NETWORK_PLAYER_SLOT_COUNT
            || launch
                .humans
                .iter()
                .any(|human| human.raw_slot_index == Some(row))
            || display_ai[usize::from(row)].is_some()
        {
            return None;
        }
        Some(vacancies[usize::from(row)])
    }

    pub(crate) fn network_display_ai(
        &self,
        row: u8,
    ) -> Option<crate::skirmish_launch::AiDifficulty> {
        let NetworkPhase::Setup { display_ai, .. } = &self.network.as_ref()?.phase else {
            return None;
        };
        display_ai.get(usize::from(row)).copied().flatten()
    }

    ///69B600 shared by peer-choice changes and5E96A0 new-node admission.
    pub(crate) fn network_color_in_use(&self, color: u8) -> bool {
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    inactive_ai_options,
                    ..
                },
            ..
        }) = self.network.as_ref()
        else {
            return false;
        };
        network_color_in_use(launch, inactive_ai_options, color, self.lobby_color_count)
    }

    pub(crate) fn network_row_choices(&self, row: u8) -> Option<SkirmishLocalSlot> {
        let NetworkPhase::Setup {
            launch,
            inactive_choices,
            ..
        } = &self.network.as_ref()?.phase
        else {
            return None;
        };
        if let Some(human) = launch
            .humans
            .iter()
            .find(|human| human.raw_slot_index == Some(row))
        {
            return Some(human.slot.clone());
        }
        if self.network_display_ai(row).is_some()
            && let Some(ai) = launch.opponents.iter().find(|ai| ai.raw_slot_index == row)
        {
            return Some(SkirmishLocalSlot {
                country: ai.country,
                country_random: ai.country_random,
                color_index: ai.color_index,
                color_random: ai.color_random,
                start_position: ai.start_position,
                team: ai.team,
            });
        }
        inactive_choices.get(usize::from(row)).cloned()
    }

    /// Build_Options_String5DBD5C reads the AI arrays even where a human
    /// occupies the same display row. Human start/team never leak into them.
    pub(crate) fn network_ai_options(&self, row: u8) -> Option<NetworkAiOptions> {
        let NetworkPhase::Setup {
            launch,
            inactive_ai_options,
            ..
        } = &self.network.as_ref()?.phase
        else {
            return None;
        };
        if let Some(ai) = launch.opponents.iter().find(|ai| ai.raw_slot_index == row) {
            return Some(NetworkAiOptions::of(ai));
        }
        inactive_ai_options.get(usize::from(row)).copied()
    }

    ///5E5BAB..5E5C3C retains every received AI word. Active rows have already
    /// entered HostSetup's opponents; only the inactive words need retention.
    pub(crate) fn accept_network_inactive_ai_options<const N: usize>(
        &mut self,
        incoming: [NetworkAiOptions; N],
    ) -> anyhow::Result<()> {
        anyhow::ensure!(!self.host, "host cannot receive its own AI arrays");
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    inactive_ai_options,
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        anyhow::ensure!(N <= NETWORK_PLAYER_SLOT_COUNT, "too many AI option rows");
        for (row, value) in incoming.into_iter().enumerate() {
            if !launch
                .opponents
                .iter()
                .any(|ai| usize::from(ai.raw_slot_index) == row)
            {
                inactive_ai_options[row] = value;
            }
        }
        Ok(())
    }

    /// LAN/WOL roster receiver5E8EA0 replaces the eight display pointers,
    /// retaining source-list order, node acknowledgements and endpoints.
    /// Unknown names enter as Waiting; a later type3 admission binds its row.
    pub(crate) fn accept_network_roster<const N: usize>(
        &mut self,
        rows: [NetworkRosterEntry; N],
    ) -> anyhow::Result<()> {
        anyhow::ensure!(!self.host, "a host cannot receive its own roster");
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    vacancies,
                    display_ai,
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        anyhow::ensure!(N <= NETWORK_PLAYER_SLOT_COUNT, "too many roster rows");
        let mut candidate = launch.clone();
        for human in &mut candidate.humans {
            human.raw_slot_index = None;
        }
        let mut next_vacancies = [NetworkVacancy::Open; NETWORK_PLAYER_SLOT_COUNT];
        let mut next_ai = [None; NETWORK_PLAYER_SLOT_COUNT];
        for (row, binding) in rows.into_iter().enumerate() {
            match binding {
                NetworkRosterEntry::Human(source) => {
                    let human = candidate
                        .humans
                        .get_mut(source)
                        .ok_or_else(|| anyhow::anyhow!("roster names an unadmitted source"))?;
                    anyhow::ensure!(
                        human.raw_slot_index.is_none(),
                        "duplicate roster participant"
                    );
                    human.raw_slot_index = Some(row as u8);
                }
                NetworkRosterEntry::Vacancy(vacancy) => next_vacancies[row] = vacancy,
                NetworkRosterEntry::Ai(difficulty) => next_ai[row] = Some(difficulty),
            }
        }
        validate_network_option_rows(&candidate)?;
        candidate.rebuild_house_roster();
        *launch = candidate;
        *vacancies = next_vacancies;
        *display_ai = next_ai;
        Ok(())
    }

    /// Sole5EC9F0 row-kind transition: any AI selection resets its choices,
    /// leavingAI/human/observer resets vacant choices, and every transition
    /// reapplies the mode's team default via5EE510. Returned peers need the
    /// caller's already-authorized service kick; no transport lives here.
    pub(crate) fn set_network_row_kind(
        &mut self,
        row: u8,
        kind: NetworkRowKind,
    ) -> anyhow::Result<Option<crate::skirmish_launch::SkirmishHumanSlot>> {
        anyhow::ensure!(self.host, "only the host may change participant row kinds");
        //5EC9F0 returns before any reset when the selected item pointer is unchanged.
        let current_kind = self
            .network_display_ai(row)
            .map(NetworkRowKind::Ai)
            .or_else(|| {
                self.network_vacancy(row).and_then(|vacancy| match vacancy {
                    NetworkVacancy::Open => Some(NetworkRowKind::Open),
                    NetworkVacancy::Closed => Some(NetworkRowKind::Closed),
                    NetworkVacancy::OpenObserver => Some(NetworkRowKind::OpenObserver),
                    NetworkVacancy::Waiting => None,
                })
            });
        if current_kind == Some(kind) {
            return Ok(None);
        }
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    vacancies,
                    inactive_choices,
                    inactive_ai_options,
                    display_ai,
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        anyhow::ensure!(
            usize::from(row) < NETWORK_PLAYER_SLOT_COUNT
                && launch.local_player().raw_slot_index != Some(row),
            "row change cannot replace the local participant"
        );
        let removed = if let Some(index) = launch
            .humans
            .iter()
            .position(|human| human.raw_slot_index == Some(row))
        {
            let human = launch.humans[index].clone();
            launch.humans[index].raw_slot_index = None;
            Some(human)
        } else {
            None
        };
        let had_ai = launch.opponents.iter().any(|ai| ai.raw_slot_index == row);
        launch.opponents.retain(|ai| ai.raw_slot_index != row);
        let index = usize::from(row);
        let was_observer = removed.as_ref().is_some_and(|human| human.is_observer())
            || vacancies[index] == NetworkVacancy::OpenObserver;
        display_ai[index] = match kind {
            NetworkRowKind::Ai(difficulty) => Some(difficulty),
            _ => None,
        };
        if let NetworkRowKind::Ai(difficulty) = kind {
            let defaults = native_network_row_defaults(&launch.mode, true);
            launch.opponents.push(SkirmishAiSlot {
                raw_slot_index: row,
                country: defaults.country,
                country_random: defaults.country_random,
                color_index: defaults.color_index,
                color_random: defaults.color_random,
                start_position: defaults.start_position,
                team: defaults.team,
                difficulty,
            });
            launch.opponents.sort_by_key(|ai| ai.raw_slot_index);
        } else {
            //5ECFD7 clears three fields when leaving an observer;5ED0CF
            //and5ED17D clear five when entering one or removing an AI.
            //Open <-> Closed alone retains these separate array words.
            if had_ai || (!was_observer && kind == NetworkRowKind::OpenObserver) {
                inactive_ai_options[index] = NetworkAiOptions::CLEARED;
            } else if was_observer && kind != NetworkRowKind::OpenObserver {
                inactive_ai_options[index].difficulty = -1;
                inactive_ai_options[index].country = -1;
                inactive_ai_options[index].color = -1;
            }
            if had_ai || removed.is_some() || vacancies[index] == NetworkVacancy::OpenObserver {
                inactive_choices[index] = native_network_row_defaults(&launch.mode, false);
            }
            vacancies[index] = match kind {
                NetworkRowKind::Open => NetworkVacancy::Open,
                NetworkRowKind::Closed => NetworkVacancy::Closed,
                NetworkRowKind::OpenObserver => NetworkVacancy::OpenObserver,
                NetworkRowKind::Ai(_) => unreachable!(),
            };
            if kind == NetworkRowKind::OpenObserver {
                inactive_choices[index] = native_network_row_defaults(&launch.mode, false);
                inactive_choices[index].country_random = false;
                inactive_choices[index].color_index = 8;
            }
            inactive_choices[index].team = native_network_row_defaults(&launch.mode, false).team;
            //5ED1B0 ->5EE510 runs after the clear above, including for Open
            //and Closed rows: the raw team array takes mode+2C last.
            inactive_ai_options[index].team = match inactive_choices[index].team {
                crate::skirmish_launch::LaunchTeam::None => -2,
                crate::skirmish_launch::LaunchTeam::Team(team) => i32::from(team),
            };
        }
        launch.ai_player_count = launch.opponents.len() as i32;
        launch.rebuild_house_roster();
        Ok(removed)
    }

    /// Cooperative's country refresh (`0x005C2500`), on the host and a
    /// guest alike: every row below the setup's capacity (`[0xA8BB0C]`)
    /// takes `country` (None is Random) in its combo and its node
    /// (`NodeNameTag__SetCountry`), whoever owns the row. Without `ai_rows`
    /// the AI rows keep theirs.
    pub(crate) fn reset_network_countries(
        &mut self,
        country: Option<crate::skirmish_launch::LaunchCountry>,
        ai_rows: bool,
    ) -> anyhow::Result<()> {
        let rows = self
            .network_setup_capacity()
            .unwrap_or(0)
            .clamp(0, NETWORK_PLAYER_SLOT_COUNT as i32) as u8;
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    inactive_choices,
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        let set = |current: crate::skirmish_launch::LaunchCountry| match country {
            Some(country) => (country, false),
            None => (current, true),
        };
        for human in &mut launch.humans {
            if human.raw_slot_index.is_some_and(|row| row < rows) {
                (human.slot.country, human.slot.country_random) = set(human.slot.country);
            }
        }
        for ai in launch.opponents.iter_mut().filter(|_| ai_rows) {
            if ai.raw_slot_index < rows {
                (ai.country, ai.country_random) = set(ai.country);
            }
        }
        for choices in inactive_choices.iter_mut().take(usize::from(rows)) {
            (choices.country, choices.country_random) = set(choices.country);
        }
        launch.rebuild_house_roster();
        Ok(())
    }

    /// VERA's room bans on the host: each player, AI and inactive row whose
    /// chosen country is `banned` goes back to Random.
    pub(crate) fn random_for_network_countries(
        &mut self,
        banned: impl Fn(crate::skirmish_launch::LaunchCountry) -> bool,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.host, "only the host enforces the room's bans");
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    inactive_choices,
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        for human in launch
            .humans
            .iter_mut()
            .filter(|human| !human.is_observer())
        {
            human.slot.country_random |= banned(human.slot.country);
        }
        for ai in &mut launch.opponents {
            ai.country_random |= banned(ai.country);
        }
        for choices in inactive_choices.iter_mut() {
            choices.country_random |= banned(choices.country);
        }
        launch.rebuild_house_roster();
        Ok(())
    }

    pub(crate) fn set_network_row_choices(
        &mut self,
        row: u8,
        choices: SkirmishLocalSlot,
    ) -> anyhow::Result<()> {
        let host = self.host;
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    inactive_choices,
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        if let Some(index) = launch
            .humans
            .iter()
            .position(|human| human.raw_slot_index == Some(row))
        {
            anyhow::ensure!(
                index == launch.local_player_index,
                "a client may edit only its own human row"
            );
            launch.humans[index].slot = choices;
        } else {
            anyhow::ensure!(
                host && usize::from(row) < NETWORK_PLAYER_SLOT_COUNT,
                "only the host may edit AI/vacant choices"
            );
            if let Some(ai) = launch
                .opponents
                .iter_mut()
                .find(|ai| ai.raw_slot_index == row)
            {
                ai.country = choices.country;
                ai.country_random = choices.country_random;
                ai.color_index = choices.color_index;
                ai.color_random = choices.color_random;
                ai.start_position = choices.start_position;
                ai.team = choices.team;
            } else {
                inactive_choices[usize::from(row)] = choices;
            }
        }
        launch.rebuild_house_roster();
        Ok(())
    }

    pub(crate) fn set_network_vacancy(
        &mut self,
        row: u8,
        vacancy: NetworkVacancy,
    ) -> anyhow::Result<()> {
        let kind = match vacancy {
            NetworkVacancy::Open => NetworkRowKind::Open,
            NetworkVacancy::Closed => NetworkRowKind::Closed,
            NetworkVacancy::OpenObserver => NetworkRowKind::OpenObserver,
            NetworkVacancy::Waiting => anyhow::bail!("Waiting is admitted by a received roster"),
        };
        anyhow::ensure!(
            self.network_setup().is_none_or(|launch| !launch
                .humans
                .iter()
                .any(|human| human.raw_slot_index == Some(row))),
            "vacancy cannot replace a human participant"
        );
        self.set_network_row_kind(row, kind).map(|_| ())
    }

    pub(crate) fn set_network_options(
        &mut self,
        options: SkirmishLaunchOptions,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.host, "only the host may change game options");
        self.editable_network_setup()?.options = options;
        // Native host publishes changes; a guest later invalidates its own
        // readiness and sends A0. Do not optimistically clear every peer here.
        Ok(())
    }

    pub(crate) fn network_map_metadata(&self) -> Option<&NetworkMapMetadata> {
        self.network.as_ref()?.map_metadata.as_ref()
    }

    ///5E4F49 mapsame flag, independent of general option/readiness changes.
    /// Call before host setup admission, avoiding a second filename cache.
    pub(crate) fn network_map_requires_verification(
        &self,
        incoming: &SkirmishLaunchSession,
        metadata: &NetworkMapMetadata,
    ) -> anyhow::Result<bool> {
        let network = self
            .network
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        let old = self
            .network_setup()
            .ok_or_else(|| anyhow::anyhow!("no network setup"))?;
        let Some(previous) = &network.map_metadata else {
            return Ok(true);
        };
        Ok(previous.display_name.is_empty()
            || previous.player_capacity != metadata.player_capacity
            || old.mode.id != incoming.mode.id
            || !old
                .selected_map_file
                .as_deref()
                .unwrap_or("")
                .eq_ignore_ascii_case(incoming.selected_map_file.as_deref().unwrap_or(""))
            || previous.official != metadata.official
            || previous.byte_length != metadata.byte_length
            || previous.digest != metadata.digest
            || previous.source_sha256 != metadata.source_sha256
            || previous.effective_digest != metadata.effective_digest)
    }

    /// Local qualified map title takes precedence; absent local bytes use the
    /// nonempty advertised title. An unchanged packet leaves the cached title.
    pub(crate) fn accept_network_map_metadata(
        &mut self,
        mut metadata: NetworkMapMetadata,
        local_title: Option<&str>,
        refresh_description: bool,
    ) -> anyhow::Result<()> {
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        let NetworkPhase::Setup {
            launch, capacity, ..
        } = &network.phase
        else {
            anyhow::bail!("network setup is not editable");
        };
        let expected_capacity = row_capacity(
            network.return_context.session_mode(),
            network.room,
            &launch.mode,
            metadata.player_capacity,
        );
        anyhow::ensure!(
            *capacity == expected_capacity,
            "map metadata and derived setup capacity disagree"
        );
        let previous_title = network
            .map_metadata
            .as_ref()
            .map_or("", |old| old.display_name.as_str());
        if refresh_description {
            if let Some(title) = local_title {
                metadata.display_name = title.to_owned();
            } else if metadata.display_name.is_empty() {
                metadata.display_name = previous_title.to_owned();
            }
        } else {
            metadata.display_name = previous_title.to_owned();
        }
        if network.map_metadata.as_ref().is_none_or(|previous| {
            previous.source_sha256 != metadata.source_sha256
                || previous.byte_length != metadata.byte_length
                || previous.effective_digest != metadata.effective_digest
        }) {
            network.local_map_source = None;
            network.local_map_effective_digest = None;
            network.local_random_map_preview = None;
        }
        network.map_metadata = Some(metadata);
        Ok(())
    }

    pub(crate) fn network_local_map_source(&self) -> Option<&crate::map::source::LoadedMapSource> {
        self.network.as_ref()?.local_map_source.as_ref()
    }

    /// Bind content only to its currently accepted map identity. A late download
    /// cannot replace a newly selected map or acknowledge it as present.
    /// An external launch's map is the launcher's file beside spawn.ini:
    /// each participant loads its own copy, while the shared logical name
    /// (the setup's `selected_map_file`) keeps every world identical.
    pub(crate) fn accept_external_network_map_source(
        &mut self,
        source: crate::map::source::LoadedMapSource,
        effective_digest: String,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.network_setup().is_some(),
            "map binding requires an admitted setup"
        );
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        network.local_map_source = Some(source);
        network.local_map_effective_digest = Some(effective_digest);
        Ok(())
    }

    pub(crate) fn accept_verified_network_map_source(
        &mut self,
        source: crate::map::source::LoadedMapSource,
        effective_digest: String,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.network_setup().is_some(),
            "map binding requires editable setup"
        );
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        let expected = network
            .map_metadata
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no map metadata"))?;
        let (length, digest) = source
            .physical_identity()
            .ok_or_else(|| anyhow::anyhow!("map source has no physical identity"))?;
        anyhow::ensure!(
            length == usize::try_from(expected.byte_length)?
                && crate::map::source::valid_source_sha256(digest)
                && digest == expected.source_sha256
                && effective_digest == expected.effective_digest,
            "local source differs from the selected map"
        );
        network.local_map_source = Some(source);
        network.local_map_effective_digest = Some(effective_digest);
        Ok(())
    }

    pub(crate) fn network_local_map_effective_digest(&self) -> Option<&str> {
        self.network.as_ref()?.local_map_effective_digest.as_deref()
    }

    pub(crate) fn network_random_map_preview(
        &self,
    ) -> Option<&crate::map::source::LoadedMapSource> {
        self.network.as_ref()?.local_random_map_preview.as_ref()
    }
    pub(crate) fn clear_network_random_map_preview(&mut self) {
        if let Some(network) = self.network.as_mut() {
            network.local_random_map_preview = None;
        }
    }

    pub(crate) fn accept_network_random_map_preview(
        &mut self,
        seed_digest: &str,
        source: crate::map::source::LoadedMapSource,
    ) -> anyhow::Result<()> {
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        anyhow::ensure!(
            network
                .map_metadata
                .as_ref()
                .is_some_and(|meta| meta.source_sha256 == seed_digest),
            "preview belongs to another seed"
        );
        network.local_random_map_preview = Some(source);
        Ok(())
    }

    /// DD8A0 copies the prepared filename to Scenario+125C atDD97F:
    /// local6951F0 resolves catalog spelling; successful693B70 supplies the
    /// host FILE_INFO name directly, without catalog insertion.
    pub(crate) fn accept_prepared_network_map_filename(
        &mut self,
        file: String,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.host,
            "host cannot accept its own guest map-preparation result"
        );
        anyhow::ensure!(
            !file.is_empty() && file.trim() == file && !file.eq_ignore_ascii_case("auto"),
            "prepared network map requires an exact filename"
        );
        self.editable_network_setup()?.selected_map_file = Some(file);
        Ok(())
    }

    /// The admitted host's options packet (ParsePacket5E3D10) replaces setup
    /// values, never roster identity or its independent ready/map acknowledgements.
    /// The service adapter applies the packet's local-ready invalidation separately.
    /// A WOL Quick Match host applies the matchbot's options the same way
    /// (`matchbot`, `0x007A6DE2..0x007A6DF3`).
    pub(crate) fn accept_network_host_setup<const N: usize>(
        &mut self,
        mut incoming: SkirmishLaunchSession,
        incoming_vacancies: [NetworkVacancy; N],
        capacity: i32,
        observer_row: Option<u8>,
        matchbot: bool,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.host || matchbot,
            "a host cannot receive guest-authored host setup"
        );
        anyhow::ensure!(capacity > 0, "host map capacity must be positive");
        anyhow::ensure!(N <= NETWORK_PLAYER_SLOT_COUNT, "too many vacancy rows");
        anyhow::ensure!(
            observer_row.is_none_or(|row| usize::from(row) < NETWORK_PLAYER_SLOT_COUNT),
            "observer reservation is outside setup rows"
        );
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    vacancies,
                    capacity: retained_capacity,
                    observer_row: retained_observer_row,
                    ..
                },
            return_context,
            room,
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        let admitted_capacity = row_capacity(
            return_context.session_mode(),
            *room,
            &incoming.mode,
            capacity,
        );
        anyhow::ensure!(
            launch.local_player_index == incoming.local_player_index
                && launch.humans.len() == incoming.humans.len()
                && launch
                    .humans
                    .iter()
                    .zip(&incoming.humans)
                    .all(|(old, new)| old.name == new.name),
            "host options cannot replace admitted participant identity or source order"
        );
        validate_network_option_rows(&incoming)?;
        incoming.opponents.sort_by_key(|ai| ai.raw_slot_index);
        incoming.rebuild_house_roster();
        // A Teams count this build does not offer leaves Teams Off.
        incoming.options.teams = incoming
            .options
            .teams
            .filter(|teams| crate::skirmish_launch::ROOM_TEAMS.contains(teams));
        // The join queue names spectators, each once.
        let queue = std::mem::take(&mut incoming.options.queue);
        for name in queue {
            let waits = incoming.humans.iter().any(|human| {
                human.name == name && human.is_observer() && human.raw_slot_index.is_some()
            });
            if waits && !incoming.options.queue.contains(&name) {
                incoming.options.queue.push(name);
            }
        }
        *launch = incoming;
        *vacancies = [NetworkVacancy::Open; NETWORK_PLAYER_SLOT_COUNT];
        vacancies[..N].copy_from_slice(&incoming_vacancies);
        *retained_capacity = admitted_capacity;
        *retained_observer_row = observer_row;
        Ok(())
    }

    /// Mode callbacks write node+6B independently of the team selection at+63.
    /// FFA uses source indices; Siege uses defender/attacker groups1/2.
    pub(crate) fn accept_network_human_role(
        &mut self,
        index: usize,
        role: crate::skirmish_launch::LaunchHumanRole,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            role != crate::skirmish_launch::LaunchHumanRole::Player { group: -1 },
            "native role -1 must be represented as Observer"
        );
        let launch = self.editable_network_setup()?;
        launch
            .humans
            .get_mut(index)
            .ok_or_else(|| anyhow::anyhow!("unknown participant"))?
            .role = role;
        launch.rebuild_house_roster();
        Ok(())
    }

    pub(crate) fn network_setup_capacity(&self) -> Option<i32> {
        match &self.network.as_ref()?.phase {
            NetworkPhase::Setup { capacity, .. } => Some(*capacity),
            _ => None,
        }
    }

    /// 5EB060/5E6440 service row limit (LAN 8, WOL 6 or a tournament
    /// room's 2) of the active visit.
    pub(crate) fn network_row_limit(&self) -> Option<i32> {
        let network = self.network.as_ref()?;
        Some(
            if self
                .network_setup_capacity()
                .is_some_and(|capacity| capacity > 8)
            {
                NETWORK_PLAYER_SLOT_COUNT as i32
            } else {
                row_limit(network.return_context.session_mode(), network.room)
            },
        )
    }

    /// The WOL game channel's kind; set when its setup opens, before the
    /// first map selection.
    pub(crate) fn set_network_room(&mut self, room: NetworkRoom) {
        if let Some(network) = self.network.as_mut() {
            network.room = room;
        }
    }

    pub(crate) fn network_room(&self) -> NetworkRoom {
        self.network
            .as_ref()
            .map(|network| network.room)
            .unwrap_or_default()
    }

    pub(crate) fn network_observer_row(&self) -> Option<u8> {
        match &self.network.as_ref()?.phase {
            NetworkPhase::Setup { observer_row, .. } => *observer_row,
            _ => None,
        }
    }

    /// Whether this is one of VERA's own rooms, where observers may take
    /// any number of rows (the Setup phase's `several_observers`).
    pub(crate) fn network_several_observers(&self) -> bool {
        self.network.as_ref().is_some_and(|network| {
            matches!(
                network.phase,
                NetworkPhase::Setup {
                    several_observers: true,
                    ..
                }
            )
        })
    }

    /// The row is open for an observer or holds one.
    pub(crate) fn network_row_for_observer(&self, row: u8) -> bool {
        self.network_vacancy(row) == Some(NetworkVacancy::OpenObserver)
            || self.network_setup().is_some_and(|launch| {
                launch
                    .humans
                    .iter()
                    .any(|human| human.raw_slot_index == Some(row) && human.is_observer())
            })
    }

    /// Whether the host of a VERA room keeps a spectator row open.
    pub(crate) fn set_network_spectators(&mut self, open: bool) -> anyhow::Result<()> {
        anyhow::ensure!(self.host, "only the host opens spectator rows");
        if let Some(NetworkSession {
            phase: NetworkPhase::Setup {
                open_spectators, ..
            },
            ..
        }) = self.network.as_mut()
        {
            *open_spectators = open;
        }
        self.keep_spectator_row()
    }

    /// The most people the host of a VERA room lets in (its Create Game
    /// choice), players and spectators together.
    pub(crate) fn set_network_people_limit(&mut self, people: u8) -> anyhow::Result<()> {
        anyhow::ensure!(self.host, "only the host limits the room");
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    people_limit,
                    several_observers: true,
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("only VERA's rooms limit their people");
        };
        *people_limit = people.clamp(2, NETWORK_PLAYER_SLOT_COUNT as u8);
        Ok(())
    }

    /// VERA's join queue (`launch.options.queue`): the spectators waiting
    /// for a seat, by name, first first.
    pub(crate) fn network_queue(&self) -> &[String] {
        self.network_setup()
            .map(|launch| launch.options.queue.as_slice())
            .unwrap_or_default()
    }

    /// Where participant `index` waits in the join queue, 1 for the first.
    pub(crate) fn network_queue_place(&self, index: usize) -> Option<usize> {
        let launch = self.network_setup()?;
        let name = &launch.humans.get(index)?.name;
        let place = launch
            .options
            .queue
            .iter()
            .position(|queued| queued == name)?;
        Some(place + 1)
    }

    /// Whether this VERA room queues the spectators who ask to play while
    /// no seat is free.
    pub(crate) fn network_queue_open(&self) -> bool {
        matches!(
            self.network.as_ref().map(|network| &network.phase),
            Some(NetworkPhase::Setup {
                join_queue: true,
                ..
            })
        )
    }

    /// The participant the host's join queue seats next: its first
    /// spectator, while a seat is free. The host's game seats it as the
    /// host's Seat does (`service::move_player`).
    pub(crate) fn network_queue_next(&self) -> Option<usize> {
        if !self.host || self.network_no_open_player_row() {
            return None;
        }
        let launch = self.network_setup()?;
        launch.options.queue.iter().find_map(|name| {
            launch.humans.iter().position(|human| {
                human.name == *name && human.is_observer() && human.raw_slot_index.is_some()
            })
        })
    }

    /// The online lobby refused the join queue: the room goes on without
    /// one, and its spectators keep watching. Returns whether anyone
    /// waited.
    pub(crate) fn stop_network_queue(&mut self) -> anyhow::Result<bool> {
        anyhow::ensure!(self.host, "only the host keeps the join queue");
        if let Some(NetworkSession {
            phase: NetworkPhase::Setup { join_queue, .. },
            ..
        }) = self.network.as_mut()
        {
            *join_queue = false;
        }
        let queue = &mut self.editable_network_setup()?.options.queue;
        let waited = !queue.is_empty();
        queue.clear();
        Ok(waited)
    }

    /// A spectator asking to play while no seat is free joins the end of
    /// the join queue, once.
    fn queue_network_spectator(&mut self, index: usize) -> anyhow::Result<()> {
        if !self.network_queue_open() {
            return Ok(());
        }
        let launch = self.editable_network_setup()?;
        let name = &launch.humans[index].name;
        if !launch.options.queue.contains(name) {
            launch.options.queue.push(name.clone());
        }
        Ok(())
    }

    /// Participant `index` leaves the join queue. Returns whether it waited
    /// there.
    fn leave_network_queue(&mut self, index: usize) -> anyhow::Result<bool> {
        let launch = self.editable_network_setup()?;
        let name = &launch.humans[index].name;
        let waited = launch.options.queue.len();
        launch.options.queue.retain(|queued| queued != name);
        Ok(launch.options.queue.len() != waited)
    }

    /// Whether this is a VERA room whose mode seats its players on teams:
    /// allying allowed, outside Cooperative and a clan room. Its rows list
    /// under team headers.
    pub(crate) fn network_team_room(&self) -> bool {
        let Some(NetworkPhase::Setup {
            launch,
            several_observers: true,
            ..
        }) = self.network.as_ref().map(|network| &network.phase)
        else {
            return false;
        };
        launch.mode.allies_allowed
            && !launch.mode.class.uses_cooperative_setup()
            && !self.network_room().clan
    }

    /// The Teams count of a VERA room that seats players on teams
    /// ([`Self::network_team_room`]), or `None` while Teams is Off.
    pub(crate) fn network_teams(&self) -> Option<u8> {
        self.network_team_room()
            .then(|| self.network_setup()?.options.teams)
            .flatten()
    }

    /// The host sets a VERA room's Teams: Off, or a count in
    /// [`crate::skirmish_launch::ROOM_TEAMS`]. Every seated player then has
    /// one of its teams. Off also turns Balance automatically off, and takes
    /// the room back to gamemd's A to D: a player or AI on a team past D gets
    /// the mode's default, as a guest's pick past D does.
    pub(crate) fn set_network_teams(&mut self, teams: Option<u8>) -> anyhow::Result<()> {
        anyhow::ensure!(self.host, "only the host sets the room's teams");
        let launch = self.editable_network_setup()?;
        let teams = teams.filter(|teams| crate::skirmish_launch::ROOM_TEAMS.contains(teams));
        launch.options.teams = teams;
        launch.options.auto_balance &= teams.is_some();
        if teams.is_none() {
            let default = mode_default_team(&launch.mode);
            let humans = launch.humans.iter_mut().map(|human| &mut human.slot.team);
            for team in humans.chain(launch.opponents.iter_mut().map(|ai| &mut ai.team)) {
                if matches!(*team, LaunchTeam::Team(letter) if letter >= crate::skirmish_launch::NATIVE_TEAMS)
                {
                    *team = default;
                }
            }
            launch.rebuild_house_roster();
        }
        self.keep_players_on_teams().map(|_| ())
    }

    /// Whether a VERA room with Teams set balances itself: the host's
    /// Balance automatically. While it does, nobody picks a team.
    pub(crate) fn network_auto_balance(&self) -> bool {
        self.network_teams().is_some()
            && self
                .network_setup()
                .is_some_and(|launch| launch.options.auto_balance)
    }

    /// The host turns Balance automatically on or off; it stays off while
    /// Teams is Off.
    pub(crate) fn set_network_auto_balance(&mut self, on: bool) -> anyhow::Result<()> {
        anyhow::ensure!(self.host, "only the host sets how the room balances");
        let teams = self.network_teams().is_some();
        self.editable_network_setup()?.options.auto_balance = on && teams;
        Ok(())
    }

    /// How many players each team of a VERA room with Teams set seats: the
    /// room's seats (the player rows its host has not removed) over its
    /// teams, rounded up.
    pub(crate) fn network_team_seats(&self) -> Option<usize> {
        let teams = usize::from(self.network_teams()?);
        let seats = (0..self.network_player_row_limit())
            .filter(|row| self.network_vacancy(*row) != Some(NetworkVacancy::Closed))
            .count();
        Some(seats.div_ceil(teams))
    }

    /// The players on `team`: its seated people and AI.
    pub(crate) fn network_team_players(&self, team: u8) -> usize {
        let Some(launch) = self.network_setup() else {
            return 0;
        };
        let team = LaunchTeam::Team(team);
        let people = launch.humans.iter().filter(|human| {
            human.raw_slot_index.is_some() && !human.is_observer() && human.slot.team == team
        });
        people.count() + launch.opponents.iter().filter(|ai| ai.team == team).count()
    }

    /// Whether `team` of a VERA room with Teams set has no seat left.
    pub(crate) fn network_team_full(&self, team: u8) -> bool {
        self.network_team_seats()
            .is_some_and(|seats| self.network_team_players(team) >= seats)
    }

    /// A guest's own team pick in a VERA room with Teams set: no team, a
    /// team past the room's count, a full team it is not on or, while the
    /// room balances itself, any other team leaves it where it is, and a
    /// spectator taking a seat then joins the team with the fewest players.
    /// The host's moves (`service::move_player`) skip this. Returns whether it
    /// changed the pick, so the host republishes the room and the guest's own
    /// row shows its team again.
    pub(crate) fn admit_network_team(&self, index: usize, choices: &mut SkirmishLocalSlot) -> bool {
        let Some(teams) = self.network_teams() else {
            return false;
        };
        let Some(human) = self
            .network_setup()
            .and_then(|launch| launch.humans.get(index))
        else {
            return false;
        };
        let seated = !human.is_observer();
        let picked = choices.team;
        let open = !self.network_auto_balance();
        let admitted = match picked {
            LaunchTeam::Team(team) => {
                team < teams
                    && (seated && human.slot.team == picked
                        || open && !self.network_team_full(team))
            }
            LaunchTeam::None => false,
        };
        if !admitted {
            choices.team = if seated {
                human.slot.team
            } else {
                LaunchTeam::None
            };
        }
        choices.team != picked
    }

    /// A VERA room with Teams set keeps every seated player, person or AI,
    /// on one of its teams: in row order, one on no team or past the count
    /// joins the team with the fewest players, the first on a tie. Only the
    /// host seats players. Returns whether anyone moved.
    fn keep_players_on_teams(&mut self) -> anyhow::Result<bool> {
        if !self.host {
            return Ok(false);
        }
        let Some(teams) = self.network_teams() else {
            return Ok(false);
        };
        let launch = self.editable_network_setup()?;
        let on_team = |team: LaunchTeam| match team {
            LaunchTeam::Team(team) if team < teams => Some(usize::from(team)),
            _ => None,
        };
        let mut players = vec![0usize; usize::from(teams)];
        let seated = launch
            .humans
            .iter()
            .filter(|human| human.raw_slot_index.is_some() && !human.is_observer())
            .map(|human| human.slot.team);
        for team in seated
            .chain(launch.opponents.iter().map(|ai| ai.team))
            .filter_map(on_team)
        {
            players[team] += 1;
        }
        let mut moved = false;
        for row in 0..NETWORK_PLAYER_SLOT_COUNT as u8 {
            let team = if let Some(human) = launch
                .humans
                .iter_mut()
                .find(|human| human.raw_slot_index == Some(row) && !human.is_observer())
            {
                &mut human.slot.team
            } else if let Some(ai) = launch
                .opponents
                .iter_mut()
                .find(|ai| ai.raw_slot_index == row)
            {
                &mut ai.team
            } else {
                continue;
            };
            if on_team(*team).is_some() {
                continue;
            }
            let smallest = (0..players.len())
                .min_by_key(|team| players[*team])
                .expect("a room has two teams or more");
            *team = LaunchTeam::Team(smallest as u8);
            players[smallest] += 1;
            moved = true;
        }
        if moved {
            launch.rebuild_house_roster();
        }
        Ok(moved)
    }

    /// The host's Team Starts and Team Colors in a VERA room with Teams
    /// set: a start or a colour for each row named, a person's or an AI's.
    pub(crate) fn set_network_seat_picks(
        &mut self,
        picks: &[(u8, SeatPick)],
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.host, "only the host gives out starts and colours");
        let launch = self.editable_network_setup()?;
        for &(row, pick) in picks {
            let (start, color, random) = if let Some(human) = launch
                .humans
                .iter_mut()
                .find(|human| human.raw_slot_index == Some(row) && !human.is_observer())
            {
                let slot = &mut human.slot;
                (
                    &mut slot.start_position,
                    &mut slot.color_index,
                    &mut slot.color_random,
                )
            } else if let Some(ai) = launch
                .opponents
                .iter_mut()
                .find(|ai| ai.raw_slot_index == row)
            {
                (
                    &mut ai.start_position,
                    &mut ai.color_index,
                    &mut ai.color_random,
                )
            } else {
                anyhow::bail!("row {row} has no player");
            };
            match pick {
                SeatPick::Start(pick) => *start = pick,
                SeatPick::Color(pick) => (*color, *random) = (pick, false),
            }
        }
        launch.rebuild_house_roster();
        Ok(())
    }

    /// A VERA room holds its people limit and lets no one else in.
    pub(crate) fn network_room_full(&self) -> bool {
        match self.network.as_ref().map(|network| &network.phase) {
            Some(NetworkPhase::Setup {
                launch,
                people_limit,
                several_observers: true,
                ..
            }) => launch.humans.len() >= usize::from(*people_limit),
            _ => false,
        }
    }

    /// Whether later arrivals may watch in a VERA room: its host allowed
    /// spectators at Create Game and its mode keeps an observer row.
    fn network_spectators_welcome(&self) -> bool {
        matches!(
            self.network.as_ref().map(|network| &network.phase),
            Some(NetworkPhase::Setup {
                observer_row: Some(_),
                open_spectators: true,
                several_observers: true,
                ..
            })
        )
    }

    /// The most people a VERA room takes, the join limit its host gives the
    /// online lobby: its Max players, and when no one else may watch, no
    /// more than the people in it and the seats a newcomer can take, an
    /// AI's among them ([`Self::free_network_seat_for_newcomer`]).
    pub(crate) fn network_people_capacity(&self) -> Option<i32> {
        let Some(NetworkPhase::Setup {
            launch,
            people_limit,
            several_observers: true,
            ..
        }) = self.network.as_ref().map(|network| &network.phase)
        else {
            return None;
        };
        let limit = i32::from(*people_limit);
        if self.network_spectators_welcome() {
            return Some(limit);
        }
        let seats = (0..self.network_player_row_limit())
            .filter(|row| {
                self.network_vacancy(*row) == Some(NetworkVacancy::Open)
                    || self.network_display_ai(*row).is_some()
            })
            .count();
        Some(limit.min((launch.humans.len() + seats) as i32))
    }

    /// The seat a newcomer to a VERA room under its host's Max players gets
    /// when every seat is taken: the AI's on the last row. AI never fill a
    /// VERA room.
    pub(crate) fn network_newcomer_ai_seat(&self) -> Option<u8> {
        if !self.host
            || !self.network_several_observers()
            || self.network_room_full()
            || !self.network_no_open_player_row()
        {
            return None;
        }
        let launch = self.network_setup()?;
        launch.opponents.iter().map(|ai| ai.raw_slot_index).max()
    }

    /// Frees [`Self::network_newcomer_ai_seat`] for the newcomer as the
    /// host's Remove does ([`Self::choose_network_row_kind`]), and returns it.
    pub(crate) fn free_network_seat_for_newcomer(&mut self) -> anyhow::Result<Option<u8>> {
        let Some(row) = self.network_newcomer_ai_seat() else {
            return Ok(None);
        };
        self.choose_network_row_kind(row, NetworkRowKind::Open)?;
        Ok(Some(row))
    }

    /// The host's own Player choice for a row (`0x005EC9F0`), after which a
    /// VERA room places its open spectator row again.
    pub(crate) fn choose_network_row_kind(
        &mut self,
        row: u8,
        kind: NetworkRowKind,
    ) -> anyhow::Result<Option<crate::skirmish_launch::SkirmishHumanSlot>> {
        let removed = self.set_network_row_kind(row, kind)?;
        self.keep_spectator_row()?;
        self.keep_players_on_teams()?;
        Ok(removed)
    }

    /// A VERA room's rows after a change on its host: an Open Observer row
    /// among the players opens for a player, and past them only the first
    /// free row after the last spectator stays open, while the host keeps
    /// spectators and no AI plays.
    fn keep_spectator_row(&mut self) -> anyhow::Result<()> {
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    several_observers: true,
                    ..
                },
            ..
        }) = self.network.as_ref()
        else {
            return Ok(());
        };
        if !self.host {
            return Ok(());
        }
        let open = self.network_spectators_welcome() && launch.opponents.is_empty();
        let seated = |row: u8| {
            launch
                .humans
                .iter()
                .any(|human| human.raw_slot_index == Some(row))
        };
        let players = self.network_player_row_limit();
        let rows = NETWORK_PLAYER_SLOT_COUNT as u8;
        let after = (players..rows)
            .filter(|row| seated(*row))
            .last()
            .map_or(players, |row| row + 1);
        let spare = (open && after < rows).then_some(after);
        for row in 0..rows {
            let kind = match self.network_vacancy(row) {
                Some(NetworkVacancy::OpenObserver) if row < players => NetworkRowKind::Open,
                Some(NetworkVacancy::Waiting) | None => continue,
                Some(_) if row < players => continue,
                Some(_) if Some(row) == spare => NetworkRowKind::OpenObserver,
                Some(_) => NetworkRowKind::Closed,
            };
            self.set_network_row_kind(row, kind)?;
        }
        Ok(())
    }

    /// The rows the host's profile keeps (`0x005ED400`). A VERA room's
    /// spectator rows are kept as the Open rows a native room hides past its
    /// players, so a later map's players find them open.
    pub(crate) fn network_remembered_vacancies(
        &self,
    ) -> [Option<NetworkVacancy>; NETWORK_PLAYER_SLOT_COUNT] {
        let spectators = if self.network_several_observers() {
            self.network_player_row_limit()
        } else {
            NETWORK_PLAYER_SLOT_COUNT as u8
        };
        std::array::from_fn(|row| match self.network_vacancy(row as u8) {
            Some(NetworkVacancy::Closed | NetworkVacancy::OpenObserver)
                if row as u8 >= spectators =>
            {
                Some(NetworkVacancy::Open)
            }
            vacancy => vacancy,
        })
    }

    /// `MPGameOptions__SelectScenario` (`0x005E7BF0`): the map file and the
    /// row count `[0xA8BB0C]` (the map's players, capped by the mode and
    /// the WOL room) only. Unlike ChooseMap it re-cuts no rows and keeps
    /// every start and team.
    pub(crate) fn select_network_scenario(
        &mut self,
        file: String,
        capacity: i32,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !file.is_empty() && capacity > 0,
            "map requires a valid participant capacity"
        );
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        let session_mode = network.return_context.session_mode();
        let room = network.room;
        let NetworkPhase::Setup {
            launch,
            capacity: rows,
            ..
        } = &mut network.phase
        else {
            anyhow::bail!("network setup is not editable");
        };
        *rows = row_capacity(session_mode, room, &launch.mode, capacity);
        launch.selected_map_file = Some(file);
        Ok(())
    }

    /// Accepted BC ChooseMap5DE520: atomic map/mode selection followed by
    /// capacity5EC3A0, starts5EE6A0 and all-row team defaults5EE510(-1).
    /// Unbound human nodes are returned for service Kick intents. Their source
    /// membership survives until the separate type10 departure response.
    /// A VERA room keeps every observer row and closes the other rows past
    /// its players.
    pub(crate) fn select_network_map(
        &mut self,
        mode: crate::skirmish_launch::SkirmishLaunchMode,
        file: String,
        capacity: i32,
    ) -> anyhow::Result<Vec<crate::skirmish_launch::SkirmishHumanSlot>> {
        anyhow::ensure!(self.host, "only host may choose the map");
        anyhow::ensure!(
            !file.is_empty() && capacity > 0,
            "map requires a valid participant capacity"
        );
        let mut candidate = self.clone();
        let several = self.network_several_observers();
        let (old_capacity, observer_row, host_extra, new_capacity) = {
            let Some(NetworkSession {
                phase:
                    NetworkPhase::Setup {
                        launch,
                        capacity: previous,
                        observer_row,
                        ..
                    },
                return_context,
                room,
                ..
            }) = candidate.network.as_mut()
            else {
                anyhow::bail!("network setup is not editable");
            };
            let session_mode = return_context.session_mode();
            let room = *room;
            let changed = launch.mode != mode;
            if changed && mode.class.uses_cooperative_setup() {
                *observer_row = None;
            } //5C1A10
            let new_capacity = row_capacity(session_mode, room, &mode, capacity);
            let host_extra = i32::from(
                *observer_row == launch.local_player().raw_slot_index
                    && host_extra_row(session_mode, room, &mode, new_capacity),
            );
            let old = *previous;
            *previous = new_capacity;
            launch.mode = mode;
            launch.selected_map_file = Some(file);
            (old, *observer_row, host_extra, new_capacity)
        };
        let mut removed = Vec::new();
        let local_row = candidate
            .network_setup()
            .unwrap()
            .local_player()
            .raw_slot_index;
        // A VERA room's players end at its player row limit, which the host's
        // own observer row extends at every capacity.
        let (old_rows, new_rows) = if several {
            (
                i32::from(self.network_player_row_limit()),
                i32::from(candidate.network_player_row_limit()),
            )
        } else {
            (
                old_capacity.saturating_add(host_extra),
                new_capacity.saturating_add(host_extra),
            )
        };
        if new_rows < old_rows {
            for row in new_rows..NETWORK_PLAYER_SLOT_COUNT as i32 {
                let row = row as u8;
                // VERA's rooms keep every observer row.
                let retained_observer = (several || Some(row) == observer_row)
                    && candidate.network_row_for_observer(row);
                if Some(row) != local_row && !retained_observer {
                    // Native's hidden eight-row suffix stays Open. Extended
                    // rows have no native slot and must remain closed until a
                    // later map explicitly makes them usable again, as must
                    // a VERA room's spectator rows.
                    let kind = if several || row >= 8 || new_capacity > 8 {
                        NetworkRowKind::Closed
                    } else {
                        NetworkRowKind::Open
                    };
                    removed.extend(candidate.set_network_row_kind(row, kind)?);
                }
            }
        } else if new_rows > old_rows {
            for row in old_rows..new_rows.min(NETWORK_PLAYER_SLOT_COUNT as i32) {
                let row = row as u8;
                // A VERA room's reserved row is no different from the others.
                if Some(row) == local_row || (!several && Some(row) == observer_row) {
                    continue;
                }
                let setup = candidate.network_setup().unwrap();
                let seated = |row: u8| {
                    setup
                        .humans
                        .iter()
                        .position(|human| human.raw_slot_index == Some(row))
                };
                if let Some(index) = seated(row) {
                    // Its spectators move on past the new players.
                    let free = (new_rows as u8..NETWORK_PLAYER_SLOT_COUNT as u8)
                        .find(|row| seated(*row).is_none())
                        .filter(|_| several && setup.humans[index].is_observer());
                    let Some(free) = free else {
                        continue;
                    };
                    candidate.editable_network_setup()?.humans[index].raw_slot_index = Some(free);
                }
                let setup = candidate.network_setup().unwrap();
                let kind = setup
                    .opponents
                    .iter()
                    .find(|ai| ai.raw_slot_index == row)
                    .map_or(NetworkRowKind::Open, |ai| NetworkRowKind::Ai(ai.difficulty));
                candidate.set_network_row_kind(row, kind)?;
            }
        }
        // A VERA room seats the new map's players: slots its host removed
        // open again.
        if several {
            for row in 0..new_rows.clamp(0, NETWORK_PLAYER_SLOT_COUNT as i32) as u8 {
                if candidate.network_vacancy(row) == Some(NetworkVacancy::Closed) {
                    candidate.set_network_row_kind(row, NetworkRowKind::Open)?;
                }
            }
        }
        //5EC030 clears former nonlocal observer slots when activation cleared
        //the reserved row. A subsequent ordinary mode reserves row7 below.
        if observer_row.is_none() {
            let observer_rows: Vec<_> = candidate
                .network_setup()
                .unwrap()
                .humans
                .iter()
                .filter(|human| human.raw_slot_index != local_row && human.is_observer())
                .filter_map(|human| human.raw_slot_index)
                .chain((0..NETWORK_PLAYER_SLOT_COUNT as u8).filter(|row| {
                    candidate.network_vacancy(*row) == Some(NetworkVacancy::OpenObserver)
                }))
                .collect();
            for row in observer_rows {
                // A VERA room's rows past its players stay Closed.
                let kind = if several && i32::from(row) >= new_rows {
                    NetworkRowKind::Closed
                } else {
                    NetworkRowKind::Open
                };
                removed.extend(candidate.set_network_row_kind(row, kind)?);
            }
        }
        let ordinary = candidate
            .network_setup()
            .unwrap()
            .mode
            .class
            .allows_observers();
        let default_observer = if new_capacity > 8 {
            NETWORK_PLAYER_SLOT_COUNT as u8 - 1
        } else {
            7
        };
        let reserved = observer_row
            .filter(|row| {
                Some(*row) == local_row || new_capacity <= 8 || *row >= new_capacity as u8
            })
            .or(ordinary.then_some(default_observer));
        if let Some(row) =
            reserved.filter(|row| Some(*row) != local_row && i32::from(*row) > new_capacity)
        {
            if !candidate.network_row_for_observer(row) {
                removed.extend(candidate.set_network_row_kind(row, NetworkRowKind::Closed)?);
            }
        }
        // A VERA room with Teams set keeps its players' teams.
        let keep_teams = candidate.network_teams().is_some();
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    inactive_choices,
                    inactive_ai_options,
                    observer_row,
                    ..
                },
            ..
        }) = candidate.network.as_mut()
        else {
            unreachable!()
        };
        *observer_row = reserved;
        let human_team = native_network_row_defaults(&launch.mode, false).team;
        let ai_team = native_network_row_defaults(&launch.mode, true).team;
        for human in &mut launch.humans {
            human.slot.start_position = crate::skirmish_launch::LaunchStartPosition::Auto;
            if !keep_teams {
                human.slot.team = human_team;
            }
        }
        for ai in &mut launch.opponents {
            ai.start_position = crate::skirmish_launch::LaunchStartPosition::Auto;
            if !keep_teams {
                ai.team = ai_team;
            }
        }
        for choices in inactive_choices {
            choices.start_position = crate::skirmish_launch::LaunchStartPosition::Auto;
            choices.team = human_team;
        }
        //5EE510 writes A8B2FC even for rows whose display pointer is human
        //or vacant.5E9E70's start reset affects controls, not A8B2DC.
        for raw in inactive_ai_options {
            raw.team = match human_team {
                crate::skirmish_launch::LaunchTeam::None => -2,
                crate::skirmish_launch::LaunchTeam::Team(team) => i32::from(team),
            };
        }
        launch.rebuild_house_roster();
        candidate.keep_spectator_row()?;
        candidate.keep_players_on_teams()?;
        *self = candidate;
        Ok(removed)
    }

    /// Mode+68 Cooperative5C30C0 commits its count before stage validation;
    /// a missing stage leaves the physical AI rows unchanged. This is distinct
    /// from a user's ordinary AI-row edit, which derives count from rows.
    pub(crate) fn apply_network_ai_preparation(
        &mut self,
        declared_count: Option<i32>,
        opponents: Option<Vec<SkirmishAiSlot>>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.host, "only host mode preparation may write AI state");
        let launch = self.editable_network_setup()?;
        if let Some(count) = declared_count {
            launch.ai_player_count = count;
        }
        if let Some(opponents) = opponents {
            let mut candidate = launch.clone();
            candidate.opponents = opponents;
            validate_network_rows(&candidate)?;
            candidate.opponents.sort_by_key(|ai| ai.raw_slot_index);
            candidate.rebuild_house_roster();
            *launch = candidate;
        }
        Ok(())
    }

    /// Host-local assignment completion or a received resolved start. Source
    /// order/identity, options and map cannot change at this seam.
    pub(crate) fn accept_network_assignments(
        &mut self,
        resolved: SkirmishLaunchSession,
    ) -> anyhow::Result<()> {
        let current = self.editable_network_setup()?;
        anyhow::ensure!(
            current.humans.len() == resolved.humans.len()
                && current
                    .humans
                    .iter()
                    .zip(&resolved.humans)
                    .all(|(left, right)| left.name == right.name
                        && left.role == right.role
                        && left.raw_slot_index == right.raw_slot_index
                        && left.slot.start_position == right.slot.start_position
                        && left.slot.team == right.slot.team)
                && current.local_player_index == resolved.local_player_index
                && current.mode == resolved.mode
                && current.selected_map_file == resolved.selected_map_file
                && current.options == resolved.options
                && current.ai_player_count == resolved.ai_player_count
                && current.opponents.len() == resolved.opponents.len()
                && current
                    .opponents
                    .iter()
                    .zip(&resolved.opponents)
                    .all(|(left, right)| left.raw_slot_index == right.raw_slot_index
                        && left.difficulty == right.difficulty
                        && left.start_position == right.start_position
                        && left.team == right.team),
            "assignment completion cannot replace admitted setup identity"
        );
        crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(resolved.clone())
            .map_err(|error| anyhow::anyhow!("unresolved network assignments: {error}"))?;
        anyhow::ensure!(
            resolved.house_roster_is_consistent(),
            "inconsistent resolved House roster"
        );
        *current = resolved;
        Ok(())
    }

    pub(crate) fn set_network_player_ready(
        &mut self,
        index: usize,
        ready: bool,
    ) -> anyhow::Result<()> {
        self.network_admission_mut(index)?.ready = ready;
        Ok(())
    }

    /// `005E9850` clears node+77 of every node not named `0xA8D610`. WOL
    /// calls it on host option edits (C2 `0078F9C1`/`0078FA97`) and whenever a
    /// user joins the game channel (`007A6625`). The host's Create copies its
    /// own name there (`0x78A0A9`); a WOL guest's join sink empties it
    /// (`0x7A620D`), so a guest clears every node, its own included. A LAN
    /// host calls it on a guest's preview request `0x1B` (`0x005E014B`);
    /// otherwise LAN peers retract acceptance through packet A0.
    pub(crate) fn clear_guest_readiness(&mut self) -> anyhow::Result<()> {
        let Some(NetworkSession {
            phase: NetworkPhase::Setup {
                launch, admission, ..
            },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        let keep = self.host.then_some(launch.local_player_index);
        for (index, entry) in admission.iter_mut().enumerate() {
            if Some(index) != keep {
                entry.ready = false;
            }
        }
        Ok(())
    }

    /// A new online options revision invalidates every remote agreement,
    /// even when a local optimistic edit made its echoed values compare equal.
    /// The host remains implicitly ready, as in clear_guest_readiness; each
    /// participant must publish fresh map/rules proof for this revision.
    pub(crate) fn invalidate_network_agreement(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.network.as_ref().is_some_and(|network| matches!(
                network.return_context.session_mode(),
                SessionMode::Lan | SessionMode::Wol
            )),
            "multiplayer agreement belongs to a network setup"
        );
        self.clear_guest_readiness()?;
        let Some(NetworkSession {
            phase: NetworkPhase::Setup {
                launch, admission, ..
            },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        for (index, entry) in admission.iter_mut().enumerate() {
            if index != launch.local_player_index {
                entry.map_status = 0;
            }
        }
        Ok(())
    }

    pub(crate) fn set_network_map_status(
        &mut self,
        index: usize,
        status: u8,
    ) -> anyhow::Result<()> {
        self.network_admission_mut(index)?.map_status = status;
        Ok(())
    }

    fn network_admission_mut(&mut self, index: usize) -> anyhow::Result<&mut ParticipantAdmission> {
        match self.network.as_mut().map(|network| &mut network.phase) {
            Some(NetworkPhase::Setup { admission, .. }) => admission
                .get_mut(index)
                .ok_or_else(|| anyhow::anyhow!("unknown participant")),
            _ => anyhow::bail!("network setup is not editable"),
        }
    }

    /// min(`[0xA8B548]`, `[0xA8BB0C]`), the row count the WOL slot tests
    /// scan. `[0xA8B548]` is written once, by `Init_Game` (`0x0052CA7C`),
    /// from Rules+0x14D0 while it still holds the constructor's 8
    /// (`0x00667331`): `[Maximums] Players` is read only by the later
    /// scenario reset (`0x00668A11`). `[0xA8BB0C]` is the options' raw
    /// capacity, 8 before any (`0x7A5890`); the WOL row limit 6 is not in it.
    /// RESIDUAL: a host's own map selection does not set the metadata yet,
    /// so a host reads 8.
    fn network_slot_test_rows(&self) -> i32 {
        let maximum_players = if self
            .network_setup_capacity()
            .is_some_and(|capacity| capacity > 8)
        {
            NETWORK_PLAYER_SLOT_COUNT as i32
        } else {
            8
        };
        let raw = self.network_map_metadata().map_or_else(
            || {
                if maximum_players > 8 {
                    self.network_setup_capacity().unwrap_or(8)
                } else {
                    8
                }
            },
            |metadata| metadata.player_capacity,
        );
        maximum_players.min(raw)
    }

    /// Rows admitting players are bounded by the active mode/map capacity.
    /// The native host-observer row0 exception adds one physical row without
    /// adding a player, up to the active eight/30-slot allocation (all 30 in
    /// VERA's rooms, whose spectator rows follow the players).
    pub(crate) fn network_player_row_limit(&self) -> u8 {
        let Some(NetworkPhase::Setup { capacity, .. }) =
            self.network.as_ref().map(|network| &network.phase)
        else {
            return 0;
        };
        let capacity = *capacity;
        let allocation = if capacity > 8 || self.network_several_observers() {
            NETWORK_PLAYER_SLOT_COUNT as i32
        } else {
            8
        };
        let extra = i32::from(self.network_observer_row() == Some(0) && capacity < allocation);
        capacity.saturating_add(extra).clamp(0, allocation) as u8
    }

    /// `0x005E9510`: no Open player row, including the host-observer row0
    /// exception. Apply the same admitted row range as the actual allocator.
    pub(crate) fn network_no_open_player_row(&self) -> bool {
        if self.network_setup().is_none() {
            return false;
        }
        !(0..self.network_player_row_limit())
            .any(|row| self.network_vacancy(row) == Some(NetworkVacancy::Open))
    }

    /// `0x005E9240`'s row for a new node: the first Open row for a player,
    /// the first open observer row for an observer, over all eight rows
    /// (all thirty in VERA's rooms, whose spectator rows follow the players).
    pub(crate) fn network_free_row(&self, observer: bool) -> Option<u8> {
        let wanted = if observer {
            NetworkVacancy::OpenObserver
        } else {
            NetworkVacancy::Open
        };
        let rows = if observer && self.network_several_observers() {
            NETWORK_PLAYER_SLOT_COUNT as u8
        } else if observer {
            self.network_setup_capacity()
                .unwrap_or(0)
                .max(8)
                .max(
                    self.network_observer_row()
                        .map_or(0, |row| i32::from(row) + 1),
                )
                .min(NETWORK_PLAYER_SLOT_COUNT as i32) as u8
        } else {
            self.network_player_row_limit()
        };
        (0..rows).find(|row| self.network_vacancy(*row) == Some(wanted))
    }

    /// A joining LAN node's row and role: [`Self::network_free_row`] for
    /// what it asked. In a VERA room a player who finds every seat taken
    /// watches while a spectator row is open, and one asking to watch where
    /// no spectator row is open takes a free seat, as online joiners do.
    pub(crate) fn network_join_row(&self, observer: bool) -> Option<(u8, bool)> {
        if self.network_room_full() {
            return None;
        }
        let other = || {
            self.network_several_observers()
                .then(|| self.network_free_row(!observer))
                .flatten()
                .map(|row| (row, !observer))
        };
        self.network_free_row(observer)
            .map(|row| (row, observer))
            .or_else(other)
    }

    /// `0x005E95E0`: the nodes, less an observer on the observer row (every
    /// observer in VERA's rooms), are more than
    /// [`Self::network_slot_test_rows`].
    pub(crate) fn network_over_capacity(&self) -> bool {
        let Some(launch) = self.network_setup() else {
            return false;
        };
        let limit = self.network_slot_test_rows();
        let observer_row = self.network_observer_row();
        let several = self.network_several_observers();
        let observers = launch
            .humans
            .iter()
            .filter(|human| {
                human.is_observer() && (several || human.raw_slot_index == observer_row)
            })
            .count();
        launch.humans.len() as i32 - observers as i32 > limit
    }

    pub(crate) fn network_player_ready(&self, index: usize) -> bool {
        match self.network.as_ref().map(|network| &network.phase) {
            Some(NetworkPhase::Setup { admission, .. }) => {
                admission.get(index).is_some_and(|entry| entry.ready)
            }
            _ => false,
        }
    }

    pub(crate) fn network_map_status(&self, index: usize) -> Option<u8> {
        match self.network.as_ref().map(|network| &network.phase) {
            Some(NetworkPhase::Setup { admission, .. }) => {
                admission.get(index).map(|entry| entry.map_status)
            }
            _ => None,
        }
    }

    /// Accept the final local response-time observations at host5DD48D or
    /// guest5DD2CD. The received host GO budget is not the guest's final timing.
    pub(crate) fn prepare_network_timing(
        &mut self,
        protocol: i32,
        response_time: impl FnMut() -> u32,
    ) -> anyhow::Result<()> {
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        anyhow::ensure!(
            !matches!(network.phase, NetworkPhase::Frozen { .. }),
            "accepted start is immutable"
        );
        network.max_ahead_frames = Some(crate::net::lockstep::initial_multiplayer_max_ahead(
            protocol,
            response_time,
        ));
        self.latency_fudge = 0;
        Ok(())
    }

    pub(crate) fn latency_fudge(&self) -> i32 {
        self.latency_fudge
    }

    /// EVENT_LATENCYFUDGE's write (`0x004C7F4F`). The event itself, its
    /// broadcast and the remote-sender message `TXT_PLAYER_CHANGED_LATENCY`
    /// (`0x004C7F7D`) travel through the transport, outside this owner.
    pub(crate) fn set_latency_fudge(&mut self, value: i32) {
        self.latency_fudge = value;
    }

    pub(crate) fn pause_event_limiters_mut(&mut self) -> &mut PauseEventLimiters {
        &mut self.pause_event_limiters
    }
    /// WOL Start Game Now timing block (`0x794F6F..0x7951F0`): MaxAhead
    /// comes from the agreed game speed, identical for host and guest.
    /// Return the transport's outputs to its existing service owner. VERA's
    /// extended roster uses the same signed maximum across every peer ping.
    pub(crate) fn prepare_wol_timing(
        &mut self,
        pings: impl IntoIterator<Item = i32>,
        channel_owner: bool,
    ) -> anyhow::Result<crate::net::lockstep::WolStartTiming> {
        anyhow::ensure!(
            self.network_mode() == Some(SessionMode::Wol),
            "WOL timing needs a WOL visit"
        );
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        let NetworkPhase::Setup { launch, .. } = &network.phase else {
            anyhow::bail!("WOL timing follows an admitted setup");
        };
        let timing =
            crate::net::lockstep::wol_start_timing(launch.options.game_speed, pings, channel_owner);
        network.max_ahead_frames = Some(timing.max_ahead);
        self.latency_fudge = timing.latency_fudge;
        Ok(timing)
    }

    /// OnGameStart's `[0xB779C4]` and Start Game Now's clan lists for the
    /// accepted WOL start.
    pub(crate) fn set_wol_start(&mut self, terms: WolStartTerms) {
        if let Some(network) = self.network.as_mut() {
            network.wol_start = terms;
        }
    }

    /// The WOL start terms: the setup's, then the frozen start's.
    pub(crate) fn network_wol_start(&self) -> Option<&WolStartTerms> {
        let network = self.network.as_ref()?;
        match &network.phase {
            NetworkPhase::Frozen { startup, .. } => Some(startup.wol_start()),
            _ => Some(&network.wol_start),
        }
    }

    pub(crate) fn network_max_ahead_frames(&self) -> Option<i32> {
        self.network.as_ref()?.max_ahead_frames
    }

    pub(crate) fn set_network_start_seed(&mut self, seed: u32) -> anyhow::Result<()> {
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit"))?;
        anyhow::ensure!(
            !matches!(network.phase, NetworkPhase::Frozen { .. }),
            "accepted start is immutable"
        );
        network.proposed_seed = Some(seed);
        Ok(())
    }

    pub(crate) fn network_start_seed(&self) -> Option<u32> {
        self.network.as_ref()?.proposed_seed
    }

    /// An accepted start freezes resolved host assignments and the agreed
    /// host seed. It never samples local time or resolves guest choices again.
    pub(crate) fn freeze_network_start(
        &mut self,
        agreed_seed: u32,
    ) -> anyhow::Result<crate::app::match_bootstrap::LoadingStartup> {
        let network = self
            .network
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no network visit is active"))?;
        let NetworkPhase::Setup { launch, .. } = &network.phase else {
            anyhow::bail!("network setup is not ready for start admission");
        };
        anyhow::ensure!(
            network.proposed_seed.is_none_or(|seed| seed == agreed_seed),
            "accepted host seed differs from admitted setup seed"
        );
        network.proposed_seed = Some(agreed_seed);
        admit_network_start_roster(launch)?;
        let max_ahead_frames = network
            .max_ahead_frames
            .ok_or_else(|| anyhow::anyhow!("network start requires accepted timing"))?;
        let NetworkPhase::Setup { launch, .. } =
            std::mem::replace(&mut network.phase, NetworkPhase::Lobby)
        else {
            unreachable!()
        };
        let startup = Arc::new(
            crate::app::match_bootstrap::PreparedNetworkStartup::new(
                launch,
                agreed_seed,
                network.return_context.session_mode(),
                self.host,
                max_ahead_frames,
                std::mem::take(&mut network.wol_start),
            )
            .with_local_map_source(
                network.local_map_source.clone(),
                network.local_map_effective_digest.clone(),
            )
            .with_random_map_preview(network.local_random_map_preview.clone()),
        );
        network.phase = NetworkPhase::Frozen {
            connected: (0..startup.launch_session().humans.len()).collect(),
            startup: Arc::clone(&startup),
        };
        Ok(crate::app::match_bootstrap::LoadingStartup::Network(
            startup,
        ))
    }

    /// Read-only identity gate for a synchronous loader resuming after normal
    /// network reducers ran. A retired/replaced admission cannot install its
    /// old finished result, even when its local progress already reached100.
    pub(crate) fn admits_network_start(
        &self,
        startup: &Arc<crate::app::match_bootstrap::PreparedNetworkStartup>,
    ) -> bool {
        matches!(&self.network,
            Some(NetworkSession { phase: NetworkPhase::Frozen { startup: admitted, .. }, .. })
                if Arc::ptr_eq(admitted, startup))
    }

    /// The loader borrows the same frozen owner as the shell. It cannot adopt
    /// a second mutable setup or rewrite LAN/WOL/host identity during loading.
    pub(crate) fn assert_network_start(
        &self,
        startup: &Arc<crate::app::match_bootstrap::PreparedNetworkStartup>,
    ) {
        assert!(
            self.admits_network_start(startup),
            "loading must consume the admitted immutable start"
        );
        assert_eq!(self.network_mode(), Some(startup.mode()));
        assert_eq!(self.host, startup.host());
    }

    pub(crate) fn take_network_loading_requests(&mut self) -> Vec<NetworkLoadingRequest> {
        self.network.as_mut().map_or_else(Vec::new, |network| {
            std::mem::take(&mut network.loading_requests)
        })
    }

    /// Borrowed synchronous loading drains only its own publications. All chat,
    /// disconnect and other requests retain their existing FIFO/consumer.
    pub(crate) fn take_network_loading_progress_reports(&mut self) -> Vec<i32> {
        let mut reports = Vec::new();
        if let Some(network) = self.network.as_mut() {
            network.loading_requests.retain(|request| {
                if let NetworkLoadingRequest::ReportProgress(percent) = request {
                    reports.push(*percent);
                    false
                } else {
                    true
                }
            });
        }
        reports
    }

    pub(crate) fn report_network_loading_progress(&mut self, percent: i32) {
        if let Some(network) = self.network.as_mut()
            && matches!(network.phase, NetworkPhase::Frozen { .. })
        {
            network
                .loading_requests
                .push(NetworkLoadingRequest::ReportProgress(percent));
        }
    }

    /// 0x0064AAE0's kick proposal for participant `target_source`, sent to
    /// every other connected node.
    pub(crate) fn request_kick_proposal(&mut self, target_source: usize) {
        if let Some(network) = self.network.as_mut()
            && matches!(network.phase, NetworkPhase::Frozen { .. })
        {
            network
                .loading_requests
                .push(NetworkLoadingRequest::ProposeKick { target_source });
        }
    }

    /// One in-battle chat packet to `recipients`.
    pub(crate) fn request_chat(&mut self, recipients: Vec<usize>, text: String) {
        if let Some(network) = self.network.as_mut()
            && matches!(
                network.phase,
                NetworkPhase::Frozen { .. } | NetworkPhase::CooperativeRound { .. }
            )
        {
            network
                .loading_requests
                .push(NetworkLoadingRequest::Chat { recipients, text });
        }
    }

    pub(crate) fn request_beacon(
        &mut self,
        record: crate::net::lan_messages::ingame::BeaconRecord,
    ) {
        if let Some(network) = self.network.as_mut()
            && matches!(network.phase, NetworkPhase::Frozen { .. })
        {
            network
                .loading_requests
                .push(NetworkLoadingRequest::Beacon(record));
        }
    }

    pub(crate) fn request_taunt(&mut self, recipients: Vec<usize>, byte: u8) {
        if let Some(network) = self.network.as_mut()
            && matches!(network.phase, NetworkPhase::Frozen { .. })
        {
            network
                .loading_requests
                .push(NetworkLoadingRequest::Taunt { recipients, byte });
        }
    }

    /// The frozen launch's owner key of the House network participant
    /// `source_index` controls, its own or one it shares.
    pub(crate) fn network_source_owner(&self, source_index: usize) -> Option<String> {
        let launch = self.network_setup()?;
        crate::sim::scenario_bootstrap::normalized_launch_slots(launch)
            .into_iter()
            .find(|slot| slot.is_controlled_by(source_index))
            .map(|slot| slot.owner_name)
    }

    /// The network participants controlling the House with owner key `owner`.
    pub(crate) fn network_owner_sources(&self, owner: &str) -> Vec<usize> {
        let Some(launch) = self.network_setup() else {
            return Vec::new();
        };
        crate::sim::scenario_bootstrap::normalized_launch_slots(launch)
            .into_iter()
            .find(|slot| slot.owner_name == owner)
            .map_or_else(Vec::new, |slot| {
                slot.controllers
                    .iter()
                    .map(|controller| controller.human_index)
                    .collect()
            })
    }

    /// Network participant `source_index`'s command sender: the header byte
    /// of its records and its timing row. That is its House's index unless
    /// players share Houses.
    pub(crate) fn network_source_sender(&self, source_index: usize) -> Option<i8> {
        let launch = self.network_setup()?;
        crate::sim::scenario_bootstrap::normalized_launch_slots(launch)
            .iter()
            .flat_map(|slot| &slot.controllers)
            .find(|controller| controller.human_index == source_index)
            .map(|controller| controller.source)
    }

    /// The network participant sending as `sender`.
    pub(crate) fn network_sender_source(&self, sender: i8) -> Option<usize> {
        let launch = self.network_setup()?;
        crate::sim::scenario_bootstrap::normalized_launch_slots(launch)
            .iter()
            .flat_map(|slot| &slot.controllers)
            .find(|controller| controller.source == sender)
            .map(|controller| controller.human_index)
    }

    /// VERA shared control: whether another connected participant controls
    /// the House participant `source_index` controls.
    pub(crate) fn network_teammate_connected(&self, source_index: usize) -> bool {
        let Some(launch) = self.network_setup() else {
            return false;
        };
        let connected = self.network_connected_source_indices();
        crate::sim::scenario_bootstrap::normalized_launch_slots(launch)
            .iter()
            .find(|slot| slot.is_controlled_by(source_index))
            .is_some_and(|slot| {
                slot.controllers.iter().any(|controller| {
                    controller.human_index != source_index
                        && connected.contains(&controller.human_index)
                })
            })
    }

    /// `IPXManager::Delete_Connection 0x00540F00` and the node removal of
    /// DestroyConnection (`0x005DAA8A`, `--[0xA8B54C]`): the participant
    /// leaves the live membership and the transport is asked to drop it.
    /// False when it is the local player or no longer connected.
    pub(crate) fn disconnect_network_participant(
        &mut self,
        source_index: usize,
        reason: i32,
    ) -> bool {
        let Some(network) = self.network.as_mut() else {
            return false;
        };
        let NetworkPhase::Frozen { startup, connected } = &mut network.phase else {
            return false;
        };
        if source_index == startup.launch_session().local_player_index {
            return false;
        }
        let Some(position) = connected.iter().position(|index| *index == source_index) else {
            return false;
        };
        connected.remove(position);
        network
            .loading_requests
            .push(NetworkLoadingRequest::Disconnect {
                source_index,
                reason,
            });
        true
    }

    pub(crate) fn network_connected_source_indices(&self) -> &[usize] {
        match self.network.as_ref().map(|network| &network.phase) {
            Some(
                NetworkPhase::Frozen { connected, .. }
                | NetworkPhase::CooperativeRound { connected, .. },
            ) => connected,
            _ => &[],
        }
    }

    /// Match exit or failed loading returns to the active service lobby while
    /// retaining its connection/return context. A later setup is a new admission.
    pub(crate) fn return_to_network_lobby(&mut self) -> Option<NetworkReturnContext> {
        let network = self.network.as_mut()?;
        network.phase = NetworkPhase::Lobby;
        network.endpoints.clear();
        network.loading_requests.clear();
        network.map_metadata = None;
        network.local_map_source = None;
        network.local_map_effective_digest = None;
        network.local_random_map_preview = None;
        network.proposed_seed = None;
        network.max_ahead_frames = None;
        Some(network.return_context)
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
    let mut capacity = capacity.min(mode.class.setup_player_limit().map_or(
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
    if mode.class.lists_in_skirmish()
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
    if mode.class.lists_in_skirmish() && !wol_room {
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
    launch.rebuild_house_roster();
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
    // VERA shared control: outside Cooperative a player may take a start
    // other players hold, to share their House.
    let shared = launch.mode.class != crate::skirmish_modes::MpModeClass::Cooperative;
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
/// exact map.
fn admit_network_start_roster(launch: &SkirmishLaunchSession) -> anyhow::Result<()> {
    validate_network_rows(launch)?;
    anyhow::ensure!(
        launch.house_roster_is_consistent(),
        "network House constructor cache disagrees with the participant roster"
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

#[cfg(test)]
mod tests {
    use super::{MatchSession, NetworkRoom, SessionMode};

    pub(super) fn network_fixture() -> crate::skirmish_launch::SkirmishLaunchSession {
        let profile: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let mut launch: crate::skirmish_launch::SkirmishLaunchSession =
            serde_json::from_value(profile["launch"].clone()).unwrap();
        launch.opponents.clear();
        launch.ai_player_count = 0;
        let mut guest = launch.local_player().clone();
        guest.name = "Guest".into();
        guest.raw_slot_index = Some(1);
        guest.slot.color_index = 2;
        launch.humans.push(guest);
        launch.local_player_index = 1;
        launch
    }

    #[test]
    fn thirty_player_setup_keeps_last_row_identity_and_cooperative_capacity() {
        let mut launch = network_fixture();
        let template = launch.humans[0].clone();
        launch.humans = (0..30)
            .map(|row| {
                let mut human = template.clone();
                human.name = format!("Player{row}");
                human.raw_slot_index = Some(row);
                human
            })
            .collect();
        launch.local_player_index = 29;
        launch.rebuild_house_roster();
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::Wol);
        session
            .accept_network_setup(launch.clone(), false, None, false)
            .unwrap();
        session
            .accept_network_host_setup(
                launch.clone(),
                [super::NetworkVacancy::Open; 30],
                30,
                None,
                false,
            )
            .unwrap();
        assert_eq!(session.network_setup_capacity(), Some(30));
        assert!(session.network_free_row(false).is_none());
        let first = session.network_row_choices(0).unwrap();
        let mut last = session.network_row_choices(29).unwrap();
        last.start_position = crate::skirmish_launch::LaunchStartPosition::Position(29);
        session.set_network_row_choices(29, last.clone()).unwrap();
        session.set_network_player_ready(29, true).unwrap();
        session.remove_network_participant(7).unwrap();
        let current = session.network_setup().unwrap();
        assert_eq!(current.local_player_index, 28);
        assert_eq!(current.local_player().name, "Player29");
        assert_eq!(session.network_row_choices(29), Some(last));
        assert_eq!(session.network_row_choices(0), Some(first));
        assert!(session.network_player_ready(28));
        let cooperative = crate::skirmish_launch::SkirmishLaunchMode::from_game_mode(
            &crate::skirmish_modes::stock_skirmish_modes()
                .into_iter()
                .find(|mode| mode.class.uses_cooperative_setup())
                .unwrap(),
        );
        assert_eq!(
            super::row_capacity(SessionMode::Wol, NetworkRoom::default(), &cooperative, 30),
            2
        );
    }

    #[test]
    fn extended_player_admission_stops_at_capacity_and_shrink_closes_hidden_rows() {
        use super::{NetworkRowKind, NetworkVacancy};
        let mut launch = network_fixture();
        launch.local_player_index = 0;
        let template = launch.humans[0].clone();
        let mode = launch.mode.clone();
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::Wol);
        session
            .accept_network_setup(launch, true, None, false)
            .unwrap();
        session
            .select_network_map(mode.clone(), "NINE.MAP".into(), 9)
            .unwrap();
        for row in 2..9 {
            assert_eq!(session.network_free_row(false), Some(row));
            let mut human = template.clone();
            human.name = format!("Player{row}");
            human.raw_slot_index = Some(row);
            session.add_network_participant(human).unwrap();
        }
        assert!(session.network_free_row(false).is_none());
        assert!(session.network_no_open_player_row());

        session
            .select_network_map(mode.clone(), "THIRTY.MAP".into(), 30)
            .unwrap();
        session
            .set_network_row_kind(
                12,
                NetworkRowKind::Ai(crate::skirmish_launch::AiDifficulty::Easy),
            )
            .unwrap();
        let mut last = template;
        last.name = "Last".into();
        last.raw_slot_index = Some(29);
        session.add_network_participant(last).unwrap();
        let removed = session
            .select_network_map(mode.clone(), "NINE.MAP".into(), 9)
            .unwrap();
        assert_eq!(
            removed
                .iter()
                .map(|human| human.name.as_str())
                .collect::<Vec<_>>(),
            ["Last"]
        );
        assert_eq!(session.network_vacancy(12), Some(NetworkVacancy::Closed));
        assert_eq!(session.network_vacancy(29), Some(NetworkVacancy::Closed));
        assert!(session.network_display_ai(12).is_none());
        assert!(session.network_free_row(false).is_none());
        session
            .set_network_row_kind(29, NetworkRowKind::OpenObserver)
            .unwrap();
        assert_eq!(session.network_free_row(true), Some(29));
        session
            .select_network_map(mode, "THIRTY.MAP".into(), 30)
            .unwrap();
        assert_eq!(session.network_free_row(false), Some(9));
        assert_eq!(session.network_free_row(true), Some(29));
    }

    #[test]
    fn extended_host_observer_keeps_the_extra_player_row() {
        let mut launch = network_fixture();
        launch.local_player_index = 0;
        launch.humans[0].set_observer(true);
        let template = launch.humans[1].clone();
        let mode = launch.mode.clone();
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::LanLobby);
        session
            .accept_network_setup(launch, true, Some(0), false)
            .unwrap();
        session
            .select_network_map(mode, "NINE.MAP".into(), 9)
            .unwrap();
        assert_eq!(session.network_observer_row(), Some(0));
        for row in 2..=9 {
            assert_eq!(session.network_free_row(false), Some(row));
            let mut human = template.clone();
            human.name = format!("Player{row}");
            human.raw_slot_index = Some(row);
            session.add_network_participant(human).unwrap();
        }
        assert!(session.network_free_row(false).is_none());
        assert!(session.network_no_open_player_row());
        assert!(!session.network_over_capacity());
    }

    /// A VERA room (`several_observers`) on FOUR.MAP whose host keeps a
    /// spectator row open; the guest plays from row 1.
    fn vera_room() -> MatchSession {
        let mut launch = network_fixture();
        launch.local_player_index = 0;
        let mode = launch.mode.clone();
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::Wol);
        session
            .accept_network_setup(launch, true, Some(7), true)
            .unwrap();
        session
            .select_network_map(mode, "FOUR.MAP".into(), 4)
            .unwrap();
        session.set_network_spectators(true).unwrap();
        session
    }

    /// The rows open for an observer.
    fn observer_rows(session: &MatchSession) -> Vec<u8> {
        (0..30)
            .filter(|row| {
                session.network_vacancy(*row) == Some(super::NetworkVacancy::OpenObserver)
            })
            .collect()
    }

    /// Whether the guest watches, and its row.
    fn guest(session: &MatchSession) -> (bool, Option<u8>) {
        let guest = &session.network_setup().unwrap().humans[1];
        (guest.is_observer(), guest.raw_slot_index)
    }

    /// The spectator row a VERA room keeps open: a guest who watches takes
    /// it and the next opens. With no seat open it keeps watching, and a
    /// LAN guest plays again from the first open seat.
    #[test]
    fn a_vera_room_guest_watches_and_plays_again() {
        use super::NetworkRowKind;
        let mut session = vera_room();
        assert_eq!(observer_rows(&session), [4]);
        let slot = session.network_setup().unwrap().humans[1].slot.clone();
        session
            .accept_network_peer_choices(1, slot.clone(), true, None)
            .unwrap();
        assert_eq!(guest(&session), (true, Some(4)));
        assert_eq!(observer_rows(&session), [5]);
        for row in 1..4 {
            session
                .choose_network_row_kind(row, NetworkRowKind::Closed)
                .unwrap();
        }
        session
            .accept_lan_peer_choices(1, slot.clone(), false)
            .unwrap();
        assert_eq!(guest(&session), (true, Some(4)));
        session
            .choose_network_row_kind(2, NetworkRowKind::Open)
            .unwrap();
        session.accept_lan_peer_choices(1, slot, false).unwrap();
        assert_eq!(guest(&session), (false, Some(2)));
        assert_eq!(observer_rows(&session), [4]);
    }

    /// Spectators who ask to play while no seat is free wait in the join
    /// queue in order: the first takes the next free seat, and asking to
    /// watch or leaving the room gives up the place.
    #[test]
    fn a_vera_room_queues_its_spectators_for_the_seats_that_free() {
        use super::NetworkRowKind;
        let mut session = vera_room();
        let slot = session.network_setup().unwrap().humans[1].slot.clone();
        session
            .accept_network_peer_choices(1, slot.clone(), true, None)
            .unwrap();
        let mut third = session.network_setup().unwrap().humans[1].clone();
        third.name = "Third".into();
        third.raw_slot_index = Some(5);
        session.add_network_participant(third).unwrap();
        for row in 1..4 {
            session
                .choose_network_row_kind(row, NetworkRowKind::Closed)
                .unwrap();
        }
        for index in [2, 1] {
            session
                .accept_network_peer_choices(index, slot.clone(), false, None)
                .unwrap();
        }
        assert_eq!(session.network_queue(), ["Third", "Guest"]);
        assert_eq!(session.network_queue_next(), None);
        session
            .choose_network_row_kind(2, NetworkRowKind::Open)
            .unwrap();
        assert_eq!(session.network_queue_next(), Some(2));
        session
            .accept_network_peer_choices(2, slot.clone(), false, None)
            .unwrap();
        assert_eq!(session.network_queue_place(1), Some(1));
        session
            .accept_network_peer_choices(1, slot.clone(), true, None)
            .unwrap();
        assert!(session.network_queue().is_empty());
        session
            .accept_network_peer_choices(1, slot, false, None)
            .unwrap();
        session.remove_network_participant(1).unwrap();
        assert!(session.network_queue().is_empty());
    }

    /// No AI plays while anyone watches (`0x5EC9F0`'s
    /// TXT_NO_AI_WITH_OBSERVER): an AI closes the spectator row, and a
    /// guest asking to watch keeps its seat. A room without spectators
    /// closes it too; it follows a spectator who joins and leaves.
    #[test]
    fn a_vera_room_closes_its_spectator_row_for_ai_and_the_host() {
        use super::NetworkRowKind;
        let mut session = vera_room();
        session
            .choose_network_row_kind(
                2,
                NetworkRowKind::Ai(crate::skirmish_launch::AiDifficulty::Easy),
            )
            .unwrap();
        assert!(observer_rows(&session).is_empty());
        let slot = session.network_setup().unwrap().humans[1].slot.clone();
        session
            .accept_network_peer_choices(1, slot, true, None)
            .unwrap();
        assert_eq!(guest(&session), (false, Some(1)));
        session
            .choose_network_row_kind(2, NetworkRowKind::Open)
            .unwrap();
        assert_eq!(observer_rows(&session), [4]);
        session.set_network_spectators(false).unwrap();
        assert!(observer_rows(&session).is_empty());
        session.set_network_spectators(true).unwrap();
        assert_eq!(observer_rows(&session), [4]);
        let mut spectator = session.network_setup().unwrap().humans[1].clone();
        spectator.name = "Spectator".into();
        spectator.raw_slot_index = Some(4);
        spectator.set_observer(true);
        let index = session.add_network_participant(spectator).unwrap();
        assert_eq!(observer_rows(&session), [5]);
        session.remove_network_participant(index).unwrap();
        assert_eq!(observer_rows(&session), [4]);
    }

    /// A VERA room seats its map's players. A slot its host removes takes no
    /// joiner, who watches instead, or finds the game full once the room
    /// takes no spectators; the next map opens the slot again.
    #[test]
    fn a_vera_room_seats_its_map_and_reopens_removed_slots_for_the_next() {
        use super::{NetworkRowKind, NetworkVacancy};
        let mut session = vera_room();
        let mode = session.network_setup().unwrap().mode.clone();
        assert_eq!(session.network_player_row_limit(), 4);
        for row in [3, 2] {
            session
                .choose_network_row_kind(row, NetworkRowKind::Closed)
                .unwrap();
        }
        assert_eq!(session.network_join_row(false), Some((4, true)));
        // Its Max players counts the spectators: two people fill it.
        session.set_network_people_limit(2).unwrap();
        assert_eq!(session.network_join_row(false), None);
        session.set_network_people_limit(30).unwrap();
        session.set_network_spectators(false).unwrap();
        assert_eq!(session.network_join_row(false), None);
        session
            .select_network_map(mode, "EIGHT.MAP".into(), 8)
            .unwrap();
        assert!((2..8).all(|row| session.network_vacancy(row) == Some(NetworkVacancy::Open)));
    }

    /// A VERA room stays open until its host's Max players: with every seat
    /// taken, a newcomer gets the last AI's seat. With spectators off its
    /// limit is the people in it and the seats AI hold or leave open.
    #[test]
    fn a_vera_room_gives_a_newcomer_the_last_ai_seat_until_its_limit() {
        use super::NetworkRowKind;
        let easy = NetworkRowKind::Ai(crate::skirmish_launch::AiDifficulty::Easy);
        let mut session = vera_room();
        for row in [2, 3] {
            session.choose_network_row_kind(row, easy).unwrap();
        }
        assert!(session.network_no_open_player_row());
        assert_eq!(session.network_people_capacity(), Some(30));
        session.set_network_spectators(false).unwrap();
        assert_eq!(session.network_people_capacity(), Some(4));
        // At its Max players the AI keep their seats.
        session.set_network_people_limit(2).unwrap();
        assert_eq!(session.network_people_capacity(), Some(2));
        assert_eq!(session.network_newcomer_ai_seat(), None);
        assert_eq!(session.free_network_seat_for_newcomer().unwrap(), None);
        session.set_network_people_limit(30).unwrap();
        assert_eq!(session.network_newcomer_ai_seat(), Some(3));
        assert_eq!(session.free_network_seat_for_newcomer().unwrap(), Some(3));
        assert_eq!(session.network_free_row(false), Some(3));
        let ai_rows: Vec<u8> = session
            .network_setup()
            .unwrap()
            .opponents
            .iter()
            .map(|ai| ai.raw_slot_index)
            .collect();
        assert_eq!(ai_rows, [2]);
    }

    /// A larger map's players take the spectators' rows: they move on past
    /// the new players, and stay there for a smaller map.
    #[test]
    fn a_vera_room_moves_its_spectators_past_new_players() {
        use super::NetworkVacancy;
        let mut session = vera_room();
        let mode = session.network_setup().unwrap().mode.clone();
        let slot = session.network_setup().unwrap().humans[1].slot.clone();
        session
            .accept_network_peer_choices(1, slot, true, None)
            .unwrap();
        let mut spectator = session.network_setup().unwrap().humans[1].clone();
        spectator.name = "Spectator".into();
        spectator.raw_slot_index = Some(5);
        session.add_network_participant(spectator).unwrap();
        let rows = |session: &MatchSession| {
            session.network_setup().unwrap().humans[1..]
                .iter()
                .map(|human| human.raw_slot_index)
                .collect::<Vec<_>>()
        };
        session
            .select_network_map(mode.clone(), "TWELVE.MAP".into(), 12)
            .unwrap();
        assert_eq!(rows(&session), [Some(12), Some(13)]);
        assert!((1..12).all(|row| session.network_vacancy(row) == Some(NetworkVacancy::Open)));
        assert_eq!(observer_rows(&session), [14]);
        let removed = session
            .select_network_map(mode, "FOUR.MAP".into(), 4)
            .unwrap();
        assert!(removed.is_empty());
        assert_eq!(rows(&session), [Some(12), Some(13)]);
        assert_eq!(observer_rows(&session), [14]);
    }

    #[test]
    fn a_vera_room_host_who_observes_leaves_every_player_row() {
        use super::NetworkVacancy;
        let mut launch = network_fixture();
        launch.local_player_index = 0;
        launch.humans[0].set_observer(true);
        let mode = launch.mode.clone();
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::LanLobby);
        session
            .accept_network_setup(launch, true, Some(0), true)
            .unwrap();
        // The host watches from row 0; rows 1..=8 seat eight players.
        assert_eq!(session.network_player_row_limit(), 9);
        assert_eq!(session.network_vacancy(8), Some(NetworkVacancy::Open));
        assert_eq!(session.network_vacancy(9), Some(NetworkVacancy::Closed));
        session
            .select_network_map(mode.clone(), "FOUR.MAP".into(), 4)
            .unwrap();
        assert!((5..30).all(|row| session.network_vacancy(row) == Some(NetworkVacancy::Closed)));
        session
            .select_network_map(mode, "TWELVE.MAP".into(), 12)
            .unwrap();
        assert!((5..13).all(|row| session.network_vacancy(row) == Some(NetworkVacancy::Open)));
        assert_eq!(session.network_vacancy(13), Some(NetworkVacancy::Closed));
    }

    #[test]
    fn a_vera_room_profile_keeps_spectator_rows_open() {
        use super::{NetworkRowKind, NetworkVacancy};
        let mut session = vera_room();
        session
            .set_network_row_kind(3, NetworkRowKind::Closed)
            .unwrap();
        // The host closed seat 3; rows 4.., the open one among them, are the
        // room's spectator rows.
        assert_eq!(observer_rows(&session), [4]);
        let remembered = session.network_remembered_vacancies();
        assert_eq!(remembered[3], Some(NetworkVacancy::Closed));
        assert!(
            remembered[4..]
                .iter()
                .all(|row| *row == Some(NetworkVacancy::Open))
        );
    }

    /// A newcomer in row 2, copied from the guest.
    fn newcomer(session: &mut MatchSession) {
        let mut newcomer = session.network_setup().unwrap().humans[1].clone();
        newcomer.name = "Newcomer".into();
        newcomer.raw_slot_index = Some(2);
        session.add_network_participant(newcomer).unwrap();
    }

    /// The people's teams.
    fn teams(session: &MatchSession) -> Vec<crate::skirmish_launch::LaunchTeam> {
        let humans = &session.network_setup().unwrap().humans;
        humans.iter().map(|human| human.slot.team).collect()
    }

    /// With Teams set, every seated player of a VERA room is on one of its
    /// teams: setting it deals them out in row order, a newcomer joins the
    /// team with the fewest players, whatever it picked, and a new map keeps
    /// them where they are.
    #[test]
    fn a_vera_room_with_teams_keeps_every_player_on_a_team() {
        use crate::skirmish_launch::LaunchTeam::Team;
        let mut session = vera_room();
        session.set_network_teams(Some(2)).unwrap();
        assert_eq!(teams(&session), [Team(0), Team(1)]);
        let mut pick = session.network_setup().unwrap().humans[1].slot.clone();
        pick.team = Team(0);
        session
            .accept_network_peer_choices(1, pick, false, None)
            .unwrap();
        newcomer(&mut session);
        assert_eq!(teams(&session), [Team(0), Team(0), Team(1)]);
        let mode = session.network_setup().unwrap().mode.clone();
        session
            .select_network_map(mode, "EIGHT.MAP".into(), 8)
            .unwrap();
        assert_eq!(teams(&session), [Team(0), Team(0), Team(1)]);
    }

    /// Four seats over two teams seat two a team. A guest cannot pick a
    /// full team and keeps its own; the host still moves anyone.
    #[test]
    fn a_vera_room_guest_cannot_pick_a_full_team() {
        use crate::skirmish_launch::LaunchTeam::Team;
        let mut session = vera_room();
        session.set_network_teams(Some(2)).unwrap();
        newcomer(&mut session);
        assert_eq!(teams(&session), [Team(0), Team(1), Team(0)]);
        assert_eq!(session.network_team_seats(), Some(2));
        let mut pick = session.network_setup().unwrap().humans[1].slot.clone();
        assert!(!session.admit_network_team(1, &mut pick));
        pick.team = Team(0);
        assert!(session.admit_network_team(1, &mut pick));
        assert_eq!(pick.team, Team(1));
        pick.team = Team(0);
        session
            .accept_network_peer_choices(1, pick, false, None)
            .unwrap();
        assert_eq!(teams(&session), [Team(0), Team(0), Team(0)]);
    }

    /// A VERA room has up to 15 teams, A to O, and a guest's online pick
    /// past gamemd's D stays until Teams goes Off, back to A to D.
    #[test]
    fn a_vera_room_keeps_a_guest_past_d_until_teams_is_off() {
        use crate::skirmish_launch::LaunchTeam::{self, Team};
        let mut session = vera_room();
        session.set_network_teams(Some(15)).unwrap();
        assert_eq!(teams(&session), [Team(0), Team(1)]);
        let mut pick = session.network_setup().unwrap().humans[1].slot.clone();
        pick.team = Team(14);
        assert!(!session.admit_network_team(1, &mut pick));
        session
            .accept_network_peer_choices(1, pick, false, None)
            .unwrap();
        assert_eq!(teams(&session), [Team(0), Team(14)]);
        session.set_network_teams(None).unwrap();
        assert_eq!(teams(&session), [Team(0), LaunchTeam::None]);
    }

    /// In a mode that must ally, a player who watched and is seated again
    /// joins the team with the fewest players, not Team A.
    #[test]
    fn a_vera_team_game_seats_a_returning_player_on_the_smallest_team() {
        use crate::skirmish_launch::LaunchTeam::{self, Team};
        let mut session = vera_room();
        session.editable_network_setup().unwrap().mode.must_ally = true;
        session.set_network_teams(Some(2)).unwrap();
        newcomer(&mut session);
        let mut pick = session.network_setup().unwrap().humans[1].slot.clone();
        session
            .accept_network_peer_choices(1, pick.clone(), true, None)
            .unwrap();
        // The host's Seat, as the join queue's, asks for no team.
        pick.team = LaunchTeam::None;
        session
            .accept_network_peer_choices(1, pick, false, None)
            .unwrap();
        assert_eq!(teams(&session), [Team(0), Team(1), Team(0)]);
    }

    #[test]
    fn a_vera_room_that_balances_itself_takes_no_team_picks() {
        use crate::skirmish_launch::LaunchTeam::Team;
        let mut session = vera_room();
        session.set_network_teams(Some(2)).unwrap();
        session.set_network_auto_balance(true).unwrap();
        assert_eq!(teams(&session), [Team(0), Team(1)]);
        let mut pick = session.network_setup().unwrap().humans[1].slot.clone();
        pick.team = Team(0);
        assert!(session.admit_network_team(1, &mut pick));
        assert_eq!(pick.team, Team(1));
        // Teams Off turns it off, and without Teams it stays off.
        session.set_network_teams(None).unwrap();
        session.set_network_auto_balance(true).unwrap();
        assert!(!session.network_auto_balance());
    }

    #[test]
    fn online_canonical_roster_preserves_admission_and_local_identity() {
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::Wol);
        let fixture = network_fixture();
        let host_name = fixture.humans[0].name.clone();
        let guest_name = fixture.humans[1].name.clone();
        session
            .accept_network_setup(fixture, false, None, false)
            .unwrap();
        session.set_network_player_ready(0, true).unwrap();
        session.set_network_map_status(0, 2).unwrap();
        session
            .canonicalize_online_roster(&[guest_name.clone(), host_name.clone()])
            .unwrap();
        let launch = session.network_setup().unwrap();
        assert_eq!(launch.local_player_index, 0);
        assert_eq!(launch.humans[0].name, guest_name);
        assert_eq!(launch.humans[1].name, host_name);
        assert!(session.network_player_ready(1));
        let unchanged = session.clone();
        assert!(
            session
                .canonicalize_online_roster(&[host_name.clone(), host_name])
                .is_err()
        );
        assert_eq!(
            session, unchanged,
            "rejected roster must not partially change admission"
        );
    }

    #[test]
    fn endpoint_bindings_follow_admitted_participants_and_source_compaction() {
        use super::NetworkReturnContext;
        use std::net::{Ipv4Addr, SocketAddrV4};
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::LanLobby);
        let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1234);
        assert!(session.bind_network_endpoint("absent", endpoint).is_err());
        let launch = network_fixture();
        let host = launch.humans[0].name.clone();
        session
            .accept_network_setup(launch, false, None, false)
            .unwrap();
        session.bind_network_endpoint(&host, endpoint).unwrap();
        assert!(session.bind_network_endpoint("Guest", endpoint).is_err());
        assert_eq!(session.network_endpoint_source(endpoint), Some(0));
        assert_eq!(session.network_endpoint(1), None);
        session.remove_network_participant(0).unwrap();
        assert_eq!(session.network_endpoint_source(endpoint), None);
        assert_eq!(session.network_setup().unwrap().local_player_index, 0);
        session.bind_network_endpoint("Guest", endpoint).unwrap();
        session.return_to_network_lobby();
        assert_eq!(session.network_endpoint_source(endpoint), None);
    }

    #[test]
    fn network_freeze_keeps_guest_identity_seed_and_return_context_through_loading() {
        use super::NetworkReturnContext;
        for (mode, context) in [
            (SessionMode::Lan, NetworkReturnContext::LanLobby),
            (SessionMode::Wol, NetworkReturnContext::Wol),
        ] {
            let mut session = MatchSession::default();
            session.enter_network(context);
            session
                .accept_network_setup(network_fixture(), false, Some(7), false)
                .unwrap();
            session.set_network_player_ready(0, true).unwrap();
            session.set_network_map_status(0, 2).unwrap();
            assert!(session.network_player_ready(0));
            session.set_network_start_seed(0x1234abcd).unwrap();
            session.prepare_network_timing(2, || 0).unwrap();
            assert!(session.freeze_network_start(123).is_err());
            assert_eq!(session.network_start_seed(), Some(0x1234abcd));
            let startup = session.freeze_network_start(0x1234abcd).unwrap();
            assert_eq!(startup.session_mode(), mode);
            let crate::app::match_bootstrap::LoadingStartup::Network(prepared) = &startup else {
                panic!("network variant");
            };
            session.assert_network_start(prepared);
            assert_eq!(session.network_connected_source_indices(), &[0, 1]);
            session.report_network_loading_progress(100);
            assert!(
                !session.disconnect_network_participant(1, 1),
                "local cannot time out itself"
            );
            assert!(session.disconnect_network_participant(0, 1));
            assert!(
                !session.disconnect_network_participant(0, 1),
                "disconnect is one-shot"
            );
            assert_eq!(session.network_connected_source_indices(), &[1]);
            assert_eq!(
                prepared.launch_session().humans.len(),
                2,
                "historical launch stays immutable"
            );
            assert_eq!(
                session.take_network_loading_requests(),
                vec![
                    super::NetworkLoadingRequest::ReportProgress(100),
                    super::NetworkLoadingRequest::Disconnect {
                        source_index: 0,
                        reason: 1
                    },
                ]
            );
            session.assert_network_start(prepared);
            assert_eq!(prepared.seed.value, 0x1234abcd);
            assert_eq!(prepared.launch_session().local_player_index, 1);
            assert_eq!(session.local_player_name(), Some("Guest"));
            assert!(!session.host());
            assert!(session.set_network_player_ready(0, false).is_err());
            assert!(
                session
                    .accept_network_setup(network_fixture(), true, Some(7), false)
                    .is_err()
            );
            assert_eq!(session.return_to_network_lobby(), Some(context));
            assert!(session.network_setup().is_none());
            assert_eq!(session.shell_mode(), Some(mode));
            session.leave_network();
            assert_eq!(session.shell_mode(), Some(SessionMode::Campaign));
            assert!(session.network_return_context().is_none());
        }
    }

    #[test]
    fn host_changes_preserve_peer_ready_until_service_update_and_removal_reindexes_local() {
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::LanLobby);
        session
            .accept_network_setup(network_fixture(), true, Some(7), false)
            .unwrap();
        session.set_network_player_ready(0, true).unwrap();
        session.set_network_map_status(0, 2).unwrap();
        let mut options = session.network_setup().unwrap().options.clone();
        options.short_game = !options.short_game;
        session.set_network_options(options).unwrap();
        assert!(session.network_player_ready(0));
        assert_eq!(session.network_map_status(0), Some(2));
        session.set_network_player_ready(0, false).unwrap();
        assert_eq!(session.network_map_status(0), Some(2));
        session.remove_network_participant(0).unwrap();
        assert_eq!(session.network_setup().unwrap().local_player_index, 0);
        assert_eq!(session.local_player_name(), Some("Guest"));
        assert!(
            session
                .network_setup()
                .unwrap()
                .house_roster_is_consistent()
        );
    }

    #[test]
    fn wol_rows_follow_mode_and_room_clears_guest_acceptance_and_times_from_game_speed() {
        let wol_host = || {
            let mut session = MatchSession::default();
            session.enter_network(super::NetworkReturnContext::Wol);
            let mut launch = network_fixture();
            launch.local_player_index = 0;
            session
                .accept_network_setup(launch, true, Some(7), false)
                .unwrap();
            session
        };
        let mut session = wol_host();
        let launch = session.network_setup().unwrap().clone();
        let file = launch.selected_map_file.clone().unwrap();
        let mut man_battle = launch.mode.clone();
        man_battle.class = crate::skirmish_modes::MpModeClass::ManBattle;
        session
            .select_network_map(man_battle.clone(), file.clone(), 8)
            .unwrap();
        // 5EC3A0 `0x5EC457`: WOL rows stop at six where LAN keeps eight.
        assert_eq!(session.network_setup_capacity(), Some(6));
        assert_eq!(session.network_row_limit(), Some(6));
        // `0x5EC41C..0x5EC430`: Battle and FreeForAll (vtable+40) skip the
        // limit outside tournament and clan rooms.
        session
            .select_network_map(launch.mode.clone(), file.clone(), 8)
            .unwrap();
        assert_eq!(session.network_setup_capacity(), Some(8));
        // A clan room rounds down to even and keeps the limit; a tournament
        // room holds two rows.
        for (room, capacity, expected, limit) in [
            (
                NetworkRoom {
                    tournament: false,
                    clan: true,
                },
                7,
                6,
                6,
            ),
            (
                NetworkRoom {
                    tournament: false,
                    clan: true,
                },
                5,
                4,
                6,
            ),
            (
                NetworkRoom {
                    tournament: true,
                    clan: false,
                },
                8,
                2,
                2,
            ),
            (
                NetworkRoom {
                    tournament: true,
                    clan: true,
                },
                3,
                2,
                6,
            ),
        ] {
            let mut session = wol_host();
            session.set_network_room(room);
            session
                .select_network_map(launch.mode.clone(), file.clone(), capacity)
                .unwrap();
            assert_eq!(
                (
                    session.network_setup_capacity(),
                    session.network_row_limit()
                ),
                (Some(expected), Some(limit)),
                "{room:?} on a map for {capacity}"
            );
        }
        // 5E9850 keeps only the node named 0xA8D610: the host's own name on
        // the host, empty on a guest, which therefore clears its own too.
        session.set_network_player_ready(0, true).unwrap();
        session.set_network_player_ready(1, true).unwrap();
        session.clear_guest_readiness().unwrap();
        assert!(session.network_player_ready(0));
        assert!(!session.network_player_ready(1));
        let mut guest = MatchSession::default();
        guest.enter_network(super::NetworkReturnContext::Wol);
        let mut launch = network_fixture();
        launch.local_player_index = 0;
        guest
            .accept_network_setup(launch, false, Some(7), false)
            .unwrap();
        guest.set_network_player_ready(0, true).unwrap();
        guest.set_network_player_ready(1, true).unwrap();
        guest.clear_guest_readiness().unwrap();
        assert!(!guest.network_player_ready(0) && !guest.network_player_ready(1));
        // Start Game Now: MaxAhead follows the agreed speed only.
        session.prepare_wol_timing([0; 8], true).unwrap();
        let speed = session.network_setup().unwrap().options.game_speed;
        assert_eq!(
            session.network_max_ahead_frames(),
            Some(crate::net::lockstep::wol_start_timing(speed, [0; 8], false).max_ahead)
        );
        // The same block seeds LatencyFudge, which FF's Connection trackbar opens on.
        assert_eq!(
            session.latency_fudge(),
            crate::net::lockstep::wol_start_timing(speed, [0; 8], true).latency_fudge
        );

        let mut lan = MatchSession::default();
        lan.enter_network(super::NetworkReturnContext::LanLobby);
        let mut launch = network_fixture();
        launch.local_player_index = 0;
        lan.accept_network_setup(launch.clone(), true, Some(7), false)
            .unwrap();
        lan.select_network_map(launch.mode.clone(), file, 8)
            .unwrap();
        assert_eq!(lan.network_setup_capacity(), Some(8));
        // A LAN host's 0x1B arm keeps only its own acceptance.
        lan.set_network_player_ready(0, true).unwrap();
        lan.set_network_player_ready(1, true).unwrap();
        lan.clear_guest_readiness().unwrap();
        assert!(lan.network_player_ready(0) && !lan.network_player_ready(1));
        assert!(lan.prepare_wol_timing([0; 8], true).is_err());
    }

    #[test]
    fn online_revision_retracts_all_remote_agreements_and_local_guest_acceptance() {
        for host in [false, true] {
            let mut session = MatchSession::default();
            session.enter_network(super::NetworkReturnContext::Wol);
            let launch = network_fixture();
            let local = launch.local_player_index;
            session
                .accept_network_setup(launch, host, None, false)
                .unwrap();
            for index in 0..2 {
                session.set_network_player_ready(index, true).unwrap();
                session.set_network_map_status(index, 1).unwrap();
            }
            session.invalidate_network_agreement().unwrap();
            assert_eq!(session.network_player_ready(local), host);
            assert!(!session.network_player_ready(1 - local));
            assert_eq!(session.network_map_status(1 - local), Some(0));
            assert_eq!(session.network_map_status(local), Some(1));
        }
    }

    #[test]
    fn lan_revision_retracts_local_acceptance_and_prior_peer_map_proof() {
        let mut guest = MatchSession::default();
        guest.enter_network(super::NetworkReturnContext::LanLobby);
        let launch = network_fixture();
        let local = launch.local_player_index;
        guest
            .accept_network_setup(launch, false, None, false)
            .unwrap();
        guest.set_network_player_ready(local, true).unwrap();
        guest.set_network_map_status(local, 1).unwrap();
        guest.set_network_map_status(1 - local, 1).unwrap();
        guest.invalidate_network_agreement().unwrap();
        assert!(!guest.network_player_ready(local));
        assert_eq!(guest.network_map_status(local), Some(1));
        assert_eq!(guest.network_map_status(1 - local), Some(0));
    }

    #[test]
    fn kick_unbinds_row_but_departure_owns_source_roster_compaction() {
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::LanLobby);
        session
            .accept_network_setup(network_fixture(), true, Some(7), false)
            .unwrap();
        session.set_network_player_ready(0, true).unwrap();
        session.set_network_map_status(0, 2).unwrap();
        let kicked = session
            .set_network_row_kind(
                0,
                super::NetworkRowKind::Ai(crate::skirmish_launch::AiDifficulty::Easy),
            )
            .unwrap()
            .unwrap();
        assert_eq!(kicked.raw_slot_index, Some(0));
        let setup = session.network_setup().unwrap();
        assert_eq!(setup.humans.len(), 2);
        assert_eq!(setup.humans[0].raw_slot_index, None);
        assert_eq!(setup.local_player_index, 1);
        assert_eq!(setup.required_start_count(), 3);
        assert!(setup.house_roster_is_consistent());
        assert!(session.network_player_ready(0));
        assert_eq!(session.network_map_status(0), Some(2));
        let mut rng = crate::sim::rng::SimRng::new(11);
        let resolved = setup.resolve_shell_random_assignments(&mut rng);
        assert_eq!(resolved.humans[0].name, kicked.name);
        session.accept_network_assignments(resolved).unwrap();
        session.remove_network_participant(0).unwrap();
        let setup = session.network_setup().unwrap();
        assert_eq!(setup.humans.len(), 1);
        assert_eq!(setup.local_player_index, 0);
        assert_eq!(setup.required_start_count(), 2);
        assert!(setup.house_roster_is_consistent());
    }

    #[test]
    fn cooperative_country_refresh_can_keep_the_ai_rows() {
        use crate::skirmish_launch::LaunchCountry;
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::LanLobby);
        let mut launch = network_fixture();
        launch
            .opponents
            .push(crate::skirmish_launch::SkirmishAiSlot {
                raw_slot_index: 2,
                country: LaunchCountry::Yuri,
                country_random: false,
                color_index: 3,
                color_random: false,
                start_position: crate::skirmish_launch::LaunchStartPosition::Auto,
                team: crate::skirmish_launch::LaunchTeam::None,
                difficulty: crate::skirmish_launch::AiDifficulty::Easy,
            });
        launch.ai_player_count = 1;
        launch.rebuild_house_roster();
        session
            .accept_network_setup(launch, false, None, false)
            .unwrap();
        let ai_country = |session: &MatchSession| {
            let ai = &session.network_setup().unwrap().opponents[0];
            (ai.country, ai.country_random)
        };
        // BC open's refreshes leave the AI rows to the profile restore.
        session
            .reset_network_countries(Some(LaunchCountry::Russia), false)
            .unwrap();
        let setup = session.network_setup().unwrap();
        assert!(
            setup
                .humans
                .iter()
                .all(|human| human.slot.country == LaunchCountry::Russia
                    && !human.slot.country_random)
        );
        assert_eq!(ai_country(&session), (LaunchCountry::Yuri, false));
        // Use Map's refresh (`+0x18`) takes every row.
        session.reset_network_countries(None, true).unwrap();
        assert_eq!(ai_country(&session), (LaunchCountry::Yuri, true));
    }

    #[test]
    fn select_scenario_keeps_rows_starts_and_teams() {
        use crate::skirmish_launch::{LaunchStartPosition, LaunchTeam};
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::LanLobby);
        let mut launch = network_fixture();
        launch.local_player_index = 0;
        launch.humans[0].slot.start_position = LaunchStartPosition::Position(3);
        launch.humans[1].slot.team = LaunchTeam::Team(2);
        session
            .accept_network_setup(launch.clone(), true, None, false)
            .unwrap();
        let before = session.network_setup().unwrap().clone();
        assert_eq!(
            before.humans[0].slot.start_position,
            LaunchStartPosition::Position(3)
        );
        // 0x005E7BF0 writes the map and [0xA8BB0C], nothing in the rows.
        session
            .select_network_scenario("C1A01MD.MAP".into(), 4)
            .unwrap();
        let after = session.network_setup().unwrap();
        assert_eq!(after.selected_map_file.as_deref(), Some("C1A01MD.MAP"));
        assert_eq!(session.network_setup_capacity(), Some(4));
        assert_eq!(after.humans, before.humans);
        assert_eq!(after.opponents, before.opponents);
    }

    #[test]
    fn open_rows_and_over_capacity_follow_the_wol_slot_tests() {
        // 0x5E9510 and 0x5E95E0 scan min(8, the options' raw capacity).
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::Wol);
        let launch = network_fixture();
        session
            .accept_network_setup(launch.clone(), false, Some(7), false)
            .unwrap();
        // Before any options the raw capacity is 8: rows 2..7 are Open.
        assert!(!session.network_no_open_player_row());
        let options =
            |session: &mut MatchSession, vacancies: [super::NetworkVacancy; 8], capacity: i32| {
                session
                    .accept_network_host_setup(launch.clone(), vacancies, capacity, None, false)
                    .unwrap();
                let metadata = super::NetworkMapMetadata {
                    display_name: "Map".into(),
                    player_capacity: capacity,
                    official: true,
                    byte_length: 1,
                    digest: "D".into(),
                    source_sha256: String::new(),
                    effective_digest: String::new(),
                };
                session
                    .accept_network_map_metadata(metadata, None, true)
                    .unwrap();
            };
        let open = [super::NetworkVacancy::Open; 8];
        options(&mut session, open, 4);
        assert!(!session.network_no_open_player_row());
        assert!(!session.network_over_capacity());
        // Two humans fill a two-row map: no Open row, not over-full.
        options(&mut session, open, 2);
        assert!(session.network_no_open_player_row());
        assert!(!session.network_over_capacity());
        // Closed rows are not Open.
        let mut closed = open;
        closed[2] = super::NetworkVacancy::Closed;
        closed[3] = super::NetworkVacancy::Closed;
        options(&mut session, closed, 4);
        assert!(session.network_no_open_player_row());
        // A third node on a two-row map is over-full.
        options(&mut session, open, 2);
        let mut third = session.network_setup().unwrap().humans[0].clone();
        third.name = "Third".into();
        third.raw_slot_index = Some(2);
        session.add_network_participant(third).unwrap();
        assert!(session.network_over_capacity());
    }

    /// VERA shared control: outside Cooperative a player may take a start
    /// another player holds, never an AI's (`0x5E9720`).
    #[test]
    fn a_player_may_share_a_players_start_but_not_an_ais() {
        use crate::skirmish_launch::LaunchStartPosition::Position;
        let mut launch = network_fixture();
        launch.humans[0].slot.start_position = Position(0);
        launch
            .opponents
            .push(crate::skirmish_launch::SkirmishAiSlot {
                raw_slot_index: 2,
                country: crate::skirmish_launch::LaunchCountry::America,
                country_random: false,
                color_index: 3,
                color_random: false,
                start_position: Position(1),
                team: crate::skirmish_launch::LaunchTeam::None,
                difficulty: crate::skirmish_launch::AiDifficulty::Hard,
            });
        let inactive = [super::NetworkAiOptions {
            difficulty: -1,
            country: -2,
            color: -2,
            start: -2,
            team: -2,
        }; crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT];
        assert!(!super::network_start_in_use(
            &launch,
            &inactive,
            Position(0)
        ));
        assert!(super::network_start_in_use(&launch, &inactive, Position(1)));
        launch.mode.class = crate::skirmish_modes::MpModeClass::Cooperative;
        assert!(super::network_start_in_use(&launch, &inactive, Position(0)));
    }

    #[test]
    fn a_peer_r_follows_the_wol_host_checks() {
        // 0x7A8260 'R': taken colour/start -> Random, team outside 0..3 ->
        // the mode default, observer moves and the vacate on a full session.
        use super::{NetworkRowKind, NetworkVacancy, PeerChoicesChange};
        use crate::skirmish_launch::{LaunchStartPosition, LaunchTeam};
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::Wol);
        let mut launch = network_fixture();
        launch.local_player_index = 0;
        launch.humans[0].slot.color_index = 5;
        launch.humans[0].slot.color_random = false;
        launch.humans[0].slot.start_position = LaunchStartPosition::Auto;
        let must_ally = launch.mode.must_ally;
        session
            .accept_network_setup(launch, true, Some(7), false)
            .unwrap();
        session
            .set_network_row_kind(7, NetworkRowKind::OpenObserver)
            .unwrap();
        let guest = |session: &MatchSession| session.network_setup().unwrap().humans[1].clone();
        let mut asked = guest(&session).slot;
        asked.color_index = 5;
        asked.color_random = false;
        asked.start_position = LaunchStartPosition::Position(0);
        asked.team = LaunchTeam::Team(7);
        let change = session
            .accept_network_peer_choices(1, asked, false, None)
            .unwrap();
        let after = guest(&session);
        assert_eq!(
            change,
            PeerChoicesChange {
                roster: false,
                options: true
            }
        );
        assert!(after.slot.color_random, "the host holds colour 5");
        assert_eq!(after.slot.start_position, LaunchStartPosition::Position(0));
        assert_eq!(
            after.slot.team,
            if must_ally {
                LaunchTeam::Team(0)
            } else {
                LaunchTeam::None
            }
        );
        // The same choices as held change nothing and publish nothing.
        let held = session
            .accept_network_peer_choices(1, after.slot.clone(), false, None)
            .unwrap();
        assert_eq!(held, PeerChoicesChange::default());
        // To the observer row and back to the first Open row.
        let change = session
            .accept_network_peer_choices(1, after.slot.clone(), true, None)
            .unwrap();
        assert!(change.roster && change.options);
        let observer = guest(&session);
        assert!(observer.is_observer());
        assert_eq!(observer.raw_slot_index, Some(7));
        assert_eq!(session.network_vacancy(1), Some(NetworkVacancy::Open));
        // Random asked again: the options still go out after the move.
        let change = session
            .accept_network_peer_choices(1, after.slot.clone(), false, None)
            .unwrap();
        assert!(change.roster && change.options);
        let player = guest(&session);
        assert!(!player.is_observer());
        assert_eq!(player.raw_slot_index, Some(1));
        assert_eq!(
            session.network_vacancy(7),
            Some(NetworkVacancy::OpenObserver)
        );
        // An observer asking to play with no Open row loses its row.
        session
            .accept_network_peer_choices(1, after.slot.clone(), true, None)
            .unwrap();
        for row in 1..7 {
            session
                .set_network_row_kind(row, NetworkRowKind::Closed)
                .unwrap();
        }
        assert!(session.network_no_open_player_row());
        let change = session
            .accept_network_peer_choices(1, after.slot.clone(), false, None)
            .unwrap();
        assert!(change.roster && change.options);
        assert_eq!(guest(&session).raw_slot_index, None);
        assert_eq!(
            session.network_vacancy(7),
            Some(NetworkVacancy::OpenObserver)
        );
        // A node without a row is ignored.
        let ignored = session
            .accept_network_peer_choices(1, after.slot, true, None)
            .unwrap();
        assert_eq!(ignored, PeerChoicesChange::default());
    }

    #[test]
    fn map_metadata_has_independent_verification_gate_and_transferred_filename_authority() {
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::LanLobby);
        let launch = network_fixture();
        session
            .accept_network_setup(launch.clone(), false, Some(7), false)
            .unwrap();
        let mut metadata = super::NetworkMapMetadata {
            display_name: "Advertised title".into(),
            player_capacity: 8,
            official: false,
            byte_length: 83573,
            digest: "CaseSensitive".into(),
            source_sha256: String::new(),
            effective_digest: String::new(),
        };
        assert!(
            session
                .network_map_requires_verification(&launch, &metadata)
                .unwrap()
        );
        session
            .accept_network_map_metadata(metadata.clone(), Some("Local title"), true)
            .unwrap();
        let mut changed_options = launch.clone();
        changed_options.options.short_game = !changed_options.options.short_game;
        assert!(
            !session
                .network_map_requires_verification(&changed_options, &metadata)
                .unwrap(),
            "general options do not repeat file probing/K publication"
        );
        let mut case_only = launch.clone();
        let file = case_only.selected_map_file.as_mut().unwrap();
        *file = if file.to_ascii_lowercase() == *file {
            file.to_ascii_uppercase()
        } else {
            file.to_ascii_lowercase()
        };
        assert_ne!(case_only.selected_map_file, launch.selected_map_file);
        assert!(
            !session
                .network_map_requires_verification(&case_only, &metadata)
                .unwrap(),
            "native filename comparison ignores case"
        );
        metadata.display_name = "Title-only packet".into();
        assert!(
            !session
                .network_map_requires_verification(&launch, &metadata)
                .unwrap()
        );
        session
            .accept_network_map_metadata(metadata.clone(), None, false)
            .unwrap();
        assert_eq!(
            session.network_map_metadata().unwrap().display_name,
            "Local title"
        );
        metadata.player_capacity = 2;
        assert!(
            session
                .network_map_requires_verification(&launch, &metadata)
                .unwrap()
        );
        session
            .accept_network_host_setup(
                launch.clone(),
                [super::NetworkVacancy::Open; 8],
                2,
                Some(7),
                false,
            )
            .unwrap();
        session
            .accept_network_map_metadata(metadata.clone(), None, true)
            .unwrap();
        assert_eq!(
            session.network_map_metadata().unwrap().display_name,
            "Title-only packet"
        );
        assert!(
            !session
                .network_map_requires_verification(&launch, &metadata)
                .unwrap()
        );
        assert_eq!(metadata.admission("CONTROLLED.MAP", false).status(), 2);
        assert!(
            metadata
                .admission("CONTROLLED.MAP", false)
                .accept_eligible()
        );
        metadata.official = true;
        assert_eq!(metadata.admission("CONTROLLED.MAP", false).status(), 0);
        assert!(
            !metadata
                .admission("CONTROLLED.MAP", false)
                .accept_eligible()
        );
        assert_eq!(metadata.admission("RandMap.Sed", false).status(), 1);
        assert_eq!(metadata.admission("CONTROLLED.MAP", true).status(), 1);
        session
            .accept_prepared_network_map_filename("CONTROLLED.MAP".into())
            .unwrap();
        session.prepare_network_timing(2, || 0).unwrap();
        let startup = session.freeze_network_start(13).unwrap();
        let crate::app::match_bootstrap::LoadingStartup::Network(prepared) = startup else {
            panic!("network");
        };
        assert_eq!(
            prepared.launch_session().selected_map_file.as_deref(),
            Some("CONTROLLED.MAP")
        );
        assert!(
            session
                .accept_prepared_network_map_filename("OTHER.MAP".into())
                .is_err()
        );
        session.return_to_network_lobby();
        assert!(session.network_map_metadata().is_none());
    }

    #[test]
    fn verified_download_keeps_advertised_name_until_freeze_and_rejects_stale_identity() {
        use crate::map::source::{TransferResource, install_received_asset, load_received_asset};
        let directory = crate::map::source::test_support::TestDirectory::new("session-map-lease");
        let bytes = b"[RandomMap]\nSeed=123\n";
        let digest = crate::util::sha256::sha256_hex(bytes);
        let source = install_received_asset(
            directory.path(),
            bytes.to_vec(),
            &digest,
            bytes.len(),
            TransferResource::Seed,
        )
        .unwrap()
        .source;
        let pin = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(directory.path().join(format!(".{digest}.sed.pin")))
            .unwrap();
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::Wol);
        let mut launch = network_fixture();
        launch.rebuild_house_roster();
        session
            .accept_network_setup(launch.clone(), false, None, false)
            .unwrap();
        let mut metadata = super::NetworkMapMetadata {
            display_name: "Shared map".into(),
            player_capacity: 8,
            official: false,
            byte_length: bytes.len() as i32,
            digest: String::new(),
            source_sha256: digest.clone(),
            effective_digest: "c".repeat(64),
        };
        session
            .accept_network_map_metadata(metadata.clone(), None, true)
            .unwrap();
        session
            .accept_verified_network_map_source(source.clone(), "c".repeat(64))
            .unwrap();
        assert_eq!(
            session.network_setup().unwrap().selected_map_file,
            launch.selected_map_file
        );
        assert!(
            !session.network_player_ready(1),
            "receiving bytes never readies the guest"
        );
        metadata.source_sha256 = "b".repeat(64);
        session
            .accept_network_map_metadata(metadata.clone(), None, true)
            .unwrap();
        assert!(session.network_local_map_source().is_none());
        assert!(
            session
                .accept_verified_network_map_source(source, "c".repeat(64))
                .is_err()
        );
        pin.try_lock()
            .expect("a rejected stale completion releases its lease");
        pin.unlock().unwrap();
        metadata.source_sha256 = digest.clone();
        session
            .accept_network_map_metadata(metadata, None, true)
            .unwrap();
        let source = load_received_asset(
            directory.path(),
            &digest,
            bytes.len(),
            TransferResource::Seed,
        )
        .unwrap()
        .source;
        session
            .accept_verified_network_map_source(source, "c".repeat(64))
            .unwrap();
        session.prepare_wol_timing([0; 2], false).unwrap();
        let startup = session.freeze_network_start(13).unwrap();
        let crate::app::match_bootstrap::LoadingStartup::Network(prepared) = startup else {
            panic!("network start");
        };
        assert_eq!(
            prepared.launch_session().selected_map_file,
            launch.selected_map_file
        );
        assert!(prepared.local_map_source().is_some());
        session.return_to_network_lobby();
        assert!(session.network_local_map_source().is_none());
        assert!(
            pin.try_lock().is_err(),
            "frozen startup retains the admitted source"
        );
        drop(prepared);
        pin.try_lock()
            .expect("lobby return and final startup drop release the source");
    }

    #[test]
    fn host_packet_preserves_admission_and_rejects_source_identity_changes_atomically() {
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::LanLobby);
        session
            .accept_network_setup(network_fixture(), false, Some(7), false)
            .unwrap();
        session.set_network_player_ready(0, true).unwrap();
        session.set_network_player_ready(1, true).unwrap();
        session.set_network_map_status(0, 2).unwrap();
        let mut incoming = session.network_setup().unwrap().clone();
        incoming.options.short_game = !incoming.options.short_game;
        incoming.humans[0].role = crate::skirmish_launch::LaunchHumanRole::Player { group: 2 };
        let vacancies = [super::NetworkVacancy::Closed; 8];
        session
            .accept_network_host_setup(incoming, vacancies, 4, Some(7), false)
            .unwrap();
        assert!(session.network_player_ready(0));
        assert!(session.network_player_ready(1));
        assert_eq!(session.network_map_status(0), Some(2));
        assert_eq!(session.network_setup_capacity(), Some(4));
        assert_eq!(session.network_observer_row(), Some(7));
        assert_eq!(
            session.network_vacancy(7),
            Some(super::NetworkVacancy::Closed)
        );
        let accepted = session.clone();
        let mut reordered = session.network_setup().unwrap().clone();
        reordered.humans.swap(0, 1);
        assert!(
            session
                .accept_network_host_setup(reordered, vacancies, 4, Some(7), false)
                .is_err()
        );
        assert_eq!(session, accepted);
        session.set_network_player_ready(1, false).unwrap();
        assert!(session.network_player_ready(0));
        assert!(!session.network_player_ready(1));
    }

    #[test]
    fn mode_role_is_distinct_from_team_and_assignment_cannot_change_selected_start() {
        use crate::skirmish_launch::{LaunchHumanRole, LaunchStartPosition};
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::LanLobby);
        session
            .accept_network_setup(network_fixture(), true, Some(7), false)
            .unwrap();
        let team = session.network_setup().unwrap().humans[0].slot.team;
        session
            .accept_network_human_role(0, LaunchHumanRole::Player { group: 2 })
            .unwrap();
        assert_eq!(session.network_setup().unwrap().humans[0].slot.team, team);
        let mut resolved = session.network_setup().unwrap().clone();
        resolved.humans[0].slot.start_position = LaunchStartPosition::Position(3);
        assert!(session.accept_network_assignments(resolved).is_err());
        session
            .accept_network_human_role(0, LaunchHumanRole::Observer)
            .unwrap();
        assert!(
            session
                .network_setup()
                .unwrap()
                .house_roster_is_consistent()
        );
        assert!(session.network_setup().unwrap().humans[0].is_observer());
        assert!(
            session
                .accept_network_human_role(0, LaunchHumanRole::Player { group: -1 })
                .is_err()
        );
    }

    #[test]
    fn multiplayer_settings_initialization_matches_native_rng_and_survives_session_changes() {
        use crate::sim::rng::{MainRng, SimRng};
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/network_options.json",
        ))
        .unwrap();
        for case in corpus["initialization_cases"]
            .as_array()
            .expect("native initialization cases")
        {
            let main: MainRng =
                SimRng::from_native_state_hex_for_test(case["initial_rng_hex"].as_str().unwrap())
                    .into();
            let mut session = MatchSession::default();
            let calls = case["calls"].as_array().unwrap();
            assert!(calls.len() >= 2, "native first/re-entry comparison");
            for (index, call) in calls.iter().enumerate() {
                session.prepare_multiplayer_settings(&main.draws());
                assert_eq!(
                    main.native_state_hex(),
                    call["rng_hex"].as_str().unwrap(),
                    "{} call {index}",
                    case["name"]
                );
                session.begin_offline(SessionMode::Skirmish, Some("Commander".into()));
            }
            // Loading another scenario does not reset the process's B07788.
            session.restore_from_save(SessionMode::Campaign, None);
            session.prepare_multiplayer_settings(&main.draws());
            assert_eq!(
                main.native_state_hex(),
                calls.last().unwrap()["rng_hex"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn process_start_is_campaign_with_no_participants() {
        let session = MatchSession::default();
        assert_eq!(session.shell_mode(), Some(SessionMode::Campaign));
        assert_eq!(session.participant_count(), 0);
        assert_eq!(session.local_player_name(), None);
        assert!(!session.host());
    }

    #[test]
    fn offline_skirmish_launch_seats_the_local_player_as_host() {
        let mut session = MatchSession::default();
        session.begin_offline(SessionMode::Skirmish, Some("Commander".to_owned()));
        assert_eq!(session.shell_mode(), Some(SessionMode::Skirmish));
        assert_eq!(session.participant_count(), 1);
        assert_eq!(session.local_player_name(), Some("Commander"));
        assert!(session.host());
    }

    #[test]
    fn campaign_launch_replaces_an_outgoing_skirmish_table() {
        let mut session = MatchSession::default();
        session.begin_offline(SessionMode::Skirmish, Some("Commander".to_owned()));
        session.begin_offline(SessionMode::Campaign, None);
        assert_eq!(session.shell_mode(), Some(SessionMode::Campaign));
        assert_eq!(session.participant_count(), 0);
        assert_eq!(session.local_player_name(), None);
        assert!(session.host());
    }

    #[test]
    fn saved_game_restore_hands_the_mode_to_the_restored_world() {
        let mut session = MatchSession::default();
        session.restore_from_save(SessionMode::Skirmish, Some("Saved"));
        assert_eq!(session.shell_mode(), None, "the restored world holds it");
        assert_eq!(session.local_player_name(), Some("Saved"));
        session.restore_from_save(SessionMode::Campaign, None);
        assert_eq!(session.shell_mode(), None);
        assert_eq!(session.local_player_name(), None);
    }

    #[test]
    fn restoring_a_save_keeps_only_a_visit_of_its_own_mode() {
        use super::NetworkReturnContext;
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::LanLobby);
        session.restore_from_save(SessionMode::Lan, None);
        assert_eq!(
            session.network_return_context(),
            Some(NetworkReturnContext::LanLobby)
        );
        session.restore_from_save(SessionMode::Wol, None);
        assert_eq!(session.network_return_context(), None);
    }

    #[test]
    fn game_mode_has_one_holder_through_install_shell_writes_and_drop() {
        let mut session = MatchSession::default();
        session.begin_offline(SessionMode::Skirmish, Some("Commander".to_owned()));
        session.hand_mode_to_world(SessionMode::Skirmish);
        assert_eq!(session.shell_mode(), None);
        // The finished world keeps holding it until the shell writes again.
        session.enter_network(super::NetworkReturnContext::LanLobby);
        assert_eq!(session.shell_mode(), Some(SessionMode::Lan));
        // A load that drops the world without a successor hands it back.
        session.hand_mode_to_world(SessionMode::Lan);
        session.retake_mode_from_world(SessionMode::Lan);
        assert_eq!(session.shell_mode(), Some(SessionMode::Lan));
        session.retake_mode_from_world(SessionMode::Skirmish);
        assert_eq!(session.shell_mode(), Some(SessionMode::Lan));
    }

    #[test]
    fn network_modes_route_queue_ai_to_the_multiplayer_body() {
        use crate::sim::world::CommandQueueRoute;
        for (mode, route) in [
            (SessionMode::Campaign, CommandQueueRoute::SinglePlayer),
            (SessionMode::Skirmish, CommandQueueRoute::SinglePlayer),
            (SessionMode::Lan, CommandQueueRoute::Multiplayer),
            (SessionMode::Wol, CommandQueueRoute::Multiplayer),
            (SessionMode::Other, CommandQueueRoute::SinglePlayer),
        ] {
            assert_eq!(mode.command_queue_route(), route, "{mode:?}");
        }
    }

    #[test]
    fn raw_game_mode_mapping_is_writer_proofed() {
        assert_eq!(SessionMode::from_game_mode(0), SessionMode::Campaign);
        assert_eq!(SessionMode::from_game_mode(3), SessionMode::Lan);
        assert_eq!(SessionMode::from_game_mode(4), SessionMode::Wol);
        assert_eq!(SessionMode::from_game_mode(5), SessionMode::Skirmish);
        for raw in [1, 2, -1, 99] {
            assert_eq!(SessionMode::from_game_mode(raw), SessionMode::Other);
        }
        assert!(SessionMode::Lan.is_network());
        assert!(SessionMode::Wol.is_network());
        assert!(!SessionMode::Campaign.is_network());
        assert!(!SessionMode::Skirmish.is_network());
        assert!(!SessionMode::Other.is_network());
    }
}
