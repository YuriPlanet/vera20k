//! Per-frame sprite instance builders — grouped by rendering phase.
//!
//! Each function builds one phase of the rendering pipeline and returns a struct
//! holding the instance vectors. This makes the pipeline flow in `render_game()`
//! scannable: build world → build debug → build UI → build sidebar → build fog.
//!
//! ## Dependency rules
//! - Internal to `presentation::render` — only called from mod.rs.

use crate::app::AppState;
use crate::app::diagnostics::debug_overlays;
use crate::app::input::commands::preferred_local_owner;
use crate::app::presentation::instances;
use crate::app::presentation::sidebar_render::{
    active_minimap_screen_rect, build_sidebar_cameo_instances, build_sidebar_chrome_instances,
    build_sidebar_instances as sidebar_inst_fn, build_sidebar_text_instances, current_sidebar_view,
};
use crate::app::presentation::ui_overlays::{
    build_bomb_clock_instances, build_building_radius_ring_instances,
    build_building_status_instances, build_cargo_pip_instances, build_occupant_pip_instances,
    build_software_cursor_instances, build_unit_status_bg_instances,
    build_unit_status_fill_instances,
};
use crate::map::terrain::TilePlacement;
use crate::map::theater::TileKey;
use crate::render::batch::SpriteInstance;
use crate::sidebar::SidebarView;

// ---------------------------------------------------------------------------
// Phase structs — group related instance vectors for clean data flow
// ---------------------------------------------------------------------------

/// Game-world sprite instances: terrain tiles, overlays, entities, bridges.
pub(super) struct WorldInstances {
    pub terrain: crate::render::terrain_instances::TerrainInstances,
    pub overlay: Vec<SpriteInstance>,
    pub overlay_render_z: Vec<crate::render::tactical_draw_plan::RenderZPolicy>,
    /// TerrainClass and Techno parents in exact signed Layer-2 order.
    pub ground: super::draw_plan_lowering::GroundObjectPass,
    /// Static smudge decals (craters, scorches) — drawn between terrain and entities.
    pub smudge: Vec<SpriteInstance>,
    pub bridge_body: Vec<SpriteInstance>,
    pub bridge_body_shadow: Vec<SpriteInstance>,
    pub bridge_railing: Vec<SpriteInstance>,
    pub unit: Vec<SpriteInstance>,
    pub unit_pages: Vec<usize>,
    pub unit_transition_paged: Vec<Vec<SpriteInstance>>,
    pub shp_paged: Vec<Vec<SpriteInstance>>,
    /// Bodies above the Ground band: voxel aircraft off their pads, missiles in
    /// flight. Drawn after every ground object — see `top_unit` in draw_passes.
    pub top_unit: Vec<SpriteInstance>,
    pub top_unit_pages: Vec<usize>,
    /// The SHP half of the same band — in stock YR, Rocketeers at hover height.
    /// Kept flat so atlas page changes cannot reorder Top-layer submissions.
    pub top_shp: Vec<SpriteInstance>,
    pub top_shp_pages: Vec<usize>,
    /// Per-particle SpriteInstances (Layer 3). Drawn at Step 7.5 — above
    /// all ground objects + cliffs, below debug/shroud/UI.
    pub particle_paged: Vec<Vec<SpriteInstance>>,
    /// PixelFX water/ore sparkles — 1-pixel cell dots emitted per frame.
    /// Empty when the live `[Options] DetailLevel` projection is zero.
    pub cell_sparkles: Vec<SpriteInstance>,
    /// Persistent WaveClass bucket-3 registrations lowered to white-pixel instances.
    pub weapon_waves: Vec<SpriteInstance>,
    /// SpotlightClass type-16 masks with authoritative child-light coordinates.
    pub spotlight_type16: Vec<SpriteInstance>,
}

/// Debug visualization overlays (toggled by hotkeys at runtime).
pub(super) struct DebugInstances {
    pub pathgrid: Vec<SpriteInstance>,
    pub cell_grid: Vec<SpriteInstance>,
    pub path: Vec<SpriteInstance>,
    pub heightmap: Vec<SpriteInstance>,
}

/// In-game UI overlays: selection brackets, health bars, placement preview, cursor.
pub(super) struct UiInstances {
    pub bracket_back: Vec<SpriteInstance>,
    pub bracket_front_first: Vec<SpriteInstance>,
    pub bracket_front: Vec<SpriteInstance>,
    pub radius_ring: Vec<SpriteInstance>,
    pub building_status: Vec<SpriteInstance>,
    pub bomb_clock: Vec<SpriteInstance>,
    pub occupant_pip: Vec<SpriteInstance>,
    pub unit_status_bg: Vec<SpriteInstance>,
    pub unit_status_fill: Vec<SpriteInstance>,
    pub cargo_pip: Vec<SpriteInstance>,
    pub software_cursor: Vec<SpriteInstance>,
    pub drag: Vec<SpriteInstance>,
    pub placement_valid: Vec<SpriteInstance>,
    pub placement_invalid: Vec<SpriteInstance>,
    pub placement_ghost: Vec<SpriteInstance>,
    pub ghost_page: u8,
    pub wall_ghost: Vec<SpriteInstance>,
    pub target_line: Vec<SpriteInstance>,
    pub factory_rally_first: Vec<SpriteInstance>,
    pub factory_rally_second: Vec<SpriteInstance>,
}

/// Sidebar chrome, cameos, text, minimap, and radar animation.
pub(super) struct SidebarInstances {
    pub sidebar: Vec<SpriteInstance>,
    pub chrome: Vec<SpriteInstance>,
    pub cameo: Vec<SpriteInstance>,
    pub gclock: Vec<SpriteInstance>,
    pub cameo_overlay: Vec<SpriteInstance>,
    pub text: Vec<SpriteInstance>,
    pub minimap: Vec<SpriteInstance>,
    pub viewport_rect: Vec<SpriteInstance>,
    pub content_boundary: Vec<SpriteInstance>,
    pub radar_anim: Vec<SpriteInstance>,
    pub view: Option<SidebarView>,
}

