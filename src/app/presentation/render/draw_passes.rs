//! Draw pass dispatch — creates the wgpu render pass and issues all draw calls in order.
//!
//! Separated from the instance-building phase in mod.rs so the orchestrator stays focused
//! on *what* to render while this module handles *how* to submit it to the GPU.
//!
//! ## Dependency rules
//! - Internal to `presentation::render` — only called from mod.rs via `dispatch_draw_passes()`.

use crate::app::AppState;
use crate::app::presentation::sidebar_render::{
    begin_main_load_pass, begin_main_pass, current_sidebar_chrome_texture,
    current_sidebar_gclock_texture,
};
use crate::app::presentation::ui_overlays::current_software_cursor_texture;
use crate::render::batch::{BatchRenderer, BatchTexture, InstanceBufferPool, SpriteInstance};
use crate::render::bridge_atlas::BridgeAtlas;
use crate::render::overlay_atlas::OverlayAtlas;
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::render::tile_atlas::TileAtlas;

use super::merge_passes;

/// Data from the instance-building phase that the draw pass needs beyond `AppState`.
///
/// These are local variables in `render_game()` that can't be accessed through `state`
/// because they're computed fresh each frame and (for the merge passes) need CPU-side
/// depth values that match the uploaded GPU buffers.
pub(super) struct DrawPassData<'a> {
    pub overlay_render_z: &'a [RenderZPolicy],
    pub ground: &'a super::draw_plan_lowering::GroundObjectPass,
    pub unit_instances: &'a [SpriteInstance],
    pub unit_pages: &'a [usize],
    pub unit_transition_paged: &'a [Vec<SpriteInstance>],
    pub shp_paged: &'a [Vec<SpriteInstance>],
    pub top_unit_pages: &'a [usize],
    pub top_shp_pages: &'a [usize],
    pub ghost_page: u8,
}

