//! App state transitions: map loading into InGame, screen clearing.
//!
//! Extracted from app.rs for file-size limits.

use crate::app::match_runtime::sim_host::SimWrite;
use std::collections::{BTreeMap, HashMap};

use crate::app::loading::init;
use crate::app::presentation::render;
use crate::map::basic::BasicSection;
use crate::map::houses::HouseRoster;
use crate::render::radar::minimap::MinimapRenderer;
use crate::render::selection_overlay::SelectionOverlay;
use crate::ui::game_screen::GameScreen;
use crate::ui::sidebar::SidebarTab;

use crate::app::AppState;

/// Background clear color for menu screens (dark blue).
const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 0.02,
    g: 0.04,
    b: 0.12,
    a: 1.0,
};

/// Mirror the authoritative speed into BBB's copy: when a complete Simulation
/// replaces the app's live match (fresh-map handoff and in-scenario load),
/// after a local change, and before BBB's init or the LAN host's options read
/// it, since another player's speed event changes only the world.
pub(crate) fn sync_in_game_options_speed_from_sim(
    match_state: &mut crate::app::match_runtime::state::MatchState,
) {
    let world = match_state.sim_runtime.runtime().map(|rt| &rt.simulation);
    mirror_world_speed(&mut match_state.match_presentation.in_game_options, world);
}

/// [`sync_in_game_options_speed_from_sim`] on `options`: `world`'s speed after
/// its next frame's speed events, when it has a selectable one.
pub(crate) fn mirror_world_speed(
    options: &mut crate::ui::shell::in_game_options_state::InGameOptionsState,
    world: Option<&crate::sim::world::Simulation>,
) {
    if let Some(game_speed) =
        world.and_then(crate::sim::world::Simulation::projected_in_game_options_speed)
    {
        options.game_speed = u32::from(game_speed);
    }
}

pub(crate) fn fallback_map_load_result() -> init::MapLoadResult {
    init::MapLoadResult {
        pending_atlases: None,
        scenario: init::ScenarioLoadInputs {
            campaign_names: None,
            startup: crate::app::match_bootstrap::LoadingStartup::Generic {
                selected_map_file: "fallback".to_string(),
            },
            map_source: crate::map::source::LoadedMapSource::LegacyFallback {
                label: "fallback".to_string(),
            },
            map_hash: None,
            basic: BasicSection::default(),
            ranking: crate::map::ranking::RankingSection::default(),
            terrain_grid: None,
            resolved_terrain: None,
            simulation: None,
            overlays: Vec::new(),
            terrain_objects: Vec::new(),
            house_roster: HouseRoster::default(),
            rules: None,
            map_lighting_config: crate::map::lighting::LightingConfig::default(),
            theater_name: "TEMPERATE".to_string(),
            theater_ext: "tem".to_string(),
            sandbox_full_visibility: false,
            opening_view_cell: None,
            camera_anchor_x: 0.0,
            camera_anchor_y: 0.0,
            ui_modules: Default::default(),
        },
        presentation: init::PresentationLoadAssets {
            tile_atlas: None,
            building_zshape: None,
            unit_atlas: None,
            palette_set: None,
            sprite_atlas: None,
            overlay_atlas: None,
            bridge_atlas: None,
            sidebar_cameo_atlas: None,
            sidebar_chrome: None,
            match_layout_ini: None,
            #[cfg(feature = "phobos")]
            sw_sidebar_ui: crate::rules::phobos_sw_sidebar::SuperWeaponSidebarUi::default(),
            #[cfg(feature = "phobos")]
            center_pause_menu_background: false,
            #[cfg(feature = "phobos")]
            sw_sidebar_art: None,
            outcome_banner_side_art: None,
            end_game_banner: None,
            software_cursor: None,
            overlay_names: BTreeMap::new(),
            overlay_radar_colors: HashMap::new(),
            house_color_map: HashMap::new(),
            lighting_grid: crate::map::lighting::CellLightGrid::new(),
            csf: None,
            fnt_file: None,
        },
        asset_manager: None,
    }
}

/// App-only projections transferred with a new runtime. This bundle has no
/// gameplay authority: its source values belong to the loaded map/resources.
pub(crate) struct MapPresentationInputs {
    pub(crate) campaign_names: Option<crate::rules::campaign_loading::CampaignScenarioNames>,
    pub(crate) basic: BasicSection,
    pub(crate) ranking: crate::map::ranking::RankingSection,
    pub(crate) source: crate::map::source::LoadedMapSource,
    pub(crate) map_hash: Option<u64>,
    pub(crate) terrain_grid: Option<crate::map::terrain::TerrainGrid>,
    pub(crate) overlays: Vec<crate::map::overlay::OverlayEntry>,
    pub(crate) terrain_objects: Vec<crate::map::overlay::TerrainObject>,
    pub(crate) house_roster: HouseRoster,
    pub(crate) lighting_config: crate::map::lighting::LightingConfig,
    /// Present only for a fresh map, from before authored Overlay admission.
    pub(crate) lighting_events: Option<Vec<crate::sim::light_sources::LightingEvent>>,
    pub(crate) theater_name: String,
    pub(crate) theater_ext: String,
}

