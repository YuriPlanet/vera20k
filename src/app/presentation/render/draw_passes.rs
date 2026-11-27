//! Draw pass dispatch — creates the wgpu render pass and issues all draw calls in order.
//!
//! Separated from the instance-building phase in mod.rs so the orchestrator stays focused
//! on *what* to render while this module handles *how* to submit it to the GPU.
//!
//! ## Dependency rules
//! - Internal to `presentation::render` — only called from mod.rs via `dispatch_draw_passes()`.

use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::presentation::instances::{CellBodyAtlas, CellBodyDraw};
use crate::app::presentation::sidebar_render::{begin_main_load_pass, begin_main_pass};
use crate::app::presentation::sidebar_surface::{
    SIDEBAR_STREAMS, SidebarPart, SidebarPicture, SidebarRedraw, TEXT_LAYERS, VIEWPORT_OUTLINE,
};
use crate::app::presentation::state::MatchPresentationState;
use crate::app::presentation::ui_overlays::software_cursor_texture;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::render::batch::{BatchRenderer, BatchTexture, InstanceBufferPool, SpriteInstance};
use crate::render::bridge_atlas::BridgeAtlas;
use crate::render::overlay_atlas::OverlayAtlas;
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::ui::game_screen::GameScreen;

use super::merge_passes;
use super::terrain_picture::{
    GroundFrame, TERRAIN_STREAMS, TerrainDraws, TerrainInputs, TerrainStep,
};

/// One isolated pass for the Phobos placement preview artwork. The texture is
/// owned by a `RefCell` cache, so the pass must end inside this call and the
/// caller recreates its own pass afterwards.
#[cfg(feature = "phobos")]
fn draw_placement_ghost_pass(
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    clip: [u32; 4],
    batch: &BatchRenderer,
    texture: &BatchTexture,
    buffer: &wgpu::Buffer,
    count: u32,
) {
    let mut pass = begin_main_load_pass(encoder, view, depth);
    pass.set_scissor_rect(clip[0], clip[1], clip[2], clip[3]);
    batch.draw_with_buffer_passthrough(&mut pass, texture, buffer, count);
}

/// Data from the instance-building phase that the draw pass needs beyond the
/// client's owners.
///
/// These are local variables in `render_game()` that can't be accessed through the owners
/// because they're computed fresh each frame and (for the merge passes) need CPU-side
/// depth values that match the uploaded GPU buffers.
pub(super) struct DrawPassData<'a> {
    pub cell_body_draws: &'a [CellBodyDraw],
    /// The ground's instances, as uploaded to the `terrain` and `smudge`
    /// streams.
    pub terrain: &'a [SpriteInstance],
    pub smudge: &'a [SpriteInstance],
    /// Per smudge, the terrain tiles drawn before it.
    pub smudge_cuts: &'a [u32],
    pub bridge_shadows: &'a [SpriteInstance],
    pub object_layers: &'a [super::draw_plan_lowering::ObjectLayerPass; 5],
    pub fogged_objects: &'a super::draw_plan_lowering::ObjectLayerPass,
    pub building_status_text:
        &'a crate::app::presentation::ui_overlays::BuildingStatusTextInstances,
    pub surface_effects: crate::render::terrain_draw::SurfaceLineBatch,
    pub tactical_lines: crate::render::terrain_draw::SurfaceLineBatch,
    pub persistent_lights: std::ops::Range<usize>,
    pub sidebar_unavailable: &'a [SpriteInstance],
    pub sidebar_flash: &'a [SpriteInstance],
    pub sidebar_gclock: &'a [SpriteInstance],
    /// The quad that shows the minimap's composition, if any.
    pub minimap: &'a [SpriteInstance],
    /// The radar's viewport outline over it.
    pub viewport_rect: &'a [SpriteInstance],
    /// `SidebarInstances::smooth_chrome` and `smooth_command_bar`.
    pub smooth_chrome: bool,
    pub smooth_command_bar: bool,
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
    platform: &PlatformState,
    renderer: &mut RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    data: &DrawPassData<'_>,
) {
    encode_radar_pictures(renderer, &mut match_state.match_presentation, encoder);
    // The column's text draws here only where its pixels are the window's.
    let text_in_surface = matches!(
        crate::app::presentation::sidebar_surface::column_text(
            platform,
            renderer,
            &frontend.screen,
            match_state,
            persistence
        ),
        crate::app::presentation::sidebar_surface::ColumnText::Surface(_)
    );
    let side_index =
        crate::app::presentation::sidebar_render::current_sidebar_side(frontend, match_state);
    let presentation = &match_state.match_presentation;
    let chrome = presentation
        .sidebar_chrome
        .as_ref()
        .and_then(|set| set.resolve_side(side_index))
        .map(|resolved| resolved.atlas);
    // Instances moved to the smooth sidebar's chrome copy draw from it.
    let chrome_texture = |smooth: bool| {
        chrome.and_then(|atlas| {
            if smooth {
                atlas.smooth_texture()
            } else {
                Some(&atlas.texture)
            }
        })
    };
    let cameo_overlay_tex = renderer.bit_font.darken_texture().or_else(|| {
        presentation
            .selection_overlay
            .as_ref()
            .map(|o| o.white_texture())
    });
    let Some(surface) = renderer.sidebar_surface.as_mut() else {
        return;
    };
    let batch = &renderer.batch_renderer;
    let pool = &renderer.instance_pool;
    let minimap = presentation.minimap.as_ref();
    let radar_anim = presentation.radar_anim.as_ref();
    let darken = chrome.and_then(|atlas| atlas.darken.as_ref());
    let outline_on_minimap = data
        .minimap
        .first()
        .and_then(|quad| surface.column_area(quad))
        .is_some_and(|area| surface.quads_within(area, data.viewport_rect));
    let redraw = surface.redraw_for(SidebarPicture {
        streams: SIDEBAR_STREAMS
            .iter()
            .filter(|stream| text_in_surface || !TEXT_LAYERS.contains(stream))
            .map(|stream| pool.generation(stream))
            .collect(),
        textures: [
            chrome_texture(data.smooth_chrome),
            chrome_texture(data.smooth_command_bar),
            presentation
                .sidebar_cameo_atlas
                .as_ref()
                .map(|atlas| atlas.sidebar_texture()),
            presentation
                .selection_overlay
                .as_ref()
                .map(|overlay| overlay.white_texture()),
            darken.map(|mask| &mask.texture),
            cameo_overlay_tex,
            chrome.and_then(|atlas| atlas.gclock_texture.as_ref()),
            Some(renderer.bit_font.atlas()),
            radar_anim.map(|ra| ra.texture()),
            minimap.map(|m| m.map_texture()),
            minimap.map(|m| m.white_texture()),
        ]
        .iter()
        .map(|texture| texture.map(|texture| texture.bind_group.clone()))
        .collect(),
        radar: [
            minimap.map_or(0, |m| m.picture_generation()),
            radar_anim.map_or(0, |ra| ra.picture_generation()),
        ],
        outline: pool.generation(VIEWPORT_OUTLINE),
        outline_on_minimap,
        text_in_surface,
    });
    let surface = &*surface;
    let minimap_area = match redraw {
        SidebarRedraw::Nothing => return,
        SidebarRedraw::Minimap => {
            let area = data.minimap.first().and_then(|q| surface.column_area(q));
            let Some(area) = area else { return };
            Some(area)
        }
        SidebarRedraw::Whole => None,
    };
    // The strip's own layers and the bar stay clear of the minimap's quad.
    let whole = minimap_area.is_none();
    let camera = surface.camera(SidebarPart::Column);
    let text_camera = surface.text_camera();
    let mut pass = match minimap_area {
        Some(area) => surface.begin_area_pass(encoder, area),
        None => surface.begin_pass(SidebarPart::Column, encoder, super::CLEAR_COLOR),
    };
    let mut draw = |tex: Option<&BatchTexture>, key: &'static str| {
        draw_pooled_with_camera(&mut pass, batch, camera, pool, tex, key);
    };
    draw(chrome_texture(data.smooth_chrome), "sidebar_chrome");
    draw(
        presentation
            .sidebar_cameo_atlas
            .as_ref()
            .map(|atlas| atlas.sidebar_texture()),
        "sidebar_cameo",
    );
    // StripDraw6A9AF8 outlines the hovered slot before its masks.
    draw(
        presentation
            .selection_overlay
            .as_ref()
            .map(|overlay| overlay.white_texture()),
        "sidebar_cameo_hover",
    );
    drop(pass);

    // Original StripDraw: icon6A9A3E -> unavailable4016A9B46 -> flash404
    //6A9BC0. All packed operations share one owner and preserve sidebar Z.
    for (key, instances, texture, kind) in [
        (
            "sidebar_unavailable",
            data.sidebar_unavailable,
            darken.map(|mask| &mask.texture),
            crate::render::terrain_draw::PackedSpriteKind::IndexMask,
        ),
        (
            "sidebar_flash",
            data.sidebar_flash,
            darken.map(|mask| &mask.texture),
            crate::render::terrain_draw::PackedSpriteKind::UiPalette,
        ),
    ] {
        if whole
            && let (Some((buffer, count)), Some(texture)) = (pool.get(key), texture)
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

    if whole {
        // Count badge rect/fill6A9D0C/6A9D4C and glyph6A9D7B precede
        // the clock. Its404 packed destination read includes these pixels.
        let mut pass = surface.begin_load_pass(SidebarPart::Column, encoder);
        let text = |pass: &mut wgpu::RenderPass<'_>, key| {
            if text_in_surface {
                let atlas = Some(renderer.bit_font.atlas());
                draw_pooled_with_camera(pass, batch, text_camera, pool, atlas, key);
            }
        };
        draw_pooled_with_camera(
            &mut pass,
            batch,
            camera,
            pool,
            cameo_overlay_tex,
            "sidebar_queue_overlay",
        );
        text(&mut pass, "sidebar_queue_text");
        // Ready and Super labels6A9D94 also precede the clock6A9E97.
        draw_pooled_with_camera(
            &mut pass,
            batch,
            camera,
            pool,
            cameo_overlay_tex,
            "sidebar_label_overlay",
        );
        text(&mut pass, "sidebar_label_text");
    }
    if whole
        && let (Some((buffer, count)), Some(texture)) = (
            pool.get("sidebar_gclock"),
            chrome.and_then(|atlas| atlas.gclock_texture.as_ref()),
        )
        && count > 0
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
    // The production HOLD label6AA014 is the only text after the clock.
    let mut pass = surface.begin_load_pass(SidebarPart::Column, encoder);
    if let Some([x, y, width, height]) = minimap_area {
        pass.set_scissor_rect(x, y, width, height);
    }
    let mut draw = |camera, tex: Option<&BatchTexture>, key: &'static str| {
        draw_pooled_with_camera(&mut pass, batch, camera, pool, tex, key);
    };
    draw(camera, cameo_overlay_tex, "sidebar_cameo_overlay");
    if text_in_surface {
        draw(text_camera, Some(renderer.bit_font.atlas()), "sidebar_text");
    }
    draw(camera, radar_anim.map(|ra| ra.texture()), "radar_anim");
    draw(camera, minimap.map(|m| m.map_texture()), "minimap");
    draw(camera, minimap.map(|m| m.white_texture()), "viewport_rect");
    draw(
        camera,
        minimap.map(|m| m.white_texture()),
        "radar_content_boundary",
    );
    drop(pass);
    if !whole {
        return;
    }

    // The bottom command bar (6D0A20) shares the chrome art.
    let mut pass = surface.begin_pass(SidebarPart::Bar, encoder, super::CLEAR_COLOR);
    draw_pooled_with_camera(
        &mut pass,
        batch,
        surface.camera(SidebarPart::Bar),
        pool,
        chrome_texture(data.smooth_command_bar),
        "command_bar",
    );
}

