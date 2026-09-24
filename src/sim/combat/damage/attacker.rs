//! `TechnoClass::FireAt @ 0x006FDD50`'s damage build (`0x006FE306..0x006FE45C`):
//! the integer damage the shot carries into its bullet, wave or immediate
//! detonation.
//!
//! In order, each stage an x87 multiply (53-bit, chop) and `Math::ftol`'s low
//! 32 bits:
//! 1. `Damage=` (WeaponType `+0xA4`), zeroed for `IsSonic=` (`+0x130`) or
//!    `UseFireParticles=` (`+0x129`): the Sonic wave (its `AmbientDamage=`)
//!    and the fire particles hurt instead of the bullet
//!    (`0x006FE306..0x006FE32A`).
//! 2. For a positive damage only (`JLE @ 0x006FE331`), the firepower fold
//!    `ftol(House+0x188 * Techno+0x160 * damage)` (`0x006FE33D..0x006FE34D`),
//!    then the FIREPOWER rank stage `ftol(damage * VeteranCombat)`
//!    (`0x006FE3C8..0x006FE3D8`). A heal is never rank-scaled.
//! 3. For any sign: an occupied building (`vt+0x400`) times the f32
//!    `OccupyDamageMultiplier=` (`0x006FE3F1`), a bunker-linked non-building
//!    (`+0x2E4`, `vt+0x2C != 6`) times `BunkerDamageMultiplier=`
//!    (`0x006FE421`), and a passenger in an open-topped transport (`+0x82`)
//!    times `OpenToppedDamageMultiplier=` (`0x006FE445`).
//!
//! Native execution: `tools/spatial_oracle/damage_build.py` (`fire` rows).

use crate::util::native_x87::{MaskedX87Chop53 as X, NativeF32Bits, NativeF64Bits};

/// The firer-side inputs of the damage build; each `Option` is `None` while
/// its native gate is closed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FireDamageStages {
    /// `House+0x188`: `[Easy]/[Normal]/[Difficult] FirePower=` times the
    /// country's `Firepower=` (`HouseClass::SetDifficulty @ 0x004F6EC0`,
    /// `0x004F6F04..0x004F6F11`).
    pub house_firepower: NativeF64Bits,
    /// `Techno+0x160`, raised only by a Firepower crate.
    pub unit_firepower: NativeF64Bits,
    /// `Rules+0x670` `VeteranCombat=` when the firer's rank holds FIREPOWER.
    pub rank_firepower: Option<NativeF64Bits>,
    /// `Rules+0xF40` `OccupyDamageMultiplier=` for an occupied building.
    pub occupied: Option<NativeF32Bits>,
    /// `Rules+0xF4C` `BunkerDamageMultiplier=` for a bunkered non-building.
    pub bunkered: Option<NativeF32Bits>,
    /// `Rules+0xF58` `OpenToppedDamageMultiplier=` for an open-topped passenger.
    pub open_topped: Option<NativeF32Bits>,
}

/// The damage `FireAt` gives the shot; see the module documentation.
/// `zeroed` is the weapon's `IsSonic=` or `UseFireParticles=`.
pub(crate) fn fire_damage(damage: i32, zeroed: bool, stages: &FireDamageStages) -> i32 {
    let mut damage = if zeroed { 0 } else { damage };
    if damage > 0 {
        // FLD House+0x188; FMUL Techno+0x160; FIMUL damage.
        damage = X::ftol_i32_low_masked(X::mul(
            X::mul(
                X::load_f64(stages.house_firepower),
                X::load_f64(stages.unit_firepower),
            ),
            X::load_i32(damage),
        ));
        if let Some(multiplier) = stages.rank_firepower {
            damage = X::ftol_i32_low_masked(X::mul(X::load_i32(damage), X::load_f64(multiplier)));
        }
    }
    for multiplier in [stages.occupied, stages.bunkered, stages.open_topped]
        .into_iter()
        .flatten()
    {
        damage = X::ftol_i32_low_masked(X::mul(X::load_i32(damage), X::load_f32(multiplier)));
    }
    damage
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::object_type::Ability;
    use crate::sim::combat::damage::receive::tests::object_with_ability;
    use crate::sim::combat::veterancy::{has_weapon_ability, rank_of};

    /// Native execution of `0x006FE302..0x006FE460`
    /// (`tools/spatial_oracle/damage_build.py`, `fire` rows): VERA's
    /// FIREPOWER gate takes the branch native took, and the build reproduces
    /// every result, including heals, int32 overflow and the zeroing flags.
    #[test]
    fn original_fire_damage_rows() {
        #[derive(serde::Deserialize)]
        struct Corpus {
            fire: Vec<Row>,
        }
        #[derive(serde::Deserialize)]
        struct Row {
            input: Input,
            stages: Vec<String>,
            damage: i32,
        }
        #[derive(serde::Deserialize)]
        struct Input {
            damage: i32,
            sonic: u8,
            fire_particles: u8,
            veterancy: u32,
            veteran_firepower: u8,
            elite_firepower: u8,
            occupied: u8,
            bunker_link: u8,
            whatami: u32,
            open_topped: u8,
            veteran_combat: u32,
            occupy_mult: u32,
            bunker_mult: u32,
            open_topped_mult: u32,
            house_firepower: Option<u64>,
            unit_firepower: Option<u64>,
        }
        let corpus: Corpus = serde_json::from_str(include_str!(
            "../../../../tools/spatial_oracle/damage_build.json"
        ))
        .expect("original damage-build corpus");
        assert_eq!(corpus.fire.len(), 629);
        for (index, row) in corpus.fire.iter().enumerate() {
            let input = &row.input;
            let staged = |name: &str| row.stages.iter().any(|stage| stage == name);
            let zeroed = input.sonic != 0 || input.fire_particles != 0;
            let object = object_with_ability(
                Ability::Firepower,
                input.veteran_firepower != 0,
                input.elite_firepower != 0,
            );
            let firepower = has_weapon_ability(
                rank_of(NativeF32Bits::from_bits(input.veterancy)),
                &object,
                Ability::Firepower,
            );
            let positive = !zeroed && input.damage > 0;
            assert_eq!(positive, staged("firepower_fold"), "row {index}: sign gate");
            assert_eq!(
                positive && firepower,
                staged("rank_firepower"),
                "row {index}: FIREPOWER gate"
            );
            let bunkered = input.bunker_link != 0 && input.whatami != 6;
            assert_eq!(bunkered, staged("bunkered"), "row {index}: bunker gate");
            let f32_bits = NativeF32Bits::from_bits;
            let stages = FireDamageStages {
                house_firepower: NativeF64Bits::from_bits(
                    input.house_firepower.unwrap_or(1.0_f64.to_bits()),
                ),
                unit_firepower: NativeF64Bits::from_bits(
                    input.unit_firepower.unwrap_or(1.0_f64.to_bits()),
                ),
                // ReadDouble's value: the parsed single widened.
                rank_firepower: firepower.then(|| {
                    NativeF64Bits::from_bits(
                        f64::from(f32::from_bits(input.veteran_combat)).to_bits(),
                    )
                }),
                occupied: (input.occupied != 0).then(|| f32_bits(input.occupy_mult)),
                bunkered: bunkered.then(|| f32_bits(input.bunker_mult)),
                open_topped: (input.open_topped != 0).then(|| f32_bits(input.open_topped_mult)),
            };
            assert_eq!(
                fire_damage(input.damage, zeroed, &stages),
                row.damage,
                "row {index}"
            );
        }
    }
}
