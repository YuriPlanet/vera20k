//! Native-derived complete587410 AL/Dummy comparisons on the production grid.
use super::*;
use crate::map::resolved_terrain::{install_bridge_query_test_catalog, test_flat_cell, test_grid};
use crate::sim::bridge_state::BridgeRecordKind;
use serde_json::Value;
use std::collections::BTreeMap;

fn point(value: &Value) -> (i16, i16) {
    (
        value[0].as_i64().unwrap() as i16,
        value[1].as_i64().unwrap() as i16,
    )
}

#[test]
fn complete_native_query_corpus_matches_al_and_retained_dummy() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_repair_query.json",
    ))
    .unwrap();
    let rows = corpus["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 188);
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let cells: BTreeMap<_, _> = input["cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|cell| (point(cell), cell))
            .collect();
        let mut terrain = test_grid(512, 106, |x, y| {
            let mut cell = test_flat_cell(x, y);
            if let Some(r) = cells.get(&(x as i16, y as i16)) {
                cell.final_tile_index = r[2].as_i64().unwrap() as i32;
                cell.final_sub_tile = r[3].as_u64().unwrap() as u8;
                cell.bridge_facts.overlay_id = r[5].as_u64().map(|v| v as u8);
            }
            cell
        });
        let allocated: Vec<_> = cells.keys().map(|&(x, y)| (x as u16, y as u16)).collect();
        terrain.test_set_native_allocated_cells(&allocated);
        terrain.test_set_high_bridge_set_starts(
            Some(input["concrete"].as_i64().unwrap() as u16),
            Some(input["wood"].as_i64().unwrap() as u16),
        );
        let tiles: Vec<_> = cells
            .values()
            .map(|r| r[2].as_u64().unwrap() as u16)
            .collect();
        install_bridge_query_test_catalog(
            &mut terrain,
            input["width"].as_u64().unwrap() as u8,
            &tiles,
        );
        let records: Vec<_> = input["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                let (a, b) = (point(&r[0]), point(&r[1]));
                BridgeEndpointRecord {
                    endpoint_a: (a.0 as u16, a.1 as u16),
                    endpoint_b: (b.0 as u16, b.1 as u16),
                    active: r[2].as_bool().unwrap(),
                    bridge_kind: if r[3] == 0 {
                        BridgeRecordKind::High
                    } else {
                        BridgeRecordKind::Low
                    },
                }
            })
            .collect();
        let query = NativeCellQuery::canonical(&terrain);
        query
            .dummy()
            .write_overlay_identity_state(input["dummy_overlay"].as_i64().unwrap() as i32, 0);
        assert_eq!(
            can_repair(&query, &records, point(&input["center"])),
            row["result_al"] == 1,
            "{name}: native AL"
        );
        assert_eq!(
            query.coord(NativeCellIdentity::Dummy),
            point(&row["dummy_coord"]),
            "{name}: retained Dummy"
        );
        assert!(
            row["cells_and_records_unchanged"].as_bool().unwrap(),
            "{name}"
        );
    }
}
