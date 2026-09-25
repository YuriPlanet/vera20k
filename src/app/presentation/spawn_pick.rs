//! Spawn-pick phase — player sees the full map and clicks a waypoint to start.
//!
//! During SpawnPick the entire map is rendered without fog or simulation ticking.
//! Multiplayer start waypoints (0..=7) are drawn as clickable markers.
//! When the player clicks a marker, MCVs are seeded and the game transitions
//! to InGame.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::frontend::skirmish::seed_skirmish_opening_if_needed;
use crate::app::loading::init_helpers::build_entity_atlases;
use crate::app::presentation::render;
use crate::map::terrain;
use crate::map::waypoints;
use crate::ui::game_screen::GameScreen;
use crate::ui::main_menu::StartPosition;

/// Radius (in screen pixels) around a waypoint marker that counts as a click.
const WAYPOINT_CLICK_RADIUS: f32 = 40.0;

/// Check if the cursor is over a waypoint marker and return its index if so.
pub(crate) fn hovered_waypoint(state: &AppState) -> Option<usize> {
    let starts =
        waypoints::multiplayer_start_waypoints(&state.match_state.match_presentation.waypoints);
    let cx: f32 = state.match_state.input.cursor_x;
    let cy: f32 = state.match_state.input.cursor_y;

    for (i, wp) in starts.iter().enumerate() {
        let z: u8 = state
            .height_map()
            .get(&(wp.rx, wp.ry))
            .copied()
            .unwrap_or(0);
        let (world_x, world_y) = terrain::iso_to_screen(wp.rx, wp.ry, z);
        let screen_x: f32 = world_x - state.match_state.input.camera_x;
        let screen_y: f32 = world_y - state.match_state.input.camera_y;

        let dx: f32 = cx - screen_x;
        let dy: f32 = cy - screen_y;
        if dx * dx + dy * dy <= WAYPOINT_CLICK_RADIUS * WAYPOINT_CLICK_RADIUS {
            return Some(i);
        }
    }
    None
}

/// Handle a left-click during SpawnPick: if the player clicked a waypoint,
/// seed MCVs and transition to InGame. Returns true if a waypoint was clicked.
pub(crate) fn handle_spawn_pick_click(state: &mut AppState) -> bool {
    let Some(wp_idx) = hovered_waypoint(state) else {
        return false;
    };

    let starts =
        waypoints::multiplayer_start_waypoints(&state.match_state.match_presentation.waypoints);
    if wp_idx >= starts.len() {
        return false;
    }

    log::info!(
        "Player picked spawn waypoint {} at ({}, {})",
        starts[wp_idx].index,
        starts[wp_idx].rx,
        starts[wp_idx].ry,
    );

    // Update skirmish settings to use the chosen position, then seed MCVs.
    state.frontend.skirmish_settings.start_position = StartPosition::Position(wp_idx as u8);

    // Build temp map data before borrowing state.simulation mutably.
    let temp_map = build_temp_map_data_for_seeding(state);
    let seeded_owner: Option<String> = if let Some(rt) = state.match_state.sim_runtime.as_mut() {
        let resources = &rt.resources;
        let ruleset = &resources.rules;
        let sim = &mut rt.simulation;
        seed_skirmish_opening_if_needed(
            sim,
            &temp_map,
            &state.match_state.match_presentation.house_roster,
            ruleset,
            &resources.height_map,
            &resources.overlay_registry,
            &state.frontend.skirmish_settings,
        )
    } else {
        None
    };

    // Set up AI players and rebuild entity atlases now that MCVs are spawned.
    if let Some(ref local_owner) = seeded_owner {
        if let Some(sim) = state
            .match_state
            .sim_runtime
            .as_mut()
            .map(|rt| &mut rt.simulation)
        {
            // F10: sim owns both writes; the app only names the local owner.
            sim.register_ai_players_from_roster(
                &state.match_state.match_presentation.house_roster,
                local_owner,
            );
            // Ensure the local player is marked human even if the map lacks PlayerControl=yes.
            sim.mark_house_human(local_owner);
        }
        // Rebuild entity atlases to include the newly spawned MCVs.
        if let Some(rt) = state.match_state.sim_runtime.as_ref() {
            let sim = &rt.simulation;
            let bound_rules = Some(&rt.resources.rules);
            let asset_manager = state.process_assets.manager();
            if let Some(assets) = asset_manager {
                let (new_unit_atlas, new_sprite_atlas, new_palette_set) = build_entity_atlases(
                    sim,
                    assets,
                    &state.renderer.gpu,
                    &state.renderer.batch_renderer,
                    &state.match_state.match_presentation.theater_ext,
                    &state.match_state.match_presentation.theater_name,
                    bound_rules,
                    bound_rules.map(|rules| &rules.art_registry),
                    &rt.resources.overlay_registry,
                    &state.match_state.match_presentation.house_color_map,
                    None, // entity_unit_palette — atlas builder loads it from assets
                    None, // cell palette reloads from the active theater archive
                );
                state.match_state.match_presentation.unit_atlas = new_unit_atlas;
                state.match_state.match_presentation.sprite_atlas = new_sprite_atlas;
                state.match_state.match_presentation.palette_set = new_palette_set;
            }
        }
    }

    // Spawn-pick completes match launch: pin the match-scoped local player
    // here too (same contract as the skirmish-session launch path).
    state.match_state.local_player_owner = seeded_owner.clone();
    state.match_state.local_owner_override = seeded_owner;
    state.match_state.input.spawn_pick_pending = false;

    // Center camera on the chosen spawn position, using the tactical viewport
    // (window minus the sidebar column) rather than the whole window.
    let chosen_wp = starts[wp_idx];
    crate::app::input::camera::center_camera_on_cell(state, chosen_wp.rx, chosen_wp.ry);
    // Spawn pick re-anchors the opening view, so the camera bookmarks are
    // re-seeded with it — the same "all four slots hold the starting view"
    // state gamemd's scenario load leaves behind.
    state
        .match_state
        .input
        .view_bookmarks
        .seed_all(chosen_wp.rx, chosen_wp.ry);

    // Reset timing for clean InGame start.
    state.platform.frame_pacer.reset_for_immediate_frame();
    let now_ms = crate::app::match_runtime::sim_tick::monotonic_frame_pacer_ms(
        state,
        std::time::Instant::now(),
    );
    state.match_state.scenario_elapsed_clock.start(now_ms);

    state.frontend.screen = GameScreen::InGame;
    crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
    log::info!("SpawnPick complete — transitioned to InGame");
    true
}

