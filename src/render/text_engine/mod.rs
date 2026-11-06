//! VERA's interface text engine beneath [`crate::render::bit_font::BitFont`].
//!
//! Text is shaped, bidi-ordered, line-broken and given fallback fonts by
//! `parley`, in native (800x600) pixels; each glyph is then rasterised by
//! `swash` at the size the draw scales it to and placed one texel to one
//! window pixel from a shared atlas. Layout does not depend on the draw
//! scale, so one cached layout serves every scale.
//!
//! Two font stacks:
//! - [`FontMode::Modern`]: the bundled Noto fonts ([`fonts`]), sized against
//!   GAME.FNT.
//! - [`FontMode::Original`]: GAME.FNT itself, as an outline font
//!   ([`fnt_font`]), ahead of the same Noto fonts, which draw only what
//!   GAME.FNT lacks.
//!
//! Presentation only: nothing here reaches the simulation.

mod atlas;
pub(crate) mod fnt_font;
pub(crate) mod fonts;

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use parley::fontique::Blob;
use parley::{
    Alignment, AlignmentOptions, BreakReason, FontContext, FontFamily, FontFamilyName, FontWeight,
    FontWidth, Layout, LayoutContext, OverflowWrap, PositionedLayoutItem, StyleProperty,
};

use crate::render::batch::{BatchRenderer, BatchTexture, SpriteInstance};

pub(crate) use atlas::SUBPIXEL_STEPS;
use atlas::{GlyphAtlas, GlyphKey};
use fonts::{BASELINE, CJK_WEIGHT, LATIN_WEIGHT, LATIN_WIDTH_PERCENT, OTHER_WEIGHT, TextClass};

/// Which face interface text draws in. `[VERA] Font=` in VERA.INI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontMode {
    /// The bundled vector fonts, drawn at window resolution.
    #[default]
    Modern,
    /// Retail GAME.FNT, scaled evenly, with the bundled fonts for the
    /// characters it lacks.
    Original,
}

/// How far a text is squeezed to fit the box it draws in: narrower letters
/// first, along the bundled fonts' width axis, then smaller ones. VERA's own:
/// gamemd cuts off what does not fit, and translations run longer than the
/// English the boxes were drawn for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fit(u8);

/// Each step's width and size as shares of the text's own. Noto Sans's width
/// axis ends at 62.5, a quarter narrower than Latin text's 82.
const FIT_STEPS: [(f32, f32); 5] = [
    (1.0, 1.0),
    (72.0 / LATIN_WIDTH_PERCENT, 1.0),
    (62.5 / LATIN_WIDTH_PERCENT, 1.0),
    (62.5 / LATIN_WIDTH_PERCENT, 0.9),
    (62.5 / LATIN_WIDTH_PERCENT, 0.8),
];

impl Fit {
    /// The text as designed.
    pub const NONE: Self = Self(0);
    /// The last step, where a word too long for a line splits.
    pub const TIGHTEST: Self = Self(FIT_STEPS.len() as u8 - 1);

    /// The next step, or `None` past the tightest.
    pub fn tighter(self) -> Option<Self> {
        (self < Self::TIGHTEST).then_some(Self(self.0 + 1))
    }

    fn width(self) -> f32 {
        FIT_STEPS[usize::from(self.0)].0
    }

    /// Each width the steps narrow text to, as designed first.
    fn widths() -> impl Iterator<Item = f32> {
        let mut widths: Vec<f32> = Vec::with_capacity(FIT_STEPS.len());
        for (width, _) in FIT_STEPS {
            if !widths.contains(&width) {
                widths.push(width);
            }
        }
        widths.into_iter()
    }

    fn size(self) -> f32 {
        FIT_STEPS[usize::from(self.0)].1
    }
}

/// One positioned glyph of a shaped line, in native pixels.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShapedGlyph {
    font: u16,
    coords: u16,
    pub(crate) id: u16,
    /// Pen position from the line's left edge.
    pub(crate) x: f32,
    /// Offset from the baseline, y down.
    pub(crate) y: f32,
    pub(crate) size: f32,
    /// Byte offset of the glyph's cluster in the shaped text.
    pub(crate) cluster: u32,
    /// GAME.FNT's missing-glyph stand-in: no font has the character.
    pub(crate) missing: bool,
}

