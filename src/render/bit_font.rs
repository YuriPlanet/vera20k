//! The interface text engine's front: measurement, wrapping and sprite
//! emission for every text path, owned by `RendererState.bit_font` and shared
//! by `render::shell_text` (Path A) and `render::sidebar_text` (Path B).
//!
//! gamemd's `BitFont` draws GAME.FNT, and so does VERA, through
//! [`crate::render::text_engine`]. Text GAME.FNT fully covers keeps native
//! measurement and wrapping (the ports below) and draws its glyphs scaled
//! evenly; other text (Cyrillic, Greek, Arabic, Thai, the hanzi it boxes) is
//! shaped with GAME.FNT's missing letters drawn in its style and the bundled
//! fonts behind them, drawn in whole pixels like it.
//!
//! Public surface:
//!   - [`BitFont::from_fnt`] / [`BitFont::without_fnt`] constructors
//!   - [`BitFont::text_width`] / [`BitFont::wrap_layout`] for measurement
//!   - [`BitFont::fit_box`] / [`BitFont::fit_line`] / [`BitFont::fitted`] to
//!     squeeze text too long for its box
//!   - [`BitFont::build_text`] for sprite-instance emission
//!   - [`BitFont::missing_color_xor`] for caller-side tint adjustment

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use crate::assets::fnt_file::{FntFile, FntGlyph};
use crate::render::batch::{BatchRenderer, BatchTexture, SpriteInstance};
use crate::render::shell_text_reveal::{PathAReveal, encoded_unit_rgb};
pub use crate::render::text_engine::Fit;
use crate::render::text_engine::{ShapedText, TextEngine};

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
/// Source codepoint for the missing-glyph stand-in: the letter `X`. Native
/// reaches it as the lookup-table **byte offset** `0xB0` (`0x0043472A`), so the
/// codepoint is `0xB0 / 2`; reading the offset as a codepoint gave the old
/// value `0xB0`, which is CP1252 'deg' - slot #133 of retail `GAME.FNT`, width
/// 4, where `X` is slot #58, width 7.
pub const MISSING_GLYPH_CODEPOINT: u16 = 0x58;
/// Darken-strip alpha for sidebar Ready overlay.
pub const DARKEN_ALPHA: u8 = 175;
/// Default fallback space width when FNT lacks a glyph at 0x20 (defensive).
const DEFAULT_SPACE_WIDTH: u32 = 4;
/// GAME.FNT's baseline row when it has no 'H' to measure it from.
const DEFAULT_BASELINE_ROW: u32 = 13;

/// A GAME.FNT glyph: its pixel width and the outline font's glyph for it.
/// Test fonts without an outline font place whole cells at `uv_*`.
#[derive(Clone, Copy, Debug)]
pub struct GlyphEntry {
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
    pub pixel_width: f32,
    pub(crate) glyph_id: u16,
    /// Drawn on the full-cell CJK grid ([`crate::render::text_engine::fonts::is_cjk_sized`]).
    pub(crate) cjk: bool,
}

impl GlyphEntry {
    fn new(codepoint: u16, pixel_width: u32, glyph_id: u16) -> Self {
        Self {
            uv_origin: [0.0; 2],
            uv_size: [0.0; 2],
            pixel_width: pixel_width as f32,
            glyph_id,
            cjk: crate::render::text_engine::fonts::is_cjk_sized(u32::from(codepoint)),
        }
    }
}

/// One line in a wrap layout -- half-open byte range into the source string.
#[derive(Clone, Copy, Debug)]
pub struct LineSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub width: u32,
    /// The line's paragraph runs right to left, so its start edge is the
    /// right one.
    pub rtl: bool,
}

/// Result of [`BitFont::wrap_layout`] -- total bounds + per-line spans.
#[derive(Clone, Debug, Default)]
pub struct WrapLayout {
    pub width: u32,
    pub height: u32,
    pub lines: Vec<LineSpan>,
    /// A word wider than a line was split across lines.
    pub split_word: bool,
}

/// Immutable atlas observation; `source_identity == None` identifies the
/// built-in fallback (or a GPU-free synthetic test font).
pub(crate) struct BitFontCaptureObservation<'a> {
    pub(crate) source_identity: Option<&'a crate::assets::fnt_file::FntSourceIdentity>,
    pub(crate) atlas_dimensions: Option<[u32; 2]>,
    pub(crate) glyph_count: usize,
    pub(crate) cell_height: u32,
    pub(crate) bitmap_rows: u32,
    pub(crate) missing_glyph_present: bool,
    pub(crate) darken_texture_present: bool,
}

