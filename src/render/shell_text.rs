//! Path A upper wrapper for shell controls. Bit-flag alignment, per-pixel
//! scissor clip, vertical center via measure-then-offset, per-line horizontal
//! alignment, `max_height` cutoff. Calls into `bit_font::BitFont` for glyph
//! data and wrap layout.

use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::shell_text_reveal::PathAReveal;

/// Alignment flag set for `draw_in_rect`.
/// 0x01 = h-center, 0x02 = h-right, 0x04 = v-center.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ShellAlign(pub u8);

impl ShellAlign {
    pub const NONE: ShellAlign = ShellAlign(0);
    pub const H_CENTER: ShellAlign = ShellAlign(0x01);
    pub const H_RIGHT: ShellAlign = ShellAlign(0x02);
    pub const V_CENTER: ShellAlign = ShellAlign(0x04);

    pub fn contains(self, flag: ShellAlign) -> bool {
        (self.0 & flag.0) != 0
    }
}

impl std::ops::BitOr for ShellAlign {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        ShellAlign(self.0 | rhs.0)
    }
}

/// Pixel-coordinate scissor rect. Apply via `wgpu::RenderPass::set_scissor_rect`.
#[derive(Copy, Clone, Debug, Default)]
pub struct ScissorRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Output of `draw_in_rect`: sprite instances plus the scissor the caller
/// must set on its render pass before drawing them.
pub struct ShellTextDraw {
    pub instances: Vec<SpriteInstance>,
    pub scissor: ScissorRect,
}

/// Pixel rect input to `draw_in_rect` -- width/height in screen pixels.
#[derive(Copy, Clone, Debug)]
pub struct TextRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// A stretched label's rect back in the glyph atlas's native units, so Path A's
/// integer layout math (wrap, centre, line admission) runs on the numbers
/// gamemd ran on. Identity at `scale == 1.0`.
fn to_native_rect(rect: TextRect, scale: f32) -> TextRect {
    TextRect {
        x: (rect.x as f32 / scale).round() as i32,
        y: (rect.y as f32 / scale).round() as i32,
        w: (rect.w as f32 / scale).round().max(0.0) as u32,
        h: (rect.h as f32 / scale).round().max(0.0) as u32,
    }
}

/// Vertical-centre offset for the v-centre flag.
///
/// gamemd's Path A wrapper computes `y += (rect_height - measured_height) / 2`
/// unconditionally — text taller than its rect gets a *negative* offset and
/// overhangs upward, and the per-pixel clip against the text rect is what
/// keeps it inside. VERA previously gated the offset on
/// `measured_height < rect_height`, which pinned over-tall text to the rect
/// top instead; that gate has no counterpart in the binary.
///
/// Both operands are C++ `int` there, so the division truncates toward zero
/// for negative values — which is also Rust's `i32` division.
fn vcenter_offset(rect_h: u32, measured_h: u32) -> f32 {
    ((rect_h as i32 - measured_h as i32) / 2) as f32
}

pub fn draw_in_rect(
    font: &BitFont,
    text: &str,
    rect: TextRect,
    color: [f32; 3],
    flags: ShellAlign,
    cam_offset: [f32; 2],
    depth: f32,
) -> ShellTextDraw {
    draw_in_rect_scaled(font, text, rect, color, flags, cam_offset, depth, 1.0)
}

