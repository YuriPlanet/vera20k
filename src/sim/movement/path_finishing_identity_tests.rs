//! Production boundary replay of original finishing +1AC receipts. The class
//! result and nested Dummy mutations are supplied native prestate/effects;
//! this checks pointer forwarding, not execution of a whole native route.
use super::*;
use crate::sim::pathfinding::{SearchEntryCandidate, SearchEntryQuery};
use serde_json::Value;

fn coord(value: &Value) -> (i16, i16) {
    (
        value[0].as_i64().unwrap() as i16,
        value[1].as_i64().unwrap() as i16,
    )
}

struct NativeReceiptEntry<'a> {
    cells: NativeCellQuery<'a>,
    receipt: &'a Value,
}

impl SearchFootEntry for NativeReceiptEntry<'_> {
    fn classify(&self, query: SearchEntryQuery) -> Result<u8, String> {
        // Native42B5AD/42BFB3/42C0B7 push the retained EDI Cell; none
        // performs another5657A0 lookup between the pointer and the virtual.
        let SearchEntryCandidate::RetainedNativeCell(candidate) = query.candidate else {
            panic!("finishing copied the retained Cell into a lookup coordinate");
        };
        assert_eq!(query.from, None);
        assert_eq!(self.receipt["previous"], 0);
        assert_eq!(self.receipt["arg5"], 1);
        assert_eq!(
            query.direction,
            self.receipt["direction"].as_i64().unwrap() as i32
        );
        assert_eq!(
            query.path_height,
            self.receipt["height"].as_i64().unwrap() as i32
        );
        assert_eq!(
            self.cells.coord(candidate),
            coord(&self.receipt["before"]["coord"])
        );
        assert_eq!(
            candidate == NativeCellIdentity::Dummy,
            self.receipt["before"]["identity"] == "dummy"
        );
        if candidate == NativeCellIdentity::Dummy {
            // These changes were observed inside the original Unit entry
            // virtual. The caller must still read this same Dummy afterwards.
            let after = &self.receipt["after"];
            let changed = coord(&after["coord"]);
            let dummy = self.cells.dummy();
            dummy.stamp_coord(i32::from(changed.0), i32::from(changed.1));
            dummy.set_level(after["level"].as_i64().unwrap() as i8);
            dummy.write_raw_flags(after["flags"].as_u64().unwrap() as u32);
        }
        Ok(self.receipt["returned"].as_u64().unwrap() as u8)
    }
}

#[test]
fn production_finisher_forwards_native_real_and_mutable_dummy_receipts() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/astar_path_finishing.json",
    ))
    .unwrap();
    let mut callers = BTreeSet::new();
    let mut changed_dummy = 0;
    for case in corpus["cases"].as_array().unwrap() {
        for receipt in case["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event.get("before").is_some())
        {
            let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(32);
            let before = &receipt["before"];
            let candidate = terrain.native_cell_identity(coord(&before["coord"]));
            terrain.write_native_cell_flags(candidate, before["flags"].as_u64().unwrap() as u32);
            terrain.write_native_cell_level(candidate, before["level"].as_i64().unwrap() as u8);
            let entry = NativeReceiptEntry {
                cells: NativeCellQuery::canonical(&terrain),
                receipt,
            };
            let grid = PathGrid::new(32, 32);
            let finishing = Finishing {
                cells: Some(NativeCellQuery::canonical(&terrain)),
                grid: &grid,
                context: PathfindingContext {
                    path_grid: Some(&grid),
                    zone_grid: None,
                    resolved_terrain: Some(&terrain),
                    playfield_bounds: None,
                    blocker_neighbor_counts: None,
                    wall_tables: None,
                },
                foot: Some(&entry),
                terrain_costs: None,
                movement_zone: None,
                facts: MoverPathFacts {
                    urgency: 0,
                    mover_is_crusher: false,
                    is_infantry: false,
                    speed_type: None,
                    owner: None,
                    is_armed: false,
                    warhead_wall: false,
                    warhead_wood: false,
                    slave_deposit_cells: [None; 2],
                },
                ground_blocks: None,
                bridge_blocks: None,
                entity_blocks: None,
                markers: None,
            };
            let cell = Cell::Native(candidate);
            assert_eq!(
                finishing
                    .can_enter(
                        cell,
                        receipt["direction"].as_i64().unwrap() as i32,
                        receipt["height"].as_i64().unwrap() as i32
                    )
                    .unwrap(),
                receipt["returned"].as_u64().unwrap() as u8,
                "{}",
                case["input"]["name"]
            );
            assert_eq!(finishing.coord(cell), coord(&receipt["after"]["coord"]));
            assert_eq!(
                finishing.flags(cell),
                receipt["after"]["flags"].as_u64().unwrap() as u32
            );
            assert_eq!(
                finishing.ground_level(cell),
                receipt["after"]["level"].as_i64().unwrap() as i32
            );
            callers.insert(receipt["caller"].as_str().unwrap());
            if before["identity"] == "dummy" && before["coord"] != receipt["after"]["coord"] {
                changed_dummy += 1;
            }
        }
    }
    assert_eq!(
        callers,
        BTreeSet::from(["0042b5b4", "0042bfbc", "0042c0c0"])
    );
    assert!(
        changed_dummy > 0,
        "native nested-lookup ordering control must remain covered"
    );
}
