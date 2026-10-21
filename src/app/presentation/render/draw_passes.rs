//! Draw pass dispatch — creates the wgpu render pass and issues all draw calls in order.
//!
//! Separated from the instance-building phase in mod.rs so the orchestrator stays focused
//! on *what* to render while this module handles *how* to submit it to the GPU.
//!
//! ## Dependency rules
//! - Internal to `presentation::render` — only called from mod.rs via `dispatch_draw_passes()`.

use crate::app::AppState;
use crate::app::presentation::instances::{CellBodyAtlas, CellBodyDraw};
use crate::app::presentation::sidebar_render::{begin_main_load_pass, begin_main_pass};
use crate::app::presentation::sidebar_surface::SidebarPart;
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
    pub cell_body_draws: &'a [CellBodyDraw],
    pub bridge_shadows: &'a [SpriteInstance],
    pub object_layers: &'a [super::draw_plan_lowering::ObjectLayerPass; 5],
    pub building_status_text:
        &'a crate::app::presentation::ui_overlays::BuildingStatusTextInstances,
    pub surface_effects: crate::render::terrain_draw::SurfaceLineBatch,
    pub tactical_lines: crate::render::terrain_draw::SurfaceLineBatch,
    pub persistent_lights: std::ops::Range<usize>,
    pub sidebar_unavailable: &'a [SpriteInstance],
    pub sidebar_flash: &'a [SpriteInstance],
    pub sidebar_gclock: &'a [SpriteInstance],
}

/// Draw the sidebar into its own surface, as gamemd draws `g_SidebarSurface`
/// before `SidebarClass::BlitToScreen` (`6A70E0`) copies it to the screen,
/// and the command bar into the bar's. Their layers are in layout space
/// (`ui::sidebar::frame`), drawn with each surface's camera at its own pixels.
///
/// SidebarClass::Draw @ 0x006A6C30 paints background, gadgets, strip, and
/// power before PowerClass::Draw reaches RadarClass::Draw @ 0x00653100.
/// Radar state/chrome preparation then precedes Update @ 0x00656EC0, whose
/// content blit, viewport rectangle, and generated-content boundary are the
/// final retained radar writes in that order.
fn draw_sidebar_surface(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    data: &DrawPassData<'_>,
) {
    let side_index = crate::app::presentation::sidebar_render::current_sidebar_side(state);
    let presentation = &state.match_state.match_presentation;
    let chrome = presentation
        .sidebar_chrome
        .as_ref()
        .and_then(|set| set.resolve_side(side_index))
        .map(|resolved| resolved.atlas);
    let renderer = &mut state.renderer;
    let Some(surface) = renderer.sidebar_surface.as_ref() else {
        return;
    };
    let batch = &renderer.batch_renderer;
    let pool = &renderer.instance_pool;
    let camera = surface.camera(SidebarPart::Column);
    let mut pass = surface.begin_pass(SidebarPart::Column, encoder, super::CLEAR_COLOR);
    let mut draw = |tex: Option<&BatchTexture>, key: &'static str| {
        draw_pooled_with_camera(&mut pass, batch, camera, pool, tex, key);
    };
    draw(chrome.map(|atlas| &atlas.texture), "sidebar_chrome");
    draw(
        presentation
            .sidebar_cameo_atlas
            .as_ref()
            .map(|atlas| &atlas.texture),
        "sidebar_cameo",
    );
    drop(pass);

    // Original StripDraw: icon6A9A3E -> unavailable4016A9B46 -> flash404
    //6A9BC0. All packed operations share one owner and preserve sidebar Z.
    for (key, instances, texture, kind) in [
        (
            "sidebar_unavailable",
            data.sidebar_unavailable,
            chrome
                .and_then(|atlas| atlas.darken.as_ref())
                .map(|mask| &mask.texture),
            crate::render::terrain_draw::PackedSpriteKind::IndexMask,
        ),
        (
            "sidebar_flash",
            data.sidebar_flash,
            chrome
                .and_then(|atlas| atlas.darken.as_ref())
                .map(|mask| &mask.texture),
            crate::render::terrain_draw::PackedSpriteKind::UiPalette,
        ),
    ] {
        if let (Some((buffer, count)), Some(texture)) = (pool.get(key), texture)
            && count > 0
        {
            assert_eq!(count as usize, instances.len());
            surface.draw_packed_layer(
                &mut renderer.terrain_draw_renderer,
                encoder,
                batch,
                buffer,
                instances,
                texture,
                kind,
            );
        }
    }

    let cameo_overlay_tex = renderer.bit_font.darken_texture().or_else(|| {
        presentation
            .selection_overlay
            .as_ref()
            .map(|o| o.white_texture())
    });
    {
        // Count badge rect/fill6A9D0C/6A9D4C and glyph6A9D7B precede
        // the clock. Its404 packed destination read includes these pixels.
        let mut pass = surface.begin_load_pass(SidebarPart::Column, encoder);
        draw_pooled_with_camera(
            &mut pass,
            batch,
            camera,
            pool,
            cameo_overlay_tex,
            "sidebar_queue_overlay",
        );
        draw_pooled_with_camera(
            &mut pass,
            batch,
            camera,
            pool,
            Some(renderer.bit_font.atlas()),
            "sidebar_queue_text",
        );
    }
    if let (Some((buffer, count)), Some(texture)) = (
        pool.get("sidebar_gclock"),
        chrome.and_then(|atlas| atlas.gclock_texture.as_ref()),
    ) && count > 0
    {
        assert_eq!(count as usize, data.sidebar_gclock.len());
        surface.draw_packed_layer(
            &mut renderer.terrain_draw_renderer,
            encoder,
            batch,
            buffer,
            data.sidebar_gclock,
            texture,
            crate::render::terrain_draw::PackedSpriteKind::UiPalette,
        );
    }
    // READY skips clock6A9E44; HOLD is after it6AA014. Separate status
    // layers preserve both without moving the earlier queue badge.
    let mut pass = surface.begin_load_pass(SidebarPart::Column, encoder);
    let mut draw = |tex: Option<&BatchTexture>, key: &'static str| {
        draw_pooled_with_camera(&mut pass, batch, camera, pool, tex, key);
    };
    draw(cameo_overlay_tex, "sidebar_cameo_overlay");
    draw(Some(renderer.bit_font.atlas()), "sidebar_text");
    draw(
        presentation.radar_anim.as_ref().map(|ra| ra.texture()),
        "radar_anim",
    );
    let minimap = presentation.minimap.as_ref();
    draw(minimap.map(|m| m.map_texture()), "minimap");
    draw(minimap.map(|m| m.white_texture()), "viewport_rect");
    draw(minimap.map(|m| m.white_texture()), "radar_content_boundary");
    drop(pass);

    // The bottom command bar (6D0A20) shares the chrome art.
    let mut pass = surface.begin_pass(SidebarPart::Bar, encoder, super::CLEAR_COLOR);
    draw_pooled_with_camera(
        &mut pass,
        batch,
        surface.camera(SidebarPart::Bar),
        pool,
        chrome.map(|atlas| &atlas.texture),
        "command_bar",
    );
}

