//! Flat tile ids and tile predicates the generation phases compare against.
//!
//! The original resolves theater `[General]` tileset ordinals to first-tile
//! ids at theater load and stores them in globals; phases then test cell tile
//! indices against those bases. `TileIds` carries the resolved values, with
//! `-1` for a missing key exactly like the native globals, so range tests can
//! be ported verbatim. Cliff/impassable classification is NOT here — it is
//! the existing `TheaterCliffRanges::is_special_terrain_tile`.

use crate::map::theater::{TheaterCliffRanges, TheaterData};

/// Value the original stores in a cell's tile field for "unassigned"; treated
/// as clear ground by the clear-tile test.
pub const TILE_UNASSIGNED: i32 = 0xFFFF;

/// Span of a LAT transition set (base..base+0x10).
const LAT_SPAN: i32 = 0x10;
/// Shore-piece set span: 42 tiles, the same length the LAT pass uses for its
/// green-group shore exemption.
const SHORE_SPAN: i32 = 42;
/// WaterSet span accepted by the active water-family predicate.
const WATER_FAMILY_SPAN: i32 = 14;
/// Each of the four waterfall families contributes four matching tiles.
const WATERFALL_SPAN: i32 = 4;
/// Fixed spans of the start-placement 6x6 gate ranges.
const PAVED_ROADS_SPAN: i32 = 15;
const PAVED_ROAD_ENDS_SPAN: i32 = 4;
const MISC_PAVE_SPAN: i32 = 14;
const PAVE_SPAN: i32 = 16;

/// Resolved flat tile ids for one theater. `-1` = key absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileIds {
    pub clear: i32,
    pub ramp_base: i32,
    pub ramp_smooth: i32,
    /// Native ABC1F8, resolved from General.SlopeSetPieces by the theater owner.
    /// All connector carvers use this shared end-piece base (545E26/591448).
    pub slope_set_pieces: i32,
    pub rough: i32,
    pub sand: i32,
    pub green: i32,
    pub rough_lat: i32,
    pub sand_lat: i32,
    pub green_lat: i32,
    pub pave_lat: i32,
    pub pave: i32,
    pub water_base: i32,
    pub shore: i32,
    pub water_bridge: i32,
    pub misc_pave: i32,
    pub paved_roads: i32,
    pub paved_road_ends: i32,
    pub medians: i32,
    /// The generator's only copy of the theater owner's resolved
    /// cliff/waterfall ranges. Family decisions use TheaterCliffRanges
    /// directly, without a second classifier.
    pub cliffs: TheaterCliffRanges,
}

fn flat(id: Option<u16>) -> i32 {
    id.map_or(-1, i32::from)
}

impl TileIds {
    pub fn resolve(theater: &TheaterData) -> Self {
        let mut ids = Self::from_keys(&theater.rmg_tiles);
        ids.cliffs = theater.cliff_ranges;
        ids
    }

    pub fn from_keys(keys: &crate::map::theater::RmgTileKeys) -> Self {
        Self {
            clear: flat(keys.clear_tile),
            ramp_base: flat(keys.ramp_base),
            ramp_smooth: flat(keys.ramp_smooth),
            slope_set_pieces: flat(keys.slope_set_pieces),
            rough: flat(keys.rough_tile),
            sand: flat(keys.sand_tile),
            green: flat(keys.green_tile),
            rough_lat: flat(keys.clear_to_rough_lat),
            sand_lat: flat(keys.clear_to_sand_lat),
            green_lat: flat(keys.clear_to_green_lat),
            pave_lat: flat(keys.clear_to_pave_lat),
            pave: flat(keys.pave_tile),
            water_base: flat(keys.water_set),
            shore: flat(keys.shore_pieces),
            water_bridge: flat(keys.water_bridge),
            misc_pave: flat(keys.misc_pave_tile),
            paved_roads: flat(keys.paved_roads),
            paved_road_ends: flat(keys.paved_road_ends),
            medians: flat(keys.medians),
            // The special-terrain bases live on the cliff-ranges side of the
            // theater, not the RMG keys; `resolve` fills them in.
            cliffs: TheaterCliffRanges::default(),
        }
    }

