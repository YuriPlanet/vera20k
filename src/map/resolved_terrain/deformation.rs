//! Terrain deformation, terrain half: the active theater's clear and ramp
//! tiles the hills corner engine writes, whether the scenario's rules switched
//! it on, and the live stamp of what its finalize chose. The world owner
//! (`sim::world::terrain_deformation`) runs the engine and the navigation,
//! lighting, radar and persistence follow-ups.

use super::build::load_tile_metadata;
use super::cliffs::{
    DynamicTilePrototype, recalc_dynamic_tile_attributes, stamp_dynamic_tile_identity,
};
use super::*;
use crate::map::corner_heights::{FinalCell, FinalTiles};
use crate::map::rmg::tiles::TileIds;

/// The ramp tiles a finalized cell may take: `RampBase + slope - 1` for each
/// non-flat slope 1..=18 of the corner engine's table.
const RAMP_TILES: i32 = 18;

#[derive(Debug, Clone)]
pub(super) struct DeformationCatalog {
    ids: TileIds,
    clear: DynamicTilePrototype,
    ramps: Vec<DynamicTilePrototype>,
    lat_config: lat::LatConfig,
    slope_config: lat::SlopeFixupConfig,
}

impl DeformationCatalog {
    fn prototype(&self, slope: u8) -> Option<&DynamicTilePrototype> {
        match slope {
            0 => Some(&self.clear),
            slope => self.ramps.get(usize::from(slope) - 1),
        }
    }

    /// Every tile a stamp or the LAT and slope fixups after it can write.
    pub(super) fn tile_ids(&self) -> Vec<u16> {
        let mut tiles: Vec<u16> = std::iter::once(self.ids.clear)
            .chain((0..RAMP_TILES).map(|offset| self.ids.ramp_base + offset))
            .chain(lat::fixup_tiles(&self.lat_config, self.slope_config))
            .filter_map(|tile| u16::try_from(tile).ok())
            .collect();
        tiles.sort_unstable();
        tiles.dedup();
        tiles
    }
}

/// The theater's clear tile and its 18 ramps `RampBase..`, with the LAT and
/// slope configs its fixups use. None when the theater lacks any of them.
pub(super) fn build_deformation_catalog(
    theater_data: Option<&TheaterData>,
    asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
    terrain_rules: Option<&TerrainRules>,
) -> Option<DeformationCatalog> {
    let td = theater_data?;
    let assets = asset_manager?;
    let ids = TileIds::from_keys(&td.rmg_tiles);
    let prototype = |tile: i32| -> Option<DynamicTilePrototype> {
        let tile_id = u16::try_from(tile).ok()?;
        td.lookup.filename(tile)?;
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
    };
    let clear = prototype(ids.clear)?;
    let ramps = (0..RAMP_TILES)
        .map(|offset| prototype(ids.ramp_base + offset))
        .collect::<Option<Vec<_>>>()?;
    Some(DeformationCatalog {
        lat_config: lat::LatConfig::from_tile_ids(&ids),
        slope_config: lat::SlopeFixupConfig::from_tile_ids(&ids),
        ids,
        clear,
        ramps,
    })
}

impl ResolvedTerrainGrid {
    /// Switch terrain deformation on or off for this scenario, from its
    /// rules' `[CombatDamage] TerrainDeformation=`. Off (the default), no
    /// cell deforms and zone builds read Levels as gamemd does.
    pub fn set_terrain_deformation(&mut self, enabled: bool) {
        self.deformation_enabled = enabled;
    }

    /// Whether cells may deform: switched on, and the theater has every tile
    /// a deformation stamps. Only then can ground sink below Level 0.
    pub(crate) fn terrain_deformation(&self) -> bool {
        self.deformation_enabled && self.deformation_catalog.is_some()
    }

    /// The clear and ramp tile ids the corner engine's finalize picks
    /// between, when [`Self::terrain_deformation`] is on.
    pub(crate) fn deformation_tiles(&self) -> Option<FinalTiles> {
        let catalog = self
            .deformation_catalog
            .as_ref()
            .filter(|_| self.deformation_enabled)?;
        Some(FinalTiles {
            clear: catalog.ids.clear,
            ramp_base: catalog.ids.ramp_base,
        })
    }