/// Create the main render pass and dispatch all draw calls in the correct order.
///
/// The frame has two regions, matching the native composition: the **tactical
/// viewport**, scissored to the frame minus the sidebar column, and the
/// **chrome**, which owns the whole frame and goes down last. Steps 1–10 below
/// are tactical; the screen-fixed block at the end copies in the sidebar's
/// surface (`draw_sidebar_surface`) and releases the scissor.
///
/// Tactical submission: terrain, the cell body sweep, overlay shadows and
/// railings, native Ground parents, particles and upper-layer bodies, then
/// debug and UI. World blitters sample the shared ABuffer. A bridge body split stays within its unit parent.
pub(super) fn dispatch_draw_passes(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    data: &DrawPassData<'_>,
) {
    // The sidebar's surface is complete before the frame begins; the
    // screen-fixed block copies it in.
    draw_sidebar_surface(state, encoder, data);
    let pool: &InstanceBufferPool = &state.renderer.instance_pool;
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

    // --- Step 2: The cell body sweep (SHP depth read + write) ---
    // High bridges, walls and ordinary overlays are 0x4E00 CC_Draw_Shape
    // draws at 0x0047F6A0, reached through one 0x006D6D10 sweep. They are not
    // the TMP tile path at 0x00480350. Their class gradient, stored frame
    // rectangle and height adjustment keep them above the ground and let them
    // occlude later buildings/units. The native cell-content layer runs this
    // whole sweep before its overlay shadow sweep, below.
    draw_pooled_cell_bodies(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        state.match_state.match_presentation.overlay_atlas.as_ref(),
        state.match_state.match_presentation.bridge_atlas.as_ref(),
        "cell_body",
        data.cell_body_draws,
    );

    // Cell::DrawOverlay_Shadow0047F510 uses native destination halving and
    // read/write Z after all overlay bodies. End the ordinary pass before the
    // shared renderer snapshots the same live color/depth attachments. Its
    // signed comparison must precede the low16 Z store; hardware Less alone
    // cannot implement negative native candidates. Original caller/shape
    // vectors: tools/spatial_oracle/bridge_shadow_render.md.
    if !data.bridge_shadows.is_empty() {
        drop(pass);
        state
            .renderer
            .terrain_draw_renderer
            .note_external_passes(encoder, 1);
        draw_bridge_shadows(
            encoder,
            view,
            &state.renderer.depth_view,
            &mut state.renderer.terrain_draw_renderer,
            [tac_x, tac_y, tac_w, tac_h],
            &state.renderer.batch_renderer,
            pool.get("overlay_bridge_body_shadow"),
            data.bridge_shadows,
            state
                .match_state
                .match_presentation
                .bridge_atlas
                .as_ref()
                .map(|atlas| &atlas.texture),
        );
        pass = begin_main_load_pass(encoder, view, &state.renderer.depth_view);
        pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);
    }

    // (Smudges are drawn back at step 1.5, inside the terrain layer, matching
    // the native per-cell tile-then-smudge dispatch. Instance construction now
    // projects the footprint origin with its resolved cell level, so hilltop
    // composites share the terrain tile's elevation. Their depth value is
    // irrelevant either
    // way — this pass neither reads nor writes the depth buffer.)

    // CellClass::DrawTileShadow4802A0 -> IsometricTileTypeClass547230 is
    // separate from overlay shadows: tile+2E1 starts false at544880 and is
    // enabled only by TileSet ShadowCaster && ShadowTiles!=0 (5464A6/546AF8).
    // All six stock MD theater INIs leave it disabled. Its literal resource is
    // C_SHADOW.SHP (545214..54524D), never RAILBRDG. Do not synthesize a bridge
    // railing pass from its tables. Retail gate/resource regression:
    // map::theater::theater_tests::active_retail_tile_shadow_route_is_dormant.

    // Native6D9920 ->43CEA0/43D290 draws Ground's building bases into the
    // background before rally6D4648. These ranges draw only here; Display's
    // building branch6D916C requests the dynamic turret43DA80 instead. Anim
    // slots and voxel turrets keep their independent later Display positions.
    if !data.object_layers[2].building_base_runs.is_empty() {
        drop(pass);
        state
            .renderer
            .terrain_draw_renderer
            .note_external_passes(encoder, 1);
        merge_passes::draw_native_object_pass(
            encoder,
            super::merge_passes::ObjectDrawPhase::BuildingBase,
            view,
            &state.renderer.depth_view,
            &mut state.renderer.terrain_draw_renderer,
            [tac_x, tac_y, tac_w, tac_h],
            &state.renderer.batch_renderer,
            pool.get_page("object_layer", 2),
            &data.object_layers[2],
            state.match_state.match_presentation.overlay_atlas.as_ref(),
            state.match_state.match_presentation.unit_atlas.as_ref(),
            Some(&pose_cache),
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
            None,
            None,
        );
        pass = begin_main_load_pass(encoder, view, &state.renderer.depth_view);
        pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);
    }

    let bracket_tex = state
        .match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|o| o.white_texture());
    // Original6D4648 draws the first rally pass BEFORE the object loop at
    // 6D465F. Building bases have already primed Z; mobile bodies and Anim
    // slots can cover the line. The second pass follows effects and BandBox.
    // Both rally leaves bypass Z.
    draw_pooled_ui_passthrough(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        bracket_tex,
        "factory_rally_first",
    );
    // Original6D4656 IonBlast DrawAll moves terrain pixels after the first
    // rally pass and before the object loop at 6D465F (render::terrain_draw).
    // Native draws the placement overlay (6D5030) before it; VERA draws the
    // placement cells after the objects, so a ripple does not shift them.
    if state.renderer.terrain_draw_renderer.has_ion_blasts() {
        drop(pass);
        state
            .renderer
            .terrain_draw_renderer
            .note_external_passes(encoder, 1);
        state
            .renderer
            .terrain_draw_renderer
            .draw_ion_blasts(encoder, view);
        pass = begin_main_load_pass(encoder, view, &state.renderer.depth_view);
        pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);
    }

    // Bridge units share the native Ground parent pass below. Their body
    // split changes per-pixel Z, never their position among buildings.

    // --- Step 5: Retained Display layers0..3 ---
    // Ground keeps the simulation's partial signed-key sort; other layers
    // retain submission order. Texture bindings never reorder parent slots.
    drop(pass);
    state
        .renderer
        .terrain_draw_renderer
        .note_external_passes(encoder, 1);
    for layer in 0..=3 {
        merge_passes::draw_native_object_pass(
            encoder,
            super::merge_passes::ObjectDrawPhase::Display,
            view,
            &state.renderer.depth_view,
            &mut state.renderer.terrain_draw_renderer,
            [tac_x, tac_y, tac_w, tac_h],
            &state.renderer.batch_renderer,
            pool.get_page("object_layer", layer),
            &data.object_layers[layer],
            state.match_state.match_presentation.overlay_atlas.as_ref(),
            state.match_state.match_presentation.unit_atlas.as_ref(),
            Some(&pose_cache),
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
            state
                .match_state
                .match_presentation
                .selection_overlay
                .as_ref()
                .zip(
                    pool.get_page("object_extras", layer)
                        .map(|(buffer, _)| buffer),
                )
                .map(|(overlay, buffer)| (overlay, buffer, Some(state.renderer.bit_font.atlas()))),
            Some(&state.renderer.combat_light_renderer),
        );
        if layer == 2 {
            // Tactical6D9427 admits this BuildingClass::Array sweep only
            // after Ground2, before Air3/Top4 and the late Extras sweep.
            super::object_extras::draw_building_status_text(
                encoder,
                view,
                &state.renderer.depth_view,
                &mut state.renderer.terrain_draw_renderer,
                [tac_x, tac_y, tac_w, tac_h],
                &state.renderer.batch_renderer,
                state
                    .match_state
                    .match_presentation
                    .selection_overlay
                    .as_ref(),
                pool.get("building_status_text").map(|(buffer, _)| buffer),
                Some(state.renderer.bit_font.atlas()),
                &data.building_status_text.runs,
            );
        }
    }

    let mut pass = begin_main_load_pass(encoder, view, &state.renderer.depth_view);
    pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);

    // (There is no separate building-turret pass. gamemd draws a building's
    // voxel turret inside the building's retained Display slot, after the
    // body. Atlas page changes preserve that parent and its piece order.)

    // Finish remaining Layer3 families before the distinct retained Top4.
    drop(pass);
    state
        .renderer
        .terrain_draw_renderer
        .note_external_passes(encoder, 1);
    let layer = 4;
    merge_passes::draw_native_object_pass(
        encoder,
        super::merge_passes::ObjectDrawPhase::Display,
        view,
        &state.renderer.depth_view,
        &mut state.renderer.terrain_draw_renderer,
        [tac_x, tac_y, tac_w, tac_h],
        &state.renderer.batch_renderer,
        pool.get_page("object_layer", layer),
        &data.object_layers[layer],
        state.match_state.match_presentation.overlay_atlas.as_ref(),
        state.match_state.match_presentation.unit_atlas.as_ref(),
        Some(&pose_cache),
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
        state
            .match_state
            .match_presentation
            .selection_overlay
            .as_ref()
            .zip(
                pool.get_page("object_extras", layer)
                    .map(|(buffer, _)| buffer),
            )
            .map(|(overlay, buffer)| (overlay, buffer, Some(state.renderer.bit_font.atlas()))),
        Some(&state.renderer.combat_light_renderer),
    );

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
    // Numbered foot route. The stroke wants the solid white texture: on a
    // one-pixel-wide line the iso diamond's transparent corners leave a dotted
    // trail. Nodes mark cells, so the diamond stays right for them. The step
    // indices are GAME.FNT glyphs — a third texture, hence a third batch.
    let white_tex = state
        .match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|o| o.white_texture());
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        white_tex,
        "debug_foot_stroke",
    );
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        debug_diamond_tex,
        "debug_foot_nodes",
    );
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        Some(state.renderer.bit_font.atlas()),
        "debug_foot_text",
    );
    draw_pooled_no_depth(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        debug_diamond_tex,
        "debug_heightmap",
    );

    // A is consumed by each world blitter, before storing its source color.
    // Native surface stores and destination shadows must not be multiplied
    // again after object drawing (493DF0/4990E0 versus4C0750/497390).

    // Native6D8DB0 completes its visible-Techno DrawExtras sweep before
    // returning to Tactical. Unit6F5190 submits rank, health, brackets and
    // pips here, before effects6D4664, BandBox6D46BD and action6D4750.
    // Evidence: tools/procedural_drawing_oracle/action_lines instruction packet.
    // Native6D9789: the later Extras sweep uses the same Display order,
    // completed body destination and live Z;4BFD30 never writes that Z.
    drop(pass);
    for (layer, objects) in data.object_layers.iter().enumerate() {
        super::object_extras::draw_object_extras(
            encoder,
            view,
            &state.renderer.depth_view,
            &mut state.renderer.terrain_draw_renderer,
            [tac_x, tac_y, tac_w, tac_h],
            &state.renderer.batch_renderer,
            state
                .match_state
                .match_presentation
                .selection_overlay
                .as_ref(),
            pool.get_page("object_extras", layer)
                .map(|(buffer, _)| buffer),
            Some(state.renderer.bit_font.atlas()),
            objects,
        );
    }
    // Original6D4664 -> Spotlight5FFFA0 edits the destination after the first
    // rally pass. Preserve this overlap, including the rally's black shadow.
    for record in data.persistent_lights.clone() {
        if state.renderer.combat_light_renderer.draw_record(
            encoder,
            [tac_x, tac_y, tac_w, tac_h],
            record,
        ) {
            state
                .renderer
                .terrain_draw_renderer
                .note_external_passes(encoder, 1);
        }
    }

    // Tactical LaserDraw550240, EBolt, LineTrail556D40, then selected
    // radial indicators6D467F edit the destination and
    // samples the same native Z authority as bridge/object rendering. End the
    // attachment pass while the ordered RGB565 stores execute, then load it.
    state
        .renderer
        .terrain_draw_renderer
        .draw_surface_lines(encoder, view, data.surface_effects);
    let mut pass = begin_main_load_pass(encoder, view, &state.renderer.depth_view);
    pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);

    // BandBox6DA180 is between the two rally passes (native6D46BD).
    // Drag rectangle — screen-fixed, use UI camera (zoom=1.0).
    let drag_tex = state
        .match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|o| o.drag_texture());
    draw_pooled_ui_passthrough(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        drag_tex,
        "drag",
    );
    // Tactical6D46CF redraws rally pixels whose ABuffer sample is zero.
    draw_pooled_ui_passthrough(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        bracket_tex,
        "factory_rally_second",
    );
    // Original6D4750 and6D47BF/6D47F6 interleave action/control lines.
    // The selected Techno's vt+438 (Foot4DC060) runs
    // AFTER rally pass1 at6D46CF. Its solid surface stores do not read/write Z.
    // See tools/procedural_drawing_oracle/rally-caller.instructions.json.
    draw_pooled_ui_passthrough(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        bracket_tex,
        "target_lines",
    );
    //6D4750..6D48F1: when Airstrike beams exist, the same ordered surface
    // batch contains action rectangles and the beams between object visits.
    if !data.tactical_lines.is_empty() {
        drop(pass);
        state
            .renderer
            .terrain_draw_renderer
            .draw_surface_lines(encoder, view, data.tactical_lines);
        pass = begin_main_load_pass(encoder, view, &state.renderer.depth_view);
        pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);
    }
    // Cell47EC90 flags20600 -> plain Convert/RLE4960A0: opaque stores,
    // world camera, neither A nor Z reads/writes. The shared neutral-A surface
    // binding and passthrough pipeline implement that material directly.
    if let (Some(shape), Some((buffer, count))) = (
        state
            .match_state
            .match_presentation
            .selection_overlay
            .as_ref()
            .and_then(|overlay| overlay.placement_shape()),
        pool.get("placement"),
    ) {
        state.renderer.batch_renderer.draw_with_buffer_passthrough(
            &mut pass,
            shape.texture(),
            buffer,
            count,
        );
    }

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

    // --- Step 10.6: timer lines ---
    // TacticalClass::Draw's Scenario, Super and blackout timers
    // (`0x006D4941..0x006D4B25`) follow the sparkles: black boxes, then text,
    // screen-fixed but clipped to the tactical view.
    draw_pooled_ui_passthrough(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        state
            .match_state
            .match_presentation
            .selection_overlay
            .as_ref()
            .map(|o| o.white_texture()),
        "super_timer_fill",
    );
    draw_pooled_ui_passthrough(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        Some(state.renderer.bit_font.atlas()),
        "super_timer_text",
    );

    // TacticalDraw6D4B25->6D4E20 is its last battlefield draw: centred
    // GRFXTXT frame, or white text if the shape/converter is unavailable.
    // Keep the tactical clip and draw before sidebar copy/Chrono capture.
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        state
            .renderer
            .outcome_banner_art
            .as_ref()
            .map(|art| art.texture()),
        "outcome_shape",
    );
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        Some(state.renderer.bit_font.atlas()),
        "outcome_text",
    );

    // Original4F4480's53BAE0 gate skips tactical redraw during ChronoScreen,
    // but sidebar/UI still follow. Surface stores therefore precede chrome.
    drop(pass);
    state
        .match_state
        .match_presentation
        .chrono_screen
        .draw_tactical(
            &state.renderer.gpu,
            encoder,
            state.renderer.combat_light_renderer.composition_texture(),
            [tac_x, tac_y, tac_w, tac_h],
        );
    let page_draw = crate::app::frontend::skirmish_shell_render::in_match_page_draw(state);
    let mut pass = begin_main_load_pass(encoder, view, &state.renderer.depth_view);

    // --- Screen-fixed UI: sidebar, messages, tooltips, cursor — UI camera (zoom=1.0) ---
    // Chrome owns the whole window: the sidebar column, the message list that
    // starts at the tactical origin, tooltips, and the cursor, which the native
    // engine draws over both regions. Release the tactical scissor before any of
    // it goes down.
    //
    // SidebarClass::BlitToScreen @ 0x006A70E0: the sidebar's surface, drawn
    // before this pass (`draw_sidebar_surface`), goes to the right-hand
    // column and the bar's to the strip left of it, scaled by the Sidebar
    // size. The copies scissor themselves.
    if let Some(surface) = state.renderer.sidebar_surface.as_ref() {
        surface.composite(&mut pass);
    }
    pass.set_scissor_rect(0, 0, state.render_width(), state.render_height());
    // The message rows' black boxes: native rows render through the shared
    // text engine with the row's opaque-background style, which fills a black
    // box one pixel out on every side (0x4A5EB0 @ 0x4A5F61) behind each line
    // before its glyphs. Fill first, then the text, in the same screen-fixed
    // pass group.
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        state
            .match_state
            .match_presentation
            .selection_overlay
            .as_ref()
            .map(|o| o.white_texture()),
        "message_fill",
    );
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        Some(state.renderer.bit_font.atlas()),
        "message_text",
    );
    // Both solid tooltip quads come from one tintable texel — the fill is tinted
    // pure black and the 1 px frame takes the tip's text colour, which is what
    // the retail pop-up shows. The darken strip would have blended the tip into
    // whatever it covers, so it is not used here.
    let tooltip_solid = state
        .match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|o| o.white_texture());
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        tooltip_solid,
        "tooltip_fill",
    );
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        tooltip_solid,
        "tooltip_border",
    );
    draw_pooled_ui(
        &mut pass,
        &state.renderer.batch_renderer,
        pool,
        Some(state.renderer.bit_font.atlas()),
        "tooltip_text",
    );
    // State 7's 0xCF window over the live battlefield (0x0077D840).
    if let Some(page) = page_draw.as_ref() {
        page.draw(&mut pass, state);
    }

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
/// Uses the overlay pipeline (LessEqual, writes depth) with bind group 0 set to
/// the UI camera, so messages, tooltips, and cursor stay at fixed screen
/// positions regardless of zoom.
fn draw_pooled_ui<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    tex: Option<&'a BatchTexture>,
    key: &'static str,
) {
    draw_pooled_with_camera(pass, batch, batch.ui_camera_bind_group(), pool, tex, key);
}

