//! Shared signed-coordinate interpolation used by native gameplay consumers.

use crate::util::native_x87::{X87Chop53 as Fpu, X87Value};

/// `CoordStruct::Lerp75F540` and the matching inline Wave762AF0/7610F0
/// sequence: `ftol((1-t)*first + t*second)` for each coordinate.
/// The caller preserves its native argument store:75F540 receives binary32,
/// while the inline Wave paths retain a double. Intermediates use the shared
/// deterministic53-bit/chop FPU owner and conversion keeps the low dword.
/// Consumed native controls: procedural_drawing_oracle/magnetic_wave.json and
/// spatial_oracle/locomotor_track_residual.json.
pub(crate) fn interpolate<const N: usize>(
    first: [i32; N],
    second: [i32; N],
    t: X87Value,
) -> [i32; N] {
    let other = Fpu::sub(Fpu::load_i32(1), t);
    std::array::from_fn(|axis| {
        Fpu::ftol_i64(Fpu::add(
            Fpu::mul(Fpu::load_i32(first[axis]), other),
            Fpu::mul(Fpu::load_i32(second[axis]), t),
        ))
        .expect("finite signed-coordinate interpolation fits i64") as i32
    })
}

/// `ObjectClass::DistanceSquaredXY_To_Object @ 0x005F6500` and its
/// coordinate twin `0x005F6560`: `dx² + dy²` of two XY lepton points, Z
/// ignored, each difference a 32-bit SUB and the squares and sum 32-bit
/// IMUL/ADD that wrap. Ares 3.0 AbstractClass_Distance2DSquared_1/2
/// (1005C1A0, hook 0x5F6515; 1005C240, hook 0x5F6560) square the same
/// 32-bit differences in 64 bits and clamp the sum to `i32::MAX`, when Ares
/// plays (`ares`, [`RuleSet::ares`]).
///
/// [`RuleSet::ares`]: crate::rules::ruleset::RuleSet::ares
pub(crate) fn squared_xy_distance(from: [i32; 2], to: [i32; 2], ares: bool) -> i32 {
    let dx = from[0].wrapping_sub(to[0]);
    let dy = from[1].wrapping_sub(to[1]);
    if ares {
        let (dx, dy) = (i64::from(dx), i64::from(dy));
        i32::try_from(dy * dy + dx * dx).unwrap_or(i32::MAX)
    } else {
        dy.wrapping_mul(dy).wrapping_add(dx.wrapping_mul(dx))
    }
}
