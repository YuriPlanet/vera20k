//! Compare the Rust control flow with original 576770/576200 execution and
//! its wooden twins 571050/570AE0.
//! The supplied host replaces object/radar/presentation effects with the same
//! declared sinks as the native corpus. This is not world-integration coverage.

use super::*;
use crate::map::bridge_facts::{
    BridgeAnchorRelation, BridgeCellFacts, BridgeFlagStamp, BridgeStampFamily, BridgeStampSlot,
    apply_bridge_fact_slot,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone)]
struct SuppliedCell {
    coord: RimCoord,
    tile: i32,
    subtile: u8,
    facts: BridgeCellFacts,
    // Native +2C is a pointer, not the derived self relation in BridgeCellFacts.
    anchor: Option<RimCoord>,
}

impl SuppliedCell {
    fn snapshot(&self) -> Value {
        json!({"coord": self.coord, "flags": self.facts.raw_flags,
            "overlay": self.facts.overlay_id.map_or(-1, i32::from),
            "state": self.facts.state_byte, "anchor": self.anchor})
    }
}

struct SuppliedHost {
    tiles: HighBridgeRimTiles,
    bounds: RimBounds,
    cells: BTreeMap<RimCoord, SuppliedCell>,
    source_order: Vec<RimCoord>,
    calls: Vec<Value>,
    dummy: SuppliedCell,
}

#[derive(Clone, Copy)]
enum SuppliedHandle {
    Real(RimCoord),
    Dummy,
}

fn coord(value: &Value) -> RimCoord {
    serde_json::from_value(value.clone()).unwrap()
}

impl SuppliedHost {
    fn stock() -> Self {
        let input: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/bridge_rim_stock_inputs.json",
        ))
        .unwrap();
        Self::from_input(&input)
    }

    fn from_input(input: &Value) -> Self {
        let mut ini = String::from("[General]\n");
        for (key, value) in input["rim_keys"].as_object().unwrap() {
            ini.push_str(&format!("{key}={value}\n"));
        }
        let mut host = Self {
            tiles: HighBridgeRimTiles::from_ini(
                input["bridge_base"].as_i64().unwrap() as i32,
                ini.as_bytes(),
            ),
            bounds: {
                let (width, height): (i32, i32) =
                    serde_json::from_value(input["size"].clone()).unwrap();
                RimBounds::Diamond { width, height }
            },
            cells: BTreeMap::new(),
            source_order: Vec::new(),
            calls: Vec::new(),
            dummy: SuppliedCell {
                coord: (0, 0),
                tile: -1,
                subtile: 0,
                facts: BridgeCellFacts::default(),
                anchor: None,
            },
        };
        for row in input["cells"].as_array().unwrap() {
            let (x, y, tile, subtile, flags, overlay, state, anchor, _, _): (
                i16,
                i16,
                i32,
                u8,
                u32,
                Option<u8>,
                u8,
                Option<RimCoord>,
                u8,
                u8,
            ) = serde_json::from_value(row.clone()).unwrap();
            host.source_order.push((x, y));
            host.cells.insert(
                (x, y),
                SuppliedCell {
                    coord: (x, y),
                    tile,
                    subtile,
                    facts: BridgeCellFacts {
                        raw_flags: flags,
                        state_byte: state,
                        overlay_id: overlay,
                        ..Default::default()
                    },
                    anchor: anchor.filter(|_| flags & 0x80 == 0),
                },
            );
        }
        host
    }

    fn canonical(coord: RimCoord) -> Option<RimCoord> {
        let index = i32::from(coord.1) * 512 + i32::from(coord.0);
        (0..0x40000)
            .contains(&index)
            .then_some(((index % 512) as i16, (index / 512) as i16))
    }

    fn get(&mut self, coord: RimCoord) -> &mut SuppliedCell {
        if let Some(canonical) = Self::canonical(coord).filter(|c| self.cells.contains_key(c)) {
            return self.cells.get_mut(&canonical).unwrap();
        }
        self.dummy.coord = coord;
        &mut self.dummy
    }

    fn by_handle(&self, cell: SuppliedHandle) -> &SuppliedCell {
        match cell {
            SuppliedHandle::Real(coord) => &self.cells[&coord],
            SuppliedHandle::Dummy => &self.dummy,
        }
    }

    fn by_handle_mut(&mut self, cell: SuppliedHandle) -> &mut SuppliedCell {
        match cell {
            SuppliedHandle::Real(coord) => self.cells.get_mut(&coord).unwrap(),
            SuppliedHandle::Dummy => &mut self.dummy,
        }
    }

    fn snapshots(&self) -> Vec<Value> {
        self.source_order
            .iter()
            .map(|coord| self.cells[coord].snapshot())
            .collect()
    }

    fn collapse_input(&mut self, coord: RimCoord) {
        let direction = if self.get(coord).facts.raw_flags & 0x800 != 0 {
            0
        } else {
            6
        };
        let cell = self.cell(coord);
        self.clear_group(cell, direction);
        self.clear_overlay_and_mark_radar(cell);
    }
}