impl SidebarInstances {
    /// Observe the exact vectors that the upload and draw phases consume.
    pub(super) fn emitted_instance_counts(&self) -> super::GameRenderInstanceCounts {
        super::GameRenderInstanceCounts::from_lengths(
            self.minimap.len(),
            self.viewport_rect.len(),
            self.radar_anim.len(),
        )
    }
}

// ---------------------------------------------------------------------------
// Phase 1: Game world (terrain, overlays, entities)
// ---------------------------------------------------------------------------

fn lookup_exact_terrain_variant<T>(
    tile_id: u16,
    sub_tile: u8,
    variant: u8,
    lookup: impl FnOnce(TileKey) -> Option<T>,
) -> Option<T> {
    lookup(TileKey {
        tile_id,
        sub_tile,
        variant,
    })
}

/// Build all game-world sprite instances: terrain tiles, map overlays, bridges,
/// VXL units, SHP buildings/infantry, AnimClass objects, damage fires.
/// Ground parents retain Display order; residual effect buckets still sort depth.
pub(super) fn build_world_instances(state: &mut AppState, sw: f32, sh: f32) -> WorldInstances {
    // Terrain tiles use the selected TMP owner exactly. A sparse/null cell in
    // a positive suffix remains absent instead of borrowing pristine UVs.
    let uv_fn_closure;
    let uv_fn: Option<&dyn Fn(u16, u8, u8) -> Option<TilePlacement>> = if let Some(atlas) =
        &state.match_state.match_presentation.tile_atlas
    {
        uv_fn_closure = |tile_id: u16, sub_tile: u8, variant: u8| -> Option<TilePlacement> {
            let uv =
                lookup_exact_terrain_variant(tile_id, sub_tile, variant, |key| atlas.get_uv(key))?;
            Some(TilePlacement {
                uv_origin: uv.uv_origin,
                uv_size: uv.uv_size,
                pixel_size: uv.pixel_size,
                draw_offset: uv.draw_offset,
            })
        };
        Some(&uv_fn_closure)
    } else {
        None
    };
    let terrain = if let Some(grid) = &state.match_state.match_presentation.terrain_grid {
        // Terrain draws regardless of shroud — the native tile walk has no
        // explored gate (`CellOverlay_TileDraw @ 0x00480350`); the flat shroud
        // curtain blacks out unexplored ground in the multiply pass.
        let bridge_state = state
            .match_state
            .sim_runtime
            .as_ref()
            .and_then(|rt| rt.view().bridge_state());
        crate::render::terrain_instances::build_visible_instances(
            grid,
            Some(state.match_state.match_presentation.lighting.grid()),
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
            sw,
            sh,
            uv_fn,
            bridge_state,
            state
                .match_state
                .sim_runtime
                .as_ref()
                .and_then(|rt| rt.view().resolved_terrain()),
        )
    } else {
        crate::render::terrain_instances::TerrainInstances { normal: Vec::new() }
    };

    // Map overlays and walls remain in the fixed per-cell draw plan. Terrain
    // objects join live Ground registrations below; low bridges (LOBRDG*) ride
    // in `overlay`, while high bridge bodies use instances::bridges.
    let ground_order = super::draw_plan_lowering::NativeGroundOrder::new(
        state.match_state.sim_runtime.as_ref().map_or(&[], |rt| {
            rt.view()
                .display_layers()
                .members(crate::sim::world::display_layers::DisplayLayer::GROUND)
        }),
    );
    let mut ground_objects = Vec::new();
    let mut overlay: Vec<SpriteInstance> = std::mem::take(
        &mut state
            .match_state
            .match_presentation
            .cached_overlay_instances,
    );
    overlay.clear();
    let mut overlay_render_z = Vec::new();
    instances::build_overlay_instances(
        state,
        sw,
        sh,
        &mut overlay,
        &mut overlay_render_z,
        &mut ground_objects,
        &ground_order,
    );
    // Bridge body, shadow, and railing emission live in instances::bridges
    // (Phase D). Read from BridgeRuntimeCell post-tick (NOT OverlayGrid).
    let mut bridge_body: Vec<SpriteInstance> = Vec::new();
    let mut bridge_body_shadow: Vec<SpriteInstance> = Vec::new();
    let mut bridge_railing: Vec<SpriteInstance> = Vec::new();
    instances::bridges::build_bridge_body_instances(state, sw, sh, &mut bridge_body);
    instances::bridges::build_bridge_shadow_instances(state, sw, sh, &mut bridge_body_shadow);
    instances::bridges::build_bridge_railing_instances(state, sw, sh, &mut bridge_railing);
    sort_by_depth_desc(&mut bridge_body);
    sort_by_depth_desc(&mut bridge_body_shadow);
    sort_by_depth_desc(&mut bridge_railing);

    // Smudges: static crater/scorch decals on top of terrain, under entities.
    // Atlas registration for SmudgeType SHPs is a deferred follow-up; until it
    // lands the lookup closure returns None and `smudge` stays empty. The
    // pipeline plumbing (build → upload → draw) is wired regardless.
    let smudge: Vec<SpriteInstance> = build_smudge_instances(state, sw, sh);

    // SHP sprites: buildings, infantry, effects — paged across sprite atlas pages.
    let shp_page_count: usize = state
        .match_state
        .match_presentation
        .sprite_atlas
        .as_ref()
        .map_or(1, |a| a.page_count().max(1));
    let mut shp_paged: Vec<Vec<SpriteInstance>> = vec![Vec::new(); shp_page_count];
    let mut top_shp: Vec<SpriteInstance> = Vec::new();
    let mut top_shp_pages: Vec<usize> = Vec::new();
    let mut top_shp_ids: Vec<u64> = Vec::new();
    let mut particle_paged: Vec<Vec<SpriteInstance>> = vec![Vec::new(); shp_page_count];

    // VXL body sources — sorted by depth descending.
    // shp_paged is passed in so harvest overlays (OREGATH SHP) route to the
    // correct sprite atlas page instead of the voxel unit instance list.
    let mut unit: Vec<SpriteInstance> =
        std::mem::take(&mut state.match_state.match_presentation.cached_unit_instances);
    unit.clear();
    let mut unit_pages: Vec<usize> =
        std::mem::take(&mut state.match_state.match_presentation.cached_unit_pages);
    unit_pages.clear();
    // Residual: Air/Top VXL, SHP and effect buckets still need one interleaved
    // Display traversal. Body buckets now consume retained membership;
    // remaining Fly landing/resubmission writers are still required in sim.
    let mut top_unit: Vec<SpriteInstance> = Vec::new();
    let mut top_unit_pages: Vec<usize> = Vec::new();
    let transition_page_count = state
        .renderer
        .vxl_slope_transition_cache
        .borrow()
        .page_count()
        .max(1);
    let mut unit_transition_paged: Vec<Vec<SpriteInstance>> =
        vec![Vec::new(); transition_page_count];
    instances::build_unit_instances(
        state,
        &mut unit,
        &mut unit_pages,
        &mut top_unit,
        &mut top_unit_pages,
        &mut unit_transition_paged,
        &mut shp_paged,
        &mut ground_objects,
        &ground_order,
    );
    for page in &mut unit_transition_paged {
        sort_by_depth_desc(page);
    }
    // A building's voxel turret remains owned by the building display call.
    // The SHP builder therefore adds it to the same contiguous Ground parent
    // instead of leaking it into an atlas-level tie.
    // Parachute canopies are composed into their body's draw, so they take the
    // body's key rather than deriving one. Collected here, consumed by
    // `build_parachute_instances` below — which is why it must run after this.
    let mut parachute_body_depths = instances::ParachuteBodyDepths::new();
    instances::build_shp_instances(
        state,
        &mut shp_paged,
        &mut top_shp,
        &mut top_shp_pages,
        &mut top_shp_ids,
        &mut parachute_body_depths,
        &mut ground_objects,
        &ground_order,
    );
    sort_by_depth_desc_with_pages(&mut unit, &mut unit_pages);
    // AnimClass objects use retained Display membership: Ground joins the
    // parent plan, Top appends to the flat page-tagged stream.
    instances::build_anim_class_instances(
        state,
        &mut shp_paged,
        &mut top_shp,
        &mut top_shp_pages,
        &mut top_shp_ids,
        &mut ground_objects,
        &ground_order,
    );
    order_top_shp_by_display(
        &mut top_shp,
        &mut top_shp_pages,
        &mut top_shp_ids,
        state.match_state.sim_runtime.as_ref().map_or(&[], |rt| {
            rt.view()
                .display_layers()
                .members(crate::sim::world::display_layers::DisplayLayer::TOP)
        }),
    );
    // In-flight projectile sprites (e.g. Guardian GI DRAGON missile).
    instances::build_projectile_visual_instances(state, &mut shp_paged);
    // Parachute SHPs above descending paradropped infantry (Layer 2 — sorts
    // with the GI body, at the body's own key).
    instances::build_parachute_instances(state, &mut ground_objects, &parachute_body_depths);
    for page in &mut shp_paged {
        sort_by_depth_desc(page);
    }

    // Layer 3 particle systems — separate paged list above all Ground-layer
    // geometry per the original's ParticleClass::GetLayer = 3.
    instances::build_particle_instances(state, &mut particle_paged);
    for page in &mut particle_paged {
        sort_by_depth_desc(page);
    }

    let ground = super::draw_plan_lowering::lower_ground_object_instances(ground_objects);

    // PixelFX water/ore sparkles — per-frame 1-pixel cell dots.
    let cell_sparkles: Vec<SpriteInstance> = build_pixel_fx_sparkle_instances(state, sw, sh);

    // One-time first-frame statistics.
    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        let total_grid: usize = state
            .match_state
            .match_presentation
            .terrain_grid
            .as_ref()
            .map_or(0, |g| g.cells.len());
        log::info!(
            "First frame: {} terrain tiles (of {} cells) + {} fixed overlays + {} Ground sprites + {} residual SHP",
            terrain.normal.len(),
            total_grid,
            overlay.len() + bridge_body.len(),
            ground.instances.len(),
            shp_paged.iter().map(|p| p.len()).sum::<usize>(),
        );
    }

    let weapon_waves = instances::build_weapon_wave_instances(state);

    // Named residual: BuildingLightRuntime currently records the parent/target
    // relation but not SpotlightClass's evolving child coordinate and angle.
    // Drawing at the parent would invent visible semantics, so the exact live
    // destination-factor path stays empty until that authoritative input exists.
    let spotlight_type16 = Vec::new();

    WorldInstances {
        terrain,
        overlay,
        overlay_render_z,
        ground,
        smudge,
        bridge_body,
        bridge_body_shadow,
        bridge_railing,
        unit,
        unit_pages,
        unit_transition_paged,
        shp_paged,
        top_unit,
        top_unit_pages,
        top_shp,
        top_shp_pages,
        particle_paged,
        cell_sparkles,
        weapon_waves,
        spotlight_type16,
    }
}

