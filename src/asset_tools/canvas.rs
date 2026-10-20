//! CPU-only RGBA canvas primitives for the headless asset browser.
//!
//! This is the presentation layer that turns decoded asset pixels into PNGs a
//! human or an agent can actually look at: alpha-correct compositing, integer
//! magnification (retail sprites are far too small to inspect at 1:1), simple
//! annotation shapes, burned-in 5x7 labels, and contact-sheet layout.
//!
//! Dependencies: `image` (PNG encode only) and `crate::render::bit_font` for the
//! shared 5x7 glyph table. Deliberately has **no** dependency on wgpu or
//! `GpuContext` — every function here runs headless and is unit testable with
//! synthetic buffers, so the CLI works over SSH and in CI without a GPU.
//!
//! Everything is bounds-safe by construction: a malformed retail asset can
//! produce absurd dimensions or short pixel runs, and none of that may panic.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use image::RgbaImage;

use crate::render::bit_font::fallback_5x7_glyphs;

/// Side length of one checkerboard square, in pixels.
pub const CHECKER_SIZE: u32 = 8;
/// Border left around the whole contact sheet.
pub const SHEET_PADDING: u32 = 4;
/// Space between adjacent contact-sheet cells.
pub const SHEET_GAP: u32 = 4;
/// Sheets wrap to a new row rather than exceed this width, so a viewer never
/// has to scroll horizontally through a hundred-frame animation.
pub const MAX_SHEET_WIDTH: u32 = 2048;

/// Width of one glyph cell in the shared 5x7 bitmap font.
pub const GLYPH_W: u32 = 5;
/// Height of one glyph cell in the shared 5x7 bitmap font.
pub const GLYPH_H: u32 = 7;

/// Vertical room a burned-in cell label occupies: the glyph box plus leading.
pub const LABEL_HEIGHT: u32 = GLYPH_H + LABEL_LEADING;

/// Lower bound for the long edge after magnification.
pub const MIN_LONG_EDGE: u32 = 256;
/// Upper bound for the long edge after magnification.
pub const MAX_LONG_EDGE: u32 = 1024;

/// Blank columns between glyphs. One pixel is enough at 5x7 to keep `AB` legible.
const GLYPH_SPACING: u32 = 1;
/// Pen movement per character, glyph included.
const GLYPH_ADVANCE: u32 = GLYPH_W + GLYPH_SPACING;
/// Blank rows beneath a label's glyph box.
const LABEL_LEADING: u32 = 3;
/// Gap between a cell's artwork and the first row of its label.
const LABEL_TOP_GAP: u32 = 1;

const CHECKER_LIGHT: [u8; 4] = [32, 32, 32, 255];
const CHECKER_DARK: [u8; 4] = [20, 20, 20, 255];
/// Faint frame drawn just outside each cell's artwork so frame boundaries stay
/// readable when a sprite's own edges are transparent.
const CELL_BORDER: [u8; 4] = [56, 56, 56, 255];
const HEADER_COLOR: [u8; 4] = [255, 255, 255, 255];
const LABEL_COLOR: [u8; 4] = [200, 200, 200, 255];

/// Refuse to allocate a single buffer larger than this. A corrupt header can
/// claim 65535x65535; that is 17 GB of RGBA and an instant OOM abort, which is
/// indistinguishable from a crash to the caller.
const MAX_IMAGE_BYTES: u64 = 512 * 1024 * 1024;

/// The '#' marker in the shared glyph table means "set this pixel".
const GLYPH_SET: u8 = b'#';

/// An RGBA8 image buffer with its dimensions.
///
/// `data` is always exactly `w * h * 4` bytes for any instance produced by this
/// module; [`Rgba::from_raw`] is the only constructor that accepts foreign
/// buffers and it validates that invariant.
#[derive(Clone, Default)]
pub struct Rgba {
    pub data: Vec<u8>,
    pub w: u32,
    pub h: u32,
}

impl std::fmt::Debug for Rgba {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never dump the pixel vector — a single frame is megabytes of noise.
        f.debug_struct("Rgba")
            .field("w", &self.w)
            .field("h", &self.h)
            .field("bytes", &self.data.len())
            .finish()
    }
}

