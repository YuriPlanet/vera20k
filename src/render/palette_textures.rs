//! Palette + per-house RGB ramp GPU resources for voxel sprite shading.
//!
//! Owns the per-theater palette (256 RGB entries) and the per-game house
//! ramps (16 RGB entries × N_houses). Consumed by the voxel sprite
//! fragment shader to translate atlas-tile palette indices into final RGB:
//!
//!   if (16 <= byte < 32) → rgb = house_ramp[house_idx][byte - 16]
//!   else                 → rgb = palette[byte]
//!
//! The atlas tile stores the post-VPL-shaded, pre-house-remap palette
//! index. Remap and palette lookup happen at fragment-shader time, so the
//! atlas does not need to be rebuilt on house list changes — only the
//! `house_ramp_tex` is re-uploaded.
//!
//! Mirrors `Palette::with_house_colors(ramp: &[Color; 16])` semantics
//! (RGB substitution at indices 16..32), just done on GPU.
//!
//! ## Dependency rules
//! - Part of render/ — depends on assets/pal_file (Palette) and
//!   rules/house_colors (HouseColorIndex). No sim deps.

use crate::assets::pal_file::Palette;
use crate::render::gpu::GpuContext;
use crate::rules::house_colors::{HouseColorIndex, HouseColorRamps, NO_REMAP};

/// Maximum number of houses supported in the per-house ramp texture
/// (per project_scale_target). Row 0 is reserved for the no-remap fallback,
/// so up to MAX_HOUSES - 1 distinct houses are addressable.
pub const MAX_HOUSES: u32 = 32;

/// Number of palette entries (RA2 standard: 256 colors).
pub const PALETTE_ENTRIES: u32 = 256;

/// House remap range size: palette indices [16, 32) get house-color RGB
/// substitution. Matches the per-scheme band length in `house_colors`.
pub const RAMP_SIZE: u32 = 16;

/// GPU resources for voxel sprite color resolution.
pub struct PaletteSet {
    /// 1×256 Rgba8UnormSrgb texture: palette[i] = RGB color for index i (alpha = 255).
    /// sRGB format so the GPU sampler decodes the raw `.pal` display bytes
    /// (which are sRGB-encoded) into linear RGB on read, matching the
    /// pre-Phase-1 atlas semantics and the sRGB surface output.
    pub palette_tex: wgpu::Texture,
    pub palette_view: wgpu::TextureView,
    /// 16 × MAX_HOUSES Rgba8UnormSrgb texture: house_ramp[house][i] = RGB substitute
    /// for palette byte (16 + i). Row 0 is the no-remap fallback — populated
    /// with the theater palette's [16, 32) RGB range, so units with
    /// `HouseColorIndex == NO_REMAP` (civilians, neutrals) render their
    /// remap-range bytes as the theater palette would (instead of black).
    pub house_ramp_tex: wgpu::Texture,
    pub house_ramp_view: wgpu::TextureView,
    /// Bind group containing both textures + a point sampler.
    pub bind_group: wgpu::BindGroup,
    pub bind_group_layout: wgpu::BindGroupLayout,
    /// Point sampler (no filtering — atlas is integer-indexed).
    pub sampler: wgpu::Sampler,
}

impl PaletteSet {
    /// Build a new PaletteSet from the current theater palette and the active house list.
    /// `houses[i]` becomes row `i + 1` of the house ramp texture.
    /// Row 0 is the no-remap fallback (theater palette's [16, 32) range).
    pub fn new(
        gpu: &GpuContext,
        palette: &Palette,
        ramps: &HouseColorRamps,
        houses: &[HouseColorIndex],
    ) -> Self {
        Self::new_on_device(&gpu.device, &gpu.queue, palette, ramps, houses)
    }

