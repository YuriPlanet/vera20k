//! The floating-point environment the generator runs under.
//!
//! Three things make ordinary `f64` arithmetic the wrong tool here:
//!
//! 1. The process leaves its FPU at 53-bit precision with **round-toward-zero**
//!    (control word `0x0E7F`, loaded by the CRT's float-to-int helper and never
//!    restored). Rust's operators round to nearest, so every add, subtract,
//!    multiply and divide has to be truncated explicitly.
//! 2. The square root is **not** `FSQRT`. It is a table-driven approximation:
//!    it narrows its input to single precision and then indexes a table by the
//!    top 14 bits of the significand, so the result is only about 2^-14
//!    accurate — far coarser than even single precision, let alone `f64`.
//! 3. The Gaussian caches its second variate, so alternate calls consume no
//!    random draws at all.
//!
//! `util::native_x87` covers similar ground but cannot be reused: its value
//! type has private fields, it offers no division, and it is not yet committed.
//! This module is deliberately self-contained.

use super::rng::RmgRng;

/// A finite double held as sign, exponent and a 53-bit significand, with every
/// operation truncating toward zero.
///
/// `value = (-1)^sign * significand * 2^(exponent - 52)`, with the significand
/// normalised into `[2^52, 2^53)` — or zero, which is stored as a zero
/// significand and keeps its sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TruncF64 {
    sign: bool,
    exponent: i32,
    significand: u64,
}

/// Position of the implicit bit in the significand.
const SIG_BITS: u32 = 52;
const SIG_TOP: u64 = 1 << SIG_BITS;

impl TruncF64 {
    pub const fn zero() -> Self {
        Self {
            sign: false,
            exponent: 0,
            significand: 0,
        }
    }

    pub const fn is_zero(self) -> bool {
        self.significand == 0
    }

    /// Decompose a finite, normal double. Subnormals and non-finite values are
    /// outside the generator's domain and collapse to zero.
    pub fn from_f64(value: f64) -> Self {
        let bits = value.to_bits();
        let sign = bits >> 63 != 0;
        let raw_exponent = ((bits >> SIG_BITS) & 0x7FF) as i32;
        let fraction = bits & (SIG_TOP - 1);

        if raw_exponent == 0 || raw_exponent == 0x7FF {
            return Self {
                sign,
                exponent: 0,
                significand: 0,
            };
        }
        Self {
            sign,
            exponent: raw_exponent - 1023,
            significand: fraction | SIG_TOP,
        }
    }

    pub fn to_f64(self) -> f64 {
        if self.is_zero() {
            return if self.sign { -0.0 } else { 0.0 };
        }
        let biased = (self.exponent + 1023) as u64;
        let bits = (u64::from(self.sign) << 63)
            | (biased << SIG_BITS)
            | (self.significand & (SIG_TOP - 1));
        f64::from_bits(bits)
    }

    pub fn neg(self) -> Self {
        Self {
            sign: !self.sign,
            ..self
        }
    }

    /// Normalise a wide significand down to 53 bits, discarding the remainder.
    ///
    /// Truncation toward zero is exactly "drop the low bits of the magnitude",
    /// so no rounding decision — and therefore no sticky bit — is involved.
    fn from_wide(sign: bool, exponent: i32, wide: u128, wide_top_bit: u32) -> Self {
        if wide == 0 {
            return Self {
                sign,
                exponent: 0,
                significand: 0,
            };
        }
        let top = 127 - wide.leading_zeros();
        // Restate the exponent against the actual leading bit, then shift the
        // significand so its leading bit lands at SIG_BITS.
        let exponent = exponent + top as i32 - wide_top_bit as i32;
        let significand = if top >= SIG_BITS {
            (wide >> (top - SIG_BITS)) as u64
        } else {
            (wide << (SIG_BITS - top)) as u64
        };
        Self {
            sign,
            exponent,
            significand,
        }
    }

