//! Active-YR launcher Options parent projection and transaction ordering.
//!
//! The retained `RetailOptionsProfile` stays authoritative. This module owns
//! the projection into the dialog, the live preview's effects and the
//! accepted parent transaction; UI drawing, concrete audio, persistence I/O
//! and child routing remain with their established owners.

use super::audio::{AudioVolume, apply_volume, set_volume};
use crate::app::App;
use crate::app::PlatformState;
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::ui::main_menu_dialogs::options::{
    LauncherCheckboxId as C, LauncherOptionsEvent, LauncherOptionsLabels, LauncherOptionsPacked,
    LauncherOptionsValues, LauncherParentResult, LauncherTrackbarId as S, OptionsDialogState, ROWS,
    Setting, admitted_initial_position, admitted_volume_position,
};

/// The profile's value behind a box. Full Screen and the FPS readout are the
/// window's and the renderer's live state, which no profile stores.
fn stored_box(profile: &mut RetailOptionsProfile, id: C) -> Option<&mut bool> {
    Some(match id {
        C::FullScreen | C::PerformanceStats => return None,
        C::Tooltips => &mut profile.tooltips,
        C::TargetLines => &mut profile.unit_action_lines,
        C::ShowHidden => &mut profile.show_hidden,
        C::OriginalShape => &mut profile.original_shape,
        C::YrOriginalStyle => &mut profile.yr_original_style,
        C::ShiftQueue => &mut profile.shift_queue,
        C::AutoRepair => &mut profile.auto_repair,
        #[cfg(target_os = "android")]
        C::Ra2Mode => &mut profile.ra2_mode,
        C::MouseWheelZoom => &mut profile.mouse_wheel_zoom,
        C::RightClickOrders => &mut profile.right_click_orders,
        C::MinimapBeacons => &mut profile.minimap_beacons,
        C::LockCursor => &mut profile.lock_cursor,
        C::VeraIcon => &mut profile.vera_icon,
        C::HdRadar => &mut profile.hd_radar,
        C::SmoothSidebar => &mut profile.smooth_sidebar,
        C::SmoothOnlineMenus => &mut profile.smooth_online_menus,
        C::VSync => &mut profile.vsync,
        C::HighRefreshRateInBattle => &mut profile.high_refresh_rate_in_battle,
        C::HdMapPreviews => &mut profile.hd_map_previews,
        C::SkipIntro => &mut profile.skip_intro,
        C::ShowStringKeys => &mut profile.show_string_keys,
        C::CustomEndGameBanner => &mut profile.custom_end_game_banner,
        C::CustomEndGameScore => &mut profile.custom_end_game_score,
        C::CustomEndGameMovie => &mut profile.custom_end_game_movie,
        C::CommanderFeedback => &mut profile.commander_feedback,
        C::DetailedEvaText => &mut profile.detailed_eva_text,
        C::ConvenienceHelp => &mut profile.convenience_help,
        C::BuildingProductionQueue => &mut profile.building_production_queue,
        C::CompactSuperTimers => &mut profile.compact_super_timers,
        C::GroupedSuperTimers => &mut profile.grouped_super_timers,
    })
}