/// The overlay pipeline with `camera` as bind group 0: the UI camera, or the
/// sidebar camera for the sidebar's surface.
fn draw_pooled_with_camera(
    pass: &mut wgpu::RenderPass<'_>,
    batch: &BatchRenderer,
    camera: &wgpu::BindGroup,
    pool: &InstanceBufferPool,
    tex: Option<&BatchTexture>,
    key: &'static str,
) {
    if let (Some(t), Some((buf, count))) = (tex, pool.get(key)) {
        if count == 0 {
            return;
        }
        pass.set_pipeline(batch.overlay_pipeline());
        pass.set_bind_group(0, camera, &[]);
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
        batch.draw_passthrough_range(pass, &a.texture, buf, 0, count);
    }
}

fn draw_pooled_ui_passthrough<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    tex: Option<&'a BatchTexture>,
    key: &'static str,
) {
    if let (Some(t), Some((buf, count))) = (tex, pool.get(key)) {
        batch.draw_with_buffer_ui_passthrough(pass, t, buf, count);
    }
}

/// Adjacent runs of equal draws preserve the sweep exactly; grouping every
/// wall or every bridge together would change color/depth ties against
/// nearby bodies.
fn cell_body_runs(draws: &[CellBodyDraw]) -> Vec<(u32, u32, CellBodyDraw)> {
    let mut runs = Vec::new();
    let mut start = 0;
    while start < draws.len() {
        let draw = draws[start];
        let mut end = start + 1;
        while end < draws.len() && draws[end] == draw {
            end += 1;
        }
        runs.push((start as u32, (end - start) as u32, draw));
        start = end;
    }
    runs
}

