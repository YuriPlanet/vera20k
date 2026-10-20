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
//! other window, like the fitted shell ([`movie_rect`]). A bare Escape release
//! (`Keyboard::Get() == 0x081B`) aborts. The ordinary Apple client additionally
//! offers an explicit pointer button or touch hold-to-skip control; other movie
//! area presses remain inert. Afterwards the screen is cleared again,
//! paused audio resumes, and the caller's dialog is recreated.

use anyhow::Result;
use std::time::{Duration, Instant};

use crate::render::batch::SpriteInstance;
use crate::render::bink_movie::BinkMovieSurface;
use crate::render::shell_paint::{CHROME_DEPTH, MOVIE_DEPTH, TEXT_DEPTH};
use crate::render::shell_text::{ScissorRect, ShellTextDraw};
use crate::ui::shell::geom::RectPx;
use winit::event::{DeviceId, Touch, TouchPhase};

const SKIP_HOLD_DURATION: Duration = Duration::from_millis(1200);

/// One movie owns the gesture and its completion latch. Replacing the movie
/// drops both, so a contact can never carry a held skip into the next route.
#[derive(Default)]
struct SkipGesture {
    contacts: Vec<(DeviceId, u64)>,
    holding: Option<((DeviceId, u64), Instant)>,
    pointer_down: bool,
    pointer_hover: bool,
    touch_mode: bool,
    fired: bool,
}

impl SkipGesture {
    fn cancel(&mut self) {
        self.contacts.clear();
        self.holding = None;
        self.pointer_down = false;
        self.pointer_hover = false;
    }

    fn progress(&self, now: Instant) -> f32 {
        self.holding.map_or(0.0, |(_, began)| {
            (now.saturating_duration_since(began).as_secs_f32() / SKIP_HOLD_DURATION.as_secs_f32())
                .clamp(0.0, 1.0)
        })
    }

    fn poll(&mut self, now: Instant) -> bool {
        if self.fired
            || !self.holding.is_some_and(|(_, began)| {
                now.saturating_duration_since(began) >= SKIP_HOLD_DURATION
            })
        {
            return false;
        }
        self.fired = true;
        self.holding = None;
        true
    }