    /// Clear-ground test: exactly tile 0 or the unassigned sentinel — NOT
    /// membership in the clear tileset.
    pub fn is_clear(&self, tile: i32) -> bool {
        tile == 0 || tile == TILE_UNASSIGNED
    }

    /// Green-terrain membership: the green base tile or its LAT range.
    pub fn is_green_lat(&self, tile: i32) -> bool {
        base_or_lat(tile, self.green, self.green_lat)
    }

    /// Sand-terrain membership: the sand base tile or its LAT range.
    #[cfg(any(test, feature = "test-support"))]
    pub fn is_sand_lat(&self, tile: i32) -> bool {
        base_or_lat(tile, self.sand, self.sand_lat)
    }

    /// Original Cell4865B0, signed ShorePieces window without an absent-base
    /// guard (original executable;
    /// tools/rules_oracle/theater_general_reader.md establishes its input).
    pub fn is_shore_piece(&self, tile: i32) -> bool {
        tile_in_native_window(tile, self.shore, SHORE_SPAN)
    }

    /// Exact tile-only predicate used by `gamemd` 0x004865D0 during region
    /// construction, island rebuilding/dilation, and low-deck validation.
    /// It deliberately ignores sub-tile: WaterSet 14, ShorePieces 42, or any
    /// of the four four-tile waterfall bands.
    pub fn is_water_shore_or_waterfall(&self, tile: i32) -> bool {
        // Original4865D0..486641 compares even a -1 base; the Lunar
        // ReadTheater postpass really publishes absent Shore/Water globals.
        self.is_shore_piece(tile)
            || tile_in_native_window(tile, self.water_base, WATER_FAMILY_SPAN)
            || self
                .cliffs
                .waterfall_bases()
                .into_iter()
                .any(|base| tile_in_native_window(tile, base, WATERFALL_SPAN))
    }

    /// Paved-road range used by the start 6x6 passability gate.
    pub fn is_paved_road(&self, tile: i32) -> bool {
        in_span(tile, self.paved_roads, PAVED_ROADS_SPAN)
    }

    /// Paved-road-ends range used by the start 6x6 passability gate.
    pub fn is_paved_road_end(&self, tile: i32) -> bool {
        in_span(tile, self.paved_road_ends, PAVED_ROAD_ENDS_SPAN)
    }

    /// Misc-pave range used by the start 6x6 passability gate.
    pub fn is_misc_pave(&self, tile: i32) -> bool {
        in_span(tile, self.misc_pave, MISC_PAVE_SPAN)
    }

    /// Pave range used by the start 6x6 passability gate.
    pub fn is_pave(&self, tile: i32) -> bool {
        in_span(tile, self.pave, PAVE_SPAN)
    }
}

fn base_or_lat(tile: i32, base: i32, lat_base: i32) -> bool {
    if base != -1 && tile == base {
        return true;
    }
    lat_base != -1 && tile >= lat_base && tile < lat_base + LAT_SPAN
}

fn in_span(tile: i32, base: i32, span: i32) -> bool {
    base != -1 && tile_in_native_window(tile, base, span)
}

