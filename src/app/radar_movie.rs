//! Tactical communications FIFO (5BF390/5BF450) and the shared Bink adapter.
//! RadarPresentation owns mode/prelude and the requested pause; RadarMovies
//! retains its acquired ScenarioPause credit. Vox owns selected EVA completion;
//! AppAudioRuntime owns the separate movie stream hold.
//!
//! Native evidence: Steam gamemd,653963/6578F0/6579E0 and the executed
//! prerequisite/lifecycle controls in the ALL01 native evidence packet.
//! Queued pause transitions: original657BFA/657C7D controls in
//! tools/scenario_pause.json.
//! GPU/rodio, failed media decode and missing device are Rust substitutes;
//! the full native copy/final-paint remains unproven.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::app::AppState;
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::movie_playback::MoviePlayback;
use crate::app::movie_subtitles::{self, SubtitleTrack};
use crate::assets::asset_manager::AssetManager;
use crate::assets::audio_bag::AudioIndex;
use crate::audio::arbiter::HandleOwner;
use crate::audio::sfx::SpatialGain;
use crate::render::bink_movie::BinkMovieSurface;
use crate::render::radar::anim::RadarMovieCommand;
use crate::rules::sound_ini::{EvaPriority, EvaRegistry, EvaType};
use crate::sim::replay::ScenarioServiceInput;

const INCOMING: &str = "EVA_IncomingTransmission"; //652D6E literal8392C0

/// The radar's own VocHandle, RadarClass+0x14C0 on the radar singleton
/// `0x0087F7E8`. Only MovieOn/MovieOff use it; RadarOn/RadarOff play unbound.
const RADAR_SOUND: HandleOwner = HandleOwner::Global(0x00880CA8);

/// `VocHandle__EndLooping @ 0x00406060` on the radar's handle. Native ends
/// the loop when the housing finishes opening or closing (`RadarClass::Draw`
/// `0x00653267`, `0x0065328C`) and on an availability change
/// (`ActivateDeactivate` `0x00656C0E`, `0x00656C4E`).
pub(crate) fn end_radar_sound_loop(state: &mut AppState) {
    if let Some(sfx) = state.audio.sfx_player.as_mut() {
        end_radar_sound_loop_with(sfx);
    }
}

pub(crate) fn end_radar_sound_loop_with(sfx: &mut crate::audio::sfx::SfxPlayer) {
    sfx.release_animation_sound(RADAR_SOUND);
}

/// EndLooping, then `PlayGlobal(Rules MovieOn or MovieOff, 0x2000, 1.0)`
/// bound to the radar's handle (`VocHandle__SetEvent 0x004060F0`). Retail
/// `[MovieOn]`/`[MovieOff]` are a `random loop all attack decay` static, so
/// it runs until the housing settles.
fn play_radar_sound_loop(state: &mut AppState, movie_on: bool) {
    end_radar_sound_loop(state);
    let sound = state.rules().and_then(|rules| {
        if movie_on {
            rules.general.movie_on.clone()
        } else {
            rules.general.movie_off.clone()
        }
    });
    if let (Some(sound), Some(catalog), Some(sfx)) = (
        sound,
        state.process_assets.audio_catalog(),
        state.audio.sfx_player.as_mut(),
    ) {
        sfx.play_animation_sound_spatial(
            RADAR_SOUND,
            &sound,
            SpatialGain::CENTRED_FULL,
            catalog.sounds(),
        );
    }
}

/// The head clip runs only while the window is active (`0x00432E40`'s
/// BinkPause) and no in-game modal is open: `StateMachine__EnterPause`
/// pauses it (`0x00683F4D` -> `0x005BF500(1)`) and `ExitPause` resumes it
/// (`0x0068403C`). Not `MatchState::paused`, which holds the clip's own
/// action-117 pause.
pub(crate) fn playback_active(state: &AppState) -> bool {
    state.platform.window_active && !state.match_state.match_presentation.in_game_menu.is_open()
}

struct QueuedMovie {
    surface: Option<BinkMovieSurface>,
    subtitles: Option<SubtitleTrack>,
    pause_simulation: bool,
}

