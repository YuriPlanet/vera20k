//! Full-screen Bink playback reached from Movies & Credits: Play_Movie
//! `0x005BED40` with the shell arguments `(name, -1, 1, 1, 1, 0)`.
//!
//! Native order: the movie stem (name truncated at its first `.`) raises the
//! campaign movie unlock progress (`0x005C0640` -> `0x005FBF80`) before the
//! `.BIK` existence check; a missing file returns with no visible change.
//! Otherwise the mouse is hidden, the screen cleared to black, the theme and
//! other stream players paused (not stopped) and the Bink soundtrack played
//! at `VoiceVolume`. Frames are copied unscaled, centered in the client area
//! with the origin clamped at zero (`0x004328BD..0x00432A32`). VERA keeps
//! that copy at gamemd's own window sizes and scales the movie to fit any
//! other window, like the fitted shell ([`movie_rect`]). Only a bare
//! Escape release (`Keyboard::Get() == 0x081B`) aborts; other keys and mouse
//! buttons are consumed and ignored. Afterwards the screen is cleared again,
//! paused audio resumes, and the caller's dialog is recreated.

use anyhow::Result;

use crate::render::batch::SpriteInstance;
use crate::render::bink_movie::BinkMovieSurface;
use crate::render::shell_paint::MOVIE_DEPTH;

/// The continuation owned by Play_Movie's caller.
pub(crate) enum MovieReturn {
    /// State `0xD` (Sneak Peeks) sets state 4: Movies & Credits `0x101`.
    MoviesAndCredits,
    /// State `0xE` stays in place: the movie list `0x129` reopens.
    MovieList,
    /// Start_Scenario683AB0 continues into ReadScenario after the selected
    /// Intro/Brief finishes, is skipped, is unavailable, or fails to decode.
    CampaignStart(Box<crate::app::loading::pump::LoadingRequest>),
    /// Basic.Action plays after ReadScenario, before tactical video mode and
    /// scenario timing begin (StartScenario683D9C..683DB6).
    CampaignLoaded(Box<crate::app::loading::init::MapLoadResult>),
    /// TAction10's5BF260 call returns to the same tactical world. The shared
    /// fullscreen owner holds media/audio; the screen gate stops new frames.
    RunningScenario,
    /// A won campaign mission's PostScore/PreMapSelect/FinalMovie
    /// (`0x00685928..0x006859E9`) continues its victory tail.
    CampaignVictory(Box<crate::app::campaign_victory::CampaignVictory>),
}

