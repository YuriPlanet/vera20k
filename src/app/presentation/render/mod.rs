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
#[cfg(test)]
mod building_viewport_bounds_tests;
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
pub(crate) use draw_passes::draw_software_cursor;
#[cfg(test)]
pub(crate) use draw_passes::{draw_bridge_shadows, draw_pooled_cell_bodies};
pub(crate) mod draw_plan_lowering;
mod merge_passes;
pub(crate) mod minimap_transaction;
mod object_stages;
#[cfg(test)]
mod object_stages_gpu_tests;
pub(crate) mod terrain_picture;

use crate::util::time::{Duration, Instant};
use anyhow::Result;

// Re-export shared types so any remaining `use crate::app::presentation::render::Foo` imports still compile.
// New code should import from `crate::app::types` directly.
pub(crate) use crate::app::types::*;

use crate::app::AppState;
use crate::app::input::commands::preferred_local_owner_name;
use crate::ui::sidebar::SidebarView;

pub(crate) use build_instances::tactical_world_viewport;
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

/// Wall-clock spans of one in-game render, in `render_game`'s phase order,
/// and of handing that frame to the driver.
///
/// Sampled by the frame owner into the dashboard's meter and the performance panel's;
/// readers only ever see a window's totals or means, never one frame's spans.
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
    /// Combat lights and spotlights: their draw records and instance upload.
    pub lights: Duration,
    /// Effect lines and rings (lasers, bolts, trails, beams, range circles,
    /// target and airstrike lines) and ion blasts: the composites' Main
    /// draws, line rasters and their uploads.
    pub effects: Duration,
    /// Phase 7 — recording the frame's render passes, without the
    /// submissions made on the way (those are `handoff`).
    pub draw_calls: Duration,
    /// What handing frames to the driver cost this thread: the bounded
    /// mid-frame submissions, the send of the frame to the hand-off thread,
    /// and any wait for the frame before it.
    pub handoff: Duration,
    /// Prep before phase 1 and between the lights and the effects (sidebar
    /// surfaces, terrain targets) — the part no named span owns.
    pub other: Duration,
}

impl RenderPhases {
    pub(crate) const COUNT: usize = 11;

