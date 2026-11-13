//! Action127 presentation/audio consumer. Native53B460/53B560 owns the
//! serialized simulation clock; this owner retains surfaces and the last
//! ordered draw command only. Load/exit retires every acquired resource.

use crate::app::AppState;
use crate::audio::arbiter::HandleOwner;
use crate::audio::sfx::{SfxPlayer, SpatialGain};
use crate::render::chrono_screen::{ChronoScreenDraw, ChronoScreenRenderer};
use crate::rules::sound_ini::SoundRegistry;
use crate::sim::superweapon::chrono_screen::ChronoScreenCommand;

const HANDLE: HandleOwner = HandleOwner::Global(0x00A9F9B8);

#[derive(Default)]
pub(crate) struct ChronoScreenPresentation {
    renderer: Option<ChronoScreenRenderer>,
    /// Pending native surface copies are consumed by the existing GPU frame.
    capture_prior: bool,
    capture_destination: bool,
    draw: Option<ChronoScreenDraw>,
}

impl ChronoScreenPresentation {
    /// The existing4F4480 interception also suppresses tactical draw-owned
    /// counters; consumers must not advance them behind a retained surface.
    pub(crate) fn overrides_tactical_draw(&self) -> bool {
        self.draw.is_some()
    }

    /// Whether a draw command or surface capture is pending: the next draws
    /// consume them.
    pub(crate) fn active(&self) -> bool {
        self.draw.is_some() || self.capture_prior || self.capture_destination
    }

    pub(crate) fn before_frame(
        &mut self,
        gpu: &crate::render::gpu::GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Texture,
        tactical: [u32; 4],
    ) {
        if !self.capture_prior {
            return;
        }
        if source.width() <= 1
            || source.height() <= 32
            || source.width() < tactical[0].saturating_add(tactical[2])
        {
            // Before the first real composition there is no valid previous
            // surface to copy. Capture the first actual frame, never a dummy.
            return;
        }
        self.renderer = Some(ChronoScreenRenderer::new(gpu, source, tactical));
        self.renderer.as_ref().unwrap().capture(encoder, source);
        self.capture_prior = false;
    }

    /// Native4F4480 skips tactical draws while53BAE0 is active, then
    /// redraws sidebar/UI. Apply stores at that boundary, before chrome.
    pub(crate) fn draw_tactical(
        &mut self,
        gpu: &crate::render::gpu::GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Texture,
        tactical: [u32; 4],
        bar_under: bool,
    ) {
        let Some(draw) = self.draw else {
            return;
        };
        if self.capture_prior
            || self.capture_destination
            || self
                .renderer
                .as_ref()
                .is_none_or(|renderer| !renderer.matches(source, tactical))
        {
            //Resize or a debug save restored during the input-locked effect
            //cannot retain native GPU pixels. Recapture the actual scene;
            //this Rust recovery policy is not native save/draw parity.
            self.renderer = Some(ChronoScreenRenderer::new(gpu, source, tactical));
            self.renderer.as_ref().unwrap().capture(encoder, source);
            self.capture_prior = false;
            self.capture_destination = false;
        }
        self.renderer
            .as_ref()
            .unwrap()
            .draw(gpu, encoder, source, draw, tactical, bar_under);
        if matches!(draw, ChronoScreenDraw::Restore) {
            self.draw = None;
            self.renderer = None;
        }
    }
}

/// A cue to play: the Rules sound name and the registry that holds it.
type Cue<'a> = Option<(&'a str, &'a SoundRegistry)>;

/// `TimeWarpOutSound=`, or `TimeWarpInSound=` on the way back.
fn cue_name(state: &AppState, returning: bool) -> Option<String> {
    state
        .match_state
        .sim_runtime
        .runtime()
        .and_then(|runtime| {
            let general = &runtime.resources.rules.general;
            if returning {
                general.time_warp_in_sound()
            } else {
                general.time_warp_out_sound()
            }
        })
        .map(str::to_owned)
}

fn play_cue(sfx: &mut SfxPlayer, cue: Cue<'_>) {
    if let Some((sound, sounds)) = cue {
        //750920 receives volume1.0, pan2000 and this global VocHandle.
        //Reuse the existing registered owned-event/handle admission owner.
        sfx.play_animation_sound_spatial(HANDLE, sound, SpatialGain::CENTRED_FULL, sounds);
    }
}

/// Start's sound edges (`0x0053B501..0x0053B554`): EVA stops with its
/// queues, the cue plays with Voc on, then Voc goes off and EVA is
/// suspended until Finish.
fn start_audio(sfx: &mut SfxPlayer, cue: Cue<'_>) {
    sfx.clear_sound_pool(); //53B501/406E80
    sfx.stop_eva(true); //53B508/7529A0, preserves nested depths
    sfx.set_voc_enabled(true);
    sfx.set_chrono_screen_active(true);
    play_cue(sfx, cue);
    sfx.set_voc_enabled(false); //53B538/7518C0
    sfx.suspend_eva(); //53B554/753570
}

fn capture_destination_audio(sfx: &mut SfxPlayer, cue: Cue<'_>) {
    sfx.set_voc_enabled(true);
    sfx.release_animation_sound(HANDLE); //53B6A0/406060
    play_cue(sfx, cue);
    sfx.set_voc_enabled(false);
}

