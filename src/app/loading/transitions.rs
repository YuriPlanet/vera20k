//! App state transitions: map loading into InGame, screen clearing.
//!
//! Extracted from app.rs for file-size limits.

use std::collections::{BTreeMap, HashMap};

use crate::app::loading::init;
use crate::app::match_runtime::sim_tick;
use crate::app::presentation::render;
use crate::map::basic::BasicSection;
use crate::map::houses::HouseRoster;
use crate::map::trigger_graph::TriggerGraph;
use crate::render::minimap::MinimapRenderer;
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
        scenario: init::ScenarioLoadInputs {
            startup: crate::match_bootstrap::LoadingStartup::Generic {
                selected_map_file: "fallback".to_string(),
            },
            map_source: crate::map::source::LoadedMapSource::LegacyFallback {
                label: "fallback".to_string(),
            },
            map_hash: None,
            basic: BasicSection::default(),
            terrain_grid: None,
            resolved_terrain: None,
            simulation: None,
            overlays: Vec::new(),
            terrain_objects: Vec::new(),
            waypoints: HashMap::new(),
            triggers: HashMap::new(),
            events: HashMap::new(),
            actions: HashMap::new(),
            trigger_graph: TriggerGraph::default(),
            overlay_registry: OverlayTypeRegistry::empty(),
            house_roster: HouseRoster::default(),
            height_map: BTreeMap::new(),
            bridge_height_map: BTreeMap::new(),
            rules: None,
            map_lighting_config: crate::map::lighting::LightingConfig::default(),
            theater_name: "TEMPERATE".to_string(),
            theater_ext: "tem".to_string(),
            initial_local_owner: None,
            sandbox_full_visibility: false,
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
            bridge_railing_atlas: None,
            sidebar_cameo_atlas: None,
            sidebar_chrome: None,
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

pub(crate) fn apply_map_load_result(state: &mut AppState, result: init::MapLoadResult) {
    crate::app::input::dispatch::selection_navigation::reset_for_world_replacement(
        &mut state.match_state.input,
    );
    // Start_Scenario 00683D21 completes Read_Scenario before switching video
    // mode at 00683DF3. Loading artwork stays at shell size; successful tactical
    // installation must size its camera/shroud against the applied game mode.
    crate::app::App::enter_game_window_mode(state);
    crate::app::loading::pump::present_game_mode_blank(state);
    crate::app::reset_scenario_exit_runtime(state);
    let startup = result.scenario.startup;
    let returns_scenario_rng_to_offline_shell = startup.launch_session().is_some();
    state.match_state.match_presentation.local_player_handle = startup
        .launch_session()
        .map(|launch| launch.player_name.clone());
    // A loaded world is not timed until the launch handoff reaches InGame.
    state.match_state.scenario_elapsed_clock.reset();
    state.match_state.match_presentation.tile_atlas = result.presentation.tile_atlas;
    state.match_state.match_presentation.building_zshape = result.presentation.building_zshape;
    crate::app::loading::pump::clear_loading_state(state);
    state.match_state.map_basic = result.scenario.basic;
    state.match_state.loaded_map_source = Some(result.scenario.map_source);
    state.match_state.loaded_map_hash = result.scenario.map_hash;
    state.match_state.match_presentation.terrain_grid = result.scenario.terrain_grid;
    state.match_state.match_presentation.height_map = result.scenario.height_map;
    state.match_state.match_presentation.bridge_height_map = result.scenario.bridge_height_map;
    state
        .match_state
        .match_presentation
        .installed_playfield_authority = None;
    state.frontend.shell_preview_overlay_registry = Some(result.scenario.overlay_registry.clone());
    // F10 lifecycle: a new match install closes the outgoing diagnostic
    // segment before the runtime slot is overwritten — the old install
    // dropped any unflushed segment silently. A failed close discards
    // rather than retains, so the new timeline cannot append under the
    // old header.
    crate::app::match_runtime::sim_tick::close_replay_segment_for_new_timeline(state);
    let match_rules = result.scenario.rules;
    state.match_state.sim_runtime =
        result
            .scenario
            .simulation
            .zip(match_rules)
            .map(|(simulation, rules)| crate::sim::runtime::SimRuntime {
                simulation,
                resources: crate::sim::runtime::SimResources {
                    overlay_registry: result.scenario.overlay_registry,
                    terrain_template: result.scenario.resolved_terrain,
                    rules,
                    trigger_graph: result.scenario.trigger_graph,
                    triggers: result.scenario.triggers,
                    events: result.scenario.events,
                    actions: result.scenario.actions,
                    waypoints: result.scenario.waypoints,
                },
            });
    state.match_state.match_presentation.combat_lights.clear();
    state.match_state.match_presentation.lasers.clear_on_load();
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
        .sinking_waterlines
        .borrow_mut()
        .clear();
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
    // A new simulation is a new scenario for `SidebarClass::AddCameo`'s
    // init gate: its first projection must seed the strip silently.
    state
        .match_state
        .match_presentation
        .sidebar_projection
        .reset_cameo_seed();
    sync_in_game_options_speed_from_sim(state);
    if let Some(sim) = state
        .match_state
        .sim_runtime
        .as_mut()
        .map(|rt| &mut rt.simulation)
    {
        sim.set_input_delay_ticks(state.match_state.configured_input_delay_ticks);
    }
    state.match_state.match_presentation.unit_atlas = result.presentation.unit_atlas;
    // Slope-transition sprites belong to the match's voxel models; the cache
    // otherwise keeps every earlier match's pages alive.
    *state.renderer.vxl_slope_transition_cache.borrow_mut() = Default::default();
    // The pose page's model memo resolves each type's `Image=` from the
    // match's rules, which the next match's mode and map can change.
    *state.renderer.vxl_pose_frame_cache.borrow_mut() = Default::default();
    state.match_state.match_presentation.palette_set = result.presentation.palette_set;
    state.match_state.match_presentation.sprite_atlas = result.presentation.sprite_atlas;
    state.match_state.match_presentation.overlay_atlas = result.presentation.overlay_atlas;
    state.match_state.match_presentation.bridge_atlas = result.presentation.bridge_atlas;
    state.match_state.match_presentation.bridge_railing_atlas =
        result.presentation.bridge_railing_atlas;
    state.match_state.match_presentation.sidebar_cameo_atlas =
        result.presentation.sidebar_cameo_atlas;
    state.match_state.match_presentation.sidebar_chrome = result.presentation.sidebar_chrome;
    if let Some(ref fnt) = result.presentation.fnt_file {
        state.renderer.bit_font = crate::render::bit_font::BitFont::from_fnt(
            &state.renderer.gpu,
            &state.renderer.batch_renderer,
            fnt,
        );
    }

    // A new match discards the outgoing radar presentation. Source selection
    // runs in the existing final sidebar refresh, after roster and the pinned
    // local owner have been installed, so it cannot observe the prior player.
    state.match_state.match_presentation.radar_anim = None;
    state.match_state.match_presentation.radar_animation_source = None;
    state.match_state.match_presentation.radar_content_insets = None;
    state.match_state.match_presentation.has_radar = false;

    state.match_state.match_presentation.software_cursor = result.presentation.software_cursor;
    state
        .match_state
        .match_presentation
        .overlays
        .replace_from_source(result.scenario.overlays);
    state.match_state.match_presentation.terrain_objects = result.scenario.terrain_objects;
    state.match_state.match_presentation.overlay_names = result.presentation.overlay_names;
    state.match_state.match_presentation.overlay_radar_colors =
        result.presentation.overlay_radar_colors;
    state.match_state.match_presentation.house_color_map = result.presentation.house_color_map;
    state.match_state.match_presentation.house_roster = result.scenario.house_roster;
    // F04: the app no longer stores a second ArtRegistry; presentation
    // borrows the sole copy owned by RuleSet (state.rules).
    state.process_assets.csf = result.presentation.csf;
    state.match_state.match_presentation.theater_name = result.scenario.theater_name;
    state.match_state.match_presentation.theater_ext = result.scenario.theater_ext;

    let runtime = state.match_state.sim_runtime.as_ref();
    let live = runtime.and_then(|rt| {
        rt.resources
            .terrain_template
            .as_ref()
            .map(|terrain| (terrain, &rt.simulation, &rt.resources.rules))
    });
    let presentation = &mut state.match_state.match_presentation;
    presentation.lighting.install(
        result.presentation.lighting_grid,
        result.scenario.map_lighting_config,
        presentation.in_game_options.detail_level,
        live,
    );
    // Map load hands over a world anchor point; the transition applies the
    // active tactical rectangle and live zoom.
    let (tactical_width, tactical_height) = crate::app::input::camera::tactical_viewport_size_px(
        state.render_width(),
        state.render_height(),
    );
    let (camera_x, camera_y) = crate::app::input::camera::tactical_camera_top_left(
        (
            result.scenario.camera_anchor_x,
            result.scenario.camera_anchor_y,
        ),
        tactical_width as f32,
        tactical_height as f32,
        state.match_state.input.zoom_level,
    );
    crate::app::input::camera::set_camera_position(state, (camera_x, camera_y));
    // gamemd's scenario reader fills all four camera bookmarks with the opening
    // view cell, so F1 before any Ctrl+F1 is a valid "go home".
    crate::app::input::camera::seed_view_bookmarks_from_current_view(state);
    // F11 slot: only an actually-carried manager returns (Loading ->
    // Available). The fallback result carries None — the old unconditional
    // assignment wiped the manager the failure path had just restored,
    // discarding the process-sticky MIX cache and theater identity.
    if let Some(manager) = result.asset_manager {
        state.process_assets.return_from_loading(manager);
    }
    state.match_state.input.targeting_mode = None;
    state.match_state.input.building_placement_preview = None;
    state.match_state.match_presentation.active_sidebar_tab = SidebarTab::default_active_tab();
    // Re-init the message surface per scenario (the native list is
    // re-initialized at scenario start): drops stale rows from the previous
    // game and any dangling pause span, so a pause→quit→new-map sequence
    // never folds the menu dwell into the new game's frozen-deadline clock.
    // Anchors mirror the AppState ctor; x/width re-sync on first use.
    state.match_state.match_presentation.message_list = crate::ui::messages::MessageList::new(
        3,
        0,
        crate::ui::messages::MESSAGE_MAX_VISIBLE_RETAIL,
        0,
    );
    state.match_state.match_presentation.message_clock =
        crate::ui::messages::PauseAwareClock::default();
    // The window keeps its title: gamemd names it once at CreateWindowExA
    // (`0x00777CC5`) and imports no SetWindowText.
    state.platform.window.set_cursor_visible(
        state
            .match_state
            .match_presentation
            .software_cursor
            .is_none(),
    );

    // Install current sim authority before the first camera/minimap frame. A
    // campaign trigger may already have changed LocalSize before presentation
    // objects exist; raw MapHeader LocalSize is not an acceptable substitute.
    let (playfield_bounds, playfield_revision) =
        crate::app::input::camera::sync_playfield_presentation_bounds(state);

    // Create minimap from terrain grid with overlay data.
    if let Some(grid) = &state.match_state.match_presentation.terrain_grid {
        let resolved_terrain = state
            .match_state
            .sim_runtime
            .as_ref()
            .and_then(|runtime| runtime.view().resolved_terrain());
        let overlay_registry = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|runtime| &runtime.resources.overlay_registry);
        let rules = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|runtime| &runtime.resources.rules);
        let overlay_data = build_minimap_overlay_data(
            state.match_state.match_presentation.overlays.as_slice(),
            &state.match_state.match_presentation.terrain_objects,
            overlay_registry,
            rules,
        );
        state.match_state.match_presentation.minimap = Some(MinimapRenderer::new(
            &state.renderer.gpu,
            &state.renderer.batch_renderer,
            grid,
            resolved_terrain,
            &overlay_data,
            &state.match_state.match_presentation.overlay_radar_colors,
            &state.match_state.match_presentation.theater_name,
            playfield_bounds,
            playfield_revision,
        ));
    }
    state.match_state.input.minimap_dragging = false;
    state.match_state.input.tactical_mouse = Default::default();
    state.match_state.input.keys_held.clear();
    let (tactical_width, tactical_height) = crate::app::input::camera::tactical_viewport_size_px(
        state.render_width(),
        state.render_height(),
    );
    state.match_state.input.cursor_x = tactical_width as f32 * 0.5;
    state.match_state.input.cursor_y = tactical_height as f32 * 0.5;

    // Create selection overlay for rendering highlights and drag rect.
    // Pass asset_manager so it can load pips.shp for authentic health bar pips.
    state.match_state.match_presentation.selection_overlay = Some(SelectionOverlay::new(
        &state.renderer.gpu,
        &state.renderer.batch_renderer,
        state.process_assets.manager(),
    ));

    // Create GPU ABuffer for per-pixel shroud darkening.
    // Loads SHROUD.SHP brightness data and the 256-byte edge LUT.
    if let Some(am) = state.process_assets.manager() {
        if let Some(grid) = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
            .and_then(crate::sim::world::Simulation::path_grid)
        {
            if let Some(shp_data) = am.get_ref("shroud.shp") {
                if let Ok(shp) = crate::assets::shp_file::ShpFile::from_bytes(shp_data) {
                    let (frame_pixels, cw, ch) =
                        crate::render::shroud_buffer::extract_shp_brightness(&shp);
                    state.match_state.match_presentation.shroud_buffer =
                        Some(crate::render::shroud_buffer::ShroudBuffer::new(
                            &state.renderer.gpu,
                            state.render_width(),
                            state.render_height(),
                            grid.width(),
                            grid.height(),
                            frame_pixels,
                            cw,
                            ch,
                            crate::render::shroud_buffer::SHROUD_EDGE_LUT,
                        ));
                }
            }
        }
    }

    state.platform.frame_pacer.reset_for_immediate_frame();
    state.match_state.input.queued_order_mode = render::OrderMode::Move;
    for group in &mut state.match_state.input.control_groups {
        group.clear();
    }
    // Ordinary identity is already bound in Simulation.session.current_house.
    // Keep a preference only for development worlds without that authority.
    state.match_state.local_owner_override = result.scenario.initial_local_owner;
    // F11: reset the whole per-match audio owner. The old reset cleared only
    // three EVA latches — the tick-indexed under-attack suppression window
    // carried into the new match (whose tick counter restarts at 0) and
    // silenced the under-attack EVA line for its first ~30 seconds, and the
    // sound-event queue kept the previous match's undrained events.
    state.match_state.match_audio.reset_for_new_match();
    state.match_state.sandbox_full_visibility = result.scenario.sandbox_full_visibility;

    // `Start_Scenario @ 0x00683AB0` after `Read_Scenario`: `[Basic] Theme`
    // resolves through `From_Name` (`0x004758F0`); -1 -> `Stop(fade=1)` of the
    // LOADING stream, else `Queue_Song(index)`. `Main_Tick` then issues
    // `Queue_Song(-2)` and the audio pump's AI picks the first allowed track
    // once the fade lands. Theme AI borrows the installed process Main cursor;
    // `Is_Allowed`'s `Side=` gate compares the local player's side.
    let music_now_ms = sim_tick::monotonic_frame_pacer_ms(state, std::time::Instant::now());
    let local_side = state.match_state.sim_runtime.as_ref().and_then(|rt| {
        let simulation = &rt.simulation;
        state
            .match_state
            .local_player_owner()
            .and_then(|owner| {
                crate::sim::house_state::house_state_for_owner(
                    &simulation.houses,
                    owner,
                    &simulation.interner,
                )
            })
            .map(|house| i32::from(house.side_index))
    });
    // `Side=` names resolve against the live `[Sides]` registry (native
    // `0x004756F0` → `0x006A46D0`); unresolved names never match any player.
    let side_names: Vec<String> = state
        .audio
        .theme
        .entries()
        .iter()
        .filter_map(|entry| entry.side_name.clone())
        .collect();
    let side_indices: Vec<(String, i32)> = state
        .rules()
        .map(|rules| {
            side_names
                .iter()
                .filter_map(|name| {
                    rules
                        .side_index(name)
                        .map(|index| (name.to_ascii_uppercase(), i32::from(index.0)))
                })
                .collect()
        })
        .unwrap_or_default();
    if let Some(assets) = state.process_assets.manager() {
        state.audio.request_scenario_theme(
            state.match_state.map_basic.theme.as_deref(),
            assets,
            crate::audio::theme::ThemeAllowContext {
                local_side,
                // Skirmish (`g_GameMode != 0`) skips the campaign `Scenario=` gate.
                campaign_scenario: None,
            },
            |name| {
                let wanted = name.to_ascii_uppercase();
                side_indices
                    .iter()
                    .find(|(candidate, _)| *candidate == wanted)
                    .map(|(_, index)| *index)
            },
            music_now_ms,
        );
    }

    match startup {
        crate::match_bootstrap::LoadingStartup::Accepted(prepared) => {
            let receipt = state.match_state.startup.acknowledge(
                prepared,
                state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .map(|rt| &rt.simulation),
                matches!(state.frontend.screen, GameScreen::Loading),
            );

            match receipt {
                Ok(()) => {
                    let now_ms =
                        sim_tick::monotonic_frame_pacer_ms(state, std::time::Instant::now());
                    state.match_state.scenario_elapsed_clock.start(now_ms);
                    state.frontend.screen = GameScreen::InGame;
                    state
                        .frontend
                        .offline_skirmish_runtime
                        .mark_gameplay_rng_return_pending();
                    log::info!("Transitioned to InGame after Rust L0 acknowledgement");
                }
                Err(err) => {
                    state.match_state.startup.clear();
                    state.frontend.screen = GameScreen::MissionResult {
                        title: "Startup Rejected".to_string(),
                        detail: err.clone(),
                    };
                    log::error!("Accepted startup failed closed at Rust L0: {err}");
                }
            }
        }
        crate::match_bootstrap::LoadingStartup::UnverifiedLegacy { .. }
        | crate::match_bootstrap::LoadingStartup::Generic { .. } => {
            state.match_state.startup.clear();
            let now_ms = sim_tick::monotonic_frame_pacer_ms(state, std::time::Instant::now());
            state.match_state.scenario_elapsed_clock.start(now_ms);
            state.frontend.screen = GameScreen::InGame;
            if returns_scenario_rng_to_offline_shell {
                state
                    .frontend
                    .offline_skirmish_runtime
                    .mark_gameplay_rng_return_pending();
            }
            log::info!("Transitioned to InGame on noncertifying startup path");
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
) -> Vec<crate::render::minimap::MinimapOverlayDatum> {
    use crate::render::minimap::{
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
        let owner = sim.interner.intern("Local");
        sim.houses
            .insert(owner, HouseState::new(owner, 0, None, true, 0, 10));
        sim.session.house_order.push(owner);
        sim.queue_command(CommandEnvelope::new(
            owner,
            1,
            Command::SetGameSpeed { speed: 4 },
        ));

        assert_eq!(sim.projected_in_game_options_speed(), Some(4));
    }
}
