//! Transient VXL slope-transition sprite cache.
//!
//! Stable unit sprites stay in `UnitAtlas`. This cache only materializes
//! gamemd's short 3-frame blended slope sprites that are actually visible.

use std::collections::HashMap;

use crate::assets::asset_manager::AssetManager;
use crate::assets::vpl_file::VplFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::gpu::GpuContext;
use crate::render::unit_atlas::{
    UnitSpriteEntry, UnitSpriteKey, VxlLayer, render_unit_sprite_with_slope_blend,
};
use crate::render::vxl_raster::{VxlSlopeBlend, VxlSprite};
use crate::rules::art_data::ArtRegistry;
use crate::rules::ruleset::RuleSet;

const PAGE_SIZE: u32 = 2048;
const SPRITE_PADDING: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransitionUnitSpriteKey {
    pub type_id: String,
    pub facing: u8,
    pub layer: VxlLayer,
    pub frame: u32,
    pub from_slope: u8,
    pub to_slope: u8,
    pub phase_num: i32,
    pub phase_den: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct TransitionUnitSpriteEntry {
    pub page: usize,
    pub entry: UnitSpriteEntry,
}

pub struct TransitionAtlasPage {
    pub texture: Option<BatchTexture>,
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    cursor_x: u32,
    cursor_y: u32,
    shelf_height: u32,
}

impl TransitionAtlasPage {
    fn new(size: u32) -> Self {
        Self {
            texture: None,
            pixels: vec![0; (size * size) as usize],
            width: size,
            height: size,
            cursor_x: 0,
            cursor_y: 0,
            shelf_height: 0,
        }
    }

    fn try_place(&mut self, sprite: &VxlSprite) -> Option<(u32, u32)> {
        let w = sprite.width;
        let h = sprite.height;
        if w > self.width || h > self.height {
            return None;
        }
        if self.cursor_x + w > self.width {
            self.cursor_y = self
                .cursor_y
                .saturating_add(self.shelf_height + SPRITE_PADDING);
            self.cursor_x = 0;
            self.shelf_height = 0;
        }
        if self.cursor_y + h > self.height {
            return None;
        }
        let px = self.cursor_x;
        let py = self.cursor_y;
        self.cursor_x = self.cursor_x.saturating_add(w + SPRITE_PADDING);
        self.shelf_height = self.shelf_height.max(h);
        Some((px, py))
    }

    fn blit(&mut self, sprite: &VxlSprite, px: u32, py: u32) {
        for y in 0..sprite.height {
            let src_start = (y * sprite.width) as usize;
            let src_end = src_start + sprite.width as usize;
            let dst_start = ((py + y) * self.width + px) as usize;
            let dst_end = dst_start + sprite.width as usize;
            self.pixels[dst_start..dst_end]
                .copy_from_slice(&sprite.palette_indices[src_start..src_end]);
        }
    }

    fn upload(&mut self, gpu: &GpuContext, batch: &BatchRenderer) {
        self.texture =
            Some(batch.create_unit_atlas_texture(gpu, self.width, self.height, &self.pixels));
    }
}

#[derive(Default)]
pub struct VxlSlopeTransitionCache {
    entries: HashMap<TransitionUnitSpriteKey, TransitionUnitSpriteEntry>,
    pages: Vec<TransitionAtlasPage>,
}

impl VxlSlopeTransitionCache {
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn page_texture(&self, page: usize) -> Option<&BatchTexture> {
        self.pages.get(page).and_then(|p| p.texture.as_ref())
    }

    pub fn get_or_render(
        &mut self,
        gpu: &GpuContext,
        batch: &BatchRenderer,
        asset_manager: &AssetManager,
        rules: Option<&RuleSet>,
        art: Option<&ArtRegistry>,
        key: TransitionUnitSpriteKey,
    ) -> Option<TransitionUnitSpriteEntry> {
        if let Some(entry) = self.entries.get(&key).copied() {
            return Some(entry);
        }

        let vpl = asset_manager
            .get_ref("VOXELS.VPL")
            .and_then(|data| VplFile::from_bytes(data).ok());
        let render_key = UnitSpriteKey {
            type_id: key.type_id.clone(),
            facing: key.facing,
            layer: key.layer,
            frame: key.frame,
            slope_type: key.to_slope,
        };
        let blend = VxlSlopeBlend {
            from_slope: key.from_slope,
            to_slope: key.to_slope,
            phase_num: key.phase_num,
            phase_den: key.phase_den,
        };
        let (sprite, _, native_draw_bounds) = render_unit_sprite_with_slope_blend(
            asset_manager,
            &render_key,
            rules,
            art,
            vpl.as_ref(),
            None,
            &gpu.device,
            &gpu.queue,
            Some(blend),
        )?;

        let page_size = PAGE_SIZE.min(gpu.device.limits().max_texture_dimension_2d);
        for page_index in 0..self.pages.len() {
            if let Some((px, py)) = self.pages[page_index].try_place(&sprite) {
                return Some(self.insert_into_page(
                    gpu,
                    batch,
                    key,
                    sprite,
                    native_draw_bounds,
                    page_index,
                    px,
                    py,
                ));
            }
        }

        let mut page = TransitionAtlasPage::new(page_size);
        let (px, py) = page.try_place(&sprite)?;
        self.pages.push(page);
        let page_index = self.pages.len() - 1;
        Some(self.insert_into_page(
            gpu,
            batch,
            key,
            sprite,
            native_draw_bounds,
            page_index,
            px,
            py,
        ))
    }

    fn insert_into_page(
        &mut self,
        gpu: &GpuContext,
        batch: &BatchRenderer,
        key: TransitionUnitSpriteKey,
        sprite: VxlSprite,
        native_draw_bounds: Option<[i32; 4]>,
        page_index: usize,
        px: u32,
        py: u32,
    ) -> TransitionUnitSpriteEntry {
        let page = &mut self.pages[page_index];
        page.blit(&sprite, px, py);
        page.upload(gpu, batch);
        let entry = TransitionUnitSpriteEntry {
            page: page_index,
            entry: UnitSpriteEntry {
                uv_origin: [
                    px as f32 / page.width as f32,
                    py as f32 / page.height as f32,
                ],
                uv_size: [
                    sprite.width as f32 / page.width as f32,
                    sprite.height as f32 / page.height as f32,
                ],
                pixel_size: [sprite.width as f32, sprite.height as f32],
                offset_x: sprite.offset_x,
                offset_y: sprite.offset_y,
                native_draw_bounds,
                page: page_index,
            },
        };
        self.entries.insert(key, entry);
        entry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(from_slope: u8, to_slope: u8, phase_num: i32) -> TransitionUnitSpriteKey {
        TransitionUnitSpriteKey {
            type_id: "CMIN".to_string(),
            facing: 64,
            layer: VxlLayer::Composite,
            frame: 0,
            from_slope,
            to_slope,
            phase_num,
            phase_den: crate::sim::movement::slope_transition::SLOPE_TRANSITION_FRAMES,
        }
    }

    #[test]
    fn vxl_slope_transition_cache_key_distinguishes_from_to_phase() {
        assert_ne!(key(1, 2, 0), key(2, 1, 0));
        assert_ne!(key(1, 2, 0), key(1, 2, 1));
    }

    #[test]
    fn vxl_slope_transition_cache_starts_empty() {
        let cache = VxlSlopeTransitionCache::default();
        assert_eq!(cache.page_count(), 0);
        assert!(cache.page_texture(0).is_none());
    }
}
