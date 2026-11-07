//! GAME.FNT as an in-memory TrueType font, so interface text shapes,
//! falls back and rasterises through the same path as the bundled fonts.
//!
//! Every lit pixel becomes part of a square outline. Drawn as the native
//! pixels whose centres those squares cover ([`super::pixels`]), on whole
//! pixels that is the retail bitmap, which then scales evenly: every font
//! pixel the same block of window pixels, only its edges blended at a
//! fractional scale. Each glyph advances by its width plus GAME.FNT's
//! 1-pixel character spacing.
//!
//! Units: `UNITS_PER_EM` makes a font pixel a whole number of units at both
//! sizes the engine asks for, 400 at the Latin size and 336 at the CJK size
//! ([`super::fonts`]), so the full-cell CJK glyphs draw 1:1 beside Latin ones.
//! Characters GAME.FNT lacks (no glyph, or one of its shared placeholders)
//! take [`super::fnt_fill`]'s glyphs in its style where it has one, and
//! otherwise fall back to the bundled fonts; glyph 0 is native's
//! missing-glyph stand-in.
//!
//! GAME.FNT has no width axis, so it is also built narrowed to each width a
//! squeeze asks for ([`super::Fit`]), one family per width: text too long for
//! its box narrows in GAME.FNT as it does in the bundled fonts, instead of
//! only shrinking.

use crate::assets::fnt_file::{FntFile, FntGlyph};

use super::fonts::{CJK_SIZE, LATIN_SIZE, is_cjk_sized};

/// 315 * 16: a multiple of both 12.6 and 15.
const UNITS_PER_EM: u16 = 5040;
const FAMILY_NAME: &str = "VERA GAME.FNT";

/// Font units per GAME.FNT pixel for a code point drawn at its text class's
/// size.
fn units_per_pixel(codepoint: u32) -> i32 {
    let size = if is_cjk_sized(codepoint) {
        CJK_SIZE
    } else {
        LATIN_SIZE
    };
    (f32::from(UNITS_PER_EM) / size).round() as i32
}

/// The family a face built `width` times as wide as designed registers
/// under: [`FAMILY_NAME`] as designed.
fn family_name(width: f32) -> String {
    if width == 1.0 {
        FAMILY_NAME.to_owned()
    } else {
        format!("{FAMILY_NAME} {}", (width * 100.0).round())
    }
}

/// GAME.FNT's TrueType faces, as designed and narrowed.
pub(crate) struct FntFont {
    /// Each face's width, as a share of the designed one, and its bytes: as
    /// designed first, then each width a [`super::Fit`] step narrows to.
    pub(crate) faces: Vec<(f32, Vec<u8>)>,
    /// The glyph each mapped code point draws with, in every face.
    pub(crate) glyph_ids: std::collections::HashMap<u16, u16>,
}

