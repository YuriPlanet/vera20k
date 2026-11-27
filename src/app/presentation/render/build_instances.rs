//! Per-frame sprite instance builders — grouped by rendering phase.
//!
//! Each function builds one phase of the rendering pipeline and returns a struct
//! holding the instance vectors. This makes the pipeline flow in `render_game()`
//! scannable: build world → build debug → build UI → build sidebar → build fog.
//!
//! ## Dependency rules
//! - Internal to `presentation::render` — only called from mod.rs.

use crate::app::diagnostics::debug_overlays;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::input::commands::preferred_local_owner;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::presentation::instances;
use crate::app::presentation::sidebar_render::{
    active_minimap_screen_rect, build_command_bar_instances, build_sidebar_cameo_instances,
    build_sidebar_chrome_instances, current_sidebar_view,
};
use crate::app::presentation::ui_overlays::{
    build_building_status_instances, build_control_group_status_instances,
    build_software_cursor_instances,
};
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::map::terrain::TilePlacement;
use crate::map::theater::TileKey;
use crate::render::batch::SpriteInstance;
use crate::render::surface_line::SurfaceStroke;
use crate::ui::game_screen::GameScreen;
use crate::util::time::{Duration, Instant};
#[path = "ferry_lines.rs"]
mod ferry_lines;
#[path = "planning_lines.rs"]
mod planning_lines;

// ---------------------------------------------------------------------------
// Phase structs — group related instance vectors for clean data flow
// ---------------------------------------------------------------------------

/// Game-world sprite instances: terrain tiles, overlays, entities, bridges.
pub(super) struct WorldInstances {
    pub terrain: crate::render::terrain_instances::TerrainInstances,
    /// Every overlay body, high bridge or ordinary, in the cell body sweep's
    /// order, with each instance's draw beside it.
    pub cell_bodies: Vec<SpriteInstance>,
    pub cell_body_draws: Vec<instances::CellBodyDraw>,
    /// Represented Display parents in their five retained native layer orders.
    pub object_layers: [super::draw_plan_lowering::ObjectLayerPass; 5],
    /// Native separate FoggedObject drawing pass before live building bases.
    pub fogged_objects: super::draw_plan_lowering::ObjectLayerPass,
    /// One BuildingClass::Array-ordered text sweep after Ground2 bodies.
    pub building_status_text: crate::app::presentation::ui_overlays::BuildingStatusTextInstances,
    /// Static smudge decals (craters, scorches) in the tile walk's order.
    pub smudge: Vec<SpriteInstance>,
    /// For each smudge, how many walk-ordered terrain tiles draw before it
    /// (`render::smudge::VisibleSmudges::cuts`).
    pub smudge_cuts: Vec<u32>,
    pub bridge_body_shadow: Vec<SpriteInstance>,
    /// Actual particle pieces observed by the existing map diagnostic policy.
    pub particle_draws: Option<Vec<instances::ParticleDrawObservation>>,
    /// PixelFX water/ore sparkles — 1-pixel cell dots emitted per frame.
    /// Empty when the live `[Options] DetailLevel` projection is zero.
    pub cell_sparkles: Vec<SpriteInstance>,
    /// The mod's and the map's status templates over the battlefield
    /// (`ui_modules::field`).
    pub module_field: crate::app::ui_modules::field::FieldInstances,
    /// What the terrain tile walk and the Display object walk took, for the
    /// performance panel's Most CPU row (`RenderPhases::terrain`,
    /// `RenderPhases::objects`).
    pub spans: WorldSpans,
}

/// Two parts of phase 1's time, measured inside `build_world_instances`.
#[derive(Clone, Copy, Default)]
pub(super) struct WorldSpans {
    /// The visible terrain tiles.
    pub terrain: Duration,
    /// The Display members: units, infantry, buildings, debris and bullets,
    /// voxel image renders included.
    pub objects: Duration,
}

/// Debug visualization overlays (toggled by hotkeys at runtime).
pub(super) struct DebugInstances {
    pub pathgrid: Vec<SpriteInstance>,
    pub cell_grid: Vec<SpriteInstance>,
    pub path: Vec<SpriteInstance>,
    /// Numbered foot route: three vectors because one batch binds one texture.
    pub foot_stroke: Vec<SpriteInstance>,
    pub foot_nodes: Vec<SpriteInstance>,
    pub foot_text: Vec<SpriteInstance>,
    pub heightmap: Vec<SpriteInstance>,
}

/// In-game UI overlays: health bars, placement preview, cursor.
pub(super) struct UiInstances {
    pub software_cursor: Vec<SpriteInstance>,
    /// The frame `software_cursor` was built from, whose texture draws it.
    pub software_cursor_draw: Option<crate::app::presentation::ui_overlays::SoftwareCursorDraw>,
    pub drag: Vec<SpriteInstance>,
    /// World-surface stores that lie on the ground: drawn before the
    /// buildings and objects, which cover them. The VERA formation line's
    /// dots, then the ferry's circles.
    pub ground_marks: Vec<SpriteInstance>,
    pub ground_strokes: Vec<SurfaceStroke>,
    pub placement: Vec<SpriteInstance>,
    /// Phobos placement preview artwork, drawn after the PLACE cells.
    #[cfg(feature = "phobos")]
    pub placement_ghost: Vec<SpriteInstance>,
    pub target_line: Vec<SurfaceStroke>,
    pub tactical_line_calls: Vec<crate::app::presentation::airstrike_lines::TacticalLineCall>,
    pub factory_rally_first: Vec<SurfaceStroke>,
    pub factory_rally_second: Vec<SurfaceStroke>,
    /// One stream per beacon row: each row has its own House Convert strip.
    pub beacons: Vec<Vec<SpriteInstance>>,
}

/// Sidebar chrome, cameos, text, minimap, and radar animation.
pub(super) struct SidebarInstances {
    /// The sidebar surface's layers, in layout space (`ui::sidebar::frame`).
    pub chrome: Vec<SpriteInstance>,
    /// The bottom command bar's surface layer, in layout space too.
    pub command_bar: Vec<SpriteInstance>,
    /// Whether `chrome` and `command_bar` draw from the smooth sidebar's copy
    /// of the chrome (`render::sidebar_smoothing`) rather than the atlas.
    pub smooth_chrome: bool,
    pub smooth_command_bar: bool,
    pub cameo: Vec<SpriteInstance>,
    pub cameo_hover: Vec<SpriteInstance>,
    pub unavailable: Vec<SpriteInstance>,
    pub flash: Vec<SpriteInstance>,
    pub gclock: Vec<SpriteInstance>,
    pub cameo_overlay: Vec<SpriteInstance>,
    pub queue_overlay: Vec<SpriteInstance>,
    pub queue_text: Vec<SpriteInstance>,
    pub label_overlay: Vec<SpriteInstance>,
    pub label_text: Vec<SpriteInstance>,
    pub text: Vec<SpriteInstance>,
    #[cfg(feature = "phobos")]
    pub sw_sidebar_chrome: Vec<SpriteInstance>,
    #[cfg(feature = "phobos")]
    pub sw_sidebar_cameo: Vec<SpriteInstance>,
    #[cfg(feature = "phobos")]
    pub sw_sidebar_gclock: Vec<SpriteInstance>,
    #[cfg(feature = "phobos")]
    pub sw_sidebar_hover: Vec<SpriteInstance>,
    #[cfg(feature = "phobos")]
    pub sw_sidebar_overlay: Vec<SpriteInstance>,
    #[cfg(feature = "phobos")]
    pub sw_sidebar_text: Vec<SpriteInstance>,
    pub minimap: Vec<SpriteInstance>,
    pub viewport_rect: Vec<SpriteInstance>,
    pub content_boundary: Vec<SpriteInstance>,
    pub radar_anim: Vec<SpriteInstance>,
    /// The revision of the retained sidebar view these instances drew,
    /// `None` when none drew.
    pub view_revision: Option<u64>,
}