/// Create the main render pass and dispatch all draw calls in the correct order.
///
/// The frame has two regions, matching the native composition: the **tactical
/// viewport**, scissored to the window minus the sidebar column, and the
/// **chrome**, which owns the whole window and goes down last. Steps 1–10 below
/// are tactical; the screen-fixed block at the end releases the scissor first.
///
/// Tactical submission: terrain, bridge/overlay bodies, overlay shadows and
/// railings, native Ground parents, particles and upper-layer bodies, then
/// debug, shroud/fog and UI. A bridge body split stays within its unit parent.
pub(super) fn dispatch_draw_passes(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    data: &DrawPassData<'_>,
) {
    let pool: &InstanceBufferPool = &state.renderer.instance_pool;
    let transition_cache = state.renderer.vxl_slope_transition_cache.borrow();
    let pose_cache = state.renderer.vxl_pose_frame_cache.borrow();
    let mut pass = begin_main_pass(encoder, view, &state.renderer.depth_view);

    // Everything from here to the screen-fixed chrome block is battlefield: it
    // belongs to the tactical viewport and must not be able to paint a pixel
    // into the sidebar column. The native engine gets that for free — the
    // battlefield composes into its own surface and every object draw is handed
    // the intersection of its screen rect with the tactical rect as a clip. VERA
    // composes into one target, so the scissor is what enforces it.
    //
    // Keep the native surface boundary independent of chrome coverage. The
    // ordinary sidebar now uses the resolved stock side archive, real source
    // canvas heights, and one pixel per source pixel in every theme.
    let (tac_x, tac_y, tac_w, tac_h) = crate::app::input::camera::tactical_viewport_px(state);
    pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);

    // --- Step 1: Terrain (Z-depth pipeline for per-pixel depth from TMP Z-data) ---
    draw_pooled_zdepth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        state.match_state.match_presentation.tile_atlas.as_ref(),
        "terrain",
    );

    // --- Step 1.5: Smudges (static decals: craters + scorches) ---
    // The native terrain-tile pass dispatches each cell's smudge right after
    // blitting that cell's tile, so smudges land in the terrain layer — well
    // before the cell-content layer that draws overlays. Drawing them after
    // overlays instead put every crater and scorch mark on top of the ore and
    // walls it should be lying under.
    draw_pooled_passthrough_overlay(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        state.match_state.match_presentation.overlay_atlas.as_ref(),
        "smudge",
    );

    // --- Step 2: Bridge body (Z-depth pipeline) ---
    draw_pooled_bridge_zdepth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        state.match_state.match_presentation.bridge_atlas.as_ref(),
        "overlay_bridge_body",
    );

    // (Bridge body shadows are NOT drawn here. The native cell-content layer
    // runs two full sweeps — every overlay body, then every overlay shadow —
    // so shadows belong after the overlay bodies at step 3.5, not between the
    // bridge body and the overlays.)

    // --- Step 3: Overlay bodies (SHP depth read + write) ---
    // Active walls/ordinary overlays are 0x4E00 CC_Draw_Shape draws at
    // 0x0047F6A0, reached through 0x006D6D10. They are not the TMP tile
    // path at 0x00480350. Their class gradient, stored frame rectangle and
    // height adjustment keep them above the ground and let them occlude later
    // buildings/units. Preserve fixed cell order, including unsupported
    // slope-shape records that still use their previous passthrough policy.
    draw_pooled_overlay_bodies(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        state.match_state.match_presentation.overlay_atlas.as_ref(),
        "overlay",
        data.overlay_render_z,
    );

    // --- Step 3.5: Overlay shadows — bridge decks ---
    //
    // **This is the only separate shadow pass in the renderer.** GSI-13.11
    // covers three — ground, object and voxel. Voxel ground vehicles and ships
    // now cast their shadow as the first piece of their own draw (see
    // `instances::units::emit_unit_shadow_sprite`, `VxlLayer::Shadow`).
    // Still missing: infantry and SHP vehicle shadow halves, building shadows,
    // and aircraft (FlyLocomotion shadow matrix/point). Recorded, not closed.
    // Trigger: every frame with such an object on screen. Player effect: those
    // objects read flat against retail. Frequency: continuous.
    // Downstream risk: the SHP-blitter contract the bridge path already honours
    // (1-bit stencil half, composited darken) is the shape the object pass has
    // to reuse, and `BRIDGE_SHADOW_DARKEN_ALPHA` already carries its own
    // recorded lightness drift against the native halve.
    // Second sweep of the native cell-content layer: after every overlay body
    // is down, each overlay-bearing cell draws its shadow half. The atlas bakes
    // these as black texels whose alpha approximates the blitter's darken, so
    // the ordinary passthrough pipeline gets the shape and the
    // composite-on-overlap behaviour. The darken STRENGTH is a known drift —
    // this pass blends in linear space against an sRGB target while the blitter
    // halves the encoded word, leaving the shadow lighter than retail. See
    // `render::bridge_atlas::SHADOW_DARKEN_ALPHA`.
    //
    // Only bridge decks are covered so far — ore, gem and wall shadows still
    // need their own instance bucket and pooled buffer.
    draw_pooled_bridge_passthrough(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        state.match_state.match_presentation.bridge_atlas.as_ref(),
        "overlay_bridge_body_shadow",
    );

    // (Smudges are drawn back at step 1.5, inside the terrain layer, matching
    // the native per-cell tile-then-smudge dispatch. Instance construction now
    // projects the footprint origin with its resolved cell level, so hilltop
    // composites share the terrain tile's elevation. Their depth value is
    // irrelevant either
    // way — this pass neither reads nor writes the depth buffer.)

    // Native 0x547230 railing draw is reached through 0x4802A0 / 0x6D7C00
    // in the cell-content layer (0x6D3040), before the object loop (0x6D3D10).
    // Flags 0x4601 do not write Z. A late railing pass would repaint units
    // after their native composite depth test. See the bridge depth report.
    draw_pooled_bridge_railing(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        state
            .match_state
            .match_presentation
            .bridge_railing_atlas
            .as_ref(),
        "overlay_bridge_railing",
    );

    // Building selection bracket back/left edges. Drawn before object bodies so
    // the normal SHP merge naturally occludes the hidden bracket edges.
    let bracket_tex = state
        .match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|o| o.white_texture());
    draw_pooled_passthrough_texture(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        bracket_tex,
        "selection_brackets_back",
    );
    // gamemd's first object pass calls DrawExtras before the nonzero +0x104
    // display call. Re-submit the front/right bracket stubs here; object body
    // draws can still occlude this first submission, and the later DrawExtras
    // phase submits them again.
    draw_pooled_passthrough_texture(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        bracket_tex,
        "selection_brackets_front_first",
    );

    // Bridge units share the native Ground parent pass below. Their body
    // split changes per-pixel Z, never their position among buildings.

    // --- Step 5: Ground objects (native integer LayerClass order) ---
    // Terrain, units, infantry, and building-owned pieces share the exact
    // signed X+Y + stable-registration order. Atlas bindings dispatch only
    // after the parent slot has been selected.
    drop(pass);
    merge_passes::draw_native_ground_object_pass(
        encoder,
        view,
        &state.renderer.depth_view,
        &mut state.renderer.terrain_draw_renderer,
        [tac_x, tac_y, tac_w, tac_h],
        &state.renderer.batch_renderer,
        pool,
        data.ground,
        state.match_state.match_presentation.overlay_atlas.as_ref(),
        state.match_state.match_presentation.unit_atlas.as_ref(),
        &transition_cache,
        state.match_state.match_presentation.sprite_atlas.as_ref(),
        state.match_state.match_presentation.palette_set.as_ref(),
        state
            .match_state
            .match_presentation
            .building_zshape
            .as_ref()
            .map_or(
                state.renderer.batch_renderer.default_zshape_bind_group(),
                |z| &z.bind_group,
            ),
    );

    let mut pass = begin_main_load_pass(encoder, view, &state.renderer.depth_view);
    pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);

    // Scheduler-owned effects not yet carrying verified class-specific
    // YSortAdjust remain in the pre-existing residual SHP stream.
    merge_passes::draw_merged_object_pass(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        data.unit_instances,
        data.unit_pages,
        data.unit_transition_paged,
        data.shp_paged,
        state.match_state.match_presentation.unit_atlas.as_ref(),
        &transition_cache,
        state.match_state.match_presentation.sprite_atlas.as_ref(),
        state.match_state.match_presentation.palette_set.as_ref(),
    );

    if let (Some(overlay), Some((buffer, count))) = (
        state
            .match_state
            .match_presentation
            .selection_overlay
            .as_ref(),
        pool.get("weapon_waves"),
    ) {
        state.renderer.batch_renderer.draw_with_buffer_passthrough(
            &mut pass,
            overlay.white_texture(),
            buffer,
            count,
        );
    }

    // (There is no separate building-turret pass. gamemd draws a building's
    // voxel turret inside the building's own display call, in the sorted
    // ground layer, right after the body — the pass that does run after
    // layer 2 walks the building array to draw a production/ally overlay and
    // never touches a turret. The turret instances are therefore emitted into
    // the same UnitAtlas stream as the vehicles and interleave with them in
    // step 5; see the note in build_instances.)

    // --- Step 7.5: Particles (Layer 3, above all ground geometry) ---
    // ParticleClass::GetLayer = 3 in the original engine, drawing particles
    // above Layer 2 (buildings, units, turrets).
    // Passthrough pipeline (no depth interaction) — particles are translucent
    // and Y-sorted on the CPU, so no GPU depth read/write needed.
    if let Some(atlas) = state.match_state.match_presentation.sprite_atlas.as_ref() {
        for (page, atlas_page) in atlas.pages.iter().enumerate() {
            if let Some((buf, count)) = pool.get_page("particle_page", page) {
                state.renderer.batch_renderer.draw_passthrough_range(
                    &mut pass,
                    &atlas_page.texture,
                    buf,
                    0,
                    count,
                );
            }
        }
    }

    // BuildingLightClass registers in layer 3. This pass is exact once its
    // authoritative child-light coordinates are emitted; it deliberately does
    // not substitute the parent building coordinate.
    if let Some((buffer, count)) = pool.get("spotlight_type16") {
        state
            .renderer
            .batch_renderer
            .draw_spotlight_type16(&mut pass, buffer, count);
    }

    // --- Step 7.7: The band above Ground (gamemd layers 3 and 4) ---
    // The native object loop walks its display layers in index order and only
    // layer 2 is kept sorted, so everything an air locomotor puts in layers 3
    // and 4 is drawn after every ground object, in submission order. That is
    // the whole reason this pass exists: an aircraft off its pad must never be
    // covered by a building or a unit, whatever iso row it happens to be over.
    //
    // Instance order inside the band is emission order, not depth — see the
    // note on `top_unit` in build_instances.
    //
    // Current SHP and VXL upper-body pipelines read depth without writing it.
    // Complete native upper-layer depth/terrain interaction remains outside
    // this Ground bridge correction; painter band alone does not prove it.
    if let (Some(unit_atlas), Some(palette_set)) = (
        state.match_state.match_presentation.unit_atlas.as_ref(),
        state.match_state.match_presentation.palette_set.as_ref(),
    ) {
        if let Some((buf, count)) = pool.get("unit_top") {
            if count > 0 {
                merge_passes::draw_unit_atlas_page_runs(
                    &mut pass,
                    &state.renderer.batch_renderer,
                    unit_atlas,
                    palette_set,
                    buf,
                    data.top_unit_pages,
                    0,
                    count,
                    pose_cache.texture(),
                );
            }
        }
    }
    if let (Some(atlas), Some((buffer, count))) = (
        state.match_state.match_presentation.sprite_atlas.as_ref(),
        pool.get("shp_top"),
    ) && count > 0
    {
        merge_passes::draw_shp_atlas_page_runs(
            &mut pass,
            &state.renderer.batch_renderer,
            atlas,
            buffer,
            data.top_shp_pages,
            0,
            count,
        );
    }

    // --- Step 7.8: Persistent combat-light vector ---
    // gamemd edits the completed tactical object surface here, tail-to-head,
    // before the later debug/shroud/UI families. End the sRGB/depth pass while
    // the dedicated renderer performs its encoded RGB565 destination edits,
    // then resume both attachments with Load.
    drop(pass);
    state
        .renderer
        .combat_light_renderer
        .draw(encoder, [tac_x, tac_y, tac_w, tac_h]);
    let mut pass = begin_main_load_pass(encoder, view, &state.renderer.depth_view);
    pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);

    // --- Step 8: Debug overlays ---
    // Drawn above entities, below fog and UI.
    // Use filled-diamond texture so cells appear as isometric diamonds, not rectangles.
    let debug_diamond_tex = state
        .match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|o| o.diamond_filled_texture());
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        debug_diamond_tex,
        "debug_pathgrid",
    );
    let grid_tex = state
        .match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|o| o.diamond_outline_texture());
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        grid_tex,
        "debug_cell_grid",
    );
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        debug_diamond_tex,
        "debug_path",
    );
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        debug_diamond_tex,
        "debug_heightmap",
    );

    // --- Step 9: Shroud (GPU ABuffer multiply pass) ---
    // Darkens every scene pixel by the shroud brightness value via
    // per-pixel ABuffer lookup.
    // Fully shrouded areas → black, edge cells → gradient, explored → no change.
    if let Some(ref buf) = state.match_state.match_presentation.shroud_buffer {
        if !state.match_state.sandbox_full_visibility {
            buf.draw(&mut pass);
        }
    }

    // --- Step 10: UI elements ---
    // Factory rally and selected action lines are separate line families.
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        bracket_tex,
        "factory_rally_first",
    );
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        bracket_tex,
        "target_lines",
    );
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        bracket_tex,
        "factory_rally_second",
    );
    // Isometric selection brackets for buildings: white 1px stub lines at 3 roof corners.
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        bracket_tex,
        "building_radius_rings",
    );
    // Final selected-building front bracket redraw: gamemd line pixels test Z
    // but do not write it — the store back into Z sits behind a caller flag
    // this path leaves clear. Each pixel carries its ground-footprint corner's
    // depth, so the marks that fall behind the building art lose the test
    // against the Z the building body wrote in the Ground pass (BUILDNGZ
    // shaped, `0x004990e0`); no separate depth stamp is needed. The CPU
    // instance builder already samples the tactical ABuffer for this
    // post-shroud redraw.
    draw_pooled_depth_test_texture(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        bracket_tex,
        "selection_brackets_front",
    );
    // Building health pips: discrete pips from pips.shp atlas.
    let building_status_tex = state
        .match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|o| o.pip_texture().unwrap_or_else(|| o.white_texture()));
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        building_status_tex,
        "status_building",
    );
    // The Crazy Ivan bomb clock, drawn before the veterancy chevrons that
    // share the occupant pass, as `DrawExtras` orders them.
    let bomb_clock_tex = state
        .match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .and_then(|o| o.bomb_clock())
        .map(|clock| clock.texture());
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        bomb_clock_tex,
        "bomb_clocks",
    );
    // Occupant pips for garrisoned buildings (pips.shp frames 6-12).
    let occupant_pip_tex = state
        .match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|o| {
            o.occupant_pip_texture()
                .unwrap_or_else(|| o.white_texture())
        });
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        occupant_pip_tex,
        "occupant_pips",
    );
    // Non-building health bar backgrounds: pipbrd.shp bracket sprites.
    let unit_bg_tex = state
        .match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .and_then(|o| o.pipbrd_texture());
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        unit_bg_tex,
        "status_unit_bg",
    );
    // Non-building health bar fills: individual pip sprites from pips.shp (or white_texture fallback).
    let unit_fill_tex = state
        .match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|o| o.unit_pip_texture().unwrap_or_else(|| o.white_texture()));
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        unit_fill_tex,
        "status_unit_fill",
    );
    // Tiberium cargo pips for harvesters (pips2.shp frames 0, 2, 5).
    let cargo_pip_tex = state
        .match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|o| {
            o.tiberium_pip_texture()
                .unwrap_or_else(|| o.white_texture())
        });
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        cargo_pip_tex,
        "cargo_pips",
    );
    // Drag rectangle — screen-fixed, use UI camera (zoom=1.0).
    let drag_tex = state
        .match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|o| o.drag_texture());
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        drag_tex,
        "drag",
    );
    // Placement preview — world-space, uses world camera (zoom).
    let ghost_tex = state
        .match_state
        .match_presentation
        .sprite_atlas
        .as_ref()
        .and_then(|a| a.page(data.ghost_page as usize))
        .map(|p| &p.texture);
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        ghost_tex,
        "placement_ghost",
    );
    let wall_ghost_tex = state
        .match_state
        .match_presentation
        .overlay_atlas
        .as_ref()
        .map(|a| &a.texture);
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        wall_ghost_tex,
        "placement_wall_ghost",
    );
    let valid_tex = state
        .match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|o| o.preview_valid_texture());
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        valid_tex,
        "placement_valid",
    );
    let invalid_tex = state
        .match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|o| o.preview_invalid_texture());
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        invalid_tex,
        "placement_invalid",
    );

    // --- Step 10.5: PixelFX water/ore sparkles ---
    // gamemd writes these opaque one-pixel effects at the tactical tail, after
    // object/effect/status/action/placement drawing and before screen-fixed
    // chrome. In VERA the global shroud translation must therefore run first.
    // The passthrough pipeline bypasses depth; an empty buffer when
    // `[Options] DetailLevel=0` short-circuits at count == 0.
    if let (Some(overlay), Some((buf, count))) = (
        state
            .match_state
            .match_presentation
            .selection_overlay
            .as_ref(),
        pool.get("cell_sparkles"),
    ) {
        state.renderer.batch_renderer.draw_with_buffer_passthrough(
            &mut pass,
            overlay.white_texture(),
            buf,
            count,
        );
    }

    // --- Screen-fixed UI: sidebar, minimap, cursor — use UI camera (zoom=1.0) ---
    // Chrome owns the whole window: the sidebar column, the message list that
    // starts at the tactical origin, tooltips, and the cursor, which the native
    // engine draws over both regions. Release the tactical scissor before any of
    // it goes down.
    pass.set_scissor_rect(0, 0, state.render_width(), state.render_height());
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        state
            .match_state
            .match_presentation
            .minimap
            .as_ref()
            .map(|m| m.white_texture()),
        "sidebar",
    );
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        current_sidebar_chrome_texture(state),
        "sidebar_chrome",
    );
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        state
            .match_state
            .match_presentation
            .sidebar_cameo_atlas
            .as_ref()
            .map(|atlas| &atlas.texture),
        "sidebar_cameo",
    );
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        current_sidebar_gclock_texture(state),
        "sidebar_gclock",
    );
    let cameo_overlay_tex = state.renderer.bit_font.darken_texture().or_else(|| {
        state
            .match_state
            .match_presentation
            .selection_overlay
            .as_ref()
            .map(|o| o.white_texture())
    });
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        cameo_overlay_tex,
        "sidebar_cameo_overlay",
    );
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        Some(state.renderer.bit_font.atlas()),
        "sidebar_text",
    );

    // SidebarClass::Draw @ 0x006A6C30 paints background, gadgets, strip, and
    // power before PowerClass::Draw reaches RadarClass::Draw @ 0x00653100.
    // Radar state/chrome preparation then precedes Update @ 0x00656EC0, whose
    // content blit, viewport rectangle, and generated-content boundary are the
    // final retained radar writes in that order.
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        state
            .match_state
            .match_presentation
            .radar_anim
            .as_ref()
            .map(|ra| ra.texture()),
        "radar_anim",
    );
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        state
            .match_state
            .match_presentation
            .minimap
            .as_ref()
            .map(|m| m.map_texture()),
        "minimap",
    );
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        state
            .match_state
            .match_presentation
            .minimap
            .as_ref()
            .map(|m| m.white_texture()),
        "viewport_rect",
    );
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        state
            .match_state
            .match_presentation
            .minimap
            .as_ref()
            .map(|m| m.white_texture()),
        "radar_content_boundary",
    );
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        Some(state.renderer.bit_font.atlas()),
        "message_text",
    );
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        state.renderer.bit_font.darken_texture(),
        "tooltip_fill",
    );
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        Some(state.renderer.bit_font.atlas()),
        "tooltip_text",
    );

    // Active YR ScreenCaptureCommandClass::Execute (0x00537BC0) hides
    // WWMouse while copying the already-presented client. Preserve the same
    // composition boundary without changing the displayed frame: retain every
    // completed UI/sidebar surface here, then resume and draw the cursor into
    // the ordinary presentation target.
    drop(pass);
    state
        .renderer
        .retail_screenshot_frame_cache
        .stage_pre_cursor_composition(
            &state.renderer.gpu.device,
            encoder,
            state.renderer.combat_light_renderer.composition_texture(),
            state.renderer.gpu.config.format,
            state.render_width(),
            state.render_height(),
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        );
    let mut pass = begin_main_load_pass(encoder, view, &state.renderer.depth_view);
    pass.set_scissor_rect(0, 0, state.render_width(), state.render_height());
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        current_software_cursor_texture(state),
        "software_cursor",
    );
}

