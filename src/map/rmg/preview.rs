//! Terrain preview image for a generated random map.
//!
//! Turns a generated map into the RGBA thumbnail the setup dialog shows in its
//! preview box and that gets written out as `RandMap.img`. Depends only on the
//! cell data handed in — no assets, no wgpu, no sim.
//!
//! The geometry here is not free-form: the original projects cell centres
//! through the isometric transform, sizes the surface from the projected extent
//! of the playable cells, and emits *two* horizontal pixels per cell. Anything
//! that changes the projection, the rounding, or the doubling changes the image
//! the player sees, so each step is pinned by a test below.

use crate::map::playfield::PlayfieldBounds;

/// A cell to draw: its grid position and the two radar colours the isometric
/// diamond's left and right halves contribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewCell {
    pub x: u16,
    pub y: u16,
    /// Left half's colour, drawn at the even pixel.
    pub left: [u8; 3],
    /// Right half's colour, drawn at the odd pixel.
    pub right: [u8; 3],
}

/// The rendered thumbnail. `rgba` is row-major, 4 bytes per pixel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Leptons per cell; a cell centre sits half a cell in on both axes.
const LEPTONS_PER_CELL: i32 = crate::util::lepton::LEPTONS_PER_CELL_I32;
const CELL_CENTRE_LEPTONS: i32 = crate::util::lepton::CELL_CENTER_LEPTON_I32;
/// Constant added to the projected X after the shift, keeping the whole
/// playfield in positive territory.
const PROJECT_X_BIAS: i32 = 0x3C00;
/// Divisors that take the projected coordinate down to preview-pixel space.
const PREVIEW_X_DIVISOR: i32 = 0x3C;
const PREVIEW_Y_DIVISOR: i32 = 0x1E;
/// Substituted whenever a cell's colour comes back black, which the original
/// treats as a data fault rather than a legitimate colour.
const BLACK_PIXEL_SUBSTITUTE: [u8; 3] = [0x80, 0x80, 0x80];
/// Baked start markers: a 4x4 square in this red, offset one pixel up and left
/// of the waypoint's own pixel.
const MARKER_SIZE: i32 = 4;
const MARKER_RGB: [u8; 3] = [0xF0, 0x00, 0x00];
const MARKER_ORIGIN_BIAS: i32 = -1;
/// Only the first eight waypoints get a marker.
pub const MARKER_WAYPOINT_COUNT: u8 = 8;

/// Project a cell centre to the preview's coordinate space.
///
/// Delegates to the canonical native projector (halved 60/30 terms, then a
/// toward-zero /256 — `util::lepton::project_absolute_lepton_xy`); only the
/// preview's X bias is local. Cell-centre inputs always divide exactly, so
/// the i64 no-wrap canonical is bit-identical here.
fn project_cell_centre(cell_x: i32, cell_y: i32) -> (i32, i32) {
    let lepton_x = cell_x * LEPTONS_PER_CELL + CELL_CENTRE_LEPTONS;
    let lepton_y = cell_y * LEPTONS_PER_CELL + CELL_CENTRE_LEPTONS;
    let (projected_x, projected_y) =
        crate::util::lepton::project_absolute_lepton_xy(lepton_x, lepton_y);
    (projected_x + PROJECT_X_BIAS, projected_y)
}

/// Preview-pixel column and row for a projected cell, before the surface origin
/// is subtracted. X is in *pairs* — the caller doubles it.
const fn preview_cell_column(projected_x: i32) -> i32 {
    projected_x / PREVIEW_X_DIVISOR
}

const fn preview_cell_row(projected_y: i32) -> i32 {
    projected_y / PREVIEW_Y_DIVISOR
}

