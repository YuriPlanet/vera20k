//! Particle-system instance builder — Layer 3 (above all ground objects).
//!
//! Reads `Simulation::particle_systems()` and emits one SpriteInstance per live
//! particle, dispatched on the per-system BehavesLike for frame-index
//! calculation. Smoke/Gas use `animation_state` as the frame directly; Fire
//! uses `facing_band * EndStateAI + animation_state`. Spark/Railgun (Tier 3)
//! are filtered at spawn but a defensive once-per-type warn-log catches any
//! that slip through.
//!
//! Output pages match the sprite atlas page layout — particle pass uses its
//! own pool stream ("particle_page", one buffer per atlas page) drawn after
//! bridge railings and before debug overlays.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use crate::app::AppState;
use crate::map::terrain;
use crate::render::batch::SpriteInstance;
use crate::render::sprite_atlas::ShpSpriteKey;
use crate::rules::house_colors::HouseColorIndex;
use crate::rules::particle_system_type::ParticleSystemBehavesLike;
use crate::rules::particle_type::ParticleBehavesLike;

use super::helpers::in_view;

/// Screen-Y nudge applied to every particle position. The original engine
/// shifts by `-15 - AdjustForZ()`; for Tier-2 (no airborne particles)
/// AdjustForZ is 0, so −15 px is the lift that puts smoke origins just
/// above the spawn cell instead of buried in it.
const PARTICLE_Y_LIFT: f32 = 15.0;

/// Build SpriteInstance entries for every live particle in the simulation.
///
/// Caller passes the paged output vector list (one Vec per atlas page, sized
/// `state.match_presentation.sprite_atlas.page_count()`). This function appends; sorting is the
/// caller's responsibility (see `build_world_instances`).
pub(crate) fn build_particle_instances(state: &AppState, paged: &mut [Vec<SpriteInstance>]) {
    let (sim, atlas, rules) = match (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        &state.match_state.match_presentation.sprite_atlas,
        state.rules().map(|r| r),
    ) {
        (Some(s), Some(a), Some(r)) => (s, a, r),
        _ => return,
    };

    let z = state.match_state.input.zoom_level;
    let (cam_x, cam_y, sw, sh) = (
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
        state.render_width() as f32 / z,
        state.render_height() as f32 / z,
    );

    for (_sys_id, sys) in sim.particle_systems().iter() {
        let pst = rules.particle_system_type(sys.type_id);
        match pst.behaves_like {
            ParticleSystemBehavesLike::Spark | ParticleSystemBehavesLike::Railgun => {
                warn_once_per_tier3_type(pst.behaves_like);
                continue;
            }
            _ => {}
        }

        for p in &sys.particles {
            let pt = rules.particle_type(p.type_id);
            let Some(image_name) = pt.image.as_deref() else {
                continue;
            };

            let frame: u16 = match pt.behaves_like {
                ParticleBehavesLike::Smoke | ParticleBehavesLike::Gas => p.animation_state as u16,
                ParticleBehavesLike::Fire => {
                    let facing_band = (sys.facing as u16 / 0x40) & 0x3;
                    facing_band * pt.end_state_ai as u16 + p.animation_state as u16
                }
                _ => continue,
            };

            let key = ShpSpriteKey {
                palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
                type_id: image_name.to_string(),
                facing: 0,
                frame,
                house_color: HouseColorIndex(0),
            };
            let Some(entry) = atlas.get(&key) else {
                continue;
            };

            let (sx, sy_raw) = terrain::lepton_to_screen(p.coords);
            let sy = sy_raw - PARTICLE_Y_LIFT;

            if !in_view(sx, sy, 64.0, 64.0, cam_x, cam_y, sw, sh, 120.0) {
                continue;
            }

            let alpha = match p.translucency {
                0x00 => 1.0,
                0x19 => 0.5,
                0x32 => 0.25,
                t if t >= 0x4A => 0.16,
                _ => 1.0,
            };

            // Y-descending depth so closer particles draw on top of farther
            // ones in the CPU sort (passthrough pipeline does no GPU depth
            // read/write, so this field only feeds sort_by_depth_desc).
            let depth = sy;

            paged[entry.page as usize].push(SpriteInstance {
                position: [sx + entry.offset_x, sy + entry.offset_y],
                size: entry.pixel_size,
                uv_origin: entry.uv_origin,
                uv_size: entry.uv_size,
                depth,
                tint: [1.0, 1.0, 1.0],
                alpha,
                ..Default::default()
            });
        }
    }
}

/// Once-per-type warn log for Tier-3 systems that slip past the spawn-side
/// filter. Defense in depth — the spawn side is the primary guard, this
/// catches snapshot loads / future bugs.
fn warn_once_per_tier3_type(kind: ParticleSystemBehavesLike) {
    static SEEN: OnceLock<Mutex<HashSet<ParticleSystemBehavesLike>>> = OnceLock::new();
    let seen = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
    if let Ok(mut set) = seen.lock() {
        if set.insert(kind) {
            log::warn!(
                "particles: render found Tier-3 system {:?} in store \
                 (spawn-side filter should have caught this); skipping",
                kind
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn translucency_byte_to_alpha_table() {
        fn alpha(b: u8) -> f32 {
            match b {
                0x00 => 1.0,
                0x19 => 0.5,
                0x32 => 0.25,
                t if t >= 0x4A => 0.16,
                _ => 1.0,
            }
        }
        assert_eq!(alpha(0x00), 1.0);
        assert_eq!(alpha(0x19), 0.5);
        assert_eq!(alpha(0x32), 0.25);
        assert_eq!(alpha(0x4A), 0.16);
        assert_eq!(alpha(0xFF), 0.16);
        assert_eq!(alpha(0x40), 1.0);
    }

    #[test]
    fn fire_frame_uses_facing_band_times_end_state() {
        fn fire_frame(facing: u8, end_state_ai: u8, animation_state: u8) -> u16 {
            let facing_band = (facing as u16 / 0x40) & 0x3;
            facing_band * end_state_ai as u16 + animation_state as u16
        }
        assert_eq!(fire_frame(0x1D, 19, 5), 5);
        assert_eq!(fire_frame(0x40, 19, 5), 24);
        assert_eq!(fire_frame(0x80, 19, 5), 43);
        assert_eq!(fire_frame(0xC0, 19, 5), 62);
    }
}
