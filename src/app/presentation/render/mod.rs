//! In-game rendering and draw-pass orchestration.
//!
//! `render_game()` is the per-frame entry point. It runs a 7-phase pipeline:
//!
//! 1. **World instances** — terrain tiles, map overlays, bridges, VXL units,
//!    SHP buildings/infantry, their bracket commands, AnimClass objects,
//!    damage fires, fog snapshots
//! 2. **Debug instances** — pathgrid, cell grid, heightmap overlays (toggled by hotkey)
//! 3. **Shroud ABuffer** - CPU shroud buffer rebuilt before UI overlays sample it
//! 4. **UI instances** - minimap dots, health bars, placement preview
//! 5. **Sidebar instances** - chrome, cameos, text, minimap rect, radar animation
//! 6. **Upload** - all instance vectors uploaded to GPU buffer pool
//! 7. **Draw** - render pass created, draw calls dispatched in layer order
//!
//! ## Sub-modules
//! - `build_instances` — phase 1-4 builders: named functions + structs per phase
//! - `draw_passes` — phase 6: render pass creation and GPU draw call dispatch
//! - `merge_passes` — replay retained native object layers across atlas textures
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

#[cfg(test)]
mod anim_translucency_gpu_tests;
#[cfg(test)]
mod bridge_shadow_gpu_tests;
mod build_instances;
#[cfg(test)]
mod building_bracket_gpu_tests;
#[cfg(test)]
mod building_shadow_gpu_tests;
#[cfg(test)]
mod building_slot_native_gpu_tests;
mod draw_passes;
#[cfg(test)]
mod magnetic_wave_gpu_tests;
mod object_extras;
#[cfg(test)]
mod parachute_native_gpu_tests;
#[cfg(test)]
mod parachute_owner_color_gpu_tests;
#[cfg(test)]
mod placement_gpu_tests;
#[cfg(test)]
mod status_gpu_tests;
#[cfg(test)]
mod target_laser_gpu_tests;
#[cfg(test)]
pub(crate) use draw_passes::{draw_bridge_shadows, draw_pooled_cell_bodies};
pub(crate) mod draw_plan_lowering;
mod merge_passes;
mod object_stages;
#[cfg(test)]
mod object_stages_gpu_tests;
pub(crate) mod minimap_transaction;

use anyhow::Result;
#[cfg(feature = "telemetry-dashboard")]
use std::time::{Duration, Instant};

// Re-export shared types so any remaining `use crate::app::presentation::render::Foo` imports still compile.
// New code should import from `crate::app::types` directly.
pub(crate) use crate::app::types::*;

use crate::app::AppState;
use crate::app::input::commands::preferred_local_owner_name;
use crate::render::batch::InstanceBufferPool;
use crate::ui::sidebar::SidebarView;

use build_instances::{DebugInstances, SidebarInstances, UiInstances, WorldInstances};

/// Exact sidebar-related instance counts emitted by the production render path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GameRenderInstanceCounts {
    pub minimap: usize,
    pub viewport_rect: usize,
    pub radar_animation: usize,
    pub sidebar_text: usize,
}

impl GameRenderInstanceCounts {
    fn from_lengths(
        minimap: usize,
        viewport_rect: usize,
        radar_animation: usize,
        sidebar_text: usize,
    ) -> Self {
        Self {
            minimap,
            viewport_rect,
            radar_animation,
            sidebar_text,
        }
    }
}

/// Wall-clock spans of one in-game render, in `render_game`'s phase order.
///
/// Sampled by the frame owner into the diagnostics meter; readers only ever see
/// the rolling means, never an individual frame's spans.
#[cfg(feature = "telemetry-dashboard")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RenderPhases {
    /// Phase 1 — terrain, overlays, entities.
    pub world: Duration,
    /// Phase 2 — pathgrid / cell grid / heightmap overlays.
    pub debug: Duration,
    /// Phase 3 — shroud ABuffer rebuild (CPU blit + upload).
    pub shroud: Duration,
    /// Phase 4 — minimap update + UI instances.
    pub ui: Duration,
    /// Phase 5 — sidebar instances.
    pub sidebar: Duration,
    /// Phase 6 — instance upload to the GPU buffer pool.
    pub upload: Duration,
    /// Prep before phase 1 and everything after phase 6 (combat lights, terrain
    /// prep, draw records) — the part no single phase owns.
    pub other: Duration,
}

