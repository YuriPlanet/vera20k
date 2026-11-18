//! Static decal rendering for the SmudgeGrid.
//!
//! Reads the per-cell SmudgeGrid + SmudgeTypeRegistry and produces SpriteInstance
//! buffers for the active smudges. Each draws inside the terrain pass right
//! after its cell's tile, so the tiles walked after it can cover it, and
//! under everything drawn after the tiles.
//!
//! Smudges are static — no animation, no remap, no facing. Multi-cell
//! SmudgeType SHPs have a single composite frame. Native asks every occupied
//! footprint cell to draw frame 0. On the selected coherent flat footprint with
//! uniform Cell material, each call recenters on the same origin and repeated
//! opaque draws are pixel-idempotent. This builder emits the origin once for
//! positive footprints whose cell-data byte does not wrap. Per-cell material
//! differences and partially cleared footprints remain open on that optimized
//! path. Wrapped and nonpositive footprints retain every source-cell draw.
//!
//! ## Dependency rules
//! - Part of render/ — depends on map/, rules/, sim/.
//! - Reads sim smudge state through immutable references; never mutates sim state.

use crate::map::lighting::CellLightGrid;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::terrain::{TILE_WIDTH, TilePlacement, iso_to_screen};
use crate::render::batch::SpriteInstance;
use crate::render::cell_window::{AnchorBounds, ArtExtent, CellWindow};
use crate::render::palette_light::PaletteLight;
use crate::render::terrain_instances::walk_key;
use crate::rules::smudge_type::{SmudgeTypeDef, SmudgeTypeRegistry};
use crate::sim::smudge_grid::{SmudgeCell, SmudgeGrid};

#[cfg(all(test, tests_render_s_z))]
#[path = "smudge_gpu_tests.rs"]
mod gpu_tests;

/// Type alias for clarity at call sites.
///
/// Smudges share `SpriteInstance` with every other static decal in the engine —
/// carrying the footprint position, atlas rectangle and current Cell material.
pub type SmudgeInstance = SpriteInstance;

/// In-range sprite ABI depth; the ordinary smudge passthrough pipeline reads
/// and writes no Z (native flags E00). Submission after terrain and before
/// entities establishes the layer, independently of this value.
const SMUDGE_DEPTH: f32 = 0.5;

fn can_collapse_footprint(definition: &SmudgeTypeDef) -> bool {
    definition.width > 0
        && definition.height > 0
        && i64::from(definition.width) * i64::from(definition.height) <= 256
}

/// SmudgeType.Draw_It6B5621..5661: data0 bypasses IDIV, while every other
/// byte recenters using the current signed Width. Height never enters the
/// native draw. Keep the original source cell's level and material separate.
fn decoded_pixel_offset(definition: &SmudgeTypeDef, data: u8) -> Option<[i32; 2]> {
    let (x, y) = if data == 0 {
        (0, 0)
    } else {
        definition.encoded_cell_offset(data)?
    };
    Some([30 * (y - x), -15 * (y + x)])
}

fn recentered_art_extent(registry: &SmudgeTypeRegistry, art: ArtExtent) -> ArtExtent {
    let mut offsets = ArtExtent::default();
    for (_, definition) in registry.iter_with_id() {
        if can_collapse_footprint(definition) {
            continue;
        }
        // Cell data has only 256 states, independently of the signed type's
        // dimensions. Use the same draw decision for these cull bounds; stock
        // unwrapped types need no extra padding or per-cell decode.
        for data in 1..=u8::MAX {
            let Some([x, y]) = decoded_pixel_offset(definition, data) else {
                continue;
            };
            offsets.left = offsets.left.min(x as f32);
            offsets.right = offsets.right.max(x as f32);
            offsets.top = offsets.top.min(y as f32);
            offsets.bottom = offsets.bottom.max(y as f32);
        }
    }
    ArtExtent {
        left: art.left + offsets.left,
        top: art.top + offsets.top,
        right: art.right + offsets.right,
        bottom: art.bottom + offsets.bottom,
    }
}

