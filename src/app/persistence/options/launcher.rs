//! Active-YR launcher Options parent projection and transaction ordering.
//!
//! The retained `RetailOptionsProfile` stays authoritative. This module owns
//! the pure projection plus narrow operation seams for the live preview and
//! accepted parent boundary; UI drawing, the window, concrete audio,
//! persistence I/O, and child routing remain with their established owners.

use super::audio::{AudioVolume, AudioVolumeOperations, apply_volume};
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::ui::main_menu_dialogs::options::{
    LauncherCue, LauncherOptionsEvent, LauncherOptionsLabels, LauncherOptionsPacked,
    LauncherOptionsValues, LauncherParentResult, OptionsDialogState, SidebarSize, Zoom,
    admitted_initial_position, admitted_volume_position,
};

/// Project the retained signed profile into the fresh native-shaped controls.
///
/// gamemd-derived: launcher primary-proc setup at
/// `0x0055FDB0..0x0056047A`. Trackbar set-position rejects an out-of-range
/// request and leaves the fresh position at zero; it does not clamp. VERA's
/// Full Screen box starts unticked; the opener sets it from the window. Its
/// Original 4:3 Shape and RA2 Loading & Menu Movie boxes and Sidebar list show
/// the retained choices.
pub(crate) fn launcher_values_from_profile(
    profile: &RetailOptionsProfile,
) -> LauncherOptionsValues {
    LauncherOptionsValues {
        detail_position: u8::from(profile.detail_level != 0),
        difficulty_position: admitted_initial_position(i64::from(profile.difficulty), 2),
        scroll_position: admitted_initial_position(6_i64 - i64::from(profile.scroll_rate), 6),
        tooltips: profile.tooltips,
        target_lines: profile.unit_action_lines,
        show_hidden: profile.show_hidden,
        full_screen: false,
        original_shape: profile.original_shape,
        ra2_look: profile.ra2_look,
        shift_queue: profile.shift_queue,
        wol_test_service: profile.wol_test_service,
        sidebar: profile.sidebar,
        score_position: admitted_volume_position(profile.score_volume),
        sound_position: admitted_volume_position(profile.sound_volume),
        voice_position: admitted_volume_position(profile.voice_volume),
    }
}

/// Build one fresh primary snapshot from already-owned platform and text facts.
/// Child return and initial open both use this same constructor path.
/// The Full Screen box shows `full_screen`, the window's own state: the
/// player can also change it with the window's title-bar button.
pub(crate) fn launcher_dialog_from_profile(
    profile: &RetailOptionsProfile,
    labels: LauncherOptionsLabels,
    full_screen: bool,
    launcher_audio_available: bool,
) -> OptionsDialogState {
    let values = LauncherOptionsValues {
        full_screen,
        ..launcher_values_from_profile(profile)
    };
    OptionsDialogState::new(labels, values, profile.zoom, launcher_audio_available)
}

/// Concrete effects admitted by one launcher UI frame.
///
/// The common audio predicate is deliberately queried again at dispatch so a
/// forged or stale audio event cannot store a profile value or touch output.
pub(crate) trait LauncherPreviewOperations: AudioVolumeOperations {
    fn play_cue(&mut self, cue: LauncherCue);
    fn store_zoom(&mut self, zoom: Zoom);
    fn store_sidebar(&mut self, size: SidebarSize);
    fn set_full_screen(&mut self, full_screen: bool);
    fn store_original_shape(&mut self, original_shape: bool);
    fn store_ra2_look(&mut self, ra2_look: bool);
    fn store_shift_queue(&mut self, shift_queue: bool);
    fn store_wol_test_service(&mut self, wol_test_service: bool);
}

