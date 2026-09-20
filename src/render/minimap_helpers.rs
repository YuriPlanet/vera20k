//! Minimap helper types, color constants, and pixel-level utility functions.
//!
//! Extracted from minimap.rs for file-size limits. Contains color definitions,
//! overlay classification, coordinate mapping, and pixel buffer operations.
//!
//! ## Dependency rules
//! - Part of render/ — depends on map/terrain, map/houses, rules/house_colors, sim/vision.

use crate::map::houses::HouseColorMap;
use crate::rules::house_colors::{HouseColorIndex, HouseColorRamps, NO_REMAP};
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::vision::FogState;
use std::collections::HashMap;

use super::minimap::{MinimapCellRadarSource, MinimapOverlayDatum};
use super::native_radar_surface::{RADAR_APERTURE_HEIGHT, RADAR_APERTURE_WIDTH};

pub(super) const MINIMAP_WIDTH: u32 = RADAR_APERTURE_WIDTH;
pub(super) const MINIMAP_HEIGHT: u32 = RADAR_APERTURE_HEIGHT;

/// Margin from the screen edges in pixels.
pub(super) const MINIMAP_MARGIN: f32 = 10.0;

/// Depth value for minimap elements — always drawn in front of everything.
pub(super) const MINIMAP_DEPTH: f32 = 0.0;

/// Shrouded / unexplored area color (pure black, matches original PutPixel(0)).
pub(super) const COLOR_SHROUD: [u8; 4] = [0, 0, 0, 255];

/// Fog-of-war dimming factor for revealed (previously seen) cells.
/// Original engine uses SHR 1 = exact halving.

const THEATER_BRIGHTNESS_DEFAULT: f32 = 1.0;
const THEATER_BRIGHTNESS_SNOW: f32 = 0.8;

/// Classification of an overlay for minimap coloring.
///
/// Defined in render/ so that the minimap doesn't depend on map/overlay_types.
/// The caller (app layer) maps `OverlayTypeFlags` to this enum via a closure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayClassification {
    Ore,
    Gem,
    Wall,
    Bridge,
    /// Trees, rocks, and other terrain objects.
    TerrainObject,
    /// Non-rendered overlay (unknown or not worth showing).
    Other,
}


/// An overlay pixel to stamp on the minimap (pre-computed at init time).
#[derive(Clone, Copy)]
pub(super) struct OverlayPixel {
    pub rx: u16,
    pub ry: u16,
    pub px: u32,
    pub py: u32,
    pub color: [u8; 4],
    pub classification: OverlayClassification,
}

/// A terrain pixel with pre-computed minimap position and color.
#[derive(Clone, Copy)]
pub(super) struct TerrainPixel {
    pub rx: u16,
    pub ry: u16,
    pub px: u32,
    pub py: u32,
    pub color: [u8; 4],
}

pub(crate) fn minimap_overlay_datum(
    rx: u16,
    ry: u16,
    overlay_id: u8,
    frame: u8,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    rules: Option<&RuleSet>,
) -> MinimapOverlayDatum {
    let flags = overlay_registry.and_then(|registry| registry.flags(overlay_id));
    let name = overlay_registry
        .and_then(|registry| registry.name(overlay_id))
        .unwrap_or("");
    let is_tiberium = flags.is_some_and(|flags| flags.tiberium);
    let has_cell_anim = flags.is_some_and(|flags| flags.cell_anim.is_some());
    let classification = if is_tiberium {
        if name
            .get(..3)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("GEM"))
        {
            OverlayClassification::Gem
        } else {
            OverlayClassification::Ore
        }
    } else if flags.is_some_and(|flags| flags.wall) {
        OverlayClassification::Wall
    } else if crate::map::overlay_types::is_bridge_overlay_index(overlay_id) {
        OverlayClassification::Bridge
    } else {
        OverlayClassification::Other
    };
    let has_tiberium_type = is_tiberium
        && overlay_registry
            .zip(rules)
            .and_then(|(registry, rules)| {
                registry.tiberium_type_for_overlay(&rules.tiberium_types, overlay_id)
            })
            .is_some();
    MinimapOverlayDatum {
        rx,
        ry,
        classification,
        source: MinimapCellRadarSource::Overlay {
            overlay_id,
            frame,
            is_tiberium,
            has_cell_anim,
            has_tiberium_type,
        },
    }
}