/// Build a `SpriteInstance` buffer for visible smudges.
///
/// Cell4804B4→Smudge6B55F0 passes the Cell center X, unchanged tile Y, and
/// signed common intensity (+10C) with its CellConvert (+34). Original
/// full-view/type/SHP execution: `tools/spatial_oracle/smudge_render`.
///
/// `atlas_lookup` resolves a (smudge_type_id, SHP frame) to a TilePlacement
/// (atlas UVs + pixel size + draw offset). The closure is borrowed because the
/// smudge atlas lives on the renderer side and uses interior types we don't
/// want to expose here. `art` bounds every placement it returns; only the
/// cells whose recentered art can then reach the view are visited. They come
/// out in the tile walk's order (no Z orders smudges), each to be drawn after
/// the tiles [`VisibleSmudges::cuts`] counts. The SHP frame is always zero;
/// the stored cell-data byte controls only position.
pub fn build_visible_instances(
    grid: &SmudgeGrid,
    registry: &SmudgeTypeRegistry,
    resolved_terrain: &ResolvedTerrainGrid,
    cell_lights: &CellLightGrid,
    atlas_lookup: &dyn Fn(u16, u8) -> Option<TilePlacement>,
    art: ArtExtent,
    camera_x: f32,
    camera_y: f32,
    screen_w: f32,
    screen_h: f32,
) -> VisibleSmudges {
    let build = |window: CellWindow| {
        visible_instances_in(
            window,
            grid,
            registry,
            resolved_terrain,
            cell_lights,
            atlas_lookup,
            [camera_x, camera_y, screen_w, screen_h],
        )
    };
    // The cull below admits art drawn half a tile right of the source anchor,
    // including every byte-decoded shift that can bring it into the view.
    let art = recentered_art_extent(registry, art);
    let window = CellWindow::covering(
        AnchorBounds {
            left: camera_x - TILE_WIDTH / 2.0 - art.right,
            top: camera_y - art.bottom,
            right: camera_x + screen_w - TILE_WIDTH / 2.0 - art.left,
            bottom: camera_y + screen_h - art.top,
        },
        resolved_terrain.level_span(),
    );
    let smudges = build(window);
    if cfg!(debug_assertions) && window != CellWindow::Whole {
        let whole = build(CellWindow::Whole);
        assert!(
            bytemuck::cast_slice::<_, u8>(&smudges.instances)
                == bytemuck::cast_slice::<_, u8>(&whole.instances)
                && smudges.walk == whole.walk,
            "the smudge window changed the smudge instances"
        );
    }
    smudges
}

/// The visible smudges in the tile walk's order, each with the
/// [`walk_key`] of the cell whose tile it draws after.
#[derive(Default)]
pub struct VisibleSmudges {
    pub instances: Vec<SmudgeInstance>,
    /// Ascending.
    pub walk: Vec<u32>,
}

impl VisibleSmudges {
    /// For each smudge, how many of the walk-ordered tiles `tile_walk`
    /// draw before it: those of its cell and the cells walked earlier.
    pub fn cuts(&self, tile_walk: &[u32]) -> Vec<u32> {
        self.walk
            .iter()
            .map(|&key| tile_walk.partition_point(|&tile| tile <= key) as u32)
            .collect()
    }
}

/// The cell after whose tile a smudge drawn from `(rx, ry)` with data
/// `data` stays on screen. Each footprint cell holding it redraws the whole
/// art at the source anchor right after its own tile (`0x004804F5`), so on
/// the collapsed route the walk-last of them decides what later tiles cover.
fn last_redraw_key(
    grid: &SmudgeGrid,
    definition: &SmudgeTypeDef,
    type_id: u16,
    (rx, ry): (u16, u16),
    collapsed: bool,
) -> u32 {
    if !collapsed {
        return walk_key(rx, ry);
    }
    let mut last = walk_key(rx, ry);
    for y in 0..definition.height {
        for x in 0..definition.width {
            let (Ok(dx), Ok(dy), Ok(data)) = (
                u16::try_from(x),
                u16::try_from(y),
                u8::try_from(y * definition.width + x),
            ) else {
                continue;
            };
            let (Some(cx), Some(cy)) = (rx.checked_add(dx), ry.checked_add(dy)) else {
                continue;
            };
            let cell = grid.cell(cx, cy);
            if cell.type_id == Some(type_id) && cell.frame_offset == data {
                last = last.max(walk_key(cx, cy));
            }
        }
    }
    last
}

