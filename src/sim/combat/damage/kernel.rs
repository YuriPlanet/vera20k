//! Shared `gamemd.exe 00489180` warhead numeric receiver.
//!
//! Original instruction comparison: `tools/spatial_oracle/estimated_damage`.
//! Native binary32 spills, PC53/chop operations and low32 `ftol` conversions
//! remain ordered here for ordinary damage, Psychedelic and Terrain callers.

use super::ArmorClass;
use crate::util::native_x87::{
    MaskedX87Chop53 as X87, MaskedX87Ordering, MaskedX87Value, NativeF32Bits, NativeF64Bits,
};

/// Original binary32 constant `007E2224`.
const KERNEL_LEPTONS_PER_CELL: NativeF32Bits = NativeF32Bits::from_bits(0x4380_0000);

#[inline]
fn decoded_f32(value: f64) -> MaskedX87Value {
    // WarheadType preserves its parsed native f32 values widened to f64 for
    // compatibility with existing consumers. Recover that memory format here;
    // all subsequent numeric operations belong to the deterministic x87 owner.
    let raw = value.to_bits();
    let bits = if raw & 0x7ff0_0000_0000_0000 == 0x7ff0_0000_0000_0000 {
        // Recover a widened native f32 infinity/NaN without a host NaN cast.
        // A noncanonical f64 NaN with only discarded payload bits stays NaN.
        let fraction = raw & 0x000f_ffff_ffff_ffff;
        let payload = (fraction >> 29) as u32;
        ((raw >> 32) as u32 & 0x8000_0000)
            | 0x7f80_0000
            | if fraction != 0 && payload == 0 {
                1
            } else {
                payload
            }
    } else {
        (value as f32).to_bits()
    };
    X87::load_f32(NativeF32Bits::from_bits(bits))
}

/// Original `489180..48926C`, with decoded warhead fields and masked exceptions.
///
/// CellSpread and PercentAtMax are widened binary32 memory values; Verses is
/// binary64. Distance subtraction wraps as signed32 before FIMUL. All three
/// conversions retain the low32 bits of native signed64 `FISTP`, not saturation.
/// The caller owns null-warhead admission because this API takes decoded fields.
/// Infinity/NaN values follow masked x87 value semantics; status flags, traps and
/// the rest of the FPU environment are outside this numeric receiver contract.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_warhead_damage(
    damage: i32,
    cell_spread: f64,
    percent_at_max: f64,
    verses_f64: &[f64; 11],
    armor: ArmorClass,
    distance_leptons: i32,
    scenario_no_damage: bool,
    max_damage: i32,
) -> i32 {
    // 489189..1A9: zero / ScenarioFlags20h / caller-owned null warhead.
    if damage == 0 || scenario_no_damage {
        return 0;
    }
    // 4891AB..1C5: signed distance<8; negative damage bypasses even MaxDamage.
    if damage < 0 {
        return if distance_leptons < 8 { damage } else { 0 };
    }

    // 4891C6..1D4: FST damage does not pop or round the live register used by
    // FMUL PercentAtMax. Both stored operands are then reloaded as binary32.
    let exact_damage = X87::load_i32(damage);
    let damage_spill = X87::store_f32_masked_chop(exact_damage);
    let product_spill =
        X87::store_f32_masked_chop(X87::mul(exact_damage, decoded_f32(percent_at_max)));
    let stored_damage = X87::load_f32(damage_spill);
    let stored_product = X87::load_f32(product_spill);

    // 4891D8..1E9: binary32 spread * original256 constant, then ftol low EAX.
    let spread = X87::ftol_i32_low_masked(X87::mul(
        decoded_f32(cell_spread),
        X87::load_f32(KERNEL_LEPTONS_PER_CELL),
    ));
    let falloff = if matches!(
        X87::compare(stored_product, stored_damage),
        MaskedX87Ordering::Less | MaskedX87Ordering::Greater
    ) && spread != 0
    {
        // FCOMP / FNSTSW / TEST AH,40h bypasses on equality OR unordered.
        // 489202..225: FSUB; wrapping SUB; FIMUL; FIDIV; FADD; ftol.
        let difference = X87::sub(stored_damage, stored_product);
        let scaled_difference = X87::mul(
            difference,
            X87::load_i32(spread.wrapping_sub(distance_leptons)),
        );
        let divided = X87::div(scaled_difference, X87::load_i32(spread));
        X87::ftol_i32_low_masked(X87::add(divided, stored_product))
    } else {
        damage
    };
    // 489227..249: signed floor0, binary64 verse multiply, ftol low EAX.
    let scaled = X87::ftol_i32_low_masked(X87::mul(
        X87::load_i32(falloff.max(0)),
        X87::load_f64(NativeF64Bits::from_bits(
            verses_f64[armor.0 as usize].to_bits(),
        )),
    ));

    // 489249..26C: signed cap only, after possible negative verse result.
    scaled.min(max_damage)
}

