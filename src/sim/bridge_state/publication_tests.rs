//! Compare observable callback boundaries and final scalar cells with original
//! code: the concrete body (bridge_body_publication) and both drivers'
//! bridgehead branches (bridge_head_publication). Native instruction-width
//! stores and setter-entry instrumentation are retained in the oracles but
//! are not part of this Rust comparison.

use super::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone)]
struct Cell {
    allocation: CellCoord,
    coord: CellCoord,
    flags: u32,
    state: u8,
    overlay: i32,
    anchor: Option<usize>,
    tile: i32,
    subtile: u8,
    level: i8,
}

struct Host {
    /// The bridgehead corpus records families and no anchor snapshots.
    head: bool,
    base: i32,
    middles: [i32; 2],
    cells: Vec<Cell>,
    allocation: BTreeMap<CellCoord, usize>,
    events: Vec<Value>,
    mutation: Option<String>,
    injected: bool,
}

fn coord(value: &Value) -> CellCoord {
    (
        value[0].as_i64().unwrap() as i16,
        value[1].as_i64().unwrap() as i16,
    )
}

impl Host {
    fn new(source: &Value, case: &Value) -> Self {
        let rows = source["cells"].as_array().unwrap();
        let allocation: BTreeMap<_, _> = rows
            .iter()
            .enumerate()
            .map(|(i, row)| (coord(row), i))
            .collect();
        let cells = rows
            .iter()
            .map(|row| {
                let flags = row[4].as_u64().unwrap() as u32;
                Cell {
                    allocation: coord(row),
                    coord: coord(row),
                    flags,
                    state: row[6].as_u64().unwrap() as u8,
                    overlay: row[5].as_i64().unwrap_or(-1) as i32,
                    anchor: (flags & 0x80 == 0 && row[7].is_array())
                        .then(|| allocation[&coord(&row[7])]),
                    tile: row[2].as_i64().unwrap() as i32,
                    subtile: row[3].as_u64().unwrap() as u8,
                    level: row[8].as_i64().unwrap() as i8,
                }
            })
            .collect();
        let key = |name: &str| source["rim_keys"][name].as_i64().unwrap() as i32;
        let mut host = Self {
            head: false,
            base: source["bridge_base"].as_i64().unwrap() as i32,
            middles: [key("BridgeMiddle1"), key("BridgeMiddle2")],
            cells,
            allocation,
            events: Vec::new(),
            mutation: case["mutation"].as_str().map(str::to_owned),
            injected: false,
        };
        if let Some(state) = case["initial_anchor_state"].as_u64() {
            host.cells[host.allocation[&(112, 140)]].state = state as u8;
        }
        host
    }

    fn head(source: &Value, case: &Value) -> Self {
        let mut host = Self::new(source, case);
        host.head = true;
        for row in case["template_cells"].as_array().unwrap() {
            let cell = &mut host.cells[host.allocation[&coord(row)]];
            cell.tile = row[2].as_i64().unwrap() as i32;
            cell.subtile = row[3].as_u64().unwrap() as u8;
            cell.level = row[4].as_i64().unwrap() as i8;
        }
        host
    }

    fn snapshot(&self, id: usize) -> Value {
        let cell = &self.cells[id];
        json!({
            "allocation_coord": cell.allocation,
            "coord": cell.coord,
            "flags": cell.flags,
            "state": cell.state,
            "overlay": cell.overlay,
            "anchor": cell.anchor.map(|anchor| self.cells[anchor].coord),
        })
    }

    fn callback_write(&mut self, id: usize) {
        self.events
            .push(json!({"kind":"callback_write", "cell":self.snapshot(id)}));
        self.injected = true;
    }
}