/// The radar's pictures for this frame: the minimap's composition
/// (`RadarClass::Update @ 0x00656EC0`), then the housing's paints
/// (`RadarClass::Draw @ 0x00653100`), whose retained online draw samples it.
/// Every frame encodes them, sidebar drawn or not, so neither waits a frame.
fn encode_radar_pictures(
    renderer: &RendererState,
    presentation: &mut MatchPresentationState,
    encoder: &mut wgpu::CommandEncoder,
) {
    if let Some(minimap) = presentation.minimap.as_mut() {
        minimap.encode_composition(&renderer.gpu.device, encoder);
    }
    if let Some(radar) = presentation.radar_anim.as_mut() {
        radar.encode_paints(
            &renderer.gpu.device,
            &renderer.gpu.queue,
            &renderer.batch_renderer,
            encoder,
        );
    }
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
    platform: &PlatformState,
    renderer: &mut RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    data: &DrawPassData<'_>,
) {
    // The voxel pictures this frame drew on first use, and the posed voxel
    // images the world draws sample.
    if let Some(atlas) = match_state.match_presentation.unit_atlas.as_mut() {
        atlas.upload(
            &renderer.gpu.device,
            &renderer.gpu.queue,
            &renderer.batch_renderer,
        );
    }
    renderer.vxl_pose_frame_cache.borrow_mut().encode(
        &renderer.gpu.device,
        &renderer.gpu.queue,
        &renderer.batch_renderer,
        encoder,
    );
    // The sidebar's surface is complete before the frame begins; the
    // screen-fixed block copies it in.
    draw_sidebar_surface(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        encoder,
        data,
    );
    // The Surrender box paints from its nodes, kept on its layout.
    crate::app::input::surrender::sync(
        renderer,
        &frontend.screen,
        &mut match_state.match_presentation,
    );
    let pool: &InstanceBufferPool = &renderer.instance_pool;
    let pose_cache = renderer.vxl_pose_frame_cache.borrow();
    let world_depth = renderer
        .world_zoom_pass
        .as_ref()
        .map_or(&renderer.depth_view, |zoom| zoom.depth_view())
        .clone();

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
    let (x, y, w, h) = crate::app::input::camera::tactical_viewport_px(
        platform,
        renderer,
        &frontend.screen,
        &match_state.match_presentation,
        persistence,
    );
    let [tac_x, tac_y, tac_w, tac_h] = renderer
        .world_zoom_pass
        .as_ref()
        .map_or([x, y, w, h], |zoom| zoom.world_clip([x, y, w, h]));
    let ground = GroundFrame {
        terrain: data.terrain,
        smudge: data.smudge,
        a_plane: match_state.match_presentation.shroud_buffer.as_ref(),
    };
    let terrain_step = {
        let presentation = &match_state.match_presentation;
        let batch = &renderer.batch_renderer;
        let overlay_atlas = presentation.overlay_atlas.as_ref();
        let inputs = TerrainInputs {
            streams: TERRAIN_STREAMS.map(|stream| pool.generation(stream)),
            camera: batch.camera_uniform(),
            camera_group: batch.camera_bind_group().clone(),
            a_plane: presentation
                .shroud_buffer
                .as_ref()
                .map_or(0, |shroud| shroud.generation()),
            tactical: [tac_x, tac_y, tac_w, tac_h],
            fogged_sources: fogged_sources(presentation, batch, &data.fogged_objects.runs),
            atlases: [
                presentation
                    .tile_atlas
                    .as_ref()
                    .map(|atlas| atlas.zdepth_bind_group.clone()),
                overlay_atlas.map(|atlas| atlas.texture.bind_group.clone()),
                overlay_atlas.and_then(|atlas| atlas.zshape_bind_group().cloned()),
                presentation
                    .bridge_atlas
                    .as_ref()
                    .map(|atlas| atlas.texture.bind_group.clone()),
            ],
            targets: [view.texture().clone(), world_depth.texture().clone()],
        };
        let draws = TerrainDraws {
            cell_bodies: data.cell_body_draws,
            fogged_runs: &data.fogged_objects.runs,
        };
        renderer
            .terrain_picture
            .step(&renderer.gpu.device, inputs, draws, ground)
    };
    let mut pass = match terrain_step {
        TerrainStep::Restore => {
            renderer.terrain_picture.restore(encoder);
            begin_main_load_pass(encoder, view, &world_depth)
        }
        TerrainStep::Shift(shift) => {
            // The clear leaves the strips as a full draw would start them.
            drop(begin_main_pass(encoder, view, &world_depth));
            renderer
                .terrain_picture
                .shift(&renderer.gpu.queue, encoder, &world_depth, shift);
            begin_main_load_pass(encoder, view, &world_depth)
        }
        TerrainStep::Draw { .. } => begin_main_pass(encoder, view, &world_depth),
    };
    pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);

    for [x, y, width, height] in terrain_step.ground_areas([tac_x, tac_y, tac_w, tac_h]) {
        if width == 0 || height == 0 {
            continue;
        }
        pass.set_scissor_rect(x, y, width, height);
        // --- Step 1: Terrain tiles (Z-depth pipeline for per-pixel depth
        // from TMP Z-data), each cell's smudge right after its tile ---
        draw_tiles_and_smudges(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            &match_state.match_presentation,
            data.smudge_cuts,
        );
    }
    if terrain_step != TerrainStep::Restore {
        pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);
        if let TerrainStep::Draw {
            keep_ground: true, ..
        } = terrain_step
        {
            drop(pass);
            renderer
                .terrain_picture
                .keep_ground(&renderer.gpu.device, encoder, ground);
            pass = begin_main_load_pass(encoder, view, &world_depth);
            pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);
        }

        // Native base terrain 0x006D3470 calls FoggedObject::DrawAll
        // (0x004D1890) after the tiles and smudges and before the cell
        // content (0x006D3290) and the live Building sweep (0x006D9920).
        // Snapshots never enter Display layers. They read and write Z, so they
        // keep their place in the terrain picture: under the cell bodies, the
        // overlay shadows and the VERA ground marks drawn after it.
        if !data.fogged_objects.runs.is_empty() {
            drop(pass);
            renderer
                .terrain_draw_renderer
                .note_external_passes(encoder, 1);
            merge_passes::draw_native_object_pass(
                encoder,
                super::merge_passes::ObjectDrawPhase::Display,
                view,
                &world_depth,
                &mut renderer.terrain_draw_renderer,
                [tac_x, tac_y, tac_w, tac_h],
                &renderer.batch_renderer,
                pool.get("fogged_objects"),
                data.fogged_objects,
                match_state.match_presentation.overlay_atlas.as_ref(),
                match_state.match_presentation.unit_atlas.as_ref(),
                Some(&pose_cache),
                match_state.match_presentation.sprite_atlas.as_ref(),
                match_state.match_presentation.palette_set.as_ref(),
                match_state
                    .match_presentation
                    .building_zshape
                    .as_ref()
                    .map_or(renderer.batch_renderer.default_zshape_bind_group(), |z| {
                        &z.bind_group
                    }),
                None,
                None,
            );
            pass = begin_main_load_pass(encoder, view, &world_depth);
            pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);
        }

        // --- Step 2: The cell body sweep (SHP depth read + write) ---
        // High bridges, walls and ordinary overlays are 0x4E00 CC_Draw_Shape
        // draws at 0x0047F6A0, reached through one 0x006D6D10 sweep. They are not
        // the TMP tile path at 0x00480350. Their class gradient, stored frame
        // rectangle and height adjustment keep them above the ground and let them
        // occlude later buildings/units. The native cell-content layer runs this
        // whole sweep before its overlay shadow sweep, below.
        draw_pooled_cell_bodies(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            match_state.match_presentation.overlay_atlas.as_ref(),
            match_state.match_presentation.bridge_atlas.as_ref(),
            "cell_body",
            data.cell_body_draws,
        );
    }
    if let TerrainStep::Draw {
        keep_whole: true, ..
    } = terrain_step
    {
        drop(pass);
        renderer.terrain_picture.keep(&renderer.gpu.device, encoder);
        pass = begin_main_load_pass(encoder, view, &world_depth);
        pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);
    }

    let bracket_tex = match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|o| o.white_texture());
    // VERA ground marks (the formation line's dots, the ferry's pick-up and
    // drop circles) lie on the ground. They follow the kept terrain picture,
    // so are never stored in it, and precede the building bases and every
    // object, which cover them. Remembered buildings are in that picture, so
    // the marks cover them. Surface stores: no Z read or write.
    draw_pooled_surface_lines(
        &mut pass,
        &renderer.batch_renderer,
        pool,
        bracket_tex,
        "ground_marks",
    );
    draw_pooled_surface_strokes(&mut pass, &renderer.batch_renderer, pool, "ground_strokes");

    // Cell::DrawOverlay_Shadow0047F510 uses native destination halving and
    // read/write Z after all overlay bodies. End the ordinary pass before the
    // shared renderer snapshots the same live color/depth attachments. Its
    // signed comparison must precede the low16 Z store; hardware Less alone
    // cannot implement negative native candidates. Original caller/shape
    // vectors: tools/spatial_oracle/bridge_shadow_render.md.
    if !data.bridge_shadows.is_empty() {
        drop(pass);
        renderer
            .terrain_draw_renderer
            .note_external_passes(encoder, 1);
        draw_bridge_shadows(
            encoder,
            view,
            &world_depth,
            &mut renderer.terrain_draw_renderer,
            [tac_x, tac_y, tac_w, tac_h],
            &renderer.batch_renderer,
            pool.get("overlay_bridge_body_shadow"),
            data.bridge_shadows,
            match_state
                .match_presentation
                .bridge_atlas
                .as_ref()
                .map(|atlas| &atlas.texture),
        );
        pass = begin_main_load_pass(encoder, view, &world_depth);
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

    // Native6D3AC0 -> Ground6D97D0 draws static trees backward, after the
    // cell content6D3290 and before the tile shadows6D3040 (dormant on
    // retail, above) and the building bases.
    // Native6D9920 ->43CEA0/43D290 draws Ground's building bases into the
    // background before rally6D4648. These ranges draw only here; Display's
    // building branch6D916C requests the dynamic turret43DA80 instead. Anim
    // slots and voxel turrets keep their independent later Display positions.
    for (phase, runs) in [
        (
            merge_passes::ObjectDrawPhase::StaticTree,
            &data.object_layers[2].static_tree_runs,
        ),
        (
            merge_passes::ObjectDrawPhase::BuildingBase,
            &data.object_layers[2].building_base_runs,
        ),
    ] {
        if runs.is_empty() {
            continue;
        }
        drop(pass);
        renderer
            .terrain_draw_renderer
            .note_external_passes(encoder, 1);
        merge_passes::draw_native_object_pass(
            encoder,
            phase,
            view,
            &world_depth,
            &mut renderer.terrain_draw_renderer,
            [tac_x, tac_y, tac_w, tac_h],
            &renderer.batch_renderer,
            pool.get_page("object_layer", 2),
            &data.object_layers[2],
            match_state.match_presentation.overlay_atlas.as_ref(),
            match_state.match_presentation.unit_atlas.as_ref(),
            Some(&pose_cache),
            match_state.match_presentation.sprite_atlas.as_ref(),
            match_state.match_presentation.palette_set.as_ref(),
            match_state
                .match_presentation
                .building_zshape
                .as_ref()
                .map_or(renderer.batch_renderer.default_zshape_bind_group(), |z| {
                    &z.bind_group
                }),
            None,
            None,
        );
        pass = begin_main_load_pass(encoder, view, &world_depth);
        pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);
    }

    // Original6D4648 draws the first rally pass BEFORE the object loop at
    // 6D465F. Building bases have already primed Z; mobile bodies and Anim
    // slots can cover the line. The second pass follows effects and BandBox.
    // Both rally leaves bypass Z.
    draw_pooled_surface_strokes(
        &mut pass,
        &renderer.batch_renderer,
        pool,
        "factory_rally_first",
    );
    // Original6D4656 IonBlast DrawAll moves terrain pixels after the first
    // rally pass and before the object loop at 6D465F (render::terrain_draw).
    // Native draws the placement cells (6D5030) at 0x006D4651, before it, and
    // again at 0x006D46D8 after the objects; VERA draws only the second, which
    // covers the first, so a ripple does not shift them.
    if renderer.terrain_draw_renderer.has_ion_blasts() {
        drop(pass);
        renderer
            .terrain_draw_renderer
            .note_external_passes(encoder, 1);
        renderer
            .terrain_draw_renderer
            .draw_ion_blasts(encoder, view);
        pass = begin_main_load_pass(encoder, view, &world_depth);
        pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);
    }

    // Bridge units share the native Ground parent pass below. Their body
    // split changes per-pixel Z, never their position among buildings.

    // --- Step 5: Retained Display layers0..3 ---
    // Ground keeps the simulation's partial signed-key sort; other layers
    // retain submission order. Texture bindings never reorder parent slots.
    drop(pass);
    renderer
        .terrain_draw_renderer
        .note_external_passes(encoder, 1);
    for layer in 0..=3 {
        merge_passes::draw_native_object_pass(
            encoder,
            super::merge_passes::ObjectDrawPhase::Display,
            view,
            &world_depth,
            &mut renderer.terrain_draw_renderer,
            [tac_x, tac_y, tac_w, tac_h],
            &renderer.batch_renderer,
            pool.get_page("object_layer", layer),
            &data.object_layers[layer],
            match_state.match_presentation.overlay_atlas.as_ref(),
            match_state.match_presentation.unit_atlas.as_ref(),
            Some(&pose_cache),
            match_state.match_presentation.sprite_atlas.as_ref(),
            match_state.match_presentation.palette_set.as_ref(),
            match_state
                .match_presentation
                .building_zshape
                .as_ref()
                .map_or(renderer.batch_renderer.default_zshape_bind_group(), |z| {
                    &z.bind_group
                }),
            match_state
                .match_presentation
                .selection_overlay
                .as_ref()
                .zip(
                    pool.get_page("object_extras", layer)
                        .map(|(buffer, _)| buffer),
                )
                .map(|(overlay, buffer)| (overlay, buffer, Some(renderer.bit_font.atlas()))),
            Some(&renderer.combat_light_renderer),
        );
        if layer == 2 && renderer.world_zoom_pass.is_none() {
            // Tactical6D9427 admits this BuildingClass::Array sweep only
            // after Ground2, before Air3/Top4 and the late Extras sweep.
            super::object_extras::draw_building_status_text(
                encoder,
                view,
                &world_depth,
                &mut renderer.terrain_draw_renderer,
                [tac_x, tac_y, tac_w, tac_h],
                &renderer.batch_renderer,
                match_state.match_presentation.selection_overlay.as_ref(),
                pool.get("building_status_text").map(|(buffer, _)| buffer),
                Some(renderer.bit_font.atlas()),
                &data.building_status_text.runs,
                renderer.batch_renderer.surface_camera_bind_group(),
            );
        }
    }

    // (There is no separate building-turret pass. gamemd draws a building's
    // voxel turret inside the building's retained Display slot, after the
    // body. Atlas page changes preserve that parent and its piece order.)

    // The distinct retained Top4 follows the Layer3 families.
    let layer = 4;
    merge_passes::draw_native_object_pass(
        encoder,
        super::merge_passes::ObjectDrawPhase::Display,
        view,
        &world_depth,
        &mut renderer.terrain_draw_renderer,
        [tac_x, tac_y, tac_w, tac_h],
        &renderer.batch_renderer,
        pool.get_page("object_layer", layer),
        &data.object_layers[layer],
        match_state.match_presentation.overlay_atlas.as_ref(),
        match_state.match_presentation.unit_atlas.as_ref(),
        Some(&pose_cache),
        match_state.match_presentation.sprite_atlas.as_ref(),
        match_state.match_presentation.palette_set.as_ref(),
        match_state
            .match_presentation
            .building_zshape
            .as_ref()
            .map_or(renderer.batch_renderer.default_zshape_bind_group(), |z| {
                &z.bind_group
            }),
        match_state
            .match_presentation
            .selection_overlay
            .as_ref()
            .zip(
                pool.get_page("object_extras", layer)
                    .map(|(buffer, _)| buffer),
            )
            .map(|(overlay, buffer)| (overlay, buffer, Some(renderer.bit_font.atlas()))),
        Some(&renderer.combat_light_renderer),
    );

    // --- Step 8: Debug overlays ---
    // Drawn above entities, below fog and UI, in a pass that opens only while
    // one of them is on.
    const DEBUG_OVERLAYS: [&str; 7] = [
        "debug_pathgrid",
        "debug_cell_grid",
        "debug_path",
        "debug_foot_stroke",
        "debug_foot_nodes",
        "debug_foot_text",
        "debug_heightmap",
    ];
    if DEBUG_OVERLAYS.iter().any(|&key| pool.get(key).is_some()) {
        let mut pass = begin_main_load_pass(encoder, view, &world_depth);
        pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);
        // Use filled-diamond texture so cells appear as isometric diamonds, not rectangles.
        let debug_diamond_tex = match_state
            .match_presentation
            .selection_overlay
            .as_ref()
            .map(|o| o.diamond_filled_texture());
        draw_pooled_no_depth(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            debug_diamond_tex,
            "debug_pathgrid",
        );
        let grid_tex = match_state
            .match_presentation
            .selection_overlay
            .as_ref()
            .map(|o| o.diamond_outline_texture());
        draw_pooled_no_depth(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            grid_tex,
            "debug_cell_grid",
        );
        draw_pooled_no_depth(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            debug_diamond_tex,
            "debug_path",
        );
        // Numbered foot route. The stroke wants the solid white texture: on a
        // one-pixel-wide line the iso diamond's transparent corners leave a dotted
        // trail. Nodes mark cells, so the diamond stays right for them. The step
        // indices are GAME.FNT glyphs — a third texture, hence a third batch.
        let white_tex = match_state
            .match_presentation
            .selection_overlay
            .as_ref()
            .map(|o| o.white_texture());
        draw_pooled_no_depth(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            white_tex,
            "debug_foot_stroke",
        );
        draw_pooled_no_depth(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            debug_diamond_tex,
            "debug_foot_nodes",
        );
        draw_pooled_no_depth(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            Some(renderer.bit_font.atlas()),
            "debug_foot_text",
        );
        draw_pooled_no_depth(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            debug_diamond_tex,
            "debug_heightmap",
        );
    }

    // A is consumed by each world blitter, before storing its source color.
    // Native surface stores and destination shadows must not be multiplied
    // again after object drawing (493DF0/4990E0 versus4C0750/497390).

    // Native6D8DB0 completes its visible-Techno DrawExtras sweep before
    // returning to Tactical. Unit6F5190 submits rank, health, brackets and
    // pips here, before effects6D4664, BandBox6D46BD and action6D4750.
    // Evidence: tools/procedural_drawing_oracle/action_lines instruction packet.
    // Native6D9789: the later Extras sweep uses the same Display order,
    // completed body destination and live Z;4BFD30 never writes that Z.
    for (layer, objects) in data.object_layers.iter().enumerate() {
        super::object_extras::draw_object_extras(
            encoder,
            view,
            &world_depth,
            &mut renderer.terrain_draw_renderer,
            [tac_x, tac_y, tac_w, tac_h],
            &renderer.batch_renderer,
            match_state.match_presentation.selection_overlay.as_ref(),
            pool.get_page("object_extras", layer)
                .map(|(buffer, _)| buffer),
            Some(renderer.bit_font.atlas()),
            objects,
        );
    }
    // Original6D4664 -> Spotlight5FFFA0 edits the destination after the first
    // rally pass. Preserve this overlap, including the rally's black shadow.
    // The lights edit in persistent-vector order, many to a pass.
    let records: Vec<_> = data.persistent_lights.clone().collect();
    let passes = renderer.combat_light_renderer.draw_records(
        encoder,
        [tac_x, tac_y, tac_w, tac_h],
        &records,
    );
    renderer
        .terrain_draw_renderer
        .note_external_passes(encoder, passes);

    // Tactical LaserDraw550240, EBolt, LineTrail556D40, then selected
    // radial indicators6D467F edit the destination and
    // samples the same native Z authority as bridge/object rendering. End the
    // attachment pass while the ordered RGB565 stores execute, then load it.
    renderer
        .terrain_draw_renderer
        .draw_surface_lines(encoder, view, &[data.surface_effects]);
    let mut pass = begin_main_load_pass(encoder, view, &world_depth);
    pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);

    // Ares Tactical_Draw_FlyingStrings hooks 6D4684, right after the radial
    // indicators 6D467F and before BeaconManager 6D46B6, and draws the copied
    // signed credit labels (->10064430) with the shared GAME.FNT passthrough.
    if crate::app::state::active_rules(frontend, match_state).is_some_and(|rules| rules.ares()) {
        draw_pooled_ui_passthrough(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            Some(renderer.bit_font.atlas()),
            "bounty_text",
        );
    }

    // BeaconManager Draw 430AC0 (6D46B6) precedes BandBox. Its flags 600
    // bypass A and Z; each House row has its own Convert strip.
    if let Some(art) = match_state.match_presentation.beacon_art.as_ref() {
        for house in 0..art.house_count() {
            if let (Some(strip), Some((buffer, count))) =
                (art.tactical_strip(house), pool.get_page("beacons", house))
            {
                renderer.batch_renderer.draw_with_buffer_passthrough(
                    &mut pass,
                    strip.texture(),
                    buffer,
                    count,
                );
            }
        }
    }
    // BeaconClass::Draw 430250's label boxes (`beacon_labels`): black fill,
    // House-colour outline, then GAME.FNT text.
    let beacon_label_solid = match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|overlay| overlay.white_texture());
    for (texture, key) in [
        (beacon_label_solid, "beacon_label_fill"),
        (beacon_label_solid, "beacon_label_outline"),
        (Some(renderer.bit_font.atlas()), "beacon_label_text"),
    ] {
        draw_pooled_ui_passthrough(&mut pass, &renderer.batch_renderer, pool, texture, key);
    }
    // BandBox6DA180 is between the two rally passes (native6D46BD).
    // Wheel zoom defers its screen-pixel border until after resampling;
    // native scale retains the original ordering between the rally passes.
    let drag_tex = match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|o| o.drag_texture());
    if renderer.world_zoom_pass.is_none() {
        draw_pooled_ui_passthrough(&mut pass, &renderer.batch_renderer, pool, drag_tex, "drag");
    }
    // Tactical6D46CF redraws rally pixels whose ABuffer sample is zero.
    draw_pooled_surface_strokes(
        &mut pass,
        &renderer.batch_renderer,
        pool,
        "factory_rally_second",
    );
    // BuildingPlacement_OverlayRenderer 0x006D5030(1) at 0x006D46D8 draws
    // the PLACE cells after the second rally pass and before PlanMgr
    // (0x006D46DD) and the line walk (0x006D4750), so lines cross over them.
    // Cell47EC90 flags20600 -> plain Convert/RLE4960A0: opaque stores,
    // world camera, neither A nor Z reads/writes. The shared neutral-A surface
    // binding and passthrough pipeline implement that material directly.
    if let (Some(shape), Some((buffer, count))) = (
        match_state
            .match_presentation
            .selection_overlay
            .as_ref()
            .and_then(|overlay| overlay.placement_shape()),
        pool.get("placement"),
    ) {
        renderer.batch_renderer.draw_with_buffer_passthrough(
            &mut pass,
            shape.texture(),
            buffer,
            count,
        );
    }
    // Phobos placement preview artwork (`0x6D528A`), after the PLACE cells.
    // The texture lives behind the cache's RefCell borrow, so end this pass
    // before releasing the guard and recreate it for the following stages.
    #[cfg(feature = "phobos")]
    if let Some((buffer, count)) = pool.get("placement_ghost")
        && count > 0
    {
        {
            let art = match_state
                .match_presentation
                .placement_preview_art
                .borrow();
            if let Some(texture) = art.current_texture() {
                drop(pass);
                draw_placement_ghost_pass(
                    encoder,
                    view,
                    &world_depth,
                    [tac_x, tac_y, tac_w, tac_h],
                    &renderer.batch_renderer,
                    texture,
                    buffer,
                    count,
                );
                pass = begin_main_load_pass(encoder, view, &world_depth);
                pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);
            }
        }
    }

    // Original6D4750 and6D47BF/6D47F6 interleave action/control lines.
    // The selected Techno's vt+438 (Foot4DC060) runs
    // AFTER rally pass1 at6D46CF. Its solid surface stores do not read/write Z.
    // See tools/procedural_drawing_oracle/rally-caller.instructions.json.
    draw_pooled_surface_strokes(&mut pass, &renderer.batch_renderer, pool, "target_lines");
    //6D4750..6D48F1: when Airstrike beams exist, the same ordered surface
    // batch contains action rectangles and the beams between object visits.
    if !data.tactical_lines.is_empty() {
        drop(pass);
        renderer
            .terrain_draw_renderer
            .draw_surface_lines(encoder, view, &[data.tactical_lines]);
        pass = begin_main_load_pass(encoder, view, &world_depth);
        pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);
    }
    // --- Step 10.5: PixelFX water/ore sparkles ---
    // gamemd writes these opaque one-pixel effects at the tactical tail, after
    // object/effect/status/action/placement drawing and before screen-fixed
    // chrome. In VERA the global shroud translation must therefore run first.
    // The passthrough pipeline bypasses depth; an empty buffer when
    // `[Options] DetailLevel=0` short-circuits at count == 0.
    if let (Some(overlay), Some((buf, count))) = (
        match_state.match_presentation.selection_overlay.as_ref(),
        pool.get("cell_sparkles"),
    ) {
        renderer.batch_renderer.draw_with_buffer_passthrough(
            &mut pass,
            overlay.white_texture(),
            buf,
            count,
        );
    }

    // Resolve wheel zoom once, after every world/depth/destination operation.
    // Timers, sidebar, messages and cursor then draw in frame pixels.
    drop(pass);
    // VERA's HD art (`render::hd_switch`): at every wheel zoom, the HD art
    // replaces the zoomed copy's terrain, overlay, building and voxel pixels
    // (resampled below 200%). It goes over the finished world only; everything
    // screen-fixed below stays sharp. Off encodes nothing.
    let (hd_art_on, hd_debug) = crate::app::state::hd_art_on(persistence);
    let (tac_x, tac_y, tac_w, tac_h) = crate::app::input::camera::tactical_viewport_px(
        platform,
        renderer,
        &frontend.screen,
        &match_state.match_presentation,
        persistence,
    );
    let view = if let Some(zoom) = renderer.world_zoom_pass.as_ref() {
        let gpu = &renderer.gpu;
        let composition = renderer.combat_light_renderer.composition_texture();
        let frame_tactical = [tac_x, tac_y, tac_w, tac_h];
        zoom.composite(
            &gpu.device,
            encoder,
            composition,
            frame_tactical,
            super::CLEAR_COLOR,
        );
        if hd_art_on {
            if let Some(tiles) = match_state.match_presentation.tile_atlas.as_ref()
                && let Some(terrain) = renderer
                    .hd_terrain
                    .get_or_insert_with(crate::render::hd_terrain::HdTerrain::open)
                    .as_mut()
            {
                let camera = renderer.batch_renderer.camera_uniform();
                let clip = zoom.world_clip(frame_tactical);
                let pieces = hd_terrain_pieces(data.terrain.iter(), camera.camera_pos, clip);
                let draws =
                    terrain.prepare(&gpu.device, &gpu.queue, &|uv| tiles.hd_tile(uv), &pieces);
                terrain.encode(
                    &gpu.device,
                    &gpu.queue,
                    encoder,
                    zoom.color_texture(),
                    composition,
                    &tiles.texture.view,
                    false,
                    zoom.scale(),
                    frame_tactical,
                    &draws,
                    hd_debug,
                );
                // Ore, gems, walls, rocks and the other overlays, then trees
                // and the other terrain objects: the overlay atlas's bodies.
                if let Some(overlays) = match_state.match_presentation.overlay_atlas.as_ref() {
                    let bodies = &match_state.match_presentation.cached_cell_body_instances;
                    let overlay_bodies = bodies
                        .iter()
                        .zip(data.cell_body_draws)
                        .filter(|(_, draw)| draw.atlas == CellBodyAtlas::Overlay)
                        .map(|(instance, _)| instance);
                    let terrain_objects = data.object_layers.iter().flat_map(|layer| {
                        layer
                            .static_tree_runs
                            .iter()
                            .chain(&layer.building_base_runs)
                            .chain(&layer.runs)
                            .filter(|run| {
                                run.target
                                    == super::draw_plan_lowering::ObjectTexture::TerrainShp(
                                        crate::render::terrain_draw::TerrainPiece::Body,
                                    )
                            })
                            .flat_map(|run| {
                                let range = run.start as usize..(run.start + run.count) as usize;
                                layer.instances.get(range).into_iter().flatten()
                            })
                    });
                    // Smudges (craters, scorch marks) draw first, right after
                    // the tiles, as the native pass draws them.
                    let pieces = hd_terrain_pieces(
                        data.smudge
                            .iter()
                            .chain(overlay_bodies)
                            .chain(terrain_objects),
                        camera.camera_pos,
                        clip,
                    );
                    let draws = terrain.prepare(
                        &gpu.device,
                        &gpu.queue,
                        &|uv| overlays.hd_sprite(uv),
                        &pieces,
                    );
                    terrain.encode(
                        &gpu.device,
                        &gpu.queue,
                        encoder,
                        zoom.color_texture(),
                        composition,
                        &overlays.texture.view,
                        true,
                        zoom.scale(),
                        frame_tactical,
                        &draws,
                        hd_debug,
                    );
                }
            }
            let art = renderer
                .hd_art
                .get_or_insert_with(crate::render::hd_art::HdArt::open);
            if let (Some(art), Some(atlas)) = (
                art.as_mut(),
                match_state.match_presentation.sprite_atlas.as_ref(),
            ) {
                let camera = renderer.batch_renderer.camera_uniform();
                let pieces = hd_art_pieces(
                    data.object_layers,
                    camera.camera_pos,
                    zoom.world_clip(frame_tactical),
                );
                art.prepare(&gpu.device, &gpu.queue, atlas, &pieces);
                let draws: Vec<_> = pieces
                    .iter()
                    .filter_map(|piece| art.placement(piece))
                    .collect();
                art.encode(
                    &gpu.device,
                    &gpu.queue,
                    encoder,
                    zoom.color_texture(),
                    composition,
                    zoom.scale(),
                    frame_tactical,
                    &draws,
                    hd_debug,
                );
            }
            if let (Some(units), Some(palette_set)) = (
                match_state.match_presentation.unit_atlas.as_ref(),
                match_state.match_presentation.palette_set.as_ref(),
            ) {
                let camera = renderer.batch_renderer.camera_uniform();
                let poses = renderer.vxl_pose_frame_cache.borrow();
                let pages = hd_voxel_pages(
                    units,
                    &poses,
                    data.object_layers,
                    camera.camera_pos,
                    zoom.world_clip(frame_tactical),
                );
                renderer
                    .hd_voxel
                    .get_or_insert_with(|| crate::render::hd_voxel::HdVoxelPass::new(&gpu.device))
                    .encode(
                        &gpu.device,
                        &gpu.queue,
                        encoder,
                        zoom.color_texture(),
                        composition,
                        &palette_set.palette_view,
                        &palette_set.house_ramp_view,
                        zoom.scale(),
                        frame_tactical,
                        &pages,
                        hd_debug,
                    );
            }
        }
        draw_zoomed_drag_rect(
            zoom,
            encoder,
            frame_tactical,
            &renderer.batch_renderer,
            pool,
            drag_tex,
        );
        zoom.color_view()
    } else {
        view
    };
    let frame_depth = renderer
        .world_zoom_pass
        .as_ref()
        .map_or(&renderer.depth_view, |zoom| zoom.frame_depth_view());
    if renderer.world_zoom_pass.is_some() {
        // Zoomed labels keep GAME.FNT and their backing in frame pixels.
        // Their producer projects the building anchor, then measures/clips
        // the text at native font size. Native zoom retains the Ground2 pass.
        super::object_extras::draw_building_status_text(
            encoder,
            view,
            frame_depth,
            &mut renderer.terrain_draw_renderer,
            [tac_x, tac_y, tac_w, tac_h],
            &renderer.batch_renderer,
            match_state.match_presentation.selection_overlay.as_ref(),
            pool.get("building_status_text").map(|(buffer, _)| buffer),
            Some(renderer.bit_font.atlas()),
            &data.building_status_text.runs,
            renderer.batch_renderer.ui_camera_bind_group(),
        );
    }
    let health_cameras = data
        .object_layers
        .iter()
        .any(|layer| !layer.frame_health.is_empty())
        .then(|| {
            renderer.batch_renderer.create_frame_world_cameras(
                &renderer.gpu.device,
                match_state.input.zoom_level,
                (!match_state.sandbox_full_visibility)
                    .then_some(match_state.match_presentation.shroud_buffer.as_ref())
                    .flatten(),
            )
        });
    let mut pass = begin_main_load_pass(encoder, view, frame_depth);
    pass.set_scissor_rect(tac_x, tac_y, tac_w, tac_h);
    if let Some(cameras) = &health_cameras {
        for (layer, objects) in data.object_layers.iter().enumerate() {
            super::object_extras::draw_frame_health(
                &mut pass,
                &renderer.batch_renderer,
                match_state.match_presentation.selection_overlay.as_ref(),
                pool.get_page("object_extras", layer)
                    .map(|(buffer, _)| buffer),
                objects,
                [&cameras[0], &cameras[1]],
            );
        }
    }
    // A zoomed match draws the battlefield's screen-fixed text, with its
    // boxes, at the window's pixels after the zoom (`window_ui`); their
    // layers are left out of the frame.
    let text_in_frame = crate::app::presentation::window_ui::battlefield_raster(
        renderer,
        &frontend.screen,
        match_state,
    )
    .is_none();
    // --- Step 10.6: timer lines ---
    // TacticalClass::Draw's Scenario, Super and blackout timers
    // (`0x006D4941..0x006D4B25`) follow the sparkles: black boxes, then text,
    // screen-fixed but clipped to the tactical view. Ares' bottom-left lines
    // (hook `0x006D4B25`) follow them in the same lanes.
    if text_in_frame {
        draw_pooled_ui_passthrough(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            match_state
                .match_presentation
                .selection_overlay
                .as_ref()
                .map(|o| o.white_texture()),
            "super_timer_fill",
        );
        draw_pooled_ui_passthrough(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            Some(renderer.bit_font.atlas()),
            "super_timer_text",
        );
    }

    // VERA: the mod's and the map's status templates over the battlefield
    // (`ui_modules::field`), after its objects and timers, clipped to the
    // view and zoomed with it.
    if match_state.ui_modules.is_some() {
        let white = match_state
            .match_presentation
            .selection_overlay
            .as_ref()
            .map(|o| o.white_texture());
        let pictures = match_state
            .ui_modules
            .as_ref()
            .and_then(|ui| ui.picture_texture());
        for (texture, key) in [
            (white, "module_field_fill"),
            (pictures, "module_field_art"),
            (Some(renderer.bit_font.atlas()), "module_field_text"),
        ] {
            draw_pooled_ui_passthrough(&mut pass, &renderer.batch_renderer, pool, texture, key);
        }
    }

    // TacticalDraw6D4B25->6D4E20 is its last battlefield draw: centred
    // GRFXTXT frame, or white text if the shape/converter is unavailable.
    // Keep the tactical clip and draw before sidebar copy/Chrono capture.
    // The atlas comes from the same selector the instances did: a country
    // card drawn through the retail `GRFXTXT` texture samples the wrong
    // picture at the right coordinates.
    draw_pooled_ui(
        &mut pass,
        &renderer.batch_renderer,
        pool,
        crate::app::presentation::outcome_banner::texture(
            &renderer.outcome_banner_art,
            match_state,
            &persistence.options_profile,
        ),
        "outcome_shape",
    );
    if text_in_frame {
        draw_pooled_ui(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            Some(renderer.bit_font.atlas()),
            "outcome_text",
        );
    }
    // VERA: the pictures of the buttons a layout adds over the battlefield,
    // from the sidebar's picture atlas, after the tactical view's own last
    // draw: screen-fixed, clipped to the view, and zoomed with the frame.
    draw_pooled_ui_passthrough(
        &mut pass,
        &renderer.batch_renderer,
        pool,
        crate::app::presentation::sidebar_render::current_sidebar_chrome(frontend, match_state)
            .map(|atlas| &atlas.texture),
        "layout_field_art",
    );

    // Original4F4480's53BAE0 gate skips tactical redraw during ChronoScreen,
    // but sidebar/UI still follow. Surface stores therefore precede chrome.
    drop(pass);
    let bar_under = crate::app::state::sidebar_frame(
        platform,
        persistence,
        renderer,
        &frontend.screen,
        &match_state.match_presentation.sidebar_layout_spec,
    )
    .places()
    .bar == Some(crate::ui::sidebar::Edge::Bottom);
    match_state.match_presentation.chrono_screen.draw_tactical(
        &renderer.gpu,
        encoder,
        renderer.game_frame_texture(),
        [tac_x, tac_y, tac_w, tac_h],
        bar_under,
    );
    let (page_draw, page_texts) = crate::app::frontend::skirmish_shell_render::in_match_page_draw(
        &platform.ime,
        renderer,
        frontend,
        match_state,
        process_assets,
    );
    renderer.window_ui.set_page_texts(page_texts);
    let mut pass = begin_main_load_pass(encoder, view, frame_depth);

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
    if let Some(surface) = renderer.sidebar_surface.as_ref() {
        surface.composite(&mut pass);
    }
    // The column's text, where its copy would resample it, over the copy
    // within the column (`sidebar_surface::column_text`).
    if let crate::app::presentation::sidebar_surface::ColumnText::Frame(_) =
        crate::app::presentation::sidebar_surface::column_text(
            platform,
            renderer,
            &frontend.screen,
            match_state,
            persistence,
        )
    {
        let (x, y, width, height) = crate::app::state::sidebar_frame(
            platform,
            persistence,
            renderer,
            &frontend.screen,
            &match_state.match_presentation.sidebar_layout_spec,
        )
        .column();
        pass.set_scissor_rect(x, y, width, height);
        for key in crate::app::presentation::sidebar_surface::TEXT_LAYERS {
            draw_pooled_ui_passthrough(
                &mut pass,
                &renderer.batch_renderer,
                pool,
                Some(renderer.bit_font.atlas()),
                key,
            );
        }
    }
    pass.set_scissor_rect(
        0,
        0,
        renderer.render_width(&frontend.screen),
        renderer.render_height(&frontend.screen),
    );
    // Phobos SuperWeaponSidebar: left-edge screen-fixed layers. Drawn after
    // the right sidebar blit and before messages, like Phobos' own gadgets.
    #[cfg(feature = "phobos")]
    {
        let presentation = &match_state.match_presentation;
        let chrome =
            crate::app::presentation::sidebar_render::current_sidebar_chrome(frontend, match_state);
        draw_pooled_ui(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            presentation.sw_sidebar_art.as_ref().map(|art| &art.texture),
            "sw_sidebar_chrome",
        );
        draw_pooled_ui(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            presentation
                .sidebar_cameo_atlas
                .as_ref()
                .map(|atlas| &atlas.texture),
            "sw_sidebar_cameo",
        );
        draw_pooled_ui(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            presentation
                .selection_overlay
                .as_ref()
                .map(|overlay| overlay.white_texture()),
            "sw_sidebar_hover",
        );
        draw_pooled_ui(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            chrome.and_then(|atlas| atlas.gclock_texture.as_ref()),
            "sw_sidebar_gclock",
        );
        draw_pooled_ui(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            renderer.bit_font.darken_texture(),
            "sw_sidebar_overlay",
        );
        draw_pooled_ui(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            Some(renderer.bit_font.atlas()),
            "sw_sidebar_text",
        );
    }
    // The message rows' black boxes: native rows render through the shared
    // text engine with the row's opaque-background style, which fills a black
    // box one pixel out on every side (0x4A5EB0 @ 0x4A5F61) behind each line
    // before its glyphs. Fill first, then the text, in the same screen-fixed
    // pass group; then the tooltip.
    //
    // The boxes and both solid tooltip quads come from one tintable texel —
    // the tooltip's fill is tinted pure black and its 1 px frame takes the
    // tip's text colour, which is what the retail pop-up shows. The darken
    // strip would have blended the tip into whatever it covers, so it is not
    // used here.
    let solid = match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|o| o.white_texture());
    // VERA: the client modules' panels (`ui.wasm`, RFC 0001 section 10)
    // over the battlefield and the sidebar, where the pointer reaches them
    // before the chrome (`ui_modules::tree`), under the message rows.
    crate::app::ui_modules::paint::draw(
        &mut pass,
        renderer,
        match_state,
        pool,
        crate::app::ui_modules::paint::ModuleLayer::Panels,
    );
    let font = Some(renderer.bit_font.atlas());
    if text_in_frame {
        for (texture, key) in [(solid, "message_fill"), (font, "message_text")] {
            draw_pooled_ui(
                &mut pass,
                &renderer.batch_renderer,
                pool,
                texture,
                key,
            );
        }
    }
    // VERA: the map's overlay (its module's nodes under `MapUI=overlay` and
    // its `[VeraUI]` parts), above the whole match interface; the tooltip
    // and the in-match dialogs go over it.
    crate::app::ui_modules::paint::draw(
        &mut pass,
        renderer,
        match_state,
        pool,
        crate::app::ui_modules::paint::ModuleLayer::Overlay,
    );
    if text_in_frame {
        for (texture, key) in [
            (solid, "tooltip_fill"),
            (solid, "tooltip_border"),
            (font, "tooltip_text"),
        ] {
            draw_pooled_ui(&mut pass, &renderer.batch_renderer, pool, texture, key);
        }
    }
    // State 7's 0xCF window over the live battlefield (0x0077D840).
    if let Some(page) = page_draw.as_ref() {
        page.draw(&mut pass, renderer, frontend);
    }

    // --- Step 10.7: build description ---
    // Original4F4480's tail: MessageListClass::Draw `0x4F455D`(->`0x5D49A0`),
    // the CCToolTip vtable+0xC call `0x4F4570` and the debug-stats call
    // `0x4F457E` all precede the Phobos VersionDescription hook `0x4F4583`,
    // which precedes the mouse-restore vtable+0x3C call `0x4F4593`. The black
    // box and red text at the tactical view's top right therefore go down at
    // the frame tail, after messages and tooltips. The dialog-vs-version-text
    // order is unpinned natively; this draws the description after the dialog.
    // `draw_pooled_ui_passthrough` keeps depth compare Always with no depth
    // write, so the late draw cannot be depth-rejected by the message/tooltip
    // quads that wrote depth, and it stays before
    // `stage_pre_cursor_composition`, so the text is inside the
    // ScreenCapture-retained pre-cursor composition (0x537BC0 hides the mouse
    // while copying).
    if text_in_frame {
        draw_pooled_ui_passthrough(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            solid,
            "build_text_fill",
        );
        draw_pooled_ui_passthrough(
            &mut pass,
            &renderer.batch_renderer,
            pool,
            Some(renderer.bit_font.atlas()),
            "build_text_text",
        );
    }

    // Active YR ScreenCaptureCommandClass::Execute (0x00537BC0) hides
    // WWMouse while copying the already-presented client. This frame keeps
    // every completed UI/sidebar surface and no cursor: the cursor goes over
    // its copies (`draw_software_cursor`, `window_ui::draw_battlefield`).
    drop(pass);
    renderer
        .retail_screenshot_frame_cache
        .stage_in_place_composition(
            renderer.game_frame_texture(),
            renderer.gpu.config.format,
            renderer.render_width(&frontend.screen),
            renderer.render_height(&frontend.screen),
            renderer.gpu.config.width,
            renderer.gpu.config.height,
        );
}

