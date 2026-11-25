//! Firestorm ice, terrain half: the theater's three ice families, the tile
//! classifiers Firestorm 2.0's ice code compares cell tiles with, and the
//! tile writes and attribute recalc it performs. The world owner
//! (`sim::world::ice`) runs the chain and its navigation, radar and
//! persistence follow-ups. See docs/ts-ice.md.
//!
//! Firestorm 2.0 keeps a cell's tile at Cell+0x28 and its sub-tile at
//! Cell+0x92; VERA's are `final_tile_index` and `final_sub_tile`.

use super::build::load_tile_metadata;
use super::cliffs::{
    DynamicTilePrototype, recalc_dynamic_tile_attributes, stamp_dynamic_tile_identity,
};
use super::*;

/// Tiles in each ice family window the Firestorm classifiers index: 0 solid
/// ice, 1..=0xF smoothed ice by neighbour mask, 0x10 cracked, 0x11..=0x3E
/// ice/water edge pieces (0x12 the broken stamp) and 0x3F open water.
pub(crate) const ICE_SET_TILES: i32 = 0x40;

/// The tile bases the Firestorm ice code reads, as signed first-tile indexes
/// (-1 when the theater key is absent, as Read_Theater 0x4f4393..0x4f43ab
/// resets them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IceTiles {
    /// Ice1Set, Ice2Set, Ice3Set (Firestorm 2.0 0x7f6018, 0x7f5fdc, 0x7f6084).
    pub(crate) ice: [i32; 3],
    /// WaterSet (Firestorm 2.0 0x7f5960).
    pub(crate) water: i32,
    /// ShorePieces (Firestorm 2.0 0x803440).
    pub(crate) shore: i32,
}

impl IceTiles {
    /// The tile a lookup off the map reads: the shared Dummy's constructor
    /// tile, in no ice window.
    pub(crate) const DUMMY_TILE: i32 = 0xFFFF;

    fn any_window(self, tile: i32, from: i32, to: i32) -> bool {
        self.ice
            .iter()
            .any(|&base| base.wrapping_add(from) <= tile && tile < base.wrapping_add(to))
    }

    /// A set's first tile: solid ice with no open neighbour.
    pub(crate) fn is_base(self, tile: i32) -> bool {
        self.ice.contains(&tile)
    }

    /// Offset 0x10, cracked ice.
    pub(crate) fn is_cracked(self, tile: i32) -> bool {
        self.ice.iter().any(|&base| tile == base.wrapping_add(0x10))
    }

    /// Offsets 0..0x10: ice a unit or explosion can crack
    /// (Crack 0x520670's range test).
    pub(crate) fn is_crackable(self, tile: i32) -> bool {
        self.any_window(tile, 0, 0x10)
    }

    /// Offsets 0..=0x10, the ice Update_Ice 0x51eb40 smooths itself.
    pub(crate) fn is_smoothed_family(self, tile: i32) -> bool {
        self.any_window(tile, 0, 0x11)
    }

    /// Offsets 0x11..0x40: ice/water edge pieces and open water
    /// (Update_Ice's and Edge 0x51fb30's range).
    pub(crate) fn is_edge_piece(self, tile: i32) -> bool {
        self.any_window(tile, 0x11, ICE_SET_TILES)
    }

    /// `[WaterSet, WaterSet+14)`.
    pub(crate) fn is_water(self, tile: i32) -> bool {
        self.water <= tile && tile < self.water.wrapping_add(14)
    }

    /// Is_Tile_Shore 0x45c190: `[ShorePieces, ShorePieces+42)`.
    pub(crate) fn is_shore(self, tile: i32) -> bool {
        self.shore <= tile && tile < self.shore.wrapping_add(0x2a)
    }

    /// Smooth 0x51ef90's own-tile test, as it compares (includeCracked is
    /// zero at every call site): Ice1 through Ice3 offsets 0..0x10 when the
    /// sets ascend.
    pub(crate) fn smooth_eligible(self, tile: i32) -> bool {
        let [i1, i2, i3] = self.ice;
        i1 <= tile
            && tile != i1.wrapping_add(0x10)
            && (tile <= i1.wrapping_add(0x10) || i2 <= tile)
            && tile != i2.wrapping_add(0x10)
            && (tile <= i2.wrapping_add(0x10) || i3 <= tile)
            && tile != i3.wrapping_add(0x10)
            && tile <= i3.wrapping_add(0x10)
    }

    /// Smooth's neighbour test: anything outside the three 0..0x10 windows
    /// is open, cracked ice (0x10) included.
    pub(crate) fn smooth_open(self, tile: i32) -> bool {
        let [i1, i2, i3] = self.ice;
        tile < i1
            || (i1.wrapping_add(0x10) <= tile && tile < i2)
            || (i2.wrapping_add(0x10) <= tile && tile < i3)
            || i3.wrapping_add(0x10) <= tile
    }

