//! Texture atlas for terrain tiles — packs many tile images into one GPU texture.
//!
//! Instead of one GPU texture per tile (expensive context switches), all tile
//! images are packed into a single large texture. Each tile gets a UV rectangle
//! that the sprite batch shader uses to sample the correct region.
//!
//! ## Atlas layout
//! Tiles are shelf-packed: sorted by height (tallest first), placed left-to-right
//! in rows. This handles variable-size tiles (e.g., 60×55 cliff tiles with extra
//! data) without clipping.
//!
//! ## Dependency rules
//! - Part of render/ — depends on render/gpu, render/batch.
//! - Reads TileKey/TileImage from map/theater (simple data types, no logic).

use std::collections::HashMap;

use crate::map::theater::{TileImage, TileKey};
use crate::render::batch::BatchRenderer;
use crate::render::batch::BatchTexture;

/// Padding between tiles in the atlas (pixels). Prevents texture bleeding
/// from bilinear filtering at tile boundaries.
const TILE_PADDING: u32 = 1;

/// UV coordinates and rendering metadata for a tile within the atlas texture.
#[derive(Debug, Clone, Copy)]
pub struct TileUV {
    /// Top-left UV coordinate in the atlas (0.0..1.0).
    pub uv_origin: [f32; 2],
    /// UV width and height (0.0..1.0).
    pub uv_size: [f32; 2],
    /// Actual pixel dimensions of this tile image.
    pub pixel_size: [f32; 2],
    /// Draw offset from the standard diamond origin (in pixels).
    /// A cliff tile with extra data above the diamond has draw_offset[1] < 0.
    pub draw_offset: [f32; 2],
}

/// A GPU texture atlas containing all terrain tiles for the current map.
///
/// Created once at map load time. The batch renderer uses `get_uv()` to
/// look up per-tile UV coordinates when building sprite instances.
pub struct TileAtlas {
    /// The GPU texture containing all packed tiles.
    pub texture: BatchTexture,
    /// Parallel R8 depth atlas — same UV layout as color, one byte per pixel.
    /// Contains per-pixel Z-depth from TMP files for occlusion (cliffs, ramps).
    pub depth_texture_view: wgpu::TextureView,
    /// Pre-built bind group for the Z-depth pipeline (color + sampler + R8 depth).
    /// Used by `draw_with_buffer_zdepth()` in the render pass.
    pub zdepth_bind_group: wgpu::BindGroup,
    /// Tile key → UV rectangle + rendering metadata.
    uv_map: HashMap<TileKey, TileUV>,
}

impl TileAtlas {
    /// Look up UV and rendering data for a tile. Returns None if not in the atlas.
    pub fn get_uv(&self, key: TileKey) -> Option<TileUV> {
        self.uv_map.get(&key).copied()
    }

    /// Number of tiles in the atlas.
    pub fn tile_count(&self) -> usize {
        self.uv_map.len()
    }
}

