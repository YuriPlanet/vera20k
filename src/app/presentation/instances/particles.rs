//! Particle and System bodies in their retained native Display slots.
//!
//! Particle62BB0E -> ObjectUnlimbo5F4EC0 queries GetLayer62D770 (AIR3),
//! then Submit4A9720 appends the Particle, independently of its System.
//! Tactical6D8DB0 visits those registrations in retained order and reaches
//! ParticleDraw62CEC0 through ObjectDrawIfVisible5F4B10. The existing tactical
//! lowering, indexed atlas, palette and packed compositor consume each piece.
//! Native caller/type/draw evidence: tools/spatial_oracle/damage_smoke.md.

use crate::app::presentation::render::draw_plan_lowering::{
    NativeDisplayOrder, ObjectBodyDraw, ObjectPieceInstance, ObjectTexture, PlannedObjectInstance,
};
use crate::render::batch::SpriteInstance;
use crate::render::draw_state::DrawState;
use crate::render::native_z::ZGradient;
use crate::render::palette_light::PaletteLight;
use crate::render::sprite_atlas::{ShpPaletteContext, ShpSpriteKey};
use crate::render::surface_line::SurfaceDraw;
use crate::render::tactical_draw_plan::{BlitPolicy, RenderZPolicy, SpriteEncoding};
use crate::rules::house_colors::HouseColorIndex;
use crate::rules::particle_type::ParticleBehavesLike;
use crate::util::lepton::absolute_leptons_to_screen;

use super::helpers::{compute_sprite_depth_params_lifted, projection_admitted};
use super::overlays::DisplayEffectInputs;

/// Read-only projection of the actual piece submitted by this builder. It is
/// collected only under the existing map diagnostic presentation policy.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ParticleDrawObservation {
    system_id: u64,
    particle_id: u64,
    particle_native_id: i32,
    display_layer: u8,
    display_order: u64,
    image: String,
    frame: u16,
    atlas_page: usize,
    source_palette: u32,
    position: [f32; 2],
    size: [f32; 2],
    uv_origin: [f32; 2],
    uv_size: [f32; 2],
    native_flags: u32,
    z_adjust: f32,
    z_gradient: u32,
    palette_light: [u32; 4],
    render_z: &'static str,
}

/// ParticleDraw62CEC0 selects the Convert leaf only at detail2. The last
/// comparison sign-extends the retained translucency byte; 128..255 are opaque.
fn particle_draw_flags(detail_level: u32, translucency: u8) -> u32 {
    let selector = if detail_level == 2 {
        match translucency {
            25 => 2,
            50 => 4,
            value if value as i8 >= 75 => 6,
            _ => 0,
        }
    } else {
        0
    };
    0x2E00 | selector
}

/// Frame-local geometry shared by concrete Particle and System body builders.
/// No retained simulation state or alternate projection authority lives here.
pub(super) struct ParticleDrawContext {
    camera: [f32; 2],
    /// The surface-line owners' camera, the battlefield corner's.
    line_camera: [i32; 2],
    viewport: [f32; 2],
    axis: crate::render::batch::DepthAxis,
    point_viewport: crate::render::tactical_compat::TacticalPointViewport,
}

impl ParticleDrawContext {
    pub(super) fn new(
        camera: [f32; 2],
        zoom: f32,
        tactical: [u32; 4],
        axis: crate::render::batch::DepthAxis,
    ) -> Self {
        let [x, y, width, height] = tactical;
        let logical = |value: u32| (value as f32 / zoom) as i32;
        Self {
            camera,
            line_camera: crate::app::presentation::render::surface_line_camera(
                camera, tactical, zoom,
            ),
            viewport: [width as f32 / zoom, height as f32 / zoom],
            axis,
            point_viewport: crate::render::tactical_compat::TacticalPointViewport {
                clip: crate::render::tactical_compat::TacticalRect {
                    left: logical(x),
                    top: logical(y),
                    width: logical(width),
                    height: logical(height),
                },
                tactical_offset_x: camera[0].floor() as i32,
                tactical_offset_y: camera[1].floor() as i32,
                // Translate once through the established common world-row
                // convention. Native point arithmetic itself stays unchanged.
                radar_viewport_offset_y: crate::util::lepton::absolute_leptons_to_screen(0, 0, 0).1
                    as i32,
                adjust_for_z_multiplier: crate::util::native_x87::STANDARD_ADJUST_FOR_Z_MULTIPLIER,
                z_origin_term: logical(y) as i16,
                z_bottom_term: crate::render::native_z::DEFAULT_Z as i16,
            },
        }
    }
}