    /// Smooth's no-change test for `mask`, the open-neighbour bits.
    pub(crate) fn smooth_settled(self, tile: i32, mask: i32) -> bool {
        self.ice.iter().any(|&base| {
            if mask != 0 {
                base.wrapping_add(1).wrapping_add(mask) == tile
            } else {
                tile == base || tile == base.wrapping_add(1)
            }
        })
    }

    /// Edge 0x51fb30's own-tile test: water or an edge piece.
    pub(crate) fn edge_eligible(self, tile: i32) -> bool {
        self.is_water(tile) || self.is_edge_piece(tile)
    }

    /// Edge's neighbour test: neither water, an edge piece nor shore.
    pub(crate) fn edge_solid(self, tile: i32) -> bool {
        !self.is_water(tile) && !self.is_edge_piece(tile) && !self.is_shore(tile)
    }

    /// Break 0x520870's footprint test: water or anything from Ice1Set's
    /// first tile to Ice3Set's last.
    pub(crate) fn breakable(self, tile: i32) -> bool {
        self.is_water(tile)
            || (self.ice[0] <= tile && tile < self.ice[2].wrapping_add(ICE_SET_TILES))
    }

    /// Grow 0x520d20's first pass: Ice1 or Ice2 offsets 1..0x3f other than
    /// cracked.
    pub(crate) fn grow_candidate(self, tile: i32) -> bool {
        let [i1, i2, _] = self.ice;
        tile != i1.wrapping_add(0x10)
            && tile != i2.wrapping_add(0x10)
            && ((i1.wrapping_add(1) <= tile && tile < i1.wrapping_add(0x3f))
                || (i2.wrapping_add(1) <= tile && tile < i2.wrapping_add(0x3f)))
    }

    /// Grow's second pass on the live tile: past Ice1's cracked tile up to
    /// Ice2's first smoothed tile, or past Ice2's cracked tile.
    pub(crate) fn grow_refreezes(self, tile: i32) -> bool {
        let [i1, i2, _] = self.ice;
        (i1.wrapping_add(0x10) < tile && tile < i2.wrapping_add(1)) || i2.wrapping_add(0x10) < tile
    }

    /// Solidify 0x520eb0's test: offsets 1..=0x10 of any set.
    pub(crate) fn solidifies(self, tile: i32) -> bool {
        self.any_window(tile, 1, 0x11)
    }
}

/// The 3x64 ice tiles' sub-tile 0 prototypes, set-major.
#[derive(Debug, Clone)]
pub(super) struct IceCatalog {
    bundled: bool,
    tiles: IceTiles,
    prototypes: Vec<DynamicTilePrototype>,
}

impl IceCatalog {
    fn prototype(&self, tile: i32) -> Option<&DynamicTilePrototype> {
        let (set, base) = self
            .tiles
            .ice
            .iter()
            .enumerate()
            .find(|&(_, &base)| base <= tile && tile < base.wrapping_add(ICE_SET_TILES))?;
        let offset = usize::try_from(tile - base).ok()?;
        self.prototypes.get(set * ICE_SET_TILES as usize + offset)
    }
}

/// Whether Firestorm 2.0 Read_Theater's land patch (0x4f54a5..0x4f5562)
/// rewrites `tile_id`: on SNOW, sub-tile 0 of each ice family's offsets
/// 0x11..=0x3f has its TMP terrain byte (+0x29) set to 9, Water.
pub(super) fn ice_tile_is_water(ice_sets: Option<[u16; 3]>, tile_id: u16) -> bool {
    ice_sets.is_some_and(|sets| {
        sets.iter()
            .any(|&base| (0x11..ICE_SET_TILES).contains(&(i32::from(tile_id) - i32::from(base))))
    })
}

