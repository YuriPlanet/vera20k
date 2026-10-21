//! VERA's own status-card face: a self-contained stroke font rasterised at
//! runtime into an anti-aliased atlas.
//!
//! The status cards must draw before (or without) a game installation, so they
//! cannot use GAME.FNT: that is a 1bpp bitmap whose edges are the design, so it
//! reads as pixel art above its native 16 px. Bundling a third-party font would
//! pull its licence into the tree, so the face stays in-repo: the glyphs are
//! stroke polylines in an em box (see `status_font_data.rs`) and a coverage
//! atlas is baked at the exact pixel size the card draws at.
//!
//! Legibility comes from three properties: wide inter-glyph tracking, a stroke
//! radius that never falls below one pixel, and coverage taken from the
//! distance to the stroke centre line, which anti-aliases at every size.
//!
//! Presentation only: nothing here affects the simulation.

use std::collections::HashMap;

use crate::render::batch::{BatchRenderer, BatchTexture, SpriteInstance};
use crate::render::gpu::GpuContext;
use crate::render::status_font_data::{CAP_HEIGHT_EM, Glyph};

/// Space after each glyph, in em units. Wide on purpose: a stroke face has no
/// side bearings, so this tracking is what keeps letters from running together.
const TRACKING: f32 = 0.78;
/// Descender depth in em units; the cell reserves it for `g`, `j`, `p`, `q`, `y`.
const DESCENDER_EM: f32 = 2.6;
/// Ascender headroom in em units above the cap height (`|`, brackets).
const ASCENDER_EM: f32 = 1.1;
/// Smallest stroke half-width in pixels. Thin strokes disappear on a black card
/// at small sizes; one pixel keeps them readable.
const MIN_RADIUS_PX: f32 = 0.7;
/// Word gap in em units. The skeleton's own space glyph is narrow, and a tight
/// word gap is the second reason such a face reads as one run of capitals.
const SPACE_EM: f32 = 4.6;
/// Padding around each glyph cell so a neighbour never bleeds in.
const CELL_PAD: u32 = 2;
/// Shelf width cap; a wide face wraps onto further shelves.
const ATLAS_MAX_WIDTH: u32 = 2048;

/// One rasterised glyph inside the atlas.
#[derive(Clone, Copy, Debug)]
struct Cell {
    uv_origin: [f32; 2],
    uv_size: [f32; 2],
    /// Top-left of the cell relative to the pen position and the baseline.
    offset: [f32; 2],
    /// Pen advance in pixels, tracking included.
    advance: f32,
}

/// A face baked for one cap height and one stroke weight.
pub(crate) struct StrokeFace {
    texture: BatchTexture,
    cells: HashMap<char, Cell>,
    /// Drawn for any character the face does not carry.
    missing: Cell,
    cap_px: f32,
    line_px: f32,
}

/// Identifies one baked face. Sizes are rounded so a float that wobbles by a
/// rounding error does not bake a second identical atlas.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FaceKey {
    cap_px: u32,
    weight_milli: u32,
    line_gap_milli: u32,
}

/// The card's faces, baked on demand and reused across frames.
#[derive(Default)]
pub(crate) struct StatusFaceCache {
    faces: Vec<(FaceKey, StrokeFace)>,
}

impl StatusFaceCache {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Index of the face for these metrics, baking it on first use.
    pub(crate) fn face(
        &mut self,
        gpu: &GpuContext,
        batch: &BatchRenderer,
        cap_px: f32,
        weight: f32,
        line_gap_px: f32,
    ) -> usize {
        let key = FaceKey {
            cap_px: cap_px.round().max(1.0) as u32,
            weight_milli: (weight * 1000.0).round() as u32,
            line_gap_milli: (line_gap_px * 1000.0).round() as u32,
        };
        if let Some(index) = self.faces.iter().position(|(known, _)| *known == key) {
            return index;
        }
        let baked = bake(
            key.cap_px as f32,
            key.weight_milli as f32 / 1000.0,
            key.line_gap_milli as f32 / 1000.0,
        );
        let rgba = coverage_to_rgba(&baked.coverage);
        let texture = batch.create_texture(gpu, &rgba, baked.width, baked.height);
        self.faces.push((
            key,
            StrokeFace {
                texture,
                cells: baked.cells,
                missing: baked.missing,
                cap_px: baked.cap_px,
                line_px: baked.line_px,
            },
        ));
        self.faces.len() - 1
    }

