//! The glyph atlas: one RGBA texture of white glyphs whose alpha is their
//! coverage, filled as text asks for glyphs at the sizes it draws them.
//!
//! The texture stays the same object for a whole frame, so every instance the
//! frame emits samples the atlas it was placed in. A glyph that no longer fits
//! is skipped for that frame; the next [`GlyphAtlas::begin_frame`] then
//! empties the atlas, doubling it first while the device allows, and the
//! glyphs on screen are drawn into it again.

use std::collections::HashMap;

use crate::render::atlas_growth::ShelfCursor;
use crate::render::batch::{BatchRenderer, BatchTexture};

/// One glyph at one size and sub-pixel offset.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) struct GlyphKey {
    pub(crate) font: u16,
    pub(crate) coords: u16,
    pub(crate) glyph: u16,
    /// Pixel size in 1/[`SIZE_STEPS`] px.
    pub(crate) size: u32,
    /// Horizontal offset in 1/[`SUBPIXEL_STEPS`] px.
    pub(crate) subpixel: u8,
    /// The size its native pixels are drawn at, in 1/[`SIZE_STEPS`] px,
    /// before they scale to `size` (`super::pixels`).
    pub(crate) native: u32,
    /// The pen's phase in its native pixel, x in the low four bits and y in
    /// the high four (`super::split_phase`).
    pub(crate) phase: u8,
}

/// Size steps per pixel: the text sizes ([`super::fonts`]) are whole steps,
/// so GAME.FNT's glyphs draw on whole pixels at 1:1 ([`super::fnt_font`]).
/// In 1/16 px, 12.6 px drew at 12.625, its pixels 0.2% wide.
pub(crate) const SIZE_STEPS: f32 = 80.0;
pub(crate) const SUBPIXEL_STEPS: u8 = 4;

/// Where a rasterised glyph sits in the atlas and how it hangs from its pen
/// position: `offset` is the bitmap's top-left relative to the pen on the
/// baseline, in pixels, y down.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AtlasGlyph {
    pub(crate) uv_origin: [f32; 2],
    pub(crate) uv_size: [f32; 2],
    pub(crate) size: [f32; 2],
    pub(crate) offset: [f32; 2],
}

/// The GPU side; absent in GPU-free tests, which still pack and measure.
struct AtlasTexture {
    queue: wgpu::Queue,
    texture: wgpu::Texture,
    batch: BatchTexture,
}

pub(crate) struct GlyphAtlas {
    width: u32,
    height: u32,
    max_dim: u32,
    shelf: ShelfCursor,
    /// `None` caches a glyph with no pixels, such as a space.
    glyphs: HashMap<GlyphKey, Option<AtlasGlyph>>,
    overflowed: bool,
    gpu: Option<AtlasTexture>,
    /// The texture's alpha, kept so GPU-free tests can read drawn glyphs back.
    #[cfg(test)]
    alpha: Vec<u8>,
}

const INITIAL_DIM: u32 = 1024;
const PADDING: u32 = 1;

impl GlyphAtlas {
    pub(crate) fn new(gpu: Option<(&wgpu::Device, &wgpu::Queue, &BatchRenderer)>) -> Self {
        let max_dim = gpu.map_or(4096, |(device, _, _)| {
            device.limits().max_texture_dimension_2d.min(4096)
        });
        let mut atlas = Self {
            width: INITIAL_DIM,
            height: INITIAL_DIM,
            max_dim,
            shelf: ShelfCursor::with_padding(INITIAL_DIM, INITIAL_DIM, PADDING),
            glyphs: HashMap::new(),
            overflowed: false,
            gpu: None,
            #[cfg(test)]
            alpha: vec![0; (INITIAL_DIM * INITIAL_DIM) as usize],
        };
        if let Some((device, queue, batch)) = gpu {
            atlas.gpu = Some(Self::create_texture(device, queue, batch, INITIAL_DIM));
        }
        atlas
    }

