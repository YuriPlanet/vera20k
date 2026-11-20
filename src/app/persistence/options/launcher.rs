//! Active-YR launcher Options parent projection and transaction ordering.
//!
//! The retained `RetailOptionsProfile` stays authoritative. This module owns
//! the pure projection, the live preview's effects and a narrow operation seam
//! for the accepted parent boundary; UI drawing, concrete audio, persistence
//! I/O, and child routing remain with their established owners.

use super::audio::{AudioVolume, apply_volume};
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::{App, AppState};
use crate::ui::main_menu_dialogs::options::{
    LauncherOptionsEvent, LauncherOptionsLabels, LauncherOptionsPacked, LauncherOptionsValues,
    LauncherParentResult, OptionsDialogState, admitted_initial_position, admitted_volume_position,
};

/// Project the retained signed profile into the fresh native-shaped controls.
///
/// gamemd-derived: launcher primary-proc setup at
/// `0x0055FDB0..0x0056047A`. Trackbar set-position rejects an out-of-range
/// request and leaves the fresh position at zero; it does not clamp. VERA's
/// Full Screen box starts unticked; the opener sets it from the window. Its
/// Original 4:3 Shape and Original YR Style boxes and Sidebar list show the
/// retained choices.
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
        yr_original_style: profile.yr_original_style,
        mouse_wheel_zoom: profile.mouse_wheel_zoom,
        right_click_orders: profile.right_click_orders,
        lock_cursor: profile.lock_cursor,
        vera_icon: profile.vera_icon,
        hd_radar: profile.hd_radar,
        smooth_sidebar: profile.smooth_sidebar,
        smooth_online_menus: profile.smooth_online_menus,
        vsync: profile.vsync,
        hd_map_previews: profile.hd_map_previews,
        shift_queue: profile.shift_queue,
        auto_repair: profile.auto_repair,
        minimap_beacons: profile.minimap_beacons,
        // Off at the left end, then 2 at the first notch.
        performance_stats: false,
        show_string_keys: profile.show_string_keys,
        skip_intro: profile.skip_intro,
        custom_end_game_banner: profile.custom_end_game_banner,
        custom_end_game_score: profile.custom_end_game_score,
        custom_end_game_movie: profile.custom_end_game_movie,
        commander_feedback: profile.commander_feedback,
        detailed_eva_text: profile.detailed_eva_text,
        convenience_help: profile.convenience_help,
        building_production_queue: profile.building_production_queue,
        compact_super_timers: profile.compact_super_timers,
        sidebar: profile.sidebar,
        sidebar_columns: profile.sidebar_columns,
        health_bars: profile.health_bars,
        ares: profile.ares,
        color_vision: profile.color_vision,
        // The slider's position is the stored level less the retail strength,
        // so gamemd's own bands sit at the left end.
        house_color_position: profile
            .house_color_strength
            .saturating_sub(crate::rules::house_colors::STRENGTH_RETAIL),
        score_position: admitted_volume_position(profile.score_volume),
        sound_position: admitted_volume_position(profile.sound_volume),
        voice_position: admitted_volume_position(profile.voice_volume),
        game_speed_position: 0,
    }
}

/// Build one fresh primary snapshot from already-owned platform and text facts.
/// Child return and initial open both use this same constructor path.
/// The Full Screen box shows `full_screen`, the window's own state: the
/// player can also change it with the window's title-bar button. The FPS
/// Counter box shows `performance_stats`, the readout's live state, which no profile
/// stores.
pub(crate) fn launcher_dialog_from_profile(
    profile: &RetailOptionsProfile,
    labels: LauncherOptionsLabels,
    full_screen: bool,
    performance_stats: bool,
    launcher_audio_available: bool,
    displays: Vec<crate::ui::main_menu_dialogs::options::DisplayChoice>,
) -> OptionsDialogState {
    let values = LauncherOptionsValues {
        full_screen,
        performance_stats,
        ..launcher_values_from_profile(profile)
    };
    OptionsDialogState::new(
        labels,
        values,
        profile.zoom,
        launcher_audio_available,
        displays,
        profile.display.clone(),
    )
}

