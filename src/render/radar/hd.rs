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
//!   the same rule as the native pixels.
//! - Objects, radar events and beacons keep their native pixels and are drawn
//!   over the terrain as blocks, so they cover the cells they always did.
//! - A click takes the cell under its HD pixel ([`super::interaction`]).
//!
//! The GPU composes it from the native composite ([`super::compose`]).

use crate::render::batch::{BatchRenderer, BatchTexture};

use super::compose::{PICTURE_FORMAT, SurfaceTextures};
use super::geometry::NativeRadarSurfaceGeometry;
use super::helpers::{MINIMAP_HEIGHT, MINIMAP_WIDTH};

/// The HD radar's inputs and picture on the GPU.
pub(super) struct HdRadar {
    scale: u32,
    /// The HD aperture, `scale` times the native 140x108: the minimap's copy
    /// frame reads it with the native texture's normalized coordinates.
    picture: BatchTexture,
    textures: SurfaceTextures,
    /// The HD terrain revision `textures` holds.
    revision: u64,
    /// Where the HD surface starts in the HD aperture: native's centring
    /// (`RebuildRadarSurfaces @ 0x00654650`) times `scale`.
    origin: [i32; 2],
    size: [i32; 2],
}

impl HdRadar {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        native: NativeRadarSurfaceGeometry,
        scale: u32,
        (words, revision): (&[u16], u64),
    ) -> Self {
        let geometry = native.subdivided(scale);
        let offset = native.aperture_offset();
        let scaled = scale as i32;
        let (width, height) = geometry.generated_size();
        Self {
            scale,
            picture: batch.create_render_target_texture(
                device,
                MINIMAP_WIDTH * scale,
                MINIMAP_HEIGHT * scale,
                PICTURE_FORMAT,
            ),
            textures: SurfaceTextures::new(device, queue, geometry, words),
            revision,
            origin: [offset.0.max(0) * scaled, offset.1.max(0) * scaled],
            size: [width, height],
        }
    }

    pub const fn scale(&self) -> u32 {
        self.scale
    }

    pub const fn origin(&self) -> [i32; 2] {
        self.origin
    }

    pub const fn size(&self) -> [i32; 2] {
        self.size
    }

    pub fn picture(&self) -> &BatchTexture {
        &self.picture
    }

    pub(super) fn textures(&self) -> &SurfaceTextures {
        &self.textures
    }

    /// Upload the HD terrain where its revision changed.
    pub fn refresh(&mut self, queue: &wgpu::Queue, (words, revision): (&[u16], u64)) {
        if revision != self.revision {
            self.textures.write_words(queue, words);
            self.revision = revision;
        }
    }
}