impl FntFont {
    /// `baseline_row`: the first bitmap row below the baseline.
    /// `missing`: native's missing-glyph stand-in, drawn for glyph 0.
    pub(crate) fn build(fnt: &FntFile, baseline_row: u32, missing: Option<&FntGlyph>) -> Self {
        let fill_ins = super::fnt_fill::glyphs(fnt, baseline_row);
        let mut codepoints: Vec<(u16, &FntGlyph)> = fnt
            .drawn_codepoints()
            .filter(|&(cp, _)| cp >= 0x20)
            .chain(fill_ins.iter().map(|(cp, glyph)| (*cp, glyph)))
            .collect();
        codepoints.sort_unstable_by_key(|&(cp, _)| cp);

        let rows = fnt.bitmap_rows;
        // Every width narrows the same rectangles, so each glyph's pixels
        // merge once.
        let mut scratch = MergeScratch::default();
        let stand_in_px = units_per_pixel(0x58);
        let stand_in =
            missing.map(|glyph| PixelGlyph::merge(glyph, rows, stand_in_px, &mut scratch));
        let pixels: Vec<PixelGlyph> = codepoints
            .iter()
            .map(|&(cp, glyph)| {
                PixelGlyph::merge(glyph, rows, units_per_pixel(u32::from(cp)), &mut scratch)
            })
            .collect();
        let outlines = |width: f32| {
            let mut glyphs: Vec<GlyphOutline> = Vec::with_capacity(pixels.len() + 1);
            glyphs.push(match &stand_in {
                Some(glyph) => glyph.outline(baseline_row, width),
                None => GlyphOutline::empty((5.0 * stand_in_px as f32 * width).round() as i32),
            });
            glyphs.extend(
                pixels
                    .iter()
                    .map(|glyph| glyph.outline(baseline_row, width)),
            );
            glyphs
        };

        // Format-12 groups: runs of consecutive code points on consecutive
        // glyph ids (glyph i + 1 is the i-th code point).
        let mut groups: Vec<[u32; 3]> = Vec::new();
        for (index, &(cp, _)) in codepoints.iter().enumerate() {
            let gid = index as u32 + 1;
            match groups.last_mut() {
                Some(group)
                    if group[1] + 1 == u32::from(cp)
                        && group[2] + (group[1] - group[0]) + 1 == gid =>
                {
                    group[1] = u32::from(cp);
                }
                _ => groups.push([u32::from(cp), u32::from(cp), gid]),
            }
        }

        let latin_px = units_per_pixel(0x41);
        let ascent = baseline_row as i32 * latin_px;
        let descent = (rows as i32 - baseline_row as i32) * latin_px;
        let line_gap = (fnt.cell_height as i32 - rows as i32).max(0) * latin_px;
        let faces = super::Fit::widths()
            .map(|width| {
                let glyphs = outlines(width);
                let family = family_name(width);
                let writer = Writer::new(&glyphs, &groups, ascent, descent, line_gap, &family);
                (width, writer.finish())
            })
            .collect();
        let glyph_ids = codepoints
            .iter()
            .enumerate()
            .map(|(index, &(cp, _))| (cp, index as u16 + 1))
            .collect();
        Self { faces, glyph_ids }
    }
}

/// One glyph's lit pixels merged into rectangles: horizontal runs per row,
/// and a run that repeats exactly on the next row extends downwards.
struct PixelGlyph {
    /// Font units per pixel.
    px: i32,
    /// Bitmap width in pixels.
    width: usize,
    /// `[x0, x1, top row, row below]` in pixels, in the order they close.
    rects: Vec<[usize; 4]>,
}

/// [`PixelGlyph::merge`]'s row buffers, kept from glyph to glyph.
#[derive(Default)]
struct MergeScratch {
    /// Rectangles still growing downwards: (x0, x1, first row).
    open: Vec<(usize, usize, usize)>,
    /// The open rectangles for the next row.
    next_open: Vec<(usize, usize, usize)>,
    /// One row's runs of lit pixels: (x0, x1).
    runs: Vec<(usize, usize)>,
}

impl PixelGlyph {
    fn merge(glyph: &FntGlyph, rows: u32, px: i32, scratch: &mut MergeScratch) -> Self {
        let width = glyph.width as usize;
        let lit = |x: usize, y: usize| {
            glyph
                .rgba
                .get((y * width + x) * 4 + 3)
                .is_some_and(|&alpha| alpha != 0)
        };
        let MergeScratch {
            open,
            next_open,
            runs,
        } = scratch;
        open.clear();
        let mut rects = Vec::new();
        for y in 0..=rows as usize {
            runs.clear();
            if y < rows as usize {
                let mut x = 0;
                while x < width {
                    if lit(x, y) {
                        let start = x;
                        while x < width && lit(x, y) {
                            x += 1;
                        }
                        runs.push((start, x));
                    } else {
                        x += 1;
                    }
                }
            }
            next_open.clear();
            for &(x0, x1, top) in open.iter() {
                if let Some(at) = runs.iter().position(|&run| run == (x0, x1)) {
                    runs.remove(at);
                    next_open.push((x0, x1, top));
                } else {
                    rects.push([x0, x1, top, y]);
                }
            }
            next_open.extend(runs.iter().map(|&(x0, x1)| (x0, x1, y)));
            std::mem::swap(open, next_open);
        }
        Self { px, width, rects }
    }

