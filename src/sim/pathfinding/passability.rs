//! Cell passability matrix - the 13x8 movement-zone x reduced-ZoneType table.
//!
//! Extracted from the original engine (416 bytes = 13 x 8 x 4).
//! The zone flood-fill and pathfinder use this matrix to determine whether
//! a cell's reduced ZoneType is passable for a given MovementZone row.
//!
//! ## How it works
//! The binary matrix columns are reduced `ZoneType` values written by
//! `CellClass::RecalcZoneType`, not raw TMP `LandType` bytes.
//! Native direct readers use the unit's **MovementZone** row, not SpeedType.
//! The matrix lookup `MOVEMENT_ZONE_PASSABILITY[movement_zone][reduced_zone_type]` returns:
//! - 1 = passable
//! - 2 = blocked
//! - 3 = the Outside/sentinel value (also returned for an invalid lookup)
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/locomotor_type (MovementZone).
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::rules::locomotor_type::MovementZone;

pub use crate::rules::terrain_rules::{LandType, tmp_terrain_to_land_type};

/// Passability values from the matrix.
pub const PASS_OK: u8 = 1;
pub const PASS_BLOCKED: u8 = 2;
pub const PASS_OUTSIDE_SENTINEL: u8 = 3;

/// Number of zone layers (rows) in the matrix.
pub const ZONE_LAYER_COUNT: usize = 13;

/// Number of reduced ZoneType columns in the binary matrix.
pub const TERRAIN_TYPE_COUNT: usize = 8;

/// Verified native 13x8 passability matrix at `0x0082A594`.
///
/// Rows = MovementZone index (0-12). Binary columns are reduced ZoneType values
/// from `CellClass::RecalcZoneType`:
/// 0=Ground, 1=Crushable, 2=Wall, 3=Beach, 4=Water, 5=Building,
/// 6=Impassable, 7=Outside.
/// Values: 1 = passable, 2 = blocked, 3 = Outside/sentinel.
///
/// Do not label column 1 as road or crate; the verified writer uses overlay
/// `Crushable=yes`.
pub const MOVEMENT_ZONE_PASSABILITY: [[u8; TERRAIN_TYPE_COUNT]; ZONE_LAYER_COUNT] = [
    // Reduced ZoneType:             Gnd Crs Wal Bch Wtr Bld Imp Out
    // Row  0 Normal:
    [1, 2, 2, 2, 2, 2, 2, 3],
    // Row  1 Crusher:
    [1, 1, 2, 2, 2, 2, 2, 3],
    // Row  2 Destroyer:
    [1, 1, 1, 2, 2, 2, 2, 3],
    // Row  3 AmphibiousDestroyer:
    [1, 1, 1, 1, 1, 1, 2, 3],
    // Row  4 AmphibiousCrusher:
    [1, 1, 2, 1, 1, 2, 2, 3],
    // Row  5 Amphibious:
    [1, 2, 2, 1, 1, 2, 2, 3],
    // Row  6 Subterranean:
    [1, 1, 1, 2, 2, 2, 1, 3],
    // Row  7 Infantry:
    [1, 2, 2, 2, 2, 1, 2, 3],
    // Row  8 InfantryDestroyer:
    [1, 1, 1, 2, 2, 1, 2, 3],
    // Row  9 Fly:
    [1, 1, 1, 1, 1, 1, 1, 3],
    // Row 10 Water:
    [2, 2, 2, 2, 1, 2, 2, 3],
    // Row 11 WaterBeach:
    [2, 2, 2, 1, 1, 2, 2, 3],
    // Row 12 CrusherAll:
    [1, 1, 1, 2, 2, 2, 2, 3],
];

/// Check if a reduced ZoneType is passable for a given MovementZone.
///
/// Used by the zone flood-fill to partition the map into connectivity regions.
pub fn is_passable_for_zone(reduced_zone_type: u8, mz: MovementZone) -> bool {
    if reduced_zone_type as usize >= TERRAIN_TYPE_COUNT {
        return false;
    }
    let Some(layer) = mz.matrix_row() else {
        return false;
    };
    MOVEMENT_ZONE_PASSABILITY[layer][reduced_zone_type as usize] == PASS_OK
}

/// Get the raw passability value (1/2/3) for a row and reduced ZoneType.
///
/// Returns `PASS_OUTSIDE_SENTINEL` for out-of-bounds inputs.
#[cfg(test)]
pub fn passability_value(zone_layer: usize, reduced_zone_type: u8) -> u8 {
    if zone_layer >= ZONE_LAYER_COUNT || reduced_zone_type as usize >= TERRAIN_TYPE_COUNT {
        return PASS_OUTSIDE_SENTINEL;
    }
    MOVEMENT_ZONE_PASSABILITY[zone_layer][reduced_zone_type as usize]
}

