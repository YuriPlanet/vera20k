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
    pub polygon: [[i32; 2]; 6],
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
            let polygon = wave.sonic_draw_polygon([0, 0])?;
            if !weapon_wave_visible(sim, observer, wave) {
                return None;
            }
            Some(WeaponWaveVisual {
                polygon,
                // Sonic samples the destination framebuffer;
                // they never select a house remap. The current sprite batch
                // has no framebuffer-distortion input, so it remains neutral.
                tint: [1.0, 1.0, 1.0],
            })
        })
        .collect()
}

/// DrawIt75F9F0's shared endpoint fog admission. Each raster consumes the
/// same simulation coordinates and the same native gate.
fn weapon_wave_visible(
    sim: &Simulation,
    observer: Option<crate::sim::intern::InternedId>,
    wave: &crate::sim::wave::Wave,
) -> bool {
    let Some(observer) = observer else {
        return true;
    };
    let cell = |point: crate::sim::projectile::ProjectileCoord| {
        Some((
            u16::try_from(point.x.div_euclid(256)).ok()?,
            u16::try_from(point.y.div_euclid(256)).ok()?,
        ))
    };
    let (Some((sx, sy)), Some((tx, ty))) = (cell(wave.source), cell(wave.target)) else {
        return false;
    };
    wave.visible_through_fog(
        sim.session.game_options().fog_of_war,
        !sim.fog.is_cell_revealed(observer, sx, sy),
        !sim.fog.is_cell_revealed(observer, tx, ty),
    )
}

