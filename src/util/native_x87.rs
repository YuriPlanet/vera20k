//! Deterministic integer implementation of the finite x87 subset used by gamemd.
//!
//! The active process uses 53-bit precision and truncate-toward-zero rounding.
//! Callers name every operation and memory store so evaluation order stays visible.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use thiserror::Error;

mod masked;
pub use masked::{MaskedX87Chop53, MaskedX87Ordering, MaskedX87Value};

const SIGNIFICAND_TOP: u64 = 1_u64 << 52;
const EXTENDED_TOP: u64 = 1_u64 << 55;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NativeF32Bits(u32);

impl NativeF32Bits {
    pub const POSITIVE_ZERO: Self = Self(0x0000_0000);
    pub const NEGATIVE_ZERO: Self = Self(0x8000_0000);
    pub const ONE: Self = Self(0x3f80_0000);

    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NativeF64Bits(u64);

impl NativeF64Bits {
    pub const POSITIVE_ZERO: Self = Self(0x0000_0000_0000_0000);
    pub const NEGATIVE_ZERO: Self = Self(0x8000_0000_0000_0000);
    pub const HALF: Self = Self(0x3fe0_0000_0000_0000);
    pub const ONE: Self = Self(0x3ff0_0000_0000_0000);

    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }
}

const ADJUST_FOR_Z_THRESHOLD_LEPTONS: i32 = 728;