/// Build a minimal MapFile for seed_skirmish_opening_if_needed.
/// Only waypoints and ini matter for seeding; all other fields are defaults.
fn build_temp_map_data_for_seeding(state: &AppState) -> crate::map::map_file::MapFile {
    use crate::map::map_file::{MapFile, MapHeader};
    use crate::rules::ini_parser::IniFile;

    MapFile {
        header: MapHeader {
            theater: state.match_state.match_presentation.theater_name.clone(),
            fill: "Clear".to_string(),
            level: 0,
            width: 0,
            height: 0,
            local_left: 0,
            local_top: 0,
            local_width: 0,
            local_height: 0,
        },
        basic: crate::map::basic::BasicSection::default(),
        briefing: crate::map::briefing::BriefingSection::default(),
        special_flags: crate::map::basic::SpecialFlagsSection::default(),
        ini: IniFile::from_str(""),
        cells: Vec::new(),
        iso_map_pack_lookups: Vec::new(),
        entities: Vec::new(),
        overlays: Vec::new(),
        authored_overlay_packs: crate::map::map_file::AuthoredOverlayPackSlot::empty(),
        smudges: Vec::new(),
        terrain_objects: Vec::new(),
        waypoints: state.match_state.match_presentation.waypoints.clone(),
        cell_tags: std::collections::HashMap::new(),
        tags: std::collections::HashMap::new(),
        triggers: std::collections::HashMap::new(),
        events: std::collections::HashMap::new(),
        actions: std::collections::HashMap::new(),
        trigger_graph: crate::map::trigger_graph::TriggerGraph::default(),
        local_variables: std::collections::HashMap::new(),
        explicit_tubes: Vec::new(),
        preview: crate::map::preview::PreviewSection::default(),
    }
}

/// Register non-local playable houses as AI opponents (same logic as `loading::init`).
/// Render the SpawnPick phase: full map visible, no fog, no simulation tick.
///
/// Temporarily enables sandbox visibility so the entire map is shown.
pub(crate) fn render_spawn_pick(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
    destination_view: &wgpu::TextureView,
) -> anyhow::Result<()> {
    // Temporarily enable sandbox visibility so the whole map is visible.
    let prev_visibility = state.match_state.sandbox_full_visibility;
    state.match_state.sandbox_full_visibility = true;
    let result = if state.renderer.upscale_pass.is_some() {
        let game_depth = state
            .renderer
            .upscale_pass
            .as_ref()
            .unwrap()
            .depth_view()
            .clone();
        let saved_depth = std::mem::replace(&mut state.renderer.depth_view, game_depth);
        let result = render::render_game(state, encoder);
        state.renderer.depth_view = saved_depth;
        result
    } else {
        render::render_game(state, encoder)
    };
    state.match_state.sandbox_full_visibility = prev_visibility;
    result?;
    if let Some(upscale) = state.renderer.upscale_pass.as_ref() {
        state
            .renderer
            .combat_light_renderer
            .copy_to(encoder, upscale.color_texture());
        upscale.draw(encoder, destination_view);
    } else {
        state
            .renderer
            .combat_light_renderer
            .copy_to(encoder, destination);
    }
    Ok(())
}

/// Draw the SpawnPick egui overlay: instructions + hovered waypoint info.
pub(crate) fn draw_spawn_pick_overlay(ctx: &egui::Context, state: &AppState) {
    let starts =
        waypoints::multiplayer_start_waypoints(&state.match_state.match_presentation.waypoints);
    let hovered = hovered_waypoint(state);

    // Top-center banner with instructions.
    egui::Area::new(egui::Id::new("spawn_pick_banner"))
        .anchor(egui::Align2::CENTER_TOP, [0.0, 20.0])
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_premultiplied(0, 0, 0, 200))
                .inner_margin(egui::Margin::symmetric(20, 12))
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("Choose Your Starting Position")
                                .size(24.0)
                                .strong()
                                .color(egui::Color32::WHITE),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(
                                "Click a marker on the map to place your MCV. Scroll to explore the map.",
                            )
                            .size(14.0)
                            .color(egui::Color32::from_rgb(200, 200, 200)),
                        );
                        if let Some(idx) = hovered {
                            ui.add_space(4.0);
                            let wp = starts[idx];
                            ui.label(
                                egui::RichText::new(format!(
                                    "Position {} — Cell ({}, {})",
                                    idx + 1,
                                    wp.rx,
                                    wp.ry
                                ))
                                .size(16.0)
                                .color(egui::Color32::from_rgb(100, 255, 100)),
                            );
                        }
                    });
                });
        });
}
