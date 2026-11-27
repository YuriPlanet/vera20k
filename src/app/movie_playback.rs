//! Shared Bink video, track-zero audio and inactive-window clock.
//! Fullscreen and tactical radar callers own their respective continuations.
//! Existing 4326C0/432E40 playback port; physical rodio/GPU timing is a
//! cross-platform substitute, not an executed DirectSound/Bink timing proof.
use crate::app::movie_subtitles::{Subtitle, SubtitleTrack};
use crate::assets::bink_audio::BinkAudioDecoder;
use crate::assets::bink_file::BinkFile;
use crate::audio::movie_stream::MovieAudioStream;
use crate::render::bink_movie::{BinkMovieStep, BinkMovieSurface, MovieSoundTiming};
use crate::util::time::Instant;
use anyhow::Result;

/// Bink soundtrack track 0 fed frame by frame alongside the video.
struct Soundtrack {
    decoder: BinkAudioDecoder,
    stream: MovieAudioStream,
    next_frame: usize,
}

impl Soundtrack {
    fn open(
        file: &BinkFile,
        mixer: &rodio::mixer::Mixer,
        volume: f32,
        active: bool,
    ) -> Option<Self> {
        let track = file.header.audio_tracks.first().copied()?;
        let decoder = BinkAudioDecoder::new(track)
            .map_err(|err| log::warn!("Movie soundtrack unavailable: {err}"))
            .ok()?;
        let stream = MovieAudioStream::connect_with_activity(
            mixer,
            decoder.sample_rate(),
            decoder.channels(),
            volume,
            active,
        )?;
        Some(Self {
            decoder,
            stream,
            next_frame: 0,
        })
    }

    /// Bink's sound service (0x10008C1B) reanchors the video timer after a
    /// device underrun. Use the PCM position delivered to the rodio mixer;
    /// its synthetic silence does not advance the picture.
    fn timing(&self, file: &BinkFile) -> MovieSoundTiming {
        let Some(position) = self.stream.take_underrun_position() else {
            return MovieSoundTiming::Playing;
        };
        // 0x10008A23..4A and 0x10008C37 leave the tail to finish normally.
        let tail_frames =
            (3 * u64::from(file.header.fps_num) / (4 * u64::from(file.header.fps_den))) as usize;
        if self.next_frame > file.frame_index.len().saturating_sub(tail_frames) {
            return MovieSoundTiming::Playing;
        }
        // A track can end before its picture. Only recover if another packet
        // will refill it; after the last packet, lack of samples is expected.
        if (self.next_frame..file.frame_index.len()).any(|i| {
            file.audio_packets(i)
                .is_ok_and(|packets| packets.iter().any(|packet| packet.track_index == 0))
        }) {
            MovieSoundTiming::Underrun(position)
        } else {
            MovieSoundTiming::Playing
        }
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

/// A visible background movie keeps its PCM cursor in step with the picture,
/// while the activation edge silences its independent rodio player.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MovieActivity {
    Paused,
    Muted,
    Playing,
}

impl MovieActivity {
    pub(crate) fn for_window(
        window_active: bool,
        window_hidden: bool,
        pause_on_focus_loss: bool,
        session: Option<crate::app::match_runtime::session::SessionMode>,
    ) -> Self {
        if window_hidden
            || crate::app::state::focus_freeze_from_flags(
                pause_on_focus_loss,
                window_active,
                session,
            )
        {
            Self::Paused
        } else if window_active {
            Self::Playing
        } else {
            Self::Muted
        }
    }

    pub(crate) fn runs(self) -> bool {
        self != Self::Paused
    }

