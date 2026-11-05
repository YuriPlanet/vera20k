//! VERA's optional smooth sidebar (`[VERA] SmoothSidebar=`, off by default).
//!
//! When the sidebar draws at a whole multiple of its own pixels
//! (`ui::sidebar::SidebarFrame::draw_scale`), its cameos and frame pieces can
//! come from copies scaled by xBR, Hyllian's pixel-art scaler
//! ([`crate::render::xbr`]), at 40% strength, instead of drawing as blocks.
//! Each piece is scaled on its own, once per scale; the finished sidebar is
//! never filtered.
//! Text, the names baked into the cameos, the darken and flash masks, the
//! build clock and the radar keep their own pixels. Presentation only.

use crate::assets::pal_file::Palette;
use crate::render::batch::{BatchRenderer, BatchTexture, SpriteInstance};
use crate::render::gpu::GpuContext;
use crate::render::xbr;

/// An atlas image kept on the CPU, with its pieces' rectangles.
pub struct SmoothSource {
    rgba: Vec<u8>,
    size: [u32; 2],
    /// `[x, y, width, height]` of each piece in `rgba`.
    pieces: Vec<[u32; 4]>,
    /// Pixels the copy draws as blocks, one flag per pixel of `rgba`; empty
    /// for none.
    sharp: Vec<bool>,
}

impl SmoothSource {
    pub fn new(rgba: Vec<u8>, size: [u32; 2], pieces: Vec<[u32; 4]>) -> Self {
        assert_eq!(rgba.len(), size[0] as usize * size[1] as usize * 4);
        Self {
            rgba,
            size,
            pieces,
            sharp: Vec::new(),
        }
    }

    /// Keep the flagged pixels, one flag per pixel, as blocks in the copy.
    pub fn with_sharp_pixels(mut self, sharp: Vec<bool>) -> Self {
        assert_eq!(sharp.len() * 4, self.rgba.len());
        self.sharp = sharp;
        self
    }

    /// The image at `scale` times its size, each piece scaled alone and laid
    /// at `scale` times its place, so the copy keeps the image's normalized
    /// layout. xBR takes the largest factor of `scale` it has (2 to 4) and
    /// blocks make up the rest; None when it has none. Sharp pixels then draw
    /// as blocks over the result.
    pub fn upscale(&self, scale: u32) -> Option<Vec<u8>> {
        let factor = (2..=4).rev().find(|factor| scale % factor == 0)? as usize;
        let (scale, block) = (scale as usize, (scale / factor as u32) as usize);
        let stride = self.size[0] as usize;
        let row = stride * scale;
        let mut out = vec![0; row * self.size[1] as usize * scale * 4];
        for &piece in &self.pieces {
            let [x, y, width, _] = piece.map(|value| value as usize);
            let Some(scaled) = scale_piece(&self.rgba, stride, piece, factor) else {
                continue;
            };
            for (line, pixels) in scaled.chunks_exact(width * factor * 4).enumerate() {
                for repeat in 0..block {
                    let start = ((y * scale + line * block + repeat) * row + x * scale) * 4;
                    let target = &mut out[start..][..pixels.len() * block];
                    if block == 1 {
                        target.copy_from_slice(pixels);
                        continue;
                    }
                    for (pixel, run) in pixels
                        .chunks_exact(4)
                        .zip(target.chunks_exact_mut(4 * block))
                    {
                        for dst in run.chunks_exact_mut(4) {
                            dst.copy_from_slice(pixel);
                        }
                    }
                }
            }
        }
        for (index, _) in self.sharp.iter().enumerate().filter(|(_, sharp)| **sharp) {
            let [x, y] = [index % stride, index / stride];
            let pixel = &self.rgba[index * 4..][..4];
            for line in y * scale..(y + 1) * scale {
                let start = (line * row + x * scale) * 4;
                for dst in out[start..][..scale * 4].chunks_exact_mut(4) {
                    dst.copy_from_slice(pixel);
                }
            }
        }
        Some(out)
    }
}

/// The name font's palette indices from its top row down; the second row
/// uses 1 or 4.
const NAME_FONT: [u8; 6] = [255, 1, 4, 34, 72, 80];
/// The widest stroke of the name font, in pixels.
const NAME_STROKE: usize = 5;