/// Fold two movement rows using the TeamType post-load selector at
/// `gamemd.exe 0x005889F0`.
///
/// Retail scores all 13 candidate rows in numeric order. A candidate is
/// rejected when it admits a column (`1`) that either input blocks (`2`), and
/// otherwise scores only columns admitted by both inputs and the candidate.
/// Score-zero candidates never win and equal scores retain the lower row.
/// The active binary's pre-table words make row `-1` absorbing; preserve that
/// result explicitly instead of reproducing its out-of-bounds read.
pub fn combine_team_movement_zones(first: MovementZone, second: MovementZone) -> MovementZone {
    let (Some(first_row), Some(second_row)) = (first.matrix_row(), second.matrix_row()) else {
        return MovementZone::Invalid;
    };
    let first_values = MOVEMENT_ZONE_PASSABILITY[first_row];
    let second_values = MOVEMENT_ZONE_PASSABILITY[second_row];
    let mut best = MovementZone::Invalid;
    let mut best_score = 0_u8;

    for (candidate_index, candidate_values) in MOVEMENT_ZONE_PASSABILITY.iter().enumerate() {
        let mut valid = true;
        let mut score = 0_u8;
        for column in 0..TERRAIN_TYPE_COUNT {
            let candidate = candidate_values[column];
            let first = first_values[column];
            let second = second_values[column];
            if (first == PASS_BLOCKED || second == PASS_BLOCKED) && candidate == PASS_OK {
                valid = false;
                break;
            }
            if first == PASS_OK && second == PASS_OK && candidate == PASS_OK {
                score += 1;
            }
        }
        if valid && score > best_score {
            best_score = score;
            best = MovementZone::all_ground()[candidate_index];
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERIFIED_NATIVE_ROWS: [[u8; TERRAIN_TYPE_COUNT]; ZONE_LAYER_COUNT] = [
        [1, 2, 2, 2, 2, 2, 2, 3],
        [1, 1, 2, 2, 2, 2, 2, 3],
        [1, 1, 1, 2, 2, 2, 2, 3],
        [1, 1, 1, 1, 1, 1, 2, 3],
        [1, 1, 2, 1, 1, 2, 2, 3],
        [1, 2, 2, 1, 1, 2, 2, 3],
        [1, 1, 1, 2, 2, 2, 1, 3],
        [1, 2, 2, 2, 2, 1, 2, 3],
        [1, 1, 1, 2, 2, 1, 2, 3],
        [1, 1, 1, 1, 1, 1, 1, 3],
        [2, 2, 2, 2, 1, 2, 2, 3],
        [2, 2, 2, 1, 1, 2, 2, 3],
        [1, 1, 1, 2, 2, 2, 2, 3],
    ];

    #[test]
    fn gsi_04_04_matrix_matches_verified_native_dump() {
        assert_eq!(MOVEMENT_ZONE_PASSABILITY, VERIFIED_NATIVE_ROWS);
    }

    #[test]
    fn clear_passable_for_all_ground() {
        // Terrain type 0 (Clear) should be passable for all non-water zone layers.
        for layer in 0..10 {
            assert_eq!(
                MOVEMENT_ZONE_PASSABILITY[layer][0], PASS_OK,
                "Zone layer {} should pass on Clear terrain",
                layer
            );
        }
    }

    #[test]
    fn gsi_04_04_impassable_zone_type_retains_row_specific_one_or_two() {
        // Reduced ZoneType 6 is the native Impassable column.
        // Subterranean (row 6) and Fly (row 9) can enter; all others blocked.
        for layer in 0..ZONE_LAYER_COUNT {
            let expected = if layer == 6 || layer == 9 {
                PASS_OK
            } else {
                PASS_BLOCKED
            };
            assert_eq!(
                MOVEMENT_ZONE_PASSABILITY[layer][6], expected,
                "Zone layer {} on Impassable ZoneType",
                layer
            );
        }
    }

    #[test]
    fn gsi_04_04_raw_three_is_only_outside_and_oob_sentinel() {
        for layer in 0..ZONE_LAYER_COUNT {
            assert_eq!(
                MOVEMENT_ZONE_PASSABILITY[layer][7], PASS_OUTSIDE_SENTINEL,
                "Zone layer {} on Outside ZoneType",
                layer
            );
            assert_ne!(MOVEMENT_ZONE_PASSABILITY[layer][6], PASS_OUTSIDE_SENTINEL);
        }
        assert_eq!(
            passability_value(ZONE_LAYER_COUNT, 0),
            PASS_OUTSIDE_SENTINEL
        );
        assert_eq!(
            passability_value(0, TERRAIN_TYPE_COUNT as u8),
            PASS_OUTSIDE_SENTINEL
        );
    }

    #[test]
    fn water_only_for_ships() {
        // Zone 10 (ships) should only pass on water (col 4).
        let row = MOVEMENT_ZONE_PASSABILITY[10];
        assert_eq!(row[4], PASS_OK);
        assert_eq!(row[0], PASS_BLOCKED); // clear = blocked for ships
        assert_eq!(row[1], PASS_BLOCKED); // crushable overlay = blocked for ships
    }

    #[test]
    fn amphibious_destroyer_passes_land_and_water() {
        // Legacy compatibility buckets; terrain-aware zoning uses reduced ZoneType.
        let row = MOVEMENT_ZONE_PASSABILITY[3];
        assert_eq!(row[0], PASS_OK); // clear
        assert_eq!(row[1], PASS_OK); // crushable overlay
        assert_eq!(row[2], PASS_OK); // wall
        assert_eq!(row[3], PASS_OK); // beach
        assert_eq!(row[4], PASS_OK); // water
        assert_eq!(row[5], PASS_OK); // building
        assert_eq!(row[6], PASS_BLOCKED); // impassable
        assert_eq!(row[7], PASS_OUTSIDE_SENTINEL); // outside
    }

    #[test]
    fn wheel_restricted() {
        // Row 1 (Crusher/wheel compatibility) passes ground and crushable only.
        let row = MOVEMENT_ZONE_PASSABILITY[1];
        assert_eq!(row[0], PASS_OK); // clear
        assert_eq!(row[1], PASS_OK); // crushable overlay
        assert_eq!(row[2], PASS_BLOCKED); // wall
    }

    #[test]
    fn movement_zone_water_is_zone_10() {
        assert_eq!(MovementZone::Water.matrix_row(), Some(10));
        assert!(!is_passable_for_zone(0, MovementZone::Water)); // clear blocked
        assert!(is_passable_for_zone(4, MovementZone::Water)); // water OK
    }

    #[test]
    fn movement_zone_is_direct_index() {
        // Valid MovementZone values map directly to passability matrix rows.
        assert_eq!(MovementZone::Normal.matrix_row(), Some(0));
        assert_eq!(MovementZone::Crusher.matrix_row(), Some(1));
        assert_eq!(MovementZone::Destroyer.matrix_row(), Some(2));
        assert_eq!(MovementZone::AmphibiousDestroyer.matrix_row(), Some(3));
        assert_eq!(MovementZone::AmphibiousCrusher.matrix_row(), Some(4));
        assert_eq!(MovementZone::Amphibious.matrix_row(), Some(5));
        assert_eq!(MovementZone::Subterranean.matrix_row(), Some(6));
        assert_eq!(MovementZone::Infantry.matrix_row(), Some(7));
        assert_eq!(MovementZone::InfantryDestroyer.matrix_row(), Some(8));
        assert_eq!(MovementZone::Fly.matrix_row(), Some(9));
        assert_eq!(MovementZone::CrusherAll.matrix_row(), Some(12));
    }

    #[test]
    fn invalid_movement_zone_is_not_passable() {
        assert_eq!(MovementZone::Invalid.matrix_row(), None);
        assert!(!is_passable_for_zone(0, MovementZone::Invalid));
    }

    #[test]
    fn gsi_04_05_team_type_zone_combine_uses_strict_native_score_and_ties() {
        assert_eq!(
            combine_team_movement_zones(MovementZone::Normal, MovementZone::Fly),
            MovementZone::Normal,
            "Fly is the constructor-neutral row"
        );
        assert_eq!(
            combine_team_movement_zones(MovementZone::CrusherAll, MovementZone::Fly),
            MovementZone::Destroyer,
            "identical rows 2 and 12 tie, so the lower native candidate wins"
        );
        assert_eq!(
            combine_team_movement_zones(MovementZone::Fly, MovementZone::CrusherAll),
            MovementZone::Destroyer,
            "the verified selector is symmetric for valid rows"
        );
    }

    #[test]
    fn gsi_04_05_team_type_zone_combine_matches_all_invalid_pairs() {
        let expected = [
            (0, 10),
            (0, 11),
            (1, 10),
            (1, 11),
            (2, 10),
            (2, 11),
            (6, 10),
            (6, 11),
            (7, 10),
            (7, 11),
            (8, 10),
            (8, 11),
            (10, 12),
            (11, 12),
        ];
        for first in 0..ZONE_LAYER_COUNT {
            for second in first..ZONE_LAYER_COUNT {
                let actual = combine_team_movement_zones(
                    MovementZone::all_ground()[first],
                    MovementZone::all_ground()[second],
                ) == MovementZone::Invalid;
                assert_eq!(
                    actual,
                    expected.contains(&(first, second)),
                    "invalid-pair verdict for rows {first}+{second}"
                );
            }
        }
    }

    #[test]
    fn gsi_04_05_invalid_team_type_zone_is_absorbing() {
        for &valid in MovementZone::all_ground() {
            assert_eq!(
                combine_team_movement_zones(MovementZone::Invalid, valid),
                MovementZone::Invalid
            );
            assert_eq!(
                combine_team_movement_zones(valid, MovementZone::Invalid),
                MovementZone::Invalid
            );
        }
    }

    // -- LandType mapping tests --

    #[test]
    fn tmp_clear_variants_map_to_clear() {
        for byte in [0, 13] {
            assert_eq!(
                tmp_terrain_to_land_type(byte),
                LandType::Clear,
                "TMP byte {}",
                byte
            );
        }
    }

    #[test]
    fn tmp_water_maps_to_water() {
        assert_eq!(tmp_terrain_to_land_type(9), LandType::Water);
    }

    #[test]
    fn tmp_beach_maps_to_beach() {
        assert_eq!(tmp_terrain_to_land_type(10), LandType::Beach);
    }

    #[test]
    fn tmp_road_variants_map_to_road() {
        assert_eq!(tmp_terrain_to_land_type(11), LandType::Road);
        assert_eq!(tmp_terrain_to_land_type(12), LandType::Road);
    }

    #[test]
    fn tmp_rough_maps_to_rough() {
        assert_eq!(tmp_terrain_to_land_type(14), LandType::Rough);
    }

    #[test]
    fn tmp_rock_and_cliff_map_to_rock() {
        assert_eq!(tmp_terrain_to_land_type(7), LandType::Rock);
        assert_eq!(tmp_terrain_to_land_type(8), LandType::Rock);
        assert_eq!(tmp_terrain_to_land_type(15), LandType::Rock);
    }

    #[test]
    fn tmp_tunnel_and_railroad_map_to_canonical_types() {
        assert_eq!(tmp_terrain_to_land_type(5), LandType::Tunnel);
        assert_eq!(tmp_terrain_to_land_type(6), LandType::Railroad);
    }

    #[test]
    fn tmp_unknown_bytes_default_to_clear() {
        for byte in 16..=255u8 {
            assert_eq!(
                tmp_terrain_to_land_type(byte),
                LandType::Clear,
                "TMP byte {}",
                byte
            );
        }
    }

    #[test]
    fn gsi_04_04_land_type_discriminants_match_retail_order() {
        assert_eq!(LandType::Clear.as_index(), 0);
        assert_eq!(LandType::Road.as_index(), 1);
        assert_eq!(LandType::Water.as_index(), 2);
        assert_eq!(LandType::Rock.as_index(), 3);
        assert_eq!(LandType::Wall.as_index(), 4);
        assert_eq!(LandType::Tiberium.as_index(), 5);
        assert_eq!(LandType::Beach.as_index(), 6);
        assert_eq!(LandType::Rough.as_index(), 7);
        assert_eq!(LandType::Ice.as_index(), 8);
        assert_eq!(LandType::Railroad.as_index(), 9);
        assert_eq!(LandType::Tunnel.as_index(), 10);
        assert_eq!(LandType::Weeds.as_index(), 11);
    }

    #[test]
    fn gsi_04_04_all_tmp_bytes_use_native_conversion_table() {
        let expected = [
            LandType::Clear,
            LandType::Ice,
            LandType::Ice,
            LandType::Ice,
            LandType::Ice,
            LandType::Tunnel,
            LandType::Railroad,
            LandType::Rock,
            LandType::Rock,
            LandType::Water,
            LandType::Beach,
            LandType::Road,
            LandType::Road,
            LandType::Clear,
            LandType::Rough,
            LandType::Rock,
        ];
        for (raw, expected) in expected.into_iter().enumerate() {
            assert_eq!(tmp_terrain_to_land_type(raw as u8), expected, "TMP {raw}");
        }
    }

    #[test]
    fn gsi_04_04_movement_zone_indexes_native_matrix_directly() {
        for movement_zone in MovementZone::all_ground() {
            let row = movement_zone.matrix_row().expect("valid retail row");
            for reduced_zone_type in 0..TERRAIN_TYPE_COUNT as u8 {
                assert_eq!(
                    is_passable_for_zone(reduced_zone_type, *movement_zone),
                    MOVEMENT_ZONE_PASSABILITY[row][reduced_zone_type as usize] == PASS_OK,
                );
            }
        }
    }
}