/// Build a texture atlas from pre-loaded tile images using shelf packing.
///
/// Tiles are sorted by height (tallest first) for packing density, then placed
/// left-to-right in rows. Each tile is stored at its actual dimensions — no
/// clipping of oversized tiles with extra data (cliffs, shores).
pub fn build_atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    batch: &BatchRenderer,
    tiles: &HashMap<TileKey, TileImage>,
) -> TileAtlas {
    if tiles.is_empty() {
        let rgba: Vec<u8> = vec![0u8; 4];
        let texture: BatchTexture =
            batch.create_texture_on_device(device, queue, &rgba, 1, 1, None);
        let depth_texture_view = create_r8_texture(device, queue, &[0u8], 1, 1);
        let zdepth_bind_group =
            batch.create_zdepth_bind_group_on_device(device, &texture.view, &depth_texture_view);
        return TileAtlas {
            texture,
            depth_texture_view,
            zdepth_bind_group,
            uv_map: HashMap::new(),
        };
    }

    // Sort by height descending for better shelf packing, then by key for determinism.
    let mut sorted_keys: Vec<TileKey> = tiles.keys().copied().collect();
    sorted_keys.sort_by(|a, b| {
        let ha: u32 = tiles[a].height;
        let hb: u32 = tiles[b].height;
        hb.cmp(&ha)
            .then(a.tile_id.cmp(&b.tile_id))
            .then(a.sub_tile.cmp(&b.sub_tile))
    });

    // Estimate atlas width from total tile area (sqrt heuristic).
    let total_area: u64 = sorted_keys
        .iter()
        .map(|k| {
            (tiles[k].width as u64 + TILE_PADDING as u64)
                * (tiles[k].height as u64 + TILE_PADDING as u64)
        })
        .sum();
    let estimated_side: u32 = (total_area as f64).sqrt().ceil() as u32;
    let max_texture_dim: u32 = device.limits().max_texture_dimension_2d;
    let mut atlas_width: u32 = estimated_side.max(64).min(max_texture_dim);

    // Shelf-pack with retry: widen atlas if height exceeds GPU texture limit.
    let placements: Vec<(TileKey, u32, u32)>;
    let atlas_height: u32;
    loop {
        let trial: Vec<(TileKey, u32, u32)> = shelf_pack(&sorted_keys, tiles, atlas_width);
        let trial_height: u32 = trial
            .iter()
            .map(|&(key, _, py)| py + tiles[&key].height)
            .max()
            .unwrap_or(1);
        if trial_height <= max_texture_dim {
            placements = trial;
            atlas_height = trial_height;
            break;
        }
        if atlas_width >= max_texture_dim {
            // Can't grow wider — use what we have and hope for the best.
            log::warn!(
                "Tile atlas height {} exceeds GPU limit {} even at max width {}",
                trial_height,
                max_texture_dim,
                atlas_width
            );
            placements = trial;
            atlas_height = trial_height.min(max_texture_dim);
            break;
        }
        atlas_width = (atlas_width.saturating_mul(2)).min(max_texture_dim);
    }

    // Allocate RGBA color buffer and R8 depth buffer (same layout), then blit tiles.
    let mut rgba: Vec<u8> = vec![0u8; (atlas_width * atlas_height * 4) as usize];
    let mut depth_buf: Vec<u8> = vec![0u8; (atlas_width * atlas_height) as usize];
    let mut uv_map: HashMap<TileKey, TileUV> = HashMap::with_capacity(placements.len());
    let aw: f32 = atlas_width as f32;
    let ah: f32 = atlas_height as f32;

    for &(key, px, py) in &placements {
        let tile: &TileImage = &tiles[&key];
        blit_tile(&mut rgba, atlas_width, px, py, tile);
        blit_depth(&mut depth_buf, atlas_width, px, py, tile);

        uv_map.insert(
            key,
            TileUV {
                uv_origin: [px as f32 / aw, py as f32 / ah],
                uv_size: [tile.width as f32 / aw, tile.height as f32 / ah],
                pixel_size: [tile.width as f32, tile.height as f32],
                draw_offset: [tile.offset_x as f32, tile.offset_y as f32],
            },
        );
    }

    log::info!(
        "Tile atlas: {}x{} px ({:.1} MB), {} tiles (shelf-packed)",
        atlas_width,
        atlas_height,
        (atlas_width as u64 * atlas_height as u64 * 4) as f64 / (1024.0 * 1024.0),
        uv_map.len()
    );

    let texture: BatchTexture =
        batch.create_texture_on_device(device, queue, &rgba, atlas_width, atlas_height, None);
    let depth_texture_view =
        create_r8_texture(device, queue, &depth_buf, atlas_width, atlas_height);
    let zdepth_bind_group =
        batch.create_zdepth_bind_group_on_device(device, &texture.view, &depth_texture_view);
    log::info!(
        "Tile depth atlas: {}x{} R8 ({:.1} KB)",
        atlas_width,
        atlas_height,
        (atlas_width as u64 * atlas_height as u64) as f64 / 1024.0,
    );
    TileAtlas {
        texture,
        depth_texture_view,
        zdepth_bind_group,
        uv_map,
    }
}

/// Shelf-pack tiles into rows within the given atlas width.
/// Returns (key, x, y) placement for each tile.
fn shelf_pack(
    sorted_keys: &[TileKey],
    tiles: &HashMap<TileKey, TileImage>,
    atlas_width: u32,
) -> Vec<(TileKey, u32, u32)> {
    let mut placements: Vec<(TileKey, u32, u32)> = Vec::with_capacity(sorted_keys.len());
    let mut cursor_x: u32 = 0;
    let mut cursor_y: u32 = 0;
    let mut shelf_height: u32 = 0;

    for &key in sorted_keys {
        let tile: &TileImage = &tiles[&key];

        // Start new shelf if tile doesn't fit on current row.
        if cursor_x + tile.width > atlas_width {
            cursor_y += shelf_height + TILE_PADDING;
            cursor_x = 0;
            shelf_height = 0;
        }

        placements.push((key, cursor_x, cursor_y));
        cursor_x += tile.width + TILE_PADDING;
        shelf_height = shelf_height.max(tile.height);
    }

    placements
}

