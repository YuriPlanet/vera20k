//! App-owned runtime for transient combat-light presentation.
//!
//! The simulation emits receiver facts; this module materializes the native
//! 24-byte light-vector payload, ages it on committed logic frames, and hands
//! reverse insertion order to the dedicated tactical renderer. It deliberately
//! owns no gameplay state and never enters snapshots or world hashes.

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::CombatLightRequest;
use crate::sim::intern::StringInterner;
use crate::sim::projectile::ProjectileCoord;

const STAGE_STEP: u8 = 8;
const EXPIRE_STAGE: u8 = 0x50;

/// Native stage-to-surface scale table. Persistent lights only observe the
/// entries at stages 0, 8, ..., 72, but retaining the complete table makes the
/// integer indexing contract explicit.
#[rustfmt::skip]
const STAGE_SCALE: [u8; 80] = [
     5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 61, 62, 63, 63,
    63, 62, 61, 60, 59, 58, 57, 56, 55, 54, 53, 52, 51, 50, 49, 48,
    47, 46, 45, 44, 43, 42, 41, 40, 39, 38, 37, 36, 35, 34, 33, 32,
    31, 30, 29, 28, 27, 26, 25, 24, 23, 22, 21, 20, 19, 18, 17, 16,
    15, 14, 13, 12, 11, 10,  9,  8,  7,  6,  5,  4,  3,  2,  1,  0,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CombatLight {
    pub coord: ProjectileCoord,
    pub stage: u8,
    pub base_size: u8,
    pub flags: u32,
}

pub(crate) use crate::render::combat_light::CombatLightDrawRecord;

#[derive(Debug, Default)]
pub(crate) struct CombatLightRuntime {
    entries: Vec<CombatLight>,
}

impl CombatLightRuntime {
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Age the pre-existing vector before this logic frame's producers append.
    pub(crate) fn commit_frame(&mut self, new_entries: impl IntoIterator<Item = CombatLight>) {
        for entry in &mut self.entries {
            entry.stage = entry.stage.saturating_add(STAGE_STEP);
        }
        self.entries.retain(|entry| entry.stage < EXPIRE_STAGE);
        self.entries.extend(new_entries);
    }

    /// Persistent draw-all snapshots the vector then walks tail to head.
    pub(crate) fn draw_records(&self) -> Vec<CombatLightDrawRecord> {
        self.entries
            .iter()
            .rev()
            .map(|entry| CombatLightDrawRecord {
                coord: entry.coord,
                surface_index: scaled_surface_index(entry.base_size, entry.stage),
                flags: entry.flags,
            })
            .collect()
    }
}

/// Materialize current-frame receiver records into the final native light-vector
/// fields. Helper-only provenance (target, warhead, damage) intentionally stops
/// at this boundary.
pub(crate) fn materialize_combat_lights(
    impacts: Vec<CombatLightRequest>,
    rules: Option<&RuleSet>,
    interner: &StringInterner,
) -> Vec<CombatLight> {
    impacts
        .into_iter()
        .filter_map(|effect| {
            let warhead =
                rules.and_then(|rules| rules.warhead(interner.resolve(effect.warhead_ref)));
            if !effect.force_create && !warhead.is_some_and(|warhead| warhead.bright) {
                return None;
            }
            let override_size = warhead.map_or(0.0, |warhead| warhead.combat_light_size_f64);
            Some(CombatLight {
                coord: effect.coord,
                stage: 0,
                base_size: materialize_base_size(effect.damage, override_size),
                flags: effect.flags,
            })
        })
        .collect()
}

fn materialize_base_size(damage: i32, override_size: f64) -> u8 {
    if override_size > 0.0 {
        // ReadDouble is f32-first; positive values are capped only at the high
        // end and Math::ftol chops the product toward zero.
        return (override_size.min(1.0) * 63.0) as u8;
    }
    // Preserve the actual signed 32-bit shifts. `/ 4` differs for negative
    // values and cannot reproduce the wrapping left shift.
    ((damage.wrapping_shl(6) >> 8).clamp(0x15, 0x3f)) as u8
}

fn scaled_surface_index(base_size: u8, stage: u8) -> u8 {
    let scale = STAGE_SCALE[usize::from(stage.min(79))];
    (u16::from(base_size) * u16::from(scale) / 64) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::world::Simulation;

    fn effect(sim: &mut Simulation, x: i32, flags: u32, damage: i32) -> CombatLightRequest {
        CombatLightRequest {
            target_id: Some(x as u64),
            damage,
            warhead_ref: sim.interner.intern("WH"),
            coord: ProjectileCoord { x, y: 512, z: 0 },
            force_create: true,
            flags,
        }
    }

    #[test]
    fn gsi_04_07_invulnerability_light_materializes_and_draws_reverse_order() {
        let ini = IniFile::from_str("[Warheads]\n0=WH\n[WH]\nCombatLightSize=40%\n");
        let rules = RuleSet::from_ini(&ini).expect("rules");
        let mut sim = Simulation::new();
        let first = effect(&mut sim, 256, 1, 400);
        let second = effect(&mut sim, 768, 6, 400);
        let mut runtime = CombatLightRuntime::default();
        let new_entries =
            materialize_combat_lights(vec![first, second], Some(&rules), &sim.interner);
        assert_eq!(
            new_entries.iter().map(|e| e.base_size).collect::<Vec<_>>(),
            vec![25, 25]
        );
        runtime.commit_frame(new_entries);

        let draw = runtime.draw_records();
        assert_eq!(
            draw.iter().map(|r| r.coord.x).collect::<Vec<_>>(),
            vec![768, 256]
        );
        assert_eq!(draw.iter().map(|r| r.flags).collect::<Vec<_>>(), vec![6, 1]);
        assert_eq!(
            draw.iter().map(|r| r.surface_index).collect::<Vec<_>>(),
            vec![1, 1]
        );
        assert!(materialize_combat_lights(Vec::new(), Some(&rules), &sim.interner).is_empty());
    }

    #[test]
    fn gsi_04_07_invulnerability_light_stages_zero_through_72_then_expires_before_80() {
        let mut runtime = CombatLightRuntime::default();
        runtime.commit_frame([CombatLight {
            coord: ProjectileCoord { x: 0, y: 0, z: 0 },
            stage: 0,
            base_size: 63,
            flags: 1,
        }]);
        let mut observed = vec![(0, runtime.draw_records()[0].surface_index)];
        for _ in 0..9 {
            runtime.commit_frame([]);
            let entry = runtime.entries[0];
            observed.push((entry.stage, runtime.draw_records()[0].surface_index));
        }
        assert_eq!(
            observed,
            vec![
                (0, 4),
                (8, 44),
                (16, 62),
                (24, 54),
                (32, 46),
                (40, 38),
                (48, 30),
                (56, 22),
                (64, 14),
                (72, 6)
            ]
        );
        runtime.commit_frame([]);
        assert_eq!(runtime.len(), 0, "stage 80 is removed before draw");
    }

    #[test]
    fn gsi_04_07_invulnerability_light_damage_size_uses_wrapping_shift_and_override_chop() {
        assert_eq!(materialize_base_size(400, 0.0), 63);
        assert_eq!(materialize_base_size(-1, 0.0), 21);
        assert_eq!(materialize_base_size(i32::MAX, 0.0), 21);
        assert_eq!(materialize_base_size(1, 0.4_f32 as f64), 25);
        assert_eq!(materialize_base_size(1, 2.0), 63);
    }
}