impl Rgba {
    /// Allocate a `w` x `h` buffer with every pixel set to `color`.
    ///
    /// Returns an empty 0x0 image if the requested allocation is implausible,
    /// so a corrupt asset header degrades to "nothing rendered" rather than an
    /// out-of-memory abort.
    pub fn new_filled(w: u32, h: u32, color: [u8; 4]) -> Self {
        let Some(len) = checked_rgba_len(w, h) else {
            log::warn!("canvas: refusing {w}x{h} allocation, exceeds the buffer cap");
            return Self::default();
        };
        let mut data = vec![0u8; len];
        for pixel in data.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&color);
        }
        Self { data, w, h }
    }

    /// Opaque two-tone checkerboard, the standard backdrop for sprites whose
    /// own transparent regions would otherwise be invisible against black.
    pub fn checkerboard(w: u32, h: u32) -> Self {
        // Named `board`, not `image`, to avoid shadowing the `image` crate.
        let mut board = Self::new_filled(w, h, CHECKER_LIGHT);
        if board.w == 0 || board.h == 0 {
            return board;
        }
        for y in 0..board.h {
            for x in 0..board.w {
                let light = (x / CHECKER_SIZE + y / CHECKER_SIZE).is_multiple_of(2);
                let color = if light { CHECKER_LIGHT } else { CHECKER_DARK };
                let idx = (y as usize * board.w as usize + x as usize) * 4;
                board.data[idx..idx + 4].copy_from_slice(&color);
            }
        }
        board
    }

    /// Wrap an existing buffer. `None` when `data.len() != w * h * 4`.
    pub fn from_raw(data: Vec<u8>, w: u32, h: u32) -> Option<Self> {
        let len = checked_rgba_len(w, h)?;
        if data.len() != len {
            return None;
        }
        Some(Self { data, w, h })
    }

    pub fn pixel_count(&self) -> usize {
        self.w as usize * self.h as usize
    }

    /// Source-over blend of `color` onto one pixel. Out-of-range coordinates are
    /// dropped silently — every drawing helper here relies on that for clipping.
    fn blend_pixel(&mut self, x: i64, y: i64, color: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            return;
        }
        let idx = (y as usize * self.w as usize + x as usize) * 4;
        let Some(dst) = self.data.get_mut(idx..idx + 4) else {
            return;
        };
        blend_over(dst, &color);
    }
}

/// `w * h * 4` as a `usize`, or `None` on overflow / above [`MAX_IMAGE_BYTES`].
fn checked_rgba_len(w: u32, h: u32) -> Option<usize> {
    let bytes = (w as u64).checked_mul(h as u64)?.checked_mul(4)?;
    if bytes > MAX_IMAGE_BYTES {
        return None;
    }
    usize::try_from(bytes).ok()
}

/// Rounded division by 255, the exact denominator for 8-bit alpha math.
fn div255(value: u32) -> u32 {
    (value + 127) / 255
}

/// Non-premultiplied source-over composite of `src` (4 bytes) onto `dst`.
fn blend_over(dst: &mut [u8], src: &[u8]) {
    let src_alpha = src[3] as u32;
    if src_alpha == 0 {
        return;
    }
    if src_alpha == 255 {
        dst.copy_from_slice(&src[..4]);
        return;
    }

    let dst_alpha = dst[3] as u32;
    let inverse = 255 - src_alpha;
    let carried = div255(dst_alpha * inverse);
    let out_alpha = src_alpha + carried;
    if out_alpha == 0 {
        dst.copy_from_slice(&[0, 0, 0, 0]);
        return;
    }

    for channel in 0..3 {
        let numerator = src[channel] as u32 * src_alpha + dst[channel] as u32 * carried;
        dst[channel] = (numerator / out_alpha).min(255) as u8;
    }
    dst[3] = out_alpha.min(255) as u8;
}