    /// The glyph in font units, columns `narrow` times as wide.
    fn outline(&self, baseline_row: u32, narrow: f32) -> GlyphOutline {
        let px = self.px;
        let column = |x: usize| (x as f32 * px as f32 * narrow).round() as i32;
        let row = |y: usize| (baseline_row as i32 - y as i32) * px;
        let rects = self
            .rects
            .iter()
            .map(|&[x0, x1, top, bottom]| [column(x0), row(bottom), column(x1), row(top)])
            .collect();
        GlyphOutline::empty(column(self.width + 1)).with_rects(rects)
    }
}

/// One glyph's rectangles in font units, y up, plus its advance.
struct GlyphOutline {
    advance: u16,
    /// [x0, y0, x1, y1] with x0 < x1, y0 < y1.
    rects: Vec<[i32; 4]>,
}

impl GlyphOutline {
    fn empty(advance: i32) -> Self {
        Self {
            advance: advance.clamp(0, i32::from(u16::MAX)) as u16,
            rects: Vec::new(),
        }
    }

    fn with_rects(mut self, rects: Vec<[i32; 4]>) -> Self {
        self.rects = rects;
        self
    }

    fn bounds(&self) -> [i32; 4] {
        self.rects
            .iter()
            .fold([i32::MAX, i32::MAX, i32::MIN, i32::MIN], |b, r| {
                [
                    b[0].min(r[0]),
                    b[1].min(r[1]),
                    b[2].max(r[2]),
                    b[3].max(r[3]),
                ]
            })
    }

    /// The `glyf` record: one clockwise four-point contour per rectangle.
    /// `xs`, `ys`: buffers for the coordinates, kept from glyph to glyph.
    fn encode(&self, out: &mut Vec<u8>, xs: &mut Vec<u8>, ys: &mut Vec<u8>) {
        if self.rects.is_empty() {
            return;
        }
        let [x_min, y_min, x_max, y_max] = self.bounds();
        push_i16(out, self.rects.len() as i32);
        for v in [x_min, y_min, x_max, y_max] {
            push_i16(out, v);
        }
        for i in 0..self.rects.len() {
            push_u16(out, (i * 4 + 3) as u32);
        }
        push_u16(out, 0); // no instructions
        const ON_CURVE: u8 = 0x01;
        const X_SAME: u8 = 0x10;
        const Y_SAME: u8 = 0x20;
        let points = self
            .rects
            .iter()
            .flat_map(|&[x0, y0, x1, y1]| [[x0, y1], [x1, y1], [x1, y0], [x0, y0]]);
        let mut previous = [0, 0];
        xs.clear();
        ys.clear();
        // The flags come first, so they go straight out.
        for point in points {
            let (dx, dy) = (point[0] - previous[0], point[1] - previous[1]);
            let mut flag = ON_CURVE;
            if dx == 0 {
                flag |= X_SAME;
            } else {
                push_i16(xs, dx);
            }
            if dy == 0 {
                flag |= Y_SAME;
            } else {
                push_i16(ys, dy);
            }
            out.push(flag);
            previous = point;
        }
        out.extend_from_slice(xs);
        out.extend_from_slice(ys);
    }
}

fn push_u16(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&(v as u16).to_be_bytes());
}
fn push_i16(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&(v as i16).to_be_bytes());
}
fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

/// The sfnt tables, assembled in tag order.
struct Writer {
    tables: Vec<([u8; 4], Vec<u8>)>,
}

