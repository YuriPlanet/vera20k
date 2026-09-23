//! In-game rendering and draw-pass orchestration.
//!
//! `render_game()` is the per-frame entry point. It runs a 7-phase pipeline:
//!
//! 1. **World instances** — terrain tiles, map overlays, bridges, VXL units,
//!    SHP buildings/infantry, AnimClass objects, damage fires, fog snapshots
//! 2. **Debug instances** — pathgrid, cell grid, heightmap overlays (toggled by hotkey)
//! 3. **Shroud ABuffer** - CPU shroud buffer rebuilt before UI overlays sample it
//! 4. **UI instances** - minimap dots, selection brackets, health bars, placement preview
//! 5. **Sidebar instances** - chrome, cameos, text, minimap rect, radar animation
//! 6. **Upload** - all instance vectors uploaded to GPU buffer pool
//! 7. **Draw** - render pass created, draw calls dispatched in layer order
//!
//! ## Sub-modules
//! - `build_instances` — phase 1-4 builders: named functions + structs per phase
//! - `draw_passes` — phase 6: render pass creation and GPU draw call dispatch
//! - `merge_passes` — Y-sorted multi-way merge algorithm for interleaving atlas textures
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

mod build_instances;
mod draw_passes;
pub(crate) mod draw_plan_lowering;
mod merge_passes;
pub(crate) mod minimap_transaction;

use anyhow::Result;

// Re-export shared types so any remaining `use crate::app::presentation::render::Foo` imports still compile.
// New code should import from `crate::app::types` directly.
pub(crate) use crate::app::types::*;

use crate::app::AppState;
use crate::app::input::commands::preferred_local_owner_name;
use crate::render::batch::InstanceBufferPool;
use crate::sidebar::SidebarView;

use build_instances::{DebugInstances, SidebarInstances, UiInstances, WorldInstances};

/// Exact sidebar-related instance counts emitted by the production render path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GameRenderInstanceCounts {
    pub minimap: usize,
    pub viewport_rect: usize,
    pub radar_animation: usize,
}

impl GameRenderInstanceCounts {
    fn from_lengths(minimap: usize, viewport_rect: usize, radar_animation: usize) -> Self {
        Self {
            minimap,
            viewport_rect,
            radar_animation,
        }
    }
}

/// Output retained from one production game render.
///
/// `sidebar_view` is the existing UI handoff. The counts are observation-only
/// evidence derived from the same vectors uploaded and dispatched this frame.
pub(crate) struct GameRenderOutput {
    pub sidebar_view: Option<SidebarView>,
    pub instance_counts: GameRenderInstanceCounts,
}

