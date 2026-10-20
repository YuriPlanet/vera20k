//! Retained shadow stencil payloads within the existing unit atlas pages.
//!
//! Native 706BD0/707280 retains the first composed-body mask for a cache key.
//! This bounded owner keeps that history across frames and atlas growth. It
//! does not emulate the original shared 4 MB arena's global purge policy.

use super::{UnitAtlas, UnitSpriteEntry, UnitSpriteKey};

/// The exact visible body's indexed source and placement in Unit's 256-square
/// composite. Frame-local sources belong to VxlPoseFrameCache.
pub(crate) struct UnitShadowPart {
    pub entry: UnitSpriteEntry,
    pub point: [i32; 2],
    pub posed: bool,
}

impl UnitAtlas {
    /// Fill only on the first supported request. Parts are the exact atlas
    /// entries and anchor offsets used by this Unit's visible body draw.
    /// Hits perform no body composition, pixel filtering, or texture upload.
    pub(crate) fn prepare_native_shadow(
        &self,
        queue: &wgpu::Queue,
        key: &UnitSpriteKey,
        parts: impl IntoIterator<Item = (UnitSpriteEntry, [f32; 2])>,
    ) -> bool {
        self.prepare_native_shadow_with(queue, key, |body| {
            let mut any = false;
            for (part, offset) in parts {
                any = true;
                let point = [
                    128 + part.offset_x as i32 + offset[0] as i32,
                    128 + part.offset_y as i32 + offset[1] as i32,
                ];
                if !self.stamp_shadow_part(body, &part, point) {
                    return false;
                }
            }
            any
        })
    }

    pub(crate) fn prepare_native_shadow_with(
        &self,
        queue: &wgpu::Queue,
        key: &UnitSpriteKey,
        stamp: impl FnOnce(&mut [u8]) -> bool,
    ) -> bool {
        if self.shadow_masks.borrow().contains_key(key) {
            return true;
        }
        let Some(entry) = self
            .entries
            .get(key)
            .filter(|e| e.native_draw_bounds.is_some())
        else {
            return false;
        };
        let Some(sprite) = self.masked_shadow(key, [0, 0], stamp) else {
            return false;
        };
        self.upload_shadow_pixels(queue, entry, &sprite.palette_indices);
        self.shadow_masks
            .borrow_mut()
            .insert(key.clone(), sprite.palette_indices);
        true
    }

    /// Sample an indexed atlas source through its owning cache. Placement is
    /// supplied by the body's actual draw pieces, including gun pivot offsets.
    pub(crate) fn stamp_shadow_part(
        &self,
        body: &mut [u8],
        part: &UnitSpriteEntry,
        point: [i32; 2],
    ) -> bool {
        let Some(source) = self.rendered_cache.iter().find(|s| {
            self.entries.get(&s.key).is_some_and(|e| {
                e.page == part.page && e.uv_origin == part.uv_origin && e.uv_size == part.uv_size
            })
        }) else {
            return false;
        };
        composite_mask_part(body, &source.pixels, source.width, source.height, point);
        true
    }

    /// The unmodified template remains authoritative even after a retained
    /// mask has been uploaded. Positive low flight requests a fresh mask and
    /// places this result on the existing frame-local pose pages.
    pub(crate) fn masked_shadow(
        &self,
        key: &UnitSpriteKey,
        body_offset: [i32; 2],
        stamp: impl FnOnce(&mut [u8]) -> bool,
    ) -> Option<super::vxl_raster::VxlSprite> {
        self.entries
            .get(key)
            .filter(|e| e.native_draw_bounds.is_some())?;
        let template = self.rendered_cache.iter().find(|s| &s.key == key)?;
        let mut body = vec![0u8; 65536];
        if !stamp(&mut body) {
            return None;
        }
        let mut pixels = template.pixels.clone();
        let sample_offset = if body_offset == [0, 0] {
            [0, 0]
        } else {
            super::vxl_raster::shadow::body_sample_offset(template.shadow_center?, body_offset)?
        };
        super::vxl_raster::shadow::mask_body(
            &mut pixels,
            template.width,
            [template.offset_x as i32, template.offset_y as i32],
            &body,
            sample_offset,
        )?;
        Some(super::vxl_raster::VxlSprite {
            palette_indices: pixels,
            depth: Vec::new(),
            width: template.width,
            height: template.height,
            offset_x: template.offset_x,
            offset_y: template.offset_y,
        })
    }

    fn upload_shadow_pixels(&self, queue: &wgpu::Queue, entry: &UnitSpriteEntry, pixels: &[u8]) {
        let page = &self.pages[entry.page].texture;
        let origin = [
            (entry.uv_origin[0] * page.width as f32).round() as u32,
            (entry.uv_origin[1] * page.height as f32).round() as u32,
        ];
        let size = entry.pixel_size.map(|v| v as u32);
        crate::render::atlas_growth::write_texels(
            queue,
            page.view.texture(),
            origin,
            size,
            1,
            pixels,
        );
    }
}

/// 4914C0 inner indexed copy skips zero source; later parts overwrite only
/// nonzero pixels. For the shadow query the composed nonzero domain suffices.
pub(crate) fn composite_mask_part(
    body: &mut [u8],
    source: &[u8],
    width: u32,
    height: u32,
    point: [i32; 2],
) {
    for y in 0..height {
        let dy = point[1] + y as i32;
        if !(0..256).contains(&dy) {
            continue;
        }
        for x in 0..width {
            let dx = point[0] + x as i32;
            if !(0..256).contains(&dx) {
                continue;
            }
            if source[(y * width + x) as usize] != 0 {
                body[(dy * 256 + dx) as usize] = 1;
            }
        }
    }
}

#[cfg(test)]
#[path = "unit_shadow_tests.rs"]
mod tests;