/// Project the retained signed profile into the fresh native-shaped controls.
///
/// gamemd-derived: launcher primary-proc setup at
/// `0x0055FDB0..0x0056047A`. Trackbar set-position rejects an out-of-range
/// request and leaves the fresh position at zero; it does not clamp. VERA's
/// Full Screen and FPS boxes start unticked; the opener sets them from the
/// window and the readout.
pub(crate) fn launcher_values_from_profile(
    profile: &RetailOptionsProfile,
) -> LauncherOptionsValues {
    let mut values = LauncherOptionsValues {
        sidebar: profile.sidebar,
        sidebar_columns: profile.sidebar_columns,
        health_bars: profile.health_bars,
        ares: profile.ares,
        kratos: profile.kratos,
        graphics_api: profile.graphics_api,
        color_vision: profile.color_vision,
        // Without an HD art pack the list shows Off whatever was saved.
        hd_look: crate::ui::main_menu_dialogs::options::HdLook::from_on(
            profile.hd_art_on && crate::render::hd_art::pack_available(),
        ),
        hd_look_available: crate::render::hd_art::pack_available(),
        ..Default::default()
    };
    let mut stored = profile.clone();
    for row in ROWS {
        if let Setting::Checkbox(id) = row.setting {
            values[id] = stored_box(&mut stored, id).is_some_and(|on| *on);
        }
    }
    // Off at the left end, then 2 at the first notch.
    values[S::Detail] = u8::from(profile.detail_level != 0);
    values[S::Difficulty] = admitted_initial_position(i64::from(profile.difficulty), 2);
    values[S::Scroll] = admitted_initial_position(6_i64 - i64::from(profile.scroll_rate), 6);
    // The slider's position is the stored level less the retail strength,
    // so gamemd's own bands sit at the left end.
    values[S::HouseColorStrength] = profile
        .house_color_strength
        .saturating_sub(crate::rules::house_colors::STRENGTH_RETAIL);
    values[S::Score] = admitted_volume_position(profile.score_volume);
    values[S::Sound] = admitted_volume_position(profile.sound_volume);
    values[S::Voice] = admitted_volume_position(profile.voice_volume);
    values[S::GameSpeed] = 0;
    values
}