    /// The tile half of the corner engine's editable test (`0x006B2520`):
    /// an unassigned or unregistered tile index passes, any other its
    /// tileset's `Morphable=` (`IsoTileType+0x2E0`, kept as `accepts_smudge`).
    pub(crate) fn deformation_tile_morphable(&self, cell: &ResolvedTerrainCell) -> bool {
        let tile = cell.final_tile_index;
        tile < 0
            || tile == 0xFFFF
            || self
                .tile_registry_len
                .is_some_and(|len| tile as usize >= len)
            || cell.accepts_smudge
    }

    /// Stamp the cells the corner engine's finalize (`0x006B3850`) chose,
    /// the way cliff collapse (`MapClass::CollapseDestroyableCliff
    /// @ 0x00581140`) stamps its replacement tiles: each cell's tile, slope
    /// and Level, then the runtime LAT and slope fixup (`0x0047CA80`) of it
    /// and its four neighbours once every cell is stamped, then its
    /// attributes from the stamped tile. Returns every cell whose tile or
    /// Level changed, the stamped cells first.
    pub(crate) fn stamp_deformation(&mut self, finals: &[FinalCell]) -> Vec<(u16, u16)> {
        let Some(catalog) = self.deformation_catalog.clone() else {
            return Vec::new();
        };
        let mut stamped = Vec::new();
        for f in finals {
            let (Ok(x), Ok(y)) = (u16::try_from(f.x), u16::try_from(f.y)) else {
                continue;
            };
            let Some(index) = self.index(x, y) else {
                continue;
            };
            let Some((slope, tile)) = f.slope_tile else {
                continue;
            };
            let (Some(prototype), Ok(tile)) = (catalog.prototype(slope), u16::try_from(tile))
            else {
                continue;
            };
            stamp_dynamic_tile_identity(&mut self.cells[index], tile, 0, prototype);
            // The engine's slope, as 0x006B3850 writes Cell+11C.
            self.cells[index].slope_type = slope;
            self.set_cell_level(index, f.level as i8 as u8);
            stamped.push((x, y, index, slope));
        }
        let mut changed: Vec<(u16, u16)> = stamped.iter().map(|&(x, y, ..)| (x, y)).collect();
        for &(x, y, ..) in &stamped {
            for (dx, dy) in [(0i16, 0i16), (0, -1), (1, 0), (0, 1), (-1, 0)] {
                self.apply_runtime_lat_slope(
                    (x as i16).wrapping_add(dx),
                    (y as i16).wrapping_add(dy),
                    &catalog.lat_config,
                    catalog.slope_config,
                    &mut changed,
                );
            }
        }
        for &(_, _, index, slope) in &stamped {
            let prototype = catalog.prototype(slope).expect("stamped slope has a tile");
            recalc_dynamic_tile_attributes(&mut self.cells[index], prototype);
            self.radar_color_valid[index] = true;
            self.damaged_radar_metadata[index] = None;
        }
        changed
    }

    /// Switch deformation on over a catalog of Clear prototypes: tile
    /// `clear`, ramps from `ramp_base` (each with its own slope) and smoothed
    /// ramps from `ramp_base + 20`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_install_deformation_catalog(&mut self, clear: u16, ramp_base: u16) {
        let prototype = |slope: u8| DynamicTilePrototype {
            metadata: super::build::TileMetadata {
                tileset_index: Some(1),
                land_type: LandType::Clear.as_index(),
                slope_type: slope,
                ..super::build::TileMetadata::default()
            },
            accepts_smudge: true,
            allows_tiberium: true,
        };
        let mut ids = TileIds::from_keys(&Default::default());
        ids.clear = i32::from(clear);
        ids.ramp_base = i32::from(ramp_base);
        ids.ramp_smooth = i32::from(ramp_base) + 20;
        self.deformation_catalog = Some(DeformationCatalog {
            lat_config: lat::LatConfig::from_tile_ids(&ids),
            slope_config: lat::SlopeFixupConfig::from_tile_ids(&ids),
            ids,
            clear: prototype(0),
            ramps: (1..=RAMP_TILES as u8).map(prototype).collect(),
        });
        self.deformation_enabled = true;
    }
}