/// Standard-session height-to-screen multiplier initialized by active YR.
///
/// Startup writer `0x006D1BDD` stores exactly `0x3FC25E5374344960`.
pub const STANDARD_ADJUST_FOR_Z_MULTIPLIER: NativeF64Bits =
    NativeF64Bits::from_bits(0x3fc2_5e53_7434_4960);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X87Ordering {
    Less,
    Equal,
    Greater,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NativeX87Error {
    #[error("{format} NaN or infinity is outside the verified x87 domain")]
    NonFiniteInput { format: &'static str },
    #[error("{format} overflow is outside the verified x87 domain")]
    StoreOverflow { format: &'static str },
    #[error("x87 division by zero is outside the verified finite domain")]
    DivisionByZero,
    #[error("x87 integer conversion is outside the verified signed 64-bit domain")]
    IntegerConversion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct X87Value {
    sign: bool,
    exponent: i32,
    significand: u64,
}

impl X87Value {
    const fn zero(sign: bool) -> Self {
        Self {
            sign,
            exponent: 0,
            significand: 0,
        }
    }

    const fn is_zero(self) -> bool {
        self.significand == 0
    }

    /// FLD normalizes a finite memory subnormal in the wider x87 exponent range.
    /// `least_exponent` names the value of the fraction's least significant bit.
    fn from_subnormal(sign: bool, fraction: u64, least_exponent: i32) -> Self {
        debug_assert_ne!(fraction, 0);
        let top = 63 - fraction.leading_zeros();
        Self {
            sign,
            exponent: least_exponent + top as i32,
            significand: fraction << (52 - top),
        }
    }

    fn magnitude_cmp(self, rhs: Self) -> Ordering {
        self.exponent
            .cmp(&rhs.exponent)
            .then_with(|| self.significand.cmp(&rhs.significand))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct X87Chop53;

impl X87Chop53 {
    pub fn load_i32(value: i32) -> X87Value {
        if value == 0 {
            return X87Value::zero(false);
        }
        let sign = value.is_negative();
        let magnitude = value.unsigned_abs() as u64;
        let top = 63 - magnitude.leading_zeros();
        X87Value {
            sign,
            exponent: top as i32,
            significand: magnitude << (52 - top),
        }
    }

    pub fn load_f32(bits: NativeF32Bits) -> Result<X87Value, NativeX87Error> {
        let raw = bits.bits();
        let sign = raw >> 31 != 0;
        let exponent = (raw >> 23) & 0xff;
        let fraction = raw & 0x007f_ffff;
        if exponent == 0xff {
            return Err(NativeX87Error::NonFiniteInput { format: "f32" });
        }
        if exponent == 0 {
            if fraction == 0 {
                return Ok(X87Value::zero(sign));
            }
            return Ok(X87Value::from_subnormal(sign, u64::from(fraction), -149));
        }
        Ok(X87Value {
            sign,
            exponent: exponent as i32 - 127,
            significand: ((1_u64 << 23) | fraction as u64) << 29,
        })
    }

    pub fn load_f64(bits: NativeF64Bits) -> Result<X87Value, NativeX87Error> {
        let raw = bits.bits();
        let sign = raw >> 63 != 0;
        let exponent = (raw >> 52) & 0x7ff;
        let fraction = raw & 0x000f_ffff_ffff_ffff;
        if exponent == 0x7ff {
            return Err(NativeX87Error::NonFiniteInput { format: "f64" });
        }
        if exponent == 0 {
            if fraction == 0 {
                return Ok(X87Value::zero(sign));
            }
            return Ok(X87Value::from_subnormal(sign, fraction, -1074));
        }
        Ok(X87Value {
            sign,
            exponent: exponent as i32 - 1023,
            significand: (1_u64 << 52) | fraction,
        })
    }

    pub fn neg(value: X87Value) -> X87Value {
        X87Value {
            sign: !value.sign,
            ..value
        }
    }

    pub fn add(lhs: X87Value, rhs: X87Value) -> X87Value {
        if lhs.is_zero() && rhs.is_zero() {
            return X87Value::zero(lhs.sign && rhs.sign);
        }
        if lhs.is_zero() {
            return rhs;
        }
        if rhs.is_zero() {
            return lhs;
        }

        let mut high = lhs;
        let mut low = rhs;
        if high.exponent < low.exponent {
            std::mem::swap(&mut high, &mut low);
        }
        let exponent_gap = (high.exponent - low.exponent) as u32;
        let high_extended = high.significand << 3;
        let low_extended = shift_right_jam_u64(low.significand << 3, exponent_gap);

        if high.sign == low.sign {
            let mut sum = high_extended + low_extended;
            let mut exponent = high.exponent;
            if sum & (EXTENDED_TOP << 1) != 0 {
                sum = shift_right_jam_u64(sum, 1);
                exponent += 1;
            }
            return chop_extended(high.sign, exponent, sum);
        }

        if high_extended == low_extended {
            return X87Value::zero(false);
        }
        let (sign, mut difference) = if high_extended > low_extended {
            (high.sign, high_extended - low_extended)
        } else {
            (low.sign, low_extended - high_extended)
        };
        let top = 63 - difference.leading_zeros();
        let normalize = 55 - top;
        difference <<= normalize;
        chop_extended(sign, high.exponent - normalize as i32, difference)
    }

    pub fn sub(lhs: X87Value, rhs: X87Value) -> X87Value {
        Self::add(lhs, Self::neg(rhs))
    }

    pub fn mul(lhs: X87Value, rhs: X87Value) -> X87Value {
        if lhs.is_zero() || rhs.is_zero() {
            return X87Value::zero(lhs.sign ^ rhs.sign);
        }
        let product = lhs.significand as u128 * rhs.significand as u128;
        let top = 127 - product.leading_zeros();
        let shift = top - 55;
        let extended = shift_right_jam_u128(product, shift);
        let exponent = lhs.exponent + rhs.exponent + (top as i32 - 104);
        chop_extended(lhs.sign ^ rhs.sign, exponent, extended)
    }

    pub fn div(lhs: X87Value, rhs: X87Value) -> Result<X87Value, NativeX87Error> {
        if rhs.is_zero() {
            return Err(NativeX87Error::DivisionByZero);
        }
        if lhs.is_zero() {
            return Ok(X87Value::zero(lhs.sign ^ rhs.sign));
        }

        let quotient = ((lhs.significand as u128) << 64) / rhs.significand as u128;
        let top = 127 - quotient.leading_zeros();
        let shift = top - 52;
        Ok(X87Value {
            sign: lhs.sign ^ rhs.sign,
            exponent: lhs.exponent - rhs.exponent + (top as i32 - 64),
            significand: (quotient >> shift) as u64,
        })
    }

    pub fn compare(lhs: X87Value, rhs: X87Value) -> X87Ordering {
        // Zero has to be settled before `magnitude_cmp`, which orders on the
        // exponent first. A zero carries exponent 0, so comparing it against a
        // value below 1 — whose exponent is negative — would otherwise report
        // that value as the SMALLER one. `FCOM` against `+0.0` is exactly the
        // shape every unit-interval probability gate uses, so this is not a
        // corner case.
        match (lhs.is_zero(), rhs.is_zero()) {
            (true, true) => return X87Ordering::Equal,
            (true, false) => {
                return if rhs.sign {
                    X87Ordering::Greater
                } else {
                    X87Ordering::Less
                };
            }
            (false, true) => {
                return if lhs.sign {
                    X87Ordering::Less
                } else {
                    X87Ordering::Greater
                };
            }
            (false, false) => {}
        }
        let ordering = if lhs.sign != rhs.sign {
            if lhs.sign {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        } else {
            let magnitude = lhs.magnitude_cmp(rhs);
            if lhs.sign {
                magnitude.reverse()
            } else {
                magnitude
            }
        };
        match ordering {
            Ordering::Less => X87Ordering::Less,
            Ordering::Equal => X87Ordering::Equal,
            Ordering::Greater => X87Ordering::Greater,
        }
    }

    pub fn store_f32(value: X87Value) -> Result<NativeF32Bits, NativeX87Error> {
        let sign = u32::from(value.sign) << 31;
        if value.is_zero() {
            return Ok(NativeF32Bits::from_bits(sign));
        }
        if value.exponent > 127 {
            return Err(NativeX87Error::StoreOverflow { format: "f32" });
        }
        if value.exponent < -126 {
            // Original FSTP m32real (70B812), masked PC53/chop: discard all
            // bits below 2^-149, preserving the sign even when none remain.
            // Native comparison: tools/spatial_oracle/x87_subnormal_primitives.
            let fraction = if value.exponent < -149 {
                0
            } else {
                (value.significand >> (29 + (-126 - value.exponent) as u32)) as u32
            };
            return Ok(NativeF32Bits::from_bits(sign | fraction));
        }
        let exponent = ((value.exponent + 127) as u32) << 23;
        let fraction = ((value.significand >> 29) as u32) & 0x007f_ffff;
        Ok(NativeF32Bits::from_bits(sign | exponent | fraction))
    }

    /// Masked `FST[P] m32real` under the native chop control word `0x0E7F`.
    /// Finite overflow stores signed maximum finite, rather than infinity.
    /// Original `4891D4` spills in `tools/spatial_oracle/estimated_damage`
    /// cover both overflow signs. The fallible storage API remains separate.
    pub fn store_f32_masked_chop(value: X87Value) -> NativeF32Bits {
        match Self::store_f32(value) {
            Ok(bits) => bits,
            Err(NativeX87Error::StoreOverflow { format: "f32" }) => {
                NativeF32Bits::from_bits((u32::from(value.sign) << 31) | 0x7f7f_ffff)
            }
            Err(error) => unreachable!("unexpected finite f32 store error: {error}"),
        }
    }

    pub fn store_f64(value: X87Value) -> Result<NativeF64Bits, NativeX87Error> {
        let sign = u64::from(value.sign) << 63;
        if value.is_zero() {
            return Ok(NativeF64Bits::from_bits(sign));
        }
        if value.exponent > 1023 {
            return Err(NativeX87Error::StoreOverflow { format: "f64" });
        }
        if value.exponent < -1022 {
            // Original FSTP m64real (4B10F6) has the same chop policy at 2^-1074.
            let fraction = if value.exponent < -1074 {
                0
            } else {
                value.significand >> (-1022 - value.exponent) as u32
            };
            return Ok(NativeF64Bits::from_bits(sign | fraction));
        }
        let exponent = ((value.exponent + 1023) as u64) << 52;
        let fraction = value.significand & 0x000f_ffff_ffff_ffff;
        Ok(NativeF64Bits::from_bits(sign | exponent | fraction))
    }

    pub fn ftol_i64(value: X87Value) -> Result<i64, NativeX87Error> {
        if value.is_zero() || value.exponent < 0 {
            return Ok(0);
        }
        if value.exponent > 63 {
            return Err(NativeX87Error::IntegerConversion);
        }
        let magnitude = if value.exponent <= 52 {
            (value.significand >> (52 - value.exponent)) as u128
        } else {
            let shift = (value.exponent - 52) as u32;
            (value.significand as u128) << shift
        };
        if value.sign {
            if magnitude > (1_u128 << 63) {
                return Err(NativeX87Error::IntegerConversion);
            }
            if magnitude == 1_u128 << 63 {
                return Ok(i64::MIN);
            }
            Ok(-(magnitude as i64))
        } else {
            if magnitude > i64::MAX as u128 {
                return Err(NativeX87Error::IntegerConversion);
            }
            Ok(magnitude as i64)
        }
    }

    /// Native `Math::ftol 7C5F00` when its caller consumes only EAX.
    /// `FISTP qword` first converts toward zero to signed64; narrowing keeps
    /// its low32 bits. Masked invalid conversion stores `i64::MIN`, whose low
    /// dword is zero. Only that finite conversion error takes this path.
    /// Original conversion/output pairs: `tools/spatial_oracle/estimated_damage`.
    pub fn ftol_i32_low_masked(value: X87Value) -> i32 {
        match Self::ftol_i64(value) {
            Ok(integer) => integer as i32,
            Err(NativeX87Error::IntegerConversion) => 0,
            Err(error) => unreachable!("unexpected finite integer conversion error: {error}"),
        }
    }
}

/// Active `gamemd.exe` `Sqrt_Approx` (`0x004CAC40`).
///
/// The helper first stores its finite positive input as an x87-chopped `f32`,
/// then indexes the retail 16,384-entry mantissa table.  The table is a pure
/// arithmetic sequence, so computing its entry keeps the executable's bytes
/// out of the repository while preserving the exact result bits.
pub fn sqrt_approx_f32(value: X87Value) -> Result<NativeF32Bits, NativeX87Error> {
    let magnitude = X87Chop53::store_f32(value)?.bits() & 0x7fff_ffff;
    if magnitude == 0 {
        return Ok(NativeF32Bits::POSITIVE_ZERO);
    }

    let mut mantissa = magnitude & 0x007f_ffff;
    let unbiased = ((magnitude >> 23) & 0xff) as i32 - 127;
    if unbiased & 1 != 0 {
        mantissa |= 0x0080_0000;
    }

    let index = mantissa >> 10;
    let significand = if index < 8192 {
        1.0 + f64::from(index) / 8192.0
    } else {
        2.0 * (1.0 + f64::from(index - 8192) / 8192.0)
    };
    let table_entry = ((significand.sqrt() - 1.0) * 8_388_608.0) as u32;
    let half_exponent = unbiased >> 1;
    Ok(NativeF32Bits::from_bits(
        table_entry.wrapping_add(((half_exponent + 127) as u32) << 23),
    ))
}

/// Evaluate the native height-to-screen conversion with an injected multiplier.
///
/// gamemd-derived: active YR `Tactical__AdjustForZ` at `0x006D20E0` multiplies
/// signed world Z by the startup-owned factor, adds one at Z >= 728, adds 0.5,
/// then converts with x87 `ftol` under 53-bit/truncate-toward-zero control.
pub fn adjust_for_z_with_multiplier(
    world_z: i32,
    multiplier: NativeF64Bits,
) -> Result<i32, NativeX87Error> {
    let product = X87Chop53::mul(
        X87Chop53::load_i32(world_z),
        X87Chop53::load_f64(multiplier)?,
    );
    let correction = X87Chop53::load_i32(i32::from(world_z >= ADJUST_FOR_Z_THRESHOLD_LEPTONS));
    let corrected = X87Chop53::add(product, correction);
    let biased = X87Chop53::add(corrected, X87Chop53::load_f64(NativeF64Bits::HALF)?);
    Ok(X87Chop53::ftol_i64(biased)? as i32)
}

/// Evaluate active YR's standard-session height-to-screen conversion.
pub fn adjust_for_z_standard(world_z: i32) -> i32 {
    adjust_for_z_with_multiplier(world_z, STANDARD_ADJUST_FOR_Z_MULTIPLIER)
        .expect("the verified finite standard multiplier maps every i32 Z into i32")
}

/// `CoordStruct__Distance3D` 0x0041C380 — the 3D lepton distance every native
/// coordinate comparison goes through.
///
/// The body squares and sums the three deltas on the x87 stack, stores the sum
/// to **f32**, runs [`sqrt_approx_f32`] (the 14-bit table lookup at
/// 0x004CAC40), then truncates through `Math__ftol`. The approximation and the
/// truncation both matter: callers compare the whole-lepton result, so two
/// objects whose true distances differ in the fraction tie, and the
/// approximation's ~3e-5 relative error can move that tie by one lepton.
/// Substituting an exact integer square root silently changes which object wins
/// those comparisons.
pub fn distance_3d_leptons(lhs: [i32; 3], rhs: [i32; 3]) -> i32 {
    let mut squared = X87Chop53::load_i32(0);
    for axis in 0..3 {
        let delta = X87Chop53::load_i32(lhs[axis].wrapping_sub(rhs[axis]));
        squared = X87Chop53::add(squared, X87Chop53::mul(delta, delta));
    }
    let root_bits =
        sqrt_approx_f32(squared).expect("map-space squared distance stays in finite f32 range");
    let root =
        X87Chop53::load_f32(root_bits).expect("Sqrt_Approx always returns a finite normal or zero");
    X87Chop53::ftol_i64(root).expect("map-space distance fits a signed integer") as i32
}

fn chop_extended(sign: bool, exponent: i32, extended: u64) -> X87Value {
    let significand = extended >> 3;
    debug_assert!(significand == 0 || significand & SIGNIFICAND_TOP != 0);
    X87Value {
        sign,
        exponent,
        significand,
    }
}

fn shift_right_jam_u64(value: u64, distance: u32) -> u64 {
    if distance == 0 {
        value
    } else if distance < 64 {
        (value >> distance) | u64::from(value << (64 - distance) != 0)
    } else {
        u64::from(value != 0)
    }
}

fn shift_right_jam_u128(value: u128, distance: u32) -> u64 {
    if distance == 0 {
        value as u64
    } else if distance < 128 {
        ((value >> distance) as u64) | u64::from(value << (128 - distance) != 0)
    } else {
        u64::from(value != 0)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn compare_orders_against_zero_by_sign_not_exponent() {
        use super::{NativeF64Bits, X87Chop53, X87Ordering};

        // `FCOM` against `+0.0` is the shape every unit-interval probability
        // gate uses. `magnitude_cmp` orders on the exponent first, and a zero
        // carries exponent 0, so before the fix a value in `(0, 1)` — whose
        // exponent is negative — compared as the SMALLER one.
        let zero = X87Chop53::load_f64(NativeF64Bits::POSITIVE_ZERO).expect("zero loads");
        let negative_zero = X87Chop53::load_f64(NativeF64Bits::NEGATIVE_ZERO).expect("zero loads");
        let half = X87Chop53::load_f64(NativeF64Bits::HALF).expect("0.5 loads");
        let minus_half = X87Chop53::neg(half);

        assert_eq!(X87Chop53::compare(half, zero), X87Ordering::Greater);
        assert_eq!(X87Chop53::compare(zero, half), X87Ordering::Less);
        assert_eq!(X87Chop53::compare(minus_half, zero), X87Ordering::Less);
        assert_eq!(X87Chop53::compare(zero, minus_half), X87Ordering::Greater);
        // x87 treats the two zeroes as equal.
        assert_eq!(X87Chop53::compare(zero, negative_zero), X87Ordering::Equal);
        // A value at or above 1 was already ordered correctly; keep it pinned.
        let one = X87Chop53::load_f64(NativeF64Bits::ONE).expect("1.0 loads");
        assert_eq!(X87Chop53::compare(one, zero), X87Ordering::Greater);
    }
    use super::*;

    fn f32_value(bits: u32) -> X87Value {
        X87Chop53::load_f32(NativeF32Bits::from_bits(bits)).unwrap()
    }

    fn f64_value(bits: u64) -> X87Value {
        X87Chop53::load_f64(NativeF64Bits::from_bits(bits)).unwrap()
    }

    #[test]
    fn signed_zero_round_trips_without_canonicalization() {
        let positive = X87Chop53::load_f32(NativeF32Bits::POSITIVE_ZERO).unwrap();
        let negative = X87Chop53::load_f32(NativeF32Bits::NEGATIVE_ZERO).unwrap();
        assert_eq!(X87Chop53::store_f32(positive).unwrap().bits(), 0x0000_0000);
        assert_eq!(X87Chop53::store_f32(negative).unwrap().bits(), 0x8000_0000);
        assert_eq!(X87Chop53::compare(positive, negative), X87Ordering::Equal);
        assert_eq!(
            X87Chop53::store_f32(X87Chop53::sub(positive, positive))
                .unwrap()
                .bits(),
            0x0000_0000,
        );
    }

    #[test]
    fn i32_to_f32_store_chops_at_the_24_bit_boundary() {
        let positive = X87Chop53::load_i32(16_777_217);
        let negative = X87Chop53::load_i32(-16_777_217);
        assert_eq!(X87Chop53::store_f32(positive).unwrap().bits(), 0x4b80_0000);
        assert_eq!(X87Chop53::store_f32(negative).unwrap().bits(), 0xcb80_0000);
    }

    #[test]
    fn pc53_addition_chops_half_ulp_and_keeps_full_ulp() {
        let one = f64_value(0x3ff0_0000_0000_0000);
        let half_ulp = f64_value(0x3ca0_0000_0000_0000);
        let full_ulp = f64_value(0x3cb0_0000_0000_0000);
        assert_eq!(
            X87Chop53::store_f64(X87Chop53::add(one, half_ulp))
                .unwrap()
                .bits(),
            0x3ff0_0000_0000_0000,
        );
        assert_eq!(
            X87Chop53::store_f64(X87Chop53::add(one, full_ulp))
                .unwrap()
                .bits(),
            0x3ff0_0000_0000_0001,
        );
    }

    #[test]
    fn subtraction_and_double_gravity_have_explicit_f32_boundaries() {
        let zero = f32_value(0x0000_0000);
        let gravity = f32_value(0x40c0_0000);
        let persistent = X87Chop53::sub(zero, gravity);
        let persistent_bits = X87Chop53::store_f32(persistent).unwrap();
        let probe = X87Chop53::sub(X87Chop53::load_f32(persistent_bits).unwrap(), gravity);
        assert_eq!(persistent_bits.bits(), 0xc0c0_0000);
        assert_eq!(X87Chop53::store_f32(probe).unwrap().bits(), 0xc140_0000);
    }

    #[test]
    fn multiplication_and_compare_use_chopped_53_bit_values() {
        let half = f64_value(0x3fe0_0000_0000_0000);
        let quarter = X87Chop53::mul(half, half);
        assert_eq!(
            X87Chop53::store_f64(quarter).unwrap().bits(),
            0x3fd0_0000_0000_0000,
        );
        assert_eq!(X87Chop53::compare(quarter, half), X87Ordering::Less);
    }

    #[test]
    fn division_uses_chopped_53_bit_values() {
        let one = f64_value(0x3ff0_0000_0000_0000);
        let three = f64_value(0x4008_0000_0000_0000);
        let third = X87Chop53::div(one, three).unwrap();
        assert_eq!(
            X87Chop53::store_f64(third).unwrap().bits(),
            0x3fd5_5555_5555_5555,
        );
        assert_eq!(
            X87Chop53::store_f64(X87Chop53::neg(third)).unwrap().bits(),
            0xbfd5_5555_5555_5555,
        );
        assert_eq!(
            X87Chop53::div(one, f64_value(0x0000_0000_0000_0000)),
            Err(NativeX87Error::DivisionByZero),
        );
    }

    #[test]
    fn ftol_chops_positive_and_negative_values_toward_zero() {
        assert_eq!(
            X87Chop53::ftol_i64(f64_value(0x400e_0000_0000_0000)).unwrap(),
            3
        );
        assert_eq!(
            X87Chop53::ftol_i64(f64_value(0xc00e_0000_0000_0000)).unwrap(),
            -3
        );
    }

    #[test]
    fn unverified_exceptional_domains_return_typed_errors() {
        assert_eq!(
            X87Chop53::load_f32(NativeF32Bits::from_bits(0x7f80_0000)),
            Err(NativeX87Error::NonFiniteInput { format: "f32" }),
        );
        assert_eq!(
            X87Chop53::load_f64(NativeF64Bits::from_bits(0x7ff0_0000_0000_0000)),
            Err(NativeX87Error::NonFiniteInput { format: "f64" }),
        );
    }

    #[test]
    fn finite_subnormal_primitives_match_original_x87_instructions() {
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/x87_subnormal_primitives.json"
        ))
        .unwrap();
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 1780);
        let mut subnormal_inputs = 0;
        let mut subnormal_outputs = 0;
        let mut signed_zero_outputs = 0;
        for row in rows {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let format = input["format"].as_str().unwrap();
            let mut load = |key: &str| {
                let bits = u64::from_str_radix(input[key].as_str().unwrap(), 16).unwrap();
                if format == "f32" {
                    subnormal_inputs +=
                        usize::from(bits & 0x7f80_0000 == 0 && bits & 0x007f_ffff != 0);
                    X87Chop53::load_f32(NativeF32Bits::from_bits(bits as u32)).unwrap()
                } else {
                    assert_eq!(format, "f64");
                    subnormal_inputs += usize::from(
                        bits & 0x7ff0_0000_0000_0000 == 0 && bits & 0x000f_ffff_ffff_ffff != 0,
                    );
                    X87Chop53::load_f64(NativeF64Bits::from_bits(bits)).unwrap()
                }
            };
            let lhs = load("lhs_bits");
            let actual = match input["operation"].as_str().unwrap() {
                "load" => lhs,
                "add" => X87Chop53::add(lhs, load("rhs_bits")),
                "sub" => X87Chop53::sub(lhs, load("rhs_bits")),
                "mul" => X87Chop53::mul(lhs, load("rhs_bits")),
                operation => panic!("unknown operation {operation}: {name}"),
            };
            let expected = u64::from_str_radix(row["output_bits"].as_str().unwrap(), 16).unwrap();
            if input["store"].as_str().unwrap() == "f32" {
                let actual = X87Chop53::store_f32(actual).unwrap().bits();
                assert_eq!(u64::from(actual), expected, "{name}");
                subnormal_outputs +=
                    usize::from(actual & 0x7f80_0000 == 0 && actual & 0x007f_ffff != 0);
                signed_zero_outputs += usize::from(actual == 0x8000_0000);
            } else {
                let actual = X87Chop53::store_f64(actual).unwrap().bits();
                assert_eq!(actual, expected, "{name}");
                subnormal_outputs += usize::from(
                    actual & 0x7ff0_0000_0000_0000 == 0 && actual & 0x000f_ffff_ffff_ffff != 0,
                );
                signed_zero_outputs += usize::from(actual == 0x8000_0000_0000_0000);
            }
        }
        assert!(subnormal_inputs > 1000);
        assert!(subnormal_outputs > 100);
        assert!(signed_zero_outputs > 100);
    }

    #[test]
    fn finite_overlay_velocity_addition_matches_original_common_tail() {
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/unit_per_cell_overlay.json"
        ))
        .unwrap();
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 296);
        let addend = f32_value(0x3ca3_d70a);
        let mut additions = 0;
        for row in rows {
            let input = &row["input"];
            let output = &row["output"];
            let bits = u32::from_str_radix(input["velocity_bits"].as_str().unwrap(), 16).unwrap();
            let loaded = X87Chop53::load_f32(NativeF32Bits::from_bits(bits)).unwrap();
            assert_eq!(X87Chop53::store_f32(loaded).unwrap().bits(), bits);
            let actual = if output["applied"].as_bool().unwrap() {
                additions += 1;
                X87Chop53::add(loaded, addend)
            } else {
                loaded
            };
            let expected =
                u32::from_str_radix(output["velocity_bits"].as_str().unwrap(), 16).unwrap();
            assert_eq!(
                X87Chop53::store_f32(actual).unwrap().bits(),
                expected,
                "{}",
                input["name"].as_str().unwrap(),
            );
        }
        assert_eq!(additions, 144);
    }

    #[test]
    fn retail_sqrt_approx_uses_the_quantized_mantissa_table() {
        let two = X87Chop53::load_i32(2);
        assert_eq!(sqrt_approx_f32(two).unwrap().bits(), 0x3fb5_04f3);

        let lower_half_last = X87Chop53::load_f32(NativeF32Bits::from_bits(0x3fff_fc00)).unwrap();
        assert_eq!(
            sqrt_approx_f32(lower_half_last).unwrap().bits(),
            0x3fb5_0389
        );
        let upper_half_last = X87Chop53::load_f32(NativeF32Bits::from_bits(0x407f_fc00)).unwrap();
        assert_eq!(
            sqrt_approx_f32(upper_half_last).unwrap().bits(),
            0x3fff_fdff
        );

        let value = X87Chop53::load_i32(1_234_567);
        let approximate = sqrt_approx_f32(value).unwrap().bits();
        assert_ne!(approximate, (1_234_567.0f32.sqrt()).to_bits());
    }

    #[test]
    fn adjust_for_z_standard_matches_retail_fixtures_and_signed_edges() {
        assert_eq!(
            STANDARD_ADJUST_FOR_Z_MULTIPLIER.bits(),
            0x3fc2_5e53_7434_4960
        );
        assert_eq!(adjust_for_z_standard(0), 0);
        assert_eq!(adjust_for_z_standard(104), 15);
        assert_eq!(adjust_for_z_standard(208), 30);
        assert_eq!(adjust_for_z_standard(256), 37);
        assert_eq!(adjust_for_z_standard(727), 104);
        assert_eq!(adjust_for_z_standard(728), 105);
        assert_eq!(adjust_for_z_standard(1_500), 216);
        assert_eq!(adjust_for_z_standard(-104), -14);
        assert_eq!(adjust_for_z_standard(-400), -56);
    }
}
