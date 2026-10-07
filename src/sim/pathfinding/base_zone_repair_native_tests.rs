// Original56D460/56D5A0 executable outputs; fallback goldens stop at56C510.
// Production fallback itself is separately covered by bridge_base_native_tests.
use super::super::zone_map_tests::terrain_from_zone_classes;
use super::*;
use crate::rules::locomotor_type::MovementZone;

#[test]
fn retained_base_repair_matches_native_selectors_and_adoption_without_live_publication() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/base_zone_repair.json",
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 28);
    for case in cases {
        let input = &case["input"];
        let label = input["name"].as_str().unwrap();
        let query = (
            input["query"][0].as_i64().unwrap() as i16,
            input["query"][1].as_i64().unwrap() as i16,
        );
        let size = (
            input["size"][0].as_i64().unwrap() as i32,
            input["size"][1].as_i64().unwrap() as i32,
        );
        let width = (size.0 + size.1) as u16;
        let side = i32::from(width) + 1;
        let (x, y) = super::super::zone_build::native_zone_grid_position(size, query).unwrap();
        assert_eq!(serde_json::json!([x, y]), case["canonical"], "{label}");
        let mut classes = vec![zone_class::OUTSIDE; usize::from(width).pow(2)];
        let mut levels = vec![0; classes.len()];
        let mut ids = vec![0; classes.len()];
        let target = y * side + x;
        let mut put = |native: i32, values: &serde_json::Value| {
            let (nx, ny) = (native % side, native / side);
            if nx >= i32::from(width) || ny >= i32::from(width) {
                return;
            }
            let i = (ny * i32::from(width) + nx) as usize;
            classes[i] = values[0].as_u64().unwrap() as u8;
            levels[i] = values[1].as_u64().unwrap() as u8;
            ids[i] = values[2].as_u64().unwrap() as u16;
        };
        put(target, &input["target"]);
        for ((dx, dy), value) in crate::util::direction_tables::CELL_DELTAS
            .iter()
            .zip(input["neighbors"].as_array().unwrap())
        {
            put(target + dy * side + dx, value);
        }
        let mut terrain = terrain_from_zone_classes(width, width, &classes, &levels);
        let path = PathGrid::from_resolved_terrain(&terrain);
        let mut zones = ZoneGrid::build_with_native_bridge_geometry(
            &path,
            &terrain,
            &[],
            width,
            width,
            Some(size),
        );
        let base = zones.base_topology_mut();
        base.zone_ids = ids.clone();
        let row0: Vec<_> = input["row0"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_u64().unwrap() as u16)
            .collect();
        for row in &mut base.raw_zone_ids_by_row {
            *row = row0.clone();
        }
        // Rebuild only the flat projections for the supplied retained IDs.
        // Native fixture supplies those IDs and rows, not a56C510 invocation.
        let base = base.clone();
        for &movement in MovementZone::all_ground() {
            let map = super::super::zone_build::build_zone_map_from_base_topology(
                &base, movement, width, width,
            );
            *zones.map_mut(movement).unwrap() = map;
        }
        if let Some(cell) = terrain.cell_mut(x as u16, y as u16) {
            cell.level = 20;
            cell.zone_type = zone_class::OUTSIDE;
            cell.outside_playfield = true;
        }
        let before_dummy = terrain.shared_cell_dummy().snapshot();
        let kind = if input["kind"] == "assign" {
            ZoneRepairKind::AssignOrphaned
        } else {
            ZoneRepairKind::MergeAdjacent
        };
        let outcome = repair_zone_cell(
            &mut zones,
            PackedZoneCoord::new(query.0, query.1),
            kind,
            &path,
            None,
            &terrain,
            &[],
        );
        let after = zones.base_topology_mut();
        assert_eq!(after.movement_classes, classes, "{label}: retained classes");
        assert_eq!(after.levels, levels, "{label}: retained heights");
        assert_eq!(
            terrain.shared_cell_dummy().snapshot(),
            before_dummy,
            "{label}: no lookup"
        );
        if case["fallback"].as_bool().unwrap() {
            assert_eq!(outcome, ZoneRepairOutcome::FullRebuild, "{label}");
            continue; // Native golden stops before the separate full rebuild.
        }
        let cluster = case["target_after"][2].as_u64().unwrap() as u16;
        let expected = if input["target"][0] == zone_class::OUTSIDE {
            ZoneRepairOutcome::SentinelNoOp
        } else {
            ZoneRepairOutcome::Adopted { cluster }
        };
        assert_eq!(outcome, expected, "{label}");
        for row in &after.raw_zone_ids_by_row {
            assert_eq!(row, &row0, "{label}");
        }
        if let Some(i) = projected_zone_record_index(width, width, Some(size), query) {
            ids[i] = cluster;
        }
        assert_eq!(
            after.zone_ids, ids,
            "{label}: only canonical target may adopt"
        );
    }
}
