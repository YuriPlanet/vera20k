//! Retail sine and cosine, for the facing-quantised rotations gamemd performs.
//!
//! gamemd does not compute trig at runtime for these: `Math__SinFromTable @
//! 0x004CACB0` and `Math__CosFromTable @ 0x004CAD00` both end in a single
//! `FLD float ptr [EAX*4 + 0x84F084]; RET` — a raw `f32` read out of one shared
//! table, with no interpolation. Every rotation that quantises its angle to a
//! multiple of pi/16 therefore reduces to a table read, and porting the read is
//! exact where recomputing the angle is not.
//!
//! Shared retail table data, bounded FLH lookups and integer Walk displacement.

// Original 0084F084 table, extracted and checked by walk_direction_table.py.
// The small FLH arrays below are derived lookups for its different, f32-narrowed
// angle input. Walk uses a full signed direction and must not quantize to 32 steps.
const RETAIL_SINE_TABLE: &[u8; 10241 * 4] = include_bytes!("native_trig_table.bin");

fn sine_table_bits(index: usize) -> u32 {
    let offset = index * 4;
    u32::from_le_bytes(RETAIL_SINE_TABLE[offset..offset + 4].try_into().unwrap())
}

fn walk_sine_index(facing: u16) -> usize {
    // 75C067: signed direction - 3FFF, times double 7E2810. Sin/Cos then
    // multiply by float 8223B0/8223B4. Their exact product is this dyadic ratio.
    // Integer evaluation gives the same truncated index for ALL 65536 words
    // as the original instruction sequence (walk_direction_table corpus).
    const NUMERATOR: i128 = 75_560_166_591_041_506_239_877;
    const DENOMINATOR: i128 = 1 << 78;
    let direction = i128::from(facing as i16) - 0x3FFF;
    let raw = (-direction * NUMERATOR / DENOMINATOR) as i32;
    let mut index = (raw / 2).rem_euclid(8192);
    if raw & 1 != 0 && index < 8191 {
        index += 1;
    }
    index as usize
}

fn advance_integer_coordinate(origin: i32, speed: i32, coefficient: u32) -> i32 {
    // Keep the table's binary fraction as an integer numerator/denominator.
    // Rounding it to I16F16 first would erase tiny negative cardinal entries:
    // native truncates the final world coordinate, so those can pay one lepton.
    // No host floating point or additional x87 emulation is used here.
    if coefficient & 0x7FFF_FFFF == 0 || speed == 0 {
        return origin;
    }
    let exponent = ((coefficient >> 23) & 0xFF) as i32;
    let shift = (150 - exponent) as u32;
    let mantissa = i128::from((coefficient & 0x7F_FFFF) | 0x80_0000);
    let signed = if coefficient >> 31 == 0 {
        mantissa
    } else {
        -mantissa
    };
    let numerator = (i128::from(origin) << shift) + signed * i128::from(speed);
    i32::try_from(numerator / (1_i128 << shift)).expect("Walk world coordinate exceeds i32")
}

/// Walk75C067..75C0CB: advance using the newly requested full direction word.
/// The body snap may retain its old destination on equality; movement still
/// consumes the new direction. Inputs/outputs are whole world leptons, with Z
/// and subsequent placement owned by the caller. Table products remain exact
/// at the simulation's bounded integer movement speeds; final truncation is
/// toward zero, after adding each displacement to its world coordinate.
pub(crate) fn walk_step_world_xy(current: [i32; 2], facing: u16, speed: i32) -> [i32; 2] {
    let index = walk_sine_index(facing);
    [
        advance_integer_coordinate(current[0], speed, sine_table_bits(index + 2048)),
        advance_integer_coordinate(current[1], speed, sine_table_bits(index) ^ 0x8000_0000),
    ]
}

