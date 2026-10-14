//! Action127 presentation/audio consumer. Native53B460/53B560 owns the
//! serialized simulation clock; this owner retains surfaces and the last
//! ordered draw command only. Load/exit retires every acquired resource.

use crate::app::AppState;
use crate::audio::arbiter::HandleOwner;
use crate::audio::sfx::SpatialGain;
use crate::render::chrono_screen::{ChronoScreenDraw, ChronoScreenRenderer};
use crate::sim::superweapon::chrono_screen::ChronoScreenCommand;

const HANDLE: HandleOwner = HandleOwner::Global(0x00A9F9B8);

#[derive(Default)]
pub(crate) struct ChronoScreenPresentation {
    renderer: Option<ChronoScreenRenderer>,
    /// Pending native surface copies are consumed by the existing GPU frame.
    capture_prior: bool,
    capture_destination: bool,
    draw: Option<ChronoScreenDraw>,
    eva_credits: u32,
    scenario_paused: bool,
}

impl ChronoScreenPresentation {
    pub(crate) const fn scenario_paused(&self) -> bool {
        self.scenario_paused
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
            .draw(gpu, encoder, source, draw, tactical);
        if matches!(draw, ChronoScreenDraw::Restore) {
            self.draw = None;
            self.renderer = None;
        }
    }
}

fn play_cue(state: &mut AppState, returning: bool) {
    let sound = state
        .match_state
        .sim_runtime
        .as_ref()
        .and_then(|runtime| {
            let general = &runtime.resources.rules.general;
            if returning {
                general.time_warp_in_sound()
            } else {
                general.time_warp_out_sound()
            }
        })
        .map(str::to_owned);
    if let (Some(sound), Some(catalog), Some(sfx)) = (
        sound,
        state.process_assets.audio_catalog(),
        state.audio.sfx_player.as_mut(),
    ) {
        //750920 receives volume1.0, pan2000 and this global VocHandle.
        //Reuse the existing registered owned-event/handle admission owner.
        sfx.play_animation_sound_spatial(
            HANDLE,
            &sound,
            SpatialGain::CENTRED_FULL,
            catalog.sounds(),
        );
    }
}

pub(crate) fn apply(state: &mut AppState, command: ChronoScreenCommand) {
    let wall_ms = crate::app::match_runtime::sim_tick::monotonic_frame_pacer_ms(
        state,
        std::time::Instant::now(),
    );
    match command {
        ChronoScreenCommand::Start => {
            let screen = &mut state.match_state.match_presentation.chrono_screen;
            screen.renderer = None;
            screen.capture_prior = true;
            screen.capture_destination = false;
            screen.draw = None;
            if let Some(sfx) = state.audio.sfx_player.as_mut() {
                sfx.clear_sound_pool(); //53B501/406E80
                sfx.stop_eva(true); //53B508/7529A0, preserves nested depths
                sfx.set_voc_enabled(true);
                sfx.set_chrono_screen_active(true);
            }
            play_cue(state, false);
            if let Some(sfx) = state.audio.sfx_player.as_mut() {
                sfx.set_voc_enabled(false); //53B538/7518C0
                sfx.suspend_eva(); //53B554/753570
                state
                    .match_state
                    .match_presentation
                    .chrono_screen
                    .eva_credits += 1;
            }
        }
        ChronoScreenCommand::Draw { step, duration } => {
            state.match_state.match_presentation.chrono_screen.draw =
                Some(ChronoScreenDraw::Warp { step, duration });
        }
        ChronoScreenCommand::White => {
            state.match_state.match_presentation.chrono_screen.draw = Some(ChronoScreenDraw::White);
        }
        ChronoScreenCommand::Pause => {
            state
                .match_state
                .match_presentation
                .chrono_screen
                .scenario_paused = true;
            state.match_state.scenario_elapsed_clock.pause(wall_ms);
            if let Some(sfx) = state.audio.sfx_player.as_mut() {
                sfx.set_paused(true, wall_ms);
            }
        }
        ChronoScreenCommand::CaptureDestination => {
            state
                .match_state
                .match_presentation
                .chrono_screen
                .capture_destination = true;
            if let Some(sfx) = state.audio.sfx_player.as_mut() {
                sfx.set_voc_enabled(true);
                sfx.release_animation_sound(HANDLE); //53B6A0/406060
            }
            play_cue(state, true);
            if let Some(sfx) = state.audio.sfx_player.as_mut() {
                sfx.set_voc_enabled(false);
            }
        }
        ChronoScreenCommand::Finish => {
            state
                .match_state
                .match_presentation
                .chrono_screen
                .scenario_paused = false;
            let paused = state.match_state.paused() || state.frontend.fullscreen_movie.is_some();
            if let Some(sfx) = state.audio.sfx_player.as_mut() {
                //684180/406F40 runs while original phase is still3: the
                //53BAD0 gate skips SFX resume but unpauses EVA.
                sfx.set_paused(paused, wall_ms);
                sfx.release_animation_sound(HANDLE);
                sfx.stop_animation_sound(HANDLE); //4060F0(NULL),405C00
                sfx.set_voc_enabled(true);
                let credits = &mut state
                    .match_state
                    .match_presentation
                    .chrono_screen
                    .eva_credits;
                if *credits != 0 {
                    sfx.resume_eva();
                    *credits -= 1;
                }
                sfx.set_chrono_screen_active(false);
            }
            if !state.match_state.presentation_paused() {
                state.match_state.scenario_elapsed_clock.resume(wall_ms);
            }
            state.match_state.match_presentation.chrono_screen.draw =
                Some(ChronoScreenDraw::Restore);
        }
    }
}

/// App teardown has no native saved GPU pointer. Release only this owner's
/// EVA credits, never the movie/menu caller's, and discard both snapshots.
pub(crate) fn retire(state: &mut AppState, wall_ms: u64) {
    let screen = std::mem::take(&mut state.match_state.match_presentation.chrono_screen);
    if let Some(sfx) = state.audio.sfx_player.as_mut() {
        if screen.scenario_paused {
            sfx.set_paused(state.match_state.paused(), wall_ms);
        }
        sfx.stop_animation_sound(HANDLE);
        sfx.set_voc_enabled(true);
        sfx.set_chrono_screen_active(false);
        for _ in 0..screen.eva_credits {
            sfx.resume_eva();
        }
    }
    if !state.match_state.presentation_paused() {
        state.match_state.scenario_elapsed_clock.resume(wall_ms);
    }
}
