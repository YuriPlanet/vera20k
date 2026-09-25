//! Per-frame sprites of crashing Fly bodies.
//!
//! `FlyLocomotionClass` Draw_Matrix's crashing arm (`0x004CF610`) keys its
//! draw -1, so the native voxel cache never holds a crash pose: the body is
//! rasterized afresh every frame with its current roll and pitch. This page
//! does the same for the crashing bodies on screen. It is rebuilt each
//! presentation frame ([`VxlPoseFrameCache::begin_frame`]) and uploaded once
//! after the unit instances are built ([`VxlPoseFrameCache::upload`]).

use crate::assets::asset_manager::AssetManager;
use crate::assets::vpl_file::VplFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::gpu::GpuContext;
use crate::render::unit_atlas::{UnitSpriteEntry, UnitSpriteKey, render_unit_sprite_posed};
use crate::render::vxl_raster::VxlSprite;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ruleset::RuleSet;

const PAGE_SIZE: u32 = 1024;
const SPRITE_PADDING: u32 = 1;

#[derive(Default)]
pub struct VxlPoseFrameCache {
    pixels: Vec<u8>,
    cursor_x: u32,
    cursor_y: u32,
    shelf_height: u32,
    dirty: bool,
    texture: Option<BatchTexture>,
}

impl VxlPoseFrameCache {
    /// Forget last frame's poses. The texture stays bound until the next
    /// upload replaces it.
    pub fn begin_frame(&mut self) {
        if self.pixels.is_empty() {
            self.pixels = vec![0; (PAGE_SIZE * PAGE_SIZE) as usize];
        } else if self.dirty || self.cursor_x != 0 || self.cursor_y != 0 {
            self.pixels.fill(0);
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.shelf_height = 0;
        self.dirty = false;
    }

    /// Rasterize one crashing body at `tilt` (roll, pitch in radians) and place
    /// it on this frame's page. None when the model does not resolve or the
    /// page is full.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        gpu: &GpuContext,
        asset_manager: &AssetManager,
        rules: Option<&RuleSet>,
        art: Option<&ArtRegistry>,
        key: &UnitSpriteKey,
        tilt: [f32; 2],
    ) -> Option<UnitSpriteEntry> {
        if self.pixels.is_empty() {
            self.begin_frame();
        }
        let vpl = asset_manager
            .get_ref("VOXELS.VPL")
            .and_then(|data| VplFile::from_bytes(data).ok());
        let (sprite, _, native_draw_bounds) = render_unit_sprite_posed(
            asset_manager,
            key,
            rules,
            art,
            vpl.as_ref(),
            None,
            gpu,
            None,
            Some(tilt),
        )?;
        let (px, py) = self.try_place(&sprite)?;
        self.blit(&sprite, px, py);
        self.dirty = true;
        Some(UnitSpriteEntry {
            uv_origin: [px as f32 / PAGE_SIZE as f32, py as f32 / PAGE_SIZE as f32],
            uv_size: [
                sprite.width as f32 / PAGE_SIZE as f32,
                sprite.height as f32 / PAGE_SIZE as f32,
            ],
            pixel_size: [sprite.width as f32, sprite.height as f32],
            offset_x: sprite.offset_x,
            offset_y: sprite.offset_y,
            native_draw_bounds,
            page: 0,
        })
    }

    /// Upload this frame's page when anything was placed on it.
    pub fn upload(&mut self, gpu: &GpuContext, batch: &BatchRenderer) {
        if self.dirty {
            self.texture =
                Some(batch.create_unit_atlas_texture(gpu, PAGE_SIZE, PAGE_SIZE, &self.pixels));
        }
    }

    pub fn texture(&self) -> Option<&BatchTexture> {
        self.texture.as_ref()
    }

    fn try_place(&mut self, sprite: &VxlSprite) -> Option<(u32, u32)> {
        let (w, h) = (sprite.width, sprite.height);
        if w > PAGE_SIZE || h > PAGE_SIZE {
            return None;
        }
        if self.cursor_x + w > PAGE_SIZE {
            self.cursor_y = self
                .cursor_y
                .saturating_add(self.shelf_height + SPRITE_PADDING);
            self.cursor_x = 0;
            self.shelf_height = 0;
        }
        if self.cursor_y + h > PAGE_SIZE {
            return None;
        }
        let position = (self.cursor_x, self.cursor_y);
        self.cursor_x = self.cursor_x.saturating_add(w + SPRITE_PADDING);
        self.shelf_height = self.shelf_height.max(h);
        Some(position)
    }

    fn blit(&mut self, sprite: &VxlSprite, px: u32, py: u32) {
        for y in 0..sprite.height {
            let src = (y * sprite.width) as usize;
            let dst = ((py + y) * PAGE_SIZE + px) as usize;
            self.pixels[dst..dst + sprite.width as usize]
                .copy_from_slice(&sprite.palette_indices[src..src + sprite.width as usize]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_frame_starts_an_empty_page() {
        let mut cache = VxlPoseFrameCache::default();
        cache.begin_frame();
        assert_eq!(cache.pixels.len(), (PAGE_SIZE * PAGE_SIZE) as usize);
        assert!(!cache.dirty && cache.texture().is_none());
    }
}
