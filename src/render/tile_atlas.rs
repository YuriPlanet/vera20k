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
use crate::render::atlas_growth;
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

    // The shared shelf plan sorts tallest first and keeps input order between
    // equal heights, so key order breaks those ties deterministically.
    let mut sorted_keys: Vec<TileKey> = tiles.keys().copied().collect();
    sorted_keys.sort_by_key(|key| (key.tile_id, key.sub_tile));
    let sizes: Vec<[u32; 2]> = sorted_keys
        .iter()
        .map(|key| [tiles[key].width, tiles[key].height])
        .collect();
    let max_texture_dim: u32 = device.limits().max_texture_dimension_2d;
    let plan = atlas_growth::plan_initial_shelves(
        &sizes,
        atlas_growth::InitialShelfPolicy {
            padding: TILE_PADDING,
            max_dim: max_texture_dim,
            split_pages: false,
        },
    );
    let atlas_width = plan.width;
    let used_height = plan.single_page_height();
    if used_height > max_texture_dim {
        // Can't grow wider — use what we have and hope for the best.
        log::warn!(
            "Tile atlas height {} exceeds GPU limit {} even at max width {}",
            used_height,
            max_texture_dim,
            atlas_width
        );
    }
    let atlas_height = used_height.min(max_texture_dim);

    // Allocate RGBA color buffer and R8 depth buffer (same layout), then blit tiles.
    let mut rgba: Vec<u8> = vec![0u8; (atlas_width * atlas_height * 4) as usize];
    let mut depth_buf: Vec<u8> = vec![0u8; (atlas_width * atlas_height) as usize];
    let mut uv_map: HashMap<TileKey, TileUV> = HashMap::with_capacity(plan.placements.len());
    let aw: f32 = atlas_width as f32;
    let ah: f32 = atlas_height as f32;

    for placed in &plan.placements {
        let [px, py] = placed.origin;
        let key = sorted_keys[placed.index];
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