/// One line of shaped text.
#[derive(Clone, Debug, Default)]
pub(crate) struct ShapedLine {
    /// Byte range in the shaped text, without the trailing whitespace a wrap
    /// broke after.
    pub(crate) start: usize,
    pub(crate) end: usize,
    /// Advance without trailing whitespace, native pixels.
    pub(crate) width: f32,
    /// The paragraph runs right to left.
    pub(crate) rtl: bool,
    /// A word too long for the line was split at its end.
    pub(crate) split: bool,
    pub(crate) glyphs: Vec<ShapedGlyph>,
}

#[derive(Debug, Default)]
pub(crate) struct ShapedText {
    pub(crate) lines: Vec<ShapedLine>,
}

impl ShapedText {
    pub(crate) fn width(&self) -> f32 {
        self.lines.iter().map(|line| line.width).fold(0.0, f32::max)
    }
}

/// A registered font: its bytes and how its glyphs are placed and drawn.
struct FontSlot {
    data: Blob<u8>,
    index: u32,
    baseline_shift: f32,
    /// Grid-fit vertical stems. Off for GAME.FNT, whose pixel squares
    /// already sit on the grid at 1:1 and must scale evenly otherwise.
    hint: bool,
    /// The GAME.FNT font: glyph 0 is native's missing-glyph stand-in.
    original: bool,
}

/// The engine: fonts, cached layouts and the glyph atlas.
pub(crate) struct TextEngine {
    fonts: FontContext,
    layout_cx: LayoutContext<()>,
    scale_cx: swash::scale::ScaleContext,
    slots: Vec<FontSlot>,
    /// Blob id and face index to slot.
    slot_of: HashMap<(u64, u32), u16>,
    coords: Vec<Vec<i16>>,
    /// Family names: the bundled fallback order.
    modern_stack: Vec<String>,
    /// GAME.FNT's family at each width a [`Fit`] narrows to, as designed
    /// first ([`fnt_font`]).
    original_families: Vec<(f32, String)>,
    /// GAME.FNT's slot and the glyph each of its code points draws with.
    original_slot: Option<u16>,
    layouts: HashMap<(FontMode, u32, Fit), HashMap<String, Arc<ShapedText>>>,
    cached_layouts: usize,
    atlas: GlyphAtlas,
    locale: Option<String>,
    /// The interface language reads right to left (`i18n::right_to_left`).
    right_to_left: bool,
}

/// Cached layouts kept before the cache starts over.
const LAYOUT_CACHE_LIMIT: usize = 8192;

/// GAME.FNT's family for `fit` among [`TextEngine::original_families`]: the
/// face narrowed to its width.
fn original_family(families: &[(f32, String)], fit: Fit) -> Option<&str> {
    let width = fit.width();
    families
        .iter()
        .min_by(|(a, _), (b, _)| (a - width).abs().total_cmp(&(b - width).abs()))
        .map(|(_, family)| family.as_str())
}

impl TextEngine {
    pub(crate) fn new(gpu: Option<(&wgpu::Device, &wgpu::Queue, &BatchRenderer)>) -> Self {
        let mut engine = Self {
            fonts: FontContext {
                collection: parley::fontique::Collection::new(
                    parley::fontique::CollectionOptions {
                        shared: false,
                        system_fonts: false,
                    },
                ),
                source_cache: Default::default(),
            },
            layout_cx: LayoutContext::new(),
            scale_cx: swash::scale::ScaleContext::new(),
            slots: Vec::new(),
            slot_of: HashMap::new(),
            coords: vec![Vec::new()],
            modern_stack: Vec::new(),
            original_families: Vec::new(),
            original_slot: None,
            layouts: HashMap::new(),
            cached_layouts: 0,
            atlas: GlyphAtlas::new(gpu),
            locale: None,
            right_to_left: false,
        };
        for font in fonts::bundled() {
            let blob = Blob::new(Arc::new(font.data));
            for family in engine.register(blob, font.baseline_shift, true, false) {
                if !engine.modern_stack.contains(&family) {
                    engine.modern_stack.push(family);
                }
            }
        }
        engine
    }

    /// Register a font's faces; returns their family names.
    fn register(
        &mut self,
        blob: Blob<u8>,
        baseline_shift: f32,
        hint: bool,
        original: bool,
    ) -> Vec<String> {
        let registered = self.fonts.collection.register_fonts(blob.clone(), None);
        let mut families = Vec::new();
        for (family, faces) in registered {
            if let Some(name) = self.fonts.collection.family_name(family) {
                families.push(name.to_owned());
            }
            for face in faces {
                let index = face.index();
                let slot = self.slots.len() as u16;
                self.slots.push(FontSlot {
                    data: blob.clone(),
                    index,
                    baseline_shift,
                    hint,
                    original,
                });
                self.slot_of.insert((blob.id(), index), slot);
            }
        }
        families
    }

