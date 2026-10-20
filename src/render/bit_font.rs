//! Lower-layer bitmap font: atlas, glyph table, measurement, wrap state
//! machine, missing-glyph fallback. Owned by `RendererState.bit_font` and shared
//! by `render::shell_text` (Path A) and `render::sidebar_text` (Path B).
//!
//! Glyph data comes from a parsed [`crate::assets::fnt_file::FntFile`]
//! (GAME.FNT). Falls back to a hardcoded 5x7 path when the FNT is unavailable.
//!
//! Public surface:
//!   - [`BitFont::from_fnt`] / [`BitFont::fallback_5x7`] constructors
//!   - [`BitFont::text_width`] / [`BitFont::wrap_layout`] for measurement
//!   - [`BitFont::build_text`] for sprite-instance emission
//!   - [`BitFont::missing_color_xor`] for caller-side tint adjustment

use std::collections::HashMap;

use crate::assets::fnt_file::{FntFile, FntGlyph};
use crate::render::batch::{BatchRenderer, BatchTexture, SpriteInstance};
use crate::render::shell_text_reveal::{PathAReveal, encoded_unit_rgb};

/// Hardcoded inter-glyph spacing.
pub const CHAR_SPACING: u32 = 1;
/// Tab stop width in pixels.
pub const TAB_WIDTH: u32 = 64;
/// Tab origin -- subtracted from x before `% TAB_WIDTH`.
pub const TAB_ORIGIN: u32 = 0;
/// Cell height for GAME.FNT (line advance = bitmap_rows + 1px gap).
pub const CELL_HEIGHT: u32 = 17;
/// Bitmap rows per glyph for GAME.FNT.
pub const BITMAP_ROWS: u32 = 16;
/// Source codepoint for the missing-glyph fallback (CP1252 'deg').
pub const MISSING_GLYPH_CODEPOINT: u16 = 0xB0;
/// Darken-strip alpha for sidebar Ready overlay.
pub const DARKEN_ALPHA: u8 = 175;
/// Default fallback space width when FNT lacks a glyph at 0x20 (defensive).
const DEFAULT_SPACE_WIDTH: u32 = 4;
/// Codepoint range packed into the atlas (ASCII + Latin-1 + CJK/fullwidth).
/// End-exclusive at u16::MAX, which is reserved for the missing-glyph sentinel.
const PACKED_CODEPOINT_RANGE: std::ops::Range<u16> = 0x20..0xFFFF;

/// UV + pixel-width record for a single glyph in the atlas.
#[derive(Clone, Copy, Debug)]
pub struct GlyphEntry {
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
    pub pixel_width: f32,
}

/// One line in a wrap layout -- half-open byte range into the source string.
#[derive(Clone, Copy, Debug)]
pub struct LineSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub width: u32,
}

/// Result of [`BitFont::wrap_layout`] -- total bounds + per-line spans.
#[derive(Clone, Debug, Default)]
pub struct WrapLayout {
    pub width: u32,
    pub height: u32,
    pub lines: Vec<LineSpan>,
}

/// Immutable atlas observation; `source_identity == None` identifies the
/// built-in 5x7 fallback (or a GPU-free synthetic test font).
pub(crate) struct BitFontCaptureObservation<'a> {
    pub(crate) source_identity: Option<&'a crate::assets::fnt_file::FntSourceIdentity>,
    pub(crate) atlas_dimensions: Option<[u32; 2]>,
    pub(crate) glyph_count: usize,
    pub(crate) cell_height: u32,
    pub(crate) bitmap_rows: u32,
    pub(crate) missing_glyph_present: bool,
    pub(crate) darken_texture_present: bool,
}

/// Atlas-backed bitmap font + measurement + missing-glyph fallback.
///
/// Texture fields are `Option<BatchTexture>` so pure-measurement tests can
/// construct a `BitFont` without a GPU context (`atlas`/`darken_texture`
/// accessors `expect` Some -- production callers always populate via
/// `from_fnt`/`fallback_5x7`).
pub struct BitFont {
    source_identity: Option<crate::assets::fnt_file::FntSourceIdentity>,
    pub(crate) atlas_texture: Option<BatchTexture>,
    pub(crate) glyphs: HashMap<u16, GlyphEntry>,
    pub(crate) missing_glyph: Option<GlyphEntry>,
    pub(crate) cell_height: u32,
    pub(crate) bitmap_rows: u32,
    pub(crate) space_width: u32,
    pub(crate) char_spacing: u32,
    pub(crate) tab_width: u32,
    pub(crate) tab_origin: u32,
    pub(crate) darken_texture: Option<BatchTexture>,
}

