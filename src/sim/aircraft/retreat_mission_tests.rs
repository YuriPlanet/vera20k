//! Replays `tools/spatial_oracle/aircraft_idle_retreat.json`'s
//! `mission_retreat` rows: the original `0x00415A50` per NavCom, `Edge=`,
//! waypoint edge and picked cell.

use super::*;
use crate::sim::rng::SimRng;
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
    fn retreat_edge(&mut self, mode: PhobosEdgeType, owner_edge: Edge) -> Edge {
        // Every oracle row predates the Phobos key: the type and the global
        // leave `Owner` (stock 0x415A7A), so the house's edge is used.
        assert_eq!(
            mode,
            PhobosEdgeType::Owner,
            "stock rows carry no edge override"
        );
        owner_edge
    }

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
            PhobosEdgeType::Owner,
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

/// Logs the edge the visit picks and the cell it assigns, drawing like
/// `PickEdgeCellForPlane`.
struct EdgeRecorder {
    rng: SimRng,
    picked: Vec<Edge>,
    destination: Option<Option<(u16, u16)>>,
}

impl RetreatHost for EdgeRecorder {
    fn retreat_edge(&mut self, mode: PhobosEdgeType, owner_edge: Edge) -> Edge {
        crate::sim::world::edge_cell::edge_for_mode(mode, owner_edge, &mut self.rng)
    }

    fn edge_cell(&mut self, edge: Edge) -> Option<(u16, u16)> {
        self.picked.push(edge);
        Some((edge_index(edge) as u16, 0x40))
    }

    fn assign_destination(&mut self, cell: Option<(u16, u16)>) {
        self.destination = Some(cell);
    }
}

/// Phobos `RetreatToEdge=random` (`Ext/Aircraft/Body.cpp:88-123`, hook
/// `0x415A7A`): `RandomRanged(Edge::North, Edge::West)` chooses the edge, the
/// picked cell is the destination, and `Owner` keeps the house's edge without
/// spending a draw. `Closest` is a recorded residual: it keeps the owner edge.
#[test]
fn retreat_to_edge_random_draws_the_edge_and_owner_keeps_the_house_edge() {
    for seed in [0x415A_0000, 0x415A_0001, 0x415A_0003] {
        let mut expected_rng = SimRng::new(seed);
        let drawn = edge_index_checked(expected_rng.next_range_i32_inclusive(0, 3) as u8);
        let mut host = EdgeRecorder {
            rng: SimRng::new(seed),
            picked: Vec::new(),
            destination: None,
        };
        // House edge East (1), waypoint edge ignored by the random arm.
        retreat_visit(RetreatNavCom::None, 1, 2, PhobosEdgeType::Random, &mut host);
        assert_eq!(host.picked, vec![drawn], "seed {seed:#x}");
        assert_eq!(
            host.destination,
            Some(Some((edge_index(drawn) as u16, 0x40)))
        );
        assert_eq!(host.rng.logical_state(), expected_rng.logical_state());

        let mut host = EdgeRecorder {
            rng: SimRng::new(seed),
            picked: Vec::new(),
            destination: None,
        };
        retreat_visit(RetreatNavCom::None, 1, 2, PhobosEdgeType::Owner, &mut host);
        assert_eq!(host.picked, vec![Edge::East], "seed {seed:#x}");
        assert_eq!(host.destination, Some(Some((1, 0x40))));
        // Owner spends no draw, so the stream is untouched.
        assert_eq!(host.rng.logical_state(), SimRng::new(seed).logical_state());

        let mut host = EdgeRecorder {
            rng: SimRng::new(seed),
            picked: Vec::new(),
            destination: None,
        };
        retreat_visit(
            RetreatNavCom::None,
            1,
            2,
            PhobosEdgeType::Closest,
            &mut host,
        );
        assert_eq!(host.picked, vec![Edge::East], "closest residual");
        assert_eq!(host.rng.logical_state(), SimRng::new(seed).logical_state());
    }
}

fn edge_index_checked(index: u8) -> Edge {
    Edge::from_index(index).expect("RandomRanged(0, 3) stays an edge")
}

/// `AircraftTypeExt::RetreatToEdge` (`Ext/AircraftType/Body.cpp:25`) reads the
/// AircraftType section only; the AircraftType's nullable value wins over
/// `[General] AircraftRetreatToEdge` (`Ext/Rules/Body.cpp:194`), whose
/// constructor default is `Owner`.
#[test]
fn retreat_to_edge_reads_the_aircraft_section_then_the_general_fallback() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nAircraftRetreatToEdge=closest\n\
         [AircraftTypes]\n0=CARG\n1=SHAD\n2=BAD\n\
         [CARG]\nSpeed=14\nRetreatToEdge=Random\n\
         [SHAD]\nSpeed=14\n\
         [BAD]\nSpeed=14\nRetreatToEdge=sideways\n",
    ))
    .unwrap();
    assert_eq!(
        rules.object("CARG").unwrap().phobos_retreat_to_edge(),
        Some(PhobosEdgeType::Random)
    );
    assert_eq!(
        rules.aircraft_retreat_to_edge_for(rules.object("CARG")),
        PhobosEdgeType::Random
    );
    // Unset type section takes the global.
    assert_eq!(rules.object("SHAD").unwrap().phobos_retreat_to_edge(), None);
    assert_eq!(
        rules.aircraft_retreat_to_edge_for(rules.object("SHAD")),
        PhobosEdgeType::Closest
    );
    // read<EdgeType> rejects any other text and keeps the unset value.
    assert_eq!(rules.object("BAD").unwrap().phobos_retreat_to_edge(), None);
    let owner_global = RuleSet::from_ini(&IniFile::from_str(
        "[AircraftTypes]\n0=SHAD\n[SHAD]\nSpeed=14\n",
    ))
    .unwrap();
    assert_eq!(
        owner_global.aircraft_retreat_to_edge_for(owner_global.object("SHAD")),
        PhobosEdgeType::Owner
    );
}
