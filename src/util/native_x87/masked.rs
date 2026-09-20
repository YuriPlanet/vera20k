//! Masked exceptional values around the existing finite PC53/chop owner.
//!
//! Original `489180` comparisons/stores/conversions in
//! `tools/spatial_oracle/estimated_damage` cover the damage consumer, including
//! parser-admitted infinities and arithmetic-generated NaNs. The separate
//! `x87_masked_hardware` corpus compares authenticated primitive fragments on
//! hardware; `x87_masked_values` retains Unicorn's signaling-load discrepancy.
//! This models values,
//! not x87 exception flags, unmasked traps, or an entire FPU register stack.

use super::{NativeF32Bits, NativeF64Bits, X87Chop53, X87Ordering, X87Value};

const QUIET: u64 = 1 << 62;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Value {
    Finite(X87Value),
    Infinity {
        negative: bool,
    },
    /// Quiet payload in the x87 significand's lower63 bits.
    NaN {
        negative: bool,
        payload: u64,
    },
}

impl Value {
    fn negative(self) -> bool {
        match self {
            Self::Finite(value) => value.sign,
            Self::Infinity { negative } | Self::NaN { negative, .. } => negative,
        }
    }

    fn zero(self) -> bool {
        matches!(self, Self::Finite(value) if value.is_zero())
    }

