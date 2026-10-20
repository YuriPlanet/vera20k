//! Water finalizer: assigns visible water tile variants.
//!
//! gamemd: `MapClass::RmgFinalizeWaterShore` 0x0057A0C0 and the three helpers
//! it alone calls — `ClearBridgeCell_Low` 0x0057A320, `UpdateBridgeTile_Low`
//! 0x0057A430 and `SelectBridgeTileVariant_Low` 0x0057ACF0 — all three of
//! which read `MapClass::ComputeBridgeSurfaceMask` 0x0057B210. Verified by
//! `get_function_callers`: none of them has a runtime caller, and
//! 0x0057A430 works in `RmgGrid__GetRegionTag` / `SetRegionTag` and
//! `g_WaterSet_TileSetBase`, so the cluster is map-generation tile work, not
//! in-match bridge behaviour. `MapClass::ComputeBridgeAdjacencyMask_Low`
//! 0x00579B70 sits in the same band and is called only from
//! `RaisePinchedCliffCell` 0x00579010, `SelectAndStampCliffFaceTile`
//! 0x00579620 and `RandomMapGenerator::JitterCliffEdges` 0x005A19E0.
//! Despite its inherited label, `CellClass::HasBridgeOverlay` 0x004865D0 is a
//! tile-family range predicate: it does not read overlay `Cell+0x44`. The
//! active low-deck validator uses it to admit waterfall families. This cluster
//! is reached by stock Create Random Map/`.SED` generation.
//!
//! For 2x2 all-water clusters (anchor + E/S/SE, none sub-tiled yet) a large
//! multi-cell variant may be placed; single cells get one of six band
//! variants. All three draws are scaled-FP chains with perturbed-mantissa
//! constants — NOT integer mods; a `%` implementation maps the same raw draw
//! to different variants.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::rng::RmgRng;
use crate::map::rmg::tiles::TileIds;
use crate::map::rmg::x87::TruncF64;
use crate::util::native_x87::X87Chop53;

use super::shore::TileBlocks;

/// ~10*2^-32 — 2x2-vs-single selector scale (with +1.0 offset).
const K10_BITS: u64 = 0x3E24_0000_0014_0000;
/// ~242*2^-32 — 2x2 variant draw scale.
const K242_BITS: u64 = 0x3E6E_4000_001E_4000;
/// ~201*2^-32 — single-cell band draw scale.
const K201_BITS: u64 = 0x3E69_2000_0019_2000;

fn scaled_draw(rng: &mut RmgRng, k_bits: u64, offset: f64, max: i32) -> i32 {
    let scale = TruncF64::from_f64(f64::from_bits(k_bits));
    loop {
        let value = X87Chop53::ftol_f64_low_masked(
            TruncF64::from_f64(f64::from(rng.next_u32()))
                .mul(scale)
                .add(TruncF64::from_f64(offset))
                .to_f64(),
        );
        if value <= max {
            return value;
        }
    }
}

pub fn run(grid: &mut RmgGrid, ids: &TileIds, blocks: &dyn TileBlocks, rng: &mut RmgRng) {
    let coords: Vec<(i32, i32)> = grid.native_cells().collect();
    for &(x, y) in &coords {
        let anchor_ok = {
            let cell = grid.get(x, y).expect("native cell");
            cell.tile == ids.water_base && cell.sub_tile == 0
        };
        if !anchor_ok {
            continue;
        }

        // E, S, SE neighbors form a 2x2 block with this cell as NW anchor.
        let cluster = [2usize, 4, 3].iter().all(|&dir| {
            let (nx, ny) = RmgGrid::step(x, y, dir);
            grid.get(nx, ny)
                .is_some_and(|cell| cell.tile == ids.water_base && cell.sub_tile == 0)
        });

        if cluster {
            let selector = scaled_draw(rng, K10_BITS, 1.0, 10);
            if selector != 1 {
                let draw = scaled_draw(rng, K242_BITS, 0.0, 241);
                let variant = if draw < 240 { draw / 10 } else { 0xF7 - draw };
                place_block(grid, blocks, ids.water_base + variant, (x, y));
                continue;
            }
        }

        // Single-cell path: six bands, band 5 only at draw 200.
        let draw = scaled_draw(rng, K201_BITS, 0.0, 200);
        grid.get_mut(x, y).expect("native cell").tile = ids.water_base + 8 + draw / 40;
    }
}

/// Multi-cell tile placement: writes tile, sub-tile index, and the block's
/// height byte per covered cell (the region/player args of the original are
/// inert on this path).
fn place_block(grid: &mut RmgGrid, blocks: &dyn TileBlocks, tile: i32, anchor: (i32, i32)) {
    let Some(block) = blocks.block(tile) else {
        return;
    };
    let block = block.clone();
    for j in 0..block.height {
        for i in 0..block.width {
            let Some(sub) = block.subtiles[(j * block.width + i) as usize] else {
                continue;
            };
            if let Some(cell) = grid.get_mut(anchor.0 + i, anchor.1 + j) {
                cell.tile = tile;
                cell.sub_tile = (j * block.width + i) as u8;
                cell.level = sub.height;
                cell.slope = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::phases::shore::{SubTile, TileBlock};
    use crate::map::theater::TheaterCliffRanges;

    struct TwoByTwo(TileBlock);

    impl TileBlocks for TwoByTwo {
        fn block(&self, _tile: i32) -> Option<&TileBlock> {
            Some(&self.0)
        }
    }

    fn blocks() -> TwoByTwo {
        TwoByTwo(TileBlock {
            width: 2,
            height: 2,
            subtiles: vec![
                Some(SubTile {
                    height: 0,
                    terrain: 0,
                    slope: 0
                });
                4
            ],
        })
    }

    fn ids() -> TileIds {
        TileIds {
            clear: 0,
            ramp_base: -1,
            ramp_smooth: -1,
            slope_set_pieces: -1,
            rough: -1,
            sand: -1,
            green: -1,
            rough_lat: -1,
            sand_lat: -1,
            green_lat: -1,
            pave_lat: -1,
            pave: -1,
            water_base: 500,
            shore: 400,
            water_bridge: -1,
            misc_pave: -1,
            paved_roads: -1,
            paved_road_ends: -1,
            medians: -1,
            cliffs: TheaterCliffRanges::default(),
        }
    }

    fn water_grid() -> RmgGrid {
        let mut grid = RmgGrid::new(30, 10, 26);
        let coords: Vec<(i32, i32)> = grid.native_cells().collect();
        for (x, y) in coords {
            grid.get_mut(x, y).unwrap().tile = 500;
        }
        grid
    }

    #[test]
    fn finalize_is_deterministic() {
        let tiles = |seed| {
            let mut grid = water_grid();
            let identity = ids();
            let block_table = blocks();
            let mut rng = RmgRng::new(seed);
            run(&mut grid, &identity, &block_table, &mut rng);
            grid.native_cells()
                .collect::<Vec<_>>()
                .iter()
                .map(|&(x, y)| {
                    let cell = grid.get(x, y).unwrap();
                    (cell.tile, cell.sub_tile)
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(tiles(4321), tiles(4321));
    }
}