    pub(crate) fn get(&self, index: usize) -> &StrokeFace {
        &self.faces[index].1
    }
}

impl StrokeFace {
    pub(crate) fn texture(&self) -> &BatchTexture {
        &self.texture
    }

    pub(crate) fn cap_px(&self) -> f32 {
        self.cap_px
    }

    pub(crate) fn line_px(&self) -> f32 {
        self.line_px
    }

    /// Width of one line in pixels, tracking included.
    pub(crate) fn text_width(&self, text: &str) -> f32 {
        width_cells(&self.cells, &self.missing, text)
    }

    /// Emit one line with its baseline at `baseline_y`; returns the end pen x.
    /// Positions are rounded to whole pixels so the atlas samples 1:1.
    ///
    /// A character the face does not carry — a CJK folder name in a path, for
    /// instance — draws the replacement box instead of vanishing, so the shown
    /// path keeps the shape of the real one.
    pub(crate) fn push_text(
        &self,
        out: &mut Vec<SpriteInstance>,
        text: &str,
        x: f32,
        baseline_y: f32,
        tint: [f32; 3],
        depth: f32,
    ) -> f32 {
        let mut pen = x;
        let texture_w = self.texture.width as f32;
        let texture_h = self.texture.height as f32;
        for ch in text.chars() {
            let cell = self.cells.get(&ch).unwrap_or(&self.missing);
            if !ch.is_whitespace() {
                out.push(SpriteInstance {
                    position: [
                        (pen + cell.offset[0]).round(),
                        (baseline_y + cell.offset[1]).round(),
                    ],
                    size: [cell.uv_size[0] * texture_w, cell.uv_size[1] * texture_h],
                    uv_origin: cell.uv_origin,
                    uv_size: cell.uv_size,
                    depth,
                    tint,
                    alpha: 1.0,
                    ..Default::default()
                });
            }
            pen += cell.advance;
        }
        pen
    }

    /// Greedy word wrap to `max_width` pixels, honouring explicit newlines.
    /// A single word wider than the line breaks on the overflowing character.
    pub(crate) fn wrap(&self, text: &str, max_width: f32) -> Vec<String> {
        wrap_cells(&self.cells, &self.missing, text, max_width)
    }
}

fn width_cells(cells: &HashMap<char, Cell>, missing: &Cell, text: &str) -> f32 {
    text.chars()
        .map(|ch| cells.get(&ch).unwrap_or(missing).advance)
        .sum()
}

fn wrap_cells(
    cells: &HashMap<char, Cell>,
    missing: &Cell,
    text: &str,
    max_width: f32,
) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.trim().is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let trial = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if line.is_empty() || width_cells(cells, missing, &trial) <= max_width {
                line = trial;
            } else {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            }
        }
        lines.push(line);
    }
    let mut wrapped = Vec::with_capacity(lines.len());
    for line in lines {
        if width_cells(cells, missing, &line) <= max_width || max_width <= 0.0 {
            wrapped.push(line);
            continue;
        }
        let mut rest = line.as_str();
        while !rest.is_empty() {
            let mut acc = 0.0;
            let mut cut = 0;
            for (index, ch) in rest.char_indices() {
                acc += cells.get(&ch).unwrap_or(missing).advance;
                if acc > max_width && index > 0 {
                    cut = index;
                    break;
                }
            }
            if cut == 0 {
                wrapped.push(rest.to_string());
                break;
            }
            wrapped.push(rest[..cut].to_string());
            rest = rest[cut..].trim_start();
        }
    }
    wrapped
}