/// Alpha-aware source-over composite.
///
/// `dst_x` / `dst_y` may be negative or push the source partly (or wholly) off
/// canvas; the overlapping rectangle is clipped, never panicked on.
pub fn blit_over(dst: &mut Rgba, src: &Rgba, dst_x: i64, dst_y: i64) {
    if dst.w == 0 || dst.h == 0 || src.w == 0 || src.h == 0 {
        return;
    }

    // Clip in i64 so a far-off-canvas offset cannot wrap before comparison.
    let x0 = dst_x.max(0);
    let y0 = dst_y.max(0);
    let x1 = (dst_x + src.w as i64).min(dst.w as i64);
    let y1 = (dst_y + src.h as i64).min(dst.h as i64);
    if x1 <= x0 || y1 <= y0 {
        return;
    }

    let dst_stride = dst.w as usize;
    let src_stride = src.w as usize;
    for y in y0..y1 {
        let src_row = (y - dst_y) as usize;
        for x in x0..x1 {
            let src_col = (x - dst_x) as usize;
            let src_idx = (src_row * src_stride + src_col) * 4;
            let dst_idx = (y as usize * dst_stride + x as usize) * 4;
            let Some(src_px) = src.data.get(src_idx..src_idx + 4) else {
                continue;
            };
            let src_px: [u8; 4] = [src_px[0], src_px[1], src_px[2], src_px[3]];
            let Some(dst_px) = dst.data.get_mut(dst_idx..dst_idx + 4) else {
                continue;
            };
            blend_over(dst_px, &src_px);
        }
    }
}

/// Integer nearest-neighbour magnification. `scale` 0 is treated as 1.
///
/// Returns a clone of the source when the magnified buffer would exceed the
/// allocation cap, so an oversized asset yields a small image instead of an OOM.
pub fn upscale_nearest(src: &Rgba, scale: u32) -> Rgba {
    let scale = scale.max(1);
    if scale == 1 || src.w == 0 || src.h == 0 {
        return src.clone();
    }

    let out_w = src.w.saturating_mul(scale);
    let out_h = src.h.saturating_mul(scale);
    let Some(len) = checked_rgba_len(out_w, out_h) else {
        log::warn!(
            "canvas: {}x{} at {scale}x exceeds the buffer cap, leaving it unscaled",
            src.w,
            src.h
        );
        return src.clone();
    };

    let mut data = vec![0u8; len];
    let src_stride = src.w as usize;
    let out_stride = out_w as usize;
    for y in 0..out_h as usize {
        let src_row = y / scale as usize;
        for x in 0..out_w as usize {
            let src_col = x / scale as usize;
            let src_idx = (src_row * src_stride + src_col) * 4;
            let dst_idx = (y * out_stride + x) * 4;
            let Some(src_px) = src.data.get(src_idx..src_idx + 4) else {
                continue;
            };
            data[dst_idx..dst_idx + 4].copy_from_slice(src_px);
        }
    }

    Rgba {
        data,
        w: out_w,
        h: out_h,
    }
}

/// Largest integer scale that lands the long edge inside
/// `[MIN_LONG_EDGE, MAX_LONG_EDGE]`. Always >= 1.
///
/// Retail sprites are tiny — a pip strip is 16x2, a unit frame 32x28 — so this
/// normally returns a large factor. Anything already at or above the upper
/// bound stays at 1:1.
pub fn choose_scale(w: u32, h: u32) -> u32 {
    let long_edge = w.max(h);
    if long_edge == 0 {
        return 1;
    }
    // floor(MAX / long) is the largest factor that still fits under the ceiling,
    // and it clears the floor for free: the scaled edge always exceeds
    // MAX - long, which is >= MIN whenever long <= MAX - MIN (768). Above that
    // the factor is 1 and the edge is already past MIN on its own. Anything
    // wider than MAX stays at 1:1 rather than shrinking — this only magnifies.
    (MAX_LONG_EDGE / long_edge).max(1)
}