/// Build PixelFX sparkle instances by calling into the dedicated render module.
/// Assembles the SparkleInput from AppState and returns the Vec. The module
/// itself gates on the extra_animations toggle; the wrapper short-circuits
/// when required sim/render state is missing (no map loaded).
fn pixel_fx_enabled_for_detail_level(detail_level: u32) -> bool {
    // Retail provenance: `DrawPixelFXSparkles @ 0x006D7840` tests
    // `g_ExtraAnimationsEnabled @ 0x00A8EB78` for nonzero. The write/read
    // owners bind that global to `OptionsClass::DetailLevel +0x18` in
    // `OptionsClass__ApplyFromInGameDialog @ 0x004E1DE0` and
    // `OptionsClass__ReadFromINI @ 0x005FA620`.
    detail_level != 0
}

fn build_pixel_fx_sparkle_instances(state: &AppState, sw: f32, sh: f32) -> Vec<SpriteInstance> {
    use crate::render::pixel_fx_sparkles::{SparkleInput, build_sparkle_instances};

    // F10 cone entry: read-only assembly consumes the runtime through the
    // view; fields without getters use the escape hatch until their cone.
    let Some(view) = state.sim_view() else {
        return Vec::new();
    };
    let sim = view.simulation();
    let Some(resolved) = state.terrain_template() else {
        return Vec::new();
    };
    let Some(overlay_registry) = state.overlay_registry() else {
        return Vec::new();
    };
    let Some(overlays) = sim.overlay_grid.as_ref() else {
        return Vec::new();
    };

    // gamemd-derived: the `DAT_00A8EB78 != 0` PixelFX gate is the process
    // `[Options] DetailLevel` field (`OptionsClass + 0x18`). The in-game state
    // is the live projection of that retained profile value, so config.toml is
    // not a second interactive authority for the same retail option.
    let enable_extra_animations = pixel_fx_enabled_for_detail_level(
        state
            .match_state
            .match_presentation
            .in_game_options
            .detail_level,
    );

    let local_owner_name = crate::app::input::commands::preferred_local_owner_name(state);
    let local_owner_id = match (state.match_state.sandbox_full_visibility, &local_owner_name) {
        (false, Some(owner)) => sim.interner.get(owner),
        _ => None,
    };

    let input = SparkleInput {
        clock_ms: sim.session.total_sim_ms,
        enable_extra_animations,
        local_owner_id,
        sandbox_full_visibility: state.match_state.sandbox_full_visibility,
        resolved_terrain: resolved,
        overlays,
        overlay_registry,
        occupancy: sim.occupancy(),
        fog: &sim.fog,
        camera_x: state.match_state.input.camera_x,
        camera_y: state.match_state.input.camera_y,
        viewport_w: sw,
        viewport_h: sh,
        map_w: resolved.width(),
        map_h: resolved.height(),
        white_uv_origin: [0.0, 0.0],
        white_uv_size: [1.0, 1.0],
    };
    build_sparkle_instances(&input)
}