/// The reachable index range on both `TechnoClass::GetFLH` branches.
///
/// `k` is the raw SIGNED step count: `dir32 - 8` on the simple branch, and the
/// difference of two such values on the turret branch. It is never reduced
/// modulo 32 — the wrapping pairs genuinely disagree in the retail table
/// (`k = -9` and `k = +23` return different sines), so folding it is a bug.
pub const NATIVE_TRIG_MIN_STEP: i32 = -31;
/// Inclusive upper bound of [`NATIVE_TRIG_MIN_STEP`]'s range.
pub const NATIVE_TRIG_MAX_STEP: i32 = 31;

/// How `TechnoClass::GetFLH @ 0x006F3AD0` forms the angle these entries answer:
///
/// ```text
/// FILD  dword [k]              ; the signed step, as an integer
/// FMUL  double [0x007E4408]    ; times -(pi/16), a DOUBLE
/// FSTP  double [tmp]           ; narrowed to f64
/// FLD   double [tmp]
/// FSTP  float  [arg]           ; then to f32 -- this is what RotateZ receives
/// ```
///
/// The double constant is load-bearing. Forming the angle as
/// `f32(k) * f32(-0.19634955)` instead selects a DIFFERENT table entry at 22 of
/// the 63 reachable steps, because the index truncates and the two products
/// straddle a boundary. The entries below were taken at the indices the
/// disassembled index computation produces for the double form.
///
/// `sin(-(pi/16) * k)` exactly as retail returns it, indexed by `k + 31`.
///
/// These are the bytes at `0x0084F084 + idx * 4`, not computed values: the
/// retail table differs from a correctly-rounded `f32` sine on 4997 of its
/// 10241 entries, and its scale constant is slightly low, so negative angles
/// land two index steps short. That asymmetry is real and load-bearing —
/// `k = -1` yields `+0.195090309` while `k = +1` yields `-0.193585575`, which
/// is NOT its negation. Do not regenerate this table from `f32::sin`.
pub const NATIVE_SIN_BY_STEP: [u32; 63] = [
    0xBE47C5C1, 0xBEC3EF15, 0xBF0E39D9, 0xBF3504F3, 0xBF54DB31, 0xBF6C835E, 0xBF7B14BE, 0xBF800000,
    0xBF7B14BE, 0xBF6C835E, 0xBF54DB31, 0xBF3504F3, 0xBF0E39D9, 0xBEC3EF15, 0xBE47C5C1, 0x250D3000,
    0x3E47C5C1, 0x3EC3EF15, 0x3F0E39D9, 0x3F3504F3, 0x3F54DB31, 0x3F6C835E, 0x3F7B14BE, 0x3F800000,
    0x3F7B14BE, 0x3F6C835E, 0x3F54DB31, 0x3F3504F3, 0x3F0E39D9, 0x3EC3EF15, 0x3E47C5C1, 0x00000000,
    0xBE463B4C, 0xBEC33544, 0xBF0DE638, 0xBF34BDCF, 0xBF54A346, 0xBF6C5CD3, 0xBF7B010E, 0xBF7FFFEC,
    0xBF7B2847, 0xBF6CA9C4, 0xBF5512FA, 0xBF354BFB, 0xBF0E8D65, 0xBEC4A8C7, 0xBE495018, 0xBAC90FD5,
    0x3E463B4C, 0x3EC33544, 0x3F0DE638, 0x3F34BDCF, 0x3F54A346, 0x3F6C5CD3, 0x3F7B010E, 0x3F7FFFEC,
    0x3F7B2847, 0x3F6CA9C4, 0x3F5512FA, 0x3F354BFB, 0x3F0E8D65, 0x3EC4A8C7, 0x3E495018,
];

