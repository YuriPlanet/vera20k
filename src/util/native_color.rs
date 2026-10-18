//! Encoded RGB arithmetic shared by native presentation and particle callers.
//! Original661190 and661020 use x87 before clamping and truncating bytes.
//! Their float arguments are already narrowed by the caller. Native controls:
//! tools/spatial_oracle/tmp_preview.json (binary identity in its sidecar).

use super::native_x87::{NativeF32Bits, NativeX87Error, X87Chop53, X87Ordering, X87Value};

fn channel_byte(value: X87Value) -> Result<u8, NativeX87Error> {
    let zero = X87Chop53::load_i32(0);
    let upper = X87Chop53::load_i32(255);
    let value = if X87Chop53::compare(value, zero) == X87Ordering::Less {
        zero
    } else if X87Chop53::compare(value, upper) == X87Ordering::Greater {
        upper
    } else {
        value
    };
    Ok(X87Chop53::ftol_i64(value)? as u8)
}

/// `RGBClass @661190`: multiply encoded channel bytes by the supplied float,
/// clamp to0..255, then Math_ftol. A host f32 product can round up before ftol.
pub fn scale_rgb8(rgb: [u8; 3], factor: NativeF32Bits) -> Result<[u8; 3], NativeX87Error> {
    let factor = X87Chop53::load_f32(factor)?;
    let mut result = [0; 3];
    for (output, source) in result.iter_mut().zip(rgb) {
        *output = channel_byte(X87Chop53::mul(
            X87Chop53::load_i32(i32::from(source)),
            factor,
        ))?;
    }
    Ok(result)
}

/// `RGBClass @661020`: `(1-t)*source + t*target`, followed by channel clamp
/// and Math_ftol. Preserve the two weighted products rather than rearranging
/// to `source+(target-source)*t`; both operations and caller float stores matter.
pub fn interpolate_rgb8(
    source: [u8; 3],
    target: [u8; 3],
    fraction: NativeF32Bits,
) -> Result<[u8; 3], NativeX87Error> {
    let fraction = X87Chop53::load_f32(fraction)?;
    let remaining = X87Chop53::sub(X87Chop53::load_i32(1), fraction);
    let mut result = [0; 3];
    for channel in 0..3 {
        result[channel] = channel_byte(X87Chop53::add(
            X87Chop53::mul(X87Chop53::load_i32(i32::from(source[channel])), remaining),
            X87Chop53::mul(X87Chop53::load_i32(i32::from(target[channel])), fraction),
        ))?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn rgb(value: &Value) -> [u8; 3] {
        std::array::from_fn(|i| value[i].as_u64().unwrap() as u8)
    }

    fn float(value: &Value) -> NativeF32Bits {
        NativeF32Bits::from_bits(u32::from_str_radix(value.as_str().unwrap(), 16).unwrap())
    }

    #[test]
    fn scaling_matches_original_radar_preview_and_clamp_controls() {
        let native = crate::test_fixture::json("tools/spatial_oracle/tmp_preview.json");
        for row in native["scale_controls"].as_array().unwrap() {
            assert_eq!(
                scale_rgb8(
                    rgb(&row["input"]["rgb"]),
                    float(&row["input"]["factor_f32_bits"])
                )
                .unwrap(),
                rgb(&row["output_rgb"]),
                "{row}"
            );
        }
    }

    #[test]
    fn interpolation_matches_original_weighted_and_particle_prefix_controls() {
        let native = crate::test_fixture::json("tools/spatial_oracle/tmp_preview.json");
        for row in native["interpolation_controls"].as_array().unwrap() {
            assert_eq!(
                interpolate_rgb8(
                    rgb(&row["input"]["left_rgb"]),
                    rgb(&row["input"]["right_rgb"]),
                    float(&row["input"]["fraction_f32_bits"]),
                )
                .unwrap(),
                rgb(&row["output_rgb"]),
                "{row}"
            );
        }
        for row in native["particle_color_controls"].as_array().unwrap() {
            // These fractions are original62BAC5..62BB06 outputs, not Rust
            // approximations of the sample conversion or RNG results.
            assert_eq!(
                interpolate_rgb8(
                    rgb(&row["input"]["start_color1"]),
                    rgb(&row["input"]["start_color2"]),
                    float(&row["output"]["fraction_f32_bits"]),
                )
                .unwrap(),
                rgb(&row["output"]["start_rgb"]),
                "{row}"
            );
        }
    }
}
