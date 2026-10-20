//! Isometric terrain grid: coordinate math, viewport culling, and instance generation.
//!
//! Converts map cells (isometric rx/ry coordinates) to screen-space pixel positions
//! for rendering. Provides viewport culling to only draw visible tiles.
//!
//! ## Coordinate system
//! RA2 uses isometric coordinates where each cell is a diamond shape (60x30 pixels).
//! Screen position: `sx = (rx - ry) * 30`, `sy = (rx + ry) * 15 - z * 15`.
//!
//! ## Dependency rules
//! - Part of map/ — reads the normalized MapClass playfield owner.

#[cfg(any(test, feature = "test-support"))]
use std::collections::BTreeMap;

use crate::map::resolved_terrain::ResolvedTerrainGrid;

#[cfg(test)]
#[path = "terrain_bridge_click_tests.rs"]
mod bridge_click_tests;

#[cfg(test)]
mod plane_inverse_tests {
    #[test]
    fn plane_xyz_matches_original_tactical_body() {
        let vectors = crate::test_fixture::json("tools/bridge_click_oracle/plane_vectors.json");
        let mut count = 0;
        for row in vectors["cases"].as_array().unwrap() {
            if row["native_exit"] != "plane" {
                continue; // Upper-window rejection is the caller's admission.
            }
            let world = &row["world_input"];
            let actual = super::screen_to_plane_leptons(
                world[0].as_i64().unwrap() as f32,
                world[1].as_i64().unwrap() as f32,
            );
            let expected = &row["expected_xyz"];
            assert_eq!(
                actual.to_array(),
                std::array::from_fn(|i| expected[i].as_i64().unwrap() as i32),
                "{}",
                row["name"],
            );
            count += 1;
        }
        assert_eq!(count, 22);
    }
}

/// Isometric tile diamond width in pixels (RA2 standard).
pub const TILE_WIDTH: f32 = 60.0;

/// Isometric tile diamond height in pixels (RA2 standard).
pub const TILE_HEIGHT: f32 = 30.0;

/// Pixels per elevation level. Each z-step raises the tile by this many pixels.
/// RA2: CellHeight = CellSizeY / 2 = 30 / 2 = 15. Confirmed by ra2_yr_map_terrain.md §1.6:
/// "Each height level shifts the cell up by 15 pixels on screen (RA2)."
/// (Note: Tiberian Sun uses 24/2 = 12 — do NOT use TS values here.)
pub const HEIGHT_STEP: f32 = 15.0;

/// Tactical screen-to-cell inverse scan cap from gamemd.exe `0x006D6590`.
pub const TACTICAL_INVERSE_MAX_SCAN_ATTEMPTS: usize = 180;

/// Tactical bridge open-edge threshold. The binary uses strict `> 15`.
pub const TACTICAL_BRIDGE_EDGE_THRESHOLD_PX: i32 = 15;

/// Extra high-bridge height adjustment: four height levels at 15 px each.
pub const TACTICAL_BRIDGE_EXTRA_HEIGHT_PX: f32 = 60.0;

const DIR_NORTH: u8 = 0;
const DIR_EAST: u8 = 2;
const DIR_SOUTH: u8 = 4;
const DIR_WEST: u8 = 6;

/// Per-cell high-bridge metadata needed by the tactical inverse.
///
/// This deliberately carries structural/orientation facts instead of only deck
/// height because gamemd's cursor inverse branches on `CellClass+0x140` flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TacticalBridgeCell {
    pub structural: bool,
    pub direction_zero: bool,
}

/// The tactical inverse reads each candidate's live Cell+11B level and
/// Cell+140 flags, then the cardinal neighbors (original6D674C..6D6888).
/// Keep both on the same borrowed authority across tile changes and restore.
pub trait TacticalCellLookup: std::fmt::Debug {
    fn cell_level(&self, rx: u16, ry: u16) -> Option<u8>;
    fn bridge_cell(&self, rx: u16, ry: u16) -> Option<TacticalBridgeCell>;
}

