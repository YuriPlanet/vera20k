//! Receiver preparation: Techno armor/gates and Object entry/kernel.
//! Object's mutable packet rewrite, HP commit and native return classification
//! live in combat::object_health and execute once after this preparation.
//! The represented order follows Techno701900 and Object5F5390; defense math
//! below retains its explicitly documented older host-f64 boundary.

use super::gates::evaluate_gates;
use super::kernel::apply_warhead_damage;
use super::{CombatMods, DamageGate, DamageOutcome, ImmunityInputs, TargetDamageView};

/// Unmigrated defense-stage host-f64 conversion. This saturating cast does not
/// implement native7C5F00's signed64/low32 contract. The shared warhead kernel
/// already uses X87Chop53; the preceding defense arithmetic still needs migration.
#[inline]
fn ftol(v: f64) -> i32 {
    v as i32
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
    mods: &CombatMods,
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

    // Nonnegative receiver divides. Healing (incoming < 0) bypasses. gamemd
    // runs the divides BEFORE the immunity gates (TypeImmune included), so the
    // gates are evaluated below, not before the divides.
    let mut dmg = incoming;
    if !gates.ignore_defenses && dmg >= 0 {
        // country-armor DIVIDE folding per-unit ArmorMultiplier, ONE ftol.
        // FDIVR: damage / (country * unit); larger mult => less damage.
        let armor_div = mods.defender_country_armor * mods.defender_unit_armor;
        if armor_div != 0.0 {
            dmg = ftol(dmg as f64 / armor_div);
        }
        // VeteranArmor DIVIDE, ONE ftol (only when set and != 1.0).
        if mods.defender_vet_armor != 0.0 && mods.defender_vet_armor != 1.0 {
            dmg = ftol(dmg as f64 / mods.defender_vet_armor);
        }
        // Defender min-1: AFTER the divides, BEFORE the gates and Verses kernel.
        dmg = dmg.max(1);
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
    // the PRE-kernel damage. An area record never gets here dead:
    // `Apply_area_damage` checks Health before dispatching it
    // (`world_receiver::area_record_dispatches`). The direct callers do — the
    // bridge, C4 and Ivan expiry receivers and Blowup_All.
    // The `Health < 1` half is UNCONDITIONAL — only the Immune clause is behind
    // `ignoreDefenses`, which VERA's C4 and Ivan expiry receivers set.
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
mod tests {
    use super::*;
    use crate::sim::combat::damage::{ArmorClass, CombatMods, ImmunityInputs, TargetDamageView};

    const MAXD: i32 = 10000;

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
            &CombatMods::default(),
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
            &CombatMods::default(),
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
            &CombatMods::default(),
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
            &CombatMods::default(),
            &allow(),
            0,
            false,
            MAXD,
        );
        assert_eq!(o.hp_delta, 500);
    }

    #[test]
    fn veteran_armor_divides() {
        // VeteranArmor 1.5: 60 incoming => ftol(60/1.5)=40.
        let mods = CombatMods {
            defender_vet_armor: 1.5,
            ..CombatMods::default()
        };
        let o = receive_damage(
            60,
            0.0,
            1.0,
            &verses(1.0),
            &tgt(300, 300),
            &mods,
            &allow(),
            0,
            false,
            MAXD,
        );
        assert_eq!(o.hp_delta, 40);
    }

    #[test]
    fn country_armor_mult_applies() {
        // Country armor mult 2.0 (tougher): 80 incoming => ftol(80/2)=40.
        let mods = CombatMods {
            defender_country_armor: 2.0,
            ..CombatMods::default()
        };
        let o = receive_damage(
            80,
            0.0,
            1.0,
            &verses(1.0),
            &tgt(300, 300),
            &mods,
            &allow(),
            0,
            false,
            MAXD,
        );
        assert_eq!(o.hp_delta, 40);
    }

    #[test]
    fn min_one_floor_positive() {
        // Country armor mult 100 makes a 50-incoming hit floor to 1 (defender
        // min-1 after the divides), then Verses 1.0 keeps 1.
        let mods = CombatMods {
            defender_country_armor: 100.0,
            ..CombatMods::default()
        };
        let o = receive_damage(
            50,
            0.0,
            1.0,
            &verses(1.0),
            &tgt(300, 300),
            &mods,
            &allow(),
            0,
            false,
            MAXD,
        );
        assert_eq!(o.hp_delta, 1);
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
            &CombatMods::default(),
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
            &CombatMods::default(),
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
            &CombatMods::default(),
            &g,
            0,
            false,
            MAXD,
        );
        assert_eq!(o.hp_delta, 0);
        assert_eq!(o.invulnerability_impact_damage, Some(200));
    }
}