impl BridgePublicationHost for Host {
    type Cell = usize;
    fn lookup(&mut self, coord: CellCoord) -> usize {
        // This corpus contains real allocated cells only; no dummy parity
        // claim is made by substituting a fixture lookup here.
        self.allocation[&coord]
    }
    fn coord(&self, cell: usize) -> CellCoord {
        self.cells[cell].coord
    }
    fn flags(&self, cell: usize) -> u32 {
        self.cells[cell].flags
    }
    fn state(&self, cell: usize) -> u8 {
        self.cells[cell].state
    }
    fn write_flags(&mut self, cell: usize, flags: u32) {
        self.cells[cell].flags = flags;
    }
    fn write_state(&mut self, cell: usize, state: u8) {
        self.cells[cell].state = state;
    }
    fn write_anchor(&mut self, cell: usize, anchor: Option<usize>) {
        self.cells[cell].anchor = anchor;
    }
    fn clear_overlay(&mut self, cell: usize) {
        self.cells[cell].overlay = -1;
    }
    fn fallout(&mut self, cell: usize) {
        self.events
            .push(json!({"kind":"fallout_sink", "cell":self.snapshot(cell)}));
        if self.injected {
            return;
        }
        match self.mutation.as_deref() {
            Some("anchor_fallout_changes_next_flags") => {
                let next = self.allocation[&(111, 140)];
                self.cells[next].flags = 0xa401_0380;
                self.callback_write(next);
            }
            Some("first_forward_fallout_moves_retained_coord")
                if self.cells[cell].allocation == (111, 140) =>
            {
                self.cells[cell].coord = (110, 140);
                self.callback_write(cell);
            }
            _ => {}
        }
    }
    fn radar(&mut self, cell: usize) {
        self.events
            .push(json!({"kind":"radar_sink", "coord":self.coord(cell)}));
    }
    fn perpendicular(
        &mut self,
        coord: CellCoord,
        axis: Axis,
        phase: Phase,
        direction: u8,
        family: Family,
    ) {
        let axis = match axis {
            Axis::NS => "ns",
            Axis::EW => "ew",
        };
        let phase = match phase {
            Phase::DamageA => "damage_a",
            Phase::DamageB => "damage_b",
            Phase::CollapseA => "collapse_a",
            Phase::CollapseB => "collapse_b",
        };
        let mut event = json!({"kind":"perpendicular_sink",
            "function":format!("{axis}_{phase}"), "coord":coord, "direction":direction});
        if self.head {
            event["family"] = json!(family_name(family));
        }
        self.events.push(event);
        if !self.injected
            && self.mutation.as_deref() == Some("first_perpendicular_changes_anchor_state")
        {
            let anchor = self.allocation[&(112, 140)];
            self.cells[anchor].state = 9;
            self.callback_write(anchor);
        }
    }
    fn rim(&mut self, coord: CellCoord, family: Family) {
        let event = if self.head {
            json!({"kind":"rim_sink", "family":family_name(family), "coord":coord})
        } else {
            json!({"kind":"rim_sink", "coord":coord,
                "anchor":self.snapshot(self.allocation[&(112,140)])})
        };
        self.events.push(event);
    }
    fn zones(&mut self, query: CellCoord) {
        let event = if self.head {
            json!({"kind":"zone_sink", "coord":query})
        } else {
            json!({"kind":"zone_sink", "coord":query,
                "anchor":self.snapshot(self.allocation[&(112,140)])})
        };
        self.events.push(event);
    }
    fn tile(&self, cell: usize) -> i32 {
        self.cells[cell].tile
    }
    fn subtile(&self, cell: usize) -> u8 {
        self.cells[cell].subtile
    }
    fn level(&self, cell: usize) -> i8 {
        self.cells[cell].level
    }
    fn flood(&mut self, coord: CellCoord, tile: i32, level: i32) {
        self.events
            .push(json!({"kind":"flood", "coord":coord, "tile":tile, "level":level}));
    }
    fn recalc_zones(&mut self, cells: &[CellCoord]) {
        self.events
            .push(json!({"kind":"recalc_sink", "cells":cells}));
    }
    fn middles(&self, _family: Family) -> Option<(i32, [i32; 2])> {
        // The corpus supplies the stock BridgeSet base to both drivers.
        Some((self.base, self.middles))
    }
}

fn family_name(family: Family) -> &'static str {
    match family {
        Family::High => "high",
        Family::Low => "low",
    }
}

#[test]
fn high_body_publication_matches_original_callback_boundaries() {
    let source: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_rim_stock_inputs.json",
    ))
    .unwrap();
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_body_publication.json",
    ))
    .unwrap();
    for case in native["cases"].as_array().unwrap() {
        let mut host = Host::new(&source, case);
        let before: Vec<_> = (0..host.cells.len()).map(|id| host.snapshot(id)).collect();
        let input = coord(&case["input_coord"]);
        let selected = host.lookup(input);
        let anchor = if host.flags(selected) & 0x80 != 0 {
            selected
        } else {
            host.cells[selected].anchor.unwrap()
        };
        let returned = advance_body_at_anchor(&mut host, input, anchor, Family::High);
        assert_eq!(
            u64::from(returned),
            case["returned"].as_u64().unwrap(),
            "{}",
            case["name"]
        );
        let expected_events: Vec<_> = case["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["kind"] != "setter")
            .cloned()
            .collect();
        assert_eq!(host.events, expected_events, "{}", case["name"]);
        let changes: Vec<_> = before
            .iter()
            .enumerate()
            .filter_map(|(id, before)| {
                let after = host.snapshot(id);
                (before != &after).then(|| json!({"before":before,"after":after}))
            })
            .collect();
        assert_eq!(json!(changes), case["changed_cells"], "{}", case["name"]);
        assert_eq!(
            host.injected,
            case["mutation"].is_string(),
            "{}",
            case["name"]
        );
    }
}

/// Both drivers' middle-tile branches over a synthetic template in the stock
/// crop: tile gate, walk, variant branch, blow-up row, flood tile and level,
/// helper family and order (the wooden EW damage calls the concrete helpers),
/// rim pair, zone query, the 586990 vector and the family return value.
#[test]
fn bridgehead_branches_match_original_callback_boundaries() {
    let source: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_rim_stock_inputs.json",
    ))
    .unwrap();
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_head_publication.json",
    ))
    .unwrap();
    for case in native["cases"].as_array().unwrap() {
        let mut host = Host::head(&source, case);
        let family = if case["family"] == "low" {
            Family::Low
        } else {
            Family::High
        };
        let returned = advance_bridgehead(&mut host, coord(&case["input_coord"]), family);
        assert_eq!(
            u64::from(returned),
            case["returned"].as_u64().unwrap(),
            "{}",
            case["name"]
        );
        assert_eq!(json!(host.events), case["calls"], "{}", case["name"]);
    }
}