/// A face baked on the CPU, before its coverage is uploaded as a texture.
struct BakedFace {
    width: u32,
    height: u32,
    coverage: Vec<f32>,
    cells: HashMap<char, Cell>,
    /// Replacement cell for characters outside the face.
    missing: Cell,
    cap_px: f32,
    line_px: f32,
}

/// Bake the printable ASCII face into a coverage atlas at `cap_px` cap height.
fn bake(cap_px: f32, weight: f32, line_gap_px: f32) -> BakedFace {
    let unit = cap_px / CAP_HEIGHT_EM;
    let radius = (weight * unit / 2.0).max(MIN_RADIUS_PX);
    let cell_h = ((CAP_HEIGHT_EM + ASCENDER_EM + DESCENDER_EM) * unit).ceil() as u32 + CELL_PAD * 2;

    let mut glyphs: Vec<(char, Glyph)> = Vec::new();
    for ch in (0x20u8..0x7F).map(char::from) {
        if let Some(glyph) = crate::render::status_font_data::glyph(ch) {
            glyphs.push((ch, glyph));
        }
    }
    // The replacement box rides along in a reserved codepoint and is pulled out
    // of the cell map once baked.
    const MISSING_KEY: char = '\u{FFFD}';
    glyphs.push((MISSING_KEY, crate::render::status_font_data::MISSING));
    let overhang = radius.ceil() as u32 + CELL_PAD;
    // A space keeps its own wide advance: tracking alone leaves words touching.
    let advance_of = |ch: char, glyph: &Glyph| -> f32 {
        if ch == ' ' {
            SPACE_EM * unit
        } else {
            (glyph.advance + TRACKING) * unit
        }
    };
    let cell_w = |advance: f32| -> u32 { advance.ceil() as u32 + overhang * 2 };

    // Shelf packing: fill one row, then wrap.
    let mut shelves: Vec<Vec<(char, Glyph, u32, u32)>> = Vec::new();
    let mut shelf: Vec<(char, Glyph, u32, u32)> = Vec::new();
    let mut shelf_x = 0u32;
    for (ch, glyph) in glyphs {
        let width = cell_w(advance_of(ch, &glyph));
        if shelf_x + width > ATLAS_MAX_WIDTH && !shelf.is_empty() {
            shelves.push(std::mem::take(&mut shelf));
            shelf_x = 0;
        }
        shelf.push((ch, glyph, shelf_x, width));
        shelf_x += width;
    }
    if !shelf.is_empty() {
        shelves.push(shelf);
    }
    let atlas_w = shelves
        .iter()
        .map(|shelf| shelf.iter().map(|(_, _, _, width)| *width).sum::<u32>())
        .max()
        .unwrap_or(1)
        .max(1);
    let atlas_h = (cell_h * shelves.len().max(1) as u32).max(1);

    let mut coverage = vec![0f32; (atlas_w * atlas_h) as usize];
    let mut cells = HashMap::new();
    for (row, shelf) in shelves.into_iter().enumerate() {
        let cell_y = row as u32 * cell_h;
        // The cell top sits one ascender above the cap line.
        let baseline_in_cell = (CAP_HEIGHT_EM + ASCENDER_EM) * unit + CELL_PAD as f32;
        let baseline_y = cell_y as f32 + baseline_in_cell;
        let origin_x_shift = overhang as f32;
        for (ch, glyph, cell_x, width) in shelf {
            rasterise_glyph(
                &glyph,
                unit,
                radius,
                cell_x,
                cell_y,
                width,
                cell_h,
                atlas_w,
                baseline_y,
                origin_x_shift,
                &mut coverage,
            );
            cells.insert(
                ch,
                Cell {
                    uv_origin: [
                        cell_x as f32 / atlas_w as f32,
                        cell_y as f32 / atlas_h as f32,
                    ],
                    uv_size: [
                        width as f32 / atlas_w as f32,
                        cell_h as f32 / atlas_h as f32,
                    ],
                    offset: [-origin_x_shift, -baseline_in_cell],
                    advance: advance_of(ch, &glyph),
                },
            );
        }
    }

    let missing = cells
        .remove(&MISSING_KEY)
        .expect("the replacement glyph is always baked");

    BakedFace {
        width: atlas_w,
        height: atlas_h,
        coverage,
        cells,
        missing,
        cap_px,
        line_px: (cap_px + line_gap_px).round(),
    }
}