fn visible_instances_in(
    window: CellWindow,
    grid: &SmudgeGrid,
    registry: &SmudgeTypeRegistry,
    resolved_terrain: &ResolvedTerrainGrid,
    cell_lights: &CellLightGrid,
    atlas_lookup: &dyn Fn(u16, u8) -> Option<TilePlacement>,
    [camera_x, camera_y, screen_w, screen_h]: [f32; 4],
) -> VisibleSmudges {
    let mut instances: Vec<(u32, SmudgeInstance)> = Vec::with_capacity(64);
    let view_right: f32 = camera_x + screen_w;
    let view_bottom: f32 = camera_y + screen_h;
    // The whole window is exactly `iter_occupied`.
    let whole = (window == CellWindow::Whole).then(|| grid.iter_occupied());
    let banded = (window != CellWindow::Whole).then(|| {
        window
            .cells(grid.width(), grid.height())
            .map(|(rx, ry)| (rx, ry, grid.cell(rx, ry)))
            .filter(|(_, _, cell): &(u16, u16, &SmudgeCell)| cell.type_id.is_some())
    });
    let occupied = whole
        .into_iter()
        .flatten()
        .chain(banded.into_iter().flatten());

    for (rx, ry, cell) in occupied {
        let Some(type_id) = cell.type_id else {
            continue;
        };
        // Confirm the type still exists in the registry — defensive against
        // map/rules mismatches; an unknown id is silently skipped.
        let Some(definition) = registry.get(type_id) else {
            continue;
        };
        // Keep the established coherent, flat, uniform-material optimization
        // only when data has not wrapped. In wrapped footprints, a decoded
        // anchor need not have any data0 source. Unequal material and partial
        // clears on the collapsed path remain outside its proven coverage.
        let collapsed = can_collapse_footprint(definition);
        if collapsed && cell.frame_offset != 0 {
            continue;
        }
        let Some(terrain_cell) = resolved_terrain.cell(rx, ry) else {
            continue;
        };
        let (sx, sy): (f32, f32) = iso_to_screen(rx, ry, terrain_cell.level);
        let placement: TilePlacement = match atlas_lookup(type_id, 0) {
            Some(p) => p,
            None => continue,
        };
        // The native art-null test precedes IDIV. Refuse an invalid retained
        // nonzero-data/Width0 mark here, without clamping the type at load.
        let Some([offset_x, offset_y]) = decoded_pixel_offset(definition, cell.frame_offset) else {
            continue;
        };
        // iso_to_screen returns the NW corner of the cell's bounding box;
        // native caller adds half a tile only in X. Adding half-height in Y
        // here places the complete crater 15px too low. The atlas retains
        // the full-canvas centered anchor, as the native shape caller does.
        let draw_x = sx + TILE_WIDTH / 2.0 + offset_x as f32 + placement.draw_offset[0];
        let draw_y = sy + offset_y as f32 + placement.draw_offset[1];
        // Cull against the complete composite SHP bounds. Testing the origin
        // cell's diamond would hide a wide footprint while part of its art is
        // still inside the tactical viewport.
        if draw_x >= view_right || draw_x + placement.pixel_size[0] <= camera_x {
            continue;
        }
        if draw_y >= view_bottom || draw_y + placement.pixel_size[1] <= camera_y {
            continue;
        }
        let walk = last_redraw_key(grid, definition, type_id, (rx, ry), collapsed);
        instances.push((
            walk,
            SmudgeInstance {
                position: [draw_x, draw_y],
                size: placement.pixel_size,
                uv_origin: placement.uv_origin,
                uv_size: placement.uv_size,
                depth: SMUDGE_DEPTH,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                palette_light: PaletteLight::cell(cell_lights, (rx, ry), false),
                ..Default::default()
            },
        ));
    }
    // Each cell holds one smudge, so the keys are unique.
    instances.sort_unstable_by_key(|&(key, _)| key);
    let (walk, instances) = instances.into_iter().unzip();
    VisibleSmudges { instances, walk }
}