/// SystemDraw62E280 owns an immediate Spotlight in its own GROUND Display slot.
pub(super) fn build_particle_system_instance(
    input: &DisplayEffectInputs<'_>,
    system: &crate::sim::particles::ParticleSystem,
    context: &ParticleDrawContext,
    objects: &mut Vec<PlannedObjectInstance>,
    order: &NativeDisplayOrder,
) {
    let Some(parent) = order.object_draw(system.stable_id(), SpriteEncoding::Plain) else {
        return;
    };
    let coord = system.coords();
    let point = absolute_leptons_to_screen(coord.x, coord.y, coord.z);
    if system.in_limbo()
        || !projection_admitted([point.0, point.1], context.camera, context.viewport)
    {
        return;
    }
    let kind = input.rules.particle_system_type(system.type_id());
    let record = crate::app::presentation::combat_lights::particle_system_light(
        system,
        kind,
        &mut input.detail.borrow_mut(),
        input.rules.ares(),
    );
    if let Some(mut record) = record {
        record.coord = crate::sim::projectile::ProjectileCoord::new(coord.x, coord.y, coord.z);
        let mut planned = PlannedObjectInstance::object(parent, Vec::new());
        planned.body_draw = Some(ObjectBodyDraw::CombatLight(record));
        objects.push(planned);
    }
}

/// BuildingLight435BE0: one temporary5FF850 glow followed by two native
/// DSurface4BBCA0 brightening lines in this child's retained AIR Display slot.
pub(super) fn build_spotlight_instance(
    input: &DisplayEffectInputs<'_>,
    id: u64,
    light: crate::sim::world::BuildingSpotlight,
    context: &ParticleDrawContext,
    objects: &mut Vec<PlannedObjectInstance>,
    order: &NativeDisplayOrder,
) {
    let Some(parent) = order.object_draw(id, SpriteEncoding::Plain) else {
        return;
    };
    let p = light.coord;
    let point = absolute_leptons_to_screen(p[0], p[1], p[2]);
    if !projection_admitted([point.0, point.1], context.camera, context.viewport) {
        return;
    }
    let glow = crate::app::presentation::combat_lights::CombatLight {
        coord: crate::sim::projectile::ProjectileCoord::new(p[0], p[1], p[2]),
        stage: light.glow_stage,
        base_size: 16,
        flags: light.glow_flags,
    }
    .draw_record(&mut input.detail.borrow_mut());
    let project = |p: [i32; 3]| {
        let xy = absolute_leptons_to_screen(p[0], p[1], p[2]);
        [
            xy.0 as i32 - context.line_camera[0],
            xy.1 as i32 - context.line_camera[1],
        ]
    };
    let beams = light
        .beam_edges
        .map(|edges| {
            edges.map(|edge| {
                Some(SurfaceDraw::Line(
                    crate::render::surface_line::SurfaceLine {
                        from: project(light.beam_origin),
                        to: project(edge),
                        z_adjust: light.beam_z_adjust,
                        blend: crate::render::surface_line::SurfaceLineBlend::Brighten {
                            amount: light.beam_amount,
                        },
                    },
                ))
            })
        })
        .unwrap_or([None, None]);
    let mut planned = PlannedObjectInstance::object(parent, Vec::new());
    planned.body_draw = Some(ObjectBodyDraw::Spotlight { glow, beams });
    objects.push(planned);
}