#[cfg(test)]
#[path = "kernel_tests.rs"]
mod native_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::combat::damage::ArmorClass;

    /// Stock YR running MaxDamage (`ini/rulesmd.ini` MaxDamage=10000, overriding
    /// the legacy 1000). The cap field is `[Rules+0x16C8]`.
    const MAXD: i32 = 10000;

    fn verses(v: f64) -> [f64; 11] {
        let mut t = [1.0; 11];
        // index 5 = heavy; set the value under test, leave others 1.0.
        t[5] = v;
        t
    }

    #[test]
    fn kernel_matches_worked_example() {
        // 100 dmg, Verses 0.5 (Heavy), CellSpread 1.0, PAM 0.25, dist 128 leptons.
        // cs_leptons = ftol(1.0*256) = 256; t = (256-128)/256 = 0.5;
        // lerped = 0.25*100 + 0.75*100*0.5 = 62.5 => ftol = 62;
        // scaled = ftol(62*0.5) = ftol(31) = 31.
        // (Kernel = AoE = 256 leptons/cell, so Q1 is resolved and this is no
        //  longer #[ignore]'d; value is 31, NOT the mis-converted-128 "12".)
        let d = apply_warhead_damage(
            100,
            1.0,
            0.25,
            &verses(0.5),
            ArmorClass(5),
            128,
            false,
            MAXD,
        );
        assert_eq!(d, 31);
    }

    #[test]
    fn kernel_double_ftol_order() {
        // 99 dmg, PAM 0.5, dist 128 (cs_leptons=256 => t=0.5).
        // lerped = 0.5*99 + 0.5*99*0.5 = 74.25 => ftol #2 = 74;
        // scaled = ftol(74 * 0.5) = ftol(37) = 37. Exercises both interior ftols.
        let d = apply_warhead_damage(99, 1.0, 0.5, &verses(0.5), ArmorClass(5), 128, false, MAXD);
        assert_eq!(d, 37);
    }

    #[test]
    fn kernel_healing_uses_point_blank_distance_gate() {
        let concrete_near =
            apply_warhead_damage(-40, 0.0, 1.0, &[1.0; 11], ArmorClass(8), 7, false, MAXD);
        let ordinary_edge =
            apply_warhead_damage(-40, 0.0, 1.0, &[1.0; 11], ArmorClass(5), 8, false, MAXD);
        assert_eq!(concrete_near, -40);
        assert_eq!(ordinary_edge, 0);
    }

    #[test]
    fn kernel_pam_one_is_flat() {
        // PAM==1.0 => branch guard false => flat damage at any distance.
        let near = apply_warhead_damage(100, 5.0, 1.0, &verses(1.0), ArmorClass(5), 0, false, MAXD);
        let far =
            apply_warhead_damage(100, 5.0, 1.0, &verses(1.0), ArmorClass(5), 600, false, MAXD);
        assert_eq!(near, 100);
        assert_eq!(far, 100);
    }

    #[test]
    fn kernel_maxdamage_cap() {
        // Verses 2.0 x large flat base => clamps to 10000.
        // base 8000, flat => ftol(8000*2.0)=16000 => min(16000,10000)=10000.
        let d = apply_warhead_damage(8000, 0.0, 1.0, &verses(2.0), ArmorClass(5), 0, false, MAXD);
        assert_eq!(d, 10000);
    }

    #[test]
    fn kernel_maxdamage_cap_inclusive_on_equal() {
        // scaled == cap is kept (inclusive); only strictly-greater is reduced.
        let d = apply_warhead_damage(5000, 0.0, 1.0, &verses(2.0), ArmorClass(5), 0, false, MAXD);
        assert_eq!(d, 10000); // 5000*2 == 10000, kept
    }

    #[test]
    fn kernel_scenario_no_damage_zero() {
        let d = apply_warhead_damage(100, 0.0, 1.0, &verses(1.0), ArmorClass(5), 0, true, MAXD);
        assert_eq!(d, 0);
    }
}