/// Literal inputs to the native comparisons; production borrows its live grid.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug)]
pub struct TacticalCellFixture<'a> {
    pub levels: &'a BTreeMap<(u16, u16), u8>,
    pub bridges: Option<&'a BTreeMap<(u16, u16), TacticalBridgeCell>>,
}

#[cfg(any(test, feature = "test-support"))]
impl TacticalCellLookup for TacticalCellFixture<'_> {
    fn cell_level(&self, rx: u16, ry: u16) -> Option<u8> {
        self.levels.get(&(rx, ry)).copied()
    }
    fn bridge_cell(&self, rx: u16, ry: u16) -> Option<TacticalBridgeCell> {
        self.bridges?.get(&(rx, ry)).copied()
    }
}

/// Inputs for the YR-shaped tactical screen-to-cell inverse.
#[derive(Debug, Clone, Copy)]
pub struct TacticalInverseContext<'a> {
    pub cells: Option<&'a dyn TacticalCellLookup>,
    pub viewport_offset_x: f32,
    pub viewport_offset_y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TacticalInverseResult {
    Cell { rx: f32, ry: f32 },
    Fallback { rx: f32, ry: f32 },
}

/// Flat (z=0) tile-frame rectangle of the cells `IsCellInPlayfield` mode zero
/// admits: the radar adapter for renderer fixtures without MapClass data.
/// Native `ComputeRadarMapBounds` (`0x0065455B..0x00654572`) enumerates mode
/// one, which `NativeRadarSurfaceGeometry::from_playfield` owns. This rectangle
/// decides radar pixels only when that surface is absent, which needs a
/// missing resolved terrain or playfield authority; a match binds both
/// (`MinimapRenderer::new`, `reconcile_playfield`), so it is kept for the
/// mapless fixtures alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayfieldPresentationGeometry {
    pub origin_x: f32,
    pub origin_y: f32,
    pub world_width: f32,
    pub world_height: f32,
    pub valid_cell_count: usize,
}

impl PlayfieldPresentationGeometry {
    /// Enumerate cells accepted by `MapClass::IsCellInPlayfield(0)` at
    /// `0x00578460` and derive their isometric radar rectangle.
    pub fn from_grid(
        grid: &TerrainGrid,
        bounds: crate::map::playfield::PlayfieldBounds,
    ) -> Option<Self> {
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        let mut valid_cell_count = 0usize;

        for cell in &grid.cells {
            if !bounds.contains_geometry_packed(i32::from(cell.rx), i32::from(cell.ry)) {
                continue;
            }
            let (sx, sy) = iso_to_screen(cell.rx, cell.ry, 0);
            min_x = min_x.min(sx);
            min_y = min_y.min(sy);
            max_x = max_x.max(sx + TILE_WIDTH);
            max_y = max_y.max(sy + TILE_HEIGHT);
            valid_cell_count += 1;
        }

        (valid_cell_count != 0).then_some(Self {
            origin_x: min_x,
            origin_y: min_y,
            world_width: max_x - min_x,
            world_height: max_y - min_y,
            valid_cell_count,
        })
    }
}

/// Per-tile rendering placement data returned by the UV lookup function.
/// Carries atlas UV coordinates, actual pixel size, and draw offset.
#[derive(Debug, Clone, Copy)]
pub struct TilePlacement {
    /// UV origin in the atlas texture (0.0..1.0).
    pub uv_origin: [f32; 2],
    /// UV extent in the atlas texture (0.0..1.0).
    pub uv_size: [f32; 2],
    /// Actual pixel dimensions of this tile (may differ from 60×30 for cliff/shore tiles).
    pub pixel_size: [f32; 2],
    /// Draw offset from the standard diamond origin (pixels).
    /// Negative values shift the sprite left/up to accommodate extra data regions.
    pub draw_offset: [f32; 2],
}

/// UV lookup function type: maps (tile_id, sub_tile) → optional placement data.
/// Returns None for tiles not in the atlas (empty template cells), which are skipped.
/// UV lookup: (tile_id, sub_tile, variant) → placement data.
pub type UvLookupFn<'a> = Option<&'a dyn Fn(u16, u8, u8) -> Option<TilePlacement>>;

