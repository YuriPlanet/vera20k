//! Shared native terrain-slope matrices and interpolation.
//!
//! `7559B0` copies the startup matrix at `B45188 + slope*48`; the exact
//! original initialization is executed by `tools/native_slope.py` and
//! consumed in `tools/projectile_oracle/ordinary_collision_vectors.json`.
//! Slopes17..20 are explicitly identity (original755852..755875).

use crate::util::native_x87::{
    NativeF32Bits, NativeF64Bits, NativeX87Error, X87Chop53, X87Ordering,
};

macro_rules! matrix {
    ($($bits:expr),+ $(,)?) => {
        [$(NativeF32Bits::from_bits($bits)),+]
    };
}

const SLOPE_MATRICES: [[NativeF32Bits; 12]; 21] = [
    matrix![
        0x3f800000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3f800000, 0x00000000,
        0x00000000, 0x00000000, 0x00000000, 0x3f800000, 0x00000000
    ],
    matrix![
        0x3f5f3968, 0xbaaf51ee, 0xbefaa753, 0x00000000, 0x3ac90fd4, 0x3f7fffec, 0x250a3d28,
        0x00000000, 0x3efaa73f, 0xba44dce0, 0x3f5f397a, 0x00000000
    ],
    matrix![
        0x3f7fffec, 0xbac90fd4, 0x248a3d28, 0x00000000, 0x3aaf51ee, 0x3f5f3968, 0x3efaa753,
        0x00000000, 0xba44dce0, 0xbefaa73f, 0x3f5f397a, 0x00000000
    ],
    matrix![
        0x3f5f3968, 0xbaaf51ee, 0x3efaa753, 0x00000000, 0x3ac90fd4, 0x3f7fffec, 0xa48a3d28,
        0x00000000, 0xbefaa73f, 0x3a44dce0, 0x3f5f397a, 0x00000000
    ],
    matrix![
        0x3f800000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3f5f397a, 0xbefaa753,
        0x00000000, 0x00000000, 0x3efaa753, 0x3f5f397a, 0x00000000
    ],
    matrix![
        0x3f773916, 0x3d06934a, 0xbe83d955, 0x00000000, 0x3d12b5d0, 0x3f77322f, 0x3e83d955,
        0x00000000, 0x3e83a584, 0xbe840d12, 0x3f6e6b6d, 0x00000000
    ],
    matrix![
        0x3f77322f, 0xbd12b5d0, 0x3e83d955, 0x00000000, 0xbd06934a, 0x3f773916, 0x3e83d955,
        0x00000000, 0xbe840d12, 0xbe83a584, 0x3f6e6b6d, 0x00000000
    ],
    matrix![
        0x3f773916, 0x3d06934a, 0x3e83d955, 0x00000000, 0x3d12b5d0, 0x3f77322f, 0xbe83d955,
        0x00000000, 0xbe83a584, 0x3e840d12, 0x3f6e6b6d, 0x00000000
    ],
    matrix![
        0x3f77322f, 0xbd12b5d0, 0xbe83d955, 0x00000000, 0xbd06934a, 0x3f773916, 0xbe83d955,
        0x00000000, 0x3e840d12, 0x3e83a584, 0x3f6e6b6d, 0x00000000
    ],
    matrix![
        0x3f773916, 0x3d06934a, 0xbe83d955, 0x00000000, 0x3d12b5d0, 0x3f77322f, 0x3e83d955,
        0x00000000, 0x3e83a584, 0xbe840d12, 0x3f6e6b6d, 0x00000000
    ],
    matrix![
        0x3f77322f, 0xbd12b5d0, 0x3e83d955, 0x00000000, 0xbd06934a, 0x3f773916, 0x3e83d955,
        0x00000000, 0xbe840d12, 0xbe83a584, 0x3f6e6b6d, 0x00000000
    ],
    matrix![
        0x3f773916, 0x3d06934a, 0x3e83d955, 0x00000000, 0x3d12b5d0, 0x3f77322f, 0xbe83d955,
        0x00000000, 0xbe83a584, 0x3e840d12, 0x3f6e6b6d, 0x00000000
    ],
    matrix![
        0x3f77322f, 0xbd12b5d0, 0xbe83d955, 0x00000000, 0xbd06934a, 0x3f773916, 0xbe83d955,
        0x00000000, 0x3e840d12, 0x3e83a584, 0x3f6e6b6d, 0x00000000
    ],
    matrix![
        0x3f6fa319, 0x3d80294a, 0xbeb13d26, 0x00000000, 0x3d860ad0, 0x3f6f963a, 0x3eb13d26,
        0x00000000, 0x3eb0f77e, 0xbeb182b2, 0x3f5f397a, 0x00000000
    ],
    matrix![
        0x3f6f963a, 0xbd860ad0, 0x3eb13d26, 0x00000000, 0xbd80294a, 0x3f6fa319, 0x3eb13d26,
        0x00000000, 0xbeb182b2, 0xbeb0f77e, 0x3f5f397a, 0x00000000
    ],
    matrix![
        0x3f6fa319, 0x3d80294a, 0x3eb13d26, 0x00000000, 0x3d860ad0, 0x3f6f963a, 0xbeb13d26,
        0x00000000, 0xbeb0f77e, 0x3eb182b2, 0x3f5f397a, 0x00000000
    ],
    matrix![
        0x3f6f963a, 0xbd860ad0, 0xbeb13d26, 0x00000000, 0xbd80294a, 0x3f6fa319, 0xbeb13d26,
        0x00000000, 0x3eb182b2, 0x3eb0f77e, 0x3f5f397a, 0x00000000
    ],
    matrix![
        0x3f800000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3f800000, 0x00000000,
        0x00000000, 0x00000000, 0x00000000, 0x3f800000, 0x00000000
    ],
    matrix![
        0x3f800000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3f800000, 0x00000000,
        0x00000000, 0x00000000, 0x00000000, 0x3f800000, 0x00000000
    ],
    matrix![
        0x3f800000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3f800000, 0x00000000,
        0x00000000, 0x00000000, 0x00000000, 0x3f800000, 0x00000000
    ],
    matrix![
        0x3f800000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3f800000, 0x00000000,
        0x00000000, 0x00000000, 0x00000000, 0x3f800000, 0x00000000
    ],
];

