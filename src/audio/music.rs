//! Optional rodio music output.
//!
//! Theme catalog and lifecycle ownership live in `audio::theme`. This module
//! owns only the physical device/player plus independent gain projections.

use std::num::NonZero;

use rodio::buffer::SamplesBuffer;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};

use crate::audio::theme::{MusicOutputState, PreparedTrack};

/// Engine default for the score (music) volume.
pub const DEFAULT_SCORE_VOLUME: f64 = 0.4;

fn effective_music_volume(
    user_volume: f64,
    lifecycle_scale: f64,
    theme_scale: f64,
    focus_output_scale: f64,
) -> f32 {
    (user_volume * lifecycle_scale * theme_scale * focus_output_scale) as f32
}

pub struct MusicPlayer {
    /// rodio mixer device sink — must be kept alive or all audio stops.
    _device: MixerDeviceSink,
    current_player: Option<Player>,
    volume: f64,
    output_scale: f64,
    theme_scale: f64,
    focus_output_scale: f64,
}

impl MusicPlayer {
    /// Create a physical music output. Theme construction never depends on
    /// this succeeding.
    pub fn new() -> Option<Self> {
        let device = DeviceSinkBuilder::open_default_sink()
            .map_err(|error| log::error!("Failed to initialize music audio: {error}"))
            .ok()?;
        Some(Self {
            _device: device,
            current_player: None,
            volume: DEFAULT_SCORE_VOLUME,
            output_scale: 1.0,
            theme_scale: 1.0,
            focus_output_scale: 1.0,
        })
    }

    pub(crate) fn state(&self) -> MusicOutputState {
        match self.current_player.as_ref() {
            Some(player) if player.empty() => MusicOutputState::Finished,
            Some(_) => MusicOutputState::Playing,
            None => MusicOutputState::Idle,
        }
    }

    pub(crate) fn submit(&mut self, prepared: PreparedTrack) -> bool {
        let Some(channels) = NonZero::new(2u16) else {
            return false;
        };
        let Some(rate) = NonZero::new(prepared.sample_rate) else {
            return false;
        };
        if let Some(player) = self.current_player.take() {
            player.stop();
        }
        let source = SamplesBuffer::new(channels, rate, prepared.samples);
        let player = Player::connect_new(self._device.mixer());
        player.set_volume(self.effective_volume());
        player.append(source);
        log::info!("Playing prepared music track: {}", prepared.stem);
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
        if let Some(player) = self.current_player.as_ref() {
            player.pause();
        }
    }

    pub fn resume_output(&mut self) {
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

    pub fn set_output_scale(&mut self, scale: f64) {
        self.output_scale = scale.clamp(0.0, 1.0);
        self.apply_effective_volume();
    }

    pub(crate) fn set_theme_scale(&mut self, scale: f64) {
        self.theme_scale = scale.clamp(0.0, 1.0);
        self.apply_effective_volume();
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
        effective_music_volume(
            self.volume,
            self.output_scale,
            self.theme_scale,
            self.focus_output_scale,
        )
    }

    fn apply_effective_volume(&self) {
        if let Some(player) = self.current_player.as_ref() {
            player.set_volume(self.effective_volume());
        }
    }

    pub fn volume(&self) -> f64 {
        self.volume
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gsi_01_02_music_focus_gate_composes_with_lifecycle_and_theme_gains() {
        let audible = effective_music_volume(0.8, 0.5, 0.25, 1.0);
        let inactive = effective_music_volume(0.8, 0.5, 0.25, 0.0);
        let restored = effective_music_volume(0.8, 0.5, 0.25, 1.0);
        assert!((audible - 0.1).abs() < f32::EPSILON);
        assert_eq!(inactive, 0.0);
        assert_eq!(restored, audible);
        assert_eq!(effective_music_volume(0.8, 0.75, 0.4, 0.0), 0.0);
        assert!((effective_music_volume(0.8, 0.75, 0.4, 1.0) - 0.24).abs() < f32::EPSILON);
    }
}