/// Rasterise one glyph's strokes into the shared coverage buffer. Alpha is the
/// distance to the nearest stroke centre mapped across the stroke radius: a
/// pixel fully inside a stroke is opaque, one within half a pixel of the edge
/// fades out, so the face anti-aliases at any baked size.
#[allow(clippy::too_many_arguments)]
fn rasterise_glyph(
    glyph: &Glyph,
    unit: f32,
    radius: f32,
    cell_x: u32,
    cell_y: u32,
    cell_width: u32,
    cell_height: u32,
    atlas_w: u32,
    baseline_y: f32,
    origin_x_shift: f32,
    coverage: &mut [f32],
) {
    if glyph.segments.is_empty() {
        return;
    }
    let origin_x = cell_x as f32 + origin_x_shift;
    let to_px = |segment: &[f32; 4]| -> [f32; 4] {
        [
            origin_x + segment[0] * unit,
            baseline_y - segment[1] * unit,
            origin_x + segment[2] * unit,
            baseline_y - segment[3] * unit,
        ]
    };
    let pixels: Vec<[f32; 4]> = glyph.segments.iter().map(to_px).collect();

    // Glyph bounds plus the stroke radius plus one pixel of anti-aliasing.
    let margin = radius + 1.0;
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for &[x0, y0, x1, y1] in &pixels {
        min_x = min_x.min(x0.min(x1));
        max_x = max_x.max(x0.max(x1));
        min_y = min_y.min(y0.min(y1));
        max_y = max_y.max(y0.max(y1));
    }
    let bounds = (
        (min_x - margin).floor().max(cell_x as f32) as u32,
        (min_y - margin).floor().max(cell_y as f32) as u32,
        ((max_x + margin).ceil() as i64).clamp(0, (cell_x + cell_width) as i64) as u32,
        ((max_y + margin).ceil() as i64).clamp(0, (cell_y + cell_height) as i64) as u32,
    );

    for py in bounds.1..bounds.3 {
        for px in bounds.0..bounds.2 {
            let fx = px as f32 + 0.5;
            let fy = py as f32 + 0.5;
            let mut nearest = f32::MAX;
            for [x0, y0, x1, y1] in &pixels {
                nearest = nearest.min(segment_distance(fx, fy, *x0, *y0, *x1, *y1));
            }
            // Coverage across the stroke edge, one pixel of feathering.
            let alpha = (radius + 0.5 - nearest).clamp(0.0, 1.0);
            if alpha <= 0.0 {
                continue;
            }
            let index = (py * atlas_w + px) as usize;
            if alpha > coverage[index] {
                coverage[index] = alpha;
            }
        }
    }
}

fn segment_distance(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let length_squared = dx * dx + dy * dy;
    if length_squared <= 1e-6 {
        return ((px - x0).powi(2) + (py - y0).powi(2)).sqrt();
    }
    let t = (((px - x0) * dx + (py - y0) * dy) / length_squared).clamp(0.0, 1.0);
    ((px - (x0 + t * dx)).powi(2) + (py - (y0 + t * dy)).powi(2)).sqrt()
}

