//! Visible terrain sprite-instance construction for the base terrain pass.
//!
//! Render-owned (F05): culling, UV lookup, lighting, fog, and bridge-aware
//! instance emission over the immutable `map::terrain` grid. Map keeps
//! parsing and static projection; the app render pass is the only caller.

use crate::map::lighting::CellLightGrid;
use crate::map::terrain::{
    HEIGHT_STEP, TILE_HEIGHT, TILE_WIDTH, TerrainCell, TerrainGrid, TilePlacement, UvLookupFn,
};
use crate::render::batch::SpriteInstance;
use crate::render::draw_state::DrawState;
use crate::render::native_z;

/// Cull margin in screen pixels beyond the viewport on every side.
const CULL_MARGIN: f32 = 120.0;

/// Visible ordinary terrain instances drawn in the base terrain pass.
pub struct TerrainInstances {
    /// Normal terrain — drawn in the base terrain pass, in walk order.
    pub normal: Vec<SpriteInstance>,
    /// Each tile's [`walk_key`], ascending.
    pub walk: Vec<u32>,
}

/// A cell's place in the tile walk (`Render_Tiles_In_Rect @ 0x006D7560`):
/// X+Y ascending, then X ascending. Each tile draws with LessEqual and writes
/// Z, so where overlapping tiles store equal Z the later one shows; and
/// `CellOverlay_TileDraw @ 0x00480350` blits each cell's smudge right after
/// its tile (`0x004804F5`), without Z, so only the tiles walked after it
/// can cover it.
pub fn walk_key(rx: u16, ry: u16) -> u32 {
    ((u32::from(rx) + u32::from(ry)) << 16) | u32::from(rx)
}

fn visible_cell_slice(grid: &TerrainGrid, view_top: f32, view_bottom: f32) -> &[TerrainCell] {
    let start = grid
        .cells
        .partition_point(|cell| cell.screen_y + TILE_HEIGHT < view_top);
    let end = grid
        .cells
        .partition_point(|cell| cell.screen_y <= view_bottom);
    &grid.cells[start..end]
}

/// Generate SpriteInstance data for all tiles visible in the current viewport.
///
/// Single-layer rendering: each cell draws exactly one tile. LAT transition
/// tiles are fully opaque inside the diamond shape (confirmed via diagnostics),
/// so no base clear-ground layer is needed. Missing tiles are skipped —
/// the caller's UV lookup should provide fallbacks if desired.
///
/// The instances go into `buffer`, emptied first: a draw hands back the list
/// of the draw before, so its allocation is reused.
pub fn build_visible_instances(
    grid: &TerrainGrid,
    lighting_grid: Option<&CellLightGrid>,
    camera_x: f32,
    camera_y: f32,
    screen_width: f32,
    screen_height: f32,
    uv_fn: UvLookupFn<'_>,
    live_terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    mut buffer: Vec<SpriteInstance>,
) -> TerrainInstances {
    let view_left: f32 = camera_x - CULL_MARGIN;
    let view_right: f32 = camera_x + screen_width + CULL_MARGIN;
    let view_top: f32 = camera_y - CULL_MARGIN;
    let view_bottom: f32 = camera_y + screen_height + CULL_MARGIN;

    buffer.clear();
    let mut instances = TerrainInstances {
        normal: buffer,
        walk: Vec::new(),
    };

    for cell in visible_cell_slice(grid, view_top, view_bottom) {
        // AABB visibility test against viewport.
        let right: f32 = cell.screen_x + TILE_WIDTH;
        let bottom: f32 = cell.screen_y + TILE_HEIGHT;

        if right < view_left || cell.screen_x > view_right {
            continue;
        }
        if bottom < view_top || cell.screen_y > view_bottom {
            continue;
        }

        // No shroud gate: the native tile walk draws every in-bounds cell
        // regardless of explored state (`CellOverlay_TileDraw @ 0x00480350`
        // has no shroud test), and the flat ABuffer curtain blacks out
        // unexplored ground. Culling here left undrawn holes the curtain's
        // feather could not darken — hard south-facing shroud edges wherever
        // the feather extended past the last drawn tile.

        // Native480350 draws the cell's current tile at its current level.
        // Bridge publication (56EB80 floods and the ramp helpers under
        // 576BA0/571490) and cliff collapse rewrite tiles and levels after
        // load, so the live terrain wins over the load-time grid. The file
        // choice checks the current TMP family (resident file count before
        // the pristine damaged-data gate), then the process-owned coordinate
        // selector. LAT may have replaced the source family after its cached
        // Recalc variant was chosen. A
        // live tile missing from the atlas keeps the grid tile and level.
        // Culling still uses the grid position; CULL_MARGIN covers 8 levels.
        let place = |tile_id: u16, sub_tile: u8, variant: u8| match &uv_fn {
            Some(f) => f(tile_id, sub_tile, variant),
            None => Some(TilePlacement {
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                pixel_size: [TILE_WIDTH, TILE_HEIGHT],
                draw_offset: [0.0, 0.0],
            }),
        };
        let live = live_terrain.and_then(|terrain| {
            let live = terrain.cell(cell.rx, cell.ry)?;
            let (tile_id, sub_tile) = terrain.presentation_tile(live);
            let variant = terrain
                .tactical_draw_variant(cell.rx, cell.ry)
                .unwrap_or(live.variant);
            Some((place(tile_id, sub_tile, variant)?, live.level))
        });
        let Some((p, level)) =
            live.or_else(|| Some((place(cell.tile_id, cell.sub_tile, cell.variant)?, cell.z)))
        else {
            continue;
        };
        let level = i32::from(level as i8);

        // Sort depth: the elevation-free iso row of the diamond top. The
        // zdepth pipeline ignores it and derives each pixel's depth from the
        // native tile Z (`TMP_TileBlitter @ 0x00547CF0`):
        //   base = DefaultZ + YOrigin - diamond_top - tileH - tileH*level/2,
        //   pixel = base + zdata, drawn and stored when `pixel <= zbuf`.
        // `z_adjust` is that base relative to the instance's canvas top
        // (extra-data tiles start above the diamond by `draw_offset.y`).
        let iso_row: f32 = cell.screen_y + f32::from(cell.z as i8) * HEIGHT_STEP;
        let screen_y = iso_row - level as f32 * HEIGHT_STEP;
        let depth: f32 = native_z::depth_for_row(iso_row, grid.origin_y, grid.world_height);
        let z_adjust = p.draw_offset[1]
            - (native_z::TILE_HEIGHT_ROWS as f32)
            - ((native_z::TILE_HEIGHT_ROWS * level) / 2) as f32;

        let tint = lighting_grid
            .map(|lights| lights.terrain_tile_tint_at((cell.rx, cell.ry)))
            .unwrap_or(cell.tint);
        let draw_state = DrawState::default();
        let inst = SpriteInstance {
            position: [
                cell.screen_x + p.draw_offset[0],
                screen_y + p.draw_offset[1],
            ],
            size: p.pixel_size,
            uv_origin: p.uv_origin,
            uv_size: p.uv_size,
            depth,
            tint,
            palette_light: lighting_grid
                .map(|lights| {
                    crate::render::palette_light::PaletteLight::cell(
                        lights,
                        (cell.rx, cell.ry),
                        false,
                    )
                })
                .unwrap_or_default(),
            alpha: 1.0,
            draw_state,
            z_adjust,
            ..Default::default()
        };
        instances.normal.push(inst);
        instances.walk.push(walk_key(cell.rx, cell.ry));
    }
    // The grid is sorted by lifted screen row for the cull above; the walk
    // orders by cell.
    if !instances.walk.is_sorted() {
        let mut walked: Vec<(u32, SpriteInstance)> = instances
            .walk
            .drain(..)
            .zip(instances.normal.drain(..))
            .collect();
        walked.sort_unstable_by_key(|&(key, _)| key);
        for (key, instance) in walked {
            instances.walk.push(key);
            instances.normal.push(instance);
        }
    }

    instances
}

