//! Shared `gamemd.exe 00489180` warhead numeric receiver.
//!
//! Original instruction comparison: `tools/spatial_oracle/estimated_damage`.
//! Native binary32 spills, PC53/chop operations and low32 `ftol` conversions
//! remain ordered here for ordinary damage, Psychedelic and Terrain callers.

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
/// The caller resolves the target's armor once through the rules owner. Ares3.0
/// hook489235 ->10054230 changes that record source to the extension table;
/// the falloff, negative-input bypass and final cap stay in this same kernel.
pub(crate) fn apply_warhead_damage(
    damage: i32,
    cell_spread: f64,
    percent_at_max: f64,
    armor_multiplier: NativeF64Bits,
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
        X87::load_f64(armor_multiplier),
    ));

    // 489249..26C: signed cap only, after possible negative verse result.
    scaled.min(max_damage)
}

#[cfg(test)]
#[path = "kernel_tests.rs"]
mod native_tests;
