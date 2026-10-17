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

/// The tactical inverse queries current CellClass flags at each candidate and
/// neighbor, not a map-load bridge list (original6D6760..6D6888). This borrowed
/// interface keeps the inverse independent of its real-cell storage owner.
pub trait TacticalBridgeLookup: std::fmt::Debug {
    fn bridge_cell(&self, rx: u16, ry: u16) -> Option<TacticalBridgeCell>;
}

/// Native oracle fixtures supply literal cells; production uses the live grid.
#[cfg(any(test, feature = "test-support"))]
impl TacticalBridgeLookup for BTreeMap<(u16, u16), TacticalBridgeCell> {
    fn bridge_cell(&self, rx: u16, ry: u16) -> Option<TacticalBridgeCell> {
        self.get(&(rx, ry)).copied()
    }
}

/// Inputs for the YR-shaped tactical screen-to-cell inverse.
#[derive(Debug, Clone, Copy)]
pub struct TacticalInverseContext<'a> {
    pub height_map: &'a BTreeMap<(u16, u16), u8>,
    pub bridge_cells: Option<&'a dyn TacticalBridgeLookup>,
    pub viewport_offset_x: f32,
    pub viewport_offset_y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TacticalInverseResult {
    Cell { rx: f32, ry: f32 },
    Fallback { rx: f32, ry: f32 },
}

/// Tactical scroll rectangle derived from the final MapClass Size/LocalSize.
/// Coordinates are VERA world pixels. The existing camera clamp subtracts
/// half a tile from `pixel_x` and from the far Y edge; the constructor adapts
/// native6D8640 to that convention. Radar-cell extents have different vertical
/// headroom and are owned separately by PlayfieldPresentationGeometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalBounds {
    pub pixel_x: f32,
    pub pixel_y: f32,
    pub pixel_w: f32,
    pub pixel_h: f32,
}

/// Exact presentation projection of the current normalized MapClass
/// playfield. Native `ComputeRadarMapBounds` enumerates mode-zero-valid cells;
/// this mirrors that ownership instead of reconstructing a raw-header TS pixel
/// rectangle. The extent is expressed in the flat (z=0) tile frame so terrain,
/// overlays, radar events and click inverse share one space. Native camera
/// clamping reads the same MapClass fields through its own arithmetic.
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

impl TerrainGrid {
    /// Install camera authority derived from the current normalized playfield.
    /// `None` intentionally removes the raw-header approximation.
    pub fn install_playfield_local_bounds(
        &mut self,
        bounds: Option<crate::map::playfield::PlayfieldBounds>,
    ) {
        self.local_bounds = bounds.map(LocalBounds::from_playfield);
    }
}

impl LocalBounds {
    /// ClampViewCenter6D8640 reads final MapClass fields directly. Its near
    /// X/Y center bounds add the viewport's integer halves; far bounds add
    /// 60*LocalWidth and (60*LocalHeight+270)/2, then subtract the full view.
    /// Keep those extent products in native wrapping i32 arithmetic before
    /// conversion to the presentation API. Original load/resize controls:
    /// tools/input_oracle/fast_scroll.json/camera_save_load.
    pub fn from_playfield(bounds: crate::map::playfield::PlayfieldBounds) -> Self {
        let left = bounds
            .off_fc
            .wrapping_mul(2)
            .wrapping_sub(bounds.base)
            .wrapping_mul(30);
        let top = bounds
            .base
            .wrapping_add(bounds.off_100.wrapping_mul(2))
            .wrapping_sub(5)
            .wrapping_mul(15);
        let width = bounds.off_104.wrapping_mul(60);
        let height = bounds.off_108.wrapping_mul(60).wrapping_add(270) / 2;
        let world_bias = crate::util::lepton::absolute_leptons_to_screen(0, 0, 0);
        Self {
            pixel_x: left as f32 + world_bias.0 + TILE_WIDTH / 2.0,
            pixel_y: top as f32 + world_bias.1,
            pixel_w: width as f32,
            pixel_h: height as f32 + TILE_HEIGHT / 2.0,
        }
    }