/// White RGB with coverage in alpha, so callers tint by the sprite's tint.
fn coverage_to_rgba(coverage: &[f32]) -> Vec<u8> {
    let mut rgba = vec![0u8; coverage.len() * 4];
    for (index, value) in coverage.iter().enumerate() {
        let pixel = index * 4;
        rgba[pixel] = 255;
        rgba[pixel + 1] = 255;
        rgba[pixel + 2] = 255;
        rgba[pixel + 3] = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_covers_printable_ascii_at_every_used_size() {
        for (cap, weight) in [(19.0, 1.5), (13.0, 1.05), (12.0, 1.0), (11.0, 1.0)] {
            let baked = bake(cap, weight, 2.0);
            for ch in (0x20u8..0x7F).map(char::from) {
                assert!(baked.cells.contains_key(&ch), "cap {cap}: missing {ch:?}");
            }
            assert!(baked.width > 0 && baked.height > 0);
        }
    }

    #[test]
    fn strokes_are_antialiased_not_aliased() {
        let baked = bake(13.0, 1.05, 2.0);
        let partial = baked
            .coverage
            .iter()
            .filter(|value| **value > 0.05 && **value < 0.95)
            .count();
        assert!(
            partial > 0,
            "no partial coverage: the face is not anti-aliased"
        );
    }

    #[test]
    fn glyph_advances_match_the_drawn_width() {
        let baked = bake(20.0, 1.0, 2.0);
        let unit = 20.0 / CAP_HEIGHT_EM;
        for (ch, cell) in &baked.cells {
            if ch.is_whitespace() {
                continue;
            }
            let glyph = crate::render::status_font_data::glyph(*ch).expect("glyph");
            let expected = (glyph.advance + TRACKING) * unit;
            assert!((cell.advance - expected).abs() < 1e-3, "{ch:?}");
        }
    }

    #[test]
    fn tracking_keeps_letters_apart() {
        // A stroke face has no side bearings, so the tracked advance must stay
        // clear of the glyph's own width at every size the card uses.
        for cap in [11.0f32, 13.0, 19.0] {
            let baked = bake(cap, 1.05, 2.0);
            let unit = cap / CAP_HEIGHT_EM;
            let cell = baked.cells[&'n'];
            let glyph = crate::render::status_font_data::glyph('n').expect("glyph");
            assert!(
                cell.advance >= glyph.advance * unit + TRACKING * unit * 0.9,
                "cap {cap}: advance {} collapsed toward the glyph width",
                cell.advance
            );
        }
    }

    #[test]
    fn line_advance_leaves_room_between_rows() {
        let baked = bake(20.0, 1.0, 7.0);
        assert_eq!(baked.line_px, 27.0);
    }

    #[test]
    fn wrap_breaks_on_words_then_characters() {
        let baked = bake(12.0, 1.0, 2.0);
        let narrow = wrap_cells(&baked.cells, &baked.missing, "alpha beta gamma", 60.0);
        assert!(narrow.len() > 1);
        let word = wrap_cells(&baked.cells, &baked.missing, "supercalifragilistic", 40.0);
        assert!(word.len() > 1, "an over-wide word must break");
        for line in &word {
            assert!(
                width_cells(&baked.cells, &baked.missing, line) <= 40.0 + 1.0,
                "{line:?}"
            );
        }
    }

    #[test]
    fn wrap_keeps_explicit_newlines() {
        let baked = bake(12.0, 1.0, 2.0);
        let lines = wrap_cells(&baked.cells, &baked.missing, "one\ntwo\n\nthree", 400.0);
        assert_eq!(lines, vec!["one", "two", "", "three"]);
    }

    #[test]
    fn unsupported_characters_still_occupy_space() {
        // A CJK folder name in a path must draw as boxes, not vanish: the shown
        // path has to keep the shape of the path that failed.
        let baked = bake(13.0, 1.05, 2.0);
        assert!(baked.missing.advance > 0.0, "no replacement cell");
        let ascii = width_cells(&baked.cells, &baked.missing, "abc");
        let cjk = width_cells(
            &baked.cells,
            &baked.missing,
            "\\u{6e38}\\u{620f}\\u{6587}\\u{4ef6}",
        );
        assert!(cjk > 0.0);
        assert!(cjk > ascii * 0.5, "fallback collapsed: {cjk} vs {ascii}");
    }
}