    /// Put GAME.FNT ahead of the bundled fonts for [`FontMode::Original`].
    pub(crate) fn set_original_font(&mut self, font: fnt_font::FntFont) {
        let first_slot = self.slots.len() as u16;
        self.original_families.clear();
        for (width, data) in font.faces {
            let families = self.register(Blob::new(Arc::new(data)), 0.0, false, true);
            if let Some(family) = families.into_iter().next() {
                self.original_families.push((width, family));
            }
        }
        self.original_slot = (usize::from(first_slot) < self.slots.len()).then_some(first_slot);
        self.clear_layouts();
    }

    /// The interface language, so CJK text takes its region's glyph forms
    /// (Noto Sans CJK carries Japanese, Korean and Traditional variants).
    pub(crate) fn locale(&self) -> Option<&str> {
        self.locale.as_deref()
    }

    /// Whether the interface language reads right to left.
    pub(crate) fn right_to_left(&self) -> bool {
        self.right_to_left
    }

    /// The interface language and whether it reads right to left.
    pub(crate) fn set_locale(&mut self, locale: Option<&str>, right_to_left: bool) {
        let locale = locale.map(str::to_owned);
        if self.locale != locale || self.right_to_left != right_to_left {
            self.locale = locale;
            self.right_to_left = right_to_left;
            self.clear_layouts();
        }
    }

    fn clear_layouts(&mut self) {
        self.layouts.clear();
        self.cached_layouts = 0;
    }

    pub(crate) fn atlas_texture(&self) -> Option<&BatchTexture> {
        self.atlas.texture()
    }

    pub(crate) fn begin_frame(&mut self, gpu: Option<(&wgpu::Device, &BatchRenderer)>) {
        self.atlas.begin_frame(gpu);
    }

    /// Shape `text` in `mode` at `fit`, wrapping at `max_width` native pixels
    /// (0: one line per paragraph). Cached.
    pub(crate) fn shape(
        &mut self,
        mode: FontMode,
        text: &str,
        max_width: u32,
        fit: Fit,
    ) -> Arc<ShapedText> {
        let mode = if mode == FontMode::Original && self.original_families.is_empty() {
            FontMode::Modern
        } else {
            mode
        };
        if let Some(shaped) = self
            .layouts
            .get(&(mode, max_width, fit))
            .and_then(|layouts| layouts.get(text))
        {
            return Arc::clone(shaped);
        }
        crate::render::text_audit::laid_out(text);
        let shaped = Arc::new(self.shape_uncached(mode, text, max_width, fit));
        if self.cached_layouts >= LAYOUT_CACHE_LIMIT {
            self.clear_layouts();
        }
        self.cached_layouts += 1;
        self.layouts
            .entry((mode, max_width, fit))
            .or_default()
            .insert(text.to_owned(), Arc::clone(&shaped));
        shaped
    }

    fn shape_uncached(
        &mut self,
        mode: FontMode,
        text: &str,
        max_width: u32,
        fit: Fit,
    ) -> ShapedText {
        let mut lines = Vec::new();
        for (start, end) in paragraphs(text) {
            let paragraph = &text[start..end];
            if paragraph.is_empty() {
                lines.push(ShapedLine {
                    start,
                    end,
                    ..Default::default()
                });
                continue;
            }
            if max_width == 0 && paragraph.contains('\t') {
                lines.push(self.shape_tabbed(mode, paragraph, start, fit));
            } else {
                let paragraph = paragraph.replace('\t', " ");
                self.shape_paragraph(mode, &paragraph, start, max_width, fit, &mut lines);
            }
        }
        ShapedText { lines }
    }

    /// GAME.FNT's tab stops: every [`crate::render::bit_font::TAB_WIDTH`]
    /// pixels from the line start.
    fn shape_tabbed(
        &mut self,
        mode: FontMode,
        paragraph: &str,
        start: usize,
        fit: Fit,
    ) -> ShapedLine {
        let tab = crate::render::bit_font::TAB_WIDTH as f32;
        let mut line = ShapedLine {
            start,
            end: start + paragraph.len(),
            ..Default::default()
        };
        let mut x = 0.0;
        let mut offset = start;
        for (index, segment) in paragraph.split('\t').enumerate() {
            if index > 0 {
                x = ((x / tab).floor() + 1.0) * tab;
            }
            let mut segment_lines = Vec::new();
            self.shape_paragraph(mode, segment, offset, 0, fit, &mut segment_lines);
            for segment_line in segment_lines {
                line.rtl |= segment_line.rtl;
                line.glyphs
                    .extend(segment_line.glyphs.into_iter().map(|mut glyph| {
                        glyph.x += x;
                        glyph
                    }));
                x += segment_line.width;
            }
            offset += segment.len() + 1;
        }
        line.width = x;
        line
    }

