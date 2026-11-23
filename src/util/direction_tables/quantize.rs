//! Facing↔direction quantization + 8/16-bit facing helpers (pure functions).
//!
//! gamemd `((f>>4)+1)>>1 & 7` (8-bit) / `((f>>12)+1)>>1 & 7` (16-bit) — PROVEN
//! bit-identical to `(f+16)/32 & 7` (study §4.3 / Verification Log #5). The
//! 8-bit form is `util::direction::direction_from_facing`; this module ADDS the
//! 16-bit form + muzzle rotation.

/// gamemd 16-bit facing → 8-direction: `((f>>12)+1)>>1 & 7`. The low byte is
/// irrelevant (only bits ≥12 feed the quantization).
pub fn dir_from_facing16(f: u16) -> u8 {
    ((((f >> 12) + 1) >> 1) & 7) as u8
}

/// gamemd 16-bit facing → nearest 256-way DirType byte: `((f>>7)+1)>>1`,
/// wrapping 0xFF80.. to 0 (e.g. ExitObject 0x00444AC5..AD8).
/// This rounds the 16-bit word; facing8_from_delta exposes its truncated byte.
/// Original execution: factory-exit geometry controls include word 32767→128,
/// 38934→152, 16383→64 and 49153→192, preserving full RNG state.
pub fn round_facing16_to_8(f: u16) -> u8 {
    (((u32::from(f) >> 7) + 1) >> 1) as u8
}

/// gamemd 16-bit facing → 32-way step (5-bit bucket):
/// `(((f>>10)+1)>>1) & 0x1F`. The single implementation of the 32-way
/// quantizer — DRAGON frames, FLH fire origins, and the voxel rasteriser all
/// rotate off this one form.
pub fn step32_from_facing16(f16: u16) -> u8 {
    ((((f16 >> 10) + 1) >> 1) & 0x1F) as u8
}

/// gamemd 8-way muzzle-flash anim index (used when the weapon's anim count == 8):
/// `(dir_from_facing16(f)+1) & 7`. The `+1` rotation is real (study §5).
pub fn muzzle_anim_index_8way(f16: u16) -> u8 {
    (dir_from_facing16(f16) + 1) & 7
}

/// Ares 3.0 facing bucket for a list of `count` images or anims, shared by
/// `UnitClass_DrawSHP_FacingsA` (Ares.dll `0x10047FE0`) and
/// `TechnoClass_Fire_Facings` (Ares.dll `0x10047CF0`). With
/// `n = floor(log2(count))`: `(((f16 + 0x2000) >> (15 - n)) + 1) >> 1`
/// masked to `2^n - 1`; from `n = 16` the sum itself, neither shifted nor
/// masked. A count below 8 gives bucket 0. For eight this is the stock
/// rotated octant, [`muzzle_anim_index_8way`].
pub fn ares_facing_bucket(count: u32, f16: u16) -> u32 {
    let Some(log2) = count.checked_ilog2().filter(|&log2| log2 >= 3) else {
        return 0;
    };
    let raw = u32::from(f16) + 0x2000;
    if log2 >= 16 {
        raw
    } else {
        (((raw >> (15 - log2)) + 1) >> 1) & ((1 << log2) - 1)
    }
}

#[cfg(all(test, tests_util))]
mod tests {
    use super::*;

    #[test]
    fn dir_from_facing16_ignores_low_byte() {
        for hi in 0u8..=255 {
            for lo in [0u8, 1, 127, 255] {
                let f16 = ((hi as u16) << 8) | lo as u16;
                assert_eq!(
                    dir_from_facing16(f16),
                    crate::util::direction::direction_from_facing(hi),
                    "hi={hi} lo={lo}"
                );
            }
        }
    }
}