#[cfg(all(test, tests_render_s_z))]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    /// Overlapping smudges in adjacent rows draw in the tile walk's order
    /// (X+Y, then X, 0x006D7560), each after its own cell's tile, also when
    /// one reaches the view only through its art's width from columns left
    /// of it.
    #[test]
    fn edge_smudges_in_adjacent_rows_follow_the_tile_walk() {
        let registry =
            SmudgeTypeRegistry::from_rules_ini(&IniFile::from_str("[SmudgeTypes]\n0=CR1\n"));
        let terrain = crate::map::resolved_terrain::test_grid(
            30,
            30,
            crate::map::resolved_terrain::test_smudge_cell,
        );
        let mut grid = SmudgeGrid::new(30, 30);
        for (rx, ry) in [(4, 11), (8, 10)] {
            grid.test_force_set(
                rx,
                ry,
                SmudgeCell {
                    type_id: Some(0),
                    frame_offset: 0,
                },
            );
        }
        let placement = TilePlacement {
            uv_origin: [0.0; 2],
            uv_size: [1.0; 2],
            pixel_size: [300.0, 60.0],
            draw_offset: [-60.0, -45.0],
        };
        let art = ArtExtent {
            left: -60.0,
            top: -45.0,
            right: 240.0,
            bottom: 15.0,
        };
        let smudges = build_visible_instances(
            &grid,
            &registry,
            &terrain,
            &CellLightGrid::new(),
            &|_, _| Some(placement),
            art,
            0.0,
            0.0,
            600.0,
            400.0,
        );
        assert_eq!(
            smudges
                .instances
                .iter()
                .map(|instance| instance.position)
                .collect::<Vec<_>>(),
            [[-270.0, 195.0], [-120.0, 240.0]]
        );
        assert_eq!(smudges.walk, [walk_key(4, 11), walk_key(8, 10)]);
        // Tiles walked before (4,11), through (4,11), up to (8,10).
        let tiles = [
            walk_key(3, 11),
            walk_key(4, 11),
            walk_key(5, 11),
            walk_key(8, 10),
        ];
        assert_eq!(smudges.cuts(&tiles), [2, 4]);
    }

    /// Every footprint cell redraws the whole art right after its own tile,
    /// so the collapsed draw follows the walk-last cell still holding it.
    #[test]
    fn a_collapsed_footprint_draws_after_its_walk_last_cell() {
        let registry = SmudgeTypeRegistry::from_rules_ini(&IniFile::from_str(
            "[SmudgeTypes]\n0=CR1\n[CR1]\nWidth=2\nHeight=2\n",
        ));
        let terrain = crate::map::resolved_terrain::test_grid(
            30,
            30,
            crate::map::resolved_terrain::test_smudge_cell,
        );
        let mut grid = SmudgeGrid::new(30, 30);
        for (data, (rx, ry)) in [(10, 10), (11, 10), (10, 11), (11, 11)]
            .into_iter()
            .enumerate()
        {
            grid.test_force_set(
                rx,
                ry,
                SmudgeCell {
                    type_id: Some(0),
                    frame_offset: data as u8,
                },
            );
        }
        let placement = TilePlacement {
            uv_origin: [0.0; 2],
            uv_size: [1.0; 2],
            pixel_size: [120.0, 60.0],
            draw_offset: [-60.0, -30.0],
        };
        let art = ArtExtent {
            left: -60.0,
            top: -30.0,
            right: 60.0,
            bottom: 30.0,
        };
        let build = |grid: &SmudgeGrid| {
            build_visible_instances(
                grid,
                &registry,
                &terrain,
                &CellLightGrid::new(),
                &|_, _| Some(placement),
                art,
                -400.0,
                0.0,
                800.0,
                600.0,
            )
        };
        let smudges = build(&grid);
        assert_eq!(smudges.instances.len(), 1);
        assert_eq!(smudges.walk, [walk_key(11, 11)]);
        // A cleared last cell hands the redraw to the next walk-last one.
        grid.test_force_set(11, 11, SmudgeCell::default());
        assert_eq!(build(&grid).walk, [walk_key(11, 10)]);
    }
}
