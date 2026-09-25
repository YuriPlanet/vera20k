//! `TechnoClass::EstimateDamage @ 0x006FDB80(target, weapon)`: the damage a
//! shot of `weapon` would deal `target`, which the target scan
//! (`Retaliate_And_Scan 0x007099B0`) and FireAt (`0x006FE622`) subtract from
//! the target's retained estimate (`Techno+0x70`,
//! [`crate::sim::estimated_health::EstimatedHealth`]).
//!
//! It chains the attacker's damage build with the receiver's divides and the
//! warhead kernel at distance 0, with one native quirk: the category armor is
//! the *target* house's multiplier for the *attacker's* type, times the
//! *attacker's* `ArmorMultiplier` (`0x006FDC5E..0x006FDC82`).
//!
//! Native execution: `tools/spatial_oracle/estimated_damage.py` runs the
//! original body with its type, rank, house-category, ftol and kernel callees;
//! [`tests`] replays every `estimator` row.

use super::attacker::{FireDamageStages, fire_damage};
use super::kernel::apply_warhead_damage;
use super::{ArmorClass, DefenceDivisors, receive::defence_divides};

/// The warhead the estimate detonates, as the kernel reads it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EstimateWarhead<'a> {
    pub cell_spread: f64,
    pub percent_at_max: f64,
    pub verses: &'a [f64; 11],
}

/// Everything `0x006FDB80` reads besides its callees' tables.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EstimateInputs<'a> {
    /// `Damage=` (WeaponType `+0xA4`).
    pub damage: i32,
    /// `IsSonic=` (`+0x130`) or `UseFireParticles=` (`+0x129`): no estimate.
    pub zeroed: bool,
    /// The attacker's firepower fold and FIREPOWER rank stage; the building,
    /// bunker and open-topped stages are FireAt's alone and stay `None`.
    pub stages: FireDamageStages,
    /// The target house's category multiplier for the attacker's type, the
    /// attacker's `ArmorMultiplier`, and `VeteranArmor=` when the *target's*
    /// rank holds STRONGER (`0x006FDC87..0x006FDCFE`).
    pub divisors: DefenceDivisors,
    /// `Warhead=` (`+0xAC`); `None` returns 0, as the kernel's null arm does.
    pub warhead: Option<EstimateWarhead<'a>>,
    /// The target type's `Armor=` (`+0x9C`).
    pub armor: ArmorClass,
    /// `ScenarioClass` flag `0x20` (no damage).
    pub scenario_no_damage: bool,
    /// `[CombatDamage] MaxDamage=`.
    pub max_damage: i32,
}

