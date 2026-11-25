//! Shared Options audio setters: native5FA4A0/5FA510/5FA590.
//! D5 previews and active B8 use profile -> output -> optional beep ordering.

use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::process_assets::ProcessAssets;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioVolume {
    Score,
    Sound,
    Voice,
}

/// Live events require the common audio gate. Accepted Back setters remain
/// unconditional and cue-silent (55FAA0 and6B68C3..6B69B5).
pub(crate) fn apply_volume(
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    profile: &mut RetailOptionsProfile,
    channel: AudioVolume,
    volume: f32,
    preview: bool,
) {
    if preview && !audio.launcher_audio_available {
        return;
    }
    set_volume(audio, profile, channel, volume);
    let beep = match channel {
        AudioVolume::Score => None,
        AudioVolume::Sound => Some(1.0),
        AudioVolume::Voice => Some(volume),
    };
    if preview && let Some(local_multiplier) = beep {
        play_generic_beep(
            frontend,
            match_state,
            process_assets,
            audio,
            local_multiplier,
        );
    }
}

/// Store a channel's volume in the profile, then set its output from it.
pub(crate) fn set_volume(
    audio: &mut AppAudioRuntime,
    profile: &mut RetailOptionsProfile,
    channel: AudioVolume,
    volume: f32,
) {
    match channel {
        AudioVolume::Score => {
            profile.set_score_volume(f64::from(volume));
            // The preceding store owns the upper-only clamp for both consumers.
            audio.set_score_volume(profile.score_volume);
        }
        AudioVolume::Sound => {
            profile.sound_volume = volume;
            audio.set_sound_volume(f64::from(volume));
        }
        AudioVolume::Voice => {
            profile.voice_volume = volume;
            audio.set_voice_volume(f64::from(volume));
        }
    }
}

fn play_generic_beep(
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    local_multiplier: f32,
) {
    let sound = crate::app::state::active_rules(frontend, match_state)
        .and_then(|rules| rules.general.generic_beep_sound.clone());
    let Some(sound) = sound else {
        return;
    };
    let Some(sfx) = audio.sfx_player.as_mut() else {
        return;
    };
    let Some(catalog) = process_assets.audio_catalog() else {
        return;
    };
    sfx.play_sound_with_volume(&sound, local_multiplier, catalog.sounds());
}