// ---------------------------------------------------------------------------
// Draw helpers — thin wrappers around BatchRenderer methods with atlas lookup
// ---------------------------------------------------------------------------

/// Draw a pooled buffer with the Z-depth pipeline (per-pixel frag_depth).
/// Uses the tile atlas's pre-built zdepth bind group (color + R8 depth textures).
fn draw_pooled_zdepth<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    atlas: Option<&'a TileAtlas>,
    key: &'static str,
) {
    if let (Some(a), Some((buf, count))) = (atlas, pool.get(key)) {
        batch.draw_with_buffer_zdepth(pass, &a.zdepth_bind_group, buf, count);
    }
}

fn draw_pooled_bridge_zdepth<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    atlas: Option<&'a BridgeAtlas>,
    key: &'static str,
) {
    if let (Some(a), Some((buf, count))) = (atlas, pool.get(key)) {
        batch.draw_with_buffer_zdepth(pass, &a.zdepth_bind_group, buf, count);
    }
}

/// Draw a pooled buffer with LessEqual depth test, depth write ON.
/// Used for the base terrain pass and UI/debug passes that write depth.
fn draw_pooled_no_depth<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    tex: Option<&'a BatchTexture>,
    key: &'static str,
) {
    if let (Some(t), Some((buf, count))) = (tex, pool.get(key)) {
        batch.draw_with_buffer_no_depth(pass, t, buf, count);
    }
}