    fn indefinite() -> Self {
        Self::NaN {
            negative: true,
            payload: QUIET,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskedX87Ordering {
    Less,
    Equal,
    Greater,
    Unordered,
}

/// Opaque value: only loads and arithmetic can construct valid encodings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaskedX87Value(Value);

/// Value semantics for PC53/chop with all exceptions masked.
///
/// Original warhead execution covers the damage consumer's exceptional paths;
/// `x87_masked_hardware` covers representative operand pairs of this API,
/// including NaN arbitration and signed infinity/zero division. Neither claims
/// full extended-exponent overflow, arbitrary operation chains or unmasked traps.
pub struct MaskedX87Chop53;

impl MaskedX87Chop53 {
    pub fn load_i32(value: i32) -> MaskedX87Value {
        MaskedX87Value(Value::Finite(X87Chop53::load_i32(value)))
    }

    pub fn load_f32(bits: NativeF32Bits) -> MaskedX87Value {
        let raw = bits.bits();
        if raw & 0x7f80_0000 != 0x7f80_0000 {
            return MaskedX87Value(Value::Finite(
                X87Chop53::load_f32(bits).expect("finite bits"),
            ));
        }
        let negative = raw >> 31 != 0;
        let fraction = raw & 0x007f_ffff;
        MaskedX87Value(if fraction == 0 {
            Value::Infinity { negative }
        } else {
            Value::NaN {
                negative,
                payload: (u64::from(fraction) << 40) | QUIET,
            }
        })
    }

    pub fn load_f64(bits: NativeF64Bits) -> MaskedX87Value {
        let raw = bits.bits();
        if raw & 0x7ff0_0000_0000_0000 != 0x7ff0_0000_0000_0000 {
            return MaskedX87Value(Value::Finite(
                X87Chop53::load_f64(bits).expect("finite bits"),
            ));
        }
        let negative = raw >> 63 != 0;
        let fraction = raw & 0x000f_ffff_ffff_ffff;
        MaskedX87Value(if fraction == 0 {
            Value::Infinity { negative }
        } else {
            Value::NaN {
                negative,
                payload: (fraction << 11) | QUIET,
            }
        })
    }

    fn propagated_nan(lhs: Value, rhs: Value) -> Option<Value> {
        use Value::NaN;
        match (lhs, rhs) {
            (
                NaN {
                    negative: left_sign,
                    payload: left,
                },
                NaN {
                    negative: right_sign,
                    payload: right,
                },
            ) => {
                // Both inputs are already quiet loaded register values. Larger
                // payload wins; equal payloads select the positive encoding.
                Some(if (left, !left_sign) >= (right, !right_sign) {
                    lhs
                } else {
                    rhs
                })
            }
            (NaN { .. }, _) => Some(lhs),
            (_, NaN { .. }) => Some(rhs),
            _ => None,
        }
    }

    fn neg_value(value: Value) -> Value {
        match value {
            Value::Finite(value) => Value::Finite(X87Chop53::neg(value)),
            Value::Infinity { negative } => Value::Infinity {
                negative: !negative,
            },
            Value::NaN { negative, payload } => Value::NaN {
                negative: !negative,
                payload,
            },
        }
    }

    /// FCHS toggles sign, including infinities, zero and quiet NaNs.
    pub fn neg(value: MaskedX87Value) -> MaskedX87Value {
        MaskedX87Value(Self::neg_value(value.0))
    }

    pub fn add(lhs: MaskedX87Value, rhs: MaskedX87Value) -> MaskedX87Value {
        let (lhs, rhs) = (lhs.0, rhs.0);
        if let Some(nan) = Self::propagated_nan(lhs, rhs) {
            return MaskedX87Value(nan);
        }
        MaskedX87Value(match (lhs, rhs) {
            (Value::Finite(left), Value::Finite(right)) => {
                Value::Finite(X87Chop53::add(left, right))
            }
            (Value::Infinity { negative: left }, Value::Infinity { negative: right })
                if left != right =>
            {
                Value::indefinite()
            }
            (infinity @ Value::Infinity { .. }, _) | (_, infinity @ Value::Infinity { .. }) => {
                infinity
            }
            _ => unreachable!("NaNs handled above"),
        })
    }

    pub fn sub(lhs: MaskedX87Value, rhs: MaskedX87Value) -> MaskedX87Value {
        // FSUB propagates an input NaN without negating that NaN's sign.
        match Self::propagated_nan(lhs.0, rhs.0) {
            Some(nan) => MaskedX87Value(nan),
            None => Self::add(lhs, MaskedX87Value(Self::neg_value(rhs.0))),
        }
    }

    pub fn mul(lhs: MaskedX87Value, rhs: MaskedX87Value) -> MaskedX87Value {
        let (lhs, rhs) = (lhs.0, rhs.0);
        if let Some(nan) = Self::propagated_nan(lhs, rhs) {
            return MaskedX87Value(nan);
        }
        MaskedX87Value(match (lhs, rhs) {
            (Value::Finite(left), Value::Finite(right)) => {
                Value::Finite(X87Chop53::mul(left, right))
            }
            _ if lhs.zero() || rhs.zero() => Value::indefinite(),
            _ => Value::Infinity {
                negative: lhs.negative() ^ rhs.negative(),
            },
        })
    }

    pub fn div(lhs: MaskedX87Value, rhs: MaskedX87Value) -> MaskedX87Value {
        let (lhs, rhs) = (lhs.0, rhs.0);
        if let Some(nan) = Self::propagated_nan(lhs, rhs) {
            return MaskedX87Value(nan);
        }
        let negative = lhs.negative() ^ rhs.negative();
        MaskedX87Value(match (lhs, rhs) {
            (Value::Infinity { .. }, Value::Infinity { .. }) => Value::indefinite(),
            _ if lhs.zero() && rhs.zero() => Value::indefinite(),
            (Value::Infinity { .. }, _) => Value::Infinity { negative },
            (_, Value::Infinity { .. }) => Value::Finite(X87Value::zero(negative)),
            _ if rhs.zero() => Value::Infinity { negative },
            (Value::Finite(left), Value::Finite(right)) => {
                Value::Finite(X87Chop53::div(left, right).expect("nonzero finite divisor"))
            }
            _ => unreachable!("NaNs handled above"),
        })
    }

    pub fn compare(lhs: MaskedX87Value, rhs: MaskedX87Value) -> MaskedX87Ordering {
        use MaskedX87Ordering::{Equal, Greater, Less, Unordered};
        match (lhs.0, rhs.0) {
            (Value::NaN { .. }, _) | (_, Value::NaN { .. }) => Unordered,
            (Value::Finite(left), Value::Finite(right)) => match X87Chop53::compare(left, right) {
                X87Ordering::Less => Less,
                X87Ordering::Equal => Equal,
                X87Ordering::Greater => Greater,
            },
            (Value::Infinity { negative: left }, Value::Infinity { negative: right }) => {
                if left == right {
                    Equal
                } else if left {
                    Less
                } else {
                    Greater
                }
            }
            (Value::Infinity { negative }, _) => {
                if negative {
                    Less
                } else {
                    Greater
                }
            }
            (_, Value::Infinity { negative }) => {
                if negative {
                    Greater
                } else {
                    Less
                }
            }
        }
    }

    pub fn store_f32_masked_chop(value: MaskedX87Value) -> NativeF32Bits {
        match value.0 {
            Value::Finite(value) => X87Chop53::store_f32_masked_chop(value),
            Value::Infinity { negative } => {
                NativeF32Bits::from_bits((u32::from(negative) << 31) | 0x7f80_0000)
            }
            Value::NaN { negative, payload } => NativeF32Bits::from_bits(
                (u32::from(negative) << 31) | 0x7f80_0000 | (payload >> 40) as u32,
            ),
        }
    }

    pub fn ftol_i32_low_masked(value: MaskedX87Value) -> i32 {
        match value.0 {
            Value::Finite(value) => X87Chop53::ftol_i32_low_masked(value),
            Value::Infinity { .. } | Value::NaN { .. } => 0,
        }
    }

    /// FSTP binary64 with masked exceptions and chop. Finite overflow saturates
    /// to signed maximum finite; nonfinite values retain their quiet payload.
    /// Native70CF69 is one production spill consumer.
    pub fn store_f64_masked_chop(value: MaskedX87Value) -> NativeF64Bits {
        match value.0 {
            Value::Finite(finite) => match X87Chop53::store_f64(finite) {
                Ok(bits) => bits,
                Err(super::NativeX87Error::StoreOverflow { format: "f64" }) => {
                    NativeF64Bits::from_bits((u64::from(finite.sign) << 63) | 0x7fef_ffff_ffff_ffff)
                }
                Err(error) => unreachable!("binary64 masked store: {error:?}"),
            },
            Value::Infinity { negative } => {
                NativeF64Bits::from_bits((u64::from(negative) << 63) | 0x7ff0_0000_0000_0000)
            }
            Value::NaN { negative, payload } => NativeF64Bits::from_bits(
                (u64::from(negative) << 63) | 0x7ff0_0000_0000_0000 | (payload >> 11),
            ),
        }
    }
}

#[cfg(test)]
#[path = "masked_tests.rs"]
mod tests;
