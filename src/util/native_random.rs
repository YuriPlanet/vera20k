//! The native random generator's state words and raw draw: a 250-word table
//! seeded by a four-round hash, drawn by XOR-ing the word at the lead cursor
//! with the word 0x67 slots ahead. `g_MapGenRng @ 0x00ABE890` (random-map
//! generation) and the scenario stream are the same machine; the generated
//! map hands its cursor on to the live scenario
//! (`rng_continuation::MapGenRngContinuation`).
//!
//! The state, the raw draw and the one constant native scales a
//! `RandomRanged(0, 0x7FFFFFFE)` draw by live here. Range reduction stays with
//! each user: `sim::rng::SimRng`'s `RandomRanged` family and
//! `map::rmg::RmgRng`'s chop-53 `uniform`.
//!
//! Native goldens: `tools/rmg_oracle/vectors/rng.json` (seeded state and raw
//! draws, `map::rmg::rng` tests) and the `sim::rng` seed and draw fixtures.

use crate::util::native_x87::NativeF64Bits;

/// Number of state words.
pub(crate) const STATE_WORDS: usize = 250;

/// The `double` at `0x007E3570` that scales a `RandomRanged(0, 0x7ffffffe)`
/// draw onto `[0, 1]`: probability gates ([`unit_draw_threshold`]: the damage
/// Spark bands, the scorch/crater coin, bridge debris, `CrewEscape=`, the AI
/// unit chooser) and unit fractions (scatter angles, Spark colors, bridge
/// debris jitter, crash spin, [`minutes_timer_frames`]). Every port of a
/// native `FMUL [0x007E3570]` reads this one constant. Raw bytes `00 00 40 00 00 00 00 3E`, i.e. the double nearest
/// `1.0 / 2147483646.0`, the reciprocal of the draw's own inclusive top, NOT
/// `2^-31`. The two differ by `2^-30` relative, which is a real bias in a
/// deterministic gate, so this is carried as bits and never recomputed.
pub(crate) const RANDOM_RANGED_UNIT_SCALE: NativeF64Bits =
    NativeF64Bits::from_bits(0x3e00_0000_0040_0000);

/// [`RANDOM_RANGED_UNIT_SCALE`] as the exact rational
/// `UNIT_SCALE_NUMERATOR · 2^UNIT_SCALE_EXPONENT`, i.e. `(2^30 + 1)·2^-61`.
const UNIT_SCALE_NUMERATOR: u128 = (1 << 30) + 1;
const UNIT_SCALE_EXPONENT: i32 = -61;

/// How many values `RandomRanged(0, 0x7FFFFFFE)` returns.
const UNIT_DRAW_COUNT: u32 = 0x7fff_ffff;

/// How many `RandomRanged(0, 0x7FFFFFFE)` draws pass native's unit gate
/// `FILD draw; FMUL [0x007E3570]; FCOMP band`, taken when the product is below
/// `band`; the gate is then the integer test `draw < unit_draw_threshold(band)`.
///
/// Under the game's chop-53 control word the product is the largest 53-bit
/// value at or below the exact `draw·(2^30+1)·2^-61`, so it is below a double
/// `band` exactly when the exact product is. That rational boundary is
/// computed here in integers for every double, subnormals included: no float,
/// no host rounding. `band <= 0` admits no draw. `band >= 1` admits all, and so
/// does NaN: `TEST AH,1` reads the C0 an unordered compare sets.
/// Native execution: the `gates` of `tools/unit_draw_oracle.json`.
pub(crate) const fn unit_draw_threshold(band: f64) -> u32 {
    if band.is_nan() || band >= 1.0 {
        return UNIT_DRAW_COUNT;
    }
    if band <= 0.0 {
        return 0;
    }
    // band = mant·2^exp, the normalized form carrying the implicit 53rd bit.
    let bits = band.to_bits();
    let exp_field = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1 << 52) - 1);
    let (mant, exp) = if exp_field == 0 {
        (frac, -1074)
    } else {
        ((1 << 52) | frac, exp_field - 1075)
    };
    // draw·(2^30+1)·2^-61 >= band  <=>  draw·(2^30+1) >= mant·2^(exp+61), so the
    // threshold is the smallest such draw, ceil(mant·2^(exp+61) / (2^30+1)).
    let k = exp - UNIT_SCALE_EXPONENT;
    let threshold = if k >= 0 {
        // band < 1 keeps k <= 8: no overflow.
        ((mant as u128) << k).div_ceil(UNIT_SCALE_NUMERATOR)
    } else if k > -64 {
        // A fractional mant·2^k: scale the divisor instead.
        (mant as u128).div_ceil(UNIT_SCALE_NUMERATOR << -k)
    } else {
        // A band below 2^-72: (2^30+1)·2^-k exceeds every 53-bit mantissa, so
        // only draw 0, whose product is 0, is below it.
        1
    };
    if threshold < UNIT_DRAW_COUNT as u128 {
        threshold as u32
    } else {
        UNIT_DRAW_COUNT
    }
}