/// Draw with the UI camera (zoom=1.0) for screen-fixed elements.
/// Uses the overlay pipeline (no depth) but sets bind group 0 to the UI camera
/// so sidebar, minimap, and cursor stay at fixed screen positions regardless of zoom.
fn draw_pooled_ui<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    tex: Option<&'a BatchTexture>,
    key: &'static str,
) {
    if let (Some(t), Some((buf, count))) = (tex, pool.get(key)) {
        if count == 0 {
            return;
        }
        pass.set_pipeline(batch.overlay_pipeline());
        pass.set_bind_group(0, batch.ui_camera_bind_group(), &[]);
        pass.set_bind_group(1, &t.bind_group, &[]);
        pass.set_vertex_buffer(0, buf.slice(..));
        pass.draw(0..6, 0..count);
    }
}

/// Draw decals with depth bypassed, using the shared overlay atlas.
fn draw_pooled_passthrough_overlay<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    atlas: Option<&'a OverlayAtlas>,
    key: &'static str,
) {
    if let (Some(a), Some((buf, count))) = (atlas, pool.get(key)) {
        batch.draw_with_buffer_passthrough(pass, &a.texture, buf, count);
    }
}

/// Adjacent policy runs preserve the native cell traversal exactly; grouping
/// every wall together would change color/depth ties against nearby overlays.
fn overlay_policy_runs(policies: &[RenderZPolicy]) -> Vec<(u32, u32, RenderZPolicy)> {
    let mut runs = Vec::new();
    let mut start = 0;
    while start < policies.len() {
        let policy = policies[start];
        let mut end = start + 1;
        while end < policies.len() && policies[end] == policy {
            end += 1;
        }
        runs.push((start as u32, (end - start) as u32, policy));
        start = end;
    }
    runs
}

