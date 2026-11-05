//! VERA's optional HD radar (`[VERA] HdRadar=`, off by default).
//!
//! When the sidebar draws at a whole multiple of its own pixels (a zoomed
//! match, or a larger sidebar), the radar can use them: its generated primary
//! is built at the same multiple, so map cells the native surface averages
//! together get their own pixels. Presentation only; the simulation never
//! reads it.
//!
//! - Terrain: the native sampler (`GenerateTerrainSurface @ 0x006547C0`) at
//!   the larger size, kept beside the native surface by [`super::terrain`].
//! - Shroud and fog: each HD pixel takes the cell the native inverse
//!   (`RenderCellPixel @ 0x00655CB8`) names for it at the larger zoom, under
//!   the same [`CellVisibility`] rule as the native pixels.
//! - Objects, radar events and beacons keep their native pixels and are drawn
//!   over the terrain as blocks, so they cover the cells they always did.
//! - A click takes the cell under its HD pixel ([`super::interaction`]).

use std::collections::HashMap;

use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::native_surface_format::RGB565;

use super::geometry::NativeRadarSurfaceGeometry;
use super::helpers::{COLOR_SHROUD, CellVisibility, MINIMAP_HEIGHT, MINIMAP_WIDTH};

/// The HD radar's picture and the texture it is uploaded to.
pub(super) struct HdRadar {
    image: HdImage,
    texture_raw: wgpu::Texture,
    texture: BatchTexture,
}

impl HdRadar {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        native: NativeRadarSurfaceGeometry,
        scale: u32,
    ) -> Self {
        let image = HdImage::new(native, scale);
        let (width, height) = image.size();
        let (texture_raw, texture) = batch.create_updatable_texture(
            device,
            queue,
            &image.rgba,
            width,
            height,
            wgpu::FilterMode::Nearest,
        );
        Self {
            image,
            texture_raw,
            texture,
        }
    }

    pub const fn scale(&self) -> u32 {
        self.image.scale
    }

    pub fn image_mut(&mut self) -> &mut HdImage {
        &mut self.image
    }

    pub fn upload(&self, queue: &wgpu::Queue) {
        let (width, height) = self.image.size();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture_raw,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.image.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// The whole HD aperture, `scale` times the native 140x108: the minimap's
    /// copy frame reads it with the native texture's normalized coordinates.
    pub fn texture(&self) -> &BatchTexture {
        &self.texture
    }
}

/// The HD aperture on the CPU: terrain, shroud and fog per HD pixel, then the
/// native object pixels over it.
pub(super) struct HdImage {
    scale: u32,
    /// The native geometry generated at `scale` times the size.
    geometry: NativeRadarSurfaceGeometry,
    /// Where the HD surface starts in the HD aperture: native's centring
    /// (`RebuildRadarSurfaces @ 0x00654650`) times `scale`.
    origin: (usize, usize),
    /// Each HD surface pixel's cell, row-major, as an index into `cells`.
    pixel_cells: Vec<u32>,
    cells: Vec<(u16, u16)>,
    /// `pixels[starts[i]..starts[i + 1]]` are the HD surface pixels of `cells[i]`.
    starts: Vec<u32>,
    pixels: Vec<u32>,
    /// Each cell's visibility when last painted; none before the first paint.
    painted: Vec<Option<CellVisibility>>,
    terrain_revision: Option<u64>,
    /// The HD aperture under the objects.
    terrain_rgba: Vec<u8>,
    /// The HD aperture with the objects, as uploaded.
    rgba: Vec<u8>,
    /// The native aperture before the objects were drawn on it.
    native_terrain: Vec<u8>,
}

impl HdImage {
    pub fn new(native: NativeRadarSurfaceGeometry, scale: u32) -> Self {
        let geometry = native.subdivided(scale);
        let (width, height) = geometry.generated_size();
        let offset = native.aperture_offset();
        let origin = (
            offset.0.max(0) as usize * scale as usize,
            offset.1.max(0) as usize * scale as usize,
        );
        let mut slots = HashMap::new();
        let mut cells = Vec::new();
        let mut pixel_cells = Vec::with_capacity(width.max(0) as usize * height.max(0) as usize);
        for y in 0..height {
            for x in 0..width {
                let cell = geometry.surface_pixel_to_visibility_cell((x, y));
                let slot = *slots.entry(cell).or_insert_with(|| {
                    cells.push(cell);
                    (cells.len() - 1) as u32
                });
                pixel_cells.push(slot);
            }
        }
        let mut starts = vec![0u32; cells.len() + 1];
        for &slot in &pixel_cells {
            starts[slot as usize + 1] += 1;
        }
        for slot in 0..cells.len() {
            starts[slot + 1] += starts[slot];
        }
        let mut next = starts.clone();
        let mut pixels = vec![0u32; pixel_cells.len()];
        for (pixel, &slot) in pixel_cells.iter().enumerate() {
            pixels[next[slot as usize] as usize] = pixel as u32;
            next[slot as usize] += 1;
        }
        let aperture = (MINIMAP_WIDTH * scale * MINIMAP_HEIGHT * scale) as usize;
        let terrain_rgba = COLOR_SHROUD.repeat(aperture);
        Self {
            scale,
            geometry,
            origin,
            pixel_cells,
            painted: vec![None; cells.len()],
            cells,
            starts,
            pixels,
            terrain_revision: None,
            rgba: terrain_rgba.clone(),
            terrain_rgba,
            native_terrain: vec![0; (MINIMAP_WIDTH * MINIMAP_HEIGHT * 4) as usize],
        }
    }