    /// The spans in declaration order, for the meter and the wire.
    pub(crate) fn spans(&self) -> [Duration; Self::COUNT] {
        [
            self.world,
            self.debug,
            self.shroud,
            self.ui,
            self.sidebar,
            self.upload,
            self.lights,
            self.effects,
            self.draw_calls,
            self.handoff,
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
    /// Phase spans of this render, for the dashboard's meter and the performance panel.
    pub phases: Option<RenderPhases>,
    /// The frame of this render's software cursor sprite, for a zoomed
    /// match's window pass (`window_ui::draw_battlefield`).
    pub software_cursor: Option<crate::app::presentation::ui_overlays::SoftwareCursorDraw>,
}

/// Actual HUD update inputs observed by the frame owner, and the radar draw
/// input consumed here. The distinct ordinary clock epochs remain unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GameRenderTimes {
    pub radar_ms: u64,
    pub tooltip_ms: u64,
    pub message_ms: Option<u64>,
}

/// Whether this render measures its phases: for an attached dashboard, or
/// while the performance panel shows its Most CPU row.
fn render_phases_wanted(state: &AppState) -> bool {
    #[cfg(feature = "telemetry-dashboard")]
    if state.diag.telemetry_observer.active() {
        return true;
    }
    state.renderer.performance_overlay.is_visible()
}

/// Render one in-game frame: terrain, units, overlays, UI, sidebar.
///
/// Orchestrates the 7-phase pipeline described in the module doc.
/// Each phase is a named function call — see `build_instances` for details.
pub(crate) fn render_game(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    times: GameRenderTimes,
    cadence: crate::app::presentation::tactical_composite::CompositeCadence,
) -> Result<GameRenderOutput> {
    // Phase accounting for the dashboard's render breakdown and the performance panel's
    // Most CPU row. One clock read per phase boundary; the spans are handed
    // back on `GameRenderOutput` and never written into the world here.
    let render_started = render_phases_wanted(state).then(Instant::now);
    // RadarClass::Draw 653100 consumes timeGetTime buckets. The frame owner
    // supplies wall time normally, or the map diagnostic's recorded OS input.
    let housing_settled = state
        .match_state
        .match_presentation
        .radar_anim
        .as_mut()
        .is_some_and(|radar| radar.tick(times.radar_ms));
    if housing_settled {
        crate::app::radar_movie::end_radar_sound_loop(state);
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
    let translucent_fog = state.translucent_fog_enabled();
    let sandbox = state.match_state.sandbox_full_visibility;
    // Effective viewport in world pixels — zoom shrinks what's visible.
    let z = state.match_state.input.zoom_level;
    let vsw = sw / z;
    let vsh = sh / z;

    // Wheel zoom is a presentation copy of the fully resolved world. Keep all
    // native source/depth/packed operations on the same one-texel lattice;
    // filtering each sprite (or its palette indices) loses edges and details.
    if z == 1.0 {
        state.renderer.world_zoom_pass = None;
    } else {
        let renderer = &mut state.renderer;
        let gpu = &renderer.gpu;
        let target = [sw as u32, sh as u32];
        if let Some(pass) = renderer.world_zoom_pass.as_mut() {
            pass.set_view(&gpu.device, &gpu.queue, target, z);
        } else {
            renderer.world_zoom_pass = Some(crate::render::upscale_pass::WorldZoomPass::new(
                &gpu.device,
                &gpu.queue,
                gpu.surface_format,
                target,
                z,
            ));
        }
        renderer.batch_renderer.set_native_world_resolution(
            &gpu.queue,
            renderer.world_zoom_pass.as_ref().unwrap().source_size(),
        );
    }
    let world_camera = state.renderer.batch_renderer.camera_uniform();

    let selected_building = state
        .match_state
        .sim_runtime
        .presentation()
        .is_some_and(|runtime| {
            runtime
                .simulation
                .entities()
                .values_of_category(crate::map::entities::EntityCategory::Structure)
                .any(|entity| entity.selection.is_selected())
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
        state
            .match_state
            .match_presentation
            .surface_palette(state.process_assets.manager())
            .expect("ready tactical assets include PALETTE.PAL")
    });
    let bolt_palette =
        surface_palette.map(crate::render::electric_bolt::ElectricBoltPalette::from_palette);
    let bracket_palette = surface_palette.map(|palette| {
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
    let phase1_started = render_started.map(|_| Instant::now());
    // The selection in order and the cursor's pick each walk every object;
    // the draw reads them once (TacticalPick 6DA380 runs once per frame).
    let cursor_targets = crate::app::input::cursor::CursorTargets::read(state);
    let mut world = build_instances::build_world_instances(
        state,
        vsw,
        vsh,
        bracket_palette,
        cursor_targets.hover,
    );
    for layer in &mut world.object_layers {
        object_extras::defer_health_to_frame(layer, world_camera.camera_pos, z);
    }
    let after_world = render_started.map(|_| Instant::now());

    // Phase 2: Build debug overlay instances (pathgrid, cell grid, heightmap).
    let debug = build_instances::build_debug_instances(state, vsw, vsh);
    let after_debug = render_started.map(|_| Instant::now());

    // Phase 3: Recompose the shroud ABuffer on the GPU, recorded into this
    // frame's encoder ahead of every reader: world blitters, surface lines
    // and A-gated surface stores all sample the same plane.
    let rw = state.render_width();
    let rh = state.render_height();
    // F10 cone: the render feed reads through SimView getters. This site
    // holds `&mut state.shroud_buffer`, so it keeps the field chain and views
    // the runtime directly for split borrows.
    if let Some(ref mut shroud_buf) = state.match_state.match_presentation.shroud_buffer
        && !state.match_state.sandbox_full_visibility
        && let (Some(rt), Some(owner)) = (state.match_state.sim_runtime.presentation(), &local_owner)
    {
        let view = rt.view();
        let owner_id = view.interner().get(owner).unwrap_or_default();
        // Rendering may perform native Dummy lookups without changing the
        // simulation's retained Dummy coordinate or its fog cache bytes.
        let cells = view
            .resolved_terrain()
            .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
        let fog_options = crate::render::shroud_buffer::FogDrawOptions {
            fog_of_war: view.session().game_options().fog_of_war,
            current_house_defeated: view
                .session()
                .current_house()
                .and_then(|house| view.houses().get(&house))
                .is_some_and(|house| house.is_defeated()),
        };
        let rules = &rt.resources.rules;
        shroud_buf.set_alpha_shapes(view.simulation().alpha_shapes().filter_map(
            |(origin, image)| {
                Some(crate::render::shroud_buffer::AlphaShapeDraw {
                    origin,
                    image: rules.alpha_image(image)?.clone(),
                })
            },
        ));
        shroud_buf.rebuild_if_needed(
            &state.renderer.gpu.device,
            &state.renderer.gpu.queue,
            encoder,
            view.fog(),
            owner_id,
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
            rw,
            rh,
            state.match_state.input.zoom_level,
            cells.as_ref(),
            fog_options,
        );
    }

    // One A plane feeds the world palette blitters, surface lines and A-gated
    // surface stores. Its view identity is stable until resize/map replacement;
    // sandbox uses neutral A without rebuilding or copying the live plane.
    let shroud = (!state.match_state.sandbox_full_visibility)
        .then_some(state.match_state.match_presentation.shroud_buffer.as_ref())
        .flatten();
    state
        .renderer
        .batch_renderer
        .bind_shroud(&state.renderer.gpu.device, shroud);
    state
        .renderer
        .terrain_draw_renderer
        .bind_surface_line_a(&state.renderer.gpu.device, shroud);
    let after_shroud = render_started.map(|_| Instant::now());

    // One tactical composite per Main_Tick (`tactical_composite`): the
    // effects below step `steps` times, and every draw shows the last one,
    // with the lines that read the clock held at that composite's time.
    let chrono_holds = state
        .match_state
        .match_presentation
        .chrono_screen
        .overrides_tactical_draw();
    let tactical_composite = &mut state.match_state.match_presentation.tactical_composite;
    let steps = tactical_composite.begin_draw(cadence, times.radar_ms, chrono_holds);
    let clock_ms = tactical_composite.clock_ms();
    // Ares DisplayCredits ages its labels in the same composite, unless the
    // chrono screen's 4F4480 interception suppresses it.
    #[cfg(feature = "ares")]
    if !chrono_holds {
        let credits = state
            .match_state
            .match_presentation
            .flying_credits
            .get_mut();
        for _ in 0..steps {
            credits.step();
        }
    }

    // Phase 4: Update minimap + build UI instances (selection, health, placement).
    build_instances::update_minimap(state, &local_owner);
    let ui = build_instances::build_ui_instances(state, clock_ms as u32, &cursor_targets);
    let after_ui = render_started.map(|_| Instant::now());

    // Phase 5: Build sidebar instances, once the smooth sidebar's copies
    // follow this frame's draw scale.
    crate::app::presentation::sidebar_render::prepare_smooth_sidebar(state);
    let sidebar = build_instances::build_sidebar_instances(state);
    let after_sidebar = render_started.map(|_| Instant::now());

    // Phase 6: Upload all instances to GPU buffer pool.
    upload_to_gpu(state, encoder, &world, &debug, &ui, &sidebar);
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
    if let Some(runtime) = state.match_state.sim_runtime.presentation() {
        state.renderer.combat_light_renderer.set_spotlight_radius(
            &state.renderer.gpu.queue,
            runtime.resources.rules.general.spotlight.radius,
        );
    }
    state.renderer.combat_light_renderer.prepare(
        &state.renderer.gpu,
        &combat_lights,
        world_camera.screen_size,
        [
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ],
        world_camera.zoom,
    );
    let after_lights = render_started.map(|_| Instant::now());
    let composition_view = state.renderer.combat_light_renderer.composition_view();
    let (_, tactical_y, _, _) = crate::app::input::camera::tactical_viewport_px(state);
    // Shader row coordinates are unscaled world pixels; scissor uses target
    // pixels. Native zoom1 is exact, and scaled views preserve that unit frame.
    let native_z_origin_y = (tactical_y as f32 / z).floor();
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
        state
            .renderer
            .world_zoom_pass
            .as_ref()
            .map_or(&state.renderer.depth_view, |zoom| zoom.depth_view()),
        state.renderer.batch_renderer.camera_uniform(),
    );
    let effects_started = render_started.map(|_| Instant::now());

    // Tactical6D4673 -> LineTrail556D40 updates once per actual composite.
    // It reads committed Bullet coordinates even when no simulation tick ran.
    // The ring belongs to presentation and survives only until it fades or loads.
    let viewport = tactical_surface_line_viewport(state);
    let armed_super_weapon = state.armed_super_weapon_type().map(str::to_owned);
    let armed_building = state.armed_building_type().map(str::to_owned);
    let cursor_cell = (armed_super_weapon.is_some() || armed_building.is_some()).then(|| {
        crate::app::match_runtime::sim_tick::screen_point_to_world_cell(
            state,
            state.match_state.input.cursor_x,
            state.match_state.input.cursor_y,
        )
    });
    let sim_runtime = state.match_state.sim_runtime.presentation();
    let simulation = sim_runtime.map(|runtime| &runtime.simulation);
    let rules = sim_runtime
        .map(|runtime| &runtime.resources.rules)
        .or(state.frontend.frontend_rules.as_ref());
    let show_power_plant_enhancer_range = {
        #[cfg(feature = "phobos")]
        {
            rules.is_some_and(|rules| rules.power_plant_enhancer.show_range)
                && state
                    .match_state
                    .match_presentation
                    .show_power_plant_enhancer_range
        }
        #[cfg(not(feature = "phobos"))]
        {
            false
        }
    };
    let match_presentation = &mut state.match_state.match_presentation;
    let radials = crate::app::presentation::radial_indicators::build(
        simulation,
        rules,
        &cursor_targets.selected,
        armed_super_weapon.as_deref(),
        armed_building.as_deref(),
        cursor_cell,
        show_power_plant_enhancer_range,
        match_presentation.target_lines.unit_action_lines_enabled(),
        &mut match_presentation.placement_range_pulse,
        &mut match_presentation.superweapon_range_pulse,
        &match_presentation.house_color_map,
        viewport,
        clock_ms,
        steps,
    );
    // EBolt4C2830 moves each owned bolt to its Unit's FLH before drawing it,
    // once per composite; the owner reads take no Main draw, so they run
    // ahead of the borrow.
    if steps > 0
        && let Some(runtime) = state.match_state.sim_runtime.presentation()
    {
        let sim = runtime.view().simulation();
        let rules = &runtime.resources.rules;
        state
            .match_state
            .match_presentation
            .electric_bolts
            .follow_owners(|owner| {
                let point = crate::sim::combat::electric_bolt::owner_source(sim, rules, owner)?;
                Some(crate::sim::projectile::ProjectileCoord::new(
                    point.x, point.y, point.z,
                ))
            });
    }
    let presentation = &mut state.match_state.match_presentation;
    // Tactical6D4669 LaserDraw550240 precedes6D466E EBolt, then6D4673
    // trails, even when this composite ran no Logic. Their shared process
    // Main requests happen here once; GPU uploads only read resolved colors.
    let mut main = crate::app::state::process_main_draws(
        state.match_state.sim_runtime.presentation(),
        &state.frontend.frontend_main_rng,
    );
    let sim = state
        .match_state
        .sim_runtime
        .presentation()
        .map(|runtime| runtime.view().simulation());
    let viewer = local_owner
        .as_deref()
        .and_then(|name| sim?.interner().get(name));
    // Use the same authoritative cell visibility as object admission. The
    // shaded screen A plane can include edge blends and height projection;
    // its brightness is not a separate visibility decision.
    let effect_hidden = |x: i32, y: i32| {
        let (rx, ry) = crate::app::presentation::instances::lepton_cell(x, y);
        translucent_fog
            && !sandbox
            && sim.is_some_and(|sim| {
                crate::app::presentation::instances::translucent_fog_conceals_cell(
                    viewer,
                    Some(sim.fog()),
                    rx,
                    ry,
                    sim.session().game_options().fog_of_war,
                )
            })
    };
    // Each composite runs Tactical's order: LaserDraw550240 (6D4669) and
    // EBolt (6D466E) request process Main, then the Airstrike shades follow
    // with the action stores (6D48F1), then LineTrail556D40 (6D4673) and the
    // radiation beams. A draw that owes no composite steps none of them.
    let frame_rate = presentation.detail.borrow().frame_rate();
    for _ in 0..steps {
        presentation.lasers.composite(&mut main);
        presentation
            .electric_bolts
            .composite(viewport, bolt_palette, &mut main, |birth| {
                !effect_hidden(birth.to.x, birth.to.y)
            });
        presentation
            .airstrike_shades
            .step(&ui.tactical_line_calls, &main);
        presentation.line_trails.composite(
            |id| Some(sim?.projectiles().get(id)?.position()),
            |owner, coord| {
                if !translucent_fog || sandbox {
                    return true;
                }
                let Some(sim) = sim else {
                    return true;
                };
                if let Some(bullet) = owner.and_then(|id| sim.projectiles().get(id)) {
                    return !crate::app::presentation::instances::translucent_fog_conceals_bullet(
                        sim,
                        bullet,
                        [coord.x, coord.y, coord.z],
                        local_owner.as_deref(),
                        viewer,
                        sim.session().game_options().fog_of_war,
                    );
                }
                !effect_hidden(coord.x, coord.y)
            },
        );
        presentation
            .radiation_beams
            .composite(frame_rate, viewport, &main, |birth| {
                !effect_hidden(birth.to.x, birth.to.y)
            });
    }
    // The effects above that age or take Main draws in each composite count
    // while any is alive, shown or not; the lines and rings that read the
    // composite's clock or pulse count while drawn.
    let animated = !presentation.lasers.is_empty()
        || !presentation.electric_bolts.is_empty()
        || !presentation.line_trails.is_empty()
        || !presentation.radiation_beams.is_empty()
        || !ui.tactical_line_calls.is_empty()
        || !ui.target_line.is_empty()
        || !radials.draws.is_empty();
    #[cfg(feature = "ares")]
    let animated = animated || !presentation.flying_credits.get_mut().is_empty();
    presentation.tactical_composite.record_contents(animated);
    let bolt_lines = presentation.electric_bolts.shown(viewport.camera);
    // Uploads consume these resolved commands and cannot take further random draws.
    let tactical_draws = crate::app::presentation::airstrike_lines::composite(
        &ui.tactical_line_calls,
        clock_ms as u32,
        &presentation.airstrike_shades,
        viewport,
    );
    let segments = presentation.line_trails.shown();
    let beam_draws = presentation.radiation_beams.shown(viewport.camera);
    let terrain = &mut state.renderer.terrain_draw_renderer;
    let gpu = &state.renderer.gpu;
    terrain.begin_surface_lines();
    let laser_draws = presentation
        .lasers
        .draws()
        .filter(|draw| !effect_hidden(draw.to.x, draw.to.y));
    let surface_effects = terrain.prepare_surface_lines(
        &gpu.device,
        &gpu.queue,
        laser_draws,
        bolt_lines
            .iter()
            .copied()
            .map(crate::render::surface_line::SurfaceDraw::Line),
        segments,
        beam_draws,
        &radials.draws,
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
                );
            }
        }
    }

