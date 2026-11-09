//! VERA's interface text engine beneath [`crate::render::bit_font::BitFont`].
//!
//! Text is shaped, bidi-ordered, line-broken and given fallback fonts by
//! `parley`, in native (800x600) pixels; each glyph is then rasterised for
//! the scale it draws at and placed one texel to one window pixel from a
//! shared atlas. Layout does not depend on the draw scale, so one cached
//! layout serves every scale.
//!
//! One font stack, all of it drawn in whole pixels as GAME.FNT is:
//! - GAME.FNT itself, as an outline font of pixel squares ([`fnt_font`]),
//!   with its missing letters drawn in its style ([`fnt_fill`]); without the
//!   game's files, those drawn letters alone.
//! - Behind it, the bundled Noto fonts ([`fonts`]) for what it lacks, such as
//!   Arabic and Thai: shaped as vector fonts, so letters join and marks
//!   stack. The browser build fetches them: Noto Sans before the game
//!   starts, the others the first time text needs one ([`web_fonts`]).
//!
//! Every glyph, GAME.FNT's own too, draws as hard pixels on the line's native
//! grid ([`pixels`]), so a glyph off that grid, squeezed narrower or shrunk
//! still lights whole pixels.
//!
//! Presentation only: nothing here reaches the simulation.

mod atlas;
mod fnt_fill;
pub(crate) mod fnt_font;
pub(crate) mod fonts;
mod pixels;
pub(crate) mod web_fonts;

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
use atlas::{GlyphAtlas, GlyphKey, SIZE_STEPS};
use fonts::{BASELINE, CJK_WEIGHT, LATIN_WEIGHT, LATIN_WIDTH_PERCENT, OTHER_WEIGHT, TextClass};

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
    /// The GAME.FNT font, whose outlines are already pixel squares on the
    /// native grid: glyph 0 is native's missing-glyph stand-in. The bundled
    /// fonts' glyphs draw through [`pixels`] instead.
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
    /// Family names in the bundled fallback order, each with its font's
    /// place in [`fonts::bundled`].
    bundled_stack: Vec<(usize, String)>,
    /// The bundled fonts this build fetches that are not registered yet
    /// ([`web_fonts`]): place and baseline shift.
    unfetched: Vec<(usize, f32)>,
    /// [`web_fonts::arrivals`] when the engine last registered what arrived.
    arrivals_seen: usize,
    /// GAME.FNT's family at each width a [`Fit`] narrows to, as designed
    /// first ([`fnt_font`]).
    original_families: Vec<(f32, String)>,
    /// GAME.FNT's slot and the glyph each of its code points draws with.
    original_slot: Option<u16>,
    layouts: HashMap<(u32, Fit), HashMap<String, Arc<ShapedText>>>,
    cached_layouts: usize,
    atlas: GlyphAtlas,
    locale: Option<String>,
    /// The interface language reads right to left (`i18n::right_to_left`).
    right_to_left: bool,
}

/// Cached layouts kept before the cache starts over.
const LAYOUT_CACHE_LIMIT: usize = 8192;

