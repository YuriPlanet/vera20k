//! Per-frame sprites of crashing Fly bodies.
//!
//! `FlyLocomotionClass` Draw_Matrix's crashing arm (`0x004CF610`) keys its
//! draw -1, so the native voxel cache never holds a crash pose: the body is
//! rasterized afresh every frame with its current roll and pitch. This page
//! does the same for the crashing bodies on screen. It is rebuilt each
//! presentation frame ([`VxlPoseFrameCache::begin_frame`]) and its used rows
//! are written into one reused GPU page after the unit instances are built
//! ([`VxlPoseFrameCache::upload`]).

use std::collections::BTreeMap;

use crate::assets::asset_manager::AssetManager;
use crate::assets::vpl_file::VplFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::gpu::GpuContext;
use crate::render::unit_atlas::{UnitModel, UnitSpriteEntry, UnitSpriteKey};
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
    /// The GPU page, created on the first crash pose and rewritten in place.
    texture: Option<BatchTexture>,
    /// `VOXELS.VPL`, parsed on the first crash pose.
    vpl: Option<Option<VplFile>>,
    /// Each crashing type's voxel model, parsed on its first crash pose.
    models: BTreeMap<String, Option<UnitModel>>,
}

impl VxlPoseFrameCache {
    /// Forget last frame's poses. The GPU page keeps its old texels until the
    /// next upload; no instance references them.
    pub fn begin_frame(&mut self) {
        if self.pixels.is_empty() {
            self.pixels = vec![0; (PAGE_SIZE * PAGE_SIZE) as usize];
        } else if self.dirty {
            let used = (self.used_rows() * PAGE_SIZE) as usize;
            self.pixels[..used].fill(0);
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.shelf_height = 0;
        self.dirty = false;
    }

    /// Rasterize one crashing body at `tilt` (roll, pitch in radians) and place
    /// it on this frame's page. None when the model does not resolve or the
    /// page is full.
    pub fn render(
        &mut self,
        asset_manager: &AssetManager,
        rules: Option<&RuleSet>,
        art: Option<&ArtRegistry>,
        key: &UnitSpriteKey,
        tilt: [f32; 2],
    ) -> Option<UnitSpriteEntry> {
        if self.pixels.is_empty() {
            self.begin_frame();
        }
        let vpl = self
            .vpl
            .get_or_insert_with(|| {
                asset_manager
                    .get_ref("VOXELS.VPL")
                    .and_then(|data| VplFile::from_bytes(data).ok())
            })
            .as_ref();
        let model = self
            .models
            .entry(key.type_id.clone())
            .or_insert_with(|| UnitModel::load(asset_manager, &key.type_id, rules, art))
            .as_ref()?;
        let (sprite, native_draw_bounds) = model.render_crash_pose(key, vpl, tilt);
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

    /// Write this frame's used rows into the GPU page when anything was
    /// placed on it.
    pub fn upload(&mut self, gpu: &GpuContext, batch: &BatchRenderer) {
        if !self.dirty {
            return;
        }
        let rows = self.used_rows();
        let texture = self.texture.get_or_insert_with(|| {
            batch.create_blank_unit_atlas_texture(&gpu.device, PAGE_SIZE, PAGE_SIZE)
        });
        crate::render::atlas_growth::write_texels(
            &gpu.queue,
            texture.view.texture(),
            [0, 0],
            [PAGE_SIZE, rows],
            1,
            &self.pixels[..(rows * PAGE_SIZE) as usize],
        );
    }

    pub fn texture(&self) -> Option<&BatchTexture> {
        self.texture.as_ref()
    }

    /// Rows holding this frame's sprites: every closed shelf and the open one.
    fn used_rows(&self) -> u32 {
        (self.cursor_y + self.shelf_height).min(PAGE_SIZE)
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

    /// Sprites pack left to right on shelves; the upload covers exactly the
    /// rows they use, and the next frame clears those rows.
    #[test]
    fn placed_sprites_bound_the_uploaded_rows() {
        let sprite = |width: u32, height: u32| VxlSprite {
            palette_indices: vec![7; (width * height) as usize],
            depth: vec![0.0; (width * height) as usize],
            width,
            height,
            offset_x: 0.0,
            offset_y: 0.0,
        };
        let mut cache = VxlPoseFrameCache::default();
        cache.begin_frame();
        let a = sprite(600, 40);
        let b = sprite(600, 30);
        let first = cache.try_place(&a).unwrap();
        cache.blit(&a, first.0, first.1);
        let second = cache.try_place(&b).unwrap();
        cache.blit(&b, second.0, second.1);
        cache.dirty = true;
        assert_eq!((first, second), ((0, 0), (0, 40 + SPRITE_PADDING)));
        assert_eq!(cache.used_rows(), 40 + SPRITE_PADDING + 30);
        cache.begin_frame();
        assert!(cache.pixels.iter().all(|&pixel| pixel == 0));
        assert_eq!(cache.used_rows(), 0);
    }
}