impl BitFont {
    pub(crate) fn capture_observation(&self) -> BitFontCaptureObservation<'_> {
        BitFontCaptureObservation {
            source_identity: self.source_identity.as_ref(),
            atlas_dimensions: self
                .atlas_texture
                .as_ref()
                .map(|atlas| [atlas.width, atlas.height]),
            glyph_count: self.glyphs.len(),
            cell_height: self.cell_height,
            bitmap_rows: self.bitmap_rows,
            missing_glyph_present: self.missing_glyph.is_some(),
            darken_texture_present: self.darken_texture.is_some(),
        }
    }

    pub fn atlas(&self) -> &BatchTexture {
        self.atlas_texture
            .as_ref()
            .expect("BitFont atlas not populated (test-only ctor)")
    }
    /// Darken-strip texture for the sidebar Ready overlay. Returns `None`
    /// when the BitFont was built via the 5x7 fallback path (sentinel that
    /// callers treat as "no FNT loaded, skip sidebar text rendering").
    pub fn darken_texture(&self) -> Option<&BatchTexture> {
        self.darken_texture.as_ref()
    }
    pub fn glyph_height(&self) -> f32 {
        self.bitmap_rows as f32
    }
    pub fn cell_height(&self) -> f32 {
        self.cell_height as f32
    }

    /// ComputeTextRect4A59E0: whole-string width, horizontal alignment and
    /// signed margins. Unlike the shell's wrapped layout, this query uses the
    /// font's cell height and measures before clipping. PRIMARY70AC60 supplies
    /// flags149 and margins[4,2]. Native pixels: primary_factory_status.json.
    pub(crate) fn compute_text_rect(
        &self,
        text: &str,
        [mut x, y]: [i32; 2],
        flags: u32,
        [margin_x, margin_y]: [i32; 2],
    ) -> [i32; 4] {
        let width = self.text_width(text) as i32;
        if flags & 0x100 != 0 {
            x = x.wrapping_sub(width / 2);
        } else if flags & 0x200 != 0 {
            x = x.wrapping_sub(width);
        }
        [
            x.wrapping_sub(margin_x),
            y.wrapping_sub(margin_y),
            width.wrapping_add(margin_x.wrapping_mul(2)),
            (self.cell_height as i32).wrapping_add(margin_y.wrapping_mul(2)),
        ]
    }

    pub fn build_text(
        &self,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        depth: f32,
        tint: [f32; 3],
        camera_offset: [f32; 2],
    ) -> Vec<SpriteInstance> {
        let mut instances = Vec::with_capacity(text.len());
        let mut cursor_x = x;
        let spacing = self.char_spacing as f32 * scale;
        let h = self.bitmap_rows as f32 * scale;
        let missing_tint = Self::missing_color_xor(tint);
        let mut emitted = 0u32;

        for ch in text.chars() {
            // \r/\n are layout-only control chars.
            if ch == '\r' || ch == '\n' {
                continue;
            }
            match ch {
                '\t' => {
                    let cell_x = ((cursor_x - x) / scale) as u32;
                    let advanced = cell_x + self.tab_width;
                    let next_cell =
                        advanced - ((advanced.saturating_sub(self.tab_origin)) % self.tab_width);
                    cursor_x = x + (next_cell as f32) * scale;
                    continue;
                }
                ' ' => {
                    if emitted > 0 {
                        cursor_x += spacing;
                    }
                    cursor_x += self.space_width as f32 * scale;
                    emitted += 1;
                    continue;
                }
                _ => {}
            }
            let cp = ch as u32;
            let (entry, use_missing_tint) = if cp <= u16::MAX as u32 {
                match self.glyphs.get(&(cp as u16)) {
                    Some(g) => (Some(*g), false),
                    None => (self.missing_glyph, true),
                }
            } else {
                (self.missing_glyph, true)
            };
            let Some(entry) = entry else { continue };
            if emitted > 0 {
                cursor_x += spacing;
            }
            let w = entry.pixel_width * scale;
            instances.push(SpriteInstance {
                position: [cursor_x + camera_offset[0], y + camera_offset[1]],
                size: [w, h],
                uv_origin: entry.uv_origin,
                uv_size: entry.uv_size,
                depth,
                tint: if use_missing_tint { missing_tint } else { tint },
                alpha: 1.0,
                ..Default::default()
            });
            cursor_x += w;
            emitted += 1;
        }
        instances
    }

    /// TextLabel's original434500 emission uses an inclusive UTF-16 count;
    /// PathA434CD0 uses the next, hidden unit. For positive count C, its
    /// cutoff and eight-unit gradient are exactly PathA(C+1, range8): both
    /// blend unit i (zero-based) by (9-C+i)*31 through shared6612C0.
    /// The white highlight is CRT4E8160's A80220 value, not a side color.
    /// Executed paired controls: tools/storage_oracle/shell_text_label.json.
    pub fn build_text_label(
        &self,
        text: &str,
        x: f32,
        y: f32,
        depth: f32,
        camera_offset: [f32; 2],
        base_rgb: [u8; 3],
        count: u32,
    ) -> Vec<SpriteInstance> {
        // Original cutoff43454D..51 is signed. Ordinary message lifetimes
        // never approach this boundary, but retain it for wrapping counters.
        if (count as i32) < 0 {
            return Vec::new();
        }
        self.build_text_path_a(
            text,
            x,
            y,
            1.0,
            depth,
            camera_offset,
            0,
            PathAReveal {
                count: if count == 0 { 0 } else { count + 1 },
                range: 8,
                base_rgb,
                highlight_rgb: [255; 3],
            },
        )
        .0
    }

    /// Glyph emission for the verified BITFONT Path-A UTF-16 reveal/tint path.
    ///
    /// Every UTF-16 code unit consumes a one-based reveal position;
    /// spaces/tabs consume a unit without emitting a quad, and surrogate
    /// halves independently take the existing missing-glyph path.
    pub fn build_text_path_a(
        &self,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        depth: f32,
        camera_offset: [f32; 2],
        already_consumed: u32,
        reveal: PathAReveal,
    ) -> (Vec<SpriteInstance>, u32) {
        let mut instances = Vec::with_capacity(text.len());
        let mut cursor_x = x;
        let spacing = self.char_spacing as f32 * scale;
        let h = self.bitmap_rows as f32 * scale;
        let mut emitted = 0u32;
        let mut consumed = already_consumed;

        for code_unit in text.encode_utf16() {
            let Some(unit_position) = consumed.checked_add(1) else {
                break;
            };
            let Some(encoded_rgb) = encoded_unit_rgb(unit_position, reveal) else {
                break;
            };
            consumed = unit_position;
            if code_unit == u16::from(b'\r') || code_unit == u16::from(b'\n') {
                continue;
            }
            match code_unit {
                value if value == u16::from(b'\t') => {
                    let cell_x = ((cursor_x - x) / scale) as u32;
                    let advanced = cell_x + self.tab_width;
                    let next_cell =
                        advanced - ((advanced.saturating_sub(self.tab_origin)) % self.tab_width);
                    cursor_x = x + next_cell as f32 * scale;
                    continue;
                }
                value if value == u16::from(b' ') => {
                    if emitted > 0 {
                        cursor_x += spacing;
                    }
                    cursor_x += self.space_width as f32 * scale;
                    emitted += 1;
                    continue;
                }
                _ => {}
            }

            // UI tints multiply the encoded texel (`palette_light`), so the
            // encoded COLORREF bytes are the tint.
            let tint = encoded_rgb.map(|channel| f32::from(channel) / 255.0);
            let missing_tint = Self::missing_color_xor(tint);
            let (entry, use_missing_tint) = match self.glyphs.get(&code_unit) {
                Some(glyph) => (Some(*glyph), false),
                None => (self.missing_glyph, true),
            };
            let Some(entry) = entry else { continue };
            if emitted > 0 {
                cursor_x += spacing;
            }
            let w = entry.pixel_width * scale;
            instances.push(SpriteInstance {
                position: [cursor_x + camera_offset[0], y + camera_offset[1]],
                size: [w, h],
                uv_origin: entry.uv_origin,
                uv_size: entry.uv_size,
                depth,
                tint: if use_missing_tint { missing_tint } else { tint },
                alpha: 1.0,
                ..Default::default()
            });
            cursor_x += w;
            emitted += 1;
        }
        (instances, consumed)
    }

    pub fn wrap_layout(&self, text: &str, max_width: u32) -> WrapLayout {
        if text.is_empty() {
            return WrapLayout::default();
        }
        let mut lines: Vec<LineSpan> = Vec::new();
        let mut line_start_byte: usize = 0;
        let mut line_x: u32 = 0;
        let mut chars_on_line: u32 = 0;
        let mut max_line_width: u32 = 0;
        // Character index and measured width before the break. Keep the
        // scanner restart and UTF-8 byte boundary tied to the same character.
        let mut last_space: Option<(usize, u32)> = None;
        let mut prev_char: Option<char> = None;
        let mut y_lines: u32 = 1;

        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut i = 0;
        while i < chars.len() {
            let (byte_off, ch) = chars[i];
            let next_byte_off = chars.get(i + 1).map(|c| c.0).unwrap_or(text.len());

            match ch {
                '\t' => {
                    let advanced = line_x + self.tab_width;
                    line_x =
                        advanced - ((advanced.saturating_sub(self.tab_origin)) % self.tab_width);
                    prev_char = Some('\t');
                    i += 1;
                    continue;
                }
                '\r' | '\n' => {
                    if ch == '\n' && prev_char == Some('\r') {
                        prev_char = Some('\n');
                        i += 1;
                        continue;
                    }
                    lines.push(LineSpan {
                        start_byte: line_start_byte,
                        end_byte: byte_off,
                        width: line_x,
                    });
                    max_line_width = max_line_width.max(line_x);
                    line_start_byte = next_byte_off;
                    line_x = 0;
                    chars_on_line = 0;
                    last_space = None;
                    y_lines += 1;
                    prev_char = Some(ch);
                    i += 1;
                    continue;
                }
                ' ' => {
                    last_space = Some((i, line_x));
                }
                _ => {}
            }

            let glyph_w = self.lookup_glyph_width(ch);
            let Some(glyph_w) = glyph_w else {
                prev_char = Some(ch);
                i += 1;
                continue;
            };
            // Every char contributes a trailing `char_spacing` per gamemd's
            // `BitFont__MeasureText` accumulator (`iVar11 += spacing + width`
            // for every char). Matches `text_width` and the
            // wrap-detection threshold used by `FUN_00434CD0`.
            let next_x = line_x + self.char_spacing + glyph_w;

            if max_width == 0 || next_x <= max_width {
                line_x = next_x;
                chars_on_line += 1;
                prev_char = Some(ch);
                i += 1;
            } else if chars_on_line == 0 {
                line_x = next_x;
                chars_on_line = 1;
                prev_char = Some(ch);
                i += 1;
            } else if let Some((space_index, space_x)) = last_space {
                lines.push(LineSpan {
                    start_byte: line_start_byte,
                    end_byte: chars[space_index].0,
                    width: space_x,
                });
                max_line_width = max_line_width.max(space_x);
                // gamemd 434F60..434F64 rewinds the scanner to the saved
                // space; 4350F5..435103 and 43512D..435138 resume after it.
                // Retrying only the overflow character either revisits an
                // already consumed space or skips measuring a word prefix.
                // Native comparison: tools/storage_oracle/shell_text_lines.py.
                i = space_index + 1;
                line_start_byte = chars.get(i).map(|c| c.0).unwrap_or(text.len());
                line_x = 0;
                chars_on_line = 0;
                last_space = None;
                prev_char = Some(' ');
                y_lines += 1;
            } else {
                // Native 434F6F..434F78 excludes the preceding fitting unit
                // from this line's emission. 43512D..435138 nevertheless
                // resumes measurement at the overflowing unit. Preserve the
                // separate emission start and scan cursor; remeasuring the
                // deferred glyph changes subsequent native line boundaries.
                let deferred_byte = chars[i - 1].0;
                lines.push(LineSpan {
                    start_byte: line_start_byte,
                    end_byte: deferred_byte,
                    width: line_x,
                });
                max_line_width = max_line_width.max(line_x);
                line_start_byte = deferred_byte;
                line_x = 0;
                chars_on_line = 0;
                y_lines += 1;
                // Retry the overflow character; the deferred glyph remains
                // in the emitted span without being measured a second time.
            }
        }
        lines.push(LineSpan {
            start_byte: line_start_byte,
            end_byte: text.len(),
            width: line_x,
        });
        max_line_width = max_line_width.max(line_x);

        WrapLayout {
            width: max_line_width,
            height: self.cell_height * y_lines,
            lines,
        }
    }

    fn lookup_glyph_width(&self, ch: char) -> Option<u32> {
        if ch == ' ' {
            return Some(self.space_width);
        }
        let cp = ch as u32;
        if cp <= u16::MAX as u32
            && let Some(g) = self.glyphs.get(&(cp as u16))
        {
            return Some(g.pixel_width as u32);
        }
        self.missing_glyph.as_ref().map(|g| g.pixel_width as u32)
    }

    /// Width per gamemd's `BitFont__MeasureText` (0x00433CF0): every char
    /// contributes its trailing `char_spacing`, so an N-char string measures
    /// to `sum(glyph_widths) + N * char_spacing`. Used by callers for
    /// dark-strip width and h-center anchor; matches the value
    /// `ComputeTextRect` and `FUN_00434CD0`'s wrap-accumulator use.
    pub fn text_width(&self, text: &str) -> u32 {
        let mut x: u32 = 0;
        let mut count: u32 = 0;
        for ch in text.chars() {
            match ch {
                '\t' => {
                    let advanced = x + self.tab_width;
                    x = advanced - ((advanced.saturating_sub(self.tab_origin)) % self.tab_width);
                    continue;
                }
                '\r' | '\n' => continue,
                ' ' => {
                    x += self.space_width;
                    count += 1;
                }
                other => {
                    let cp = other as u32;
                    let w = if cp <= u16::MAX as u32 {
                        self.glyphs
                            .get(&(cp as u16))
                            .map(|g| g.pixel_width as u32)
                            .or_else(|| self.missing_glyph.as_ref().map(|g| g.pixel_width as u32))
                    } else {
                        self.missing_glyph.as_ref().map(|g| g.pixel_width as u32)
                    };
                    if let Some(w) = w {
                        x += w;
                        count += 1;
                    }
                }
            }
        }
        x + count * self.char_spacing
    }

    /// Tint adjustment for missing-glyph fallback rendering -- caller XORs
    /// the input tint to produce the visible "wrong color" effect that
    /// distinguishes missing glyphs at a glance. Faithful 32-bit port of the
    /// original RGB565 `color ^= 0x5555`: decomposing 0x5555 into RGB565
    /// component XOR masks gives R5 ^= 0x0A, G6 ^= 0x2A, B5 ^= 0x15.
    pub fn missing_color_xor(rgb: [f32; 3]) -> [f32; 3] {
        fn xor_565(c: f32, bits: u32, mask: u8) -> f32 {
            let max_val = (1u32 << bits) - 1;
            // The surface unit of the encoded byte, as the presenter packs it.
            let byte = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
            let quantized = byte >> (8 - bits);
            let flipped = (quantized ^ mask) & (max_val as u8);
            (flipped as f32) / (max_val as f32)
        }
        [
            xor_565(rgb[0], 5, 0x0A),
            xor_565(rgb[1], 6, 0x2A),
            xor_565(rgb[2], 5, 0x15),
        ]
    }

    pub fn fallback_5x7(device: &wgpu::Device, queue: &wgpu::Queue, batch: &BatchRenderer) -> Self {
        const GLYPH_W: u32 = 5;
        const GLYPH_H: u32 = 7;
        const GLYPH_PAD: u32 = 1;
        const ATLAS_COLUMNS: usize = 8;

        let supported = fallback_5x7_glyphs();
        let rows = supported.len().div_ceil(ATLAS_COLUMNS);
        let cell_w = GLYPH_W + GLYPH_PAD * 2;
        let cell_h = GLYPH_H + GLYPH_PAD * 2;
        let atlas_w = (ATLAS_COLUMNS as u32) * cell_w;
        let atlas_h = (rows as u32) * cell_h;
        let mut rgba = vec![0u8; (atlas_w * atlas_h * 4) as usize];
        let mut glyphs = HashMap::new();

        for (idx, (ch, bitmap)) in supported.iter().enumerate() {
            let col = (idx % ATLAS_COLUMNS) as u32;
            let row = (idx / ATLAS_COLUMNS) as u32;
            let origin_x = col * cell_w + GLYPH_PAD;
            let origin_y = row * cell_h + GLYPH_PAD;
            write_5x7_glyph_bitmap(&mut rgba, atlas_w, origin_x, origin_y, bitmap);
            glyphs.insert(
                *ch as u16,
                GlyphEntry {
                    uv_origin: [
                        origin_x as f32 / atlas_w as f32,
                        origin_y as f32 / atlas_h as f32,
                    ],
                    uv_size: [
                        GLYPH_W as f32 / atlas_w as f32,
                        GLYPH_H as f32 / atlas_h as f32,
                    ],
                    pixel_width: GLYPH_W as f32,
                },
            );
        }

        Self {
            source_identity: None,
            atlas_texture: Some(
                batch.create_texture_on_device(device, queue, &rgba, atlas_w, atlas_h, None),
            ),
            glyphs,
            missing_glyph: None,
            cell_height: GLYPH_H + 1,
            bitmap_rows: GLYPH_H,
            space_width: GLYPH_W,
            char_spacing: CHAR_SPACING,
            tab_width: TAB_WIDTH,
            tab_origin: TAB_ORIGIN,
            darken_texture: None,
        }
    }

    pub fn from_fnt(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        fnt: &FntFile,
    ) -> Self {
        let mut entries: Vec<(u16, &FntGlyph)> = Vec::new();
        for cp in PACKED_CODEPOINT_RANGE {
            if let Some(g) = fnt.glyph(cp) {
                entries.push((cp, g));
            }
        }

        // Synthesize the missing-glyph bitmap: inverted '°' (codepoint 0xB0).
        // Source is white-on-transparent; invert RGB on set pixels so the
        // fallback is visually distinct against tinted destinations.
        let missing_owned: Option<FntGlyph> = fnt.glyph(MISSING_GLYPH_CODEPOINT).map(|src| {
            let mut rgba = src.rgba.clone();
            let mut i = 0;
            while i + 3 < rgba.len() {
                let a = rgba[i + 3];
                rgba[i] = !rgba[i] & a;
                rgba[i + 1] = !rgba[i + 1] & a;
                rgba[i + 2] = !rgba[i + 2] & a;
                i += 4;
            }
            FntGlyph {
                width: src.width,
                rgba,
            }
        });

        if entries.is_empty() && missing_owned.is_none() {
            log::warn!("FNT has no glyphs, falling back to hardcoded font");
            return Self::fallback_5x7(device, queue, batch);
        }

        // Reserve a sentinel codepoint for the missing glyph that won't collide
        // with the packed range. u16::MAX is well outside 0x20..0x180.
        const MISSING_SENTINEL: u16 = u16::MAX;
        let mut all_entries: Vec<(u16, &FntGlyph)> = Vec::with_capacity(entries.len() + 1);
        for e in &entries {
            all_entries.push((e.0, e.1));
        }
        if let Some(ref g) = missing_owned {
            all_entries.push((MISSING_SENTINEL, g));
        }

        let row_h = fnt.bitmap_rows;
        let pad = 1u32;
        let max_atlas_w = 4096u32;

        struct Placement {
            x: u32,
            y: u32,
        }
        let mut placements: Vec<Placement> = Vec::with_capacity(all_entries.len());
        let mut cursor_x = 0u32;
        let mut cursor_y = 0u32;
        let mut atlas_w = 0u32;

        for (_cp, g) in &all_entries {
            let w = g.width + pad * 2;
            if cursor_x + w > max_atlas_w {
                cursor_x = 0;
                cursor_y += row_h + pad * 2;
            }
            placements.push(Placement {
                x: cursor_x + pad,
                y: cursor_y + pad,
            });
            cursor_x += w;
            if cursor_x > atlas_w {
                atlas_w = cursor_x;
            }
        }
        let atlas_h = cursor_y + row_h + pad * 2;

        let mut rgba = vec![0u8; (atlas_w * atlas_h * 4) as usize];
        let mut glyphs = HashMap::new();
        let mut missing_glyph = None;

        for (idx, (cp, g)) in all_entries.iter().enumerate() {
            let pl = &placements[idx];
            for row in 0..row_h {
                for col in 0..g.width {
                    let src = ((row * g.width + col) * 4) as usize;
                    if src + 3 >= g.rgba.len() {
                        continue;
                    }
                    let dst_x = pl.x + col;
                    let dst_y = pl.y + row;
                    let dst = ((dst_y * atlas_w + dst_x) * 4) as usize;
                    rgba[dst..dst + 4].copy_from_slice(&g.rgba[src..src + 4]);
                }
            }
            let entry = GlyphEntry {
                uv_origin: [pl.x as f32 / atlas_w as f32, pl.y as f32 / atlas_h as f32],
                uv_size: [
                    g.width as f32 / atlas_w as f32,
                    row_h as f32 / atlas_h as f32,
                ],
                pixel_width: g.width as f32,
            };
            if *cp == MISSING_SENTINEL {
                missing_glyph = Some(entry);
            } else {
                glyphs.insert(*cp, entry);
            }
        }

        let space_width = fnt
            .glyph(0x20)
            .map(|g| g.width)
            .unwrap_or(DEFAULT_SPACE_WIDTH);
        let atlas_texture =
            batch.create_texture_on_device(device, queue, &rgba, atlas_w, atlas_h, None);
        let darken_texture =
            batch.create_texture_on_device(device, queue, &[0u8, 0, 0, DARKEN_ALPHA], 1, 1, None);

        log::info!(
            "BitFont atlas: {}x{} px, {} glyphs (+missing={}), space_width={}",
            atlas_w,
            atlas_h,
            glyphs.len(),
            missing_glyph.is_some(),
            space_width
        );

        Self {
            source_identity: fnt.source_identity().cloned(),
            atlas_texture: Some(atlas_texture),
            glyphs,
            missing_glyph,
            cell_height: fnt.cell_height,
            bitmap_rows: fnt.bitmap_rows,
            space_width,
            char_spacing: CHAR_SPACING,
            tab_width: TAB_WIDTH,
            tab_origin: TAB_ORIGIN,
            darken_texture: Some(darken_texture),
        }
    }
}

