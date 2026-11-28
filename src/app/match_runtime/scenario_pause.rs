//! System-clock620 for the shared ScenarioPause684060/684180 owner.
//! Simulation's ScenarioSession owns62C/630 and the input-lock transitions.
//! This leaf supplies no gameplay clock, RNG or competing pause counter.
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_diagnostics::MatchDiagnosticsState;
use crate::app::match_runtime::sim_host::{SimHost, SimWrite};
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::{AppState, PlatformState};
use crate::sim::replay::{ReplayLog, ScenarioPauseInput, ScenarioServiceInput};
use crate::sim::scenario_session::ScenarioPauseChange;
use crate::sim::timer::CdTimer;
use crate::sim::world::Simulation;

#[derive(Debug, Default)]
pub(crate) struct ScenarioPauseClock {
    timer: CdTimer,
}

impl ScenarioPauseClock {
    /// PostRead683571 records the current bucket, even for duration0;
    /// preserve signed elapsed behavior across the low32 millisecond wrap.
    fn after_load(wall_ms: u64) -> Self {
        Self {
            timer: CdTimer::started(bucket(wall_ms), 0),
        }
    }

    fn rearm(&mut self, duration: i32, wall_ms: u64) {
        if duration != 0 {
            self.timer.start(bucket(wall_ms), duration.wrapping_mul(60));
        }
    }

    pub(super) fn elapsed(&self, wall_ms: u64) -> bool {
        self.timer.remaining(bucket(wall_ms)) <= 0
    }
}

const fn bucket(wall_ms: u64) -> i32 {
    ((wall_ms as u32) >> 4) as i32
}

/// Main_Tick55D7C2..55D877 checks Scenario62C only in offline modes0/5.
/// Share its admission with message Manage55DDA5; a radar flag alone misses
/// timed Action110 and other consumers of the same Scenario pause owner.
/// Source: original gamemd.exe instructions.
pub(crate) fn blocks_main_tick(mode: super::session::SessionMode, paused: bool) -> bool {
    paused
        && matches!(
            mode,
            super::session::SessionMode::Campaign | super::session::SessionMode::Skirmish
        )
}

pub(crate) fn enter(
    match_state: &mut crate::app::match_runtime::state::MatchState,
    audio: &mut crate::app::audio_runtime::AppAudioRuntime,
    duration: i32,
    outermost: bool,
    wall_ms: u64,
) {
    let mode = match_state.session_mode();
    enter_with_owners(
        &mut match_state.match_presentation.scenario_pause,
        &mut match_state.scenario_elapsed_clock,
        mode,
        audio.sfx_player.as_mut(),
        duration,
        outermost,
        wall_ms,
    );
}

/// ScenarioPause684060's physical effects also run at an in-frame Action110
/// boundary, before a following outcome cue can reach Vox. The same clocks
/// and SFX owner serve both callers. Source: original gamemd.exe instructions.
pub(crate) fn enter_with_owners(
    clock: &mut ScenarioPauseClock,
    elapsed: &mut super::frame_pacer::ScenarioElapsedClock,
    mode: super::session::SessionMode,
    sfx: Option<&mut crate::audio::sfx::SfxPlayer>,
    duration: i32,
    outermost: bool,
    wall_ms: u64,
) {
    clock.rearm(duration, wall_ms);
    if !outermost {
        return;
    }
    if matches!(
        mode,
        super::session::SessionMode::Campaign | super::session::SessionMode::Skirmish
    ) {
        elapsed.pause(wall_ms);
    }
    if let Some(sfx) = sfx {
        sfx.set_paused(true, wall_ms);
    }
}

pub(crate) fn resume(
    screen: &crate::ui::game_screen::GameScreen,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    audio: &mut crate::app::audio_runtime::AppAudioRuntime,
    wall_ms: u64,
) {
    let elapsed_paused = match_state.elapsed_clock_paused(screen);
    let audio_paused = match_state.presentation_paused(screen);
    resume_with_owners(
        &mut match_state.scenario_elapsed_clock,
        audio.sfx_player.as_mut(),
        elapsed_paused,
        audio_paused,
        wall_ms,
    );
}

/// ScenarioResume684180 releases this hold through the existing elapsed and
/// SFX owners; another local pause remains effective at either caller.
pub(crate) fn resume_with_owners(
    elapsed: &mut super::frame_pacer::ScenarioElapsedClock,
    sfx: Option<&mut crate::audio::sfx::SfxPlayer>,
    elapsed_paused: bool,
    audio_paused: bool,
    wall_ms: u64,
) {
    if !elapsed_paused {
        elapsed.resume(wall_ms);
    }
    if let Some(sfx) = sfx {
        sfx.set_paused(audio_paused, wall_ms);
    }
}