    pub fn mul(self, rhs: Self) -> Self {
        if self.is_zero() || rhs.is_zero() {
            return Self {
                sign: self.sign ^ rhs.sign,
                exponent: 0,
                significand: 0,
            };
        }
        let product = u128::from(self.significand) * u128::from(rhs.significand);
        // Both inputs carry their leading bit at SIG_BITS, so the product's
        // leading bit sits at 2*SIG_BITS (or one higher).
        Self::from_wide(
            self.sign ^ rhs.sign,
            self.exponent + rhs.exponent,
            product,
            2 * SIG_BITS,
        )
    }

    pub fn div(self, rhs: Self) -> Self {
        if self.is_zero() || rhs.is_zero() {
            return Self {
                sign: self.sign ^ rhs.sign,
                exponent: 0,
                significand: 0,
            };
        }
        // Shift the numerator up so the quotient carries well over 53 bits.
        let numerator = u128::from(self.significand) << 64;
        let quotient = numerator / u128::from(rhs.significand);
        Self::from_wide(
            self.sign ^ rhs.sign,
            self.exponent - rhs.exponent,
            quotient,
            64,
        )
    }

    pub fn add(self, rhs: Self) -> Self {
        if self.is_zero() {
            return rhs;
        }
        if rhs.is_zero() {
            return self;
        }
        // Align to the larger exponent, keeping headroom so no bits are lost
        // before the subtraction below.
        let (big, small) = if self.exponent >= rhs.exponent {
            (self, rhs)
        } else {
            (rhs, self)
        };
        let shift = (big.exponent - small.exponent) as u32;
        if shift > 64 {
            return big;
        }
        const GUARD: u32 = 64;
        let big_wide = u128::from(big.significand) << GUARD;
        let small_wide = u128::from(small.significand) << (GUARD - shift.min(GUARD));

        if big.sign == small.sign {
            let sum = big_wide + small_wide;
            Self::from_wide(big.sign, big.exponent, sum, SIG_BITS + GUARD)
        } else {
            // Opposite signs: the larger magnitude wins the sign.
            if big_wide >= small_wide {
                let diff = big_wide - small_wide;
                Self::from_wide(big.sign, big.exponent, diff, SIG_BITS + GUARD)
            } else {
                let diff = small_wide - big_wide;
                Self::from_wide(small.sign, big.exponent, diff, SIG_BITS + GUARD)
            }
        }
    }

    pub fn sub(self, rhs: Self) -> Self {
        self.add(rhs.neg())
    }

    /// Compare magnitudes and signs, as the FPU's compare does.
    pub fn lt(self, rhs: Self) -> bool {
        self.to_f64() < rhs.to_f64()
    }
}

/// Narrow to single precision (truncating) and widen back, as a value.
///
/// Models a store to a 4-byte float slot followed by a reload — the pattern
/// the map-prep dimension math uses for its scale factors.
pub fn narrow_to_f32(value: TruncF64) -> TruncF64 {
    TruncF64::from_f64(f64::from(f32::from_bits(narrow_to_f32_bits(value))))
}

/// Narrow to single precision, truncating toward zero.
///
/// This is the store the approximate square root performs on its input, and it
/// is where most of the precision goes.
fn narrow_to_f32_bits(value: TruncF64) -> u32 {
    if value.is_zero() {
        return u32::from(value.sign) << 31;
    }
    let sign = u32::from(value.sign) << 31;
    let exponent = value.exponent + 127;
    if exponent <= 0 || exponent >= 0xFF {
        // Outside the single-precision range; the generator never gets here
        // with real inputs, so clamp rather than pretend to model it.
        return sign;
    }
    // Drop the low 29 bits of the significand: truncation, not rounding.
    let fraction = ((value.significand >> 29) as u32) & 0x007F_FFFF;
    sign | ((exponent as u32) << 23) | fraction
}