/// `cos(-(pi/16) * k)` from the same table, indexed by `k + 31`.
pub const NATIVE_COS_BY_STEP: [u32; 63] = [
    0x3F7B14BE, 0x3F6C835E, 0x3F54DB31, 0x3F3504F3, 0x3F0E39D9, 0x3EC3EF15, 0x3E47C5C1, 0xA58D3000,
    0xBE47C5C1, 0xBEC3EF15, 0xBF0E39D9, 0xBF3504F3, 0xBF54DB31, 0xBF6C835E, 0xBF7B14BE, 0xBF800000,
    0xBF7B14BE, 0xBF6C835E, 0xBF54DB31, 0xBF3504F3, 0xBF0E39D9, 0xBEC3EF15, 0xBE47C5C1, 0x250D3000,
    0x3E47C5C1, 0x3EC3EF15, 0x3F0E39D9, 0x3F3504F3, 0x3F54DB31, 0x3F6C835E, 0x3F7B14BE, 0x3F800000,
    0x3F7B2847, 0x3F6CA9C4, 0x3F5512FA, 0x3F354BFB, 0x3F0E8D65, 0x3EC4A8C7, 0x3E495018, 0x3AC90FD5,
    0xBE463B4C, 0xBEC33544, 0xBF0DE638, 0xBF34BDCF, 0xBF54A346, 0xBF6C5CD3, 0xBF7B010E, 0xBF7FFFEC,
    0xBF7B2847, 0xBF6CA9C4, 0xBF5512FA, 0xBF354BFB, 0xBF0E8D65, 0xBEC4A8C7, 0xBE495018, 0xBAC90FD5,
    0x3E463B4C, 0x3EC33544, 0x3F0DE638, 0x3F34BDCF, 0x3F54A346, 0x3F6C5CD3, 0x3F7B010E,
];

/// Look up the retail `(sin, cos)` pair for a signed quarter-facing step.
///
/// Returns `None` outside the reachable range rather than wrapping, because a
/// wrap would silently return the wrong entry.
pub fn native_sin_cos_by_step(step: i32) -> Option<(f32, f32)> {
    if !(NATIVE_TRIG_MIN_STEP..=NATIVE_TRIG_MAX_STEP).contains(&step) {
        return None;
    }
    let idx = (step - NATIVE_TRIG_MIN_STEP) as usize;
    Some((
        f32::from_bits(NATIVE_SIN_BY_STEP[idx]),
        f32::from_bits(NATIVE_COS_BY_STEP[idx]),
    ))
}