/// A single terrain cell with pre-computed screen position.
#[derive(Debug, Clone)]
pub struct TerrainCell {
    /// Screen X position (top-left of diamond bounding box).
    pub screen_x: f32,
    /// Screen Y position (top-left of diamond bounding box).
    pub screen_y: f32,
    /// Tile index for atlas lookup (truncated from i32 to u16; -1 filtered out).
    pub tile_id: u16,
    /// Sub-tile index within the template.
    pub sub_tile: u8,
    /// Elevation level (0 = ground). Used for depth buffer computation.
    pub z: u8,
    /// Isometric cell X coordinate (preserved for lighting lookups).
    pub rx: u16,
    /// Isometric cell Y coordinate (preserved for lighting lookups).
    pub ry: u16,
    /// Tile visual variant index: 0 = pristine, positive = suffix sibling.
    pub variant: u8,
    /// RGB color tint from map lighting. [1,1,1] = full brightness (default).
    pub tint: [f32; 3],
    /// Per-tile radar minimap color (left half of isometric diamond), from TMP header.
    pub radar_left: [u8; 3],
    /// Per-tile radar minimap color (right half of isometric diamond), from TMP header.
    pub radar_right: [u8; 3],
}

/// Pre-computed terrain grid ready for rendering.
///
/// Cells are sorted by screen_y for correct back-to-front draw order.
/// World bounds are computed from all cell positions.
#[derive(Debug)]
pub struct TerrainGrid {
    /// All terrain cells, sorted by screen_y (draw order).
    pub cells: Vec<TerrainCell>,
    /// Total world width in pixels.
    pub world_width: f32,
    /// Total world height in pixels.
    pub world_height: f32,
    /// Minimum screen_x across all cells (world origin x).
    pub origin_x: f32,
    /// Minimum screen_y across all cells (world origin y).
    pub origin_y: f32,
    /// Bridge middle tiles a live publication can write, for the atlas
    /// pre-load. None when the theater lacks BridgeMiddle1/2 or both sets.
    pub bridge_middle_tiles: Option<crate::map::theater::BridgeMiddleTiles>,
}

/// Convert isometric cell coordinates to screen-space pixel position — **the
/// tile frame**.
///
/// Returns the top-left corner of the tile's diamond bounding box, which is the
/// point the original's terrain and overlay loops blit from:
///   X = 30*(rx-ry) - 30
///   Y = 15*(rx+ry) + 15 - z*15
///
/// This is *not* where an entity standing on the cell is drawn. An entity is
/// drawn on the cell's diamond centre, `iso_to_screen + (TILE_WIDTH/2,
/// TILE_HEIGHT/2)` — see `util::lepton::lepton_to_screen`. Callers that want to
/// place something on the middle of a cell rather than on its tile art must add
/// that half-tile themselves, as the sparkle and target-line paths do. The
/// native smudge caller adds only half-width; see `render::smudge`.
///
/// The `+ TILE_HEIGHT/2` in Y is a constant bias VERA carries on every world
/// layer relative to the original's absolute tactical pixel; it is invisible
/// because the camera absorbs it, but it must not be removed here alone — see
/// `util::lepton::WORLD_ROW_BIAS_PX`.
pub fn iso_to_screen(rx: u16, ry: u16, z: u8) -> (f32, f32) {
    let sx: f32 = (rx as f32 - ry as f32) * TILE_WIDTH / 2.0 - TILE_WIDTH / 2.0;
    let sy: f32 = (rx as f32 + ry as f32) * TILE_HEIGHT / 2.0 + TILE_HEIGHT / 2.0
        - f32::from(z as i8) * HEIGHT_STEP;
    (sx, sy)
}