/// Match-scoped media resources. VecDeque preserves the native FIFO without
/// the old vector's capacity limit/growth policy affecting admission.
#[derive(Default)]
pub(crate) struct RadarMovies {
    queue: VecDeque<QueuedMovie>,
    playing: Option<MoviePlayback>,
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
}

/// Request5BF390: campaign mode0, fixed catalogue ordinal, unlock progress
/// before the asset check, source-preserving YR media resolution, then append.
pub(crate) fn enqueue(state: &mut AppState, index: i32, pause_simulation: bool) {
    if state
        .match_state
        .sim_runtime
        .runtime()
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
    enqueue_file(state, stem, pause_simulation);
}

/// The tail of [`enqueue`], for a caller that already knows the clip's file
/// name: resolve the media, resolve the cues, append to the FIFO. Everything
/// after admission — the IncomingTransmission prelude, the housing, the movie
/// audio, the HUD cue rows and the closing — is [`service`]'s, so a clip added
/// here plays exactly the way a campaign transmission does.
///
/// Returns whether the clip was admitted. `false` means it is missing or
/// undecodable; the caller decides what to do with the failed request.
pub(crate) fn enqueue_file(state: &mut AppState, stem: &str, pause_simulation: bool) -> bool {
    let file_name = format!("{stem}.BIK");
    let Some((bytes, source)) = state.process_assets.manager().and_then(|assets| {
        assets
            .get_with_source_ref(&file_name)
            .map(|(bytes, source)| (Arc::<[u8]>::from(bytes), source.to_string()))
    }) else {
        log::info!("Radar movie {file_name} is not installed");
        return false;
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
            // BinkMovie_Open 0x00432750 resolves cues when the clip is admitted.
            let header = &surface.file().header;
            let subtitles = movie_subtitles::cues_for_movie(state.process_assets.manager(), stem)
                .and_then(|cues| SubtitleTrack::new(cues, header.fps_num, header.fps_den));
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
                    subtitles,
                    pause_simulation,
                });
            true
        }
        Err(error) => {
            log::warn!("Radar movie {file_name} decode unavailable: {error}");
            false
        }
    }
}

/// Whether the retail aperture still owes the player a clip: one is queued or
/// running. A caller that adds its own clips waits for this to clear so it
/// never stacks behind a transmission.
pub(crate) fn has_pending(state: &AppState) -> bool {
    !state
        .match_state
        .match_presentation
        .radar_movies
        .queue
        .is_empty()
}

/// Reconcile117's pause with an open modal. The existing clock is idempotent;
/// movie removal never rearms time while another offline pause still exists.
fn reconcile_pause_clock(state: &mut AppState, wall_ms: u64) {
    if state
        .match_state
        .elapsed_clock_paused(&state.frontend.screen)
    {
        state.match_state.scenario_elapsed_clock.pause(wall_ms);
    } else {
        state.match_state.scenario_elapsed_clock.resume(wall_ms);
    }
}

/// Service outside the simulation gate. Only actual sidebar draws advance the
/// housing timer; media and its pause must remain live when117 stops simulation.
pub(crate) fn service(state: &mut AppState, wall_ms: u64) {
    if state.frontend.screen != crate::ui::game_screen::GameScreen::InGame {
        return;
    }
    if ai_runs(state) {
        radar_ai(state, wall_ms);
    }
    if state
        .match_state
        .match_presentation
        .radar_movies
        .queue
        .is_empty()
    {
        wind_down_retired(state, wall_ms);
        return;
    }
    play_head(state, wall_ms);
}

/// Whether `RadarClass::AI @ 0x00653850` (vt+0x28) runs this pass. It runs
/// on Main_Tick's input path (`0x0055D8AB`), which needs `g_GameState`
/// `0x00A8EDA0` zero (`0x0055D883`). Every in-game menu runs inside
/// State_Machine `0x0048C8B0` with it 1..9, so even the network modes' modal
/// pump (`0x00623120` -> Main_Tick) skips the radar. Main_Tick's offline
/// scenario-pause path (`0x0055D82E..0x0055D877`: audio pump, ChronoScreen,
/// `GScreenClass::Render @ 0x004F4480`, throttle, return) skips it too, and
/// still draws the housing and a playing clip.
fn radar_ai_runs(
    menu_open: bool,
    mode: crate::app::match_runtime::session::SessionMode,
    scenario_paused: bool,
) -> bool {
    !menu_open
        && !crate::app::match_runtime::scenario_pause::blocks_main_tick(mode, scenario_paused)
}