    pub(crate) fn new_on_device(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        palette: &Palette,
        ramps: &HouseColorRamps,
        houses: &[HouseColorIndex],
    ) -> Self {
        let palette_bytes: Vec<u8> = build_palette_bytes(palette);
        let palette_tex: wgpu::Texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("voxel_palette_tex"),
            size: wgpu::Extent3d {
                width: PALETTE_ENTRIES,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &palette_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &palette_bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(PALETTE_ENTRIES * 4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: PALETTE_ENTRIES,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let palette_view: wgpu::TextureView = palette_tex.create_view(&Default::default());

        let house_ramp_bytes: Vec<u8> = build_house_ramp_bytes(palette, ramps, houses);
        let house_ramp_tex: wgpu::Texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("voxel_house_ramp_tex"),
            size: wgpu::Extent3d {
                width: RAMP_SIZE,
                height: MAX_HOUSES,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &house_ramp_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &house_ramp_bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(RAMP_SIZE * 4),
                rows_per_image: Some(MAX_HOUSES),
            },
            wgpu::Extent3d {
                width: RAMP_SIZE,
                height: MAX_HOUSES,
                depth_or_array_layers: 1,
            },
        );
        let house_ramp_view: wgpu::TextureView = house_ramp_tex.create_view(&Default::default());

        let sampler: wgpu::Sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("voxel_palette_sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Binding 0: theater palette (Rgba8UnormSrgb).
        // Binding 1: per-house RGB ramp (Rgba8UnormSrgb).
        // Binding 2: point sampler.
        let bind_group_layout: wgpu::BindGroupLayout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("voxel_palette_bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                        count: None,
                    },
                ],
            });

        let bind_group: wgpu::BindGroup = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("voxel_palette_bg"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&palette_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&house_ramp_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        Self {
            palette_tex,
            palette_view,
            house_ramp_tex,
            house_ramp_view,
            bind_group,
            bind_group_layout,
            sampler,
        }
    }
}

/// Convert a 256-entry RGB palette to row-major Rgba8UnormSrgb bytes (alpha = 255).
/// Bytes are written as-is — the sRGB texture format handles decode on sample.
fn build_palette_bytes(palette: &Palette) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(PALETTE_ENTRIES as usize * 4);
    for i in 0..PALETTE_ENTRIES as usize {
        let c = palette.colors[i];
        out.extend_from_slice(&[c.r, c.g, c.b, 255]);
    }
    out
}