    fn output_volume(self, voice_volume: f32) -> f32 {
        if self == Self::Playing {
            voice_volume
        } else {
            0.0
        }
    }
}

pub(crate) struct MoviePlayback {
    surface: BinkMovieSurface,
    soundtrack: Option<Soundtrack>,
    /// None until the first serviced frame after a resume.
    last_step: Option<Instant>,
    ended: bool,
    /// The player's VoiceVolume, restored when the movie becomes audible.
    voice_volume: f32,
    activity: MovieActivity,
    /// Diagnostic shell capture only: playback is pinned at one frame.
    capture_hold: bool,
    /// The movie's `subtitlemd.txt` cues, when it has any.
    subtitles: Option<SubtitleTrack>,
}

impl MoviePlayback {
    pub(crate) fn new(
        surface: BinkMovieSurface,
        mixer: Option<&rodio::mixer::Mixer>,
        voice_volume: f32,
        mut subtitles: Option<SubtitleTrack>,
        activity: MovieActivity,
    ) -> Self {
        let mut soundtrack = mixer.and_then(|mixer| {
            Soundtrack::open(
                surface.file(),
                mixer,
                activity.output_volume(voice_volume),
                activity.runs(),
            )
        });
        // Frame 0 is decoded on construction; queue its audio (the preload).
        if let Some(soundtrack) = soundtrack.as_mut() {
            soundtrack.feed(surface.file(), surface.next_frame());
        }
        if let Some(subtitles) = subtitles.as_mut() {
            subtitles.advance(surface.next_frame() as u32);
        }
        Self {
            surface,
            soundtrack,
            last_step: activity.runs().then(Instant::now),
            ended: false,
            voice_volume,
            activity,
            capture_hold: false,
            subtitles,
        }
    }

    /// Run 0x006C9C60 for each frame decoded since `first`, as 0x00432E40 does.
    fn advance_subtitles(&mut self, first: usize) {
        if let Some(subtitles) = self.subtitles.as_mut() {
            // Bink's one-based FrameNum of each frame just decoded.
            for frame in first + 1..=self.surface.next_frame() {
                subtitles.advance(frame as u32);
            }
        }
    }

    /// The subtitle on screen, if any.
    pub(crate) fn shown_subtitle(&self) -> Option<&Subtitle> {
        self.subtitles.as_ref()?.shown()
    }

    /// The retail Bink service at 0x00432E80 reads the activation byte and
    /// calls BinkPause(1) at 0x00432EAB or BinkPause(0) at 0x00432ED1.
    /// VERA keeps the timer and PCM cursor running for a visible muted movie.
    pub(crate) fn set_activity(&mut self, activity: MovieActivity) {
        if self.activity == activity {
            return;
        }
        if self.activity.runs() != activity.runs() {
            self.last_step = None;
        }
        self.activity = activity;
        if let Some(soundtrack) = self.soundtrack.as_ref() {
            soundtrack
                .stream
                .set_volume(activity.output_volume(self.voice_volume));
            if !activity.runs() {
                soundtrack.stream.pause();
            }
        }
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
            soundtrack
                .stream
                .set_volume(self.activity.output_volume(volume));
        }
    }