/// One generated-primary terrain pixel plus the exact cell gamemd queries for
/// shroud/fog when `RenderCellPixel @ 0x00655C50` refreshes that pixel.
#[derive(Clone, Copy)]
pub(super) struct RadarSurfacePixel {
    pub rx: u16,
    pub ry: u16,
    pub px: u32,
    pub py: u32,
    pub packed_rgb565: u16,
    pub color: [u8; 4],
}

/// Compute the minimap color for a terrain cell.
///
/// Uses per-tile radar colors from TMP files when available (RadarLeft/RadarRight
/// RGB triplets baked into each tile cell header). Falls back to hardcoded colors
/// when TMP data is absent (both triplets are [0,0,0]).
pub(super) fn radar_color_for_cell(
    cell: &crate::map::terrain::TerrainCell,
    radar_color_valid: bool,
    terrain_brightness: f32,
) -> [u8; 4] {
    let (left, _) = radar_colors_for_cell(cell, radar_color_valid, terrain_brightness);
    [left[0], left[1], left[2], 255]
}

/// Preserve the two raw half-pixel colors consumed by
/// `FillTerrainColors @ 0x00654EA0`; averaging is only for legacy metadata.
pub(super) fn radar_colors_for_cell(
    cell: &crate::map::terrain::TerrainCell,
    radar_color_valid: bool,
    terrain_brightness: f32,
) -> ([u8; 3], [u8; 3]) {
    radar_colors_for_tmp_metadata(
        cell.radar_left,
        cell.radar_right,
        radar_color_valid,
        terrain_brightness,
    )
}

pub(super) fn radar_colors_for_tmp_metadata(
    radar_left: [u8; 3],
    _radar_right: [u8; 3],
    radar_color_valid: bool,
    terrain_brightness: f32,
) -> ([u8; 3], [u8; 3]) {
    if !radar_color_valid {
        return ([60, 60, 60], [60, 60, 60]);
    }
    // `ApplyTheaterBrightness @ 0x00661190` multiplies through x87, clamps,
    // and `Math__ftol` truncates. `CellClass::GetRadarColor @ 0x0047C060`
    // then reads only TMP +0x2B..0x2D (RadarLeft), unsigned-shifts each byte,
    // and copies the same triple to both raw footprint pixels.
    let color = radar_left.map(|channel| {
        ((f32::from(channel) * terrain_brightness)
            .clamp(0.0, 255.0)
            .trunc() as u8)
            >> 1
    });
    (color, color)
}

pub(super) fn structural_bridge_radar_color(
    colors: &HashMap<(u8, u8), [u8; 3]>,
) -> [u8; 3] {
    colors.get(&(24, 0)).copied().unwrap_or([0, 0, 0])
}

/// Resolve the overlay branches below structural bridge priority in
/// `CellClass::GetRadarColor @ 0x0047C060`.
pub(super) fn overlay_radar_color(
    datum: MinimapOverlayDatum,
    colors: &HashMap<(u8, u8), [u8; 3]>,
) -> Option<[u8; 3]> {
    let MinimapCellRadarSource::Overlay {
        overlay_id,
        frame,
        is_tiberium,
        has_cell_anim,
        has_tiberium_type,
    } = datum.source
    else {
        return Some([200, 200, 160]);
    };
    if matches!(overlay_id, 100 | 101 | 231 | 232 | 239) {
        return None;
    }
    let frame = if matches!(overlay_id, 0x4A..=0x63 | 0xCD..=0xE6) {
        1
    } else {
        frame
    };
    if is_tiberium {
        // `CellClass::GetRadarColor @ 0x0047C060` reads the resolved
        // OverlayType+0x29C CellAnim pointer. A non-null pointer delegates to
        // that AnimType's SHP and missing/out-of-range frames are black. Only
        // the null-pointer mapped-tiberium branch uses fixed khaki. Retail
        // RULESMD has no active `CellAnim=` entry (including TIB*/GEM*), so
        // stock ore/gems take this khaki branch despite their own SHP files.
        return if has_cell_anim {
            Some(
                colors
                    .get(&(overlay_id, frame))
                    .copied()
                    .unwrap_or([0, 0, 0]),
            )
        } else {
            has_tiberium_type.then_some([170, 170, 130])
        };
    }
    let [r, g, b] = colors
        .get(&(overlay_id, frame))
        .copied()
        .unwrap_or([0, 0, 0]);
    if matches!(overlay_id, 0x7F..=0x8A | 0x93..=0x9E) {
        Some([r, b, g])
    } else {
        Some([r, g, b])
    }
}

