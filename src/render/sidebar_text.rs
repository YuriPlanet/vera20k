//! Path B upper wrapper for sidebar text. Single-line emission with optional
//! selected-unit fade (first N characters tinted from side-highlight color
//! toward the base text color). Side-color highlight table per side
//! (Allied / Soviet / Yuri).
//!
//! Most sidebar callers use the plain pass-through fns; only the Ready cameo
//! text needs `build_text_with_fade`.

use crate::render::batch::{BatchTexture, SpriteInstance};
use crate::render::bit_font::BitFont;
use crate::render::sidebar_chrome::SidebarTheme;
use crate::sidebar::Rect;

/// Side highlight colors used as fade endpoint for selected-unit text effect.
const HIGHLIGHT_ALLIED: [f32; 3] = [164.0 / 255.0, 210.0 / 255.0, 1.0];
const HIGHLIGHT_SOVIET: [f32; 3] = [1.0, 1.0, 0.0];
const HIGHLIGHT_YURI: [f32; 3] = [1.0, 1.0, 0.0];

pub fn side_highlight_color(theme: SidebarTheme) -> [f32; 3] {
    match theme {
        SidebarTheme::Allied => HIGHLIGHT_ALLIED,
        SidebarTheme::Soviet => HIGHLIGHT_SOVIET,
        SidebarTheme::Yuri => HIGHLIGHT_YURI,
    }
}

/// Packed `RadarClass+0x1208` color expanded back to the presentation frame.
///
/// `SetSidebarTextColor @ 0x0072F440` selects the exact RGB table above;
/// `RadarClass::Init_For_House @ 0x00652E90` then packs it through the active
/// retail 16-bit DirectDraw descriptor before `Update @ 0x00656EC0` submits
/// both retained radar rectangles. This is deliberately not a color inferred
/// from radar/chrome art or a UI-theme palette.
pub fn native_radar_outline_color(theme: SidebarTheme) -> [f32; 3] {
    let source = side_highlight_color(theme);
    let to_byte = |channel: f32| (channel * 255.0).round() as u8;
    let packed = super::native_radar_terrain::pack_rgb565(
        to_byte(source[0]),
        to_byte(source[1]),
        to_byte(source[2]),
    );
    let expanded = super::native_radar_terrain::unpack_rgb565(packed);
    // Batch's Legacy palette path multiplies encoded RGB, then decodes for
    // the sRGB target. Feed the same enrolled packed-word presentation used
    // by chrome art; decoding this tint a second time darkens Allied outlines.
    let presented =
        super::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION.quantize_rgba8(expanded);
    [presented[0], presented[1], presented[2]].map(|byte| f32::from(byte) / 255.0)
}

// --- Plain pass-throughs preserved for existing single-color callers ---

pub fn text_width(font: &BitFont, text: &str) -> f32 {
    font.text_width(text) as f32
}

pub fn glyph_height(font: &BitFont) -> f32 {
    font.glyph_height()
}

pub fn darken_texture(font: &BitFont) -> Option<&BatchTexture> {
    font.darken_texture()
}

pub fn texture(font: &BitFont) -> &BatchTexture {
    font.atlas()
}

#[allow(clippy::too_many_arguments)]
pub fn build_text(
    font: &BitFont,
    text: &str,
    x: f32,
    y: f32,
    scale: f32,
    depth: f32,
    tint: [f32; 3],
    camera_offset: [f32; 2],
) -> Vec<SpriteInstance> {
    font.build_text(text, x, y, scale, depth, tint, camera_offset)
}

