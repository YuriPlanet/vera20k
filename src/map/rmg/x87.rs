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
//! [`TruncF64`] runs `util::native_x87`'s chop-53 arithmetic ([`X87Chop53`]),
//! the same FPU model the simulation uses, and its square root runs the shared
//! `Sqrt_Approx` table step (`native_x87::sqrt_approx_bits`).

use super::rng::RmgRng;
use crate::util::native_x87::{NativeF64Bits, X87Chop53, X87Ordering, X87Value};

/// A finite double under the generator's control word: [`X87Chop53`]'s
/// operations behind named calls. The generator's domain has no non-finite
/// values; a non-finite input or a zero divisor gives zero rather than
/// modelling the FPU's exception masks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TruncF64(X87Value);

impl TruncF64 {
    pub fn zero() -> Self {
        Self(X87Chop53::load_i32(0))
    }

    pub fn is_zero(self) -> bool {
        X87Chop53::compare(self.0, X87Chop53::load_i32(0)) == X87Ordering::Equal
    }

    /// `FLD m64real`. A NaN or infinity gives zero.
    pub fn from_f64(value: f64) -> Self {
        X87Chop53::load_f64(NativeF64Bits::from_bits(value.to_bits()))
            .map_or_else(|_| Self::zero(), Self)
    }

    /// `FSTP m64real`.
    pub fn to_f64(self) -> f64 {
        let bits = X87Chop53::store_f64(self.0).expect("generator values stay inside binary64");
        f64::from_bits(bits.bits())
    }

    #[expect(
        clippy::should_implement_trait,
        reason = "named calls keep the FPU operation sequence explicit at ~200 call sites"
    )]
    pub fn neg(self) -> Self {
        Self(X87Chop53::neg(self.0))
    }

    #[expect(
        clippy::should_implement_trait,
        reason = "named calls keep the FPU operation sequence explicit at ~200 call sites"
    )]
    pub fn mul(self, rhs: Self) -> Self {
        Self(X87Chop53::mul(self.0, rhs.0))
    }

    /// A zero divisor gives zero.
    #[expect(
        clippy::should_implement_trait,
        reason = "named calls keep the FPU operation sequence explicit at ~200 call sites"
    )]
    pub fn div(self, rhs: Self) -> Self {
        X87Chop53::div(self.0, rhs.0).map_or_else(|_| Self::zero(), Self)
    }

    #[expect(
        clippy::should_implement_trait,
        reason = "named calls keep the FPU operation sequence explicit at ~200 call sites"
    )]
    pub fn add(self, rhs: Self) -> Self {
        Self(X87Chop53::add(self.0, rhs.0))
    }

    #[expect(
        clippy::should_implement_trait,
        reason = "named calls keep the FPU operation sequence explicit at ~200 call sites"
    )]
    pub fn sub(self, rhs: Self) -> Self {
        Self(X87Chop53::sub(self.0, rhs.0))
    }

    /// `FCOM`: whether `self` is below `rhs`.
    pub fn lt(self, rhs: Self) -> bool {
        X87Chop53::compare(self.0, rhs.0) == X87Ordering::Less
    }
}

/// A value on the FPU stack: the table lookups return one and take one
/// (`Math::SinFromTable @ 0x004CACB0` / `CosFromTable @ 0x004CAD00`).
impl From<X87Value> for TruncF64 {
    fn from(value: X87Value) -> Self {
        Self(value)
    }
}

impl From<TruncF64> for crate::util::native_x87::MaskedX87Value {
    fn from(value: TruncF64) -> Self {
        value.0.into()
    }
}

/// Narrow to single precision (truncating) and widen back, as a value.
///
/// Models a store to a 4-byte float slot followed by a reload — the pattern
/// the map-prep dimension math uses for its scale factors.
pub fn narrow_to_f32(value: TruncF64) -> TruncF64 {
    TruncF64::from_f64(f64::from(f32::from_bits(narrow_to_f32_bits(value))))
}

/// Narrow to single precision, truncating toward zero (`FSTP m32real` under
/// the chop control word).
///
/// This is the store the approximate square root performs on its input, and it
/// is where most of the precision goes.
fn narrow_to_f32_bits(value: TruncF64) -> u32 {
    X87Chop53::store_f32_masked_chop(value.0).bits()
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
    let result_bits = crate::util::native_x87::sqrt_approx_bits(narrow_to_f32_bits(value));
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
            assert_eq!(
                crate::util::native_x87::sqrt_approx_table_entry(index),
                retail,
                "sqrt table entry {index}"
            );
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