pub(super) fn current_cell_radar_source(
    terrain_object_present: bool,
    structural_bridge_present: bool,
    overlay: Option<MinimapOverlayDatum>,
    structural_bridge_color: [u8; 3],
    colors: &HashMap<(u8, u8), [u8; 3]>,
) -> Option<([u8; 3], OverlayClassification)> {
    if terrain_object_present {
        Some(([200, 200, 160], OverlayClassification::TerrainObject))
    } else if structural_bridge_present {
        Some((structural_bridge_color, OverlayClassification::Bridge))
    } else {
        overlay.and_then(|datum| {
            overlay_radar_color(datum, colors).map(|color| (color, datum.classification))
        })
    }
}

/// Headless fallback mapping when no authoritative generated surface exists.
///
/// Returns `(offset_x, offset_y, mapped_w, mapped_h)` — the sub-region of the
/// 140×108 aperture that the world maps to, centered with black margins on the
/// shorter axis.
pub(super) fn compute_aspect_fit(world_w: f32, world_h: f32) -> (f32, f32, f32, f32) {
    let width = MINIMAP_WIDTH as f32;
    let height = MINIMAP_HEIGHT as f32;
    let scale = (width / world_w).min(height / world_h);
    let mapped_w = (world_w * scale).min(width);
    let mapped_h = (world_h * scale).min(height);
    let offset_x = (width - mapped_w) * 0.5;
    let offset_y = (height - mapped_h) * 0.5;
    (offset_x, offset_y, mapped_w, mapped_h)
}

/// Map a world-space position to a minimap pixel coordinate.
///
/// Uses the aspect-fit sub-region `(map_off_x, map_off_y, map_w, map_h)` so
/// the fallback map is centered within the native aperture.
pub(super) fn world_to_minimap_pixel(
    world_x: f32,
    world_y: f32,
    origin_x: f32,
    origin_y: f32,
    world_w: f32,
    world_h: f32,
    map_off_x: f32,
    map_off_y: f32,
    map_w: f32,
    map_h: f32,
) -> (u32, u32) {
    let nx: f32 = (world_x - origin_x) / world_w;
    let ny: f32 = (world_y - origin_y) / world_h;
    let max_x: u32 = MINIMAP_WIDTH.saturating_sub(1);
    let max_y: u32 = MINIMAP_HEIGHT.saturating_sub(1);
    let px: u32 = (nx * map_w + map_off_x).clamp(0.0, max_x as f32) as u32;
    let py: u32 = (ny * map_h + map_off_y).clamp(0.0, max_y as f32) as u32;
    (px, py)
}

/// Set a pixel in an RGBA buffer. Bounds-checked; out-of-range writes are ignored.
pub(super) fn set_pixel(rgba: &mut [u8], width: u32, x: u32, y: u32, color: [u8; 4]) {
    let height = (rgba.len() / 4) as u32 / width.max(1);
    if x >= width || y >= height {
        return;
    }
    let offset: usize = ((y * width + x) * 4) as usize;
    if offset + 3 < rgba.len() {
        rgba[offset] = color[0];
        rgba[offset + 1] = color[1];
        rgba[offset + 2] = color[2];
        rgba[offset + 3] = color[3];
    }
}

/// Map an owner name to a minimap dot color using house color data.
///
/// Looks up the owner's `[Colors]` entry index from the HouseColorMap, then uses
/// shade 0 of that scheme's ramp — the brightest band (palette index 16), which
/// is gamemd's radar color. Falls back to the default scheme for unknown owners.
pub(super) fn owner_dot_color(
    owner: &str,
    house_colors: &HouseColorMap,
    ramps: &HouseColorRamps,
) -> [u8; 4] {
    // Unknown owner → NO_REMAP, which ramp() resolves to the default scheme
    // (matching the producers' DEFAULT_SCHEME_ENTRY fallback), not entry 0.
    let index: HouseColorIndex = house_colors.get(owner).copied().unwrap_or(NO_REMAP);
    let c = ramps.ramp(index)[0];
    [c.r, c.g, c.b, 255]
}