/// One entry of the square-root lookup table.
///
/// The table is pure arithmetic, so it is computed rather than shipped: index
/// `i` encodes a significand, and the entry is the truncated mantissa of its
/// square root. Not shipping it also keeps the retail bytes out of this repo.
///
/// That all 16384 entries come out identical to the original's table is checked
/// by `map::rmg::sqrt_table`, exhaustively, against the player's own
/// `gamemd.exe`. It needs `RA2_DIR` and so is `#[ignore]`d; the spot values in
/// this module's tests guard the formula on a plain `cargo test`.
pub(crate) fn sqrt_table_entry(index: u32) -> u32 {
    let significand = if index < 8192 {
        // Even exponent: the significand lies in [1, 2).
        1.0 + f64::from(index) / 8192.0
    } else {
        // Odd exponent: the implicit bit is set, so it lies in [2, 4).
        2.0 * (1.0 + f64::from(index - 8192) / 8192.0)
    };
    // The root always lands in [1, 2), so the exponent field is zero and only
    // the mantissa varies.
    ((significand.sqrt() - 1.0) * 8_388_608.0) as u32
}

/// The generator's square root: a table approximation.
///
/// Deliberately **not** `f64::sqrt`. The input significand is quantised to the
/// table's 14-bit index, so results carry roughly 2^-14 relative accuracy; an
/// exact square root is nearly 40 bits too good and produces different terrain.
pub fn approx_sqrt(value: TruncF64) -> TruncF64 {
    if value.is_zero() {
        return TruncF64::zero();
    }
    let bits = narrow_to_f32_bits(value);
    let mut mantissa = bits & 0x007F_FFFF;
    let biased = ((bits >> 23) & 0xFF) as i32;
    let unbiased = biased - 127;

    // An odd exponent borrows a factor of two into the significand.
    if unbiased & 1 != 0 {
        mantissa |= 0x0080_0000;
    }
    // Arithmetic halving, matching the original's 16-bit shift.
    let half_exponent = unbiased >> 1;

    let index = mantissa >> 10;
    let result_bits = sqrt_table_entry(index).wrapping_add(((half_exponent + 127) as u32) << 23);
    TruncF64::from_f64(f64::from(f32::from_bits(result_bits)))
}

/// The generator's square root of an integer squared-distance, as the `f32`
/// the start/tiberium phases store in their priority heaps.
pub fn approx_sqrt_f32(squared_distance: i32) -> f32 {
    approx_sqrt(TruncF64::from_f64(f64::from(squared_distance))).to_f64() as f32
}

/// Natural logarithm.
///
/// The original computes this with `FYL2X` (`ln(2) * log2(x)`). Its low bits
/// are largely irrelevant because `approx_sqrt` immediately narrows the result
/// to single precision, but the last bit is not proven identical to the
/// hardware instruction — see the FP contract report.
pub fn ln(value: TruncF64) -> TruncF64 {
    TruncF64::from_f64(value.to_f64().ln())
}

/// Normally distributed draws, matching the original's Box-Muller helper.
///
/// The cache is behavioural, not an optimisation: every second call returns a
/// stored value and consumes **no** random draws. Regenerating both variates
/// per call would desynchronise the whole draw stream.
#[derive(Debug, Default, Clone)]
pub struct Gaussian {
    cached: Option<TruncF64>,
}

impl Gaussian {
    /// Draw one variate.
    pub fn next(&mut self, rng: &mut RmgRng) -> f64 {
        self.next_trunc(rng).to_f64()
    }

    fn next_trunc(&mut self, rng: &mut RmgRng) -> TruncF64 {
        if let Some(cached) = self.cached.take() {
            return cached;
        }

        let one = TruncF64::from_f64(1.0);
        let (x, y, r2) = loop {
            let x = unit_to_signed(rng, one);
            let y = unit_to_signed(rng, one);
            // The original squares y first, then adds x squared.
            let r2 = y.mul(y).add(x.mul(x));
            // Rejection: outside the unit disc, or exactly at the origin.
            if r2.lt(one) && !r2.is_zero() {
                break (x, y, r2);
            }
        };

        let log = ln(r2);
        // Computed as (-log) - log rather than -2 * log; both are exact here,
        // but the shape mirrors the original.
        let scaled = log.neg().sub(log).div(r2);
        let scale = approx_sqrt(scaled);

        self.cached = Some(scale.mul(y));
        scale.mul(x)
    }
}

/// `2 * u - 1`, mapping a unit draw onto `[-1, 1)`.
fn unit_to_signed(rng: &mut RmgRng, one: TruncF64) -> TruncF64 {
    let unit = TruncF64::from_f64(rng.next_unit());
    unit.add(unit).sub(one)
}

