//! Builds a `TileBlocks` provider from a theater's real TMP tile data.
//!
//! The generation phases (shore tiler, zone classifier) ask a `TileBlocks`
//! provider for a tile's sub-cell layout — its `width × height` grid of
//! `(height, terrain)` bytes. This adapter resolves each flat tile index to its
//! TMP filename via the theater `TilesetLookup`, parses the TMP, and projects
//! its cells into a `TileBlock`. The app layer supplies the byte loader (an
//! asset-manager wrapper); this module stays free of the asset layer so the
//! generator's dependency direction is preserved.
//!
//! Every tile the theater defines is loaded eagerly into a table, since the
//! zone classifier can query any cell's tile. A tile whose TMP is missing or
//! unparseable is simply absent — `block()` returns `None`, which the phases
//! read as clear ground (terrain 0), matching a blank/undefined tile.

use std::collections::HashMap;

use crate::assets::tmp_file::TmpFile;
use crate::map::theater::TilesetLookup;

use super::phases::shore::{SubTile, TileBlock, TileBlocks};

/// A `TileBlocks` provider backed by the theater's parsed TMP templates.
pub struct TheaterTileBlocks {
    blocks: HashMap<i32, TileBlock>,
}

impl TheaterTileBlocks {
    /// Build the block table for every tile the theater's `lookup` defines.
    ///
    /// `load` fetches a TMP file's bytes by filename; the app layer wraps the
    /// asset manager. Tiles whose filename is blank, whose bytes are missing, or
    /// whose TMP fails to parse are skipped.
    pub fn build(lookup: &TilesetLookup, mut load: impl FnMut(&str) -> Option<Vec<u8>>) -> Self {
        let mut blocks = HashMap::new();
        for tile_id in 0..lookup.len() {
            let Some(filename) = lookup.filename(tile_id as i32) else {
                continue;
            };
            let Some(bytes) = load(filename) else {
                continue;
            };
            let Ok(tmp) = TmpFile::from_bytes(&bytes) else {
                continue;
            };
            blocks.insert(tile_id as i32, tile_block_from_tmp(&tmp));
        }
        Self { blocks }
    }