/// Get the terrain color for a pixel based on fog-of-war visibility.
///
/// Visible cells show at full brightness, revealed (previously seen) cells
/// Standard YR (FogOfWar=false): explored cells at full brightness, shrouded = None.
pub(super) fn cell_visibility_color(
    local_owner: InternedId,
    fog: &FogState,
    pixel: &TerrainPixel,
) -> Option<[u8; 4]> {
    // Hostile gap generator: terrain renders black like unexplored shroud.
    // Checked before fog so black wins over half-bright on overlap (native order).
    if fog.is_cell_gap_covered(local_owner, pixel.rx, pixel.ry) {
        return Some(COLOR_SHROUD);
    }
    // Friendly (own/allied) gap generator: half-bright fog over the terrain.
    if fog.is_cell_gap_fog(local_owner, pixel.rx, pixel.ry) {
        return Some(dim_color(pixel.color, 0.5));
    }
    if fog.is_cell_revealed(local_owner, pixel.rx, pixel.ry) {
        Some(pixel.color)
    } else {
        None
    }
}

/// Dim an RGBA color by a brightness factor (0.0 = black, 1.0 = original).
pub(super) fn dim_color(color: [u8; 4], factor: f32) -> [u8; 4] {
    [
        (color[0] as f32 * factor).round().clamp(0.0, 255.0) as u8,
        (color[1] as f32 * factor).round().clamp(0.0, 255.0) as u8,
        (color[2] as f32 * factor).round().clamp(0.0, 255.0) as u8,
        color[3],
    ]
}

/// Get the terrain brightness multiplier for a theater.
///
/// Live TheaterType table field read by `CellClass::GetRadarColor`.
///
/// The `TEMPERATE,SNOW,URBAN,DESERT,NEWURBAN,LUNAR` records at
/// `0x007E1B78 + index*0x70` carry brightness at `+0x58`: Snow is 0.8 and
/// every other retail theater is 1.0. The later unsigned `>>1` is separate.
pub(super) fn terrain_brightness_for_theater(theater_name: &str) -> f32 {
    if theater_name.eq_ignore_ascii_case("SNOW") {
        THEATER_BRIGHTNESS_SNOW
    } else {
        THEATER_BRIGHTNESS_DEFAULT
    }
}

/// Resolve a gamemd foundation name into minimap footprint dimensions.
pub(super) fn parse_foundation_size(foundation: &str) -> (u32, u32) {
    let (w, h) = crate::rules::foundation::foundation_dimensions(foundation);
    (u32::from(w), u32::from(h))
}

/// Check if an entity should be visible on the minimap (test helper).
#[cfg(test)]
pub(super) fn minimap_entity_visible(
    local_owner: InternedId,
    fog: &FogState,
    pos: &crate::sim::components::Position,
    owner: InternedId,
) -> bool {
    let interner = crate::sim::intern::test_interner();
    fog.is_friendly_id(local_owner, owner, &interner)
        || fog.is_cell_revealed(local_owner, pos.rx, pos.ry)
}

/// Default minimap screen rectangle (bottom-left corner with margin).
pub fn default_minimap_rect(screen_h: f32) -> (f32, f32, f32, f32) {
    let mm_w = MINIMAP_WIDTH as f32;
    let mm_h = MINIMAP_HEIGHT as f32;
    let mm_x = MINIMAP_MARGIN;
    let mm_y = screen_h - mm_h - MINIMAP_MARGIN;
    (mm_x, mm_y, mm_w, mm_h)
}

pub(super) fn surface_visibility_color(
    local_owner: InternedId,
    fog: &FogState,
    pixel: &RadarSurfacePixel,
) -> Option<[u8; 4]> {
    if fog.is_cell_gap_covered(local_owner, pixel.rx, pixel.ry) {
        return Some(COLOR_SHROUD);
    }
    if fog.is_cell_gap_fog(local_owner, pixel.rx, pixel.ry) {
        return Some(super::native_radar_terrain::half_bright_rgb565(
            pixel.packed_rgb565,
        ));
    }
    fog.is_cell_revealed(local_owner, pixel.rx, pixel.ry)
        .then_some(pixel.color)
}

#[cfg(test)]
mod radar_color_tests {
    use super::*;
    use crate::map::terrain::TerrainCell;

    fn cell(left: [u8; 3], right: [u8; 3]) -> TerrainCell {
        TerrainCell {
            screen_x: 0.0,
            screen_y: 0.0,
            tile_id: 0,
            sub_tile: 0,
            z: 0,
            rx: 1,
            ry: 1,
            is_water: false,
            variant: 0,
            tint: [1.0; 3],
            radar_left: left,
            radar_right: right,
            has_damaged_data: false,
        }
    }

    fn overlay(
        overlay_id: u8,
        frame: u8,
        is_tiberium: bool,
        has_cell_anim: bool,
        has_tiberium_type: bool,
    ) -> MinimapOverlayDatum {
        MinimapOverlayDatum {
            rx: 1,
            ry: 1,
            classification: OverlayClassification::Other,
            source: MinimapCellRadarSource::Overlay {
                overlay_id,
                frame,
                is_tiberium,
                has_cell_anim,
                has_tiberium_type,
            },
        }
    }

