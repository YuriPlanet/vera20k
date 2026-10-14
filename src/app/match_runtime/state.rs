//! Match owner (F12 `MatchState`): the aggregate for everything scoped to a
//! single running match — the authoritative `SimRuntime`, the app-side input,
//! presentation, audio, and diagnostics owners, plus the scenario identity and
//! pacing facts that accompany them.
//!
//! App-side only: nothing here is serialized, hashed, or read by the
//! deterministic simulation except through `SimRuntime` itself.

use crate::map::basic::BasicSection;

pub(crate) struct MatchState {
    pub(crate) startup: super::startup::MatchStartup,
    /// Live session owner: gamemd's `SessionClass` at `0x00A8B238` (mode,
    /// participants, local participant, host). Written by the launch path
    /// and the saved-game restore; read by the app loop only.
    pub(crate) session: super::session::MatchSession,
    pub(crate) sim_runtime: Option<crate::sim::runtime::SimRuntime>,
    /// Match input owner (F12): camera, zoom, cursor, keys, hotkeys.
    pub(crate) input: crate::app::input::state::MatchInputState,
    /// Match presentation owner (F12), part 1: per-match atlases + cursor.
    pub(crate) match_presentation: crate::app::presentation::state::MatchPresentationState,
    /// Per-match audio owner (F11): sound event queue + EVA latches; resets
    /// on every match install and on leaving a match for the shell.
    pub(crate) match_audio: crate::app::match_audio::MatchAudioState,
    /// App-owned diagnostic recording (F10) — never inside the simulation, so
    /// no load/install path can silently drop an unflushed segment.
    pub(crate) match_diagnostics: crate::app::match_diagnostics::MatchDiagnosticsState,
    pub(crate) map_basic: BasicSection,
    /// Exact source whose bytes produced the active parsed map.
    pub(crate) loaded_map_source: Option<crate::map::source::LoadedMapSource>,
    /// Deterministic digest of the parsed source map INI. `None` only for
    /// generated/fallback worlds without an authoritative source-map payload.
    pub(crate) loaded_map_hash: Option<u64>,
    /// App-owned wall-clock outcome-EVA drain. The deterministic accepted
    /// result and SavourDelay target live in serialized `HouseState`.
    pub(crate) scenario_outcome:
        Option<crate::app::match_runtime::scenario_exit::ScenarioOutcomeVoiceWait>,
    /// Active running-scenario audio teardown. While present the tactical
    /// frame remains visible but simulation is frozen; its destination is
    /// committed only after the retail fade/voice-wait sequence completes.
    pub(crate) scenario_exit: Option<crate::app::match_runtime::scenario_exit::ScenarioExitCascade>,
    /// Match elapsed wall time for the retail score screen. App-local and never
    /// serialized, hashed, or read by deterministic simulation.
    pub(crate) scenario_elapsed_clock: crate::app::match_runtime::frame_pacer::ScenarioElapsedClock,
    /// Config-sourced input delay — copied to each new Simulation instance at game start.
    pub(crate) configured_input_delay_ticks: u64,
    /// Explicit development HUD/command preference. Only consulted when the
    /// live scenario has no current House; ordinary matches derive their
    /// identity from serialized `Simulation.session.current_house`.
    pub(crate) local_owner_override: Option<String>,
    /// Seeded empty-map sandbox keeps full map visibility while still locking control.
    pub(crate) sandbox_full_visibility: bool,
    /// The debug pause (`J` hotkey / dev overlay): the simulation is frozen
    /// without an in-scenario modal. Every modal transition ends it.
    pub(crate) debug_pause: bool,
    /// Effective simulation ticks per second — controls game speed.
    /// Default follows retail/YR skirmish stored game speed 1.
    pub(crate) sim_speed_tps: u32,
}

impl MatchState {
    /// ScenarioPause684060 during ChronoScreen affects audio/elapsed time,
    /// while the original Logic53A747 effect clock continues advancing.
    pub(crate) fn presentation_paused(&self) -> bool {
        self.paused() || self.match_presentation.chrono_screen.scenario_paused()
    }
    /// True when the game is paused and the simulation frozen: an in-scenario
    /// modal is open, or the debug pause is on.
    pub(crate) fn paused(&self) -> bool {
        self.match_presentation.in_game_menu.is_open()
            || self.debug_pause
            || self
                .match_presentation
                .radar_anim
                .as_ref()
                .is_some_and(|radar| radar.movie_pause())
    }

    /// The live scenario's pinned player, including a successfully restored save.
    pub(crate) fn local_player_owner(&self) -> Option<&str> {
        self.sim_runtime
            .as_ref()
            .and_then(|runtime| scenario_local_owner(&runtime.simulation))
    }
}

/// Native current House A83D4C is saved67F802, loaded67F9F3 and swizzled67FA0E.
/// Do not retain a separate app owner across Simulation replacement or infer it
/// from selection/human flags. Save validation owns reference validity.
/// Evidence: docs/research/PHASE3_CURRENT_HOUSE_IDENTITY_GHIDRA_REPORT.md.
pub(crate) fn scenario_local_owner(simulation: &crate::sim::world::Simulation) -> Option<&str> {
    simulation
        .session
        .current_house
        .map(|owner| simulation.interner.resolve(owner))
}