/// Build static smudge decal instances for the current frame.
///
/// Pulls the SmudgeGrid + SmudgeTypeRegistry from the active simulation and
/// hands them to `render::smudge::build_visible_instances` along with a
/// closure that resolves SmudgeType id + frame index to atlas UVs.
///
/// The atlas stores each smudge SHP as one composite frame. The render helper
/// uses the resolved terrain level at the footprint origin and emits that
/// frame once even though native recenters an identical draw from every
/// occupied footprint cell.
fn build_smudge_instances(state: &AppState, sw: f32, sh: f32) -> Vec<SpriteInstance> {
    let (sim, rules) = match (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
    ) {
        (Some(s), Some(r)) => (s, r),
        _ => return Vec::new(),
    };
    let Some(grid) = sim.smudge_grid.as_ref() else {
        return Vec::new();
    };
    let Some(resolved_terrain) = sim.resolved_terrain.as_ref() else {
        return Vec::new();
    };
    let Some(atlas) = state.match_state.match_presentation.overlay_atlas.as_ref() else {
        return Vec::new();
    };
    // Resolve (smudge_type_id, frame_offset) → atlas placement.
    // Smudge SHPs are registered into the OverlayAtlas under
    // `crate::render::overlay_atlas::SMUDGE_KEY_PREFIX` at map-load time
    // (see render/overlay_atlas.rs render_smudge_sprite). Frame is always 0
    // because gamemd draws every footprint cell with frame 0 and shifts
    // screen position back to footprint origin — render-side handles the
    // shift cancellation by skipping non-origin cells inside
    // build_visible_instances.
    let lookup = |type_id: u16, _frame: u8| -> Option<TilePlacement> {
        let def = rules.smudge_types.get(type_id)?;
        let entry = atlas.get(&crate::render::overlay_atlas::smudge_key(&def.name))?;
        Some(TilePlacement {
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            pixel_size: entry.pixel_size,
            draw_offset: [entry.offset_x, entry.offset_y],
        })
    };
    crate::render::smudge::build_visible_instances(
        grid,
        &rules.smudge_types,
        resolved_terrain,
        &lookup,
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
        sw,
        sh,
    )
}

// ---------------------------------------------------------------------------
// Phase 2: Debug overlays
// ---------------------------------------------------------------------------

/// Build debug visualization instances (only when toggled on via hotkeys).
pub(super) fn build_debug_instances(state: &AppState, sw: f32, sh: f32) -> DebugInstances {
    DebugInstances {
        pathgrid: if state.diag.debug_show_pathgrid {
            debug_overlays::build_terrain_cost_overlay_instances(state, sw, sh)
        } else {
            Vec::new()
        },
        cell_grid: if state.diag.debug_show_cell_grid {
            debug_overlays::build_cell_grid_overlay_instances(state, sw, sh)
        } else {
            Vec::new()
        },
        path: if state.diag.debug_show_pathgrid {
            debug_overlays::build_path_overlay_instances(state, sw, sh)
        } else {
            Vec::new()
        },
        heightmap: if state.diag.debug_show_heightmap {
            debug_overlays::build_heightmap_overlay_instances(state, sw, sh)
        } else {
            Vec::new()
        },
    }
}

