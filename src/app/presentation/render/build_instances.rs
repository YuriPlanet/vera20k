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
    active_minimap_screen_rect, build_command_bar_instances, build_sidebar_cameo_instances,
    build_sidebar_chrome_instances, build_sidebar_text_instances, current_sidebar_view,
};
use crate::app::presentation::ui_overlays::{
    build_building_status_instances, build_control_group_status_instances,
    build_software_cursor_instances,
};
use crate::map::terrain::TilePlacement;
use crate::map::theater::TileKey;
use crate::render::batch::SpriteInstance;
use crate::ui::sidebar::SidebarView;
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
    /// Static smudge decals (craters, scorches) — drawn between terrain and entities.
    pub smudge: Vec<SpriteInstance>,
    pub bridge_body_shadow: Vec<SpriteInstance>,
    /// Actual particle pieces observed by the existing map diagnostic policy.
    pub particle_draws: Option<Vec<instances::ParticleDrawObservation>>,
    /// PixelFX water/ore sparkles — 1-pixel cell dots emitted per frame.
    /// Empty when the live `[Options] DetailLevel` projection is zero.
    pub cell_sparkles: Vec<SpriteInstance>,
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
    pub drag: Vec<SpriteInstance>,
    pub placement: Vec<SpriteInstance>,
    pub target_line: Vec<SpriteInstance>,
    pub tactical_line_calls: Vec<crate::app::presentation::airstrike_lines::TacticalLineCall>,
    pub factory_rally_first: Vec<SpriteInstance>,
    pub factory_rally_second: Vec<SpriteInstance>,
    /// One stream per beacon row: each row has its own House Convert strip.
    pub beacons: Vec<Vec<SpriteInstance>>,
}