/// The pixels of the name baked into a cameo, which the smooth copy keeps:
/// xBR would round its 5-pixel letters into blobs. `indices` is the cameo's
/// canvas, `width` x `height` palette indices; one flag per pixel.
///
/// Westwood drew every retail cameo's name in one font, a white-to-grey ramp
/// of [`NAME_FONT`] indices with a near-black shadow right of each stroke, in
/// the canvas's bottom rows (36-40 and 42-46 of 48). There, every run of 1 to
/// [`NAME_STROKE`] font pixels that ends in a near-black pixel is a stroke.
/// The strokes and shadows, grown by a pixel so that no xBR blend of the
/// letters shows around them, stay sharp. 173 of the 175 retail cameos have a
/// name in this font.
pub fn cameo_name_pixels(
    indices: &[u8],
    width: usize,
    height: usize,
    palette: &Palette,
) -> Vec<bool> {
    assert_eq!(indices.len(), width * height);
    let near_black = |index: u8| {
        let color = palette.color(index);
        index != 0 && color.r.max(color.g).max(color.b) <= 32
    };
    let mut strokes = vec![false; indices.len()];
    for y in height.saturating_sub(12)..height.saturating_sub(1) {
        let row = &indices[y * width..][..width];
        let mut x = 0;
        while x < width {
            let start = x;
            while x < width && NAME_FONT.contains(&row[x]) {
                x += 1;
            }
            if x == start {
                x += 1;
            } else if x < width && x - start <= NAME_STROKE && near_black(row[x]) {
                strokes[y * width + start..=y * width + x].fill(true);
            }
        }
    }
    let mut sharp = vec![false; indices.len()];
    for (index, _) in strokes.iter().enumerate().filter(|(_, stroke)| **stroke) {
        let [x, y] = [index % width, index / width];
        for line in y.saturating_sub(1)..=(y + 1).min(height - 1) {
            sharp[line * width + x.saturating_sub(1)..=line * width + (x + 1).min(width - 1)]
                .fill(true);
        }
    }
    sharp
}

/// How far each pixel of a smooth copy moves from its source pixel's colour
/// toward xBR's, in tenths. Full xBR, then half, looked too soft in play.
const STRENGTH: u16 = 4;

/// One piece of `rgba`, `stride` pixels wide, scaled alone by xBR `factor` at
/// [`STRENGTH`]. xBR continues a piece's edge pixels past it, so a piece that
/// tiles or meets another keeps its corners. None for an empty piece.
fn scale_piece(rgba: &[u8], stride: usize, piece: [u32; 4], factor: usize) -> Option<Vec<u8>> {
    let [x, y, width, height] = piece.map(|value| value as usize);
    if width == 0 || height == 0 {
        return None;
    }
    let mut pixels = Vec::with_capacity(width * height * 4);
    for row in y..y + height {
        pixels.extend_from_slice(&rgba[(row * stride + x) * 4..][..width * 4]);
    }
    let mut scaled = xbr::scale(&pixels, width, height, factor);
    let row = width * factor;
    for (index, pixel) in scaled.chunks_exact_mut(4).enumerate() {
        let [column, line] = [index % row / factor, index / row / factor];
        // The colour only: xBR already keeps the source pixel's alpha.
        let source = &pixels[(line * width + column) * 4..][..3];
        for (channel, &own) in pixel.iter_mut().zip(source) {
            let mixed = u16::from(own) * (10 - STRENGTH) + u16::from(*channel) * STRENGTH;
            *channel = ((mixed + 5) / 10) as u8;
        }
    }
    Some(scaled)
}

/// A [`SmoothSource`] scaled for the sidebar's draw scale.
pub struct SmoothCopy {
    scale: u32,
    /// None when the copy would not fit a texture or xBR has no factor of
    /// the scale: the sidebar then draws the source's own pixels.
    texture: Option<BatchTexture>,
}

impl SmoothCopy {
    pub fn texture(&self) -> Option<&BatchTexture> {
        self.texture.as_ref()
    }
}

/// Keep `copy` at `scale`: build it when the scale changes, drop it for None.
pub fn follow_scale(
    copy: &mut Option<SmoothCopy>,
    source: &SmoothSource,
    scale: Option<u32>,
    gpu: &GpuContext,
    batch: &BatchRenderer,
) {
    let Some(scale) = scale else {
        *copy = None;
        return;
    };
    if copy.as_ref().is_some_and(|copy| copy.scale == scale) {
        return;
    }
    let [width, height] = source.size.map(|value| value.saturating_mul(scale));
    let limit = gpu.device.limits().max_texture_dimension_2d;
    let texture = if width > limit || height > limit {
        log::warn!("Smooth sidebar: a {width}x{height} copy exceeds the {limit} px texture limit");
        None
    } else {
        let started = crate::util::time::Instant::now();
        let texture = source
            .upscale(scale)
            .map(|rgba| batch.create_texture(gpu, &rgba, width, height));
        match texture {
            Some(_) => log::info!(
                "Smooth sidebar: {width}x{height} copy at {scale}x in {} ms",
                started.elapsed().as_millis()
            ),
            None => {
                log::info!("Smooth sidebar: xBR has no factor of {scale}x; keeping the pixels")
            }
        }
        texture
    };
    *copy = Some(SmoothCopy { scale, texture });
}