/// Install the complete map presentation after its runtime and process assets
/// are available. Fresh startup and saved restoration share this replacement;
/// route-specific input, sidebar order, waterlines and timers stay outside it.
/// Missing map-owned surfaces replace the old ones with absence, so a different
/// map can never inherit the outgoing geometry or shroud dimensions.
/// The cameo status strip's labels and Ares hook 6CC2B0's, only when Ares
/// plays in the game (`RuleSet::ares`): resolved with the incoming world and
/// after the string table's labels change, instead of with every sidebar draw.
pub(crate) fn resolve_status_texts(
    frontend: &crate::app::frontend::state::FrontendState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    process_assets: &crate::app::process_assets::ProcessAssets,
) {
    let ares_status_texts = crate::app::state::active_rules(frontend, match_state)
        .filter(|rules| rules.ares())
        .map(|rules| {
            crate::app::presentation::sidebar_build::ares_status_texts(rules, |label| {
                process_assets.csf_text(label)
            })
        });
    let status_texts = crate::app::presentation::state::StatusTexts::resolve(process_assets);
    let presentation = &mut match_state.match_presentation;
    presentation.ares_status_texts = ares_status_texts;
    presentation.status_texts = status_texts;
}

pub(crate) fn install_world_presentation(
    platform: &crate::app::PlatformState,
    renderer: &mut crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    process_assets: &mut crate::app::process_assets::ProcessAssets,
    persistence: &crate::app::persistence::PersistenceState,
    map: MapPresentationInputs,
    assets: init::PresentationLoadAssets,
) {
    if let Some(names) = map.campaign_names {
        match_state.campaign_names = names;
    }
    match_state.map_basic = map.basic;
    match_state.map_ranking = map.ranking;
    match_state.loaded_map_source = Some(map.source);
    match_state.loaded_map_hash = map.map_hash;
    match_state.match_presentation.terrain_grid = map.terrain_grid;
    frontend.shell_preview_overlay_registry = match_state
        .sim_runtime
        .runtime()
        .map(|runtime| runtime.resources.rules.retain_overlay_registry());

    match_state.match_presentation.unit_motion.clear();
    match_state.match_presentation.combat_lights.clear();
    renderer.performance_overlay.clear_simulation_rate();
    match_state.match_presentation.lasers.clear_on_load();
    // Empty unless a bounty or ProduceCashDisplay queued labels, which only Ares does.
    match_state
        .match_presentation
        .flying_credits
        .borrow_mut()
        .clear_on_load();
    match_state.match_presentation.talk_bubble.clear_on_load();
    match_state
        .match_presentation
        .electric_bolts
        .clear_on_load();
    match_state.match_presentation.airstrike_shades = Default::default();
    match_state.match_presentation.pixel_fx.clear_on_load();
    if let Some(runtime) = match_state.sim_runtime.runtime() {
        match_state
            .match_presentation
            .detail
            .borrow_mut()
            .configure_normal(&runtime.resources.rules.general.detail);
    }
    match_state.match_presentation.line_trails.clear_on_load();
    match_state.match_presentation.hidden_super_anims.clear();
    match_state.match_presentation.super_timer_blinks.clear();
    match_state.match_presentation.super_timer_next_flip_ms = None;
    match_state.match_presentation.super_timer_dock.reset();
    sync_in_game_options_speed_from_sim(match_state);

    match_state.match_presentation.tile_atlas = assets.tile_atlas;
    match_state.match_presentation.building_zshape = assets.building_zshape;
    // Growths prepared against the outgoing atlases never publish into these,
    // and what was checked against them says nothing about these.
    match_state.match_presentation.atlas_growth.cancel();
    match_state.match_presentation.atlas_check = Default::default();
    match_state.match_presentation.unit_atlas = assets.unit_atlas;
    // Both voxel caches resolve model images through the replaced RuleSet.
    *renderer.vxl_pose_frame_cache.borrow_mut() = Default::default();
    // The declared models' GPU copies belong to the replaced RuleSet.
    *renderer.model_frame.borrow_mut() = Default::default();
    match_state.match_presentation.palette_set = assets.palette_set;
    match_state.match_presentation.surface_palette = Default::default();
    match_state.match_presentation.sprite_atlas = assets.sprite_atlas;
    // The skip table decoded its frames from the atlas just replaced.
    match_state
        .match_presentation
        .zshape_skips
        .get_mut()
        .forget_frames();
    match_state.match_presentation.overlay_atlas = assets.overlay_atlas;
    match_state.match_presentation.bridge_atlas = assets.bridge_atlas;
    match_state.match_presentation.sidebar_cameo_atlas = assets.sidebar_cameo_atlas;
    match_state.match_presentation.sidebar_chrome = assets.sidebar_chrome;
    // The side's layout, the mod's with the player's own over it, sizes the
    // sidebar, and with it the battlefield, from the world's first frame.
    crate::app::presentation::player_layout::install(
        platform,
        frontend,
        match_state,
        persistence,
        assets.match_layout_ini,
    );
    #[cfg(feature = "phobos")]
    {
        match_state.match_presentation.center_pause_menu_background =
            assets.center_pause_menu_background;
        match_state.match_presentation.sw_sidebar_art = assets.sw_sidebar_art;
        let significance = match_state
            .match_presentation
            .sw_sidebar
            .required_significance;
        match_state
            .match_presentation
            .sw_sidebar
            .configure(assets.sw_sidebar_ui, significance);
        match_state
            .match_presentation
            .sidebar_projection
            .model
            .reset_sw_sidebar();
    }
    match_state.match_presentation.outcome_banner_side_art = assets.outcome_banner_side_art;
    resolve_status_texts(frontend, match_state, process_assets);
    match_state.match_presentation.end_game_banner = assets.end_game_banner;
    // `Game_LoadUI_LoadSideData` resolves the dialog background pair once per
    // game load. The shell chrome atlas carries the three side slots, so a
    // changed pair drops it and the next paint rebuilds with the side's art;
    // the process-lifetime slot 0 (`PUDLGBGN`) is not part of the hook, which
    // runs only when Ares plays in the game (`RuleSet::ares`).
    if crate::app::state::active_rules(frontend, match_state).is_some_and(|rules| rules.ares()) {
        let side = match_state
            .sim_runtime
            .runtime()
            .map_or(-1, |runtime| runtime.simulation.session().scenario_side());
        let pair = process_assets.side_dialog_background(side);
        if frontend.shell_dialog_background != pair {
            frontend.shell_dialog_background = pair;
            frontend.skirmish_shell_chrome = None;
        }
    }
    // GAME.FNT is process artwork, so retain the established process fallback
    // when optional font preparation did not produce a replacement, and keep
    // the font itself, with the text it has laid out and drawn, when the
    // scenario read the same bytes: a new font would build GAME.FNT's
    // outline faces again (`FntFont::build`) and draw every glyph into an
    // empty atlas.
    if let Some(ref fnt) = assets.fnt_file
        && !renderer.bit_font.is_built_from(fnt)
    {
        renderer.bit_font = crate::render::bit_font::BitFont::from_fnt(
            &renderer.gpu.device,
            &renderer.gpu.queue,
            &renderer.batch_renderer,
            fnt,
        );
    }

    // The final sidebar projection resolves incoming side/radar authority.
    // `RadarClass::Init_Clear @ 0x00652DE0` starts each scenario offline at
    // frame 0, outside any movie mode, with TacticalMapAvailable clear.
    match_state.match_presentation.radar_anim = None;
    match_state.match_presentation.radar_animation_source = None;
    match_state.match_presentation.radar_content_insets = None;
    match_state.match_presentation.has_radar = false;
    match_state.match_presentation.loaded_radar_housing = None;
    match_state.match_presentation.software_cursor = assets.software_cursor;
    match_state
        .match_presentation
        .overlays
        .replace_from_source(map.overlays);
    match_state.match_presentation.terrain_objects = map.terrain_objects;
    match_state.match_presentation.overlay_names = assets.overlay_names;
    match_state.match_presentation.overlay_radar_colors = assets.overlay_radar_colors;
    match_state.match_presentation.house_color_map = assets.house_color_map;
    // VERA relation and team colours start each scenario off, in the player
    // colours; the launch team cache belongs to the match that just ended.
    match_state.match_presentation.relation_colors = false;
    match_state.match_presentation.team_colors = false;
    match_state.match_presentation.team_by_owner = None;
    match_state.match_presentation.relation_color_map = None;
    // VERA's Super Weapon Building Timers start from the saved switch; a
    // match's own hotkey flips it from there.
    let saved_timers = persistence.options_profile.superweapon_building_timers;
    match_state.match_presentation.superweapon_building_timers = saved_timers;
    // VERA's Convenience Help starts each scenario from the saved switch too;
    // Ctrl+Alt+H flips it from there and writes it back to the profile.
    let help = persistence.options_profile.convenience_help;
    match_state.match_presentation.convenience_help = help;
    match_state.sim_runtime.show_status_rows(help);
    match_state.match_presentation.house_roster = map.house_roster;
    process_assets.csf = assets.csf;
    match_state.match_presentation.theater_name = map.theater_name;
    match_state.match_presentation.theater_ext = map.theater_ext;

    let live = match_state.sim_runtime.runtime().and_then(|runtime| {
        runtime
            .view()
            .resolved_terrain()
            .map(|terrain| (terrain, &runtime.simulation))
    });
    let presentation = &mut match_state.match_presentation;
    presentation.lighting.install(
        assets.lighting_grid,
        map.lighting_config,
        presentation.in_game_options.detail_level,
        live,
        map.lighting_events.as_deref(),
    );
    // Current MapClass authority, including restored/triggered LocalSize,
    // reaches camera and minimap before either route positions its view.
    let (playfield_bounds, playfield_revision) =
        crate::app::input::camera::playfield_presentation_authority(&match_state.sim_runtime);
    let minimap = match_state
        .match_presentation
        .terrain_grid
        .as_ref()
        .map(|grid| {
            let runtime = match_state.sim_runtime.runtime();
            let overlay_registry =
                runtime.map(|runtime| runtime.resources.rules.overlay_registry());
            let rules = runtime.map(|runtime| &runtime.resources.rules);
            let overlay_data = build_minimap_overlay_data(
                match_state.match_presentation.overlays.as_slice(),
                &match_state.match_presentation.terrain_objects,
                overlay_registry,
                rules,
            );
            MinimapRenderer::new(
                &renderer.gpu.device,
                &renderer.gpu.queue,
                &renderer.batch_renderer,
                grid,
                runtime,
                &overlay_data,
                &match_state.match_presentation.overlay_radar_colors,
                &match_state.match_presentation.theater_name,
                playfield_bounds,
                playfield_revision,
            )
        });
    match_state.match_presentation.minimap = minimap;
    // The interface pictures take `PictureSize=` from the match's art.
    let picture_sizes = match_state.sim_runtime.runtime().map_or(
        crate::render::interface_picture::PictureSizes::None,
        |runtime| {
            crate::render::interface_picture::PictureSizes::Art(runtime.resources.rules.art())
        },
    );
    match_state.match_presentation.selection_overlay = Some(SelectionOverlay::new(
        &renderer.gpu,
        &renderer.batch_renderer,
        process_assets.manager(),
        picture_sizes,
    ));
    // Phobos DigitalDisplay `Shape=`/`Palette=` assets, retained once per match
    // like `DigitalDisplayTypeClass`'s SHP pointers.
    #[cfg(feature = "phobos")]
    if let Some(assets) = process_assets.manager()
        && let Some(rules) = match_state
            .sim_runtime
            .runtime()
            .map(|runtime| &runtime.resources.rules)
        && let Some(overlay) = match_state.match_presentation.selection_overlay.as_mut()
    {
        overlay.load_digital_display_shapes(
            &renderer.gpu.device,
            &renderer.gpu.queue,
            &renderer.batch_renderer,
            assets,
            rules.digital_display_types().types(),
            crate::render::interface_picture::PictureSizes::Art(rules.art()),
        );
    }
    // Ares/Phobos `Insignia` SHPs, retained once per match like the
    // TechnoTypeExt's SHPStruct pointers.
    if let Some(assets) = process_assets.manager()
        && let Some(rules) = match_state
            .sim_runtime
            .runtime()
            .map(|runtime| &runtime.resources.rules)
        && let Some(overlay) = match_state.match_presentation.selection_overlay.as_mut()
    {
        overlay.load_insignia_shapes(
            &renderer.gpu.device,
            &renderer.gpu.queue,
            &renderer.batch_renderer,
            assets,
            rules,
        );
    }
    // Ares `IvanBomb.Image` SHPs, retained once per match like the
    // WeaponTypeExt's SHPStruct pointers, when the Rules were read the Ares
    // way: the bomb clock draws them then (`techno_extras_for_parent`).
    if let Some(assets) = process_assets.manager()
        && let Some(rules) = match_state
            .sim_runtime
            .runtime()
            .map(|runtime| &runtime.resources.rules)
        && rules.ares_form()
        && let Some(overlay) = match_state.match_presentation.selection_overlay.as_mut()
    {
        overlay.load_ares_bomb_images(
            &renderer.gpu.device,
            &renderer.gpu.queue,
            &renderer.batch_renderer,
            assets,
            rules,
        );
    }
    // Phobos placement preview strips embed the active theater's palette and
    // the local house colour; a map load invalidates them.
    #[cfg(feature = "phobos")]
    match_state
        .match_presentation
        .placement_preview_art
        .borrow_mut()
        .reset();
    // Full_Init 687BB2 -> Init_BeaconArt 4309D0, after 68781F mounted the
    // side's MIX group; each row's ColorScheme is its House's (+0x16054),
    // whose Convert Init_Theater rebuilt from UNIT%s.PAL (534D77).
    let beacon_side =
        crate::app::presentation::sidebar_render::current_sidebar_side(frontend, match_state);
    let beacon_ares =
        crate::app::state::active_rules(frontend, match_state).is_some_and(|rules| rules.ares());
    match_state.match_presentation.beacon_art = process_assets.manager().and_then(|assets| {
        let runtime = match_state.sim_runtime.runtime()?;
        let sim = &runtime.simulation;
        let colors = &match_state.match_presentation.house_color_map;
        let ramps: Vec<_> = sim
            .houses()
            .order()
            .iter()
            .map(|house| {
                let index = colors
                    .get(sim.interner().resolve(*house))
                    .copied()
                    .unwrap_or(crate::rules::house_colors::NO_REMAP);
                *runtime.resources.rules.house_color_ramps.ramp(index)
            })
            .collect();
        let unit_palette = crate::map::theater::load_theater_unit_and_iso_palettes(
            assets,
            &match_state.match_presentation.theater_name,
        )
        .map(|(unit, _)| unit);
        Some(crate::render::beacon_art::BeaconArt::new(
            &renderer.gpu.device,
            &renderer.gpu.queue,
            &renderer.batch_renderer,
            assets,
            beacon_side,
            beacon_ares,
            unit_palette.as_ref(),
            &ramps,
            crate::render::interface_picture::PictureSizes::Art(runtime.resources.rules.art()),
        ))
    });
    let shroud_buffer = process_assets.manager().and_then(|assets| {
        let grid = match_state.sim_runtime.runtime()?.simulation.path_grid()?;
        let shp =
            crate::assets::shp_file::ShpFile::from_bytes(assets.get_ref("shroud.shp")?).ok()?;
        let (frame_pixels, cw, ch) = crate::render::shroud_buffer::extract_shp_brightness(&shp);
        let mut buffer = crate::render::shroud_buffer::ShroudBuffer::new(
            &renderer.gpu.device,
            &renderer.gpu.queue,
            [
                renderer.render_width(&frontend.screen),
                renderer.render_height(&frontend.screen),
            ],
            [grid.width(), grid.height()],
            frame_pixels,
            [cw, ch],
            crate::render::shroud_buffer::SHROUD_EDGE_LUT,
        );
        buffer.set_translucent_fog(&renderer.gpu.queue, platform.translucent_fog_enabled());
        // Cell47EFE0/47F250 use FOG.SHP's raw A bytes when Scenario0x1000
        // is set. It belongs to this same tactical source, not a sprite atlas.
        let fog = crate::assets::shp_file::ShpFile::from_bytes(assets.get_ref("fog.shp")?).ok()?;
        let (frames, width, height) = crate::render::shroud_buffer::extract_shp_brightness(&fog);
        buffer.install_fog_frames(
            &renderer.gpu.device,
            &renderer.gpu.queue,
            frames,
            [width, height],
        );
        Some(buffer)
    });
    match_state.match_presentation.shroud_buffer = shroud_buffer;
}