/// Rotate a point about Z by a quantised facing step, the way
/// `Matrix3x4_RotateZ @ 0x005AF1A0` does.
///
/// gamemd-derived: starting from the identity, `RotateZ` leaves the 3x4 matrix
/// as `[cos, -sin, 0, tx; sin, cos, 0, ty; 0, 0, 1, tz]` — it updates only
/// columns 0 and 1, as `col0' = col0*cos + col1*sin` and
/// `col1' = col1*cos - col0*sin`. Applying that to a point through
/// `Matrix3x4_TransformPoint @ 0x005AFB80` gives the two expressions below; Z
/// is carried through untouched.
///
/// `step` is the signed quarter-facing count, NOT an angle: the caller keeps it
/// signed and unwrapped so the asymmetric table is indexed exactly as retail
/// indexes it. Returns `None` for an out-of-range step rather than wrapping.
pub fn rotate_z_by_step(point: (f32, f32, f32), step: i32) -> Option<(f32, f32, f32)> {
    let (sin, cos) = native_sin_cos_by_step(step)?;
    let (px, py, pz) = point;
    Some((px * cos - py * sin, px * sin + py * cos, pz))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_walk_heading_uses_the_original_trig_entries() {
        use crate::util::sha256::{Sha256, digest_hex, sha256_hex};
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/walk_direction_table.json"
        ))
        .unwrap();
        assert_eq!(sha256_hex(RETAIL_SINE_TABLE), oracle["table_sha256"]);
        let mut indexes = Sha256::new();
        let mut values = Sha256::new();
        for heading in 0..=u16::MAX {
            let sine = walk_sine_index(heading);
            for index in [sine, sine + 2048] {
                indexes.update(&(index as u16).to_le_bytes());
                values.update(&sine_table_bits(index).to_le_bytes());
            }
        }
        assert_eq!(
            digest_hex(indexes.finalize()),
            oracle["sin_cos_index_sha256"]
        );
        assert_eq!(digest_hex(values.finalize()), oracle["sin_cos_bits_sha256"]);
    }

    #[test]
    fn gsi_08_04_table_covers_the_full_signed_step_range() {
        assert_eq!(NATIVE_SIN_BY_STEP.len(), 63);
        assert_eq!(NATIVE_COS_BY_STEP.len(), 63);
        assert!(native_sin_cos_by_step(NATIVE_TRIG_MIN_STEP).is_some());
        assert!(native_sin_cos_by_step(NATIVE_TRIG_MAX_STEP).is_some());
        assert!(native_sin_cos_by_step(32).is_none());
        assert!(native_sin_cos_by_step(-32).is_none());
    }

    /// Zero is exact in the retail table, which is the one entry a wrong index
    /// base would most obviously break.
    #[test]
    fn gsi_08_04_zero_step_is_exactly_zero_and_one() {
        let (sin, cos) = native_sin_cos_by_step(0).expect("in range");
        assert_eq!(sin.to_bits(), 0x0000_0000);
        assert_eq!(cos.to_bits(), 0x3F80_0000);
    }

    /// The table is NOT odd-symmetric: retail's negative-angle indices land two
    /// steps short of the true angle. A port that negates instead of reading is
    /// wrong for half the facings, so this pins the asymmetry.
    #[test]
    fn gsi_08_04_table_is_not_odd_symmetric() {
        let (minus_one, _) = native_sin_cos_by_step(-1).expect("in range");
        let (plus_one, _) = native_sin_cos_by_step(1).expect("in range");
        assert_eq!(minus_one.to_bits(), 0x3E47_C5C1);
        assert_eq!(plus_one.to_bits(), 0xBE46_3B4C);
        assert_ne!(plus_one, -minus_one);
    }

    /// Wrapping `k` modulo 32 returns a different entry — the trap that makes a
    /// "tidier" 32-entry table wrong.
    #[test]
    fn gsi_08_04_steps_32_apart_disagree() {
        let (a, _) = native_sin_cos_by_step(-9).expect("in range");
        let (b, _) = native_sin_cos_by_step(23).expect("in range");
        assert_ne!(a.to_bits(), b.to_bits());
    }

    /// The zero step is the identity, and a quarter turn moves `+X` onto the
    /// other axis — the orientation a transposed rotation would swap.
    ///
    /// Note what the retail table does at the two quarter turns: step `-8` is
    /// exact (sin 1.0, cos 0.0), while step `+8` returns sin `-0.999998808` and
    /// cos `+0.001533980`. That ~0.0015 error is RETAIL's, produced by its
    /// slightly-low scale constant and a truncating index, and reproducing it is
    /// the point — a port that used a real cosine would be the one that drifts.
    #[test]
    fn gsi_08_04_rotate_z_matches_the_native_matrix_layout() {
        let identity = rotate_z_by_step((190.0, 25.0, 120.0), 0).expect("in range");
        assert_eq!(identity, (190.0, 25.0, 120.0));

        let (sin_neg, cos_neg) = native_sin_cos_by_step(-8).expect("in range");
        assert_eq!(
            sin_neg.to_bits(),
            0x3F80_0000,
            "step -8 sine is exactly 1.0"
        );
        assert_eq!(
            cos_neg.to_bits(),
            0x250D_3000,
            "step -8 cosine is retail's ~0"
        );

        let (sin_pos, cos_pos) = native_sin_cos_by_step(8).expect("in range");
        assert_eq!(
            sin_pos.to_bits(),
            0xBF7F_FFEC,
            "step +8 sine is NOT exactly -1"
        );
        assert_eq!(
            cos_pos.to_bits(),
            0x3AC9_0FD5,
            "step +8 cosine is NOT exactly 0"
        );

        // The geometry still has to be right within the table's own error.
        let turned = rotate_z_by_step((190.0, 25.0, 120.0), 8).expect("in range");
        assert!((turned.0 - 25.0).abs() < 0.5, "x={}", turned.0);
        assert!((turned.1 + 190.0).abs() < 0.5, "y={}", turned.1);
        assert_eq!(turned.2, 120.0, "Z is carried through untouched");
    }
}
