//! Original successful-search finishing comparisons. Concrete Unit entry
//! results are explicit transcript seams; these tests establish the finishing
//! consumer and its argument/query order, not another Unit entry implementation.

use super::*;
use crate::util::native_x87::NativeF64Bits;
use serde_json::{Value, json};
use std::cell::RefCell;

struct TranscriptContext<'a> {
    row: &'a Value,
    entries: Vec<&'a Value>,
    next_entry: RefCell<usize>,
    events: RefCell<Vec<Value>>,
    dummy_coord: RefCell<PackedCell>,
}

#[derive(Clone, Copy, Debug)]
enum FixtureCell {
    Real(PackedCell),
    Dummy,
}

fn coord(value: &Value) -> PackedCell {
    (
        value[0].as_i64().unwrap() as i16,
        value[1].as_i64().unwrap() as i16,
    )
}

fn outer_event(event: &Value) -> Option<Value> {
    let caller = event["caller"].as_str().unwrap();
    match event["pc"].as_str().unwrap() {
        "005657a0" if caller.starts_with("0042b") || caller.starts_with("0042c") => {
            Some(json!(["cell", event["xy"]]))
        }
        "0073f0a0" => Some(json!([
            "entry",
            event["xy"],
            event["direction"],
            event["height"],
            event["previous"],
            event["arg5"]
        ])),
        "0056bcd0" => Some(json!(["threat", event["xy"]])),
        _ => None,
    }
}

impl TranscriptContext<'_> {
    fn cell_row(&self, cell: PackedCell) -> Option<&Value> {
        self.row["input"]["cells"]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| coord(r) == cell))
    }

    fn cell_coord(&self, cell: FixtureCell) -> PackedCell {
        match cell {
            FixtureCell::Real(coord) => coord,
            FixtureCell::Dummy => *self.dummy_coord.borrow(),
        }
    }

    // This is the supplied sparse table, not a second Unit admission port.
    // Native packed lookup indexes signed words in a 512-stride linear table;
    // missing queries stamp the same retained Dummy object.
    fn lookup(&self, requested: PackedCell) -> FixtureCell {
        let index = i32::from(requested.1) * 512 + i32::from(requested.0);
        if (0..0x40000).contains(&index) {
            let allocated = ((index % 512) as i16, (index / 512) as i16);
            let missing = self.row["input"]["missing_cells"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|r| coord(r) == allocated));
            if allocated.0 < 32 && allocated.1 < 32 && !missing {
                return FixtureCell::Real(allocated);
            }
        }
        *self.dummy_coord.borrow_mut() = requested;
        FixtureCell::Dummy
    }
}

impl PathFinishingContext for TranscriptContext<'_> {
    type Cell = FixtureCell;

    fn get_cell(&self, cell: PackedCell) -> Self::Cell {
        self.events.borrow_mut().push(json!(["cell", cell]));
        self.lookup(cell)
    }

    fn ground_level(&self, cell: Self::Cell) -> i32 {
        match cell {
            FixtureCell::Real(coord) => self
                .cell_row(coord)
                .map_or(0, |r| r[2].as_i64().unwrap() as u8 as i8 as i32),
            FixtureCell::Dummy => {
                self.row["input"]["dummy"]["level"].as_i64().unwrap_or(0) as u8 as i8 as i32
            }
        }
    }

    fn flags(&self, cell: Self::Cell) -> u32 {
        match cell {
            FixtureCell::Real(coord) => self
                .cell_row(coord)
                .map_or(0, |r| r[3].as_u64().unwrap() as u32),
            FixtureCell::Dummy => self.row["input"]["dummy"]["flags"].as_u64().unwrap_or(0) as u32,
        }
    }

    fn can_enter(&self, cell: Self::Cell, direction: i32, height: i32) -> Result<u8, String> {
        let mut next = self.next_entry.borrow_mut();
        let event = self
            .entries
            .get(*next)
            .ok_or_else(|| format!("unexpected extra entry {cell:?}/{direction}/{height}"))?;
        let actual = json!(["entry", self.cell_coord(cell), direction, height, 0, 1]);
        assert_eq!(
            Some(actual.clone()),
            outer_event(event),
            "{} entry{}",
            self.row["input"]["name"],
            *next
        );
        self.events.borrow_mut().push(actual);
        if !event["before"].is_null() {
            assert_eq!(
                event["before"]["identity"],
                match cell {
                    FixtureCell::Real(_) => "real",
                    FixtureCell::Dummy => "dummy",
                }
            );
            assert_eq!(event["before"]["level"], self.ground_level(cell));
            assert_eq!(event["before"]["flags"], self.flags(cell));
            // Original Unit entry may perform nested Map queries. Its returned
            // code and Dummy mutation are both explicit original-body seams.
            *self.dummy_coord.borrow_mut() = coord(&event["after"]["dummy_coord"]);
        }
        *next += 1;
        Ok(event["returned"].as_u64().unwrap() as u8)
    }

    fn house_threat(&self, cell: PackedCell) -> Result<i32, String> {
        self.events.borrow_mut().push(json!(["threat", cell]));
        self.lookup(cell); //56BCD0 inlines lookup; no outer5657A0 event.
        Ok(self.row["input"]["threat"].as_i64().unwrap_or(0) as i32)
    }

    fn threat_coefficient(&self) -> Result<NativeF64Bits, String> {
        if let Some(bits) = self.row["input"]["coefficient_bits"].as_str() {
            return Ok(NativeF64Bits::from_bits(
                u64::from_str_radix(bits, 16).unwrap(),
            ));
        }
        Ok(NativeF64Bits::from_bits(
            self.row["input"]["coefficient"]
                .as_f64()
                .unwrap_or(0.0)
                .to_bits(),
        ))
    }

    fn tube_exit(&self, cell: Self::Cell) -> Option<PackedCell> {
        let FixtureCell::Real(cell) = cell else {
            return None;
        };
        self.row["input"]["tubes"]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| coord(&r["cell"]) == cell))
            .map(|r| coord(&r["exit"]))
    }
}