/// An unzoomed match's software cursor, over the window's copy of the frame.
pub(crate) fn draw_software_cursor(
    renderer: &RendererState,
    screen: &GameScreen,
    presentation: &MatchPresentationState,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    cursor: Option<crate::app::presentation::ui_overlays::SoftwareCursorDraw>,
) {
    let Some(texture) = cursor.and_then(|draw| software_cursor_texture(presentation, draw)) else {
        return;
    };
    let frame_depth = renderer
        .world_zoom_pass
        .as_ref()
        .map_or(&renderer.depth_view, |zoom| zoom.frame_depth_view());
    let mut pass = begin_main_load_pass(encoder, view, frame_depth);
    pass.set_scissor_rect(
        0,
        0,
        renderer.render_width(screen),
        renderer.render_height(screen),
    );
    draw_pooled_ui(
        &mut pass,
        &renderer.batch_renderer,
        &renderer.instance_pool,
        Some(texture),
        "software_cursor",
    );
}

// ---------------------------------------------------------------------------
// Draw helpers — thin wrappers around BatchRenderer methods with atlas lookup
// ---------------------------------------------------------------------------

/// The walk-ordered terrain tiles with each smudge after the tiles `cuts`
/// counts for it. `CellOverlay_TileDraw @ 0x00480350` blits a cell's tile,
/// then its smudge (`0x004804F5`, flags 0xE00: no Z read or write), inside
/// the tile walk: the tiles walked later still cover it where their Z
/// passes, and it lies under the cell-content layer that draws overlays.
/// What the remembered buildings' pixels are drawn through besides their
/// instances and runs: the building z-shape and each run's atlas page.
fn fogged_sources(
    presentation: &crate::app::presentation::state::MatchPresentationState,
    batch: &BatchRenderer,
    runs: &[super::draw_plan_lowering::ObjectDrawRun],
) -> Vec<wgpu::BindGroup> {
    use super::draw_plan_lowering::ObjectTexture;
    let zshape = presentation
        .building_zshape
        .as_ref()
        .map_or(batch.default_zshape_bind_group(), |z| &z.bind_group);
    let pages = runs.iter().filter_map(|run| match run.target {
        ObjectTexture::ShpPage(page) | ObjectTexture::SpriteAtlasEdit(page, _) => {
            presentation.sprite_atlas.as_ref()?.page(page)
        }
        _ => None,
    });
    std::iter::once(zshape)
        .chain(pages.map(|page| &page.texture.bind_group))
        .cloned()
        .collect()
}

