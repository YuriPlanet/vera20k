//! CPU-side primary-radar projection and final aperture copy frame.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::map::playfield::PlayfieldBounds;
use crate::map::terrain::{PlayfieldPresentationGeometry, TerrainGrid};

use super::current_cell::CurrentRadarCellAuthority;
use super::geometry::NativeRadarSurfaceGeometry;
use super::helpers::{
    COLOR_SHROUD, MINIMAP_HEIGHT, MINIMAP_WIDTH, OverlayPixel, RadarSurfacePixel, TerrainPixel,
    TerrainPixels, compute_aspect_fit, overlay_radar_color, radar_color_for_cell,
    radar_colors_for_cell, set_pixel, structural_bridge_radar_color,
    terrain_brightness_for_theater, world_to_minimap_pixel,
};
use super::minimap::{MinimapCellRadarSource, MinimapOverlayDatum};
use super::terrain::{NativeRadarCellColors, NativeRadarTerrainSurface};
use crate::render::native_surface_format::RGB565;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::render) struct PrimaryRadarCopyFrame {
    pub offset: [f32; 2],
    pub size: [f32; 2],
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
}

pub(in crate::render) fn generated_primary_copy_frame(
    surface: NativeRadarSurfaceGeometry,
    aperture_width: f32,
    aperture_height: f32,
) -> PrimaryRadarCopyFrame {
    let offset = surface.aperture_offset();
    let generated = surface.generated_size();
    let scale_x = aperture_width / MINIMAP_WIDTH as f32;
    let scale_y = aperture_height / MINIMAP_HEIGHT as f32;
    PrimaryRadarCopyFrame {
        offset: [offset.0 as f32 * scale_x, offset.1 as f32 * scale_y],
        size: [generated.0 as f32 * scale_x, generated.1 as f32 * scale_y],
        uv_origin: [
            offset.0 as f32 / MINIMAP_WIDTH as f32,
            offset.1 as f32 / MINIMAP_HEIGHT as f32,
        ],
        uv_size: [
            generated.0 as f32 / MINIMAP_WIDTH as f32,
            generated.1 as f32 / MINIMAP_HEIGHT as f32,
        ],
    }
}

pub(super) fn aperture_pixel(
    surface: Option<NativeRadarSurfaceGeometry>,
    pixel: (u32, u32),
) -> Option<(u32, u32)> {
    let pixel = if let Some(surface) = surface {
        let size = surface.generated_size();
        if pixel.0 >= size.0.max(0) as u32 || pixel.1 >= size.1.max(0) as u32 {
            return None;
        }
        surface.surface_to_aperture_pixel((pixel.0 as i32, pixel.1 as i32))
    } else {
        (pixel.0 as i32, pixel.1 as i32)
    };
    (pixel.0 >= 0 && pixel.1 >= 0).then_some((pixel.0 as u32, pixel.1 as u32))
}

pub(super) struct MinimapPlayfieldProjection {
    pub base_rgba: Vec<u8>,
    pub world_origin_x: f32,
    pub world_origin_y: f32,
    pub world_width: f32,
    pub world_height: f32,
    pub terrain_pixels: TerrainPixels,
    pub surface_pixels: Vec<RadarSurfacePixel>,
    pub overlay_pixels: Vec<OverlayPixel>,
    pub map_offset_x: f32,
    pub map_offset_y: f32,
    pub map_pixel_w: f32,
    pub map_pixel_h: f32,
    pub native_radar_surface: Option<NativeRadarSurfaceGeometry>,
    pub native_radar_terrain: Option<NativeRadarTerrainSurface>,
}

pub(super) fn rasterize_native_terrain(
    surface: &NativeRadarTerrainSurface,
) -> (Vec<u8>, Vec<RadarSurfacePixel>) {
    let mut rgba = vec![0u8; (MINIMAP_WIDTH * MINIMAP_HEIGHT * 4) as usize];
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.copy_from_slice(&COLOR_SHROUD);
    }
    let geometry = surface.geometry();
    let generated_size = geometry.generated_size();
    let mut pixels = Vec::with_capacity(surface.generated_rgb565().len());
    for (index, &packed) in surface.generated_rgb565().iter().enumerate() {
        let px = index as i32 % generated_size.0;
        let py = index as i32 / generated_size.0;
        let [red, green, blue] = RGB565.unpack_rgb8(packed);
        let color = [red, green, blue, 255];
        let (rx, ry) = geometry.surface_pixel_to_visibility_cell((px, py));
        if let Some((dest_x, dest_y)) = aperture_pixel(geometry.into(), (px as u32, py as u32)) {
            set_pixel(&mut rgba, MINIMAP_WIDTH, dest_x, dest_y, color);
        }
        pixels.push(RadarSurfacePixel {
            rx,
            ry,
            px: px as u32,
            py: py as u32,
            packed_rgb565: packed,
            color,
        });
    }
    (rgba, pixels)
}