/// Create an R8Unorm GPU texture from a single-channel byte buffer.
fn create_r8_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    data: &[u8],
    width: u32,
    height: u32,
) -> wgpu::TextureView {
    use wgpu::util::DeviceExt;
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("Tile Depth Atlas R8"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        data,
    );
    texture.create_view(&Default::default())
}

/// Copy tile depth bytes (R8) into the depth atlas buffer at position (px, py).
fn blit_depth(depth_buf: &mut [u8], atlas_width: u32, px: u32, py: u32, tile: &TileImage) {
    for y in 0..tile.height {
        let src_start: usize = (y * tile.width) as usize;
        let src_end: usize = src_start + tile.width as usize;
        let dst_start: usize = ((py + y) * atlas_width + px) as usize;
        let dst_end: usize = dst_start + tile.width as usize;
        if src_end <= tile.depth.len() && dst_end <= depth_buf.len() {
            depth_buf[dst_start..dst_end].copy_from_slice(&tile.depth[src_start..src_end]);
        }
    }
}

/// Copy tile RGBA pixels into the atlas buffer at position (px, py).
fn blit_tile(rgba: &mut [u8], atlas_width: u32, px: u32, py: u32, tile: &TileImage) {
    for y in 0..tile.height {
        let src_start: usize = (y * tile.width * 4) as usize;
        let src_end: usize = src_start + (tile.width * 4) as usize;
        let dst_start: usize = (((py + y) * atlas_width + px) * 4) as usize;
        let dst_end: usize = dst_start + (tile.width * 4) as usize;
        if src_end <= tile.rgba.len() && dst_end <= rgba.len() {
            rgba[dst_start..dst_end].copy_from_slice(&tile.rgba[src_start..src_end]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a solid-color tile image with given dimensions and offset.
    fn make_tile_sized(w: u32, h: u32, ox: i32, oy: i32) -> TileImage {
        let mut rgba: Vec<u8> = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            rgba.extend_from_slice(&[128, 128, 128, 255]);
        }
        TileImage {
            rgba,
            depth: vec![0u8; (w * h) as usize],
            width: w,
            height: h,
            offset_x: ox,
            offset_y: oy,
        }
    }

    /// Create a standard 60×30 tile.
    fn make_tile(r: u8, g: u8, b: u8) -> TileImage {
        let mut rgba: Vec<u8> = Vec::with_capacity(60 * 30 * 4);
        for _ in 0..(60 * 30) {
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
        TileImage {
            rgba,
            depth: vec![0u8; 60 * 30],
            width: 60,
            height: 30,
            offset_x: 0,
            offset_y: 0,
        }
    }

    #[test]
    fn test_shelf_packing_basic() {
        // 4 standard 60×30 tiles should pack into rows.
        let mut tiles: HashMap<TileKey, TileImage> = HashMap::new();
        tiles.insert(
            TileKey {
                variant: 0,
                tile_id: 0,
                sub_tile: 0,
            },
            make_tile(255, 0, 0),
        );
        tiles.insert(
            TileKey {
                variant: 0,
                tile_id: 1,
                sub_tile: 0,
            },
            make_tile(0, 255, 0),
        );
        tiles.insert(
            TileKey {
                variant: 0,
                tile_id: 2,
                sub_tile: 0,
            },
            make_tile(0, 0, 255),
        );
        tiles.insert(
            TileKey {
                variant: 0,
                tile_id: 3,
                sub_tile: 0,
            },
            make_tile(255, 255, 0),
        );

        let mut sorted: Vec<TileKey> = tiles.keys().copied().collect();
        sorted.sort_by_key(|a| a.tile_id);

        // Atlas width of 130 fits 2 tiles per row (60 + 1 + 60 = 121 < 130).
        let placements: Vec<(TileKey, u32, u32)> = shelf_pack(&sorted, &tiles, 130);
        assert_eq!(placements.len(), 4);
        // First two on row 0, next two on row 1.
        assert_eq!(placements[0].1, 0); // x=0
        assert_eq!(placements[0].2, 0); // y=0
        assert_eq!(placements[1].1, 61); // x=60+1 padding
        assert_eq!(placements[1].2, 0); // y=0
        assert_eq!(placements[2].2, 31); // y=30+1 padding (new shelf)
    }

    #[test]
    fn test_oversized_tile_not_clipped() {
        // A cliff tile 60×55 should be stored at full size.
        let mut tiles: HashMap<TileKey, TileImage> = HashMap::new();
        tiles.insert(
            TileKey {
                variant: 0,
                tile_id: 0,
                sub_tile: 0,
            },
            make_tile_sized(60, 55, 0, -25),
        );

        let mut sorted: Vec<TileKey> = tiles.keys().copied().collect();
        sorted.sort_by_key(|a| a.tile_id);

        let placements: Vec<(TileKey, u32, u32)> = shelf_pack(&sorted, &tiles, 256);
        assert_eq!(placements.len(), 1);

        let atlas_h: u32 = placements
            .iter()
            .map(|&(k, _, py)| py + tiles[&k].height)
            .max()
            .unwrap();
        // Atlas must be at least 55 pixels tall.
        assert!(atlas_h >= 55);
    }

    #[test]
    fn test_mixed_sizes_packing() {
        // Mix of standard and oversized tiles.
        let mut tiles: HashMap<TileKey, TileImage> = HashMap::new();
        tiles.insert(
            TileKey {
                variant: 0,
                tile_id: 0,
                sub_tile: 0,
            },
            make_tile_sized(60, 30, 0, 0),
        );
        tiles.insert(
            TileKey {
                variant: 0,
                tile_id: 1,
                sub_tile: 0,
            },
            make_tile_sized(60, 55, 0, -25),
        );
        tiles.insert(
            TileKey {
                variant: 0,
                tile_id: 2,
                sub_tile: 0,
            },
            make_tile_sized(80, 45, -10, -15),
        );

        let mut sorted: Vec<TileKey> = tiles.keys().copied().collect();
        // Sort by height descending (tallest first) like build_atlas does.
        sorted.sort_by(|a, b| tiles[b].height.cmp(&tiles[a].height));

        let placements: Vec<(TileKey, u32, u32)> = shelf_pack(&sorted, &tiles, 256);
        assert_eq!(placements.len(), 3);

        // Verify no overlaps: each tile's rectangle doesn't intersect others.
        for i in 0..placements.len() {
            for j in (i + 1)..placements.len() {
                let (ki, xi, yi) = placements[i];
                let (kj, xj, yj) = placements[j];
                let wi: u32 = tiles[&ki].width;
                let hi: u32 = tiles[&ki].height;
                let wj: u32 = tiles[&kj].width;
                let hj: u32 = tiles[&kj].height;
                // Rectangles don't overlap if separated on either axis.
                let no_overlap: bool = xi + wi + TILE_PADDING <= xj
                    || xj + wj + TILE_PADDING <= xi
                    || yi + hi + TILE_PADDING <= yj
                    || yj + hj + TILE_PADDING <= yi;
                assert!(no_overlap, "Tiles {} and {} overlap", i, j);
            }
        }
    }

    #[test]
    fn test_tile_uv_metadata() {
        // Verify TileUV carries correct pixel_size and draw_offset.
        let mut tiles: HashMap<TileKey, TileImage> = HashMap::new();
        tiles.insert(
            TileKey {
                variant: 0,
                tile_id: 0,
                sub_tile: 0,
            },
            make_tile_sized(60, 55, 0, -25),
        );

        let sorted: Vec<TileKey> = vec![TileKey {
            tile_id: 0,
            sub_tile: 0,
            variant: 0,
        }];
        let placements: Vec<(TileKey, u32, u32)> = shelf_pack(&sorted, &tiles, 256);
        let (key, px, py) = placements[0];
        let tile: &TileImage = &tiles[&key];
        let atlas_w: f32 = 256.0;
        let atlas_h: f32 = 55.0;

        let uv: TileUV = TileUV {
            uv_origin: [px as f32 / atlas_w, py as f32 / atlas_h],
            uv_size: [tile.width as f32 / atlas_w, tile.height as f32 / atlas_h],
            pixel_size: [tile.width as f32, tile.height as f32],
            draw_offset: [tile.offset_x as f32, tile.offset_y as f32],
        };

        assert!((uv.pixel_size[0] - 60.0).abs() < f32::EPSILON);
        assert!((uv.pixel_size[1] - 55.0).abs() < f32::EPSILON);
        assert!((uv.draw_offset[0] - 0.0).abs() < f32::EPSILON);
        assert!((uv.draw_offset[1] - (-25.0)).abs() < f32::EPSILON);
    }
}