/// Selected-unit fade. First `fade_param` characters (capped at 8) tint from
/// `side_highlight` toward `base_color`; subsequent characters use
/// `base_color`. `fade_param == 0` => no fade (equivalent to `build_text`).
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub fn build_text_with_fade(
    font: &BitFont,
    text: &str,
    x: f32,
    y: f32,
    scale: f32,
    depth: f32,
    base_color: [f32; 3],
    side_highlight: [f32; 3],
    fade_param: u32,
    camera_offset: [f32; 2],
) -> Vec<SpriteInstance> {
    if fade_param == 0 {
        return font.build_text(text, x, y, scale, depth, base_color, camera_offset);
    }
    let chars_to_fade = fade_param.min(8);
    let mut line_offset: u32 = 9u32.saturating_sub(fade_param) * 0x1F;
    let mut out: Vec<SpriteInstance> = Vec::with_capacity(text.len());
    let mut cursor_x = x;
    let spacing = scale; // CHAR_SPACING = 1
    let mut emitted = 0u32;

    for (char_idx, ch) in text.chars().enumerate() {
        if ch == '\r' || ch == '\n' {
            continue;
        }
        let tint = if (char_idx as u32) < chars_to_fade {
            // Fade from highlight back to the normal text color: line_offset
            // starts small ((9-fade_param)*0x1F) and grows by 0x1F per char,
            // so early chars are near highlight, later chars near base.
            let t = (line_offset.min(255) as f32) / 255.0;
            lerp_rgb(side_highlight, base_color, t)
        } else {
            base_color
        };
        if (char_idx as u32) < chars_to_fade {
            line_offset = line_offset.saturating_add(0x1F);
        }
        if ch == ' ' {
            if emitted > 0 {
                cursor_x += spacing;
            }
            // Match `BitFont::build_text` glyph-emission cursor advance: just
            // the bare space width. `text_width(" ")` now includes a trailing
            // char_spacing (matching gamemd's MeasureText) and would
            // double-count the spacing we already added above.
            cursor_x += font.space_width as f32 * scale;
            emitted += 1;
            continue;
        }
        let mut single = font.build_text(
            &ch.to_string(),
            cursor_x,
            y,
            scale,
            depth,
            tint,
            camera_offset,
        );
        if let Some(inst) = single.first() {
            let w = inst.size[0];
            if emitted > 0 {
                for s in &mut single {
                    s.position[0] += spacing;
                }
                cursor_x += spacing;
            }
            cursor_x += w;
        }
        out.append(&mut single);
        emitted += 1;
    }
    out
}