/// 1px rectangle outline, clipped to the buffer.
pub fn draw_rect_outline(dst: &mut Rgba, x: i64, y: i64, w: u32, h: u32, color: [u8; 4]) {
    if w == 0 || h == 0 || dst.w == 0 || dst.h == 0 {
        return;
    }
    let right = x + w as i64 - 1;
    let bottom = y + h as i64 - 1;

    // Clip the spans *before* iterating. Relying on `blend_pixel` to drop
    // out-of-range writes would still walk billions of coordinates when a
    // corrupt asset asks for an enormous rectangle — safe, but a hang.
    let (x0, x1) = (x.max(0), right.min(dst.w as i64 - 1));
    let (y0, y1) = (y.max(0), bottom.min(dst.h as i64 - 1));

    for px in x0..=x1 {
        dst.blend_pixel(px, y, color);
        dst.blend_pixel(px, bottom, color);
    }
    for py in y0..=y1 {
        dst.blend_pixel(x, py, color);
        dst.blend_pixel(right, py, color);
    }
}

/// Small cross marking a coordinate origin, clipped to the buffer.
pub fn draw_crosshair(dst: &mut Rgba, x: i64, y: i64, arm: u32, color: [u8; 4]) {
    if dst.w == 0 || dst.h == 0 {
        return;
    }
    let arm = arm as i64;
    let (x0, x1) = ((x - arm).max(0), (x + arm).min(dst.w as i64 - 1));
    let (y0, y1) = ((y - arm).max(0), (y + arm).min(dst.h as i64 - 1));

    for px in x0..=x1 {
        dst.blend_pixel(px, y, color);
    }
    for py in y0..=y1 {
        dst.blend_pixel(x, py, color);
    }
}