/// Render one in-game frame: terrain, units, overlays, UI, sidebar.
///
/// Orchestrates the 7-phase pipeline described in the module doc.
/// Each phase is a named function call — see `build_instances` for details.
pub(crate) fn render_game(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
) -> Result<GameRenderOutput> {
    // RadarClass::Draw 653100 uses wall-clock buckets, not simulation ticks.
    let wall_ms = crate::app::match_runtime::sim_tick::monotonic_frame_pacer_ms(
        state, std::time::Instant::now(),
    );
    if let Some(radar) = state.match_state.match_presentation.radar_anim.as_mut() {
        radar.tick(&state.renderer.gpu, wall_ms);
    }
    let (sw, sh) = (state.render_width() as f32, state.render_height() as f32);

    // Trigger action 0x28 mutates scroll/radar authority inside the committed
    // sim frame. Install and clamp it before constructing any world instances,
    // so contraction/expansion is visible in this very presentation frame.
    crate::app::input::camera::clamp_camera_to_playable_area(state, sw, sh);

    let local_owner = preferred_local_owner_name(state);
    // Effective viewport in world pixels — zoom shrinks what's visible.
    let z = state.match_state.input.zoom_level;
    let vsw = sw / z;
    let vsh = sh / z;

    // Phase 1: Build game-world instances (terrain, overlays, entities).
    let world = build_instances::build_world_instances(state, vsw, vsh);

    // Phase 2: Build debug overlay instances (pathgrid, cell grid, heightmap).
    let debug = build_instances::build_debug_instances(state, vsw, vsh);

    // Phase 3: Rebuild shroud ABuffer (CPU blit + GPU upload). The final
    // building-bracket front redraw samples this CPU buffer during UI build.
    let rw = state.render_width();
    let rh = state.render_height();
    // F10 cone: the render feed reads through SimView getters. This site
    // holds `&mut state.shroud_buffer`, so it keeps the field chain and views
    // the runtime directly for split borrows.
    if let Some(ref mut shroud_buf) = state.match_state.match_presentation.shroud_buffer {
        if !state.match_state.sandbox_full_visibility {
            if let (Some(rt), Some(owner)) = (state.match_state.sim_runtime.as_ref(), &local_owner)
            {
                let view = rt.view();
                let owner_id = view.interner().get(owner).unwrap_or_default();
                shroud_buf.rebuild_if_needed(
                    &state.renderer.gpu,
                    view.fog(),
                    owner_id,
                    state.match_state.input.camera_x,
                    state.match_state.input.camera_y,
                    rw,
                    rh,
                    state.match_state.input.zoom_level,
                );
            }
        }
    }

    // Phase 4: Update minimap + build UI instances (selection, health, placement).
    build_instances::update_minimap(state, &local_owner);
    let ui = build_instances::build_ui_instances(state, vsw, vsh);

    // Phase 5: Build sidebar instances.
    let sidebar = build_instances::build_sidebar_instances(state);

    // Phase 6: Upload all instances to GPU buffer pool.
    upload_to_gpu(state, &world, &debug, &ui, &sidebar);
    state
        .match_state
        .match_presentation
        .cached_overlay_instances = world.overlay;

    let combat_lights = state
        .match_state
        .match_presentation
        .combat_lights
        .draw_records();
    state.renderer.combat_light_renderer.prepare(
        &state.renderer.gpu,
        &combat_lights,
        [sw, sh],
        [
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ],
        state.match_state.input.zoom_level,
    );
    let composition_view = state.renderer.combat_light_renderer.composition_view();
    let (_, tactical_y, _, _) = crate::app::input::camera::tactical_viewport_px(state);
    // Shader row coordinates are unscaled world pixels; scissor uses target
    // pixels. Native zoom1 is exact, and scaled views preserve that unit frame.
    let native_z_origin_y = tactical_y as f32 / state.match_state.input.zoom_level;
    state.renderer.batch_renderer.update_native_z_origin(&state.renderer.gpu.queue, native_z_origin_y);
    state.renderer.terrain_draw_renderer.prepare(
        &state.renderer.gpu.device,
        state.renderer.combat_light_renderer.composition_texture(),
        &state.renderer.depth_view,
        state.renderer.batch_renderer.camera_uniform(),
    );

    // Phase 7: Dispatch draw calls in render order.
    draw_passes::dispatch_draw_passes(
        state,
        encoder,
        &composition_view,
        &draw_passes::DrawPassData {
            overlay_render_z: &world.overlay_render_z,
            ground: &world.ground,
            unit_instances: &world.unit,
            unit_pages: &world.unit_pages,
            unit_transition_paged: &world.unit_transition_paged,
            shp_paged: &world.shp_paged,
            top_unit_pages: &world.top_unit_pages,
            top_shp_pages: &world.top_shp_pages,
            ghost_page: ui.ghost_page,
        },
    );
    // Return unit instances vec to AppState (deferred until after the draw pass
    // because the multi-way merge needs the CPU-side Y values).
    state.match_state.match_presentation.cached_unit_instances = world.unit;
    state.match_state.match_presentation.cached_unit_pages = world.unit_pages;
    Ok(GameRenderOutput {
        instance_counts: sidebar.emitted_instance_counts(),
        sidebar_view: sidebar.view,
    })
}