fn draw_tiles_and_smudges<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    presentation: &'a crate::app::presentation::state::MatchPresentationState,
    cuts: &[u32],
) {
    let tiles = presentation.tile_atlas.as_ref().zip(pool.get("terrain"));
    let smudges = presentation.overlay_atlas.as_ref().zip(pool.get("smudge"));
    let draw_tiles = |pass: &mut wgpu::RenderPass<'a>, range: std::ops::Range<u32>| {
        if let Some((atlas, (buffer, count))) = tiles {
            let range = range.start.min(count)..range.end.min(count);
            batch.draw_zdepth_range(pass, &atlas.zdepth_bind_group, buffer, range);
        }
    };
    let mut drawn = 0;
    let mut smudge = 0;
    if let Some((atlas, (buffer, count))) = smudges {
        let count = count.min(cuts.len() as u32);
        while smudge < count {
            let cut = cuts[smudge as usize];
            let run = cuts[smudge as usize..count as usize]
                .iter()
                .take_while(|&&next| next == cut)
                .count() as u32;
            draw_tiles(pass, drawn..cut.max(drawn));
            drawn = cut.max(drawn);
            batch.draw_passthrough_range(pass, &atlas.texture, buffer, smudge, run);
            smudge += run;
        }
    }
    draw_tiles(pass, drawn..u32::MAX);
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