    /// Shape one paragraph (no hard breaks) into lines appended to `lines`.
    /// `base` is the paragraph's byte offset in the caller's text.
    fn shape_paragraph(
        &mut self,
        mode: FontMode,
        paragraph: &str,
        base: usize,
        max_width: u32,
        fit: Fit,
        lines: &mut Vec<ShapedLine>,
    ) {
        let mut stack: Vec<FontFamilyName<'_>> = Vec::with_capacity(self.modern_stack.len() + 1);
        if mode == FontMode::Original
            && let Some(family) = original_family(&self.original_families, fit)
        {
            stack.push(FontFamilyName::Named(Cow::Borrowed(family)));
        }
        stack.extend(
            self.modern_stack
                .iter()
                .map(|family| FontFamilyName::Named(Cow::Borrowed(family.as_str()))),
        );
        let locale = self
            .locale
            .as_deref()
            .and_then(|tag| parley::Language::parse(tag).ok());
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.fonts, paragraph, 1.0, false);
        // In a right-to-left language a paragraph with a right-to-left letter
        // reads from the right, though it may start with a name or a number;
        // others take the direction of their first letter that has one.
        if self.right_to_left && paragraph.chars().any(crate::i18n::is_right_to_left_letter) {
            builder.set_base_direction(parley::BaseDirection::Rtl);
        }
        builder.push_default(StyleProperty::FontFamily(FontFamily::List(Cow::Borrowed(
            &stack,
        ))));
        builder.push_default(StyleProperty::FontSize(
            TextClass::Latin.size() * fit.size(),
        ));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(LATIN_WEIGHT)));
        builder.push_default(StyleProperty::FontWidth(FontWidth::from_percentage(
            LATIN_WIDTH_PERCENT * fit.width(),
        )));
        builder.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
        builder.push_default(StyleProperty::Locale(locale));
        // Runs of one size class: CJK at GAME.FNT's full cell, other scripts at
        // their reading size, both at their own weight and normal width, each
        // narrowed and shrunk as `fit` asks.
        let mut run: Option<(usize, TextClass)> = None;
        let push_run = |builder: &mut parley::RangedBuilder<'_, ()>,
                        start: usize,
                        end: usize,
                        class: TextClass| {
            let weight = match class {
                TextClass::Latin => return,
                TextClass::Cjk => CJK_WEIGHT,
                TextClass::Other(_) => OTHER_WEIGHT,
            };
            builder.push(
                StyleProperty::FontSize(class.size() * fit.size()),
                start..end,
            );
            builder.push(
                StyleProperty::FontWeight(FontWeight::new(weight)),
                start..end,
            );
            builder.push(
                StyleProperty::FontWidth(FontWidth::from_percentage(100.0 * fit.width())),
                start..end,
            );
        };
        for (at, c) in paragraph.char_indices() {
            let class = TextClass::of(c);
            match run {
                Some((_, current)) if current == class => {}
                Some((run_start, current)) => {
                    push_run(&mut builder, run_start, at, current);
                    run = Some((at, class));
                }
                None => run = Some((at, class)),
            }
        }
        if let Some((run_start, current)) = run {
            push_run(&mut builder, run_start, paragraph.len(), current);
        }
        let mut layout: Layout<()> = builder.build(paragraph);
        layout.break_all_lines((max_width > 0).then_some(max_width as f32));
        layout.align(Alignment::Left, AlignmentOptions::default());
        let rtl = layout.is_rtl();

        for line in layout.lines() {
            let metrics = line.metrics();
            let range = line.text_range();
            let trimmed_end = range.start + paragraph[range.clone()].trim_end().len();
            let mut shaped = ShapedLine {
                start: base + range.start,
                end: base + trimmed_end,
                width: (metrics.advance - metrics.hanging_advance).max(0.0),
                rtl,
                split: line.break_reason() == BreakReason::Emergency,
                glyphs: Vec::new(),
            };
            // A right-to-left line hangs its trailing whitespace at the left.
            let left = if rtl { metrics.hanging_advance } else { 0.0 } + metrics.offset;
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let run = glyph_run.run();
                let font = run.font();
                let Some(&slot) = self.slot_of.get(&(font.data.id(), font.index)) else {
                    continue;
                };
                let coords = intern_coords(
                    &mut self.coords,
                    run.normalized_coords().iter().map(|coord| coord.to_bits()),
                );
                let size = run.font_size();
                let original = self.slots[usize::from(slot)].original;
                let mut x = glyph_run.offset() - left;
                for cluster in run.visual_clusters() {
                    let cluster_start = (base + cluster.text_range().start) as u32;
                    for glyph in cluster.glyphs() {
                        shaped.glyphs.push(ShapedGlyph {
                            font: slot,
                            coords,
                            id: glyph.id as u16,
                            x: x + glyph.x,
                            y: glyph.y,
                            size,
                            cluster: cluster_start,
                            missing: original && glyph.id == 0,
                        });
                        x += glyph.advance;
                    }
                }
            }
            lines.push(shaped);
        }
    }

    /// Emit `line`'s glyphs with its left edge at `origin` (device pixels,
    /// the top of the line) and every native pixel `scale` device pixels.
    /// `tint` picks each glyph's colour, or `None` to leave it out.
    pub(crate) fn emit_line(
        &mut self,
        line: &ShapedLine,
        origin: [f32; 2],
        scale: f32,
        depth: f32,
        mut tint: impl FnMut(&ShapedGlyph) -> Option<[f32; 3]>,
        out: &mut Vec<SpriteInstance>,
    ) {
        for glyph in &line.glyphs {
            let Some(color) = tint(glyph) else { continue };
            let slot = &self.slots[usize::from(glyph.font)];
            let x = origin[0] + glyph.x * scale;
            let y = origin[1] + (BASELINE + slot.baseline_shift + glyph.y) * scale;
            if let Some(instance) = self.place_glyph(
                glyph.font,
                glyph.coords,
                glyph.id,
                glyph.size * scale,
                [x, y],
                depth,
                color,
            ) {
                out.push(instance);
            }
        }
    }

    /// One glyph with its pen at `pen` (device pixels, on the baseline).
    pub(crate) fn place_glyph(
        &mut self,
        font: u16,
        coords: u16,
        id: u16,
        pixel_size: f32,
        pen: [f32; 2],
        depth: f32,
        tint: [f32; 3],
    ) -> Option<SpriteInstance> {
        let (whole_x, subpixel) = split_subpixel(pen[0]);
        let key = GlyphKey {
            font,
            coords,
            glyph: id,
            size: (pixel_size * 16.0).round() as u32,
            subpixel,
        };
        let placed = match self.atlas.get(&key) {
            Some(placed) => placed,
            None => self.rasterize(key)?,
        }?;
        Some(SpriteInstance {
            position: [
                whole_x + placed.offset[0],
                pen[1].round() + placed.offset[1],
            ],
            size: placed.size,
            uv_origin: placed.uv_origin,
            uv_size: placed.uv_size,
            depth,
            tint,
            alpha: 1.0,
            ..Default::default()
        })
    }

    /// Rasterise `key` into the atlas. `None` when the atlas is full this
    /// frame; `Some(None)` for a glyph with no pixels.
    fn rasterize(&mut self, key: GlyphKey) -> Option<Option<atlas::AtlasGlyph>> {
        match self.render_glyph(key) {
            Some(glyph) => self
                .atlas
                .insert(key, glyph.size, glyph.offset, &glyph.alpha),
            None => self.atlas.insert(key, [0, 0], [0, 0], &[]),
        }
    }

    /// `key`'s coverage bitmap and where it hangs from the pen, or `None`
    /// for a glyph with no outline.
    fn render_glyph(&mut self, key: GlyphKey) -> Option<RasterGlyph> {
        use swash::scale::{Render, Source};
        use swash::zeno::{Format, Vector};
        let slot = &self.slots[usize::from(key.font)];
        let font = swash::FontRef::from_index(slot.data.as_ref(), slot.index as usize)?;
        let mut scaler = self
            .scale_cx
            .builder(font)
            .size(key.size as f32 / 16.0)
            .hint(slot.hint)
            .normalized_coords(self.coords[usize::from(key.coords)].iter())
            .build();
        let image = Render::new(&[Source::Outline])
            .format(Format::Alpha)
            .offset(Vector::new(
                f32::from(key.subpixel) / f32::from(SUBPIXEL_STEPS),
                0.0,
            ))
            .render(&mut scaler, key.glyph)?;
        let placement = image.placement;
        Some(RasterGlyph {
            size: [placement.width, placement.height],
            offset: [placement.left, -placement.top],
            alpha: image.data,
        })
    }

    /// GAME.FNT's outline font's slot.
    #[cfg(test)]
    pub(crate) fn original_slot(&self) -> Option<u16> {
        self.original_slot
    }

    /// Draw GAME.FNT glyph `id` with its pen at `pen` (device pixels: left
    /// edge, top of the line), every font pixel `scale` device pixels.
    pub(crate) fn place_original_glyph(
        &mut self,
        id: u16,
        cjk: bool,
        pen: [f32; 2],
        scale: f32,
        depth: f32,
        tint: [f32; 3],
    ) -> Option<SpriteInstance> {
        let slot = self.original_slot?;
        let size = if cjk {
            fonts::CJK_SIZE
        } else {
            fonts::LATIN_SIZE
        };
        self.place_glyph(
            slot,
            0,
            id,
            size * scale,
            [pen[0], pen[1] + BASELINE * scale],
            depth,
            tint,
        )
    }
}