    /// Number of tiles that resolved to a parsed block (for tests).
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

impl TileBlocks for TheaterTileBlocks {
    fn block(&self, tile: i32) -> Option<&TileBlock> {
        self.blocks.get(&tile)
    }
}

/// Project one parsed TMP template into a `TileBlock`: the template dimensions
/// and each cell's `(height, terrain_type)` bytes (empty cells stay `None`).
fn tile_block_from_tmp(tmp: &TmpFile) -> TileBlock {
    TileBlock {
        width: tmp.template_width as i32,
        height: tmp.template_height as i32,
        subtiles: tmp
            .tiles
            .iter()
            .map(|cell| {
                cell.as_ref().map(|tile| SubTile {
                    height: tile.height,
                    terrain: tile.terrain_type,
                    slope: tile.ramp_type,
                })
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::theater::parse_tileset_ini;

    /// Diamond pixel count for the 8×4 tiles the fixtures use (rows 4,8,4,0).
    const DIAMOND_PIXELS: usize = 16;

    /// Append one 52-byte TMP cell header + its pixel/depth payload, with the
    /// given `height`/`terrain`/`ramp` in the header's +40/+41/+42 slots.
    fn push_cell(data: &mut Vec<u8>, height: u8, terrain: u8, ramp: u8) {
        data.extend_from_slice(&[0u8; 20]); // bytes 0-19: metadata
        data.extend_from_slice(&0i32.to_le_bytes()); // +20 extra_x
        data.extend_from_slice(&0i32.to_le_bytes()); // +24 extra_y
        data.extend_from_slice(&0u32.to_le_bytes()); // +28 extra_width
        data.extend_from_slice(&0u32.to_le_bytes()); // +32 extra_height
        data.extend_from_slice(&0u32.to_le_bytes()); // +36 flags (no extra)
        data.push(height); // +40 height
        data.push(terrain); // +41 terrain
        data.push(ramp); // +42 ramp
        data.extend_from_slice(&[0u8; 3]); // +43 radar_left
        data.extend_from_slice(&[0u8; 3]); // +46 radar_right
        data.extend_from_slice(&[0u8; 3]); // +49 padding → 52
        for i in 0..DIAMOND_PIXELS {
            data.push(i as u8 + 1); // diamond pixels
        }
        data.extend_from_slice(&[0u8; DIAMOND_PIXELS]); // depth
    }

    /// A `width × 1` TMP with a distinct `(height, terrain, ramp)` per cell.
    fn tmp_row(cells: &[(u8, u8, u8)]) -> Vec<u8> {
        let n = cells.len() as u32;
        let cell_size = 52 + DIAMOND_PIXELS + DIAMOND_PIXELS;
        let table_bytes = cells.len() * 4;
        let first = 16 + table_bytes;
        let mut data = Vec::new();
        data.extend_from_slice(&n.to_le_bytes()); // template_width
        data.extend_from_slice(&1u32.to_le_bytes()); // template_height
        data.extend_from_slice(&8u32.to_le_bytes()); // tile_width
        data.extend_from_slice(&4u32.to_le_bytes()); // tile_height
        for i in 0..cells.len() {
            let offset = (first + i * cell_size) as u32;
            data.extend_from_slice(&offset.to_le_bytes());
        }
        for &(height, terrain, ramp) in cells {
            push_cell(&mut data, height, terrain, ramp);
        }
        data
    }

    #[test]
    fn tmp_projects_into_a_tile_block() {
        let bytes = tmp_row(&[(3, 5, 0), (7, 9, 0)]);
        let tmp = TmpFile::from_bytes(&bytes).expect("valid fixture TMP");
        let block = tile_block_from_tmp(&tmp);
        assert_eq!((block.width, block.height), (2, 1));
        assert_eq!(block.subtiles.len(), 2);
        let a = block.subtiles[0].expect("cell 0 present");
        let b = block.subtiles[1].expect("cell 1 present");
        assert_eq!((a.height, a.terrain), (3, 5));
        assert_eq!((b.height, b.terrain), (7, 9));
    }

    #[test]
    fn the_ramp_byte_reaches_the_sub_tile_slope() {
        // Header slot +42 is the ramp type, and it has to survive the trip
        // into `SubTile` or a carved cliff stair comes out flat. Distinct
        // values per cell so a swap with the +40/+41 neighbours cannot pass:
        // ramp differs from both height and terrain in each cell.
        let bytes = tmp_row(&[(3, 5, 1), (7, 9, 4)]);
        let tmp = TmpFile::from_bytes(&bytes).expect("valid fixture TMP");
        let block = tile_block_from_tmp(&tmp);
        let a = block.subtiles[0].expect("cell 0 present");
        let b = block.subtiles[1].expect("cell 1 present");
        assert_eq!(a.slope, 1);
        assert_eq!(b.slope, 4);
        // And the neighbouring header bytes still land where they did.
        assert_eq!((a.height, a.terrain), (3, 5));
        assert_eq!((b.height, b.terrain), (7, 9));
    }

    #[test]
    fn build_resolves_every_defined_tile() {
        // A one-tileset theater with two tiles; the loader returns the same
        // fixture for either filename.
        let ini = "\
[General]
[TileSet0000]
SetName=Clear
FileName=clear
TilesInSet=2
";
        let lookup = parse_tileset_ini(ini.as_bytes(), "tem").expect("lookup");
        let fixture = tmp_row(&[(0, 0, 0), (1, 8, 2)]);
        let blocks = TheaterTileBlocks::build(&lookup, |_name| Some(fixture.clone()));
        assert_eq!(blocks.len(), 2, "both tile ids resolved to a block");
        let block = blocks.block(0).expect("tile 0 has a block");
        assert_eq!((block.width, block.height), (2, 1));
        assert_eq!(block.subtiles[1].unwrap().terrain, 8);
    }

    #[test]
    fn missing_or_unparseable_tiles_are_absent() {
        let ini = "\
[General]
[TileSet0000]
SetName=Clear
FileName=clear
TilesInSet=2
";
        let lookup = parse_tileset_ini(ini.as_bytes(), "tem").expect("lookup");
        // Loader yields nothing → no blocks; block() falls back to None.
        let blocks = TheaterTileBlocks::build(&lookup, |_name| None);
        assert!(blocks.is_empty());
        assert!(blocks.block(0).is_none());

        // Loader yields garbage → parse fails → still absent, no panic.
        let blocks = TheaterTileBlocks::build(&lookup, |_name| Some(vec![0u8; 4]));
        assert!(blocks.block(0).is_none());
    }
}