/// How many fonts the browser build has fetched ([`web_fonts`]): text drawn
/// before one arrived may draw differently after.
pub(crate) fn fetched_fonts() -> usize {
    web_fonts::arrivals()
}

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
        Self::with_fonts(gpu, fonts::bundled())
    }

    /// The engine with `bundled`'s fonts in their fallback order; one without
    /// bytes is registered once [`web_fonts`] has fetched it.
    fn with_fonts(
        gpu: Option<(&wgpu::Device, &wgpu::Queue, &BatchRenderer)>,
        bundled: impl IntoIterator<Item = fonts::BundledFont>,
    ) -> Self {
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
            bundled_stack: Vec::new(),
            unfetched: Vec::new(),
            arrivals_seen: 0,
            original_families: Vec::new(),
            original_slot: None,
            layouts: HashMap::new(),
            cached_layouts: 0,
            atlas: GlyphAtlas::new(gpu),
            locale: None,
            right_to_left: false,
        };
        for (place, font) in bundled.into_iter().enumerate() {
            match font.data {
                Some(data) => {
                    engine.add_bundled(place, Blob::new(Arc::new(data)), font.baseline_shift);
                }
                None => engine.unfetched.push((place, font.baseline_shift)),
            }
        }
        engine.add_fetched_fonts();
        engine
    }

    /// Register bundled font `place` behind the bundled fonts before it.
    fn add_bundled(&mut self, place: usize, blob: Blob<u8>, baseline_shift: f32) {
        for family in self.register(blob, baseline_shift, false) {
            if !self.bundled_stack.iter().any(|(_, known)| *known == family) {
                let at = self
                    .bundled_stack
                    .partition_point(|&(before, _)| before <= place);
                self.bundled_stack.insert(at, (place, family));
            }
        }
    }

    /// Register the waiting fonts [`web_fonts`] has fetched since the engine
    /// last looked, and lay text out again with them.
    fn add_fetched_fonts(&mut self) {
        let arrivals = web_fonts::arrivals();
        if arrivals == self.arrivals_seen {
            return;
        }
        self.arrivals_seen = arrivals;
        let waiting = self.unfetched.len();
        for (place, baseline_shift) in std::mem::take(&mut self.unfetched) {
            match web_fonts::fetched(place) {
                Some(blob) => self.add_bundled(place, blob, baseline_shift),
                None => self.unfetched.push((place, baseline_shift)),
            }
        }
        if self.unfetched.len() != waiting {
            self.clear_layouts();
        }
    }

    /// Fetch the waiting fonts `text` draws from: for each character GAME.FNT
    /// lacks, the first bundled font that maps it.
    fn fetch_fonts_for(&self, text: &str) {
        for c in text.chars() {
            if let Some(place) = web_fonts::owner(c)
                && self.unfetched.iter().any(|&(waiting, _)| waiting == place)
                && !self.original_draws(c)
            {
                web_fonts::fetch(place);
            }
        }
    }

    /// Whether GAME.FNT, first in the fallback order, draws `c`.
    fn original_draws(&self, c: char) -> bool {
        self.original_slot.is_some_and(|slot| {
            let slot = &self.slots[usize::from(slot)];
            swash::FontRef::from_index(slot.data.as_ref(), slot.index as usize)
                .is_some_and(|font| font.charmap().map(c) != 0)
        })
    }

    /// The engine without GAME.FNT (no game files yet, or a damaged
    /// archive): the letters it lacks, drawn in its style ([`fnt_fill`]),
    /// stand in for all of its own.
    pub(crate) fn without_fnt(gpu: Option<(&wgpu::Device, &wgpu::Queue, &BatchRenderer)>) -> Self {
        use crate::render::bit_font::{BITMAP_ROWS, CELL_HEIGHT};
        let mut engine = Self::new(gpu);
        engine.set_original_font(fnt_font::FntFont::build(
            &crate::assets::fnt_file::FntFile::blank(CELL_HEIGHT, BITMAP_ROWS),
            BASELINE as u32,
            None,
        ));
        engine
    }

    /// Register a font's faces; returns their family names.
    fn register(&mut self, blob: Blob<u8>, baseline_shift: f32, original: bool) -> Vec<String> {
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
                    original,
                });
                self.slot_of.insert((blob.id(), index), slot);
            }
        }
        families
    }

    /// Put GAME.FNT ahead of the bundled fonts.
    pub(crate) fn set_original_font(&mut self, font: fnt_font::FntFont) {
        let first_slot = self.slots.len() as u16;
        self.original_families.clear();
        for (width, data) in font.faces {
            let families = self.register(Blob::new(Arc::new(data)), 0.0, true);
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

    /// Shape `text` at `fit`, wrapping at `max_width` native pixels (0: one
    /// line per paragraph). Cached.
    pub(crate) fn shape(&mut self, text: &str, max_width: u32, fit: Fit) -> Arc<ShapedText> {
        if !self.unfetched.is_empty() {
            self.add_fetched_fonts();
        }
        if let Some(shaped) = self
            .layouts
            .get(&(max_width, fit))
            .and_then(|layouts| layouts.get(text))
        {
            return Arc::clone(shaped);
        }
        if !self.unfetched.is_empty() {
            self.fetch_fonts_for(text);
        }
        crate::render::text_audit::laid_out(text);
        let shaped = Arc::new(self.shape_uncached(text, max_width, fit));
        if self.cached_layouts >= LAYOUT_CACHE_LIMIT {
            self.clear_layouts();
        }
        self.cached_layouts += 1;
        self.layouts
            .entry((max_width, fit))
            .or_default()
            .insert(text.to_owned(), Arc::clone(&shaped));
        shaped
    }

    fn shape_uncached(&mut self, text: &str, max_width: u32, fit: Fit) -> ShapedText {
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
                lines.push(self.shape_tabbed(paragraph, start, fit));
            } else {
                let paragraph = paragraph.replace('\t', " ");
                self.shape_paragraph(&paragraph, start, max_width, fit, &mut lines);
            }
        }
        ShapedText { lines }
    }

    /// GAME.FNT's tab stops: every [`crate::render::bit_font::TAB_WIDTH`]
    /// pixels from the line start.
    fn shape_tabbed(&mut self, paragraph: &str, start: usize, fit: Fit) -> ShapedLine {
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
            self.shape_paragraph(segment, offset, 0, fit, &mut segment_lines);
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
        paragraph: &str,
        base: usize,
        max_width: u32,
        fit: Fit,
        lines: &mut Vec<ShapedLine>,
    ) {
        let mut stack: Vec<FontFamilyName<'_>> = Vec::with_capacity(self.bundled_stack.len() + 1);
        if let Some(family) = original_family(&self.original_families, fit) {
            stack.push(FontFamilyName::Named(Cow::Borrowed(family)));
        }
        stack.extend(
            self.bundled_stack
                .iter()
                .map(|(_, family)| FontFamilyName::Named(Cow::Borrowed(family.as_str()))),
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
            // A glyph draws on the line's native pixel grid: its pen's whole
            // native pixel places it, and the rest of the pen position goes
            // into which pixels it lights.
            let (x, phase_x) = split_phase(glyph.x);
            let (y, phase_y) = split_phase(BASELINE + slot.baseline_shift + glyph.y);
            if let Some(instance) = self.place_glyph(
                glyph.font,
                glyph.coords,
                glyph.id,
                glyph.size * scale,
                (glyph.size, phase_x | phase_y << 4),
                [origin[0] + x * scale, origin[1] + y * scale],
                depth,
                color,
            ) {
                out.push(instance);
            }
        }
    }

    /// One glyph drawn `pixel_size` pixels tall, `native` its native size and
    /// pen phase ([`split_phase`]), with `pen` (device pixels, on the
    /// baseline) at the corner of the native pixel the pen lies in.
    #[allow(clippy::too_many_arguments)]
    fn place_glyph(
        &mut self,
        font: u16,
        coords: u16,
        id: u16,
        pixel_size: f32,
        (native_size, phase): (f32, u8),
        pen: [f32; 2],
        depth: f32,
        tint: [f32; 3],
    ) -> Option<SpriteInstance> {
        let (whole_x, subpixel) = split_subpixel(pen[0]);
        let key = GlyphKey {
            font,
            coords,
            glyph: id,
            size: (pixel_size * SIZE_STEPS).round() as u32,
            subpixel,
            native: (native_size * SIZE_STEPS).round() as u32,
            phase,
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

    /// `key`'s native pixels scaled to its size ([`pixels`]) and where they
    /// hang from the pen, or `None` for a glyph with no pixels.
    fn render_glyph(&mut self, key: GlyphKey) -> Option<RasterGlyph> {
        let slot = &self.slots[usize::from(key.font)];
        let font = swash::FontRef::from_index(slot.data.as_ref(), slot.index as usize)?;
        let mut scaler = self
            .scale_cx
            .builder(font)
            .size(key.native as f32 / SIZE_STEPS)
            .normalized_coords(self.coords[usize::from(key.coords)].iter())
            .build();
        let phase = [key.phase & 0xF, key.phase >> 4].map(|p| f32::from(p) / PHASE_STEPS);
        let lit = pixels::of_outline(&scaler.scale_outline(key.glyph)?, phase);
        let subpixel = f32::from(key.subpixel) / f32::from(SUBPIXEL_STEPS);
        pixels::scaled(&lit, key.size as f32 / key.native as f32, subpixel)
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
            (size, 0),
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

/// Pen phase steps per native pixel, each way.
const PHASE_STEPS: f32 = 8.0;

/// A native coordinate split into its whole pixel and the nearest
/// [`PHASE_STEPS`] step past it.
fn split_phase(at: f32) -> (f32, u8) {
    let whole = at.floor();
    let step = ((at - whole) * PHASE_STEPS).round();
    if step == PHASE_STEPS {
        (whole + 1.0, 0)
    } else {
        (whole, step as u8)
    }
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
    fn draw(engine: &mut TextEngine, text: &str, scale: f32, w: usize, h: usize) -> Vec<u8> {
        draw_fit(engine, text, Fit::NONE, scale, w, h)
    }

    /// [`draw`] squeezed to `fit`.
    fn draw_fit(
        engine: &mut TextEngine,
        text: &str,
        fit: Fit,
        scale: f32,
        w: usize,
        h: usize,
    ) -> Vec<u8> {
        let shaped = engine.shape(text, 0, fit);
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
        let alpha = draw(&mut engine, "A", 1.0, 8, 16);
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
        let alpha = draw(&mut engine, "A", 2.0, 16, 32);
        assert_eq!(alpha.iter().filter(|&&a| a == 255).count(), 6 * 4);
        assert_eq!(alpha.iter().filter(|&&a| a != 0 && a != 255).count(), 0);
        // Fractional scale: coverage is area, so every font pixel adds the
        // same 1.5 x 1.5 pixels of ink, with only the seams blended.
        let alpha = draw(&mut engine, "A", 1.5, 16, 32);
        let ink: f32 = alpha.iter().map(|&a| f32::from(a) / 255.0).sum();
        assert!((ink - 6.0 * 2.25).abs() < 0.1, "ink {ink}");
    }

    /// A squeeze narrows GAME.FNT's own letters by the width it narrows the
    /// bundled fonts to, before shrinking them; GAME.FNT has no width axis.
    #[test]
    fn a_squeeze_narrows_game_fnt_letters() {
        let mut engine = engine_with_tiny_fnt();
        let narrowest = Fit(2);
        let designed = engine.shape("AAAA", 0, Fit::NONE);
        let narrowed = engine.shape("AAAA", 0, narrowest);
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

    /// A letter GAME.FNT lacks draws from it in its style ([`fnt_fill`]); a
    /// character with no glyph in its style falls back to the bundled fonts,
    /// such as '中', which retail GAME.FNT draws itself.
    #[test]
    fn original_font_falls_back_for_characters_game_fnt_lacks() {
        let mut engine = engine_with_tiny_fnt();
        let shaped = engine.shape("AБ中", 0, Fit::NONE);
        let glyphs = &shaped.lines[0].glyphs;
        assert_eq!(glyphs.len(), 3);
        let original = engine.original_slot().expect("GAME.FNT registered");
        assert_eq!([glyphs[0].font, glyphs[1].font], [original; 2]);
        assert!(glyphs[2].font != original && glyphs[2].id != 0);
        // The fallback hanzi takes GAME.FNT's full-cell size.
        assert_eq!(glyphs[2].size, fonts::CJK_SIZE);
        assert_eq!(glyphs[1].size, fonts::LATIN_SIZE);
    }

    /// Cyrillic and Greek letters shaped like Latin ones draw as GAME.FNT's
    /// own Latin letter, and the others as whole pixels like it, at 1:1.
    #[test]
    fn original_font_draws_letters_game_fnt_lacks_as_its_pixels() {
        let mut engine = engine_with_tiny_fnt();
        let latin = draw(&mut engine, "A", 1.0, 16, 16);
        for twin in ["А", "Α"] {
            assert_eq!(draw(&mut engine, twin, 1.0, 16, 16), latin);
        }
        let alpha = draw(&mut engine, "Б", 1.0, 16, 16);
        assert!(alpha.contains(&255) && alpha.iter().all(|&a| a == 0 || a == 255));
    }

    /// The bundled fonts' glyphs draw as GAME.FNT's do: whole pixels at 1:1,
    /// each a solid block when scaled by a whole number.
    #[test]
    fn bundled_fonts_draw_in_whole_pixels() {
        let mut engine = TextEngine::new(None);
        for text in ["مرحبا", "สวัสดี", "中"] {
            let one = draw(&mut engine, text, 1.0, 64, 24);
            assert!(one.contains(&255), "{text}");
            assert!(one.iter().all(|&a| a == 0 || a == 255), "{text}");
            let two = draw(&mut engine, text, 2.0, 128, 48);
            assert!(two.iter().all(|&a| a == 0 || a == 255), "{text}");
            let ink = |alpha: &[u8]| alpha.iter().filter(|&&a| a == 255).count();
            assert_eq!(ink(&two), 4 * ink(&one), "{text}");
        }
    }

    /// GAME.FNT's own letters keep whole pixels off its grid: squeezed
    /// narrower or smaller, or after a bundled glyph's fractional advance.
    #[test]
    fn game_fnt_letters_stay_whole_pixels_off_the_grid() {
        let mut engine = engine_with_tiny_fnt();
        let after_thai = engine.shape("กA", 0, Fit::NONE);
        assert!(after_thai.lines[0].glyphs[1].x.fract() != 0.0);
        for (text, fit) in [("AAAA", Fit(2)), ("AAAA", Fit::TIGHTEST), ("กA", Fit::NONE)] {
            let alpha = draw_fit(&mut engine, text, fit, 1.0, 64, 16);
            assert!(alpha.contains(&255), "{text} {fit:?}");
            assert!(alpha.iter().all(|&a| a == 0 || a == 255), "{text} {fit:?}");
        }
    }

    /// Without the game's files, text still draws in GAME.FNT's style: every
    /// letter as [`fnt_fill`] draws the ones it lacks, in whole pixels.
    #[test]
    fn without_game_fnt_text_draws_in_its_style() {
        let mut engine = TextEngine::without_fnt(None);
        let text = "Import the game files: 1.001";
        let original = engine.original_slot().expect("drawn letters registered");
        let shaped = engine.shape(text, 0, Fit::NONE);
        let glyphs = &shaped.lines[0].glyphs;
        assert!(glyphs.iter().all(|g| g.font == original && g.id != 0));
        let alpha = draw(&mut engine, text, 1.0, 200, 24);
        assert!(alpha.contains(&255) && alpha.iter().all(|&a| a == 0 || a == 255));
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
            let shaped = engine.shape(text, 0, Fit::NONE);
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
        let shaped = engine.shape("مرحبا 42", 0, Fit::NONE);
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
            let shaped = engine.shape(text, 0, Fit::NONE);
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
        let shaped = engine.shape(text, 5 * 15, Fit::NONE);
        assert!(shaped.lines.len() >= 3);
        for line in &shaped.lines {
            assert!(line.width <= 75.0, "{line:?}");
            assert!(text.is_char_boundary(line.start) && line.end > line.start);
        }
        // Thai has no word dictionary (Cargo.toml's parley): a run breaks
        // where its line runs out, never before a combining vowel or tone
        // mark.
        let text = "ภาษาไทยไม่มีช่องว่างระหว่างคำ";
        let shaped = engine.shape(text, 60, Fit::NONE);
        assert!(shaped.lines.len() >= 2);
        for line in &shaped.lines {
            assert!(line.width <= 60.0, "{line:?}");
        }
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
        let shaped = engine.shape(text, 120, Fit::NONE);
        assert!(shaped.lines.len() >= 2);
        for pair in shaped.lines.windows(2) {
            assert_eq!(&text[pair[0].end..pair[1].start], " ");
        }
        assert_eq!(shaped.lines.last().unwrap().end, text.len());
    }

    /// The bundled fonts with `file`'s bytes held back, as the browser build
    /// holds them, and its place and bytes.
    fn waiting_for(file: &str) -> ([fonts::BundledFont; 21], usize, &'static [u8]) {
        let mut bundled = fonts::bundled();
        let place = bundled.iter().position(|font| font.file == file).unwrap();
        let data = bundled[place].data.take().unwrap();
        (bundled, place, data)
    }

    /// A font fetched for text draws it once it arrives, in its place in the
    /// fallback order.
    #[test]
    fn a_fetched_font_draws_the_text_that_asked_for_it() {
        let (bundled, thai, data) = waiting_for("NotoSansThai-VF.ttf");
        let mut engine = TextEngine::with_fonts(None, bundled);
        let text = "สวัสดี";
        let before = engine.shape(text, 0, Fit::NONE);
        assert!(web_fonts::asked_for(thai));
        assert!(before.lines[0].glyphs.iter().all(|glyph| glyph.id == 0));

        web_fonts::arrive_for_test(thai, data.to_vec());
        let after = engine.shape(text, 0, Fit::NONE);
        assert!(after.lines[0].glyphs.iter().all(|glyph| glyph.id != 0));
        let places: Vec<usize> = engine
            .bundled_stack
            .iter()
            .map(|&(place, _)| place)
            .collect();
        assert!(places.contains(&thai) && places.is_sorted(), "{places:?}");
    }

    /// Hanzi GAME.FNT draws, its own or drawn in its style, fetch no font.
    #[test]
    fn characters_game_fnt_draws_fetch_nothing() {
        use crate::render::bit_font::{BITMAP_ROWS, CELL_HEIGHT};
        let (bundled, cjk, _) = waiting_for("NotoSansCJKsc-Regular.otf");
        let mut engine = TextEngine::with_fonts(None, bundled);
        engine.set_original_font(fnt_font::FntFont::build(
            &FntFile::blank(CELL_HEIGHT, BITMAP_ROWS),
            BASELINE as u32,
            None,
        ));
        // Retail GAME.FNT lacks 语, which VERA draws in its style.
        engine.shape("语", 0, Fit::NONE);
        assert!(!web_fonts::asked_for(cjk));
        engine.shape("的", 0, Fit::NONE);
        assert!(web_fonts::asked_for(cjk));
    }
}