/// Draw the cell body sweep in its uploaded order. The bodies share one
/// pipeline and differ only in atlas: Cell47F80B flags4E00 selects the opaque
/// SHP leaf4990E0, strict signed candidate<oldZ unlike TMP547CF0's <= policy,
/// through the native SHP row/gradient owner and its read/write pipeline.
pub(crate) fn draw_pooled_cell_bodies<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    overlay_atlas: Option<&'a OverlayAtlas>,
    bridge_atlas: Option<&'a BridgeAtlas>,
    key: &'static str,
    draws: &[CellBodyDraw],
) {
    let Some((buf, count)) = pool.get(key) else {
        return;
    };
    assert_eq!(
        count as usize,
        draws.len(),
        "cell body draws must describe the uploaded buffer"
    );
    for (start, count, draw) in cell_body_runs(draws) {
        let texture = match draw.atlas {
            CellBodyAtlas::Overlay => overlay_atlas.map(|atlas| {
                (
                    &atlas.texture,
                    atlas
                        .zshape_bind_group()
                        .unwrap_or_else(|| batch.default_zshape_bind_group()),
                )
            }),
            CellBodyAtlas::Bridge => {
                bridge_atlas.map(|atlas| (&atlas.texture, batch.default_zshape_bind_group()))
            }
        };
        let Some((texture, zshape)) = texture else {
            continue;
        };
        if draw.render_z == RenderZPolicy::None {
            batch.draw_passthrough_range(pass, texture, buf, start, count);
        } else {
            batch.draw_zsprite_range(
                pass,
                texture,
                zshape,
                buf,
                start,
                count,
                matches!(
                    draw.render_z,
                    RenderZPolicy::ReadWrite | RenderZPolicy::AlphaReadWrite
                ),
            );
        }
    }
}