/// [`draw_in_rect`] with a glyph scale. `scale == 1.0` is gamemd's 1:1 Path A
/// draw; above it the label is laid out in the rect's native units (the rect
/// divided back by `scale`) and every glyph and advance is emitted at
/// `scale`x, so a stretched shell's caption still centres in its stretched
/// cell. The scissor stays in window pixels.
#[allow(clippy::too_many_arguments)]
pub fn draw_in_rect_scaled(
    font: &BitFont,
    text: &str,
    rect: TextRect,
    color: [f32; 3],
    flags: ShellAlign,
    cam_offset: [f32; 2],
    depth: f32,
    scale: f32,
) -> ShellTextDraw {
    let scissor = ScissorRect {
        x: rect.x.max(0) as u32,
        y: rect.y.max(0) as u32,
        w: rect.w,
        h: rect.h,
    };
    if text.is_empty() {
        return ShellTextDraw {
            instances: Vec::new(),
            scissor,
        };
    }
    let native = to_native_rect(rect, scale);
    let layout = font.wrap_layout(text, native.w);
    let base_x = native.x as f32;
    let mut line_y = native.y as f32;
    if flags.contains(ShellAlign::V_CENTER) {
        line_y += vcenter_offset(native.h, layout.height);
    }
    let line_advance = font.cell_height();

    let mut instances: Vec<SpriteInstance> = Vec::with_capacity(text.len());
    for (line_index, span) in layout.lines.iter().enumerate() {
        // 434CD0 paints the first line before consulting max-height. Its
        // newline/wrap tails (434EC2..434ED7 /435112..435127) stop only after
        // consumed cell advances reach the nonzero limit. The raster scissor
        // clips overhanging glyph pixels; a short clip must not discard an
        // admitted line. Retail GAME.FNT has16 bitmap rows and17px cell advance.
        if line_index > 0 && native.h != 0 && line_index as f32 * line_advance >= native.h as f32 {
            break;
        }
        let line_x_offset = if flags.contains(ShellAlign::H_CENTER) && span.width < native.w {
            ((native.w - span.width) / 2) as f32
        } else if flags.contains(ShellAlign::H_RIGHT) && span.width < native.w {
            (native.w - span.width) as f32
        } else {
            0.0
        };
        let segment = &text[span.start_byte..span.end_byte];
        instances.append(&mut font.build_text(
            segment,
            (base_x + line_x_offset) * scale,
            line_y * scale,
            scale,
            depth,
            color,
            cam_offset,
        ));
        line_y += line_advance;
    }
    ShellTextDraw { instances, scissor }
}

/// Path-A counterpart to [`draw_in_rect`]: the kind-1 statics' UTF-16 unit
/// reveal and tint.
pub fn draw_in_rect_path_a(
    font: &BitFont,
    text: &str,
    rect: TextRect,
    flags: ShellAlign,
    cam_offset: [f32; 2],
    depth: f32,
    reveal: PathAReveal,
) -> ShellTextDraw {
    draw_in_rect_path_a_scaled(font, text, rect, flags, cam_offset, depth, reveal, 1.0)
}

