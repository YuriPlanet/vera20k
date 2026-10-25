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

type PreviewColorPair = ([u8; 3], [u8; 3]);

/// Original TMP549E90's13-row color table, derived from selected TMP bytes.
/// This preview-local cache keys the actual resolved RGB pair, including the
/// selected independent file. It is discarded with the preview; there is no
/// mutable asset/Cell authority or invalidation state here.
#[derive(Default)]
struct PreviewPalette {
    tiles: std::collections::HashMap<PreviewColorPair, [PreviewColorPair; 13]>,
}

impl PreviewPalette {
    fn tile_colors(&mut self, pair: PreviewColorPair, level: u8) -> PreviewColorPair {
        use crate::util::native_color::{interpolate_rgb8, scale_rgb8};
        use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53};
        assert!(
            level <= 12,
            "RMG preview level{level} exceeds the native TMP color table"
        );
        let table = self.tiles.entry(pair).or_insert_with(|| {
            // Original549F72/549F8E load float1.4;549FB7 multiplies the
            // integer level by stored double1/12, then narrows to float.
            // In particular level12 is3f7fffff, not the bright endpoint.
            let brightness = NativeF32Bits::from_bits(0x3fb3_3333);
            let bright_left = scale_rgb8(pair.0, brightness).expect("finite TMP RGB scale");
            let bright_right = scale_rgb8(pair.1, brightness).expect("finite TMP RGB scale");
            std::array::from_fn(|level| {
                let fraction = X87Chop53::store_f32(X87Chop53::mul(
                    X87Chop53::load_i32(level as i32),
                    X87Chop53::load_f64(NativeF64Bits::from_bits(0x3fb5_5555_5555_5555))
                        .expect("finite TMP level multiplier"),
                ))
                .expect("finite TMP level fraction");
                (
                    interpolate_rgb8(pair.0, bright_left, fraction).expect("finite TMP RGB blend"),
                    interpolate_rgb8(pair.1, bright_right, fraction).expect("finite TMP RGB blend"),
                )
            })
        });
        table[usize::from(level)]
    }
}