// Original initialized quaternions atB43188; native_slope.py --blend.
const SLOPE_QUATERNIONS: [[NativeF32Bits; 4]; 21] = [
    matrix![0x00000000, 0x00000000, 0x00000000, 0x3f800000],
    matrix![0x00000000, 0xbe81bb4a, 0x00000000, 0x3f77a551],
    matrix![0xbe81bb4a, 0x00000000, 0x00000000, 0x3f77a551],
    matrix![0x00000000, 0x3e81bb4a, 0x00000000, 0x3f77a551],
    matrix![0x3e81bb4a, 0x00000000, 0x00000000, 0x3f77a551],
    matrix![0xbe062d3c, 0xbe062d3c, 0x00000000, 0x3f7b9107],
    matrix![0xbe062d3c, 0x3e062d3c, 0x00000000, 0x3f7b9107],
    matrix![0x3e062d3c, 0x3e062d3c, 0x00000000, 0x3f7b9107],
    matrix![0x3e062d3c, 0xbe062d3c, 0x00000000, 0x3f7b9107],
    matrix![0xbe062d3c, 0xbe062d3c, 0x00000000, 0x3f7b9107],
    matrix![0xbe062d3c, 0x3e062d3c, 0x00000000, 0x3f7b9107],
    matrix![0x3e062d3c, 0x3e062d3c, 0x00000000, 0x3f7b9107],
    matrix![0x3e062d3c, 0xbe062d3c, 0x00000000, 0x3f7b9107],
    matrix![0xbe377949, 0xbe377949, 0x00000000, 0x3f77a551],
    matrix![0xbe377949, 0x3e377949, 0x00000000, 0x3f77a551],
    matrix![0x3e377949, 0x3e377949, 0x00000000, 0x3f77a551],
    matrix![0x3e377949, 0xbe377949, 0x00000000, 0x3f77a551],
    matrix![0x00000000, 0x00000000, 0x00000000, 0x3f800000],
    matrix![0x00000000, 0x00000000, 0x00000000, 0x3f800000],
    matrix![0x00000000, 0x00000000, 0x00000000, 0x3f800000],
    matrix![0x00000000, 0x00000000, 0x00000000, 0x3f800000],
];

/// `7559B0`: select one of the 21 initialized terrain matrices.
pub(crate) fn slope_matrix(slope: u8) -> Option<[NativeF32Bits; 12]> {
    SLOPE_MATRICES.get(slope as usize).copied()
}

/// Original `755A40`, including the double-to-float phase store and its
/// `Quaternion_Slerp646590` → shared `Quaternion_ToMatrix646980` call chain.
/// Unlike a normalized/shortest-path SLERP, this uses the initialized original
/// quaternions unchanged and does not normalize the result. Native controls:
/// `tools/native_slope.py --blend` / `tools/native_slope_blend.json`.
pub(crate) fn blend_matrix(
    from: u8,
    to: u8,
    phase: NativeF64Bits,
) -> Result<[NativeF32Bits; 12], NativeX87Error> {
    if from == to {
        return Ok(slope_matrix(from).expect("initialized terrain slope"));
    }
    let phase = X87Chop53::store_f32(X87Chop53::load_f64(phase)?)?;
    let quaternion = quaternion_slerp(
        SLOPE_QUATERNIONS[usize::from(from)],
        SLOPE_QUATERNIONS[usize::from(to)],
        phase,
    )?;
    crate::util::native_quaternion::quaternion_to_matrix(quaternion)
}