fn ai_runs(state: &AppState) -> bool {
    radar_ai_runs(
        state.match_state.match_presentation.in_game_menu.is_open(),
        state.match_state.session_mode(),
        state
            .match_state
            .sim_runtime
            .runtime()
            .is_some_and(|rt| rt.simulation.session().scenario_paused()),
    )
}

/// The radar's own passes over a clip `retire_for_load` took from the queue
/// (`RadarPresentation::wind_down_retired`). Stage 2 resumes EVA
/// (`0x0065792B`) and plays MovieOn; the movie-done arm then plays MovieOff
/// (`0x00657C2C..0x00657C5B`), restores the Normal floor (`0x00657C71`) and
/// drops the movie's pause (`0x00657C85..0x00657CD4`).
fn wind_down_retired(state: &mut AppState, wall_ms: u64) {
    if !state
        .match_state
        .match_presentation
        .radar_anim
        .as_ref()
        .is_some_and(|radar| radar.has_movie())
    {
        return;
    }
    let ai_runs = ai_runs(state);
    let eva = state
        .process_assets
        .audio_catalog()
        .map(|catalog| catalog.eva());
    let (selected_done, _) = prelude_inputs(&state.audio, eva);
    let Some(movie_on) = state
        .match_state
        .match_presentation
        .radar_anim
        .as_mut()
        .and_then(|radar| radar.wind_down_retired_movie(ai_runs, selected_done))
    else {
        return;
    };
    let pause = state
        .match_state
        .match_presentation
        .radar_movies
        .set_scenario_pause(false);
    if movie_on {
        if let Some(sfx) = state.audio.sfx_player.as_mut() {
            sfx.resume_eva(); //65792B/753580
        }
        play_radar_sound_loop(state, true);
    }
    play_radar_sound_loop(state, false);
    set_detail_floor(state, false);
    if let Some(input) = pause {
        crate::app::match_runtime::scenario_pause::apply_service(state, input);
    }
    reconcile_pause_clock(state, wall_ms);
}

/// The movie half of `RadarClass::AI`: admission (`0x00653963`) and, in mode
/// 4, the prelude (`0x006539A2` -> `0x006578F0`). Every other pass resumes
/// EVA unless ChronoScreen holds the screen (`0x006539AE..0x006539B7`), so
/// a prelude's suspend, or one a retired movie left, lifts on the next pass.
fn radar_ai(state: &mut AppState, wall_ms: u64) {
    let queued = !state
        .match_state
        .match_presentation
        .radar_movies
        .queue
        .is_empty();
    let prelude = state
        .match_state
        .match_presentation
        .radar_anim
        .as_mut()
        .is_some_and(|radar| {
            if queued {
                radar.admit_movie();
            }
            radar.movie_prelude()
        });
    if !prelude {
        resume_eva_outside_prelude(state);
        return;
    }
    if !queued {
        // A stage `retire_for_load` left: `wind_down_retired` steps it.
        return;
    }
    let catalog = state.process_assets.audio_catalog();
    let eva = catalog.map(|catalog| catalog.eva());
    let (selected_done, streams_ready) = prelude_inputs(&state.audio, eva);
    let Some(radar) = state.match_state.match_presentation.radar_anim.as_mut() else {
        return;
    };
    let command = radar.movie_command(selected_done, streams_ready);
    apply_prelude_audio(
        command,
        &mut state.audio,
        eva,
        catalog.and_then(|catalog| catalog.index()),
        state.process_assets.manager(),
        wall_ms,
    );
    if command == RadarMovieCommand::StartPlayback {
        //6578F0 stage2: SetRadarMode(3), EndLooping, then MovieOn.
        play_radar_sound_loop(state, true);
        set_detail_floor(state, true);
    }
}

fn incoming_index(eva: Option<&EvaRegistry>) -> i32 {
    eva.and_then(|registry| registry.index_of(INCOMING))
        .unwrap_or(-1)
}