/// Dispatch one already-ordered UI event without an INI write. The three audio
/// paths preserve native profile -> output -> cue order; the five display
/// choices and Shift-Click Batch Queue take effect at once.
pub(crate) fn dispatch_launcher_preview_event(
    operations: &mut impl LauncherPreviewOperations,
    event: LauncherOptionsEvent,
) {
    match event {
        LauncherOptionsEvent::Cue(cue) => operations.play_cue(cue),
        LauncherOptionsEvent::ZoomSelected(zoom) => operations.store_zoom(zoom),
        LauncherOptionsEvent::SidebarSelected(size) => operations.store_sidebar(size),
        LauncherOptionsEvent::FullScreenChanged(full_screen) => {
            operations.set_full_screen(full_screen);
        }
        LauncherOptionsEvent::OriginalShapeChanged(original_shape) => {
            operations.store_original_shape(original_shape);
        }
        LauncherOptionsEvent::Ra2LookChanged(ra2_look) => operations.store_ra2_look(ra2_look),
        LauncherOptionsEvent::ShiftQueueChanged(shift_queue) => {
            operations.store_shift_queue(shift_queue);
        }
        LauncherOptionsEvent::WolTestServiceChanged(wol_test_service) => {
            operations.store_wol_test_service(wol_test_service);
        }
        LauncherOptionsEvent::ScorePreview(volume) => {
            apply_volume(operations, AudioVolume::Score, volume, true)
        }
        LauncherOptionsEvent::SoundPreview(volume) => {
            apply_volume(operations, AudioVolume::Sound, volume, true)
        }
        LauncherOptionsEvent::VoicePreview(volume) => {
            apply_volume(operations, AudioVolume::Voice, volume, true)
        }
    }
}

/// Narrow effect boundary for the unconditional accepted parent transaction.
/// Production and the recorder both enter through
/// [`dispatch_launcher_parent_result`].
pub(crate) trait LauncherParentOperations {
    fn observe_pack(&mut self, packed: LauncherOptionsPacked);
    fn store_detail_level(&mut self, value: i32) -> bool;
    fn refresh_detail(&mut self);
    fn store_difficulty(&mut self, value: i32);
    fn store_unit_action_lines(&mut self, value: bool);
    fn refresh_unit_action_lines(&mut self);
    fn store_show_hidden(&mut self, value: bool);
    fn store_tooltips(&mut self, value: bool);
    fn store_scroll_rate(&mut self, value: i32);
    fn store_score_volume(&mut self, value: f32);
    fn apply_score_output(&mut self, value: f32);
    fn queue_then_stop_score_zero(&mut self);
    fn store_sound_volume(&mut self, value: f32);
    fn apply_sound_output(&mut self, value: f32);
    fn store_voice_volume(&mut self, value: f32);
    fn apply_voice_output(&mut self, value: f32);
    fn primary_dropped(&mut self);
    fn persist_profile(&mut self);
    fn prepare_network_settings(&mut self);
    fn route_network(&mut self);
    fn route_keyboard(&mut self);
    fn reopen_parent(&mut self);
}

/// Apply the packed parent subset in the literal native order.
///
/// gamemd-derived: `OptionsClass__ApplyFromLauncherDialog @ 0x0055FAA0`.
/// Accepted audio setters are unconditional and cue-silent. Detail refresh is
/// changed-only; action-line refresh is unconditional.
fn apply_launcher_parent(
    operations: &mut impl LauncherParentOperations,
    packed: LauncherOptionsPacked,
) {
    let detail_changed = operations.store_detail_level(packed.detail_level);
    if detail_changed {
        operations.refresh_detail();
    }
    operations.store_difficulty(packed.difficulty);
    operations.store_unit_action_lines(packed.unit_action_lines);
    operations.refresh_unit_action_lines();
    operations.store_show_hidden(packed.show_hidden);
    operations.store_tooltips(packed.tooltips);
    operations.store_scroll_rate(packed.scroll_rate);
    operations.store_score_volume(packed.score_volume);
    operations.apply_score_output(packed.score_volume);
    if packed.score_volume == 0.0 {
        operations.queue_then_stop_score_zero();
    }
    operations.store_sound_volume(packed.sound_volume);
    operations.apply_sound_output(packed.sound_volume);
    operations.store_voice_volume(packed.voice_volume);
    operations.apply_voice_output(packed.voice_volume);
}

