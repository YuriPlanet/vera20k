//! Shared Bink video, track-zero audio and inactive-window clock.
//! Fullscreen and tactical radar callers own their respective continuations.
//! Existing 4326C0/432E40 playback port; physical rodio/GPU timing is a
//! cross-platform substitute, not an executed DirectSound/Bink timing proof.
use crate::assets::bink_audio::BinkAudioDecoder;
use crate::assets::bink_file::BinkFile;
use crate::audio::movie_stream::MovieAudioStream;
use crate::render::bink_movie::{BinkMovieStep, BinkMovieSurface};
use anyhow::Result;
use std::time::Instant;

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

pub(crate) struct MoviePlayback {
    surface: BinkMovieSurface,
    soundtrack: Option<Soundtrack>,
    last_step: Instant,
    ended: bool,
    /// Paused while the application is inactive (`BinkPause(1)`).
    paused: bool,
    /// Diagnostic shell capture only: playback is pinned at one frame.
    capture_hold: bool,
}

impl MoviePlayback {
    pub(crate) fn new(
        surface: BinkMovieSurface,
        mixer: Option<&rodio::mixer::Mixer>,
        voice_volume: f32,
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
            ended: false,
            paused: false,
            capture_hold: false,
        }
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
}