/// Convert screen-space pixel position back to isometric cell coordinates.
///
/// Inverse of `iso_to_screen`. Assumes z=0 (ground level). Returns floating-point
/// coordinates; caller should round to get the nearest cell.
///
/// `iso_to_screen` maps (rx,ry) to `((rx-ry)*30 - 30, (rx+ry)*15 + 15)`.
/// The tile center is at NW + (30, 15). To map tile centers back to integer
/// cell coords, shift by half tile before dividing.
///
/// Derivation (clicking at tile center):
///   tile_center_X = (rx-ry)*30,  tile_center_Y = (rx+ry)*15 + 30
///   col = center_X / 30 = screen_x / 30  (after shifting click left by 30)
///   row = (center_Y - 30) / 15 = (screen_y - 30) / 15
pub fn screen_to_iso(screen_x: f32, screen_y: f32) -> (f32, f32) {
    let half_w: f32 = TILE_WIDTH / 2.0;
    let col: f32 = screen_x / half_w;
    let row: f32 = (screen_y - TILE_HEIGHT) / (TILE_HEIGHT / 2.0);
    let rx: f32 = (col + row) / 2.0;
    let ry: f32 = (row - col) / 2.0;
    (rx, ry)
}

/// The separate Z=0 plane XYZ returned by Tactical6D2280, not the
/// height/bridge Cell answer from6D6590. Input uses VERA world pixels.
///
/// Original ctor6D1DC5 stores matrix coefficients408888CE/410888CE:
/// A=8947918/2^21, B=2*A. For map-sized integer pixels (|x|,|y| <=65536)
/// both products/sums are exact in native53-bit precision. Its toward-zero
/// f32 store then integer conversion equals truncating this exact rational:
/// the result is below2^24, where every integer is representable. A normal
/// nearest-rounded f32 store is wrong; consumed +/-2006 controls catch it.
/// No x87 emulator or presentation write into simulation state is needed.
/// Tools/bridge_click_oracle/plane_vectors.json executes the original body.
///
/// As with the shared Cell inverse, fractional camera/zoom pixels select the
/// containing pixel; this presentation extension is not native parity proof.
/// Arbitrary out-of-map/overflow pixel inputs are outside this numeric bound.
pub fn screen_to_plane_leptons(world_x: f32, world_y: f32) -> glam::IVec3 {
    let x = i64::from(world_x.floor() as i32);
    let y = i64::from(world_y.floor() as i32) - i64::from(crate::util::lepton::WORLD_ROW_BIAS_PX);
    const NUMERATOR: i64 = 0x88_88ce;
    const DENOMINATOR: i64 = 1 << 21;
    glam::IVec3::new(
        ((x + 2 * y) * NUMERATOR / DENOMINATOR) as i32,
        ((-x + 2 * y) * NUMERATOR / DENOMINATOR) as i32,
        0,
    )
}

/// Convert tactical/client pixels to isometric cell coordinates using the
/// vertical scan shape verified from gamemd.exe `0x006D6590`.
pub fn screen_to_cell_tactical_inverse(
    screen_x: f32,
    screen_y: f32,
    context: TacticalInverseContext<'_>,
) -> TacticalInverseResult {
    // gamemd 0x006D6590 receives integer tactical pixels. VERA's camera/zoom
    // can produce fractional world pixels: choose the containing pixel once,
    // before every scan, direct-neighbor, and edge decision. This fractional
    // extension is VERA-internal (native equivalent UNCHECKED), while integer
    // inputs retain their exact native pixel. Floor also preserves translation
    // across negative world X, unlike truncating individual edge deltas.
    let input_x = (screen_x - context.viewport_offset_x).floor();
    let input_y = (screen_y - context.viewport_offset_y).floor();
    let (fallback_rx, fallback_ry) = screen_to_iso(input_x, input_y);
    let mut scan_y = input_y + TACTICAL_INVERSE_MAX_SCAN_ATTEMPTS as f32;

    for _ in 0..TACTICAL_INVERSE_MAX_SCAN_ATTEMPTS {
        let (candidate_rx, candidate_ry) = screen_to_iso(input_x, scan_y);
        let Some((cell_rx, cell_ry)) = rounded_lookup_cell(candidate_rx, candidate_ry) else {
            scan_y -= 1.0;
            continue;
        };
        let terrain_z = context
            .cells
            .and_then(|cells| cells.cell_level(cell_rx, cell_ry))
            .unwrap_or(0);
        let mut adjusted_scan_y = scan_y - f32::from(terrain_z as i8) * HEIGHT_STEP;

        if let Some(bridge_result) = apply_tactical_bridge_inverse(
            input_x,
            input_y,
            scan_y,
            cell_rx,
            cell_ry,
            terrain_z,
            context,
            &mut adjusted_scan_y,
        ) {
            return bridge_result;
        }

        if adjusted_scan_y <= input_y {
            return TacticalInverseResult::Cell {
                rx: candidate_rx,
                ry: candidate_ry,
            };
        }
        scan_y -= 1.0;
    }

    TacticalInverseResult::Fallback {
        rx: fallback_rx,
        ry: fallback_ry,
    }
}