    #[test]
    fn native_tmp_color_is_identical_truncated_shifted_and_has_explicit_absence() {
        let sample = cell([144, 151, 64], [3, 5, 7]);
        assert_eq!(
            radar_colors_for_cell(&sample, true, terrain_brightness_for_theater("TEMPERATE")),
            ([72, 75, 32], [72, 75, 32]),
        );
        assert_eq!(
            radar_colors_for_cell(&sample, true, terrain_brightness_for_theater("SNOW")),
            ([57, 60, 25], [57, 60, 25]),
            "x87 144*.8 truncates to 115 before >>1",
        );
        assert_eq!(terrain_brightness_for_theater("URBAN"), 1.0);
        assert_eq!(
            radar_colors_for_cell(&cell([0; 3], [9; 3]), true, 1.0),
            ([0; 3], [0; 3]),
            "valid black is not an absence sentinel",
        );
        assert_eq!(
            radar_colors_for_cell(&sample, false, 0.8),
            ([60; 3], [60; 3]),
            "missing TMP subimage bypasses theater math",
        );
    }

    #[test]
    fn native_overlay_color_tree_pins_skip_forced_swap_and_fallbacks() {
        let colors = HashMap::from([
            ((74, 1), [1, 2, 3]),
            ((127, 4), [10, 20, 30]),
            ((147, 4), [40, 50, 60]),
        ]);
        assert_eq!(
            overlay_radar_color(overlay(100, 9, false, false, false), &colors),
            None
        );
        assert_eq!(
            overlay_radar_color(overlay(74, 7, false, false, false), &colors),
            Some([1, 2, 3]),
            "low bridge range forces frame 1",
        );
        assert_eq!(
            overlay_radar_color(overlay(127, 4, false, false, false), &colors),
            Some([10, 30, 20]),
            "OverlayClass swaps G/B only in the special ID ranges",
        );
        assert_eq!(
            overlay_radar_color(overlay(147, 4, true, true, true), &colors),
            Some([40, 50, 60]),
            "direct tiberium header path bypasses OverlayClass byte swap",
        );
        assert_eq!(
            overlay_radar_color(overlay(10, 2, false, false, false), &colors),
            Some([0, 0, 0]),
            "missing ordinary overlay image is black",
        );
        assert_eq!(
            overlay_radar_color(overlay(102, 11, true, false, true), &colors),
            Some([170, 170, 130]),
            "stock mapped tiberium with no CellAnim is khaki",
        );
        assert_eq!(
            overlay_radar_color(overlay(102, 11, true, false, false), &colors),
            None,
            "unmapped tiberium falls through to TMP terrain",
        );
        let terrain = MinimapOverlayDatum {
            rx: 1,
            ry: 1,
            classification: OverlayClassification::TerrainObject,
            source: MinimapCellRadarSource::TerrainObject,
        };
        assert_eq!(overlay_radar_color(terrain, &colors), Some([200, 200, 160]));

        assert_eq!(
            overlay_radar_color(overlay(147, 3, true, true, true), &colors),
            Some([0, 0, 0]),
            "missing CellAnim frame is black, never khaki",
        );
        let own_shp = HashMap::from([((102, 11), [9, 8, 7])]);
        assert_eq!(
            overlay_radar_color(overlay(102, 11, true, false, true), &own_shp),
            Some([170, 170, 130]),
            "stock no-CellAnim tiberium ignores its own overlay SHP",
        );

        let live_overlay = overlay(127, 4, false, false, false);
        assert_eq!(
            current_cell_radar_source(true, true, Some(live_overlay), [4, 5, 6], &colors),
            Some(([200, 200, 160], OverlayClassification::TerrainObject)),
        );
        assert_eq!(
            current_cell_radar_source(false, true, Some(live_overlay), [4, 5, 6], &colors),
            Some(([4, 5, 6], OverlayClassification::Bridge)),
        );
        assert_eq!(
            current_cell_radar_source(false, false, Some(live_overlay), [4, 5, 6], &colors),
            Some(([10, 30, 20], OverlayClassification::Other)),
            "a dirty visit must use the current OverlayData/header source",
        );
        assert_eq!(
            current_cell_radar_source(false, false, None, [4, 5, 6], &colors),
            None,
            "clearing an overlay restores the selected TMP source",
        );
    }
}