/// The band box on the wheel-zoomed frame, after the world went in. Drawn
/// on the final pixel grid: a 1px border must not become a subpixel span on
/// the world attachment and disappear before the filtered copy.
fn draw_zoomed_drag_rect(
    zoom: &crate::render::upscale_pass::WorldZoomPass,
    encoder: &mut wgpu::CommandEncoder,
    tactical: [u32; 4],
    batch: &BatchRenderer,
    pool: &InstanceBufferPool,
    drag: Option<&BatchTexture>,
) {
    if drag.is_some() && pool.get("drag").is_some_and(|(_, count)| count != 0) {
        let mut pass = begin_main_load_pass(encoder, zoom.color_view(), zoom.frame_depth_view());
        pass.set_scissor_rect(tactical[0], tactical[1], tactical[2], tactical[3]);
        draw_pooled_ui_passthrough(&mut pass, batch, pool, drag, "drag");
    }
}

/// VERA's HD art terrain: every tile the terrain pass drew inside the
/// native rect `clip`, in walk order, placed in native composition pixels.
fn hd_terrain_pieces<'a>(
    terrain: impl Iterator<Item = &'a SpriteInstance>,
    camera: [f32; 2],
    clip: [u32; 4],
) -> Vec<crate::render::hd_terrain::HdTerrainPiece> {
    let [cx, cy, cw, ch] = clip.map(|v| v as f32);
    terrain
        .filter_map(|instance| {
            let origin = [
                (instance.position[0] - camera[0] + 0.5).floor(),
                (instance.position[1] - camera[1] + 0.5).floor(),
            ];
            let size = instance.size;
            let outside = origin[0] + size[0] <= cx
                || origin[1] + size[1] <= cy
                || origin[0] >= cx + cw
                || origin[1] >= cy + ch;
            (!outside).then_some(crate::render::hd_terrain::HdTerrainPiece {
                uv_origin: instance.uv_origin,
                origin,
                size,
            })
        })
        .collect()
}

