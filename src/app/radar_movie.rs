//! Tactical communications FIFO (5BF390/5BF450) and the shared Bink adapter.
//! RadarPresentation owns mode/prelude and the requested pause; RadarMovies
//! retains its acquired ScenarioPause credit. Vox owns selected EVA completion;
//! AppAudioRuntime owns the separate movie stream hold.
//!
//! Native evidence: Steam3e81a617..08600,653963/6578F0/6579E0 and the executed
//! prerequisite/lifecycle controls in the ALL01 native evidence packet.
//! Queued pause transitions: original657BFA/657C7D controls in
//! tools/scenario_pause.json; the sidecar records the executable identity.
//! GPU/rodio, failed media decode and missing device are Rust substitutes;
//! full native copy/final-paint and saved raw Radar fields remain unproven.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::app::AppState;
use crate::app::movie_playback::MoviePlayback;
use crate::render::bink_movie::BinkMovieSurface;
use crate::render::radar::anim::RadarMovieCommand;
use crate::rules::sound_ini::{EvaPriority, EvaType};
use crate::sim::replay::ScenarioServiceInput;

const INCOMING: &str = "EVA_IncomingTransmission"; //652D6E literal8392C0

struct QueuedMovie {
    surface: Option<BinkMovieSurface>,
    pause_simulation: bool,
}

/// Match-scoped media resources. VecDeque preserves the native FIFO without
/// the old vector's capacity limit/growth policy affecting admission.
#[derive(Default)]
pub(crate) struct RadarMovies {
    queue: VecDeque<QueuedMovie>,
    playing: Option<MoviePlayback>,
    /// Credit for this FIFO's prelude suspend. Retirement may run after
    /// playback already released it, and must not release another caller.
    eva_suspended: bool,
    /// This FIFO's actual684060 credit, independent of other pause callers.
    scenario_hold: bool,
}
impl RadarMovies {
    pub(crate) const fn scenario_hold(&self) -> bool {
        self.scenario_hold
    }

    fn set_scenario_pause(&mut self, paused: bool) -> Option<ScenarioServiceInput> {
        //657C85..657CD4 compares the requested flag with the retained credit.
        if paused == self.scenario_hold {
            return None;
        }
        self.scenario_hold = paused;
        Some(if paused {
            ScenarioServiceInput::Pause
        } else {
            ScenarioServiceInput::Resume
        })
    }

    fn finish_playing(&mut self) -> (bool, Option<ScenarioServiceInput>) {
        self.playing = None;
        self.queue.pop_front();
        let has_next = !self.queue.is_empty();
        //657BFA..657C37 keeps the finishing clip's pause through a queued
        //transition. The next head draw decides whether that credit changes.
        let service = if has_next {
            None
        } else {
            self.set_scenario_pause(false)
        };
        (has_next, service)
    }
    pub(crate) fn set_active(&mut self, active: bool) {
        if let Some(player) = self.playing.as_mut() {
            player.set_active(active);
        }
    }

    fn release_eva_hold(&mut self, sfx: Option<&mut crate::audio::sfx::SfxPlayer>) {
        if std::mem::take(&mut self.eva_suspended)
            && let Some(sfx) = sfx
        {
            sfx.resume_eva();
        }
    }
}

/// Request5BF390: campaign mode0, fixed catalogue ordinal, unlock progress
/// before the asset check, source-preserving YR media resolution, then append.
pub(crate) fn enqueue(state: &mut AppState, index: i32, pause_simulation: bool) {
    if state
        .match_state
        .sim_runtime
        .as_ref()
        .is_none_or(|rt| rt.simulation.session().game_mode_nonzero())
    {
        return;
    }
    let Some(name) = state
        .process_assets
        .native_rules()
        .and_then(|rules| rules.movies().name(index))
        .map(str::to_owned)
    else {
        return;
    };
    let stem = crate::app::frontend::fullscreen_movie::movie_stem(&name);
    state
        .persistence
        .options_profile
        .movie_progress
        .note_played(stem);
    let file_name = format!("{stem}.BIK");
    let Some((bytes, source)) = state.process_assets.manager().and_then(|assets| {
        assets
            .get_with_source_ref(&file_name)
            .map(|(bytes, source)| (Arc::<[u8]>::from(bytes), source.to_string()))
    }) else {
        log::info!("Radar movie {file_name} is not installed");
        return;
    };
    match BinkMovieSurface::from_bytes(
        &state.renderer.gpu,
        &state.renderer.batch_renderer,
        bytes,
        source,
        false,
        wgpu::FilterMode::Nearest,
    ) {
        Ok(surface) => {
            log::info!(
                "Radar movie admitted: {file_name}, source={}, pause={pause_simulation}",
                surface.source_archive()
            );
            state
                .match_state
                .match_presentation
                .radar_movies
                .queue
                .push_back(QueuedMovie {
                    surface: Some(surface),
                    pause_simulation,
                });
        }
        Err(error) => log::warn!("Radar movie {file_name} decode unavailable: {error}"),
    }
}