/// Particle62CEC0 consumes one retained AIR registration. Its mutable FPS
/// query is made by the shared Display traversal, before its later A/Z read.
/// Move-dead children keep their last draw until their lifecycle removes them.
pub(super) fn build_particle_instance(
    input: &DisplayEffectInputs<'_>,
    system: Option<&crate::sim::particles::ParticleSystem>,
    particle: &crate::sim::particles::Particle,
    context: &ParticleDrawContext,
    objects: &mut Vec<PlannedObjectInstance>,
    order: &NativeDisplayOrder,
    observations: &mut Option<Vec<ParticleDrawObservation>>,
) {
    let Some(mut parent) = order.object_draw(particle.stable_id(), SpriteEncoding::Plain) else {
        return;
    };
    let rules = input.rules;
    let detail_level = input.detail_level;
    let camera = context.camera;
    let viewport = context.viewport;
    let axis = context.axis;
    let coord = particle.coords();
    let point = absolute_leptons_to_screen(coord.x, coord.y, coord.z);
    if !projection_admitted([point.0, point.1], camera, viewport) {
        return;
    }
    let kind = rules.particle_type(particle.type_id());
    // ParticleDraw62CEC0 reads the shared unsigned FPS/minimum owner.
    // Damaging particles alone can pass a below-minimum frame rate.
    let enough_fps = {
        let mut detail = input.detail.borrow_mut();
        detail.frame_rate() >= detail.minimum_frame_rate()
    };
    if !enough_fps && kind.damage == 0 {
        return;
    }
    if detail_level == 0
        && matches!(
            kind.behaves_like,
            ParticleBehavesLike::Smoke | ParticleBehavesLike::Spark
        )
    {
        return;
    }
    // ParticleDraw62CF65..62CF7D: Spark (3) and Railgun (4) both take the
    // coloured point path at 62D02C, reading the same +B0/+B4/+B8 colour.
    if matches!(
        kind.behaves_like,
        ParticleBehavesLike::Spark | ParticleBehavesLike::Railgun
    ) {
        {
            let start_rgb = particle.start_rgb();
            let color_index = particle.color_index();
            let color_accumulator = particle.color_accumulator();
            let command = crate::render::tactical_compat::SparkPointCommand {
                world: coord,
                start_rgb,
                color_index,
                color_accumulator,
                damage: kind.damage,
                gates: crate::render::tactical_compat::SparkDrawGates {
                    performance_passed: enough_fps,
                    // The detail-0 gate (62CEF3) already returned for Spark;
                    // it never applies to Railgun.
                    extra_animations_enabled: true,
                    // Active-YR5865E0 is XOR AL,AL; RET4. The conditional
                    // Scenario1000 fog call in62CF11 never rejects this body.
                    fog_passed: true,
                },
                draw_ordinal: parent.display_order,
            };
            if let crate::render::tactical_compat::SparkPointPreparation::Ready { point, .. } =
                crate::render::tactical_compat::prepare_spark_point(
                    command,
                    &kind.color_list,
                    context.point_viewport,
                )
                .expect("produced point-particle state has finite native arithmetic")
            {
                let mut planned = PlannedObjectInstance::object(parent, Vec::new());
                planned.body_draw = Some(ObjectBodyDraw::Surface(SurfaceDraw::Point(point)));
                objects.push(planned);
            }
        }
        return;
    }
    let Some(atlas) = input.atlas else {
        return;
    };
    let image = kind.image.as_str();
    let native_frame = particle.draw_frame(kind);
    let Ok(frame) = u16::try_from(native_frame) else {
        return;
    };
    let flags = particle_draw_flags(detail_level, particle.translucency());
    let Some((piece, page)) =
        particle_shp_piece(atlas, image, frame, coord, point, flags, &mut parent, axis)
    else {
        return;
    };
    if let (Some(rows), Some(system)) = (observations.as_mut(), system) {
        rows.push(ParticleDrawObservation {
            system_id: system.stable_id(),
            particle_id: parent.id,
            particle_native_id: particle.native_unique_id(),
            display_layer: parent.layer.0,
            display_order: parent.display_order,
            image: image.to_owned(),
            frame,
            atlas_page: page,
            source_palette: piece.instance.source_palette,
            position: piece.instance.position,
            size: piece.instance.size,
            uv_origin: piece.instance.uv_origin,
            uv_size: piece.instance.uv_size,
            native_flags: flags,
            z_adjust: piece.instance.z_adjust,
            z_gradient: piece.instance.z_gradient,
            palette_light: piece.instance.palette_light.0,
            render_z: match piece.render_z {
                RenderZPolicy::ReadOnly => "read_only",
                _ => unreachable!("ParticleDraw62CEC0 never writes render Z"),
            },
        });
    }
    objects.push(PlannedObjectInstance::object(parent, vec![piece]));
}