fn draw_pooled_overlay_bodies<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    atlas: Option<&'a OverlayAtlas>,
    key: &'static str,
    policies: &[RenderZPolicy],
) {
    let (Some(atlas), Some((buf, count))) = (atlas, pool.get(key)) else {
        return;
    };
    assert_eq!(
        count as usize,
        policies.len(),
        "overlay policies must describe the uploaded buffer"
    );
    for (start, count, policy) in overlay_policy_runs(policies) {
        if policy == RenderZPolicy::None {
            batch.draw_passthrough_range(pass, &atlas.texture, buf, start, count);
        } else {
            batch.draw_zsprite_range(
                pass,
                &atlas.texture,
                batch.default_zshape_bind_group(),
                buf,
                start,
                count,
                matches!(
                    policy,
                    RenderZPolicy::ReadWrite | RenderZPolicy::AlphaReadWrite
                ),
            );
        }
    }
}

fn draw_pooled_passthrough_texture<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    tex: Option<&'a BatchTexture>,
    key: &'static str,
) {
    if let (Some(t), Some((buf, count))) = (tex, pool.get(key)) {
        batch.draw_with_buffer_passthrough(pass, t, buf, count);
    }
}

/// Draw a pooled buffer that tests the depth buffer and does not write it.
fn draw_pooled_depth_test_texture<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    tex: Option<&'a BatchTexture>,
    key: &'static str,
) {
    if let (Some(t), Some((buf, count))) = (tex, pool.get(key)) {
        batch.draw_with_buffer_depth_test(pass, t, buf, count);
    }
}

