//! Native DirectDraw pixel-format semantics shared by compatibility renderers.
//!
//! Gamemd derives component losses and shifts from the active DirectDraw
//! surface descriptor. The known RGB565/RGB555 values here are fixtures for
//! verified branches, while the struct remains runtime-format-shaped.

/// Runtime-derived component layout for one native DirectDraw surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectDrawPixelFormat {
    pub red_loss: u32,
    pub red_shift: u32,
    pub green_loss: u32,
    pub green_shift: u32,
    pub blue_loss: u32,
    pub blue_shift: u32,
    pub destination_bytes_per_pixel: u8,
}

/// The active local DDrawCompat R5G6B5 classifier branch.
pub const RGB565: DirectDrawPixelFormat = DirectDrawPixelFormat {
    red_loss: 3,
    red_shift: 11,
    green_loss: 2,
    green_shift: 5,
    blue_loss: 3,
    blue_shift: 0,
    destination_bytes_per_pixel: 2,
};

/// Gamemd's separately supported X1R5G5B5 classifier branch.
pub const RGB555: DirectDrawPixelFormat = DirectDrawPixelFormat {
    red_loss: 3,
    red_shift: 10,
    green_loss: 3,
    green_shift: 5,
    blue_loss: 3,
    blue_shift: 0,
    destination_bytes_per_pixel: 2,
};

impl DirectDrawPixelFormat {
    /// Pack native component losses/shifts without display expansion.
    pub(crate) fn pack_rgb8(self, rgb: [u8; 3]) -> u16 {
        ((u16::from(rgb[0]) >> self.red_loss) << self.red_shift)
            | ((u16::from(rgb[1]) >> self.green_loss) << self.green_shift)
            | ((u16::from(rgb[2]) >> self.blue_loss) << self.blue_shift)
    }

    /// Native packed-word extraction (House50B840 and WriteSurfaceAsPCX7B05C0).
    /// Channel losses are zero-filled; display expansion is a later operation.
    pub(crate) fn unpack_rgb8(self, word: u16) -> [u8; 3] {
        [
            ((u32::from(word) >> self.red_shift) << self.red_loss) as u8,
            ((u32::from(word) >> self.green_shift) << self.green_loss) as u8,
            ((u32::from(word) >> self.blue_shift) << self.blue_loss) as u8,
        ]
    }
}

/// Expansion codebooks observed in the enrolled native presentation chain.
///
/// These values are tied to the sealed local gamemd/DDrawCompat/AMD capture,
/// not asserted as a universal DirectDraw expansion algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeSurfacePresentationProfile {
    pub format: DirectDrawPixelFormat,
    pub five_bit: [u8; 32],
    pub six_bit: [u8; 64],
}

impl NativeSurfacePresentationProfile {
    /// Quantize encoded RGBA8 through the profile while preserving alpha.
    pub fn quantize_rgba8(self, rgba: [u8; 4]) -> [u8; 4] {
        [
            self.expand(rgba[0], self.format.red_loss),
            self.expand(rgba[1], self.format.green_loss),
            self.expand(rgba[2], self.format.blue_loss),
            rgba[3],
        ]
    }

    /// Apply a transparent RGB key after both colors have been packed into the
    /// active DirectDraw word. Non-key pixels retain their incoming alpha.
    ///
    /// Retail provenance: packed flag-image transparency — `OwnerDraw_Static_006153E0` @ `0x006153E0`.
    pub(crate) fn apply_packed_color_key_rgba8(self, rgba: &mut [u8], transparent_rgb: [u8; 3]) {
        assert_eq!(rgba.len() % 4, 0, "RGBA8 input must contain whole pixels");
        let transparent_word = self.format.pack_rgb8(transparent_rgb);
        for pixel in rgba.chunks_exact_mut(4) {
            if self.format.pack_rgb8([pixel[0], pixel[1], pixel[2]]) == transparent_word {
                pixel[3] = 0;
            }
        }
    }

    /// Recover the component bytes that native emits after a packed-surface
    /// round trip: discarded low bits are zero-filled, not replicated.
    ///
    /// Retail provenance: RandMap.img packed-surface RGB extraction — `WriteSurfaceAsPCX` @ `0x007B05C0`.
    pub(crate) fn storage_roundtrip_rgb8(self, rgb: [u8; 3]) -> [u8; 3] {
        [
            (rgb[0] >> self.format.red_loss) << self.format.red_loss,
            (rgb[1] >> self.format.green_loss) << self.format.green_loss,
            (rgb[2] >> self.format.blue_loss) << self.format.blue_loss,
        ]
    }

    /// Flatten the exact codebooks for the read-only shader buffer.
    pub(crate) fn shader_words(self) -> [u32; 96] {
        let mut words = [0; 96];
        for (destination, source) in words[..32].iter_mut().zip(self.five_bit) {
            *destination = u32::from(source);
        }
        for (destination, source) in words[32..].iter_mut().zip(self.six_bit) {
            *destination = u32::from(source);
        }
        words
    }

    fn expand(self, channel: u8, loss: u32) -> u8 {
        match loss {
            3 => self.five_bit[usize::from(channel >> 3)],
            2 => self.six_bit[usize::from(channel >> 2)],
            _ => panic!("presentation profile supports only five- and six-bit channels"),
        }
    }
}

const OBSERVED_FIVE_BIT: [u8; 32] = [
    0, 8, 16, 25, 33, 41, 49, 58, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140, 148, 156, 164, 173,
    181, 189, 197, 206, 214, 222, 230, 238, 247, 255,
];