#[cfg(test)]
fn lerp_rgb(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::bit_font::tests::make_test_font;

    #[test]
    fn side_highlight_table_matches_doc() {
        assert_eq!(side_highlight_color(SidebarTheme::Allied), HIGHLIGHT_ALLIED);
        assert_eq!(side_highlight_color(SidebarTheme::Soviet), HIGHLIGHT_SOVIET);
        assert_eq!(side_highlight_color(SidebarTheme::Yuri), HIGHLIGHT_YURI);
    }

    #[test]
    fn native_radar_outline_color_uses_encoded_batch_tint_contract() {
        // 72F440 side colors -> 652E90 packed Radar+1208 -> enrolled surface
        // display. The production shader readback is in sidebar_gpu_tests.
        for (theme, packed_bytes, display_bytes) in [
            (SidebarTheme::Allied, [160, 208, 248], [164, 210, 255]),
            (SidebarTheme::Soviet, [248, 252, 0], [255, 255, 0]),
            (SidebarTheme::Yuri, [248, 252, 0], [255, 255, 0]),
        ] {
            let tint = native_radar_outline_color(theme);
            let bytes = tint.map(|c| (c * 255.0).round() as u8);
            assert_eq!(bytes, display_bytes, "theme={theme:?}");
            assert_eq!(
                [bytes[0] & 0xf8, bytes[1] & 0xfc, bytes[2] & 0xf8],
                packed_bytes
            );
        }
    }

    #[test]
    fn lerp_endpoints() {
        let a = [0.0, 0.0, 0.0];
        let b = [1.0, 1.0, 1.0];
        assert_eq!(lerp_rgb(a, b, 0.0), a);
        assert_eq!(lerp_rgb(a, b, 1.0), b);
        let mid = lerp_rgb(a, b, 0.5);
        assert!((mid[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn fade_param_zero_equivalent_to_build_text() {
        let font = make_test_font(&[(b'a' as u16, 6), (b'b' as u16, 6), (b'c' as u16, 6)], 4);
        let plain = build_text(
            &font,
            "abc",
            0.0,
            0.0,
            1.0,
            0.5,
            [0.0, 0.0, 0.0],
            [0.0, 0.0],
        );
        let faded = build_text_with_fade(
            &font,
            "abc",
            0.0,
            0.0,
            1.0,
            0.5,
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            0,
            [0.0, 0.0],
        );
        assert_eq!(plain.len(), faded.len());
        for (p, f) in plain.iter().zip(faded.iter()) {
            assert_eq!(p.tint, f.tint);
            assert!((p.position[0] - f.position[0]).abs() < 1e-3);
        }
    }

    #[test]
    fn fade_only_first_8_chars() {
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
        let base = [0.0, 0.0, 0.0];
        let highlight = [1.0, 1.0, 1.0];
        let instances = build_text_with_fade(
            &font,
            "abcdefghij",
            0.0,
            0.0,
            1.0,
            0.5,
            base,
            highlight,
            8,
            [0.0, 0.0],
        );
        assert_eq!(instances.len(), 10);
        // Char 0: fade_param=8 -> initial line_offset = (9-8)*0x1F = 0x1F = 31,
        // t = 31/255 ~ 0.12, tint ~ lerp(white,black,0.12) ~ (0.88,0.88,0.88).
        assert!(
            instances[0].tint[0] > 0.8,
            "char 0 tint = {:?} should be near highlight",
            instances[0].tint
        );
        // Char 7: line_offset has advanced 7 times => 31 + 7*31 = 248,
        // t = 248/255 ~ 0.97, tint ~ near base black.
        assert!(
            instances[7].tint[0] < 0.2,
            "char 7 tint = {:?} should be near base",
            instances[7].tint
        );
        // Chars 8/9 are past the fade band -> pure base color.
        assert_eq!(instances[8].tint, base);
        assert_eq!(instances[9].tint, base);
    }
}

/// Anchor row of the credits string on the sidebar surface, in native
/// (unscaled) pixels. gamemd passes the literal `2`.
pub const CREDITS_SURFACE_Y: f32 = 2.0;

/// Render depth for the credits glyph quads. Shares the `sidebar_text` layer
/// with the cameo Ready/queue labels, which is drawn from the GAME.FNT atlas
/// above every sidebar chrome piece.
pub const CREDITS_DEPTH: f32 = 0.00042;

/// Format the credit total exactly as the normal-player branch of gamemd's
/// credits draw does: the wide `"%ld"` literal applied to the *displayed*
/// (animated) value, so a negative total keeps its leading minus sign. `%ld`
/// and `%d` produce identical text for the 32-bit credit value.
///
/// The observer branch formats CSF `TXT_TIME_FORMAT_HOURS` instead; VERA has
/// no observer view yet, so only the player branch is modelled here.
pub fn format_credits(credits: i32) -> String {
    credits.to_string()
}

/// Colour the credits counter is drawn in: the side-dependent sidebar text
/// colour, the same one the cameo status labels use. gamemd packs that colour
/// through the surface's loss/shift masks before the draw; VERA keeps it
/// linear RGB. Named here so the choice is one testable decision rather than
/// an argument the render lane happens to pass.
pub fn credits_tint(theme: crate::render::sidebar_chrome::SidebarTheme) -> [f32; 3] {
    crate::render::sidebar_text::side_highlight_color(theme)
}

/// Build the credits counter as GAME.FNT glyph quads.
///
/// `panel_rect` is the sidebar surface in screen space — its origin is the
/// screen-top-left corner of the sidebar column, which is what gamemd measures
/// `surface_width / 2` and `y = 2` against. Both `panel_rect` and `ui_scale`
/// are already in scaled screen pixels, so the native `y = 2` is scaled here
/// and the glyphs are emitted at `ui_scale`, matching the cameo text lane.
pub fn build_credits_instances(
    font: &BitFont,
    credits: i32,
    panel_rect: Rect,
    ui_scale: f32,
    tint: [f32; 3],
    camera_offset: [f32; 2],
) -> Vec<SpriteInstance> {
    let text = format_credits(credits);
    // Horizontal centre: gamemd's anchor is the surface midpoint and the
    // h-centre flag pulls the *measured* string half its width to the left.
    // `BitFont::text_width` is that same measurement (trailing inter-character
    // spacing on every glyph), so the half-width offset lands where retail's
    // does rather than on the visual ink centre.
    //
    // Both halvings are integer divisions, as in the native rect helper. In
    // float they would land on a half-pixel for any odd surface width or odd
    // measured width, and the counter would resolve to either of two columns
    // as the fractional camera moved — a visible 1 px shimmer on a number that
    // is on screen the whole match. The camera offset is added *after* the
    // rounding, so panning never feeds back into the fixed UI geometry (the
    // same convention `place_canvas_crop_in_slot` uses for cameo art).
    let text_w = (font.text_width(&text) as f32 * ui_scale).round() as i32;
    let surface_w = panel_rect.w.round() as i32;
    let x = panel_rect.x.round() + (surface_w / 2 - text_w / 2) as f32;
    let y = panel_rect.y.round() + (CREDITS_SURFACE_Y * ui_scale).round();
    font.build_text(&text, x, y, ui_scale, CREDITS_DEPTH, tint, camera_offset)
}
