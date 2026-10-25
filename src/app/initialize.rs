//! Process, window, GPU, frontend, and initial app-state construction.

use std::path::Path;

use anyhow::Context;

use crate::app::frontend::startup_options::{RetailStartupOptions, ScreenSize};
use crate::app::persistence::options_profile::{RetailOptionsLoad, RetailOptionsProfile};

use super::{
    ActiveEventLoop, App, AppState, Arc, AssetManager, BTreeMap, BasicSection, BatchRenderer,
    BitFont, GameConfig, GameScreen, GpuContext, HashMap, HouseRoster, Instant, MusicPlayer,
    PhysicalSize, PlatformState, RandomMapGenerationRetention, Result, SfxPlayer,
    SidebarChromeLayoutSpec, SidebarTab, StartupAudioDisposition, Window, WindowAttributes,
    frontend::startup_splash,
};
use crate::map::scenario_sources;

/// Main-thread startup slices own the not-yet-installed app resources. No
/// window/GPU or process authority is lent to a worker or duplicated.
pub(super) enum InitializationStep {
    Pending(InitializationJob),
    Ready(Box<AppState>, Vec<winit::event::WindowEvent>),
}

pub(super) struct InitializationJob {
    window: Arc<Window>,
    resume: Box<dyn FnOnce() -> Result<InitializationStep>>,
    deferred_window_events: Vec<winit::event::WindowEvent>,
    resume_admitted: bool,
}

impl InitializationStep {
    /// Hidden captures keep their sealed, synchronous startup. Interactive
    /// windows return to winit between these same completed operations.
    pub(super) fn finish_synchronously(mut self) -> Result<Self> {
        loop {
            match self {
                Self::Pending(job) => self = job.advance()?,
                ready => return Ok(ready),
            }
        }
    }

    fn pending(
        window: Arc<Window>,
        resume: impl FnOnce() -> Result<Self> + 'static,
    ) -> Result<Self> {
        Ok(Self::Pending(InitializationJob {
            window,
            resume: Box::new(resume),
            deferred_window_events: Vec::new(),
            resume_admitted: false,
        }))
    }
}

impl InitializationJob {
    pub(super) fn admit_event_loop_turn(&mut self) {
        self.resume_admitted = true;
    }

    pub(super) fn may_resume(&self) -> bool {
        self.resume_admitted
    }

    pub(super) fn defer_window_event(&mut self, event: winit::event::WindowEvent) {
        self.deferred_window_events.push(event);
    }

    pub(super) fn window_id(&self) -> winit::window::WindowId {
        self.window.id()
    }
    pub(super) fn advance(self) -> Result<InitializationStep> {
        let step = (self.resume)()?;
        Ok(match step {
            InitializationStep::Pending(mut next) => {
                next.deferred_window_events = self.deferred_window_events;
                InitializationStep::Pending(next)
            }
            InitializationStep::Ready(state, mut events) => {
                let mut deferred = self.deferred_window_events;
                deferred.append(&mut events);
                InitializationStep::Ready(state, deferred)
            }
        })
    }
}

fn startup_window_projection(
    profile_screen: ScreenSize,
    capture_dimensions: Option<(u32, u32)>,
) -> (u32, u32, bool) {
    capture_dimensions.map_or(
        (profile_screen.width, profile_screen.height, true),
        |(width, height)| (width, height, false),
    )
}

/// Keep the first actionable failure while still constructing the error-screen state.
fn startup_asset_or_error<T>(result: Result<T>, error: &mut Option<String>) -> Option<T> {
    match result {
        Ok(asset) => Some(asset),
        Err(err) => {
            // The log keeps the raw chain, which carries the host's localized
            // OS text; the card gets an English description instead, because
            // its in-repo face cannot draw a localised sentence.
            log::warn!("{err:#}");
            error.get_or_insert_with(|| describe_startup_error(&err));
            None
        }
    }
}

/// English description of a startup failure, for the error card.
///
/// `std::io::Error`'s own message is localized by the host, so on a Chinese
/// Windows the raw chain reads a localized sentence and the card could only
/// draw boxes. This walks the `anyhow` chain and states each link in English:
/// our own context strings already are, and I/O kinds are mapped here.
fn describe_startup_error(err: &anyhow::Error) -> String {
    use crate::assets::error::AssetError;

    let mut parts: Vec<String> = Vec::new();
    for cause in err.chain() {
        let part = if let Some(asset) = cause.downcast_ref::<AssetError>() {
            match asset {
                AssetError::Io(io) => describe_io_error(io),
                other => other.to_string(),
            }
        } else if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            describe_io_error(io)
        } else {
            cause.to_string()
        };
        // The same I/O failure arrives through several context layers; say it
        // once.
        if parts.last() != Some(&part) {
            parts.push(part);
        }
    }
    parts.join(": ")
}

/// One I/O failure in English, with the numeric code kept for the log search.
fn describe_io_error(err: &std::io::Error) -> String {
    use std::io::ErrorKind;

    let text = match err.kind() {
        ErrorKind::NotFound => "the file or path does not exist",
        ErrorKind::PermissionDenied => "access was denied",
        ErrorKind::AlreadyExists => "the target already exists",
        ErrorKind::InvalidData => "the file is not a valid game archive",
        ErrorKind::InvalidInput => "the file name or data is invalid",
        ErrorKind::UnexpectedEof => "the file is truncated",
        ErrorKind::WouldBlock => "the file is in use by another program",
        _ => "the file could not be read",
    };
    match err.raw_os_error() {
        Some(code) => format!("{text} (os error {code})"),
        None => text.to_owned(),
    }
}

fn select_startup_options_load<LoadProfile>(
    capture_dimensions: Option<(u32, u32)>,
    startup_options: &RetailStartupOptions,
    ra2_dir: Option<&Path>,
    load_profile: LoadProfile,
) -> RetailOptionsLoad
where
    LoadProfile: FnOnce(&Path, &RetailStartupOptions) -> RetailOptionsLoad,
{
    if capture_dimensions.is_some() {
        RetailOptionsLoad::without_settings(&RetailStartupOptions::default())
    } else if let Some(ra2_dir) = ra2_dir {
        load_profile(ra2_dir, startup_options)
    } else {
        RetailOptionsLoad::without_settings(startup_options)
    }
}

trait StartupAudioProfileOperations {
    fn set_sound_volume(&mut self, volume: f64);
    fn set_voice_volume(&mut self, volume: f64);
    fn set_score_volume(&mut self, volume: f32);
    fn set_score_repeat_shuffle(&mut self, repeat: bool, shuffle: bool);
}

impl StartupAudioProfileOperations for crate::app::audio_runtime::AppAudioRuntime {
    fn set_sound_volume(&mut self, volume: f64) {
        crate::app::audio_runtime::AppAudioRuntime::set_sound_volume(self, volume);
    }

    fn set_voice_volume(&mut self, volume: f64) {
        crate::app::audio_runtime::AppAudioRuntime::set_voice_volume(self, volume);
    }

    fn set_score_volume(&mut self, volume: f32) {
        crate::app::audio_runtime::AppAudioRuntime::set_score_volume(self, volume);
    }

