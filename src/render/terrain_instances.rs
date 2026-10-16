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
    /// Normal terrain — drawn in the base terrain pass.
    pub normal: Vec<SpriteInstance>,
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
pub fn build_visible_instances(
    grid: &TerrainGrid,
    lighting_grid: Option<&CellLightGrid>,
    camera_x: f32,
    camera_y: f32,
    screen_width: f32,
    screen_height: f32,
    uv_fn: UvLookupFn<'_>,
    live_terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> TerrainInstances {
    let view_left: f32 = camera_x - CULL_MARGIN;
    let view_right: f32 = camera_x + screen_width + CULL_MARGIN;
    let view_top: f32 = camera_y - CULL_MARGIN;
    let view_bottom: f32 = camera_y + screen_height + CULL_MARGIN;

    let mut instances = TerrainInstances {
        normal: Vec::with_capacity(grid.cells.len() / 2),
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
        // choice checks current pavement flags (resident file count before
        // the pristine damaged-data gate), else the live Recalc variant. A
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
                .pavement_draw_variant(cell.rx, cell.ry)
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
    }

    instances
}

#[cfg(test)]
#[path = "bridge_pavement_caller_tests.rs"]
mod pavement_caller_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gsi_04_03c_terrain_instances_have_one_ordinary_bucket() {
        // Create a small grid manually.
        let grid: TerrainGrid = TerrainGrid {
            cells: vec![
                TerrainCell {
                    screen_x: 0.0,
                    screen_y: 0.0,
                    tile_id: 0,
                    sub_tile: 0,
                    z: 0,
                    rx: 1,
                    ry: 0,
                    is_water: false,
                    variant: 0,
                    tint: [1.0, 1.0, 1.0],
                    radar_left: [0, 0, 0],
                    radar_right: [0, 0, 0],
                    has_damaged_data: false,
                },
                TerrainCell {
                    screen_x: 5000.0,
                    screen_y: 5000.0,
                    tile_id: 0,
                    sub_tile: 0,
                    z: 0,
                    rx: 100,
                    ry: 100,
                    is_water: false,
                    variant: 0,
                    tint: [1.0, 1.0, 1.0],
                    radar_left: [0, 0, 0],
                    radar_right: [0, 0, 0],
                    has_damaged_data: false,
                },
            ],
            world_width: 5060.0,
            world_height: 5030.0,
            origin_x: 0.0,
            origin_y: 0.0,
            local_bounds: None,
            bridge_middle_tiles: None,
        };

        // Camera at origin, 1024x768 viewport — only first cell should be visible.
        let result: TerrainInstances =
            build_visible_instances(&grid, None, 0.0, 0.0, 1024.0, 768.0, None, None);
        assert_eq!(result.normal.len(), 1);
    }

    #[test]
    fn terrain_tile_instances_consume_per_cell_lighting() {
        let grid: TerrainGrid = TerrainGrid {
            cells: vec![
                TerrainCell {
                    screen_x: 0.0,
                    screen_y: 0.0,
                    tile_id: 0,
                    sub_tile: 0,
                    z: 0,
                    rx: 1,
                    ry: 0,
                    is_water: false,
                    variant: 0,
                    tint: [1.0, 1.0, 1.0],
                    radar_left: [0, 0, 0],
                    radar_right: [0, 0, 0],
                    has_damaged_data: false,
                },
                TerrainCell {
                    screen_x: 60.0,
                    screen_y: 0.0,
                    tile_id: 0,
                    sub_tile: 0,
                    z: 4,
                    rx: 2,
                    ry: 0,
                    is_water: false,
                    variant: 0,
                    tint: [1.0, 1.0, 1.0],
                    radar_left: [0, 0, 0],
                    radar_right: [0, 0, 0],
                    has_damaged_data: false,
                },
            ],
            world_width: 120.0,
            world_height: 30.0,
            origin_x: 0.0,
            origin_y: 0.0,
            local_bounds: None,
            bridge_middle_tiles: None,
        };
        let lights = crate::map::lighting::build_cell_light_grid_from_heights(
            [((1, 0), 0), ((2, 0), 4)],
            &crate::map::lighting::LightingConfig::default(),
        );

        let result =
            build_visible_instances(&grid, Some(&lights), 0.0, 0.0, 1024.0, 768.0, None, None);

        assert_eq!(result.normal.len(), 2);
        assert!((result.normal[0].tint[0] - 0.95).abs() < 0.001);
        assert!((result.normal[1].tint[0] - 0.982).abs() < 0.001);
    }

    fn live_tile_grid() -> TerrainGrid {
        TerrainGrid {
            cells: vec![TerrainCell {
                screen_x: 0.0,
                screen_y: 0.0,
                tile_id: 100,
                sub_tile: 3,
                z: 4,
                rx: 0,
                ry: 0,
                is_water: false,
                variant: 2,
                tint: [1.0; 3],
                radar_left: [0; 3],
                radar_right: [0; 3],
                has_damaged_data: false,
            }],
            world_width: TILE_WIDTH,
            world_height: TILE_HEIGHT,
            origin_x: 0.0,
            origin_y: 0.0,
            local_bounds: None,
            bridge_middle_tiles: None,
        }
    }

    type LiveTileDraw = (Vec<(u16, u8, u8)>, Vec<(f32, f32)>);

    /// Draws the level-4 grid cell against a live cell holding `live` (tile,
    /// subtile, level, Recalc variant), with an atlas holding `resident`
    /// tiles. Returns every lookup and the drawn (screen y, z_adjust).
    fn draw_live_tile(live: (i32, u8, u8, u8), resident: &[u16]) -> LiveTileDraw {
        let mut cell = crate::map::resolved_terrain::test_flat_cell(0, 0);
        (
            cell.final_tile_index,
            cell.final_sub_tile,
            cell.level,
            cell.variant,
        ) = live;
        let terrain =
            crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(1, 1, vec![cell]);
        let calls = std::cell::RefCell::new(Vec::new());
        let lookup = |tid: u16, sub: u8, var: u8| -> Option<TilePlacement> {
            calls.borrow_mut().push((tid, sub, var));
            resident.contains(&tid).then_some(TilePlacement {
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                pixel_size: [TILE_WIDTH, TILE_HEIGHT],
                draw_offset: [0.0, 0.0],
            })
        };
        let drawn = build_visible_instances(
            &live_tile_grid(),
            None,
            0.0,
            0.0,
            1024.0,
            768.0,
            Some(&lookup),
            Some(&terrain),
        )
        .normal
        .iter()
        .map(|instance| (instance.position[1], instance.z_adjust))
        .collect();
        (calls.into_inner(), drawn)
    }

    #[test]
    fn draw_reads_the_live_tile_and_level_a_publication_wrote() {
        let rows = native_z::TILE_HEIGHT_ROWS as f32;
        // Unchanged cell: grid position (level 4) and its Recalc file.
        assert_eq!(
            draw_live_tile((100, 3, 4, 2), &[100]),
            (vec![(100, 3, 2)], vec![(0.0, -rows - 2.0 * rows)])
        );
        // A collapse flood writes a new tile and level 0; Recalc picked file 1.
        assert_eq!(
            draw_live_tile((203, 3, 0, 1), &[100, 203]),
            (vec![(203, 3, 1)], vec![(4.0 * HEIGHT_STEP, -rows)])
        );
    }

    #[test]
    fn draw_keeps_the_grid_tile_when_the_live_tile_is_not_resident() {
        let rows = native_z::TILE_HEIGHT_ROWS as f32;
        assert_eq!(
            draw_live_tile((203, 3, 0, 1), &[100]),
            (
                vec![(203, 3, 1), (100, 3, 2)],
                vec![(0.0, -rows - 2.0 * rows)]
            )
        );
    }
}