// ---------------------------------------------------------------------------
// Phase 3: Minimap + UI overlays
// ---------------------------------------------------------------------------

/// Update minimap unit dots for the current frame.
pub(super) fn update_minimap(state: &mut AppState, local_owner: &Option<String>) {
    let (playfield_bounds, playfield_revision) =
        crate::app::input::camera::sync_playfield_presentation_bounds(state);
    let needs_playfield_reconcile = state
        .match_state
        .match_presentation
        .minimap
        .as_ref()
        .is_some_and(|minimap| {
            minimap.needs_playfield_reconcile(playfield_bounds, playfield_revision)
        });
    if needs_playfield_reconcile {
        let overlay_data = crate::app::loading::transitions::build_minimap_overlay_data(
            state.match_state.match_presentation.overlays.as_slice(),
            &state.match_state.match_presentation.terrain_objects,
            state.overlay_registry(),
            state.rules(),
        );
        let runtime = state.match_state.sim_runtime.as_ref();
        let presentation = &mut state.match_state.match_presentation;
        if let (Some(minimap), Some(grid)) = (
            presentation.minimap.as_mut(),
            presentation.terrain_grid.as_ref(),
        ) {
            minimap.reconcile_playfield(
                &state.renderer.gpu,
                grid,
                runtime,
                &overlay_data,
                &presentation.overlay_radar_colors,
                &presentation.theater_name,
                playfield_bounds,
                playfield_revision,
            );
        }
    }

    let full_visibility = state.match_state.sandbox_full_visibility;
    let presentation = &mut state.match_state.match_presentation;
    if let (Some(minimap), Some(runtime)) = (
        presentation.minimap.as_mut(),
        state.match_state.sim_runtime.as_mut(),
    ) {
        let transaction = super::minimap_transaction::present_minimap_frame(runtime, |runtime| {
            // F10 cone: render-feed reads go through SimView getters. The
            // production transaction includes composition, queue upload, and
            // only then the simulation acknowledgement.
            let view = runtime.view();
            let (radar_dirty_cells, radar_dirty_generation) = view.radar_terrain_dirty();
            Ok::<_, std::convert::Infallible>(
                minimap.update_unit_dots(
                    &state.renderer.gpu,
                    view.entities(),
                    view.logic_order(),
                    view.houses(),
                    &presentation.house_color_map,
                    view.session().tick,
                    local_owner
                        .as_deref()
                        .and_then(|owner| view.interner().get(owner)),
                    view.fog(),
                    full_visibility,
                    view.session().game_mode_nonzero,
                    Some(&runtime.resources.rules),
                    Some(view.interner()),
                    view.bridge_state(),
                    view.overlay_grid(),
                    Some(&runtime.resources.overlay_registry),
                    &presentation.overlay_radar_colors,
                    view.resolved_terrain(),
                    radar_dirty_cells,
                    radar_dirty_generation,
                ),
            )
        });
        match transaction {
            Ok(_) => {}
            Err(never) => match never {},
        }
    }
}

/// Build in-game UI overlay instances: selection brackets, health bars,
/// drag rectangle, building placement preview, and software cursor.
pub(super) fn build_ui_instances(state: &AppState, sw: f32, sh: f32) -> UiInstances {
    let bracket = crate::app::presentation::selection_brackets::build_selection_bracket_instances(
        state, sw, sh,
    );
    let radius_ring: Vec<SpriteInstance> = build_building_radius_ring_instances(state, sw, sh);
    let building_status: Vec<SpriteInstance> = build_building_status_instances(state, sw, sh);
    let bomb_clock = build_bomb_clock_instances(state, sw, sh);
    let occupant_pip = build_occupant_pip_instances(state, sw, sh);
    let unit_status_bg = build_unit_status_bg_instances(state, sw, sh);
    let unit_status_fill = build_unit_status_fill_instances(state, sw, sh);
    let cargo_pip = build_cargo_pip_instances(state, sw, sh);
    let software_cursor = build_software_cursor_instances(state);
    let drag = match &state.match_state.match_presentation.selection_overlay {
        Some(o) => o.build_drag_rect(
            &state.match_state.input.selection_state,
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ),
        None => Vec::new(),
    };

    // Building placement preview: cell grid + ghost sprite (or wall ghost for wall types).
    let (placement_valid, placement_invalid, placement_ghost, ghost_page, wall_ghost) =
        build_placement_preview(state);

    // Target/action lines from selected units to command destinations.
    let target_line = crate::app::presentation::target_lines::build_target_line_instances(
        &state.match_state.match_presentation.target_lines,
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        &state.height_map(),
    );
    let factory_rally = crate::app::presentation::target_lines::build_factory_rally_line_instances(
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
        &state.height_map(),
        &state.match_state.match_presentation.house_color_map,
        preferred_local_owner(state).as_deref(),
    );

    UiInstances {
        bracket_back: bracket.back,
        bracket_front_first: bracket.front_first,
        bracket_front: bracket.front,
        radius_ring,
        building_status,
        bomb_clock,
        occupant_pip,
        unit_status_bg,
        unit_status_fill,
        cargo_pip,
        software_cursor,
        drag,
        placement_valid,
        placement_invalid,
        placement_ghost,
        ghost_page,
        wall_ghost,
        target_line,
        factory_rally_first: factory_rally.clone(),
        factory_rally_second: factory_rally,
    }
}