/// Dispatch one already-ordered UI event without an INI write. The three audio
/// paths preserve native profile -> output -> cue order. A list's pick and a
/// VERA box take effect at once: most store the profile value that the next
/// frame, cameo press, radar click, battle, match result or launch reads (and
/// Options' accept writes out); the rest also apply here to the window, the
/// renderer or the rules. The audio predicate is queried again in
/// [`apply_volume`] so a forged or stale audio event stores nothing.
pub(crate) fn dispatch_launcher_preview_event(state: &mut AppState, event: LauncherOptionsEvent) {
    use LauncherOptionsEvent as E;
    let profile = &mut state.persistence.options_profile;
    match event {
        E::Cue(cue) => App::play_launcher_options_cue(state, cue),
        E::ZoomSelected(zoom) => {
            profile.zoom = zoom;
            App::sync_zoom_pass(state);
        }
        E::OriginalShapeChanged(original_shape) => {
            profile.original_shape = original_shape;
            App::sync_zoom_pass(state);
        }
        E::SidebarSelected(size) => profile.sidebar = size,
        E::SidebarColumnsSelected(columns) => profile.sidebar_columns = columns,
        E::HealthBarModeSelected(mode) => profile.health_bars = mode,
        // Read at start, as the mod is: this launch keeps the setting it
        // opened its files with.
        E::AresSelected(setting) => profile.ares = setting,
        // The final-frame pass reads the profile every frame; no atlas or
        // ramp has to be rebuilt.
        E::ColorVisionModeSelected(mode) => profile.color_vision = mode,
        // Applied at once, like Full Screen: the window moves to the picked
        // screen (or back to the system's choice) before the page is accepted.
        E::DisplaySelected(display) => {
            profile.display = display.clone();
            let displays = crate::app::display_target::current_displays(&state.platform.window);
            crate::app::display_target::apply(&state.platform.window, &displays, &display);
        }
        // The open Options page reads its captions again once the events
        // have run.
        E::LanguageSelected(tag) => {
            if state.use_language(&tag) {
                state.persistence.options_profile.language = Some(tag);
            }
        }
        E::FullScreenChanged(full_screen) => App::set_full_screen(state, full_screen),
        E::SetWindowSizeRequested => App::restore_launch_window(state),
        // The in-game menu opens these dialogs, and starts the layout edit
        // mode, itself (`app::input::pause_menu`).
        E::InGameKeyboardRequested | E::InGameSoundRequested | E::EditLayoutRequested => {}
        // The menu movie's session identity carries the choice, so its next
        // paint loads the selected game copy; the splash follows next launch.
        // The shell atlas carries the same profile value; it is rebuilt (or
        // built) now, before another shell screen can paint.
        E::YrOriginalStyleChanged(yr_original_style) => {
            profile.yr_original_style = yr_original_style;
            let _ = App::ensure_skirmish_shell_chrome(state);
        }
        E::MouseWheelZoomChanged(enabled) => {
            profile.mouse_wheel_zoom = enabled;
            if !enabled {
                crate::app::input::camera::reset_view_distance(state);
            }
        }
        E::RightClickOrdersChanged(enabled) => profile.right_click_orders = enabled,
        E::LockCursorChanged(locked) => {
            profile.lock_cursor = locked;
            state.platform.cursor_confined = locked;
            crate::app::input::cursor::apply_cursor_confinement(state);
        }
        E::VeraIconChanged(vera_icon) => store_vera_icon(state, vera_icon),
        E::HdRadarChanged(enabled) => profile.hd_radar = enabled,
        E::SmoothSidebarChanged(enabled) => profile.smooth_sidebar = enabled,
        E::SmoothOnlineMenusChanged(enabled) => profile.smooth_online_menus = enabled,
        E::VSyncChanged(vsync) => {
            profile.vsync = vsync;
            App::sync_present_mode(state);
        }
        E::HdMapPreviewsChanged(enabled) => profile.hd_map_previews = enabled,
        E::ShiftQueueChanged(shift_queue) => profile.shift_queue = shift_queue,
        E::AutoRepairChanged(enabled) => profile.auto_repair = enabled,
        E::MinimapBeaconsChanged(enabled) => profile.minimap_beacons = enabled,
        // Live state only; no profile stores it, so each launch starts hidden.
        E::PerformanceStatsChanged(shown) => state.renderer.performance_overlay.set_visible(shown),
        // The box arms the hotkey but leaves the texts showing, so ticking it
        // never swaps the page under the pointer. The loaded table and the
        // language catalog both answer with keys once the hotkey asks; map
        // titles follow, as on a language change.
        E::ShowStringKeysChanged(shown) => {
            profile.show_string_keys = shown;
            crate::i18n::set_show_string_keys(shown);
            state.refresh_string_key_labels();
        }
        E::SkipIntroChanged(skip) => profile.skip_intro = skip,
        // Read when a match result is admitted or a match ends, so a match
        // already on screen keeps what it started with, and a teardown already
        // waiting for a clip keeps waiting.
        E::CustomEndGameBannerChanged(enabled) => profile.custom_end_game_banner = enabled,
        E::CustomEndGameScoreChanged(enabled) => profile.custom_end_game_score = enabled,
        E::CustomEndGameMovieChanged(enabled) => profile.custom_end_game_movie = enabled,
        // The clip layer's next pass reads it, so a match mutes or unmutes
        // its commanders at once; muted, it still spends what it sees.
        E::CommanderFeedbackChanged(enabled) => profile.commander_feedback = enabled,
        E::DetailedEvaTextChanged(enabled) => profile.detailed_eva_text = enabled,
        E::ConvenienceHelpChanged(enabled) => profile.convenience_help = enabled,
        // A rule switch, as Terrain Deformation is: the next offline
        // scenario's rules read it, so a running match keeps what it started
        // with and every peer, save and replay plays the same rules.
        E::BuildingProductionQueueChanged(enabled) => {
            profile.building_production_queue = enabled;
            state.process_assets.set_building_production_queue(enabled);
        }
        E::CompactSuperTimersChanged(enabled) => profile.compact_super_timers = enabled,
        // Baked into a battle's colour tables when it loads, like the sprite
        // atlas that reads them.
        E::HouseColorStrengthChanged(strength) => profile.house_color_strength = strength,
        E::ScorePreview(volume) => apply_volume(state, AudioVolume::Score, volume, true),
        E::SoundPreview(volume) => apply_volume(state, AudioVolume::Sound, volume, true),
        E::VoicePreview(volume) => apply_volume(state, AudioVolume::Voice, volume, true),
    }
}