fn rounded_lookup_cell(rx: f32, ry: f32) -> Option<(u16, u16)> {
    if !rx.is_finite() || !ry.is_finite() || rx < 0.0 || ry < 0.0 {
        return None;
    }
    Some((rx.round() as u16, ry.round() as u16))
}

fn apply_tactical_bridge_inverse(
    input_x: f32,
    input_y: f32,
    scan_y: f32,
    cell_rx: u16,
    cell_ry: u16,
    terrain_z: u8,
    context: TacticalInverseContext<'_>,
    adjusted_scan_y: &mut f32,
) -> Option<TacticalInverseResult> {
    let bridge_cells = context.cells?;
    let bridge = bridge_cells.bridge_cell(cell_rx, cell_ry)?;
    if !bridge.structural {
        return None;
    }

    let dir2 = tactical_bridge_neighbor(bridge_cells, cell_rx, cell_ry, DIR_EAST);
    let dir4 = tactical_bridge_neighbor(bridge_cells, cell_rx, cell_ry, DIR_SOUTH);
    let dir0 = bridge
        .direction_zero
        .then(|| tactical_bridge_neighbor(bridge_cells, cell_rx, cell_ry, DIR_NORTH))
        .flatten();
    let dir6 = (!bridge.direction_zero)
        .then(|| tactical_bridge_neighbor(bridge_cells, cell_rx, cell_ry, DIR_WEST))
        .flatten();

    let dir2_is_bridge = dir2.is_some_and(|b| b.structural);
    let dir4_is_bridge = dir4.is_some_and(|b| b.structural);
    let dir0_open_edge = bridge.direction_zero && !dir0.is_some_and(|b| b.structural);
    let dir6_open_edge = !bridge.direction_zero && !dir6.is_some_and(|b| b.structural);

    let dir2_height = tactical_neighbor_height(bridge_cells, cell_rx, cell_ry, DIR_EAST);
    let dir4_height = tactical_neighbor_height(bridge_cells, cell_rx, cell_ry, DIR_SOUTH);
    let terrain_z_i16 = i16::from(terrain_z as i8);
    let direct_y = if bridge.direction_zero {
        !dir4_is_bridge
    } else {
        !dir4_is_bridge && (terrain_z_i16 - i16::from(dir4_height as i8)).abs() <= 1
    };
    let direct_x = if bridge.direction_zero {
        !dir2_is_bridge && (terrain_z_i16 - i16::from(dir2_height as i8)).abs() <= 1
    } else {
        !dir2_is_bridge
    };

    // gamemd 0x006D6895..0x006D68FF projects the raw cell corner with Z=0,
    // then subtracts BASE level * 15. The artwork origin is half a tile left
    // of that corner; deck height belongs only to the later 60-pixel scan
    // adjustment. Using the deck's artwork origin displaced clicks by
    // (-30, -60) on a normal high bridge. Keep VERA's shared world-row bias.
    // Evidence: TACTICAL_BRIDGE_MOVE_CLICK_DIAGNOSIS_20260909.md.
    let (tile_left, bridge_ref_y) = iso_to_screen(cell_rx, cell_ry, terrain_z);
    let bridge_ref_x = tile_left + TILE_WIDTH / 2.0;
    if bridge_ref_y <= input_y {
        if direct_y {
            return Some(TacticalInverseResult::Cell {
                rx: cell_rx as f32,
                ry: cell_ry.saturating_add(1) as f32,
            });
        }
        if direct_x {
            return Some(TacticalInverseResult::Cell {
                rx: cell_rx.saturating_add(1) as f32,
                ry: cell_ry as f32,
            });
        }
    }

    // Input and reference are whole pixels. Native 0x006D697D..0x006D69C0
    // divides signed dx by two toward zero, then compares strictly against 15.
    // Float division changes the result for odd dx at the railing boundary.
    let input_x_delta = (input_x - bridge_ref_x) as i32;
    let input_y_delta = (input_y - bridge_ref_y) as i32;
    let apply_extra_bridge_lift = if dir0_open_edge {
        input_y_delta - input_x_delta / 2 > TACTICAL_BRIDGE_EDGE_THRESHOLD_PX
    } else if dir6_open_edge {
        input_y_delta + input_x_delta / 2 > TACTICAL_BRIDGE_EDGE_THRESHOLD_PX
    } else {
        true
    };
    if apply_extra_bridge_lift {
        *adjusted_scan_y =
            scan_y - f32::from(terrain_z as i8) * HEIGHT_STEP - TACTICAL_BRIDGE_EXTRA_HEIGHT_PX;
    }
    None
}

