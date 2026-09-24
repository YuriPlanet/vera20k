//! Receiver preparation: Techno armor/gates and Object entry/kernel.
//! Object's mutable packet rewrite, HP commit and native return classification
//! live in combat::object_health and execute once after this preparation.
//! The represented order follows Techno701900 and Object5F5390.

use super::gates::evaluate_gates;
use super::kernel::apply_warhead_damage;
use super::{DamageGate, DamageOutcome, DefenceDivisors, ImmunityInputs, TargetDamageView};
use crate::util::native_x87::MaskedX87Chop53 as X;

/// `TechnoClass::ReceiveDamage @ 0x00701900`'s defence divides for a
/// defended, non-negative hit (`0x00701939..0x007019E3`), each an x87 divide
/// (53-bit, chop) and `Math::ftol`'s low 32 bits:
/// `ftol(damage / (GetArmorMultForType * Techno+0x158))`, then for a STRONGER
/// rank `ftol(damage / VeteranArmor)`, then at least 1. A zero divisor gives
/// an infinity whose conversion is 0, so the hit still lands for 1.
///
/// Native execution: `tools/spatial_oracle/damage_build.py` (`receive` rows).
pub(crate) fn defence_divides(damage: i32, divisors: &DefenceDivisors) -> i32 {
    let mut damage = X::ftol_i32_low_masked(X::div(
        X::load_i32(damage),
        X::mul(
            X::load_f32(divisors.house_type_armor),
            X::load_f64(divisors.unit_armor),
        ),
    ));
    if let Some(rank_armor) = divisors.rank_armor {
        damage = X::ftol_i32_low_masked(X::div(X::load_i32(damage), X::load_f64(rank_armor)));
    }
    damage.max(1)
}