/// Build one fresh primary snapshot from already-owned platform and text facts.
/// Child return and initial open both use this same constructor path.
/// The Full Screen box shows `full_screen`, the window's own state: the
/// player can also change it with the window's title-bar button. The FPS
/// Counter box shows `performance_stats`, the readout's live state.
pub(crate) fn launcher_dialog_from_profile(
    profile: &RetailOptionsProfile,
    labels: LauncherOptionsLabels,
    full_screen: bool,
    performance_stats: bool,
    launcher_audio_available: bool,
    displays: Vec<crate::ui::main_menu_dialogs::options::DisplayChoice>,
) -> OptionsDialogState {
    let mut values = launcher_values_from_profile(profile);
    values[C::FullScreen] = full_screen;
    values[C::PerformanceStats] = performance_stats;
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
/// VERA box take effect at once: they store the profile value that the next
/// frame, cameo press, radar click, battle, match result or launch reads (and
/// Options' accept writes out); the boxes below also apply here to the
/// window, the renderer or the rules. The audio predicate is queried again in
/// [`apply_volume`] so a forged or stale audio event stores nothing.
pub(crate) fn dispatch_launcher_preview_event(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &mut ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
    event: LauncherOptionsEvent,
) {
    use LauncherOptionsEvent as E;
    let profile = &mut persistence.options_profile;
    match event {
        E::ZoomSelected(zoom) => {
            profile.zoom = zoom;
            App::sync_zoom_pass(platform, renderer, profile);
        }
        E::SidebarSelected(size) => profile.sidebar = size,
        E::SidebarColumnsSelected(columns) => profile.sidebar_columns = columns,
        E::HealthBarModeSelected(mode) => profile.health_bars = mode,
        // Read at start, as the mod is: this launch keeps the setting it
        // opened its files with.
        E::AresSelected(setting) => profile.ares = setting,
        E::KratosSelected(setting) => profile.kratos = setting,
        // Read at the next start (`startup_backend`): this launch keeps the
        // API it draws through.
        E::GraphicsApiSelected(api) => profile.graphics_api = api,
        // The final-frame pass reads the profile every frame; no atlas or
        // ramp has to be rebuilt.
        E::ColorVisionModeSelected(mode) => profile.color_vision = mode,
        // The frame reads the switch every frame.
        E::HdLookSelected(look) => profile.hd_art_on = look.is_on(),
        // Applied at once, like Full Screen: the window moves to the picked
        // screen (or back to the system's choice) before the page is accepted.
        E::DisplaySelected(display) => {
            profile.display = display.clone();
            let displays = crate::app::display_target::current_displays(&platform.window);
            crate::app::display_target::apply(&platform.window, &displays, &display);
        }
        // The open Options page reads its captions again once the events
        // have run.
        E::LanguageSelected(tag) => {
            if crate::app::shell_main_menu::use_language(
                frontend,
                match_state,
                process_assets,
                &tag,
            ) {
                persistence.options_profile.language = Some(tag);
            }
        }
        E::SetWindowSizeRequested => App::restore_launch_window(platform),
        // The in-game menu opens these dialogs, and starts the layout edit
        // mode, itself (`app::input::pause_menu`).
        E::InGameKeyboardRequested | E::InGameSoundRequested | E::EditLayoutRequested => {}
        // Retail's three boxes apply with the accepted pack and publish
        // nothing. The custom end-game stages are read when a match ends, so
        // a match on screen keeps what it started with; the clip layer reads
        // Commander Clips every pass.
        E::Toggled(id, on) => {
            if let Some(stored) = stored_box(profile, id) {
                *stored = on;
            }
            apply_box(
                platform,
                renderer,
                frontend,
                match_state,
                process_assets,
                persistence,
                id,
                on,
            );
        }
        // Baked into a battle's colour tables when it loads, like the sprite
        // atlas that reads them.
        E::HouseColorStrengthChanged(strength) => profile.house_color_strength = strength,
        E::ScorePreview(volume) => apply_volume(
            frontend,
            match_state,
            process_assets,
            audio,
            &mut persistence.options_profile,
            AudioVolume::Score,
            volume,
            true,
        ),
        E::SoundPreview(volume) => apply_volume(
            frontend,
            match_state,
            process_assets,
            audio,
            &mut persistence.options_profile,
            AudioVolume::Sound,
            volume,
            true,
        ),
        E::VoicePreview(volume) => apply_volume(
            frontend,
            match_state,
            process_assets,
            audio,
            &mut persistence.options_profile,
            AudioVolume::Voice,
            volume,
            true,
        ),
    }
}

/// A VERA box's change beyond the profile value it stores.
fn apply_box(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &mut ProcessAssets,
    persistence: &PersistenceState,
    id: C,
    on: bool,
) {
    match id {
        C::FullScreen => App::set_full_screen(platform, &persistence.options_profile, on),
        C::OriginalShape => App::sync_zoom_pass(platform, renderer, &persistence.options_profile),
        // The menu movie's session identity carries the choice, so its next
        // paint loads the selected game copy; the splash follows next launch.
        // The shell atlas carries the same profile value; it is rebuilt (or
        // built) now, before another shell screen can paint.
        C::YrOriginalStyle => {
            let _ = App::ensure_skirmish_shell_chrome(
                renderer,
                frontend,
                process_assets,
                &persistence.options_profile,
            );
        }
        C::MouseWheelZoom if !on => crate::app::input::camera::reset_view_distance(
            platform,
            renderer,
            &frontend.screen,
            match_state,
            persistence,
        ),
        C::LockCursor => {
            platform.cursor_confined = on;
            crate::app::input::cursor::apply_cursor_confinement(platform);
        }
        C::VeraIcon => show_vera_icon(platform, &persistence.options_profile, on),
        C::VSync => {
            App::sync_present_mode(platform, &mut renderer.gpu, &persistence.options_profile)
        }
        // Live state only; no profile stores it, so each launch starts hidden.
        C::PerformanceStats => renderer.performance_overlay.set_visible(on),
        // The box arms the hotkey but leaves the texts showing, so ticking it
        // never swaps the page under the pointer. The loaded table and the
        // language catalog both answer with keys once the hotkey asks; map
        // titles follow, as on a language change.
        C::ShowStringKeys => {
            crate::i18n::set_show_string_keys(on);
            crate::app::shell_main_menu::refresh_string_key_labels(
                frontend,
                match_state,
                process_assets,
            );
        }
        // A rule switch, as Terrain Deformation is: the next offline
        // scenario's rules read it, so a running match keeps what it started
        // with and every peer, save and replay plays the same rules.
        C::BuildingProductionQueue => process_assets.set_building_production_queue(on),
        _ => {}
    }
}

/// Shown at once, once the profile holds it. Unticked in an install without
/// the red star, the window keeps VERA's icon until the next launch, which
/// starts with the program's own.
fn show_vera_icon(
    platform: &crate::app::PlatformState,
    profile: &RetailOptionsProfile,
    vera_icon: bool,
) {
    // Android's launcher icon is a compiled resource carried by the two
    // activity aliases, and changing it closes the app, so the choice has to
    // be on disk first. Every other platform applies the icon to the live
    // window below and keeps its own write timing.
    #[cfg(target_os = "android")]
    {
        crate::app::persistence::options::persist_options_profile(platform, profile);
        crate::util::app_icon::apply(vera_icon);
    }
    #[cfg(not(target_os = "android"))]
    let _ = profile;
    if platform.window.is_visible() == Some(false) {
        return;
    }
    let ra2_dir = platform
        .game_config
        .as_ref()
        .map(|config| config.paths.ra2_dir.as_path());
    if let Some(icon) = crate::app::window_icon::WindowIcon::chosen(vera_icon, ra2_dir) {
        icon.show_on(&platform.window);
    }
}

/// The value to write: `now`, unless the in-game page opened with it (`opened`).
fn moved<T: PartialEq>(opened: Option<T>, now: T) -> Option<T> {
    (opened.as_ref() != Some(&now)).then_some(now)
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
    platform: &crate::app::PlatformState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    audio: &mut crate::app::audio_runtime::AppAudioRuntime,
    profile: &mut RetailOptionsProfile,
    packed: LauncherOptionsPacked,
    opened: Option<LauncherOptionsPacked>,
) {
    if let Some(detail) = moved(opened.map(|o| o.detail_level), packed.detail_level) {
        let changed = profile.detail_level != detail;
        profile.detail_level = detail;
        let presentation = &mut match_state.match_presentation;
        presentation.in_game_options.detail_level = detail.max(0) as u32;
        // Display4AE450 resamples synchronously, before the next Logic visit
        // can copy a Cell's light into a grapple anim. This local operation
        // never enters Simulation's replay/sync hash.
        if changed {
            if let Some(runtime) = match_state.sim_runtime.runtime()
                && let Some(terrain) = runtime.simulation.resolved_terrain()
            {
                let detail = presentation.in_game_options.detail_level;
                presentation
                    .lighting
                    .set_detail_level(terrain, &runtime.simulation, detail);
            }
            platform.window.request_redraw();
        }
    }
    let presentation = &mut match_state.match_presentation;
    if let Some(difficulty) = moved(opened.map(|o| o.difficulty), packed.difficulty) {
        profile.difficulty = difficulty;
    }
    if let Some(lines) = moved(
        opened.map(|o| o.unit_action_lines),
        packed.unit_action_lines,
    ) {
        profile.unit_action_lines = lines;
        presentation.in_game_options.unit_action_lines = lines;
        presentation
            .target_lines
            .set_unit_action_lines_enabled(lines);
    }
    if let Some(hidden) = moved(opened.map(|o| o.show_hidden), packed.show_hidden) {
        profile.show_hidden = hidden;
        presentation.in_game_options.show_hidden = hidden;
    }
    if let Some(tooltips) = moved(opened.map(|o| o.tooltips), packed.tooltips) {
        profile.tooltips = tooltips;
        presentation.in_game_options.tooltips = tooltips;
        presentation.tooltips.set_enabled(tooltips);
    }
    if let Some(rate) = moved(opened.map(|o| o.scroll_rate), packed.scroll_rate) {
        profile.scroll_rate = rate;
        presentation.in_game_options.scroll_rate = rate.max(0) as u32;
    }
    if let Some(volume) = moved(opened.map(|o| o.score_volume), packed.score_volume) {
        set_volume(audio, profile, AudioVolume::Score, volume);
        if volume == 0.0 {
            let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
            audio.queue_then_stop_score_zero(now_ms);
        }
    }
    if let Some(volume) = moved(opened.map(|o| o.sound_volume), packed.sound_volume) {
        set_volume(audio, profile, AudioVolume::Sound, volume);
    }
    if let Some(volume) = moved(opened.map(|o| o.voice_volume), packed.voice_volume) {
        set_volume(audio, profile, AudioVolume::Voice, volume);
    }
}

/// Consume one completed parent pass: Pack -> Apply -> physical dialog drop ->
/// result-specific continuation. See [`apply_launcher_parent`] for `opened`.
///
/// gamemd-derived: `OptionsClass__ShowLauncherDialog @ 0x0055FC80` always
/// applies and destroys the primary before child preparation/route or the one
/// final write. Keyboard then rebuilds a fresh primary without writing.
pub(crate) fn dispatch_launcher_parent_result(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
    dialog: OptionsDialogState,
    result: LauncherParentResult,
    opened: Option<LauncherOptionsPacked>,
) {
    apply_launcher_parent(
        platform,
        match_state,
        audio,
        &mut persistence.options_profile,
        dialog.pack(),
        opened,
    );
    drop(dialog);
    match result {
        LauncherParentResult::Back | LauncherParentResult::Terminal => {
            super::persist_options_profile(platform, &persistence.options_profile);
        }
        LauncherParentResult::Keyboard => {
            use crate::ui::shell::keyboard::KeyboardParent;
            crate::app::input::keyboard::open(
                platform,
                renderer,
                frontend,
                match_state,
                process_assets,
                audio,
                persistence,
                KeyboardParent::Launcher,
            );
            // Native caller blocks in A3. Our asynchronous child resumes this
            // continuation when it closes (after its teardown slide).
            if frontend.keyboard_dialog.is_none() {
                App::open_launcher_options_dialog(
                    platform,
                    renderer,
                    frontend,
                    process_assets,
                    audio,
                    &persistence.options_profile,
                );
            }
        }
        // VERA's own. Written first: Use This Mod leaves the game from the
        // Mods page, whose Back builds the parent again.
        LauncherParentResult::Mods => {
            super::persist_options_profile(platform, &persistence.options_profile);
            App::open_mods_page(platform.game_config.as_ref(), &renderer.gpu, frontend);
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
        assert_eq!(moved(Some(opened.detail_level), opened.detail_level), None);
        assert_eq!(
            moved(Some(opened.tooltips), !opened.tooltips),
            Some(!opened.tooltips)
        );
        assert_eq!(moved(None, opened.detail_level), Some(2));
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
                launcher_values_from_profile(&profile)[S::Difficulty],
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
                launcher_values_from_profile(&profile)[S::Scroll],
                expected,
                "ScrollRate={input}"
            );
        }
        for input in [i32::MIN, -2, -1, 1, 2, i32::MAX] {
            let profile = RetailOptionsProfile {
                detail_level: input,
                ..RetailOptionsProfile::default()
            };
            assert_eq!(launcher_values_from_profile(&profile)[S::Detail], 1);
        }
        assert_eq!(
            launcher_values_from_profile(&RetailOptionsProfile {
                detail_level: 0,
                ..RetailOptionsProfile::default()
            })[S::Detail],
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
            assert_eq!(values[S::Score], expected, "volume={input:?}");
            assert_eq!(values[S::Sound], expected, "volume={input:?}");
            assert_eq!(values[S::Voice], expected, "volume={input:?}");
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
        assert!(!booleans[C::Tooltips]);
        assert!(!booleans[C::TargetLines]);
        assert!(booleans[C::ShowHidden]);
        assert!(booleans[C::YrOriginalStyle]);
        assert!(booleans[C::SkipIntro]);
        assert!(booleans[C::MouseWheelZoom]);
        assert!(booleans[C::ShiftQueue]);
    }
}
