//! Receiver pipeline: armor divides -> immunity gates -> Verses kernel ->
//! building min-1 -> overkill clamp -> classify. Pure over caller-built views.
//!
//! Stage order verified against TechnoClass::ReceiveDamage (0x00701900) +
//! ObjectClass::ReceiveDamage (0x005f5390):
//!   1. country-armor DIVIDE (folds Techno+0x158), ftol
//!   2. VeteranArmor DIVIDE (Rules+0x688), ftol
//!   3. defender min-1
//!   4. immunity gates (TypeImmune is checked HERE, after the divides)
//!   5. Verses kernel (falloff -> Verses -> MaxDamage cap)
//!   6. building min-1 (Building && !CanC4), applied to the kernel output
//!   7. overkill clamp to remaining HP
//!   8. classify (Yellow = integer Strength>>1; Red = double Strength*ratio)

use super::gates::evaluate_gates;
use super::kernel::apply_warhead_damage;
use super::{CombatMods, DamageGate, DamageOutcome, DamageState, ImmunityInputs, TargetDamageView};

/// Unmigrated defense-stage host-f64 conversion. This saturating cast does not
/// implement native7C5F00's signed64/low32 contract. The shared warhead kernel
/// already uses X87Chop53; the preceding defense arithmetic still needs migration.
#[inline]
fn ftol(v: f64) -> i32 {
    v as i32
}