/// Draw a pooled bridge buffer with passthrough (no depth test, no depth
/// write). Used for the body shadow pass — same texture as the bridge body,
/// just a different draw pipeline.
fn draw_pooled_bridge_passthrough<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    atlas: Option<&'a BridgeAtlas>,
    key: &'static str,
) {
    if let (Some(a), Some((buf, count))) = (atlas, pool.get(key)) {
        batch.draw_with_buffer_passthrough(pass, &a.texture, buf, count);
    }
}

/// Draw a pooled buffer using the bridge railing atlas with passthrough
/// (Z-test ON, Z-write OFF).
fn draw_pooled_bridge_railing<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    atlas: Option<&'a crate::render::bridge_railing_atlas::BridgeRailingAtlas>,
    key: &'static str,
) {
    if let (Some(a), Some((buf, count))) = (atlas, pool.get(key)) {
        batch.draw_with_buffer_passthrough(pass, &a.texture, buf, count);
    }
}

#[cfg(test)]
mod tests {
    use super::{RenderZPolicy, overlay_policy_runs};
    const SOURCE: &str = include_str!("draw_passes.rs");

    #[test]
    fn overlay_depth_runs_do_not_reorder_walls_around_special_overlays() {
        use RenderZPolicy::{None, ReadWrite};
        assert_eq!(
            overlay_policy_runs(&[ReadWrite, ReadWrite, None, ReadWrite]),
            [(0, 2, ReadWrite), (2, 1, None), (3, 1, ReadWrite)]
        );
        assert!(overlay_policy_runs(&[]).is_empty());
    }