/// Complete StartScenario's successful ReadScenario boundary. Basic.Action
/// executes before the tactical video-mode/timing handoff. Its existing movie
/// owner suspends/resumes audio; the later Theme queue/Stop(true) belongs to
/// apply_map_load_result (683E4D..683E66), after this movie returns.
pub(crate) fn finish_fresh_scenario_read(state: &mut AppState, mut result: init::MapLoadResult) {
    assert!(
        result.pending_atlases.is_none(),
        "install requires completed presentation"
    );
    // Read_INI_Basic689FD6/689FF4 retains these two buffers across fresh
    // ClearScene reads. Saved restoration instead installs the saved values
    // directly through install_world_presentation; it must not inherit here.
    result
        .scenario
        .basic
        .resolve_retained_destinations(&state.match_state.map_basic);
    let action_movie = result.scenario.startup.campaign().and_then(|startup| {
        // Start_Scenario683D9C tests its DL argument before the movie.
        if !startup.plays_action_movie() {
            return None;
        }
        let name = result.scenario.basic.action.as_deref()?;
        let movies = state.process_assets.native_rules()?.movies();
        movies.name(movies.find_index(name)).map(str::to_owned)
    });
    if let Some(movie) = action_movie {
        if let Some(manager) = result.asset_manager.take() {
            state.process_assets.return_from_loading(manager);
        }
        crate::app::loading::pump::clear_loading_state(
            &mut state.renderer,
            &mut state.frontend,
            &mut state.process_assets,
        );
        crate::app::App::start_fullscreen_movie(
            state,
            &movie,
            crate::app::frontend::fullscreen_movie::MovieReturn::CampaignLoaded(Box::new(result)),
        );
    } else {
        apply_map_load_result(state, result);
    }
}

