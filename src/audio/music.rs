//! Optional rodio music output.
//!
//! Theme catalog and lifecycle ownership live in `audio::theme`. This module
//! owns only the physical device/player, the source the device pulls each song
//! through (decoding it as it plays), and independent gain projections.

use std::num::NonZero;
use std::time::Duration;

use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player, Source};

use crate::audio::arbiter::{VolumeInterp, combine_linear};
use crate::audio::sfx::native_volume_amplitude;
use crate::audio::theme::{MusicOutputState, PreparedTrack};
use crate::rules::sound_ini::VOLUME_SCALE;
use crate::util::native_x87::X87Chop53;

/// The output's score volume before startup applies the Options profile
/// (`apply_startup_audio_profile`). It mirrors `OptionsClass__SetDefaults
/// @ 0x005FA350`'s 0.4; the profile is the persisted authority.
pub const DEFAULT_SCORE_VOLUME: f64 = 0.4;

/// The Theme stream's DirectSound amplitude.
///
/// gamemd-derived: the Theme stream is a channel of the shared pool
/// (`StreamPlayer` ctor `0x00407860` -> `0x004036C0`), so
/// `DSoundBuffer::CombineInterps @ 0x00402220` multiplies its linears as
/// `(a * b) >> 14` and `DSoundBuffer__ApplyVolumePanFrequency @ 0x0040A6D0`
/// converts the product once through the DirectSound table, exactly as for
/// effects. The ctor clears groups `+0x90..+0x9C` (`0x004078D1..0x004078F3`);
/// `ThemeClass__ReadINI` links only `+0x94` = Score `[0x0087E744]`
/// (`0x00720743`) and `+0x9C` = `[0x0087E754]` (`0x00720751`), which only
/// `AudioSystem__Init` writes (`0x00406B92`), so it is unity here. The
/// effects master `[0x0087E758]` never reaches this stream.
///
/// `own_linear` is the stream's own `+0x10` interp (see `MusicPlayer::own`).
///
/// Score: `OptionsClass::Set_Score_Volume @ 0x005FA4A0` targets
/// `ftol(volume * 16384.0)` (`0x005FA4C3..0x005FA4D6`).
fn effective_music_volume(score_volume: f64, own_linear: i32, focus_output_scale: f64) -> f32 {
    let score = X87Chop53::ftol_f64_low_masked(score_volume * f64::from(VOLUME_SCALE));
    native_volume_amplitude(combine_linear(own_linear, score)) * focus_output_scale as f32
}

/// Samples per span the Theme stream reports. rodio 0.22.2
/// `UniformSourceIterator::bootstrap` cuts the device mixer's input into spans
/// of at most 32768 samples, as it cut the whole-song buffer this stream
/// replaced, so its rate converter restarts at the same points and
/// `Player::get_pos` stays exact. VERA-owned, not native.
const SPAN_SAMPLES: usize = 32_768;

const STEREO: NonZero<u16> = NonZero::new(2).unwrap();

#[cfg(not(test))]
type MusicOutput = MixerDeviceSink;

// Control-state regressions use rodio's offline mixer. Opening devices from
// short-lived Windows test threads can outlive CPAL's cached COM apartment:
// https://github.com/RustAudio/cpal/issues/1302.
#[cfg(test)]
enum MusicOutput {
    Device(MixerDeviceSink),
    Offline(rodio::mixer::Mixer),
}

#[cfg(test)]
impl MusicOutput {
    fn mixer(&self) -> &rodio::mixer::Mixer {
        match self {
            Self::Device(device) => device.mixer(),
            Self::Offline(mixer) => mixer,
        }
    }
}

/// The device-thread end of the Theme stream: rodio pulls the song from it,
/// decoding it as it plays (see [`PreparedTrack`]).
struct MusicStream {
    track: PreparedTrack,
    sample_rate: NonZero<u32>,
}

impl Iterator for MusicStream {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        self.track.next()
    }
}

impl Source for MusicStream {
    fn current_span_len(&self) -> Option<usize> {
        // rodio 0.22.2 `Source::current_span_len`: `Some(0)` only once no
        // sample remains.
        Some(if self.track.exhausted() {
            0
        } else {
            SPAN_SAMPLES
        })
    }

    fn channels(&self) -> NonZero<u16> {
        STEREO
    }

    fn sample_rate(&self) -> NonZero<u32> {
        self.sample_rate
    }