/// Particle62CFF9's DrawSHP of one frame at `coord` (screen `point`): global
/// ANIM87F6C0 at brightness 1000 (PUSH0x3E8), independently of cell lighting,
/// depth `-15 - AdjustForZ` and the vertical Z gradient, with the `flags`
/// translucency selector. Ares' ExtData::Draw (`10026430`) passes the same for
/// its smoke records. Returns the piece and its atlas page.
#[allow(clippy::too_many_arguments)]
fn particle_shp_piece(
    atlas: &crate::render::sprite_atlas::SpriteAtlas,
    image: &str,
    frame: u16,
    coord: glam::IVec3,
    point: (f32, f32),
    flags: u32,
    parent: &mut crate::render::tactical_draw_plan::ObjectDraw,
    axis: crate::render::batch::DepthAxis,
) -> Option<(ObjectPieceInstance, usize)> {
    let key = ShpSpriteKey {
        palette_context: ShpPaletteContext::Particle,
        type_id: image.into(),
        facing: 0,
        frame,
        house_color: HouseColorIndex(0),
    };
    let entry = atlas.get(&key)?;
    if flags & 6 != 0 {
        parent.policy = BlitPolicy::translucent(SpriteEncoding::Plain, RenderZPolicy::ReadOnly);
    }
    let lift = crate::util::native_x87::adjust_for_z_standard(coord.z) as f32;
    let piece = ObjectPieceInstance {
        target: ObjectTexture::ShpPage(entry.page as usize),
        render_z: parent.policy.render_z,
        instance: SpriteInstance {
            position: [point.0 + entry.offset_x, point.1 + entry.offset_y],
            size: entry.pixel_size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            source_palette: entry.source_palette,
            depth: compute_sprite_depth_params_lifted(
                axis.origin_y,
                axis.world_height,
                point.1,
                lift.round() as i32,
            ),
            tint: crate::map::lighting::DEFAULT_TINT,
            alpha: 1.0,
            draw_state: DrawState::from_native_selector(0, (flags & 6) as u8),
            palette_light: PaletteLight::plain(53, 1000),
            // 62D01D: this is the depth argument, not a screen-Y lift.
            z_adjust: -15. - lift,
            z_gradient: ZGradient::Vertical as u32,
            ..Default::default()
        },
    };
    Some((piece, entry.page as usize))
}

/// Frame-local parents for the Ares draw: past every simulation id and after
/// every retained Air member, in the order the draw visits them.
const ARES_DRAW_ID_BASE: u64 = 0xA4E5_0000_0000_0000;
const ARES_DRAW_ORDER_BASE: u64 = 1 << 62;

