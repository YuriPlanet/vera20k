//! Original SpotlightInitialize5FF5AA..5FF727's surfaces64..73. BuildingLight
//! supplies stages80..89; the shared5FF850 owner resolves the stage table and
//! draws these through the existing packed RGB565 compositor.
use crate::util::native_x87::{NativeF64Bits, X87Chop53 as X};
pub const TYPE16_MASK_WIDTH: usize = 256;
pub const TYPE16_MASK_HEIGHT: usize = 128;
pub const TYPE16_MASK_COUNT: usize = 10;
const PEAKS: [u8; 10] = [128, 122, 116, 110, 104, 98, 92, 86, 80, 74];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Type16MaskDescriptor {
    pub mask_index: u8,
    pub peak: u8,
}
pub fn type16_mask_descriptor(stage: u8) -> Option<Type16MaskDescriptor> {
    let i = stage.checked_sub(80)?;
    let peak = *PEAKS.get(usize::from(i))?;
    Some(Type16MaskDescriptor {
        mask_index: 64 + i,
        peak,
    })
}
/// 5FF675..5FF69B: wrapping signed Rules radius*60, double multiplier,
/// Math::ftol, then subtraction from zero under the ambient chop mode.
pub fn type16_circle_radius(radius: i32) -> i32 {
    let scale = X::load_f64(NativeF64Bits::from_bits(0xbf66db6db6db6db7)).unwrap();
    X::ftol_i32_low_masked(X::mul(X::load_i32(radius.wrapping_mul(60)), scale)).wrapping_neg()
}
pub fn generate_type16_mask(radius: i32, stage: u8) -> Option<Vec<u8>> {
    let descriptor = type16_mask_descriptor(stage)?;
    // The loop ordinal at+14 widens each successive disk by one pixel.
    let r = type16_circle_radius(radius).wrapping_add(i32::from(descriptor.mask_index - 64));
    let mut scratch = vec![0; 256 * 256];
    if r >= 0 {
        crate::render::combat_light::fill_midpoint_circle(&mut scratch, r, descriptor.peak);
    }
    let mut mask = vec![0; 256 * 128];
    for y in 0..128 {
        mask[y * 256..(y + 1) * 256].copy_from_slice(&scratch[y * 2 * 256..(y * 2 + 1) * 256]);
    }
    Some(mask)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spotlight_masks_match_all_twenty_original_surface_controls() {
        let fixture = crate::test_fixture::json("tools/procedural_drawing_oracle/spotlight.json");
        for bank in fixture["banks"].as_array().unwrap() {
            let radius = bank["radius"].as_i64().unwrap() as i32;
            for row in bank["masks"].as_array().unwrap() {
                let index = row["index"].as_u64().unwrap() as u8;
                let mask = generate_type16_mask(radius, index + 16).unwrap();
                let hash = mask.iter().fold(0xcbf29ce484222325_u64, |h, &b| {
                    (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
                });
                assert_eq!(
                    format!("{hash:016x}"),
                    row["fnv64"].as_str().unwrap(),
                    "radius{radius} surface{index}"
                );
                assert_eq!(
                    mask.iter().filter(|&&b| b != 0).count() as u64,
                    row["nonzero"].as_u64().unwrap()
                );
                assert_eq!(
                    u64::from(*mask.iter().max().unwrap()),
                    row["peak"].as_u64().unwrap()
                );
            }
        }
        assert_eq!(generate_type16_mask(175, 79), None);
        assert_eq!(generate_type16_mask(175, 90), None);
    }
}