/// Native type3 geometry is owned by WaveClass, including the Unit endpoint
/// reversal and projected fade. Rendering translates it once and supplies the
/// current packed destination; it never rebuilds a quad from world endpoints.
pub(crate) fn build_magnetic_wave_draws(
    sim: &Simulation,
    observer: Option<crate::sim::intern::InternedId>,
    camera: [i32; 2],
) -> Vec<(u64, crate::render::wave_geometry::MagneticWaveDraw)> {
    use crate::render::wave_geometry::{MagneticClipPath, MagneticWaveDraw};

    sim.waves
        .iter()
        .filter_map(|(&id, wave)| {
            let geometry = wave.magnetic_draw_geometry(camera)?;
            if !weapon_wave_visible(sim, observer, wave) {
                return None;
            }
            Some((
                id,
                MagneticWaveDraw {
                    polygon: geometry.polygon,
                    source_point: geometry.source_point,
                    source_z: wave.source.z,
                    phase: wave.lifetime,
                    direction: u8::try_from(wave.direction_octant).ok()?,
                    // VERA's existing scene Z is contiguous. Native ring
                    // fallback can differ by one clipping-edge pixel; C32's
                    // production footprints remain inside both margins.
                    clip_path: MagneticClipPath::Fast,
                },
            ))
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

    /// Dustbowl/Battle's production RULESMD/LANGRULE/mode/map reader, then
    /// the actual object visit through bullet, AnimClass and positioned audio.
    /// The native GetFLH corpus bounds the displacement; this is a Rust
    /// integration regression, not a native whole-scene damage comparison.
    #[test]
    fn stock_voxel_turret_shot_positions_bullet_flash_and_report_together() {
        use crate::rules::art_data::ArtRegistry;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::anim_class::AnimWorldCoord;
        use crate::sim::combat::{TargetKind, fire_coord};
        use crate::sim::movement::FacingClass;

        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
        // A deliberate control of the final processed projection. The stock
        // candidate above retains the real source selection and every layer;
        // only GTGCAN's voxel flag is cleared in this synthetic control.
        let mut control_ini = (*retail.processed_rules).clone();
        control_ini.merge(&crate::rules::ini_parser::IniFile::from_str(
            "[GTGCAN]\nTurretAnimIsVoxel=false\n",
        ));
        let mut control =
            RuleSet::from_ini_with_fixed_art_for_test(&control_ini, &retail.fixed_art).unwrap();
        control.install_art_data(ArtRegistry::from_ini(&retail.fixed_art));
        let mut stock = retail.rules;
        let gun = stock.object("GTGCAN").unwrap();
        let bare = control.object("GTGCAN").unwrap();
        assert!(gun.turret_anim_is_voxel && !bare.turret_anim_is_voxel);
        assert_eq!(
            (gun.turret_anim_x, gun.turret_anim_y),
            (bare.turret_anim_x, bare.turret_anim_y)
        );
        assert_eq!(gun.primary(), bare.primary());
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/building_voxel_flh.json",
        ))
        .unwrap();
        let native_source = |name: &str| {
            let row = native["voxel_flh"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["input"]["name"] == name)
                .unwrap();
            std::array::from_fn::<_, 3, _>(|axis| row["source"][axis].as_i64().unwrap() as i32)
        };
        let voxel = native_source("gtgcan_burst0");
        let base = native_source("voxel_disabled");
        let displacement = (voxel[0] - base[0], voxel[1] - base[1]);

        let shoot = |rules: &mut RuleSet| {
            let weapon = rules
                .weapon(rules.object("GTGCAN").unwrap().primary().unwrap())
                .unwrap();
            let flashes = weapon.anim.clone();
            assert!(!flashes.is_empty() && !weapon.report.is_empty());
            // Bind the real SHP bytes so absence of an animation cannot pass.
            for flash in flashes {
                rules
                    .bind_authored_load_anim(&flash, &assets, "tem", "TEMPERATE")
                    .unwrap();
            }
            let mut sim = Simulation::new();
            crate::sim::arena_fixture::flat_ground(&mut sim, rules);
            // Keep both actors and the bullet inside MapClass's Size diamond
            // (the arena's Size width is 20); generous LocalSize alone does
            // not admit a bullet from the former (8,8) source.
            let gun = sim
                .spawn_object("GTGCAN", "French", 12, 12, 0, rules)
                .unwrap();
            let target = sim
                .spawn_object("NAPOWR", "Russians", 16, 12, 0, rules)
                .unwrap();
            let facing = fire_coord::building_direction_to(
                &sim,
                rules,
                sim.substrate.entities.get(gun).unwrap(),
                TargetKind::Entity(target),
            )
            .unwrap();
            sim.substrate.entities.get_mut(gun).unwrap().body_facing = FacingClass::new(facing, 0);
            sim.assign_target_represented(
                gun,
                Some(TargetKind::Entity(target)),
                Some(rules),
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
            for _ in 0..300 {
                sim.fire_events.clear();
                let before: std::collections::BTreeSet<_> =
                    sim.projectiles.iter().map(|(&id, _)| id).collect();
                sim.advance_tick(&[], Some(rules), None, None, 67);
                let Some(event) = sim
                    .fire_events
                    .iter()
                    .find(|event| event.attacker_id == gun)
                else {
                    continue;
                };
                let event = event.clone();
                let shell = sim
                    .projectiles
                    .iter()
                    .find(|(id, projectile)| projectile.source_id == gun && !before.contains(id))
                    .unwrap_or_else(|| panic!(
                        "a real Grand Cannon shell: frame={} shot={event:?} live_bullets={:?} target_health={:?}",
                        sim.session.binary_frame(),
                        sim.projectiles.iter().collect::<Vec<_>>(),
                        sim.substrate.entities.get(target).map(|entity| entity.health)
                    ))
                    .1;
                assert_eq!(shell.launch_origin, event.fire_coord);
                let flash_type = event.muzzle_anim.expect("stock weapon selects a flash");
                let (flash_id, flash) = sim
                    .substrate
                    .anims
                    .iter()
                    .find(|(_, anim)| anim.type_id == flash_type)
                    .expect("the real muzzle AnimClass exists");
                assert_eq!(
                    sim.anim_absolute_coord(*flash_id),
                    Some(AnimWorldCoord {
                        x: event.fire_coord.x,
                        y: event.fire_coord.y,
                        z: event.fire_coord.z,
                    })
                );
                assert!(
                    flash.owner_entity.is_none(),
                    "a building flash stays at its birth"
                );
                assert_eq!(
                    flash.z_adjust,
                    fire_coord::building_muzzle_z_adjust(event.fire_offset_y, false)
                );
                let sounds = weapon_report_sounds(&sim, std::slice::from_ref(&event));
                let coord = event.fire_coord;
                let expected_source = SoundSource::new(
                    crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z),
                    ((coord.x / 256) as u16, (coord.y / 256) as u16),
                );
                match sounds.as_slice() {
                    [GameSoundEvent::WeaponFired { sound_id, source }] => {
                        assert_eq!(
                            sound_id,
                            sim.interner.resolve(event.report_sound_id.unwrap())
                        );
                        assert_eq!(*source, Some(expected_source));
                    }
                    other => panic!("unexpected stock shot audio: {other:?}"),
                }
                return (event, facing);
            }
            panic!("stock Grand Cannon did not fire");
        };
        let (shot, facing) = shoot(&mut stock);
        let (bare, control_facing) = shoot(&mut control);
        assert_eq!(facing, control_facing, "the control holds turret aim fixed");
        assert_eq!(
            (
                shot.fire_coord.x - bare.fire_coord.x,
                shot.fire_coord.y - bare.fire_coord.y
            ),
            displacement
        );
        assert_eq!(shot.fire_coord.z, bare.fire_coord.z);
        assert_eq!(shot.fire_offset_y - bare.fire_offset_y, displacement.1);
    }
}