/// Prepare the receiver packet without committing HP or predicting its result.
/// `cell_spread`/`percent_at_max`/`verses_f64` are the warhead's decoded kernel
/// inputs (see kernel::apply_warhead_damage). `distance_leptons` is the impact
/// distance in the kernel lepton unit (256 leptons/cell).
#[allow(clippy::too_many_arguments)]
pub(crate) fn receive_damage(
    incoming: i32,
    cell_spread: f64,
    percent_at_max: f64,
    verses_f64: &[f64; 11],
    target: &TargetDamageView,
    divisors: &DefenceDivisors,
    gates: &ImmunityInputs,
    distance_leptons: i32,
    scenario_no_damage: bool,
    max_damage: i32,
) -> DamageOutcome {
    let unaffected = DamageOutcome {
        apply_object_damage: false,
        hp_delta: 0,
        post_object_damage: None,
        psychedelic_value: None,
        invulnerability_impact_damage: None,
        reached_survivor_postlude: false,
    };

    // Nonnegative receiver divides (`0x0070192B..0x00701933` skips them for
    // ignoreDefenses or a heal). gamemd runs them BEFORE the immunity gates
    // (TypeImmune included), so the gates are evaluated below.
    let mut dmg = incoming;
    if !gates.ignore_defenses && dmg >= 0 {
        dmg = defence_divides(dmg, divisors);
    }

    // Immunity gates (after the divides; TypeImmune handled inside).
    match evaluate_gates(gates) {
        DamageGate::Nullified => return unaffected,
        DamageGate::Invulnerable => {
            return DamageOutcome {
                invulnerability_impact_damage: Some(dmg.wrapping_shl(1)),
                ..unaffected
            };
        }
        DamageGate::MindControlled => {
            // Accepted Psychedelic is a separate Techno receiver transaction:
            // run the same signed kernel at literal distance zero, store its
            // result in Techno state, and return code 1 before Object HP.
            let psychedelic_value = apply_warhead_damage(
                dmg,
                cell_spread,
                percent_at_max,
                verses_f64,
                target.armor,
                0,
                scenario_no_damage,
                max_damage,
            );
            return DamageOutcome {
                apply_object_damage: false,
                hp_delta: 0,
                post_object_damage: None,
                psychedelic_value: Some(psychedelic_value),
                invulnerability_impact_damage: None,
                reached_survivor_postlude: false,
            };
        }
        DamageGate::Pass => {}
    }

    // ObjectClass::ReceiveDamage entry gate. Ordered area receivers always
    // enter with ignoreDefenses=false, so an Immune type returns before the
    // ordinary kernel, HP write, and every downstream callback. This stays
    // after the Techno gates because accepted Psychedelic returns above without
    // delegating to ObjectClass.
    //
    // `ObjectClass::ReceiveDamage @ 0x005F5390` opens with `if (Health < 1)
    // return 0`, ahead of the Immune test: the kernel, the building min-1, the
    // HP write and the state classification are all skipped for an
    // already-dead receiver, while the Techno tail still feeds the anger nodes
    // the PRE-kernel damage. VERA filtered dead targets at collection only, so
    // a target driven to zero by an earlier record of the SAME blast — a Demo
    // Truck, an Ivan-bombed cluster, any `DeathWeapon` cascade — was
    // re-processed here and fed the post-Verses value instead.
    // The `Health < 1` half is UNCONDITIONAL — only the Immune clause is behind
    // `ignoreDefenses`. That distinction is the whole point here: VERA's two
    // `ignore_defenses` callers are the C4 and Ivan expiry receivers, which is
    // exactly the same-blast cascade this gate exists to stop.
    if target.current_hp < 1 || dmg == 0 || (!gates.ignore_defenses && target.object_immune) {
        return DamageOutcome {
            post_object_damage: Some(dmg),
            reached_survivor_postlude: true,
            ..unaffected
        };
    }

    // Verses kernel (falloff -> Verses -> cap; also re-runs the D1/D2 early-outs).
    let delta = if gates.ignore_defenses {
        dmg
    } else {
        apply_warhead_damage(
            dmg,
            cell_spread,
            percent_at_max,
            verses_f64,
            target.armor,
            distance_leptons,
            scenario_no_damage,
            max_damage,
        )
    };

    // Object HP mutation, Building minimum, packet clamp and return-code
    // classification belong to object_health::commit, after preparation.
    DamageOutcome {
        apply_object_damage: true,
        hp_delta: delta,
        post_object_damage: Some(delta),
        psychedelic_value: None,
        invulnerability_impact_damage: None,
        reached_survivor_postlude: true,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::sim::combat::damage::{
        ArmorClass, DefenceDivisors, ImmunityInputs, TargetDamageView,
    };

    const MAXD: i32 = 10000;

    /// A TechnoType whose rank lists hold (or lack) `ability`.
    pub(crate) fn object_with_ability(
        ability: crate::rules::object_type::Ability,
        veteran: bool,
        elite: bool,
    ) -> crate::rules::object_type::ObjectType {
        use crate::rules::object_type::{AbilityFlags, ObjectCategory, ObjectType};
        let ini = crate::rules::ini_parser::IniFile::from_str("[X]\nStrength=300\n");
        let mut object =
            ObjectType::from_ini_section("X", ini.section("X").unwrap(), ObjectCategory::Vehicle);
        let flags = |on: bool| {
            if on {
                AbilityFlags::from_abilities(&[ability])
            } else {
                AbilityFlags::from_abilities(&[])
            }
        };
        object.veteran_abilities = flags(veteran);
        object.elite_abilities = flags(elite);
        object
    }

    /// The production adapter picks the float native loaded:
    /// `RuleSet::country_armor_mult_for_type` over every corpus row whose
    /// WhatAmI VERA represents (infantry, unit, aircraft, building with and
    /// without `BuildCat=Combat`), parsed through the production reader.
    #[test]
    fn original_armor_mult_for_type_rows_through_the_rules_reader() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        #[derive(serde::Deserialize)]
        struct Corpus {
            receive: Vec<Row>,
        }
        #[derive(serde::Deserialize)]
        struct Row {
            input: Input,
            armor_mult_for_type: u32,
        }
        #[derive(serde::Deserialize)]
        struct Input {
            type_whatami: u32,
            build_cat: u32,
            house_type_mults: [u32; 5],
        }
        let corpus: Corpus = serde_json::from_str(include_str!(
            "../../../../tools/spatial_oracle/damage_build.json"
        ))
        .expect("original damage-build corpus");
        let mut rulesets = std::collections::BTreeMap::new();
        let mut compared = 0;
        for (index, row) in corpus.receive.iter().enumerate() {
            let input = &row.input;
            let list = match input.type_whatami {
                0x10 => "InfantryTypes",
                0x28 => "VehicleTypes",
                0x03 => "AircraftTypes",
                0x07 => "BuildingTypes",
                _ => continue,
            };
            let rules = rulesets
                .entry((input.house_type_mults, list, input.build_cat))
                .or_insert_with(|| {
                    let float = |bits: u32| f32::from_bits(bits).to_string();
                    let [infantry, units, aircraft, buildings, defenses] =
                        input.house_type_mults.map(float);
                    let build_cat = match input.build_cat {
                        5 => "BuildCat=Combat\n",
                        3 => "BuildCat=Power\n",
                        _ => "",
                    };
                    RuleSet::from_ini(&IniFile::from_str(&format!(
                        "[Countries]\n0=T\n[T]\nArmorInfantryMult={infantry}\n\
                         ArmorUnitsMult={units}\nArmorAircraftMult={aircraft}\n\
                         ArmorBuildingsMult={buildings}\nArmorDefensesMult={defenses}\n\
                         [{list}]\n0=X\n[X]\nStrength=1\n{build_cat}"
                    )))
                    .expect("armor-mult fixture parses")
                });
            let object = rules.object("X").expect("fixture type");
            assert_eq!(
                rules.country_armor_mult_for_type("T", object).to_bits(),
                row.armor_mult_for_type,
                "row {index}"
            );
            compared += 1;
        }
        assert!(compared > 1000, "{compared} rows compared");
    }

    /// Native execution of `0x00701939..0x007019E3`
    /// (`tools/spatial_oracle/damage_build.py`, `receive` rows): VERA's
    /// STRONGER gate takes the branch native took, and the divides reproduce
    /// every result from the float `GetArmorMultForType` loaded.
    #[test]
    fn original_defence_divides_rows() {
        use crate::rules::object_type::Ability;
        use crate::sim::combat::veterancy::{has_weapon_ability, rank_of};
        use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
        #[derive(serde::Deserialize)]
        struct Corpus {
            receive: Vec<Row>,
        }
        #[derive(serde::Deserialize)]
        struct Row {
            input: Input,
            stages: Vec<String>,
            armor_mult_for_type: u32,
            damage: i32,
        }
        #[derive(serde::Deserialize)]
        struct Input {
            damage: i32,
            armor_mult: Option<u64>,
            veterancy: u32,
            veteran_stronger: u8,
            elite_stronger: u8,
            veteran_armor: u32,
        }
        let corpus: Corpus = serde_json::from_str(include_str!(
            "../../../../tools/spatial_oracle/damage_build.json"
        ))
        .expect("original damage-build corpus");
        assert_eq!(corpus.receive.len(), 1659);
        for (index, row) in corpus.receive.iter().enumerate() {
            let input = &row.input;
            let object = object_with_ability(
                Ability::Stronger,
                input.veteran_stronger != 0,
                input.elite_stronger != 0,
            );
            let stronger = has_weapon_ability(
                rank_of(NativeF32Bits::from_bits(input.veterancy)),
                &object,
                Ability::Stronger,
            );
            assert_eq!(
                stronger,
                row.stages.iter().any(|stage| stage == "rank_armor"),
                "row {index}: STRONGER gate"
            );
            // ReadDouble's value: the parsed single widened.
            let veteran_armor = f64::from(f32::from_bits(input.veteran_armor));
            let divisors = DefenceDivisors {
                house_type_armor: NativeF32Bits::from_bits(row.armor_mult_for_type),
                unit_armor: NativeF64Bits::from_bits(input.armor_mult.unwrap_or(1.0_f64.to_bits())),
                rank_armor: stronger.then(|| NativeF64Bits::from_bits(veteran_armor.to_bits())),
            };
            assert_eq!(
                defence_divides(input.damage, &divisors),
                row.damage,
                "row {index}"
            );
        }
    }

    fn tgt(_strength: i32, hp: i32) -> TargetDamageView {
        TargetDamageView {
            armor: ArmorClass(5),
            current_hp: hp,
            object_immune: false,
        }
    }
    fn allow() -> ImmunityInputs {
        ImmunityInputs {
            affects_allies: true,
            ..Default::default()
        }
    }
    fn verses(v: f64) -> [f64; 11] {
        let mut t = [1.0; 11];
        t[5] = v;
        t
    }

    /// `ObjectClass::ReceiveDamage @ 0x005F5390` returns before the kernel for
    /// a receiver already at zero health, and the Techno tail feeds the anger
    /// nodes the PRE-kernel value.
    #[test]
    fn gsi_08_09_dead_target_skips_the_kernel_and_reports_pre_kernel_damage() {
        let dead = receive_damage(
            400,
            0.0,
            1.0,
            &verses(0.25),
            &tgt(300, 0),
            &DefenceDivisors::default(),
            &allow(),
            0,
            false,
            MAXD,
        );
        assert_eq!(dead.hp_delta, 0);
        // Pre-kernel: the two armour divides are no-ops here, so 400 survives
        // whole rather than being scaled by the 0.25 Verses entry.
        assert_eq!(dead.post_object_damage, Some(400));
        assert!(dead.reached_survivor_postlude);
    }

    /// The `Health < 1` half of the entry gate is unconditional: only the
    /// Immune clause sits behind `ignoreDefenses`. VERA's two `ignore_defenses`
    /// callers are the C4 and Ivan expiry receivers, which is exactly the
    /// same-blast cascade the gate exists to stop.
    #[test]
    fn gsi_08_09_dead_target_is_gated_even_when_defenses_are_ignored() {
        let ignoring = ImmunityInputs {
            affects_allies: true,
            ignore_defenses: true,
            ..Default::default()
        };
        let out = receive_damage(
            400,
            0.0,
            1.0,
            &verses(1.0),
            &tgt(300, 0),
            &DefenceDivisors::default(),
            &ignoring,
            0,
            false,
            MAXD,
        );
        assert_eq!(out.hp_delta, 0);
    }

    /// One HP is still alive, so the gate must not fire.
    #[test]
    fn gsi_08_09_hp_one_still_enters_the_kernel() {
        let alive = receive_damage(
            400,
            0.0,
            1.0,
            &verses(0.25),
            &tgt(300, 1),
            &DefenceDivisors::default(),
            &allow(),
            0,
            false,
            MAXD,
        );
        assert_eq!(alive.hp_delta, 100);
    }

    #[test]
    fn overkill_stays_uncommitted_until_object_owner() {
        // Preparation retains500; the Object commit caps its mutable packet to50.
        let o = receive_damage(
            500,
            0.0,
            1.0,
            &verses(1.0),
            &tgt(300, 50),
            &DefenceDivisors::default(),
            &allow(),
            0,
            false,
            MAXD,
        );
        assert_eq!(o.hp_delta, 500);
    }

    #[test]
    fn non_building_zero_verses_is_unaffected() {
        // A unit whose Verses collapses to 0 is genuinely unaffected (no floor).
        let o = receive_damage(
            10,
            0.0,
            1.0,
            &verses(0.0001),
            &tgt(1000, 1000),
            &DefenceDivisors::default(),
            &allow(),
            0,
            false,
            MAXD,
        );
        assert_eq!(o.hp_delta, 0);
    }

    #[test]
    fn mindcontrol_applies_zero_hp() {
        let g = ImmunityInputs {
            attacker_present: true,
            psychedelic: true,
            ..allow()
        };
        let o = receive_damage(
            100,
            0.0,
            1.0,
            &verses(1.0),
            &tgt(300, 300),
            &DefenceDivisors::default(),
            &g,
            0,
            false,
            MAXD,
        );
        assert_eq!(o.hp_delta, 0);
        assert_eq!(o.psychedelic_value, Some(100));
    }

    #[test]
    fn invulnerability_returns_native_doubled_impact_argument() {
        let g = ImmunityInputs {
            invulnerable: true,
            ..allow()
        };
        let o = receive_damage(
            100,
            0.0,
            1.0,
            &verses(1.0),
            &tgt(300, 300),
            &DefenceDivisors::default(),
            &g,
            0,
            false,
            MAXD,
        );
        assert_eq!(o.hp_delta, 0);
        assert_eq!(o.invulnerability_impact_damage, Some(200));
    }
}