/// Main_Tick55D7C2..55D82E: only modes0/5 inspect620. Restored timers
/// begin at zero (PostRead683571..68358E); a saved timed hold releases on
/// the first live poll, while untimed holds remain with their existing owner.
pub(crate) fn poll(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
    wall_ms: u64,
) {
    if matches!(
        match_state.session_mode(),
        super::session::SessionMode::Campaign | super::session::SessionMode::Skirmish
    ) && match_state
        .match_presentation
        .scenario_pause
        .elapsed(wall_ms)
        && let Some(rt) = match_state.sim_runtime.runtime()
        && rt.simulation.session().scenario_pause_timed()
    {
        apply_pause_service(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            ScenarioPauseInput::ExpireTimedPause,
        );
    }
}

/// Ordered external pause ingress shared by timers, Radar and the narrative
/// barrier. A pause service changes only ScenarioPause684060/684180, so its
/// change goes to the handlers [`super::sim_tick::apply_trigger_effects`]
/// runs for the same effects, in the same order.
pub(crate) fn apply_pause_service(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
    input: ScenarioPauseInput,
) {
    let change = apply_live_service(
        &mut match_state.sim_runtime,
        &mut match_state.match_diagnostics,
        input,
        ScenarioPauseInput::apply,
        ReplayLog::apply_scenario_pause,
    );
    match change {
        ScenarioPauseChange::NoEffect => {}
        ScenarioPauseChange::Pause {
            duration,
            outermost,
            lock_input,
        } => {
            let now = super::frame_pacer::wall_clock_ms();
            enter(match_state, audio, duration, outermost, now);
            if lock_input {
                crate::app::input::campaign_input::apply_lock_effect(
                    platform,
                    renderer,
                    diag,
                    frontend,
                    match_state,
                    process_assets,
                    audio,
                    persistence,
                    true,
                    false,
                );
            }
        }
        ScenarioPauseChange::Resume { unlock_input } => {
            if let Some(was_locked) = unlock_input {
                crate::app::input::campaign_input::apply_lock_effect(
                    platform,
                    renderer,
                    diag,
                    frontend,
                    match_state,
                    process_assets,
                    audio,
                    persistence,
                    false,
                    was_locked,
                );
            }
            let now = super::frame_pacer::wall_clock_ms();
            resume(&frontend.screen, match_state, audio, now);
        }
    }
}

/// Main_Tick55D83A's visit to53B560 while the scenario is paused. It can
/// yield any trigger effect, so the general effect pass applies them.
pub(crate) fn apply_chrono_screen_step(state: &mut AppState) {
    let effects = apply_live_service(
        &mut state.match_state.sim_runtime,
        &mut state.match_state.match_diagnostics,
        ScenarioServiceInput::StepChronoScreen,
        ScenarioServiceInput::apply,
        ReplayLog::apply_scenario_service,
    );
    super::sim_tick::apply_trigger_effects(state, &effects);
}

/// Both paths' world write: `apply_recorded` when this match records a
/// replay, else `apply`. Record the transition itself, never a wall clock or
/// a second state owner.
fn apply_live_service<I, T: Default>(
    runtime: &mut SimHost,
    diagnostics: &mut MatchDiagnosticsState,
    input: I,
    apply: fn(I, &mut Simulation) -> T,
    apply_recorded: fn(&mut ReplayLog, &mut Simulation, I) -> T,
) -> T {
    // The radar movie FIFO still runs for presentation during playback.
    // Recorded services own the simulation's pause credits in that timeline.
    if diagnostics.is_replay_playback() {
        return T::default();
    }
    let Some(rt) = runtime.write_sim(SimWrite::Tick) else {
        return T::default();
    };
    match diagnostics.get_or_begin_replay(rt) {
        Some(log) => apply_recorded(log, &mut rt.simulation, input),
        None => apply(input, &mut rt.simulation),
    }
}

/// Retire only this world's physical clock. Saved62C/630 remain simulation
/// state; a failed load cannot release the outgoing world's authoritative hold.
pub(crate) fn load_clock(
    presentation: &mut crate::app::presentation::state::MatchPresentationState,
    wall_ms: u64,
) {
    presentation.scenario_pause = ScenarioPauseClock::after_load(wall_ms);
}

pub(crate) fn reset_clock(
    presentation: &mut crate::app::presentation::state::MatchPresentationState,
) {
    presentation.scenario_pause = ScenarioPauseClock::default();
    presentation.narrative_sync = Default::default();
}

#[cfg(all(test, tests_app_match))]
#[path = "scenario_pause_tests.rs"]
mod tests;
