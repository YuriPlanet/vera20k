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
    assert_eq!(
        provenance["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
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
        let cell = &mut terrain.cells[usize::from(y) * 512 + usize::from(x)];
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