    fn set_score_repeat_shuffle(&mut self, repeat: bool, shuffle: bool) {
        self.theme.set_score_options(repeat, shuffle);
    }
}

fn apply_startup_audio_profile(
    profile: &RetailOptionsProfile,
    operations: &mut impl StartupAudioProfileOperations,
) {
    // gamemd-derived: the Audio tail of `OptionsClass__ReadFromINI @ 0x005FA620`
    // applies SoundVolume, VoiceVolume, then ScoreVolume through distinct owners,
    // then writes IsScoreRepeat / IsScoreShuffle straight into `g_Theme+0x10` /
    // `+0x12` (`0x005FAB1C` / `0x005FAB5B`).
    operations.set_sound_volume(f64::from(profile.sound_volume));
    operations.set_voice_volume(f64::from(profile.voice_volume));
    operations.set_score_volume(profile.score_volume);
    operations.set_score_repeat_shuffle(profile.is_score_repeat, profile.is_score_shuffle);
}

impl App {
    fn load_version_txt() -> String {
        crate::util::version::retail_internal_version().to_owned()
    }

    /// Create the window and GPU presentation owners. Does NOT load a map —
    /// starts in MainMenu state. Map loading is deferred to when the user
    /// clicks "Quick Play".
    pub(super) fn initialize(
        event_loop: &ActiveEventLoop,
        capture_dimensions: Option<(u32, u32)>,
        startup_options: RetailStartupOptions,
        external_launch: bool,
        replay_retail_directory: Option<&Path>,
    ) -> Result<InitializationStep> {
        // One parsed retail profile snapshot yields two ordered products: the
        // independent frontend size selected after the early Video read, and the later
        // full-read profile retained by persistence. Capture is a sealed
        // automation lane, so it uses exact defaults and its explicit
        // dimensions instead of ingesting operator argv/profile screen state.
        let mut main_menu_shell_error = None;
        let mut personal_rules = crate::rules::personal_rules::PersonalRules::default();
        let game_config = startup_asset_or_error(
            GameConfig::load().and_then(|config| {
                let config = match replay_retail_directory {
                    Some(directory) => config.with_retail_directory(directory.to_owned())?,
                    None => config,
                };
                if capture_dimensions.is_none() {
                    crate::app::persistence::profile_file::import_legacy(&config.paths).map_err(
                        |error| anyhow::Error::new(error).context("Could not prepare VERA.INI"),
                    )?;
                    let bytes = crate::app::persistence::rules_profile::load_or_create(
                        config.paths.settings_dir(),
                    )
                    .context("Could not prepare VERARULES.INI")?;
                    personal_rules =
                        crate::rules::personal_rules::PersonalRules::from_bytes(&bytes)
                            .context("Could not read VERARULES.INI")?;
                }
                Ok(config)
            }),
            &mut main_menu_shell_error,
        );
        let options_load = select_startup_options_load(
            capture_dimensions,
            &startup_options,
            game_config
                .as_ref()
                .map(|config| config.paths.settings_dir()),
            RetailOptionsLoad::from_settings,
        );
        let profile_screen = options_load.startup_shell_screen;
        let mut options_profile = options_load.retained_profile;
        options_profile.render_interpolation =
            crate::app::presentation::interpolation::startup_enabled(
                options_profile.render_interpolation,
                std::env::var("VERA20K_RENDER_INTERPOLATION")
                    .ok()
                    .as_deref(),
            );
        let (window_width, window_height, window_visible) =
            startup_window_projection(profile_screen, capture_dimensions);
        let startup_audio =
            StartupAudioDisposition::for_audio_enabled(startup_options.audio_enabled);
        // The player's window has a title bar and any size they drag it to; it
        // opens at the frontend size in logical units, so a high-DPI display
        // shows it as large as a standard one. The menus fit whatever it gets
        // and the match zoom follows it. A sealed capture keeps its exact,
        // fixed, hidden physical surface.
        let capture = capture_dimensions.is_some();
        let inner_size: winit::dpi::Size = if capture {
            PhysicalSize::new(window_width, window_height).into()
        } else {
            winit::dpi::LogicalSize::new(window_width, window_height).into()
        };
        // The saved Full Screen choice goes in at creation: macOS retries it
        // only then, when the first attempt fails (a launch from an app that
        // is itself full screen). The resize that ends the animation re-fits
        // the surface. The saved screen is resolved here too, so full screen
        // covers it and a windowed launch opens on it; a screen that is no
        // longer attached leaves both to the system.
        let displays = if capture {
            Vec::new()
        } else {
            crate::app::display_target::current_displays_for(event_loop)
        };
        let wanted_display = super::display_target::screen_for(&displays, &options_profile.display);
        let fullscreen = (!capture && options_profile.full_screen)
            .then(|| super::display_target::fullscreen_for(&displays, &options_profile.display));
        let mut window_attrs: WindowAttributes = WindowAttributes::default()
            .with_title(super::WINDOW_TITLE)
            .with_inner_size(inner_size)
            .with_resizable(!capture)
            .with_fullscreen(fullscreen)
            .with_visible(window_visible)
            .with_active(window_visible);
        // The Wayland app ID and X11 class, which the AppImage's desktop entry
        // names (StartupWMClass), so docks show its icon for the window.
        // Unset, Wayland has none and X11 takes the name the program was
        // started by: an AppImage's own file name, version included.
        #[cfg(target_os = "linux")]
        {
            use winit::platform::wayland::WindowAttributesExtWayland as _;
            window_attrs = window_attrs.with_name("vera20k", "vera20k");
        }
        // macOS full screen hides the menu bar and Dock instead of revealing
        // them at the edges the player scrolls against, where the system would
        // draw its own cursor over the game's. winit applies this to its own
        // set_fullscreen only, not to full screen from the title-bar button.
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::WindowAttributesExtMacOS;
            window_attrs = window_attrs.with_borderless_game(true);
        }
        // Capture windows stay hidden, so only a visible session shows the icon.
        let retail_icon = game_config
            .as_ref()
            .filter(|_| window_visible)
            .and_then(|config| super::window_icon::RetailIcon::load(&config.paths.ra2_dir));
        if let Some(icon) = &retail_icon {
            window_attrs = icon.apply(window_attrs);
        }
        let window: Arc<Window> = Arc::new(event_loop.create_window(window_attrs)?);
        // A windowed launch on a saved screen: the window opens centred there.
        // Full screen already covers it from the attributes above.
        if let Some(monitor) = wanted_display
            .as_ref()
            .filter(|_| !options_profile.full_screen)
        {
            window.set_outer_position(super::display_target::centered_outer_position(
                &window, monitor,
            ));
        }
        // gamemd's WinMain builds WWMouse (`0x006BDF1C`), whose constructor hides
        // the cursor, before the startup splash; from the first frame after it,
        // `show_os_cursor_only_without_game_cursor` owns the cursor.
        window.set_cursor_visible(false);
        #[cfg(target_os = "macos")]
        if let Some(icon) = &retail_icon {
            icon.set_dock_icon();
        }
        let gpu: GpuContext =
            GpuContext::new(window.clone(), event_loop.owned_display_handle(), capture)?;
        let batch_renderer: BatchRenderer = BatchRenderer::new(&gpu);
        let status_screen_fill = batch_renderer.create_texture(&gpu, &[255; 4], 1, 1);
        let status_card_glow =
            crate::app::frontend::status_screen::build_button_glow(&gpu, &batch_renderer);
        let mut terrain_draw_renderer = crate::render::terrain_draw::TerrainDrawRenderer::new(
            &gpu.device,
            &gpu.queue,
            gpu.surface_format,
            &batch_renderer,
        );
        terrain_draw_renderer.generate_ion_blast_frames(&gpu.device, &gpu.queue);
        let combat_light_renderer = crate::render::combat_light::CombatLightRenderer::new(&gpu);
        let mut bit_font = BitFont::fallback_5x7(&gpu.device, &gpu.queue, &batch_renderer);
        let fps_overlay =
            crate::app::diagnostics::fps_overlay::FpsOverlay::new(&gpu, &batch_renderer);
        let depth_view: wgpu::TextureView = gpu.create_depth_texture();
        let shell_surface_presenter =
            crate::render::shell_surface_present::ShellSurfacePresenter::new(&gpu)?;
        let input_delay_ticks: u64 = game_config
            .as_ref()
            .map(|cfg| u64::from(cfg.gameplay.input_delay_ticks))
            .unwrap_or_else(|| u64::from(crate::util::config::default_input_delay_ticks()));
        // Ordinary gamemd chrome is one physical render pixel per asset pixel.
        // 6A5090/6A5130 change row capacity on resize, never artwork scale.
        let ui_scale = 1.0;
        let sidebar_layout_spec = SidebarChromeLayoutSpec::stock();
        let mut startup_asset_manager = game_config.as_ref().and_then(|config| {
            startup_asset_or_error(
                AssetManager::new(&config.paths.ra2_dir, startup_options.media_archive_mode)
                    .and_then(|assets| assets.with_ini_roots(&config.ini))
                    .context("Could not load the game archives"),
                &mut main_menu_shell_error,
            )
        });
        // Native process startup seeds Scenario before the MPModes loader. The
        // Cooperative factory reached by that loader then advances this cursor
        // before the first shell is shown.
        let mut frontend_seed_clock = crate::app::match_bootstrap::OrdinaryMatchSeedClock;
        let frontend_seed = crate::app::match_bootstrap::read_match_seed(&mut frontend_seed_clock);
        // The splash goes up as soon as the archives are mounted and the two
        // things it draws with are available: native presents it immediately
        // after the mix mount and lets the rules/type initialization run under
        // the artwork, padding out the remaining hold only if that work
        // finished early. The string-table load has to stay ahead of the
        // present. A missing or corrupt required CSF belongs to the startup
        // error screen, so it must not abort AppState construction or show the splash.
        //
        // What makes the move safe is that the archive stack is identical at
        // both positions: the only registration that changes it sits after the
        // splash in the old ordering as well, so first-winner resolution for
        // the splash palette and SHP cannot differ. (The steps that moved below
        // do share the asset manager mutably in effect — its mix cache is
        // interior-mutable behind a lock — but caching a lookup does not change
        // which archive wins it.)
        let startup_csf = startup_asset_manager.as_ref().and_then(|assets| {
            startup_asset_or_error(
                crate::app::loading::init::load_csf(assets),
                &mut main_menu_shell_error,
            )
        });
        let startup_fnt = startup_asset_manager.as_ref().and_then(|assets| {
            assets.get_ref("GAME.FNT").and_then(|data| {
                crate::assets::fnt_file::FntFile::from_bytes(data)
                    .map_err(|err| log::warn!("Failed to parse startup GAME.FNT: {err}"))
                    .ok()
            })
        });
        if let Some(fnt) = startup_fnt.as_ref() {
            bit_font = BitFont::from_fnt(&gpu.device, &gpu.queue, &batch_renderer, fnt);
        }
        let outcome_banner_art = startup_asset_manager.as_ref().and_then(|assets| {
            crate::app::presentation::outcome_banner::OutcomeBannerArt::build(
                &gpu,
                &batch_renderer,
                assets,
            )
        });
        // A client-selected game enters scenario loading directly. This splash
        // is presented synchronously below, before start_spawn_session runs;
        // clearing it in that later callback cannot hide the first frame.
        let mut startup_splash = if !external_launch
            && capture_dimensions.is_none()
            && main_menu_shell_error.is_none()
        {
            startup_asset_manager
                .as_ref()
                .zip(startup_fnt.as_ref())
                .and_then(|(assets, fnt)| {
                    startup_splash::StartupSplashPresentation::build(
                        &gpu,
                        &batch_renderer,
                        assets,
                        startup_csf.as_ref(),
                        fnt,
                        gpu.config.width,
                        gpu.config.height,
                        options_profile.yr_original_style,
                    )
                    .map_err(|err| log::warn!("Could not build retail startup splash: {err:#}"))
                    .ok()
                })
        } else {
            None
        };
        if let Some(splash) = startup_splash.as_mut() {
            // Surface acquisition can be transient before the first event-loop
            // redraw. A splash that did not reach the screen stays unarmed for
            // the frame loop to retry.
            match startup_splash::render_and_present(
                &gpu,
                &batch_renderer,
                &shell_surface_presenter,
                &depth_view,
                &fps_overlay,
                splash,
            ) {
                Ok(true) => splash.mark_presented(Instant::now()),
                Ok(false) => {
                    log::warn!("Initial retail startup splash present deferred: no surface frame");
                }
                Err(err) => {
                    log::warn!("Initial retail startup splash present deferred: {err:#}");
                }
            }
        }
        if external_launch {
            log::info!("External launch: process-start splash skipped");
        }
        let quickplay_allowed = replay_retail_directory.is_none();
        InitializationStep::pending(window.clone(), move || {
            // Ordinary interactive startup runs with the splash already on screen: the
            // presented swapchain frame stays composited while this thread blocks,
            // and the hold armed above is measured from that present, so a slow
            // load is spent inside the five seconds instead of before them.
            let mut process_assets =
                crate::app::process_assets::ProcessAssets::new_with_ini_config(
                    startup_options.media_archive_mode,
                    startup_audio.load_audio_indices,
                    game_config
                        .as_ref()
                        .map(|config| config.ini.clone())
                        .unwrap_or_default(),
                )
                .with_personal_rules(personal_rules);
            process_assets.csf = startup_csf;
            let (startup_rules, startup_rules_projection) = startup_asset_manager
                .as_ref()
                .and_then(|assets| {
                    process_assets
                        .initialize_sources_if_needed(assets)
                        .map_err(|error| log::warn!("Startup sources unavailable: {error}"))
                        .ok()
                })
                .unwrap_or((None, None));
            // Return to the host only after the process Rules registry is complete.
            InitializationStep::pending(window.clone(), move || {
                if let Some(assets) = startup_asset_manager.as_mut() {
                    match assets.register_neutral_archives() {
                        Ok(true) => {
                            log::info!("Registered retail neutral shell archives");
                        }
                        Ok(false) => {
                            log::warn!(
                                "Retail neutral shell archives are unavailable; shell presentation may fall back"
                            );
                        }
                        Err(err) => {
                            log::warn!("Could not register retail neutral shell archives: {err:#}");
                        }
                    }
                }
                let skirmish_shell_chrome = None;
                let main_menu_shell_chrome = startup_asset_manager.as_ref().and_then(|assets| {
                    startup_asset_or_error(
                        crate::render::main_menu_shell_chrome::build_main_menu_shell_chrome_atlas(
                            &gpu,
                            &batch_renderer,
                            assets,
                        ),
                        &mut main_menu_shell_error,
                    )
                });
                let version_txt = Self::load_version_txt();
                let skirmish_scenario_records =
                    match (startup_asset_manager.as_mut(), game_config.as_ref()) {
                        (Some(assets), Some(config)) => {
                            scenario_sources::list_skirmish_scenario_records_with_assets(
                                &config.paths.ra2_dir,
                                assets,
                                process_assets.csf.as_ref(),
                            )
                        }
                        _ => Ok(Vec::new()),
                    }
                    .unwrap_or_else(|err| {
                        log::warn!("Could not list Skirmish scenario records: {err:#}");
                        Vec::new()
                    });
                let skirmish_scenario_records = if skirmish_scenario_records.is_empty() {
                    scenario_sources::list_available_maps()
                .unwrap_or_else(|err| {
                    log::warn!("Could not list fallback maps: {:#}", err);
                    Vec::new()
                })
                .iter()
                .enumerate()
                .map(|(idx, map)| {
                    crate::map::skirmish_scenarios::SkirmishScenarioRecord::from_map_menu_entry(
                        idx, map,
                    )
                })
                .collect()
                } else {
                    skirmish_scenario_records
                };
                // F11: the catalog owns the records and derives the shell-map
                // projection internally; nothing re-projects by hand anymore.
                let scenario_catalog = crate::app::scenario_catalog::ScenarioCatalog::from_records(
                    skirmish_scenario_records,
                );
                let skirmish_modes = startup_asset_manager
                    .as_ref()
                    .and_then(|assets| {
                        match crate::skirmish_modes::skirmish_modes_from_assets(assets) {
                            Ok(modes) => Some(modes),
                            Err(err) => {
                                log::warn!("Could not load Skirmish mode roster: {err}");
                                None
                            }
                        }
                    })
                    .unwrap_or_default();
                if let Some(assets) = startup_asset_manager.as_ref() {
                    process_assets.install_native_ini_hashes(assets, &skirmish_modes);
                }
                let mut skirmish_shell_state =
                    crate::ui::skirmish_shell::SkirmishShellState::default();
                // Seed the Credits/Unit Count slider ranges from rulesmd's
                // [MultiplayerDialogSettings] so a mod that changes the money/unit bounds
                // shifts the slider extents like gamemd does (it reads them from Rules at
                // dialog-build time); without assets we keep the stock-default ranges.
                if let Some(rules_projection) = startup_rules_projection.as_ref() {
                    skirmish_shell_state.trackbar_bounds =
                crate::ui::skirmish_shell::SkirmishTrackbarBounds::from_multiplayer_dialog_settings(
                    rules_projection,
                );
                    // Seed the per-match option values (Money/UnitCount/TechLevel/
                    // GameSpeed and the checkbox toggles) from the merged rules
                    // [MultiplayerDialogSettings], so a mod that changes a default opens
                    // the dialog on — and launches the match with — its value. Without
                    // assets we keep the stock-default values.
                    let dialog_options =
                        crate::sim::game_options::GameOptions::from_multiplayer_dialog_settings(
                            rules_projection,
                        );
                    skirmish_shell_state.apply_multiplayer_dialog_values(&dialog_options);
                }
                let skirmish_defaults =
                    crate::app::frontend::skirmish_session::skirmish_global_defaults(
                        &skirmish_shell_state,
                    );
                let multiplayer_shell_runtime =
                    crate::app::frontend::skirmish_session::MultiplayerShellRuntime::initialize(
                        frontend_seed.value,
                        &mut process_assets,
                        game_config
                            .as_ref()
                            .map(|config| config.paths.settings_dir()),
                        startup_asset_manager.as_ref(),
                        startup_rules.as_ref(),
                        skirmish_defaults,
                    );
                multiplayer_shell_runtime.hydrate_shell(
                    &mut skirmish_shell_state,
                    scenario_catalog.shell_maps(),
                    &skirmish_modes,
                );
                // Pre-fill the player-name field from the persistent profile name when
                // configured, mirroring the original seeding the field from a profile
                // source rather than always showing a fixed default.
                if let Some(profile_name) = game_config
                    .as_ref()
                    .and_then(|config| config.profile.player_name())
                {
                    skirmish_shell_state.player_name_edit =
                        crate::ui::skirmish_shell::PlayerNameEditState::with_name(profile_name);
                }
                crate::ui::skirmish_shell::repair_teams_for_selected_mode(
                    &mut skirmish_shell_state,
                    &skirmish_modes,
                );
                crate::ui::skirmish_shell::initialize_rows_for_selected_map(
                    &mut skirmish_shell_state,
                    scenario_catalog.shell_maps(),
                );

                // Build the software cursor at startup so the main menu draws the SHP
                // arrow and hides the OS cursor, matching the original which hides the
                // OS cursor for the whole process and blits the cursor SHP every frame.
                let startup_software_cursor = startup_asset_manager.as_ref().and_then(|assets| {
                    crate::render::cursor_atlas::build_software_cursor(
                        &gpu,
                        &batch_renderer,
                        assets,
                    )
                });
                let hotkey_bindings = game_config.as_ref().map_or_else(
                    || crate::app::input::hotkeys::HotkeyBindings::from_ini_bytes(None),
                    |config| {
                        crate::app::persistence::keyboard::startup(
                            config.paths.settings_dir(),
                            startup_asset_manager
                                .as_ref()
                                .and_then(|assets| assets.get_archive_ref("KEYBOARDMD.INI")),
                        )
                    },
                );
                let startup_in_game_options =
                    crate::app::persistence::options::in_game_options_from_profile(
                        &options_profile,
                    );
                let mut startup_tooltips = crate::ui::tooltips::TooltipService::new();
                let mut startup_target_lines =
                    crate::app::presentation::target_lines::TargetLineState::default();
                crate::app::persistence::options::apply_presentation_option_gates(
                    &mut startup_target_lines,
                    &mut startup_tooltips,
                    &startup_in_game_options,
                );

                let music_player = startup_audio
                    .initialize_music_output
                    .then(MusicPlayer::new)
                    .flatten();
                let sfx_player = startup_audio
                    .initialize_sfx_output
                    .then(SfxPlayer::new)
                    .flatten();
                let launcher_audio_available =
                    crate::app::audio_runtime::derive_launcher_audio_available(
                        startup_options.audio_enabled,
                        music_player.is_some(),
                        sfx_player.is_some(),
                    );
                let mut startup_audio_runtime = crate::app::audio_runtime::AppAudioRuntime {
                    theme: crate::audio::theme::ThemeRuntime::default(),
                    service_clock: crate::audio::arbiter::AudioServiceClock::default(),
                    music_player,
                    sfx_player,
                    launcher_audio_available,
                    theme_startup_suppressed: false,
                    master_groups: Default::default(),
                };
                if let Some(assets) = startup_asset_manager.as_ref() {
                    startup_audio_runtime.initialize_theme(assets);
                }

                if let Some(assets) = startup_asset_manager {
                    process_assets.return_from_loading(assets);
                }
                // Phobos `[Phobos]` client switches (RA2MD.INI, imported into
                // VERA.INI). The `phobos` build is the capability; the profile keys are
                // the user's switches. Sealed capture keeps the source defaults, like
                // the retail options snapshot, so operator profile state cannot change
                // captured output.
                #[cfg(feature = "phobos")]
                let phobos_client = match game_config.as_ref() {
                    Some(config) if capture_dimensions.is_none() => {
                        crate::app::persistence::phobos_settings::read(config.paths.settings_dir())
                    }
                    _ => crate::app::persistence::phobos_settings::PhobosClientSettings::default(),
                };
                let mut state = AppState {
            platform: PlatformState::new(
                window,
                game_config,
                capture_dimensions.map(|(w, h)| PhysicalSize::new(w, h)),
            ),
            match_state: crate::app::match_runtime::state::MatchState {
                campaign_names: Default::default(),
                session: Default::default(),
                network_wait: Default::default(),
                network_failure: None,
                live_network: None,
                lan_service: None,
                startup: Default::default(),
                sim_runtime: None,
                input: crate::app::input::state::MatchInputState::new(hotkey_bindings),
                match_presentation: crate::app::presentation::state::MatchPresentationState {
                    interpolation:
                        crate::app::presentation::interpolation::RenderInterpolation::new(
                            options_profile.render_interpolation,
                        ),
                    sinking_waterlines: Default::default(),
                    barrel_image_pitches: Default::default(),
                    building_zshape: None,
                    power_bar_anim: crate::ui::sidebar::PowerBarAnimState::new(),
                    sidebar_gadget_state: crate::ui::sidebar::gadget_flash::SidebarGadgetState::new(
                    ),
                    in_game_gadgets: crate::app::input::gadget_input::InGameGadgets::new(),
                    sidebar_projection: Default::default(),
                    active_sidebar_tab: SidebarTab::default_active_tab(),
                    sidebar_layout_spec,
                    ui_scale,
                    tooltips: startup_tooltips,
                    tooltip_epoch: Instant::now(),
                    message_list: crate::ui::messages::MessageList::new(
                        3,
                        0,
                        crate::ui::messages::MESSAGE_MAX_VISIBLE_RETAIL,
                        0,
                    ),
                    chat: Default::default(),
                    beacons: Default::default(),
                    in_game_menu: crate::ui::pause_menu::InGameMenuState::default(),
                    modal_latch: true,
                    pause_menu_has_saves: false,
                    pause_menu_interaction: Default::default(),
                    pause_menu_template: Default::default(),
                    internet_pause: Default::default(),
                    abort_secondary: Default::default(),
                    sound_dialog: None,
                    abort_buttons: Default::default(),
                    surrender_buttons: Default::default(),
                    diplomacy: Default::default(),
                    restate: Default::default(),
                    restate_text: String::new(),
                    saved_game_browser: None,
                    in_game_options: startup_in_game_options,
                    in_game_options_anchor: None,
                    combat_lights: Default::default(),
                    screen_shake: Default::default(),
                    line_trails: Default::default(),
                    lasers: Default::default(),
                    talk_bubble: Default::default(),
                    electric_bolts: Default::default(),
                    radiation_beams: Default::default(),
                    superweapon_range_pulse: Default::default(),
                    detail: Default::default(),
                    hidden_super_anims: Default::default(),
                    super_timer_blinks: Default::default(),
                    minimap: None,
                    radar_anim: None,
                    radar_movies: Default::default(),
                    chrono_screen: Default::default(),
                    scenario_pause: Default::default(),
                    radar_animation_source: None,
                    radar_content_insets: None,
                    has_radar: false,
                    selection_overlay: None,
                    #[cfg(feature = "phobos")]
                    digital_display_enabled: phobos_client.digital_display_enabled,
                    #[cfg(feature = "phobos")]
                    show_placement_preview: phobos_client.show_placement_preview,
                    #[cfg(feature = "phobos")]
                    placement_preview_art: Default::default(),
                    beacon_art: None,
                    shroud_buffer: None,
                    theater_name: "TEMPERATE".to_string(),
                    theater_ext: "tem".to_string(),
                    target_lines: startup_target_lines,
                    cached_cell_body_instances: Vec::new(),
                    terrain_grid: None,
                    overlays: Default::default(),
                    terrain_objects: Vec::new(),
                    overlay_names: BTreeMap::new(),
                    overlay_radar_colors: HashMap::new(),
                    house_color_map: HashMap::new(),
                    house_roster: HouseRoster::default(),
                    lighting: Default::default(),
                    tile_atlas: None,
                    unit_atlas: None,
                    palette_set: None,
                    sprite_atlas: None,
                    overlay_atlas: None,
                    bridge_atlas: None,
                    sidebar_cameo_atlas: None,
                    sidebar_chrome: None,
                    software_cursor: startup_software_cursor,
                },
                match_audio: Default::default(),
                match_diagnostics: Default::default(),
                #[cfg(feature = "telemetry-dashboard")]
                telemetry_timing: Default::default(),
                #[cfg(feature = "telemetry-dashboard")]
                telemetry_history: Default::default(),
                #[cfg(feature = "telemetry-dashboard")]
                report_history: Default::default(),
                #[cfg(feature = "telemetry-dashboard")]
                telemetry_tick_rate: Default::default(),
                map_basic: BasicSection::default(),
                map_ranking: crate::map::ranking::RankingSection::default(),
                loaded_map_source: None,
                loaded_map_hash: None,
                scenario_outcome: None,
                scenario_exit: None,
                scenario_restart_requested: false,
                scenario_elapsed_clock:
                    crate::app::match_runtime::frame_pacer::ScenarioElapsedClock::new(),
                configured_input_delay_ticks: input_delay_ticks,
                local_owner_override: None,
                sandbox_full_visibility: false,
                debug_pause: false,
                // KD-3: unify the two game-speed sources. `in_game_options.game_speed`
                // (in the presentation owner) is the single source of truth; seed it
                // from the skirmish-setup speed (internal 1) and derive
                // `sim_speed_tps` from the same value, so the Options slider
                // reflects the current pace (GS1 -> 63 tps).
                sim_speed_tps: crate::app::types::tps_for_game_speed(
                    crate::app::types::DEFAULT_YR_SKIRMISH_GAME_SPEED,
                ),
            },
            frontend: crate::app::frontend::state::FrontendState {
                campaign_speed_control: startup_options.campaign_speed_control,
                external_launch: false,
                external_launch_error: None,
                screen: GameScreen::default(),
                scenario_catalog,
                skirmish_modes,
                loading_session: None,
                frontend_main_rng: crate::sim::rng::MainRng::new(u64::from(frontend_seed.value)),
                startup_rng_seed: frontend_seed.value,
                legacy_crt_rng: crate::util::legacy_crt_rng::LegacyCrtRng::default(),
                next_match_correlation: 1,
                random_map_generation: None,
                random_map_retention: RandomMapGenerationRetention::default(),
                skirmish_preview_texture: None,
                loading_screen_atlas: None,
                loading_progress:
                    crate::app::loading::pump::LoadingProgressState::standard_skirmish(),
                frontend_rules: startup_rules,
                shell_preview_overlay_registry: None,
                skirmish_shell_state,
                choose_map_last_mode_row: None,
                multiplayer_shell_runtime,
                skirmish_shell_chrome,
                main_menu_shell_state: crate::ui::main_menu_shell::MainMenuShellState::default(),
                single_player_shell_state:
                    crate::ui::single_player_shell::SinglePlayerShellState::default(),
                shell_controller: crate::ui::shell::controller::DialogController::default(),
                main_menu_shell_chrome,
                main_menu_movie: None,
                main_menu_movie_identity: None,
                main_menu_movie_last_step: Instant::now(),
                main_menu_shell_error,
                status_screen_drawn: false,
                version_txt,
                shell_first_paint_slide: None,
                shell_slide_active_shell: None,
                shell_slide_generation: 0,
                shell_monitor: Default::default(),
                shell_page_title: crate::ui::shell::static_reveal::PresentedKind1Static::new(
                    crate::ui::shell::static_reveal::HEADING_KIND1,
                ),
                shell_status_line: crate::ui::shell::static_reveal::PresentedKind1Static::new(
                    crate::ui::shell::static_reveal::STATUS_LINE_KIND1,
                ),
                shell_exit: None,
                quit_cascade: None,
                startup_splash,
                shell_message_box: None,
                options_dialog: None,
                keyboard_dialog: None,
                movie_list: None,
                movie_list_selection: -1,
                fullscreen_movie: None,
                credits_roll: None,
                campaign: None,
                campaign_art: None,
                load_saved_game: None,
                wol: None,
                online_cleanup: Default::default(),
                wol_account: Default::default(),
                wol_test_credentials: Vec::new(),
                network_options: None,
                lan: None,
                multiplayer_setup: None,
                network_setup_protocol: None,
                wol_welcome_art: None,
                wol_login_art: None,
                wol_lobby_art: None,
                wol_options_art: None,
                wol_auto_login_art: None,
                wol_my_info_art: None,
                wol_buddy_art: None,
                wol_quick_match_art: None,
                wol_buddies: Default::default(),
                wol_test_buddies: crate::app::shell_wol::test_service_buddies(),
                wol_test_guest: Default::default(),
                wol_sound_options_art: None,
                wol_lobby_globals: Default::default(),
                wol_preferences: Default::default(),
                score_page: None,
                campaign_score_page: None,
                coop_score_page: None,
                score_art: None,
                campaign_score_art: None,
                finished_game_count: 0,
                shell_route: Default::default(),
            },
            renderer: crate::app::renderer_state::RendererState {
                gpu,
                batch_renderer,
                fps_overlay,
                combat_light_renderer,
                screen_presentation:
                    crate::render::screen_presentation::ScreenPresentationPass::new(),
                terrain_draw_renderer,
                instance_pool: crate::render::batch::InstanceBufferPool::new(),
                depth_view,
                shell_surface_presenter,
                upscale_pass: None,
                sidebar_surface: None,
                in_game_shell_surface: None,
                status_screen_fill,
                status_card_glow,
                bit_font,
                status_font: crate::render::status_font::StatusFaceCache::new(),
                outcome_banner_art,
                vxl_pose_frame_cache: std::cell::RefCell::new(Default::default()),
                retail_screenshot_frame_cache: Default::default(),
            },
            process_assets,
            audio: startup_audio_runtime,
            persistence: crate::app::persistence::PersistenceState::new(options_profile),
            diag: crate::app::diagnostics::state::DiagnosticsState::new(
                match crate::sim::parity_digest::ParityDigestSink::from_env() {
                    Ok(sink) => {
                        if let Some(sink) = sink.as_ref() {
                            log::info!("parity digest capture -> {}", sink.path().display());
                        }
                        sink
                    }
                    Err(error) => {
                        log::error!("parity digest sink could not be opened: {error}");
                        None
                    }
                },
            ),
        };

                // Project all three retained profile gains before any player can start
                // an audible source. Score also initializes Theme's native +0xC gate,
                // even without a music device. The profile keeps native negative values
                // for round-trip; physical output owners provide the safe gain clamp.
                apply_startup_audio_profile(&state.persistence.options_profile, &mut state.audio);
                Self::sync_zoom_pass(&mut state);
                #[cfg(feature = "telemetry-dashboard")]
                attach_dashboard(&mut state);

                if quickplay_allowed && let Ok(quickplay) = std::env::var("RA2_QUICKPLAY") {
                    // The developer shortcut carries an authored-map sandbox through
                    // the unverified legacy Battle loader. It has no artificial AI
                    // opponent or starting forces; this is not campaign admission.
                    let session = quickplay_launch_session(quickplay);
                    #[cfg(feature = "ares")]
                    crate::app::frontend::skirmish_session::prepare_external_skirmish_countries(
                        &mut state.process_assets,
                        &session,
                    );
                    let mut clock = crate::app::match_bootstrap::OrdinaryMatchSeedClock;
                    let seed = crate::app::match_bootstrap::read_match_seed(&mut clock);
                    let request =
                        crate::app::loading::pump::LoadingRequest::unverified_legacy_skirmish(
                            session, seed,
                        );
                    crate::app::loading::pump::begin_loading(&mut state, request);
                }

                Ok(InitializationStep::Ready(Box::new(state), Vec::new()))
            })
        })
    }
}

