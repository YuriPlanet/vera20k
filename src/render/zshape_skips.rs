//! The skip table: what the building bodies the view's left edge cuts read
//! their z-shape through.
//!
//! The RLE ZReadWrite leaf (`0x004990E0`) skips the columns left of the view a
//! token at a time and leaves the body's z-shape pointer behind by the rest of
//! the transparent run the skip ends in, per row
//! ([`zshape_skip_overshoot`]). That number comes from the frame's pixels, so
//! it is computed here on the CPU, once per row of each such body in a
//! picture, and uploaded as one byte per row into a texture bound beside the
//! z-shape (group 2, binding 1 of the Z-sprite pipelines). A body names its
//! slot in `fx_params.w`, 1-based; 0 is no table and no overshoot.
//! zsprite_shader.wgsl `skip_overshoot` reads one texel for it. Until 0.3.6 the
//! fragment shader walked the frame's indices itself, up to a run of 255
//! texels per pixel in two data-dependent loops, and an Intel HD 620 lost its
//! DirectX 12 device seconds into a 0.3.6 match; a lookup costs every pixel the
//! same.

use std::collections::HashMap;

use crate::render::batch::BatchRenderer;
use crate::render::native_z::zshape_skip_overshoot;
use crate::render::sprite_atlas::{ShpSpriteEntry, ShpSpriteKey, SpriteAtlas};

/// Rows a slot holds, the table's width: a body taller than this keeps no
/// overshoot below them. Retail building frames are a few hundred rows.
pub(crate) const SKIP_TABLE_ROWS: u32 = 1024;
/// Slots, the table's height: the bodies one picture can cut at its left
/// edge; any more keep no overshoot.
pub(crate) const SKIP_TABLE_SLOTS: u32 = 32;

#[derive(Default)]
pub(crate) struct ZShapeSkips {
    /// The palette indices of each body frame the left edge has cut, decoded
    /// once per atlas ([`Self::forget_frames`]); None for a frame the atlas
    /// cannot give or whose size disagrees with its entry.
    decoded: HashMap<ShpSpriteKey, Option<Vec<u8>>>,
    /// This picture's slots so far, `SKIP_TABLE_ROWS` bytes each.
    rows: Vec<u8>,
}

impl ZShapeSkips {
    /// The slot (1-based, for `fx_params.w`) of a body drawn from `entry` of
    /// `key` with `skip` source columns left of the view
    /// (`native_z::zshape_lead_skip`), or 0 when none of its rows trails or
    /// the table is full.
    pub(crate) fn slot(
        &mut self,
        atlas: &SpriteAtlas,
        key: &ShpSpriteKey,
        entry: &ShpSpriteEntry,
        skip: f32,
    ) -> f32 {
        let skip = skip as usize;
        let [width, height] = entry.pixel_size.map(|pixels| pixels as usize);
        if skip == 0 || skip >= width {
            return 0.0;
        }
        let Self { decoded, rows } = self;
        let Some(indices) = decoded.entry(key.clone()).or_insert_with(|| {
            atlas
                .frame_indices(key)
                .filter(|indices| indices.len() == width * height)
        }) else {
            return 0.0;
        };
        let per_slot = SKIP_TABLE_ROWS as usize;
        if rows.len() / per_slot >= SKIP_TABLE_SLOTS as usize {
            return 0.0;
        }
        let mut slot = [0u8; SKIP_TABLE_ROWS as usize];
        for (overshoot, row) in slot.iter_mut().zip(indices.chunks_exact(width)) {
            *overshoot = zshape_skip_overshoot(row, skip);
        }
        if slot.iter().all(|&overshoot| overshoot == 0) {
            return 0.0;
        }
        rows.extend_from_slice(&slot);
        (rows.len() / per_slot) as f32
    }

    /// Write this picture's slots and start the next picture's. Called once
    /// the picture's bodies are built, before its command buffers go.
    pub(crate) fn upload(&mut self, batch: &BatchRenderer, queue: &wgpu::Queue) {
        batch.write_zshape_skips(queue, &self.rows);
        self.rows.clear();
    }

    /// Forget the decoded frames: the atlas they came from is replaced.
    pub(crate) fn forget_frames(&mut self) {
        self.decoded.clear();
    }
}