#[cfg(all(test, tests_render_s_z))]
#[path = "bridge_pavement_caller_tests.rs"]
mod pavement_caller_tests;

#[cfg(all(test, tests_render_s_z))]
mod tests {
    use super::*;

    /// A draw hands back the list of the draw before; none of it may reach
    /// this draw's tiles.
    #[test]
    fn a_handed_back_buffer_holds_only_this_draws_tiles() {
        let grid = TerrainGrid {
            cells: vec![TerrainCell {
                screen_x: 0.0,
                screen_y: 0.0,
                tile_id: 0,
                sub_tile: 0,
                z: 0,
                rx: 1,
                ry: 1,
                variant: 0,
                tint: [1.0; 3],
                radar_left: [0; 3],
                radar_right: [0; 3],
            }],
            world_width: TILE_WIDTH,
            world_height: TILE_HEIGHT,
            origin_x: 0.0,
            origin_y: 0.0,
            bridge_middle_tiles: None,
            deformation_tiles: Vec::new(),
        };
        let draw = |buffer| {
            build_visible_instances(&grid, None, 0.0, 0.0, 800.0, 600.0, None, None, buffer).normal
        };
        let fresh = draw(Vec::new());
        assert_eq!(fresh.len(), 1);
        let stale = vec![SpriteInstance::default(); 3];
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&draw(stale)),
            bytemuck::cast_slice::<_, u8>(&fresh)
        );
    }

    /// The grid is sorted by lifted screen row for culling; the tiles draw
    /// in the walk's order (0x006D7560), so a raised cell lifted above a
    /// lower one walked before it still draws after it.
    #[test]
    fn tiles_draw_in_the_tile_walk_order() {
        let cell = |rx: u16, ry: u16, z: u8| {
            let (screen_x, screen_y) = crate::map::terrain::iso_to_screen(rx, ry, z);
            TerrainCell {
                screen_x,
                screen_y,
                tile_id: 0,
                sub_tile: 0,
                z,
                rx,
                ry,
                variant: 0,
                tint: [1.0; 3],
                radar_left: [0; 3],
                radar_right: [0; 3],
            }
        };
        // Lifted rows 30, 60 and 90: the raised (5,4) sorts before (3,2).
        let cells = vec![cell(1, 0, 0), cell(5, 4, 6), cell(3, 2, 0)];
        assert!(cells.is_sorted_by(|a, b| a.screen_y <= b.screen_y));
        let grid = TerrainGrid {
            cells,
            world_width: 600.0,
            world_height: 600.0,
            origin_x: -300.0,
            origin_y: 0.0,
            bridge_middle_tiles: None,
            deformation_tiles: Vec::new(),
        };
        let tiles = build_visible_instances(
            &grid,
            None,
            -300.0,
            0.0,
            800.0,
            600.0,
            None,
            None,
            Vec::new(),
        );
        assert_eq!(tiles.walk, [walk_key(1, 0), walk_key(3, 2), walk_key(5, 4)]);
        let rows: Vec<f32> = tiles.normal.iter().map(|tile| tile.position[1]).collect();
        assert_eq!(rows, [30.0, 90.0, 60.0]);
    }
}
