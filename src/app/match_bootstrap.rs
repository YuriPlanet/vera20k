//! App-owned match startup authority and pre-first-tick evidence.
//!
//! This module sits above `sim/`: it classifies explicit-start Battle
//! sessions, reads the ordinary Windows seed once, and observes an already
//! initialized `Simulation` without mutating it.

use crate::sim::campaign_carry_over::CampaignCarryOver;
use crate::sim::world::{Simulation, SimulationRngState};
use crate::skirmish_launch::{LaunchStartPosition, SKIRMISH_AI_SLOT_COUNT, SkirmishLaunchSession};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct MatchCorrelationId(u64);

impl MatchCorrelationId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchSeedSource {
    WindowsGetTickCount,
    Controlled,
    NonWindowsDevelopmentFallback,
    /// Seed supplied by the admitted host start, never sampled by a guest.
    AgreedNetworkHost,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MatchSeed {
    pub value: u32,
    pub source: MatchSeedSource,
    /// This describes only the seed source, never whole-startup or MapGen parity.
    pub seed_authority_certifying: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptedBattleSession {
    launch: SkirmishLaunchSession,
}

impl AcceptedBattleSession {
    pub fn launch_session(&self) -> &SkirmishLaunchSession {
        &self.launch
    }

    pub fn selected_map_file(&self) -> &str {
        self.launch
            .selected_map_file
            .as_deref()
            .expect("accepted sessions always own a selected map record")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnverifiedStartupReason {
    MissingMap,
    NonBattleMode,
    RandomLocalCountry,
    RandomAiCountry { index: usize },
    RandomLocalColor,
    RandomAiColor { index: usize },
    AutoLocalStart,
    AutoAiStart { index: usize },
    InvalidActiveAiCount { count: usize },
    IncompleteLedger,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StartupSessionClassification {
    AcceptedExplicitFixedBattle(Box<AcceptedBattleSession>),
    UnverifiedLegacy(UnverifiedStartupReason),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedMatchStartup {
    pub correlation: MatchCorrelationId,
    pub seed: MatchSeed,
    pub session: AcceptedBattleSession,
}

/// Immutable accepted network handoff, shared with the live session owner.
/// No UI or loading phase keeps a second mutable roster after this freeze.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedNetworkStartup {
    launch: SkirmishLaunchSession,
    pub(crate) seed: MatchSeed,
    mode: crate::app::match_runtime::session::SessionMode,
    host: bool,
    max_ahead_frames: i32,
}

impl PreparedNetworkStartup {
    pub(crate) fn new(
        launch: SkirmishLaunchSession,
        seed: u32,
        mode: crate::app::match_runtime::session::SessionMode,
        host: bool,
        max_ahead_frames: i32,
    ) -> Self {
        Self {
            launch,
            seed: MatchSeed {
                value: seed,
                source: MatchSeedSource::AgreedNetworkHost,
                seed_authority_certifying: false,
            },
            mode,
            host,
            max_ahead_frames,
        }
    }

    pub(crate) fn max_ahead_frames(&self) -> i32 {
        self.max_ahead_frames
    }
    pub fn launch_session(&self) -> &SkirmishLaunchSession {
        &self.launch
    }
    pub(crate) fn mode(&self) -> crate::app::match_runtime::session::SessionMode {
        self.mode
    }
    pub(crate) fn host(&self) -> bool {
        self.host
    }
}

/// Explicit settings supplied by an external campaign launcher.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignLaunchOptions {
    pub game_speed: i32,
    pub build_off_ally: bool,
    /// External client map root, independent of the retail asset directory.
    pub scenario_directory: Option<std::path::PathBuf>,
}

impl CampaignLaunchOptions {
    fn loose_scenario_file(&self, scenario: &str) -> Option<String> {
        let path = self.scenario_directory.as_ref()?.join(scenario);
        path.is_file().then(|| path.to_string_lossy().into_owned())
    }
}

/// StartScenario683B26..683B62 copies Scenario+125C and uppercases it via
/// 7DCFC4 before the exact-case MISSIONMD reads (original gamemd.exe).
/// Keep the supplied physical spelling for case-sensitive loose-file lookup.
fn prepare_campaign_scenario(
    scenario: &str,
    launch_options: Option<&CampaignLaunchOptions>,
) -> (String, Option<String>) {
    let logical_name = scenario.to_ascii_uppercase();
    let source = launch_options
        .and_then(|options| options.loose_scenario_file(scenario))
        .or_else(|| (logical_name != scenario).then(|| scenario.to_owned()));
    (logical_name, source)
}

/// Start_Scenario683AB0's immutable campaign-array selection, separate from
/// multiplayer slot staging. Full_Init686B6A uses Options.Difficulty literally.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCampaignStartup {
    campaign_index: usize,
    campaign: crate::rules::campaigns::CampaignDefinition,
    /// Scenario+0x125C, the map Start_Scenario reads. A new campaign copies
    /// the campaign's Scenario (`0x00683AF8`); a won mission passes the map
    /// MapSelect chose (`0x00685C4E`).
    scenario: String,
    /// Physical source spelling when it differs from the logical name, or
    /// the resolved external path. Metadata and continuation use `scenario`.
    scenario_file: Option<String>,
    /// Scenario+0x1254 for this start. A new campaign starts after
    /// BattleControlTerminated6865B0, whose reset matches the ctor's 1; a won
    /// mission increments it before Start_Scenario (`0x00685AAF`).
    mission_counter: i32,
    difficulty: i32,
    launch_options: Option<CampaignLaunchOptions>,
    /// The carry-over record `0x004C6210` writes back once Start_Scenario
    /// has read the map. A new campaign (Main__PrepareSession) never calls
    /// it; a won mission, the restart and the defeat retry all do.
    carry_over: Option<CampaignCarryOver>,
    /// Start_Scenario's DL argument (`0x00683ABC`), tested at `0x00683D9C`
    /// before the post-read `[Basic] Action` movie (Scenario+0x1444). A new
    /// campaign (`0x0052E70C`) and a won mission (`0x00685C45`) pass 1; the
    /// restart (`0x006864ED`) and the defeat retry (`0x00686287`) pass 0.
    /// The pre-read Intro/Brief movie does not depend on it.
    play_action_movie: bool,
    pub seed: MatchSeed,
}

impl PreparedCampaignStartup {
    pub fn campaign_index(&self) -> usize {
        self.campaign_index
    }

    pub fn campaign(&self) -> &crate::rules::campaigns::CampaignDefinition {
        &self.campaign
    }

    pub fn scenario(&self) -> &str {
        &self.scenario
    }

    pub fn selected_map_file(&self) -> &str {
        self.scenario_file.as_deref().unwrap_or(&self.scenario)
    }

    pub fn mission_counter(&self) -> i32 {
        self.mission_counter
    }

    pub fn difficulty(&self) -> i32 {
        self.difficulty
    }

    /// Only explicit external overrides replace the existing campaign defaults.
    pub fn game_options(&self) -> Option<crate::sim::game_options::GameOptions> {
        self.launch_options
            .as_ref()
            .map(|options| crate::sim::game_options::GameOptions {
                game_speed: options.game_speed,
                build_off_ally: options.build_off_ally,
                ..Default::default()
            })
    }

    pub fn carry_over(&self) -> Option<&CampaignCarryOver> {
        self.carry_over.as_ref()
    }

    pub fn plays_action_movie(&self) -> bool {
        self.play_action_movie
    }

    /// Start_Scenario for the same mission: the in-battle Restart
    /// (`0x006863E0`, call at `0x006864FC`) and the defeat retry (`0x00686296`)
    /// both pass Scenario+0x125C, DL 0 and the retained campaign index
    /// Scenario+0x34CC. Neither path touches the mission counter
    /// Scenario+0x1254 or saves the carry-over record, so the record
    /// `0x004C6210` writes back afterwards (`0x00686506`, `0x0068639A`) is the
    /// one this mission started from. Without a won mission the record is the
    /// one `0x004C6140` cleared at session preparation (`0x0052DF0E`): every
    /// global false and the player's difficulty row 1, which SetDifficulty
    /// puts on the player House whatever Options.Difficulty says. The
    /// computer rows and the session's Options.Difficulty are read again.
    pub fn restart(
        &self,
        difficulty: i32,
        game_speed: i32,
        clock: &mut impl MatchSeedClock,
    ) -> Self {
        Self {
            campaign_index: self.campaign_index,
            campaign: self.campaign.clone(),
            scenario: self.scenario.clone(),
            scenario_file: self.scenario_file.clone(),
            mission_counter: self.mission_counter,
            difficulty,
            launch_options: self
                .launch_options
                .as_ref()
                .map(|options| CampaignLaunchOptions {
                    game_speed,
                    ..options.clone()
                }),
            carry_over: Some(self.carry_over.unwrap_or(CampaignCarryOver::CLEARED)),
            play_action_movie: false,
            seed: read_match_seed(clock),
        }
    }

    /// The won mission's Start_Scenario (`0x00685C4E`): the same campaign
    /// index, the map MapSelect chose, the incremented mission counter and
    /// the record `0x004C6180` saved from the won mission. Options.Difficulty
    /// is read again for the session; the player House's row comes from the
    /// record.
    pub fn next_mission(
        &self,
        scenario: String,
        mission_counter: i32,
        difficulty: i32,
        game_speed: i32,
        carry_over: CampaignCarryOver,
        clock: &mut impl MatchSeedClock,
    ) -> Self {
        let (scenario, scenario_file) =
            prepare_campaign_scenario(&scenario, self.launch_options.as_ref());
        Self {
            campaign_index: self.campaign_index,
            campaign: self.campaign.clone(),
            scenario_file,
            scenario,
            mission_counter,
            difficulty,
            launch_options: self
                .launch_options
                .as_ref()
                .map(|options| CampaignLaunchOptions {
                    game_speed,
                    ..options.clone()
                }),
            carry_over: Some(carry_over),
            play_action_movie: true,
            seed: read_match_seed(clock),
        }
    }
}

pub fn prepare_campaign_startup(
    campaigns: &crate::rules::campaigns::CampaignRegistry,
    name: &str,
    difficulty: i32,
    scenario: Option<&str>,
    launch_options: Option<CampaignLaunchOptions>,
    clock: &mut impl MatchSeedClock,
) -> Option<PreparedCampaignStartup> {
    let campaign_index = campaigns.find_index(name)?;
    let campaign = campaigns.get(campaign_index)?.clone();
    let (scenario, scenario_file) = prepare_campaign_scenario(
        scenario.unwrap_or_else(|| campaign.scenario()),
        launch_options.as_ref(),
    );
    Some(PreparedCampaignStartup {
        campaign_index,
        // External mission selectors can enter a later authored scenario while
        // retaining the selected campaign's final movie and continuation identity.
        scenario_file,
        scenario,
        campaign,
        mission_counter: 1,
        difficulty,
        launch_options,
        carry_over: None,
        play_action_movie: true,
        seed: read_match_seed(clock),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoadingStartup {
    Network(std::sync::Arc<PreparedNetworkStartup>),
    Campaign(PreparedCampaignStartup),
    Accepted(PreparedMatchStartup),
    UnverifiedLegacy {
        session: SkirmishLaunchSession,
        seed: MatchSeed,
    },
    Generic {
        selected_map_file: String,
    },
}

impl LoadingStartup {
    pub fn selected_map_file(&self) -> &str {
        match self {
            Self::Network(startup) => startup
                .launch
                .selected_map_file
                .as_deref()
                .expect("accepted network map"),
            Self::Campaign(startup) => startup.selected_map_file(),
            Self::Accepted(startup) => startup.session.selected_map_file(),
            Self::UnverifiedLegacy { session, .. } => {
                session.selected_map_file.as_deref().unwrap_or("auto")
            }
            Self::Generic { selected_map_file } => selected_map_file,
        }
    }

    pub fn launch_session(&self) -> Option<&SkirmishLaunchSession> {
        match self {
            Self::Network(startup) => Some(startup.launch_session()),
            Self::Campaign(_) => None,
            Self::Accepted(startup) => Some(startup.session.launch_session()),
            Self::UnverifiedLegacy { session, .. } => Some(session),
            Self::Generic { .. } => None,
        }
    }

    pub fn accepted(&self) -> Option<&PreparedMatchStartup> {
        match self {
            Self::Accepted(startup) => Some(startup),
            Self::Network(_)
            | Self::Campaign(_)
            | Self::UnverifiedLegacy { .. }
            | Self::Generic { .. } => None,
        }
    }

    pub fn campaign(&self) -> Option<&PreparedCampaignStartup> {
        match self {
            Self::Campaign(startup) => Some(startup),
            _ => None,
        }
    }

    /// The `GameMode` the shell wrote before this launch: 0 for a campaign
    /// (`0x0052E175`), 5 for every Battle launch (`0x0052E10F`). A generic map
    /// load has no campaign either, matching the simulation's own
    /// `game_mode_nonzero = campaign.is_none()` classification.
    pub(crate) fn session_mode(&self) -> crate::app::match_runtime::session::SessionMode {
        use crate::app::match_runtime::session::SessionMode;
        match self {
            Self::Network(startup) => startup.mode(),
            Self::Campaign(_) => SessionMode::Campaign,
            Self::Accepted(_) | Self::UnverifiedLegacy { .. } | Self::Generic { .. } => {
                SessionMode::Skirmish
            }
        }
    }

    #[cfg(test)]
    pub fn seed_or_else(&self, unverified_fallback: impl FnOnce() -> u32) -> u32 {
        match self {
            Self::Network(startup) => startup.seed.value,
            Self::Campaign(startup) => startup.seed.value,
            Self::Accepted(startup) => startup.seed.value,
            Self::UnverifiedLegacy { seed, .. } => seed.value,
            Self::Generic { .. } => unverified_fallback(),
        }
    }
}

/// Classify without reading entropy or mutating the supplied UI/session value.
pub fn classify_startup_session(session: &SkirmishLaunchSession) -> StartupSessionClassification {
    let Some(selected_map) = session.selected_map_file.as_deref() else {
        return StartupSessionClassification::UnverifiedLegacy(UnverifiedStartupReason::MissingMap);
    };
    let trimmed_map = selected_map.trim();
    if trimmed_map.is_empty()
        || trimmed_map.len() != selected_map.len()
        || trimmed_map.eq_ignore_ascii_case("auto")
    {
        return StartupSessionClassification::UnverifiedLegacy(UnverifiedStartupReason::MissingMap);
    }

    let mode = &session.mode;
    let is_stock_battle = mode.id == 1
        && mode.ui_name_key.eq_ignore_ascii_case("GUI:Battle")
        && mode.tooltip_key.eq_ignore_ascii_case("STT:ModeBattle")
        && mode.override_file.eq_ignore_ascii_case("MPBattleMD.ini")
        && mode.map_filter.eq_ignore_ascii_case("standard")
        && mode.random_maps_allowed
        && mode.allies_allowed
        && !mode.must_ally;
    if !is_stock_battle {
        return StartupSessionClassification::UnverifiedLegacy(
            UnverifiedStartupReason::NonBattleMode,
        );
    }
    if session.opponents.len() > SKIRMISH_AI_SLOT_COUNT {
        return StartupSessionClassification::UnverifiedLegacy(
            UnverifiedStartupReason::InvalidActiveAiCount {
                count: session.opponents.len(),
            },
        );
    }
    if session.humans.len() != 1
        || session.local_player_index != 0
        || session.humans[0].is_observer()
    {
        return StartupSessionClassification::UnverifiedLegacy(
            UnverifiedStartupReason::IncompleteLedger,
        );
    }
    if session.local_player().name.is_empty() {
        return StartupSessionClassification::UnverifiedLegacy(
            UnverifiedStartupReason::IncompleteLedger,
        );
    }
    if session.local_slot().country_random {
        return StartupSessionClassification::UnverifiedLegacy(
            UnverifiedStartupReason::RandomLocalCountry,
        );
    }
    if session.local_slot().color_random {
        return StartupSessionClassification::UnverifiedLegacy(
            UnverifiedStartupReason::RandomLocalColor,
        );
    }
    if matches!(
        session.local_slot().start_position,
        LaunchStartPosition::Auto
    ) {
        return StartupSessionClassification::UnverifiedLegacy(
            UnverifiedStartupReason::AutoLocalStart,
        );
    }
    for (index, slot) in session.opponents.iter().enumerate() {
        if slot.country_random {
            return StartupSessionClassification::UnverifiedLegacy(
                UnverifiedStartupReason::RandomAiCountry { index },
            );
        }
        if slot.color_random {
            return StartupSessionClassification::UnverifiedLegacy(
                UnverifiedStartupReason::RandomAiColor { index },
            );
        }
        if matches!(slot.start_position, LaunchStartPosition::Auto) {
            return StartupSessionClassification::UnverifiedLegacy(
                UnverifiedStartupReason::AutoAiStart { index },
            );
        }
    }

    StartupSessionClassification::AcceptedExplicitFixedBattle(Box::new(AcceptedBattleSession {
        launch: session.clone(),
    }))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MatchCorrelationError {
    #[error("match correlation space is exhausted")]
    Exhausted,
}

/// Allocate a process-lifetime identity. Zero and wrapping reuse are forbidden.
pub fn allocate_match_correlation(
    next: &mut u64,
) -> Result<MatchCorrelationId, MatchCorrelationError> {
    if *next == 0 || *next == u64::MAX {
        return Err(MatchCorrelationError::Exhausted);
    }
    let correlation = MatchCorrelationId(*next);
    *next += 1;
    Ok(correlation)
}

pub trait MatchSeedClock {
    fn low_u32(&mut self) -> u32;
    fn source(&self) -> MatchSeedSource;
    fn seed_authority_certifying(&self) -> bool;
}

#[derive(Default)]
pub struct OrdinaryMatchSeedClock;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetTickCount() -> u32;
}

impl MatchSeedClock for OrdinaryMatchSeedClock {
    fn low_u32(&mut self) -> u32 {
        #[cfg(windows)]
        {
            // SAFETY: GetTickCount takes no arguments and returns the native
            // elapsed-millisecond authority directly as an unsigned 32-bit word.
            unsafe { GetTickCount() }
        }
        #[cfg(not(windows))]
        {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default();
            now.subsec_nanos() ^ (now.as_secs() as u32).rotate_left(16)
        }
    }

    fn source(&self) -> MatchSeedSource {
        #[cfg(windows)]
        {
            MatchSeedSource::WindowsGetTickCount
        }
        #[cfg(not(windows))]
        {
            MatchSeedSource::NonWindowsDevelopmentFallback
        }
    }

    fn seed_authority_certifying(&self) -> bool {
        cfg!(windows)
    }
}

pub fn prepare_match_startup(
    correlation: MatchCorrelationId,
    session: AcceptedBattleSession,
    clock: &mut impl MatchSeedClock,
) -> PreparedMatchStartup {
    PreparedMatchStartup {
        correlation,
        seed: read_match_seed(clock),
        session,
    }
}

/// Read one ordinary offline match seed without coupling the caller to the
/// accepted-startup receipt path. Every successful Skirmish Start uses this
/// same authority, including sessions that still have noncertifying mechanics.
pub fn read_match_seed(clock: &mut impl MatchSeedClock) -> MatchSeed {
    MatchSeed {
        value: clock.low_u32(),
        source: clock.source(),
        seed_authority_certifying: clock.seed_authority_certifying(),
    }
}

pub struct RustL0Observation<'a> {
    pub startup: &'a PreparedMatchStartup,
    pub simulation: &'a Simulation,
    pub active_correlation: MatchCorrelationId,
    pub prior_receipt: Option<&'a RustL0Receipt>,
    pub screen_is_loading: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustL0Receipt {
    pub correlation: MatchCorrelationId,
    pub seed: u32,
    pub seed_source: MatchSeedSource,
    pub seed_authority_certifying: bool,
    pub session: AcceptedBattleSession,
    pub tick: u64,
    pub total_sim_ms: u64,
    pub binary_frame: u32,
    pub rngs: SimulationRngState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RustL0Error {
    #[error("a Rust L0 receipt already exists")]
    DuplicateReceipt,
    #[error("loaded startup correlation does not match the active attempt")]
    CorrelationMismatch,
    #[error("installed simulation seed does not match the prepared startup")]
    SeedMismatch,
    #[error("simulation tick is not zero")]
    NonzeroTick,
    #[error("simulation elapsed time is not zero")]
    NonzeroSimulationTime,
    #[error("simulation binary frame is not zero")]
    NonzeroBinaryFrame,
    #[error("the acknowledgement was not observed on the Loading screen")]
    ScreenNotLoading,
}

impl RustL0Observation<'_> {
    pub fn acknowledge(&self) -> Result<RustL0Receipt, RustL0Error> {
        if self.prior_receipt.is_some() {
            return Err(RustL0Error::DuplicateReceipt);
        }
        if self.startup.correlation != self.active_correlation {
            return Err(RustL0Error::CorrelationMismatch);
        }
        if self.simulation.session().seed() != u64::from(self.startup.seed.value) {
            return Err(RustL0Error::SeedMismatch);
        }
        if self.simulation.session().tick() != 0 {
            return Err(RustL0Error::NonzeroTick);
        }
        if self.simulation.session().total_sim_ms() != 0 {
            return Err(RustL0Error::NonzeroSimulationTime);
        }
        if self.simulation.session().binary_frame() != 0 {
            return Err(RustL0Error::NonzeroBinaryFrame);
        }
        if !self.screen_is_loading {
            return Err(RustL0Error::ScreenNotLoading);
        }

        Ok(RustL0Receipt {
            correlation: self.startup.correlation,
            seed: self.startup.seed.value,
            seed_source: self.startup.seed.source,
            seed_authority_certifying: self.startup.seed.seed_authority_certifying,
            session: self.startup.session.clone(),
            tick: self.simulation.session().tick(),
            total_sim_ms: self.simulation.session().total_sim_ms(),
            binary_frame: self.simulation.session().binary_frame(),
            rngs: self.simulation.rng_state(),
        })
    }
}

/// Admission check at the existing app-to-simulation tick boundary.
pub fn accepted_tick_is_admitted(
    startup: Option<&PreparedMatchStartup>,
    receipt: Option<&RustL0Receipt>,
) -> bool {
    let Some(startup) = startup else {
        return true;
    };
    receipt.is_some_and(|receipt| {
        receipt.correlation == startup.correlation
            && receipt.seed == startup.seed.value
            && receipt.seed_source == startup.seed.source
            && receipt.seed_authority_certifying == startup.seed.seed_authority_certifying
            && receipt.session == startup.session
            && receipt.tick == 0
            && receipt.total_sim_ms == 0
            && receipt.binary_frame == 0
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skirmish_launch::{
        AiDifficulty, LaunchCountry, LaunchTeam, SkirmishAiSlot, SkirmishLaunchMode,
        SkirmishLaunchOptions, SkirmishLocalSlot,
    };

    fn explicit_session() -> SkirmishLaunchSession {
        SkirmishLaunchSession {
            mode: SkirmishLaunchMode {
                class: crate::skirmish_modes::MpModeClass::Battle,
                id: 1,
                ui_name_key: "GUI:Battle".into(),
                tooltip_key: "STT:ModeBattle".into(),
                override_file: "MPBattleMD.ini".into(),
                map_filter: "standard".into(),
                random_maps_allowed: true,
                allies_allowed: true,
                must_ally: false,
            },
            selected_map_file: Some("DeepFrze.map".into()),
            humans: vec![crate::skirmish_launch::SkirmishHumanSlot {
                raw_slot_index: Some(0),
                name: "Player".into(),
                slot: SkirmishLocalSlot {
                    country: LaunchCountry::America,
                    country_random: false,
                    color_index: 0,
                    color_random: false,
                    start_position: LaunchStartPosition::Position(0),
                    team: LaunchTeam::None,
                },
                role: crate::skirmish_launch::LaunchHumanRole::from_observer(false),
            }],
            local_player_index: 0,
            ai_player_count: 1,
            opponents: vec![SkirmishAiSlot {
                raw_slot_index: 1,
                country: LaunchCountry::Russia,
                country_random: false,
                color_index: 1,
                color_random: false,
                start_position: LaunchStartPosition::Position(1),
                team: LaunchTeam::None,
                difficulty: AiDifficulty::Easy,
            }],
            pre_fill_house_roster:
                crate::skirmish_launch::PreFillHouseRoster::from_compact_skirmish(1),
            options: SkirmishLaunchOptions::default(),
        }
    }

    fn accepted_session() -> AcceptedBattleSession {
        match classify_startup_session(&explicit_session()) {
            StartupSessionClassification::AcceptedExplicitFixedBattle(session) => *session,
            other => panic!("explicit fixture was not accepted: {other:?}"),
        }
    }

    struct CountingClock {
        value: u32,
        reads: usize,
    }

    impl MatchSeedClock for CountingClock {
        fn low_u32(&mut self) -> u32 {
            self.reads += 1;
            self.value
        }

        fn source(&self) -> MatchSeedSource {
            MatchSeedSource::Controlled
        }

        fn seed_authority_certifying(&self) -> bool {
            true
        }
    }

    fn prepared(seed: u32) -> PreparedMatchStartup {
        let mut next = 1;
        let correlation = allocate_match_correlation(&mut next).unwrap();
        let mut clock = CountingClock {
            value: seed,
            reads: 0,
        };
        prepare_match_startup(correlation, accepted_session(), &mut clock)
    }

    #[test]
    fn cncnet_campaign_enters_a_selected_mission_and_keeps_continuation_identity() {
        let ini = crate::rules::ini_parser::IniFile::from_str(
            "[Battles]\n0=ALL1\n[ALL1]\nCD=2\nScenario=ALL01UMD.MAP\n",
        );
        let mut campaigns = crate::rules::campaigns::CampaignRegistry::default();
        campaigns.apply_ini(&ini, &crate::rules::movies::MovieRegistry::default());
        let mut clock = CountingClock {
            value: 731,
            reads: 0,
        };
        let startup = prepare_campaign_startup(
            &campaigns,
            "ALL1",
            2,
            Some("ALL04uMd.map"),
            Some(CampaignLaunchOptions {
                game_speed: 2,
                build_off_ally: false,
                scenario_directory: None,
            }),
            &mut clock,
        )
        .expect("campaign admission");
        assert_eq!(startup.scenario(), "ALL04UMD.MAP");
        assert_eq!(startup.selected_map_file(), "ALL04uMd.map");
        assert_eq!(startup.campaign().scenario(), "ALL01UMD.MAP");
        assert_eq!(startup.campaign().id(), "ALL1");
        assert_eq!(startup.campaign().cd(), 2);
        assert_eq!(startup.difficulty(), 2);
        assert_eq!(startup.game_options().unwrap().game_speed, 2);
        assert!(!startup.game_options().unwrap().build_off_ally);
        assert_eq!(startup.seed.value, 731);
        assert_eq!(clock.reads, 1);
        let next = startup.next_mission(
            "ALL05uMd.map".into(),
            2,
            2,
            4,
            CampaignCarryOver::CLEARED,
            &mut clock,
        );
        assert_eq!(next.campaign_index(), startup.campaign_index());
        assert_eq!(next.campaign(), startup.campaign());
        assert_eq!(next.scenario(), "ALL05UMD.MAP");
        assert_eq!(next.selected_map_file(), "ALL05uMd.map");
        assert_eq!(next.game_options().unwrap().game_speed, 4);
        assert!(!next.game_options().unwrap().build_off_ally);
        assert!(startup.plays_action_movie() && next.plays_action_movie());
    }

    /// Restart and the defeat retry re-enter Start_Scenario with the same
    /// FileName and campaign index, DL 0 and the record this mission
    /// started from; only the seed and Options.Difficulty are read again.
    #[test]
    fn restart_reenters_the_same_mission_without_the_action_movie() {
        let ini = crate::rules::ini_parser::IniFile::from_str(
            "[Battles]\n0=ALL1\n1=SOV1\n[ALL1]\nScenario=ALL01UMD.MAP\n[SOV1]\nScenario=SOV01UMD.MAP\n",
        );
        let mut campaigns = crate::rules::campaigns::CampaignRegistry::default();
        campaigns.apply_ini(&ini, &crate::rules::movies::MovieRegistry::default());
        let mut clock = CountingClock { value: 5, reads: 0 };
        let first = prepare_campaign_startup(&campaigns, "SOV1", 0, None, None, &mut clock)
            .expect("campaign admission");
        let mut won = crate::sim::world::Simulation::new();
        won.set_trigger_variable_for_test(true, 3, true);
        let record = won.save_campaign_carry_over();
        let second = first.next_mission("SOV02SMD.MAP".into(), 2, 0, 1, record, &mut clock);
        clock.value = 77;
        let restarted = second.restart(2, 1, &mut clock);
        assert_eq!(restarted.campaign_index(), second.campaign_index());
        assert_eq!(restarted.campaign(), second.campaign());
        assert_eq!(restarted.scenario(), "SOV02SMD.MAP");
        assert_eq!(restarted.selected_map_file(), second.selected_map_file());
        assert_eq!(restarted.mission_counter(), 2);
        assert_eq!(restarted.carry_over(), Some(&record));
        assert!(restarted.carry_over().unwrap().globals()[3]);
        assert_eq!(restarted.difficulty(), 2);
        assert_eq!(restarted.seed.value, 77);
        assert!(!restarted.plays_action_movie());
        // A first mission starts without the record but restarts with the
        // one `0x004C6140` cleared, player row 1 included.
        assert_eq!(first.carry_over(), None);
        assert_eq!(
            first.restart(0, 1, &mut clock).carry_over(),
            Some(&CampaignCarryOver::CLEARED)
        );
    }

    fn acknowledge<'a>(
        startup: &'a PreparedMatchStartup,
        simulation: &'a Simulation,
    ) -> Result<RustL0Receipt, RustL0Error> {
        RustL0Observation {
            startup,
            simulation,
            active_correlation: startup.correlation,
            prior_receipt: None,
            screen_is_loading: true,
        }
        .acknowledge()
    }

    #[test]
    fn unresolved_choices_are_unverified() {
        let mut automatic_map = explicit_session();
        automatic_map.selected_map_file = Some("auto".into());
        assert_eq!(
            classify_startup_session(&automatic_map),
            StartupSessionClassification::UnverifiedLegacy(UnverifiedStartupReason::MissingMap)
        );

        let mut session = explicit_session();
        session.local_slot_mut().country_random = true;
        assert_eq!(
            classify_startup_session(&session),
            StartupSessionClassification::UnverifiedLegacy(
                UnverifiedStartupReason::RandomLocalCountry
            )
        );
        session.local_slot_mut().country_random = false;
        session.opponents[0].start_position = LaunchStartPosition::Auto;
        assert_eq!(
            classify_startup_session(&session),
            StartupSessionClassification::UnverifiedLegacy(UnverifiedStartupReason::AutoAiStart {
                index: 0
            })
        );
    }

    #[test]
    fn accepted_loading_seed_preserves_u32_boundaries_through_descriptor_and_simulation() {
        for value in [0, 1, 0x7fff_ffff, u32::MAX] {
            let loading = LoadingStartup::Accepted(prepared(value));
            let mut fallback_calls = 0;
            let descriptor = crate::sim::scenario_session::ScenarioDescriptor {
                seed: loading.seed_or_else(|| {
                    fallback_calls += 1;
                    0xDEAD_BEEF
                }),
                ..Default::default()
            };
            let simulation = Simulation::from_descriptor(&descriptor);
            let expected_rng = crate::sim::rng::SimRng::new(u64::from(value)).logical_state();
            let expected_mapgen = crate::sim::rng::SimRng::new(0).logical_state();

            assert_eq!(fallback_calls, 0);
            assert_eq!(descriptor.seed, value);
            assert_eq!(simulation.session().seed(), u64::from(value));
            assert_eq!(simulation.rng_state().scenario, expected_rng);
            assert_eq!(simulation.rng_state().main, expected_rng);
            assert_eq!(simulation.rng_state().mapgen, expected_mapgen);
        }
    }

    #[test]
    fn rust_l0_receipt_captures_each_rng_stream_exactly() {
        let startup = prepared(7);
        let mut simulation = Simulation::with_seed(7);
        simulation.scenario_rng_mut_for_test().next_u32();
        simulation.main_rng().next_u32();
        simulation.mapgen_rng_mut_for_test().next_u32();
        let expected = simulation.rng_state();
        let receipt = RustL0Observation {
            startup: &startup,
            simulation: &simulation,
            active_correlation: startup.correlation,
            prior_receipt: None,
            screen_is_loading: true,
        }
        .acknowledge()
        .unwrap();
        assert_eq!(receipt.rngs, expected);
        assert!(!accepted_tick_is_admitted(Some(&startup), None));
        assert!(accepted_tick_is_admitted(Some(&startup), Some(&receipt)));
    }

    #[test]
    fn rust_l0_acknowledges_once_before_in_game() {
        let startup = prepared(11);
        let simulation = Simulation::with_seed(11);
        let receipt = acknowledge(&startup, &simulation).unwrap();
        assert_eq!(receipt.tick, 0);
        assert_eq!(receipt.total_sim_ms, 0);
        assert_eq!(receipt.binary_frame, 0);
        assert_eq!(
            RustL0Observation {
                startup: &startup,
                simulation: &simulation,
                active_correlation: startup.correlation,
                prior_receipt: Some(&receipt),
                screen_is_loading: true,
            }
            .acknowledge(),
            Err(RustL0Error::DuplicateReceipt)
        );
    }

    #[test]
    fn rust_l0_rejects_seed_or_active_correlation_mismatch() {
        let startup = prepared(16);
        let wrong_seed_simulation = Simulation::with_seed(17);
        assert_eq!(
            acknowledge(&startup, &wrong_seed_simulation),
            Err(RustL0Error::SeedMismatch)
        );

        let simulation = Simulation::with_seed(16);
        let mut next = 2;
        let other = allocate_match_correlation(&mut next).unwrap();
        assert_eq!(
            RustL0Observation {
                startup: &startup,
                simulation: &simulation,
                active_correlation: other,
                prior_receipt: None,
                screen_is_loading: true,
            }
            .acknowledge(),
            Err(RustL0Error::CorrelationMismatch)
        );
    }

    #[test]
    fn accepted_tick_rejects_each_mismatched_receipt_field_and_nonzero_clock() {
        let startup = prepared(20);
        let simulation = Simulation::with_seed(20);
        let receipt = acknowledge(&startup, &simulation).unwrap();
        assert!(accepted_tick_is_admitted(Some(&startup), Some(&receipt)));

        let mut rejected = Vec::new();

        let mut candidate = receipt.clone();
        candidate.correlation = MatchCorrelationId(candidate.correlation.get() + 1);
        rejected.push(("correlation", candidate));

        let mut candidate = receipt.clone();
        candidate.seed ^= 1;
        rejected.push(("seed", candidate));

        let mut candidate = receipt.clone();
        candidate.seed_source = MatchSeedSource::WindowsGetTickCount;
        rejected.push(("seed source", candidate));

        let mut candidate = receipt.clone();
        candidate.seed_authority_certifying = false;
        rejected.push(("seed authority", candidate));

        let mut candidate = receipt.clone();
        candidate
            .session
            .launch
            .local_player_mut()
            .name
            .push_str("-other");
        rejected.push(("session", candidate));

        let mut candidate = receipt.clone();
        candidate.tick = 1;
        rejected.push(("tick", candidate));

        let mut candidate = receipt.clone();
        candidate.total_sim_ms = 1;
        rejected.push(("total simulation time", candidate));

        let mut candidate = receipt.clone();
        candidate.binary_frame = 1;
        rejected.push(("binary frame", candidate));

        for (field, candidate) in rejected {
            assert!(
                !accepted_tick_is_admitted(Some(&startup), Some(&candidate)),
                "receipt mutation must be rejected: {field}"
            );
        }
        assert!(accepted_tick_is_admitted(None, None));
    }
}