fn write_5x7_glyph_bitmap(
    rgba: &mut [u8],
    atlas_w: u32,
    origin_x: u32,
    origin_y: u32,
    rows: &[&str; 7],
) {
    for (y, row) in rows.iter().enumerate() {
        for (x, pixel) in row.as_bytes().iter().enumerate() {
            if *pixel != b'#' {
                continue;
            }
            let idx = (((origin_y + y as u32) * atlas_w + (origin_x + x as u32)) * 4) as usize;
            rgba[idx..idx + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
}

/// The built-in 5x7 glyph table. Each entry is seven 5-character rows where `#`
/// sets a pixel. Visible to the crate so the headless asset tools can burn
/// labels into rendered sheets without a font dependency or a second table.
pub(crate) fn fallback_5x7_glyphs() -> Vec<(char, [&'static str; 7])> {
    vec![
        (
            ' ',
            [
                ".....", ".....", ".....", ".....", ".....", ".....", ".....",
            ],
        ),
        (
            '-',
            [
                ".....", ".....", ".....", ".###.", ".....", ".....", ".....",
            ],
        ),
        (
            ':',
            [
                ".....", "..#..", ".....", ".....", "..#..", ".....", ".....",
            ],
        ),
        (
            '/',
            [
                "....#", "...#.", "..#..", ".#...", "#....", ".....", ".....",
            ],
        ),
        (
            '0',
            [
                "#####", "#...#", "#...#", "#...#", "#...#", "#...#", "#####",
            ],
        ),
        (
            '1',
            [
                "..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###.",
            ],
        ),
        (
            '2',
            [
                "#####", "....#", "....#", "#####", "#....", "#....", "#####",
            ],
        ),
        (
            '3',
            [
                "#####", "....#", "..##.", "....#", "....#", "....#", "#####",
            ],
        ),
        (
            '4',
            [
                "#...#", "#...#", "#...#", "#####", "....#", "....#", "....#",
            ],
        ),
        (
            '5',
            [
                "#####", "#....", "#....", "#####", "....#", "....#", "#####",
            ],
        ),
        (
            '6',
            [
                "#####", "#....", "#....", "#####", "#...#", "#...#", "#####",
            ],
        ),
        (
            '7',
            [
                "#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#...",
            ],
        ),
        (
            '8',
            [
                "#####", "#...#", "#...#", "#####", "#...#", "#...#", "#####",
            ],
        ),
        (
            '9',
            [
                "#####", "#...#", "#...#", "#####", "....#", "....#", "#####",
            ],
        ),
        (
            'A',
            [
                ".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#",
            ],
        ),
        (
            'B',
            [
                "####.", "#...#", "#...#", "####.", "#...#", "#...#", "####.",
            ],
        ),
        (
            'C',
            [
                ".####", "#....", "#....", "#....", "#....", "#....", ".####",
            ],
        ),
        (
            'D',
            [
                "####.", "#...#", "#...#", "#...#", "#...#", "#...#", "####.",
            ],
        ),
        (
            'E',
            [
                "#####", "#....", "#....", "####.", "#....", "#....", "#####",
            ],
        ),
        (
            'F',
            [
                "#####", "#....", "#....", "####.", "#....", "#....", "#....",
            ],
        ),
        (
            'G',
            [
                ".####", "#....", "#....", "#.###", "#...#", "#...#", ".###.",
            ],
        ),
        (
            'H',
            [
                "#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#",
            ],
        ),
        (
            'I',
            [
                "#####", "..#..", "..#..", "..#..", "..#..", "..#..", "#####",
            ],
        ),
        (
            'J',
            [
                "#####", "...#.", "...#.", "...#.", "...#.", "#..#.", ".##..",
            ],
        ),
        (
            'K',
            [
                "#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#",
            ],
        ),
        (
            'L',
            [
                "#....", "#....", "#....", "#....", "#....", "#....", "#####",
            ],
        ),
        (
            'M',
            [
                "#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#",
            ],
        ),
        (
            'N',
            [
                "#...#", "##..#", "#.#.#", "#..##", "#...#", "#...#", "#...#",
            ],
        ),
        (
            'O',
            [
                ".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###.",
            ],
        ),
        (
            'P',
            [
                "####.", "#...#", "#...#", "####.", "#....", "#....", "#....",
            ],
        ),
        (
            'Q',
            [
                ".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#",
            ],
        ),
        (
            'R',
            [
                "####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#",
            ],
        ),
        (
            'S',
            [
                ".####", "#....", "#....", ".###.", "....#", "....#", "####.",
            ],
        ),
        (
            'T',
            [
                "#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#..",
            ],
        ),
        (
            'U',
            [
                "#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###.",
            ],
        ),
        (
            'V',
            [
                "#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#..",
            ],
        ),
        (
            'W',
            [
                "#...#", "#...#", "#...#", "#.#.#", "#.#.#", "##.##", "#...#",
            ],
        ),
        (
            'X',
            [
                "#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#",
            ],
        ),
        (
            'Y',
            [
                "#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#..",
            ],
        ),
        (
            'Z',
            [
                "#####", "....#", "...#.", "..#..", ".#...", "#....", "#####",
            ],
        ),
        (
            'a',
            [
                ".....", ".....", ".###.", "....#", ".####", "#...#", ".####",
            ],
        ),
        (
            'b',
            [
                "#....", "#....", "####.", "#...#", "#...#", "#...#", "####.",
            ],
        ),
        (
            'c',
            [
                ".....", ".....", ".####", "#....", "#....", "#....", ".####",
            ],
        ),
        (
            'd',
            [
                "....#", "....#", ".####", "#...#", "#...#", "#...#", ".####",
            ],
        ),
        (
            'e',
            [
                ".....", ".....", ".###.", "#...#", "#####", "#....", ".###.",
            ],
        ),
        (
            'f',
            [
                "..##.", ".#...", ".#...", "####.", ".#...", ".#...", ".#...",
            ],
        ),
        (
            'g',
            [
                ".....", ".####", "#...#", "#...#", ".####", "....#", ".###.",
            ],
        ),
        (
            'h',
            [
                "#....", "#....", "####.", "#...#", "#...#", "#...#", "#...#",
            ],
        ),
        (
            'i',
            [
                "..#..", ".....", "..#..", "..#..", "..#..", "..#..", "..#..",
            ],
        ),
        (
            'j',
            [
                "...#.", ".....", "...#.", "...#.", "...#.", "#..#.", ".##..",
            ],
        ),
        (
            'k',
            [
                "#....", "#....", "#..#.", "#.#..", "##...", "#.#..", "#..#.",
            ],
        ),
        (
            'l',
            [
                ".##..", "..#..", "..#..", "..#..", "..#..", "..#..", ".###.",
            ],
        ),
        (
            'm',
            [
                ".....", ".....", "##.#.", "#.#.#", "#.#.#", "#...#", "#...#",
            ],
        ),
        (
            'n',
            [
                ".....", ".....", "####.", "#...#", "#...#", "#...#", "#...#",
            ],
        ),
        (
            'o',
            [
                ".....", ".....", ".###.", "#...#", "#...#", "#...#", ".###.",
            ],
        ),
        (
            'p',
            [
                ".....", "####.", "#...#", "#...#", "####.", "#....", "#....",
            ],
        ),
        (
            'q',
            [
                ".....", ".####", "#...#", "#...#", ".####", "....#", "....#",
            ],
        ),
        (
            'r',
            [
                ".....", ".....", ".####", "#....", "#....", "#....", "#....",
            ],
        ),
        (
            's',
            [
                ".....", ".....", ".####", "#....", ".###.", "....#", "####.",
            ],
        ),
        (
            't',
            [
                ".#...", ".#...", "####.", ".#...", ".#...", ".#...", "..##.",
            ],
        ),
        (
            'u',
            [
                ".....", ".....", "#...#", "#...#", "#...#", "#...#", ".####",
            ],
        ),
        (
            'v',
            [
                ".....", ".....", "#...#", "#...#", "#...#", ".#.#.", "..#..",
            ],
        ),
        (
            'w',
            [
                ".....", ".....", "#...#", "#...#", "#.#.#", "#.#.#", ".#.#.",
            ],
        ),
        (
            'x',
            [
                ".....", ".....", "#...#", ".#.#.", "..#..", ".#.#.", "#...#",
            ],
        ),
        (
            'y',
            [
                ".....", "#...#", "#...#", ".####", "....#", "...#.", ".##..",
            ],
        ),
        (
            'z',
            [
                ".....", ".....", "#####", "...#.", "..#..", ".#...", "#####",
            ],
        ),
    ]
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Build a measurement-only BitFont without a GPU context, for pure-logic
    /// tests. Atlas/darken textures are left as None -- production callers
    /// always populate both via `from_fnt`/`fallback_5x7`.
    pub(crate) fn make_test_font(glyph_widths: &[(u16, u32)], space_width: u32) -> BitFont {
        let mut glyphs = HashMap::new();
        for (cp, w) in glyph_widths {
            glyphs.insert(
                *cp,
                GlyphEntry {
                    uv_origin: [0.0, 0.0],
                    uv_size: [1.0, 1.0],
                    pixel_width: *w as f32,
                },
            );
        }
        BitFont {
            source_identity: None,
            atlas_texture: None,
            glyphs,
            missing_glyph: Some(GlyphEntry {
                uv_origin: [0.0, 0.0],
                uv_size: [0.0, 0.0],
                pixel_width: 5.0,
            }),
            cell_height: CELL_HEIGHT,
            bitmap_rows: BITMAP_ROWS,
            space_width,
            char_spacing: CHAR_SPACING,
            tab_width: TAB_WIDTH,
            tab_origin: TAB_ORIGIN,
            darken_texture: None,
        }
    }

    #[test]
    fn text_width_uses_fnt_space_width() {
        let font = make_test_font(&[(b'a' as u16, 6), (b'b' as u16, 6)], 4);
        // gamemd MeasureText: every char adds trailing spacing.
        // "a b" = 6 + 4 + 6 + 3 * 1 = 19
        assert_eq!(font.text_width("a b"), 19);
    }

    #[test]
    fn text_width_with_tab() {
        let font = make_test_font(&[(b'a' as u16, 6), (b'b' as u16, 6)], 4);
        // 'a': x = 6, count = 1.
        // '\t': x jumps to next 64-boundary = 64.
        // 'b': x += 6 = 70, count = 2.
        // End: x + count * char_spacing = 70 + 2 = 72.
        assert_eq!(font.text_width("a\tb"), 72);
    }

    #[test]
    fn text_width_with_missing_glyph() {
        let font = make_test_font(&[(b'a' as u16, 6)], 4);
        // 'X' not in table -> missing_glyph (width 5).
        // "aX" = 6 + 5 + 2 * 1 = 13
        assert_eq!(font.text_width("aX"), 13);
    }

    #[test]
    fn wrap_layout_breaks_at_last_space() {
        let font = make_test_font(&[(b'a' as u16, 6), (b'b' as u16, 6), (b'c' as u16, 6)], 4);
        let layout = font.wrap_layout("ab c", 20);
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.height, CELL_HEIGHT * 2);
    }

    #[test]
    fn wrap_layout_single_char_overflow_accepted() {
        let font = make_test_font(&[(b'a' as u16, 20)], 4);
        let layout = font.wrap_layout("a", 10);
        assert_eq!(layout.lines.len(), 1);
        // gamemd-parity: char_spacing always trails. Single 'a' width = 20 + 1.
        assert_eq!(layout.lines[0].width, 21);
    }

    #[test]
    fn wrap_layout_crlf_one_newline() {
        let font = make_test_font(&[(b'a' as u16, 6), (b'b' as u16, 6)], 4);
        let layout = font.wrap_layout("a\r\nb", 1000);
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.height, CELL_HEIGHT * 2);
    }

    #[test]
    fn path_a_tint_is_the_encoded_colorref() {
        // UI tints multiply the encoded texel (`palette_light`): the terminal
        // highlight unit's (248, 252, 30) must reach the surface as blue 30.
        let font = make_test_font(&[(b'a' as u16, 6)], 4);
        let reveal = PathAReveal {
            count: 9,
            range: 8,
            base_rgb: [255, 255, 0],
            highlight_rgb: [255, 255, 255],
        };
        let (instances, _) = font.build_text_path_a("a", 0.0, 0.0, 1.0, 0.5, [0.0; 2], 0, reveal);
        assert_eq!(
            instances[0].tint,
            [248.0 / 255.0, 252.0 / 255.0, 30.0 / 255.0]
        );
    }

    #[test]
    fn text_label_glyph_count_and_colors_match_the_original_paired_paths() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/shell_text_label.json",
        ))
        .unwrap();
        let font = make_test_font(&[(b'A' as u16, 6)], 4);
        let format = crate::render::native_surface_format::RGB565;
        let packed = |instances: Vec<SpriteInstance>| {
            instances
                .iter()
                .map(|glyph| format.pack_rgb8(glyph.tint.map(|c| (c * 255.0).round() as u8)))
                .collect::<Vec<_>>()
        };
        let controls = corpus["controls"].as_array().unwrap();
        assert_eq!(controls.len(), 16);
        let mut label_controls = 0;
        for row in controls {
            let text = row["text"].as_str().unwrap();
            let base_rgb = format.unpack_rgb8(row["base_rgb565"].as_u64().unwrap() as u16);
            let highlight_rgb =
                std::array::from_fn(|i| row["highlight_rgb"][i].as_u64().unwrap() as u8);
            let expected = |field: &str| {
                row[field]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u16)
                    .collect::<Vec<_>>()
            };
            // Both original bodies execute the shared6612C0, including the
            // decreasing white-to-black signed-division controls. The count
            // handed to PathA here is recorded from the native fixture.
            let path_a = font
                .build_text_path_a(
                    text,
                    0.0,
                    0.0,
                    1.0,
                    0.5,
                    [0.0; 2],
                    0,
                    PathAReveal {
                        count: row["path_a_count"].as_u64().unwrap() as u32,
                        range: 8,
                        base_rgb,
                        highlight_rgb,
                    },
                )
                .0;
            assert_eq!(packed(path_a), expected("path_a_rgb565"), "{row}");
            // Actual TextLabel always uses the white CRT highlight; its
            // production adapter must reproduce the other native body too.
            if highlight_rgb == [255; 3] {
                let label = font.build_text_label(
                    text,
                    0.0,
                    0.0,
                    0.5,
                    [0.0; 2],
                    base_rgb,
                    row["native_count"].as_u64().unwrap() as u32,
                );
                assert_eq!(packed(label), expected("generic_rgb565"), "{row}");
                label_controls += 1;
            }
        }
        assert_eq!(label_controls, 8);
    }

    #[test]
    fn missing_color_xor_starts_from_the_surface_unit() {
        // Path-A yellow is (248, 252, 0) in bytes, (31, 63, 0) in units.
        let xored = BitFont::missing_color_xor([248.0 / 255.0, 252.0 / 255.0, 0.0]);
        assert_eq!(xored, [21.0 / 31.0, 21.0 / 63.0, 21.0 / 31.0]);
    }
}