impl Writer {
    fn new(
        glyphs: &[GlyphOutline],
        groups: &[[u32; 3]],
        ascent: i32,
        descent: i32,
        line_gap: i32,
        family: &str,
    ) -> Self {
        let mut glyf = Vec::new();
        let mut loca = Vec::with_capacity((glyphs.len() + 1) * 4);
        let (mut max_points, mut max_contours) = (0u32, 0u32);
        let mut bbox = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        let mut hmtx = Vec::with_capacity(glyphs.len() * 4);
        let mut advance_max = 0u16;
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        for glyph in glyphs {
            push_u32(&mut loca, glyf.len() as u32);
            glyph.encode(&mut glyf, &mut xs, &mut ys);
            while glyf.len() % 4 != 0 {
                glyf.push(0);
            }
            let lsb = if glyph.rects.is_empty() {
                0
            } else {
                let b = glyph.bounds();
                bbox = [
                    bbox[0].min(b[0]),
                    bbox[1].min(b[1]),
                    bbox[2].max(b[2]),
                    bbox[3].max(b[3]),
                ];
                b[0]
            };
            max_points = max_points.max(glyph.rects.len() as u32 * 4);
            max_contours = max_contours.max(glyph.rects.len() as u32);
            advance_max = advance_max.max(glyph.advance);
            push_u16(&mut hmtx, u32::from(glyph.advance));
            push_i16(&mut hmtx, lsb);
        }
        push_u32(&mut loca, glyf.len() as u32);
        if bbox[0] > bbox[2] {
            bbox = [0, 0, 0, 0];
        }
        let num_glyphs = glyphs.len() as u32;

        let mut head = Vec::with_capacity(54);
        push_u32(&mut head, 0x0001_0000);
        push_u32(&mut head, 0x0001_0000);
        push_u32(&mut head, 0); // checksum adjustment: unchecked by readers
        push_u32(&mut head, 0x5F0F_3CF5);
        push_u16(&mut head, 0x000B); // baseline at y=0, lsb at x=0, integer ppem
        push_u16(&mut head, u32::from(UNITS_PER_EM));
        head.extend_from_slice(&[0; 16]); // created, modified
        for v in bbox {
            push_i16(&mut head, v);
        }
        push_u16(&mut head, 0); // macStyle
        push_u16(&mut head, 8); // lowestRecPPEM
        push_i16(&mut head, 2); // fontDirectionHint
        push_i16(&mut head, 1); // long loca
        push_i16(&mut head, 0);

        let mut hhea = Vec::with_capacity(36);
        push_u32(&mut hhea, 0x0001_0000);
        push_i16(&mut hhea, ascent);
        push_i16(&mut hhea, -descent);
        push_i16(&mut hhea, line_gap);
        push_u16(&mut hhea, u32::from(advance_max));
        push_i16(&mut hhea, bbox[0].min(0));
        push_i16(&mut hhea, 0);
        push_i16(&mut hhea, bbox[2]);
        push_i16(&mut hhea, 1); // caret slope rise
        push_i16(&mut hhea, 0);
        push_i16(&mut hhea, 0);
        hhea.extend_from_slice(&[0; 8]);
        push_i16(&mut hhea, 0);
        push_u16(&mut hhea, num_glyphs);

        let mut maxp = Vec::with_capacity(32);
        push_u32(&mut maxp, 0x0001_0000);
        push_u16(&mut maxp, num_glyphs);
        push_u16(&mut maxp, max_points);
        push_u16(&mut maxp, max_contours);
        push_u16(&mut maxp, 0);
        push_u16(&mut maxp, 0);
        push_u16(&mut maxp, 2); // maxZones
        maxp.extend_from_slice(&[0; 18]);

        let latin_px = units_per_pixel(0x41);
        let mut os2 = Vec::with_capacity(96);
        push_u16(&mut os2, 4);
        push_i16(&mut os2, 6 * latin_px); // xAvgCharWidth
        push_u16(&mut os2, 400); // weight: the engine asks GAME.FNT for nothing else
        push_u16(&mut os2, 5); // width: normal
        push_u16(&mut os2, 0); // fsType: installable
        os2.extend_from_slice(&[0; 20]); // sub/superscript and strikeout metrics
        push_i16(&mut os2, 0); // family class
        os2.extend_from_slice(&[0; 10]); // panose
        os2.extend_from_slice(&[0; 16]); // unicode ranges
        os2.extend_from_slice(b"VERA");
        push_u16(&mut os2, 0x00C0); // REGULAR | USE_TYPO_METRICS
        let first = groups.first().map_or(0x20, |g| g[0]).min(0xFFFF);
        let last = groups.last().map_or(0x20, |g| g[1]).min(0xFFFF);
        push_u16(&mut os2, first);
        push_u16(&mut os2, last);
        push_i16(&mut os2, ascent);
        push_i16(&mut os2, -descent);
        push_i16(&mut os2, line_gap);
        push_u16(&mut os2, ascent as u32);
        push_u16(&mut os2, descent as u32);
        os2.extend_from_slice(&[0; 8]); // code page ranges
        push_i16(&mut os2, 7 * latin_px); // x height: GAME.FNT 'x' rows 6..12
        push_i16(&mut os2, 9 * latin_px); // cap height: 'H' rows 4..12
        push_u16(&mut os2, 0);
        push_u16(&mut os2, 0x20);
        push_u16(&mut os2, 0);

        let mut cmap = Vec::new();
        push_u16(&mut cmap, 0);
        push_u16(&mut cmap, 1);
        push_u16(&mut cmap, 3); // Windows
        push_u16(&mut cmap, 10); // full Unicode
        push_u32(&mut cmap, 12);
        push_u16(&mut cmap, 12);
        push_u16(&mut cmap, 0);
        push_u32(&mut cmap, 16 + groups.len() as u32 * 12);
        push_u32(&mut cmap, 0);
        push_u32(&mut cmap, groups.len() as u32);
        for group in groups {
            for v in group {
                push_u32(&mut cmap, *v);
            }
        }

        let mut post = Vec::with_capacity(32);
        push_u32(&mut post, 0x0003_0000);
        post.extend_from_slice(&[0; 28]);

        let postscript = family
            .replacen(FAMILY_NAME, "VERA-GAMEFNT", 1)
            .replace(' ', "-");
        let names = [
            (1, family),
            (2, "Regular"),
            (4, family),
            (6, postscript.as_str()),
        ];
        let mut name = Vec::new();
        push_u16(&mut name, 0);
        push_u16(&mut name, names.len() as u32);
        push_u16(&mut name, 6 + names.len() as u32 * 12);
        let mut strings = Vec::new();
        for (id, text) in names {
            let encoded: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
            for v in [3, 1, 0x0409, id, encoded.len() as u32, strings.len() as u32] {
                push_u16(&mut name, v);
            }
            strings.extend_from_slice(&encoded);
        }
        name.extend_from_slice(&strings);

        Self {
            tables: vec![
                (*b"OS/2", os2),
                (*b"cmap", cmap),
                (*b"glyf", glyf),
                (*b"head", head),
                (*b"hhea", hhea),
                (*b"hmtx", hmtx),
                (*b"loca", loca),
                (*b"maxp", maxp),
                (*b"name", name),
                (*b"post", post),
            ],
        }
    }

    fn finish(self) -> Vec<u8> {
        let count = self.tables.len() as u32;
        let entry_selector = 31 - count.leading_zeros();
        let search_range = (1u32 << entry_selector) * 16;
        let tables_len: usize = self
            .tables
            .iter()
            .map(|(_, data)| data.len().next_multiple_of(4))
            .sum();
        let mut out = Vec::with_capacity(12 + self.tables.len() * 16 + tables_len);
        push_u32(&mut out, 0x0001_0000);
        push_u16(&mut out, count);
        push_u16(&mut out, search_range);
        push_u16(&mut out, entry_selector);
        push_u16(&mut out, count * 16 - search_range);
        let mut offset = 12 + count * 16;
        for (tag, data) in &self.tables {
            out.extend_from_slice(tag);
            push_u32(&mut out, checksum(data));
            push_u32(&mut out, offset);
            push_u32(&mut out, data.len() as u32);
            offset += data.len().next_multiple_of(4) as u32;
        }
        for (_, data) in &self.tables {
            out.extend_from_slice(data);
            while out.len() % 4 != 0 {
                out.push(0);
            }
        }
        out
    }
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}
