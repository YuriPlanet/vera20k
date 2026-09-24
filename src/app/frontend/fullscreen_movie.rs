//! Full-screen Bink playback reached from Movies & Credits: Play_Movie
//! `0x005BED40` with the shell arguments `(name, -1, 1, 1, 1, 0)`.
//!
//! Native order: the movie stem (name truncated at its first `.`) raises the
//! campaign movie unlock progress (`0x005C0640` -> `0x005FBF80`) before the
//! `.BIK` existence check; a missing file returns with no visible change.
//! Otherwise the mouse is hidden, the screen cleared to black, the theme and
//! other stream players paused (not stopped) and the Bink soundtrack played
//! at `VoiceVolume`. Frames are copied unscaled, centered in the client area
//! with the origin clamped at zero (`0x004328BD..0x00432A32`). Only a bare
//! Escape release (`Keyboard::Get() == 0x081B`) aborts; other keys and mouse
//! buttons are consumed and ignored. Afterwards the screen is cleared again,
//! paused audio resumes, and the caller's dialog is recreated.

use std::time::Instant;

use anyhow::Result;

use crate::assets::bink_audio::BinkAudioDecoder;
use crate::assets::bink_file::BinkFile;
use crate::audio::movie_stream::MovieAudioStream;
use crate::render::batch::SpriteInstance;
use crate::render::bink_movie::{BinkMovieStep, BinkMovieSurface};
use crate::render::shell_paint::MOVIE_DEPTH;

/// Which dialog Play_Movie's caller recreates afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MovieReturn {
    /// State `0xD` (Sneak Peeks) sets state 4: Movies & Credits `0x101`.
    MoviesAndCredits,
    /// State `0xE` stays in place: the movie list `0x129` reopens.
    MovieList,
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

/// Bink soundtrack track 0 fed frame by frame alongside the video.
struct Soundtrack {
    decoder: BinkAudioDecoder,
    stream: MovieAudioStream,
    next_frame: usize,
}

impl Soundtrack {
    fn open(file: &BinkFile, mixer: &rodio::mixer::Mixer, volume: f32) -> Option<Self> {
        let track = file.header.audio_tracks.first().copied()?;
        let decoder = BinkAudioDecoder::new(track)
            .map_err(|err| log::warn!("Movie soundtrack unavailable: {err}"))
            .ok()?;
        let stream =
            MovieAudioStream::connect(mixer, decoder.sample_rate(), decoder.channels(), volume)?;
        Some(Self {
            decoder,
            stream,
            next_frame: 0,
        })
    }

    /// Decode and queue every audio packet of frames before `until`.
    fn feed(&mut self, file: &BinkFile, until: usize) {
        while self.next_frame < until {
            match file.audio_packets(self.next_frame) {
                Ok(packets) => {
                    for packet in packets.into_iter().filter(|p| p.track_index == 0) {
                        match self.decoder.decode_packet(packet.bytes) {
                            Ok(samples) => {
                                self.stream.push(&samples);
                            }
                            Err(err) => log::warn!(
                                "Movie audio decode error at frame {}: {err}",
                                self.next_frame
                            ),
                        }
                    }
                }
                Err(err) => log::warn!(
                    "Movie audio packet error at frame {}: {err}",
                    self.next_frame
                ),
            }
            self.next_frame += 1;
        }
    }
}

pub(crate) struct FullscreenMovie {
    surface: BinkMovieSurface,
    soundtrack: Option<Soundtrack>,
    last_step: Instant,
    return_to: MovieReturn,
    ended: bool,
    /// Paused while the application is inactive (`BinkPause(1)`).
    paused: bool,
    /// Diagnostic shell capture only: playback is pinned at one frame.
    capture_hold: bool,
}

impl FullscreenMovie {
    pub(crate) fn new(
        surface: BinkMovieSurface,
        mixer: Option<&rodio::mixer::Mixer>,
        voice_volume: f32,
        return_to: MovieReturn,
    ) -> Self {
        let mut soundtrack =
            mixer.and_then(|mixer| Soundtrack::open(surface.file(), mixer, voice_volume));
        // Frame 0 is decoded on construction; queue its audio (the preload).
        if let Some(soundtrack) = soundtrack.as_mut() {
            soundtrack.feed(surface.file(), surface.next_frame());
        }
        Self {
            surface,
            soundtrack,
            last_step: Instant::now(),
            return_to,
            ended: false,
            paused: false,
            capture_hold: false,
        }
    }

    pub(crate) fn return_to(&self) -> MovieReturn {
        self.return_to
    }

    /// `BinkPause` while the application is inactive (`[0xA8ED80]`): the
    /// clock and the soundtrack stop, and reactivation resumes where it left
    /// off. Idempotent per state.
    pub(crate) fn set_active(&mut self, active: bool) {
        if self.paused != active {
            return;
        }
        self.paused = !active;
        if let Some(soundtrack) = self.soundtrack.as_ref() {
            if active {
                soundtrack.stream.resume();
            } else {
                soundtrack.stream.pause();
            }
        }
        self.last_step = Instant::now();
    }

    /// Advance by wall time. Returns `true` once the last frame has played.
    pub(crate) fn step(&mut self, gpu: &crate::render::gpu::GpuContext) -> Result<bool> {
        if self.ended {
            return Ok(true);
        }
        if self.paused || self.capture_hold {
            return Ok(false);
        }
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_step).as_secs_f64();
        self.last_step = now;
        let step = self.surface.step(gpu, elapsed)?;
        if let Some(soundtrack) = self.soundtrack.as_mut() {
            soundtrack.feed(self.surface.file(), self.surface.next_frame());
        }
        if matches!(step, BinkMovieStep::Ended) {
            self.ended = true;
        }
        Ok(self.ended)
    }

    /// Mark the movie finished (bare Escape release).
    pub(crate) fn abort(&mut self) {
        self.ended = true;
    }

    /// Diagnostic shell capture: decode forward until video frame `target`
    /// (zero-based) is displayed, independent of wall time, then pin it.
    pub(crate) fn hold_at_frame_for_capture(
        &mut self,
        gpu: &crate::render::gpu::GpuContext,
        target: usize,
    ) -> Result<()> {
        let frame_time = 1.0 / self.surface.fps().max(1.0);
        while self.surface.next_frame() <= target {
            if matches!(self.surface.step(gpu, frame_time)?, BinkMovieStep::Ended) {
                break;
            }
        }
        self.capture_hold = true;
        Ok(())
    }

    /// Zero-based index of the displayed video frame.
    pub(crate) fn displayed_frame(&self) -> usize {
        self.surface.next_frame().saturating_sub(1)
    }

    pub(crate) fn surface(&self) -> &BinkMovieSurface {
        &self.surface
    }

    pub(crate) fn instance(&self, screen_w: i32, screen_h: i32) -> SpriteInstance {
        let (w, h) = (self.surface.width() as i32, self.surface.height() as i32);
        let (x, y) = movie_origin(screen_w, screen_h, w, h);
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
}