    /// Advance by wall time. Returns `true` once the last frame has played.
    pub(crate) fn step(&mut self, gpu: &crate::render::gpu::GpuContext) -> Result<bool> {
        if self.ended {
            return Ok(true);
        }
        if !self.activity.runs() || self.capture_hold {
            return Ok(false);
        }
        let now = Instant::now();
        let elapsed = if let Some(last) = self.last_step.replace(now) {
            now.duration_since(last).as_secs_f64()
        } else {
            // BinkPause(0) 0x1000A5DA clears the DLL timer origin. BinkWait
            // initializes it on its next call (0x10009B90). Keep the audio
            // paused through any wait for a drawable surface, too. Re-anchor
            // after a parked stretch rather than charging that span to the
            // movie clock.
            self.surface.resume_clock();
            if let Some(soundtrack) = self.soundtrack.as_ref() {
                soundtrack.stream.resume();
            }
            0.0
        };
        let first = self.surface.next_frame();
        let sound = self
            .soundtrack
            .as_ref()
            .map_or(MovieSoundTiming::Silent, |s| s.timing(self.surface.file()));
        let step = self.surface.step_with_sound(gpu, elapsed, sound)?;
        self.advance_subtitles(first);
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
        let first = self.surface.next_frame();
        let end = target.saturating_add(1).min(self.surface.frame_count());
        self.surface
            .advance_frames(gpu, end.saturating_sub(first))?;
        self.advance_subtitles(first);
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

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use rodio::Source;
    use std::num::NonZero;

    #[test]
    fn background_playback_mutes_the_track_and_honors_the_session_freeze() {
        use crate::app::match_runtime::session::SessionMode;
        let visible = MovieActivity::for_window(false, false, false, Some(SessionMode::Campaign));
        assert!(visible.runs());
        assert_eq!(visible.output_volume(0.6), 0.0);
        assert!(!MovieActivity::for_window(false, false, true, None).runs());
        assert!(!MovieActivity::for_window(false, false, true, Some(SessionMode::Campaign)).runs());
        assert_eq!(
            MovieActivity::for_window(false, false, true, Some(SessionMode::Wol)),
            visible
        );
        assert!(!MovieActivity::for_window(true, true, false, None).runs());
        assert_eq!(
            MovieActivity::for_window(true, false, true, None).output_volume(0.6),
            0.6
        );
    }

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
        let mut soundtrack = Soundtrack::open(&file, music.mixer(), 1.0, true).unwrap();
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
        let mut soundtrack = Soundtrack::open(file, &mixer, volume, true).unwrap();
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

    fn synthetic_movie(case: &serde_json::Value) -> BinkFile {
        let bytes = case["bik_hex"]
            .as_str()
            .unwrap()
            .as_bytes()
            .chunks_exact(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        BinkFile::parse(bytes.into()).unwrap()
    }

    #[test]
    fn movie_opened_while_inactive_keeps_its_soundtrack_preload() {
        let corpus = crate::test_fixture::json("tools/bink_audio_oracle.json");
        let file = synthetic_movie(&corpus["synthetic"][0]);
        let (mixer, mut output) = rodio::mixer::mixer(
            NonZero::new(1).unwrap(),
            NonZero::new(u32::from(file.header.audio_tracks[0].sample_rate)).unwrap(),
        );
        let mut soundtrack = Soundtrack::open(&file, &mixer, 1.0, false).unwrap();
        soundtrack.feed(&file, 1);
        // Read well past this clip's initial audio packet with the window hidden.
        assert!(output.by_ref().take(4096).all(|sample| sample == 0.0));
        soundtrack.stream.resume();
        assert!(output.take(1024).any(|sample| sample != 0.0));
    }

    #[test]
    fn soundtrack_feed_preserves_ffmpeg_pcm_and_stereo_through_rodio() {
        let corpus = crate::test_fixture::json("tools/bink_audio_oracle.json");
        let pcm = crate::test_fixture::bytes("tests/fixtures/bink_audio/synthetic.f32le");
        for case in corpus["synthetic"].as_array().unwrap() {
            let file = synthetic_movie(case);
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
    fn retail_intro_stall_resumes_at_the_first_unplayed_pcm_sample() {
        let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        assets.load_nested("MOVIES01.MIX").unwrap();
        let bytes = assets
            .archive("MOVIES01.MIX")
            .unwrap()
            .get_by_name("A00_F00E.BIK")
            .unwrap();
        let file = BinkFile::parse(bytes.to_vec().into()).unwrap();
        let (mixer, mut output) =
            rodio::mixer::mixer(NonZero::new(2).unwrap(), NonZero::new(44_100).unwrap());
        let mut soundtrack = Soundtrack::open(&file, &mixer, 1.0, true).unwrap();
        soundtrack.feed(&file, 1);
        // Two seconds of output, of which only the 34,560 stereo frames
        // in the retail preload are movie time. The rest was silence.
        assert_eq!(output.by_ref().take(176_400).count(), 176_400);
        use crate::render::bink_movie::BinkFrameClock;
        use crate::util::time::Duration;
        let mut clock = BinkFrameClock::default();
        let (num, den) = (file.header.fps_num, file.header.fps_den);
        let first = soundtrack.next_frame;
        let due = clock.frames_due(
            Duration::from_secs(2),
            first,
            num,
            den,
            soundtrack.timing(&file),
        );
        let next = first + due;
        assert_eq!(next - 1, 11);
        soundtrack.feed(&file, next);
        // At 783.673 ms of real PCM the next frame is due at 800 ms.
        assert_eq!(
            clock.frames_due(
                Duration::from_millis(16),
                next,
                num,
                den,
                soundtrack.timing(&file)
            ),
            0
        );
        assert_eq!(
            clock.frames_due(
                Duration::from_millis(1),
                next,
                num,
                den,
                soundtrack.timing(&file)
            ),
            1
        );
        // FFmpeg's first stereo pair in the following packet, at PCM frame 34,560.
        for expected in [-0.035_147_775_f32, -0.055_132_65] {
            assert!((output.next().unwrap() - expected).abs() < 1.0 / 32_768.0);
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
                let mut soundtrack = Soundtrack::open(&file, &mixer, 1.0, true).unwrap();
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