/// `RA2_QUICKPLAY=<map>`: VERA-internal authored-map sandbox using the legacy
/// Battle loader, with one local house and no generated opponents or forces.
fn quickplay_launch_session(selected_map: String) -> crate::skirmish_launch::SkirmishLaunchSession {
    use crate::skirmish_launch::{
        LaunchCountry, LaunchStartPosition, LaunchTeam, SkirmishLaunchMode, SkirmishLaunchOptions,
        SkirmishLaunchSession, SkirmishLocalSlot,
    };
    SkirmishLaunchSession {
        mode: SkirmishLaunchMode {
            class: crate::skirmish_modes::MpModeClass::Battle,
            id: 1,
            ui_name_key: "GUI:Battle".to_string(),
            tooltip_key: "STT:ModeBattle".to_string(),
            override_file: "MPBattleMD.ini".to_string(),
            map_filter: "standard".to_string(),
            random_maps_allowed: true,
            allies_allowed: true,
            must_ally: false,
        },
        selected_map_file: Some(selected_map),
        // The local House takes the player name, so naming it after the
        // country lets a fixture map's `Americans`-owned objects spawn.
        humans: vec![crate::skirmish_launch::SkirmishHumanSlot {
            raw_slot_index: Some(0),
            name: "Americans".to_string(),
            slot: SkirmishLocalSlot {
                country: LaunchCountry::America,
                country_random: false,
                // Stock colour slot 2 (DarkBlue), the colour the fixture campaign
                // maps give their Allied objects, so retail captures compare cleanly.
                color_index: 2,
                color_random: false,
                start_position: LaunchStartPosition::Position(0),
                team: LaunchTeam::None,
            },
            role: crate::skirmish_launch::LaunchHumanRole::from_observer(false),
        }],
        local_player_index: 0,
        // An empty AI house is still a contender: its automatic defeat would
        // award victory to the fixture owner and end the comparison session.
        ai_player_count: 0,
        opponents: Vec::new(),
        pre_fill_house_roster: crate::skirmish_launch::PreFillHouseRoster::from_compact_skirmish(0),
        options: SkirmishLaunchOptions {
            // VERA developer shortcut: fixture maps carry their own objects.
            // Both native starting-force gates must be off: UnitCount=0 alone
            // still adds an MCV at each start and reveals an extra radar area.
            bases: false,
            unit_count: 0,
            ..SkirmishLaunchOptions::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_errors_are_described_in_english_not_host_locale() {
        // A missing install surfaces as io::ErrorKind::NotFound inside the
        // asset error. The card cannot draw the host's localized sentence, so
        // the description must state the cause in English and keep the code.
        let io = std::io::Error::from_raw_os_error(3);
        let asset = crate::assets::error::AssetError::Io(io);
        let err = anyhow::Error::new(asset).context("Could not load the game archives");
        let text = describe_startup_error(&err);
        assert!(text.is_ascii(), "{text}");
        assert!(text.contains("Could not load the game archives"), "{text}");
        assert!(
            text.contains("does not exist") || text.contains("could not be read"),
            "{text}"
        );
        assert!(text.contains("os error 3"), "{text}");
    }

    #[test]
    fn asset_errors_keep_their_own_context() {
        let asset = crate::assets::error::AssetError::AssetNotFound {
            name: "SDBTNANM.SHP".to_owned(),
        };
        let err = anyhow::Error::new(asset).context("Required game-menu artwork is unavailable");
        let text = describe_startup_error(&err);
        assert!(text.contains("SDBTNANM.SHP"), "{text}");
        assert!(
            text.contains("Required game-menu artwork is unavailable"),
            "{text}"
        );
    }

    #[test]
    fn required_string_table_failure_is_retained_for_the_startup_error_screen() {
        let root = std::env::temp_dir().join(format!("vera-startup-csf-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        for malformed in [false, true] {
            if malformed {
                std::fs::write(root.join("ra2md.csf"), b"bad").unwrap();
            }
            let assets = AssetManager::from_loose_root_for_test(&root);
            let mut error = None;
            let csf =
                startup_asset_or_error(crate::app::loading::init::load_csf(&assets), &mut error);
            assert!(csf.is_none());
            let first = error
                .clone()
                .expect("error screen must receive the failure");
            assert!(first.contains("ra2md.csf"), "{first}");
            assert!(
                first.contains(if malformed { "3 bytes" } else { "missing" }),
                "{first}"
            );
            // Later resource failures must not replace the original diagnosis.
            startup_asset_or_error::<()>(Err(anyhow::anyhow!("later artwork failure")), &mut error);
            assert_eq!(error.as_deref(), Some(first.as_str()));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn quickplay_authored_fixture_does_not_add_starting_forces() {
        let session = quickplay_launch_session("rendering-fixture.map".into());
        assert_eq!(
            session.selected_map_file.as_deref(),
            Some("rendering-fixture.map")
        );
        assert!(
            !session.options.bases,
            "an MCV would reveal an extra start area"
        );
        assert_eq!(session.options.unit_count, 0);
        assert!(session.opponents.is_empty());
        assert_eq!(session.required_start_count(), 1);
        assert!(
            session
                .pre_fill_house_roster
                .ai_slots()
                .iter()
                .all(|slot| !slot.valid)
        );
    }

    #[test]
    #[ignore = "requires retail archives and VERA_SIDEBAR_FIXTURE_MAP"]
    fn quickplay_authored_capture_runs_300_production_frames_without_false_victory() {
        use crate::sim::house_state::HouseOutcomeKind;
        use crate::sim::scenario_bootstrap::MatchLaunchDescriptor;
        use crate::sim::world::{SimSoundEvent, TickLane};
        use crate::skirmish_launch::{
            AiDifficulty, LaunchCountry, LaunchStartPosition, LaunchTeam, PreFillHouseRoster,
            SkirmishAiSlot,
        };

        let path = std::env::var("VERA_SIDEBAR_FIXTURE_MAP").expect("exact neutral capture map");
        let root = crate::util::config::GameConfig::load()
            .unwrap()
            .paths
            .ra2_dir;
        for previous_empty_opponent in [false, true] {
            let mut session = quickplay_launch_session(path.clone());
            if previous_empty_opponent {
                // Reproduce the v3 launch defect without adding any MCVs.
                // Its empty second contender must still exercise ordinary
                // automatic defeat/victory, rather than globally disabling it.
                session.opponents.push(SkirmishAiSlot {
                    raw_slot_index: 1,
                    country: LaunchCountry::Russia,
                    country_random: false,
                    color_index: 1,
                    color_random: false,
                    start_position: LaunchStartPosition::Position(1),
                    team: LaunchTeam::None,
                    difficulty: AiDifficulty::Easy,
                });
                session.pre_fill_house_roster = PreFillHouseRoster::from_compact_skirmish(1);
            }
            let descriptor = MatchLaunchDescriptor::from_resolved(session).unwrap();
            let mut loaded =
                crate::headless_scenario::load_with_launch(&root, &path, 12345, descriptor)
                    .unwrap();
            assert_eq!(loaded.map.entities.len(), 19);
            assert_eq!(loaded.sim().entities().len(), 19, "no generated forces");
            let owner = loaded.sim().interner().get("Americans").unwrap();
            let authored: Vec<_> = loaded
                .sim()
                .entities()
                .values()
                .map(|e| e.stable_id())
                .collect();
            assert!(loaded.sim().entities().values().all(|e| e.owner() == owner));
            assert_eq!(
                loaded
                    .sim()
                    .houses()
                    .values()
                    .filter(|house| !house.multiplay_passive())
                    .count(),
                if previous_empty_opponent { 2 } else { 1 }
            );
            assert!(
                loaded.sim().path_grid().is_some(),
                "full construction publishes navigation"
            );
            assert!(loaded.sim().ready_outcome_for_owner(owner).is_none());

            let mut victory_edges = 0;
            let mut terminal_frame = None;
            for frame in 1..=300 {
                // Use the app's cadence and bound production transaction, not
                // the tooling tick helper's different millisecond cadence.
                let output = loaded
                    .runtime
                    .advance_frame(
                        &[],
                        crate::app::types::SIM_TICK_MS,
                        TickLane::Ordinary,
                        crate::sim::world::FrameEffects::default(),
                    )
                    .expect("fixture frame must complete");
                victory_edges += output.sound_events.iter().filter(|event| matches!(event,
                    SimSoundEvent::MatchOutcome { owner: event_owner, kind: HouseOutcomeKind::Victory }
                        if *event_owner == owner
                )).count();
                if previous_empty_opponent {
                    if loaded.sim().ready_outcome_for_owner(owner).is_some() {
                        assert!(!output.tick.frame_committed);
                        assert!(output.tick.terminal_score_finalized);
                        terminal_frame = Some(frame);
                        break;
                    }
                } else {
                    assert!(
                        output.tick.frame_committed,
                        "sandbox stopped on frame {frame}"
                    );
                    assert!(!output.tick.terminal_score_finalized);
                    assert!(loaded.sim().ready_outcome_for_owner(owner).is_none());
                    let house = &loaded.sim().houses()[&owner];
                    assert!(!house.has_won() && !house.has_lost() && !house.is_defeated());
                    assert!(
                        house.outcome().is_none(),
                        "no hidden false outcome on frame {frame}"
                    );
                }
            }
            assert_eq!(loaded.sim().entities().len(), 19);
            assert!(
                authored
                    .iter()
                    .all(|id| loaded.sim().entities().get(*id).is_some())
            );
            if previous_empty_opponent {
                let frame =
                    terminal_frame.expect("two-contender control must reach the ending loop");
                assert!(
                    frame <= 100,
                    "control must reproduce the observed early result"
                );
                assert_eq!(
                    victory_edges, 1,
                    "ordinary victory EVA remains a single edge"
                );
                eprintln!(
                    "Previous empty-opponent control: victory on production frame {frame}; 19 authored entities retained"
                );
            } else {
                assert_eq!(loaded.sim().session().tick(), 300);
                assert_eq!(victory_edges, 0, "no phantom victory EVA");
                let exit = crate::sim::command::CommandEnvelope::new(
                    owner,
                    1,
                    crate::sim::command::Command::ExitMatch,
                );
                let output = loaded
                    .runtime
                    .advance_frame(
                        &[exit],
                        crate::app::types::SIM_TICK_MS,
                        TickLane::Ordinary,
                        crate::sim::world::FrameEffects::default(),
                    )
                    .expect("fixture frame must complete");
                assert_eq!(output.tick.executed_commands, 1);
                assert!(!output.tick.frame_committed);
                assert!(
                    loaded.sim().quit_requested(),
                    "the sandbox still accepts explicit exit"
                );
                assert!(loaded.sim().ready_outcome_for_owner(owner).is_none());
                eprintln!(
                    "Authored quickplay: 300 production frames; 19 entities retained; no generated MCVs, victory state, EVA or ready exit; explicit ExitMatch still exits"
                );
            }
        }
    }

    #[derive(Debug, PartialEq)]
    enum StartupAudioCall {
        Sound(f64),
        Voice(f64),
        Score(f32),
        RepeatShuffle(bool, bool),
    }

    #[derive(Default)]
    struct RecordingStartupAudioOperations {
        calls: Vec<StartupAudioCall>,
    }

    impl StartupAudioProfileOperations for RecordingStartupAudioOperations {
        fn set_sound_volume(&mut self, volume: f64) {
            self.calls.push(StartupAudioCall::Sound(volume));
        }

        fn set_voice_volume(&mut self, volume: f64) {
            self.calls.push(StartupAudioCall::Voice(volume));
        }

        fn set_score_volume(&mut self, volume: f32) {
            self.calls.push(StartupAudioCall::Score(volume));
        }

        fn set_score_repeat_shuffle(&mut self, repeat: bool, shuffle: bool) {
            self.calls
                .push(StartupAudioCall::RepeatShuffle(repeat, shuffle));
        }
    }

    #[test]
    fn startup_audio_profile_applies_exact_values_in_native_order() {
        let profile = RetailOptionsProfile {
            sound_volume: 0.125,
            voice_volume: 0.5,
            score_volume: 0.875,
            is_score_repeat: true,
            is_score_shuffle: false,
            ..Default::default()
        };
        let mut operations = RecordingStartupAudioOperations::default();

        apply_startup_audio_profile(&profile, &mut operations);

        assert_eq!(
            operations.calls,
            vec![
                StartupAudioCall::Sound(0.125),
                StartupAudioCall::Voice(0.5),
                StartupAudioCall::Score(0.875),
                StartupAudioCall::RepeatShuffle(true, false),
            ]
        );
    }

    #[test]
    fn capture_dimensions_override_profile_and_remain_hidden() {
        let capture_dimensions = Some((1024, 768));
        let startup_options = RetailStartupOptions {
            audio_enabled: false,
            screen_width: 320,
            screen_height: 200,
            ..RetailStartupOptions::default()
        };
        let conflicting_ra2md_load = RetailOptionsLoad {
            retained_profile: crate::app::persistence::options_profile::RetailOptionsProfile {
                detail_level: 0,
                unit_action_lines: false,
                tooltips: false,
                screen_width: 640,
                screen_height: 480,
                sound_volume: 0.1,
                voice_volume: 0.2,
                score_volume: 0.3,
                ..Default::default()
            },
            startup_shell_screen: ScreenSize {
                width: 640,
                height: 480,
            },
        };
        let profile_loader_called = std::cell::Cell::new(false);
        let options_load = select_startup_options_load(
            capture_dimensions,
            &startup_options,
            Some(Path::new("C:/operator-profile")),
            |_, _| {
                profile_loader_called.set(true);
                conflicting_ra2md_load
            },
        );

        assert!(!profile_loader_called.get());
        assert_eq!(
            options_load,
            RetailOptionsLoad::without_settings(&RetailStartupOptions::default())
        );
        assert_eq!(
            startup_window_projection(options_load.startup_shell_screen, capture_dimensions),
            (1024, 768, false)
        );
        assert_eq!(
            options_load.startup_shell_screen,
            ScreenSize {
                width: 800,
                height: 600,
            }
        );

        let profile = &options_load.retained_profile;
        assert_eq!((profile.screen_width, profile.screen_height), (800, 600));
        assert_eq!(profile.detail_level, 2);
        assert!(profile.unit_action_lines);
        assert!(profile.tooltips);
        assert_eq!(
            (
                profile.sound_volume,
                profile.voice_volume,
                profile.score_volume,
            ),
            (0.7, 0.7, 0.4)
        );

        let options_projection =
            crate::app::persistence::options::in_game_options_from_profile(profile);
        assert_eq!(options_projection.detail_level, 2);
        assert!(options_projection.unit_action_lines);
        assert!(options_projection.tooltips);
    }
}

/// The credential is printed only to the local terminal, outside captured logs.
#[cfg(feature = "telemetry-dashboard")]
fn attach_dashboard(state: &mut crate::app::AppState) {
    state.diag.telemetry_server = match crate::app::telemetry_server::start_from_env() {
        Some(Ok(server)) => {
            crate::util::logging::enable_telemetry_capture();
            #[cfg(feature = "debug-tools")]
            let description = "Telemetry dashboard (offline debug tools enabled)";
            #[cfg(not(feature = "debug-tools"))]
            let description = "Read-only telemetry dashboard";
            eprintln!("{description}: {}", server.bootstrap_url());
            Some(server)
        }
        Some(Err(error)) => {
            log::error!("Telemetry dashboard could not start: {error}");
            None
        }
        None => None,
    };
}
