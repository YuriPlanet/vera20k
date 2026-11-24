//! Executed Map586E50 comparisons, using the native harness's supplied sparse
//! map and retained Dummy state. House lifecycle/RNG are tested by their owners.

use super::*;
use crate::map::resolved_terrain::{NativeCellQuery, test_clear_cell, test_grid};

#[derive(serde::Deserialize)]
struct ClampRow {
    xy: [i32; 2],
    bounds: [i32; 5],
    dummy: [i32; 2],
    output: [i32; 2],
    final_dummy: [i32; 2],
}

#[test]
fn native_house_playfield_clamp_matches_all_72_original_rows() {
    let provenance: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/house_cell_clamp.meta.json",
    ))
    .unwrap();
    assert_eq!(provenance["schema_version"], 1);
    let rows: Vec<ClampRow> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/house_cell_clamp.json",
    ))
    .unwrap();
    assert_eq!(rows.len(), 72);

    // Same allocations as map_queries.state, a fixture premise, not expected
    // output calculated from Rust geometry.
    let allocated = [
        (0, 0, -128_i8, 1),
        (511, 0, 127, 255),
        (0, 1, -1, 0),
        (10, 10, 0, 0),
        (40, 48, 2, 1),
        (50, 40, 7, 4),
        (80, 50, 0, 0),
    ];
    let mut terrain = test_grid(512, 51, test_clear_cell);
    let cells: Vec<_> = allocated.iter().map(|&(x, y, _, _)| (x, y)).collect();
    terrain.test_set_native_allocated_cells(&cells);
    for &(x, y, level, slope) in &allocated {
        let cell = &mut terrain.cells_mut_for_test()[usize::from(y) * 512 + usize::from(x)];
        cell.level = level as u8;
        cell.slope_type = slope;
    }

    for row in rows {
        terrain.test_set_dummy_cell_level_slope(row.dummy[0] as i8, row.dummy[1] as u8);
        terrain.stamp_dummy_cell_requested_coord(1234, -2345);
        let [base, off_fc, off_100, off_104, off_108] = row.bounds;
        let bounds = PlayfieldBounds {
            base,
            off_fc,
            off_100,
            off_104,
            off_108,
        };
        let query = NativeCellQuery::canonical(&terrain);
        let actual =
            clamp_cell_to_playfield((row.xy[0], row.xy[1]), bounds, Some(&terrain), Some(&query));
        assert_eq!(actual, (row.output[0], row.output[1]), "{:?}", row.xy);
        assert_eq!(
            terrain.dummy_cell_requested_coord(),
            (row.final_dummy[0], row.final_dummy[1]),
            "Dummy after {:?}, retained {:?}",
            row.xy,
            row.dummy,
        );
        assert_eq!(
            terrain.dummy_cell_level_slope(),
            (row.dummy[0] as i8, row.dummy[1] as u8),
        );
    }
}

#[test]
fn reservation_bits_reach_the_one_hundred_twentieth_house_without_wrapping() {
    let mut grid = CellReservationGrid::new();
    grid.reserve(None, 10, 20, 119);
    grid.reserve(None, 10, 20, 31);
    assert_eq!(
        grid.raw_mask(None, 10, 20),
        (1u128 << 119) | (1u128 << 31),
        "each House keeps its own bit past the native 32-House word"
    );
    assert!(grid.has_reservation(None, 10, 20, 119));
    // The native 32-bit word wraps bit 119 onto bit 23; the widened one must not.
    assert!(!grid.has_reservation(None, 10, 20, 23));
    grid.clear(None, 10, 20, 119);
    assert_eq!(grid.raw_mask(None, 10, 20), 1u128 << 31);
}

/// A save made before extended House ids contains a (cell, u32) map. Its
/// bytes and replay hash survive an extended reservation being set and cleared.
#[test]
fn legacy_reservation_save_and_hash_survive_extended_houses() {
    use crate::util::save_codec::{deserialize, layout, serialize};
    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(rename = "CellReservationGrid")]
    struct LegacyGrid {
        masks: BTreeMap<(u16, u16), u32>,
    }
    assert_eq!(
        layout::describe::<CellReservationGrid>().unwrap(),
        layout::describe::<LegacyGrid>().unwrap(),
    );
    let legacy = serialize(&LegacyGrid {
        masks: BTreeMap::from([((10, 20), 1 << 31)]),
    })
    .unwrap();
    let mut sim = crate::sim::world::Simulation::new();
    *sim.base_reservations_mut_for_test() = deserialize(&legacy).unwrap();
    assert!(sim.base_reservations().has_reservation(None, 10, 20, 31));
    let old_hash = sim.state_hash();
    sim.base_reservations_mut_for_test()
        .reserve(None, 10, 20, 119);
    assert_ne!(sim.state_hash(), old_hash);
    let extended = serialize(sim.base_reservations()).unwrap();
    let restored: CellReservationGrid = deserialize(&extended).unwrap();
    assert_eq!(&restored, sim.base_reservations());
    sim.base_reservations_mut_for_test()
        .clear(None, 10, 20, 119);
    assert_eq!(sim.state_hash(), old_hash);
    assert_eq!(serialize(sim.base_reservations()).unwrap(), legacy);
}