/// [`draw_in_rect_path_a`] with a glyph scale; see [`draw_in_rect_scaled`].
#[allow(clippy::too_many_arguments)]
pub fn draw_in_rect_path_a_scaled(
    font: &BitFont,
    text: &str,
    rect: TextRect,
    flags: ShellAlign,
    cam_offset: [f32; 2],
    depth: f32,
    reveal: PathAReveal,
    scale: f32,
) -> ShellTextDraw {
    let scissor = ScissorRect {
        x: rect.x.max(0) as u32,
        y: rect.y.max(0) as u32,
        w: rect.w,
        h: rect.h,
    };
    if text.is_empty() {
        return ShellTextDraw {
            instances: Vec::new(),
            scissor,
        };
    }
    let native = to_native_rect(rect, scale);
    let layout = font.wrap_layout(text, native.w);
    let base_x = native.x as f32;
    let mut line_y = native.y as f32;
    if flags.contains(ShellAlign::V_CENTER) {
        line_y += vcenter_offset(native.h, layout.height);
    }
    let line_advance = font.cell_height();
    let mut instances = Vec::with_capacity(text.len());
    let mut consumed = 0u32;

    for (line_index, span) in layout.lines.iter().enumerate() {
        // Same original 434CD0 line admission as the non-reveal entry above.
        if line_index > 0 && native.h != 0 && line_index as f32 * line_advance >= native.h as f32 {
            break;
        }
        let line_x_offset = if flags.contains(ShellAlign::H_CENTER) && span.width < native.w {
            ((native.w - span.width) / 2) as f32
        } else if flags.contains(ShellAlign::H_RIGHT) && span.width < native.w {
            (native.w - span.width) as f32
        } else {
            0.0
        };
        let segment = &text[span.start_byte..span.end_byte];
        let (mut line_instances, new_consumed) = font.build_text_path_a(
            segment,
            (base_x + line_x_offset) * scale,
            line_y * scale,
            scale,
            depth,
            cam_offset,
            consumed,
            reveal,
        );
        consumed = new_consumed;
        instances.append(&mut line_instances);
        line_y += line_advance;
    }
    ShellTextDraw { instances, scissor }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::bit_font::tests::make_test_font;

    fn test_font() -> BitFont {
        make_test_font(&[(b'x' as u16, 6), (b'a' as u16, 6), (b'b' as u16, 6)], 4)
    }

    #[test]
    fn line_submissions_match_original_434cd0_for_short_static_rectangles() {
        let golden: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/shell_text_lines.json",
        ))
        .unwrap();
        let mut font = make_test_font(&[(b'A' as u16, 6)], 3);
        font.cell_height = 17;
        font.bitmap_rows = 16;
        font.char_spacing = 0;
        let cases = golden["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 66);
        for case in cases {
            let rect = TextRect {
                x: 11,
                y: 13,
                w: case["width"].as_u64().unwrap() as u32,
                h: case["height"].as_u64().unwrap() as u32,
            };
            let text = case["text"].as_str().unwrap();
            let plain = draw_in_rect(&font, text, rect, [1.0; 3], ShellAlign::NONE, [0.0; 2], 0.5);
            let reveal = draw_in_rect_path_a(
                &font,
                text,
                rect,
                ShellAlign::NONE,
                [0.0; 2],
                0.5,
                PathAReveal {
                    count: 0,
                    range: 8,
                    base_rgb: [255; 3],
                    highlight_rgb: [255; 3],
                },
            );
            let expected: Vec<_> = case["submissions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    [
                        p["x"].as_u64().unwrap() as f32,
                        p["y"].as_u64().unwrap() as f32,
                    ]
                })
                .collect();
            for draw in [plain, reveal] {
                let positions: Vec<_> = draw.instances.iter().map(|i| i.position).collect();
                assert_eq!(positions, expected, "{case}");
                assert_eq!(draw.scissor.h, rect.h);
            }
        }
    }

    #[test]
    fn short_native_static_keeps_first_line_and_clips_pixels_in_both_paths() {
        let font = test_font();
        let rect = TextRect {
            x: 10,
            y: 20,
            w: 100,
            h: 1,
        };
        let plain = draw_in_rect(
            &font,
            "x\nx",
            rect,
            [1.0; 3],
            ShellAlign::NONE,
            [0.0; 2],
            0.5,
        );
        let reveal = draw_in_rect_path_a(
            &font,
            "x\nx",
            rect,
            ShellAlign::NONE,
            [0.0; 2],
            0.5,
            PathAReveal {
                count: 0,
                range: 8,
                base_rgb: [255; 3],
                highlight_rgb: [255; 3],
            },
        );
        for draw in [plain, reveal] {
            assert_eq!(draw.instances.len(), 1);
            assert_eq!(draw.scissor.h, 1);
            assert!(draw.instances[0].size[1] > 1.0);
        }
    }

    #[test]
    fn native_height_limit_counts_cell_advances_independent_of_vertical_center() {
        let font = test_font();
        let h = font.cell_height() as u32 + 1;
        for align in [ShellAlign::NONE, ShellAlign::V_CENTER] {
            let draw = draw_in_rect(
                &font,
                "x\nx\nx",
                TextRect {
                    x: 0,
                    y: 0,
                    w: 100,
                    h,
                },
                [1.0; 3],
                align,
                [0.0; 2],
                0.5,
            );
            assert_eq!(draw.instances.len(), 2);
        }
    }

    fn rect_100x30() -> TextRect {
        TextRect {
            x: 0,
            y: 0,
            w: 100,
            h: 30,
        }
    }

    /// gamemd computes the v-centre offset unconditionally, so text taller
    /// than its rect overhangs upward with a negative offset and the clip rect
    /// trims it. VERA used to gate the offset on the text fitting, which
    /// pinned over-tall text to the rect top instead.
    #[test]
    fn vcenter_offset_is_negative_when_text_is_taller_than_the_rect() {
        let font = test_font();
        // Two lines measure 2 * 17 = 34 px against a 30 px rect.
        let draw = draw_in_rect(
            &font,
            "x\nx",
            TextRect {
                x: 0,
                y: 0,
                w: 100,
                h: 30,
            },
            [1.0, 1.0, 1.0],
            ShellAlign::V_CENTER,
            [0.0, 0.0],
            0.5,
        );
        // Native height admission counts cell advances independently of the
        // centering offset: 17 < 30 admits line two, then the scissor clips it.
        assert_eq!(draw.instances.len(), 2);
        assert_eq!(draw.instances[0].position[1], -2.0);
        assert_eq!(draw.instances[1].position[1], 15.0);
        assert_eq!(draw.scissor.y, 0);
        assert_eq!(draw.scissor.h, 30);
        assert_eq!(vcenter_offset(30, 34), -2.0);
        // C++ integer division truncates toward zero for negatives.
        assert_eq!(vcenter_offset(10, 13), -1.0);
        assert_eq!(vcenter_offset(40, 17), 11.0);
    }

    #[test]
    fn align_center_single_line() {
        let font = test_font();
        let draw = draw_in_rect(
            &font,
            "x",
            TextRect {
                x: 0,
                y: 0,
                w: 100,
                h: 30,
            },
            [1.0, 1.0, 1.0],
            ShellAlign::H_CENTER,
            [0.0, 0.0],
            0.5,
        );
        assert_eq!(draw.instances.len(), 1);
        // Single 'x' measured width per gamemd = 6 + 1*char_spacing = 7.
        let expected_x = ((100 - 7) / 2) as f32;
        assert!(
            (draw.instances[0].position[0] - expected_x).abs() < 0.01,
            "x = {}",
            draw.instances[0].position[0]
        );
    }

    #[test]
    fn path_a_count_one_is_hidden_and_count_two_draws_first_unit() {
        let font = test_font();
        let reveal = |count| PathAReveal {
            count,
            range: 8,
            base_rgb: [255, 255, 0],
            highlight_rgb: [255; 3],
        };
        let blank = draw_in_rect_path_a(
            &font,
            "xax",
            rect_100x30(),
            ShellAlign::NONE,
            [0.0, 0.0],
            0.5,
            reveal(1),
        );
        assert!(blank.instances.is_empty());
        let first = draw_in_rect_path_a(
            &font,
            "xax",
            rect_100x30(),
            ShellAlign::NONE,
            [0.0, 0.0],
            0.5,
            reveal(2),
        );
        assert_eq!(first.instances.len(), 1);
    }

    #[test]
    fn path_a_uses_utf16_units_for_surrogate_halves_spaces_and_tabs() {
        let font = test_font();
        let reveal = PathAReveal {
            count: 6,
            range: 8,
            base_rgb: [255, 255, 0],
            highlight_rgb: [255; 3],
        };
        // x=1 unit, 😀=2 surrogate units, space=1, tab=1, x=1. At count 6,
        // positions 1..5 are visible and position 6 is cut. The two surrogate
        // halves use the missing glyph, while space/tab emit no quads.
        let draw = draw_in_rect_path_a(
            &font,
            "x😀 \tx",
            rect_100x30(),
            ShellAlign::NONE,
            [0.0, 0.0],
            0.5,
            reveal,
        );
        assert_eq!(draw.instances.len(), 3);
    }

    /// The scaled entry points must not move the retail path at all: every
    /// existing caller, golden and pixel guard runs through them now.
    #[test]
    fn scale_one_reproduces_the_retail_draw_exactly() {
        let font = test_font();
        let rect = TextRect {
            x: 10,
            y: 20,
            w: 40,
            h: 30,
        };
        let reveal = || PathAReveal {
            count: 4,
            range: 8,
            base_rgb: [255, 255, 0],
            highlight_rgb: [255; 3],
        };
        for (text, flags) in [
            ("xa b\nxx", ShellAlign::NONE),
            ("xax ab", ShellAlign::H_CENTER | ShellAlign::V_CENTER),
            ("x ax", ShellAlign::H_RIGHT),
        ] {
            let plain = draw_in_rect(&font, text, rect, [1.0; 3], flags, [0.0; 2], 0.5);
            let plain_scaled =
                draw_in_rect_scaled(&font, text, rect, [1.0; 3], flags, [0.0; 2], 0.5, 1.0);
            assert_same_scissor(&plain.scissor, &plain_scaled.scissor);
            assert_same_quads(&plain.instances, &plain_scaled.instances, "plain");

            let path_a = draw_in_rect_path_a(&font, text, rect, flags, [0.0; 2], 0.5, reveal());
            let path_a_scaled =
                draw_in_rect_path_a_scaled(&font, text, rect, flags, [0.0; 2], 0.5, reveal(), 1.0);
            assert_same_scissor(&path_a.scissor, &path_a_scaled.scissor);
            assert_same_quads(&path_a.instances, &path_a_scaled.instances, "path a");
        }
    }

    /// A label in a 2x-stretched shell is the retail label with every glyph
    /// position, size and advance doubled, and the window-pixel scissor kept.
    #[test]
    fn a_stretched_label_doubles_every_glyph_and_advance() {
        let font = test_font();
        let reveal = || PathAReveal {
            count: 4,
            range: 8,
            base_rgb: [255, 255, 0],
            highlight_rgb: [255; 3],
        };
        let retail = TextRect {
            x: 10,
            y: 20,
            w: 40,
            h: 30,
        };
        let stretched = TextRect {
            x: 20,
            y: 40,
            w: 80,
            h: 60,
        };
        // The stretched rect maps back to the retail rect exactly, so the line
        // structure is identical and only the emitted quads are doubled.
        for (text, flags) in [("xab", ShellAlign::NONE), ("xa b", ShellAlign::H_CENTER)] {
            let plain = draw_in_rect(&font, text, retail, [1.0; 3], flags, [0.0; 2], 0.5);
            let doubled =
                draw_in_rect_scaled(&font, text, stretched, [1.0; 3], flags, [0.0; 2], 0.5, 2.0);
            assert_eq!(
                plain.instances.len(),
                doubled.instances.len(),
                "plain count"
            );
            assert!(plain.instances.len() >= 2, "the test needs an advance");
            for (one, two) in plain.instances.iter().zip(&doubled.instances) {
                assert_eq!([two.position[0] / 2.0, two.position[1] / 2.0], one.position);
                assert_eq!([two.size[0] / 2.0, two.size[1] / 2.0], one.size);
                assert_eq!(two.uv_origin, one.uv_origin);
                assert_eq!(two.uv_size, one.uv_size);
            }
            assert_eq!(doubled.scissor.x, stretched.x as u32);
            assert_eq!(doubled.scissor.y, stretched.y as u32);
            assert_eq!(doubled.scissor.w, stretched.w);
            assert_eq!(doubled.scissor.h, stretched.h);

            let path_a = draw_in_rect_path_a(&font, text, retail, flags, [0.0; 2], 0.5, reveal());
            let path_a_doubled = draw_in_rect_path_a_scaled(
                &font,
                text,
                stretched,
                flags,
                [0.0; 2],
                0.5,
                reveal(),
                2.0,
            );
            assert_eq!(
                path_a.instances.len(),
                path_a_doubled.instances.len(),
                "path a count"
            );
            for (one, two) in path_a.instances.iter().zip(&path_a_doubled.instances) {
                assert_eq!([two.position[0] / 2.0, two.position[1] / 2.0], one.position);
                assert_eq!([two.size[0] / 2.0, two.size[1] / 2.0], one.size);
            }
        }
    }

    fn assert_same_scissor(one: &ScissorRect, two: &ScissorRect) {
        assert_eq!((one.x, one.y, one.w, one.h), (two.x, two.y, two.w, two.h));
    }

    fn assert_same_quads(one: &[SpriteInstance], two: &[SpriteInstance], what: &str) {
        assert_eq!(one.len(), two.len(), "{what} instance count");
        for (a, b) in one.iter().zip(two) {
            assert_eq!(a.position, b.position, "{what} position");
            assert_eq!(a.size, b.size, "{what} size");
            assert_eq!(a.uv_origin, b.uv_origin, "{what} uv origin");
            assert_eq!(a.uv_size, b.uv_size, "{what} uv size");
        }
    }
}