/// Reconcile117's pause with an open modal. The existing clock is idempotent;
/// movie removal never rearms time while another offline pause still exists.
fn reconcile_pause_clock(state: &mut AppState, wall_ms: u64) {
    if state
        .match_state
        .presentation_paused(&state.frontend.screen)
    {
        state.match_state.scenario_elapsed_clock.pause(wall_ms);
    } else {
        state.match_state.scenario_elapsed_clock.resume(wall_ms);
    }
}

/// Service outside the simulation gate. Only actual sidebar draws advance the
/// housing timer; media and its pause must remain live when117 stops simulation.
pub(crate) fn service(state: &mut AppState, wall_ms: u64) {
    if state.frontend.screen != crate::ui::game_screen::GameScreen::InGame
        || state
            .match_state
            .match_presentation
            .radar_movies
            .queue
            .is_empty()
    {
        return;
    }
    let selected_index = state
        .process_assets
        .audio_catalog()
        .and_then(|catalog| catalog.eva().index_of(INCOMING))
        .unwrap_or(-1);
    let selected_done = match (
        state.audio.sfx_player.as_ref(),
        state.process_assets.audio_catalog(),
    ) {
        (Some(sfx), Some(catalog)) => sfx.eva_entry_done(selected_index, catalog.eva()),
        // Native admission needs its stream/device. VERA's no-device route
        // leaves the initially-done entry alone, so it does not guess a delay.
        _ => true,
    };
    let streams_ready = state.audio.movie_streams_ready();
    let Some(radar) = state.match_state.match_presentation.radar_anim.as_mut() else {
        return;
    };
    radar.admit_movie();
    let command = radar.movie_command(selected_done, streams_ready);
    match command {
        RadarMovieCommand::Announce => {
            let side = crate::app::presentation::building_anim::local_eva_side(state);
            if let Some(sfx) = state.audio.sfx_player.as_mut() {
                sfx.clear_eva_queues();
                if let (Some(catalog), Some(assets)) = (
                    state.process_assets.audio_catalog(),
                    state.process_assets.manager(),
                ) {
                    sfx.queue_eva(
                        selected_index,
                        Some(EvaType::Standard),
                        Some(EvaPriority::Critical),
                        catalog.eva(),
                        side,
                        catalog.sounds(),
                        assets,
                        catalog.index(),
                    );
                }
                sfx.suspend_eva();
                state
                    .match_state
                    .match_presentation
                    .radar_movies
                    .eva_suspended = true;
            }
        }
        RadarMovieCommand::PrepareStreams => state.audio.prepare_movie_streams(wall_ms),
        RadarMovieCommand::StartPlayback => {
            state
                .match_state
                .match_presentation
                .radar_movies
                .release_eva_hold(state.audio.sfx_player.as_mut());
            state.audio.set_movie_audio_held(true, wall_ms);
        }
        RadarMovieCommand::None => {}
    }
    let ready = state
        .match_state
        .match_presentation
        .radar_anim
        .as_ref()
        .is_some_and(|radar| radar.movie_ready_to_draw());
    if !ready {
        return;
    }
    let pause = state
        .match_state
        .match_presentation
        .radar_movies
        .queue
        .front()
        .expect("nonempty admitted FIFO")
        .pause_simulation;
    state
        .match_state
        .match_presentation
        .radar_anim
        .as_mut()
        .expect("radar admission")
        .set_movie_pause(pause);
    if let Some(input) = state
        .match_state
        .match_presentation
        .radar_movies
        .set_scenario_pause(pause)
    {
        crate::app::match_runtime::scenario_pause::apply_service(state, input);
    }
    reconcile_pause_clock(state, wall_ms);
    if state
        .match_state
        .match_presentation
        .radar_movies
        .playing
        .is_none()
    {
        let surface = state
            .match_state
            .match_presentation
            .radar_movies
            .queue
            .front_mut()
            .expect("FIFO head")
            .surface
            .take()
            .expect("unstarted head owns video");
        let voice_volume = state
            .persistence
            .options_profile
            .voice_volume
            .clamp(0.0, 1.0);
        let playback = MoviePlayback::new(surface, state.audio.movie_mixer(), voice_volume);
        state.match_state.match_presentation.radar_movies.playing = Some(playback);
    }
    let playback = state
        .match_state
        .match_presentation
        .radar_movies
        .playing
        .as_mut()
        .expect("started head");
    // Menu pause does not stop a communications clip; inactive-window BinkPause
    // is separate from both scenario pause and the normal audio service gate.
    playback.set_active(state.platform.window_active);
    let completed = match playback.step(&state.renderer.gpu) {
        Ok(completed) => completed,
        Err(error) => {
            log::warn!("Radar movie playback failed: {error}");
            true
        }
    };
    let surface = playback.surface();
    state
        .match_state
        .match_presentation
        .radar_anim
        .as_mut()
        .expect("radar draw owner")
        .paint_movie(
            &state.renderer.gpu,
            surface.rgba(),
            [surface.width(), surface.height()],
        );
    if completed {
        let (has_next, service) = state
            .match_state
            .match_presentation
            .radar_movies
            .finish_playing();
        state
            .match_state
            .match_presentation
            .radar_anim
            .as_mut()
            .expect("radar retirement")
            .finish_movie(has_next);
        if let Some(input) = service {
            crate::app::match_runtime::scenario_pause::apply_service(state, input);
        }
        if !has_next {
            state.audio.retire_movie_audio(wall_ms);
            reconcile_pause_clock(state, wall_ms);
        }
    }
    state.platform.window.request_redraw();
}