    let ion_blasts = crate::app::presentation::ion_blasts::draws(state);
    state.renderer.terrain_draw_renderer.prepare_ion_blasts(
        &state.renderer.gpu.device,
        &state.renderer.gpu.queue,
        &ion_blasts,
        world_camera.zoom,
    );
    let after_effects = render_started.map(|_| Instant::now());

    // Phase 7: Dispatch draw calls in render order.
    draw_passes::dispatch_draw_passes(
        state,
        encoder,
        &composition_view,
        &draw_passes::DrawPassData {
            cell_body_draws: &world.cell_body_draws,
            terrain: &world.terrain.normal,
            smudge: &world.smudge,
            bridge_shadows: &world.bridge_body_shadow,
            object_layers: &world.object_layers,
            fogged_objects: &world.fogged_objects,
            building_status_text: &world.building_status_text,
            surface_effects,
            tactical_lines,
            persistent_lights,
            sidebar_unavailable: &sidebar.unavailable,
            sidebar_flash: &sidebar.flash,
            sidebar_gclock: &sidebar.gclock,
            minimap: &sidebar.minimap,
            viewport_rect: &sidebar.viewport_rect,
            smooth_chrome: sidebar.smooth_chrome,
            smooth_command_bar: sidebar.smooth_command_bar,
        },
    );
    let finished = render_started.map(|_| Instant::now());
    // The bounded submissions made while recording belong to the hand-off;
    // the frame owner adds its final submission and present.
    let submitted = state.renderer.terrain_draw_renderer.frame_submit_time();
    let phases = render_started.map(|render_started| RenderPhases {
        world: after_world.unwrap().duration_since(phase1_started.unwrap()),
        debug: after_debug.unwrap().duration_since(after_world.unwrap()),
        shroud: after_shroud.unwrap().duration_since(after_debug.unwrap()),
        ui: after_ui.unwrap().duration_since(after_shroud.unwrap()),
        sidebar: after_sidebar.unwrap().duration_since(after_ui.unwrap()),
        upload: after_upload.unwrap().duration_since(after_sidebar.unwrap()),
        lights: after_lights.unwrap().duration_since(after_upload.unwrap()),
        effects: after_effects
            .unwrap()
            .duration_since(effects_started.unwrap()),
        draw_calls: finished
            .unwrap()
            .duration_since(after_effects.unwrap())
            .saturating_sub(submitted),
        handoff: submitted,
        other: phase1_started.unwrap().duration_since(render_started)
            + effects_started
                .unwrap()
                .duration_since(after_lights.unwrap()),
    });
    state
        .match_state
        .match_presentation
        .cached_terrain_instances = std::mem::take(&mut world.terrain.normal);
    Ok(GameRenderOutput {
        instance_counts: sidebar.emitted_instance_counts(),
        sidebar_view: sidebar.view,
        times,
        particle_draws: world.particle_draws,
        phases,
        software_cursor: ui.software_cursor_draw,
    })
}

