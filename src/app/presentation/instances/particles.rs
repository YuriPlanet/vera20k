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
use crate::map::terrain;
use crate::render::batch::SpriteInstance;
use crate::render::draw_state::DrawState;
use crate::render::native_z::ZGradient;
use crate::render::palette_light::PaletteLight;
use crate::render::sprite_atlas::{ShpPaletteContext, ShpSpriteKey};
use crate::render::surface_line::SurfaceDraw;
use crate::render::tactical_draw_plan::{BlitPolicy, RenderZPolicy, SpriteEncoding};
use crate::rules::house_colors::HouseColorIndex;
use crate::rules::particle_type::ParticleBehavesLike;
use crate::util::native_x87::adjust_for_z_standard;

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
    let Some(parent) = order.object_draw(system.stable_id, SpriteEncoding::Plain) else {
        return;
    };
    let point = terrain::lepton_to_screen(system.coords);
    if system.in_limbo()
        || !projection_admitted([point.0, point.1], context.camera, context.viewport)
    {
        return;
    }
    let kind = input.rules.particle_system_type(system.type_id);
    let record = crate::app::presentation::combat_lights::particle_system_light(
        system,
        kind,
        &mut input.detail.borrow_mut(),
    );
    if let Some(record) = record {
        let mut planned = PlannedObjectInstance::object(parent, Vec::new());
        planned.body_draw = Some(ObjectBodyDraw::CombatLight(record));
        objects.push(planned);
    }
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
    let point = terrain::lepton_to_screen(particle.coords);
    if !projection_admitted([point.0, point.1], camera, viewport) {
        return;
    }
    let kind = rules.particle_type(particle.type_id);
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
    if kind.behaves_like == ParticleBehavesLike::Spark {
        if let Some(spark) = particle.spark {
            let command = crate::render::tactical_compat::SparkPointCommand {
                world: particle.coords,
                start_rgb: spark.start_rgb,
                color_index: spark.color_index,
                color_accumulator: spark.color_accumulator,
                damage: kind.damage,
                gates: crate::render::tactical_compat::SparkDrawGates {
                    performance_passed: enough_fps,
                    extra_animations_enabled: detail_level != 0,
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
                    crate::render::tactical_compat::SparkPointPlanes::Deferred,
                )
                .expect("produced Spark state has finite native point arithmetic")
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
    let Some(image) = kind.image.as_deref() else {
        return;
    };
    let native_frame = match kind.behaves_like {
        // ParticleGetDrawFrame62D830: MOVSX from state+12E. The atlas
        // accepts nonnegative frames only; stock Smoke is0..19.
        ParticleBehavesLike::Smoke | ParticleBehavesLike::Gas => {
            i32::from(particle.animation_state as i8)
        }
        // The separate Fire frame policy retains its existing path;
        // this mechanism is bounded to stock building damage-Smoke.
        ParticleBehavesLike::Fire => {
            // Residual: detached Fire frame selection lacks original evidence.
            // The existing approximate policy needs a System's facing; do not
            // invent that facing or retain a fabricated parent association.
            let Some(system) = system else {
                return;
            };
            let band = (u16::from(system.facing) / 0x40) & 3;
            i32::from(band * u16::from(kind.end_state_ai) + u16::from(particle.animation_state))
        }
        ParticleBehavesLike::Spark | ParticleBehavesLike::Railgun => return,
    };
    let Ok(frame) = u16::try_from(native_frame) else {
        return;
    };
    let key = ShpSpriteKey {
        palette_context: ShpPaletteContext::GlobalAnim,
        type_id: image.to_owned(),
        facing: 0,
        frame,
        house_color: HouseColorIndex(0),
    };
    let Some(entry) = atlas.get(&key) else {
        return;
    };
    let flags = particle_draw_flags(detail_level, particle.translucency);
    if flags & 6 != 0 {
        parent.policy = BlitPolicy::translucent(SpriteEncoding::Plain, RenderZPolicy::ReadOnly);
    }
    let lift = adjust_for_z_standard(particle.coords.z);
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
                lift,
            ),
            tint: crate::map::lighting::DEFAULT_TINT,
            alpha: 1.0,
            draw_state: DrawState::from_native_selector(0, (flags & 6) as u8),
            // Particle62CFF9 passes decimal1000 (PUSH0x3E8) and
            // global ANIM87F6C0, independently of cell lighting.
            palette_light: PaletteLight::plain(53, 1000),
            // 62D01D: this is the depth argument, not a screen-Y lift.
            z_adjust: (-15i32).wrapping_sub(lift) as f32,
            z_gradient: ZGradient::Vertical as u32,
            ..Default::default()
        },
    };
    if let (Some(rows), Some(system)) = (observations.as_mut(), system) {
        rows.push(ParticleDrawObservation {
            system_id: system.stable_id,
            particle_id: parent.id,
            particle_native_id: particle.native_unique_id(),
            display_layer: parent.layer.0,
            display_order: parent.display_order,
            image: image.to_owned(),
            frame,
            atlas_page: entry.page as usize,
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
