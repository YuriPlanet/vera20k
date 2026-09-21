//! App-owned weapon fire presentation: positioned weapon report sounds and
//! persistent wave geometry.
//!
//! The sim owns the fire coordinate (`sim::combat::fire_coord`) and constructs
//! the muzzle animations itself as `AnimClass` objects; this module only
//! projects that coordinate for audio and lowers waves to render geometry.

use crate::app::AppState;
use crate::audio::events::{GameSoundEvent, SoundSource};
use crate::sim::world::{SimFireEvent, Simulation};

/// One persistent WaveClass draw resolved from simulation-owned registration state.
#[derive(Debug, Clone)]
pub(crate) struct WeaponWaveVisual {
    pub geometry: crate::render::wave_geometry::WaveGeometryInput,
    pub tint: [f32; 3],
}

/// The weapon `Report=` cues of a batch of shots, each positioned at its
/// shot's fire coordinate.
///
/// gamemd-derived: `TechnoClass::Fire_At` plays the report at the same local
/// it hands the muzzle animation (`0x006FF372..0x006FF38F`).
fn weapon_report_sounds(sim: &Simulation, events: &[SimFireEvent]) -> Vec<GameSoundEvent> {
    events
        .iter()
        .filter_map(|event| {
            let report_id = event.report_sound_id?;
            let coord = event.fire_coord;
            let screen = crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
            let cell = (
                coord.x.div_euclid(256).clamp(0, i32::from(u16::MAX)) as u16,
                coord.y.div_euclid(256).clamp(0, i32::from(u16::MAX)) as u16,
            );
            Some(GameSoundEvent::WeaponFired {
                sound_id: sim.interner.resolve(report_id).to_string(),
                source: Some(SoundSource::new(screen, cell)),
            })
        })
        .collect()
}

pub(crate) fn build_weapon_wave_visuals(
    sim: &Simulation,
    observer: Option<crate::sim::intern::InternedId>,
) -> Vec<WeaponWaveVisual> {
    sim.waves
        .iter()
        .filter_map(|(_, wave)| {
            let (kind, wave_type) = match wave.wave_type {
                0 => (
                    crate::render::wave_geometry::WaveGeometryKind::NonMagnetic,
                    0,
                ),
                3 => (crate::render::wave_geometry::WaveGeometryKind::Magnetic, 3),
                // Types 1/2 use the fixed laser rasterizer, which the current
                // white-pixel beam backend does not emulate.
                _ => return None,
            };
            let cell = |point: crate::sim::projectile::ProjectileCoord| {
                let rx = u16::try_from(point.x.div_euclid(256)).ok()?;
                let ry = u16::try_from(point.y.div_euclid(256)).ok()?;
                Some((rx, ry))
            };
            if let Some(observer) = observer {
                let (source_rx, source_ry) = cell(wave.source)?;
                let (target_rx, target_ry) = cell(wave.target)?;
                let source_fogged = !sim.fog.is_cell_revealed(observer, source_rx, source_ry);
                let target_fogged = !sim.fog.is_cell_revealed(observer, target_rx, target_ry);
                if !wave.visible_through_fog(
                    sim.session.game_options.fog_of_war,
                    source_fogged,
                    target_fogged,
                ) {
                    return None;
                }
            }
            Some(WeaponWaveVisual {
                geometry: crate::render::wave_geometry::WaveGeometryInput {
                    kind,
                    wave_type,
                    a: crate::render::wave_geometry::WavePoint {
                        x: wave.source.x,
                        y: wave.source.y,
                        z: wave.source.z,
                    },
                    b: crate::render::wave_geometry::WavePoint {
                        x: wave.target.x,
                        y: wave.target.y,
                        z: wave.target.z,
                    },
                },
                // Sonic and Magnetron sample the destination framebuffer;
                // they never select a house remap. The current sprite batch
                // has no framebuffer-distortion input, so it remains neutral.
                tint: [1.0, 1.0, 1.0],
            })
        })
        .collect()
}

pub(crate) fn queue_weapon_report_sounds(state: &mut AppState, events: &[SimFireEvent]) {
    let sounds = {
        let Some(sim) = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
        else {
            return;
        };
        weapon_report_sounds(sim, events)
    };
    for sound in sounds {
        state.match_state.match_audio.sound_events.push(sound);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_sound_is_positioned_at_the_sim_fire_coordinate() {
        let mut sim = Simulation::new();
        let report = sim.interner.intern("GIAttack");
        let mut event = crate::sim::world::SimFireEvent::for_test(1);
        event.report_sound_id = Some(report);
        event.fire_coord =
            crate::sim::projectile::ProjectileCoord::new(10 * 256 + 200, 11 * 256 + 40, 105);

        let sounds = weapon_report_sounds(&sim, &[event.clone()]);
        let expected =
            crate::util::lepton::absolute_leptons_to_screen(10 * 256 + 200, 11 * 256 + 40, 105);
        match sounds.as_slice() {
            [GameSoundEvent::WeaponFired { sound_id, source }] => {
                assert_eq!(sound_id, "GIAttack");
                assert_eq!(*source, Some(SoundSource::new(expected, (10, 11))));
            }
            other => panic!("unexpected sound events: {other:?}"),
        }

        event.report_sound_id = None;
        assert!(weapon_report_sounds(&sim, &[event]).is_empty());
    }

    #[test]
    fn persistent_sonic_wave_produces_geometry() {
        let mut sim = Simulation::new();
        let wave_id = sim.allocate_stable_id();
        sim.admit_wave(
            wave_id,
            crate::sim::wave::Wave::new(
                0,
                crate::sim::projectile::ProjectileCoord::new(10 * 256, 10 * 256, 0),
                crate::sim::projectile::ProjectileCoord::new(14 * 256, 11 * 256, 0),
            ),
        );

        let waves = build_weapon_wave_visuals(&sim, None);
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0].geometry.wave_type, 0);
        assert_eq!(
            waves[0].geometry.kind,
            crate::render::wave_geometry::WaveGeometryKind::NonMagnetic
        );
        assert_eq!(
            crate::render::wave_geometry::draw_order(waves[0].geometry).len(),
            6
        );
    }
}