/// The tactical surface object-layer DSurface stores address: logical
/// (unzoomed) pixels from the floored camera, clipped to the tactical rect.
///
/// Its camera is the world point at the battlefield's corner, as gamemd's
/// tactical position is: the surface-line owners project to client points
/// from it and add the clip's corner for the surface point. The stored camera
/// is the frame corner's, so it comes in by the battlefield's corner.
///
/// The line and wave stores take their depth from the surface row less the
/// clip's row, on top of this origin (`4BEF03`). A sprite's depth goes by its
/// row in the battlefield, so the origin carries the clip's row twice and a
/// line keeps its depth against the objects wherever a layout puts the
/// battlefield. The original's clip starts at row 0.
pub(crate) fn tactical_surface_line_viewport(
    state: &AppState,
) -> crate::render::surface_line::SurfaceLineViewport {
    let z = state.match_state.input.zoom_level;
    let (x, y, w, h) = crate::app::input::camera::tactical_viewport_px(state);
    let clip = [x, y, w, h].map(|v| (v as f32 / z) as i32);
    crate::render::surface_line::SurfaceLineViewport {
        camera: surface_line_camera(
            [
                state.match_state.input.camera_x,
                state.match_state.input.camera_y,
            ],
            [x, y, w, h],
            z,
        ),
        clip,
        z_origin_y: 2 * clip[1],
        zoom: 1.0,
    }
}

