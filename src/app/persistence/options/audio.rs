//! Shared Options audio setters: native5FA4A0/5FA510/5FA590.
//! D5 previews and active B8 use profile -> output -> optional beep ordering.

use crate::app::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioVolume {
    Score,
    Sound,
    Voice,
}

pub(crate) trait AudioVolumeOperations {
    fn launcher_audio_available(&self) -> bool;
    fn store_score_volume(&mut self, volume: f32);
    fn apply_score_output(&mut self, volume: f32);
    fn store_sound_volume(&mut self, volume: f32);
    fn apply_sound_output(&mut self, volume: f32);
    fn store_voice_volume(&mut self, volume: f32);
    fn apply_voice_output(&mut self, volume: f32);
    fn play_generic_beep(&mut self, local_multiplier: f32);
}

/// Live events require the common audio gate. Accepted Back setters remain
/// unconditional and cue-silent (55FAA0 and6B68C3..6B69B5).
pub(crate) fn apply_volume(
    operations: &mut impl AudioVolumeOperations,
    channel: AudioVolume,
    volume: f32,
    preview: bool,
) {
    if preview && !operations.launcher_audio_available() {
        return;
    }
    match channel {
        AudioVolume::Score => {
            operations.store_score_volume(volume);
            operations.apply_score_output(volume);
        }
        AudioVolume::Sound => {
            operations.store_sound_volume(volume);
            operations.apply_sound_output(volume);
            if preview {
                operations.play_generic_beep(1.0);
            }
        }
        AudioVolume::Voice => {
            operations.store_voice_volume(volume);
            operations.apply_voice_output(volume);
            if preview {
                operations.play_generic_beep(volume);
            }
        }
    }
}

impl AudioVolumeOperations for AppState {
    fn launcher_audio_available(&self) -> bool {
        self.audio.launcher_audio_available
    }
    fn store_score_volume(&mut self, volume: f32) {
        self.persistence.options_profile.score_volume = volume;
    }
    fn apply_score_output(&mut self, volume: f32) {
        if let Some(player) = self.audio.music_player.as_mut() {
            player.set_volume(f64::from(volume));
        }
    }
    fn store_sound_volume(&mut self, volume: f32) {
        self.persistence.options_profile.sound_volume = volume;
    }
    fn apply_sound_output(&mut self, volume: f32) {
        if let Some(player) = self.audio.sfx_player.as_mut() {
            player.set_sound_volume(f64::from(volume));
        }
    }
    fn store_voice_volume(&mut self, volume: f32) {
        self.persistence.options_profile.voice_volume = volume;
    }
    fn apply_voice_output(&mut self, volume: f32) {
        if let Some(player) = self.audio.sfx_player.as_mut() {
            player.set_voice_volume(f64::from(volume));
        }
    }
    fn play_generic_beep(&mut self, local_multiplier: f32) {
        let sound = self
            .rules()
            .and_then(|rules| rules.general.generic_beep_sound.clone());
        let Some(sound) = sound else {
            return;
        };
        let Some(sfx) = self.audio.sfx_player.as_mut() else {
            return;
        };
        let Some(catalog) = self.process_assets.audio_catalog() else {
            return;
        };
        sfx.play_sound_with_volume(&sound, local_multiplier, catalog.sounds());
    }
}