/// The projected extent of a set of cells: `(min_col, min_row, max_col, max_row)`.
fn projected_bounds(cells: &[PreviewCell]) -> Option<(i32, i32, i32, i32)> {
    let mut bounds: Option<(i32, i32, i32, i32)> = None;
    for cell in cells {
        let (projected_x, projected_y) = project_cell_centre(i32::from(cell.x), i32::from(cell.y));
        let column = preview_cell_column(projected_x);
        let row = preview_cell_row(projected_y);
        bounds = Some(match bounds {
            None => (column, row, column, row),
            Some((min_col, min_row, max_col, max_row)) => (
                min_col.min(column),
                min_row.min(row),
                max_col.max(column),
                max_row.max(row),
            ),
        });
    }
    bounds
}

/// Map/RMG-facing view of the canonical MapClass playfield bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Playfield {
    bounds: PlayfieldBounds,
}

impl Playfield {
    /// Derive the playfield from a raw map header through the final normalized
    /// fields established by `0x004AD79D -> 0x00654490 -> 0x00567230`.
    pub fn from_header(header: &crate::map::map_file::MapHeader) -> Self {
        Self {
            bounds: PlayfieldBounds::from_map_header(header),
        }
    }

    /// The same geometry from final normalized LocalSize fields.
    ///
    /// Active RMG's synthetic `(2,5,genW,genH)` tuple is already stable under
    /// native normalization. Raw authored headers must use [`Self::from_header`].
    pub const fn from_local_size(
        map_width: i32,
        left: i32,
        top: i32,
        width: i32,
        height: i32,
    ) -> Self {
        Self {
            bounds: PlayfieldBounds::from_normalized_local_size(
                map_width, left, top, width, height,
            ),
        }
    }

    /// Whether a cell is inside the playfield.
    ///
    /// Deliberately takes no elevation: the test is constant in `z`, so feeding
    /// it a height would make cells drop in and out of the preview as the
    /// terrain rises. Callers that *do* want the elevation-aware form — map
    /// generation is the one that does — use [`Playfield::contains_raised`].
    pub const fn contains(&self, x: u16, y: u16) -> bool {
        self.bounds.contains_geometry_packed(x as i32, y as i32)
    }

    /// The elevation-aware playfield test: the same rectangle, with its
    /// `x + y` band **shifted** by the cell's terrain level.
    ///
    /// Raised ground is drawn further up the screen, so the band of cells that
    /// lands inside the visible playfield moves with elevation. The band is
    /// shifted, not widened — both bounds move by the same amount — so a tall
    /// cell gains room at the far edge and loses it at the near one.
    ///
    /// A cell that is *sloped* and sits near the near edge counts one step
    /// higher still, because the ramp's upper lip already reads as the next
    /// level up. That extra step needs the slope byte, which is why it is a
    /// parameter and not derived from the level alone.
    ///
    /// The `diff` bounds do not move; elevation only shifts along `x + y`.
    pub const fn contains_raised(&self, x: u16, y: u16, level: i8, slope: u8) -> bool {
        self.bounds
            .contains_height_aware_packed(x as i32, y as i32, level, slope)
    }
}

/// Overlay id bands whose radar colour comes back with green and blue swapped.
///
/// Two contiguous bands are re-packed on the way out; everything else is copied
/// straight through. Copying all of them straight through leaves these bands the
/// wrong hue while the rest look correct, which is a miserable thing to spot by
/// eye.
const SWAPPED_CHANNEL_BANDS: [(u8, u8); 2] = [(0x7F, 0x8A), (0x93, 0x9E)];

/// Reorder an overlay's radar triple for the bands that need it.
pub const fn overlay_radar_channel_order(overlay_id: u8, rgb: [u8; 3]) -> [u8; 3] {
    let mut band = 0;
    while band < SWAPPED_CHANNEL_BANDS.len() {
        let (low, high) = SWAPPED_CHANNEL_BANDS[band];
        if overlay_id >= low && overlay_id <= high {
            return [rgb[0], rgb[2], rgb[1]];
        }
        band += 1;
    }
    rgb
}

/// Density (growth stage) of each overlay cell, keyed by cell position.
pub type OverlayDensities = std::collections::HashMap<(u16, u16), (u8, u8)>;

