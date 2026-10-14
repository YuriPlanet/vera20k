//! SHP particles in their retained native Display slots.
//!
//! Particle62BB0E -> ObjectUnlimbo5F4EC0 queries GetLayer62D770 (AIR3),
//! then Submit4A9720 appends the Particle, independently of its System.
//! Tactical6D8DB0 visits those registrations in retained order and reaches
//! ParticleDraw62CEC0 through ObjectDrawIfVisible5F4B10. The existing tactical
//! lowering, indexed atlas, palette and packed compositor consume each piece.
//! Native caller/type/draw evidence: tools/spatial_oracle/damage_smoke.md.

use crate::app::AppState;
use crate::app::presentation::render::draw_plan_lowering::{
    NativeDisplayOrder, ObjectPieceInstance, ObjectTexture, PlannedObjectInstance,
};
use crate::map::terrain;
use crate::render::batch::SpriteInstance;
use crate::render::draw_state::DrawState;
use crate::render::native_z::ZGradient;
use crate::render::palette_light::PaletteLight;
use crate::render::sprite_atlas::{ShpPaletteContext, ShpSpriteKey};
use crate::render::tactical_draw_plan::{BlitPolicy, RenderZPolicy, SpriteEncoding};
use crate::rules::house_colors::HouseColorIndex;
use crate::rules::particle_type::ParticleBehavesLike;
use crate::util::native_x87::adjust_for_z_standard;

use super::helpers::{compute_sprite_depth_params_lifted, depth_axis, projection_admitted};

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

/// Append each SHP Particle as its own retained Display parent. A Move-dead
/// Particle retains that registration for its last draw; the lifecycle owner
/// removes it, rather than a marked-for-deletion render filter.
pub(crate) fn build_particle_instances(
    state: &AppState,
    objects: &mut Vec<PlannedObjectInstance>,
    order: &NativeDisplayOrder,
) -> Option<Vec<ParticleDrawObservation>> {
    let mut observations = state.diagnostic_presentation_ms().map(|_| Vec::new());
    let (Some(runtime), Some(atlas)) = (
        state.match_state.sim_runtime.as_ref(),
        state.match_state.match_presentation.sprite_atlas.as_ref(),
    ) else {
        return observations;
    };
    let sim = &runtime.simulation;
    let rules = &runtime.resources.rules;
    let presentation = &state.match_state.match_presentation;
    let detail_level = presentation.in_game_options.detail_level;
    let input = &state.match_state.input;
    let (_, _, width, height) = crate::app::input::camera::tactical_viewport_px(state);
    let viewport = [
        width as f32 / input.zoom_level,
        height as f32 / input.zoom_level,
    ];
    let camera = [input.camera_x, input.camera_y];
    let axis = depth_axis(state);

    // Frame-local borrowed inputs, derived solely from retained Display ranks.
    // Drop them after this build. DetailState's mutative 55AF60 query must run
    // in AIR registration order, before later atlas lowering can restore it.
    let mut particles: Vec<_> = sim
        .particle_systems()
        .iter()
        .flat_map(|(&system_id, system)| {
            system.particles.iter().filter_map(move |particle| {
                order
                    .object_draw(particle.stable_id(), SpriteEncoding::Plain)
                    .map(|parent| (system_id, system, particle, parent))
            })
        })
        .collect();
    particles.sort_unstable_by_key(|(_, _, _, parent)| (parent.layer.0, parent.display_order));

    for (system_id, system, particle, mut parent) in particles {
        let point = terrain::lepton_to_screen(particle.coords);
        if !projection_admitted([point.0, point.1], camera, viewport) {
            continue;
        }
        let kind = rules.particle_type(particle.type_id);
        // ParticleDraw62CEC0 reads the shared unsigned FPS/minimum owner.
        // Damaging particles alone can pass a below-minimum frame rate.
        let enough_fps = {
            let mut detail = presentation.detail.borrow_mut();
            detail.frame_rate() >= detail.minimum_frame_rate()
        };
        if !enough_fps && kind.damage == 0 {
            continue;
        }
        if detail_level == 0
            && matches!(
                kind.behaves_like,
                ParticleBehavesLike::Smoke | ParticleBehavesLike::Spark
            )
        {
            continue;
        }
        let Some(image) = kind.image.as_deref() else {
            continue;
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
                let band = (u16::from(system.facing) / 0x40) & 3;
                i32::from(band * u16::from(kind.end_state_ai) + u16::from(particle.animation_state))
            }
            ParticleBehavesLike::Spark | ParticleBehavesLike::Railgun => continue,
        };
        let Ok(frame) = u16::try_from(native_frame) else {
            continue;
        };
        let key = ShpSpriteKey {
            palette_context: ShpPaletteContext::GlobalAnim,
            type_id: image.to_owned(),
            facing: 0,
            frame,
            house_color: HouseColorIndex(0),
        };
        let Some(entry) = atlas.get(&key) else {
            continue;
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
        if let Some(rows) = &mut observations {
            rows.push(ParticleDrawObservation {
                system_id,
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
    observations
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
