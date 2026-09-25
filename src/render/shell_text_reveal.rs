//! Verified integer color arithmetic for shell BITFONT Path A.
//!
//! The native renderer decides visibility and interpolates colour bytes per
//! one-based UTF-16 unit. Its base is the font's 16-bit text colour: the shell
//! print `0x00621040` truncates the static's COLORREF to R5G6B5 units, and the
//! blend (`0x00434DED..0x00434E2E`) shifts each unit back to a byte with the
//! low bits clear before mixing in the highlight. The existing shell
//! presentation boundary later quantizes the stored bytes to RGB565 and expands
//! the enrolled channel indices. This module stops before that packing boundary.

/// One opt-in Path-A reveal window for a shell label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathAReveal {
    pub count: u32,
    pub range: u32,
    pub base_rgb: [u8; 3],
    pub highlight_rgb: [u8; 3],
}

/// The base colour as the blend sees it: its R5G6B5 units with the low bits
/// clear.
fn surface_base(rgb: [u8; 3]) -> [u8; 3] {
    [rgb[0] & 0xF8, rgb[1] & 0xFC, rgb[2] & 0xF8]
}

/// Encoded RGB for one one-based UTF-16 unit, or `None` when it is still cut.
pub fn encoded_unit_rgb(unit_position: u32, reveal: PathAReveal) -> Option<[u8; 3]> {
    debug_assert!(unit_position > 0);
    if reveal.count != 0 && reveal.count <= unit_position {
        return None;
    }
    let base_rgb = surface_base(reveal.base_rgb);
    if reveal.count == 0 || reveal.range == 0 {
        return Some(base_rgb);
    }

    let remaining = reveal.count - unit_position - 1;
    if remaining >= reveal.range {
        return Some(base_rgb);
    }
    let gradient = reveal.range - remaining;
    let coefficient = (255 / reveal.range) * gradient;
    Some(std::array::from_fn(|channel| {
        let base = i32::from(base_rgb[channel]);
        let highlight = i32::from(reveal.highlight_rgb[channel]);
        let interpolated = base + (highlight - base) * coefficient as i32 / 256;
        debug_assert!((0..=255).contains(&interpolated));
        interpolated as u8
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const YELLOW_TO_WHITE: PathAReveal = PathAReveal {
        count: 17,
        range: 8,
        base_rgb: [255, 255, 0],
        highlight_rgb: [255, 255, 255],
    };

    #[test]
    fn main_menu_terminal_unit_uses_verified_encoded_vector() {
        // Yellow is (31, 63, 0) in units: the blend starts from (248, 252, 0).
        assert_eq!(encoded_unit_rgb(1, YELLOW_TO_WHITE), Some([248, 252, 0]));
        assert_eq!(encoded_unit_rgb(8, YELLOW_TO_WHITE), Some([248, 252, 0]));
        assert_eq!(encoded_unit_rgb(9, YELLOW_TO_WHITE), Some([248, 252, 30]));
        assert_eq!(encoded_unit_rgb(17, YELLOW_TO_WHITE), None);
    }

    #[test]
    fn the_blend_starts_from_the_sixteen_bit_text_colour() {
        // Retail still: "[New Player]" (DarkBlue, COLORREF (34, 105, 212)) at
        // its last reveal paint shows its final unit in (4, 27, 26). Blending
        // from the COLORREF bytes gives red unit 5.
        let reveal = PathAReveal {
            count: 44,
            range: 32,
            base_rgb: [34, 105, 212],
            highlight_rgb: [255; 3],
        };
        let rgb = encoded_unit_rgb(12, reveal).unwrap();
        assert_eq!([rgb[0] >> 3, rgb[1] >> 2, rgb[2] >> 3], [4, 27, 26]);
        assert_eq!(encoded_unit_rgb(11, reveal), Some([32, 104, 208]));
    }

    #[test]
    fn count_one_is_a_blank_first_paint() {
        let reveal = PathAReveal {
            count: 1,
            ..YELLOW_TO_WHITE
        };
        assert_eq!(encoded_unit_rgb(1, reveal), None);
    }

    #[test]
    fn signed_channel_division_truncates_toward_zero() {
        let reveal = PathAReveal {
            count: 2,
            range: 8,
            base_rgb: [255; 3],
            highlight_rgb: [0; 3],
        };
        // White is (248, 252, 248) at 16 bits; 248 + (-248 * 248 / 256) = 8
        // and 252 + (-252 * 248 / 256) = 8 with signed truncation toward zero.
        assert_eq!(encoded_unit_rgb(1, reveal), Some([8, 8, 8]));
    }

    #[test]
    fn zero_count_is_plain_and_zero_range_has_no_gradient() {
        let plain = PathAReveal {
            count: 0,
            ..YELLOW_TO_WHITE
        };
        assert_eq!(encoded_unit_rgb(99, plain), Some([248, 252, 0]));
        let no_gradient = PathAReveal {
            count: 2,
            range: 0,
            ..YELLOW_TO_WHITE
        };
        assert_eq!(encoded_unit_rgb(1, no_gradient), Some([248, 252, 0]));
    }
}