/// Index a map's overlays by cell so the preview can look up id and density.
pub fn overlay_densities(map: &crate::map::map_file::MapFile) -> OverlayDensities {
    map.overlays
        .iter()
        .map(|overlay| {
            (
                (overlay.rx, overlay.ry),
                (overlay.overlay_id, overlay.frame),
            )
        })
        .collect()
}

/// Collect the playfield cells of a map with the radar colours the resolved
/// terrain gave them.
///
/// Every playfield cell contributes, including ones the resolver produced no
/// entry for: a missing colour becomes the same grey a black lookup does, rather
/// than dropping the cell. That matters because the preview's surface is sized
/// from the extent of this set, so silently skipping cells shrinks the image.
/// The colours the rasteriser needs, with no asset handles — safe to move to a
/// worker thread.
///
/// Keyed on the cell identity as it appears in the map file, with the values
/// taken from what the terrain resolver produced for cells carrying that
/// identity. Recording the resolver's answer rather than re-deriving one is the
/// point: the resolver decides which tile a cell actually ends up with, and a
/// table that second-guessed it would drift from the colours on screen today.
///
/// A tile the table has never seen falls back to black, which the rasteriser
/// then substitutes with grey — the same treatment a failed colour lookup gets
/// in the original.
#[derive(Debug, Clone, Default)]
pub struct PreviewPalette {
    tiles: std::collections::HashMap<(i32, u8), ([u8; 3], [u8; 3])>,
    overlays: std::collections::HashMap<(u8, u8), [u8; 3]>,
}

impl PreviewPalette {
    /// Record what the resolver produced for every tile identity this map uses,
    /// plus the overlay colours its overlays resolve to.
    pub fn from_map(
        map: &crate::map::map_file::MapFile,
        resolved: &crate::map::resolved_terrain::ResolvedTerrainGrid,
        overlay_radar: &dyn Fn(u8, u8) -> Option<[u8; 3]>,
    ) -> Self {
        let mut palette = Self::default();
        for cell in &map.cells {
            if let Some(resolved_cell) = resolved.cell(cell.rx, cell.ry) {
                palette
                    .tiles
                    .entry((cell.tile_index, cell.sub_tile))
                    .or_insert((resolved_cell.radar_left, resolved_cell.radar_right));
            }
        }
        for overlay in &map.overlays {
            let key = (overlay.overlay_id, overlay.frame);
            if palette.overlays.contains_key(&key) {
                continue;
            }
            if let Some(rgb) = overlay_radar(overlay.overlay_id, overlay.frame) {
                palette.overlays.insert(key, rgb);
            }
        }
        palette
    }

    /// The terrain colour pair for a raw cell identity.
    pub fn tile_colours(&self, tile_index: i32, sub_tile: u8) -> ([u8; 3], [u8; 3]) {
        self.tiles
            .get(&(tile_index, sub_tile))
            .copied()
            .unwrap_or(([0, 0, 0], [0, 0, 0]))
    }

    /// The overlay colour for an id and growth stage, already channel-ordered.
    pub fn overlay_colour(&self, overlay_id: u8, density: u8) -> Option<[u8; 3]> {
        self.overlays
            .get(&(overlay_id, density))
            .map(|rgb| overlay_radar_channel_order(overlay_id, *rgb))
            .filter(|rgb| *rgb != [0, 0, 0])
    }

    /// How many distinct tile identities the table covers, for diagnostics.
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }
}

