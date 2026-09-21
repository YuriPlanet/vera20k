//! App-owned match startup authority and pre-first-tick evidence.
//!
//! This module sits above `sim/`: it classifies explicit-start Battle
//! sessions, reads the ordinary Windows seed once, and observes an already
//! initialized `Simulation` without mutating it.

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
    AcceptedExplicitFixedBattle(AcceptedBattleSession),
    UnverifiedLegacy(UnverifiedStartupReason),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedMatchStartup {
    pub correlation: MatchCorrelationId,
    pub seed: MatchSeed,
    pub session: AcceptedBattleSession,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoadingStartup {
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
            Self::Accepted(startup) => startup.session.selected_map_file(),
            Self::UnverifiedLegacy { session, .. } => {
                session.selected_map_file.as_deref().unwrap_or("auto")
            }
            Self::Generic { selected_map_file } => selected_map_file,
        }
    }

    pub fn launch_session(&self) -> Option<&SkirmishLaunchSession> {
        match self {
            Self::Accepted(startup) => Some(startup.session.launch_session()),
            Self::UnverifiedLegacy { session, .. } => Some(session),
            Self::Generic { .. } => None,
        }
    }

    pub fn accepted(&self) -> Option<&PreparedMatchStartup> {
        match self {
            Self::Accepted(startup) => Some(startup),
            Self::UnverifiedLegacy { .. } | Self::Generic { .. } => None,
        }
    }

    #[cfg(test)]
    pub fn seed_or_else(&self, unverified_fallback: impl FnOnce() -> u32) -> u32 {
        match self {
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
    if session.player_name.is_empty() {
        return StartupSessionClassification::UnverifiedLegacy(
            UnverifiedStartupReason::IncompleteLedger,
        );
    }
    if session.local.country_random {
        return StartupSessionClassification::UnverifiedLegacy(
            UnverifiedStartupReason::RandomLocalCountry,
        );
    }
    if session.local.color_random {
        return StartupSessionClassification::UnverifiedLegacy(
            UnverifiedStartupReason::RandomLocalColor,
        );
    }
    if matches!(session.local.start_position, LaunchStartPosition::Auto) {
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

    StartupSessionClassification::AcceptedExplicitFixedBattle(AcceptedBattleSession {
        launch: session.clone(),
    })
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
    pub spawn_pick_active: bool,
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
    #[error("SpawnPick cannot produce an accepted direct-start receipt")]
    SpawnPickActive,
}

impl RustL0Observation<'_> {
    pub fn acknowledge(&self) -> Result<RustL0Receipt, RustL0Error> {
        if self.prior_receipt.is_some() {
            return Err(RustL0Error::DuplicateReceipt);
        }
        if self.startup.correlation != self.active_correlation {
            return Err(RustL0Error::CorrelationMismatch);
        }
        if self.simulation.session.seed != u64::from(self.startup.seed.value) {
            return Err(RustL0Error::SeedMismatch);
        }
        if self.simulation.session.tick != 0 {
            return Err(RustL0Error::NonzeroTick);
        }
        if self.simulation.session.total_sim_ms != 0 {
            return Err(RustL0Error::NonzeroSimulationTime);
        }
        if self.simulation.session.binary_frame != 0 {
            return Err(RustL0Error::NonzeroBinaryFrame);
        }
        if !self.screen_is_loading {
            return Err(RustL0Error::ScreenNotLoading);
        }
        if self.spawn_pick_active {
            return Err(RustL0Error::SpawnPickActive);
        }

        Ok(RustL0Receipt {
            correlation: self.startup.correlation,
            seed: self.startup.seed.value,
            seed_source: self.startup.seed.source,
            seed_authority_certifying: self.startup.seed.seed_authority_certifying,
            session: self.startup.session.clone(),
            tick: self.simulation.session.tick,
            total_sim_ms: self.simulation.session.total_sim_ms,
            binary_frame: self.simulation.session.binary_frame,
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
            player_name: "Player".into(),
            local: SkirmishLocalSlot {
                country: LaunchCountry::America,
                country_random: false,
                color_index: 0,
                color_random: false,
                start_position: LaunchStartPosition::Position(0),
                team: LaunchTeam::None,
            },
            opponents: vec![SkirmishAiSlot {
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
            StartupSessionClassification::AcceptedExplicitFixedBattle(session) => session,
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

    fn assert_unverified(session: &SkirmishLaunchSession, expected: UnverifiedStartupReason) {
        assert_eq!(
            classify_startup_session(session),
            StartupSessionClassification::UnverifiedLegacy(expected)
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
            spawn_pick_active: false,
        }
        .acknowledge()
    }

    #[test]
    fn explicit_fixed_battle_session_is_accepted() {
        assert!(matches!(
            classify_startup_session(&explicit_session()),
            StartupSessionClassification::AcceptedExplicitFixedBattle(_)
        ));
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
        session.local.country_random = true;
        assert_eq!(
            classify_startup_session(&session),
            StartupSessionClassification::UnverifiedLegacy(
                UnverifiedStartupReason::RandomLocalCountry
            )
        );
        session.local.country_random = false;
        session.opponents[0].start_position = LaunchStartPosition::Auto;
        assert_eq!(
            classify_startup_session(&session),
            StartupSessionClassification::UnverifiedLegacy(UnverifiedStartupReason::AutoAiStart {
                index: 0
            })
        );
    }

    #[test]
    fn random_local_country_is_unverified() {
        let mut session = explicit_session();
        session.local.country_random = true;
        assert_unverified(&session, UnverifiedStartupReason::RandomLocalCountry);
    }

    #[test]
    fn random_ai_country_is_unverified() {
        let mut session = explicit_session();
        session.opponents[0].country_random = true;
        assert_unverified(
            &session,
            UnverifiedStartupReason::RandomAiCountry { index: 0 },
        );
    }

    #[test]
    fn random_local_color_is_unverified() {
        let mut session = explicit_session();
        session.local.color_random = true;
        assert_unverified(&session, UnverifiedStartupReason::RandomLocalColor);
    }

    #[test]
    fn random_ai_color_is_unverified() {
        let mut session = explicit_session();
        session.opponents[0].color_random = true;
        assert_unverified(
            &session,
            UnverifiedStartupReason::RandomAiColor { index: 0 },
        );
    }

    #[test]
    fn auto_local_start_is_unverified() {
        let mut session = explicit_session();
        session.local.start_position = LaunchStartPosition::Auto;
        assert_unverified(&session, UnverifiedStartupReason::AutoLocalStart);
    }

    #[test]
    fn auto_ai_start_is_unverified() {
        let mut session = explicit_session();
        session.opponents[0].start_position = LaunchStartPosition::Auto;
        assert_unverified(&session, UnverifiedStartupReason::AutoAiStart { index: 0 });
    }

    #[test]
    fn missing_map_is_unverified() {
        let mut session = explicit_session();
        session.selected_map_file = None;
        assert_unverified(&session, UnverifiedStartupReason::MissingMap);
    }

    #[test]
    fn non_battle_mode_is_unverified() {
        let mut session = explicit_session();
        session.mode.id = 2;
        assert_unverified(&session, UnverifiedStartupReason::NonBattleMode);
    }

    #[test]
    fn invalid_active_ai_count_is_unverified() {
        let mut session = explicit_session();
        let slot = session.opponents[0].clone();
        session.opponents = vec![slot; SKIRMISH_AI_SLOT_COUNT + 1];
        assert_unverified(
            &session,
            UnverifiedStartupReason::InvalidActiveAiCount {
                count: SKIRMISH_AI_SLOT_COUNT + 1,
            },
        );
    }

    #[test]
    fn team_none_is_concrete_and_accepted() {
        let session = explicit_session();
        assert_eq!(session.local.team, LaunchTeam::None);
        assert_eq!(session.opponents[0].team, LaunchTeam::None);
        assert!(matches!(
            classify_startup_session(&session),
            StartupSessionClassification::AcceptedExplicitFixedBattle(_)
        ));
    }

    #[test]
    fn accepted_startup_reads_seed_once_and_preserves_boundaries() {
        for value in [0, 1, 0x7fff_ffff, u32::MAX] {
            let mut next = 1;
            let correlation = allocate_match_correlation(&mut next).unwrap();
            let mut clock = CountingClock { value, reads: 0 };
            let startup = prepare_match_startup(correlation, accepted_session(), &mut clock);
            assert_eq!(clock.reads, 1);
            assert_eq!(startup.seed.value, value);
            assert_eq!(startup.seed.source, MatchSeedSource::Controlled);
        }
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
            assert_eq!(simulation.session.seed, u64::from(value));
            assert_eq!(simulation.rng_state().scenario, expected_rng);
            assert_eq!(simulation.rng_state().main, expected_rng);
            assert_eq!(simulation.rng_state().mapgen, expected_mapgen);
        }
    }

    #[test]
    fn shell_resolved_loading_uses_preselected_seed_while_generic_calls_fallback() {
        let legacy = LoadingStartup::UnverifiedLegacy {
            session: explicit_session(),
            seed: MatchSeed {
                value: 0x1234_5678,
                source: MatchSeedSource::Controlled,
                seed_authority_certifying: true,
            },
        };
        let mut legacy_calls = 0;
        assert_eq!(
            legacy.seed_or_else(|| {
                legacy_calls += 1;
                0x1234_5678
            }),
            0x1234_5678
        );
        assert_eq!(legacy_calls, 0);

        let generic = LoadingStartup::Generic {
            selected_map_file: "DeepFrze.map".into(),
        };
        let mut generic_calls = 0;
        assert_eq!(
            generic.seed_or_else(|| {
                generic_calls += 1;
                0x8765_4321
            }),
            0x8765_4321
        );
        assert_eq!(generic_calls, 1);
    }

    #[test]
    fn correlations_are_monotonic_across_consecutive_attempts() {
        let mut next = 1;
        let first = allocate_match_correlation(&mut next).unwrap();
        let second = allocate_match_correlation(&mut next).unwrap();
        assert_eq!(first.get(), 1);
        assert_eq!(second.get(), 2);
        assert_eq!(next, 3);
    }

    #[test]
    fn correlation_exhaustion_fails_without_reuse() {
        let mut next = u64::MAX;
        assert_eq!(
            allocate_match_correlation(&mut next),
            Err(MatchCorrelationError::Exhausted)
        );
        assert_eq!(next, u64::MAX);
    }

    #[test]
    fn correlation_exhaustion_fails_without_reading_seed() {
        let mut next = u64::MAX;
        let mut clock = CountingClock { value: 7, reads: 0 };
        assert_eq!(
            allocate_match_correlation(&mut next),
            Err(MatchCorrelationError::Exhausted)
        );
        assert_eq!(clock.reads, 0);
        // Keep the clock live so the assertion proves no hidden preparation call.
        assert_eq!(clock.low_u32(), 7);
        assert_eq!(clock.reads, 1);
    }

    #[test]
    fn rust_l0_receipt_captures_each_rng_stream_exactly() {
        let startup = prepared(7);
        let mut simulation = Simulation::with_seed(7);
        simulation.scatter_rng().next_u32();
        simulation.weapon_spread_rng().next_u32();
        simulation.mapgen_rng.next_u32();
        let expected = simulation.rng_state();
        let receipt = RustL0Observation {
            startup: &startup,
            simulation: &simulation,
            active_correlation: startup.correlation,
            prior_receipt: None,
            screen_is_loading: true,
            spawn_pick_active: false,
        }
        .acknowledge()
        .unwrap();
        assert_eq!(receipt.rngs, expected);
        assert!(!accepted_tick_is_admitted(Some(&startup), None));
        assert!(accepted_tick_is_admitted(Some(&startup), Some(&receipt)));
    }

    #[test]
    fn rust_l0_rejects_acknowledgement_after_leaving_loading_screen() {
        let startup = prepared(9);
        let simulation = Simulation::with_seed(9);
        assert_eq!(
            RustL0Observation {
                startup: &startup,
                simulation: &simulation,
                active_correlation: startup.correlation,
                prior_receipt: None,
                screen_is_loading: false,
                spawn_pick_active: false,
            }
            .acknowledge(),
            Err(RustL0Error::ScreenNotLoading)
        );
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
                spawn_pick_active: false,
            }
            .acknowledge(),
            Err(RustL0Error::DuplicateReceipt)
        );
    }

    #[test]
    fn rust_l0_rejects_nonzero_tick() {
        let startup = prepared(12);
        let mut simulation = Simulation::with_seed(12);
        simulation.session.tick = 1;
        assert_eq!(
            acknowledge(&startup, &simulation),
            Err(RustL0Error::NonzeroTick)
        );
    }

    #[test]
    fn rust_l0_rejects_nonzero_sim_time() {
        let startup = prepared(13);
        let mut simulation = Simulation::with_seed(13);
        simulation.session.total_sim_ms = 1;
        assert_eq!(
            acknowledge(&startup, &simulation),
            Err(RustL0Error::NonzeroSimulationTime)
        );
    }

    #[test]
    fn rust_l0_rejects_nonzero_binary_frame() {
        let startup = prepared(14);
        let mut simulation = Simulation::with_seed(14);
        simulation.session.binary_frame = 1;
        assert_eq!(
            acknowledge(&startup, &simulation),
            Err(RustL0Error::NonzeroBinaryFrame)
        );
    }

    #[test]
    fn rust_l0_rejects_spawn_pick() {
        let startup = prepared(15);
        let simulation = Simulation::with_seed(15);
        assert_eq!(
            RustL0Observation {
                startup: &startup,
                simulation: &simulation,
                active_correlation: startup.correlation,
                prior_receipt: None,
                screen_is_loading: true,
                spawn_pick_active: true,
            }
            .acknowledge(),
            Err(RustL0Error::SpawnPickActive)
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
                spawn_pick_active: false,
            }
            .acknowledge(),
            Err(RustL0Error::CorrelationMismatch)
        );
    }

    #[test]
    fn accepted_match_tick_is_gated_without_receipt() {
        let startup = prepared(18);
        assert!(!accepted_tick_is_admitted(Some(&startup), None));
        assert!(accepted_tick_is_admitted(None, None));
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
        candidate.session.launch.player_name.push_str("-other");
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

    #[test]
    fn second_match_cannot_reuse_first_receipt() {
        let first = prepared(19);
        let first_simulation = Simulation::with_seed(19);
        let receipt = acknowledge(&first, &first_simulation).unwrap();

        let mut next = 2;
        let second_correlation = allocate_match_correlation(&mut next).unwrap();
        let mut clock = CountingClock {
            value: 19,
            reads: 0,
        };
        let second = prepare_match_startup(second_correlation, accepted_session(), &mut clock);
        assert!(!accepted_tick_is_admitted(Some(&second), Some(&receipt)));
    }
}