/// The text engine front: GAME.FNT's metrics for the native paths, the
/// shaping engine and its glyph atlas.
///
/// Texture fields are `Option<BatchTexture>` so pure-measurement tests can
/// construct a `BitFont` without a GPU context (`atlas`/`darken_texture`
/// accessors `expect` Some -- production callers always populate via
/// `from_fnt`/`without_fnt`).
pub struct BitFont {
    source_identity: Option<crate::assets::fnt_file::FntSourceIdentity>,
    /// The engine's glyph atlas as of this frame ([`Self::begin_frame`]).
    pub(crate) atlas_texture: Option<BatchTexture>,
    /// GAME.FNT's own drawings, without the code points it points at its
    /// shared placeholder box.
    pub(crate) glyphs: HashMap<u16, GlyphEntry>,
    pub(crate) missing_glyph: Option<GlyphEntry>,
    pub(crate) cell_height: u32,
    pub(crate) bitmap_rows: u32,
    pub(crate) space_width: u32,
    pub(crate) char_spacing: u32,
    pub(crate) tab_width: u32,
    pub(crate) tab_origin: u32,
    pub(crate) darken_texture: Option<BatchTexture>,
    engine: RefCell<TextEngine>,
    /// GPU-free test fonts: every text takes the native path and places
    /// whole cells, as GAME.FNT's own glyph table did.
    cells_only: bool,
    /// The squeeze every measurement and draw takes ([`Self::fitted`]).
    fit: Cell<Fit>,
    /// Where the text drawn now lands ([`Self::set_raster`]).
    raster: Cell<TextRaster>,
}

/// Where the text a [`BitFont`] emits lands, and how finely it rasterizes
/// ([`BitFont::set_raster`]). Text laid out for one pass at `p`, its camera
/// included, lands at `origin + (p - camera) * scale` in a pass whose camera
/// is at the origin, with every glyph drawn `scale` times as large; a clip
/// rect, in the first pass's pixels, at `origin + rect * scale`. A zoomed
/// match lays its text out in the frame and draws it in the window
/// (`app::presentation::window_ui`), so it shows the window's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextRaster {
    pub scale: f32,
    pub origin: [f32; 2],
    pub camera: [f32; 2],
}

impl TextRaster {
    /// Text drawn in the pass it is laid out for.
    pub const NATIVE: Self = Self {
        scale: 1.0,
        origin: [0.0; 2],
        camera: [0.0; 2],
    };

    pub fn point(self, [x, y]: [f32; 2]) -> [f32; 2] {
        [
            self.origin[0] + (x - self.camera[0]) * self.scale,
            self.origin[1] + (y - self.camera[1]) * self.scale,
        ]
    }

    /// A box, frame or picture laid out with the text, moved and grown alike.
    pub fn quad(self, instance: SpriteInstance) -> SpriteInstance {
        SpriteInstance {
            position: self.point(instance.position),
            size: instance.size.map(|length| length * self.scale),
            ..instance
        }
    }
}

/// The Path-A reveal a TextLabel draws for its inclusive count (see
/// [`BitFont::build_text_label`]); zero draws the whole text plainly.
pub(crate) fn text_label_reveal(count: u32, base_rgb: [u8; 3]) -> PathAReveal {
    PathAReveal {
        count: if count == 0 { 0 } else { count + 1 },
        range: crate::ui::messages::TEXT_LABEL_HIGHLIGHT_UNITS,
        base_rgb,
        highlight_rgb: [255; 3],
    }
}

