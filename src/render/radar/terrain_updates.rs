//! Current-cell updates for retained native radar terrain layers.
//!
//! This is the GPU-free body used by `MinimapRenderer` after the simulation's
//! terrain-dirty generation changes. Full reconstruction and these updates
//! both consume `CurrentRadarCellAuthority`.

use std::collections::HashMap;

use super::current_cell::CurrentRadarCellAuthority;
use super::geometry::NativeRadarSurfaceGeometry;
use super::helpers::{MINIMAP_WIDTH, OverlayPixel, RadarSurfacePixel, TerrainPixels, set_pixel};
use super::projection::{aperture_pixel, rasterize_native_terrain};
use super::terrain::{NativeRadarCellColors, NativeRadarCellUpdate, NativeRadarTerrainSurface};

pub(crate) struct RadarTerrainUpdateLayers<'a> {
    pub base_rgba: &'a mut Vec<u8>,
    pub terrain_pixels: &'a TerrainPixels,
    pub surface_pixels: &'a mut Vec<RadarSurfacePixel>,
    pub overlay_pixels: &'a mut Vec<OverlayPixel>,
    pub native_surface: Option<NativeRadarSurfaceGeometry>,
    pub native_terrain: &'a mut Option<NativeRadarTerrainSurface>,
}

/// A retained-radar terrain update staged before final frame composition.
///
/// CPU pixels may already have changed when an upload fails. The retained
/// generation is deliberately committed only by [`Self::finish_with_upload`],
/// so the next presentation visit re-runs the idempotent CellClass projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StagedRadarTerrainUpdate {
    observed_generation: u64,
    acknowledge_generation: Option<u64>,
}

impl StagedRadarTerrainUpdate {
    /// Run the presentation-owned upload and commit its retained generation
    /// only after that boundary reports success.
    pub(crate) fn finish_with_upload<E>(
        self,
        last_generation: &mut u64,
        upload: impl FnOnce() -> Result<(), E>,
    ) -> Result<Option<u64>, E> {
        upload()?;
        *last_generation = self.observed_generation;
        Ok(self.acknowledge_generation)
    }

    pub(crate) fn finish_infallible(
        self,
        last_generation: &mut u64,
        upload: impl FnOnce(),
    ) -> Option<u64> {
        match self.finish_with_upload(last_generation, || {
            upload();
            Ok::<(), std::convert::Infallible>(())
        }) {
            Ok(generation) => generation,
            Err(never) => match never {},
        }
    }
}

/// Stage one newly published simulation batch through the retained radar
/// terrain layers. Final upload and acknowledgement are separate ordered
/// commits; see [`StagedRadarTerrainUpdate::finish_with_upload`].
pub(crate) fn stage_radar_terrain_dirty_generation(
    layers: RadarTerrainUpdateLayers<'_>,
    authority: CurrentRadarCellAuthority<'_>,
    structural_bridge_color: [u8; 3],
    overlay_radar_colors: &HashMap<(u8, u8), [u8; 3]>,
    cells: &[(u16, u16)],
    generation: u64,
    last_generation: u64,
) -> StagedRadarTerrainUpdate {
    if generation == last_generation {
        return StagedRadarTerrainUpdate {
            observed_generation: generation,
            acknowledge_generation: None,
        };
    }
    apply_radar_terrain_dirty_cells(
        layers,
        authority,
        structural_bridge_color,
        overlay_radar_colors,
        cells,
    );
    StagedRadarTerrainUpdate {
        observed_generation: generation,
        acknowledge_generation: (!cells.is_empty()).then_some(generation),
    }
}

/// Re-run the current CellClass radar source for one acknowledged dirty batch.
///
/// gamemd-derived: `CellClass::GetRadarColor @ 0x0047C060` is called by the
/// terrain-dirty consumer `RadarClass::ClearBackground @ 0x00655250`; its source
/// priority is TerrainClass, current structural-high flag, current overlay,
/// then selected TMP.
pub(super) fn apply_radar_terrain_dirty_cells(
    layers: RadarTerrainUpdateLayers<'_>,
    authority: CurrentRadarCellAuthority<'_>,
    structural_bridge_color: [u8; 3],
    overlay_radar_colors: &HashMap<(u8, u8), [u8; 3]>,
    cells: &[(u16, u16)],
) {
    let mut native_changes = Vec::new();
    let terrain_brightness = layers
        .native_terrain
        .as_ref()
        .map_or(1.0, NativeRadarTerrainSurface::terrain_brightness);
    for &(rx, ry) in cells {
        let source = authority.source(rx, ry, structural_bridge_color, overlay_radar_colors);
        if let Some((left, right)) = authority.tile_radar_colors(rx, ry, terrain_brightness) {
            native_changes.push(NativeRadarCellUpdate {
                base: NativeRadarCellColors {
                    cell: (rx, ry),
                    left,
                    right,
                },
                override_color: source.map(|(color, _)| color),
            });
        }
        let Some(terrain_pixel) = layers.terrain_pixels.cell(rx, ry).copied() else {
            continue;
        };
        if let Some((x, y)) =
            aperture_pixel(layers.native_surface, (terrain_pixel.px, terrain_pixel.py))
        {
            set_pixel(layers.base_rgba, MINIMAP_WIDTH, x, y, terrain_pixel.color);
        }

        if let Some((native_color, classification)) = source {
            let color = [native_color[0], native_color[1], native_color[2], 255];
            if let Some(existing) = layers
                .overlay_pixels
                .iter_mut()
                .find(|pixel| pixel.rx == rx && pixel.ry == ry)
            {
                existing.px = terrain_pixel.px;
                existing.py = terrain_pixel.py;
                existing.color = color;
                existing.classification = classification;
            } else {
                layers.overlay_pixels.push(OverlayPixel {
                    rx,
                    ry,
                    px: terrain_pixel.px,
                    py: terrain_pixel.py,
                    color,
                    classification,
                });
            }
        } else {
            layers
                .overlay_pixels
                .retain(|pixel| !(pixel.rx == rx && pixel.ry == ry));
        }
    }
    if let Some(surface) = layers.native_terrain
        && surface.set_cell_states(native_changes)
    {
        let (rgba, pixels) = rasterize_native_terrain(surface);
        *layers.base_rgba = rgba;
        *layers.surface_pixels = pixels;
    }
}
