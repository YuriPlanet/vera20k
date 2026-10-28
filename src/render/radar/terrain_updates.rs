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
    let Some(surface) = layers.native_terrain.as_mut() else {
        apply_mapless_dirty_cells(
            layers,
            authority,
            structural_bridge_color,
            overlay_radar_colors,
            cells,
        );
        return;
    };
    // The generated surface bakes overlay precedence into its raw RGB and a
    // changed batch re-rasterizes the whole base. The mapless per-cell terrain
    // and overlay layers are not drawn beside it, so they are not maintained;
    // stamping them would leave a non-native pixel when nothing changed.
    let terrain_brightness = surface.terrain_brightness();
    let changes = cells.iter().filter_map(|&(rx, ry)| {
        let (left, right) = authority.tile_radar_colors(rx, ry, terrain_brightness)?;
        Some(NativeRadarCellUpdate {
            base: NativeRadarCellColors {
                cell: (rx, ry),
                left,
                right,
            },
            override_color: authority
                .source(rx, ry, structural_bridge_color, overlay_radar_colors)
                .map(|(color, _)| color),
        })
    });
    if surface.set_cell_states(changes) {
        let (rgba, pixels) = rasterize_native_terrain(surface);
        *layers.base_rgba = rgba;
        *layers.surface_pixels = pixels;
    }
}

/// Mapless fallback (no generated surface): restamp each dirty cell's terrain
/// pixel and update the per-cell overlay list the minimap stamps in list
/// order. A changed cell keeps its first slot, a cleared cell drops every
/// entry, and a new cell appends in batch order. One pass over the list per
/// batch replaces a list scan per dirty cell.
fn apply_mapless_dirty_cells(
    layers: RadarTerrainUpdateLayers<'_>,
    authority: CurrentRadarCellAuthority<'_>,
    structural_bridge_color: [u8; 3],
    overlay_radar_colors: &HashMap<(u8, u8), [u8; 3]>,
    cells: &[(u16, u16)],
) {
    let mut updates: Vec<Option<OverlayPixel>> = Vec::new();
    let mut slots: HashMap<(u16, u16), usize> = HashMap::new();
    for &(rx, ry) in cells {
        let Some(terrain_pixel) = layers.terrain_pixels.cell(rx, ry).copied() else {
            continue;
        };
        if let Some((x, y)) =
            aperture_pixel(layers.native_surface, (terrain_pixel.px, terrain_pixel.py))
        {
            set_pixel(layers.base_rgba, MINIMAP_WIDTH, x, y, terrain_pixel.color);
        }
        slots.entry((rx, ry)).or_insert_with(|| {
            let source = authority.source(rx, ry, structural_bridge_color, overlay_radar_colors);
            updates.push(source.map(|(color, classification)| OverlayPixel {
                rx,
                ry,
                px: terrain_pixel.px,
                py: terrain_pixel.py,
                color: [color[0], color[1], color[2], 255],
                classification,
            }));
            updates.len() - 1
        });
    }
    let mut placed = vec![false; updates.len()];
    layers.overlay_pixels.retain_mut(|pixel| {
        let Some(&slot) = slots.get(&(pixel.rx, pixel.ry)) else {
            return true;
        };
        let Some(update) = updates[slot] else {
            return false;
        };
        if !placed[slot] {
            *pixel = update;
            placed[slot] = true;
        }
        true
    });
    layers.overlay_pixels.extend(
        updates
            .into_iter()
            .zip(placed)
            .filter_map(|(update, placed)| update.filter(|_| !placed)),
    );
}