/// Saved, and shown at once. Unticked in an install without the red star, the
/// window keeps VERA's icon until the next launch, which starts with the
/// program's own.
fn store_vera_icon(state: &mut AppState, vera_icon: bool) {
    state.persistence.options_profile.vera_icon = vera_icon;
    // Android's launcher icon is a compiled resource carried by the two
    // activity aliases, and changing it closes the app, so the choice has to
    // be on disk first. Every other platform applies the icon to the live
    // window below and keeps its own write timing.
    #[cfg(target_os = "android")]
    {
        crate::app::persistence::options::persist_options_profile(state);
        crate::util::app_icon::apply(vera_icon);
    }
    if state.platform.window.is_visible() == Some(false) {
        return;
    }
    let ra2_dir = state.platform.game_config.as_ref().map(|config| config.paths.ra2_dir.as_path());
    if let Some(icon) = crate::app::window_icon::WindowIcon::chosen(vera_icon, ra2_dir) {
        icon.show_on(&state.platform.window);
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
    fn route_keyboard(&mut self);
    fn reopen_parent(&mut self);
    /// VERA's Mods page (`App::open_mods_page`); its Back builds the parent
    /// again.
    fn route_mods(&mut self);
}

/// Apply the packed parent subset in the literal native order.
///
/// gamemd-derived: `OptionsClass__ApplyFromLauncherDialog @ 0x0055FAA0`.
/// Accepted audio setters are unconditional and cue-silent. Detail refresh is
/// changed-only; action-line refresh is unconditional.
///
/// `opened` is VERA's in-game menu's: what its page showed when it opened, so
/// that only a control the player moved writes. Unlike gamemd's main menu,
/// which applies once per visit, that page closes every time the menu does,
/// and Game Controls' three-step Detail would keep turning its Medium to High.
fn apply_launcher_parent(
    operations: &mut impl LauncherParentOperations,
    packed: LauncherOptionsPacked,
    opened: Option<LauncherOptionsPacked>,
) {
    let moved = |unchanged: &dyn Fn(LauncherOptionsPacked) -> bool| !opened.is_some_and(unchanged);
    if moved(&|o| o.detail_level == packed.detail_level)
        && operations.store_detail_level(packed.detail_level)
    {
        operations.refresh_detail();
    }
    if moved(&|o| o.difficulty == packed.difficulty) {
        operations.store_difficulty(packed.difficulty);
    }
    if moved(&|o| o.unit_action_lines == packed.unit_action_lines) {
        operations.store_unit_action_lines(packed.unit_action_lines);
        operations.refresh_unit_action_lines();
    }
    if moved(&|o| o.show_hidden == packed.show_hidden) {
        operations.store_show_hidden(packed.show_hidden);
    }
    if moved(&|o| o.tooltips == packed.tooltips) {
        operations.store_tooltips(packed.tooltips);
    }
    if moved(&|o| o.scroll_rate == packed.scroll_rate) {
        operations.store_scroll_rate(packed.scroll_rate);
    }
    if moved(&|o| o.score_volume == packed.score_volume) {
        operations.store_score_volume(packed.score_volume);
        operations.apply_score_output(packed.score_volume);
        if packed.score_volume == 0.0 {
            operations.queue_then_stop_score_zero();
        }
    }
    if moved(&|o| o.sound_volume == packed.sound_volume) {
        operations.store_sound_volume(packed.sound_volume);
        operations.apply_sound_output(packed.sound_volume);
    }
    if moved(&|o| o.voice_volume == packed.voice_volume) {
        operations.store_voice_volume(packed.voice_volume);
        operations.apply_voice_output(packed.voice_volume);
    }
}

/// Consume one completed parent pass: Pack -> Apply -> physical dialog drop ->
/// result-specific continuation. See [`apply_launcher_parent`] for `opened`.
///
/// gamemd-derived: `OptionsClass__ShowLauncherDialog @ 0x0055FC80` always
/// applies and destroys the primary before child preparation/route or the one
/// final write. Keyboard then rebuilds a fresh primary without writing.
pub(crate) fn dispatch_launcher_parent_result(
    operations: &mut impl LauncherParentOperations,
    dialog: OptionsDialogState,
    result: LauncherParentResult,
    opened: Option<LauncherOptionsPacked>,
) {
    let packed = dialog.pack();
    operations.observe_pack(packed);
    apply_launcher_parent(operations, packed, opened);
    drop(dialog);
    operations.primary_dropped();

    match result {
        LauncherParentResult::Back | LauncherParentResult::Terminal => {
            operations.persist_profile();
        }
        LauncherParentResult::Keyboard => {
            operations.route_keyboard();
            operations.reopen_parent();
        }
        // VERA's own. Written first: Use This Mod leaves the game from the
        // Mods page.
        LauncherParentResult::Mods => {
            operations.persist_profile();
            operations.route_mods();
        }
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    /// The Health Bars list starts on the retained profile's mode, so the page
    /// never shows a different strip policy from the one the battle draws.
    #[test]
    fn health_bar_mode_reaches_the_dialog_from_the_profile() {
        use crate::ui::main_menu_dialogs::options::HealthBarMode;
        for mode in [
            HealthBarMode::Vanilla,
            HealthBarMode::Always,
            HealthBarMode::AlwaysExceptFull,
        ] {
            let profile = RetailOptionsProfile {
                health_bars: mode,
                ..RetailOptionsProfile::default()
            };
            assert_eq!(launcher_values_from_profile(&profile).health_bars, mode);
        }
    }

    /// The writes one accepted page makes.
    #[derive(Default)]
    struct WriteRecorder(Vec<&'static str>);

    impl LauncherParentOperations for WriteRecorder {
        fn observe_pack(&mut self, _: LauncherOptionsPacked) {}
        fn store_detail_level(&mut self, _: i32) -> bool {
            self.0.push("detail");
            true
        }
        fn refresh_detail(&mut self) {}
        fn store_difficulty(&mut self, _: i32) {
            self.0.push("difficulty");
        }
        fn store_unit_action_lines(&mut self, _: bool) {
            self.0.push("target lines");
        }
        fn refresh_unit_action_lines(&mut self) {}
        fn store_show_hidden(&mut self, _: bool) {
            self.0.push("show hidden");
        }
        fn store_tooltips(&mut self, _: bool) {
            self.0.push("tooltips");
        }
        fn store_scroll_rate(&mut self, _: i32) {
            self.0.push("scroll rate");
        }
        fn store_score_volume(&mut self, _: f32) {
            self.0.push("music");
        }
        fn apply_score_output(&mut self, _: f32) {}
        fn queue_then_stop_score_zero(&mut self) {
            self.0.push("stop music");
        }
        fn store_sound_volume(&mut self, _: f32) {
            self.0.push("sound");
        }
        fn apply_sound_output(&mut self, _: f32) {}
        fn store_voice_volume(&mut self, _: f32) {
            self.0.push("voice");
        }
        fn apply_voice_output(&mut self, _: f32) {}
        fn primary_dropped(&mut self) {}
        fn persist_profile(&mut self) {}
        fn route_keyboard(&mut self) {}
        fn route_mods(&mut self) {}
        fn reopen_parent(&mut self) {}
    }

    /// Game Controls' Medium Detail shows as High on the page's two-step
    /// slider, so closing the page must not write a slider nobody moved, even
    /// when another control changed. The main menu's accept still writes all.
    #[test]
    fn the_in_game_page_writes_only_what_the_player_moved() {
        let profile = RetailOptionsProfile {
            detail_level: 1,
            ..RetailOptionsProfile::default()
        };
        let opened = launcher_dialog_from_profile(
            &profile,
            LauncherOptionsLabels::resolve(&|_| None),
            false,
            false,
            true,
            Vec::new(),
        )
        .pack();
        assert_eq!(opened.detail_level, 2);

        let mut untouched = WriteRecorder::default();
        apply_launcher_parent(&mut untouched, opened, Some(opened));
        assert!(untouched.0.is_empty());

        let moved = LauncherOptionsPacked {
            tooltips: !opened.tooltips,
            score_volume: 0.0,
            ..opened
        };
        let mut page = WriteRecorder::default();
        apply_launcher_parent(&mut page, moved, Some(opened));
        assert_eq!(page.0, ["tooltips", "music", "stop music"]);

        let mut accept = WriteRecorder::default();
        apply_launcher_parent(&mut accept, opened, None);
        assert_eq!(
            accept.0,
            [
                "detail",
                "difficulty",
                "target lines",
                "show hidden",
                "tooltips",
                "scroll rate",
                "music",
                "sound",
                "voice"
            ]
        );
    }

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
            yr_original_style: true,
            skip_intro: true,
            mouse_wheel_zoom: true,
            shift_queue: true,
            ..RetailOptionsProfile::default()
        });
        assert!(!booleans.tooltips);
        assert!(!booleans.target_lines);
        assert!(booleans.show_hidden);
        assert!(booleans.yr_original_style);
        assert!(booleans.skip_intro);
        assert!(booleans.mouse_wheel_zoom);
        assert!(booleans.shift_queue);
    }
}