    /// Check if a screen position is within the playable area.
    pub fn contains(&self, screen_x: f32, screen_y: f32) -> bool {
        screen_x >= self.pixel_x
            && screen_x < self.pixel_x + self.pixel_w
            && screen_y >= self.pixel_y
            && screen_y < self.pixel_y + self.pixel_h
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
    /// True when the resolved terrain classifies the cell as water.
    pub is_water: bool,
    /// Tile visual variant index: 0 = pristine, positive = suffix sibling.
    pub variant: u8,
    /// RGB color tint from map lighting. [1,1,1] = full brightness (default).
    pub tint: [f32; 3],
    /// Per-tile radar minimap color (left half of isometric diamond), from TMP header.
    pub radar_left: [u8; 3],
    /// Per-tile radar minimap color (right half of isometric diamond), from TMP header.
    pub radar_right: [u8; 3],
    /// Mirrors `ResolvedTerrainCell.has_damaged_data` — true when this cell's
    /// TMP sub-tile carries a baked damaged-variant pixel set. Cached at
    /// `TerrainGrid` construction; treat as map-load-immutable. Drives the
    /// per-frame variant-override decision in `build_visible_instances`.
    pub has_damaged_data: bool,
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
    /// Playable area bounds (from LocalSize). Used to clip overlays/entities too.
    pub local_bounds: Option<LocalBounds>,
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

/// Convert absolute lepton-world coords to screen pixels with sub-cell
/// precision — **the entity frame**, the absolute-lepton twin of
/// `util::lepton::lepton_to_screen`.
///
/// The shared projector preserves active YR's signed integer arithmetic and
/// final `/ 256` truncation. The returned `f32`s therefore always represent
/// whole native pixels; float conversion is only the render API boundary.
pub fn lepton_to_screen(coords: glam::IVec3) -> (f32, f32) {
    crate::util::lepton::absolute_leptons_to_screen(coords.x, coords.y, coords.z)
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
            .height_map
            .get(&(cell_rx, cell_ry))
            .copied()
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
    let bridge_cells = context.bridge_cells?;
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

    let dir2_height = tactical_neighbor_height(context.height_map, cell_rx, cell_ry, DIR_EAST);
    let dir4_height = tactical_neighbor_height(context.height_map, cell_rx, cell_ry, DIR_SOUTH);
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
    bridge_cells: &dyn TacticalBridgeLookup,
    rx: u16,
    ry: u16,
    direction: u8,
) -> Option<TacticalBridgeCell> {
    let (nx, ny) = tactical_cardinal_neighbor(rx, ry, direction)?;
    bridge_cells.bridge_cell(nx, ny)
}

fn tactical_neighbor_height(
    height_map: &BTreeMap<(u16, u16), u8>,
    rx: u16,
    ry: u16,
    direction: u8,
) -> u8 {
    tactical_cardinal_neighbor(rx, ry, direction)
        .and_then(|pos| height_map.get(&pos).copied())
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

/// Convert screen-space pixel position to isometric cell, accounting for terrain elevation.
///
/// Iteratively refines the cell guess by looking up the actual terrain height at each
/// candidate cell and re-solving with the corrected screen Y. This fixes the z=0
/// assumption in `screen_to_iso` which causes clicks on elevated terrain to resolve
/// to the wrong cell.
///
/// Converges in 1-3 iterations on typical RA2 terrain gradients.
#[cfg(any(test, feature = "test-support"))]
pub fn screen_to_iso_with_height(
    screen_x: f32,
    screen_y: f32,
    height_map: &BTreeMap<(u16, u16), u8>,
) -> (f32, f32) {
    screen_to_iso_with_height_and_bridges(screen_x, screen_y, height_map, None)
}

/// Convert screen coordinates to isometric cell coordinates, accounting for
/// terrain height and optionally bridge deck height.
///
/// When `bridge_height_map` is provided and the resolved cell has a bridge deck,
/// the bridge deck elevation is used instead of the ground elevation. This makes
/// clicks on high bridge surfaces resolve to the correct cell.
pub fn screen_to_iso_with_height_and_bridges(
    screen_x: f32,
    screen_y: f32,
    height_map: &BTreeMap<(u16, u16), u8>,
    bridge_height_map: Option<&BTreeMap<(u16, u16), u8>>,
) -> (f32, f32) {
    // First pass: resolve using ground height (existing behavior).
    let (mut rx, mut ry) = screen_to_iso(screen_x, screen_y);
    for _ in 0..3 {
        let cell_rx: u16 = rx.round().max(0.0) as u16;
        let cell_ry: u16 = ry.round().max(0.0) as u16;
        let z: u8 = height_map.get(&(cell_rx, cell_ry)).copied().unwrap_or(0);
        if z == 0 {
            break;
        }
        let corrected_y: f32 = screen_y + f32::from(z as i8) * HEIGHT_STEP;
        let (new_rx, new_ry) = screen_to_iso(screen_x, corrected_y);
        if (new_rx - rx).abs() < 0.01 && (new_ry - ry).abs() < 0.01 {
            break;
        }
        rx = new_rx;
        ry = new_ry;
    }

    // Second pass: bridge deck click resolution. The bridge surface is elevated
    // (deck_level = ground + 4), so the ground resolution above can shift the
    // result by up to ~3 cells away from the actual bridge cell. We search a
    // neighborhood around the ground-resolved cell for any bridge entries and
    // test each at its deck height. The closest match wins.
    if let Some(bridge_map) = bridge_height_map {
        let cell_rx: u16 = rx.round().max(0.0) as u16;
        let cell_ry: u16 = ry.round().max(0.0) as u16;
        let mut best: Option<(f32, f32)> = None;
        let mut best_dist: f32 = f32::MAX;
        for dy in -3i32..=3 {
            for dx in -3i32..=3 {
                let bx_i: i32 = cell_rx as i32 + dx;
                let by_i: i32 = cell_ry as i32 + dy;
                if bx_i < 0 || by_i < 0 {
                    continue;
                }
                let bx: u16 = bx_i as u16;
                let by: u16 = by_i as u16;
                if let Some(&bridge_z) = bridge_map.get(&(bx, by)) {
                    let corrected_y: f32 = screen_y + f32::from(bridge_z as i8) * HEIGHT_STEP;
                    let (new_rx, new_ry) = screen_to_iso(screen_x, corrected_y);
                    let dist: f32 = (new_rx - bx as f32).abs() + (new_ry - by as f32).abs();
                    if dist < 0.7 && dist < best_dist {
                        best = Some((new_rx, new_ry));
                        best_dist = dist;
                    }
                }
            }
        }
        if let Some((brx, bry)) = best {
            rx = brx;
            ry = bry;
        }
    }

    (rx, ry)
}

/// Build a TerrainGrid from the resolved terrain stage: the final
/// LAT-adjusted tile choice and the water classification from resolved
/// terrain metadata.
pub fn build_terrain_grid_from_resolved(
    resolved: &ResolvedTerrainGrid,
    local_bounds: Option<LocalBounds>,
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
            is_water: cell.final_tile_index >= 0 && cell.is_water,
            variant: cell.variant,
            tint: [1.0, 1.0, 1.0],
            radar_left: cell.radar_left,
            radar_right: cell.radar_right,
            has_damaged_data: cell.has_damaged_data,
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
        local_bounds,
        bridge_middle_tiles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    #[test]
    fn test_iso_to_screen_origin() {
        // (0,0,0) → X = 0-30 = -30, Y = 0+15 = 15
        let (sx, sy): (f32, f32) = iso_to_screen(0, 0, 0);
        assert!((sx - (-30.0)).abs() < f32::EPSILON);
        assert!((sy - 15.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_iso_to_screen_positive() {
        // rx=10, ry=0, z=0 → sx = 300-30 = 270, sy = 150+15 = 165
        let (sx, sy): (f32, f32) = iso_to_screen(10, 0, 0);
        assert!((sx - 270.0).abs() < f32::EPSILON);
        assert!((sy - 165.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_iso_to_screen_diagonal() {
        // rx=5, ry=5, z=0 → sx = 0-30 = -30, sy = 150+15 = 165
        let (sx, sy): (f32, f32) = iso_to_screen(5, 5, 0);
        assert!((sx - (-30.0)).abs() < f32::EPSILON);
        assert!((sy - 165.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_iso_to_screen_elevation() {
        // rx=0, ry=0, z=2 → sx = -30, sy = 15 - 30 = -15
        let (sx, sy): (f32, f32) = iso_to_screen(0, 0, 2);
        assert!((sx - (-30.0)).abs() < f32::EPSILON);
        assert!((sy - (-15.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn gsi_04_03b_iso_to_screen_sign_extends_raw_level() {
        let (sx, sy) = iso_to_screen(0, 0, 0xff);
        assert_eq!(sx, -30.0);
        assert_eq!(sy, 30.0, "raw level 0xFF is signed -1");
    }

    #[test]
    fn screen_to_cell_tactical_inverse_uses_vertical_height_scan() {
        let mut height_map = BTreeMap::new();
        height_map.insert((10, 5), 4);
        let result = screen_to_cell_tactical_inverse(
            150.0,
            180.0,
            TacticalInverseContext {
                height_map: &height_map,
                bridge_cells: None,
                viewport_offset_x: 0.0,
                viewport_offset_y: 0.0,
            },
        );

        assert_eq!(result, TacticalInverseResult::Cell { rx: 9.5, ry: 4.5 });
    }

    #[test]
    fn screen_to_cell_tactical_inverse_returns_initial_fallback_on_scan_cap() {
        let height_map = BTreeMap::new();
        let result = screen_to_cell_tactical_inverse(
            -500.0,
            -500.0,
            TacticalInverseContext {
                height_map: &height_map,
                bridge_cells: None,
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
    fn tactical_bridge_edge_threshold_is_strict() {
        let height_map = BTreeMap::from([((0, 0), 0)]);
        let mut bridge_cells = BTreeMap::new();
        bridge_cells.insert(
            (0, 0),
            TacticalBridgeCell {
                structural: true,
                direction_zero: true,
            },
        );
        bridge_cells.insert(
            (1, 0),
            TacticalBridgeCell {
                structural: true,
                direction_zero: true,
            },
        );
        bridge_cells.insert(
            (0, 1),
            TacticalBridgeCell {
                structural: true,
                direction_zero: true,
            },
        );

        let mut adjusted = 90.0;
        assert_eq!(
            apply_tactical_bridge_inverse(
                0.0,
                30.0,
                90.0,
                0,
                0,
                0,
                TacticalInverseContext {
                    height_map: &height_map,
                    bridge_cells: Some(&bridge_cells),
                    viewport_offset_x: 0.0,
                    viewport_offset_y: 0.0,
                },
                &mut adjusted,
            ),
            None
        );
        assert_eq!(adjusted, 90.0);

        let mut adjusted = 90.0;
        assert_eq!(
            apply_tactical_bridge_inverse(
                0.0,
                31.0,
                90.0,
                0,
                0,
                0,
                TacticalInverseContext {
                    height_map: &height_map,
                    bridge_cells: Some(&bridge_cells),
                    viewport_offset_x: 0.0,
                    viewport_offset_y: 0.0,
                },
                &mut adjusted,
            ),
            None
        );
        assert_eq!(adjusted, 30.0);
    }

    #[test]
    fn test_local_bounds_contains() {
        let bounds = LocalBounds {
            pixel_x: -1950.0,
            pixel_y: 1215.0,
            pixel_w: 3900.0,
            pixel_h: 2010.0,
        };
        assert!(bounds.contains(-1950.0, 1215.0)); // top-left (inclusive)
        assert!(bounds.contains(0.0, 2000.0)); // center
        assert!(!bounds.contains(-1951.0, 1215.0)); // just left
        assert!(!bounds.contains(-1950.0, 1214.0)); // just above
        assert!(!bounds.contains(1950.0, 1215.0)); // at right edge (exclusive)
        assert!(!bounds.contains(-1950.0, 3225.0)); // at bottom edge (exclusive)
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
        let bounds = LocalBounds::from_playfield(
            crate::map::playfield::PlayfieldBounds::from_map_header(&map.header),
        );
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
        let grid = build_terrain_grid_from_resolved(&resolved, Some(bounds), None);

        const XMP29U2_NO_TILE: i32 = 0xFFFF;
        let in_bounds_sentinels: Vec<_> = map
            .cells
            .iter()
            .filter(|cell| cell.tile_index == XMP29U2_NO_TILE)
            .filter(|cell| {
                let (sx, sy) = iso_to_screen(cell.rx, cell.ry, cell.z);
                bounds.contains(sx, sy)
            })
            .collect();
        assert_eq!(
            map.cells
                .iter()
                .filter(|cell| cell.tile_index == XMP29U2_NO_TILE)
                .count(),
            164
        );
        assert_eq!(in_bounds_sentinels.len(), 125);
        assert_eq!(grid.cells.len(), 6_160);

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
            "at least one of the 125 visible ClearTile fallbacks must select e/f/g"
        );
    }

    #[test]
    fn test_screen_to_iso_with_height_flat_terrain() {
        // On flat terrain (z=0 everywhere), result matches plain screen_to_iso.
        let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
        let (rx, ry) = screen_to_iso_with_height(300.0, 150.0, &height_map);
        let (rx0, ry0) = screen_to_iso(300.0, 150.0);
        assert!((rx - rx0).abs() < 0.01);
        assert!((ry - ry0).abs() < 0.01);
    }

    #[test]
    fn test_screen_to_iso_with_height_elevated() {
        // Cell (10, 5) at z=4:
        //   iso_to_screen = ((10-5)*30-30, (10+5)*15+15-4*15) = (120, 165)
        //   Tile center = (120+30, 165+15) = (150, 180)
        let mut height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
        for rx in 8..=12 {
            for ry in 3..=7 {
                height_map.insert((rx, ry), 4);
            }
        }
        let (rx, ry) = screen_to_iso_with_height(150.0, 180.0, &height_map);
        assert!((rx - 10.0).abs() < 0.6, "rx={rx}, expected ~10");
        assert!((ry - 5.0).abs() < 0.6, "ry={ry}, expected ~5");
    }

    #[test]
    fn test_screen_to_iso_with_height_convergence() {
        // Verify the function converges and doesn't overshoot on steep terrain.
        let mut height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
        // A ridge: cells with ry < 10 are at z=6, ry >= 10 at z=0.
        for rx in 0..30 {
            for ry in 0..10 {
                height_map.insert((rx, ry), 6);
            }
        }
        // Click on the elevated part: cell (15, 5) at z=6.
        // iso_to_screen = ((15-5)*30-30, (15+5)*15+15-6*15) = (270, 225)
        // Center: (270+30, 225+15) = (300, 240).
        let (rx, ry) = screen_to_iso_with_height(300.0, 240.0, &height_map);
        assert!((rx - 15.0).abs() < 0.6, "rx={rx}, expected ~15");
        assert!((ry - 5.0).abs() < 0.6, "ry={ry}, expected ~5");
    }

    #[test]
    fn lepton_to_screen_zero_matches_iso_origin() {
        let (sx, sy) = lepton_to_screen(glam::IVec3::ZERO);
        assert_eq!(sx, 0.0);
        assert_eq!(sy, TILE_HEIGHT / 2.0);
    }

    #[test]
    fn lepton_to_screen_integer_cell_lands_at_iso_center() {
        // 4 cells east, 2 cells south = (4*256, 2*256, 0).
        let (sx, sy) = lepton_to_screen(glam::IVec3::new(4 * 256, 2 * 256, 0));
        assert_eq!(sx, (4.0 - 2.0) * TILE_WIDTH / 2.0);
        assert_eq!(sy, (4.0 + 2.0) * TILE_HEIGHT / 2.0 + TILE_HEIGHT / 2.0);
    }

    #[test]
    fn lepton_to_screen_sub_cell_offset_is_iso_subdivided() {
        // Sub-cell offset of (128, 0) — native truncates the complete Y
        // numerator from 7.5 to 7 before VERA's common +15 row bias.
        let (sx, sy) = lepton_to_screen(glam::IVec3::new(128, 0, 0));
        assert_eq!((sx, sy), (15.0, 22.0));
    }

    #[test]
    fn lepton_to_screen_negative_coords_use_signed_truncation() {
        let (sx, sy) = lepton_to_screen(glam::IVec3::new(-50, 0, 0));
        assert_eq!((sx, sy), (-5.0, 13.0));
    }

    #[test]
    fn adjust_for_z_lepton_projection_matches_retail_fixtures_and_boundary() {
        let (_, baseline) = lepton_to_screen(glam::IVec3::ZERO);
        for (z, expected_lift) in [
            (104, 15.0),
            (208, 30.0),
            (256, 37.0),
            (727, 104.0),
            (728, 105.0),
            (1_500, 216.0),
            (-400, -56.0),
        ] {
            let (_, projected) = lepton_to_screen(glam::IVec3::new(0, 0, z));
            assert_eq!(baseline - projected, expected_lift, "z={z}");
        }
    }
}