fn tactical_bridge_neighbor(
    bridge_cells: &dyn TacticalCellLookup,
    rx: u16,
    ry: u16,
    direction: u8,
) -> Option<TacticalBridgeCell> {
    let (nx, ny) = tactical_cardinal_neighbor(rx, ry, direction)?;
    bridge_cells.bridge_cell(nx, ny)
}

fn tactical_neighbor_height(cells: &dyn TacticalCellLookup, rx: u16, ry: u16, direction: u8) -> u8 {
    tactical_cardinal_neighbor(rx, ry, direction)
        .and_then(|(rx, ry)| cells.cell_level(rx, ry))
        .unwrap_or(0)
}

fn tactical_cardinal_neighbor(rx: u16, ry: u16, direction: u8) -> Option<(u16, u16)> {
    let (dx, dy) = match direction {
        DIR_NORTH => (0_i32, -1_i32),
        DIR_EAST => (1, 0),
        DIR_SOUTH => (0, 1),
        DIR_WEST => (-1, 0),
        _ => return None,
    };
    let nx = rx as i32 + dx;
    let ny = ry as i32 + dy;
    if nx < 0 || ny < 0 || nx > u16::MAX as i32 || ny > u16::MAX as i32 {
        return None;
    }
    Some((nx as u16, ny as u16))
}