/// Build the building placement preview: valid/invalid cell markers, ghost sprite,
/// and wall connectivity ghost for wall-type buildings.
fn build_placement_preview(
    state: &AppState,
) -> (
    Vec<SpriteInstance>,
    Vec<SpriteInstance>,
    Vec<SpriteInstance>,
    u8,
    Vec<SpriteInstance>,
) {
    match (
        &state.match_state.match_presentation.selection_overlay,
        &state.match_state.input.building_placement_preview,
    ) {
        (Some(o), Some(preview)) => {
            let preview_type_str = state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|rt| &rt.simulation)
                .map(|s| s.interner.resolve(preview.type_id).to_string())
                .unwrap_or_default();
            let is_wall: bool = state
                .rules()
                .and_then(|r| r.object(&preview_type_str))
                .map(|obj| obj.wall)
                .unwrap_or(false);

            if is_wall {
                // Walls show the cursor cell + auto-fill cells toward existing walls.
                // Draws place.shp on every intermediate cell between cursor and
                // nearest same-type wall.
                let (mut valid, mut invalid) =
                    o.build_building_preview(preview, &state.height_map());
                if !preview.wall_autofill_cells.is_empty() {
                    let (av, ai) = o.build_wall_autofill_diamonds(
                        &preview.wall_autofill_cells,
                        preview.valid,
                        &state.height_map(),
                    );
                    valid.extend(av);
                    invalid.extend(ai);
                }
                (valid, invalid, Vec::new(), 0, Vec::new())
            } else {
                let (valid, invalid) = o.build_building_preview(preview, &state.height_map());
                let hc: crate::rules::house_colors::HouseColorIndex = state
                    .match_state
                    .match_presentation
                    .house_color_map
                    .get(
                        &crate::app::input::commands::preferred_local_owner(state)
                            .unwrap_or_else(|| "Americans".to_string()),
                    )
                    .copied()
                    // Missing local owner → the producers' default scheme entry, not entry 0.
                    .unwrap_or(crate::rules::house_colors::HouseColorIndex(
                        crate::rules::house_colors::DEFAULT_SCHEME_ENTRY as u8,
                    ));
                let ghost_result =
                    crate::render::selection_overlay::SelectionOverlay::build_ghost_sprite(
                        preview,
                        state.match_state.match_presentation.sprite_atlas.as_ref(),
                        hc,
                        &state.height_map(),
                        state
                            .match_state
                            .sim_runtime
                            .as_ref()
                            .map(|rt| &rt.simulation)
                            .map(|s| &s.interner),
                    );
                let (ghost, page) = match ghost_result {
                    Some((inst, p)) => (vec![inst], p),
                    None => (Vec::new(), 0),
                };
                (valid, invalid, ghost, page, Vec::new())
            }
        }
        _ => (Vec::new(), Vec::new(), Vec::new(), 0, Vec::new()),
    }
}

// ---------------------------------------------------------------------------
// Phase 4: Sidebar
// ---------------------------------------------------------------------------