/// The frames a minutes-based timer waits: a draw between `minutes·450` and
/// `minutes·1800`. `CrateSlot__PlaceOverlayAndInitTimer`
/// (`0x004A1868..0x004A18B5`, `CrateRegen=`) and the infantry idle timer
/// (`0x0051CDD9..0x0051CE26`, `IdleActionFrequency=`) inline one sequence: the
/// Rules double times `[0x007E44B8]` (1800.0) and times `[0x007E5D78]` (450.0),
/// each stored as a double, then `(upper - lower)·(draw·[0x007E3570]) + lower`
/// to `Math::ftol` (`0x007C5F00`). Its masked `FISTP qword` stores the integer
/// indefinite out of range, whose low dword, 0, both callers keep.
///
/// Returns that duration and the stored upper bound.
/// Native execution: the `crate_timer` rows of `tools/unit_draw_oracle.json`.
pub(crate) fn minutes_timer_frames(minutes: NativeF64Bits, draw: u32) -> (i32, NativeF64Bits) {
    use crate::util::native_x87::MaskedX87Chop53 as X87;
    let minutes = X87::load_f64(minutes);
    let upper = X87::store_f64_masked_chop(X87::mul(minutes, X87::load_i32(1800)));
    let lower = X87::load_f64(X87::store_f64_masked_chop(X87::mul(
        minutes,
        X87::load_i32(450),
    )));
    let fraction = X87::mul(
        X87::load_i32(draw as i32),
        X87::load_f64(RANDOM_RANGED_UNIT_SCALE),
    );
    let duration = X87::ftol_i32_low_masked(X87::add(
        X87::mul(X87::sub(X87::load_f64(upper), lower), fraction),
        lower,
    ));
    (duration, upper)
}

/// The executed native gates of `tools/unit_draw_oracle.json` named `gate`:
/// `(band, draw, passed)`.
#[cfg(test)]
pub(crate) fn native_gate_rows(gate: &str) -> Vec<(f64, u32, bool)> {
    let rows: Vec<_> = crate::test_fixture::json("tools/unit_draw_oracle.json")["gates"]
        .as_array()
        .expect("native gate rows")
        .iter()
        .filter(|row| row["gate"] == gate)
        .map(|row| {
            let band = row["band"].as_str().expect("band bits");
            let band = u64::from_str_radix(band.trim_start_matches("0x"), 16).expect("hex bits");
            (
                f64::from_bits(band),
                row["draw"].as_u64().expect("draw") as u32,
                row["passed"].as_bool().expect("passed"),
            )
        })
        .collect();
    assert!(!rows.is_empty(), "no native {gate} rows");
    rows
}
/// Distance between the two cursors; also the second cursor's start.
pub(crate) const LAG: usize = 0x67;