/// Build a TerrainGrid from the resolved terrain stage's final LAT-adjusted
/// tile choice.
pub fn build_terrain_grid_from_resolved(
    resolved: &ResolvedTerrainGrid,
    bridge_middle_tiles: Option<crate::map::theater::BridgeMiddleTiles>,
) -> TerrainGrid {
    let mut cells: Vec<TerrainCell> = Vec::with_capacity(resolved.cells().len());
    let mut min_x: f32 = f32::MAX;
    let mut min_y: f32 = f32::MAX;
    let mut max_x: f32 = f32::MIN;
    let mut max_y: f32 = f32::MIN;

    for cell in resolved.iter() {
        let (tile_id, sub_tile) = resolved.presentation_tile(cell);
        let (sx, sy) = iso_to_screen(cell.rx, cell.ry, cell.level);
        // Border filler cells (outside LocalSize) are kept: gamemd draws every
        // allocated cell and hides the border purely by clamping the tactical
        // camera to LocalSize. Dropping them while the fog still reveals them
        // left revealed-but-undrawn holes — hard black cutouts with no shroud
        // feathering — wherever a zoomed-out view reached the map border.
        cells.push(TerrainCell {
            screen_x: sx,
            screen_y: sy,
            tile_id,
            sub_tile,
            z: cell.level,
            rx: cell.rx,
            ry: cell.ry,
            variant: cell.variant,
            tint: [1.0, 1.0, 1.0],
            radar_left: cell.radar_left,
            radar_right: cell.radar_right,
        });
        min_x = min_x.min(sx);
        min_y = min_y.min(sy);
        max_x = max_x.max(sx + TILE_WIDTH);
        max_y = max_y.max(sy + TILE_HEIGHT);
    }

    cells.sort_by(|a, b| {
        a.screen_y
            .partial_cmp(&b.screen_y)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    TerrainGrid {
        cells,
        world_width: max_x - min_x,
        world_height: max_y - min_y,
        origin_x: min_x,
        origin_y: min_y,
        bridge_middle_tiles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    #[test]
    fn gsi_04_03b_iso_to_screen_sign_extends_raw_level() {
        let (sx, sy) = iso_to_screen(0, 0, 0xff);
        assert_eq!(sx, -30.0);
        assert_eq!(sy, 30.0, "raw level 0xFF is signed -1");
    }

    #[test]
    fn screen_to_cell_tactical_inverse_returns_initial_fallback_on_scan_cap() {
        let height_map = BTreeMap::new();
        let result = screen_to_cell_tactical_inverse(
            -500.0,
            -500.0,
            TacticalInverseContext {
                cells: Some(&crate::map::terrain::TacticalCellFixture {
                    levels: &height_map,
                    bridges: None,
                }),
                viewport_offset_x: 0.0,
                viewport_offset_y: 0.0,
            },
        );

        let (rx, ry) = screen_to_iso(-500.0, -500.0);
        assert_eq!(result, TacticalInverseResult::Fallback { rx, ry });
    }

    #[test]
    fn tactical_cardinal_neighbor_rejects_direction_eight() {
        assert_eq!(tactical_cardinal_neighbor(10, 10, 8), None);
    }

    #[test]
    #[ignore = "requires RA2_DIR with retail RA2/YR assets"]
    fn gsi_02_11_xmp29u2_clear_fallback_loads_and_selects_all_retail_variants() {
        let ra2_dir = PathBuf::from(
            std::env::var("RA2_DIR").expect("set RA2_DIR to the retail RA2/YR directory"),
        );
        let mut assets = crate::assets::asset_manager::AssetManager::new(
            &ra2_dir,
            crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        )
        .expect("retail assets");
        let map_bytes = assets.get("XMP29U2.MAP").expect("XMP29U2.MAP asset");
        let map = crate::map::map_file::MapFile::from_bytes(&map_bytes).expect("retail map");
        let theater = crate::map::theater::load_theater(&mut assets, &map.header.theater)
            .expect("urban theater");
        let clear_tile_id = theater.rmg_tiles.clear_tile.expect("Urban ClearTile");
        assert_eq!(clear_tile_id, 0);
        assert_eq!(theater.lookup.variant_count(clear_tile_id), 7);
        assert_eq!(theater.lookup.total_file_count(clear_tile_id), 8);
        let suffixes: Vec<_> = theater
            .lookup
            .variant_filenames(clear_tile_id)
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect();
        assert_eq!(
            suffixes,
            (b'a'..=b'g')
                .map(|suffix| format!("clear01{}.urb", char::from(suffix)))
                .collect::<Vec<_>>()
        );
        let bounds = crate::map::playfield::PlayfieldBounds::from_map_header(&map.header);
        let mut selector_cache =
            crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        let mut main_rng = crate::sim::rng::SimRng::new(0);
        let mut raw_draw = || main_rng.next_u32();
        let mut scenario_fill_ranged = |_low, _high| 0;
        let resolved = {
            let mut selector = selector_cache.begin_load(&mut raw_draw);
            let resolved =
                crate::map::resolved_terrain::ResolvedTerrainGrid::build_with_variant_selector(
                    &map,
                    Some(&theater),
                    Some(&assets),
                    None,
                    None,
                    None,
                    true,
                    0,
                    &mut scenario_fill_ranged,
                    &mut selector,
                );
            assert!(selector.generated_table());
            resolved
        };
        let grid = build_terrain_grid_from_resolved(&resolved, None);

        const XMP29U2_NO_TILE: i32 = 0xFFFF;
        let in_bounds_sentinels: Vec<_> = map
            .cells
            .iter()
            .filter(|cell| cell.tile_index == XMP29U2_NO_TILE)
            .filter(|cell| bounds.contains_geometry_packed(i32::from(cell.rx), i32::from(cell.ry)))
            .collect();
        assert_eq!(
            map.cells
                .iter()
                .filter(|cell| cell.tile_index == XMP29U2_NO_TILE)
                .count(),
            164
        );
        assert_eq!(in_bounds_sentinels.len(), 108);
        // The map load allocates more cells than the IsoMapPack holds; every
        // allocated cell reaches presentation.
        assert_eq!(grid.cells.len(), resolved.iter().count());

        let needed = HashSet::from([crate::map::theater::TileKey {
            tile_id: clear_tile_id,
            sub_tile: 0,
            variant: 0,
        }]);
        let clear_images = crate::map::theater::load_tile_images(
            &assets,
            &theater.lookup,
            &theater.iso_palette,
            &needed,
        );
        assert!(
            clear_images.contains_key(&crate::map::theater::TileKey {
                tile_id: clear_tile_id,
                sub_tile: 0,
                variant: 0,
            }),
            "ClearTile must be available to the tactical atlas"
        );
        for variant in 0..=7 {
            assert!(
                clear_images.contains_key(&crate::map::theater::TileKey {
                    tile_id: clear_tile_id,
                    sub_tile: 0,
                    variant,
                }),
                "Clear01 variant index {variant} must reach the tactical atlas"
            );
        }

        let owner_metadata = |variant: u8| {
            let filename = theater
                .lookup
                .filename_for_variant(clear_tile_id, variant)
                .expect("contiguous Clear01 owner filename");
            let bytes = assets.get_ref(filename).expect("selected Clear01 TMP");
            let tmp = crate::assets::tmp_file::TmpFile::from_bytes(bytes)
                .expect("selected Clear01 TMP parses");
            let source_sub = crate::map::theater::wrapped_subtile_index(
                0,
                tmp.template_width,
                tmp.template_height,
            )
            .expect("Clear01 sub-tile wraps");
            let tile = tmp.tiles[source_sub]
                .as_ref()
                .expect("selected Clear01 owner has sub-tile zero");
            (
                tile.radar_left,
                tile.radar_right,
                tile.ramp_type,
                tile.height,
                tile.offset_x,
                tile.offset_y,
                tile.has_damaged_data,
            )
        };
        let pristine_cellclass = owner_metadata(0);
        let expected_by_variant: HashMap<_, _> = (0u8..=7)
            .map(|variant| {
                let owner = owner_metadata(variant);
                (variant, (owner.0, owner.1, owner.4, owner.5))
            })
            .collect();

        let presented: HashMap<_, _> = grid
            .cells
            .iter()
            .map(|cell| ((cell.rx, cell.ry), cell))
            .collect();
        let mut high_suffix_sentinels = 0usize;
        for source in in_bounds_sentinels {
            let resolved_cell = resolved
                .cell(source.rx, source.ry)
                .expect("sentinel remains in resolved terrain");
            assert_eq!(source.tile_index, XMP29U2_NO_TILE);
            assert_eq!(resolved_cell.final_tile_index, source.tile_index);

            let rendered = presented
                .get(&(source.rx, source.ry))
                .expect("sentinel reaches presentation grid");
            assert_eq!(rendered.tile_id, clear_tile_id);
            assert_eq!(rendered.sub_tile, 0);
            assert_eq!(rendered.z, source.z);
            let expected = expected_by_variant
                .get(&resolved_cell.variant)
                .expect("selected Clear01 owner metadata");
            assert_eq!(resolved_cell.radar_left, expected.0);
            assert_eq!(resolved_cell.radar_right, expected.1);
            assert_eq!(resolved_cell.slope_type, pristine_cellclass.2);
            assert_eq!(resolved_cell.template_height, pristine_cellclass.3);
            assert_eq!(resolved_cell.render_offset_x, expected.2);
            assert_eq!(resolved_cell.render_offset_y, expected.3);
            assert_eq!(resolved_cell.has_damaged_data, pristine_cellclass.6);
            assert_eq!(rendered.radar_left, expected.0);
            assert_eq!(rendered.radar_right, expected.1);
            high_suffix_sentinels += usize::from(rendered.variant > 4);
        }
        assert!(
            high_suffix_sentinels > 0,
            "at least one in-playfield ClearTile fallback must select e/f/g"
        );
    }
}