/// Submit the bridge shadow cell sweep between overlay bodies and objects.
pub(crate) fn draw_bridge_shadows(
    encoder: &mut wgpu::CommandEncoder,
    color: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    terrain: &mut crate::render::terrain_draw::TerrainDrawRenderer,
    tactical: [u32; 4],
    batch: &BatchRenderer,
    buffer: Option<(&wgpu::Buffer, u32)>,
    instances: &[SpriteInstance],
    texture: Option<&BatchTexture>,
) -> crate::render::terrain_draw::TerrainBatchStats {
    let (Some((buffer, count)), Some(texture)) = (buffer, texture) else {
        return Default::default();
    };
    assert_eq!(count as usize, instances.len());
    terrain.draw_span(
        encoder,
        color,
        depth,
        batch,
        |_| Some(texture),
        buffer,
        instances,
        (0..count).map(
            |index| crate::render::terrain_draw::DestinationEditCommand {
                index,
                piece: crate::render::terrain_draw::TerrainPiece::Shadow,
                render_z: RenderZPolicy::ReadWrite,
                atlas_slot: 0,
            },
        ),
        tactical,
    )
}

#[cfg(test)]
mod tests {
    use super::{CellBodyAtlas, CellBodyDraw, RenderZPolicy, cell_body_runs};
    const SOURCE: &str = include_str!("draw_passes.rs");

