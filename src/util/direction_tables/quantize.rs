//! Facing↔direction quantization + 8/16-bit facing helpers (pure functions).
//!
//! gamemd `((f>>4)+1)>>1 & 7` (8-bit) / `((f>>12)+1)>>1 & 7` (16-bit) — PROVEN
//! bit-identical to `(f+16)/32 & 7` (study §4.3 / Verification Log #5). The
//! 8-bit form already lives in util; this is the sim-facing entry point that
//! ADDS the 16-bit form + muzzle rotation.

/// gamemd 8-bit facing → 8-direction: `((f>>4)+1)>>1 & 7`.
pub fn dir_from_facing8(f: u8) -> u8 {
    crate::util::direction::direction_from_facing(f)
}

/// gamemd 16-bit facing → 8-direction: `((f>>12)+1)>>1 & 7`. The low byte is
/// irrelevant (only bits ≥12 feed the quantization).
pub fn dir_from_facing16(f: u16) -> u8 {
    ((((f >> 12) + 1) >> 1) & 7) as u8
}

/// Native nearest256-way direction, e.g. ExitObject444AC5..AD8.
/// This rounds the16-bit word; facing8_from_delta exposes its truncated byte.
/// Original execution: factory-exit geometry controls include word32767→128,
/// 38934→152,16383→64 and49153→192, preserving full RNG state.
pub(crate) fn round_facing16_to_8(f: u16) -> u8 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_from_facing8_full_input_space() {
        for f in 0u8..=255 {
            let gamemd = ((((f as u16) >> 4) + 1) >> 1) as u8 & 7;
            assert_eq!(dir_from_facing8(f), gamemd, "f={f}");
            assert_eq!(dir_from_facing8(f), (f.wrapping_add(16) / 32) & 7, "f={f}");
        }
        // Boundaries (study S3): rounds UP at 16+32n; 240..255 wrap to N.
        assert_eq!(dir_from_facing8(15), 0);
        assert_eq!(dir_from_facing8(16), 1);
        assert_eq!(dir_from_facing8(240), 0);
        assert_eq!(dir_from_facing8(255), 0);
    }

    #[test]
    fn dir_from_facing16_ignores_low_byte() {
        for hi in 0u8..=255 {
            for lo in [0u8, 1, 127, 255] {
                let f16 = ((hi as u16) << 8) | lo as u16;
                assert_eq!(
                    dir_from_facing16(f16),
                    dir_from_facing8(hi),
                    "hi={hi} lo={lo}"
                );
            }
        }
    }

    #[test]
    fn muzzle_anim_8way_plus1_rotation() {
        // f=0 → bucket 0 → anim 1 (the +1 rotation).
        assert_eq!(muzzle_anim_index_8way(0x0000), 1);
        for f16 in [0u16, 0x2000, 0x4000, 0x8000, 0xE000] {
            assert_eq!(
                muzzle_anim_index_8way(f16),
                (dir_from_facing16(f16) + 1) & 7
            );
        }
    }
}