/// `overlay_radar` resolves an ore/gem overlay's colour from its id and growth
/// stage. An overlay that resolves to a colour paints BOTH pixels with it —
/// unlike terrain, whose two halves differ — so ore reads as a solid patch
/// rather than a dither of ore and ground.
pub fn preview_cells_from_map(
    map: &crate::map::map_file::MapFile,
    resolved: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    overlay_radar: &dyn Fn(u8, u8) -> Option<[u8; 3]>,
) -> Vec<PreviewCell> {
    let playfield = Playfield::from_header(&map.header);
    let overlays = overlay_densities(map);
    let mut cells = Vec::with_capacity(map.cells.len());
    for cell in &map.cells {
        if !playfield.contains(cell.rx, cell.ry) {
            continue;
        }
        let overlay = overlays
            .get(&(cell.rx, cell.ry))
            .and_then(|(id, density)| {
                overlay_radar(*id, *density).map(|rgb| overlay_radar_channel_order(*id, rgb))
            })
            // A black overlay colour means "not an ore/gem overlay" rather than
            // a real colour, so it falls through to the terrain underneath.
            .filter(|rgb| *rgb != [0, 0, 0]);
        let resolved_cell = resolved.cell(cell.rx, cell.ry);
        let (left, right) = match overlay {
            Some(rgb) => (rgb, rgb),
            None => (
                resolved_cell.map_or([0, 0, 0], |resolved| resolved.radar_left),
                resolved_cell.map_or([0, 0, 0], |resolved| resolved.radar_right),
            ),
        };
        cells.push(PreviewCell {
            x: cell.rx,
            y: cell.ry,
            left,
            right,
        });
    }
    cells
}

/// Start-position waypoints in slot order, ready for [`render_preview`].
pub fn marker_waypoints(start_waypoints: &[(u8, u16, u16)]) -> Vec<(u16, u16)> {
    let mut ordered: Vec<(u8, u16, u16)> = start_waypoints.to_vec();
    ordered.sort_by_key(|(slot, _, _)| *slot);
    ordered
        .into_iter()
        .map(|(_, x, y)| (x, y))
        .take(MARKER_WAYPOINT_COUNT as usize)
        .collect()
}

/// Render the preview for a generated map.
///
/// `cells` must already be filtered to the playable area — the surface is sized
/// from their projected extent, so border filler would inflate the image.
/// `waypoints` are `(cell_x, cell_y)` per start position in slot order; only the
/// first [`MARKER_WAYPOINT_COUNT`] are marked, and the markers are baked into
/// the image rather than drawn over it later.
///
/// Returns `None` when there is nothing to draw or the extent collapses to an
/// empty surface.
pub fn render_preview(cells: &[PreviewCell], waypoints: &[(u16, u16)]) -> Option<PreviewImage> {
    let (min_col, min_row, max_col, max_row) = projected_bounds(cells)?;

    // X is doubled because each cell contributes two horizontal pixels; Y is
    // not, because each cell is one row tall.
    let width = (max_col - min_col) * 2;
    let height = max_row - min_row;
    if width <= 0 || height <= 0 {
        return None;
    }
    let (width, height) = (width as u32, height as u32);

    let mut image = PreviewImage {
        width,
        height,
        rgba: vec![0; (width as usize) * (height as usize) * 4],
    };

    for cell in cells {
        let (projected_x, projected_y) = project_cell_centre(i32::from(cell.x), i32::from(cell.y));
        let column = (preview_cell_column(projected_x) - min_col) * 2;
        let row = preview_cell_row(projected_y) - min_row;
        image.put_pixel(column, row, substitute_if_black(cell.left));
        image.put_pixel(column + 1, row, substitute_if_black(cell.right));
    }

    for (cell_x, cell_y) in waypoints.iter().take(MARKER_WAYPOINT_COUNT as usize) {
        let (projected_x, projected_y) =
            project_cell_centre(i32::from(*cell_x), i32::from(*cell_y));
        let column = (preview_cell_column(projected_x) - min_col) * 2 + MARKER_ORIGIN_BIAS;
        let row = preview_cell_row(projected_y) - min_row + MARKER_ORIGIN_BIAS;
        image.fill_rect(column, row, MARKER_SIZE, MARKER_SIZE, MARKER_RGB);
    }

    Some(image)
}

/// A black colour means the lookup failed rather than that the cell is black,
/// so it becomes mid grey.
const fn substitute_if_black(rgb: [u8; 3]) -> [u8; 3] {
    if rgb[0] == 0 && rgb[1] == 0 && rgb[2] == 0 {
        BLACK_PIXEL_SUBSTITUTE
    } else {
        rgb
    }
}