/// Presentation callbacks project an Overlay or ground-head Terrain color
/// over the current TMP pair. They own device-format packing; None preserves
/// the TMP pair. An Overlay suppresses the Terrain branch even on a color miss.
pub fn preview_cells_from_map(
    map: &crate::map::map_file::MapFile,
    resolved: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    overlay_radar: &dyn Fn(u8, u8, PreviewColorPair) -> Option<PreviewColorPair>,
    terrain_radar: &dyn Fn(u16, u16, PreviewColorPair) -> Option<PreviewColorPair>,
) -> Vec<PreviewCell> {
    let playfield = PlayfieldBounds::from_map_header(&map.header);
    let overlays = overlay_densities(map);
    let mut palette = PreviewPalette::default();
    let mut cells = Vec::with_capacity(map.cells.len());
    for cell in &map.cells {
        // The flat test: with elevation, cells would drop in and out of the
        // preview as terrain rises. Generation uses the height-aware form.
        if !playfield.contains_geometry_packed(i32::from(cell.rx), i32::from(cell.ry)) {
            continue;
        }
        let terrain = resolved
            .cell(cell.rx, cell.ry)
            .map_or(([0; 3], [0; 3]), |resolved| {
                palette.tile_colors((resolved.radar_left, resolved.radar_right), resolved.level)
            });
        // GenerateTerrainPreview6413AA..6413C3 reaches the Terrain head
        // branch6415E1 only for Overlay==-1, not when an Overlay color misses.
        let (left, right) = match overlays.get(&(cell.rx, cell.ry)) {
            Some((id, density)) => overlay_radar(*id, *density, terrain),
            None => terrain_radar(cell.rx, cell.ry, terrain),
        }
        .unwrap_or(terrain);
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
        let (left, right) = substitute_black_pair((cell.left, cell.right));
        image.put_pixel(column, row, left);
        image.put_pixel(column + 1, row, right);
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

/// GenerateTerrainPreview641140 at64168B..6416EF replaces both packed
/// halves when either is zero. App device conversion precedes this owner.
const fn substitute_black_pair(pair: PreviewColorPair) -> PreviewColorPair {
    if (pair.0[0] == 0 && pair.0[1] == 0 && pair.0[2] == 0)
        || (pair.1[0] == 0 && pair.1[1] == 0 && pair.1[2] == 0)
    {
        (BLACK_PIXEL_SUBSTITUTE, BLACK_PIXEL_SUBSTITUTE)
    } else {
        pair
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

    /// The pixel at `(x, y)` as RGB, for tests that inspect output.
    #[cfg(test)]
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
    fn overlay_color_miss_still_suppresses_terrain_callback() {
        let mut map = super::super::emit::empty_map_file(&Default::default(), 20, 20);
        map.cells.push(crate::map::map_file::MapCell {
            rx: 20,
            ry: 20,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        });
        map.overlays.push(crate::map::overlay::OverlayEntry {
            rx: 20,
            ry: 20,
            overlay_id: 3,
            frame: 7,
        });
        let resolved = crate::map::resolved_terrain::test_grid(32, 32, |rx, ry| {
            let mut terrain = crate::map::resolved_terrain::test_flat_cell(rx, ry);
            terrain.radar_left = [10, 20, 30];
            terrain.radar_right = [40, 50, 60];
            terrain
        });
        let overlay_calls = std::cell::Cell::new(0);
        let cells = preview_cells_from_map(
            &map,
            &resolved,
            &|id, density, _| {
                assert_eq!((id, density), (3, 7));
                overlay_calls.set(overlay_calls.get() + 1);
                None
            },
            &|_, _, _| panic!("6413AA: an existing Overlay suppresses Terrain"),
        );
        assert_eq!(overlay_calls.get(), 1);
        assert_eq!(cells, [cell(20, 20, [10, 20, 30], [40, 50, 60])]);
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
    fn a_black_cell_becomes_grey_rather_than_black() {
        let cells = vec![
            cell(0, 0, [0, 0, 0], [10, 20, 30]),
            cell(20, 0, [1, 1, 1], [1, 1, 1]),
            cell(0, 20, [1, 1, 1], [1, 1, 1]),
            cell(20, 20, [1, 1, 1], [1, 1, 1]),
        ];
        let image = render_preview(&cells, &[]).expect("image");
        assert_eq!(
            image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|px| px[..3] == BLACK_PIXEL_SUBSTITUTE)
                .count(),
            2,
            "64168B..6416EF substitutes both halves when either is black"
        );
    }

    #[test]
    fn markers_are_baked_in_and_capped_at_eight() {
        let cells = diamond(40);
        let waypoints: Vec<(u16, u16)> = (0..12).map(|i| (10 + i, 10 + i)).collect();
        let image = render_preview(&cells, &waypoints).expect("image");
        let marker_pixels = image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[..3] == MARKER_RGB)
            .count();
        assert!(marker_pixels > 0, "markers are baked into the image");
        assert!(
            marker_pixels <= (MARKER_WAYPOINT_COUNT as usize) * 16,
            "at most eight 4x4 markers, got {marker_pixels} pixels"
        );
    }

    #[test]
    fn physical_clear_level_table_matches_original_producer_rgb() {
        let native = crate::test_fixture::json("tools/spatial_oracle/tmp_preview.json");
        let raw = &native["inputs"]["selected_tmp"]["raw_pair_rgb"];
        let rgb =
            |value: &serde_json::Value| std::array::from_fn(|i| value[i].as_u64().unwrap() as u8);
        let pair = (rgb(&raw[0]), rgb(&raw[1]));
        let mut palette = PreviewPalette::default();
        for row in native["rows"].as_array().unwrap() {
            let level = row["input"]["level"].as_u64().unwrap() as u8;
            let (left, right) = palette.tile_colors(pair, level);
            assert_eq!(
                serde_json::json!([left, right]),
                row["output"]["preview_rgb_pair"],
                "original549E90 level{level}"
            );
        }
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