/// `0x006FDB80`: 0 for a sonic or fire-particle weapon, the raw `Damage=` when
/// it is not positive, otherwise the damage build, the divides (at least 1) and
/// `Apply_warhead_damage @ 0x00489180` at distance 0. A null target (0) is the
/// caller's.
pub(crate) fn estimated_damage(inputs: &EstimateInputs<'_>) -> i32 {
    if inputs.zeroed {
        return 0;
    }
    if inputs.damage <= 0 {
        return inputs.damage;
    }
    let built = fire_damage(inputs.damage, false, &inputs.stages);
    let divided = defence_divides(built, &inputs.divisors);
    let Some(warhead) = inputs.warhead else {
        return 0;
    };
    apply_warhead_damage(
        divided,
        warhead.cell_spread,
        warhead.percent_at_max,
        warhead.verses,
        inputs.armor,
        0,
        inputs.scenario_no_damage,
        inputs.max_damage,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
    use serde_json::Value;

    fn f64_bits(value: &Value) -> NativeF64Bits {
        NativeF64Bits::from_bits(u64::from_str_radix(value.as_str().unwrap(), 16).unwrap())
    }

    fn f32_bits(value: &Value) -> NativeF32Bits {
        NativeF32Bits::from_bits(u32::from_str_radix(value.as_str().unwrap(), 16).unwrap())
    }

    /// The x87 rank predicates `0x0074FF90` (veteran: `1.0 <= r < 2.0`) and
    /// `0x00750010` (elite: `r >= 2.0`); a NaN rank is neither.
    fn rank(bits: &Value) -> (bool, bool) {
        let rank = f32::from_bits(f32_bits(bits).bits());
        ((1.0..2.0).contains(&rank), rank >= 2.0)
    }

    fn widened_f32(bits: &Value) -> f64 {
        let raw = f32_bits(bits).bits();
        if raw & 0x7f80_0000 == 0x7f80_0000 {
            f64::from_bits(
                (u64::from(raw & 0x8000_0000) << 32)
                    | 0x7ff0_0000_0000_0000
                    | (u64::from(raw & 0x007f_ffff) << 29),
            )
        } else {
            f64::from(f32::from_bits(raw))
        }
    }

    /// `HouseClass::GetArmorMultForType @ 0x0050BD30` on the attacker's type
    /// RTTI: Aircraft 3, Building 7 (Defenses when `BuildCat=5`), Infantry 16,
    /// Unit 40, else 1.0.
    fn category_armor(input: &Value) -> NativeF32Bits {
        let rtti =
            input["attacker_type_rtti_override"]
                .as_i64()
                .unwrap_or(match input["attacker_class"].as_str().unwrap() {
                    "aircraft" => 3,
                    "building" => 7,
                    "infantry" => 16,
                    _ => 40,
                });
        let slot = match rtti {
            16 => 0,
            40 => 1,
            3 => 2,
            7 if input["attacker_build_category"] == 5 => 4,
            7 => 3,
            _ => return NativeF32Bits::from_bits(1.0_f32.to_bits()),
        };
        f32_bits(&input["target_country_armor"][slot])
    }

    #[test]
    fn estimated_damage_matches_the_original() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../../tools/spatial_oracle/estimated_damage.json"
        ))
        .unwrap();
        let mut compared = 0;
        for row in corpus["rows"].as_array().unwrap() {
            let input = &row["input"];
            if input["scope"] != "estimator" || input["target_present"] == false {
                continue;
            }
            let (attacker_veteran, attacker_elite) = rank(&input["attacker_rank"]);
            let (target_veteran, target_elite) = rank(&input["target_rank"]);
            let flag = |key: &str| input[key].as_u64().unwrap() != 0;
            let firepower = (attacker_veteran && flag("attacker_veteran_combat"))
                || (attacker_elite
                    && (flag("attacker_veteran_combat") || flag("attacker_elite_combat")));
            let stronger = (target_veteran && flag("target_veteran_armor"))
                || (target_elite && (flag("target_veteran_armor") || flag("target_elite_armor")));
            let verses: [f64; 11] = std::array::from_fn(|index| {
                f64::from_bits(f64_bits(&input["warhead_verses"][index]).bits())
            });
            let inputs = EstimateInputs {
                damage: input["weapon_damage"].as_i64().unwrap() as i32,
                zeroed: flag("weapon_flag_130") || flag("weapon_flag_129"),
                stages: FireDamageStages {
                    house_firepower: f64_bits(&input["attacker_house_firepower"]),
                    unit_firepower: f64_bits(&input["attacker_firepower"]),
                    rank_firepower: firepower.then(|| f64_bits(&input["rules_veteran_combat"])),
                    occupied: None,
                    bunkered: None,
                    open_topped: None,
                },
                divisors: DefenceDivisors {
                    house_type_armor: category_armor(input),
                    unit_armor: f64_bits(&input["attacker_armor"]),
                    rank_armor: stronger.then(|| f64_bits(&input["rules_veteran_armor"])),
                },
                warhead: (input["warhead_present"] == true).then_some(EstimateWarhead {
                    cell_spread: widened_f32(&input["warhead_cell_spread"]),
                    percent_at_max: widened_f32(&input["warhead_percent_at_max"]),
                    verses: &verses,
                }),
                armor: ArmorClass(input["target_armor_index"].as_u64().unwrap() as u8),
                scenario_no_damage: input["scenario_flags"].as_u64().unwrap() & 0x20 != 0,
                max_damage: input["rules_max_damage"].as_i64().unwrap() as i32,
            };
            assert_eq!(
                i64::from(estimated_damage(&inputs)),
                row["result_i32"].as_i64().unwrap(),
                "{}",
                input["name"]
            );
            compared += 1;
        }
        assert_eq!(compared, 379);
    }
}