/// Finish's sound edges (`0x0053B6F0..0x0053B714`). 684180/406F40 runs
/// while the phase is still 3. Its SFX resume has nothing to undo, since
/// Enter skipped the sounds under the 53BAD0 gate; it unpauses EVA.
fn finish_audio(sfx: &mut SfxPlayer) {
    sfx.release_animation_sound(HANDLE);
    sfx.stop_animation_sound(HANDLE); //4060F0(NULL),405C00
    sfx.set_voc_enabled(true);
    sfx.resume_eva(); //53B714/753580
    sfx.set_chrono_screen_active(false);
}

pub(crate) fn apply(state: &mut AppState, command: ChronoScreenCommand) {
    let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    match command {
        ChronoScreenCommand::Start => {
            let screen = &mut state.match_state.match_presentation.chrono_screen;
            screen.renderer = None;
            screen.capture_prior = true;
            screen.capture_destination = false;
            screen.draw = None;
            let cue = cue_name(state, false);
            if let Some(sfx) = state.audio.sfx_player.as_mut() {
                let sounds = state.process_assets.audio_catalog().map(|c| c.sounds());
                start_audio(sfx, cue.as_deref().zip(sounds));
            }
        }
        ChronoScreenCommand::Draw { step, duration } => {
            state.match_state.match_presentation.chrono_screen.draw =
                Some(ChronoScreenDraw::Warp { step, duration });
        }
        ChronoScreenCommand::White => {
            state.match_state.match_presentation.chrono_screen.draw = Some(ChronoScreenDraw::White);
        }
        ChronoScreenCommand::Pause => {}
        ChronoScreenCommand::CaptureDestination => {
            state
                .match_state
                .match_presentation
                .chrono_screen
                .capture_destination = true;
            let cue = cue_name(state, true);
            if let Some(sfx) = state.audio.sfx_player.as_mut() {
                let sounds = state.process_assets.audio_catalog().map(|c| c.sounds());
                capture_destination_audio(sfx, cue.as_deref().zip(sounds));
            }
        }
        ChronoScreenCommand::Finish => {
            if let Some(sfx) = state.audio.sfx_player.as_mut() {
                finish_audio(sfx);
            }
            if !state
                .match_state
                .elapsed_clock_paused(&state.frontend.screen)
            {
                state.match_state.scenario_elapsed_clock.resume(wall_ms);
            }
            state.match_state.match_presentation.chrono_screen.draw =
                Some(ChronoScreenDraw::Restore);
        }
    }
}

/// App teardown has no native saved GPU pointer: discard both snapshots. An
/// EVA suspend left by Start lifts at RadarClass::AI's next resume
/// (`0x006539B7`) once the phase reads zero, or at the world's ResetAll.
pub(crate) fn retire(state: &mut AppState, wall_ms: u64) {
    state.match_state.match_presentation.chrono_screen = Default::default();
    if let Some(sfx) = state.audio.sfx_player.as_mut() {
        sfx.stop_animation_sound(HANDLE);
        sfx.set_voc_enabled(true);
        sfx.set_chrono_screen_active(false);
    }
    if !state
        .match_state
        .elapsed_clock_paused(&state.frontend.screen)
    {
        state.match_state.scenario_elapsed_clock.resume(wall_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::audio_runtime::AppAudioRuntime;
    use crate::app::radar_movie::{apply_prelude_audio, prelude_inputs};
    use crate::render::radar::anim::RadarMovieCommand;

    /// ChronoScreen inside a transmission's prelude, as ALL01 plays it: Start
    /// stops IncomingTransmission (`0x0053B508` -> `0x007529A0` marks it
    /// done, so the prelude goes on) and suspends EVA a second time
    /// (`0x0053B554`). StartPlayback's resume (`0x0065792B`) lifts one
    /// suspend; EVA takes lines again only once Finish resumes (`0x0053B714`).
    #[test]
    fn chrono_screen_keeps_eva_shut_through_a_transmission_until_it_finishes() {
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
        let sfx = audio.sfx_player.as_mut().unwrap();
        sfx.set_volume(0.0);
        sfx.set_paused(false, 1_000);

        apply_prelude_audio(
            RadarMovieCommand::Announce,
            &mut audio,
            eva,
            index,
            Some(&assets),
            1_000,
        );
        let sfx = audio.sfx_player.as_mut().unwrap();
        assert_eq!(sfx.current_eva_event(), Some("EVA_INCOMINGTRANSMISSION"));
        start_audio(sfx, None);
        assert_eq!(sfx.current_eva_event(), None);
        assert!(prelude_inputs(&audio, eva).0);

        apply_prelude_audio(
            RadarMovieCommand::StartPlayback,
            &mut audio,
            eva,
            index,
            Some(&assets),
            1_000,
        );
        let sfx = audio.sfx_player.as_mut().unwrap();
        assert!(!sfx.play_eva("EVA_UnitReady", None, definitions.eva(), &assets, index));
        finish_audio(sfx);
        assert!(sfx.play_eva("EVA_UnitReady", None, definitions.eva(), &assets, index));
        sfx.stop_all();
    }
}