    #[cfg_attr(not(any(target_os = "ios", target_os = "android")), allow(dead_code))]
    fn touch(&mut self, touch: &Touch, rect: RectPx, now: Instant) {
        self.touch_mode = true;
        self.pointer_down = false;
        self.pointer_hover = false;
        let key = (touch.device_id, touch.id);
        let inside = rect.contains(
            touch.location.x.round() as i32,
            touch.location.y.round() as i32,
        );
        match touch.phase {
            TouchPhase::Started => {
                if self.contacts.contains(&key) {
                    return;
                }
                let first = self.contacts.is_empty();
                self.contacts.push(key);
                // An additional finger cancels the pending action. A finger
                // already held on the video cannot become a fresh skip press.
                self.holding = (first && inside && !self.fired).then_some((key, now));
            }
            TouchPhase::Moved => {
                if self.holding.is_some_and(|(held, _)| held == key) && !inside {
                    self.holding = None;
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.contacts.retain(|contact| *contact != key);
                if self.holding.is_some_and(|(held, _)| held == key) {
                    self.holding = None;
                }
            }
        }
    }

    fn pointer_move(&mut self, inside: bool) {
        self.pointer_hover = inside;
        if !inside {
            self.pointer_down = false;
        }
    }

    fn pointer_button(&mut self, pressed: bool, inside: bool) -> bool {
        // A mouse never completes a touch hold, including synthesized edges
        // from a touch provider. Native touches use the separate owner above.
        if self.touch_mode || self.fired {
            return false;
        }
        self.pointer_hover = inside;
        if pressed {
            self.pointer_down = inside;
            return false;
        }
        let fire = std::mem::take(&mut self.pointer_down) && inside;
        self.fired |= fire;
        fire
    }
}

pub(crate) fn skip_button_rect(screen: [u32; 2], density: f32) -> RectPx {
    // 44 logical pixels high, with the full hit target moved with its face.
    // The upper-right placement keeps both movie subtitles and the home
    // indicator clear, in portrait test windows and landscape phone windows.
    let scale = density
        .clamp(0.5, 4.0)
        .min(screen[0] as f32 / 220.0)
        .min(screen[1] as f32 / 100.0)
        .max(0.25);
    let w = (142.0 * scale).round() as i32;
    let h = (44.0 * scale).round() as i32;
    let margin_x = (24.0 * scale).round() as i32;
    let margin_y = (18.0 * scale).round() as i32;
    RectPx::new((screen[0] as i32 - margin_x - w).max(0), margin_y, w, h)
}

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
    /// A lost campaign mission's Lose movie (`0x00686073`) returns to the
    /// empty backdrop and the `TXT_TO_REPLAY` prompt.
    CampaignDefeat(Box<crate::app::campaign_defeat::CampaignDefeat>),
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
    subtitles: Option<crate::app::movie_subtitles::MovieSubtitles>,
    skip_enabled: bool,
    skip: SkipGesture,
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
            subtitles: None,
            skip_enabled: false,
            skip: SkipGesture {
                touch_mode: cfg!(any(target_os = "ios", target_os = "android")),
                ..Default::default()
            },
        }
    }
    /// Sidecars are opt-in for ordinary Apple packages. Reference captures
    /// and other platforms keep the retail video-only presentation.
    pub(crate) fn with_subtitles(mut self, name: &str, ordinary_client: bool) -> Self {
        self.subtitles = crate::app::movie_subtitles::MovieSubtitles::load(name, ordinary_client);
        self
    }
    pub(crate) fn with_skip_control(mut self, ordinary_client: bool) -> Self {
        self.skip_enabled = ordinary_client
            && cfg!(any(
                target_os = "ios",
                target_os = "macos",
                target_os = "android"
            ));
        self
    }
    pub(crate) fn pointer_skip_visible(&self) -> bool {
        self.skip_enabled && !self.skip.touch_mode
    }
    pub(crate) fn cancel_skip_pointer(&mut self) {
        self.skip.cancel();
    }
    #[cfg(any(target_os = "ios", target_os = "android"))]
    pub(crate) fn skip_touch(&mut self, touch: &Touch, screen: [u32; 2], density: f32) {
        if self.skip_enabled {
            self.skip
                .touch(touch, skip_button_rect(screen, density), Instant::now());
        }
    }
    pub(crate) fn skip_pointer_move(
        &mut self,
        position: (i32, i32),
        screen: [u32; 2],
        density: f32,
    ) {
        if self.skip_enabled {
            self.skip
                .pointer_move(skip_button_rect(screen, density).contains(position.0, position.1));
        }
    }
    pub(crate) fn skip_pointer_button(
        &mut self,
        pressed: bool,
        position: (i32, i32),
        screen: [u32; 2],
        density: f32,
    ) {
        if self.skip_enabled
            && self.skip.pointer_button(
                pressed,
                skip_button_rect(screen, density).contains(position.0, position.1),
            )
        {
            self.abort();
        }
    }
    pub(crate) fn skip_overlay(
        &self,
        font: &crate::render::bit_font::BitFont,
        screen: [u32; 2],
        density: f32,
    ) -> Option<(Vec<SpriteInstance>, ShellTextDraw)> {
        if !self.skip_enabled {
            return None;
        }
        let rect = skip_button_rect(screen, density);
        let progress = self.skip.progress(Instant::now());
        let active = self.skip.pointer_down || self.skip.holding.is_some();
        let border = (rect.h as f32 / 44.0).round().max(1.0);
        let [x, y, w, h] = [rect.x as f32, rect.y as f32, rect.w as f32, rect.h as f32];
        let solid = |position, size, tint, alpha, depth| SpriteInstance {
            position,
            size,
            tint,
            alpha,
            depth,
            uv_origin: [0.0; 2],
            uv_size: [1.0; 2],
            ..Default::default()
        };
        let mut background = vec![solid(
            [x, y],
            [w, h],
            if active {
                [0.16, 0.19, 0.20]
            } else {
                [0.05, 0.07, 0.08]
            },
            0.9,
            CHROME_DEPTH,
        )];
        for (position, size) in [
            ([x, y], [w, border]),
            ([x, y + h - border], [w, border]),
            ([x, y], [border, h]),
            ([x + w - border, y], [border, h]),
        ] {
            background.push(solid(
                position,
                size,
                if active || self.skip.pointer_hover {
                    [0.94, 0.77, 0.37]
                } else {
                    [0.58, 0.61, 0.58]
                },
                1.0,
                CHROME_DEPTH - 0.00001,
            ));
        }
        if self.skip.touch_mode {
            background.push(solid(
                [x + border * 2.0, y + h - border * 5.0],
                [(w - border * 4.0) * progress, border * 3.0],
                [1.0, 0.78, 0.25],
                1.0,
                CHROME_DEPTH - 0.00002,
            ));
        }
        let label = if self.skip.touch_mode {
            "长按跳过"
        } else {
            "跳过（Esc）"
        };
        let scale = (rect.h as f32 * 16.0 / 44.0) / font.glyph_height().max(1.0);
        let text_x = x + (w - font.text_width(label) as f32 * scale) * 0.5;
        let text_y = y + (h - font.glyph_height() * scale) * 0.5
            - if self.skip.touch_mode { border } else { 0.0 };
        let text = ShellTextDraw {
            instances: font.build_text(
                label,
                text_x,
                text_y,
                scale,
                TEXT_DEPTH,
                [0.96, 0.91, 0.75],
                [0.0; 2],
            ),
            scissor: ScissorRect {
                x: rect.x as u32,
                y: rect.y as u32,
                w: rect.w as u32,
                h: rect.h as u32,
            },
        };
        Some((background, text))
    }
    pub(crate) fn subtitle_draw(
        &self,
        font: &crate::render::bit_font::BitFont,
        screen: [u32; 2],
    ) -> Option<crate::render::shell_text::ShellTextDraw> {
        let track = self.subtitles.as_ref()?;
        let header = &self.surface().file().header;
        track.draw(
            font,
            self.displayed_frame(),
            [header.fps_num, header.fps_den],
            movie_rect(
                screen[0] as i32,
                screen[1] as i32,
                header.width as i32,
                header.height as i32,
            ),
            screen,
        )
    }
    pub(crate) fn into_return(self) -> MovieReturn {
        self.return_to
    }
    pub(crate) fn set_active(&mut self, active: bool) {
        if !active {
            self.cancel_skip_pointer();
        }
        self.playback.set_active(active);
    }
    pub(crate) fn step(&mut self, gpu: &crate::render::gpu::GpuContext) -> Result<bool> {
        if self.skip_enabled && self.skip.poll(Instant::now()) {
            self.abort();
        }
        self.playback.step(gpu)
    }
    pub(crate) fn abort(&mut self) {
        self.skip.cancel();
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

    fn touch(id: u64, phase: TouchPhase, inside: bool) -> Touch {
        Touch {
            device_id: DeviceId::dummy(),
            id,
            phase,
            location: winit::dpi::PhysicalPosition::new(if inside { 50.0 } else { 500.0 }, 30.0),
            force: None,
        }
    }

    #[test]
    fn touch_skip_requires_a_full_hold_and_fires_only_once() {
        let mut gesture = SkipGesture::default();
        let now = Instant::now();
        let rect = RectPx::new(0, 0, 142, 44);
        gesture.touch(&touch(1, TouchPhase::Started, true), rect, now);
        assert!(!gesture.poll(now + Duration::from_millis(1199)));
        assert!(gesture.progress(now + Duration::from_millis(600)) > 0.49);
        assert!(gesture.poll(now + SKIP_HOLD_DURATION));
        assert!(!gesture.poll(now + Duration::from_secs(10)));
    }

    #[test]
    fn normal_movie_taps_and_drags_into_skip_do_not_arm_it() {
        let mut gesture = SkipGesture::default();
        let now = Instant::now();
        let rect = RectPx::new(0, 0, 142, 44);
        gesture.touch(&touch(1, TouchPhase::Started, false), rect, now);
        gesture.touch(&touch(1, TouchPhase::Moved, true), rect, now);
        assert!(!gesture.poll(now + Duration::from_secs(3)));
        gesture.touch(&touch(1, TouchPhase::Ended, true), rect, now);
        gesture.touch(&touch(2, TouchPhase::Started, true), rect, now);
        gesture.touch(
            &touch(2, TouchPhase::Ended, true),
            rect,
            now + Duration::from_millis(200),
        );
        assert!(!gesture.poll(now + Duration::from_secs(3)));
    }

    #[test]
    fn leaving_skip_hit_area_cancels_even_when_finger_returns() {
        let mut gesture = SkipGesture::default();
        let now = Instant::now();
        let rect = RectPx::new(0, 0, 142, 44);
        gesture.touch(&touch(1, TouchPhase::Started, true), rect, now);
        gesture.touch(
            &touch(1, TouchPhase::Moved, false),
            rect,
            now + Duration::from_millis(600),
        );
        gesture.touch(
            &touch(1, TouchPhase::Moved, true),
            rect,
            now + Duration::from_millis(800),
        );
        assert_eq!(gesture.progress(now + Duration::from_secs(3)), 0.0);
        assert!(!gesture.poll(now + Duration::from_secs(3)));
    }

    #[test]
    fn cancel_focus_loss_and_next_movie_cannot_complete_a_previous_hold() {
        let now = Instant::now();
        let rect = RectPx::new(0, 0, 142, 44);
        let mut gesture = SkipGesture::default();
        gesture.touch(&touch(1, TouchPhase::Started, true), rect, now);
        gesture.touch(
            &touch(1, TouchPhase::Cancelled, true),
            rect,
            now + Duration::from_millis(600),
        );
        assert!(!gesture.poll(now + Duration::from_secs(3)));
        gesture.touch(&touch(2, TouchPhase::Started, true), rect, now);
        gesture.cancel(); // window focus/suspend cleanup
        gesture.touch(&touch(2, TouchPhase::Moved, true), rect, now);
        assert!(!gesture.poll(now + Duration::from_secs(3)));
        // A route continuation owns a newly constructed movie and gesture.
        let mut next_movie = SkipGesture::default();
        next_movie.touch(&touch(2, TouchPhase::Ended, true), rect, now);
        assert!(!next_movie.poll(now + Duration::from_secs(3)));
    }

    #[test]
    fn an_additional_finger_cancels_the_pending_skip() {
        let mut gesture = SkipGesture::default();
        let now = Instant::now();
        let rect = RectPx::new(0, 0, 142, 44);
        gesture.touch(&touch(1, TouchPhase::Started, true), rect, now);
        gesture.touch(
            &touch(2, TouchPhase::Started, false),
            rect,
            now + Duration::from_millis(500),
        );
        gesture.touch(
            &touch(2, TouchPhase::Ended, false),
            rect,
            now + Duration::from_millis(700),
        );
        assert!(!gesture.poll(now + Duration::from_secs(3)));
    }

    #[test]
    fn desktop_skip_requires_press_and_release_inside_its_button() {
        let mut gesture = SkipGesture::default();
        assert!(!gesture.pointer_button(false, true));
        assert!(!gesture.pointer_button(true, false));
        assert!(!gesture.pointer_button(false, true));
        assert!(!gesture.pointer_button(true, true));
        gesture.pointer_move(false);
        gesture.pointer_move(true);
        assert!(!gesture.pointer_button(false, true));
        assert!(!gesture.pointer_button(true, true));
        assert!(gesture.pointer_button(false, true));
        assert!(!gesture.pointer_button(false, true));
    }

    #[test]
    fn skip_button_tracks_display_density_and_stays_in_the_screen() {
        for (screen, density) in [([2622, 1206], 3.0), ([2880, 1800], 2.0), ([400, 300], 1.0)] {
            let rect = skip_button_rect(screen, density);
            assert!(rect.x > 0 && rect.y > 0);
            assert!(rect.x + rect.w < screen[0] as i32);
            assert!(rect.y + rect.h < screen[1] as i32);
            assert_eq!(rect.h, (44.0 * density) as i32);
        }
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