/// Full receiver pipeline. `condition_red_ratio` = Rules+0x1708 (~0.25).
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
    condition_red_ratio: f64,
) -> DamageOutcome {
    let unaffected = DamageOutcome {
        hp_delta: 0,
        post_object_damage: None,
        state: DamageState::Unaffected,
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
                hp_delta: 0,
                post_object_damage: None,
                state: DamageState::Damaged,
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
    if target.current_hp < 1 || (!gates.ignore_defenses && target.object_immune) {
        return DamageOutcome {
            post_object_damage: Some(dmg),
            reached_survivor_postlude: true,
            ..unaffected
        };
    }

    // Verses kernel (falloff -> Verses -> cap; also re-runs the D1/D2 early-outs).
    let mut delta = if gates.ignore_defenses {
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

    // Building min-1 (ObjectClass::ReceiveDamage, post-Verses, Building && !CanC4).
    // MUST run before the sign/zero dispatch: a building whose Verses collapses
    // to zero, or whose packet was negative, is rewritten to positive one.
    if target.is_building && !target.can_c4 {
        delta = delta.max(1);
    }
    // Healing path (delta < 0): caller adds back, clamped to strength elsewhere.
    if delta < 0 {
        return DamageOutcome {
            hp_delta: delta,
            post_object_damage: Some(delta),
            state: classify(target, delta, condition_red_ratio),
            psychedelic_value: None,
            invulnerability_impact_damage: None,
            reached_survivor_postlude: true,
        };
    }
    if delta == 0 {
        return DamageOutcome {
            post_object_damage: Some(0),
            reached_survivor_postlude: true,
            ..unaffected
        };
    }

    // Overkill clamp: damage never exceeds remaining HP.
    if delta > target.current_hp {
        delta = target.current_hp;
    }

    DamageOutcome {
        hp_delta: delta,
        post_object_damage: Some(delta),
        state: classify(target, delta, condition_red_ratio),
        psychedelic_value: None,
        invulnerability_impact_damage: None,
        reached_survivor_postlude: true,
    }
}

/// Health-state classification. Yellow uses integer `Strength >> 1` (the state
/// crossing, NOT the ConditionYellow ratio); Red uses `Strength * red_ratio`
/// (Rules+0x1708) compared in DOUBLE precision (verified: no ftol on the Red
/// threshold). Dead when post-hit HP reaches 0.
///
/// NB the ConditionYellow ratio (Rules+0x1700) is a SEPARATE mechanism used by
/// the health-bar/smoke/fear gate (IsYellowHP), distinct from this integer
/// state crossing — do not unify them at the cutover.
fn classify(target: &TargetDamageView, delta: i32, red_ratio: f64) -> DamageState {
    let prev = target.current_hp;
    let post = prev - delta; // delta may be negative (heal) => post > prev
    if post <= 0 {
        return DamageState::Dead;
    }
    // Red: double multiply + double compare (no integer truncation).
    let red = target.strength as f64 * red_ratio;
    if red < prev as f64 && (post as f64) < red {
        return DamageState::Red;
    }
    // Yellow: integer Strength>>1 (arithmetic shift).
    let yellow = target.strength >> 1;
    if yellow <= prev && post < yellow {
        return DamageState::Yellow;
    }
    DamageState::Damaged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::combat::damage::{ArmorClass, CombatMods, ImmunityInputs, TargetDamageView};

    const MAXD: i32 = 10000;
    const RED: f64 = 0.25;

    fn tgt(strength: i32, hp: i32) -> TargetDamageView {
        TargetDamageView {
            armor: ArmorClass(5),
            strength,
            current_hp: hp,
            object_immune: false,
            is_building: false,
            can_c4: false,
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
            RED,
        );
        assert_eq!(dead.hp_delta, 0);
        assert_eq!(dead.state, DamageState::Unaffected);
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
            RED,
        );
        assert_eq!(out.hp_delta, 0);
        assert_eq!(out.state, DamageState::Unaffected);
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
            RED,
        );
        assert_eq!(alive.hp_delta, 1);
        assert_eq!(alive.state, DamageState::Dead);
    }

    #[test]
    fn overkill_clamped_to_remaining_hp() {
        // 500 incoming vs 50-HP target reports 50, not 500.
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
            RED,
        );
        assert_eq!(o.hp_delta, 50);
        assert_eq!(o.state, DamageState::Dead);
    }

    #[test]
    fn yellow_uses_integer_strength_halved() {
        // Strength 100 => yellow at >>1 = 50. Full-HP target, 60 damage:
        // prev=100, post=40, crosses 50 => Yellow (not the 0.25 ratio).
        let o = receive_damage(
            60,
            0.0,
            1.0,
            &verses(1.0),
            &tgt(100, 100),
            &CombatMods::default(),
            &allow(),
            0,
            false,
            MAXD,
            RED,
        );
        assert_eq!(o.hp_delta, 60);
        assert_eq!(o.state, DamageState::Yellow);
    }

    #[test]
    fn red_uses_double_condition_ratio() {
        // Strength 100, red ratio 0.25 => red threshold 25.0 (double, no ftol).
        // Target at 30 HP, 10 damage: post=20; 25<30 && 20<25 => Red.
        let o = receive_damage(
            10,
            0.0,
            1.0,
            &verses(1.0),
            &tgt(100, 30),
            &CombatMods::default(),
            &allow(),
            0,
            false,
            MAXD,
            RED,
        );
        assert_eq!(o.hp_delta, 10);
        assert_eq!(o.state, DamageState::Red);
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
            RED,
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
            RED,
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
            RED,
        );
        assert_eq!(o.hp_delta, 1);
    }

    #[test]
    fn building_min_one_after_verses() {
        // Building (no CanC4): tiny Verses collapses the kernel to 0, but the
        // building floor (run BEFORE the zero-check) raises it to 1.
        let bldg = TargetDamageView {
            is_building: true,
            ..tgt(1000, 1000)
        };
        let o = receive_damage(
            10,
            0.0,
            1.0,
            &verses(0.0001),
            &bldg,
            &CombatMods::default(),
            &allow(),
            0,
            false,
            MAXD,
            RED,
        );
        assert_eq!(o.hp_delta, 1);
        assert_eq!(o.state, DamageState::Damaged);
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
            RED,
        );
        assert_eq!(o.hp_delta, 0);
        assert_eq!(o.state, DamageState::Unaffected);
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
            RED,
        );
        assert_eq!(o.hp_delta, 0);
        assert_eq!(o.state, DamageState::Damaged);
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
            RED,
        );
        assert_eq!(o.hp_delta, 0);
        assert_eq!(o.state, DamageState::Unaffected);
        assert_eq!(o.invulnerability_impact_damage, Some(200));
    }
}