/// VERA's HD art: every building body, bib and animation piece drawn
/// from an SHP atlas page inside the native rect `clip`, placed in native
/// composition pixels as the SHP blitter places it. Bodies first, then
/// everything drawn after them: powered and active animations sit in the
/// ordinary runs. Only frames the art knows give a draw, so the units in
/// those runs pass through.
fn hd_art_pieces(
    layers: &[super::draw_plan_lowering::ObjectLayerPass],
    camera: [f32; 2],
    clip: [u32; 4],
) -> Vec<crate::render::hd_art::HdArtPiece> {
    let [cx, cy, cw, ch] = clip.map(|v| v as f32);
    let mut pieces = Vec::new();
    for layer in layers {
        for run in layer.building_base_runs.iter().chain(&layer.runs) {
            let super::draw_plan_lowering::ObjectTexture::ShpPage(page) = run.target else {
                continue;
            };
            let range = run.start as usize..(run.start + run.count) as usize;
            for instance in layer.instances.get(range).into_iter().flatten() {
                let origin = [
                    (instance.position[0] - camera[0] + 0.5).floor(),
                    (instance.position[1] - camera[1] + 0.5).floor(),
                ];
                let size = instance.size;
                if origin[0] + size[0] <= cx
                    || origin[1] + size[1] <= cy
                    || origin[0] >= cx + cw
                    || origin[1] >= cy + ch
                {
                    continue;
                }
                pieces.push(crate::render::hd_art::HdArtPiece {
                    page,
                    uv_origin: instance.uv_origin,
                    origin,
                    size,
                    source_palette: instance.source_palette,
                });
            }
        }
    }
    pieces
}