    /// Unknown before the last block decodes; nothing reads it. The Theme
    /// list shows the WAV header length (`ThemeEntry::duration_seconds`).
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

pub struct MusicPlayer {
    /// rodio mixer device sink — must be kept alive or all audio stops.
    _device: MusicOutput,
    current_player: Option<Player>,
    volume: f64,
    /// The Theme stream's own `+0x10` interp (`stream+0x14 -> +0x10`), built
    /// by the stream constructor at rate `0x10624D` (1000 ms from full to
    /// silence, `FUN_00407100`, `FUN_004071A0`). One interp carries every
    /// writer: `StreamPlayer__PlayFile` snaps it to full for each new file
    /// (`0x00407E7D`), Theme's fades (`0x004080C0`) and the radar-movie
    /// pre-fade (`0x00408390`) glide it to zero, and hold releases glide it
    /// back (`0x004083D0`). Theme's fade ends when it stops moving
    /// (`0x00720FE1` -> `0x004080D0`).
    own: VolumeInterp,
    focus_output_scale: f64,
    paused: bool,
}

impl MusicPlayer {
    /// Create a physical music output. Theme construction never depends on
    /// this succeeding.
    pub fn new() -> Option<Self> {
        let device = DeviceSinkBuilder::open_default_sink()
            .map_err(|error| log::error!("Failed to initialize music audio: {error}"))
            .ok()?;
        #[cfg(test)]
        let device = MusicOutput::Device(device);
        Some(Self::with_output(device))
    }