/// Build the per-house ramp texture as MAX_HOUSES × 16 RGBA bytes, row-major.
/// Row 0 = theater palette's [16, 32) range (no-remap fallback for units
/// whose `HouseColorIndex == NO_REMAP`). Sampling this row reproduces the
/// raw palette colors for remap-range bytes.
///
/// Rows are **value-indexed**: house index `hc` writes row `hc.0 + 1`, matching
/// the shader's `house_color_to_remap_row(hc)`. This is required because
/// `HouseColorIndex` is now a (sparse) `[Colors]` entry index — position-indexed
/// rows would only line up while indices were dense `0..N`. `NO_REMAP` houses are
/// skipped (they sample row 0). Unwritten rows stay zero-filled (never sampled).
fn build_house_ramp_bytes(
    palette: &Palette,
    ramps: &HouseColorRamps,
    houses: &[HouseColorIndex],
) -> Vec<u8> {
    let row_bytes: usize = (RAMP_SIZE * 4) as usize;
    let mut out: Vec<u8> = vec![0u8; (MAX_HOUSES * RAMP_SIZE * 4) as usize];
    // Row 0: theater palette [16, 32) — no-remap fallback.
    for i in 0..RAMP_SIZE as usize {
        let c = palette.colors[16 + i];
        let off = i * 4;
        out[off] = c.r;
        out[off + 1] = c.g;
        out[off + 2] = c.b;
        out[off + 3] = 255;
    }
    // Per-house rows, indexed by value: row = hc.0 + 1.
    for &house in houses {
        if house == NO_REMAP {
            continue; // NO_REMAP samples row 0 (the theater fallback above).
        }
        let row: usize = house.0 as usize + 1;
        if row >= MAX_HOUSES as usize {
            continue; // out of texture range (safety; shouldn't happen for stock [Colors]).
        }
        let row_start: usize = row * row_bytes;
        let ramp = ramps.ramp(house);
        for (i, c) in ramp.iter().enumerate() {
            let off: usize = row_start + i * 4;
            out[off] = c.r;
            out[off + 1] = c.g;
            out[off + 2] = c.b;
            out[off + 3] = 255;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::pal_file::Color;

    fn dummy_palette() -> Palette {
        let mut colors: [Color; 256] = [Color {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        }; 256];
        for i in 16..32 {
            colors[i] = Color {
                r: i as u8,
                g: 100,
                b: 200,
                a: 255,
            };
        }
        Palette { colors }
    }

    /// 16 distinct schemes (one per entry index) so each ramp differs and a
    /// value-indexed row can be matched against `ramps.ramp(hc)`.
    fn test_ramps() -> HouseColorRamps {
        let schemes: Vec<crate::rules::color_scheme::ColorSchemeEntry> = (0..16u8)
            .map(|i| crate::rules::color_scheme::ColorSchemeEntry {
                name: format!("S{i}"),
                hsv: [i.wrapping_mul(15), 200, 240],
            })
            .collect();
        HouseColorRamps::from_schemes(&schemes)
    }

    fn assert_row_matches(bytes: &[u8], row: usize, ramp: &[Color; RAMP_SIZE as usize]) {
        let row_bytes: usize = (RAMP_SIZE * 4) as usize;
        let row_start = row * row_bytes;
        for (i, c) in ramp.iter().enumerate() {
            let off = row_start + i * 4;
            assert_eq!(bytes[off], c.r, "row {row} r at i={i}");
            assert_eq!(bytes[off + 1], c.g, "row {row} g at i={i}");
            assert_eq!(bytes[off + 2], c.b, "row {row} b at i={i}");
            assert_eq!(bytes[off + 3], 255, "row {row} a at i={i}");
        }
    }

    #[test]
    fn build_house_ramp_row0_mirrors_theater_palette_range() {
        let pal: Palette = dummy_palette();
        let bytes: Vec<u8> = build_house_ramp_bytes(&pal, &HouseColorRamps::default(), &[]);
        for i in 0..RAMP_SIZE as usize {
            let off: usize = i * 4;
            assert_eq!(bytes[off], (16 + i) as u8, "row 0 r at i={}", i);
            assert_eq!(bytes[off + 1], 100);
            assert_eq!(bytes[off + 2], 200);
            assert_eq!(bytes[off + 3], 255);
        }
    }

    #[test]
    fn build_house_ramp_unused_rows_zero() {
        let pal: Palette = dummy_palette();
        let bytes: Vec<u8> = build_house_ramp_bytes(&pal, &HouseColorRamps::default(), &[]);
        let row_bytes: usize = (RAMP_SIZE * 4) as usize;
        let row5_start: usize = 5 * row_bytes;
        for i in 0..row_bytes {
            assert_eq!(bytes[row5_start + i], 0, "row 5 byte {} not zero", i);
        }
    }

    #[test]
    fn build_house_ramp_rows_are_value_indexed() {
        // Sparse indices {1, 5, 10} must land on rows {2, 6, 11} (= hc.0 + 1),
        // matching the shader's house_color_to_remap_row.
        let pal: Palette = dummy_palette();
        let ramps = test_ramps();
        let houses = [HouseColorIndex(1), HouseColorIndex(5), HouseColorIndex(10)];
        let bytes: Vec<u8> = build_house_ramp_bytes(&pal, &ramps, &houses);
        for &hc in &houses {
            assert_row_matches(&bytes, hc.0 as usize + 1, ramps.ramp(hc));
        }
        // A non-listed row stays zero-filled (e.g. row 4 = index 3, not present).
        let row_bytes: usize = (RAMP_SIZE * 4) as usize;
        let row4_start = 4 * row_bytes;
        for i in 0..row_bytes {
            assert_eq!(bytes[row4_start + i], 0, "row 4 byte {} not zero", i);
        }
    }

    #[test]
    fn build_house_ramp_skips_no_remap() {
        // NO_REMAP houses must not write a per-house row (they sample row 0).
        let pal: Palette = dummy_palette();
        let ramps = test_ramps();
        let bytes: Vec<u8> = build_house_ramp_bytes(&pal, &ramps, &[NO_REMAP]);
        // Every row past row 0 stays zero.
        let row_bytes: usize = (RAMP_SIZE * 4) as usize;
        for byte in bytes.iter().skip(row_bytes) {
            assert_eq!(*byte, 0, "NO_REMAP wrote a per-house row");
        }
    }
}
