//! Active-retail delta-to-facing conversion.
//!
//! The finite path stores both inputs as `f32`, indexes a fixed arctangent
//! table, performs quadrant correction, then converts radians with the native
//! 65,534-unit scale. See
//! `docs/research/substrate/tables/FACING_DIRECTION_SUBSTRATE_STUDY.md`.

use std::sync::OnceLock;

use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53, X87Ordering, X87Value};

use super::native_angle_table::{ATAN_HEAD, ATAN_TAIL_DELTAS};

const ATAN_TABLE_LEN: usize = 4_097;
const ATAN_STEP: NativeF32Bits = NativeF32Bits::from_bits(0x3cc7_fe84);
const PI_OVER_TWO_F32: NativeF32Bits = NativeF32Bits::from_bits(0x3fc9_0fdb);
const PI_F64: NativeF64Bits = NativeF64Bits::from_bits(0x4009_21fb_5444_2d18);
const PI_OVER_TWO_F64: NativeF64Bits = NativeF64Bits::from_bits(0x3ff9_21fb_5444_2d18);
const FACING_SCALE_F64: NativeF64Bits = NativeF64Bits::from_bits(0xc0c4_5f07_af68_ecef);

static ATAN_BITS: OnceLock<[u32; ATAN_TABLE_LEN]> = OnceLock::new();

fn atan_bits() -> &'static [u32; ATAN_TABLE_LEN] {
    ATAN_BITS.get_or_init(|| {
        let mut bits = [0_u32; ATAN_TABLE_LEN];
        bits[..ATAN_HEAD.len()].copy_from_slice(&ATAN_HEAD);
        for (offset, delta) in ATAN_TAIL_DELTAS.iter().copied().enumerate() {
            let index = ATAN_HEAD.len() + offset;
            bits[index] = bits[index - 1] + u32::from(delta);
        }
        bits
    })
}

fn load_f32(bits: NativeF32Bits) -> X87Value {
    X87Chop53::load_f32(bits).expect("active-retail angle constants are finite normal f32")
}

fn load_f64(bits: NativeF64Bits) -> X87Value {
    X87Chop53::load_f64(bits).expect("active-retail angle constants are finite normal f64")
}

fn stored_i32(value: i32) -> X87Value {
    let bits = X87Chop53::store_f32(X87Chop53::load_i32(value))
        .expect("every i32 is representable in the verified normal f32 domain");
    load_f32(bits)
}

fn native_atan2(y: i32, x: i32) -> X87Value {
    native_atan2_f32(
        X87Chop53::store_f32(stored_i32(y)).unwrap(),
        X87Chop53::store_f32(stored_i32(x)).unwrap(),
    )
}

/// Original 4CB3D0 finite f32-input kernel. The 4CAE30 double wrapper first
/// stores each argument as f32, then executes the same table and quadrants.
pub(crate) fn native_atan2_f32(y: NativeF32Bits, x: NativeF32Bits) -> X87Value {
    let zero = load_f32(NativeF32Bits::POSITIVE_ZERO);
    let y = load_f32(y);
    let x = load_f32(x);

    if X87Chop53::compare(x, zero) == X87Ordering::Equal {
        return match X87Chop53::compare(y, zero) {
            X87Ordering::Equal => zero,
            X87Ordering::Greater => load_f32(PI_OVER_TWO_F32),
            X87Ordering::Less => load_f32(NativeF32Bits::from_bits(0xbfc9_0fdb)),
        };
    }

    let ratio = X87Chop53::div(y, x).expect("the nonzero divisor was checked");
    let table_position =
        X87Chop53::div(ratio, load_f32(ATAN_STEP)).expect("the table step is nonzero");
    let index = (X87Chop53::ftol_i64(table_position)
        .expect("finite i32 ratios fit the native integer-conversion domain")
        as i32)
        .unsigned_abs() as usize;
    let mut angle = if index < ATAN_TABLE_LEN {
        load_f32(NativeF32Bits::from_bits(atan_bits()[index]))
    } else {
        load_f32(PI_OVER_TWO_F32)
    };

    if X87Chop53::compare(x, zero) == X87Ordering::Less {
        angle = X87Chop53::sub(load_f64(PI_F64), angle);
    }
    if X87Chop53::compare(y, zero) == X87Ordering::Less {
        angle = X87Chop53::neg(angle);
    }
    angle
}

/// Returns the full native facing word for a screen-relative coordinate delta.
pub fn facing16_from_delta(dx: i32, dy: i32) -> u16 {
    let angle = native_atan2(dy.wrapping_neg(), dx);
    facing16_from_angle(angle)
}

/// Building447BFB..447C36 subtracts the two retained coordinates in x87,
/// before the atan2 wrapper narrows them to f32. An i32 delta would wrap at
/// opposite signed coordinate boundaries. Reuse the existing table/conversion.
pub(crate) fn facing16_between(from: [i32; 2], to: [i32; 2]) -> u16 {
    let difference = |a, b| {
        X87Chop53::store_f32(X87Chop53::sub(
            X87Chop53::load_i32(a),
            X87Chop53::load_i32(b),
        ))
        .expect("differences of i32 coordinates fit finite f32")
    };
    facing16_from_angle(native_atan2_f32(
        difference(from[1], to[1]),
        difference(to[0], from[0]),
    ))
}

fn facing16_from_angle(angle: X87Value) -> u16 {
    let centered = X87Chop53::sub(angle, load_f64(PI_OVER_TWO_F64));
    let scaled = X87Chop53::mul(centered, load_f64(FACING_SCALE_F64));
    X87Chop53::ftol_i64(scaled)
        .expect("the bounded angle conversion fits the native integer domain") as u16
}

/// Returns the high facing byte exposed by the simulation's byte-facing fields.
pub fn facing8_from_delta(dx: i32, dy: i32) -> u8 {
    (facing16_from_delta(dx, dy) >> 8) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored_f32_bits(value: X87Value) -> u32 {
        X87Chop53::store_f32(value).unwrap().bits()
    }

    #[test]
    fn active_table_reconstructs_exact_sentinels() {
        let bits = atan_bits();
        assert_eq!(bits[0], 0x0000_0000);
        assert_eq!(bits[1], 0x3cc7_f458);
        assert_eq!(bits[40], 0x3f46_05d2);
        assert_eq!(bits[4_096], 0x3fc7_c820);
        assert!(bits.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn normal_and_cutover_ratios_follow_the_retail_lookup_path() {
        assert_eq!(stored_f32_bits(native_atan2(1, 1)), 0x3f46_05d2);
        assert_eq!(stored_f32_bits(native_atan2(100, 1)), 0x3fc7_c820);
        assert_eq!(stored_f32_bits(native_atan2(101, 1)), 0x3fc9_0fdb);
    }

    #[test]
    fn cardinals_and_zero_use_the_native_65534_scale() {
        assert_eq!(facing16_from_delta(0, -1), 0x0000);
        assert_eq!(facing16_from_delta(1, 0), 0x3fff);
        assert_eq!(facing16_from_delta(0, 1), 0x7fff);
        assert_eq!(facing16_from_delta(-1, 0), 0xc001);
        assert_eq!(facing16_from_delta(0, 0), 0x3fff);
    }
}
