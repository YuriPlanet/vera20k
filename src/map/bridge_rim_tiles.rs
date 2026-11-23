//! Theater identity used by native high-bridge edge cleanup, 576770/576200.

use crate::assets::asset_manager::AssetManager;
use crate::assets::tmp_file::TmpFile;
use crate::map::resolved_terrain::ResolvedTerrainCell;
use crate::map::theater::TheaterData;
use crate::rules::terrain_rules::{LandType, tmp_terrain_to_land_type};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighBridgeRimTiles {
    pub base: i32,
    pub top_left: [i32; 2],
    pub bottom_right: [i32; 2],
    pub top_right: [i32; 2],
    pub bottom_left: [i32; 2],
    pub middle: [i32; 2],
}

impl HighBridgeRimTiles {
    /// No active theater: every native global keeps its -1 reset.
    pub const ABSENT: Self = Self {
        base: -1,
        top_left: [-1; 2],
        bottom_right: [-1; 2],
        top_right: [-1; 2],
        bottom_left: [-1; 2],
        middle: [-1; 2],
    };

    /// Read_Theater_TileSets_INI (`0x00545150`) reads these ten `[General]`
    /// keys with CCINIClass::ReadInt (`0x005276D0`; calls `0x00545B4D` through
    /// `0x00545C2E`, default -1) straight into signed globals. Their values
    /// are BridgeSet-relative tile identities, not TileSet indices; `base` is
    /// the published BridgeSet tile base. `load_theater` is the one reader.
    pub(crate) fn read(base: i32, general: Option<&crate::rules::ini_parser::IniSection>) -> Self {
        let value = |key| general.map_or(-1, |section| section.read_int(key, -1));
        Self {
            base,
            top_left: [value("BridgeTopLeft1"), value("BridgeTopLeft2")],
            bottom_right: [value("BridgeBottomRight1"), value("BridgeBottomRight2")],
            top_right: [value("BridgeTopRight1"), value("BridgeTopRight2")],
            bottom_left: [value("BridgeBottomLeft1"), value("BridgeBottomLeft2")],
            middle: [value("BridgeMiddle1"), value("BridgeMiddle2")],
        }
    }

    /// [`Self::read`] over theater INI bytes, for fixtures.
    #[cfg(any(test, feature = "test-support"))]
    pub fn from_ini(base: i32, bytes: &[u8]) -> Self {
        let ini = crate::rules::ini_parser::IniFile::from_bytes(bytes).ok();
        Self::read(base, ini.as_ref().and_then(|ini| ini.section("General")))
    }

    fn relative(self, tile: i32) -> i32 {
        tile.wrapping_sub(self.base).wrapping_add(1)
    }

    fn middle_matches(self, relative: i32, axis: usize) -> bool {
        (0..4).any(|variant| relative == self.middle[axis].wrapping_add(variant))
    }

    /// Direction of the edge search dispatched by 576770. Preserve the native
    /// first-match order, including aliased theater keys.
    pub(crate) fn start_direction(self, tile: i32, subtile: u8) -> Option<u8> {
        let relative = self.relative(tile);
        if (subtile == 8 && self.top_left.contains(&relative))
            || (subtile == 5 && self.middle_matches(relative, 0))
        {
            Some(2)
        } else if (subtile == 12 && self.top_right.contains(&relative))
            || (subtile == 7 && self.middle_matches(relative, 1))
        {
            Some(4)
        } else {
            None
        }
    }

    /// `MapClass::IsBridgeRampTile` 0x005746C0: the tile the CABHUT death
    /// fallback walks to from the bridge anchor (0x00574415, 0x00575031).
    pub(crate) fn is_ramp(self, tile: i32, subtile: u8) -> bool {
        let relative = self.relative(tile);
        (subtile == 12 && self.top_right.contains(&relative))
            || (subtile == 4 && self.middle_matches(relative, 0))
            || (subtile == 8 && self.top_left.contains(&relative))
            || (subtile == 2 && self.middle_matches(relative, 1))
    }

    /// The end test of 576200's edge search, and the whole body of
    /// `MapClass::IsLowBridgeEndpointTile` 0x00574600, which the CABHUT
    /// death fallback calls (0x00574544, 0x00575160).
    pub(crate) fn is_end(self, tile: i32, subtile: u8, direction: u8) -> bool {
        let relative = self.relative(tile);
        match direction {
            2 => {
                subtile == 4
                    && (self.bottom_right.contains(&relative) || self.middle_matches(relative, 0))
            }
            4 => {
                subtile == 2
                    && (self.bottom_left.contains(&relative) || self.middle_matches(relative, 1))
            }
            _ => false,
        }
    }
}