/// Build sidebar UI instances: chrome, cameos, text, minimap, viewport rect, radar animation.
pub(super) fn build_sidebar_instances(state: &mut AppState) -> SidebarInstances {
    let view = current_sidebar_view(state).cloned();
    let minimap_rect = active_minimap_screen_rect(state);
    let (tactical_w, tactical_h) = crate::app::input::camera::tactical_viewport_size_px(
        state.render_width(),
        state.render_height(),
    );
    let tactical_center_cell = crate::app::input::camera::tactical_centre_cell(state);
    let sidebar_color = crate::render::sidebar_text::native_radar_outline_color(
        crate::app::presentation::sidebar_render::current_sidebar_theme(state),
    );
    // Native g_SidebarSurface is 168 x screen_height in surface-local space;
    // SidebarClass::BlitToScreen @ 0x006A70E0 translates that retained surface
    // to the right-sidebar destination without changing the copied extent.
    // The live view panel is the corresponding outer UI-scaled screen frame.
    let sidebar_surface = view.as_ref().map(|view| {
        [
            view.panel_rect.x,
            view.panel_rect.y,
            view.panel_rect.w,
            view.panel_rect.h,
        ]
    });

    // Only show minimap when radar is online (or no radar_anim = legacy fallback).
    let minimap_visible: bool = state
        .match_state
        .match_presentation
        .radar_anim
        .as_ref()
        .map_or(true, |ra| ra.is_minimap_visible());

    let (minimap, viewport_rect, content_boundary) = if minimap_visible {
        match &mut state.match_state.match_presentation.minimap {
            Some(mm) => {
                let minimap = vec![mm.build_minimap_instance_in_rect(
                    state.match_state.input.camera_x,
                    state.match_state.input.camera_y,
                    minimap_rect.x,
                    minimap_rect.y,
                    minimap_rect.w,
                    minimap_rect.h,
                )];
                let viewport_rect = sidebar_surface.map_or_else(Vec::new, |sidebar_surface| {
                    mm.build_viewport_rect_in_rect(
                        state.match_state.input.camera_x,
                        state.match_state.input.camera_y,
                        tactical_center_cell,
                        tactical_w as i32,
                        tactical_h as i32,
                        minimap_rect.x,
                        minimap_rect.y,
                        minimap_rect.w,
                        minimap_rect.h,
                        sidebar_surface,
                        sidebar_color,
                    )
                });
                let content_boundary = sidebar_surface.map_or_else(Vec::new, |sidebar_surface| {
                    mm.build_content_boundary_in_rect(
                        state.match_state.input.camera_x,
                        state.match_state.input.camera_y,
                        minimap_rect.x,
                        minimap_rect.y,
                        minimap_rect.w,
                        minimap_rect.h,
                        sidebar_surface,
                        sidebar_color,
                    )
                });
                (minimap, viewport_rect, content_boundary)
            }
            None => (Vec::new(), Vec::new(), Vec::new()),
        }
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };

    let sidebar = view
        .as_ref()
        .map(|v| sidebar_inst_fn(state, v))
        .unwrap_or_default();
    let chrome = view
        .as_ref()
        .map(|v| build_sidebar_chrome_instances(state, v))
        .unwrap_or_default();

    let ready_text = state
        .process_assets
        .csf
        .as_ref()
        .map(|csf| csf.text("TXT_READY"))
        .unwrap_or_else(|| std::borrow::Cow::Borrowed("Ready"));
    // gamemd's strip draw pairs TXT_READY with TXT_HOLD ("On Hold"), shown on
    // the same cameo slot when production is suspended.
    let hold_text = state
        .process_assets
        .csf
        .as_ref()
        .map(|csf| csf.text("TXT_HOLD"))
        .unwrap_or_else(|| std::borrow::Cow::Borrowed("On Hold"));
    let ready_tint = {
        let theme = crate::app::presentation::sidebar_render::current_sidebar_theme(state);
        crate::render::sidebar_text::side_highlight_color(theme)
    };
    let (cameo, gclock, cameo_overlay) = view
        .as_ref()
        .map(|v| build_sidebar_cameo_instances(state, v, ready_text.as_ref(), hold_text.as_ref()))
        .unwrap_or_default();
    let mut text = view
        .as_ref()
        .map(|v| {
            build_sidebar_text_instances(
                state,
                v,
                ready_text.as_ref(),
                hold_text.as_ref(),
                ready_tint,
            )
        })
        .unwrap_or_default();
    // Credits counter shares the GAME.FNT sidebar-text layer: gamemd draws it
    // with the same BitFont on the sidebar surface, in the same packed side
    // text colour as the cameo labels.
    if let Some(v) = view.as_ref() {
        let theme = crate::app::presentation::sidebar_render::current_sidebar_theme(state);
        text.extend(
            crate::app::presentation::sidebar_text::build_sidebar_credits_instances(
                &state.renderer.bit_font,
                v,
                state.match_state.match_presentation.ui_scale,
                crate::app::presentation::sidebar_text::credits_tint(theme),
                [
                    state.match_state.input.camera_x,
                    state.match_state.input.camera_y,
                ],
            ),
        );
    }

    // Preserve the pixels actually drawn online for a possible pre-due loss.
    // This is display history, separate from ongoing internal minimap updates.
    let origin = [
        state.match_state.input.camera_x + state.render_width() as f32 - 168.0,
        state.match_state.input.camera_y + 48.0,
    ];
    let presentation = &mut state.match_state.match_presentation;
    if let (Some(radar), Some(mm)) = (
        presentation.radar_anim.as_mut(),
        presentation.minimap.as_ref(),
    ) {
        let (rgba, size) = mm.composed_rgba();
        radar.retain_online_content(
            rgba,
            size,
            &minimap,
            viewport_rect.iter().chain(&content_boundary).copied(),
            origin,
        );
    }
    let radar_anim = build_radar_anim_instance(state);

    SidebarInstances {
        sidebar,
        chrome,
        cameo,
        gclock,
        cameo_overlay,
        text,
        minimap,
        viewport_rect,
        content_boundary,
        radar_anim,
        view,
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Specialized instance builders
// ---------------------------------------------------------------------------

/// Build a SpriteInstance for the animated radar chrome overlay.
/// Positioned at the same location as the static radar.shp in the sidebar chrome.
fn build_radar_anim_instance(state: &AppState) -> Vec<SpriteInstance> {
    let ra = match &state.match_state.match_presentation.radar_anim {
        Some(ra) => ra,
        None => return Vec::new(),
    };

    let sw: f32 = state.render_width() as f32;
    let sh: f32 = state.render_height() as f32;
    let spec = state.match_state.match_presentation.sidebar_layout_spec;
    let layout = crate::sidebar::compute_layout_with_spec(spec, sw, sh, 0);

    let s = state.match_state.match_presentation.ui_scale;
    vec![SpriteInstance {
        position: [
            state.match_state.input.camera_x + layout.sidebar_x,
            state.match_state.input.camera_y + layout.radar_y,
        ],
        size: [ra.width as f32 * s, ra.height as f32 * s],
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        depth: 0.00048,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    }]
}

// ---------------------------------------------------------------------------
// Sort helpers
// ---------------------------------------------------------------------------

/// Sort instances by depth descending (furthest-back first for back-to-front draw).
fn sort_by_depth_desc(instances: &mut [SpriteInstance]) {
    instances.sort_by(|a, b| {
        b.depth
            .partial_cmp(&a.depth)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// Sort a flat UnitAtlas stream without letting texture-page assignment become
/// a new ordering authority. `sort_by` is stable, preserving insertion order at
/// equal depth (notably body → barrel/turret).
fn sort_by_depth_desc_with_pages(instances: &mut Vec<SpriteInstance>, pages: &mut Vec<usize>) {
    assert_eq!(
        instances.len(),
        pages.len(),
        "every stable UnitAtlas instance must carry one page tag"
    );
    let mut paired: Vec<(SpriteInstance, usize)> =
        instances.drain(..).zip(pages.drain(..)).collect();
    paired.sort_by(|(a, _), (b, _)| {
        b.depth
            .partial_cmp(&a.depth)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    instances.reserve(paired.len());
    pages.reserve(paired.len());
    for (instance, page) in paired {
        instances.push(instance);
        pages.push(page);
    }
}

/// Restore native Top-layer append order after the disjoint SHP builders have
/// emitted into one flat, page-tagged stream. Atlas identity remains aligned
/// payload and never becomes an ordering authority.
fn order_top_shp_by_display(
    instances: &mut Vec<SpriteInstance>,
    pages: &mut Vec<usize>,
    ids: &mut Vec<u64>,
    display_members: &[u64],
) {
    assert_eq!(
        instances.len(),
        pages.len(),
        "every Top SHP instance must carry one page tag"
    );
    assert_eq!(
        instances.len(),
        ids.len(),
        "every Top SHP instance must carry one stable object id"
    );

    let ranks: std::collections::BTreeMap<u64, usize> = display_members
        .iter()
        .enumerate()
        .map(|(rank, &id)| (id, rank))
        .collect();
    let mut emitted: Vec<(usize, SpriteInstance, usize, u64)> = instances
        .drain(..)
        .zip(pages.drain(..))
        .zip(ids.drain(..))
        .enumerate()
        .map(|(emission, ((instance, page), id))| (emission, instance, page, id))
        .collect();
    emitted.sort_by_key(|(emission, _, _, id)| {
        (ranks.get(id).copied().unwrap_or(usize::MAX), *emission)
    });

    instances.reserve(emitted.len());
    pages.reserve(emitted.len());
    ids.reserve(emitted.len());
    for (_, instance, page, id) in emitted {
        instances.push(instance);
        pages.push(page);
        ids.push(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::draw_state::DrawState;

    #[test]
    fn pixel_fx_uses_the_profile_detail_level_nonzero_gate() {
        assert!(!pixel_fx_enabled_for_detail_level(0));
        assert!(pixel_fx_enabled_for_detail_level(1));
        assert!(pixel_fx_enabled_for_detail_level(2));
    }

    #[test]
    fn paired_unit_sort_keeps_page_tags_and_equal_depth_order() {
        let mut instances = vec![
            SpriteInstance {
                depth: 0.5,
                draw_state: DrawState {
                    fx_flags: 10,
                    ..Default::default()
                },
                ..Default::default()
            },
            SpriteInstance {
                depth: 0.8,
                draw_state: DrawState {
                    fx_flags: 20,
                    ..Default::default()
                },
                ..Default::default()
            },
            SpriteInstance {
                depth: 0.8,
                draw_state: DrawState {
                    fx_flags: 30,
                    ..Default::default()
                },
                ..Default::default()
            },
        ];
        let mut pages = vec![1usize, 0, 2];

        sort_by_depth_desc_with_pages(&mut instances, &mut pages);

        assert_eq!(
            instances
                .iter()
                .map(|instance| instance.draw_state.fx_flags)
                .collect::<Vec<_>>(),
            vec![20, 30, 10]
        );
        assert_eq!(pages, vec![0, 2, 1]);
    }

    #[test]
    fn gsi_13_04_top_shp_stream_uses_registration_not_atlas_page_order() {
        let mut instances = vec![
            marker_instance(0.0, 20),
            marker_instance(0.0, 10),
            marker_instance(0.0, 30),
        ];
        let mut pages = vec![2usize, 0, 1];
        let mut ids = vec![20u64, 10, 30];

        order_top_shp_by_display(&mut instances, &mut pages, &mut ids, &[10, 20, 30]);

        assert_eq!(ids, vec![10, 20, 30]);
        assert_eq!(pages, vec![0, 2, 1]);
        assert_eq!(
            instances
                .iter()
                .map(|instance| instance.draw_state.fx_flags)
                .collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
    }

    /// Building turrets are appended to the voxel stream after every vehicle
    /// body and then sorted with them, so they now interleave by iso row
    /// instead of being flushed in a pass of their own. A vehicle at a nearer
    /// row must end up after the turret; one at the same row must stay before
    /// it, which is what leaves the turret sitting on its own building.
    fn marker_instance(depth: f32, marker: u32) -> SpriteInstance {
        SpriteInstance {
            depth,
            draw_state: crate::render::draw_state::DrawState {
                fx_flags: marker,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn building_turrets_interleave_with_vehicles_by_depth_once_appended() {
        const BEHIND: f32 = 0.8;
        const SAME_ROW: f32 = 0.5;
        const IN_FRONT: f32 = 0.2;
        // fx_flags is only a marker here: 1 = vehicle body, 2 = building turret.
        let mut instances = vec![
            marker_instance(IN_FRONT, 1),
            marker_instance(SAME_ROW, 1),
            // Turrets are emitted after every vehicle body.
            marker_instance(BEHIND, 2),
            marker_instance(SAME_ROW, 2),
        ];
        let mut pages = vec![0usize, 1, 2, 3];

        sort_by_depth_desc_with_pages(&mut instances, &mut pages);

        assert_eq!(
            instances
                .iter()
                .map(|i| (i.draw_state.fx_flags, i.depth))
                .collect::<Vec<_>>(),
            vec![(2, BEHIND), (1, SAME_ROW), (2, SAME_ROW), (1, IN_FRONT),],
            "a turret behind draws first, a turret on the same row draws after \
             the vehicle already there, and a vehicle in front draws over both"
        );
    }

    #[test]
    fn positive_terrain_variant_never_falls_back_to_pristine_owner() {
        let pristine = TileKey {
            tile_id: 9,
            sub_tile: 3,
            variant: 0,
        };
        let selected = TileKey {
            variant: 2,
            ..pristine
        };
        let mut looked_up = Vec::new();

        let result = lookup_exact_terrain_variant(9, 3, 2, |key| {
            looked_up.push(key);
            (key == pristine).then_some(())
        });

        assert_eq!(result, None);
        assert_eq!(looked_up, vec![selected]);
    }
}