const OBSERVED_SIX_BIT: [u8; 64] = [
    0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 45, 49, 53, 57, 61, 65, 69, 73, 77, 81, 85, 89, 93,
    97, 101, 105, 109, 113, 117, 121, 125, 129, 133, 138, 142, 146, 150, 154, 158, 162, 166, 170,
    174, 178, 182, 186, 190, 194, 198, 202, 206, 210, 214, 219, 223, 227, 231, 235, 239, 243, 247,
    251, 255,
];

/// Exact codebook derived from all three sealed active-retail shell sources.
pub const ACTIVE_RETAIL_RGB565_PRESENTATION: NativeSurfacePresentationProfile =
    NativeSurfacePresentationProfile {
        format: RGB565,
        five_bit: OBSERVED_FIVE_BIT,
        six_bit: OBSERVED_SIX_BIT,
    };

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn active_profile_exhaustively_uses_guard_observed_channel_indices() {
        for channel in u8::MIN..=u8::MAX {
            let rgba =
                ACTIVE_RETAIL_RGB565_PRESENTATION.quantize_rgba8([channel, channel, channel, 0xa5]);
            assert_eq!(
                rgba[0],
                OBSERVED_FIVE_BIT[usize::from(channel >> RGB565.red_loss)]
            );
            assert_eq!(
                rgba[1],
                OBSERVED_SIX_BIT[usize::from(channel >> RGB565.green_loss)]
            );
            assert_eq!(
                rgba[2],
                OBSERVED_FIVE_BIT[usize::from(channel >> RGB565.blue_loss)]
            );
            assert_eq!(rgba[3], 0xa5);
        }
    }

    #[test]
    fn rgb555_exhaustively_uses_five_bit_indices_for_every_channel() {
        let profile = NativeSurfacePresentationProfile {
            format: RGB555,
            five_bit: OBSERVED_FIVE_BIT,
            six_bit: OBSERVED_SIX_BIT,
        };
        let expected_values = OBSERVED_FIVE_BIT.into_iter().collect::<BTreeSet<_>>();
        let mut red = BTreeSet::new();
        let mut green = BTreeSet::new();
        let mut blue = BTreeSet::new();

        for channel in u8::MIN..=u8::MAX {
            let rgba = profile.quantize_rgba8([channel, channel, channel, channel]);
            let expected = OBSERVED_FIVE_BIT[usize::from(channel >> 3)];
            assert_eq!(rgba[..3], [expected; 3]);
            assert_eq!(rgba[3], channel);
            red.insert(rgba[0]);
            green.insert(rgba[1]);
            blue.insert(rgba[2]);
        }

        assert_eq!(red, expected_values);
        assert_eq!(green, expected_values);
        assert_eq!(blue, expected_values);
    }

    #[test]
    fn active_profile_preserves_every_alpha_value() {
        for alpha in u8::MIN..=u8::MAX {
            assert_eq!(
                ACTIVE_RETAIL_RGB565_PRESENTATION.quantize_rgba8([17, 101, 249, alpha])[3],
                alpha
            );
        }
    }

    #[test]
    fn rgb565_packed_color_key_accepts_low_bit_aliases_and_preserves_other_alpha() {
        let mut rgba = [
            255, 0, 255, 255, // exact magenta
            248, 3, 249, 127, // same R5G6B5 word
            247, 3, 249, 37, // adjacent red word
        ];

        ACTIVE_RETAIL_RGB565_PRESENTATION.apply_packed_color_key_rgba8(&mut rgba, [255, 0, 255]);

        assert_eq!(&rgba[0..4], &[255, 0, 255, 0]);
        assert_eq!(&rgba[4..8], &[248, 3, 249, 0]);
        assert_eq!(&rgba[8..12], &[247, 3, 249, 37]);
    }

    #[test]
    fn rgb565_storage_roundtrip_zero_fills_discarded_channel_bits() {
        assert_eq!(
            ACTIVE_RETAIL_RGB565_PRESENTATION.storage_roundtrip_rgb8([17, 101, 249]),
            [16, 100, 248]
        );
    }

    #[test]
    fn active_profile_has_exact_guard_cardinalities() {
        let red = (u8::MIN..=u8::MAX)
            .map(|channel| {
                ACTIVE_RETAIL_RGB565_PRESENTATION.quantize_rgba8([channel, 0, 0, 255])[0]
            })
            .collect::<BTreeSet<_>>();
        let green = (u8::MIN..=u8::MAX)
            .map(|channel| {
                ACTIVE_RETAIL_RGB565_PRESENTATION.quantize_rgba8([0, channel, 0, 255])[1]
            })
            .collect::<BTreeSet<_>>();
        let blue = (u8::MIN..=u8::MAX)
            .map(|channel| {
                ACTIVE_RETAIL_RGB565_PRESENTATION.quantize_rgba8([0, 0, channel, 255])[2]
            })
            .collect::<BTreeSet<_>>();

        assert_eq!(red.len(), 32);
        assert_eq!(green.len(), 64);
        assert_eq!(blue.len(), 32);
        assert_eq!(red, blue);
        assert_eq!(red.into_iter().collect::<Vec<_>>(), OBSERVED_FIVE_BIT);
        assert_eq!(green.into_iter().collect::<Vec<_>>(), OBSERVED_SIX_BIT);
    }

    #[test]
    fn shader_words_preserve_profile_order_exactly() {
        let words = ACTIVE_RETAIL_RGB565_PRESENTATION.shader_words();
        assert_eq!(&words[..32], OBSERVED_FIVE_BIT.map(u32::from).as_slice());
        assert_eq!(&words[32..], OBSERVED_SIX_BIT.map(u32::from).as_slice());
    }
}