/// Consume one completed parent pass: Pack -> Apply -> physical dialog drop ->
/// result-specific continuation.
///
/// gamemd-derived: `OptionsClass__ShowLauncherDialog @ 0x0055FC80` always
/// applies and destroys the primary before child preparation/route or the one
/// final write. Network/Keyboard then rebuild a fresh primary without writing.
pub(crate) fn dispatch_launcher_parent_result(
    operations: &mut impl LauncherParentOperations,
    dialog: OptionsDialogState,
    result: LauncherParentResult,
) {
    let packed = dialog.pack();
    operations.observe_pack(packed);
    apply_launcher_parent(operations, packed);
    drop(dialog);
    operations.primary_dropped();

    match result {
        LauncherParentResult::Back | LauncherParentResult::Terminal => {
            operations.persist_profile();
        }
        LauncherParentResult::Network => {
            operations.prepare_network_settings();
            operations.route_network();
            operations.reopen_parent();
        }
        LauncherParentResult::Keyboard => {
            operations.route_keyboard();
            operations.reopen_parent();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_projection_uses_reject_to_zero_edges() {
        let default = launcher_values_from_profile(&RetailOptionsProfile::default());
        assert_eq!(default, LauncherOptionsValues::default());

        for (input, expected) in [(0, 0), (2, 2), (3, 0), (4, 0), (-1, 0)] {
            let profile = RetailOptionsProfile {
                difficulty: input,
                ..RetailOptionsProfile::default()
            };
            assert_eq!(
                launcher_values_from_profile(&profile).difficulty_position,
                expected,
                "Difficulty={input}"
            );
        }
        for (input, expected) in [(-1, 0), (0, 6), (6, 0), (7, 0)] {
            let profile = RetailOptionsProfile {
                scroll_rate: input,
                ..RetailOptionsProfile::default()
            };
            assert_eq!(
                launcher_values_from_profile(&profile).scroll_position,
                expected,
                "ScrollRate={input}"
            );
        }
        for input in [i32::MIN, -2, -1, 1, 2, i32::MAX] {
            let profile = RetailOptionsProfile {
                detail_level: input,
                ..RetailOptionsProfile::default()
            };
            assert_eq!(launcher_values_from_profile(&profile).detail_position, 1);
        }
        assert_eq!(
            launcher_values_from_profile(&RetailOptionsProfile {
                detail_level: 0,
                ..RetailOptionsProfile::default()
            })
            .detail_position,
            0
        );

        for (input, expected) in [
            (-2.0, 0),
            (-0.05, 0),
            (0.0, 0),
            (0.04, 0),
            // Stored f32 0.95 promotes slightly below 0.95, so the native
            // x87 truncation of `v * 10 + 0.5` yields 9, not 10.
            (0.95, 9),
            (1.0, 10),
            (f32::NAN, 0),
            (f32::INFINITY, 0),
        ] {
            let profile = RetailOptionsProfile {
                score_volume: input,
                sound_volume: input,
                voice_volume: input,
                ..RetailOptionsProfile::default()
            };
            let values = launcher_values_from_profile(&profile);
            assert_eq!(values.score_position, expected, "volume={input:?}");
            assert_eq!(values.sound_position, expected, "volume={input:?}");
            assert_eq!(values.voice_position, expected, "volume={input:?}");
        }

        let booleans = launcher_values_from_profile(&RetailOptionsProfile {
            tooltips: false,
            unit_action_lines: false,
            show_hidden: true,
            ra2_look: true,
            shift_queue: true,
            wol_test_service: true,
            ..RetailOptionsProfile::default()
        });
        assert!(!booleans.tooltips);
        assert!(!booleans.target_lines);
        assert!(booleans.show_hidden);
        assert!(booleans.ra2_look);
        assert!(booleans.shift_queue);
        assert!(booleans.wol_test_service);
    }
}