/// The pieces of a chrome atlas that the sidebar's surfaces draw, packed on
/// their own: the atlas stacks every piece at x 0, and the pause menu's
/// backgrounds alone would make a scaled copy of it too tall for a texture.
pub struct SidebarPieces {
    source: SmoothSource,
    atlas_size: [f32; 2],
    /// Each piece's rectangle in the atlas, by its index in the source.
    in_atlas: Vec<[u32; 4]>,
    /// Each piece's first atlas row, rising, with its index.
    rows: Vec<(u32, usize)>,
}

impl SidebarPieces {
    /// Pack `pieces`, `[x, y, width, height]` rectangles of the atlas `rgba`
    /// that share no row, with `padding` clear pixels between them.
    pub fn gather(
        rgba: &[u8],
        atlas_size: [u32; 2],
        pieces: &[[u32; 4]],
        padding: u32,
        max_dim: u32,
    ) -> Self {
        let sizes: Vec<[u32; 2]> = pieces.iter().map(|&[_, _, w, h]| [w, h]).collect();
        let plan = crate::render::atlas_growth::plan_initial_shelves(
            &sizes,
            crate::render::atlas_growth::InitialShelfPolicy {
                padding,
                max_dim,
                split_pages: false,
            },
        );
        let size = [plan.width, plan.single_page_height()];
        let mut packed = vec![0; size[0] as usize * size[1] as usize * 4];
        let mut placed = Vec::with_capacity(plan.placements.len());
        let mut in_atlas = Vec::with_capacity(plan.placements.len());
        let mut rows = Vec::with_capacity(plan.placements.len());
        for placement in &plan.placements {
            let [x, y, width, height] = pieces[placement.index];
            let [px, py] = placement.origin;
            for line in 0..height as usize {
                let from = ((y as usize + line) * atlas_size[0] as usize + x as usize) * 4;
                let to = ((py as usize + line) * size[0] as usize + px as usize) * 4;
                let len = width as usize * 4;
                packed[to..to + len].copy_from_slice(&rgba[from..from + len]);
            }
            rows.push((y, placed.len()));
            placed.push([px, py, width, height]);
            in_atlas.push(pieces[placement.index]);
        }
        rows.sort_unstable();
        Self {
            source: SmoothSource::new(packed, size, placed),
            atlas_size: atlas_size.map(|value| value as f32),
            in_atlas,
            rows,
        }
    }

    pub fn source(&self) -> &SmoothSource {
        &self.source
    }

    /// Point `instances`, drawn from the atlas, at the same pixels of a copy
    /// of these pieces. False, leaving them unchanged, when one draws a piece
    /// that is not here.
    pub fn retarget(&self, instances: &mut [SpriteInstance]) -> bool {
        let Some(targets) = instances
            .iter()
            .map(|instance| self.retargeted(instance))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        for (instance, (uv_origin, uv_size)) in instances.iter_mut().zip(targets) {
            instance.uv_origin = uv_origin;
            instance.uv_size = uv_size;
        }
        true
    }

    fn retargeted(&self, instance: &SpriteInstance) -> Option<([f32; 2], [f32; 2])> {
        let [atlas_width, atlas_height] = self.atlas_size;
        let [width, height] = self.source.size.map(|value| value as f32);
        // The atlas texels the instance spans; its middle names the piece.
        let origin = [
            instance.uv_origin[0] * atlas_width,
            instance.uv_origin[1] * atlas_height,
        ];
        let size = [
            instance.uv_size[0] * atlas_width,
            instance.uv_size[1] * atlas_height,
        ];
        let middle = [origin[0] + size[0] / 2.0, origin[1] + size[1] / 2.0];
        let after = self
            .rows
            .partition_point(|&(row, _)| row as f32 <= middle[1]);
        let &(_, index) = self.rows.get(after.checked_sub(1)?)?;
        let [ax, ay, aw, ah] = self.in_atlas[index].map(|value| value as f32);
        if !(ax..ax + aw).contains(&middle[0]) || !(ay..ay + ah).contains(&middle[1]) {
            return None;
        }
        let [px, py, _, _] = self.source.pieces[index].map(|value| value as f32);
        Some((
            [
                (px + origin[0] - ax) / width,
                (py + origin[1] - ay) / height,
            ],
            [size[0] / width, size[1] / height],
        ))
    }
}

#[cfg(test)]
#[path = "sidebar_smoothing_tests.rs"]
mod tests;