    fn with_output(device: MusicOutput) -> Self {
        Self {
            _device: device,
            current_player: None,
            volume: DEFAULT_SCORE_VOLUME,
            own: VolumeInterp::new(VOLUME_SCALE, 0),
            focus_output_scale: 1.0,
            paused: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn offline_for_test() -> (Self, rodio::mixer::MixerSource) {
        let (mixer, source) = rodio::mixer::mixer(STEREO, NonZero::new(22_050).unwrap());
        (Self::with_output(MusicOutput::Offline(mixer)), source)
    }

    pub(crate) fn state(&self) -> MusicOutputState {
        match self.current_player.as_ref() {
            Some(player) if player.empty() => MusicOutputState::Finished,
            Some(_) if self.own.moving() => MusicOutputState::Ramping,
            Some(_) => MusicOutputState::Playing,
            None => MusicOutputState::Idle,
        }
    }

    pub(crate) fn submit(&mut self, prepared: PreparedTrack) -> bool {
        let Some(sample_rate) = NonZero::new(prepared.sample_rate) else {
            return false;
        };
        if let Some(player) = self.current_player.take() {
            player.stop();
        }
        // `StreamPlayer__PlayFile`: `SetTargetImmediate(+0x10, 0x4000)`.
        self.own.set_target_immediate(VOLUME_SCALE);
        self.own.tick(0);
        log::info!("Playing prepared music track: {}", prepared.stem);
        let player = Player::connect_new(self._device.mixer());
        player.set_volume(self.effective_volume());
        player.append(MusicStream {
            track: prepared,
            sample_rate,
        });
        if self.paused {
            player.pause();
        }
        self.current_player = Some(player);
        true
    }

    pub fn stop(&mut self) {
        if let Some(player) = self.current_player.take() {
            player.stop();
        }
    }

    /// Play_Movie `0x005BED40` blocks and pauses every stream player
    /// (`0x00408270`) and resumes them afterwards (`0x00408230`), so the
    /// theme continues from where it stopped rather than restarting.
    pub fn pause_output(&mut self) {
        self.paused = true;
        if let Some(player) = self.current_player.as_ref() {
            player.pause();
        }
    }

    pub fn resume_output(&mut self) {
        self.paused = false;
        if let Some(player) = self.current_player.as_ref() {
            player.play();
        }
    }

    /// The music device mixer, shared by streams that must play alongside
    /// the theme output (the shell movie soundtrack).
    pub fn mixer(&self) -> &rodio::mixer::Mixer {
        self._device.mixer()
    }

    pub(crate) fn discard_finished(&mut self) {
        if self.state() == MusicOutputState::Finished {
            self.current_player = None;
        }
    }

    pub fn set_volume(&mut self, volume: f64) {
        self.volume = volume.clamp(0.0, 1.0);
        self.apply_effective_volume();
    }

    /// Glide the stream's own interp to silence: Theme's fade
    /// (`0x004080C0`) and the radar-movie pre-fade (`0x00408390`).
    pub(crate) fn fade_stream_out(&mut self, now_ms: u64) {
        self.own.set_target(0, now_ms);
    }

    /// `StreamPlayers__RestoreVolumeTargets @ 0x004083D0`.
    pub(crate) fn restore_stream_volume(&mut self, now_ms: u64) {
        self.own.set_target(VOLUME_SCALE, now_ms);
    }

    #[cfg(test)]
    pub(crate) fn stream_volume_target(&self) -> i32 {
        self.own.target_value()
    }

    pub(crate) fn tick_stream_volume(&mut self, now_ms: u64) {
        if self.own.tick(now_ms) {
            self.apply_effective_volume();
        }
    }

    /// Gate the primary music output on the application-activation edge.
    ///
    /// gamemd-derived: `WM_ACTIVATEAPP` at `0x007778AC` reaches primary
    /// Stop/restore through `0x00407020`/`0x00407040`; secondary cursors keep
    /// advancing.
    pub fn set_focus_output_active(&mut self, active: bool) {
        self.focus_output_scale = if active { 1.0 } else { 0.0 };
        self.apply_effective_volume();
    }

    fn effective_volume(&self) -> f32 {
        effective_music_volume(self.volume, self.own.value(), self.focus_output_scale)
    }

    fn apply_effective_volume(&self) {
        if let Some(player) = self.current_player.as_ref() {
            player.set_volume(self.effective_volume());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::assets::mix_archive::SharedBytes;

    fn db(amplitude: f32) -> f64 {
        20.0 * f64::from(amplitude).log10()
    }

    /// Default ScoreVolume 0.4 is index 39 of the table at `0x00816380`
    /// (-13.58 dB), not the linear 0.4 (-7.96 dB) of a plain gain.
    #[test]
    fn score_volume_goes_through_the_directsound_table() {
        let default = effective_music_volume(0.4, VOLUME_SCALE, 1.0);
        assert!((db(default) - -13.58).abs() < 0.01, "{}", db(default));
        assert_eq!(effective_music_volume(1.0, VOLUME_SCALE, 1.0), 1.0);
        // The stream's half-way fade joins the linear product before the table.
        let half = effective_music_volume(1.0, VOLUME_SCALE / 2, 1.0);
        assert!((db(half) - -10.0).abs() < 0.01, "{}", db(half));
        assert_eq!(effective_music_volume(0.4, VOLUME_SCALE, 0.0), 0.0);
    }

    /// One `+0x10` interp: a new file starts at full after a radar pre-fade
    /// (`0x00407E7D`), and a Theme fade on it reports `Ramping` until silent.
    #[test]
    fn a_new_file_resets_the_interp_every_fade_shares() {
        let (mut player, _output) = MusicPlayer::offline_for_test();
        player.fade_stream_out(0);
        player.tick_stream_volume(1_001);
        assert_eq!(player.own.value(), 0);
        assert!(player.submit(PreparedTrack::decoded(
            "Silence".into(),
            vec![0.0; 22_050 * 2 * 10],
            22_050,
        )));
        assert_eq!(player.own.value(), VOLUME_SCALE);
        assert_eq!(player.state(), MusicOutputState::Playing);
        player.fade_stream_out(1_001);
        assert_eq!(player.state(), MusicOutputState::Ramping);
        player.tick_stream_volume(1_501);
        assert_eq!(player.own.value(), VOLUME_SCALE / 2);
        player.tick_stream_volume(2_002);
        assert_eq!(player.state(), MusicOutputState::Playing);
    }

    /// `state` reads `Finished` from `Player::empty`, which rodio's `Done`
    /// sets on the pull after a source's last sample. A streamed song must
    /// reach it then, not at a decode boundary before.
    #[test]
    fn a_streamed_song_empties_its_player_after_the_last_sample() {
        // 16-bit stereo PCM over two runs: 4096 bytes, then a two-frame tail.
        let pcm: Vec<u8> = (0..4_104u32).map(|i| (i % 251) as u8).collect();
        let mut file =
            b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x02\0\x22\x56\0\0\0\0\0\0\x04\0\x10\0data"
                .to_vec();
        file.extend((pcm.len() as u32).to_le_bytes());
        file.extend(&pcm);
        let file = SharedBytes::from(Arc::<[u8]>::from(file));
        let track = PreparedTrack::streamed("Song", &file).expect("PCM song streams");
        let (player, mut output) = Player::new();
        player.append(MusicStream {
            track,
            sample_rate: NonZero::new(22_050).unwrap(),
        });
        for _ in 0..pcm.len() / 2 {
            assert!(!player.empty());
            output.next();
        }
        assert!(
            !player.empty(),
            "the last sample is out, the end not yet read"
        );
        output.next();
        assert!(player.empty());
    }
}