/// Seed-hash table mixed into each round's input.
const SEED_TABLE_1: [u32; 4] = [0xBAA9_6887, 0x1E17_D32C, 0x03BC_DC3C, 0x0F33_D1B2];
/// Seed-hash table XOR-ed into each round's rotated square sum. The original
/// table starts with one more word (`0x48AA_D7E4`) that its pre-incremented
/// index never reads.
const SEED_TABLE_2: [u32; 4] = [0x4B0F_3B58, 0xE874_F0C3, 0x6955_C5A6, 0x55A7_CA46];

/// The state words for `seed`. Word `i` starts from `i` and takes four hash
/// rounds; each round folds in the previous round's input, the first the
/// seed.
pub(crate) fn seeded_words(seed: u32) -> [u32; STATE_WORDS] {
    let mut words = [0; STATE_WORDS];
    for (index, word) in words.iter_mut().enumerate() {
        let mut previous = seed;
        let mut mixed = index as u32;
        for round in 0..SEED_TABLE_1.len() {
            let input = SEED_TABLE_1[round] ^ mixed;
            // Signed high half: the original shifts arithmetically.
            let high = (input as i32) >> 16;
            let low = input & 0xFFFF;
            let square_mix =
                (!high.wrapping_mul(high)).wrapping_add((low as i32).wrapping_mul(low as i32));
            let rotated = ((square_mix >> 16) as u32) | ((square_mix as u32) << 16);
            let cross = (high as u32).wrapping_mul(low);
            let next = (rotated ^ SEED_TABLE_2[round]).wrapping_add(cross) ^ previous;
            previous = mixed;
            mixed = next;
        }
        *word = mixed;
    }
    words
}

/// One raw draw: XOR the lagged pair into the word at `lead`, return it, then
/// advance both cursors with wraparound.
#[inline]
pub(crate) fn draw(words: &mut [u32], lead: &mut usize, lagged: &mut usize) -> u32 {
    let value = words[*lead] ^ words[*lagged];
    words[*lead] = value;
    *lead += 1;
    if *lead >= STATE_WORDS {
        *lead = 0;
    }
    *lagged += 1;
    if *lagged >= STATE_WORDS {
        *lagged = 0;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scale_is_its_exact_rational() {
        assert_eq!(
            f64::from_bits(RANDOM_RANGED_UNIT_SCALE.bits()),
            UNIT_SCALE_NUMERATOR as f64 * 2f64.powi(UNIT_SCALE_EXPONENT)
        );
    }

    /// Every executed native gate, at its boundary draw and both neighbours:
    /// the bridge, scorch and crew bands, and through the Spark gate every
    /// band magnitude from the smallest subnormal up, zero, negatives,
    /// infinity and NaN. Before the bound on the divisor shift, any band below
    /// 2^-135 overflowed it.
    #[test]
    fn the_threshold_takes_every_executed_native_gate() {
        for gate in ["bridge_outer", "bridge_metallic", "scorch", "spark", "crew"] {
            for (band, draw, passed) in native_gate_rows(gate) {
                assert_eq!(
                    draw < unit_draw_threshold(band),
                    passed,
                    "{gate}: band {band:e}, draw {draw:#x}"
                );
            }
        }
    }

    /// The scorch/crater coin and both stock damage Spark bands.
    #[test]
    fn the_threshold_of_the_stock_bands() {
        assert_eq!(unit_draw_threshold(0.5), 0x4000_0000);
        assert_eq!(unit_draw_threshold(0.02), 42_949_673);
        assert_eq!(unit_draw_threshold(0.01), 21_474_837);
    }

    #[test]
    fn the_threshold_admits_nothing_or_everything_outside_the_unit_interval() {
        for band in [0.0, -0.0, -1.0, f64::NEG_INFINITY] {
            assert_eq!(unit_draw_threshold(band), 0);
        }
        for band in [1.0, 2.0, f64::INFINITY, f64::NAN] {
            assert_eq!(unit_draw_threshold(band), UNIT_DRAW_COUNT);
        }
    }
}