    #[test]
    fn cell_body_runs_keep_the_sweep_across_atlases_and_policies() {
        use RenderZPolicy::{None, ReadWrite};
        let draw = |atlas, render_z| CellBodyDraw { atlas, render_z };
        let wall = draw(CellBodyAtlas::Overlay, ReadWrite);
        let ore = draw(CellBodyAtlas::Overlay, None);
        let bridge = draw(CellBodyAtlas::Bridge, ReadWrite);
        assert_eq!(
            cell_body_runs(&[ore, wall, wall, bridge, wall]),
            [(0, 1, ore), (1, 2, wall), (3, 1, bridge), (4, 1, wall)]
        );
        assert!(cell_body_runs(&[]).is_empty());
    }

    fn source_offset(needle: &str) -> usize {
        SOURCE
            .split("#[cfg(test)]")
            .next()
            .expect("production source before tests")
            .find(needle)
            .unwrap_or_else(|| panic!("missing production draw anchor {needle:?}"))
    }

    #[test]
    fn gsi_13_01_pixel_fx_and_timers_precede_native_outcome_and_chrono_capture() {
        let first_rally = source_offset("\"factory_rally_first\"");
        let ion_blasts = source_offset(".draw_ion_blasts(encoder, view)");
        let building_bases = source_offset("ObjectDrawPhase::BuildingBase");
        let objects = source_offset("for layer in 0..=3 {");
        let combat_lights = source_offset("for record in data.persistent_lights.clone()");
        let second_rally = source_offset("\"factory_rally_second\"");
        let target_lines = source_offset("\"target_lines\"");
        let tactical_lines =
            source_offset(".draw_surface_lines(encoder, view, data.tactical_lines)");
        let status = source_offset("super::object_extras::draw_object_extras(");
        let placement = source_offset("pool.get(\"placement\")");
        let sparkle = source_offset("pool.get(\"cell_sparkles\")");
        let timer_fill = source_offset("\"super_timer_fill\"");
        let timer_text = source_offset("\"super_timer_text\"");
        let outcome_start = source_offset("// TacticalDraw6D4B25->6D4E20");
        let outcome_shape = source_offset("\"outcome_shape\"");
        let outcome_text = source_offset("\"outcome_text\"");
        let chrono_capture = source_offset(".draw_tactical(");
        let screen_fixed = source_offset("// --- Screen-fixed UI:");
        let sidebar_blit = source_offset("surface.composite(&mut pass)");
        let full_window_scissor = source_offset(
            "pass.set_scissor_rect(0, 0, state.render_width(), state.render_height());",
        );
        let first_screen_submission = source_offset("\"message_text\"");

        // Original6D4648,6D4656,6D465F: IonBlast DrawAll between the first
        // rally pass and the object loop.
        assert!(building_bases < first_rally);
        assert!(first_rally < ion_blasts);
        assert!(ion_blasts < objects);
        // Original6D4648,6D4664,6D46CF,6D4750. A is now consumed by
        // each world blitter, so there is no final framebuffer multiply.
        assert!(first_rally < combat_lights);
        assert!(combat_lights < second_rally);
        assert!(second_rally < target_lines);
        assert!(target_lines < tactical_lines);
        assert!(tactical_lines < placement);
        // Unit vt110 DrawExtras completes inside6D8DB0, before6D4664.
        assert!(status < combat_lights);
        assert!(status < placement);
        assert!(placement < sparkle);
        // 6D492B, then timer lines6D4941..6D4B25 and tactical text6D4E20.
        // The result owner supplies either GRFXTXT or its CSF fallback.
        assert!(sparkle < timer_fill);
        assert!(timer_fill < timer_text);
        assert!(timer_text < outcome_start);
        assert!(outcome_start < outcome_shape);
        assert!(outcome_shape < outcome_text);
        assert!(outcome_text < chrono_capture);
        assert!(chrono_capture < screen_fixed);
        // The sidebar blits scissor to the column and strip; the rest of the
        // chrome owns the whole frame.
        assert!(screen_fixed < sidebar_blit);
        assert!(sidebar_blit < full_window_scissor);
        assert!(full_window_scissor < first_screen_submission);

        let timer_tail = &SOURCE[sparkle..outcome_start];
        assert_eq!(
            timer_tail.matches(".draw").count() + timer_tail.matches("draw_pooled").count(),
            3,
            "PixelFX and the two timer submissions precede the native result draw"
        );
    }

