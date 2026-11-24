use super::*;
use serde_json::{Value, json};

struct Row {
    coord: Coord,
    tile: i32,
    sub: u8,
    flags: u32,
}

struct ControlHost {
    cells: Vec<Row>,
    dummy: Row,
    predicate: Vec<Option<bool>>,
    callback: Option<(Coord, i32)>,
    lookups: usize,
    trace: Vec<Value>,
}

impl ControlHost {
    fn row(&self, index: usize) -> &Row {
        self.cells.get(index).unwrap_or(&self.dummy)
    }

    fn row_mut(&mut self, index: usize) -> &mut Row {
        self.cells.get_mut(index).unwrap_or(&mut self.dummy)
    }
}

impl PavementHost for ControlHost {
    type Cell = usize;

    fn lookup(&mut self, requested: Coord) -> usize {
        self.lookups += 1;
        if self.lookups == 3
            && let Some((coord, tile)) = self.callback.take()
        {
            self.cells
                .iter_mut()
                .find(|row| row.coord == coord)
                .unwrap()
                .tile = tile;
            self.trace
                .push(json!({"kind":"entry_write","coord":coord,"tile":tile}));
        }
        let slot = i32::from(requested.1) * 512 + i32::from(requested.0);
        if (0..0x40000).contains(&slot)
            && let Some(index) = self
                .cells
                .iter()
                .position(|row| i32::from(row.coord.1) * 512 + i32::from(row.coord.0) == slot)
        {
            index
        } else {
            self.dummy.coord = requested;
            self.cells.len()
        }
    }

    fn tile(&self, cell: usize) -> i32 {
        self.row(cell).tile
    }
    fn flags(&self, cell: usize) -> u32 {
        self.row(cell).flags
    }

    fn write_flags(&mut self, cell: usize, flags: u32) {
        self.trace
            .push(json!({"kind":"flags","coord":self.row(cell).coord,"flags":flags}));
        self.row_mut(cell).flags = flags;
    }

    fn has_damaged_data(&mut self, cell: usize) -> bool {
        let row = self.row(cell);
        let sub = row.sub;
        self.trace
            .push(json!({"kind":"gate","tile":row.tile,"sub":sub}));
        self.predicate[usize::from(sub) % self.predicate.len()].unwrap_or(false)
    }

    fn initial_screen(&mut self, _: Coord, _: usize) {
        self.trace.push(json!({"kind":"screen"}));
    }
    fn radar(&mut self, cell: usize) {
        self.trace
            .push(json!({"kind":"radar","coord":self.row(cell).coord}));
    }
}

fn coord(row: &Value) -> Coord {
    (
        row[0].as_i64().unwrap() as i16,
        row[1].as_i64().unwrap() as i16,
    )
}

#[test]
fn pavement_flags_match_original_control_and_stock_footprint() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_pavement.json",
    ))
    .unwrap();
    let stock: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_pavement_stock_inputs.json",
    ))
    .unwrap();
    let mut cases = corpus["control"].as_array().unwrap().clone();
    let stock_rows: Vec<_> = stock["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| json!([row[0], row[1], row[2], row[3], row[4]]))
        .collect();
    // The actual native stock predicate executes5471F0 over Ovrps02.urb.
    // All15 pristine entries in this stock TMP advertise damaged data.
    cases.push(json!({"name":"stock_pavement", "cells":stock_rows,
        "predicate":vec![Some(true);15], "start":corpus["stock"]["start"],
        "steps":corpus["stock"]["steps"], "second_entry_write":null}));
    for case in cases {
        let mut host = ControlHost {
            cells: case["cells"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| Row {
                    coord: coord(row),
                    tile: row[2].as_i64().unwrap() as i32,
                    sub: row[3].as_u64().unwrap() as u8,
                    flags: row[4].as_u64().unwrap() as u32,
                })
                .collect(),
            dummy: Row {
                coord: (0, 0),
                tile: 0xffff,
                sub: 0,
                flags: 0,
            },
            predicate: serde_json::from_value(case["predicate"].clone()).unwrap(),
            callback: (!case["second_entry_write"].is_null()).then(|| {
                let row = &case["second_entry_write"];
                (coord(row), row[2].as_i64().unwrap() as i32)
            }),
            lookups: 0,
            trace: Vec::new(),
        };
        for step in case["steps"].as_array().unwrap() {
            host.trace.clear();
            set_connected(&mut host, coord(&case["start"]), step["state"] == 1);
            assert_eq!(
                json!(host.trace),
                step["trace"],
                "{}: callback order",
                case["name"]
            );
            let actual: Vec<_> = host
                .cells
                .iter()
                .map(|row| json!([row.coord.0, row.coord.1, row.tile, row.flags]))
                .collect();
            assert_eq!(
                json!(actual),
                step["final"],
                "{}: current real values",
                case["name"]
            );
            assert_eq!(
                json!([host.dummy.coord.0, host.dummy.coord.1, host.dummy.flags]),
                step["dummy"],
                "{}: fallback receiver",
                case["name"]
            );
        }
    }
}