/// What `0x006578F0` reads before it steps the prelude: whether the selected
/// IncomingTransmission entry is done, and whether the streams settled
/// (`0x00408410`).
pub(super) fn prelude_inputs(audio: &AppAudioRuntime, eva: Option<&EvaRegistry>) -> (bool, bool) {
    let selected_done = match (audio.sfx_player.as_ref(), eva) {
        (Some(sfx), Some(registry)) => sfx.eva_entry_done(incoming_index(eva), registry),
        // Native admission needs its stream/device. VERA's no-device route
        // leaves the initially-done entry alone, so it does not guess a delay.
        _ => true,
    };
    (selected_done, audio.movie_streams_ready())
}

/// The EVA and stream edges of one prelude step (`0x006578F0`); the caller
/// plays the radar's MovieOn loop after StartPlayback.
pub(super) fn apply_prelude_audio(
    command: RadarMovieCommand,
    audio: &mut AppAudioRuntime,
    eva: Option<&EvaRegistry>,
    audio_index: Option<&AudioIndex>,
    assets: Option<&AssetManager>,
    wall_ms: u64,
) {
    match command {
        RadarMovieCommand::Announce => {
            if let Some(sfx) = audio.sfx_player.as_mut() {
                sfx.clear_eva_queues();
                if let (Some(registry), Some(assets)) = (eva, assets) {
                    sfx.queue_eva(
                        incoming_index(eva),
                        Some(EvaType::Standard),
                        Some(EvaPriority::Critical),
                        registry,
                        assets,
                        audio_index,
                    );
                }
                sfx.suspend_eva(); //6579C0/753570
            }
        }
        RadarMovieCommand::PrepareStreams => audio.prepare_movie_streams(wall_ms),
        RadarMovieCommand::StartPlayback => {
            if let Some(sfx) = audio.sfx_player.as_mut() {
                sfx.resume_eva(); //65792B/753580
            }
            audio.set_movie_audio_held(true, wall_ms);
        }
        RadarMovieCommand::None => {}
    }
}

/// `RadarClass::AI`'s pass outside mode 4.
pub(crate) fn resume_eva_outside_prelude(state: &mut AppState) {
    if let Some(sfx) = state.audio.sfx_player.as_mut() {
        sfx.resume_eva_unless_chrono_screen();
    }
}

/// Draw and step the FIFO head once Radar has opened for it.
fn play_head(state: &mut AppState, wall_ms: u64) {
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
    let voice_volume = state
        .persistence
        .options_profile
        .voice_volume
        .clamp(0.0, 1.0);
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
        // Radar's 0x005BF360 callback posts HUD rows instead of a movie overlay.
        // Its cue cursor stays with the FIFO head and advances only while active.
        let playback = MoviePlayback::new(surface, state.audio.movie_mixer(), voice_volume, None);
        state.match_state.match_presentation.radar_movies.playing = Some(playback);
    }
    let active = playback_active(state);
    let playback = state
        .match_state
        .match_presentation
        .radar_movies
        .playing
        .as_mut()
        .expect("started head");
    // Both pauses are separate from scenario pause and the audio service gate.
    playback.set_active(active);
    playback.set_voice_volume(voice_volume);
    let completed = match playback.step(&state.renderer.gpu) {
        Ok(completed) => completed,
        Err(error) => {
            log::warn!("Radar movie playback failed: {error}");
            true
        }
    };
    let surface = playback.surface();
    let movie_frame = surface.next_frame() as u32;
    state
        .match_state
        .match_presentation
        .radar_anim
        .as_mut()
        .expect("radar draw owner")
        .paint_movie(
            surface.batch_texture(),
            [surface.width(), surface.height()],
            state
                .match_state
                .match_presentation
                .sidebar_layout_spec
                .radar_movie_origin(),
        );
    let mut due = Vec::new();
    if active
        && let Some(subtitles) = state
            .match_state
            .match_presentation
            .radar_movies
            .queue
            .front_mut()
            .and_then(|movie| movie.subtitles.as_mut())
    {
        subtitles.advance_with(movie_frame, |cue| {
            if let Some(text) = cue.text.as_ref() {
                due.push((text.clone(), cue.duration));
            }
        });
    }
    for (text, duration) in due {
        crate::app::input::messages::post_movie_subtitle(state, &text, duration);
    }
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
        if has_next {
            //657BB9..657BE1: EndLooping, MovieOn, phase4 frame25.
            play_radar_sound_loop(state, true);
        }
        if let Some(input) = service {
            crate::app::match_runtime::scenario_pause::apply_service(state, input);
        }
        if !has_next {
            //657C2C..657C5B: ResumeAfterMovie, EndLooping, MovieOff.
            state.audio.retire_movie_audio(wall_ms);
            play_radar_sound_loop(state, false);
            set_detail_floor(state, false);
            reconcile_pause_clock(state, wall_ms);
        }
    }
    state.platform.window.request_redraw();
}