#[cfg(feature = "telemetry-dashboard")]
impl RenderPhases {
    /// The seven spans in declaration order, for the meter and the wire.
    pub(crate) fn spans(&self) -> [Duration; 7] {
        [
            self.world,
            self.debug,
            self.shroud,
            self.ui,
            self.sidebar,
            self.upload,
            self.other,
        ]
    }
}

/// Output retained from one production game render.
///
/// `sidebar_view` is the existing UI handoff. The counts are observation-only
/// evidence derived from the same vectors uploaded and dispatched this frame.
pub(crate) struct GameRenderOutput {
    pub sidebar_view: Option<SidebarView>,
    pub instance_counts: GameRenderInstanceCounts,
    pub times: GameRenderTimes,
    pub particle_draws: Option<Vec<crate::app::presentation::instances::ParticleDrawObservation>>,
    /// Phase spans of this render, for the diagnostics meter.
    #[cfg(feature = "telemetry-dashboard")]
    pub phases: Option<RenderPhases>,
}

/// Actual HUD update inputs observed by the frame owner, and the radar draw
/// input consumed here. The distinct ordinary clock epochs remain unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GameRenderTimes {
    pub radar_ms: u64,
    pub tooltip_ms: u64,
    pub message_ms: Option<u64>,
}

/// Render one in-game frame: terrain, units, overlays, UI, sidebar.
///
/// Orchestrates the 7-phase pipeline described in the module doc.
/// Each phase is a named function call — see `build_instances` for details.
pub(crate) fn render_game(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    times: GameRenderTimes,
) -> Result<GameRenderOutput> {
    // Phase accounting for the dashboard's render breakdown. One clock read per
    // phase boundary; the spans are handed back on `GameRenderOutput` and never
    // written into the world here.
    #[cfg(feature = "telemetry-dashboard")]
    let render_started = state.diag.telemetry_observer.active().then(Instant::now);
    // RadarClass::Draw 653100 consumes timeGetTime buckets. The frame owner
    // supplies wall time normally, or the map diagnostic's recorded OS input.
    if let Some(radar) = state.match_state.match_presentation.radar_anim.as_mut() {
        radar.tick(&state.renderer.gpu, times.radar_ms);
    }
    let (sw, sh) = (state.render_width() as f32, state.render_height() as f32);

    // Trigger action 0x28 mutates scroll/radar authority inside the committed
    // sim frame. Install and clamp it before constructing any world instances,
    // so contraction/expansion is visible in this very presentation frame.
    crate::app::input::camera::clamp_camera_to_playable_area(state, sw, sh);
    // Publish the final view once: world instances, CPU A admission and GPU
    // palette sampling must use this frame's clamped camera and zoom.
    state.renderer.batch_renderer.update_camera(
        &state.renderer.gpu,
        sw,
        sh,
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
        state.match_state.input.zoom_level,
        crate::app::presentation::instances::depth_axis(state),
    );

    let local_owner = preferred_local_owner_name(state);
    // Effective viewport in world pixels — zoom shrinks what's visible.
    let z = state.match_state.input.zoom_level;
    let vsw = sw / z;
    let vsh = sh / z;

    let selected_building = state
        .match_state
        .sim_runtime
        .as_ref()
        .is_some_and(|runtime| {
            runtime.simulation.entities().values().any(|entity| {
                entity.category() == crate::map::entities::EntityCategory::Structure
                    && entity.selection.is_selected()
            })
        });
    // FileSystem52BE61..52BFCE's PALETTE.PAL plain N53 Convert is shared
    // by Bolt and the selected Building's15/12 bracket colors.
    let surface_palette = (selected_building
        || !state
            .match_state
            .match_presentation
            .electric_bolts
            .is_empty())
    .then(|| {
        let bytes = state
            .process_assets
            .manager()
            .and_then(|assets| assets.get_ref("palette.pal"))
            .expect("ready tactical assets include PALETTE.PAL");
        crate::assets::pal_file::Palette::from_bytes(bytes).expect("validated retail PALETTE.PAL")
    });
    let bracket_palette = surface_palette.as_ref().map(|palette| {
        [15_u8, 12].map(|index| {
            let color = palette.color(index);
            crate::render::palette_light::PaletteLight::plain(53, 1000).rgb565(
                [color.r, color.g, color.b],
                index,
                127,
            )
        })
    });
    // Phase 1: Build game-world instances (terrain, overlays, entities).
    #[cfg(feature = "telemetry-dashboard")]
    let phase1_started = render_started.map(|_| Instant::now());
    let mut world = build_instances::build_world_instances(state, vsw, vsh, bracket_palette);
    #[cfg(feature = "telemetry-dashboard")]
    let after_world = render_started.map(|_| Instant::now());

    // Phase 2: Build debug overlay instances (pathgrid, cell grid, heightmap).
    let debug = build_instances::build_debug_instances(state, vsw, vsh);
    #[cfg(feature = "telemetry-dashboard")]
    let after_debug = render_started.map(|_| Instant::now());

    // Phase 3: Rebuild shroud ABuffer (CPU blit + GPU upload). The final
    // shared surface lines sample this CPU buffer during upload preparation.
    let rw = state.render_width();
    let rh = state.render_height();
    // F10 cone: the render feed reads through SimView getters. This site
    // holds `&mut state.shroud_buffer`, so it keeps the field chain and views
    // the runtime directly for split borrows.
    if let Some(ref mut shroud_buf) = state.match_state.match_presentation.shroud_buffer
        && !state.match_state.sandbox_full_visibility
        && let (Some(rt), Some(owner)) = (state.match_state.sim_runtime.as_ref(), &local_owner)
    {
        let view = rt.view();
        let owner_id = view.interner().get(owner).unwrap_or_default();
        shroud_buf.rebuild_if_needed(
            &state.renderer.gpu.device,
            &state.renderer.gpu.queue,
            view.fog(),
            owner_id,
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
            rw,
            rh,
            state.match_state.input.zoom_level,
        );
    }

    // One A source feeds both CPU surface admission and world palette blitters.
    // Its view identity is stable until resize/map replacement; sandbox uses
    // neutral A without rebuilding or copying the live shroud plane.
    let shroud = (!state.match_state.sandbox_full_visibility)
        .then_some(state.match_state.match_presentation.shroud_buffer.as_ref())
        .flatten();
    state
        .renderer
        .batch_renderer
        .bind_shroud(&state.renderer.gpu.device, shroud);
    #[cfg(feature = "telemetry-dashboard")]
    let after_shroud = render_started.map(|_| Instant::now());

    // Phase 4: Update minimap + build UI instances (selection, health, placement).
    build_instances::update_minimap(state, &local_owner);
    let ui = build_instances::build_ui_instances(state, times.radar_ms as u32);
    #[cfg(feature = "telemetry-dashboard")]
    let after_ui = render_started.map(|_| Instant::now());

    // Phase 5: Build sidebar instances.
    let sidebar = build_instances::build_sidebar_instances(state);
    #[cfg(feature = "telemetry-dashboard")]
    let after_sidebar = render_started.map(|_| Instant::now());

    // Phase 6: Upload all instances to GPU buffer pool.
    upload_to_gpu(state, &world, &debug, &ui, &sidebar);
    #[cfg(feature = "telemetry-dashboard")]
    let after_upload = render_started.map(|_| Instant::now());
    state
        .match_state
        .match_presentation
        .cached_cell_body_instances = world.cell_bodies;

    // A single immutable upload serves System body slots and the distinct
    // late5FFFA0 vector. Neither phase can rewrite another light's slot.
    let mut combat_lights = Vec::new();
    for layer in &mut world.object_layers {
        layer.light_record_base = combat_lights.len();
        combat_lights.extend_from_slice(&layer.light_records);
    }
    let persistent_light_start = combat_lights.len();
    {
        let presentation = &state.match_state.match_presentation;
        combat_lights.extend(
            presentation
                .combat_lights
                .draw_records(&mut presentation.detail.borrow_mut()),
        );
    }
    let persistent_lights = persistent_light_start..combat_lights.len();
    if let Some(runtime) = state.match_state.sim_runtime.as_ref() {
        state.renderer.combat_light_renderer.set_spotlight_radius(
            &state.renderer.gpu.queue,
            runtime.resources.rules.general.spotlight.radius,
        );
    }
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
    state
        .renderer
        .batch_renderer
        .update_native_z_origin(&state.renderer.gpu.queue, native_z_origin_y);
    // The sidebar and its bar draw into their own surfaces at the Sidebar
    // size.
    let sidebar_frame = state.sidebar_frame();
    let renderer = &mut state.renderer;
    let gpu = &renderer.gpu;
    if let Some(surface) = renderer.sidebar_surface.as_mut() {
        surface.set_frame(&gpu.device, &gpu.queue, sidebar_frame);
    }
    let surface = renderer.sidebar_surface.get_or_insert_with(|| {
        crate::app::presentation::sidebar_surface::SidebarSurface::new(
            &gpu.device,
            &gpu.queue,
            gpu.surface_format,
            &renderer.batch_renderer,
            sidebar_frame,
        )
    });
    surface.write_cameras(&gpu.queue, renderer.batch_renderer.ui_camera_uniform());
    surface.prepare_packed(&gpu.device, &renderer.terrain_draw_renderer);
    state.renderer.terrain_draw_renderer.prepare(
        &state.renderer.gpu.device,
        state.renderer.combat_light_renderer.composition_texture(),
        &state.renderer.depth_view,
        state.renderer.batch_renderer.camera_uniform(),
    );

    // Tactical6D4673 -> LineTrail556D40 updates once per actual composite.
    // It reads committed Bullet coordinates even when no simulation tick ran.
    // The ring belongs to presentation and survives only until it fades or loads.
    let camera = [
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
    ];
    let viewport = tactical_surface_line_viewport(state);
    let palette = surface_palette
        .as_ref()
        .map(crate::render::electric_bolt::ElectricBoltPalette::from_palette);
    let radial_selection = crate::app::input::dispatch::selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    let armed_super_weapon = state.armed_super_weapon_type().map(str::to_owned);
    let cursor_cell = armed_super_weapon.as_ref().map(|_| {
        crate::app::match_runtime::sim_tick::screen_point_to_world_cell(
            state,
            state.match_state.input.cursor_x,
            state.match_state.input.cursor_y,
        )
    });
    let sim_runtime = state.match_state.sim_runtime.as_ref();
    let simulation = sim_runtime.map(|runtime| &runtime.simulation);
    let rules = sim_runtime
        .map(|runtime| &runtime.resources.rules)
        .or(state.frontend.frontend_rules.as_ref());
    let match_presentation = &mut state.match_state.match_presentation;
    let radials = crate::app::presentation::radial_indicators::build(
        simulation,
        rules,
        &radial_selection,
        armed_super_weapon.as_deref(),
        cursor_cell,
        &mut match_presentation.superweapon_range_pulse,
        &match_presentation.house_color_map,
        viewport,
        times.radar_ms,
    );
    // EBolt4C2830 moves each owned bolt to its Unit's FLH before drawing it;
    // the owner reads take no Main draw, so they run ahead of the borrow.
    if let Some(runtime) = state.match_state.sim_runtime.as_ref() {
        let sim = runtime.view().simulation();
        let rules = &runtime.resources.rules;
        let interpolation = &state.match_state.match_presentation.interpolation;
        state
            .match_state
            .match_presentation
            .electric_bolts
            .follow_owners(|owner| {
                let point = crate::sim::combat::electric_bolt::owner_source(sim, rules, owner)?;
                let [x, y, z] = interpolation
                    .sample_draw_point(
                        crate::app::presentation::interpolation::PointKind::BoltSource,
                        owner.unit,
                        crate::app::presentation::interpolation::PointPose::new(
                            [point.x, point.y, point.z],
                            0,
                            None,
                        ),
                        sim.session().binary_frame(),
                    )
                    .coord;
                Some(crate::sim::projectile::ProjectileCoord::new(x, y, z))
            });
    }
    let presentation = &mut state.match_state.match_presentation;
    // Tactical6D4669 LaserDraw550240 precedes6D466E EBolt, then6D4673
    // trails, even when this composite ran no Logic. Their shared process
    // Main requests happen here once; GPU uploads only read resolved colors.
    let mut main = crate::app::state::process_main_draws(
        state.match_state.sim_runtime.as_mut(),
        &mut state.frontend.frontend_main_rng,
    );
    let bolt_lines = {
        presentation.lasers.composite(&mut main);
        presentation
            .electric_bolts
            .composite(viewport, palette, &mut main)
    };
    // Tactical6D48F1 follows Laser/Bolt Main requests and interleaves the
    // opaque action stores with each owning Airstrike designation. Uploads
    // consume these resolved commands and cannot take further random draws.
    let tactical_draws = crate::app::presentation::airstrike_lines::composite(
        &ui.tactical_line_calls,
        times.radar_ms as u32,
        &main,
        viewport,
    );
    let sim = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|runtime| runtime.view().simulation());
    let segments = presentation.line_trails.composite(|id| {
        let sim = sim?;
        sim.projectiles().get(id).map(|bullet| {
            presentation
                .interpolation
                .sample_point(
                    crate::app::presentation::interpolation::PointKind::Bullet,
                    id,
                    crate::app::presentation::interpolation::PointPose::bullet(bullet),
                    sim.session().binary_frame(),
                )
                .projectile_coord()
        })
    });
    let frame_rate = presentation.detail.borrow().frame_rate();
    presentation.radiation_beams.composite(frame_rate, viewport);
    let shroud = presentation.shroud_buffer.as_ref();
    let sandbox = state.match_state.sandbox_full_visibility;
    let mut alpha = |point: [i32; 2]| {
        if sandbox {
            127
        } else {
            shroud
                .and_then(|buffer| {
                    buffer.sample_world(
                        point[0] as f32 + camera[0].floor(),
                        point[1] as f32 + camera[1].floor(),
                        camera[0],
                        camera[1],
                    )
                })
                .map_or(127, u16::from)
        }
    };
    let terrain = &mut state.renderer.terrain_draw_renderer;
    let gpu = &state.renderer.gpu;
    terrain.begin_surface_lines();
    let surface_effects = terrain.prepare_surface_lines(
        &gpu.device,
        &gpu.queue,
        presentation.lasers.draws(),
        bolt_lines
            .iter()
            .copied()
            .map(crate::render::surface_line::SurfaceDraw::Line),
        segments,
        presentation.radiation_beams.draws(),
        &radials,
        viewport,
        |color| {
            if matches!(color, crate::render::laser::LaserDrawColor::House(_)) {
                let mut detail = presentation.detail.borrow_mut();
                detail.frame_rate() >= detail.minimum_frame_rate()
                    && presentation.in_game_options.detail_level != 0
            } else {
                // General550611 reads only Options.DetailLevel; its
                // explicit-color arm never queries or changes FPS policy.
                presentation.in_game_options.detail_level != 0
            }
        },
        &mut alpha,
    );
    let tactical_lines = terrain.prepare_surface_lines(
        &gpu.device,
        &gpu.queue,
        std::iter::empty(),
        tactical_draws.iter().copied(),
        &[],
        &[],
        &[],
        viewport,
        |_| false,
        &mut alpha,
    );
    for layer in &mut world.object_layers {
        layer.line_batches = layer
            .surface_draws
            .iter()
            .map(|lines| {
                terrain.prepare_surface_lines(
                    &gpu.device,
                    &gpu.queue,
                    std::iter::empty(),
                    lines.iter().copied(),
                    &[],
                    &[],
                    &[],
                    viewport,
                    |_| false,
                    &mut alpha,
                )
            })
            .collect();
    }
    for layer in &mut world.object_layers {
        for run in &mut layer.extras_runs {
            if let draw_plan_lowering::ObjectExtrasRun::Lines { lines, batch, .. } = run {
                *batch = terrain.prepare_surface_lines(
                    &gpu.device,
                    &gpu.queue,
                    std::iter::empty(),
                    layer.extras_lines[lines.clone()]
                        .iter()
                        .copied()
                        .map(crate::render::surface_line::SurfaceDraw::Line),
                    &[],
                    &[],
                    &[],
                    viewport,
                    |_| false,
                    &mut alpha,
                );
            }
        }
    }

    let ion_blasts = crate::app::presentation::ion_blasts::draws(state);
    state.renderer.terrain_draw_renderer.prepare_ion_blasts(
        &state.renderer.gpu.device,
        &state.renderer.gpu.queue,
        &ion_blasts,
        z,
    );

    // Phase 7: Dispatch draw calls in render order.
    draw_passes::dispatch_draw_passes(
        state,
        encoder,
        &composition_view,
        &draw_passes::DrawPassData {
            cell_body_draws: &world.cell_body_draws,
            bridge_shadows: &world.bridge_body_shadow,
            object_layers: &world.object_layers,
            building_status_text: &world.building_status_text,
            surface_effects,
            tactical_lines,
            persistent_lights,
            sidebar_unavailable: &sidebar.unavailable,
            sidebar_flash: &sidebar.flash,
            sidebar_gclock: &sidebar.gclock,
        },
    );
    // Prep before phase 1 plus everything after phase 6 is the `other` row:
    // combat lights and terrain prep are real work, just not a named phase.
    #[cfg(feature = "telemetry-dashboard")]
    let finished = render_started.map(|_| Instant::now());
    #[cfg(feature = "telemetry-dashboard")]
    let phases = render_started.map(|render_started| RenderPhases {
        world: after_world.unwrap().duration_since(phase1_started.unwrap()),
        debug: after_debug.unwrap().duration_since(after_world.unwrap()),
        shroud: after_shroud.unwrap().duration_since(after_debug.unwrap()),
        ui: after_ui.unwrap().duration_since(after_shroud.unwrap()),
        sidebar: after_sidebar.unwrap().duration_since(after_ui.unwrap()),
        upload: after_upload.unwrap().duration_since(after_sidebar.unwrap()),
        other: phase1_started.unwrap().duration_since(render_started)
            + finished.unwrap().duration_since(after_upload.unwrap()),
    });
    Ok(GameRenderOutput {
        instance_counts: sidebar.emitted_instance_counts(),
        sidebar_view: sidebar.view,
        times,
        particle_draws: world.particle_draws,
        #[cfg(feature = "telemetry-dashboard")]
        phases,
    })
}