/// VERA's HD art voxels: every unit atlas piece inside the native rect
/// `clip` whose page has an HD page, grouped by page, placed in native
/// composition pixels as the voxel blitter places it unzoomed.
fn hd_voxel_pages<'a>(
    units: &'a crate::render::unit_atlas::UnitAtlas,
    poses: &'a crate::render::unit_pose_cache::VxlPoseFrameCache,
    layers: &[super::draw_plan_lowering::ObjectLayerPass],
    camera: [f32; 2],
    clip: [u32; 4],
) -> Vec<crate::render::hd_voxel::HdVoxelPage<'a>> {
    use super::draw_plan_lowering::ObjectTexture;
    let [cx, cy, cw, ch] = clip.map(|v| v as f32);
    let mut pages: Vec<crate::render::hd_voxel::HdVoxelPage<'a>> = Vec::new();
    // The atlas's pages, then the pose cache's (tilted, crashing, rocking
    // and tumbling voxels, `unit_pose_cache`).
    let mut slots: std::collections::HashMap<(bool, usize), Option<usize>> = Default::default();
    for layer in layers {
        for run in layer.building_base_runs.iter().chain(&layer.runs) {
            let key = match run.target {
                ObjectTexture::UnitAtlasPage(page) => (false, page),
                ObjectTexture::UnitPosePage(page) => (true, page),
                _ => continue,
            };
            let slot = *slots.entry(key).or_insert_with(|| {
                let (native, hd) = match key {
                    (false, page) => (
                        units.page_texture(page).map(|t| &t.view),
                        units.hd_page(page),
                    ),
                    (true, page) => (
                        poses.page_texture(page).map(|t| &t.view),
                        poses.hd_page(page),
                    ),
                };
                let (native, hd) = (native?, hd?);
                pages.push(crate::render::hd_voxel::HdVoxelPage {
                    native,
                    hd,
                    draws: Vec::new(),
                });
                Some(pages.len() - 1)
            });
            let Some(slot) = slot else {
                continue;
            };
            let range = run.start as usize..(run.start + run.count) as usize;
            for instance in layer.instances.get(range).into_iter().flatten() {
                let origin = [
                    (instance.position[0] - camera[0] + 0.5).floor(),
                    (instance.position[1] - camera[1] + 0.5).floor(),
                ];
                let size = instance.size;
                if origin[0] + size[0] <= cx
                    || origin[1] + size[1] <= cy
                    || origin[0] >= cx + cw
                    || origin[1] >= cy + ch
                {
                    continue;
                }
                pages[slot]
                    .draws
                    .push(crate::render::hd_voxel::HdVoxelDraw {
                        origin,
                        size,
                        uv_origin: instance.uv_origin,
                        uv_size: instance.uv_size,
                        remap_row: instance.draw_state.remap_row,
                        source_palette: instance.source_palette,
                    });
            }
        }
    }
    pages
}

/// The wheel-zoomed world, resampled once from world pixels into the window
/// after the Options zoom scaled the frame there (`app::frame`), instead of
/// only through the frame. VERA-only: stock YR has no world zoom. Whatever the
/// frame drew over the battlefield keeps showing at the frame's pixels
/// (`WorldZoomPass::draw_window_copy`); the battlefield's window-pixel text and
/// the cursor go over this (`window_ui`). It draws only when the frame's
/// `set_view` kept a window copy (`render::mod`).
pub(crate) fn draw_world_window_copy(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    presentation: &MatchPresentationState,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
) {
    let (Some(zoom), Some(world)) = (
        renderer.tactical_zoom(screen),
        renderer.world_zoom_pass.as_ref(),
    ) else {
        return;
    };
    let gpu = &renderer.gpu;
    world.draw_window_copy(
        &gpu.device,
        &gpu.queue,
        encoder,
        view,
        renderer.combat_light_renderer.composition_texture(),
        zoom,
        crate::app::input::camera::tactical_viewport_px(
            platform,
            renderer,
            screen,
            presentation,
            persistence,
        ),
    );
}