pub(crate) fn apply_map_load_result(state: &mut AppState, result: init::MapLoadResult) {
    // Retiring Radar may record a service input after the outgoing frame.
    // Publish that frame before the retire/reset sequence.
    crate::app::match_runtime::sim_tick::finish_threaded_frame(state);
    crate::app::match_runtime::scenario_pause::reset_clock(
        &mut state.match_state.match_presentation,
    );
    let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    crate::app::radar_movie::retire(
        &state.platform,
        &state.renderer,
        &state.diag,
        &state.frontend,
        &mut state.match_state,
        &state.process_assets,
        &mut state.audio,
        &state.persistence,
        wall_ms,
    );
    crate::app::chrono_screen::retire(
        &state.frontend.screen,
        &mut state.match_state,
        &mut state.audio,
        wall_ms,
    );
    crate::app::input::dispatch::selection_navigation::reset_for_world_replacement(
        &mut state.match_state.input,
    );
    // Start_Scenario 00683D21 completes Read_Scenario before switching video
    // mode at 00683DF3. VERA keeps the window the player gave it: the match
    // takes the zoom pass's source size, already current for this window.
    crate::app::loading::pump::present_game_mode_blank(
        &state.platform,
        &state.renderer,
        &state.match_state,
    );
    // Only fresh startup cancels its outgoing scenario-owned request. Saved
    // loading preserves the process Theme state until ThemeLoad721040.
    state.audio.cancel_scenario_theme_request();
    let startup = result.scenario.startup;
    let returns_scenario_rng_to_multiplayer_shell = startup.launch_session().is_some();
    // A loaded world is not timed until the launch handoff reaches InGame.
    state.match_state.scenario_elapsed_clock.reset();
    #[cfg(feature = "telemetry-dashboard")]
    crate::app::telemetry_capture::reset_observation(&mut state.diag, &mut state.match_state);
    crate::app::loading::pump::clear_loading_state(
        &mut state.renderer,
        &mut state.frontend,
        &mut state.process_assets,
    );

    // Close the outgoing diagnostic timeline before replacing its runtime.
    crate::app::match_runtime::sim_tick::close_replay_segment_for_new_timeline(state);
    crate::app::reset_scenario_exit_runtime(
        &mut state.frontend,
        &mut state.match_state,
        &mut state.audio,
    );
    let incoming = result.scenario.simulation.zip(result.scenario.rules);
    if let Some(sfx) = state.audio.sfx_player.as_mut() {
        // Every committed replacement retires outgoing sources, including a
        // generic fallback with no incoming world. Cancellation precedes both
        // dropping the world and installing a fresh Main continuation.
        sfx.stop_all();
    }
    if incoming.is_some() {
        // The inactive load has inherited this process cursor. Only a world
        // actually installed here retires cold MapGen transport; failed and
        // cancelled attempts retain it in RandomMapGenerationRetention.
        state
            .frontend
            .random_map_retention
            .discard_pending_mapgen_on_simulation_install();
    } else if let Some(outgoing) = state.match_state.sim_runtime.take_runtime() {
        // The generic fallback drops its resident Simulation. MapGen is
        // process-global in gamemd, so retain its cursor before that drop.
        let continuation = outgoing.simulation.mapgen_continuation_for_fresh_load();
        state
            .frontend
            .random_map_retention
            .publish_mapgen_continuation(None, continuation);
        // GameMode is process-global too; the shell takes it back.
        state
            .match_state
            .session
            .retake_mode_from_world(outgoing.simulation.session().game_mode());
    }
    state
        .match_state
        .sim_runtime
        .install(incoming.map(|(mut simulation, rules)| {
            simulation.install_fresh_process_main(&state.frontend.frontend_main_rng);
            simulation.publish_scenario_started();
            simulation.batch_path_searches();
            simulation.batch_target_scans();
            simulation.present_effect_anims();
            simulation.separate_turn_streams(true);
            simulation.sleep_idle_objects_in_match();
            crate::sim::runtime::SimRuntime {
                simulation,
                resources: crate::sim::runtime::SimResources {
                    terrain_template: result.scenario.resolved_terrain,
                    rules,
                }
                .into(),
            }
        }));
    if let Some(runtime) = state.match_state.sim_runtime.runtime() {
        state
            .match_state
            .session
            .hand_mode_to_world(runtime.simulation.session().game_mode());
    }
    crate::app::audio_runtime::bind_match_eva_context(
        &state.match_state.sim_runtime,
        &mut state.audio,
    );
    // Only an actually-carried lease returns. A fallback result must not wipe
    // the process-sticky manager already restored by its failure path.
    if let Some(manager) = result.asset_manager {
        state.process_assets.return_from_loading(manager);
    }
    let lighting_events = state
        .match_state
        .sim_runtime
        .write_sim(SimWrite::Presentation)
        .map(|runtime| runtime.simulation.take_lighting_events());
    install_world_presentation(
        &state.platform,
        &mut state.renderer,
        &mut state.frontend,
        &mut state.match_state,
        &mut state.process_assets,
        &state.persistence,
        MapPresentationInputs {
            campaign_names: result.scenario.campaign_names,
            basic: result.scenario.basic,
            ranking: result.scenario.ranking,
            source: result.scenario.map_source,
            map_hash: result.scenario.map_hash,
            terrain_grid: result.scenario.terrain_grid,
            overlays: result.scenario.overlays,
            terrain_objects: result.scenario.terrain_objects,
            house_roster: result.scenario.house_roster,
            lighting_config: result.scenario.map_lighting_config,
            lighting_events,
            theater_name: result.scenario.theater_name,
            theater_ext: result.scenario.theater_ext,
        },
        result.presentation,
    );
    if let Some(rules) = crate::app::state::active_rules(&state.frontend, &state.match_state) {
        state
            .match_state
            .match_presentation
            .voxel_images
            .borrow_mut()
            .start_scenario(|type_id| {
                rules
                    .object(type_id)
                    .is_some_and(|object| object.voxel_image_load().is_some())
            });
    }
    state
        .match_state
        .match_presentation
        .sinking_waterlines
        .borrow_mut()
        .clear();
    // Fresh SidebarClass admission silently seeds its first projection.
    // ClearScene685573 -> Mouse5BDF50 -> Sidebar6A5030 -> Power63F730:
    // fresh matches clear the retained Power fields. The saved-load route
    // shares install_world_presentation but has a distinct NoInit contract.
    let wall_ms =
        crate::app::diagnostics::state::radar_presentation_ms(&state.diag, &state.match_state);
    state
        .match_state
        .match_presentation
        .power_bar_anim
        .reset(wall_ms);
    state
        .match_state
        .match_presentation
        .sidebar_projection
        .model
        .reset_cameo_seed();
    let owner = state.match_state.local_player_owner().map(str::to_owned);
    state
        .match_state
        .match_presentation
        .sidebar_projection
        .model
        .start_scenario_credits_counter(owner.as_deref());
    if let Some(runtime) = state.match_state.sim_runtime.write_sim(SimWrite::Lifecycle) {
        runtime
            .simulation
            .set_input_delay_ticks(state.match_state.configured_input_delay_ticks);
    }

    // Map load hands over a world anchor point; the transition applies the
    // active tactical rectangle and live zoom.
    let (tactical_x, tactical_y, tactical_width, tactical_height) =
        crate::app::input::camera::tactical_viewport_px(
            &state.platform,
            &state.renderer,
            &state.frontend.screen,
            &state.match_state.match_presentation,
            &state.persistence,
        );
    let (centring_width, centring_height) = crate::app::input::camera::tactical_centring_size(
        &state.platform,
        &state.renderer,
        &state.frontend.screen,
        &state.match_state.match_presentation,
        &state.persistence,
    );
    let (camera_x, camera_y) = crate::app::input::camera::tactical_camera_top_left(
        (
            result.scenario.camera_anchor_x,
            result.scenario.camera_anchor_y,
        ),
        centring_width as f32,
        centring_height as f32,
        state.match_state.input.zoom_level,
    );
    crate::app::input::camera::reset_camera_for_new_scenario(&mut state.match_state.input);
    state.match_state.input.cursor_visibility.reset();
    crate::app::input::camera::set_camera_position(
        &mut state.match_state.input,
        (camera_x, camera_y),
    );
    // gamemd's scenario reader fills all four camera bookmarks with the opening
    // view cell, so F1 before any Ctrl+F1 is a valid "go home".
    crate::app::input::camera::seed_view_bookmarks_from_opening_view(
        &state.platform,
        &state.renderer,
        &state.frontend.screen,
        &mut state.match_state,
        &state.persistence,
        result.scenario.opening_view_cell,
    );
    state.match_state.input.reset_targeting_for_new_world();
    state.match_state.match_presentation.active_sidebar_tab = SidebarTab::default_active_tab();
    // The match's chrome: the sidebar's and the command bar's nodes, which
    // take their clicks and draw them, until the match ends.
    crate::app::input::chrome_input::build_for_match(&mut state.match_state.match_presentation);
    // Read_Scenario's Init (`0x00684AC9`). Start_Scenario then changes video
    // mode when the game size differs from the shell's (`0x00683DB8..0x00683DF3`):
    // Change_Video_Mode's Set_View_Dimensions (`0x005610EC`) Inits the empty
    // list again and SetWidth leaves it 6 px wider. VERA's match takes the
    // window's size, so a match frame other than the 800x600 shell stands for
    // that switch.
    let mut message_list = crate::ui::messages::MessageList::scenario();
    let shell = crate::app::persistence::options_profile::RETAIL_SHELL_SIZE;
    if (
        state.renderer.render_width(&state.frontend.screen),
        state.renderer.render_height(&state.frontend.screen),
    ) != (shell.width, shell.height)
    {
        message_list.set_width(tactical_width as i32);
    }
    state.match_state.match_presentation.message_list = message_list;
    crate::app::input::messages::post_layout_problem(
        &state.platform,
        &state.renderer,
        &state.diag,
        &state.frontend,
        &mut state.match_state,
        &state.process_assets,
        &mut state.audio.sfx_player,
        &state.persistence,
    );
    crate::app::input::messages::post_art_problems(
        &state.platform,
        &state.renderer,
        &state.diag,
        &state.frontend,
        &mut state.match_state,
        &state.process_assets,
        &mut state.audio.sfx_player,
        &state.persistence,
    );
    state.match_state.match_presentation.chat = Default::default();
    // Clear_Scene6851F0 -> 534450 resets BeaconManagerClass (53499B).
    // Display Init_Clear4A88C0 clears +11B0..+11B2, not beacon mode +11B4.
    state.match_state.match_presentation.beacons = Default::default();
    state
        .match_state
        .match_presentation
        .sidebar_gadget_state
        .clear_for_new_scenario();
    state.match_state.input.cancel_minimap_press();
    state.match_state.input.tactical_mouse.reset_for_new_view();
    state.match_state.input.keys_held.clear();
    state.match_state.input.cursor_x = tactical_x as f32 + tactical_width as f32 * 0.5;
    state.match_state.input.cursor_y = tactical_y as f32 + tactical_height as f32 * 0.5;
    state.platform.frame_pacer.reset_for_new_world();
    state
        .match_state
        .network_quality
        .reset(state.platform.frame_pacer.total_render_deferrals());
    state.match_state.input.queued_order_mode = render::OrderMode::Move;
    state.match_state.match_audio.reset_for_new_match();
    state.match_state.sandbox_full_visibility = result.scenario.sandbox_full_visibility;
    // The match's client modules, now that the world, its rules modules and
    // its chrome exist: loaded and `client_init` run here, `client_on_start`
    // with the first frame the match draws (`ui_modules::render_frame`).
    crate::app::ui_modules::install(state, result.scenario.ui_modules);

    // `Start_Scenario @ 0x00683AB0` after `Read_Scenario`: `[Basic] Theme`
    // resolves through `From_Name` (`0x004758F0`); -1 -> `Stop(fade=1)` of the
    // LOADING stream, else `Queue_Song(index)`. `Main_Tick` then issues
    // `Queue_Song(-2)` and the audio pump's AI picks the first allowed track
    // once the fade lands. Theme AI borrows the installed process Main cursor;
    // `Is_Allowed`'s `Side=` gate compares the local player's side.
    let music_now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    crate::app::audio_runtime::bind_match_theme_context(
        &state.match_state,
        &state.process_assets,
        &mut state.audio,
    );
    if let Some(assets) = state.process_assets.manager() {
        state.audio.request_scenario_theme(
            state.match_state.map_basic.theme.as_deref(),
            assets,
            music_now_ms,
        );
    }

    let mut initial_replay_effects = Vec::new();
    let installation = match startup {
        crate::app::match_bootstrap::LoadingStartup::Campaign(prepared) => {
            let installed = state.match_state.startup.acknowledge_campaign(
                &prepared,
                state
                    .match_state
                    .sim_runtime
                    .runtime()
                    .map(|rt| &rt.simulation),
                matches!(state.frontend.screen, GameScreen::Loading),
            );
            // GameExit::Victory (`0x00685D7A`), the restart (`0x00686506`)
            // and the defeat retry (`0x0068639A`) write the carry-over record
            // back after Start_Scenario through `0x004C6210`.
            if installed.is_ok()
                && let Some(record) = prepared.carry_over()
                && let Some(runtime) = state.match_state.sim_runtime.write_sim(SimWrite::Lifecycle)
            {
                runtime
                    .simulation
                    .restore_campaign_carry_over(record, &runtime.resources.rules);
            }
            installed
        }
        crate::app::match_bootstrap::LoadingStartup::Accepted(prepared) => {
            state.match_state.startup.acknowledge(
                prepared,
                state
                    .match_state
                    .sim_runtime
                    .runtime()
                    .map(|rt| &rt.simulation),
                matches!(state.frontend.screen, GameScreen::Loading),
            )
        }
        crate::app::match_bootstrap::LoadingStartup::Network(prepared) => {
            state.match_state.session.assert_network_start(&prepared);
            state.match_state.startup.clear();
            // A co-op continuation's GameExit tail (`0x00685D7A` victory,
            // `0x0068639A` defeat) restores the record after Start_Scenario.
            if let Some(record) = prepared.carry_over()
                && let Some(runtime) = state.match_state.sim_runtime.write_sim(SimWrite::Lifecycle)
            {
                runtime
                    .simulation
                    .restore_campaign_carry_over(record, &runtime.resources.rules);
            }
            let installed = crate::app::lan_service::install_gameplay(
                &state.platform,
                &mut state.match_state,
                &mut state.process_assets,
            );
            if installed.is_ok() {
                install_watcher(
                    &state.platform,
                    &state.frontend,
                    &mut state.match_state,
                    &mut state.audio.sfx_player,
                );
            }
            installed.map_err(|error| error.to_string())
        }
        crate::app::match_bootstrap::LoadingStartup::Replay { .. }
        | crate::app::match_bootstrap::LoadingStartup::AuthoredCampaignReplay { .. } => {
            state.match_state.startup.clear();
            let playback = match (
                state.match_state.sim_runtime.write_sim(SimWrite::Lifecycle),
                state.match_state.loaded_map_source.as_ref(),
                state.match_state.loaded_map_hash,
            ) {
                (Some(runtime), Some(source), Some(map_hash)) => state
                    .match_state
                    .match_diagnostics
                    .install_playback(runtime, source, map_hash),
                _ => Err(anyhow::anyhow!(
                    "replay loader did not install an authored world"
                )),
            };
            match playback {
                Ok(effects) => initial_replay_effects = effects,
                Err(error) => {
                    log::error!("Replay startup failed: {error:#}");
                    let message = if matches!(
                        startup,
                        crate::app::match_bootstrap::LoadingStartup::AuthoredCampaignReplay { .. }
                    ) {
                        crate::i18n::tr("VERA:CampaignReplayUnavailable")
                    } else {
                        format!("Replay startup failed: {error:#}")
                    };
                    state.frontend.external_launch_error = Some(message);
                }
            }
            // A rejected replay is a frozen inspection view, never a fresh
            // playable match. Its diagnostics owner blocks every tick/input.
            Ok(())
        }
        crate::app::match_bootstrap::LoadingStartup::UnverifiedLegacy { session, .. } => {
            state.match_state.startup.install_legacy(session);
            Ok(())
        }
        crate::app::match_bootstrap::LoadingStartup::Generic { .. } => {
            state.match_state.startup.clear();
            Ok(())
        }
    };
    match installation {
        Ok(()) => {
            let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
            state.match_state.scenario_elapsed_clock.start(now_ms);
            // Main_Game 0x0048CE7E: State_Machine's admission latch.
            state.match_state.match_presentation.modal_latch = true;
            state.frontend.screen = GameScreen::InGame;
            // The skirmish shell's chrome atlas draws nothing in a match until
            // the in-game menu or a network dialog opens: the menu's opening
            // (`App::enter_in_game_menu_state`) and the frame loop build it
            // again then, and the shell's return to its page builds it as
            // every entry does (`App::ensure_skirmish_shell_chrome`).
            state.frontend.skirmish_shell_chrome = None;
            // The main menu's chrome atlas likewise: the first shell screen
            // after the match builds it again
            // (`App::ensure_main_menu_shell_chrome`).
            state.frontend.main_menu_shell_chrome = None;
            if !initial_replay_effects.is_empty() {
                crate::app::match_runtime::sim_tick::apply_trigger_effects(
                    state,
                    &initial_replay_effects,
                );
            }
            #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
            crate::web::match_started();
            if returns_scenario_rng_to_multiplayer_shell {
                state
                    .frontend
                    .multiplayer_shell_runtime
                    .mark_gameplay_scenario_return_pending();
            }
            log::info!("Transitioned to InGame after scenario installation");
        }
        Err(err) => {
            state.match_state.startup.clear();
            state.frontend.screen = GameScreen::MissionResult {
                title: crate::i18n::tr("VERA:StartupRejected"),
                detail: err.clone(),
            };
            log::error!("Scenario installation rejected startup: {err}");
        }
    }
    crate::app::presentation::sidebar_render::refresh_sidebar_projection(
        &state.platform,
        &state.renderer,
        &state.diag,
        &state.frontend,
        &mut state.match_state,
        &state.process_assets,
        &mut state.audio,
        &state.persistence,
    );
}