#[test]
fn retained_dummy_and_requested_endpoint_match_original_control_cases() {
    let original: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_rim_control.json",
    ))
    .unwrap();
    for case in original["cases"].as_array().unwrap() {
        let mut host = SuppliedHost::from_input(&case["input"]);
        assert_eq!(json!(host.snapshots()), case["before"]);
        let result = update_edge(
            &mut host,
            coord(&case["start"]),
            case["direction"].as_u64().unwrap() as u8,
        );
        assert_eq!(
            u64::from(result),
            case["result"].as_u64().unwrap(),
            "{} return",
            case["name"]
        );
        assert_eq!(
            json!(host.snapshots()),
            case["after"],
            "{} cells",
            case["name"]
        );
        assert_eq!(
            host.dummy.snapshot(),
            case["dummy"],
            "{} dummy",
            case["name"]
        );
        let calls: Vec<_> = case["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["kind"] != "edge_entry")
            .cloned()
            .collect();
        assert_eq!(
            json!(host.calls),
            json!(calls),
            "{} callbacks",
            case["name"]
        );
    }
}

impl HighBridgeRimHost for SuppliedHost {
    type Cell = SuppliedHandle;
    fn tiles(&self) -> HighBridgeRimTiles {
        self.tiles
    }
    fn bounds(&self) -> RimBounds {
        self.bounds
    }
    fn cell(&mut self, coord: RimCoord) -> SuppliedHandle {
        if let Some(canonical) = Self::canonical(coord).filter(|c| self.cells.contains_key(c)) {
            SuppliedHandle::Real(canonical)
        } else {
            self.dummy.coord = coord;
            SuppliedHandle::Dummy
        }
    }
    fn read(&self, handle: SuppliedHandle) -> RimCell {
        let cell = self.by_handle(handle);
        RimCell {
            coord: cell.coord,
            flags: cell.facts.raw_flags,
            tile: cell.tile,
            subtile: cell.subtile,
            anchor: cell.anchor,
        }
    }
    fn allocated(&self, coord: RimCoord) -> bool {
        Self::canonical(coord).is_some_and(|coord| self.cells.contains_key(&coord))
    }
    fn clear_group(&mut self, handle: SuppliedHandle, direction: u8) {
        let before = self.by_handle(handle).snapshot();
        let anchor = self.by_handle(handle).coord;
        self.calls
            .push(json!({"kind": "setter", "cell": before, "direction": direction, "set": 0}));
        let stamp = BridgeFlagStamp {
            anchor,
            direction,
            set: false,
        };
        for (slot, coord) in stamp.slots().unwrap() {
            let Some((x, y)) = coord else { continue };
            let cell = self.get((x as i16, y as i16));
            apply_bridge_fact_slot(
                &mut cell.facts,
                slot,
                BridgeAnchorRelation {
                    anchor: (anchor.0 as u16, anchor.1 as u16),
                    slot,
                    family: BridgeStampFamily::Nesw,
                    direction,
                },
                false,
            );
            if !matches!(slot, BridgeStampSlot::Anchor | BridgeStampSlot::Forward3) {
                cell.anchor = None;
            }
            if matches!(
                slot,
                BridgeStampSlot::Anchor
                    | BridgeStampSlot::Forward1
                    | BridgeStampSlot::Forward2
                    | BridgeStampSlot::Opposite
            ) {
                let snapshot = cell.snapshot();
                let coord = cell.coord;
                self.calls
                    .push(json!({"kind": "fallout_sink", "cell": snapshot}));
                self.calls
                    .push(json!({"kind": "radar_sink", "coord": coord}));
            }
        }
    }
    fn clear_overlay_and_mark_radar(&mut self, handle: SuppliedHandle) {
        let cell = self.by_handle_mut(handle);
        cell.facts.state_byte = 0;
        cell.facts.overlay_id = None;
        let coord = cell.coord;
        self.calls
            .push(json!({"kind": "radar_sink", "coord": coord}));
    }
    fn notify_span(&mut self, first: RimCoord, end: RimCoord) {
        self.calls
            .push(json!({"kind": "notify", "endpoints": [first, end]}));
        notify_span_cells(self, first, end);
    }
    fn notify_cell(&mut self, _cell: SuppliedHandle) {}
    fn mark_span(&mut self, _start: RimCoord, _end: RimCoord) {}
    fn reset_span_marks(&mut self) {}
    fn mark_screen(&mut self) {
        self.calls.push(json!({"kind": "screen_sink"}));
    }
}