    fn create_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        dim: u32,
    ) -> AtlasTexture {
        let blank = vec![0u8; (dim * dim * 4) as usize];
        // Glyphs are drawn one texel to one pixel at the size they were
        // rasterised for, so the sampler never blends neighbours.
        let (texture, batch_texture) = batch.create_updatable_texture(
            device,
            queue,
            &blank,
            dim,
            dim,
            wgpu::FilterMode::Nearest,
        );
        AtlasTexture {
            queue: queue.clone(),
            texture,
            batch: batch_texture,
        }
    }

    pub(crate) fn texture(&self) -> Option<&BatchTexture> {
        self.gpu.as_ref().map(|gpu| &gpu.batch)
    }

    /// Start a frame: after an overflow, empty the atlas, doubling it first
    /// while the device allows.
    pub(crate) fn begin_frame(&mut self, gpu: Option<(&wgpu::Device, &BatchRenderer)>) {
        if !self.overflowed {
            return;
        }
        self.overflowed = false;
        self.glyphs.clear();
        if self.width < self.max_dim {
            self.width = (self.width * 2).min(self.max_dim);
            self.height = self.width;
            if let (Some((device, batch)), Some(old)) = (gpu, self.gpu.as_ref()) {
                let queue = old.queue.clone();
                self.gpu = Some(Self::create_texture(device, &queue, batch, self.width));
            }
            log::info!("Glyph atlas grew to {0}x{0}", self.width);
        }
        self.shelf = ShelfCursor::with_padding(self.width, self.height, PADDING);
        #[cfg(test)]
        {
            self.alpha = vec![0; (self.width * self.height) as usize];
        }
    }

    pub(crate) fn get(&self, key: &GlyphKey) -> Option<Option<AtlasGlyph>> {
        self.glyphs.get(key).copied()
    }

    /// Place a rasterised glyph: `alpha` is `size[0] * size[1]` coverage
    /// bytes. Returns `None` when the atlas is full this frame.
    pub(crate) fn insert(
        &mut self,
        key: GlyphKey,
        size: [u32; 2],
        offset: [i32; 2],
        alpha: &[u8],
    ) -> Option<Option<AtlasGlyph>> {
        if size[0] == 0 || size[1] == 0 {
            self.glyphs.insert(key, None);
            return Some(None);
        }
        let Some([x, y]) = self.shelf.place(size[0], size[1]) else {
            self.overflowed = true;
            return None;
        };
        if let Some(gpu) = self.gpu.as_ref() {
            let mut rgba = Vec::with_capacity(alpha.len() * 4);
            for &a in alpha {
                rgba.extend_from_slice(&[255, 255, 255, a]);
            }
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &gpu.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size[0] * 4),
                    rows_per_image: Some(size[1]),
                },
                wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
            );
        }
        #[cfg(test)]
        for (row, line) in alpha.chunks(size[0] as usize).enumerate() {
            let at = ((y as usize + row) * self.width as usize) + x as usize;
            self.alpha[at..at + line.len()].copy_from_slice(line);
        }
        let (w, h) = (self.width as f32, self.height as f32);
        let glyph = AtlasGlyph {
            uv_origin: [x as f32 / w, y as f32 / h],
            uv_size: [size[0] as f32 / w, size[1] as f32 / h],
            size: [size[0] as f32, size[1] as f32],
            offset: [offset[0] as f32, offset[1] as f32],
        };
        self.glyphs.insert(key, Some(glyph));
        Some(Some(glyph))
    }

    /// The coverage at texel `x`, `y`.
    #[cfg(test)]
    pub(crate) fn alpha_at(&self, x: u32, y: u32) -> u8 {
        self.alpha[(y * self.width + x) as usize]
    }

    #[cfg(test)]
    pub(crate) fn size(&self) -> [u32; 2] {
        [self.width, self.height]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(glyph: u16) -> GlyphKey {
        GlyphKey {
            font: 0,
            coords: 0,
            glyph,
            size: 16 * 16,
            subpixel: 0,
            native: 0,
            phase: 0,
        }
    }

    #[test]
    fn a_full_atlas_skips_the_glyph_then_empties_and_doubles_at_the_next_frame() {
        let mut atlas = GlyphAtlas::new(None);
        let tile = vec![255u8; 600 * 600];
        assert!(atlas.insert(key(1), [600, 600], [0, 0], &tile).is_some());
        // A second 600 px glyph cannot fit beside or below the first in 1024.
        assert!(atlas.insert(key(2), [600, 600], [0, 0], &tile).is_none());
        assert!(atlas.get(&key(1)).is_some());

        atlas.begin_frame(None);
        assert_eq!([atlas.width, atlas.height], [2048, 2048]);
        assert!(atlas.get(&key(1)).is_none(), "the glyphs are drawn again");
        assert!(atlas.insert(key(1), [600, 600], [0, 0], &tile).is_some());
        assert!(atlas.insert(key(2), [600, 600], [0, 0], &tile).is_some());
        // Without an overflow the next frame keeps everything.
        atlas.begin_frame(None);
        assert!(atlas.get(&key(2)).is_some());
    }
}
