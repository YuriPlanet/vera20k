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

use crate::net::lockstep::MultiplayerChecksumHistory;
use crate::skirmish_launch::{
    SkirmishAiSlot, SkirmishLaunchOptions, SkirmishLaunchSession, SkirmishLocalSlot,
};
use std::sync::Arc;

pub(crate) use crate::sim::scenario_session::ScenarioGameMode as SessionMode;

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
        vacancies: [NetworkVacancy; 8],
        /// Authoritative only while the corresponding row is vacant. Entering
        /// a human/AI row makes its canonical participant choices authoritative.
        inactive_choices: [SkirmishLocalSlot; 8],
        /// Native831700/8316A4: prior capacity and independent reserved observer row.
        capacity: i32,
        observer_row: Option<u8>,
    },
    Frozen {
        startup: Arc<crate::app::match_bootstrap::PreparedNetworkStartup>,
        /// Live source membership indexes the immutable historical launch.
        connected: Vec<usize>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NetworkLoadingRequest {
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
    map_metadata: Option<NetworkMapMetadata>,
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
    /// gamemd's 256-entry checksum ring at `0x00B04474`, written once per
    /// multiplayer frame by `Queue_AI_Multiplayer` (`0x006476A5`) and read by
    /// the FRAMEINFO comparison. Process-lifetime like the native global: no
    /// launch or load clears it; only the multiplayer frame-0 reset does.
    checksum_history: MultiplayerChecksumHistory,
    /// Process byte B07788, set by the first ReadMultiPlayerSettings call.
    /// It survives dialog, match and saved-game transitions.
    multiplayer_settings_initialized: bool,
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
            checksum_history: MultiplayerChecksumHistory::new(),
            multiplayer_settings_initialized: false,
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
            map_metadata: None,
            proposed_seed: None,
            max_ahead_frames: None,
            return_context,
            phase: NetworkPhase::Lobby,
        });
    }

    /// Adopt a service-admitted setup. Native LAN own-node creation precedes
    /// remote-node events (5E00B0); UI requests alone do not create participants.
    pub(crate) fn accept_network_setup(
        &mut self,
        mut launch: SkirmishLaunchSession,
        host: bool,
        observer_row: Option<u8>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.network.is_some(), "no network visit is active");
        anyhow::ensure!(
            observer_row.is_none_or(|row| row < 8),
            "observer reservation is outside setup rows"
        );
        anyhow::ensure!(
            !launch.humans.is_empty() && launch.local_player_index < launch.humans.len(),
            "network setup has no local participant"
        );
        anyhow::ensure!(
            launch.humans.len() + launch.opponents.len()
                <= crate::skirmish_launch::SKIRMISH_PLAYER_SLOT_COUNT,
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
        network.phase = NetworkPhase::Setup {
            capacity: 8,
            observer_row,
            launch,
            admission,
            vacancies: [NetworkVacancy::Open; 8],
            inactive_choices,
        };
        self.host = host;
        Ok(())
    }

    pub(crate) fn network_setup(&self) -> Option<&SkirmishLaunchSession> {
        match &self.network.as_ref()?.phase {
            NetworkPhase::Lobby => None,
            NetworkPhase::Setup { launch, .. } => Some(launch),
            NetworkPhase::Frozen { startup, .. } => Some(startup.launch_session()),
        }
    }

    fn editable_network_setup(&mut self) -> anyhow::Result<&mut SkirmishLaunchSession> {
        match self.network.as_mut().map(|network| &mut network.phase) {
            Some(NetworkPhase::Setup { launch, .. }) => Ok(launch),
            _ => anyhow::bail!("network setup is not editable"),
        }
    }

    /// Type3 roster admission appends a remote node. It does not reorder the
    /// accepted source vector or clear unrelated participant acknowledgements.
    pub(crate) fn add_network_participant(
        &mut self,
        human: crate::skirmish_launch::SkirmishHumanSlot,
    ) -> anyhow::Result<usize> {
        let Some(NetworkSession {
            phase: NetworkPhase::Setup {
                launch, admission, ..
            },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        anyhow::ensure!(
            launch.humans.len() + launch.opponents.len()
                < crate::skirmish_launch::SKIRMISH_PLAYER_SLOT_COUNT,
            "network setup has no free slot"
        );
        // A WOL guest's node has no row until the host's 'L' binds one.
        anyhow::ensure!(
            human.raw_slot_index.is_none_or(|row| {
                row < 8
                    && !launch
                        .humans
                        .iter()
                        .any(|entry| entry.raw_slot_index == Some(row))
                    && !launch
                        .opponents
                        .iter()
                        .any(|entry| entry.raw_slot_index == row)
            }),
            "participant row is occupied or out of range"
        );
        let index = launch.humans.len();
        launch.humans.push(human);
        admission.push(ParticipantAdmission {
            ready: false,
            map_status: 0,
        });
        launch.rebuild_house_roster();
        Ok(index)
    }

    pub(crate) fn remove_network_participant(&mut self, index: usize) -> anyhow::Result<()> {
        let Some(NetworkSession {
            phase: NetworkPhase::Setup {
                launch, admission, ..
            },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        anyhow::ensure!(
            index < launch.humans.len() && index != launch.local_player_index,
            "remote removal requires a remote participant"
        );
        launch.humans.remove(index);
        admission.remove(index);
        if index < launch.local_player_index {
            launch.local_player_index -= 1;
        }
        launch.rebuild_house_roster();
        Ok(())
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
    /// takes the mode default (`+0x34`/`+0x2C`). No sound, text or reply.
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
        let was_observer = human.is_observer();
        let observer_row = self.network_observer_row();
        let mut change = PeerChoicesChange::default();
        if was_observer && !observer {
            change.roster = true;
            change.options = true;
            if self.network_no_open_player_row() {
                self.set_network_row_kind(row, NetworkRowKind::OpenObserver)?;
                return Ok(change);
            }
            // 0x7A8838..0x7A88A4: country Random and a player role, the
            // observer row opened for observers, then 0x5E9240's row. The
            // country check still compares against the -3 read at 0x7A865F,
            // so the options always go out (0x7A8905..0x7A8915).
            self.set_network_row_kind(observer_row.unwrap_or(row), NetworkRowKind::OpenObserver)?;
            let free = self
                .network_free_row(false)
                .ok_or_else(|| anyhow::anyhow!("no Open row after 0x5E9510"))?;
            let human = &mut self.editable_network_setup()?.humans[index];
            human.set_observer(false);
            human.slot.country_random = true;
            human.raw_slot_index = Some(free);
        } else if !was_observer && observer {
            change.roster = true;
            change.options = true;
            let target = self.network_free_row(true);
            self.set_network_row_kind(row, NetworkRowKind::Open)?;
            if let Some(target) = target {
                // 0x7A89F4..0x7A8A6F: the node goes to [0x8316A4].
                let human = &mut self.editable_network_setup()?.humans[index];
                human.set_observer(true);
                human.raw_slot_index = Some(observer_row.unwrap_or(target));
            }
            self.editable_network_setup()?.rebuild_house_roster();
            return Ok(change);
        }
        let launch = self.editable_network_setup()?;
        let current = launch.humans[index].slot.clone();
        let native_country =
            |slot: &SkirmishLocalSlot| (!slot.country_random).then_some(slot.country);
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
        let native_color =
            |slot: &SkirmishLocalSlot| (!slot.color_random).then_some(slot.color_index);
        if native_color(&choices) != native_color(&current) {
            // 0x69B600: every node of the session and every AI row.
            let taken = native_color(&choices).is_some_and(|color| {
                launch
                    .humans
                    .iter()
                    .any(|human| native_color(&human.slot) == Some(color))
                    || launch
                        .opponents
                        .iter()
                        .any(|ai| !ai.color_random && ai.color_index == color)
            });
            slot.color_random = choices.color_random || taken;
            if !slot.color_random {
                slot.color_index = choices.color_index;
            }
            change.options = true;
        }
        if choices.start_position != current.start_position {
            // 0x5E9720: the nodes on a row and every AI row.
            let taken = choices.start_position != crate::skirmish_launch::LaunchStartPosition::Auto
                && (launch.humans.iter().any(|human| {
                    human.raw_slot_index.is_some()
                        && human.slot.start_position == choices.start_position
                }) || launch
                    .opponents
                    .iter()
                    .any(|ai| ai.start_position == choices.start_position));
            slot.start_position = if taken {
                crate::skirmish_launch::LaunchStartPosition::Auto
            } else {
                choices.start_position
            };
            change.options = true;
        }
        if choices.team != current.team {
            // Mode +0x34 (0x5D5DE0) admits only 0..3; +0x2C (0x5D5DC0) is
            // Team 0 under MustAlly, else none.
            use crate::skirmish_launch::LaunchTeam;
            slot.team = match choices.team {
                LaunchTeam::Team(team) if team <= 3 => choices.team,
                _ if launch.mode.must_ally => LaunchTeam::Team(0),
                _ => LaunchTeam::None,
            };
            change.options = true;
        }
        launch.humans[index].slot = slot;
        launch.rebuild_house_roster();
        Ok(change)
    }

    pub(crate) fn network_vacancy(&self, row: u8) -> Option<NetworkVacancy> {
        let NetworkPhase::Setup {
            launch, vacancies, ..
        } = &self.network.as_ref()?.phase
        else {
            return None;
        };
        if row >= 8
            || launch
                .humans
                .iter()
                .any(|human| human.raw_slot_index == Some(row))
            || launch.opponents.iter().any(|ai| ai.raw_slot_index == row)
        {
            return None;
        }
        Some(vacancies[usize::from(row)])
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
        if let Some(ai) = launch.opponents.iter().find(|ai| ai.raw_slot_index == row) {
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
            .network_setup()
            .and_then(|launch| {
                launch
                    .opponents
                    .iter()
                    .find(|ai| ai.raw_slot_index == row)
                    .map(|ai| NetworkRowKind::Ai(ai.difficulty))
            })
            .or_else(|| {
                self.network_vacancy(row).map(|vacancy| match vacancy {
                    NetworkVacancy::Open => NetworkRowKind::Open,
                    NetworkVacancy::Closed => NetworkRowKind::Closed,
                    NetworkVacancy::OpenObserver => NetworkRowKind::OpenObserver,
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
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("network setup is not editable");
        };
        anyhow::ensure!(
            row < 8 && launch.local_player().raw_slot_index != Some(row),
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
        }
        launch.ai_player_count = launch.opponents.len() as i32;
        launch.rebuild_house_roster();
        Ok(removed)
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
            anyhow::ensure!(host && row < 8, "only the host may edit AI/vacant choices");
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
            || previous.digest != metadata.digest)
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
        network.map_metadata = Some(metadata);
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
    pub(crate) fn accept_network_host_setup(
        &mut self,
        mut incoming: SkirmishLaunchSession,
        incoming_vacancies: [NetworkVacancy; 8],
        capacity: i32,
        observer_row: Option<u8>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.host,
            "a host cannot receive guest-authored host setup"
        );
        anyhow::ensure!(capacity > 0, "host map capacity must be positive");
        anyhow::ensure!(
            observer_row.is_none_or(|row| row < 8),
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
        validate_network_rows(&incoming)?;
        incoming.opponents.sort_by_key(|ai| ai.raw_slot_index);
        incoming.rebuild_house_roster();
        *launch = incoming;
        *vacancies = incoming_vacancies;
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
        Some(row_limit(
            network.return_context.session_mode(),
            network.room,
        ))
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

    /// Accepted BC ChooseMap5DE520: atomic map/mode selection followed by
    /// capacity5EC3A0, starts5EE6A0 and all-row team defaults5EE510(-1).
    /// Unbound human nodes are returned for service Kick intents. Their source
    /// membership survives until the separate type10 departure response.
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
        if new_capacity < old_capacity {
            for row in new_capacity.saturating_add(host_extra)..8 {
                let row = row as u8;
                if Some(row) != local_row && Some(row) != observer_row {
                    removed.extend(candidate.set_network_row_kind(row, NetworkRowKind::Open)?);
                }
            }
        } else if new_capacity > old_capacity {
            for row in old_capacity.saturating_add(host_extra)
                ..new_capacity.saturating_add(host_extra).min(8)
            {
                let row = row as u8;
                if Some(row) == local_row || Some(row) == observer_row {
                    continue;
                }
                let setup = candidate.network_setup().unwrap();
                if setup
                    .humans
                    .iter()
                    .any(|human| human.raw_slot_index == Some(row))
                {
                    continue;
                }
                let kind = setup
                    .opponents
                    .iter()
                    .find(|ai| ai.raw_slot_index == row)
                    .map_or(NetworkRowKind::Open, |ai| NetworkRowKind::Ai(ai.difficulty));
                candidate.set_network_row_kind(row, kind)?;
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
                .chain((0..8).filter(|row| {
                    candidate.network_vacancy(*row) == Some(NetworkVacancy::OpenObserver)
                }))
                .collect();
            for row in observer_rows {
                removed.extend(candidate.set_network_row_kind(row, NetworkRowKind::Open)?);
            }
        }
        let ordinary = candidate
            .network_setup()
            .unwrap()
            .mode
            .class
            .allows_observers();
        let reserved = observer_row.or(ordinary.then_some(7));
        if let Some(row) =
            reserved.filter(|row| Some(*row) != local_row && i32::from(*row) > new_capacity)
        {
            let is_observer = candidate
                .network_setup()
                .unwrap()
                .humans
                .iter()
                .any(|human| human.raw_slot_index == Some(row) && human.is_observer())
                || candidate.network_vacancy(row) == Some(NetworkVacancy::OpenObserver);
            if !is_observer {
                removed.extend(candidate.set_network_row_kind(row, NetworkRowKind::Closed)?);
            }
        }
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    inactive_choices,
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
            human.slot.team = human_team;
        }
        for ai in &mut launch.opponents {
            ai.start_position = crate::skirmish_launch::LaunchStartPosition::Auto;
            ai.team = ai_team;
        }
        for choices in inactive_choices {
            choices.start_position = crate::skirmish_launch::LaunchStartPosition::Auto;
            choices.team = human_team;
        }
        launch.rebuild_house_roster();
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
        const MAXIMUM_PLAYERS: i32 = 8;
        let raw = self
            .network_map_metadata()
            .map_or(8, |metadata| metadata.player_capacity);
        MAXIMUM_PLAYERS.min(raw)
    }

    /// `0x005E9510`: no Open player row among the first
    /// [`Self::network_slot_test_rows`] rows, unless the observer row is 0
    /// and the row past them is Open.
    pub(crate) fn network_no_open_player_row(&self) -> bool {
        if self.network_setup().is_none() {
            return false;
        }
        let limit = self.network_slot_test_rows();
        let open = |row: i32| {
            u8::try_from(row)
                .ok()
                .and_then(|row| self.network_vacancy(row))
                == Some(NetworkVacancy::Open)
        };
        if (0..limit).any(open) {
            return false;
        }
        !(self.network_observer_row() == Some(0) && limit < 8 && open(limit))
    }

    /// `0x005E9240`'s row for a new node: the first Open row for a player,
    /// the first open observer row for an observer, over all eight rows.
    pub(crate) fn network_free_row(&self, observer: bool) -> Option<u8> {
        let wanted = if observer {
            NetworkVacancy::OpenObserver
        } else {
            NetworkVacancy::Open
        };
        (0..8u8).find(|row| self.network_vacancy(*row) == Some(wanted))
    }

    /// `0x005E95E0`: the nodes, less an observer on the observer row, are
    /// more than [`Self::network_slot_test_rows`].
    pub(crate) fn network_over_capacity(&self) -> bool {
        let Some(launch) = self.network_setup() else {
            return false;
        };
        let limit = self.network_slot_test_rows();
        let observer_row = self.network_observer_row();
        let observer_present = launch
            .humans
            .iter()
            .any(|human| human.is_observer() && human.raw_slot_index == observer_row);
        launch.humans.len() as i32 - i32::from(observer_present) > limit
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
    /// FrameSendRate, the PreCalc values and the IPX retry timing it also
    /// writes belong to the transport, outside this change.
    pub(crate) fn prepare_wol_timing(
        &mut self,
        pings: [i32; 8],
        channel_owner: bool,
    ) -> anyhow::Result<()> {
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
        Ok(())
    }

    /// OnGameStart's `[0xB779C4]` and Start Game Now's clan lists for the
    /// accepted WOL start.
    pub(crate) fn set_wol_start(&mut self, terms: WolStartTerms) {
        if let Some(network) = self.network.as_mut() {
            network.wol_start = terms;
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
        let max_ahead_frames = network
            .max_ahead_frames
            .ok_or_else(|| anyhow::anyhow!("network start requires accepted timing"))?;
        let NetworkPhase::Setup { launch, .. } =
            std::mem::replace(&mut network.phase, NetworkPhase::Lobby)
        else {
            unreachable!()
        };
        let startup = Arc::new(crate::app::match_bootstrap::PreparedNetworkStartup::new(
            launch,
            agreed_seed,
            network.return_context.session_mode(),
            self.host,
            max_ahead_frames,
            std::mem::take(&mut network.wol_start),
        ));
        network.phase = NetworkPhase::Frozen {
            connected: (0..startup.launch_session().humans.len()).collect(),
            startup: Arc::clone(&startup),
        };
        Ok(crate::app::match_bootstrap::LoadingStartup::Network(
            startup,
        ))
    }

    /// The loader borrows the same frozen owner as the shell. It cannot adopt
    /// a second mutable setup or rewrite LAN/WOL/host identity during loading.
    pub(crate) fn assert_network_start(
        &self,
        startup: &Arc<crate::app::match_bootstrap::PreparedNetworkStartup>,
    ) {
        let Some(NetworkSession {
            phase: NetworkPhase::Frozen {
                startup: admitted, ..
            },
            ..
        }) = &self.network
        else {
            panic!("loading requires an admitted network start");
        };
        assert!(
            Arc::ptr_eq(admitted, startup),
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
            && matches!(network.phase, NetworkPhase::Frozen { .. })
        {
            network
                .loading_requests
                .push(NetworkLoadingRequest::Chat { recipients, text });
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

    /// The frozen launch's owner key of network participant `source_index`.
    pub(crate) fn network_source_owner(&self, source_index: usize) -> Option<String> {
        let launch = self.network_setup()?;
        crate::sim::scenario_bootstrap::normalized_launch_slots(launch)
            .into_iter()
            .find(|slot| slot.source_human_index == Some(source_index))
            .map(|slot| slot.owner_name)
    }

    /// The network participant whose House has owner key `owner`.
    pub(crate) fn network_owner_source(&self, owner: &str) -> Option<usize> {
        let launch = self.network_setup()?;
        crate::sim::scenario_bootstrap::normalized_launch_slots(launch)
            .into_iter()
            .find(|slot| slot.owner_name == owner)
            .and_then(|slot| slot.source_human_index)
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
            Some(NetworkPhase::Frozen { connected, .. }) => connected,
            _ => &[],
        }
    }

    /// Match exit or failed loading returns to the active service lobby while
    /// retaining its connection/return context. A later setup is a new admission.
    pub(crate) fn return_to_network_lobby(&mut self) -> Option<NetworkReturnContext> {
        let network = self.network.as_mut()?;
        network.phase = NetworkPhase::Lobby;
        network.loading_requests.clear();
        network.map_metadata = None;
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

    /// Store one multiplayer frame's checksum at `history[frame & 0xFF]`.
    ///
    /// On frame 0 `Queue_AI_Multiplayer` stores the value (`0x006476A5`), then
    /// takes `0x006476B1 -> 0x00647825` and zeroes all 256 slots
    /// (`0x00647879`), so frame 0's own slot ends at 0. The same reset also
    /// runs while `[0x00A8B8B8]` is set; that flag belongs to the network
    /// timing service of a later stage and VERA has no writer for it yet.
    pub(crate) fn record_frame_checksum(&mut self, frame: u32, checksum: u32) {
        self.checksum_history.record(frame, checksum);
        if frame == 0 {
            self.checksum_history = MultiplayerChecksumHistory::new();
        }
    }

    #[cfg(test)]
    pub(crate) fn checksum_history(&self) -> &MultiplayerChecksumHistory {
        &self.checksum_history
    }

    /// Whether this client owns the match setup: it reads the seed and fixes
    /// the participant list. Every offline session is its own host.
    pub(crate) fn host(&self) -> bool {
        self.host
    }
}

/// What a WOL start hands the match: OnGameStart's `[0xB779C4]` (the
/// channel's tournament value, 2 in a clan room) and Start Game Now's two
/// clan name lists `[0xB77A04]`/`[0xB76568]`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WolStartTerms {
    pub(crate) tournament: i32,
    pub(crate) squad_lists: [Vec<String>; 2],
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
    let mut capacity = capacity.min(
        mode.class
            .setup_player_limit()
            .map_or(8, |limit| limit as i32),
    );
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
        capacity.min(row_limit(session_mode, room))
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

fn validate_network_rows(launch: &SkirmishLaunchSession) -> anyhow::Result<()> {
    anyhow::ensure!(
        launch.local_player().raw_slot_index.is_some(),
        "local participant must have a visible setup row"
    );
    let mut occupied = [false; 8];
    for row in launch
        .humans
        .iter()
        .filter_map(|human| human.raw_slot_index)
        .chain(launch.opponents.iter().map(|ai| ai.raw_slot_index))
    {
        anyhow::ensure!(
            row < 8 && !occupied[usize::from(row)],
            "network rows must be distinct and below8"
        );
        occupied[usize::from(row)] = true;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MatchSession, NetworkRoom, SessionMode};

    fn network_fixture() -> crate::skirmish_launch::SkirmishLaunchSession {
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
    fn network_freeze_keeps_guest_identity_seed_and_return_context_through_loading() {
        use super::NetworkReturnContext;
        for (mode, context) in [
            (SessionMode::Lan, NetworkReturnContext::LanLobby),
            (SessionMode::Wol, NetworkReturnContext::Wol),
        ] {
            let mut session = MatchSession::default();
            session.enter_network(context);
            session
                .accept_network_setup(network_fixture(), false, Some(7))
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
                    .accept_network_setup(network_fixture(), true, Some(7))
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
            .accept_network_setup(network_fixture(), true, Some(7))
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
            session.accept_network_setup(launch, true, Some(7)).unwrap();
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
        guest.accept_network_setup(launch, false, Some(7)).unwrap();
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
        lan.accept_network_setup(launch.clone(), true, Some(7))
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
    fn kick_unbinds_row_but_departure_owns_source_roster_compaction() {
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::LanLobby);
        session
            .accept_network_setup(network_fixture(), true, Some(7))
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
    fn open_rows_and_over_capacity_follow_the_wol_slot_tests() {
        // 0x5E9510 and 0x5E95E0 scan min(8, the options' raw capacity).
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::Wol);
        let launch = network_fixture();
        session
            .accept_network_setup(launch.clone(), false, Some(7))
            .unwrap();
        // Before any options the raw capacity is 8: rows 2..7 are Open.
        assert!(!session.network_no_open_player_row());
        let options =
            |session: &mut MatchSession, vacancies: [super::NetworkVacancy; 8], capacity: i32| {
                session
                    .accept_network_host_setup(launch.clone(), vacancies, capacity, None)
                    .unwrap();
                let metadata = super::NetworkMapMetadata {
                    display_name: "Map".into(),
                    player_capacity: capacity,
                    official: true,
                    byte_length: 1,
                    digest: "D".into(),
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
        session.accept_network_setup(launch, true, Some(7)).unwrap();
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
            .accept_network_setup(launch.clone(), false, Some(7))
            .unwrap();
        let mut metadata = super::NetworkMapMetadata {
            display_name: "Advertised title".into(),
            player_capacity: 8,
            official: false,
            byte_length: 83573,
            digest: "CaseSensitive".into(),
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
            .accept_network_host_setup(launch.clone(), [super::NetworkVacancy::Open; 8], 2, Some(7))
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
    fn host_packet_preserves_admission_and_rejects_source_identity_changes_atomically() {
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::LanLobby);
        session
            .accept_network_setup(network_fixture(), false, Some(7))
            .unwrap();
        session.set_network_player_ready(0, true).unwrap();
        session.set_network_player_ready(1, true).unwrap();
        session.set_network_map_status(0, 2).unwrap();
        let mut incoming = session.network_setup().unwrap().clone();
        incoming.options.short_game = !incoming.options.short_game;
        incoming.humans[0].role = crate::skirmish_launch::LaunchHumanRole::Player { group: 2 };
        let vacancies = [super::NetworkVacancy::Closed; 8];
        session
            .accept_network_host_setup(incoming, vacancies, 4, Some(7))
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
                .accept_network_host_setup(reordered, vacancies, 4, Some(7))
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
            .accept_network_setup(network_fixture(), true, Some(7))
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
    fn frame_zero_resets_the_checksum_ring_and_launches_do_not() {
        let mut session = MatchSession::default();
        session.record_frame_checksum(0x1_05, 0xdead_beef);
        assert_eq!(session.checksum_history().get(0x05), 0xdead_beef);
        assert_eq!(session.checksum_history().get(0x2_05), 0xdead_beef);
        session.begin_offline(SessionMode::Skirmish, Some("Commander".to_owned()));
        assert_eq!(session.checksum_history().get(0x05), 0xdead_beef);

        session.record_frame_checksum(0, 0x1234_5678);
        assert_eq!(session.checksum_history().get(0), 0);
        assert_eq!(session.checksum_history().get(0x05), 0);
        session.record_frame_checksum(1, 0x1234_5678);
        assert_eq!(session.checksum_history().get(1), 0x1234_5678);
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