/// Ares 3.0 DrawParticleSystems (Ares.dll `10026760`), which the hook at
/// `0x6D9427` (`10029100`) runs after the Air layer: every system's Ares
/// records (`ExtData::Draw @ 10026430`), in ParticleSystemClass::Array order,
/// when the detail level is above 0 and `RulesExt::DetailsCurrentlyEnabled`
/// holds (the frame rate reaches `60 / GameSpeed` or the minimum). Spark and
/// railgun records are single points with the stock point's A and Z tests;
/// those on shrouded cells are skipped under FogOfWar. Smoke records draw
/// their type's frame `State`.
///
/// RESIDUAL (presentation): the smoke draws through ANIM.PAL, not the
/// particle type's Ares `Palette=` (`ParticleTypeExt`, not ported), and a
/// point whose colour index has no next entry is skipped where Ares reads past
/// the list.
pub(super) fn build_ares_particle_draws(
    input: &DisplayEffectInputs<'_>,
    context: &ParticleDrawContext,
    objects: &mut Vec<PlannedObjectInstance>,
) {
    use crate::render::tactical_compat::{
        PreparedSparkPoint, adjust_for_z, project_spark_point, z_candidate,
    };
    use crate::render::tactical_draw_plan::{ObjectDraw, TacticalLayer};

    // Without the game's Ares switch no system has records, and the hook's
    // Detail query below must not touch the stock Detail latch.
    if !input.rules.ares() {
        return;
    }
    let sim = input.sim;
    let systems = sim.particle_systems();
    if systems.is_empty() || input.detail_level == 0 {
        return;
    }
    let enabled = {
        let mut detail = input.detail.borrow_mut();
        let current = detail.frame_rate();
        let speed = sim.session().game_options().game_speed.clamp(1, 6) as u32;
        current >= 60 / speed || current >= detail.minimum_frame_rate()
    };
    if !enabled {
        return;
    }
    // ignoreShroud unless FogOfWar (SpecialFlags 0x1000) is on.
    let fog_viewer = input
        .local_owner
        .filter(|_| sim.session().game_options().fog_of_war);
    let cells = sim
        .resolved_terrain()
        .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
    let viewport = context.point_viewport;
    let mut sequence = 0;
    let mut next_parent = |policy: BlitPolicy| {
        let parent = ObjectDraw {
            id: ARES_DRAW_ID_BASE + sequence,
            layer: TacticalLayer(3),
            display_order: ARES_DRAW_ORDER_BASE + sequence,
            policy,
        };
        sequence += 1;
        parent
    };
    for (_, system) in systems.iter() {
        let Some(ext) = system.ares() else {
            continue;
        };
        let held = input.rules.particle_type(ext.held_type());
        for record in ext.particles() {
            let coord = record.coords();
            if let (Some(viewer), Some(cells)) = (fog_viewer, cells.as_ref())
                && crate::sim::vision::point_is_shrouded(
                    sim.fog(),
                    cells,
                    viewer,
                    crate::sim::components::DriveCoord {
                        x: coord.x,
                        y: coord.y,
                        z: coord.z,
                    },
                )
            {
                continue;
            }
            let Ok(screen) = project_spark_point(coord, viewport) else {
                continue;
            };
            if !viewport.clip.contains(screen) {
                continue;
            }
            let Ok(adjust) = adjust_for_z(coord.z, viewport.adjust_for_z_multiplier) else {
                continue;
            };
            let Some(rgb) = record.draw_color(&held.color_list) else {
                continue;
            };
            let point = PreparedSparkPoint {
                screen,
                candidate_z: z_candidate(
                    viewport.z_origin_term,
                    viewport.z_bottom_term,
                    screen.y,
                    adjust,
                ),
                rgb: rgb.map(i32::from),
            };
            let mut planned = PlannedObjectInstance::object(
                next_parent(BlitPolicy::z_read(SpriteEncoding::Plain)),
                Vec::new(),
            );
            planned.body_draw = Some(ObjectBodyDraw::Surface(SurfaceDraw::Point(point)));
            objects.push(planned);
        }
        let Some(atlas) = input.atlas else {
            continue;
        };
        for smoke in ext.smoke() {
            let kind = input.rules.particle_type(smoke.type_id());
            let Ok(frame) = u16::try_from(smoke.state()) else {
                continue;
            };
            let coord = smoke.location();
            let point = absolute_leptons_to_screen(coord.x, coord.y, coord.z);
            if !projection_admitted([point.0, point.1], context.camera, context.viewport) {
                continue;
            }
            // 100266CE..100266F7: 25, 50 and (unsigned) 75 or more select
            // the translucent leaves at the highest detail level.
            let selector = match smoke.translucency() {
                _ if input.detail_level != 2 => 0,
                25 => 2,
                50 => 4,
                75.. => 6,
                _ => 0,
            };
            let mut parent = next_parent(BlitPolicy::z_read(SpriteEncoding::Plain));
            if let Some((piece, _)) = particle_shp_piece(
                atlas,
                kind.image.as_str(),
                frame,
                coord,
                point,
                0x2E00 | selector,
                &mut parent,
                context.axis,
            ) {
                objects.push(PlannedObjectInstance::object(parent, vec![piece]));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::particle_draw_flags;

    #[test]
    fn native_particle_selector_uses_detail_and_signed_byte_comparison() {
        // Instruction-level control boundaries at ParticleDraw62CEC0;
        // packed RGB565 arithmetic is tested by its existing compositor owner.
        for (byte, selector) in [
            (0, 0),
            (25, 2),
            (50, 4),
            (74, 0),
            (75, 6),
            (127, 6),
            (128, 0),
            (255, 0),
        ] {
            assert_eq!(particle_draw_flags(2, byte), 0x2E00 | selector);
            for detail in [0, 1, 3] {
                assert_eq!(particle_draw_flags(detail, byte), 0x2E00);
            }
        }
    }
}