/// The tactical surface object-layer DSurface stores address: logical
/// (unzoomed) pixels from the floored camera, clipped to the tactical rect.
pub(crate) fn tactical_surface_line_viewport(
    state: &AppState,
) -> crate::render::surface_line::SurfaceLineViewport {
    let z = state.match_state.input.zoom_level;
    let (x, y, w, h) = crate::app::input::camera::tactical_viewport_px(state);
    crate::render::surface_line::SurfaceLineViewport {
        camera: [
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ]
        .map(|v| v.floor() as i32),
        clip: [x, y, w, h].map(|v| (v as f32 / z) as i32),
        z_origin_y: (y as f32 / z) as i32,
        zoom: z,
    }
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
    // tooltip (O10). Prepare once: nonsilent TextLabel72A4A0 advances its
    // raw-wall reveal and requests typing audio once per successful paint.
    // Keep the row's existing opaque black background behind its glyphs.
    let (message_fill, message_text) = crate::app::input::messages::build_message_instances(state);
    let (outcome_shape, outcome_text) = crate::app::presentation::outcome_banner::instances(state);

    // A4 in-game tooltip: built before the pool borrow (it reads &AppState);
    // fill (darken texture) + text (GAME.FNT atlas), drawn after the chat
    // overlay and before the software cursor (O10).
    let (tooltip_fill, tooltip_border, tooltip_text) =
        crate::app::input::tooltips::build_tooltip_instances(state);

    // TacticalClass::Draw's timer lines, which follow the PixelFX sparkles;
    // built before the pool borrow (they step the Supers' blinks).
    let (super_timer_fill, super_timer_text) =
        crate::app::presentation::super_timers::build_super_timer_instances(state);

    let pool: &mut InstanceBufferPool = &mut state.renderer.instance_pool;

    // Debug overlays
    pool.upload(&state.renderer.gpu, "debug_pathgrid", &debug.pathgrid);
    pool.upload(&state.renderer.gpu, "debug_cell_grid", &debug.cell_grid);
    pool.upload(&state.renderer.gpu, "debug_path", &debug.path);
    pool.upload(&state.renderer.gpu, "debug_foot_stroke", &debug.foot_stroke);
    pool.upload(&state.renderer.gpu, "debug_foot_nodes", &debug.foot_nodes);
    pool.upload(&state.renderer.gpu, "debug_foot_text", &debug.foot_text);
    pool.upload(&state.renderer.gpu, "debug_heightmap", &debug.heightmap);

    // Terrain + overlays
    pool.upload(&state.renderer.gpu, "terrain", &world.terrain.normal);
    pool.upload(&state.renderer.gpu, "cell_body", &world.cell_bodies);
    pool.upload(
        &state.renderer.gpu,
        "building_status_text",
        &world.building_status_text.instances,
    );
    for (layer, objects) in world.object_layers.iter().enumerate() {
        pool.upload_page(
            &state.renderer.gpu,
            "object_layer",
            layer,
            &objects.instances,
        );
        pool.upload_page(
            &state.renderer.gpu,
            "object_extras",
            layer,
            &objects.extras_instances,
        );
    }
    pool.upload(
        &state.renderer.gpu,
        "overlay_bridge_body_shadow",
        &world.bridge_body_shadow,
    );
    // Smudges: drawn inside the terrain layer, before the cell bodies,
    // matching the native per-cell tile-then-smudge dispatch.
    pool.upload(&state.renderer.gpu, "smudge", &world.smudge);

    // Residual effects keep their existing uploads and pass ownership.
    pool.upload(&state.renderer.gpu, "cell_sparkles", &world.cell_sparkles);
    pool.upload(&state.renderer.gpu, "super_timer_fill", &super_timer_fill);
    pool.upload(&state.renderer.gpu, "super_timer_text", &super_timer_text);

    // UI overlays
    pool.upload(&state.renderer.gpu, "drag", &ui.drag);
    pool.upload(&state.renderer.gpu, "software_cursor", &ui.software_cursor);
    pool.upload(&state.renderer.gpu, "placement", &ui.placement);
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
    pool.upload(&state.renderer.gpu, "sidebar_chrome", &sidebar.chrome);
    pool.upload(&state.renderer.gpu, "command_bar", &sidebar.command_bar);
    pool.upload(&state.renderer.gpu, "radar_anim", &sidebar.radar_anim);
    pool.upload(&state.renderer.gpu, "sidebar_cameo", &sidebar.cameo);
    pool.upload(
        &state.renderer.gpu,
        "sidebar_unavailable",
        &sidebar.unavailable,
    );
    pool.upload(&state.renderer.gpu, "sidebar_flash", &sidebar.flash);
    pool.upload(&state.renderer.gpu, "sidebar_gclock", &sidebar.gclock);
    pool.upload(
        &state.renderer.gpu,
        "sidebar_queue_overlay",
        &sidebar.queue_overlay,
    );
    pool.upload(
        &state.renderer.gpu,
        "sidebar_queue_text",
        &sidebar.queue_text,
    );
    pool.upload(
        &state.renderer.gpu,
        "sidebar_cameo_overlay",
        &sidebar.cameo_overlay,
    );
    pool.upload(&state.renderer.gpu, "sidebar_text", &sidebar.text);
    pool.upload(&state.renderer.gpu, "message_text", &message_text);
    pool.upload(&state.renderer.gpu, "message_fill", &message_fill);
    pool.upload(&state.renderer.gpu, "outcome_shape", &outcome_shape);
    pool.upload(&state.renderer.gpu, "outcome_text", &outcome_text);
    pool.upload(&state.renderer.gpu, "tooltip_fill", &tooltip_fill);
    pool.upload(&state.renderer.gpu, "tooltip_border", &tooltip_border);
    pool.upload(&state.renderer.gpu, "tooltip_text", &tooltip_text);
}

#[cfg(test)]
mod tests {
    include!("../render_tests.rs");
}