/// The detail floor a radar movie sets: `PlayRadarMovie` stage 2 calls
/// `Game__SetMinFrameRate` with DetailMinFrameRateMovie (`0x0065797E`) and
/// `PerFrameMovieUpdate`'s last clip with DetailMinFrameRateNormal
/// (`0x00657C71`). Retirement (`0x005BF450`) leaves it for the next
/// scenario's `Fill_In_Data`, or for a failed load's wind-down.
fn set_detail_floor(state: &AppState, playing: bool) {
    if let Some(rules) = state.rules() {
        apply_detail_floor(
            &mut state.match_state.match_presentation.detail.borrow_mut(),
            &rules.general.detail,
            playing,
        );
    }
}

fn apply_detail_floor(
    detail: &mut crate::app::presentation::detail::DetailState,
    rules: &crate::rules::ruleset::DetailRules,
    playing: bool,
) {
    detail.set_min_frame_rate(if playing {
        rules.min_frame_rate_movie
    } else {
        rules.min_frame_rate_normal
    });
}

/// `Audio__ResumeAfterMovieAndCleanup @ 0x005BF450` alone, before a load's
/// preparation: it deletes the queued clips and releases the movie audio,
/// and leaves the radar's Mode, MovieStage, EVA suspend and movie pause. A
/// load that fails keeps the match, and the radar winds down on its own
/// passes (`wind_down_retired`); one that succeeds retires the rest.
pub(crate) fn retire_for_load(state: &mut AppState, wall_ms: u64) {
    let movies = &mut state.match_state.match_presentation.radar_movies;
    movies.queue.clear();
    movies.playing = None;
    state.audio.retire_movie_audio(wall_ms);
}