/// The shared 5x7 table, keyed for lookup. Built once per process.
fn glyph_table() -> &'static HashMap<char, [&'static str; 7]> {
    static TABLE: OnceLock<HashMap<char, [&'static str; 7]>> = OnceLock::new();
    TABLE.get_or_init(|| fallback_5x7_glyphs().into_iter().collect())
}

/// Draw 5x7 bitmap text. Returns the advance width in pixels.
///
/// Characters absent from the shared table still advance the pen but draw
/// nothing, which keeps [`text_width`] exact for arbitrary input — the table
/// covers only space, `-`, `:`, `/`, digits and both letter cases.
pub fn draw_text(dst: &mut Rgba, x: i64, y: i64, text: &str, color: [u8; 4]) -> u32 {
    let table = glyph_table();
    let mut drawn = 0u32;

    for (index, ch) in text.chars().enumerate() {
        let pen_x = x + index as i64 * GLYPH_ADVANCE as i64;
        drawn += 1;
        // Skip glyph cells that cannot touch the canvas, so a long label that
        // runs off the edge costs nothing per character.
        if pen_x + GLYPH_W as i64 <= 0
            || pen_x >= dst.w as i64
            || y + GLYPH_H as i64 <= 0
            || y >= dst.h as i64
        {
            continue;
        }
        let Some(bitmap) = table.get(&ch) else {
            continue;
        };
        for (row, pattern) in bitmap.iter().enumerate() {
            for (col, cell) in pattern.bytes().enumerate() {
                if cell != GLYPH_SET {
                    continue;
                }
                dst.blend_pixel(pen_x + col as i64, y + row as i64, color);
            }
        }
    }

    advance_width(drawn)
}

/// Pixel width [`draw_text`] will report for `text`.
pub fn text_width(text: &str) -> u32 {
    advance_width(text.chars().count() as u32)
}

/// Total pen travel for `count` characters, trailing spacing trimmed.
fn advance_width(count: u32) -> u32 {
    count
        .saturating_mul(GLYPH_ADVANCE)
        .saturating_sub(GLYPH_SPACING)
}

/// Longest label that fits in `width` pixels, in characters.
fn label_char_budget(width: u32) -> usize {
    // n glyphs occupy n * ADVANCE - SPACING, so n <= (width + SPACING) / ADVANCE.
    (width.saturating_add(GLYPH_SPACING) / GLYPH_ADVANCE) as usize
}

/// One labelled cell of a contact sheet.
#[derive(Clone)]
pub struct SheetCell {
    pub image: Rgba,
    pub label: String,
}

/// Smallest `r` with `r * r >= n`, used to keep sheets roughly square.
fn ceil_sqrt(n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    let root = n.isqrt();
    if root * root < n { root + 1 } else { root }
}

/// Lay cells out in a grid, each with its label burned in beneath it, under a
/// header block of one line per string. Wraps at [`MAX_SHEET_WIDTH`].
///
/// Cells are sized to the largest member so the grid stays aligned; artwork is
/// centred within its cell. Always returns at least a 1x1 image, including for
/// an empty cell list, so callers can save the result unconditionally.
pub fn build_contact_sheet(header: &[String], cells: &[SheetCell]) -> Rgba {
    // Every dimension below is saturating: a corrupt asset can yield an
    // enormous frame or label, and the layout must clamp rather than wrap a u32
    // (which panics in debug builds). An implausible total is caught by the
    // allocation cap in `checkerboard` and falls back to a marker image.
    let header_text_w = header
        .iter()
        .map(|line| text_width(line))
        .max()
        .unwrap_or(0);
    let header_h = if header.is_empty() {
        0
    } else {
        (header.len() as u32)
            .saturating_mul(LABEL_HEIGHT)
            .saturating_add(SHEET_GAP)
    };

    // A cell is as wide as the widest artwork or label it must hold, so a long
    // label can never bleed into the neighbouring column.
    let mut cell_w = 0u32;
    let mut art_h = 0u32;
    for cell in cells {
        cell_w = cell_w.max(cell.image.w).max(text_width(&cell.label));
        art_h = art_h.max(cell.image.h);
    }
    cell_w = cell_w.max(1);
    art_h = art_h.max(1);
    let block_h = art_h.saturating_add(LABEL_HEIGHT);

    let (cols, rows) = if cells.is_empty() {
        (0, 0)
    } else {
        let usable = MAX_SHEET_WIDTH.saturating_sub(SHEET_PADDING * 2).max(1);
        let max_cols = (usable / cell_w.saturating_add(SHEET_GAP)).max(1);
        let cols = ceil_sqrt(cells.len() as u32)
            .clamp(1, max_cols)
            .min(cells.len() as u32);
        (cols, (cells.len() as u32).div_ceil(cols))
    };

    let grid_w = if cols == 0 {
        0
    } else {
        cols.saturating_mul(cell_w)
            .saturating_add((cols - 1).saturating_mul(SHEET_GAP))
    };
    let grid_h = if rows == 0 {
        0
    } else {
        rows.saturating_mul(block_h)
            .saturating_add((rows - 1).saturating_mul(SHEET_GAP))
    };

    let sheet_w = grid_w
        .max(header_text_w)
        .max(1)
        .saturating_add(SHEET_PADDING * 2);
    let sheet_h = header_h
        .saturating_add(grid_h)
        .max(1)
        .saturating_add(SHEET_PADDING * 2);
    let mut sheet = Rgba::checkerboard(sheet_w, sheet_h);
    if sheet.w == 0 || sheet.h == 0 {
        // The cap rejected the layout; fall back to a marker pixel rather than
        // handing the caller something it cannot encode.
        return Rgba::new_filled(1, 1, CHECKER_DARK);
    }

    for (line_index, line) in header.iter().enumerate() {
        let y = SHEET_PADDING as i64 + line_index as i64 * LABEL_HEIGHT as i64;
        draw_text(&mut sheet, SHEET_PADDING as i64, y, line, HEADER_COLOR);
    }

    // Cell origins are computed in i64 — the products can legitimately exceed
    // u32 for a pathological cell size, and off-canvas origins clip harmlessly.
    let grid_origin_y = SHEET_PADDING as i64 + header_h as i64;
    for (index, cell) in cells.iter().enumerate() {
        let col = (index as u32 % cols.max(1)) as i64;
        let row = (index as u32 / cols.max(1)) as i64;
        let cell_x = SHEET_PADDING as i64 + col * (cell_w as i64 + SHEET_GAP as i64);
        let cell_y = grid_origin_y + row * (block_h as i64 + SHEET_GAP as i64);

        // Frame sits one pixel outside the artwork so it never covers sprite
        // pixels; SHEET_PADDING guarantees it stays on canvas.
        draw_rect_outline(
            &mut sheet,
            cell_x - 1,
            cell_y - 1,
            cell_w.saturating_add(2),
            art_h.saturating_add(2),
            CELL_BORDER,
        );

        let art_x = cell_x + (cell_w.saturating_sub(cell.image.w) / 2) as i64;
        let art_y = cell_y + (art_h.saturating_sub(cell.image.h) / 2) as i64;
        blit_over(&mut sheet, &cell.image, art_x, art_y);

        let budget = label_char_budget(cell_w);
        let label: String = cell.label.chars().take(budget).collect();
        let label_y = cell_y + art_h as i64 + LABEL_TOP_GAP as i64;
        draw_text(&mut sheet, cell_x, label_y, &label, LABEL_COLOR);
    }

    sheet
}

/// Write a PNG, creating parent directories. `Err` carries a human-readable reason.
pub fn save_png(path: &Path, image: &Rgba) -> Result<(), String> {
    if image.w == 0 || image.h == 0 {
        return Err(format!(
            "refusing to write a {}x{} image to {}",
            image.w,
            image.h,
            path.display()
        ));
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create directory {}: {err}", parent.display()))?;
    }

    let buffer = RgbaImage::from_raw(image.w, image.h, image.data.clone()).ok_or_else(|| {
        format!(
            "pixel buffer is {} bytes, expected {} for {}x{}",
            image.data.len(),
            image.pixel_count() * 4,
            image.w,
            image.h
        )
    })?;
    buffer
        .save(path)
        .map_err(|err| format!("write {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];

    fn pixel(image: &Rgba, x: u32, y: u32) -> [u8; 4] {
        let idx = (y as usize * image.w as usize + x as usize) * 4;
        let px = &image.data[idx..idx + 4];
        [px[0], px[1], px[2], px[3]]
    }

    fn cell(w: u32, h: u32, label: &str) -> SheetCell {
        SheetCell {
            image: Rgba::new_filled(w, h, RED),
            label: label.to_string(),
        }
    }

    #[test]
    fn choose_scale_leaves_large_images_alone() {
        assert_eq!(choose_scale(640, 480), 1);
        assert_eq!(choose_scale(4096, 4096), 1);
    }

    #[test]
    fn choose_scale_handles_zero_dimensions() {
        assert_eq!(choose_scale(0, 0), 1);
        assert_eq!(choose_scale(0, 32), 32);
    }

    #[test]
    fn choose_scale_result_always_lands_in_the_target_band() {
        for long_edge in 1..=MAX_LONG_EDGE {
            let scaled = long_edge * choose_scale(long_edge, 1);
            assert!(
                (MIN_LONG_EDGE..=MAX_LONG_EDGE).contains(&scaled),
                "long edge {long_edge} scaled to {scaled}"
            );
        }
    }

    #[test]
    fn blit_over_clips_at_every_edge_without_panicking() {
        let src = Rgba::new_filled(4, 4, RED);
        for (dx, dy) in [
            (-2i64, 1i64), // left
            (8, 1),        // right
            (1, -2),       // top
            (1, 8),        // bottom
            (-100, -100),  // fully negative
            (-4, -4),      // exactly off by the source size
            (10, 10),      // fully past the far corner
            (i64::MIN, 0), // extreme negative must not wrap
            (i64::MAX - 4, 0),
        ] {
            let mut dst = Rgba::checkerboard(10, 10);
            blit_over(&mut dst, &src, dx, dy);
            assert_eq!(dst.data.len(), dst.pixel_count() * 4);
        }
    }

    #[test]
    fn blit_over_writes_only_the_overlapping_region() {
        let src = Rgba::new_filled(4, 4, RED);
        let mut dst = Rgba::new_filled(10, 10, BLUE);
        // Two of four columns and two of four rows land on canvas.
        blit_over(&mut dst, &src, -2, -2);
        assert_eq!(pixel(&dst, 0, 0), RED);
        assert_eq!(pixel(&dst, 1, 1), RED);
        assert_eq!(pixel(&dst, 2, 2), BLUE);
        assert_eq!(pixel(&dst, 0, 2), BLUE);
    }

    #[test]
    fn blit_over_respects_source_alpha() {
        let mut src = Rgba::new_filled(2, 1, RED);
        src.data[3] = 0; // pixel 0 fully transparent
        src.data[7] = 255; // pixel 1 fully opaque
        let mut dst = Rgba::new_filled(2, 1, BLUE);

        blit_over(&mut dst, &src, 0, 0);
        assert_eq!(pixel(&dst, 0, 0), BLUE, "alpha 0 must leave dst untouched");
        assert_eq!(pixel(&dst, 1, 0), RED, "alpha 255 must replace dst");
    }

    #[test]
    fn blit_over_blends_partial_alpha_between_the_two_colors() {
        let src = Rgba::new_filled(1, 1, [255, 0, 0, 128]);
        let mut dst = Rgba::new_filled(1, 1, BLUE);
        blit_over(&mut dst, &src, 0, 0);
        let out = pixel(&dst, 0, 0);
        assert_eq!(out[3], 255, "opaque dst stays opaque");
        assert!(out[0] > 100 && out[0] < 160, "red channel {}", out[0]);
        assert!(out[2] > 100 && out[2] < 160, "blue channel {}", out[2]);
    }

    #[test]
    fn upscale_nearest_multiplies_dimensions_and_replicates_pixels() {
        let mut src = Rgba::new_filled(2, 2, BLUE);
        // Mark the top-right source pixel so a spot check can locate it.
        src.data[4..8].copy_from_slice(&RED);

        let out = upscale_nearest(&src, 3);
        assert_eq!((out.w, out.h), (6, 6));
        assert_eq!(out.data.len(), out.pixel_count() * 4);
        // Source (1,0) covers destination columns 3..6, rows 0..3.
        assert_eq!(pixel(&out, 3, 0), RED);
        assert_eq!(pixel(&out, 5, 2), RED);
        assert_eq!(pixel(&out, 2, 0), BLUE);
        assert_eq!(pixel(&out, 3, 3), BLUE);
    }

    #[test]
    fn upscale_nearest_treats_zero_scale_as_one() {
        let src = Rgba::new_filled(3, 2, RED);
        let out = upscale_nearest(&src, 0);
        assert_eq!((out.w, out.h), (3, 2));
        assert_eq!(out.data, src.data);
    }

    #[test]
    fn upscale_nearest_declines_an_allocation_over_the_cap() {
        let src = Rgba::new_filled(64, 64, RED);
        let out = upscale_nearest(&src, 100_000);
        assert_eq!((out.w, out.h), (64, 64), "must fall back, not allocate");
    }

    #[test]
    fn from_raw_rejects_a_wrong_length_buffer() {
        assert!(Rgba::from_raw(vec![0u8; 4 * 4 * 4], 4, 4).is_some());
        assert!(Rgba::from_raw(vec![0u8; 4 * 4 * 4 - 1], 4, 4).is_none());
        assert!(Rgba::from_raw(vec![0u8; 4 * 4 * 4 + 1], 4, 4).is_none());
        assert!(Rgba::from_raw(Vec::new(), 0, 0).is_some());
        // A dimension pair that overflows the cap is rejected outright.
        assert!(Rgba::from_raw(vec![0u8; 16], u32::MAX, u32::MAX).is_none());
    }

    #[test]
    fn text_width_matches_the_draw_text_advance() {
        let mut target = Rgba::new_filled(200, 32, CHECKER_DARK);
        for text in [
            "",
            "A",
            "frame 12",
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
            "abcdefghijklmnopqrstuvwxyz",
            "0123456789 -:/",
            "unknown chars still advance",
        ] {
            let drawn = draw_text(&mut target, 2, 2, text, HEADER_COLOR);
            assert_eq!(drawn, text_width(text), "advance mismatch for {text:?}");
        }
    }

    #[test]
    fn draw_text_sets_pixels_and_clips_offscreen_positions() {
        let mut target = Rgba::new_filled(40, 16, [0, 0, 0, 255]);
        draw_text(&mut target, 1, 1, "A", HEADER_COLOR);
        let lit = target
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[0] == 255 && px[1] == 255 && px[2] == 255)
            .count();
        assert!(lit > 0, "glyph 'A' should light some pixels");

        // Way off canvas in both directions must not panic or write anything.
        let before = target.data.clone();
        draw_text(&mut target, -1000, -1000, "OFFSCREEN", HEADER_COLOR);
        draw_text(&mut target, 5000, 5000, "OFFSCREEN", HEADER_COLOR);
        assert_eq!(target.data, before);
    }

    #[test]
    fn draw_shapes_clip_to_the_buffer() {
        let mut target = Rgba::new_filled(8, 8, [0, 0, 0, 255]);
        draw_rect_outline(&mut target, -4, -4, 20, 20, RED);
        draw_rect_outline(&mut target, 0, 0, 0, 0, RED);
        draw_crosshair(&mut target, 0, 0, 100, BLUE);
        draw_crosshair(&mut target, -50, -50, 3, BLUE);
        assert_eq!(target.data.len(), target.pixel_count() * 4);
    }

    #[test]
    fn build_contact_sheet_handles_an_empty_cell_list() {
        let sheet = build_contact_sheet(&["no frames".to_string()], &[]);
        assert!(sheet.w > 0 && sheet.h > 0);
        assert_eq!(sheet.data.len(), sheet.pixel_count() * 4);
    }

    #[test]
    fn build_contact_sheet_wraps_forty_cells_into_a_grid() {
        let cells: Vec<SheetCell> = (0..40)
            .map(|i| cell(24, 24, &format!("frame {i}")))
            .collect();
        let sheet = build_contact_sheet(&["40 frames".to_string()], &cells);

        assert!(sheet.w > 0 && sheet.h > 0);
        assert!(sheet.w <= MAX_SHEET_WIDTH, "sheet width {}", sheet.w);
        assert_eq!(sheet.data.len(), sheet.pixel_count() * 4);
        // ceil(sqrt(40)) = 7 columns, so 6 rows of a 24px cell plus its label.
        let block_h = 24 + LABEL_HEIGHT;
        assert!(sheet.h >= SHEET_PADDING * 2 + 6 * block_h);
    }

    #[test]
    fn build_contact_sheet_wraps_wide_cells_within_the_width_limit() {
        // Cells wider than half the limit force a single column.
        let cells: Vec<SheetCell> = (0..4).map(|i| cell(1500, 20, &format!("w{i}"))).collect();
        let sheet = build_contact_sheet(&[], &cells);
        assert!(sheet.w <= MAX_SHEET_WIDTH, "sheet width {}", sheet.w);
        assert!(sheet.h >= SHEET_PADDING * 2 + 4 * (20 + LABEL_HEIGHT));
    }

    #[test]
    fn build_contact_sheet_tolerates_zero_sized_cell_images() {
        let cells = [
            SheetCell {
                image: Rgba::default(),
                label: "empty".to_string(),
            },
            cell(8, 8, "ok"),
        ];
        let sheet = build_contact_sheet(&[], &cells);
        assert!(sheet.w > 0 && sheet.h > 0);
        assert_eq!(sheet.data.len(), sheet.pixel_count() * 4);
    }

    #[test]
    fn ceil_sqrt_rounds_up_for_non_squares() {
        assert_eq!(ceil_sqrt(0), 0);
        assert_eq!(ceil_sqrt(1), 1);
        assert_eq!(ceil_sqrt(2), 2);
        assert_eq!(ceil_sqrt(16), 4);
        assert_eq!(ceil_sqrt(17), 5);
        assert_eq!(ceil_sqrt(40), 7);
    }

    #[test]
    fn label_char_budget_never_overflows_its_width() {
        for width in 0..64u32 {
            let budget = label_char_budget(width);
            assert!(
                advance_width(budget as u32) <= width || budget == 0,
                "budget {budget} overflows width {width}"
            );
        }
    }

    #[test]
    fn new_filled_refuses_an_implausible_allocation() {
        let image = Rgba::new_filled(u32::MAX, u32::MAX, RED);
        assert_eq!((image.w, image.h), (0, 0));
        assert!(image.data.is_empty());
    }

    #[test]
    fn save_png_rejects_a_zero_sized_image() {
        let err = save_png(Path::new("unused.png"), &Rgba::default()).unwrap_err();
        assert!(err.contains("0x0"), "unexpected message: {err}");
    }
}