/// The theater's ice catalog, or None when the theater runs no ice
/// ([`TheaterData::ice_sets`]) or lacks any of its 192 tiles.
pub(super) fn build_ice_catalog(
    theater_data: Option<&TheaterData>,
    asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
    terrain_rules: Option<&TerrainRules>,
) -> Option<IceCatalog> {
    let td = theater_data?;
    let assets = asset_manager?;
    let sets = td.ice_sets?;
    let base = |key: Option<u16>| key.map_or(-1, i32::from);
    let tiles = IceTiles {
        ice: sets.map(i32::from),
        water: base(td.rmg_tiles.water_set),
        shore: base(td.rmg_tiles.shore_pieces),
    };
    let prototypes = sets
        .iter()
        .flat_map(|&set| (0..ICE_SET_TILES as u16).map(move |offset| set + offset))
        .map(|tile_id| {
            td.lookup.filename(i32::from(tile_id))?;
            Some(DynamicTilePrototype {
                metadata: load_tile_metadata(
                    Some(td),
                    Some(assets),
                    terrain_rules,
                    TileKey {
                        tile_id,
                        sub_tile: 0,
                        variant: 0,
                    },
                ),
                accepts_smudge: td.lookup.is_morphable(tile_id),
                allows_tiberium: td.lookup.allows_tiberium(tile_id),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(IceCatalog {
        bundled: td.uses_bundled_ice(),
        tiles,
        prototypes,
    })
}

impl ResolvedTerrainGrid {
    /// Whether a saved map needs VERA's embedded tiles when rebinding the theater.
    pub(crate) fn uses_bundled_ice(&self) -> bool {
        self.ice_catalog
            .as_ref()
            .is_some_and(|catalog| catalog.bundled)
    }

    /// The ice tile bases when this map runs the Firestorm ice chain: a Snow
    /// theater with all three ice families. None keeps every ice path off.
    pub(crate) fn ice_tiles(&self) -> Option<IceTiles> {
        self.ice_catalog.as_ref().map(|catalog| catalog.tiles)
    }

    /// Tiles the ice chain may write at runtime, for the atlas pre-load;
    /// empty when the map runs no ice.
    pub fn ice_atlas_tiles(&self) -> Vec<u16> {
        self.ice_catalog.as_ref().map_or_else(Vec::new, |catalog| {
            catalog
                .tiles
                .ice
                .iter()
                .flat_map(|&base| (0..ICE_SET_TILES).map(move |offset| base + offset))
                .filter_map(|tile| u16::try_from(tile).ok())
                .collect()
        })
    }

    /// The real cell at a Firestorm map lookup's coordinate (`x + y*0x200`,
    /// a null slot giving the Dummy).
    pub(crate) fn ice_cell_index(&self, (x, y): (i16, i16)) -> Option<usize> {
        self.native_fixed_cell_index(x, y)
    }

    /// Cell+0x28 at `coord`: a real cell's tile, the Dummy's constructor
    /// tile off the map.
    pub(crate) fn ice_tile_at(&self, coord: (i16, i16)) -> i32 {
        self.ice_cell_index(coord)
            .map_or(IceTiles::DUMMY_TILE, |index| {
                self.cells[index].final_tile_index
            })
    }

    /// Write Cell+0x28 alone, and Cell+0x92 = 0 when `reset_sub_tile`. A
    /// write to the Dummy changes nothing anyone reads. Returns the real
    /// cell written.
    pub(crate) fn set_ice_tile(
        &mut self,
        coord: (i16, i16),
        tile: i32,
        reset_sub_tile: bool,
    ) -> Option<(u16, u16)> {
        let index = self.ice_cell_index(coord)?;
        let cell = &mut self.cells[index];
        cell.final_tile_index = tile;
        if reset_sub_tile {
            cell.final_sub_tile = 0;
        }
        Some((cell.rx, cell.ry))
    }

    /// Firestorm 2.0 Recalc_Attributes 0x4531b0 on an ice cell: its tile's
    /// land, speeds, slope and radar colour from the catalog. Bridge state
    /// is the bridge owner's and stays. Overlay land is re-applied by the
    /// caller. Returns the real cell recalculated.
    pub(crate) fn recalc_ice_cell(&mut self, coord: (i16, i16)) -> Option<(u16, u16)> {
        let index = self.ice_cell_index(coord)?;
        let catalog = self.ice_catalog.as_ref()?;
        let cell = &self.cells[index];
        let prototype = catalog.prototype(cell.final_tile_index)?.clone();
        let tile = u16::try_from(cell.final_tile_index).ok()?;
        let cell = &mut self.cells[index];
        let sub_tile = cell.final_sub_tile;
        let bridge = (
            cell.has_bridge_deck,
            cell.bridge_walkable,
            cell.bridge_transition,
            cell.bridge_deck_level,
            cell.bridge_layer.take(),
            cell.bridge_facts,
        );
        stamp_dynamic_tile_identity(cell, tile, sub_tile, &prototype);
        recalc_dynamic_tile_attributes(cell, &prototype);
        (
            cell.has_bridge_deck,
            cell.bridge_walkable,
            cell.bridge_transition,
            cell.bridge_deck_level,
            cell.bridge_layer,
            cell.bridge_facts,
        ) = bridge;
        let coord = (cell.rx, cell.ry);
        self.radar_color_valid[index] = true;
        self.damaged_radar_metadata[index] = None;
        Some(coord)
    }

    /// Run ice over `tiles`: each set's offsets 0..=0x10 Ice land and
    /// 0x11..=0x3f Water, as the theater patch leaves them.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_install_ice_catalog(&mut self, tiles: IceTiles) {
        let prototype = |offset: i32| {
            let land = if offset > 0x10 {
                LandType::Water
            } else {
                LandType::Ice
            };
            DynamicTilePrototype {
                metadata: super::build::TileMetadata {
                    tileset_index: Some(1),
                    land_type: land.as_index(),
                    ..super::build::TileMetadata::default()
                },
                accepts_smudge: false,
                allows_tiberium: false,
            }
        };
        self.ice_catalog = Some(IceCatalog {
            bundled: false,
            tiles,
            prototypes: (0..3)
                .flat_map(|_| (0..ICE_SET_TILES).map(prototype))
                .collect(),
        });
    }
}