impl SidebarInstances {
    /// Observe the exact vectors that the upload and draw phases consume.
    pub(super) fn emitted_instance_counts(&self) -> super::GameRenderInstanceCounts {
        super::GameRenderInstanceCounts::from_lengths(
            self.minimap.len(),
            self.viewport_rect.len(),
            self.radar_anim.len(),
            self.text.len() + self.queue_text.len() + self.label_text.len(),
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

/// Whether the TechnoExtras path draws an entity's frozen convenience rows.
/// Buildings are excluded: their rows are drawn by
/// `build_building_status_instances` at the building status anchor. Drawing
/// them here too showed two repair countdowns over a captured-and-repairing
/// neutral building. Foot and air units keep their rows here.
fn convenience_info_lines_admitted(
    convenience_help: bool,
    viewer_allied: bool,
    category: crate::map::entities::EntityCategory,
) -> bool {
    use crate::map::entities::EntityCategory;
    convenience_help && viewer_allied && category != EntityCategory::Structure
}

/// Build all game-world sprite instances: terrain tiles, map overlays, bridges,
/// VXL units, SHP buildings/infantry, AnimClass objects, damage fires.
/// Ground parents retain Display order; residual effect buckets still sort depth.
/// The SHP visitor, as the unit visitor draws a Carryall's SHP cargo with it.
fn shp_cargo<'a, V>(shp: &'a mut Option<V>) -> Option<&'a mut instances::ShpVisit<'a>>
where
    V: FnMut(instances::ShpDraw<'_>, &crate::app::presentation::frame::ObjectRecord) + 'a,
{
    shp.as_mut()
        .map(|visit| visit as &mut instances::ShpVisit<'a>)
}

pub(super) fn build_world_instances(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    sw: f32,
    sh: f32,
    bracket_palette: Option<[u16; 2]>,
    cursor: &crate::app::input::cursor::CursorTargets,
) -> WorldInstances {
    let hover = cursor.hover;
    let terrain_started = Instant::now();
    // The draw before hands its tile list back (`render_game`).
    let terrain_buffer =
        std::mem::take(&mut match_state.match_presentation.cached_terrain_instances);
    // Terrain tiles use the selected TMP owner exactly. A sparse/null cell in
    // a positive suffix remains absent instead of borrowing pristine UVs.
    let uv_fn_closure;
    let uv_fn: Option<&dyn Fn(u16, u8, u8) -> Option<TilePlacement>> = if let Some(atlas) =
        &match_state.match_presentation.tile_atlas
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
    let terrain = if let Some(grid) = &match_state.match_presentation.terrain_grid {
        // Terrain draws regardless of shroud — the native tile walk has no
        // explored gate (`CellOverlay_TileDraw @ 0x00480350`); the flat shroud
        // curtain blacks out unexplored ground in the multiply pass.
        crate::render::terrain_instances::build_visible_instances(
            grid,
            Some(match_state.match_presentation.lighting.grid()),
            match_state.input.camera_x,
            match_state.input.camera_y,
            sw,
            sh,
            uv_fn,
            match_state
                .sim_runtime
                .presentation()
                .and_then(|rt| rt.view().resolved_terrain()),
            terrain_buffer,
        )
    } else {
        crate::render::terrain_instances::TerrainInstances {
            normal: terrain_buffer,
            walk: Vec::new(),
        }
    };
    let terrain_span = terrain_started.elapsed();

    // Map overlays and walls remain in the fixed per-cell draw plan. Terrain
    // objects join live Ground registrations below; low bridges (LOBRDG*) ride
    // in `overlay`, while high bridge bodies use instances::bridges.
    // The frame keeps the order for all its draws; without a frame it is
    // built for this one.
    let own_order;
    let display_order = match match_state.sim_runtime.presentation() {
        Some(rt) => {
            let world = rt.view().simulation();
            match match_state.sim_runtime.frame() {
                Some(frame) => frame.display_order(),
                None => {
                    own_order = super::draw_plan_lowering::NativeDisplayOrder::from_world(
                        world,
                        Some(&rt.resources.rules),
                    );
                    &own_order
                }
            }
        }
        None => {
            own_order = super::draw_plan_lowering::NativeDisplayOrder::default();
            &own_order
        }
    };
    let mut planned_objects = super::draw_plan_lowering::PlannedObjects::default();
    let mut bodies = Vec::new();
    instances::build_overlay_instances(
        platform,
        frontend,
        match_state,
        sw,
        sh,
        &mut bodies,
        &mut planned_objects,
        display_order,
    );
    // High-bridge bodies read live CellClass bridge fields post-tick (NOT
    // OverlayGrid), then join the ordinary bodies in one native sweep.
    instances::bridges::build_bridge_body_instances(match_state, sw, sh, &mut bodies);
    let mut cell_bodies: Vec<SpriteInstance> =
        std::mem::take(&mut match_state.match_presentation.cached_cell_body_instances);
    cell_bodies.clear();
    let mut cell_body_draws = Vec::new();
    instances::submit_cell_body_sweep(bodies, &mut cell_bodies, &mut cell_body_draws);
    // The shadow sweep keeps the same cell order; accepted pixels write Z.
    let mut bridge_body_shadow: Vec<SpriteInstance> = Vec::new();
    instances::bridges::build_bridge_shadow_instances(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        &mut bridge_body_shadow,
    );

    // Smudges: static crater/scorch decals on top of terrain, under entities.
    // Registered SmudgeType SHPs share the live OverlayAtlas and Cell light
    // grid with the ordinary ground drawing consumers.
    let smudges = build_smudge_instances(frontend, match_state, sw, sh);
    let smudge_cuts = smudges.cuts(&terrain.walk);
    let smudge = smudges.instances;

    let objects_started = Instant::now();
    renderer.vxl_pose_frame_cache.borrow_mut().begin_frame();
    // Every voxel render shares the renderer's buffers and the UseBuffer
    // latch (`crate::render::voxel_image_cache`), so the objects draw in
    // the order Tactical draws them: the Display members, layer by layer
    // (`0x006D8F19..0x006D95A9`). Each member is one unit, piece of debris,
    // building or infantry, or Bullet. A building's voxel turret remains
    // owned by the building display call, in the same contiguous Ground
    // parent instead of an atlas-level tie.
    if let (Some(runtime), Some(frame)) = (
        match_state.sim_runtime.presentation(),
        match_state.sim_runtime.frame(),
    ) {
        let local_owner = crate::app::input::commands::preferred_local_owner_name(match_state);
        let shp_inputs = instances::shp_inputs(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            persistence,
            local_owner.as_deref(),
        );
        let mut units = instances::unit_visitor(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            persistence,
            display_order,
        );
        let mut debris = instances::voxel_anim_visitor(
            platform,
            renderer,
            &frontend.screen,
            match_state,
            process_assets,
            persistence,
            display_order,
        );
        let mut shp = shp_inputs
            .as_ref()
            .map(|input| instances::shp_visitor(input, display_order));
        let mut bullets = instances::projectile_visitor(
            platform,
            renderer,
            &frontend.screen,
            match_state,
            process_assets,
            persistence,
            display_order,
        );
        // A record far off the view is passed by on its span, before its
        // large record is read: neither visitor would plan it.
        let window = instances::PassByWindow::of(
            platform,
            renderer,
            &frontend.screen,
            match_state,
            persistence,
            shp_inputs.as_ref().map(|input| input.building_viewport),
        );
        use crate::app::presentation::display_members::ObjectKind;
        for member in frame.display_members(&runtime.simulation).objects() {
            let objects = &mut planned_objects;
            let slot = match member.kind {
                ObjectKind::Record { slot, span } if window.reaches(span) => slot,
                ObjectKind::Record { slot, .. } => {
                    // The cross-check runs the visitors on the passed-by
                    // object into a scratch list: debug builds only.
                    if cfg!(debug_assertions) {
                        let draws_nothing = frame
                            .record_at(slot)
                            .zip(instances::layer_draw_band(member.layer))
                            .is_none_or(|(entity, band)| {
                                let mut scratch =
                                    super::draw_plan_lowering::PlannedObjects::default();
                                if let Some(visit) = units.as_mut() {
                                    visit(&mut scratch, entity, band, shp_cargo(&mut shp));
                                }
                                if let Some(visit) = shp.as_mut() {
                                    visit(instances::ShpDraw::Member(&mut scratch), entity);
                                }
                                scratch.is_empty()
                            });
                        assert!(
                            draws_nothing,
                            "object {} was passed by but draws",
                            member.id
                        );
                    }
                    continue;
                }
                ObjectKind::Debris => {
                    if let Some(visit) = debris.as_mut() {
                        visit(objects, member.id);
                    }
                    continue;
                }
                ObjectKind::Bullet => {
                    if let Some(visit) = bullets.as_mut() {
                        visit(objects, member.id);
                    }
                    continue;
                }
            };
            let (Some(entity), Some(band)) = (
                frame.record_at(slot),
                instances::layer_draw_band(member.layer),
            ) else {
                continue;
            };
            // A Carryall's SHP cargo draws inside the Carryall's draw.
            if let Some(visit) = units.as_mut() {
                visit(objects, entity, band, shp_cargo(&mut shp));
            }
            if let Some(visit) = shp.as_mut() {
                visit(instances::ShpDraw::Member(objects), entity);
            }
        }
    }
    let objects_span = objects_started.elapsed();
    let mut particle_draws =
        crate::app::diagnostics::state::diagnostic_presentation_ms(diag, match_state)
            .map(|_| Vec::new());
    let display_inputs = match_state
        .sim_runtime
        .presentation()
        .zip(match_state.sim_runtime.frame())
        .map(|(runtime, frame)| {
            let presentation = &match_state.match_presentation;
            let (x, y, width, height) = crate::app::input::camera::tactical_viewport_px(
                platform,
                renderer,
                &frontend.screen,
                &match_state.match_presentation,
                persistence,
            );
            let local_owner = crate::app::input::commands::preferred_local_owner_name(match_state)
                .and_then(|name| runtime.simulation.interner().get(&name));
            instances::DisplayEffectInputs {
                unit_motion: Some(&presentation.unit_motion),
                sim: &runtime.simulation,
                frame,
                rules: &runtime.resources.rules,
                camera: [match_state.input.camera_x, match_state.input.camera_y],
                render_size: [
                    renderer.render_width(&frontend.screen),
                    renderer.render_height(&frontend.screen),
                ],
                tactical: [x, y, width, height],
                zoom: match_state.input.zoom_level,
                axis: instances::depth_axis(&match_state.match_presentation),
                z_seed: renderer.native_z_seed.seed(),
                atlas: presentation.sprite_atlas.as_ref(),
                detail: &presentation.detail,
                detail_level: presentation.in_game_options.detail_level,
                lighting: presentation.lighting.grid(),
                retained_anim_intensities: Some(presentation.lighting.anim_intensities()),
                house_colors: presentation.effective_house_color_map(),
                hidden_super_anims: &presentation.hidden_super_anims,
                show_hidden: presentation.in_game_options.show_hidden,
                local_owner,
                observe_particles: crate::app::diagnostics::state::diagnostic_presentation_ms(
                    diag,
                    match_state,
                )
                .is_some(),
            }
        });
    if let Some(input) = display_inputs.as_ref() {
        // Prepare slot binding/light pieces without querying the shared FPS
        // latch. Each Anim's retained Display encounter admits and appends its
        // pieces; preparation shares that encounter's view.
        let mut prepared_building_anims = super::draw_plan_lowering::PlannedObjects::default();
        instances::build_building_anim_instances(
            renderer,
            frontend,
            match_state,
            input.tactical_admission(),
            &mut prepared_building_anims,
            display_order,
        );
        particle_draws = instances::build_display_effect_instances(
            input,
            &mut planned_objects,
            display_order,
            prepared_building_anims,
        );
    }

    // Every dynamic pose consumer, including Bullet46B0C0, has now populated
    // the shared pages; the draw passes encode them before the main pass.

    // Selection is read by its existing presentation producer; only parents
    // admitted by the Display/body plan receive commands. Native6D913C and
    //6D9153 run in that parent's slot, before its body at6D916C.
    if let Some(palette) = bracket_palette {
        let camera = super::tactical_surface_line_viewport(
            platform,
            renderer,
            &frontend.screen,
            match_state,
            persistence,
        )
        .camera;
        let mut brackets =
            crate::app::presentation::selection_brackets::build_selection_bracket_lines(
                frontend,
                match_state,
                camera,
                palette,
                planned_objects.iter().map(|parent| parent.parent.id),
            );
        for parent in planned_objects.iter_mut() {
            parent.brackets = brackets.remove(&parent.parent.id);
        }
    }
    // The same admitted Display parents feed native6D9789's later sweep.
    // Shared health admission/selection/hover owners derive frame-local strips;
    // storage order and atlas identity never determine their submission order.
    // The frame's cursor pick serves the group labels and every health bar:
    // both read the hover flag the cursor's action target sets.
    let local_owner = crate::app::input::commands::preferred_local_owner_name(match_state);
    let hovered = crate::app::presentation::ui_overlays::health_hover_target(hover);
    let mut group_labels = build_control_group_status_instances(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        local_owner.as_deref(),
        hovered,
        sw,
        sh,
    );
    if let (Some(sim), Some(frame), Some(overlay)) = (
        match_state
            .sim_runtime
            .presentation()
            .map(|rt| &rt.simulation),
        match_state.sim_runtime.frame(),
        match_state.match_presentation.selection_overlay.as_ref(),
    ) {
        let speech_wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        // The original's families the mod's or map's status templates hide
        // (`ui_modules::status`).
        let hidden_native = match_state
            .ui_modules
            .as_ref()
            .map_or(0, |ui| ui.status.hidden());
        // The text families' built-in templates (`health_templates`) may hide
        // or move them; asked once for the frame.
        let info_shift = crate::app::presentation::health_templates::family(
            crate::app::presentation::health_templates::Mark::InfoLabel,
        );
        #[cfg(feature = "phobos")]
        let digital_shift = crate::app::presentation::health_templates::family(
            crate::app::presentation::health_templates::Mark::DigitalDisplay,
        );
        for parent in planned_objects.iter_mut() {
            parent.status = frame.object(parent.parent.id).map(|entity| {
                let mut status = crate::app::presentation::ui_overlays::techno_extras_for_parent(
                    sim,
                    entity,
                    crate::app::state::active_rules(frontend, match_state),
                    overlay,
                    hovered,
                    local_owner.as_deref(),
                    crate::app::presentation::ui_overlays::StatusViewport {
                        camera: [match_state.input.camera_x, match_state.input.camera_y],
                        size: [sw, sh],
                        zoom: match_state.input.zoom_level,
                        ignore_visibility: match_state.sandbox_full_visibility,
                    },
                    persistence.health_bar_mode(),
                );
                status.group = group_labels.remove(&parent.parent.id).unwrap_or_default();
                // VERA convenience status rows (reload / unit repair): built
                // here because the GAME.FNT atlas lives on the renderer. The
                // texts are formatted here from facts captured with the frame,
                // with the glyph geometry. Uses the same
                // health-bar visibility admission the pips already passed.
                // Only the local house and its allies: an enemy's row is
                // never drawn, even when its body is in view.
                let viewer_allied = local_owner.as_deref().is_some_and(|viewer| {
                    crate::map::houses::is_allied_with(
                        sim.house_alliances().as_map(),
                        viewer,
                        sim.interner().resolve(entity.owner()),
                    )
                });
                // Buildings' convenience rows (repair/reload) are drawn once
                // by build_building_status_instances, which owns the building
                // status anchor. Drawing them here as well produced two repair
                // countdowns over a captured-and-repairing neutral building.
                // This TechnoExtras path is for foot/air units only.
                if let Some(shift) = info_shift
                    && convenience_info_lines_admitted(
                        match_state.match_presentation.convenience_help,
                        viewer_allied,
                        entity.category(),
                    )
                {
                    let font = &renderer.bit_font;
                    let camera = [match_state.input.camera_x, match_state.input.camera_y];
                    let coord = entity.hud.get_coords;
                    let (base_x, base_y) =
                        crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
                    let tint = [1.0_f32, 1.0, 1.0];
                    let line_height = font.cell_height() as f32 + 2.0;
                    for (row, text) in entity.hud.info_lines().enumerate() {
                        let (x, y) = crate::app::presentation::health_templates::shifted(
                            (base_x - 4.0, base_y + 10.0 + row as f32 * line_height),
                            shift,
                        );
                        let origin = [x, y];
                        let measure = font.compute_text_rect(
                            &text,
                            [
                                (origin[0] - camera[0]).round() as i32,
                                (origin[1] - camera[1]).round() as i32,
                            ],
                            0x149,
                            [4, 2],
                        );
                        status.info_text.push(
                            crate::app::presentation::ui_overlays::status_text_instances(
                                font, &text, origin, measure, camera, tint,
                            ),
                        );
                    }
                }
                // Phobos DigitalDisplay: built here because the GAME.FNT atlas
                // lives on the renderer; the value/position owner is
                // presentation::digital_display.
                #[cfg(feature = "phobos")]
                if match_state.match_presentation.digital_display_enabled
                    && let Some(rules) = crate::app::state::active_rules(frontend, match_state)
                    && let Some(offset) = digital_shift
                {
                    let digital =
                        crate::app::presentation::digital_display::digital_display_instances(
                            sim,
                            entity,
                            rules,
                            overlay,
                            &renderer.bit_font,
                            hovered,
                            local_owner.as_deref(),
                            crate::app::presentation::ui_overlays::StatusViewport {
                                camera: [match_state.input.camera_x, match_state.input.camera_y],
                                size: [sw, sh],
                                zoom: match_state.input.zoom_level,
                                ignore_visibility: match_state.sandbox_full_visibility,
                            },
                            true,
                        );
                    let mut digital = digital;
                    crate::app::presentation::health_templates::shift_instances(
                        digital
                            .texts
                            .iter_mut()
                            .flat_map(|text| text.rectangles.iter_mut().chain(text.text.iter_mut()))
                            .chain(digital.shapes.iter_mut().map(|(_, shape)| shape)),
                        offset,
                    );
                    status.digital_text = digital.texts;
                    status.digital_shapes = digital.shapes;
                }
                if !entity.sinking.is_active() && !entity.passenger_role.is_inside_transport() {
                    if let (Some(frame), Some(shape)) = (
                        match_state
                            .match_presentation
                            .talk_bubble
                            .frame_for(entity.stable_id(), speech_wall_ms),
                        overlay.talk_bubble(),
                    ) {
                        let coord = entity.hud.get_coords;
                        let point = crate::util::lepton::absolute_leptons_to_screen(
                            coord.x, coord.y, coord.z,
                        );
                        use crate::app::presentation::health_templates::{Mark, mark, shifted};
                        status.talk_bubble = mark(Mark::TalkBubble, None, frame)
                            .and_then(|(frame, offset)| {
                                shape.instance(frame, shifted(point, offset), 0.0)
                            })
                            .map(|mut instance| {
                                instance.palette_light =
                                    crate::render::palette_light::PaletteLight::plain(53, 1000);
                                instance
                            });
                    }
                }
                status.hide_families(hidden_native);
                status.shift_owner(
                    match_state
                        .match_presentation
                        .unit_motion
                        .offset(entity.stable_id()),
                );
                status
            });
        }
    }
    instances::build_weapon_wave_instances(match_state, &mut planned_objects, display_order);
    if let Some(frame) = match_state.sim_runtime.frame() {
        instances::assign_tether_redraws(frame, &mut planned_objects);
    }
    // VERA's translucent fog lifts the A buffer's near-black concealment, so
    // live enemy objects the local viewer cannot see leave the draw plan fully
    // (body, shadow, attached anim parts and extras) instead of showing through
    // the fog. Terrain, overlays and revealed map art stay visible.
    if let (Some(rt), Some(frame)) = (
        match_state.sim_runtime.presentation(),
        match_state.sim_runtime.frame(),
    ) {
        let sim = &rt.simulation;
        if let Some(viewer) = instances::translucent_fog_viewer(platform, match_state, sim) {
            let rules = &rt.resources.rules;
            let fog_of_war = sim.session().game_options().fog_of_war;
            let concealed = |entity: &crate::app::presentation::frame::ObjectRecord| {
                instances::translucent_fog_conceals(sim, viewer, &entity.position, entity.owner())
            };
            let members = frame.display_members(sim);
            let trace = match_state
                .match_diagnostics
                .traces_actor_draws()
                .then_some(&match_state.match_diagnostics);
            planned_objects.retain(|parent| {
                let id = parent.parent.id;
                if let Some(entity) = frame.object(id) {
                    if concealed(entity) {
                        match_state.match_presentation.unit_motion.conceal(id);
                        if let Some(trace) = trace {
                            trace.discard_actor_draw(id);
                        }
                        return false;
                    }
                    return true;
                }
                // Particles owned by an object follow it out of the fog.
                if let Some(place) = members.particle_place(id) {
                    let owner = sim
                        .particle_systems()
                        .display_particle_at(id, place)
                        .and_then(|(system, _)| system?.owner_entity());
                    return owner.is_none_or(|owner_id| {
                        frame.object(owner_id).is_none_or(|e| !concealed(e))
                    });
                }
                let Some(anim) = sim.anim(id) else {
                    return true;
                };
                // Building-attached AnimClass parts (idle components, damage
                // fire) follow their owner object out of the fog, whether the
                // attachment is the Anim object pointer or a Building slot.
                if let Some(owner_id) = anim
                    .owner_entity()
                    .or_else(|| anim.building_slot().map(|(owner, _)| owner))
                {
                    return frame
                        .object(owner_id)
                        .is_none_or(|entity| !concealed(entity));
                }
                // Standalone effect (explosion, fire, smoke): hidden over an
                // unseen cell unless rules declare it as a superweapon effect.
                let name = sim.interner().resolve(anim.type_id());
                if instances::is_superweapon_effect_anim(name, &rules.general) {
                    return true;
                }
                let Some(coord) = frame.anim_coord(id) else {
                    return true;
                };
                let cell = instances::lepton_cell(coord.x, coord.y);
                !instances::translucent_fog_conceals_cell(
                    Some(viewer),
                    Some(sim.fog()),
                    cell.0,
                    cell.1,
                    fog_of_war,
                )
            });
        }
    }
    // Native +99 is set before a Building's body opportunity, independently
    // of successful pixels. Empty body frames still have a retained parent.
    let building_body_opportunities = planned_objects
        .iter()
        .filter(|parent| parent.is_building())
        .map(|parent| parent.parent.id)
        .collect();
    let building_status_text = build_building_status_instances(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
        &building_body_opportunities,
    );
    if let Some(rt) = match_state.sim_runtime.presentation() {
        display_order.apply_unit_motion(
            &mut planned_objects,
            &match_state.match_presentation.unit_motion,
            &rt.simulation,
        );
    }
    let mut object_layers = super::draw_plan_lowering::lower_object_instances(planned_objects);
    let mut fogged_objects = instances::build_fogged_instances(
        platform,
        renderer,
        &frontend.screen,
        match_state,
        persistence,
    );
    // Bit_Blit seeds a raw frame, and a Unit composite, from its rect clipped
    // to the view (`native_z::Z_GRADIENT_RAW_FRAME_FLAG`,
    // `native_z::unit_composite`), which ends this far down.
    let view_height = crate::app::input::camera::tactical_view_height(
        platform,
        renderer,
        &frontend.screen,
        match_state,
        persistence,
    );
    for instance in object_layers
        .iter_mut()
        .flat_map(|layer| &mut layer.instances)
        .chain(&mut fogged_objects.instances)
        .chain(&mut cell_bodies)
    {
        if instance.z_gradient & crate::render::native_z::Z_GRADIENT_RAW_FRAME_FLAG != 0
            || crate::render::native_z::unit_composite(instance.z_gradient)
        {
            instance.draw_state.fx_params[3] = view_height;
        }
    }

    // PixelFX water/ore sparkles — per-frame 1-pixel cell dots. The draw
    // steps the cells beside the world it reads, then hands them back.
    let mut pixel_fx = std::mem::take(&mut match_state.match_presentation.pixel_fx);
    let cell_sparkles: Vec<SpriteInstance> =
        build_pixel_fx_sparkle_instances(frontend, match_state, &mut pixel_fx, sw, sh);
    match_state.match_presentation.pixel_fx = pixel_fx;

    // One-time first-frame statistics.
    static LOGGED: std::sync::Once = std::sync::Once::new();
    LOGGED.call_once(|| {
        let total_grid: usize = match_state.match_presentation.terrain_grid.as_ref()
            .map_or(0, |g| g.cells.len());
        log::info!(
            "First frame: {} terrain tiles (of {} cells) + {} fixed overlays + {} retained object sprites",
            terrain.normal.len(),
            total_grid,
            cell_bodies.len(),
            object_layers
                .iter()
                .map(|layer| layer.instances.len())
                .sum::<usize>(),
        );
    });

    // The status templates' layer reads the frame the strips read.
    let module_field = match (
        match_state.ui_modules.as_ref(),
        match_state.sim_runtime.presentation(),
        match_state.sim_runtime.frame(),
    ) {
        (Some(ui), Some(runtime), Some(frame)) => {
            let sim = &runtime.simulation;
            let local_owner = crate::app::input::commands::preferred_local_owner_name(match_state);
            let view = crate::app::ui_modules::field::FieldView {
                unit_motion: &match_state.match_presentation.unit_motion,
                sim,
                frame,
                rules: crate::app::state::active_rules(frontend, match_state),
                player: local_owner.as_deref().and_then(|name| sim.interner().get(name)),
                hovered: crate::app::presentation::ui_overlays::health_hover_target(hover),
                viewport: crate::app::presentation::ui_overlays::StatusViewport {
                    camera: [match_state.input.camera_x, match_state.input.camera_y],
                    size: [sw, sh],
                    zoom: match_state.input.zoom_level,
                    ignore_visibility: match_state.sandbox_full_visibility,
                },
            };
            crate::app::ui_modules::field::build(ui, &view, &renderer.bit_font)
        }
        _ => Default::default(),
    };

    WorldInstances {
        module_field,
        terrain,
        cell_bodies,
        cell_body_draws,
        object_layers,
        fogged_objects,
        building_status_text,
        smudge,
        smudge_cuts,
        bridge_body_shadow,
        particle_draws,
        cell_sparkles,
        spans: WorldSpans {
            terrain: terrain_span,
            objects: objects_span,
        },
    }
}

/// Build PixelFX sparkle instances by calling into the dedicated render module.
/// Assembles the SparkleInput from the match, steps `cells` and returns the
/// Vec. The module itself gates on the extra_animations toggle; the wrapper
/// short-circuits when required sim/render state is missing (no map loaded).
fn pixel_fx_enabled_for_detail_level(detail_level: u32) -> bool {
    // Retail provenance: `DrawPixelFXSparkles @ 0x006D7840` tests
    // `g_ExtraAnimationsEnabled @ 0x00A8EB78` for nonzero. The write/read
    // owners bind that global to `OptionsClass::DetailLevel +0x18` in
    // `OptionsClass__ApplyFromInGameDialog @ 0x004E1DE0` and
    // `OptionsClass__ReadFromINI @ 0x005FA620`.
    detail_level != 0
}

fn build_pixel_fx_sparkle_instances(
    frontend: &FrontendState,
    match_state: &MatchState,
    cells: &mut crate::render::pixel_fx_sparkles::PixelFxCells,
    sw: f32,
    sh: f32,
) -> Vec<SpriteInstance> {
    use crate::render::pixel_fx_sparkles::{SparkleInput, build_sparkle_instances};

    // F10 cone entry: read-only assembly consumes the runtime through the
    // view; fields without getters use the escape hatch until their cone.
    let Some(view) = match_state.sim_view() else {
        return Vec::new();
    };
    let sim = view.simulation();
    let Some(resolved) = match_state.terrain_template() else {
        return Vec::new();
    };
    let Some(overlay_registry) = crate::app::state::active_overlay_registry(frontend, match_state)
    else {
        return Vec::new();
    };
    let Some(overlays) = sim.overlay_grid() else {
        return Vec::new();
    };
    let Some(map_size) = sim.map_size_diamond() else {
        return Vec::new();
    };

    // gamemd-derived: the `DAT_00A8EB78 != 0` PixelFX gate is the process
    // `[Options] DetailLevel` field (`OptionsClass + 0x18`). The in-game state
    // is the live projection of that retained profile value, so config.toml is
    // not a second interactive authority for the same retail option.
    // `DrawPixelFXSparkles @ 0x006D7840` first asks `Game__GetMinFrameRate
    // @ 0x0055AF60` (`0x006D7847`, the latch Anim and LaserDraw share) and
    // draws nothing while the published rate is below it (`0x006D784C`).
    let frame_rate_holds = {
        let mut detail = match_state.match_presentation.detail.borrow_mut();
        let minimum = detail.minimum_frame_rate();
        minimum <= detail.frame_rate()
    };
    let enable_extra_animations = frame_rate_holds
        && pixel_fx_enabled_for_detail_level(
            match_state.match_presentation.in_game_options.detail_level,
        );

    let local_owner_name = crate::app::input::commands::preferred_local_owner_name(match_state);
    let local_owner_id = match (match_state.sandbox_full_visibility, &local_owner_name) {
        (false, Some(owner)) => sim.interner().get(owner),
        _ => None,
    };

    let input = SparkleInput {
        wall_ms: crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32,
        enable_extra_animations,
        local_owner_id,
        sandbox_full_visibility: match_state.sandbox_full_visibility,
        resolved_terrain: resolved,
        overlays,
        overlay_registry,
        occupancy: sim.occupancy(),
        fog: sim.fog(),
        camera_x: match_state.input.camera_x,
        camera_y: match_state.input.camera_y,
        viewport_w: sw,
        viewport_h: sh,
        map_size,
        white_uv_origin: [0.0, 0.0],
        white_uv_size: [1.0, 1.0],
    };
    build_sparkle_instances(&input, cells)
}

/// Build static smudge decal instances for the current frame.
///
/// Pulls the SmudgeGrid + SmudgeTypeRegistry from the active simulation and
/// hands them to `render::smudge::build_visible_instances` along with a
/// closure that resolves SmudgeType id + frame index to atlas UVs.
///
/// The atlas stores each smudge SHP as one composite frame. The render helper
/// uses the resolved terrain level at the footprint origin and emits that
/// frame once on the coherent flat uniform-material route. Native recenters
/// a separate draw from every occupied cell; unequal material remains open.
fn build_smudge_instances(
    frontend: &FrontendState,
    match_state: &MatchState,
    sw: f32,
    sh: f32,
) -> crate::render::smudge::VisibleSmudges {
    let (sim, rules) = match (
        match_state
            .sim_runtime
            .presentation()
            .map(|rt| &rt.simulation),
        crate::app::state::active_rules(frontend, match_state),
    ) {
        (Some(s), Some(r)) => (s, r),
        _ => return Default::default(),
    };
    let Some(grid) = sim.smudge_grid() else {
        return Default::default();
    };
    let Some(resolved_terrain) = sim.resolved_terrain() else {
        return Default::default();
    };
    let Some(atlas) = match_state.match_presentation.overlay_atlas.as_ref() else {
        return Default::default();
    };
    // Resolve (smudge_type_id, frame_offset) → atlas placement.
    // Smudge SHPs are registered into the OverlayAtlas under
    // `crate::render::overlay_atlas::SMUDGE_KEY_PREFIX` at map-load time
    // (see render/overlay_atlas.rs render_smudge_sprite). Frame is always 0
    // because gamemd draws every footprint cell with frame 0 and shifts
    // screen position back to footprint origin — render-side handles the
    // shift cancellation by skipping non-origin cells inside
    // build_visible_instances.
    // Each visible smudge cell asks for its type's art: build and hash a
    // type's name key once a frame.
    let placements = std::cell::RefCell::new(vec![None; rules.smudge_types.len()]);
    let lookup = |type_id: u16, _frame: u8| -> Option<TilePlacement> {
        *placements
            .borrow_mut()
            .get_mut(usize::from(type_id))?
            .get_or_insert_with(|| {
                let def = rules.smudge_types.get(type_id)?;
                let entry = atlas.get(&crate::render::overlay_atlas::smudge_key(&def.name))?;
                Some(TilePlacement {
                    uv_origin: entry.uv_origin,
                    uv_size: entry.uv_size,
                    pixel_size: entry.pixel_size,
                    draw_offset: [entry.offset_x, entry.offset_y],
                })
            })
    };
    crate::render::smudge::build_visible_instances(
        grid,
        &rules.smudge_types,
        resolved_terrain,
        match_state.match_presentation.lighting.grid(),
        &lookup,
        atlas.art_extent(),
        match_state.input.camera_x,
        match_state.input.camera_y,
        sw,
        sh,
    )
}

// ---------------------------------------------------------------------------
// Phase 2: Debug overlays
// ---------------------------------------------------------------------------

/// Build debug visualization instances (only when toggled on via hotkeys).
pub(super) fn build_debug_instances(
    diag: &DiagnosticsState,
    match_state: &MatchState,
    bit_font: &crate::render::bit_font::BitFont,
    sw: f32,
    sh: f32,
) -> DebugInstances {
    let foot_path = if diag.debug_show_foot_path {
        debug_overlays::build_foot_path_overlay_instances(match_state, bit_font, sw, sh)
    } else {
        debug_overlays::FootPathInstances {
            stroke: Vec::new(),
            nodes: Vec::new(),
            text: Vec::new(),
        }
    };
    DebugInstances {
        pathgrid: if diag.debug_show_pathgrid {
            debug_overlays::build_terrain_cost_overlay_instances(diag, match_state, sw, sh)
        } else {
            Vec::new()
        },
        cell_grid: if diag.debug_show_cell_grid {
            debug_overlays::build_cell_grid_overlay_instances(match_state, sw, sh)
        } else {
            Vec::new()
        },
        path: if diag.debug_show_pathgrid {
            debug_overlays::build_path_overlay_instances(match_state, sw, sh)
        } else {
            Vec::new()
        },
        foot_stroke: foot_path.stroke,
        foot_nodes: foot_path.nodes,
        foot_text: foot_path.text,
        heightmap: if diag.debug_show_heightmap {
            debug_overlays::build_heightmap_overlay_instances(match_state, sw, sh)
        } else {
            Vec::new()
        },
    }
}

// ---------------------------------------------------------------------------
// Phase 3: Minimap + UI overlays
// ---------------------------------------------------------------------------

/// Update minimap unit dots for the current frame.
pub(super) fn update_minimap(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    persistence: &PersistenceState,
    local_owner: &Option<String>,
) {
    let (playfield_bounds, playfield_revision) =
        crate::app::input::camera::playfield_presentation_authority(&match_state.sim_runtime);
    let needs_playfield_reconcile =
        match_state
            .match_presentation
            .minimap
            .as_ref()
            .is_some_and(|minimap| {
                minimap.needs_playfield_reconcile(playfield_bounds, playfield_revision)
            });
    if needs_playfield_reconcile {
        let overlay_data = crate::app::loading::transitions::build_minimap_overlay_data(
            match_state.match_presentation.overlays.as_slice(),
            &match_state.match_presentation.terrain_objects,
            crate::app::state::active_overlay_registry(frontend, match_state),
            crate::app::state::active_rules(frontend, match_state),
        );
        let runtime = match_state.sim_runtime.presentation();
        let presentation = &mut match_state.match_presentation;
        if let (Some(minimap), Some(grid)) = (
            presentation.minimap.as_mut(),
            presentation.terrain_grid.as_ref(),
        ) {
            minimap.reconcile_playfield(
                &renderer.gpu,
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

    // VERA's HD radar draws at the sidebar's whole scale while it is on; a
    // fractional surface has none (a 1.5x radar has no whole pixel grid).
    let hd_scale = persistence
        .hd_radar(platform.capture_choices())
        .then(|| {
            crate::app::state::sidebar_frame(
                platform,
                persistence,
                renderer,
                &frontend.screen,
                &match_state.match_presentation.sidebar_layout_spec,
            )
            .draw_scale()
        })
        .filter(|scale| scale.fract() == 0.0)
        .map(|scale| scale as u32);
    if let Some(minimap) = match_state.match_presentation.minimap.as_mut() {
        minimap.set_hd_scale(
            &renderer.gpu.device,
            &renderer.gpu.queue,
            &renderer.batch_renderer,
            hd_scale,
        );
    }

    let full_visibility = match_state.sandbox_full_visibility;
    let beacon_draws = crate::app::match_runtime::beacons::draws(match_state);
    let presentation = &mut match_state.match_presentation;
    if let (Some(minimap), Some(runtime), Some(frame)) = (
        presentation.minimap.as_mut(),
        match_state.sim_runtime.presentation(),
        match_state.sim_runtime.frame(),
    ) {
        let transaction = super::minimap_transaction::present_minimap_frame(minimap, |minimap| {
            // F10 cone: render-feed reads go through SimView getters. The
            // production transaction includes composition, queue upload, and
            // only then the acknowledgement of the minimap's own backlog.
            let view = runtime.view();
            Ok::<_, std::convert::Infallible>(
                minimap.update_unit_dots(
                    &renderer.gpu.device,
                    &renderer.gpu.queue,
                    frame,
                    view.logic_order(),
                    view.houses(),
                    presentation
                        .relation_color_map
                        .as_ref()
                        .unwrap_or(&presentation.house_color_map),
                    view.session().tick(),
                    view.session().binary_frame(),
                    local_owner
                        .as_deref()
                        .and_then(|owner| view.interner().get(owner)),
                    view.fog(),
                    view.house_alliances(),
                    full_visibility,
                    view.control_projection(),
                    Some(&runtime.resources.rules),
                    Some(view.interner()),
                    view.overlay_grid(),
                    &presentation.overlay_radar_colors,
                    view.resolved_terrain(),
                    presentation
                        .beacon_art
                        .as_ref()
                        .map(|art| (art, beacon_draws.as_slice())),
                    presentation.relation_color_generation,
                ),
            )
        });
        match transaction {
            Ok(_) => {}
            Err(never) => match never {},
        }
    }
}

/// Build in-game UI overlay instances: health bars,
/// drag rectangle, building placement preview, and software cursor.
/// `wall_ms` is the last tactical composite's timeGetTime: planning dashes
/// (63D8B8), mind-control links (704E40), psychic detection lines (4DC340)
/// and the airstrike wobble move with it once per composite.
pub(super) fn build_ui_instances(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    wall_ms: u32,
    cursor: &crate::app::input::cursor::CursorTargets,
) -> UiInstances {
    let (software_cursor, software_cursor_draw) = build_software_cursor_instances(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        cursor,
    );
    let drag = match &match_state.match_presentation.selection_overlay {
        Some(o) => o.build_drag_rect(
            &match_state.input.selection_state,
            match_state.input.camera_x,
            match_state.input.camera_y,
        ),
        None => Vec::new(),
    };
    // VERA formation line: its dots, in world pixels, in the Move action
    // line's colour from the palette those lines read. They lie on the
    // ground, with the ferry's circles.
    let mut ground_marks = Vec::new();
    if let (Some(path), Some(palette)) = (
        match_state.input.formation_line.path(),
        match_state
            .match_presentation
            .surface_palette(process_assets.manager()),
    ) {
        ground_marks.extend(
            crate::render::selection_overlay::SelectionOverlay::build_formation_dots(
                &path.dots(),
                crate::app::presentation::target_lines::move_action_line_color(palette),
            ),
        );
    }
    let placement_preview = build_placement_preview(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
    );

    let input = &match_state.input;
    let view = tactical_world_view(
        platform,
        renderer,
        &frontend.screen,
        match_state,
        persistence,
    );
    // The drained action stores go onto the whole logical surface, whose
    // pixels start at the frame's corner: the view's camera less the
    // battlefield's corner, which is the stored camera rounded as the view's.
    let camera = [input.camera_x, input.camera_y].map(|v| v.round() as i32);
    // All path, planning and rally pixels go onto the world surface in world
    // pixels. Its camera owns the scale (each world pixel a block of the
    // camera's zoom, `docs/zoom-one-pass.md`). Pre-scaling individual spans
    // here would quantize them twice, distorting dash spacing and node
    // borders at fractional zoom.
    let mut designations = std::collections::HashMap::new();
    if !match_state.input.planning.active()
        && let (Some(runtime), Some(frame)) = (
            match_state.sim_runtime.presentation(),
            match_state.sim_runtime.frame(),
        )
    {
        // Health 0 skips only the CaptureManager links (`0x006D47A4` jumps
        // to the airstrike check at `0x006D47FB`).
        for entity in frame.line_sources() {
            if let Some(designation) = entity.place.airstrike_designation {
                let _ = runtime;
                let style = crate::app::presentation::airstrike_lines::FlareStyle {
                    #[cfg(feature = "phobos")]
                    color: designation.color,
                    #[cfg(feature = "phobos")]
                    z_adjust: runtime.resources.rules.general.airstrike_line_z_adjust,
                };
                designations.insert(entity.stable_id(), (designation.coords, style));
            }
        }
    }
    let interleave_enabled = !designations.is_empty();
    // DSurface4C0750's dashed stores read the A plane on the GPU, whose
    // pixels (like the surface lines') start at the floored camera.
    let a_offset = [
        camera[0] - input.camera_x.floor() as i32,
        camera[1] - input.camera_y.floor() as i32,
    ];
    let mut tactical_line_calls = Vec::new();
    let mut after_entity = |id, strokes: &mut Vec<SurfaceStroke>| {
        use crate::app::presentation::airstrike_lines::{TacticalLineCall, drain_opaque};
        drain_opaque(&mut tactical_line_calls, strokes, camera, a_offset);
        if let Some((coords, style)) = designations.remove(&id) {
            tactical_line_calls.push(TacticalLineCall::Designation { id, coords, style });
        }
    };
    let ground_strokes = ferry_lines::build_rings(match_state, view);
    let (mut target_line, planning_highlights) = planning_lines::build(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        view,
        wall_ms,
    );
    target_line.extend(ferry_lines::build_paths(match_state, view, wall_ms));
    let action_lines = crate::app::presentation::target_lines::build_target_line_strokes(
        Some(&match_state.match_presentation.unit_motion),
        &match_state.match_presentation.target_lines,
        match_state.input.planning.active(),
        match_state
            .sim_runtime
            .presentation()
            .map(|rt| &rt.simulation),
        match_state.sim_runtime.frame(),
        crate::app::state::active_rules(frontend, match_state),
        match_state
            .match_presentation
            .surface_palette(process_assets.manager()),
        &match_state.match_presentation.house_color_map,
        wall_ms,
        view,
        if interleave_enabled {
            Some(&mut after_entity)
        } else {
            None
        },
    );
    target_line.extend(action_lines);
    target_line.extend(planning_highlights);
    let [factory_rally_first, factory_rally_second] =
        crate::app::presentation::target_lines::build_factory_rally_line_strokes(
            match_state
                .sim_runtime
                .presentation()
                .map(|rt| &rt.simulation),
            match_state.sim_runtime.frame(),
            crate::app::state::active_rules(frontend, match_state),
            &cursor.selected,
            &match_state.match_presentation.house_color_map,
            preferred_local_owner(match_state).as_deref(),
            view,
        );

    UiInstances {
        software_cursor,
        software_cursor_draw,
        drag,
        ground_marks,
        ground_strokes,
        placement: placement_preview.cells,
        #[cfg(feature = "phobos")]
        placement_ghost: placement_preview.ghost,
        target_line,
        tactical_line_calls,
        factory_rally_first,
        factory_rally_second,
        beacons: build_beacon_instances(match_state),
    }
}

/// The one world-surface viewport for object admission, planning, action and rally producers.
/// Wheel zoom changes its clip, while the native raster keeps one-pixel spans.
pub(crate) fn tactical_world_view(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> crate::app::presentation::target_lines::TacticalViewport {
    let input = &match_state.input;
    let (x, y, width, height) = crate::app::input::camera::tactical_viewport_px(
        platform,
        renderer,
        screen,
        &match_state.match_presentation,
        persistence,
    );
    // The world camera carries the wheel zoom (`docs/zoom-one-pass.md`), so
    // the view is in world pixels: the frame's battlefield divided by the
    // zoom. At zoom 1 it is the frame's.
    tactical_world_viewport(
        [input.camera_x, input.camera_y],
        world_pixels([x, y, width, height], input.zoom_level),
    )
}

/// The frame rect `[x, y, width, height]` in the world pixels a world camera
/// at `zoom` shows there: its edges floored and ceiled outwards, so it covers
/// every world pixel part of it shows.
fn world_pixels([x, y, width, height]: [u32; 4], zoom: f32) -> [u32; 4] {
    let left = (x as f32 / zoom).floor() as u32;
    let top = (y as f32 / zoom).floor() as u32;
    let right = ((x + width) as f32 / zoom).ceil() as u32;
    let bottom = ((y + height) as f32 / zoom).ceil() as u32;
    [left, top, right - left, bottom - top]
}

/// [`tactical_world_view`] of a stored `camera` and `tactical` rect in world
/// pixels.
pub(crate) fn tactical_world_viewport(
    camera: [f32; 2],
    tactical: [u32; 4],
) -> crate::app::presentation::target_lines::TacticalViewport {
    let clip = tactical.map(|v| v as i32);
    // The view is in the battlefield's own pixels: its camera is the world
    // point at the battlefield's corner, the stored camera (the frame
    // corner's) in by the clip's corner, and its clip starts at 0, 0.
    crate::app::presentation::target_lines::TacticalViewport {
        camera: [
            camera[0].round() as i32 + clip[0],
            camera[1].round() as i32 + clip[1],
        ],
        clip: [0, 0, clip[2], clip[3]],
    }
}

/// `BeaconManagerClass` Draw `0x00430AC0`, called by TacticalClass::Draw at
/// `0x006D46B6` just before BandBox.
fn build_beacon_instances(match_state: &MatchState) -> Vec<Vec<SpriteInstance>> {
    let (Some(art), Some(runtime)) = (
        match_state.match_presentation.beacon_art.as_ref(),
        match_state.sim_runtime.presentation(),
    ) else {
        return Vec::new();
    };
    let mut rows = vec![Vec::new(); art.house_count()];
    let frame = runtime.simulation.session().binary_frame();
    for beacon in crate::app::match_runtime::beacons::draws(match_state) {
        if let (Some(row), Some(instance)) = (
            rows.get_mut(beacon.house),
            art.tactical_instance(&beacon, frame),
        ) {
            row.push(instance);
        }
    }
    rows
}

/// The PLACE cells and, in Phobos builds, the preview artwork drawn after them.
struct PlacementPreviewInstances {
    cells: Vec<SpriteInstance>,
    #[cfg(feature = "phobos")]
    ghost: Vec<SpriteInstance>,
}

/// The current and pending PLACE cells. Tactical6D5030 suppresses the
/// building ghost outside the map editor; Cell47EC90 supplies frame0, slope+2
/// or the pending frame1. Keep one ordered stream rather than regrouping
/// overlapping cells by admitted color. `process_assets` serves the Phobos
/// ghost only.
#[cfg_attr(not(feature = "phobos"), allow(unused_variables))]
fn build_placement_preview(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
) -> PlacementPreviewInstances {
    let mut out = PlacementPreviewInstances {
        cells: Vec::new(),
        #[cfg(feature = "phobos")]
        ghost: Vec::new(),
    };
    let current = match_state.input.building_placement_preview.as_ref();
    let cursor_in = crate::app::input::camera::cursor_over_battlefield(
        platform,
        renderer,
        &frontend.screen,
        match_state,
        persistence,
    );
    let draws = crate::render::selection_overlay::placement_cell_draws(
        current,
        match_state.input.pending_building_placement_preview(),
        cursor_in,
    );
    let shape = match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .and_then(|overlay| overlay.placement_shape());
    // Residual: native6D53CB suppresses auto-fill when the wall is blocked;
    // the production list still includes those cells. The ordinary2x2 path
    // has no filler cells and shares one current-cell presentation transport.
    // Phobos `0x47EFAE` selects the grid translucency from the rules global
    // plus the client switch, independently of the per-type preview gate.
    #[cfg(feature = "phobos")]
    let alpha = crate::app::state::active_rules(frontend, match_state).map_or(1.0, |rules| {
        rules
            .placement_preview_globals()
            .effective_grid_translucency(match_state.match_presentation.show_placement_preview)
            .opacity()
    });
    #[cfg(not(feature = "phobos"))]
    let alpha = 1.0;
    if let Some(shape) = shape {
        out.cells = shape.build_building_preview(&draws, alpha);
    }
    // The ghost (`0x6D528A`) follows the current grid, inside the same
    // tactical-rect gate.
    #[cfg(feature = "phobos")]
    if let Some(preview) = current.filter(|_| cursor_in) {
        out.ghost = build_placement_ghost(renderer, frontend, match_state, process_assets, preview);
    }
    out
}

/// Phobos `TacticalClass_DrawPlacement_PlacementPreview 0x6D528A`: the
/// per-type preview image at the current placement cell. The rules global,
/// the client switch and the type's own `PlacementPreview=` must all admit it.
#[cfg(feature = "phobos")]
fn build_placement_ghost(
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    preview: &crate::sim::production::BuildingPlacementPreview,
) -> Vec<SpriteInstance> {
    let Some(rules) = crate::app::state::active_rules(frontend, match_state) else {
        return Vec::new();
    };
    let globals = rules.placement_preview_globals();
    if !globals.enabled || !match_state.match_presentation.show_placement_preview {
        return Vec::new();
    }
    let Some(type_id) = match_state.input.armed_building_type() else {
        return Vec::new();
    };
    let Some(object) = rules.object(type_id) else {
        return Vec::new();
    };
    if !object.placement_preview() {
        return Vec::new();
    }
    let Some(point) = crate::app::presentation::placement_preview::preview_screen_point(
        preview,
        object.placement_preview_offset(),
    ) else {
        return Vec::new();
    };
    let Some(assets) = process_assets.manager() else {
        return Vec::new();
    };
    let local_owner = crate::app::input::commands::preferred_local_owner_name(match_state);
    // The placement ghost is a building of the local player's, so it follows
    // the relation colours like the finished building will.
    let remap_house = local_owner
        .as_deref()
        .and_then(|owner| {
            match_state
                .match_presentation
                .effective_house_color_map()
                .get(owner)
        })
        .copied();
    let translucency = object
        .placement_preview_translucency()
        .unwrap_or(globals.translucency);
    let presentation = &match_state.match_presentation;
    let mut art = presentation.placement_preview_art.borrow_mut();
    art.build_instance(
        &renderer.gpu.device,
        &renderer.gpu.queue,
        &renderer.batch_renderer,
        assets,
        rules,
        type_id,
        object,
        &presentation.theater_name,
        &presentation.theater_ext,
        remap_house,
        point,
        translucency.opacity(),
    )
    .into_iter()
    .collect()
}

// ---------------------------------------------------------------------------
// Phase 4: Sidebar
// ---------------------------------------------------------------------------

/// Build sidebar UI instances: chrome, cameos, text, minimap, viewport rect, radar animation.
/// The minimap's name in the sidebar's tree.
const MINIMAP_AREA: &str = "minimap";

pub(super) fn build_sidebar_instances(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
) -> SidebarInstances {
    // Nothing below changes the retained view; the minimap build and the
    // font raster switches write other fields, so the view is read again
    // after each instead of being copied.
    let view_revision = current_sidebar_view(&match_state.match_presentation).map(|_| {
        match_state
            .match_presentation
            .sidebar_projection
            .view_revision()
    });
    let minimap_rect =
        active_minimap_screen_rect(platform, renderer, frontend, match_state, persistence);
    let (tactical_w, tactical_h) = crate::app::input::camera::tactical_viewport_size(
        platform,
        renderer,
        &frontend.screen,
        &match_state.match_presentation,
        persistence,
    );
    let tactical_center_cell = crate::app::input::camera::tactical_centre_cell(
        platform,
        renderer,
        &frontend.screen,
        match_state,
        persistence,
    );
    // The side text colour the layout gives (`[Money] Color=`).
    let sidebar_color = crate::render::sidebar_text::native_outline_from(
        match_state.match_presentation.sidebar_layout_spec.side_text,
    );
    // Native g_SidebarSurface is 168 x screen_height in surface-local space;
    // SidebarClass::BlitToScreen @ 0x006A70E0 translates that retained surface
    // to the right-sidebar destination without changing the copied extent.
    // The live view panel is the corresponding outer UI-scaled screen frame.
    let sidebar_surface = current_sidebar_view(&match_state.match_presentation).map(|view| {
        [
            view.panel_rect.x,
            view.panel_rect.y,
            view.panel_rect.w,
            view.panel_rect.h,
        ]
    });

    // Only show minimap when radar is online (or no radar_anim = legacy fallback).
    let minimap_visible =
        crate::app::presentation::sidebar_render::minimap_shown(&match_state.match_presentation);

    let (minimap, viewport_rect, content_boundary) = if minimap_visible {
        let markers = match_state.ui_modules.as_ref().map(|ui| {
            let sim = match_state.sim_runtime.presentation().map(|rt| &rt.simulation);
            let frame = match_state.sim_runtime.frame();
            let player = crate::app::input::commands::preferred_local_owner_name(match_state);
            let player = sim.and_then(|sim| player.as_deref().and_then(|name| sim.interner().get(name)));
            let rules = crate::app::state::active_rules(frontend, match_state);
            crate::app::ui_modules::field::radar_markers(ui, |id| {
                let entity = frame?.object(id)?;
                crate::app::presentation::ui_overlays::module_status_anchor(
                    sim?, entity, rules, player, match_state.sandbox_full_visibility,
                )?;
                Some((entity.position.rx(), entity.position.ry()))
            })
        });
        match &mut match_state.match_presentation.minimap {
            Some(mm) => {
                let quad = mm.build_minimap_instance_in_rect(
                    match_state.input.camera_x,
                    match_state.input.camera_y,
                    minimap_rect.x,
                    minimap_rect.y,
                    minimap_rect.w,
                    minimap_rect.h,
                );
                // The radar's picture is a host area of the sidebar's tree,
                // its node over the whole pixels the picture covers.
                let area = crate::app::presentation::picture_hud::covering(
                    minimap_rect.x,
                    minimap_rect.y,
                    minimap_rect.w,
                    minimap_rect.h,
                );
                let minimap = match_state
                    .match_presentation
                    .minimap_hud
                    .draw(MINIMAP_AREA, Some((area, quad)))
                    .to_vec();
                let mut viewport_rect = sidebar_surface.map_or_else(Vec::new, |sidebar_surface| {
                    mm.build_viewport_rect_in_rect(
                        match_state.input.camera_x,
                        match_state.input.camera_y,
                        tactical_center_cell,
                        tactical_w as i32,
                        tactical_h as i32,
                        match_state.input.zoom_level,
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
                        match_state.input.camera_x,
                        match_state.input.camera_y,
                        minimap_rect.x,
                        minimap_rect.y,
                        minimap_rect.w,
                        minimap_rect.h,
                        sidebar_surface,
                        sidebar_color,
                    )
                });
                // VERA: the client modules' map markers on the radar.
                if let Some(markers) = markers {
                    viewport_rect.extend(mm.build_marker_dots_in_rect(
                        match_state.input.camera_x,
                        match_state.input.camera_y,
                        minimap_rect.x,
                        minimap_rect.y,
                        minimap_rect.w,
                        minimap_rect.h,
                        markers,
                    ));
                }
                (minimap, viewport_rect, content_boundary)
            }
            None => {
                match_state
                    .match_presentation
                    .minimap_hud
                    .draw(MINIMAP_AREA, None);
                (Vec::new(), Vec::new(), Vec::new())
            }
        }
    } else {
        match_state
            .match_presentation
            .minimap_hud
            .draw(MINIMAP_AREA, None);
        (Vec::new(), Vec::new(), Vec::new())
    };

    // The bottom bar draws into a surface of its own.
    let mut command_bar = if current_sidebar_view(&match_state.match_presentation).is_some() {
        build_command_bar_instances(platform, renderer, frontend, match_state, persistence)
    } else {
        Vec::new()
    };
    // The column's chrome, drawn by its nodes on the chrome layer.
    let mut chrome = build_sidebar_chrome_instances(frontend, match_state);
    // Taken before the frame's view, which borrows the state.
    let mut huds = std::mem::take(&mut match_state.match_presentation.sidebar_huds);
    let view = current_sidebar_view(&match_state.match_presentation);
    // The smooth sidebar's chrome copy packs the pieces apart from the atlas.
    let chrome_atlas =
        crate::app::presentation::sidebar_render::current_sidebar_chrome(frontend, match_state);
    let smooth_command_bar =
        chrome_atlas.is_some_and(|atlas| atlas.retarget_to_smooth(&mut command_bar));

    // The status strip's labels and Ares hook 6CC2B0's, resolved once per
    // match (`resolve_status_texts`); Ares's only when it plays in the game.
    let texts = &match_state.match_presentation.status_texts;
    let ares_texts = match_state.match_presentation.ares_status_texts.as_ref();
    let status_labels = crate::app::presentation::sidebar_build::CameoStatusLabels {
        ready: &texts.ready,
        hold: &texts.hold,
        charging: &texts.charging,
        active: &texts.active,
        ares: ares_texts,
    };
    // StripClass::Draw packs the B0FA1C side colour into the 16-bit surface
    // format before drawing its labels (6A9BF3..6A9C46).
    // The layout's `[Cameos] LabelColor=`, by default the side text colour.
    let ready_tint = crate::render::sidebar_text::native_outline_from(
        match_state
            .match_presentation
            .sidebar_layout_spec
            .label_text,
    );
    let cameo_layers = view
        .map(|v| build_sidebar_cameo_instances(renderer, frontend, match_state, v, status_labels))
        .unwrap_or_default();
    #[cfg(feature = "phobos")]
    let sw_sidebar = crate::app::presentation::sw_sidebar_build::build(
        renderer,
        frontend,
        match_state,
        &mut huds.sw,
        view.is_some(),
        status_labels,
        ready_tint,
    );
    // The column's text, at the window's pixels wherever it is drawn.
    let frame_raster = renderer.bit_font.set_raster(
        crate::app::presentation::sidebar_surface::column_text(
            platform,
            renderer,
            &frontend.screen,
            match_state,
            persistence,
        )
        .raster()
        .unwrap_or(crate::render::bit_font::TextRaster::NATIVE),
    );
    let view = current_sidebar_view(&match_state.match_presentation);
    for hud in [&mut huds.status, &mut huds.queue, &mut huds.label] {
        hud.begin();
    }
    // Without the retail FNT the native sidebar text path is unavailable.
    if let Some(v) = view.filter(|_| renderer.bit_font.darken_texture().is_some()) {
        crate::app::presentation::sidebar_build::draw_cameo_labels(
            &renderer.bit_font,
            frontend,
            match_state,
            v,
            status_labels,
            ready_tint,
            &mut huds,
        );
        if let Some(houses) = v.observer.as_deref() {
            crate::app::presentation::sidebar_build::draw_observer_strip_text(
                &mut huds.status,
                &renderer.bit_font,
                houses,
                [match_state.input.camera_x, match_state.input.camera_y],
            );
        }
    }
    // Credits counter shares the GAME.FNT sidebar-text layer: gamemd draws it
    // with the same BitFont on the sidebar surface, in the same packed side
    // text colour as the cameo labels.
    let layout_spec = match_state.match_presentation.sidebar_layout_spec;
    if let Some(v) = view.filter(|_| layout_spec.shows(crate::ui::sidebar::SidebarPart::Money)) {
        // The layout's `[Money] Value=` and `Align=`, read when it was loaded.
        let money = match_state.match_presentation.stock_parts.money;
        let shown = crate::app::presentation::layout_parts::money_value_text(match_state);
        crate::app::presentation::sidebar_text::draw_credits(
            &mut huds.status,
            &renderer.bit_font,
            v,
            layout_spec,
            // The plate is as wide as its picture.
            crate::app::presentation::sidebar_render::current_sidebar_chrome(frontend, match_state)
                .and_then(|atlas| {
                    atlas.art(
                        crate::ui::sidebar::SidebarPart::Credits,
                        crate::ui::sidebar::ArtKey::Art,
                        0,
                    )
                })
                .map_or(layout_spec.sidebar_width, |plate| plate.pixel_size[0]),
            &process_assets.csf_text("TXT_TIME_FORMAT_HOURS"),
            shown.as_deref(),
            money.align,
            // The layout's `[Money] Color=`.
            crate::render::sidebar_text::native_outline_from(layout_spec.side_text),
            [match_state.input.camera_x, match_state.input.camera_y],
        );
    }
    for hud in [&mut huds.status, &mut huds.queue, &mut huds.label] {
        hud.finish(&renderer.bit_font);
    }
    let mut status_text: Vec<SpriteInstance> = huds.status.glyphs().copied().collect();
    let queue_text: Vec<SpriteInstance> = huds.queue.glyphs().copied().collect();
    let label_text: Vec<SpriteInstance> = huds.label.glyphs().copied().collect();
    // The buttons and texts the side's layout adds to the sidebar. A
    // button's box rides the white lane of the cameo's hover outline: over
    // the chrome and the cameos, under the text.
    let placed = crate::app::presentation::layout_parts::placed(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
    );
    let layout_parts = &mut match_state.match_presentation.added_part_huds.side;
    layout_parts.draw(
        &placed,
        ready_tint,
        [match_state.input.camera_x, match_state.input.camera_y],
        &renderer.bit_font,
    );
    status_text.extend_from_slice(layout_parts.texts());
    // Their pictures draw over the sidebar's own, from the same atlas.
    chrome.extend_from_slice(layout_parts.pictures());
    renderer.bit_font.set_raster(frame_raster);
    // The observer's strip draws over the strip's tiles from the same atlas.
    chrome.extend_from_slice(
        crate::app::presentation::sidebar_build::draw_observer_strip(
            frontend,
            match_state,
            current_sidebar_view(&match_state.match_presentation),
            &mut huds.observer,
        ),
    );
    match_state.match_presentation.sidebar_huds = huds;
    let smooth_chrome =
        crate::app::presentation::sidebar_render::current_sidebar_chrome(frontend, match_state)
            .is_some_and(|atlas| atlas.retarget_to_smooth(&mut chrome));

    // Preserve the pixels actually drawn online for a possible pre-due loss.
    // This is display history, separate from ongoing internal minimap updates.
    // The radar housing's corner, where `build_radar_anim_instance` draws it.
    let [radar_x, radar_y] = crate::app::presentation::sidebar_render::current_sidebar_layout(
        platform,
        renderer,
        &frontend.screen,
        &match_state.match_presentation,
        persistence,
    )
    .origin(
        &match_state.match_presentation.sidebar_layout_spec,
        crate::ui::sidebar::SidebarPart::Radar,
    );
    let origin = [
        match_state.input.camera_x + radar_x,
        match_state.input.camera_y + radar_y,
    ];
    let presentation = &mut match_state.match_presentation;
    if let (Some(radar), Some(mm)) = (
        presentation.radar_anim.as_mut(),
        presentation.minimap.as_ref(),
    ) {
        radar.retain_online_content(
            mm.native_map_texture(),
            mm.white_texture(),
            &minimap,
            viewport_rect.iter().chain(&content_boundary).copied(),
            origin,
        );
    }
    let radar_anim = build_radar_anim_instance(
        platform,
        renderer,
        &frontend.screen,
        match_state,
        persistence,
    );

    SidebarInstances {
        chrome,
        command_bar,
        smooth_chrome,
        smooth_command_bar,
        cameo: cameo_layers.cameo,
        cameo_hover: {
            let mut hover = cameo_layers.hover;
            hover.extend_from_slice(match_state.match_presentation.added_part_huds.side.boxes());
            hover
        },
        unavailable: cameo_layers.unavailable,
        flash: cameo_layers.flash,
        gclock: cameo_layers.gclock,
        cameo_overlay: cameo_layers.overlay,
        queue_overlay: cameo_layers.queue_overlay,
        queue_text,
        label_overlay: cameo_layers.label_overlay,
        label_text,
        text: status_text,
        #[cfg(feature = "phobos")]
        sw_sidebar_chrome: sw_sidebar.chrome,
        #[cfg(feature = "phobos")]
        sw_sidebar_cameo: sw_sidebar.cameo,
        #[cfg(feature = "phobos")]
        sw_sidebar_gclock: sw_sidebar.gclock,
        #[cfg(feature = "phobos")]
        sw_sidebar_hover: sw_sidebar.hover,
        #[cfg(feature = "phobos")]
        sw_sidebar_overlay: sw_sidebar.overlay,
        #[cfg(feature = "phobos")]
        sw_sidebar_text: sw_sidebar.text,
        minimap,
        viewport_rect,
        content_boundary,
        radar_anim,
        view_revision,
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Specialized instance builders
// ---------------------------------------------------------------------------

/// Build a SpriteInstance for the animated radar chrome overlay.
/// Positioned at the same location as the static radar.shp in the sidebar chrome.
fn build_radar_anim_instance(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> Vec<SpriteInstance> {
    let ra = match &match_state.match_presentation.radar_anim {
        Some(ra) => ra,
        None => return Vec::new(),
    };

    let spec = match_state.match_presentation.sidebar_layout_spec;
    if !spec.shows(crate::ui::sidebar::SidebarPart::Radar) {
        return Vec::new();
    }
    let [x, y] = crate::app::presentation::sidebar_render::current_sidebar_layout(
        platform,
        renderer,
        screen,
        &match_state.match_presentation,
        persistence,
    )
    .origin(&spec, crate::ui::sidebar::SidebarPart::Radar);

    vec![SpriteInstance {
        position: [
            match_state.input.camera_x + x,
            match_state.input.camera_y + y,
        ],
        size: [ra.width as f32, ra.height as f32],
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        depth: 0.00048,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    }]
}

#[cfg(all(test, tests_app_presentation_render))]
mod tests {
    use super::*;

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

    #[test]
    fn techno_extras_drops_building_rows_so_repair_countdown_draws_once() {
        use crate::map::entities::EntityCategory;
        // A captured-and-repairing neutral building is now friendly and has
        // info lines; the building-status path owns its row, so this path must
        // not draw a second one.
        assert!(!convenience_info_lines_admitted(
            true,
            true,
            EntityCategory::Structure,
        ));
        // Units and aircraft still get their reload row here.
        assert!(convenience_info_lines_admitted(
            true,
            true,
            EntityCategory::Unit,
        ));
        assert!(convenience_info_lines_admitted(
            true,
            true,
            EntityCategory::Infantry,
        ));
        assert!(convenience_info_lines_admitted(
            true,
            true,
            EntityCategory::Aircraft,
        ));
        // Existing gates still hold for the other cases.
        assert!(!convenience_info_lines_admitted(
            false,
            true,
            EntityCategory::Unit,
        ));
        assert!(!convenience_info_lines_admitted(
            true,
            false,
            EntityCategory::Unit,
        ));
    }
}