    pub const fn size(&self) -> (u32, u32) {
        (MINIMAP_WIDTH * self.scale, MINIMAP_HEIGHT * self.scale)
    }

    /// Repaint the cells whose visibility changed, or every pixel when the HD
    /// terrain did. `visibility` answers each cell's state now.
    pub fn paint(
        &mut self,
        terrain: &[u16],
        terrain_revision: u64,
        visibility: impl Fn(u16, u16) -> CellVisibility,
    ) {
        debug_assert_eq!(terrain.len(), self.pixel_cells.len());
        let layout = Layout {
            surface_width: self.geometry.generated_size().0.max(1) as usize,
            origin: self.origin,
            row: (MINIMAP_WIDTH * self.scale) as usize,
        };
        let all = self.terrain_revision != Some(terrain_revision);
        self.terrain_revision = Some(terrain_revision);
        for (slot, &(rx, ry)) in self.cells.iter().enumerate() {
            let state = visibility(rx, ry);
            if self.painted[slot] == Some(state) && !all {
                continue;
            }
            self.painted[slot] = Some(state);
            if !all {
                let pixels =
                    &self.pixels[self.starts[slot] as usize..self.starts[slot + 1] as usize];
                for &pixel in pixels {
                    layout.paint(&mut self.terrain_rgba, terrain, pixel as usize, state);
                }
            }
        }
        if all {
            for (pixel, &slot) in self.pixel_cells.iter().enumerate() {
                let state = self.painted[slot as usize].expect("every cell painted above");
                layout.paint(&mut self.terrain_rgba, terrain, pixel, state);
            }
        }
    }

    /// Remember the native aperture before objects are drawn on it.
    pub fn keep_native_terrain(&mut self, native: &[u8]) {
        self.native_terrain.copy_from_slice(native);
    }

    /// Lay the native pixels that objects, radar events or beacons changed
    /// since [`Self::keep_native_terrain`] over the HD terrain as blocks.
    pub fn draw_objects(&mut self, native: &[u8]) {
        self.rgba.copy_from_slice(&self.terrain_rgba);
        let scale = self.scale as usize;
        let width = MINIMAP_WIDTH as usize;
        let row = width * scale;
        let changes = native
            .as_chunks::<4>()
            .0
            .iter()
            .zip(self.native_terrain.as_chunks::<4>().0)
            .enumerate();
        for (index, (now, before)) in changes {
            if now == before {
                continue;
            }
            let (x, y) = (index % width, index / width);
            for line in y * scale..(y + 1) * scale {
                let start = (line * row + x * scale) * 4;
                for pixel in self.rgba[start..start + scale * 4].as_chunks_mut::<4>().0 {
                    *pixel = *now;
                }
            }
        }
    }

    #[cfg(test)]
    pub(super) fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// The cell whose shroud and fog HD surface pixel `pixel` shows.
    #[cfg(test)]
    pub(super) fn cell_at(&self, pixel: (i32, i32)) -> (u16, u16) {
        let width = self.geometry.generated_size().0;
        self.cells[self.pixel_cells[(pixel.1 * width + pixel.0) as usize] as usize]
    }
}

#[derive(Clone, Copy)]
struct Layout {
    surface_width: usize,
    origin: (usize, usize),
    row: usize,
}

impl Layout {
    fn paint(self, rgba: &mut [u8], terrain: &[u16], pixel: usize, state: CellVisibility) {
        let packed = terrain[pixel];
        let [red, green, blue] = RGB565.unpack_rgb8(packed);
        let color = state
            .color(packed, [red, green, blue, 255])
            .unwrap_or(COLOR_SHROUD);
        let (x, y) = (pixel % self.surface_width, pixel / self.surface_width);
        let at = ((self.origin.1 + y) * self.row + self.origin.0 + x) * 4;
        rgba[at..at + 4].copy_from_slice(&color);
    }
}

#[cfg(test)]
#[path = "hd_tests.rs"]
mod tests;
