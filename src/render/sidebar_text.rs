//! Sidebar text: the side's text colour as a draw tint and as the B0FA1C
//! users draw it, and the credits counter's depth and words (placed by
//! `app::presentation::sidebar_text`).

/// A side text colour as a draw tint.
pub fn side_text_tint(rgb: [u8; 3]) -> [f32; 3] {
    rgb.map(|channel| f32::from(channel) / 255.0)
}

/// Side text colour `rgb` as the B0FA1C users draw it: the packed
/// `RadarClass+0x1208` color expanded back to the presentation frame.
/// StripClass::Draw packs the same B0FA1C colour for its labels
/// (6A9BF3..6A9C46) and its hover outline (6A9A4F).
///
/// `SetSidebarTextColor @ 0x0072F440` selects the exact RGB;
/// `RadarClass::Init_For_House @ 0x00652E90` then packs it through the active
/// retail 16-bit DirectDraw descriptor before `Update @ 0x00656EC0` submits
/// both retained radar rectangles. This is deliberately not a color inferred
/// from radar/chrome art or a UI-theme palette.
pub fn native_outline_from(rgb: [u8; 3]) -> [f32; 3] {
    let format = super::native_surface_format::RGB565;
    let [red, green, blue] = format.unpack_rgb8(format.pack_rgb8(rgb));
    let expanded = [red, green, blue, 255];
    // Batch's Legacy palette path multiplies encoded RGB, then decodes for
    // the sRGB target. Feed the same enrolled packed-word presentation used
    // by chrome art; decoding this tint a second time darkens Allied outlines.
    let presented =
        super::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION.quantize_rgba8(expanded);
    [presented[0], presented[1], presented[2]].map(|byte| f32::from(byte) / 255.0)
}

/// Render depth for the credits glyph quads. Shares the `sidebar_text` layer
/// with the cameo Ready/queue labels, which is drawn from the GAME.FNT atlas
/// above every sidebar chrome piece.
pub const CREDITS_DEPTH: f32 = 0.00042;

/// Format the credit total exactly as the normal-player branch of gamemd's
/// credits draw does: the wide `"%ld"` literal applied to the *displayed*
/// (animated) value, so a negative total keeps its leading minus sign. `%ld`
/// and `%d` produce identical text for the 32-bit credit value.
pub fn format_credits(credits: i32) -> String {
    credits.to_string()
}

/// The observer branch of `CreditsClass::Draw @ 0x004A2370`
/// (`0x004A23B0..0x004A24A4`): the counter's seconds as hours, minutes and
/// seconds through CSF `TXT_TIME_FORMAT_HOURS`, with anything past 99 hours
/// shown as 359,999 seconds (`0x004A2437..0x004A243C`). Native divides
/// signed, truncating toward zero.
pub fn format_observer_clock(seconds: i32, format: &str) -> String {
    use crate::assets::csf_file::{CsfArg, format_csf};
    let seconds = if seconds / 3600 > 99 {
        359_999
    } else {
        seconds
    };
    let hours = seconds / 3600;
    let rest = seconds - hours * 3600;
    let minutes = rest / 60;
    format_csf(
        format,
        &[
            CsfArg::Int(hours.into()),
            CsfArg::Int(minutes.into()),
            CsfArg::Int((rest - minutes * 60).into()),
        ],
    )
}

#[cfg(all(test, tests_render_s_z))]
mod tests {
    use super::*;
    use crate::render::bit_font::tests::make_test_font;
    use crate::render::sidebar_chrome::SidebarTheme;
    use crate::ui::sidebar::SidebarChromeLayoutSpec;

    #[test]
    fn native_radar_outline_color_uses_encoded_batch_tint_contract() {
        // 72F440 side colors -> 652E90 packed Radar+1208 -> enrolled surface
        // display. The production shader readback is in sidebar_gpu_tests.
        for (theme, packed_bytes, display_bytes) in [
            (SidebarTheme::Allied, [160, 208, 248], [164, 210, 255]),
            (SidebarTheme::Soviet, [248, 252, 0], [255, 255, 0]),
            (SidebarTheme::Yuri, [248, 252, 0], [255, 255, 0]),
        ] {
            let tint = native_outline_from(SidebarChromeLayoutSpec::for_theme(theme).side_text);
            let bytes = tint.map(|c| (c * 255.0).round() as u8);
            assert_eq!(bytes, display_bytes, "theme={theme:?}");
            assert_eq!(
                [bytes[0] & 0xf8, bytes[1] & 0xfc, bytes[2] & 0xf8],
                packed_bytes
            );
        }
    }

    #[test]
    fn a_text_labels_highlight_fades_in_over_its_last_8_units() {
        let font = make_test_font(
            &[
                (b'a' as u16, 6),
                (b'b' as u16, 6),
                (b'c' as u16, 6),
                (b'd' as u16, 6),
                (b'e' as u16, 6),
                (b'f' as u16, 6),
                (b'g' as u16, 6),
                (b'h' as u16, 6),
                (b'i' as u16, 6),
                (b'j' as u16, 6),
            ],
            4,
        );
        // Count 10 draws all ten units; the white highlight fades in over
        // the last eight, newest brightest, and the first two keep the base.
        let instances = font.build_text_label("abcdefghij", 0.0, 0.0, 0.5, [0.0, 0.0], [0; 3], 10);
        assert_eq!(instances.len(), 10);
        assert_eq!(instances[0].tint, [0.0; 3]);
        assert_eq!(instances[1].tint, [0.0; 3]);
        assert!(
            instances[2].tint[0] < 0.2,
            "unit 2 tint = {:?} should be near base",
            instances[2].tint
        );
        assert!(
            instances[9].tint[0] > 0.8,
            "unit 9 tint = {:?} should be near highlight",
            instances[9].tint
        );
    }
}