/// Original646590; the table lookups, sum order, and explicit memory stores
/// determine interpolation on every locomotor that calls755A40.
fn quaternion_slerp(
    from: [NativeF32Bits; 4],
    to: [NativeF32Bits; 4],
    phase: NativeF32Bits,
) -> Result<[NativeF32Bits; 4], NativeX87Error> {
    let mut a = [X87Chop53::load_i32(0); 4];
    let mut b = a;
    for i in 0..4 {
        a[i] = X87Chop53::load_f32(from[i])?;
        b[i] = X87Chop53::load_f32(to[i])?;
    }
    let add = X87Chop53::add;
    let sub = X87Chop53::sub;
    let mul = X87Chop53::mul;
    let double_store = |v| X87Chop53::load_f64(X87Chop53::store_f64(v)?);
    let dot = double_store(add(
        add(add(mul(a[3], b[3]), mul(a[2], b[2])), mul(a[1], b[1])),
        mul(a[0], b[0]),
    ))?;
    let one = X87Chop53::load_i32(1);
    let epsilon = X87Chop53::load_f64(NativeF64Bits::from_bits(0x3ee4_f8b5_88e3_68f1))?; //7E3810
    let t = X87Chop53::load_f32(phase)?;
    let inv_t = sub(one, t);
    let trig = super::retail_trig::TrigTable::embedded();
    let (weight_a, weight_b) = if X87Chop53::compare(add(dot, one), epsilon) == X87Ordering::Greater
    {
        if X87Chop53::compare(sub(one, dot), epsilon) == X87Ordering::Greater {
            let (_, theta) = super::retail_trig::AcosTable::retail().acos_lookup(dot);
            let theta = double_store(theta)?;
            let sin_theta = double_store(trig.sin_from_table(theta))?;
            let weight_a = double_store(X87Chop53::div(
                trig.sin_from_table(double_store(mul(inv_t, theta))?),
                sin_theta,
            )?)?;
            let weight_b =
                X87Chop53::div(trig.sin_from_table(double_store(mul(t, theta))?), sin_theta)?;
            (weight_a, weight_b)
        } else {
            (double_store(inv_t)?, t)
        }
    } else {
        //7F0288 contains this literal, slightly below the usual pi/2 constant.
        let angle = X87Chop53::load_f64(NativeF64Bits::from_bits(0x3ff9_21fb_5444_2d11))?;
        (
            double_store(trig.sin_from_table(double_store(mul(inv_t, angle))?))?,
            trig.sin_from_table(double_store(mul(t, angle))?),
        )
    };
    let mut result = [NativeF32Bits::POSITIVE_ZERO; 4];
    for i in 0..4 {
        result[i] = X87Chop53::store_f32(add(mul(b[i], weight_b), mul(a[i], weight_a)))?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_startup_tables_and_interpolation_match_native_execution() {
        let golden: serde_json::Value =
            serde_json::from_str(crate::test_fixture::text("tools/native_slope_blend.json"))
                .unwrap();
        for (slope, row) in golden["matrices"].as_array().unwrap().iter().enumerate() {
            let expected: [u32; 12] = std::array::from_fn(|i| row[i].as_u64().unwrap() as u32);
            assert_eq!(
                slope_matrix(slope as u8).unwrap().map(NativeF32Bits::bits),
                expected,
                "slope {slope}"
            );
        }
        for (slope, row) in golden["quaternions"].as_array().unwrap().iter().enumerate() {
            let expected: [u32; 4] = std::array::from_fn(|i| row[i].as_u64().unwrap() as u32);
            assert_eq!(
                SLOPE_QUATERNIONS[slope].map(NativeF32Bits::bits),
                expected,
                "quaternion {slope}"
            );
        }
        assert_eq!(golden["cases"].as_array().unwrap().len(), 7);
        for row in golden["cases"].as_array().unwrap() {
            let input = &row["input"];
            let actual = blend_matrix(
                input["from_slope"].as_u64().unwrap() as u8,
                input["to_slope"].as_u64().unwrap() as u8,
                NativeF64Bits::from_bits(input["phase_bits"].as_u64().unwrap()),
            )
            .unwrap();
            let expected: [u32; 12] =
                std::array::from_fn(|i| row["matrix_bits"][i].as_u64().unwrap() as u32);
            assert_eq!(actual.map(NativeF32Bits::bits), expected, "{input}");
        }
    }
}
