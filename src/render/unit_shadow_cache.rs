//! Retained shadow stencil payloads within the existing unit atlas pages.
//!
//! Native 706BD0/707280 retains the first composed-body mask for a cache key.
//! This bounded owner keeps that history across frames and first-use draws,
//! until the shared pool's flush ([`crate::render::voxel_image_cache`])
//! frees it.

use super::{UnitAtlas, UnitSpriteEntry, UnitSpriteKey};

/// The exact visible body's indexed source and placement in Unit's 256-square
/// composite. Frame-local sources belong to VxlPoseFrameCache.
pub(crate) struct UnitShadowPart {
    pub entry: UnitSpriteEntry,
    pub point: [i32; 2],
    pub posed: bool,
}

impl UnitAtlas {
    /// [`Self::prepare_native_shadow_with`] stamping atlas parts at their
    /// anchor offsets.
    #[cfg(test)]
    pub(crate) fn prepare_native_shadow(
        &self,
        key: &UnitSpriteKey,
        flushes: u32,
        parts: impl IntoIterator<Item = (UnitSpriteEntry, [f32; 2])>,
    ) -> bool {
        self.prepare_native_shadow_with(key, flushes, |body| {
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

    /// Fill only on the first supported request since the image cache's
    /// `flushes`-th flush. `stamp` composes the exact parts and anchor
    /// offsets of this Unit's visible body draw. Hits perform no body
    /// composition, pixel filtering, or texture upload. A fill reaches the
    /// template's page at the next [`Self::upload`].
    pub(crate) fn prepare_native_shadow_with(
        &self,
        key: &UnitSpriteKey,
        flushes: u32,
        stamp: impl FnOnce(&mut [u8]) -> bool,
    ) -> bool {
        if self
            .shadow_masks
            .borrow()
            .get(key)
            .is_some_and(|(filled, _)| *filled == flushes)
        {
            return true;
        }
        let Some(sprite) = self.masked_shadow(key, [0, 0], stamp) else {
            return false;
        };
        let Some(entry) = self.get(key) else {
            return false;
        };
        self.resident
            .borrow_mut()
            .restage(&entry, sprite.palette_indices.clone());
        self.shadow_masks
            .borrow_mut()
            .insert(key.clone(), (flushes, sprite.palette_indices));
        true
    }

    /// The pool bytes `key`'s shadow image takes: its composed mask when one
    /// was retained, else its template.
    pub(crate) fn shadow_pool_bytes(&self, key: &UnitSpriteKey) -> Option<u32> {
        let masks = self.shadow_masks.borrow();
        let Some((_, mask)) = masks.get(key) else {
            return self.pool_bytes(key);
        };
        let entry = self.get(key)?;
        Some(crate::render::voxel_image_cache::sprite_pool_bytes(
            mask,
            entry.pixel_size.map(|side| side as u32),
            [entry.offset_x, entry.offset_y],
            None,
        ))
    }

    /// Sample an indexed atlas source through its owning cache. Placement is
    /// supplied by the body's actual draw pieces, including gun pivot offsets.
    pub(crate) fn stamp_shadow_part(
        &self,
        body: &mut [u8],
        part: &UnitSpriteEntry,
        point: [i32; 2],
    ) -> bool {
        let resident = self.resident.borrow();
        let Some(source) = resident.rendered_by_place(part) else {
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
        self.get(key).filter(|e| e.native_draw_bounds.is_some())?;
        // Copied out, so `stamp` may draw the body parts it asks for.
        let (mut pixels, width, height, offset, center) = {
            let resident = self.resident.borrow();
            let template = resident.rendered_by_key(key)?;
            (
                template.pixels.clone(),
                template.width,
                template.height,
                [template.offset_x, template.offset_y],
                template.shadow_center,
            )
        };
        let mut body = vec![0u8; 65536];
        if !stamp(&mut body) {
            return None;
        }
        let sample_offset = if body_offset == [0, 0] {
            [0, 0]
        } else {
            super::vxl_raster::shadow::body_sample_offset(center?, body_offset)?
        };
        super::vxl_raster::shadow::mask_body(
            &mut pixels,
            width,
            offset.map(|v| v as i32),
            &body,
            sample_offset,
        )?;
        Some(super::vxl_raster::VxlSprite {
            palette_indices: pixels,
            depth: Vec::new(),
            width,
            height,
            offset_x: offset[0],
            offset_y: offset[1],
        })
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