/// A rasterised glyph: `size[0] * size[1]` coverage bytes whose top-left
/// sits `offset` pixels from the pen (y down).
struct RasterGlyph {
    size: [u32; 2],
    offset: [i32; 2],
    alpha: Vec<u8>,
}

/// A device x split into its whole pixel and the nearest sub-pixel step.
fn split_subpixel(x: f32) -> (f32, u8) {
    let whole = x.floor();
    let step = ((x - whole) * f32::from(SUBPIXEL_STEPS)).round() as u8;
    if step == SUBPIXEL_STEPS {
        (whole + 1.0, 0)
    } else {
        (whole, step)
    }
}

fn intern_coords(table: &mut Vec<Vec<i16>>, coords: impl Iterator<Item = i16>) -> u16 {
    let coords: Vec<i16> = coords.collect();
    if let Some(index) = table.iter().position(|known| *known == coords) {
        return index as u16;
    }
    table.push(coords);
    (table.len() - 1) as u16
}

/// Paragraph byte ranges, split at `\r\n`, `\r` and `\n`.
fn paragraphs(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let (mut start, mut at) = (0, 0);
    while at < bytes.len() {
        match bytes[at] {
            b'\r' => {
                out.push((start, at));
                at += if bytes.get(at + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                start = at;
            }
            b'\n' => {
                out.push((start, at));
                at += 1;
                start = at;
            }
            _ => at += 1,
        }
    }
    out.push((start, text.len()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::fnt_file::FntFile;

    /// A 4-row FNT with 'A' (3 wide: `.#.`, `###`, `#.#`, `...`) on a row-3
    /// baseline, and nothing else.
    fn tiny_fnt() -> FntFile {
        const HEADER: usize = 4 + 6 * 4;
        const TABLE: usize = 65536 * 2;
        const STRIDE: usize = 1 + 4;
        let mut data = vec![0u8; HEADER + TABLE + STRIDE];
        data[..4].copy_from_slice(&0x546E_6F66u32.to_le_bytes());
        for (i, field) in [20u32, 1, 4, 5, 1, STRIDE as u32].iter().enumerate() {
            data[4 + i * 4..8 + i * 4].copy_from_slice(&field.to_le_bytes());
        }
        let off = HEADER + usize::from(b'A') * 2;
        data[off..off + 2].copy_from_slice(&1u16.to_le_bytes());
        data[HEADER + TABLE..].copy_from_slice(&[3, 0b0100_0000, 0b1110_0000, 0b1010_0000, 0]);
        FntFile::from_bytes(&data).expect("synthetic FNT parses")
    }

    fn engine_with_tiny_fnt() -> TextEngine {
        let mut engine = TextEngine::new(None);
        engine.set_original_font(fnt_font::FntFont::build(&tiny_fnt(), 3, None));
        engine
    }

    /// Alpha of an `w` x `h` canvas after drawing `text` at (2, 0): the
    /// glyph quads the engine emits, filled from its atlas texel for pixel.
    fn draw(
        engine: &mut TextEngine,
        mode: FontMode,
        text: &str,
        scale: f32,
        w: usize,
        h: usize,
    ) -> Vec<u8> {
        let shaped = engine.shape(mode, text, 0, Fit::NONE);
        let mut quads = Vec::new();
        for line in &shaped.lines {
            engine.emit_line(line, [2.0, 0.0], scale, 0.0, |_| Some([1.0; 3]), &mut quads);
        }
        let [atlas_w, atlas_h] = engine.atlas.size().map(|d| d as f32);
        let mut canvas = vec![0u8; w * h];
        for quad in &quads {
            let texel = [
                (quad.uv_origin[0] * atlas_w).round() as u32,
                (quad.uv_origin[1] * atlas_h).round() as u32,
            ];
            for row in 0..quad.size[1] as usize {
                for col in 0..quad.size[0] as usize {
                    let x = quad.position[0] as usize + col;
                    let y = quad.position[1] as usize + row;
                    let a = engine
                        .atlas
                        .alpha_at(texel[0] + col as u32, texel[1] + row as u32);
                    let dst = &mut canvas[y * w + x];
                    *dst = (u32::from(a) + u32::from(*dst) * (255 - u32::from(a)) / 255) as u8;
                }
            }
        }
        canvas
    }

    #[test]
    fn original_font_draws_game_fnt_pixels_exactly_at_1_to_1() {
        let mut engine = engine_with_tiny_fnt();
        let alpha = draw(&mut engine, FontMode::Original, "A", 1.0, 8, 16);
        // Baseline at row 13; the glyph's row 0 sits 3 rows above it.
        let lit: Vec<(usize, usize)> = (0..16)
            .flat_map(|y| (0..8).map(move |x| (x, y)))
            .filter(|&(x, y)| alpha[y * 8 + x] != 0)
            .collect();
        assert_eq!(lit, [(3, 10), (2, 11), (3, 11), (4, 11), (2, 12), (4, 12)]);
        assert!(lit.iter().all(|&(x, y)| alpha[y * 8 + x] == 255));
    }

    #[test]
    fn original_font_scales_every_pixel_evenly() {
        let mut engine = engine_with_tiny_fnt();
        // Whole scale: each font pixel is a solid 2x2 block.
        let alpha = draw(&mut engine, FontMode::Original, "A", 2.0, 16, 32);
        assert_eq!(alpha.iter().filter(|&&a| a == 255).count(), 6 * 4);
        assert_eq!(alpha.iter().filter(|&&a| a != 0 && a != 255).count(), 0);
        // Fractional scale: coverage is area, so every font pixel adds the
        // same 1.5 x 1.5 pixels of ink, with only the seams blended.
        let alpha = draw(&mut engine, FontMode::Original, "A", 1.5, 16, 32);
        let ink: f32 = alpha.iter().map(|&a| f32::from(a) / 255.0).sum();
        assert!((ink - 6.0 * 2.25).abs() < 0.1, "ink {ink}");
    }

    /// A squeeze narrows GAME.FNT's own letters by the width it narrows the
    /// bundled fonts to, before shrinking them; GAME.FNT has no width axis.
    #[test]
    fn a_squeeze_narrows_game_fnt_letters() {
        let mut engine = engine_with_tiny_fnt();
        let narrowest = Fit(2);
        let designed = engine.shape(FontMode::Original, "AAAA", 0, Fit::NONE);
        let narrowed = engine.shape(FontMode::Original, "AAAA", 0, narrowest);
        let slots = &engine.slots;
        assert!(
            narrowed.lines[0]
                .glyphs
                .iter()
                .all(|g| slots[usize::from(g.font)].original)
        );
        let share = narrowed.width() / designed.width();
        assert!((share - narrowest.width()).abs() < 0.01, "{share}");
        assert!(narrowest.width() < 0.8 && narrowest.size() == 1.0);
    }

    #[test]
    fn original_font_falls_back_for_characters_game_fnt_lacks() {
        let mut engine = engine_with_tiny_fnt();
        let shaped = engine.shape(FontMode::Original, "AБ中", 0, Fit::NONE);
        let glyphs = &shaped.lines[0].glyphs;
        assert_eq!(glyphs.len(), 3);
        let original = engine.original_slot().expect("GAME.FNT registered");
        assert_eq!(glyphs[0].font, original);
        assert!(glyphs[1..].iter().all(|g| g.font != original && g.id != 0));
        // The fallback hanzi takes GAME.FNT's full-cell size.
        assert_eq!(glyphs[2].size, fonts::CJK_SIZE);
        assert_eq!(glyphs[1].size, fonts::LATIN_SIZE);
    }

    #[test]
    fn bundled_fonts_cover_the_scripts_they_ship_for() {
        let mut engine = TextEngine::new(None);
        for text in [
            "Hello, Привет, Γειά σου, Xin chào",
            "简体中文 繁體中文 日本語のかなカナ 한국어",
            "مرحبا שלום नमस्ते হ্যালো สวัสดี வணக்கம் ສະບາຍດີ ជំរាបសួរ",
            "ಹಲೋ ഹലോ నమస్కారం ਸਤ ਸ੍ਰੀ ਅਕਾਲ નમસ્તે ନମସ୍କାର ආයුබෝවන් မင်္ဂလာပါ Բարեւ გამარჯობა ሰላም",
        ] {
            let shaped = engine.shape(FontMode::Modern, text, 0, Fit::NONE);
            let missing: Vec<u32> = shaped.lines[0]
                .glyphs
                .iter()
                .filter(|g| g.id == 0)
                .map(|g| g.cluster)
                .collect();
            assert!(missing.is_empty(), "{text}: no glyph at bytes {missing:?}");
        }
    }

    #[test]
    fn arabic_runs_right_to_left() {
        let mut engine = TextEngine::new(None);
        let shaped = engine.shape(FontMode::Modern, "مرحبا 42", 0, Fit::NONE);
        let line = &shaped.lines[0];
        assert!(line.rtl);
        let first = line
            .glyphs
            .iter()
            .find(|g| g.cluster == 0)
            .expect("first letter");
        let last_letter = line
            .glyphs
            .iter()
            .filter(|g| (g.cluster as usize) < "مرحبا".len())
            .min_by(|a, b| a.x.total_cmp(&b.x))
            .expect("letters");
        assert!(
            first.x > last_letter.x,
            "the first letter is drawn rightmost"
        );
        // The number keeps its own left-to-right order, left of the word.
        let four = line
            .glyphs
            .iter()
            .find(|g| g.cluster as usize == "مرحبا ".len())
            .unwrap();
        let two = line
            .glyphs
            .iter()
            .find(|g| g.cluster as usize == "مرحبا 4".len())
            .unwrap();
        assert!(four.x < two.x && two.x < last_letter.x);
    }

    /// In a right-to-left language a sentence that starts with a name still
    /// reads from the right; text without a right-to-left letter keeps the
    /// order of its first letter.
    #[test]
    fn a_right_to_left_language_reads_a_sentence_from_the_right() {
        let mut engine = TextEngine::new(None);
        let name_on_the_right = |engine: &mut TextEngine, text: &str| {
            let shaped = engine.shape(FontMode::Modern, text, 0, Fit::NONE);
            let line = &shaped.lines[0];
            let x = |at: usize| {
                line.glyphs
                    .iter()
                    .find(|g| g.cluster as usize == at)
                    .unwrap()
                    .x
            };
            let last = text.char_indices().last().unwrap().0;
            (line.rtl, x(0) > x(last))
        };
        let sentence = "Tanya انضم";
        assert_eq!(name_on_the_right(&mut engine, sentence), (false, false));
        engine.set_locale(Some("ar"), true);
        assert_eq!(name_on_the_right(&mut engine, sentence), (true, true));
        assert_eq!(name_on_the_right(&mut engine, "Abort?"), (false, false));
    }

    #[test]
    fn text_without_spaces_breaks_between_its_words() {
        let mut engine = TextEngine::new(None);
        // Chinese breaks between any two ideographs.
        let text = "这是一个没有空格的很长的句子";
        let shaped = engine.shape(FontMode::Modern, text, 5 * 15, Fit::NONE);
        assert!(shaped.lines.len() >= 3);
        for line in &shaped.lines {
            assert!(line.width <= 75.0, "{line:?}");
            assert!(text.is_char_boundary(line.start) && line.end > line.start);
        }
        // Thai breaks at word boundaries from the dictionary, never before a
        // combining vowel or tone mark.
        let text = "ภาษาไทยไม่มีช่องว่างระหว่างคำ";
        let shaped = engine.shape(FontMode::Modern, text, 60, Fit::NONE);
        assert!(shaped.lines.len() >= 2);
        for line in &shaped.lines[1..] {
            let first = text[line.start..].chars().next().unwrap() as u32;
            assert!(
                !matches!(first, 0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E),
                "line starts with mark U+{first:04X}"
            );
        }
    }

    #[test]
    fn wrapped_lines_report_their_text_without_the_break_space() {
        let mut engine = TextEngine::new(None);
        let text = "Pause requested - waiting for the relay";
        let shaped = engine.shape(FontMode::Modern, text, 120, Fit::NONE);
        assert!(shaped.lines.len() >= 2);
        for pair in shaped.lines.windows(2) {
            assert_eq!(&text[pair[0].end..pair[1].start], " ");
        }
        assert_eq!(shaped.lines.last().unwrap().end, text.len());
    }
}