fn tile_in_native_window(tile: i32, base: i32, span: i32) -> bool {
    tile >= base && tile < base.wrapping_add(span)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::theater::{RmgTileKeys, parse_tileset_ini};

    fn contract_ids() -> TileIds {
        let lookup = parse_tileset_ini(
            crate::test_fixture::bytes("tests/fixtures/ini/rmg_theater_tiles_contract.ini"),
            "tem",
        )
        .expect("parse synthetic theater contract");
        let bounds = lookup.bounds();
        let start = |ordinal: usize| Some(bounds[ordinal].start);

        // The cumulative parser-derived starts keep every predicate boundary
        // visible without making default CI depend on the private retail INI.
        let keys = RmgTileKeys {
            clear_tile: start(0),
            ramp_base: start(1),
            ramp_smooth: start(2),
            slope_set_pieces: start(3),
            rough_tile: start(2),
            clear_to_rough_lat: start(3),
            sand_tile: start(4),
            clear_to_sand_lat: start(5),
            green_tile: start(6),
            clear_to_green_lat: start(7),
            pave_tile: start(8),
            misc_pave_tile: start(9),
            clear_to_pave_lat: start(10),
            water_set: start(11),
            shore_pieces: start(12),
            water_bridge: start(13),
            paved_roads: start(14),
            paved_road_ends: start(15),
            medians: start(16),
        };
        let ids = TileIds::from_keys(&keys);
        assert_eq!(ids.clear, 0);
        assert_eq!(ids.ramp_base, 64);
        assert_eq!(ids.ramp_smooth, 128);
        assert_eq!(ids.slope_set_pieces, 192);
        assert_eq!(ids.medians, 16 * 64);
        ids
    }

    #[test]
    fn clear_test_is_exact_not_set_membership() {
        let ids = contract_ids();
        assert!(ids.is_clear(0));
        assert!(ids.is_clear(TILE_UNASSIGNED));
        assert!(!ids.is_clear(1), "tile 1 is in the clear SET but not clear");
        assert!(!ids.is_clear(ids.green));
    }

    #[test]
    fn green_lat_membership_covers_base_and_transition_range() {
        let ids = contract_ids();
        assert!(ids.is_green_lat(ids.green));
        assert!(ids.is_green_lat(ids.green_lat));
        assert!(ids.is_green_lat(ids.green_lat + 0xF));
        assert!(!ids.is_green_lat(ids.green_lat + 0x10));
        assert!(!ids.is_green_lat(0));
        assert!(!ids.is_green_lat(ids.sand));
    }

    #[test]
    fn missing_keys_keep_each_predicates_absent_base_policy() {
        let ids = TileIds {
            clear: -1,
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
            water_base: -1,
            shore: -1,
            water_bridge: -1,
            misc_pave: -1,
            paved_roads: -1,
            paved_road_ends: -1,
            medians: -1,
            cliffs: TheaterCliffRanges::default(),
        };
        // Cell4865B0/4865D0 accept the -1 windows. The other predicates
        // retain their separate guards; this is not a blanket policy change.
        assert!(!ids.is_green_lat(-1));
        assert!(ids.is_shore_piece(-1));
        assert!(ids.is_shore_piece(40));
        assert!(!ids.is_shore_piece(41));
        assert!(ids.is_water_shore_or_waterfall(0));
        assert!(!ids.is_water_shore_or_waterfall(TILE_UNASSIGNED));
        assert!(!ids.is_paved_road(0));
        assert!(ids.is_clear(TILE_UNASSIGNED));
    }

    #[test]
    fn shore_and_gate_ranges_use_fixed_spans() {
        let ids = contract_ids();
        assert!(ids.is_shore_piece(ids.shore));
        assert!(ids.is_shore_piece(ids.shore + 41));
        assert!(!ids.is_shore_piece(ids.shore + 42));
        assert!(ids.is_paved_road(ids.paved_roads + 14));
        assert!(!ids.is_paved_road(ids.paved_roads + 15));
        assert!(ids.is_paved_road_end(ids.paved_road_ends + 3));
        assert!(!ids.is_paved_road_end(ids.paved_road_ends + 4));
        assert!(ids.is_misc_pave(ids.misc_pave + 13));
        assert!(!ids.is_misc_pave(ids.misc_pave + 14));
        assert!(ids.is_pave(ids.pave + 15));
        assert!(!ids.is_pave(ids.pave + 16));
    }

    #[test]
    fn native_water_family_uses_exact_tile_only_boundaries() {
        let mut ids = contract_ids();
        ids.cliffs = TheaterCliffRanges {
            waterfall_north: Some(2_000),
            waterfall_east: Some(2_100),
            waterfall_south: Some(2_200),
            waterfall_west: Some(2_300),
            ..TheaterCliffRanges::default()
        };

        for (base, span) in [
            (ids.water_base, 14),
            (ids.shore, 42),
            (2_000, 4),
            (2_100, 4),
            (2_200, 4),
            (2_300, 4),
        ] {
            assert!(!ids.is_water_shore_or_waterfall(base - 1));
            assert!(ids.is_water_shore_or_waterfall(base));
            assert!(ids.is_water_shore_or_waterfall(base + span - 1));
            assert!(!ids.is_water_shore_or_waterfall(base + span));
        }
        assert!(ids.is_water_shore_or_waterfall(ids.water_base + 13));
        assert!(!ids.is_water_shore_or_waterfall(flat(ids.cliffs.bridge_set)));
        assert!(!ids.is_water_shore_or_waterfall(flat(ids.cliffs.wood_bridge_set)));
    }
}