impl MinimapPlayfieldProjection {
    /// Raw terrain enumerates every allocated Size-diamond cell; retained
    /// terrain/overlay metadata uses mode zero; generated radar bounds use
    /// native mode one. All retained pixels are generated-surface local, and
    /// the only later transform is RebuildRadarSurfaces' centered aperture
    /// copy.
    pub(super) fn derive(
        grid: &TerrainGrid,
        resolved_terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
        overlay_data: &[MinimapOverlayDatum],
        overlay_radar_colors: &HashMap<(u8, u8), [u8; 3]>,
        theater_name: &str,
        bounds: Option<PlayfieldBounds>,
        current_cell_authority: Option<CurrentRadarCellAuthority<'_>>,
    ) -> Self {
        let mut base_rgba = vec![0u8; (MINIMAP_WIDTH * MINIMAP_HEIGHT * 4) as usize];
        for pixel in base_rgba.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&COLOR_SHROUD);
        }

        let geometry =
            bounds.and_then(|bounds| PlayfieldPresentationGeometry::from_grid(grid, bounds));
        let native_radar_surface = bounds.and_then(|bounds| {
            resolved_terrain
                .and_then(|terrain| NativeRadarSurfaceGeometry::from_playfield(terrain, bounds))
        });
        let (world_origin_x, world_origin_y, world_width, world_height) = geometry
            .map(|geometry| {
                (
                    geometry.origin_x,
                    geometry.origin_y,
                    geometry.world_width.max(1.0),
                    geometry.world_height.max(1.0),
                )
            })
            .unwrap_or((0.0, 0.0, 1.0, 1.0));
        let (map_offset_x, map_offset_y, map_pixel_w, map_pixel_h) =
            compute_aspect_fit(world_width, world_height);
        let project_cell = |rx: u16, ry: u16| -> Option<(u32, u32)> {
            if let Some(surface) = native_radar_surface {
                let pixel = surface.cell_to_surface_pixel((rx, ry));
                let size = surface.generated_size();
                return (pixel.0 >= 0 && pixel.1 >= 0 && pixel.0 < size.0 && pixel.1 < size.1)
                    .then_some((pixel.0 as u32, pixel.1 as u32));
            }
            let (sx, sy) = crate::map::terrain::iso_to_screen(rx, ry, 0);
            Some(world_to_minimap_pixel(
                sx,
                sy,
                world_origin_x,
                world_origin_y,
                world_width,
                world_height,
                map_offset_x,
                map_offset_y,
                map_pixel_w,
                map_pixel_h,
            ))
        };
        let terrain_brightness = terrain_brightness_for_theater(theater_name);
        let mut terrain_pixels =
            Vec::with_capacity(geometry.map_or(0, |geometry| geometry.valid_cell_count));
        let mut overlays_by_cell = BTreeMap::new();
        let mut terrain_object_cells = BTreeSet::new();
        for &datum in overlay_data {
            match datum.source {
                MinimapCellRadarSource::Overlay { .. } => {
                    overlays_by_cell.insert((datum.rx, datum.ry), datum);
                }
                MinimapCellRadarSource::TerrainObject => {
                    terrain_object_cells.insert((datum.rx, datum.ry));
                }
            }
        }
        let base_cell_colors = |cell: &crate::map::terrain::TerrainCell| {
            if let Some(colors) = current_cell_authority.and_then(|authority| {
                authority.tile_radar_colors(cell.rx, cell.ry, terrain_brightness)
            }) {
                return colors;
            }
            let valid =
                resolved_terrain.is_some_and(|terrain| terrain.radar_color_valid(cell.rx, cell.ry));
            radar_colors_for_cell(cell, valid, terrain_brightness)
        };
        let structural_color = structural_bridge_radar_color(overlay_radar_colors);
        let fallback_cell_source = |cell: &crate::map::terrain::TerrainCell| {
            let resolved = resolved_terrain.and_then(|terrain| terrain.cell(cell.rx, cell.ry));
            let terrain_object = terrain_object_cells.contains(&(cell.rx, cell.ry))
                || resolved.is_some_and(|cell| cell.terrain_object_occupation.is_some());
            let structural_bridge =
                resolved.is_some_and(|cell| cell.bridge_facts.has_structural_bridge());
            super::helpers::current_cell_radar_source(
                terrain_object,
                structural_bridge,
                overlays_by_cell.get(&(cell.rx, cell.ry)).copied(),
                structural_color,
                overlay_radar_colors,
            )
        };
        let cell_source = |cell: &crate::map::terrain::TerrainCell| {
            current_cell_authority.map_or_else(
                || fallback_cell_source(cell),
                |authority| {
                    authority.source(cell.rx, cell.ry, structural_color, overlay_radar_colors)
                },
            )
        };

        // `FillTerrainColors @ 0x00654EA0` walks MapClass's allocation-backed
        // CellIterator and clips each raw 2x1 footprint against the source
        // rectangle. It does not call IsCellInPlayfield and does not require
        // the cell center to land inside the generated surface. Production
        // TerrainGrid is built from ResolvedTerrainGrid::iter(), the same
        // allocated Size-diamond authority (`0x00578350/0x00578290`).
        let native_cells = if native_radar_surface.is_some() {
            grid.cells
                .iter()
                .map(|cell| {
                    let (left, right) = base_cell_colors(cell);
                    NativeRadarCellColors {
                        cell: (cell.rx, cell.ry),
                        left,
                        right,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        if let Some(bounds) = bounds {
            for cell in &grid.cells {
                if !bounds.contains_geometry_packed(i32::from(cell.rx), i32::from(cell.ry)) {
                    continue;
                }
                let Some((px, py)) = project_cell(cell.rx, cell.ry) else {
                    continue;
                };
                let valid = resolved_terrain
                    .is_some_and(|terrain| terrain.radar_color_valid(cell.rx, cell.ry));
                let color = radar_color_for_cell(cell, valid, terrain_brightness);
                if native_radar_surface.is_none()
                    && let Some((dest_x, dest_y)) = aperture_pixel(None, (px, py))
                {
                    set_pixel(&mut base_rgba, MINIMAP_WIDTH, dest_x, dest_y, color);
                }
                terrain_pixels.push(TerrainPixel {
                    rx: cell.rx,
                    ry: cell.ry,
                    px,
                    py,
                    color,
                });
            }
        }

        let mut overlay_pixels = Vec::new();
        let native_overrides = if native_radar_surface.is_some() {
            grid.cells
                .iter()
                .filter_map(|cell| cell_source(cell).map(|(color, _)| ((cell.rx, cell.ry), color)))
                .collect()
        } else {
            BTreeMap::new()
        };
        if let Some(bounds) = bounds
            && current_cell_authority.is_some()
        {
            for cell in &grid.cells {
                if !bounds.contains_geometry_packed(i32::from(cell.rx), i32::from(cell.ry)) {
                    continue;
                }
                let Some((native_color, classification)) = cell_source(cell) else {
                    continue;
                };
                let Some((px, py)) = project_cell(cell.rx, cell.ry) else {
                    continue;
                };
                overlay_pixels.push(OverlayPixel {
                    rx: cell.rx,
                    ry: cell.ry,
                    px,
                    py,
                    color: [native_color[0], native_color[1], native_color[2], 255],
                    classification,
                });
            }
        } else if let Some(bounds) = bounds {
            for &datum in overlay_data {
                if !bounds.contains_geometry_packed(i32::from(datum.rx), i32::from(datum.ry)) {
                    continue;
                }
                let Some(native_color) = overlay_radar_color(datum, overlay_radar_colors) else {
                    continue;
                };
                let color = [native_color[0], native_color[1], native_color[2], 255];
                let Some((px, py)) = project_cell(datum.rx, datum.ry) else {
                    continue;
                };
                if native_radar_surface.is_none()
                    && let Some((dest_x, dest_y)) = aperture_pixel(None, (px, py))
                {
                    set_pixel(&mut base_rgba, MINIMAP_WIDTH, dest_x, dest_y, color);
                }
                overlay_pixels.push(OverlayPixel {
                    rx: datum.rx,
                    ry: datum.ry,
                    px,
                    py,
                    color,
                    classification: datum.classification,
                });
            }
        }

        let native_radar_terrain = native_radar_surface.map(|surface| {
            NativeRadarTerrainSurface::new(
                surface,
                native_cells,
                native_overrides,
                terrain_brightness,
            )
        });
        let surface_pixels = if let Some(surface) = &native_radar_terrain {
            let (native_rgba, pixels) = rasterize_native_terrain(surface);
            base_rgba = native_rgba;
            pixels
        } else {
            Vec::new()
        };

        Self {
            base_rgba,
            world_origin_x,
            world_origin_y,
            world_width,
            world_height,
            terrain_pixels: TerrainPixels::new(terrain_pixels),
            surface_pixels,
            overlay_pixels,
            map_offset_x,
            map_offset_y,
            map_pixel_w,
            map_pixel_h,
            native_radar_surface,
            native_radar_terrain,
        }
    }
}

pub(super) fn minimap_screen_point_to_camera_top_left(
    screen_x: f32,
    screen_y: f32,
    screen_w: f32,
    screen_h: f32,
    rect_x: f32,
    rect_y: f32,
    rect_w: f32,
    rect_h: f32,
    world_origin_x: f32,
    world_origin_y: f32,
    world_width: f32,
    world_height: f32,
    map_offset_x: f32,
    map_offset_y: f32,
    map_pixel_w: f32,
    map_pixel_h: f32,
) -> (f32, f32) {
    let tex_x = (screen_x - rect_x) / rect_w.max(1.0) * MINIMAP_WIDTH as f32;
    let tex_y = (screen_y - rect_y) / rect_h.max(1.0) * MINIMAP_HEIGHT as f32;
    let nx = ((tex_x - map_offset_x) / map_pixel_w.max(1.0)).clamp(0.0, 1.0);
    let ny = ((tex_y - map_offset_y) / map_pixel_h.max(1.0)).clamp(0.0, 1.0);
    let world_x = world_origin_x + nx * world_width;
    let world_y = world_origin_y + ny * world_height;
    (world_x - screen_w * 0.5, world_y - screen_h * 0.5)
}