/// `text` without `\r` and `\n`, which the single-line native paths skip, and
/// for each kept byte its offset in `text`.
fn single_line(text: &str) -> (Cow<'_, str>, Option<Vec<u32>>) {
    if !text.contains(['\r', '\n']) {
        return (Cow::Borrowed(text), None);
    }
    let mut kept = String::with_capacity(text.len());
    let mut offsets = Vec::with_capacity(text.len());
    for (at, c) in text.char_indices() {
        if c == '\r' || c == '\n' {
            continue;
        }
        kept.push(c);
        offsets.extend((0..c.len_utf8() as u32).map(|i| at as u32 + i));
    }
    (Cow::Owned(kept), Some(offsets))
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
    /// when the BitFont was built via the fallback path (sentinel that
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

    /// The interface language's tag, for region-specific CJK glyph forms
    /// and its reading direction (`i18n::right_to_left`).
    pub fn set_locale(&mut self, locale: &str) {
        let right_to_left = crate::i18n::right_to_left(locale);
        self.engine
            .get_mut()
            .set_locale(Some(locale), right_to_left);
    }

    /// Measure and draw at `fit` for the length of `draw`.
    pub fn fitted<R>(&self, fit: Fit, draw: impl FnOnce(&BitFont) -> R) -> R {
        let outer = self.fit.replace(fit);
        let result = draw(self);
        self.fit.set(outer);
        result
    }

    /// Draw every text for `raster` from now on; returns the raster it
    /// replaces, for the caller to put back. Measurement is unchanged.
    pub fn set_raster(&self, raster: TextRaster) -> TextRaster {
        self.raster.replace(raster)
    }

    pub fn raster(&self) -> TextRaster {
        self.raster.get()
    }

    /// The first [`Fit`] step, from the one [`Self::fitted`] set, at which
    /// `text` wraps into `max_width` native pixels without splitting a word
    /// and in no more lines than `max_height` holds whole (at least one, or
    /// as many as the text's own line breaks make; 0 holds any number). The
    /// tightest when none does.
    pub fn fit_box(&self, text: &str, max_width: u32, max_height: u32) -> Fit {
        if max_width == 0 || text.is_empty() {
            return self.fit.get();
        }
        let rows = match max_height {
            0 => usize::MAX,
            height => {
                (height.saturating_sub(self.bitmap_rows) / self.cell_height.max(1) + 1) as usize
            }
        }
        .max(text.split('\n').count());
        self.first_fit(|font| {
            let layout = font.wrap_layout(text, max_width);
            layout.lines.len() <= rows && !layout.split_word && layout.width <= max_width
        })
        .unwrap_or_else(|tightest| {
            crate::render::text_audit::cut_off(text, max_width);
            tightest
        })
    }

    /// The first [`Fit`] step, from the one [`Self::fitted`] set, at which
    /// `text` is no wider than `max_width` native pixels on one line; the
    /// tightest when none is.
    pub fn fit_line(&self, text: &str, max_width: u32) -> Fit {
        self.first_fit(|font| font.text_width(text) <= max_width)
            .unwrap_or_else(|tightest| {
                crate::render::text_audit::cut_off(text, max_width);
                tightest
            })
    }

    /// The first step that `fits`, or the tightest as the error when none
    /// does. GPU-free test fonts draw their cells as they are.
    pub(crate) fn first_fit(&self, fits: impl Fn(&BitFont) -> bool) -> Result<Fit, Fit> {
        let mut fit = self.fit.get();
        if self.cells_only {
            return Ok(fit);
        }
        while !self.fitted(fit, &fits) {
            let Some(tighter) = fit.tighter() else {
                return Err(fit);
            };
            fit = tighter;
        }
        Ok(fit)
    }

    /// Take `previous`'s language, for a font rebuilt from a scenario's
    /// GAME.FNT.
    pub fn inherit_settings(&mut self, previous: &BitFont) {
        let (locale, right_to_left) = {
            let engine = previous.engine.borrow();
            (engine.locale().map(str::to_owned), engine.right_to_left())
        };
        self.engine
            .get_mut()
            .set_locale(locale.as_deref(), right_to_left);
    }

    /// Start a frame: the glyph atlas may empty or grow here, never while
    /// instances that sample it are being built.
    pub fn begin_frame(&mut self, device: &wgpu::Device, batch: &BatchRenderer) {
        let engine = self.engine.get_mut();
        engine.begin_frame(Some((device, batch)));
        if let Some(texture) = engine.atlas_texture() {
            self.atlas_texture = Some(texture.clone());
        }
    }

    /// Whether `text` takes the native GAME.FNT paths: no squeeze and every
    /// character drawn by GAME.FNT itself.
    fn native(&self, text: &str) -> bool {
        if self.cells_only {
            return true;
        }
        self.fit.get() == Fit::NONE
            && !self.glyphs.is_empty()
            && text.chars().all(|c| match c {
                '\t' | '\r' | '\n' | ' ' => true,
                // GAME.FNT has no right-to-left letters, and its native paths
                // would draw them left to right.
                _ if crate::i18n::is_right_to_left_letter(c) => false,
                _ => u16::try_from(c as u32).is_ok_and(|cp| self.glyphs.contains_key(&cp)),
            })
    }

    fn shape(&self, text: &str, max_width: u32) -> Arc<ShapedText> {
        self.engine
            .borrow_mut()
            .shape(text, max_width, self.fit.get())
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
        let missing_tint = Self::missing_color_xor(tint);
        let ([x, y], scale) = self.rastered([x, y], scale, camera_offset);
        if self.native(text) {
            let mut instances = Vec::with_capacity(text.len());
            self.native_glyph_pens(text, x, scale, |glyph, pen_x, missing| {
                let tint = if missing { missing_tint } else { tint };
                instances.extend(self.native_instance(glyph, [pen_x, y], scale, depth, tint));
                true
            });
            return instances;
        }
        let (line, _) = single_line(text);
        let shaped = self.shape(&line, 0);
        let mut instances = Vec::with_capacity(line.len());
        let mut engine = self.engine.borrow_mut();
        for shaped_line in &shaped.lines {
            engine.emit_line(
                shaped_line,
                [x, y],
                scale,
                depth,
                |glyph| Some(if glyph.missing { missing_tint } else { tint }),
                &mut instances,
            );
        }
        instances
    }

    /// A text's pen `[x, y]` and glyph `scale`, laid out with `camera_offset`
    /// added, where the raster in use draws them.
    fn rastered(&self, [x, y]: [f32; 2], scale: f32, camera_offset: [f32; 2]) -> ([f32; 2], f32) {
        let raster = self.raster.get();
        (
            raster.point([x + camera_offset[0], y + camera_offset[1]]),
            scale * raster.scale,
        )
    }

    /// Walk `text` the way native `BitFont` advances its pen: `visit` gets
    /// each drawn glyph, its pen x in device pixels and whether it is the
    /// missing-glyph stand-in, and returns `false` to stop. Spaces, tabs and
    /// line breaks move the pen without a glyph.
    fn native_glyph_pens(
        &self,
        text: &str,
        x: f32,
        scale: f32,
        mut visit: impl FnMut(GlyphEntry, f32, bool) -> bool,
    ) {
        let mut cursor_x = x;
        let spacing = self.char_spacing as f32 * scale;
        let mut emitted = 0u32;
        for ch in text.chars() {
            match ch {
                // \r/\n are layout-only control chars.
                '\r' | '\n' => continue,
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
            let (entry, missing) = match u16::try_from(ch as u32)
                .ok()
                .and_then(|cp| self.glyphs.get(&cp))
            {
                Some(g) => (Some(*g), false),
                None => (self.missing_glyph, true),
            };
            let Some(entry) = entry else { continue };
            if emitted > 0 {
                cursor_x += spacing;
            }
            if !visit(entry, cursor_x, missing) {
                return;
            }
            cursor_x += entry.pixel_width * scale;
            emitted += 1;
        }
    }

    /// One native glyph with its cell's top-left at `pen`.
    fn native_instance(
        &self,
        glyph: GlyphEntry,
        pen: [f32; 2],
        scale: f32,
        depth: f32,
        tint: [f32; 3],
    ) -> Option<SpriteInstance> {
        if self.cells_only {
            return Some(SpriteInstance {
                position: pen,
                size: [glyph.pixel_width * scale, self.bitmap_rows as f32 * scale],
                uv_origin: glyph.uv_origin,
                uv_size: glyph.uv_size,
                depth,
                tint,
                alpha: 1.0,
                ..Default::default()
            });
        }
        self.engine.borrow_mut().place_original_glyph(
            glyph.glyph_id,
            glyph.cjk,
            pen,
            scale,
            depth,
            tint,
        )
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
            text_label_reveal(count, base_rgb),
        )
        .0
    }

    /// Glyph emission for the verified BITFONT Path-A UTF-16 reveal/tint path.
    ///
    /// Every UTF-16 code unit consumes a one-based reveal position;
    /// spaces/tabs consume a unit without emitting a quad, and surrogate
    /// halves independently take the existing missing-glyph path. Shaped
    /// text reveals a cluster with its first unit.
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
        // Each unit's encoded colour, until the reveal cuts the text off.
        let mut unit_rgb: Vec<[u8; 3]> = Vec::with_capacity(text.len());
        let mut consumed = already_consumed;
        for _ in text.encode_utf16() {
            let Some(unit_position) = consumed.checked_add(1) else {
                break;
            };
            let Some(encoded_rgb) = encoded_unit_rgb(unit_position, reveal) else {
                break;
            };
            consumed = unit_position;
            unit_rgb.push(encoded_rgb);
        }
        // UI tints multiply the encoded texel (`palette_light`), so the
        // encoded COLORREF bytes are the tint.
        let tint_of = |unit: usize| {
            unit_rgb
                .get(unit)
                .map(|rgb| rgb.map(|c| f32::from(c) / 255.0))
        };
        let ([x, y], scale) = self.rastered([x, y], scale, camera_offset);

        let mut instances = Vec::with_capacity(text.len());
        if self.native(text) {
            let mut units = text.encode_utf16().enumerate().filter(|&(_, unit)| {
                unit != u16::from(b'\r')
                    && unit != u16::from(b'\n')
                    && unit != u16::from(b'\t')
                    && unit != u16::from(b' ')
            });
            self.native_glyph_pens_utf16(text, x, scale, |glyph, pen_x, missing| {
                let Some((unit, _)) = units.next() else {
                    return false;
                };
                let Some(tint) = tint_of(unit) else {
                    return false;
                };
                let tint = if missing {
                    Self::missing_color_xor(tint)
                } else {
                    tint
                };
                instances.extend(self.native_instance(glyph, [pen_x, y], scale, depth, tint));
                true
            });
            return (instances, consumed);
        }

        let (line, offsets) = single_line(text);
        // UTF-16 unit index of every byte of `text`.
        let mut unit_at = vec![0u32; text.len() + 1];
        let mut unit = 0u32;
        for (at, c) in text.char_indices() {
            for slot in &mut unit_at[at..at + c.len_utf8()] {
                *slot = unit;
            }
            unit += c.len_utf16() as u32;
        }
        let shaped = self.shape(&line, 0);
        let mut engine = self.engine.borrow_mut();
        for shaped_line in &shaped.lines {
            engine.emit_line(
                shaped_line,
                [x, y],
                scale,
                depth,
                |glyph| {
                    let byte = offsets.as_ref().map_or(glyph.cluster, |map| {
                        map.get(glyph.cluster as usize)
                            .copied()
                            .unwrap_or(glyph.cluster)
                    });
                    let tint = tint_of(unit_at[byte as usize] as usize)?;
                    Some(if glyph.missing {
                        Self::missing_color_xor(tint)
                    } else {
                        tint
                    })
                },
                &mut instances,
            );
        }
        (instances, consumed)
    }

    /// [`Self::native_glyph_pens`] over UTF-16 units, as Path A walks them: a
    /// surrogate half is its own (missing) glyph.
    fn native_glyph_pens_utf16(
        &self,
        text: &str,
        x: f32,
        scale: f32,
        mut visit: impl FnMut(GlyphEntry, f32, bool) -> bool,
    ) {
        let mut cursor_x = x;
        let spacing = self.char_spacing as f32 * scale;
        let mut emitted = 0u32;
        for code_unit in text.encode_utf16() {
            match code_unit {
                value if value == u16::from(b'\r') || value == u16::from(b'\n') => continue,
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
            let (entry, missing) = match self.glyphs.get(&code_unit) {
                Some(glyph) => (Some(*glyph), false),
                None => (self.missing_glyph, true),
            };
            let Some(entry) = entry else { continue };
            if emitted > 0 {
                cursor_x += spacing;
            }
            if !visit(entry, cursor_x, missing) {
                return;
            }
            cursor_x += entry.pixel_width * scale;
            emitted += 1;
        }
    }

    pub fn wrap_layout(&self, text: &str, max_width: u32) -> WrapLayout {
        if text.is_empty() {
            return WrapLayout::default();
        }
        if self.native(text) {
            return self.wrap_layout_native(text, max_width);
        }
        let shaped = self.shape(text, max_width);
        let lines: Vec<LineSpan> = shaped
            .lines
            .iter()
            .map(|line| LineSpan {
                start_byte: line.start,
                end_byte: line.end,
                width: line.width.ceil() as u32,
                rtl: line.rtl,
            })
            .collect();
        WrapLayout {
            width: lines.iter().map(|line| line.width).max().unwrap_or(0),
            height: self.cell_height * lines.len() as u32,
            lines,
            split_word: shaped.lines.iter().any(|line| line.split),
        }
    }

    fn wrap_layout_native(&self, text: &str, max_width: u32) -> WrapLayout {
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
        let mut split_word = false;

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
                        rtl: false,
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
                    rtl: false,
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
                // Hanzi break between any two; anything else was a word.
                split_word |=
                    !crate::render::text_engine::fonts::is_cjk_sized(chars[i - 1].1 as u32);
                lines.push(LineSpan {
                    start_byte: line_start_byte,
                    end_byte: deferred_byte,
                    width: line_x,
                    rtl: false,
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
            rtl: false,
        });
        max_line_width = max_line_width.max(line_x);

        WrapLayout {
            width: max_line_width,
            height: self.cell_height * y_lines,
            lines,
            split_word,
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
        if !self.native(text) {
            return self.shape(&single_line(text).0, 0).width().ceil() as u32;
        }
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

    /// The longest start of `text` that fits `fit_width`, cut the way the
    /// shell's owner-drawn labels cut theirs: GAME.FNT text loses whole
    /// UTF-16 units, each measured as [`Self::text_width`] measures a
    /// character and ending at a NUL; shaped text loses whole characters.
    pub fn truncate_to_width<'a>(&self, text: &'a str, fit_width: u32) -> Cow<'a, str> {
        if self.native(text) {
            let mut utf16: Vec<u16> = text.encode_utf16().collect();
            if self.utf16_text_width(&utf16) <= fit_width {
                return Cow::Borrowed(text);
            }
            while !utf16.is_empty() && self.utf16_text_width(&utf16) > fit_width {
                utf16.pop();
            }
            return Cow::Owned(String::from_utf16_lossy(&utf16));
        }
        if self.text_width(text) <= fit_width {
            return Cow::Borrowed(text);
        }
        // Each kept character count's end; shaped widths grow with the count,
        // so the longest fitting start is a binary search.
        let ends: Vec<usize> = text
            .char_indices()
            .map(|(start, c)| start + c.len_utf8())
            .collect();
        let (mut kept, mut over) = (0, ends.len());
        while kept < over {
            let mid = (kept + over).div_ceil(2);
            if self.text_width(&text[..ends[mid - 1]]) <= fit_width {
                kept = mid;
            } else {
                over = mid - 1;
            }
        }
        Cow::Owned(text[..kept.checked_sub(1).map_or(0, |last| ends[last])].to_owned())
    }

    fn utf16_text_width(&self, units: &[u16]) -> u32 {
        const TAB: u16 = b'\t' as u16;
        const CR: u16 = b'\r' as u16;
        const LF: u16 = b'\n' as u16;
        const SPACE: u16 = b' ' as u16;

        let mut x: u32 = 0;
        let mut count: u32 = 0;
        for unit in units.iter().copied() {
            match unit {
                0 => break,
                TAB => {
                    let advanced = x + self.tab_width;
                    x = advanced - ((advanced.saturating_sub(self.tab_origin)) % self.tab_width);
                    continue;
                }
                CR | LF => continue,
                SPACE => {
                    x += self.space_width;
                    count += 1;
                }
                other => {
                    let w = self
                        .glyphs
                        .get(&other)
                        .map(|g| g.pixel_width as u32)
                        .or_else(|| self.missing_glyph.as_ref().map(|g| g.pixel_width as u32));
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

    /// Without GAME.FNT (no game files yet, or a damaged archive): the
    /// letters it lacks, drawn in its style, stand in for all of its own,
    /// with the bundled fonts behind them.
    pub fn without_fnt(device: &wgpu::Device, queue: &wgpu::Queue, batch: &BatchRenderer) -> Self {
        let engine = TextEngine::without_fnt(Some((device, queue, batch)));
        Self {
            source_identity: None,
            atlas_texture: engine.atlas_texture().cloned(),
            glyphs: HashMap::new(),
            missing_glyph: None,
            cell_height: CELL_HEIGHT,
            bitmap_rows: BITMAP_ROWS,
            space_width: DEFAULT_SPACE_WIDTH,
            char_spacing: CHAR_SPACING,
            tab_width: TAB_WIDTH,
            tab_origin: TAB_ORIGIN,
            darken_texture: None,
            engine: RefCell::new(engine),
            cells_only: false,
            fit: Cell::new(Fit::NONE),
            raster: Cell::new(TextRaster::NATIVE),
        }
    }

    /// The stand-in bitmap for a codepoint the font does not carry.
    ///
    /// gamemd-derived: `BitFont`'s ctor calls `0x00434700` right after the
    /// loader (`0x004338ED`), which reads `word ptr [lookup table + 0xB0]`
    /// (`0x0043472A`) - a byte offset, so the source codepoint is
    /// `0xB0 / 2 = 0x58`, the letter `X` - copies that glyph's width byte
    /// (`0x00434743..0x00434748`) and then copies its rows as `not row_byte`
    /// (`0x00434764..0x00434774`). Inverting the 1-bit mask, not the colour,
    /// is what makes the cell fill where the glyph was clear.
    fn missing_glyph_stand_in(src: &FntGlyph) -> FntGlyph {
        let width = src.width.max(1) as usize;
        let rows = src.rgba.len() / (width * 4);
        let mut rgba = vec![0u8; width * rows * 4];
        for row in 0..rows {
            for x in 0..width {
                let i = (row * width + x) * 4;
                let set = src.rgba.get(i + 3).copied().unwrap_or(0) != 0;
                if !set {
                    rgba[i] = 255;
                    rgba[i + 1] = 255;
                    rgba[i + 2] = 255;
                    rgba[i + 3] = 255;
                }
            }
        }
        FntGlyph {
            slot: src.slot,
            width: src.width,
            rgba,
        }
    }

    /// The record the stand-in is cut from. Native's row-cache builder
    /// (`0x00434700`) indexes the lookup table at the **byte** offset
    /// `0xB0`, so the source codepoint is `0x58` ('X'), not `0xB0` ('deg').
    fn missing_glyph_record(fnt: &FntFile) -> Option<FntGlyph> {
        fnt.glyph(MISSING_GLYPH_CODEPOINT)
            .map(Self::missing_glyph_stand_in)
    }

    pub fn from_fnt(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        fnt: &FntFile,
    ) -> Self {
        Self::with_fnt(fnt, Some((device, queue, batch)))
    }

    /// [`Self::from_fnt`]'s font; without `gpu` it only measures and lays
    /// out, for tests.
    fn with_fnt(fnt: &FntFile, gpu: Option<(&wgpu::Device, &wgpu::Queue, &BatchRenderer)>) -> Self {
        let missing = Self::missing_glyph_record(fnt);
        let baseline_row = fnt
            .glyph(u16::from(b'H'))
            .and_then(|glyph| {
                let width = glyph.width as usize;
                (0..fnt.bitmap_rows as usize)
                    .rev()
                    .find(|row| {
                        (0..width).any(|x| {
                            glyph
                                .rgba
                                .get((row * width + x) * 4 + 3)
                                .is_some_and(|&a| a != 0)
                        })
                    })
                    .map(|row| row as u32 + 1)
            })
            .unwrap_or(DEFAULT_BASELINE_ROW);
        let outline = crate::render::text_engine::fnt_font::FntFont::build(
            fnt,
            baseline_row,
            missing.as_ref(),
        );
        let mut engine = TextEngine::new(gpu);
        let glyph_ids = outline.glyph_ids.clone();
        engine.set_original_font(outline);

        let glyphs: HashMap<u16, GlyphEntry> = fnt
            .drawn_codepoints()
            .filter_map(|(cp, glyph)| {
                let id = *glyph_ids.get(&cp)?;
                Some((cp, GlyphEntry::new(cp, glyph.width, id)))
            })
            .collect();
        let missing_glyph =
            missing.map(|glyph| GlyphEntry::new(MISSING_GLYPH_CODEPOINT, glyph.width, 0));
        let space_width = fnt
            .glyph(0x20)
            .map(|g| g.width)
            .unwrap_or(DEFAULT_SPACE_WIDTH);
        let darken_texture = gpu.map(|(device, queue, batch)| {
            batch.create_texture_on_device(device, queue, &[0u8, 0, 0, DARKEN_ALPHA], 1, 1, None)
        });

        log::info!(
            "BitFont: GAME.FNT draws {} code points (+missing={}), baseline row {}, space_width={}",
            glyphs.len(),
            missing_glyph.is_some(),
            baseline_row,
            space_width
        );

        Self {
            source_identity: fnt.source_identity().cloned(),
            atlas_texture: engine.atlas_texture().cloned(),
            glyphs,
            missing_glyph,
            cell_height: fnt.cell_height,
            bitmap_rows: fnt.bitmap_rows,
            space_width,
            char_spacing: CHAR_SPACING,
            tab_width: TAB_WIDTH,
            tab_origin: TAB_ORIGIN,
            darken_texture,
            engine: RefCell::new(engine),
            cells_only: false,
            fit: Cell::new(Fit::NONE),
            raster: Cell::new(TextRaster::NATIVE),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A GPU-free font that shapes every text with the bundled fonts alone.
    pub(crate) fn make_vector_test_font() -> BitFont {
        BitFont {
            source_identity: None,
            atlas_texture: None,
            glyphs: HashMap::new(),
            missing_glyph: None,
            cell_height: CELL_HEIGHT,
            bitmap_rows: BITMAP_ROWS,
            space_width: DEFAULT_SPACE_WIDTH,
            char_spacing: CHAR_SPACING,
            tab_width: TAB_WIDTH,
            tab_origin: TAB_ORIGIN,
            darken_texture: None,
            engine: RefCell::new(TextEngine::new(None)),
            cells_only: false,
            fit: Cell::new(Fit::NONE),
            raster: Cell::new(TextRaster::NATIVE),
        }
    }

    /// A GPU-free font of `fnt` itself, falling back to the bundled fonts
    /// for what it lacks, as [`BitFont::from_fnt`] builds it.
    pub(crate) fn make_original_test_font(fnt: &FntFile) -> BitFont {
        BitFont::with_fnt(fnt, None)
    }

    /// Build a measurement-only BitFont without a GPU context, for pure-logic
    /// tests. Atlas/darken textures are left as None -- production callers
    /// always populate both via `from_fnt`/`without_fnt`.
    pub(crate) fn make_test_font(glyph_widths: &[(u16, u32)], space_width: u32) -> BitFont {
        let mut glyphs = HashMap::new();
        for (cp, w) in glyph_widths {
            glyphs.insert(
                *cp,
                GlyphEntry {
                    uv_origin: [0.0, 0.0],
                    uv_size: [1.0, 1.0],
                    pixel_width: *w as f32,
                    glyph_id: 0,
                    cjk: false,
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
                glyph_id: 0,
                cjk: false,
            }),
            cell_height: CELL_HEIGHT,
            bitmap_rows: BITMAP_ROWS,
            space_width,
            char_spacing: CHAR_SPACING,
            tab_width: TAB_WIDTH,
            tab_origin: TAB_ORIGIN,
            darken_texture: None,
            engine: RefCell::new(TextEngine::new(None)),
            cells_only: true,
            fit: Cell::new(Fit::NONE),
            raster: Cell::new(TextRaster::NATIVE),
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

    /// Native's stand-in is the `X` glyph with its 1-bit rows inverted, so the
    /// cell fills where the glyph was clear and keeps the source width byte.
    #[test]
    fn the_missing_glyph_stand_in_complements_the_mask() {
        let src = FntGlyph {
            slot: 1,
            width: 3,
            rgba: vec![
                255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0,
            ],
        };
        let stand_in = BitFont::missing_glyph_stand_in(&src);
        assert_eq!(
            stand_in.width, 3,
            "the width byte is copied, not recomputed"
        );
        let alpha: Vec<u8> = (0..6).map(|pixel| stand_in.rgba[pixel * 4 + 3]).collect();
        assert_eq!(
            alpha,
            vec![0, 0, 255, 255, 255, 255],
            "lit pixels clear and clear pixels fill, as `not row_byte` does"
        );
    }

    /// Two-record font: codepoint `0x58` ('X') width 7 with clear rows,
    /// codepoint `0xB0` ('deg') width 4 with set rows, so picking the wrong
    /// source changes the observed width.
    fn two_record_fnt() -> FntFile {
        const HEADER_BYTES: usize = 4 + 6 * 4;
        const TABLE_BYTES: usize = 65536 * 2;
        const STRIDE: usize = 3; // 1 width byte + 2 rows of 1 byte
        let mut data = vec![0u8; HEADER_BYTES + TABLE_BYTES + STRIDE * 2];
        data[..4].copy_from_slice(&0x546E_6F66u32.to_le_bytes());
        for (i, field) in [20u32, 1, 2, 3, 2, STRIDE as u32].iter().enumerate() {
            data[4 + i * 4..8 + i * 4].copy_from_slice(&field.to_le_bytes());
        }
        for (codepoint, index) in [(0x58u16, 1u16), (0xB0, 2)] {
            // The retail table stores 1-based glyph indices.
            let off = HEADER_BYTES + codepoint as usize * 2;
            data[off..off + 2].copy_from_slice(&index.to_le_bytes());
        }
        let glyphs = HEADER_BYTES + TABLE_BYTES;
        data[glyphs] = 7; // 'X': width 7, both rows clear
        data[glyphs + STRIDE] = 4; // 'deg': width 4, both rows set
        data[glyphs + STRIDE + 1] = 0xFF;
        data[glyphs + STRIDE + 2] = 0xFF;
        FntFile::from_bytes(&data).expect("synthetic FNT parses")
    }

    #[test]
    fn the_missing_glyph_record_uses_the_table_byte_offset() {
        let record = BitFont::missing_glyph_record(&two_record_fnt())
            .expect("the fixture carries the stand-in");
        assert_eq!(
            record.width, 7,
            "the stand-in is the record at byte offset 0xB0 (codepoint 0x58, 'X'), not 0xB0 ('deg')"
        );
        assert!(
            record.rgba.chunks(4).all(|pixel| pixel[3] == 255),
            "'X' is clear where it is set in 'deg', so the cell fills"
        );
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