///5BF450 runs before abort/victory/defeat fades, scene replacement and a
///load's world replacement; no radar pass follows it there, so the radar's
///own wind-down runs here too. Repeated callers must be harmless.
pub(crate) fn retire(state: &mut AppState, wall_ms: u64) {
    let held = state
        .match_state
        .match_presentation
        .radar_movies
        .scenario_hold;
    // Clear Radar's own movie flag before its shared684180 output is applied.
    // 5BF450 leaves a prelude's EVA suspend to RadarClass::AI's next resume.
    let presentation = &mut state.match_state.match_presentation;
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
/// releases only the FIFO's recorded credit through684180.
/// Deliberate difference, the maintainer's choice: native keeps the radar's
/// `MoviePausedGame` with the rest of its body, and
/// `RadarClass::Init @ 0x00655B20` ends a playing clip without the movie-done
/// arm's `ScenarioPause__ExitOneLevel` (`0x00657CC8`), so a game saved under
/// a pausing clip (action 117) loads paused and nothing in the match lifts
/// it. A stage still waiting for its clip ends through that arm a few passes
/// after the load, which lifts it in native too; VERA lifts it at the load.
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

    /// Rules+6F8/+6FC (`0x0066AB5F..0x0066ABC7`) name the static that
    /// `PlayRadarMovie 0x006578F0` and `PerFrameMovieUpdate 0x006579E0` play.
    #[test]
    fn retail_rules_name_the_transmission_static() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let general = &retail.rules.general;
        assert_eq!(
            (general.movie_on.as_deref(), general.movie_off.as_deref()),
            (Some("MovieOn"), Some("MovieOff"))
        );
    }

    /// Retail DetailMinFrameRateMovie=20 (`0x0065797E`) cuts effects at 18
    /// Logic frames a second during a movie; after the last clip's Normal=15
    /// (`0x00657C71`) they come back once the rate reaches 15 plus the
    /// buffer of 5 (`0x0055AF60`).
    #[test]
    fn radar_movie_raises_the_detail_floor_while_it_plays() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = &retail.rules.general.detail;
        let mut detail = crate::app::presentation::detail::DetailState::default();
        detail.configure_normal(rules);
        for _ in 0..18 {
            detail.record_logic_visit();
        }
        detail.throttle_tail(0);
        assert_eq!(detail.frame_rate(), 18);
        assert_eq!(detail.minimum_frame_rate(), 15);
        apply_detail_floor(&mut detail, rules, true);
        assert_eq!(detail.minimum_frame_rate(), 25);
        apply_detail_floor(&mut detail, rules, false);
        assert_eq!(detail.minimum_frame_rate(), 20);
    }

    /// A time jump's scenario pause (`0x0055D82E..0x0055D877`) returns before
    /// `GScreenClass::Input`, so a clip queued meanwhile is not admitted until
    /// the pause lifts; network modes never take that path.
    #[test]
    fn offline_scenario_pause_holds_radar_admission() {
        use crate::app::match_runtime::session::SessionMode;
        assert!(radar_ai_runs(false, SessionMode::Campaign, false));
        assert!(!radar_ai_runs(false, SessionMode::Campaign, true));
        assert!(radar_ai_runs(false, SessionMode::Lan, true));
    }

    /// The prelude (`0x006578F0`) suspends EVA right after it queues
    /// IncomingTransmission, so `QueueVoice @ 0x00752480` refuses a line
    /// asked for then (`0x00B1D3D8`). StartPlayback's resume (`0x0065792B`)
    /// lifts that under the clip's PauseEVA (`0x005BF580`): a line asked for
    /// during the clip waits, and starts once the hold lifts (`0x005BF530`).
    #[test]
    fn a_transmission_refuses_eva_in_its_prelude_and_holds_it_through_the_clip() {
        let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let Some(mut audio) = AppAudioRuntime::with_sfx_output_for_test() else {
            return;
        };
        let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
            .expect("retail audio definitions");
        let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
        let index = loaded_index.map(|loaded| &loaded.index);
        let eva = Some(&**definitions.eva());
        let started = crate::util::time::Instant::now();
        let now = || 1_000 + u64::try_from(started.elapsed().as_millis()).unwrap();
        let step = |audio: &mut AppAudioRuntime, command| {
            apply_prelude_audio(command, audio, eva, index, Some(&assets), now());
        };
        let pump = |audio: &mut AppAudioRuntime| {
            let sfx = audio.sfx_player.as_mut().unwrap();
            sfx.set_paused(false, now());
            sfx.advance_voice_queue(definitions.eva(), &assets, index);
        };
        audio.sfx_player.as_mut().unwrap().set_volume(0.0);
        pump(&mut audio);

        step(&mut audio, RadarMovieCommand::Announce);
        let sfx = audio.sfx_player.as_mut().unwrap();
        assert_eq!(sfx.current_eva_event(), Some("EVA_INCOMINGTRANSMISSION"));
        assert!(!sfx.play_eva("EVA_UnitReady", None, definitions.eva(), &assets, index));
        while !prelude_inputs(&audio, eva).0 {
            assert!(
                started.elapsed().as_secs() < 15,
                "IncomingTransmission ends"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
            pump(&mut audio);
        }
        step(&mut audio, RadarMovieCommand::PrepareStreams);
        step(&mut audio, RadarMovieCommand::StartPlayback);
        let sfx = audio.sfx_player.as_mut().unwrap();
        assert!(sfx.play_eva("EVA_UnitReady", None, definitions.eva(), &assets, index));
        pump(&mut audio);
        assert_eq!(audio.sfx_player.as_ref().unwrap().current_eva_event(), None);

        audio.retire_movie_audio(now());
        pump(&mut audio);
        let sfx = audio.sfx_player.as_mut().unwrap();
        assert_eq!(sfx.current_eva_event(), Some("EVA_UNITREADY"));
        sfx.stop_all();
    }

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
                        subtitles: None,
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