    fn source_offset(needle: &str) -> usize {
        SOURCE
            .find(needle)
            .unwrap_or_else(|| panic!("missing production draw anchor {needle:?}"))
    }

    #[test]
    fn gsi_13_01_pixel_fx_is_last_tactical_write_before_screen_chrome() {
        let shroud = source_offset("// --- Step 9: Shroud");
        let target_lines = source_offset("\"target_lines\"");
        let status = source_offset("\"status_unit_fill\"");
        let placement = source_offset("\"placement_invalid\"");
        let sparkle = source_offset("pool.get(\"cell_sparkles\")");
        let screen_fixed = source_offset("// --- Screen-fixed UI:");
        let full_window_scissor = source_offset(
            "pass.set_scissor_rect(0, 0, state.render_width(), state.render_height());",
        );
        let first_screen_submission = source_offset("\"minimap\"");

        assert!(shroud < target_lines);
        assert!(target_lines < status);
        assert!(status < placement);
        assert!(placement < sparkle);
        assert!(sparkle < screen_fixed);
        assert!(screen_fixed < full_window_scissor);
        assert!(full_window_scissor < first_screen_submission);

        let final_tactical_slice = &SOURCE[sparkle..screen_fixed];
        assert_eq!(
            final_tactical_slice.matches(".draw").count(),
            1,
            "PixelFX must remain the final tactical draw submission"
        );
    }

