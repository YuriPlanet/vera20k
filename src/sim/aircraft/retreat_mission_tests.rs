//! Replays `tools/spatial_oracle/aircraft_idle_retreat.json`'s
//! `mission_retreat` rows: the original `0x00415A50` per NavCom, `Edge=`,
//! waypoint edge and picked cell.

use super::*;
use serde_json::{Value, json};

fn rows() -> Vec<Value> {
    let oracle: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/aircraft_idle_retreat.json",
    ))
    .expect("aircraft_idle_retreat.json");
    oracle["mission_retreat"].as_array().unwrap().clone()
}

fn edge_index(edge: Edge) -> u8 {
    match edge {
        Edge::North => 0,
        Edge::East => 1,
        Edge::South => 2,
        Edge::West => 3,
    }
}

/// Logs PickCellOnEdge and the destination as the oracle's hooks do;
/// the pick answers the row's cell (its words as the map stores them).
struct Recorder {
    pick: (u16, u16),
    calls: Vec<Value>,
}

impl RetreatHost for Recorder {
    fn edge_cell(&mut self, edge: Edge) -> Option<(u16, u16)> {
        self.calls.push(json!([
            "pick_cell_on_edge",
            edge_index(edge),
            "empty_cell",
            "empty_cell",
            4,
            1,
            0
        ]));
        Some(self.pick)
    }

    fn assign_destination(&mut self, cell: Option<(u16, u16)>) {
        let cell = cell.map(|(x, y)| json!(["cell", x as i16, y as i16]));
        if let Some(cell) = &cell {
            self.calls.push(json!(["map_cell", [cell[1], cell[2]]]));
        }
        self.calls.push(json!(["assign_destination", cell, 1]));
    }
}

/// Every row: the call log and the returned delay. GetCell (`vt+0x1BC`)
/// is the NavCom's classification, which the oracle logs as `get_cell`.
#[test]
fn mission_retreat_matches_the_original() {
    let rows = rows();
    assert_eq!(rows.len(), 51);
    for row in &rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let nav_com = match &input["nav"] {
            Value::Null => RetreatNavCom::None,
            Value::String(kind) if kind == "own" => RetreatNavCom::OwnCell,
            nav if *nav == input["own"] => RetreatNavCom::OwnCell,
            _ => RetreatNavCom::Elsewhere,
        };
        let pick = input["pick"].as_array().unwrap();
        let word = |n: usize| pick[n].as_i64().unwrap() as i16 as u16;
        let mut host = Recorder {
            pick: (word(0), word(1)),
            calls: Vec::new(),
        };
        let delay = retreat_visit(
            nav_com,
            input["house_edge"].as_i64().unwrap() as i32,
            input["waypoint_edge"].as_i64().unwrap() as u8,
            &mut host,
        );
        let expected: Vec<Value> = row["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| call[0] != "get_cell")
            .cloned()
            .collect();
        assert_eq!(host.calls, expected, "{name}: calls");
        assert_eq!(i64::from(delay), row["ret"].as_i64().unwrap(), "{name}");
        // GetCell is read exactly when a NavCom is set.
        let reads_cell = row["calls"][0][0] == "get_cell";
        assert_eq!(reads_cell, nav_com != RetreatNavCom::None, "{name}");
    }
}