/// VERA watching (docs/Networking.md "Watching"): the live session that
/// `install_gameplay` made says whether this game watches its match. Its world
/// is then a watcher's before its first frame: it sends nothing, also in the
/// seat of the player it follows, whose sight it starts with, and Tab may
/// change its view (`match_runtime::watching`).
/// A launcher's watcher (docs/spawn.md "Watching") stands in source 0's seat
/// only because a world needs a current House: it starts on the observer's
/// sidebar (`watching::begin_on_observers_view`).
fn install_watcher(
    platform: &crate::app::PlatformState,
    frontend: &crate::app::frontend::state::FrontendState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    sfx_player: &mut Option<crate::audio::sfx::SfxPlayer>,
) {
    if !crate::app::match_runtime::watching::active(match_state) {
        return;
    }
    if let Some(runtime) = match_state.sim_runtime.write_sim(SimWrite::Lifecycle) {
        runtime.simulation.install_watcher();
    }
    if frontend.external_launch {
        crate::app::match_runtime::watching::begin_on_observers_view(
            platform,
            frontend,
            match_state,
            sfx_player,
        );
    }
}

/// Build overlay classification data for the minimap from map overlay entries.
///
/// Carries parsed native overlay flags/IDs and the current OverlayData frame;
/// only the Ore-v-Gem display label uses the stock three-character name family.
/// `CellClass::GetRadarColor` source selection does not consume that label.
pub(crate) fn build_minimap_overlay_data(
    overlays: &[crate::map::overlay::OverlayEntry],
    terrain_objects: &[crate::map::overlay::TerrainObject],
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> Vec<crate::render::radar::minimap::MinimapOverlayDatum> {
    use crate::render::radar::minimap::{
        MinimapCellRadarSource, MinimapOverlayDatum, OverlayClassification, minimap_overlay_datum,
    };

    let mut data = Vec::with_capacity(overlays.len() + terrain_objects.len());

    for entry in overlays {
        data.push(minimap_overlay_datum(
            entry.rx,
            entry.ry,
            entry.overlay_id,
            entry.frame,
            overlay_registry,
            rules,
        ));
    }

    for obj in terrain_objects {
        data.push(MinimapOverlayDatum {
            rx: obj.rx,
            ry: obj.ry,
            classification: OverlayClassification::TerrainObject,
            source: MinimapCellRadarSource::TerrainObject,
        });
    }

    data
}

/// Clear the screen to the background color (no depth buffer).
pub(crate) fn clear_screen(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Clear Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(CLEAR_COLOR),
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
}

#[cfg(all(test, tests_app))]
mod game_speed_tests {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::house_state::HouseState;
    use crate::sim::world::Simulation;

    #[test]
    fn loaded_lobby_speed_projection_includes_due_transition() {
        let mut sim = Simulation::new();
        let owner = sim.intern("Local");
        sim.houses_mut_for_test()
            .register(HouseState::new(owner, 0, None, true, 0, 10));
        sim.queue_command(CommandEnvelope::new(
            owner,
            1,
            Command::SetGameSpeed { speed: 4 },
        ));

        assert_eq!(sim.projected_in_game_options_speed(), Some(4));
    }
}