impl PreviewImage {
    /// Write one pixel, ignoring anything outside the surface. Markers near the
    /// edge legitimately hang off it and are clipped rather than growing it.
    fn put_pixel(&mut self, x: i32, y: i32, rgb: [u8; 3]) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let offset = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        self.rgba[offset] = rgb[0];
        self.rgba[offset + 1] = rgb[1];
        self.rgba[offset + 2] = rgb[2];
        self.rgba[offset + 3] = 0xFF;
    }

    fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, rgb: [u8; 3]) {
        for row in y..y + h {
            for column in x..x + w {
                self.put_pixel(column, row, rgb);
            }
        }
    }

    /// The pixel at `(x, y)` as RGB, for tests and callers that inspect output.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 3]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        Some([
            self.rgba[offset],
            self.rgba[offset + 1],
            self.rgba[offset + 2],
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(x: u16, y: u16, left: [u8; 3], right: [u8; 3]) -> PreviewCell {
        PreviewCell { x, y, left, right }
    }

    /// A filled diamond of cells, which is the shape the generator produces.
    fn diamond(extent: u16) -> Vec<PreviewCell> {
        let mut cells = Vec::new();
        for y in 0..extent {
            for x in 0..extent {
                cells.push(cell(x, y, [10, 20, 30], [40, 50, 60]));
            }
        }
        cells
    }

    #[test]
    fn origin_cell_projects_to_the_x_bias() {
        // Cell 0's centre sits on the projection's axis, so X lands exactly on
        // the bias and Y on zero.
        assert_eq!(project_cell_centre(0, 0), (PROJECT_X_BIAS, 15));
    }

    #[test]
    fn moving_east_and_south_moves_opposite_ways_in_x() {
        let (east_x, east_y) = project_cell_centre(10, 0);
        let (south_x, south_y) = project_cell_centre(0, 10);
        let (origin_x, _) = project_cell_centre(0, 0);
        assert!(east_x > origin_x, "east goes right");
        assert!(south_x < origin_x, "south goes left");
        assert_eq!(
            preview_cell_row(east_y),
            preview_cell_row(south_y),
            "both are the same distance down the screen"
        );
    }

    #[test]
    fn projection_matches_the_canonical_with_preview_bias() {
        // Cell (0, 1): leptons (128, 384) → canonical (-30, 30); only the X
        // bias is preview-local. The toward-zero truncation of the shared
        // canonical is pinned at util::direction_tables and mission::readiness.
        assert_eq!(project_cell_centre(0, 1), (-30 + PROJECT_X_BIAS, 30));
        assert_eq!(project_cell_centre(0, 0), (PROJECT_X_BIAS, 15));
    }

    #[test]
    fn width_is_the_doubled_column_extent_and_height_is_not_doubled() {
        let cells = diamond(20);
        let (min_col, min_row, max_col, max_row) = projected_bounds(&cells).expect("bounds");
        let image = render_preview(&cells, &[]).expect("image");
        assert_eq!(image.width, ((max_col - min_col) * 2) as u32);
        assert_eq!(image.height, (max_row - min_row) as u32);
    }

    #[test]
    fn each_cell_writes_a_left_and_a_right_pixel() {
        let cells = diamond(20);
        let image = render_preview(&cells, &[]).expect("image");
        // Every written column pair carries the two distinct colours, so a
        // solid-colour run would mean the pair collapsed.
        let mut saw_left = false;
        let mut saw_right = false;
        for y in 0..image.height {
            for x in 0..image.width {
                match image.pixel(x, y) {
                    Some([10, 20, 30]) => saw_left = true,
                    Some([40, 50, 60]) => saw_right = true,
                    _ => {}
                }
            }
        }
        assert!(
            saw_left && saw_right,
            "both halves of the diamond are drawn"
        );
    }

    #[test]
    fn a_black_cell_becomes_grey_rather_than_black() {
        let cells = vec![
            cell(0, 0, [0, 0, 0], [0, 0, 0]),
            cell(20, 0, [1, 1, 1], [1, 1, 1]),
            cell(0, 20, [1, 1, 1], [1, 1, 1]),
            cell(20, 20, [1, 1, 1], [1, 1, 1]),
        ];
        let image = render_preview(&cells, &[]).expect("image");
        assert!(
            image
                .rgba
                .chunks_exact(4)
                .any(|px| px[..3] == BLACK_PIXEL_SUBSTITUTE),
            "the black cell was substituted"
        );
    }

    #[test]
    fn markers_are_baked_in_and_capped_at_eight() {
        let cells = diamond(40);
        let waypoints: Vec<(u16, u16)> = (0..12).map(|i| (10 + i, 10 + i)).collect();
        let image = render_preview(&cells, &waypoints).expect("image");
        let marker_pixels = image
            .rgba
            .chunks_exact(4)
            .filter(|px| px[..3] == MARKER_RGB)
            .count();
        assert!(marker_pixels > 0, "markers are baked into the image");
        assert!(
            marker_pixels <= (MARKER_WAYPOINT_COUNT as usize) * 16,
            "at most eight 4x4 markers, got {marker_pixels} pixels"
        );
    }

    fn header(
        width: u32,
        local_left: u32,
        local_top: u32,
        local_width: u32,
        local_height: u32,
    ) -> crate::map::map_file::MapHeader {
        crate::map::map_file::MapHeader {
            theater: "TEMPERATE".to_string(),
            fill: "Clear".to_string(),
            level: 0,
            width,
            // Keep the authored LocalSize stable under native bottom-margin
            // normalization for the geometry fixtures below.
            height: width,
            local_left,
            local_top,
            local_width,
            local_height,
        }
    }

    #[test]
    fn playfield_bounds_are_asymmetric_exactly_where_the_test_is() {
        // Only the x+y upper bound is inclusive; the other three are strict.
        // Each assertion below sits one cell either side of a boundary.
        let field = Playfield::from_header(&header(20, 2, 5, 10, 8));
        // sum bounds: (20 + 10, 20 + 10 + 16 + 2] = (30, 48]
        assert!(!field.contains(15, 15), "sum 30 is excluded at the low end");
        assert!(field.contains(16, 15), "sum 31 is the first admitted");
        assert!(field.contains(24, 24), "sum 48 is admitted -- inclusive");
        assert!(!field.contains(25, 24), "sum 49 is past the inclusive top");
        // diff bounds: (2*2 - 20, 2*2 - 20 + 20) = (-16, 4)
        let on_sum = |diff: i32| {
            // Pick a cell with sum 40 (inside) and the requested difference.
            let x = (40 + diff) / 2;
            let y = 40 - x;
            (x as u16, y as u16)
        };
        let (x, y) = on_sum(-16);
        assert!(!field.contains(x, y), "diff -16 is excluded, strict");
        let (x, y) = on_sum(-14);
        assert!(field.contains(x, y), "diff -14 is inside");
        let (x, y) = on_sum(2);
        assert!(field.contains(x, y), "diff 2 is inside");
        let (x, y) = on_sum(4);
        assert!(!field.contains(x, y), "diff 4 is excluded, strict");
    }

    #[test]
    fn preview_playfield_normalizes_header_local_size() {
        let field = Playfield::from_header(&crate::map::map_file::MapHeader {
            theater: "TEMPERATE".to_string(),
            fill: "Clear".to_string(),
            level: 0,
            width: 80,
            height: 80,
            local_left: (-5i32) as u32,
            local_top: (-6i32) as u32,
            local_width: 100,
            local_height: 100,
        });

        assert_eq!(
            field.bounds,
            PlayfieldBounds {
                base: 80,
                off_fc: 2,
                off_100: 2,
                off_104: 76,
                off_108: 72,
            }
        );
        assert!(field.contains(43, 42));
        assert!(!field.contains(42, 42));
    }

    #[test]
    fn raising_a_cell_shifts_the_band_rather_than_widening_it() {
        // Elevation moves both x+y bounds by the same amount: a raised cell
        // gains room at the far edge and loses exactly as much at the near
        // one. Widening instead of shifting would quietly enlarge the
        // playfield for every plateau on the map.
        let field = Playfield::from_header(&header(20, 2, 5, 10, 8));
        // sum band at level 0 is (30, 48]; at level 1 it is (31, 49].
        assert!(field.contains_raised(16, 15, 0, 0), "sum 31, flat");
        assert!(
            !field.contains_raised(16, 15, 1, 0),
            "sum 31 lost when raised"
        );
        assert!(!field.contains_raised(25, 24, 0, 0), "sum 49, flat");
        assert!(
            field.contains_raised(25, 24, 1, 0),
            "sum 49 gained when raised"
        );
    }

    #[test]
    fn a_flat_cell_at_level_zero_matches_the_plain_test() {
        // The elevation-aware form must degenerate to the original, or the
        // preview and the generator would disagree about the same map.
        let field = Playfield::from_header(&header(20, 2, 5, 10, 8));
        for x in 0..30u16 {
            for y in 0..30u16 {
                assert_eq!(
                    field.contains(x, y),
                    field.contains_raised(x, y, 0, 0),
                    "({x},{y})"
                );
            }
        }
    }

    #[test]
    fn a_sloped_cell_near_the_near_edge_counts_one_step_higher() {
        // The ramp's upper lip already reads as the next level up, but only
        // close to the near edge — far from it the slope byte changes nothing.
        let field = Playfield::from_header(&header(20, 2, 5, 10, 8));
        // sum 31 is inside when flat, and pushed out by the slope bump.
        assert!(field.contains_raised(16, 15, 0, 0), "flat, admitted");
        assert!(!field.contains_raised(16, 15, 0, 1), "sloped, bumped out");
        // sum 40 is far from the near edge, so the bump does not apply.
        assert_eq!(
            field.contains_raised(20, 20, 0, 0),
            field.contains_raised(20, 20, 0, 1),
            "slope is irrelevant away from the near edge"
        );
        // The sharp case: sum 49 sits one past the far bound. If the bump were
        // applied regardless of position it would drag the far bound out to 49
        // and admit this cell. It must stay excluded — the probe is what keeps
        // the bump local to the near edge.
        assert!(
            !field.contains_raised(25, 24, 0, 1),
            "a sloped cell past the far bound is not rescued by the bump"
        );
    }

    #[test]
    fn elevation_never_moves_the_diff_bounds() {
        // Only x+y shifts. A cell outside on the diff axis stays outside no
        // matter how high it is.
        let field = Playfield::from_header(&header(20, 2, 5, 10, 8));
        // diff 4 is excluded (strict upper bound), sum 40 is well inside.
        let (x, y) = (22u16, 18u16);
        assert_eq!(x as i32 - y as i32, 4, "the fixture really is on diff 4");
        for level in 0..6i8 {
            assert!(
                !field.contains_raised(x, y, level, 0),
                "still out at level {level}"
            );
        }
    }

    #[test]
    fn normalized_predicate_ignores_full_map_height_after_bounds_are_stable() {
        // Full map height participates in LocalSize normalization, but once the
        // authored LocalSize is stable only the full map width enters 0x00578460.
        let short = Playfield::from_header(&header(20, 2, 5, 10, 8));
        let mut tall_header = header(20, 2, 5, 10, 8);
        tall_header.height = 500;
        assert_eq!(Playfield::from_header(&tall_header), short);
    }

    #[test]
    fn the_playfield_shifts_with_the_full_map_width() {
        // Width enters both axes, so widening the map moves the whole diamond.
        let narrow = Playfield::from_header(&header(20, 2, 5, 10, 8));
        let wide = Playfield::from_header(&header(30, 2, 5, 10, 8));
        assert_ne!(narrow, wide);
        // Cell (20,30): sum 50, diff -10. The wider map admits it on both axes;
        // the narrower one's sum window tops out at 48.
        assert!(wide.contains(20, 30), "inside the wider map's playfield");
        assert!(!narrow.contains(20, 30), "outside the narrower one");
    }

    #[test]
    fn only_the_two_bands_get_their_channels_reordered() {
        let rgb = [10, 20, 30];
        // Straight through outside the bands.
        assert_eq!(overlay_radar_channel_order(0x00, rgb), rgb);
        assert_eq!(overlay_radar_channel_order(0x7E, rgb), rgb);
        assert_eq!(overlay_radar_channel_order(0x8B, rgb), rgb);
        assert_eq!(overlay_radar_channel_order(0x92, rgb), rgb);
        assert_eq!(overlay_radar_channel_order(0x9F, rgb), rgb);
        // Green and blue swap inside them, edges included.
        for id in [0x7F, 0x8A, 0x93, 0x9E] {
            assert_eq!(
                overlay_radar_channel_order(id, rgb),
                [10, 30, 20],
                "id {id:#04x} sits in a swapped band"
            );
        }
    }

    /// A palette built by hand, standing in for one recorded off a resolved map.
    fn palette_with(tile: (i32, u8), colours: ([u8; 3], [u8; 3])) -> PreviewPalette {
        let mut palette = PreviewPalette::default();
        palette.tiles.insert(tile, colours);
        palette
    }

    #[test]
    fn an_unknown_tile_falls_back_to_black_so_the_rasteriser_greys_it() {
        let palette = palette_with((7, 0), ([1, 2, 3], [4, 5, 6]));
        assert_eq!(palette.tile_colours(7, 0), ([1, 2, 3], [4, 5, 6]));
        assert_eq!(
            palette.tile_colours(9, 0),
            ([0, 0, 0], [0, 0, 0]),
            "an unseen tile yields black, which becomes grey when drawn"
        );
        assert_eq!(
            substitute_if_black(palette.tile_colours(9, 0).0),
            BLACK_PIXEL_SUBSTITUTE
        );
    }

    #[test]
    fn the_palette_applies_the_channel_order_exactly_once() {
        let mut palette = PreviewPalette::default();
        // Stored raw, as recorded from the overlay lookup.
        palette.overlays.insert((0x80, 3), [10, 20, 30]);
        palette.overlays.insert((0x10, 3), [10, 20, 30]);
        assert_eq!(
            palette.overlay_colour(0x80, 3),
            Some([10, 30, 20]),
            "a swapped-band id reorders on the way out"
        );
        assert_eq!(
            palette.overlay_colour(0x10, 3),
            Some([10, 20, 30]),
            "an ordinary id passes straight through"
        );
    }

    #[test]
    fn a_black_overlay_colour_is_treated_as_absent() {
        let mut palette = PreviewPalette::default();
        palette.overlays.insert((0x10, 0), [0, 0, 0]);
        assert_eq!(
            palette.overlay_colour(0x10, 0),
            None,
            "black means no overlay colour, so the terrain shows through"
        );
        assert_eq!(palette.overlay_colour(0x10, 5), None, "unseen density");
    }

    #[test]
    fn no_cells_renders_nothing() {
        assert_eq!(render_preview(&[], &[]), None);
        // A single cell has no extent, so there is no surface to allocate.
        assert_eq!(
            render_preview(&[cell(3, 3, [1, 2, 3], [4, 5, 6])], &[]),
            None
        );
    }

    #[test]
    fn a_marker_off_the_edge_is_clipped_not_grown() {
        let cells = diamond(20);
        let baseline = render_preview(&cells, &[]).expect("baseline");
        let with_marker = render_preview(&cells, &[(0, 0)]).expect("with marker");
        assert_eq!(
            (baseline.width, baseline.height),
            (with_marker.width, with_marker.height),
            "markers never resize the surface"
        );
    }
}