#[test]
fn successful_search_finishing_matches_original_caller_transcript() {
    let packet: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/astar_path_finishing.json",
    ))
    .unwrap();
    assert_eq!(
        packet["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    let cases = packet["cases"].as_array().unwrap();
    assert!(cases.len() >= 51);
    for row in cases {
        let snapshots = row["snapshots"].as_array().unwrap();
        let before = &snapshots[0];
        let after_corners = &snapshots[1];
        let after_straight = &snapshots[2];
        let context = TranscriptContext {
            row,
            entries: row["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["pc"] == "0073f0a0")
                .collect(),
            next_entry: RefCell::new(0),
            events: RefCell::new(Vec::new()),
            dummy_coord: RefCell::new((0, 0)),
        };
        let mut directions: Vec<i32> = before["directions"]
            .as_array()
            .unwrap()
            .iter()
            .take(before["count"].as_u64().unwrap() as usize - 1)
            .map(|d| d.as_i64().unwrap() as i32)
            .collect();
        let heights: Vec<i32> = before["retained_heights"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h.as_i64().unwrap() as i32)
            .collect();
        let start = coord(&before["start"]);
        smooth_corners(start, &mut directions, &heights, &context).unwrap();
        let corner_expected: Vec<i32> = after_corners["directions"]
            .as_array()
            .unwrap()
            .iter()
            .take(after_corners["count"].as_u64().unwrap() as usize - 1)
            .map(|d| d.as_i64().unwrap() as i32)
            .collect();
        assert_eq!(
            directions, corner_expected,
            "{} corners",
            row["input"]["name"]
        );
        assert_eq!(
            before["retained_heights"],
            after_corners["retained_heights"]
        );
        optimize_straight_segments(start, &mut directions, &heights, &context).unwrap();
        let straight_expected: Vec<i32> = after_straight["directions"]
            .as_array()
            .unwrap()
            .iter()
            .take(after_straight["count"].as_u64().unwrap() as usize - 1)
            .map(|d| d.as_i64().unwrap() as i32)
            .collect();
        assert_eq!(
            directions, straight_expected,
            "{} straight",
            row["input"]["name"]
        );
        assert_eq!(
            before["retained_heights"],
            after_straight["retained_heights"]
        );
        let expected_events: Vec<Value> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(outer_event)
            .collect();
        assert_eq!(
            *context.events.borrow(),
            expected_events,
            "{} outer queries",
            row["input"]["name"]
        );
        assert_eq!(*context.next_entry.borrow(), context.entries.len());
        if row["input"]["extended"] == true {
            assert_eq!(
                json!(*context.dummy_coord.borrow()),
                row["dummy_coord"],
                "{} retained Dummy",
                row["input"]["name"]
            );
        }
    }
}