fn compare_stock_cases(cases: &Value, bounds: impl Fn(&Value) -> Option<RimBounds>) {
    for case in cases.as_array().unwrap() {
        let mut host = SuppliedHost::stock();
        if let Some(bounds) = bounds(case) {
            host.bounds = bounds;
        }
        let breaks = case["breaks"].as_array().unwrap();
        for (index, refresh) in case["refreshes"].as_array().unwrap().iter().enumerate() {
            if let Some(break_coord) = breaks.get(index) {
                host.collapse_input(coord(break_coord));
            }
            let before = host.snapshots();
            host.calls.clear();
            update_adjacent(&mut host, coord(&refresh["coord"]));
            let changes: Vec<_> = before
                .into_iter()
                .zip(host.snapshots())
                .filter(|(before, after)| before != after)
                .map(|(before, after)| json!({"before": before, "after": after}))
                .collect();
            assert_eq!(
                json!(changes),
                refresh["changes"],
                "{} refresh{index}: fields",
                case["name"]
            );
            // The Rust loop replaces native tail recursion. Compare observable
            // callback order/receiver snapshots, excluding internal edge entries.
            let calls: Vec<_> = refresh["calls"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|event| event["kind"] != "edge_entry")
                .cloned()
                .collect();
            assert_eq!(
                json!(host.calls),
                json!(calls),
                "{} refresh{index}: calls",
                case["name"]
            );
        }
    }
}

fn stock_original() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_rim.json",
    ))
    .unwrap()
}

#[test]
fn stock_rim_control_matches_original_calls_and_full_cell_changes() {
    compare_stock_cases(&stock_original()["cases"], |_| None);
}

/// 571050's inclusive MapClass+0x124..+0x130 rectangle, including a top edge
/// on the span's start tile and one row past it.
#[test]
fn stock_wooden_rim_matches_original_under_its_search_rectangle() {
    compare_stock_cases(&stock_original()["low_cases"], |case| {
        let [left, top, width, height]: [u16; 4] =
            serde_json::from_value(case["rect"].clone()).unwrap();
        Some(RimBounds::Rect(
            crate::sim::scenario_bootstrap::NativeStartBounds {
                min_rx: left,
                min_ry: top,
                width,
                height,
            },
        ))
    });
}