/// The pristine WoodBridgeSet has separate template geometry from BridgeSet.
#[derive(Debug, Clone, Default)]
pub(super) struct WoodBridgeRimTiles(Vec<(i32, WoodBridgeRim)>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WoodBridgeRim {
    direction: u8,
    /// The first anchor is on the low strip of a start template.
    anchor_subtile: Option<u8>,
    road_subtile: u8,
}

impl WoodBridgeRim {
    fn from_template(tmp: &TmpFile) -> Option<Self> {
        let (direction, depth) = match (tmp.template_width, tmp.template_height) {
            (depth @ (2 | 3), 5) => (2, depth),
            (5, depth @ (2 | 3)) => (4, depth),
            _ => return None,
        };
        let subtile = |along, across| {
            if direction == 2 {
                across * tmp.template_width + along
            } else {
                along * tmp.template_width + across
            }
        };
        let tile = |along, across| tmp.tiles.get(subtile(along, across) as usize)?.as_ref();
        let deck_height = crate::util::lepton::BRIDGE_DECK_HEIGHT_LEVELS as u8;
        // The two-deep raised road occupies the inner three lanes. A start
        // template has a lower third row at the span, and an end has road
        // at its span-facing edge. This is the retail Ovrpsb TMP layout
        // (NewUrban Ovrpsb01/02/03/06) and works with reordered tile IDs.
        if !(0..2).all(|along| {
            (1..=3).all(|across| {
                tile(along, across).is_some_and(|tile| {
                    tile.has_damaged_data
                        && tile.height == deck_height
                        && tmp_terrain_to_land_type(tile.terrain_type) == LandType::Road
                })
            })
        }) || (depth == 3
            && !tile(2, 2).is_some_and(|tile| tile.has_damaged_data && tile.height < deck_height))
        {
            return None;
        }
        Some(Self {
            direction,
            anchor_subtile: (depth == 3).then(|| subtile(2, 2) as u8),
            road_subtile: subtile(if depth == 3 { 1 } else { 0 }, 2) as u8,
        })
    }
}

impl WoodBridgeRimTiles {
    pub(super) fn from_theater(theater: &TheaterData, assets: &AssetManager) -> Self {
        let Some(bounds) = theater
            .wood_bridge_set
            .and_then(|set| theater.lookup.bounds().get(usize::from(set)))
        else {
            return Self::default();
        };
        let rims = (0..bounds.count)
            .filter_map(|relative| {
                let tile = bounds.start.checked_add(relative)?;
                // Damaged art suffixes can carry a different footprint.
                let filename = theater.lookup.filename_for_variant(tile, 0)?;
                let tmp = TmpFile::from_bytes(assets.get_ref(filename)?).ok()?;
                Some((i32::from(tile), WoodBridgeRim::from_template(&tmp)?))
            })
            .collect();
        Self(rims)
    }

    fn at(&self, cell: &ResolvedTerrainCell, direction: u8) -> Option<WoodBridgeRim> {
        self.0.iter().find_map(|&(tile, rim)| {
            (cell.final_tile_index == tile && rim.direction == direction).then_some(rim)
        })
    }

    pub(super) fn is_start(
        &self,
        anchor: &ResolvedTerrainCell,
        approach: &ResolvedTerrainCell,
        direction: u8,
    ) -> bool {
        self.at(anchor, direction).is_some_and(|rim| {
            rim.anchor_subtile == Some(anchor.final_sub_tile)
                && approach.final_tile_index == anchor.final_tile_index
                && approach.final_sub_tile == rim.road_subtile
        })
    }

    pub(super) fn is_end(&self, approach: &ResolvedTerrainCell, direction: u8) -> bool {
        self.at(approach, direction).is_some_and(|rim| {
            rim.anchor_subtile.is_none() && approach.final_sub_tile == rim.road_subtile
        })
    }
}

#[cfg(all(test, tests_map))]
mod tests {
    use super::HighBridgeRimTiles;

    /// 0x005746C0 pairs TopRight with sub-tile 12, TopLeft with 8, the four
    /// Middle1 tiles with 4 and the four Middle2 tiles with 2; no bottom key
    /// is a ramp.
    #[test]
    fn ramp_tiles_follow_the_native_key_and_sub_tile_pairs() {
        let tiles = HighBridgeRimTiles::from_ini(
            100,
            b"[General]\nBridgeTopLeft1=1\nBridgeTopLeft2=2\nBridgeBottomRight1=3\n\
              BridgeBottomRight2=4\nBridgeTopRight1=5\nBridgeTopRight2=6\n\
              BridgeBottomLeft1=7\nBridgeBottomLeft2=8\nBridgeMiddle1=9\nBridgeMiddle2=13\n",
        );
        // Keys are one-based BridgeSet indices: tile = base + key - 1.
        let ramp = |key: i32, sub: u8| tiles.is_ramp(100 + key - 1, sub);
        for (key, sub) in [
            (1, 8),
            (2, 8),
            (5, 12),
            (6, 12),
            (9, 4),
            (12, 4),
            (13, 2),
            (16, 2),
        ] {
            assert!(ramp(key, sub), "key {key} at sub-tile {sub}");
        }
        for (key, sub) in [(1, 12), (5, 8), (9, 2), (13, 4), (17, 2)] {
            assert!(!ramp(key, sub), "key {key} at sub-tile {sub}");
        }
        for key in [3, 4, 7, 8] {
            assert!((0..16).all(|sub| !ramp(key, sub)), "bottom key {key}");
        }
    }
}
