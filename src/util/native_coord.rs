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