///5BF450 runs before load preparation (even failure), abort/victory/defeat
///fades and scene replacement. Repeated callers must be harmless.
pub(crate) fn retire(state: &mut AppState, wall_ms: u64) {
    let held = state
        .match_state
        .match_presentation
        .radar_movies
        .scenario_hold;
    // Clear Radar's own movie flag before its shared684180 output is applied.
    let presentation = &mut state.match_state.match_presentation;
    presentation
        .radar_movies
        .release_eva_hold(state.audio.sfx_player.as_mut());
    presentation.radar_movies = RadarMovies::default();
    if let Some(radar) = presentation.radar_anim.as_mut() {
        radar.retire_movies();
    }
    if held {
        crate::app::match_runtime::scenario_pause::apply_service(
            state,
            ScenarioServiceInput::Resume,
        );
    }
    state.audio.retire_movie_audio(wall_ms);
    if state.frontend.screen == crate::ui::game_screen::GameScreen::InGame {
        reconcile_pause_clock(state, wall_ms);
    }
}

/// A loaded world retains Scenario62C but not the physical video FIFO. This
/// Rust recovery policy releases only its recorded credit through684180;
/// native saved Radar/Bink reconstruction remains an explicit boundary.
pub(crate) fn release_saved_scenario_hold(
    sim: &mut crate::sim::world::Simulation,
    held: bool,
) -> Vec<crate::sim::trigger_runtime::TriggerEffect> {
    if held {
        sim.resume_scenario()
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::replay::{ReplayHeader, ReplayLog};
    use crate::sim::trigger_runtime::TriggerEffect;
    use crate::sim::world::Simulation;

    #[test]
    fn pause_holds_follow_native_queued_movie_transitions() {
        let controls = crate::test_fixture::json("tools/scenario_pause.json");
        for row in controls["radar_controls"].as_array().unwrap() {
            let mut sim = Simulation::with_seed(0);
            sim.session_mut_for_test().set_input_locked_for_test(true);
            if row["initial_hold"].as_bool().unwrap() {
                sim.pause_scenario_for_test(0);
            }
            let mut log = ReplayLog::new(ReplayHeader {
                pixel_conversion_bounds: Default::default(),
                version: 2,
                tick_hz: 15,
                seed: 0,
                map_name: String::new(),
                rules_hash: 0,
            });
            let mut movies = RadarMovies::default();
            movies.queue.extend(
                row["clips"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|pause| QueuedMovie {
                        surface: None,
                        pause_simulation: pause.as_bool().unwrap(),
                    }),
            );
            for step in row["steps"].as_array().unwrap() {
                let input = match step["operation"].as_str().unwrap() {
                    "draw" => {
                        movies.set_scenario_pause(movies.queue.front().unwrap().pause_simulation)
                    }
                    "finish" => movies.finish_playing().1,
                    _ => unreachable!(),
                };
                let effects = input.map_or_else(Vec::new, |input| {
                    log.apply_scenario_service(&mut sim, input)
                });
                let events: Vec<_> = effects
                    .iter()
                    .filter_map(|effect| match effect {
                        TriggerEffect::ScenarioPause {
                            outermost: true, ..
                        } => Some("enter_audio"),
                        TriggerEffect::ScenarioResume => Some("exit_audio"),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    movies.scenario_hold(),
                    step["held"].as_bool().unwrap(),
                    "{step}"
                );
                assert_eq!(
                    serde_json::to_value(sim.session()).unwrap()["scenario_pause"]["depth"],
                    step["depth"],
                    "{step}"
                );
                assert_eq!(serde_json::json!(events), step["events"], "{step}");
            }
            assert_eq!(
                sim.session().scenario_paused(),
                row["initial_hold"].as_bool().unwrap()
            );
        }
    }
}