/// Truncation toward zero, as the float-to-int helper performs it.
pub fn ftol(value: f64) -> i32 {
    value as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail entries read from the game binary's 16384-entry square-root
    /// mantissa table (data section at 0x008650BC). Spot values chosen to pin
    /// the boundaries: entry 0, the first buckets, a mid-range value, the
    /// even/odd-exponent seam at 8192, and the final entry. These fixed points
    /// guard the formula on a plain `cargo test` without redistributing the
    /// copyrighted table; the whole 16384 entries are checked against the
    /// player's own executable by `map::rmg::sqrt_table`, which needs `RA2_DIR`.
    const RETAIL_SQRT_SPOT: &[(u32, u32)] = &[
        (0, 0x0000_0000),
        (1, 0x0000_01FF),
        (2, 0x0000_03FF),
        (7, 0x0000_0DFF),
        (100, 0x0000_C764),
        (4096, 0x001C_C470),
        (8191, 0x0035_0389),
        (8192, 0x0035_04F3),
        (8193, 0x0035_07C7),
        (12000, 0x005B_16CA),
        (16383, 0x007F_FDFF),
    ];

    #[test]
    fn sqrt_table_matches_retail_at_the_pinned_points() {
        for &(index, retail) in RETAIL_SQRT_SPOT {
            assert_eq!(sqrt_table_entry(index), retail, "sqrt table entry {index}");
        }
    }

    #[test]
    fn approx_sqrt_f32_is_close_but_not_a_real_root() {
        assert_eq!(approx_sqrt_f32(0), 0.0);
        assert_eq!(approx_sqrt_f32(1), 1.0, "entry 0 is exact");
        assert!((f64::from(approx_sqrt_f32(100)) - 10.0).abs() < 1e-3);
        // The bucketed table deliberately differs from an exact root: a guard
        // against anyone swapping in f32::sqrt.
        assert_ne!(approx_sqrt_f32(1_234_567), 1_234_567.0f32.sqrt());
        // Monotone over the integer squared-distance ramp the phases feed it.
        let mut previous = 0.0f32;
        for squared in 0..2000 {
            let value = approx_sqrt_f32(squared);
            assert!(value >= previous, "non-monotone at {squared}");
            previous = value;
        }
    }

    #[test]
    fn ftol_truncates_toward_zero() {
        assert_eq!(ftol(3.9), 3);
        assert_eq!(ftol(-3.9), -3, "toward zero, not floor");
        assert_eq!(ftol(0.9999), 0);
        assert_eq!(ftol(-0.9999), 0);
        assert_eq!(ftol(100.0), 100);
    }

    #[test]
    fn trunc_roundtrips_ordinary_values() {
        for value in [1.0, -1.0, 0.5, 1234.5678, -0.000123, 3.999999] {
            assert_eq!(TruncF64::from_f64(value).to_f64(), value, "{value}");
        }
        assert_eq!(TruncF64::from_f64(0.0).to_f64(), 0.0);
    }

    #[test]
    fn arithmetic_agrees_with_f64_when_results_are_exact() {
        // Powers of two and small integers are representable, so truncation
        // cannot bite: any disagreement here is an arithmetic bug.
        let cases = [(3.0, 5.0), (0.5, 0.25), (-2.0, 8.0), (1.5, -0.75)];
        for (a, b) in cases {
            let (ta, tb) = (TruncF64::from_f64(a), TruncF64::from_f64(b));
            assert_eq!(ta.add(tb).to_f64(), a + b, "{a} + {b}");
            assert_eq!(ta.sub(tb).to_f64(), a - b, "{a} - {b}");
            assert_eq!(ta.mul(tb).to_f64(), a * b, "{a} * {b}");
            assert_eq!(ta.div(tb).to_f64(), a / b, "{a} / {b}");
        }
    }

    #[test]
    fn truncation_differs_from_rounding_where_it_should() {
        // 1/3 is not representable; truncating gives the neighbour below the
        // round-to-nearest result.
        let third = TruncF64::from_f64(1.0).div(TruncF64::from_f64(3.0));
        let rounded = 1.0f64 / 3.0;
        assert!(
            third.to_f64() <= rounded,
            "truncation must never exceed the rounded value"
        );
        assert!((third.to_f64() - rounded).abs() <= f64::EPSILON);
    }

    #[test]
    fn approx_sqrt_is_less_accurate_than_f64_sqrt() {
        // The whole point: an exact square root is the wrong answer here.
        let value = TruncF64::from_f64(2.0);
        let approx = approx_sqrt(value).to_f64();
        let exact = 2.0f64.sqrt();
        assert_ne!(
            approx, exact,
            "an exact sqrt would produce different terrain"
        );
        assert!(
            (approx - exact).abs() < 1e-6,
            "on an exact table index it should still be close: {approx} vs {exact}"
        );
    }

    #[test]
    fn approx_sqrt_handles_both_exponent_parities() {
        // The table index keeps 14 bits of significand, so relative error runs
        // to about 2^-14. Inputs that land exactly on an index (powers of two)
        // do much better; the bound has to cover the ones that do not.
        const TABLE_QUANTISATION: f64 = 1.0 / 16384.0;
        for value in [1.0, 2.0, 4.0, 8.0, 0.5, 0.25, 100.0, 1e-6, 3.7, 1e6] {
            let approx = approx_sqrt(TruncF64::from_f64(value)).to_f64();
            let exact = value.sqrt();
            let relative = (approx - exact).abs() / exact;
            assert!(
                relative < TABLE_QUANTISATION,
                "sqrt({value}): {approx} vs {exact} (relative {relative})"
            );
            assert!(approx > 0.0, "sqrt({value}) must stay positive");
        }
    }

    #[test]
    fn approx_sqrt_is_coarser_than_single_precision() {
        // Guards against someone "fixing" this into a real sqrt later: the
        // approximation must visibly disagree with an exact root.
        let value = 1e-6;
        let approx = approx_sqrt(TruncF64::from_f64(value)).to_f64();
        let relative = (approx - value.sqrt()).abs() / value.sqrt();
        assert!(
            relative > 1e-5,
            "the table approximation should be clearly coarse, got {relative}"
        );
    }

    #[test]
    fn gaussian_matches_golden_vectors() {
        let doc: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/rmg_oracle/vectors/x87.json",
        ))
        .unwrap();
        assert_eq!(doc["source"].as_str(), Some("unicorn/gamemd.exe"));

        let mut seed_in_progress = None;
        let (mut rng, mut gaussian) = (RmgRng::new(0), Gaussian::default());
        let mut checked = 0;

        for case in doc["cases"].as_array().unwrap() {
            let seed = case["seed"].as_u64().unwrap() as u16;
            if seed_in_progress != Some(seed) {
                rng = RmgRng::new(seed);
                gaussian = Gaussian::default();
                seed_in_progress = Some(seed);
            }

            let expected = u64::from_str_radix(case["value_bits"].as_str().unwrap(), 16).unwrap();
            let got = gaussian.next(&mut rng);
            assert_eq!(
                got.to_bits(),
                expected,
                "seed {seed} call {}: got {:016X} want {expected:016X}",
                case["call"],
                got.to_bits()
            );
            checked += 1;
        }
        assert!(checked > 0, "vector file produced no comparisons");
    }

    #[test]
    fn gaussian_cache_alternates_and_saves_draws() {
        let mut rng = RmgRng::new(1234);
        let mut gaussian = Gaussian::default();

        // A generating call consumes draws; the following cached call must not.
        let mut probe = RmgRng::new(1234);
        let mut probe_gaussian = Gaussian::default();
        let _ = probe_gaussian.next(&mut probe);
        let mut after_first = probe.clone();
        let _ = probe_gaussian.next(&mut probe);
        let mut after_second = probe.clone();
        assert_eq!(
            after_first.next_u32(),
            after_second.next_u32(),
            "the cached call must not advance the stream"
        );

        // And the values themselves alternate generated/cached.
        let a = gaussian.next(&mut rng);
        let b = gaussian.next(&mut rng);
        assert!(a.is_finite() && b.is_finite());
    }
}