/// Sidebar chrome, cameos, text, minimap, and radar animation.
pub(super) struct SidebarInstances {
    /// The sidebar surface's layers, in layout space (`ui::sidebar::frame`).
    pub chrome: Vec<SpriteInstance>,
    /// The bottom command bar's surface layer, in layout space too.
    pub command_bar: Vec<SpriteInstance>,
    pub cameo: Vec<SpriteInstance>,
    pub unavailable: Vec<SpriteInstance>,
    pub flash: Vec<SpriteInstance>,
    pub gclock: Vec<SpriteInstance>,
    pub cameo_overlay: Vec<SpriteInstance>,
    pub queue_overlay: Vec<SpriteInstance>,
    pub queue_text: Vec<SpriteInstance>,
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
            self.text.len() + self.queue_text.len(),
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
pub(super) fn build_world_instances(
    state: &mut AppState,
    sw: f32,
    sh: f32,
    bracket_palette: Option<[u16; 2]>,
) -> WorldInstances {
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
        crate::render::terrain_instances::build_visible_instances(
            grid,
            Some(state.match_state.match_presentation.lighting.grid()),
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
            sw,
            sh,
            uv_fn,
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
    let display_order = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| {
            super::draw_plan_lowering::NativeDisplayOrder::from_display(rt.view().display_layers())
        })
        .unwrap_or_default();
    let mut planned_objects = Vec::new();
    let mut bodies = Vec::new();
    instances::build_overlay_instances(
        state,
        sw,
        sh,
        &mut bodies,
        &mut planned_objects,
        &display_order,
    );
    // High-bridge bodies read live CellClass bridge fields post-tick (NOT
    // OverlayGrid), then join the ordinary bodies in one native sweep.
    instances::bridges::build_bridge_body_instances(state, sw, sh, &mut bodies);
    let mut cell_bodies: Vec<SpriteInstance> = std::mem::take(
        &mut state
            .match_state
            .match_presentation
            .cached_cell_body_instances,
    );
    cell_bodies.clear();
    let mut cell_body_draws = Vec::new();
    instances::submit_cell_body_sweep(bodies, &mut cell_bodies, &mut cell_body_draws);
    // The shadow sweep keeps the same cell order; accepted pixels write Z.
    let mut bridge_body_shadow: Vec<SpriteInstance> = Vec::new();
    instances::bridges::build_bridge_shadow_instances(state, &mut bridge_body_shadow);

    // Smudges: static crater/scorch decals on top of terrain, under entities.
    // Registered SmudgeType SHPs share the live OverlayAtlas and Cell light
    // grid with the ordinary ground drawing consumers.
    let smudge: Vec<SpriteInstance> = build_smudge_instances(state, sw, sh);

    state
        .renderer
        .vxl_pose_frame_cache
        .borrow_mut()
        .begin_frame();
    instances::build_unit_instances(state, &mut planned_objects, &display_order);
    instances::build_voxel_anim_instances(state, &mut planned_objects, &display_order);
    // A building's voxel turret remains owned by the building display call.
    // The SHP builder therefore adds it to the same contiguous Ground parent
    // instead of leaking it into an atlas-level tie.
    instances::build_shp_instances(state, &mut planned_objects, &display_order);
    // Prepare slot binding/light pieces without querying the shared FPS latch.
    // Each Anim's retained Display encounter admits and appends its pieces.
    let mut building_anim_instances = Vec::new();
    instances::build_building_anim_instances(state, &mut building_anim_instances, &display_order);
    let prepared_building_anims = building_anim_instances
        .into_iter()
        .map(|parent| (parent.parent.id, parent))
        .collect();
    let mut particle_draws = state.diagnostic_presentation_ms().map(|_| Vec::new());
    let display_inputs = state.match_state.sim_runtime.as_ref().map(|runtime| {
        let presentation = &state.match_state.match_presentation;
        let (x, y, width, height) = crate::app::input::camera::tactical_viewport_px(state);
        let local_owner = crate::app::input::commands::preferred_local_owner_name(state)
            .and_then(|name| runtime.simulation.interner().get(&name));
        instances::DisplayEffectInputs {
            interpolation: Some(&presentation.interpolation),
            sim: &runtime.simulation,
            rules: &runtime.resources.rules,
            camera: [
                state.match_state.input.camera_x,
                state.match_state.input.camera_y,
            ],
            render_size: [state.render_width(), state.render_height()],
            tactical: [x, y, width, height],
            zoom: state.match_state.input.zoom_level,
            axis: instances::depth_axis(state),
            atlas: presentation.sprite_atlas.as_ref(),
            detail: &presentation.detail,
            detail_level: presentation.in_game_options.detail_level,
            lighting: presentation.lighting.grid(),
            retained_anim_intensities: Some(presentation.lighting.anim_intensities()),
            house_colors: &presentation.house_color_map,
            hidden_super_anims: &presentation.hidden_super_anims,
            show_hidden: presentation.in_game_options.show_hidden,
            local_owner,
            observe_particles: state.diagnostic_presentation_ms().is_some(),
        }
    });
    if let Some(input) = display_inputs.as_ref() {
        particle_draws = instances::build_display_effect_instances(
            input,
            &mut planned_objects,
            &display_order,
            prepared_building_anims,
        );
    }
    instances::build_projectile_visual_instances(state, &mut planned_objects, &display_order);

    // Every dynamic pose consumer, including Bullet46B0C0, has now populated
    // the shared page. One upload precedes this frame's draw submission.
    state.renderer.vxl_pose_frame_cache.borrow_mut().upload(
        &state.renderer.gpu.device,
        &state.renderer.gpu.queue,
        &state.renderer.batch_renderer,
    );

    // Selection is read by its existing presentation producer; only parents
    // admitted by the Display/body plan receive commands. Native6D913C and
    //6D9153 run in that parent's slot, before its body at6D916C.
    if let Some(palette) = bracket_palette {
        let camera = [
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ]
        .map(|value| value.floor() as i32);
        let mut brackets =
            crate::app::presentation::selection_brackets::build_selection_bracket_lines(
                state,
                camera,
                palette,
                planned_objects.iter().map(|parent| parent.parent.id),
            );
        for parent in &mut planned_objects {
            parent.brackets = brackets.remove(&parent.parent.id);
        }
    }
    // The same admitted Display parents feed native6D9789's later sweep.
    // Shared health admission/selection/hover owners derive frame-local strips;
    // storage order and atlas identity never determine their submission order.
    let mut group_labels = build_control_group_status_instances(state, sw, sh);
    if let (Some(sim), Some(overlay)) = (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state
            .match_state
            .match_presentation
            .selection_overlay
            .as_ref(),
    ) {
        let local_owner = crate::app::input::commands::preferred_local_owner_name(state);
        let hovered = crate::app::presentation::ui_overlays::health_hover_target(
            state,
            local_owner.as_deref(),
        );
        let speech_wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        for parent in &mut planned_objects {
            parent.status = sim.entities().get(parent.parent.id).map(|entity| {
                let mut status = crate::app::presentation::ui_overlays::techno_extras_for_parent(
                    sim,
                    entity,
                    state.rules(),
                    overlay,
                    hovered,
                    local_owner.as_deref(),
                    crate::app::presentation::ui_overlays::StatusViewport {
                        camera: [
                            state.match_state.input.camera_x,
                            state.match_state.input.camera_y,
                        ],
                        size: [sw, sh],
                        ignore_visibility: state.match_state.sandbox_full_visibility,
                    },
                );
                status.group = group_labels.remove(&parent.parent.id).unwrap_or_default();
                if !entity.sinking.is_active() && !entity.passenger_role.is_inside_transport() {
                    if let (Some(frame), Some(shape)) = (
                        state
                            .match_state
                            .match_presentation
                            .talk_bubble
                            .frame_for(entity.stable_id(), speech_wall_ms),
                        overlay.talk_bubble(),
                    ) {
                        let coord = crate::sim::movement::ground_pose::object_get_coords(
                            entity,
                            sim.resolved_terrain(),
                        );
                        let point = crate::util::lepton::absolute_leptons_to_screen(
                            coord.x, coord.y, coord.z,
                        );
                        status.talk_bubble =
                            shape.instance(frame, point, 0.0).map(|mut instance| {
                                instance.palette_light =
                                    crate::render::palette_light::PaletteLight::plain(53, 1000);
                                instance
                            });
                    }
                }
                let painted =
                    crate::app::presentation::interpolation::screen_position(state, entity);
                let native = crate::render::locomotor_visual::screen_position(entity);
                status.translate_for_interpolation([painted.0 - native.0, painted.1 - native.1]);
                status
            });
        }
    }
    instances::build_weapon_wave_instances(state, &mut planned_objects, &display_order);
    // Entity body builders already sampled their anchors. Cancel the same
    // sampled height for depth and per-pixel Z rather than the authority's
    // next height; otherwise climbing units slide through their occluders.
    if let Some(rt) = state.match_state.sim_runtime.as_ref() {
        let axis = instances::depth_axis(state);
        let cache = &state.match_state.match_presentation.interpolation;
        for object in &mut planned_objects {
            if let Some(entity) = rt.simulation.entities().get(object.parent.id) {
                let dz = cache.entity_offset(entity, rt.simulation.session().binary_frame())[2];
                if dz != 0. {
                    for piece in &mut object.pieces {
                        piece.instance.z_adjust -= dz;
                        piece.instance.depth = (piece.instance.depth
                            - dz / axis.world_height.max(1.))
                        .clamp(0.001, 0.999);
                    }
                }
            }
        }
    }
    if let Some(rt) = state.match_state.sim_runtime.as_ref() {
        instances::assign_tether_redraws(&rt.simulation, &mut planned_objects);
    }
    // VERA's translucent fog lifts the A buffer's near-black concealment, so
    // live enemy objects the local viewer cannot see leave the draw plan fully
    // (body, shadow, attached anim parts and extras) instead of showing through
    // the fog. Terrain, overlays and revealed map art stay visible.
    if !state.match_state.sandbox_full_visibility {
        let viewer_name = crate::app::input::commands::preferred_local_owner_name(state);
        if let (Some(rt), Some(viewer_name)) = (
            state.match_state.sim_runtime.as_ref(),
            viewer_name.as_deref(),
        ) {
            let sim = &rt.simulation;
            let rules = &rt.resources.rules;
            let viewer = sim.interner().get(viewer_name);
            let fog_of_war = sim.session().game_options().fog_of_war;
            let concealed = |entity: &crate::sim::game_entity::GameEntity| {
                let owner = sim.interner().resolve(entity.owner());
                instances::translucent_fog_conceals(
                    Some(viewer_name),
                    sim.fog(),
                    sim.house_alliances(),
                    &entity.position,
                    owner,
                    false,
                    viewer,
                    fog_of_war,
                )
            };
            // Particles owned by an object follow it out of the fog.
            let particle_owners: std::collections::BTreeMap<u64, Option<u64>> = sim
                .particle_systems()
                .display_particles()
                .map(|(system, particle)| {
                    (
                        particle.stable_id(),
                        system.and_then(|system| system.owner_entity()),
                    )
                })
                .collect();
            planned_objects.retain(|parent| {
                let id = parent.parent.id;
                if let Some(entity) = sim.entities().get(id) {
                    return !concealed(entity);
                }
                if let Some(owner) = particle_owners.get(&id) {
                    return owner.is_none_or(|owner_id| {
                        sim.entities().get(owner_id).is_none_or(|e| !concealed(e))
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
                    return sim
                        .entities()
                        .get(owner_id)
                        .is_none_or(|entity| !concealed(entity));
                }
                // Standalone effect (explosion, fire, smoke): hidden over an
                // unseen cell unless rules declare it as a superweapon effect.
                let name = sim.interner().resolve(anim.type_id());
                if instances::is_superweapon_effect_anim(name, &rules.general) {
                    return true;
                }
                let Some(coord) = sim.anim_absolute_coord(id) else {
                    return true;
                };
                let cell = instances::lepton_cell(coord.x, coord.y);
                !instances::translucent_fog_conceals_cell(
                    viewer,
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
        .filter(|parent| parent.building_pieces.is_some())
        .map(|parent| parent.parent.id)
        .collect();
    let building_status_text = build_building_status_instances(state, &building_body_opportunities);
    let object_layers = super::draw_plan_lowering::lower_object_instances(planned_objects);
    let fogged_objects = instances::build_fogged_instances(state);

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
            "First frame: {} terrain tiles (of {} cells) + {} fixed overlays + {} retained object sprites",
            terrain.normal.len(),
            total_grid,
            cell_bodies.len(),
            object_layers
                .iter()
                .map(|layer| layer.instances.len())
                .sum::<usize>(),
        );
    }

    WorldInstances {
        terrain,
        cell_bodies,
        cell_body_draws,
        object_layers,
        fogged_objects,
        building_status_text,
        smudge,
        bridge_body_shadow,
        particle_draws,
        cell_sparkles,
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
    let Some(overlays) = sim.overlay_grid() else {
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
        (false, Some(owner)) => sim.interner().get(owner),
        _ => None,
    };

    let input = SparkleInput {
        clock_ms: sim.session().total_sim_ms(),
        enable_extra_animations,
        local_owner_id,
        sandbox_full_visibility: state.match_state.sandbox_full_visibility,
        resolved_terrain: resolved,
        overlays,
        overlay_registry,
        occupancy: sim.occupancy(),
        fog: sim.fog(),
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
/// frame once on the coherent flat uniform-material route. Native recenters
/// a separate draw from every occupied cell; unequal material remains open.
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
    let Some(grid) = sim.smudge_grid() else {
        return Vec::new();
    };
    let Some(resolved_terrain) = sim.resolved_terrain() else {
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
        state.match_state.match_presentation.lighting.grid(),
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
    let foot_path = if state.diag.debug_show_foot_path {
        debug_overlays::build_foot_path_overlay_instances(state, sw, sh)
    } else {
        debug_overlays::FootPathInstances {
            stroke: Vec::new(),
            nodes: Vec::new(),
            text: Vec::new(),
        }
    };
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
        foot_stroke: foot_path.stroke,
        foot_nodes: foot_path.nodes,
        foot_text: foot_path.text,
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
        crate::app::input::camera::playfield_presentation_authority(state);
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
    let beacon_draws = crate::app::match_runtime::beacons::draws(state);
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
                    &state.renderer.gpu.queue,
                    view.entities(),
                    view.logic_order(),
                    view.houses(),
                    &presentation.house_color_map,
                    view.session().tick(),
                    view.session().binary_frame(),
                    local_owner
                        .as_deref()
                        .and_then(|owner| view.interner().get(owner)),
                    view.fog(),
                    view.house_alliances(),
                    full_visibility,
                    view.session().game_mode_nonzero(),
                    Some(&runtime.resources.rules),
                    Some(view.interner()),
                    view.overlay_grid(),
                    Some(&runtime.resources.overlay_registry),
                    &presentation.overlay_radar_colors,
                    view.resolved_terrain(),
                    radar_dirty_cells,
                    radar_dirty_generation,
                    presentation
                        .beacon_art
                        .as_ref()
                        .map(|art| (art, beacon_draws.as_slice())),
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
pub(super) fn build_ui_instances(state: &AppState, wall_ms: u32) -> UiInstances {
    let software_cursor = build_software_cursor_instances(state);
    let drag = match &state.match_state.match_presentation.selection_overlay {
        Some(o) => o.build_drag_rect(
            &state.match_state.input.selection_state,
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ),
        None => Vec::new(),
    };

    let placement = build_placement_preview(state);

    let input = &state.match_state.input;
    let camera = [input.camera_x.round() as i32, input.camera_y.round() as i32];
    let (x, y, width, height) = crate::app::input::camera::tactical_viewport_px(state);
    let view = crate::app::presentation::target_lines::TacticalViewport {
        camera,
        clip: [
            x as i32,
            y as i32,
            (width as f32 / input.zoom_level).ceil() as i32,
            (height as f32 / input.zoom_level).ceil() as i32,
        ],
        zoom: input.zoom_level,
    };
    // Shared native logical viewport; both overlays scale the raster before
    // the UI passthrough draw, so fractional zoom cannot pad individual pixels.
    let mut designations = std::collections::HashMap::new();
    if !state.match_state.input.planning.active()
        && let Some(runtime) = state.match_state.sim_runtime.as_ref()
    {
        let sim = &runtime.simulation;
        for (id, entity) in sim.entities().iter_sorted() {
            if entity.health.current() != 0
                && let Some(mut coords) =
                    sim.airstrike_designation_coordinates(id, &runtime.resources.rules)
            {
                use crate::app::presentation::interpolation::{PointKind, PointPose};
                for (i, coord) in coords.iter_mut().enumerate() {
                    let [x, y, z] = state
                        .match_state
                        .match_presentation
                        .interpolation
                        .sample_draw_point(
                            PointKind::Airstrike(i as u8),
                            id,
                            PointPose::new([coord.x, coord.y, coord.z], 0, None),
                            sim.session().binary_frame(),
                        )
                        .coord;
                    *coord = crate::sim::components::DriveCoord { x, y, z };
                }
                designations.insert(id, coords);
            }
        }
    }
    let interleave_enabled = !designations.is_empty();
    // Surface commands keep logical pixels; the shared GPU owner applies zoom.
    let line_view = crate::app::presentation::target_lines::TacticalViewport {
        zoom: if interleave_enabled { 1. } else { view.zoom },
        ..view
    };
    // The shroud A-buffer sample DSurface4C0750's dashed lines read.
    let mut shroud_alpha = |point: [i32; 2]| -> u8 {
        if state.match_state.sandbox_full_visibility {
            return 127;
        }
        state
            .match_state
            .match_presentation
            .shroud_buffer
            .as_ref()
            .and_then(|buffer| {
                buffer.sample_world(
                    (point[0] + camera[0] - view.clip[0]) as f32,
                    (point[1] + camera[1] - view.clip[1]) as f32,
                    input.camera_x,
                    input.camera_y,
                )
            })
            .unwrap_or(127)
    };
    let mut tactical_line_calls = Vec::new();
    let mut after_entity = |id, instances: &mut Vec<SpriteInstance>| {
        use crate::app::presentation::airstrike_lines::{TacticalLineCall, drain_opaque};
        drain_opaque(&mut tactical_line_calls, instances, camera);
        if let Some(coords) = designations.remove(&id) {
            tactical_line_calls.push(TacticalLineCall::Designation(coords));
        }
    };
    let (mut target_line, planning_highlights) = planning_lines::build(state, view);
    target_line.extend(
        crate::app::presentation::target_lines::build_target_line_instances_sampled(
            &state.match_state.match_presentation.target_lines,
            state.match_state.input.planning.active(),
            state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|rt| &rt.simulation),
            state.rules(),
            state
                .process_assets
                .manager()
                .and_then(|assets| assets.get_ref("palette.pal")),
            &state.match_state.match_presentation.house_color_map,
            wall_ms,
            line_view,
            if interleave_enabled {
                Some(&mut after_entity)
            } else {
                None
            },
            &mut shroud_alpha,
            Some(&state.match_state.match_presentation.interpolation),
        ),
    );
    target_line.extend(planning_highlights);
    let selected = crate::app::input::dispatch::selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    let [factory_rally_first, factory_rally_second] =
        crate::app::presentation::target_lines::build_factory_rally_line_instances_sampled(
            state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|rt| &rt.simulation),
            state.rules(),
            &selected,
            &state.match_state.match_presentation.house_color_map,
            preferred_local_owner(state).as_deref(),
            view,
            &mut shroud_alpha,
            Some(&state.match_state.match_presentation.interpolation),
        );

    UiInstances {
        software_cursor,
        drag,
        placement,
        target_line,
        tactical_line_calls,
        factory_rally_first,
        factory_rally_second,
        beacons: build_beacon_instances(state),
    }
}

/// `BeaconManagerClass` Draw `0x00430AC0`, called by TacticalClass::Draw at
/// `0x006D46B6` just before BandBox.
fn build_beacon_instances(state: &AppState) -> Vec<Vec<SpriteInstance>> {
    let (Some(art), Some(runtime)) = (
        state.match_state.match_presentation.beacon_art.as_ref(),
        state.match_state.sim_runtime.as_ref(),
    ) else {
        return Vec::new();
    };
    let mut rows = vec![Vec::new(); art.house_count()];
    let frame = runtime.simulation.session().binary_frame();
    for beacon in crate::app::match_runtime::beacons::draws(state) {
        if let (Some(row), Some(instance)) = (
            rows.get_mut(beacon.house),
            art.tactical_instance(&beacon, frame),
        ) {
            row.push(instance);
        }
    }
    rows
}

/// Active PLACE cells. Tactical6D5030 suppresses the building ghost outside
/// the map editor; Cell47EC90 supplies frame0 or slope+2. Keep one ordered
/// stream rather than regrouping overlapping cells by admitted color.
fn build_placement_preview(state: &AppState) -> Vec<SpriteInstance> {
    let (Some(shape), Some(preview)) = (
        state
            .match_state
            .match_presentation
            .selection_overlay
            .as_ref()
            .and_then(|overlay| overlay.placement_shape()),
        &state.match_state.input.building_placement_preview,
    ) else {
        return Vec::new();
    };
    // Residual: native6D53CB suppresses auto-fill when the wall is blocked;
    // the production list still includes those cells. The ordinary2x2 path
    // has no filler cells and shares one current-cell presentation transport.
    shape.build_building_preview(preview)
}

// ---------------------------------------------------------------------------
// Phase 4: Sidebar
// ---------------------------------------------------------------------------

/// Build sidebar UI instances: chrome, cameos, text, minimap, viewport rect, radar animation.
pub(super) fn build_sidebar_instances(state: &mut AppState) -> SidebarInstances {
    let view = current_sidebar_view(state).cloned();
    let minimap_rect = active_minimap_screen_rect(state);
    let (tactical_w, tactical_h) = crate::app::input::camera::tactical_viewport_size(state);
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
        .is_none_or(|ra| ra.is_minimap_visible());

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

    let chrome = view
        .as_ref()
        .map(|v| build_sidebar_chrome_instances(state, v))
        .unwrap_or_default();
    // The bottom bar draws into a surface of its own.
    let command_bar = view
        .as_ref()
        .map(|_| build_command_bar_instances(state))
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
    // SuperClass::NameReadiness6CC2B0: native drain state labels share the
    // same status-strip/text owner as ordinary Ready and Hold.
    let charging_text = state
        .process_assets
        .csf
        .as_ref()
        .map(|csf| csf.text("TXT_CHARGING"))
        .unwrap_or_else(|| std::borrow::Cow::Borrowed("Charging"));
    let active_text = state
        .process_assets
        .csf
        .as_ref()
        .map(|csf| csf.text("TXT_FIRESTORM_ON"))
        .unwrap_or_else(|| std::borrow::Cow::Borrowed("Firestorm On"));
    let status_labels = crate::app::presentation::sidebar_build::CameoStatusLabels {
        ready: ready_text.as_ref(),
        hold: hold_text.as_ref(),
        charging: charging_text.as_ref(),
        active: active_text.as_ref(),
    };
    let ready_tint = {
        let theme = crate::app::presentation::sidebar_render::current_sidebar_theme(state);
        crate::render::sidebar_text::side_highlight_color(theme)
    };
    let cameo_layers = view
        .as_ref()
        .map(|v| build_sidebar_cameo_instances(state, v, status_labels))
        .unwrap_or_default();
    let mut text_layers = view
        .as_ref()
        .map(|v| build_sidebar_text_instances(state, v, status_labels, ready_tint))
        .unwrap_or_default();
    // Credits counter shares the GAME.FNT sidebar-text layer: gamemd draws it
    // with the same BitFont on the sidebar surface, in the same packed side
    // text colour as the cameo labels.
    if let Some(v) = view.as_ref() {
        let theme = crate::app::presentation::sidebar_render::current_sidebar_theme(state);
        text_layers.status.extend(
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
        state.match_state.input.camera_x + state.sidebar_frame().layout_left(),
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
        chrome,
        command_bar,
        cameo: cameo_layers.cameo,
        unavailable: cameo_layers.unavailable,
        flash: cameo_layers.flash,
        gclock: cameo_layers.gclock,
        cameo_overlay: cameo_layers.overlay,
        queue_overlay: cameo_layers.queue_overlay,
        queue_text: text_layers.queue,
        text: text_layers.status,
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

    let (sw, sh) = state.sidebar_frame().layout_screen();
    let spec = state.match_state.match_presentation.sidebar_layout_spec;
    let layout = crate::ui::sidebar::compute_layout_with_spec(spec, sw, sh, 0);

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

#[cfg(test)]
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
}
