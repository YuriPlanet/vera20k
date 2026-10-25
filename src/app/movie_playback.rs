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
    /// The VoiceVolume last given to the soundtrack.
    voice_volume: f32,
    /// `BinkPause(1)`: the window is inactive, or (radar clips) an in-game
    /// modal is open.
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
            voice_volume,
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

    /// `0x00432E40` re-reads VoiceVolume `[0x00A8EB9C]` every frame and calls
    /// BinkSetVolume when it changed (`0x00432E40..0x00432E7A`). BINKW32's
    /// DirectSound volume (`0x1002B250` → `0x1002AD40`) clamps to 32767 and
    /// picks `trunc(vol * 127 / 32767)` from a `2000·log10(k/127)` millibel
    /// table (`0x1003E820`), so linear gain is the right curve. RESIDUAL: native
    /// steps in 1/127s, at most 0.5 dB quieter (at 0.1); inaudible.
    pub(crate) fn set_voice_volume(&mut self, volume: f32) {
        if volume == self.voice_volume {
            return;
        }
        self.voice_volume = volume;
        if let Some(soundtrack) = self.soundtrack.as_ref() {
            soundtrack.stream.set_volume(volume);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::Source;
    use std::num::NonZero;

    /// Optional end-to-end device route. On Linux ALSA's file PCM can
    /// capture this output with ALSA_CONFIG_PATH, without physical hardware.
    /// Keep the capture outside the checkout; the default wait permits
    /// ordinary listening, while a free-running null device needs only 30ms.
    #[test]
    #[ignore = "requires configured audio output; optional movie device capture"]
    fn retail_soundtrack_plays_through_the_music_output_device() {
        let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()
            .expect("RA2_DIR is required for the movie device control");
        assets.load_nested("MOVIES01.MIX").unwrap();
        let bytes = assets
            .archive("MOVIES01.MIX")
            .unwrap()
            .get_by_name("A00_F00E.BIK")
            .unwrap();
        let file = BinkFile::parse(bytes.to_vec().into()).unwrap();
        let music = crate::audio::music::MusicPlayer::new()
            .expect("configure an output device before running this control");
        let mut soundtrack = Soundtrack::open(&file, music.mixer(), 1.0).unwrap();
        soundtrack.stream.pause();
        soundtrack.feed(&file, 3);
        soundtrack.stream.resume();
        let wait_ms = std::env::var("BINK_AUDIO_DEVICE_WAIT_MS")
            .map(|value| value.parse::<u64>().expect("device wait milliseconds"))
            .unwrap_or(1100);
        std::thread::sleep(std::time::Duration::from_millis(wait_ms));
        assert!(soundtrack.stream.position() > std::time::Duration::ZERO);
    }

    fn assert_soundtrack_output(file: &BinkFile, expected: &[f32], volume: f32, name: &str) {
        let track = file.header.audio_tracks[0];
        let channels = if track.is_stereo() { 2 } else { 1 };
        let (mixer, mut output) = rodio::mixer::mixer(
            NonZero::new(channels).unwrap(),
            NonZero::new(u32::from(track.sample_rate)).unwrap(),
        );
        let mut soundtrack = Soundtrack::open(file, &mixer, volume).unwrap();
        assert_eq!(
            soundtrack.stream.sample_rate(),
            u32::from(track.sample_rate)
        );
        assert_eq!(soundtrack.stream.channels(), channels);
        assert_eq!(output.channels().get(), channels);
        assert_eq!(output.sample_rate().get(), u32::from(track.sample_rate));
        // Same feed path used by MoviePlayback::new/step. Repeated frame
        // boundaries must not enqueue the preload a second time.
        soundtrack.feed(file, 1);
        soundtrack.feed(file, 1);
        soundtrack.feed(file, 3.min(file.frame_index.len()));
        for (index, &sample) in expected.iter().enumerate() {
            let actual = output.next().expect("connected movie source");
            let expected = sample * volume;
            assert!(
                actual.is_finite() && (actual - expected).abs() <= 2e-7 + expected.abs() * 3e-5,
                "{name} movie mixer sample {index}: {actual} != FFmpeg {expected}"
            );
        }
        // The live stream fills underruns with silence rather than replaying.
        assert_eq!(output.next(), Some(0.0));
        assert_eq!(output.next(), Some(0.0));
    }

    #[test]
    fn soundtrack_feed_preserves_ffmpeg_pcm_and_stereo_through_rodio() {
        let corpus = crate::test_fixture::json("tools/bink_audio_oracle.json");
        let pcm = crate::test_fixture::bytes("tests/fixtures/bink_audio/synthetic.f32le");
        for case in corpus["synthetic"].as_array().unwrap() {
            let text = case["bik_hex"].as_str().unwrap();
            let bytes: Vec<u8> = text
                .as_bytes()
                .chunks_exact(2)
                .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
                .collect();
            let file = BinkFile::parse(bytes.into()).unwrap();
            let offset = case["pcm_offset"].as_u64().unwrap() as usize;
            let len = case["sample_count"].as_u64().unwrap() as usize * 4;
            let expected: Vec<f32> = pcm[offset..offset + len]
                .chunks_exact(4)
                .map(|p| f32::from_le_bytes(p.try_into().unwrap()))
                .collect();
            assert_soundtrack_output(&file, &expected, 0.5, case["name"].as_str().unwrap());
        }
    }

    #[test]
    fn retail_movie_soundtrack_feed_matches_external_packet_pcm() {
        let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        assets.load_nested("MOVIES01.MIX").unwrap();
        let archive = assets.archive("MOVIES01.MIX").unwrap();
        let corpus = crate::test_fixture::json("tools/bink_audio_oracle.json");
        for case in corpus["retail"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let bytes = archive
                .get_by_id(case["entry_id"].as_u64().unwrap() as i32)
                .unwrap();
            assert_eq!(
                crate::util::sha256::sha256_hex(bytes),
                case["file_sha256"].as_str().unwrap()
            );
            let file = BinkFile::parse(bytes.to_vec().into()).unwrap();
            if let Some(directory) = std::env::var_os("BINK_AUDIO_REFERENCE_DIR") {
                let path = std::path::Path::new(&directory)
                    .join(name)
                    .with_extension("f32le");
                let reference = std::fs::read(path).unwrap();
                assert_eq!(
                    crate::util::sha256::sha256_hex(&reference),
                    case["pcm_sha256"].as_str().unwrap()
                );
                let expected: Vec<f32> = reference
                    .chunks_exact(4)
                    .map(|p| f32::from_le_bytes(p.try_into().unwrap()))
                    .collect();
                assert_soundtrack_output(&file, &expected, 1.0, name);
            } else {
                let rate = case["sample_rate"].as_u64().unwrap() as u32;
                let channels = case["channels"].as_u64().unwrap() as u16;
                let (mixer, mut output) = rodio::mixer::mixer(
                    NonZero::new(channels).unwrap(),
                    NonZero::new(rate).unwrap(),
                );
                let mut soundtrack = Soundtrack::open(&file, &mixer, 1.0).unwrap();
                assert_eq!(
                    (
                        soundtrack.stream.sample_rate(),
                        soundtrack.stream.channels()
                    ),
                    (rate, channels)
                );
                soundtrack.feed(&file, 3);
                let actual: Vec<f32> = output
                    .by_ref()
                    .take(case["sample_count"].as_u64().unwrap() as usize)
                    .collect();
                for control in case["samples"].as_array().unwrap() {
                    let index = control[0].as_u64().unwrap() as usize;
                    let expected = control[1].as_f64().unwrap() as f32;
                    assert!(
                        (actual[index] - expected).abs() <= 2e-7 + expected.abs() * 3e-5,
                        "{name} mixer sample {index}: {} != FFmpeg {expected}",
                        actual[index]
                    );
                }
            }
        }
    }
}
