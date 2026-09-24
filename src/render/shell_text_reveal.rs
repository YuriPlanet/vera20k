//! Verified integer color arithmetic for shell BITFONT Path A.
//!
//! The native renderer decides visibility and interpolates encoded COLORREF
//! bytes per one-based UTF-16 unit. The existing shell presentation boundary
//! later quantizes those stored sRGB bytes to RGB565 and expands the enrolled
//! channel indices. This module stops before that packing boundary.

/// One opt-in Path-A reveal window for a shell label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathAReveal {
    pub count: u32,
    pub range: u32,
    pub base_rgb: [u8; 3],
    pub highlight_rgb: [u8; 3],
}

/// Encoded RGB for one one-based UTF-16 unit, or `None` when it is still cut.
pub fn encoded_unit_rgb(unit_position: u32, reveal: PathAReveal) -> Option<[u8; 3]> {
    debug_assert!(unit_position > 0);
    if reveal.count != 0 && reveal.count <= unit_position {
        return None;
    }
    if reveal.count == 0 || reveal.range == 0 {
        return Some(reveal.base_rgb);
    }

    let remaining = reveal.count - unit_position - 1;
    if remaining >= reveal.range {
        return Some(reveal.base_rgb);
    }
    let gradient = reveal.range - remaining;
    let coefficient = (255 / reveal.range) * gradient;
    Some(std::array::from_fn(|channel| {
        let base = i32::from(reveal.base_rgb[channel]);
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
        assert_eq!(encoded_unit_rgb(1, YELLOW_TO_WHITE), Some([255, 255, 0]));
        assert_eq!(encoded_unit_rgb(8, YELLOW_TO_WHITE), Some([255, 255, 0]));
        assert_eq!(encoded_unit_rgb(9, YELLOW_TO_WHITE), Some([255, 255, 30]));
        assert_eq!(encoded_unit_rgb(17, YELLOW_TO_WHITE), None);
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
        // 255 + (-255 * 248 / 256) = 8 with signed truncation toward zero.
        assert_eq!(encoded_unit_rgb(1, reveal), Some([8; 3]));
    }

    #[test]
    fn zero_count_is_plain_and_zero_range_has_no_gradient() {
        let plain = PathAReveal {
            count: 0,
            ..YELLOW_TO_WHITE
        };
        assert_eq!(encoded_unit_rgb(99, plain), Some([255, 255, 0]));
        let no_gradient = PathAReveal {
            count: 2,
            range: 0,
            ..YELLOW_TO_WHITE
        };
        assert_eq!(encoded_unit_rgb(1, no_gradient), Some([255, 255, 0]));
    }
}
