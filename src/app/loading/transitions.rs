//! App state transitions: map loading into InGame, screen clearing.
//!
//! Extracted from app.rs for file-size limits.

use std::collections::{BTreeMap, HashMap};

use crate::app::loading::init;
use crate::app::presentation::render;
use crate::map::basic::BasicSection;
use crate::map::houses::HouseRoster;
use crate::render::radar::minimap::MinimapRenderer;
use crate::render::selection_overlay::SelectionOverlay;
use crate::rules::overlay_types::OverlayTypeRegistry;
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

/// Mirror the authoritative speed whenever a complete Simulation replaces the
/// app's live match, including both fresh-map handoff and in-scenario load.
pub(crate) fn sync_in_game_options_speed_from_sim(state: &mut AppState) {
    let Some(game_speed) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
        .and_then(crate::sim::world::Simulation::projected_in_game_options_speed)
        .map(u32::from)
    else {
        return;
    };
    state
        .match_state
        .match_presentation
        .in_game_options
        .game_speed = game_speed;
    state.match_state.sim_speed_tps = crate::app::types::tps_for_game_speed(game_speed);
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
            overlay_registry: OverlayTypeRegistry::empty(),
            house_roster: HouseRoster::default(),
            rules: None,
            map_lighting_config: crate::map::lighting::LightingConfig::default(),
            theater_name: "TEMPERATE".to_string(),
            theater_ext: "tem".to_string(),
            initial_local_owner: None,
            sandbox_full_visibility: false,
            opening_view_cell: None,
            camera_anchor_x: 0.0,
            camera_anchor_y: 0.0,
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
            #[cfg(feature = "phobos")]
            sw_sidebar_ui: crate::rules::phobos_sw_sidebar::SuperWeaponSidebarUi::default(),
            #[cfg(feature = "phobos")]
            center_pause_menu_background: false,
            #[cfg(feature = "phobos")]
            sw_sidebar_art: None,
            outcome_banner_side_art: None,
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
pub(crate) fn install_world_presentation(
    state: &mut AppState,
    map: MapPresentationInputs,
    assets: init::PresentationLoadAssets,
) {
    if let Some(names) = map.campaign_names {
        state.match_state.campaign_names = names;
    }
    state.match_state.map_basic = map.basic;
    state.match_state.map_ranking = map.ranking;
    state.match_state.loaded_map_source = Some(map.source);
    state.match_state.loaded_map_hash = map.map_hash;
    state.match_state.match_presentation.terrain_grid = map.terrain_grid;
    state.frontend.shell_preview_overlay_registry = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|runtime| runtime.resources.overlay_registry.clone());

    state.match_state.match_presentation.combat_lights.clear();
    state.match_state.match_presentation.interpolation.clear();
    state.match_state.match_presentation.lasers.clear_on_load();
    #[cfg(feature = "ares")]
    state.match_state.match_presentation.flying_credits.borrow_mut().clear_on_load();
    state
        .match_state
        .match_presentation
        .talk_bubble
        .clear_on_load();
    state
        .match_state
        .match_presentation
        .electric_bolts
        .clear_on_load();
    if let Some(runtime) = state.match_state.sim_runtime.as_ref() {
        state
            .match_state
            .match_presentation
            .detail
            .borrow_mut()
            .configure_normal(&runtime.resources.rules.general.detail);
    }
    state
        .match_state
        .match_presentation
        .barrel_image_pitches
        .borrow_mut()
        .clear();
    state
        .match_state
        .match_presentation
        .line_trails
        .clear_on_load();
    state
        .match_state
        .match_presentation
        .hidden_super_anims
        .clear();
    state
        .match_state
        .match_presentation
        .super_timer_blinks
        .clear();
    sync_in_game_options_speed_from_sim(state);

    state.match_state.match_presentation.tile_atlas = assets.tile_atlas;
    state.match_state.match_presentation.building_zshape = assets.building_zshape;
    state.match_state.match_presentation.unit_atlas = assets.unit_atlas;
    // Both voxel caches resolve model images through the replaced RuleSet.
    *state.renderer.vxl_pose_frame_cache.borrow_mut() = Default::default();
    state.match_state.match_presentation.palette_set = assets.palette_set;
    state.match_state.match_presentation.sprite_atlas = assets.sprite_atlas;
    state.match_state.match_presentation.overlay_atlas = assets.overlay_atlas;
    state.match_state.match_presentation.bridge_atlas = assets.bridge_atlas;
    state.match_state.match_presentation.sidebar_cameo_atlas = assets.sidebar_cameo_atlas;
    state.match_state.match_presentation.sidebar_chrome = assets.sidebar_chrome;
    #[cfg(feature = "phobos")]
    {
        state.match_state.match_presentation.center_pause_menu_background =
            assets.center_pause_menu_background;
        state.match_state.match_presentation.sw_sidebar_art = assets.sw_sidebar_art;
        let significance = state
            .match_state
            .match_presentation
            .sw_sidebar
            .required_significance;
        state
            .match_state
            .match_presentation
            .sw_sidebar
            .configure(assets.sw_sidebar_ui, significance);
    }
    state.match_state.match_presentation.outcome_banner_side_art = assets.outcome_banner_side_art;
    // `Game_LoadUI_LoadSideData` resolves the dialog background pair once per
    // game load. The shell chrome atlas carries the three side slots, so a
    // changed pair drops it and the next paint rebuilds with the side's art;
    // the process-lifetime slot 0 (`PUDLGBGN`) is not part of the hook.
    #[cfg(feature = "ares")]
    {
        let side = state
            .match_state
            .sim_runtime
            .as_ref()
            .map_or(-1, |runtime| runtime.simulation.session().scenario_side());
        let pair = state.process_assets.side_dialog_background(side);
        if state.frontend.shell_dialog_background != pair {
            state.frontend.shell_dialog_background = pair;
            state.frontend.skirmish_shell_chrome = None;
        }
    }
    // GAME.FNT is process artwork, so retain the established process fallback
    // when optional font preparation did not produce a replacement.
    if let Some(ref fnt) = assets.fnt_file {
        state.renderer.bit_font = crate::render::bit_font::BitFont::from_fnt(
            &state.renderer.gpu.device,
            &state.renderer.gpu.queue,
            &state.renderer.batch_renderer,
            fnt,
        );
    }

    // The final sidebar projection resolves incoming side/radar authority.
    state.match_state.match_presentation.radar_anim = None;
    state.match_state.match_presentation.radar_animation_source = None;
    state.match_state.match_presentation.radar_content_insets = None;
    state.match_state.match_presentation.has_radar = false;
    state.match_state.match_presentation.software_cursor = assets.software_cursor;
    state
        .match_state
        .match_presentation
        .overlays
        .replace_from_source(map.overlays);
    state.match_state.match_presentation.terrain_objects = map.terrain_objects;
    state.match_state.match_presentation.overlay_names = assets.overlay_names;
    state.match_state.match_presentation.overlay_radar_colors = assets.overlay_radar_colors;
    state.match_state.match_presentation.house_color_map = assets.house_color_map;
    state.match_state.match_presentation.house_roster = map.house_roster;
    state.process_assets.csf = assets.csf;
    state.match_state.match_presentation.theater_name = map.theater_name;
    state.match_state.match_presentation.theater_ext = map.theater_ext;

    let live = state.match_state.sim_runtime.as_ref().and_then(|runtime| {
        runtime
            .view()
            .resolved_terrain()
            .map(|terrain| (terrain, &runtime.simulation))
    });
    let presentation = &mut state.match_state.match_presentation;
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
        crate::app::input::camera::playfield_presentation_authority(state);
    let minimap = state
        .match_state
        .match_presentation
        .terrain_grid
        .as_ref()
        .map(|grid| {
            let runtime = state.match_state.sim_runtime.as_ref();
            let overlay_registry = runtime.map(|runtime| &runtime.resources.overlay_registry);
            let rules = runtime.map(|runtime| &runtime.resources.rules);
            let overlay_data = build_minimap_overlay_data(
                state.match_state.match_presentation.overlays.as_slice(),
                &state.match_state.match_presentation.terrain_objects,
                overlay_registry,
                rules,
            );
            MinimapRenderer::new(
                &state.renderer.gpu.device,
                &state.renderer.gpu.queue,
                &state.renderer.batch_renderer,
                grid,
                runtime,
                &overlay_data,
                &state.match_state.match_presentation.overlay_radar_colors,
                &state.match_state.match_presentation.theater_name,
                playfield_bounds,
                playfield_revision,
            )
        });
    state.match_state.match_presentation.minimap = minimap;
    state.match_state.match_presentation.selection_overlay = Some(SelectionOverlay::new(
        &state.renderer.gpu,
        &state.renderer.batch_renderer,
        state.process_assets.manager(),
    ));
    // Phobos DigitalDisplay `Shape=`/`Palette=` assets, retained once per match
    // like `DigitalDisplayTypeClass`'s SHP pointers.
    #[cfg(feature = "phobos")]
    if let Some(assets) = state.process_assets.manager()
        && let Some(rules) = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|runtime| &runtime.resources.rules)
        && let Some(overlay) = state
            .match_state
            .match_presentation
            .selection_overlay
            .as_mut()
    {
        overlay.load_digital_display_shapes(
            &state.renderer.gpu.device,
            &state.renderer.gpu.queue,
            &state.renderer.batch_renderer,
            assets,
            rules.digital_display_types().types(),
        );
    }
    // Phobos placement preview strips embed the active theater's palette and
    // the local house colour; a map load invalidates them.
    #[cfg(feature = "phobos")]
    state
        .match_state
        .match_presentation
        .placement_preview_art
        .borrow_mut()
        .reset();
    // Full_Init 687BB2 -> Init_BeaconArt 4309D0; each row's ColorScheme is
    // its House's (+0x16054).
    state.match_state.match_presentation.beacon_art =
        state.process_assets.manager().and_then(|assets| {
            let runtime = state.match_state.sim_runtime.as_ref()?;
            let sim = &runtime.simulation;
            let colors = &state.match_state.match_presentation.house_color_map;
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
            Some(crate::render::beacon_art::BeaconArt::new(
                &state.renderer.gpu.device,
                &state.renderer.gpu.queue,
                &state.renderer.batch_renderer,
                assets,
                &ramps,
            ))
        });
    let shroud_buffer = state.process_assets.manager().and_then(|assets| {
        let grid = state
            .match_state
            .sim_runtime
            .as_ref()?
            .simulation
            .path_grid()?;
        let shp =
            crate::assets::shp_file::ShpFile::from_bytes(assets.get_ref("shroud.shp")?).ok()?;
        let (frame_pixels, cw, ch) = crate::render::shroud_buffer::extract_shp_brightness(&shp);
        let mut buffer = crate::render::shroud_buffer::ShroudBuffer::new(
            &state.renderer.gpu.device,
            &state.renderer.gpu.queue,
            [state.render_width(), state.render_height()],
            [grid.width(), grid.height()],
            frame_pixels,
            [cw, ch],
            crate::render::shroud_buffer::SHROUD_EDGE_LUT,
        );
        buffer.set_translucent_fog(&state.renderer.gpu.queue, state.translucent_fog_enabled());
        // Cell47EFE0/47F250 use FOG.SHP's raw A bytes when Scenario0x1000
        // is set. It belongs to this same tactical source, not a sprite atlas.
        let fog = crate::assets::shp_file::ShpFile::from_bytes(assets.get_ref("fog.shp")?).ok()?;
        let (frames, width, height) = crate::render::shroud_buffer::extract_shp_brightness(&fog);
        buffer.install_fog_frames(frames, [width, height]);
        Some(buffer)
    });
    state.match_state.match_presentation.shroud_buffer = shroud_buffer;
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
        crate::app::loading::pump::clear_loading_state(state);
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
    crate::app::match_runtime::scenario_pause::reset_clock(state);
    let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    crate::app::radar_movie::retire(state, wall_ms);
    crate::app::chrono_screen::retire(state, wall_ms);
    crate::app::input::dispatch::selection_navigation::reset_for_world_replacement(
        &mut state.match_state.input,
    );
    // Start_Scenario 00683D21 completes Read_Scenario before switching video
    // mode at 00683DF3. VERA keeps the window the player gave it: the match
    // takes the zoom pass's source size, already current for this window.
    crate::app::loading::pump::present_game_mode_blank(state);
    // Only fresh startup cancels its outgoing scenario-owned request. Saved
    // loading preserves the process Theme state until ThemeLoad721040.
    state.audio.cancel_scenario_theme_request();
    crate::app::reset_scenario_exit_runtime(state);
    let startup = result.scenario.startup;
    let returns_scenario_rng_to_multiplayer_shell = startup.launch_session().is_some();
    // A loaded world is not timed until the launch handoff reaches InGame.
    state.match_state.scenario_elapsed_clock.reset();
    #[cfg(feature = "telemetry-dashboard")]
    crate::app::telemetry_capture::reset_observation(state);
    crate::app::loading::pump::clear_loading_state(state);

    // Close the outgoing diagnostic timeline before replacing its runtime.
    crate::app::match_runtime::sim_tick::close_replay_segment_for_new_timeline(state);
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
    } else if let Some(outgoing) = state.match_state.sim_runtime.take() {
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
    state.match_state.sim_runtime = incoming.map(|(mut simulation, rules)| {
        simulation.install_fresh_process_main(&state.frontend.frontend_main_rng);
        simulation.publish_scenario_started();
        simulation.batch_path_searches();
        crate::sim::runtime::SimRuntime {
            simulation,
            resources: crate::sim::runtime::SimResources {
                overlay_registry: result.scenario.overlay_registry,
                terrain_template: result.scenario.resolved_terrain,
                rules,
            },
        }
    });
    if let Some(runtime) = state.match_state.sim_runtime.as_ref() {
        state
            .match_state
            .session
            .hand_mode_to_world(runtime.simulation.session().game_mode());
    }
    crate::app::audio_runtime::bind_match_eva_context(state);
    // Only an actually-carried lease returns. A fallback result must not wipe
    // the process-sticky manager already restored by its failure path.
    if let Some(manager) = result.asset_manager {
        state.process_assets.return_from_loading(manager);
    }
    let lighting_events = state
        .match_state
        .sim_runtime
        .as_mut()
        .map(|runtime| runtime.simulation.take_lighting_events());
    install_world_presentation(
        state,
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
    let wall_ms = state.radar_presentation_ms();
    state
        .match_state
        .match_presentation
        .power_bar_anim
        .reset(wall_ms);
    state
        .match_state
        .match_presentation
        .sidebar_projection
        .reset_cameo_seed();
    if let Some(runtime) = state.match_state.sim_runtime.as_mut() {
        runtime
            .simulation
            .set_input_delay_ticks(state.match_state.configured_input_delay_ticks);
    }

    // Map load hands over a world anchor point; the transition applies the
    // active tactical rectangle and live zoom.
    let (tactical_width, tactical_height) =
        crate::app::input::camera::tactical_viewport_size(state);
    let (camera_x, camera_y) = crate::app::input::camera::tactical_camera_top_left(
        (
            result.scenario.camera_anchor_x,
            result.scenario.camera_anchor_y,
        ),
        tactical_width as f32,
        tactical_height as f32,
        state.match_state.input.zoom_level,
    );
    crate::app::input::camera::reset_camera_for_new_scenario(state);
    state.match_state.input.cursor_visibility.reset();
    crate::app::input::camera::set_camera_position(
        &mut state.match_state.input,
        (camera_x, camera_y),
    );
    // gamemd's scenario reader fills all four camera bookmarks with the opening
    // view cell, so F1 before any Ctrl+F1 is a valid "go home".
    crate::app::input::camera::seed_view_bookmarks_from_opening_view(
        state,
        result.scenario.opening_view_cell,
    );
    state.match_state.input.reset_targeting_for_new_world();
    state.match_state.match_presentation.active_sidebar_tab = SidebarTab::default_active_tab();
    state.match_state.match_presentation.message_list = crate::ui::messages::MessageList::new(
        3,
        0,
        crate::ui::messages::MESSAGE_MAX_VISIBLE_RETAIL,
        0,
    );
    state.match_state.match_presentation.chat = Default::default();
    // Clear_Scene6851F0 -> 534450 resets BeaconManagerClass (53499B).
    // Display Init_Clear4A88C0 clears +11B0..+11B2, not beacon mode +11B4.
    state.match_state.match_presentation.beacons = Default::default();
    state.match_state.input.minimap_dragging = false;
    state.match_state.input.tactical_mouse.reset_for_new_view();
    state.match_state.input.keys_held.clear();
    state.match_state.input.cursor_x = tactical_width as f32 * 0.5;
    state.match_state.input.cursor_y = tactical_height as f32 * 0.5;
    state.platform.frame_pacer.reset_for_new_world();
    state.match_state.input.queued_order_mode = render::OrderMode::Move;
    state.match_state.local_owner_override = result.scenario.initial_local_owner;
    state.match_state.match_audio.reset_for_new_match();
    state.match_state.sandbox_full_visibility = result.scenario.sandbox_full_visibility;

    // `Start_Scenario @ 0x00683AB0` after `Read_Scenario`: `[Basic] Theme`
    // resolves through `From_Name` (`0x004758F0`); -1 -> `Stop(fade=1)` of the
    // LOADING stream, else `Queue_Song(index)`. `Main_Tick` then issues
    // `Queue_Song(-2)` and the audio pump's AI picks the first allowed track
    // once the fade lands. Theme AI borrows the installed process Main cursor;
    // `Is_Allowed`'s `Side=` gate compares the local player's side.
    let music_now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    crate::app::audio_runtime::bind_match_theme_context(state);
    if let Some(assets) = state.process_assets.manager() {
        state.audio.request_scenario_theme(
            state.match_state.map_basic.theme.as_deref(),
            assets,
            music_now_ms,
        );
    }

    let installation = match startup {
        crate::app::match_bootstrap::LoadingStartup::Campaign(prepared) => {
            let installed = state.match_state.startup.acknowledge_campaign(
                &prepared,
                state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .map(|rt| &rt.simulation),
                matches!(state.frontend.screen, GameScreen::Loading),
            );
            // GameExit::Victory (`0x00685D7A`), the restart (`0x00686506`)
            // and the defeat retry (`0x0068639A`) write the carry-over record
            // back after Start_Scenario through `0x004C6210`.
            if installed.is_ok()
                && let Some(record) = prepared.carry_over()
                && let Some(runtime) = state.match_state.sim_runtime.as_mut()
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
                    .as_ref()
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
                && let Some(runtime) = state.match_state.sim_runtime.as_mut()
            {
                runtime
                    .simulation
                    .restore_campaign_carry_over(record, &runtime.resources.rules);
            }
            crate::app::lan_service::install_gameplay(state).map_err(|error| error.to_string())
        }
        crate::app::match_bootstrap::LoadingStartup::Replay { .. } => {
            state.match_state.startup.clear();
            let playback = match (
                state.match_state.sim_runtime.as_mut(),
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
            if let Err(error) = playback {
                let message = format!("Replay startup failed: {error:#}");
                log::error!("{message}");
                state.frontend.external_launch_error = Some(message);
            }
            // A rejected replay is a frozen inspection view, never a fresh
            // playable match. Its diagnostics owner blocks every tick/input.
            Ok(())
        }
        crate::app::match_bootstrap::LoadingStartup::UnverifiedLegacy { .. }
        | crate::app::match_bootstrap::LoadingStartup::Generic { .. } => {
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
                title: "Startup Rejected".to_string(),
                detail: err.clone(),
            };
            log::error!("Scenario installation rejected startup: {err}");
        }
    }
    crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
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

#[cfg(test)]
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