/// The name Play_Movie resolves: truncated at the first `.`.
pub(crate) fn movie_stem(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

/// Native Bink copy origin: `(client - movie) / 2` per axis, clamped at 0.
/// An oversized movie is shown as its top-left crop; how retail's
/// `BinkCopyToBuffer@28` (no destination width) renders an 800-wide movie on
/// a 640-wide surface is not established.
pub(crate) fn movie_origin(screen_w: i32, screen_h: i32, movie_w: i32, movie_h: i32) -> (i32, i32) {
    (
        ((screen_w - movie_w) / 2).max(0),
        ((screen_h - movie_h) / 2).max(0),
    )
}

/// Where a full-screen movie is drawn, as window-pixel `[x, y, w, h]`.
///
/// At gamemd's own shell sizes this is the native unscaled copy
/// ([`movie_origin`]). Any other window gets the fitted shell
/// (`shell_layout::window_takes_fitted_shell`), and the movie follows it: one
/// uniform factor, the largest at which the movie still fits the window,
/// centred, with black bars on the spare sides. gamemd has no counterpart at
/// those sizes: its `StretchMovies` option only reaches the VQA branch of
/// Play_Movie `0x005BED40`, and every retail movie is a Bink file.
pub(crate) fn movie_rect(screen_w: i32, screen_h: i32, movie_w: i32, movie_h: i32) -> [i32; 4] {
    if !crate::app::frontend::shell_layout::window_takes_fitted_shell((
        screen_w as u32,
        screen_h as u32,
    )) {
        let (x, y) = movie_origin(screen_w, screen_h, movie_w, movie_h);
        return [x, y, movie_w, movie_h];
    }
    let factor = (screen_w as f32 / movie_w as f32).min(screen_h as f32 / movie_h as f32);
    let w = (movie_w as f32 * factor).round() as i32;
    let h = (movie_h as f32 * factor).round() as i32;
    [(screen_w - w) / 2, (screen_h - h) / 2, w, h]
}

pub(crate) struct FullscreenMovie {
    playback: crate::app::movie_playback::MoviePlayback,
    return_to: MovieReturn,
}

impl FullscreenMovie {
    pub(crate) fn new(
        surface: BinkMovieSurface,
        mixer: Option<&rodio::mixer::Mixer>,
        voice_volume: f32,
        return_to: MovieReturn,
    ) -> Self {
        Self {
            playback: crate::app::movie_playback::MoviePlayback::new(surface, mixer, voice_volume),
            return_to,
        }
    }
    pub(crate) fn into_return(self) -> MovieReturn {
        self.return_to
    }
    pub(crate) fn set_active(&mut self, active: bool) {
        self.playback.set_active(active);
    }
    pub(crate) fn step(&mut self, gpu: &crate::render::gpu::GpuContext) -> Result<bool> {
        self.playback.step(gpu)
    }
    pub(crate) fn abort(&mut self) {
        self.playback.abort();
    }
    pub(crate) fn hold_at_frame_for_capture(
        &mut self,
        gpu: &crate::render::gpu::GpuContext,
        target: usize,
    ) -> Result<()> {
        self.playback.hold_at_frame_for_capture(gpu, target)
    }
    pub(crate) fn displayed_frame(&self) -> usize {
        self.playback.displayed_frame()
    }
    pub(crate) fn surface(&self) -> &BinkMovieSurface {
        self.playback.surface()
    }
    pub(crate) fn instance(&self, screen_w: i32, screen_h: i32) -> SpriteInstance {
        let [x, y, w, h] = movie_rect(
            screen_w,
            screen_h,
            self.surface().width() as i32,
            self.surface().height() as i32,
        );
        SpriteInstance {
            position: [x as f32, y as f32],
            size: [w as f32, h as f32],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            depth: MOVIE_DEPTH,
            tint: [1.0, 1.0, 1.0],
            alpha: 1.0,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stem_truncates_at_the_first_dot() {
        assert_eq!(movie_stem("RENEGADE.BIK"), "RENEGADE");
        assert_eq!(movie_stem("A00_F00E"), "A00_F00E");
        assert_eq!(movie_stem("a.b.c"), "a");
    }

    #[test]
    fn origin_centers_movies_and_clamps_negative_offsets() {
        // 800x600 movies fill 800x600 and center at 1024x768; the negative
        // 640x480 origin is clamped (the retail result there is unverified).
        assert_eq!(movie_origin(800, 600, 800, 600), (0, 0));
        assert_eq!(movie_origin(1024, 768, 800, 600), (112, 84));
        assert_eq!(movie_origin(640, 480, 800, 600), (0, 0));
        assert_eq!(movie_origin(1280, 1024, 800, 600), (240, 212));
    }

    #[test]
    fn retail_windows_keep_the_native_unscaled_copy() {
        // YR 800x600 and RA2 640x480 movies at gamemd's two shell sizes.
        assert_eq!(movie_rect(800, 600, 800, 600), [0, 0, 800, 600]);
        assert_eq!(movie_rect(800, 600, 640, 480), [80, 60, 640, 480]);
        assert_eq!(movie_rect(640, 480, 640, 480), [0, 0, 640, 480]);
        assert_eq!(movie_rect(640, 480, 800, 600), [0, 0, 800, 600]);
    }

    #[test]
    fn other_windows_fit_the_movie_at_its_own_shape() {
        // Widescreen: full height, black bars left and right.
        assert_eq!(movie_rect(1920, 1080, 800, 600), [240, 0, 1440, 1080]);
        assert_eq!(movie_rect(1920, 1080, 640, 480), [240, 0, 1440, 1080]);
        assert_eq!(movie_rect(2880, 1800, 800, 600), [240, 0, 2400, 1800]);
        // Taller than 4:3: full width, bars above and below.
        assert_eq!(movie_rect(1280, 1024, 800, 600), [0, 32, 1280, 960]);
        // Exactly 4:3 fills the window; smaller windows shrink the movie.
        assert_eq!(movie_rect(1024, 768, 800, 600), [0, 0, 1024, 768]);
        assert_eq!(movie_rect(400, 300, 800, 600), [0, 0, 400, 300]);
    }
}