    #[test]
    fn gsi_04_01_retained_sidebar_radar_subpass_ends_with_content_boundary() {
        let chrome = source_offset("\"sidebar_chrome\"");
        let cameo = source_offset("\"sidebar_cameo\"");
        let gclock = source_offset("\"sidebar_gclock\"");
        let unavailable = source_offset("\"sidebar_unavailable\"");
        let flash = source_offset("\"sidebar_flash\"");
        let queue_fill = source_offset("\"sidebar_queue_overlay\"");
        let queue_text = source_offset("\"sidebar_queue_text\"");
        let cameo_overlay = source_offset("\"sidebar_cameo_overlay\"");
        let sidebar_text = source_offset("\"sidebar_text\"");
        let radar_anim = source_offset("\"radar_anim\"");
        let minimap = source_offset("\"minimap\"");
        let viewport = source_offset("\"viewport_rect\"");
        let boundary = source_offset("\"radar_content_boundary\"");
        let message = source_offset("\"message_text\"");

        assert!(chrome < cameo);
        assert!(cameo < unavailable);
        assert!(unavailable < flash);
        assert!(flash < queue_fill);
        assert!(queue_fill < queue_text);
        assert!(queue_text < gclock);
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
            "\"sidebar_chrome\"",
            "\"sidebar_cameo\"",
            "\"sidebar_gclock\"",
            "\"sidebar_unavailable\"",
            "\"sidebar_flash\"",
            "\"sidebar_queue_overlay\"",
            "\"sidebar_queue_text\"",
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

    /// SidebarClass::BlitToScreen (`6A70E0`) copies a finished
    /// g_SidebarSurface: VERA draws the whole surface before the frame pass
    /// begins, and copies it in after the tactical writes, before messages.
    #[test]
    fn gsi_04_01_sidebar_surface_is_finished_before_its_blit() {
        let surface_pass = source_offset("draw_sidebar_surface(state, encoder, data);");
        let frame_pass = source_offset("let mut pass = begin_main_pass(");
        let timer_text = source_offset("\"super_timer_text\"");
        let blit = source_offset("surface.composite(&mut pass)");
        let message = source_offset("\"message_text\"");

        assert!(surface_pass < frame_pass);
        assert!(timer_text < blit);
        assert!(blit < message);

        // No sidebar or bar layer draws in the frame pass, so the frame keeps
        // no sidebar depth: the tooltips and the cursor show over the radar.
        let production = SOURCE.split("#[cfg(test)]").next().unwrap();
        for layer in [
            "\"sidebar_chrome\"",
            "\"sidebar_cameo\"",
            "\"sidebar_gclock\"",
            "\"sidebar_cameo_overlay\"",
            "\"sidebar_text\"",
            "\"radar_anim\"",
            "\"minimap\"",
            "\"viewport_rect\"",
            "\"radar_content_boundary\"",
            "\"command_bar\"",
        ] {
            assert!(source_offset(layer) < frame_pass, "{layer}");
            assert!(!production[frame_pass..].contains(layer), "{layer}");
        }
    }

    /// gamemd's pop-up hides what it lands on, so its fill is a solid texel
    /// tinted black. Binding the sidebar darken strip instead (175/255 alpha)
    /// reads as a translucent box, which is what this pins out.
    #[test]
    fn tooltip_fill_binds_the_solid_texel_not_the_darken_strip() {
        let binding = source_offset("let tooltip_solid");
        let fill = source_offset("\"tooltip_fill\"");
        let border = source_offset("\"tooltip_border\"");
        assert!(binding < fill);
        let slice = &SOURCE[binding..border];
        assert!(
            !slice.contains("darken_texture"),
            "tooltip lanes must not draw with the translucent darken strip"
        );
        assert!(fill < border);
    }
}

#[cfg(test)]
#[path = "tmp_clear_gpu_tests.rs"]
mod tmp_clear_gpu_tests;
