//! Shared match inputs for animation pixel offsets. These are gameplay inputs,
//! never the local window size: Building/Tile animations can cause area damage.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PixelConversionBounds {
    pub width: i32,
    pub height: i32,
}

impl Default for PixelConversionBounds {
    fn default() -> Self {
        // Compatibility profile: original 640x480 display minus the 168x32
        // sidebar/bottom margins (72AD20). This explicit match default does not
        // change when a client resizes. Custom profiles travel in the session.
        Self {
            width: 472,
            height: 448,
        }
    }
}

impl PixelConversionBounds {
    /// Tactical6D2360's signed upper-bound guard; negative offsets are legal.
    /// The fixed matrix uses the exact rational value of native f32 4.2667.
    /// We intentionally omit native intermediate f32 rounding: large offsets
    /// can differ by leptons, consistently on every client. Final low32 wrapping
    /// and truncation toward zero are explicit, including i32 extremes.
    pub fn offset_to_leptons(self, x: i32, y: i32) -> (i32, i32) {
        if x >= self.width || y >= self.height {
            return (0, 0);
        }
        Self::isometric_pixel_to_leptons(x, y)
    }

    /// `TacticalClass::IsometricPixelToWorld @ 0x006D2070`: the same matrix
    /// (`Tactical+0xDE4`, first element `0x408888CE` = 4.2667) as
    /// `PixelOffsetToLeptons @ 0x006D2360`, with no bounds guard. Shares that
    /// function's documented rational-arithmetic policy.
    pub fn isometric_pixel_to_leptons(x: i32, y: i32) -> (i32, i32) {
        let scale = |k: i64| ((4_473_959 * k) / 1_048_576) as i32;
        let (x, y) = (i64::from(x), i64::from(y));
        (scale(x + 2 * y), scale(2 * y - x))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_are_signed_and_do_not_clamp_negative_offsets() {
        let bounds = PixelConversionBounds {
            width: 640,
            height: 480,
        };
        assert_eq!(bounds.offset_to_leptons(30, 15), (256, 0));
        assert_ne!(bounds.offset_to_leptons(639, 479), (0, 0));
        assert_eq!(bounds.offset_to_leptons(640, 0), (0, 0));
        assert_eq!(bounds.offset_to_leptons(0, 480), (0, 0));
        // Documented deterministic policy differs from native f32 rounding.
        assert_eq!(
            bounds.offset_to_leptons(-16_777_217, -16_777_219),
            (-214_750_061, -71_583_365)
        );
        // i64 intermediates cover every admitted signed32 input.
        let _ = bounds.offset_to_leptons(i32::MIN, i32::MIN);
    }
}