/// VERA's ground marks: world-surface stores at neutral A, before wheel zoom.
pub(super) fn draw_pooled_surface_lines<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    tex: Option<&'a BatchTexture>,
    key: &'static str,
) {
    if let (Some(t), Some((buf, count))) = (tex, pool.get(key)) {
        batch.draw_surface_stores(pass, t, buf, count);
    }
}

/// Native path, rally and planning strokes belong to the world surface,
/// before wheel zoom. Their dashed stores keep or skip each pixel by the
/// live A on the GPU.
pub(super) fn draw_pooled_surface_strokes<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    key: &'static str,
) {
    if let Some((buf, count)) = pool.get(key) {
        batch.draw_surface_strokes(pass, buf, count);
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

#[cfg(all(test, tests_app_presentation_render))]
mod tests {
    use super::{CellBodyAtlas, CellBodyDraw, RenderZPolicy, cell_body_runs};
    // Keep byte offsets and slices in the same source on LF and CRLF checkouts.
    static PRODUCTION: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        include_str!("draw_passes.rs")
            .split_once("\nmod tests {")
            .expect("production source before tests")
            .0
            .replace("\r\n", "\n")
    });

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
        PRODUCTION
            .find(needle)
            .unwrap_or_else(|| panic!("missing production draw anchor {needle:?}"))
    }

    #[test]
    fn ground_marks_follow_the_kept_ground_and_precede_buildings_and_objects() {
        let kept = source_offset(".keep(&renderer.gpu.device, encoder)");
        let marks = source_offset("\"ground_marks\"");
        let rings = source_offset("\"ground_strokes\"");
        let building_bases = source_offset("ObjectDrawPhase::BuildingBase");
        let objects = source_offset("for layer in 0..=3 {");
        assert!(kept < marks, "the kept ground must not store the marks");
        assert!(
            marks < rings,
            "the ferry's circles follow the formation dots"
        );
        assert!(rings < building_bases && rings < objects);
    }

    /// Base terrain 0x006D3470 draws the remembered buildings between the
    /// tiles and smudges and the cell content (0x006D3290).
    #[test]
    fn fogged_snapshots_draw_between_the_ground_and_the_cell_bodies() {
        let ground = source_offset(".keep_ground(&renderer.gpu.device, encoder, ground)");
        let fogged = source_offset("pool.get(\"fogged_objects\")");
        let cell_bodies = source_offset("\"cell_body\",");
        let bridge_shadows = source_offset("pool.get(\"overlay_bridge_body_shadow\")");
        assert!(ground < fogged && fogged < cell_bodies && cell_bodies < bridge_shadows);
    }

    #[test]
    fn gsi_13_01_pixel_fx_and_timers_precede_native_outcome_and_chrono_capture() {
        let first_rally = source_offset("\"factory_rally_first\"");
        let ion_blasts = source_offset(".draw_ion_blasts(encoder, view)");
        let building_bases = source_offset("ObjectDrawPhase::BuildingBase");
        let objects = source_offset("for layer in 0..=3 {");
        let combat_lights = source_offset("data.persistent_lights.clone().collect()");
        let second_rally = source_offset("\"factory_rally_second\"");
        let target_lines = source_offset("\"target_lines\"");
        let tactical_lines = source_offset("&[data.tactical_lines]");
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
            "pass.set_scissor_rect(\n        0,\n        0,\n        renderer.render_width(&frontend.screen),",
        );
        let first_screen_submission = source_offset("\"message_text\"");
        let tooltip_text = source_offset("\"tooltip_text\"");
        let build_fill = source_offset("\"build_text_fill\"");
        let build_text = source_offset("\"build_text_text\"");
        let pre_cursor_capture = source_offset(".stage_in_place_composition(");
        let cursor = source_offset("\"software_cursor\"");

        // Original6D4648,6D4656,6D465F: IonBlast DrawAll between the first
        // rally pass and the object loop.
        assert!(building_bases < first_rally);
        assert!(first_rally < ion_blasts);
        assert!(ion_blasts < objects);
        // Original6D4648,6D4664,6D46CF,6D4750. A is now consumed by
        // each world blitter, so there is no final framebuffer multiply.
        assert!(first_rally < combat_lights);
        assert!(combat_lights < second_rally);
        // 0x006D46D8 draws the PLACE cells before the lines at 0x006D4750.
        assert!(second_rally < placement);
        assert!(placement < target_lines);
        assert!(target_lines < tactical_lines);
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
        // The description lives in the chrome pass, after the scissor reset.
        assert!(full_window_scissor < build_fill);
        // Native tail: CCToolTip 0x4F4570 then Phobos 0x4F4583.
        assert!(tooltip_text < build_fill);
        // Fill box first, then the red text, like every pooled text pair.
        assert!(build_fill < build_text);
        // The text stays inside the pre-cursor composition (mouse restore
        // 0x4F4593 follows the Phobos hook 0x4F4583 natively).
        assert!(build_text < pre_cursor_capture);
        // The software cursor draws after the retained composition.
        assert!(pre_cursor_capture < cursor);

        let timer_tail = &PRODUCTION[sparkle..outcome_start];
        assert_eq!(
            timer_tail.matches(".draw").count() + timer_tail.matches("draw_pooled").count(),
            4,
            "PixelFX, the two timer submissions and VERA's module battlefield layer \
             precede the native result draw; the Phobos build description now draws \
             at the RenderFrame_main tail"
        );
        // Exhaustiveness: the description draws exactly once in the span from
        // tooltips to cursor, pinning it out of the tactical tail.
        let tail = &PRODUCTION[tooltip_text..cursor];
        assert_eq!(
            tail.matches("\"build_text_fill\"").count(),
            1,
            "the Phobos build description draws exactly once, at the frame tail"
        );
    }

    #[test]
    fn gsi_04_01_retained_sidebar_radar_subpass_ends_with_content_boundary() {
        let chrome = source_offset("\"sidebar_chrome\"");
        let cameo = source_offset("\"sidebar_cameo\"");
        let cameo_hover = source_offset("\"sidebar_cameo_hover\"");
        let gclock = source_offset("\"sidebar_gclock\"");
        let unavailable = source_offset("\"sidebar_unavailable\"");
        let flash = source_offset("\"sidebar_flash\"");
        let queue_fill = source_offset("\"sidebar_queue_overlay\"");
        let queue_text = source_offset("\"sidebar_queue_text\"");
        let label_overlay = source_offset("\"sidebar_label_overlay\"");
        let label_text = source_offset("\"sidebar_label_text\"");
        let cameo_overlay = source_offset("\"sidebar_cameo_overlay\"");
        let sidebar_text = source_offset("\"sidebar_text\"");
        let radar_anim = source_offset("\"radar_anim\"");
        let minimap = source_offset("\"minimap\"");
        let viewport = source_offset("\"viewport_rect\"");
        let boundary = source_offset("\"radar_content_boundary\"");
        let message = source_offset("\"message_text\"");

        assert!(chrome < cameo);
        assert!(cameo < cameo_hover);
        assert!(cameo_hover < unavailable);
        assert!(unavailable < flash);
        assert!(flash < queue_fill);
        assert!(queue_fill < queue_text);
        assert!(queue_text < label_overlay);
        assert!(label_overlay < label_text);
        assert!(label_text < gclock);
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
        let retained_tail = &PRODUCTION[boundary..message];
        for later_retained_batch in [
            "\"sidebar_chrome\"",
            "\"sidebar_cameo\"",
            "\"sidebar_gclock\"",
            "\"sidebar_unavailable\"",
            "\"sidebar_flash\"",
            "\"sidebar_queue_overlay\"",
            "\"sidebar_queue_text\"",
            "\"sidebar_label_overlay\"",
            "\"sidebar_label_text\"",
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
        let surface_pass = source_offset("draw_sidebar_surface(\n        platform,");
        let frame_pass = source_offset("begin_main_pass(encoder, view, &world_depth)");
        let timer_text = source_offset("\"super_timer_text\"");
        let blit = source_offset("surface.composite(&mut pass)");
        let message = source_offset("\"message_text\"");

        assert!(surface_pass < frame_pass);
        assert!(timer_text < blit);
        assert!(blit < message);

        // No sidebar or bar layer draws in the frame pass, so the frame keeps
        // no sidebar depth: the tooltips and the cursor show over the radar.
        // The column's text may go over the copy there, without depth
        // (`sidebar_surface::column_text`).
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
            assert!(!PRODUCTION[frame_pass..].contains(layer), "{layer}");
        }
    }

    /// gamemd's pop-up hides what it lands on, so its fill is a solid texel
    /// tinted black. Binding the sidebar darken strip instead (175/255 alpha)
    /// reads as a translucent box, which is what this pins out.
    #[test]
    fn tooltip_fill_binds_the_solid_texel_not_the_darken_strip() {
        let binding = source_offset("let solid = match_state");
        let fill = source_offset("\"tooltip_fill\"");
        let border = source_offset("\"tooltip_border\"");
        assert!(binding < fill);
        let slice = &PRODUCTION[binding..border];
        assert!(
            !slice.contains("darken_texture"),
            "tooltip lanes must not draw with the translucent darken strip"
        );
        assert!(fill < border);
    }
}

#[cfg(all(test, tests_app_presentation_render))]
#[path = "tmp_clear_gpu_tests.rs"]
mod tmp_clear_gpu_tests;