    #[test]
    fn gsi_13_01_pixel_fx_tail_remains_passthrough_and_tactically_scissored() {
        let tactical_scissor = source_offset("pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);");
        let sparkle = source_offset("pool.get(\"cell_sparkles\")");
        let full_window_scissor = source_offset(
            "pass.set_scissor_rect(0, 0, state.render_width(), state.render_height());",
        );

        assert!(tactical_scissor < sparkle);
        assert!(sparkle < full_window_scissor);
        assert!(SOURCE[sparkle..full_window_scissor].contains("draw_with_buffer_passthrough"));
    }

    #[test]
    fn gsi_04_01_retained_sidebar_radar_subpass_ends_with_content_boundary() {
        let sidebar = source_offset("\"sidebar\"");
        let chrome = source_offset("\"sidebar_chrome\"");
        let cameo = source_offset("\"sidebar_cameo\"");
        let gclock = source_offset("\"sidebar_gclock\"");
        let cameo_overlay = source_offset("\"sidebar_cameo_overlay\"");
        let sidebar_text = source_offset("\"sidebar_text\"");
        let radar_anim = source_offset("\"radar_anim\"");
        let minimap = source_offset("\"minimap\"");
        let viewport = source_offset("\"viewport_rect\"");
        let boundary = source_offset("\"radar_content_boundary\"");
        let message = source_offset("\"message_text\"");

        assert!(sidebar < chrome);
        assert!(chrome < cameo);
        assert!(cameo < gclock);
        assert!(gclock < cameo_overlay);
        assert!(cameo_overlay < sidebar_text);
        assert!(sidebar_text < radar_anim);
        assert!(radar_anim < minimap);
        assert!(minimap < viewport);
        assert!(viewport < boundary);
        assert!(boundary < message);

        // An oversize native viewport edge may leave the 140x108 aperture but
        // still remain inside g_SidebarSurface. No later retained-sidebar or
        // radar batch may repaint that accepted line before the screen-overlay
        // strata begin.
        let retained_tail = &SOURCE[boundary..message];
        for later_retained_batch in [
            "\"sidebar\"",
            "\"sidebar_chrome\"",
            "\"sidebar_cameo\"",
            "\"sidebar_gclock\"",
            "\"sidebar_cameo_overlay\"",
            "\"sidebar_text\"",
            "\"radar_anim\"",
            "\"minimap\"",
            "\"viewport_rect\"",
        ] {
            assert!(
                !retained_tail.contains(later_retained_batch),
                "{later_retained_batch} must not overwrite the final radar outline"
            );
        }
    }

    #[test]
    fn gsi_04_01_tooltip_and_cursor_remain_after_the_retained_sidebar_surface() {
        let viewport = source_offset("\"viewport_rect\"");
        let boundary = source_offset("\"radar_content_boundary\"");
        let message = source_offset("\"message_text\"");
        let tooltip_fill = source_offset("\"tooltip_fill\"");
        let tooltip_text = source_offset("\"tooltip_text\"");
        let screenshot_boundary = source_offset("stage_pre_cursor_composition");
        let cursor = source_offset("\"software_cursor\"");

        assert!(viewport < boundary);
        assert!(boundary < message);
        assert!(message < tooltip_fill);
        assert!(tooltip_fill < tooltip_text);
        assert!(tooltip_text < screenshot_boundary);
        assert!(screenshot_boundary < cursor);
    }
}
