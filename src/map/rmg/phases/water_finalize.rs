//! Water finalizer: assigns visible water tile variants.
//!
//! gamemd: `RandomMapGenerator::VaryWaterTiles` 0x0059C630, called from
//! `RandomMapGenerator::Generate` at 0x00598B14.
//!
//! For 2x2 all-water clusters (anchor + E/S/SE, none sub-tiled yet) a large
//! multi-cell variant may be placed; single cells get one of six band
//! variants. All three draws are scaled-FP chains with perturbed-mantissa
//! constants — NOT integer mods; a `%` implementation maps the same raw draw
//! to different variants.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::rng::RmgRng;
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;
use crate::map::rmg::x87::TruncF64;
use crate::util::native_x87::X87Chop53;

use super::carve::stamp_iso_block;
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

pub fn run(
    grid: &mut RmgGrid,
    scratch: &mut RmgScratch,
    ids: &TileIds,
    blocks: &dyn TileBlocks,
    rng: &mut RmgRng,
) {
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
                // 59C74B: signed /40 (0x66666667, SAR 4), so draws 0..239
                // pick +0..+5 and only 240/241 pick +7/+6, which are rocks
                // in water (Rock land) in the retail temperate/snow sets.
                let variant = if draw < 240 { draw / 40 } else { 0xF7 - draw };
                // 59C75E..59C795: the anchor's scratch region and level -1,
                // so the stamp keeps every cell's level.
                let region = scratch.get(x, y).region;
                stamp_iso_block(
                    grid,
                    scratch,
                    blocks,
                    ids.water_base + variant,
                    (x, y),
                    region,
                    None,
                );
                continue;
            }
        }

        // Single-cell path: six bands, band 5 only at draw 200.
        let draw = scaled_draw(rng, K201_BITS, 0.0, 200);
        grid.get_mut(x, y).expect("native cell").tile = ids.water_base + 8 + draw / 40;
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

    /// Run the finalizer over an all-water grid whose cells sit at level 4,
    /// the level the sea seeder leaves.
    fn finalized(seed: u16) -> RmgGrid {
        let mut grid = water_grid();
        let mut scratch = RmgScratch::new(30, 10, 26);
        run(
            &mut grid,
            &mut scratch,
            &ids(),
            &blocks(),
            &mut RmgRng::new(seed),
        );
        grid
    }

    #[test]
    fn finalize_is_deterministic() {
        let tiles = |seed| {
            let grid = finalized(seed);
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

    /// 59C790 pushes level -1, so the 2x2 stamp keeps each cell's level.
    /// Writing the TMP height (0) instead sank the clusters four levels below
    /// the surrounding sea, where they drew as black, impassable holes.
    #[test]
    fn water_blocks_keep_the_sea_level() {
        let grid = finalized(4321);
        assert!(
            grid.native_cells()
                .all(|(x, y)| grid.get(x, y).unwrap().level == 4)
        );
    }

    /// 59C74B divides the 0..239 draw by 40, so a 2x2 cluster takes one of
    /// the WaterSet's 2x2 tiles (+0..+7), never a tile past the 14-tile set.
    #[test]
    fn water_blocks_stay_inside_the_water_set() {
        let grid = finalized(4321);
        let water_set = 500..514;
        assert!(
            grid.native_cells()
                .all(|(x, y)| water_set.contains(&grid.get(x, y).unwrap().tile))
        );
    }
}