/// Upload all per-frame instance vectors to the GPU buffer pool.
///
/// The pool reuses GPU buffers across frames to avoid per-frame allocation.
/// Buffer names here must match the keys used in `draw_passes::dispatch_draw_passes`.
fn upload_to_gpu(
    state: &mut AppState,
    world: &WorldInstances,
    debug: &DebugInstances,
    ui: &UiInstances,
    sidebar: &SidebarInstances,
) {
    // A5 chat/system message lines (GAME.FNT atlas) — chat draws before the
    // tooltip (O10). Built before the pool borrow (it reads &AppState).
    let message_text = crate::app::input::messages::build_message_text_instances(state);

    // A4 in-game tooltip: built before the pool borrow (it reads &AppState);
    // fill (darken texture) + text (GAME.FNT atlas), drawn after the chat
    // overlay and before the software cursor (O10).
    let (tooltip_fill, tooltip_text) = crate::app::input::tooltips::build_tooltip_instances(state);

    let pool: &mut InstanceBufferPool = &mut state.renderer.instance_pool;

    // Debug overlays
    pool.upload(&state.renderer.gpu, "debug_pathgrid", &debug.pathgrid);
    pool.upload(&state.renderer.gpu, "debug_cell_grid", &debug.cell_grid);
    pool.upload(&state.renderer.gpu, "debug_path", &debug.path);
    pool.upload(&state.renderer.gpu, "debug_heightmap", &debug.heightmap);

    // Terrain + overlays
    pool.upload(&state.renderer.gpu, "terrain", &world.terrain.normal);
    pool.upload(&state.renderer.gpu, "overlay", &world.overlay);
    pool.upload(
        &state.renderer.gpu,
        "ground_objects",
        &world.ground.instances,
    );
    pool.upload(
        &state.renderer.gpu,
        "overlay_bridge_body",
        &world.bridge_body,
    );
    pool.upload(
        &state.renderer.gpu,
        "overlay_bridge_body_shadow",
        &world.bridge_body_shadow,
    );
    pool.upload(
        &state.renderer.gpu,
        "overlay_bridge_railing",
        &world.bridge_railing,
    );
    // Smudges: drawn inside the terrain layer, before the bridge body and
    // before overlays, matching the native per-cell tile-then-smudge dispatch.
    pool.upload(&state.renderer.gpu, "smudge", &world.smudge);

    // Entities (VXL + SHP)
    pool.upload(&state.renderer.gpu, "unit", &world.unit);
    const UNIT_TRANSITION_KEYS: [&str; 4] = [
        "unit_transition_p0",
        "unit_transition_p1",
        "unit_transition_p2",
        "unit_transition_p3",
    ];
    for (i, page_inst) in world.unit_transition_paged.iter().enumerate() {
        if let Some(key) = UNIT_TRANSITION_KEYS.get(i) {
            pool.upload(&state.renderer.gpu, key, page_inst);
        }
    }
    const SHP_PAGE_KEYS: [&str; 4] = ["shp_p0", "shp_p1", "shp_p2", "shp_p3"];
    for (i, page_inst) in world.shp_paged.iter().enumerate() {
        if i < SHP_PAGE_KEYS.len() {
            pool.upload(&state.renderer.gpu, SHP_PAGE_KEYS[i], page_inst);
        }
    }
    // The band above Ground (gamemd layers 3 and 4) — drawn after every ground
    // object. Voxel bodies and SHP bodies keep separate streams because they
    // sample different atlases; the band is unsorted either way.
    pool.upload(&state.renderer.gpu, "unit_top", &world.top_unit);
    pool.upload(&state.renderer.gpu, "shp_top", &world.top_shp);
    // (No `building_turret` buffer: a building's voxel turret rides the `unit`
    // stream and is drawn inside the sorted ground pass, as gamemd draws it.)
    // PixelFX water/ore sparkles — drawn after the ground object pass.
    // Empty when the live `[Options] DetailLevel` projection is zero.
    pool.upload(&state.renderer.gpu, "cell_sparkles", &world.cell_sparkles);
    pool.upload(&state.renderer.gpu, "weapon_waves", &world.weapon_waves);
    pool.upload(
        &state.renderer.gpu,
        "spotlight_type16",
        &world.spotlight_type16,
    );
    const PARTICLE_KEYS: [&str; 4] = ["particle_p0", "particle_p1", "particle_p2", "particle_p3"];
    for (i, page_inst) in world.particle_paged.iter().enumerate() {
        if i < PARTICLE_KEYS.len() {
            pool.upload(&state.renderer.gpu, PARTICLE_KEYS[i], page_inst);
        }
    }

    // UI overlays
    pool.upload(&state.renderer.gpu, "drag", &ui.drag);
    pool.upload(
        &state.renderer.gpu,
        "selection_brackets_back",
        &ui.bracket_back,
    );
    pool.upload(
        &state.renderer.gpu,
        "selection_brackets_front_first",
        &ui.bracket_front_first,
    );
    pool.upload(
        &state.renderer.gpu,
        "selection_brackets_front",
        &ui.bracket_front,
    );
    pool.upload(
        &state.renderer.gpu,
        "building_radius_rings",
        &ui.radius_ring,
    );
    pool.upload(&state.renderer.gpu, "status_building", &ui.building_status);
    pool.upload(&state.renderer.gpu, "bomb_clocks", &ui.bomb_clock);
    pool.upload(&state.renderer.gpu, "occupant_pips", &ui.occupant_pip);
    pool.upload(&state.renderer.gpu, "status_unit_bg", &ui.unit_status_bg);
    pool.upload(
        &state.renderer.gpu,
        "status_unit_fill",
        &ui.unit_status_fill,
    );
    pool.upload(&state.renderer.gpu, "cargo_pips", &ui.cargo_pip);
    pool.upload(&state.renderer.gpu, "software_cursor", &ui.software_cursor);
    pool.upload(&state.renderer.gpu, "placement_valid", &ui.placement_valid);
    pool.upload(
        &state.renderer.gpu,
        "placement_invalid",
        &ui.placement_invalid,
    );
    pool.upload(&state.renderer.gpu, "placement_ghost", &ui.placement_ghost);
    pool.upload(&state.renderer.gpu, "placement_wall_ghost", &ui.wall_ghost);
    pool.upload(
        &state.renderer.gpu,
        "factory_rally_first",
        &ui.factory_rally_first,
    );
    pool.upload(&state.renderer.gpu, "target_lines", &ui.target_line);
    pool.upload(
        &state.renderer.gpu,
        "factory_rally_second",
        &ui.factory_rally_second,
    );

    // Sidebar + minimap
    pool.upload(&state.renderer.gpu, "minimap", &sidebar.minimap);
    pool.upload(&state.renderer.gpu, "viewport_rect", &sidebar.viewport_rect);
    pool.upload(
        &state.renderer.gpu,
        "radar_content_boundary",
        &sidebar.content_boundary,
    );
    pool.upload(&state.renderer.gpu, "sidebar", &sidebar.sidebar);
    pool.upload(&state.renderer.gpu, "sidebar_chrome", &sidebar.chrome);
    pool.upload(&state.renderer.gpu, "radar_anim", &sidebar.radar_anim);
    pool.upload(&state.renderer.gpu, "sidebar_cameo", &sidebar.cameo);
    pool.upload(&state.renderer.gpu, "sidebar_gclock", &sidebar.gclock);
    pool.upload(
        &state.renderer.gpu,
        "sidebar_cameo_overlay",
        &sidebar.cameo_overlay,
    );
    pool.upload(&state.renderer.gpu, "sidebar_text", &sidebar.text);
    pool.upload(&state.renderer.gpu, "message_text", &message_text);
    pool.upload(&state.renderer.gpu, "tooltip_fill", &tooltip_fill);
    pool.upload(&state.renderer.gpu, "tooltip_text", &tooltip_text);
}

#[cfg(test)]
mod tests {
    include!("../render_tests.rs");

    #[test]
    fn game_render_counts_preserve_exact_emitted_lengths() {
        let sprite = crate::render::batch::SpriteInstance::default();
        let instances = super::build_instances::SidebarInstances {
            sidebar: Vec::new(),
            chrome: Vec::new(),
            cameo: Vec::new(),
            gclock: Vec::new(),
            cameo_overlay: Vec::new(),
            text: Vec::new(),
            minimap: vec![sprite],
            viewport_rect: vec![sprite; 4],
            content_boundary: vec![sprite; 4],
            radar_anim: vec![sprite; 2],
            view: None,
        };
        let counts = instances.emitted_instance_counts();

        assert_eq!(counts.minimap, 1);
        assert_eq!(counts.viewport_rect, 4);
        assert_eq!(counts.radar_animation, 2);
        assert_eq!(instances.content_boundary.len(), 4);
    }
}