/// The camera the surface-line owners project from: the floored world point
/// at the battlefield's corner. `camera` is the stored one, the frame
/// corner's; `tactical` is the battlefield in frame pixels.
pub(crate) fn surface_line_camera(camera: [f32; 2], tactical: [u32; 4], zoom: f32) -> [i32; 2] {
    [
        camera[0] + tactical[0] as f32 / zoom,
        camera[1] + tactical[1] as f32 / zoom,
    ]
    .map(|v| v.floor() as i32)
}

/// Upload all per-frame instance vectors to the GPU buffer pool.
///
/// The pool reuses GPU buffers across frames to avoid per-frame allocation.
/// Buffer names here must match the keys used in `draw_passes::dispatch_draw_passes`.
fn upload_to_gpu(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    world: &WorldInstances,
    debug: &DebugInstances,
    ui: &UiInstances,
    sidebar: &SidebarInstances,
) {
    // A zoomed match draws the battlefield's screen-fixed text below, with
    // the boxes and frames behind it, at the window's pixels
    // (`window_ui`): its producers lay out in the frame and emit for the
    // window.
    let window_raster = crate::app::presentation::window_ui::battlefield_raster(state);
    let frame_raster = state
        .renderer
        .bit_font
        .set_raster(window_raster.unwrap_or(crate::render::bit_font::TextRaster::NATIVE));

    // A5 chat/system message lines (GAME.FNT atlas) — chat draws before the
    // tooltip (O10). Prepare once: nonsilent TextLabel72A4A0 advances its
    // raw-wall reveal and requests typing audio once per successful paint.
    // Keep the row's existing opaque black background behind its glyphs.
    let (message_fill, message_text) = crate::app::input::messages::build_message_instances(state);
    let (outcome_shape, outcome_text) = crate::app::presentation::outcome_banner::instances(state);

    // A4 in-game tooltip: built before the pool borrow (it reads &AppState);
    // fill (darken texture) + text (GAME.FNT atlas), drawn after the chat
    // overlay and before the software cursor (O10).
    let (mut tooltip_fill, mut tooltip_border, mut tooltip_text) =
        crate::app::input::tooltips::build_tooltip_instances(state);
    // The layout edit mode's outlines and names share the tooltip's lanes:
    // over the whole frame, sidebar included.
    {
        let (fills, borders, texts) = crate::app::input::layout_edit::instances(state);
        tooltip_fill.extend(fills);
        tooltip_border.extend(borders);
        tooltip_text.extend(texts);
    }

    // TacticalClass::Draw's timer lines, which follow the PixelFX sparkles;
    // built before the pool borrow (they step the Supers' blinks).
    let (mut super_timer_fill, mut super_timer_text) =
        crate::app::presentation::super_timers::build_super_timer_instances(state);
    // The buttons and texts the side's layout adds over the battlefield
    // share the timers' lanes: white boxes, then text, inside its clip.
    {
        let theme = crate::app::presentation::sidebar_render::current_sidebar_theme(state);
        let (boxes, texts) = crate::app::presentation::layout_parts::instances(
            state,
            crate::ui::added_parts::Holder::Field,
            crate::render::sidebar_text::native_radar_outline_color(theme),
            [
                state.match_state.input.camera_x,
                state.match_state.input.camera_y,
            ],
        );
        super_timer_fill.extend(boxes);
        super_timer_text.extend(texts);
    }
    // Their pictures come from the sidebar's atlas and zoom with the frame.
    let layout_field_art = crate::app::presentation::layout_parts::pictures(
        state,
        crate::ui::added_parts::Holder::Field,
        [
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ],
    );

    // GScreenClass_DrawText's Phobos build description, the tactical view's
    // top-right identity box; empty outside Phobos builds.
    let (build_text_fill, build_text_text) =
        crate::app::presentation::build_text::build_instances(state);
    state.renderer.bit_font.set_raster(frame_raster);
    let [
        message_fill,
        tooltip_fill,
        tooltip_border,
        super_timer_fill,
        build_text_fill,
    ] = [
        message_fill,
        tooltip_fill,
        tooltip_border,
        super_timer_fill,
        build_text_fill,
    ]
    .map(|quads| match window_raster {
        Some(raster) => quads.into_iter().map(|quad| raster.quad(quad)).collect(),
        None => quads,
    });
    // The software cursor follows them into the window, last.
    let software_cursor: std::borrow::Cow<'_, [crate::render::batch::SpriteInstance]> =
        match crate::app::presentation::window_ui::zoom_raster(state) {
            Some(raster) => ui
                .software_cursor
                .iter()
                .map(|quad| raster.quad(*quad))
                .collect(),
            None => ui.software_cursor.as_slice().into(),
        };

    #[cfg(feature = "ares")]
    let bounty_text = {
        let presentation = &state.match_state.match_presentation;
        if presentation.chrono_screen.overrides_tactical_draw() {
            Vec::new()
        } else {
            let (left, top, width, height) = crate::app::input::camera::tactical_viewport_px(state);
            presentation.flying_credits.borrow().build_instances(
                &state.renderer.bit_font,
                [
                    state.match_state.input.camera_x,
                    state.match_state.input.camera_y,
                ],
                state.match_state.input.zoom_level,
                [left, top, width, height],
            )
        }
    };

    // The sidebar's streams scroll with the UI camera, and its text with it
    // unless a raster already placed that on the surface.
    let ui_camera = state.renderer.batch_renderer.ui_camera_uniform().camera_pos;
    let text_camera = if matches!(
        crate::app::presentation::sidebar_surface::column_text(state),
        crate::app::presentation::sidebar_surface::ColumnText::Surface(Some(_))
    ) {
        [0.0; 2]
    } else {
        ui_camera
    };

    // Recorded ahead of every pass that draws them (`InstanceBufferPool::
    // record_uploads`).
    let renderer = &mut state.renderer;
    let mut pool = renderer
        .instance_pool
        .record_uploads(&renderer.gpu.device, encoder);

    // Debug overlays
    pool.upload("debug_pathgrid", &debug.pathgrid);
    pool.upload("debug_cell_grid", &debug.cell_grid);
    pool.upload("debug_path", &debug.path);
    pool.upload("debug_foot_stroke", &debug.foot_stroke);
    pool.upload("debug_foot_nodes", &debug.foot_nodes);
    pool.upload("debug_foot_text", &debug.foot_text);
    pool.upload("debug_heightmap", &debug.heightmap);

    // Terrain + overlays
    pool.upload("terrain", &world.terrain.normal);
    pool.upload("cell_body", &world.cell_bodies);
    pool.upload(
        "building_status_text",
        &world.building_status_text.instances,
    );
    pool.upload("fogged_objects", &world.fogged_objects.instances);
    for (layer, objects) in world.object_layers.iter().enumerate() {
        pool.upload_page("object_layer", layer, &objects.instances);
        pool.upload_page("object_extras", layer, &objects.extras_instances);
    }
    pool.upload("overlay_bridge_body_shadow", &world.bridge_body_shadow);
    // Smudges: drawn inside the terrain layer, before the cell bodies,
    // matching the native per-cell tile-then-smudge dispatch.
    pool.upload("smudge", &world.smudge);

    // Residual effects keep their existing uploads and pass ownership.
    pool.upload("cell_sparkles", &world.cell_sparkles);
    pool.upload("layout_field_art", &layout_field_art);
    pool.upload("super_timer_fill", &super_timer_fill);
    pool.upload("super_timer_text", &super_timer_text);
    pool.upload("build_text_fill", &build_text_fill);
    pool.upload("build_text_text", &build_text_text);
    #[cfg(feature = "ares")]
    pool.upload("bounty_text", &bounty_text);

    // UI overlays
    for (house, beacons) in ui.beacons.iter().enumerate() {
        pool.upload_page("beacons", house, beacons);
    }
    pool.upload("drag", &ui.drag);
    pool.upload("ground_marks", &ui.ground_marks);
    pool.upload("software_cursor", &software_cursor);
    pool.upload("placement", &ui.placement);
    #[cfg(feature = "phobos")]
    pool.upload("placement_ghost", &ui.placement_ghost);
    pool.upload("factory_rally_first", &ui.factory_rally_first);
    pool.upload("target_lines", &ui.target_line);
    pool.upload("factory_rally_second", &ui.factory_rally_second);

    // Sidebar + minimap
    pool.upload_following("minimap", &sidebar.minimap, ui_camera);
    pool.upload_following("viewport_rect", &sidebar.viewport_rect, ui_camera);
    pool.upload_following(
        "radar_content_boundary",
        &sidebar.content_boundary,
        ui_camera,
    );
    pool.upload_following("sidebar_chrome", &sidebar.chrome, ui_camera);
    pool.upload_following("command_bar", &sidebar.command_bar, ui_camera);
    pool.upload_following("radar_anim", &sidebar.radar_anim, ui_camera);
    pool.upload_following("sidebar_cameo", &sidebar.cameo, ui_camera);
    pool.upload_following("sidebar_cameo_hover", &sidebar.cameo_hover, ui_camera);
    pool.upload_following("sidebar_unavailable", &sidebar.unavailable, ui_camera);
    pool.upload_following("sidebar_flash", &sidebar.flash, ui_camera);
    pool.upload_following("sidebar_gclock", &sidebar.gclock, ui_camera);
    pool.upload_following("sidebar_queue_overlay", &sidebar.queue_overlay, ui_camera);
    pool.upload_following("sidebar_queue_text", &sidebar.queue_text, text_camera);
    pool.upload_following("sidebar_label_overlay", &sidebar.label_overlay, ui_camera);
    pool.upload_following("sidebar_label_text", &sidebar.label_text, text_camera);
    pool.upload_following("sidebar_cameo_overlay", &sidebar.cameo_overlay, ui_camera);
    pool.upload_following("sidebar_text", &sidebar.text, text_camera);
    #[cfg(feature = "phobos")]
    {
        pool.upload("sw_sidebar_chrome", &sidebar.sw_sidebar_chrome);
        pool.upload("sw_sidebar_cameo", &sidebar.sw_sidebar_cameo);
        pool.upload("sw_sidebar_gclock", &sidebar.sw_sidebar_gclock);
        pool.upload("sw_sidebar_hover", &sidebar.sw_sidebar_hover);
        pool.upload("sw_sidebar_overlay", &sidebar.sw_sidebar_overlay);
        pool.upload("sw_sidebar_text", &sidebar.sw_sidebar_text);
    }
    pool.upload("message_text", &message_text);
    pool.upload("message_fill", &message_fill);
    pool.upload("outcome_shape", &outcome_shape);
    pool.upload("outcome_text", &outcome_text);
    pool.upload("tooltip_fill", &tooltip_fill);
    pool.upload("tooltip_border", &tooltip_border);
    pool.upload("tooltip_text", &tooltip_text);
}

#[cfg(test)]
mod tests {
    include!("../render_tests.rs");
}
