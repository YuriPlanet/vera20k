//! Differential host substitutes only the seams declared by the native oracle.
use super::*;
use crate::map::{bridge_pavement, iso_tile_flood};
use serde_json::{Value, json};
use std::collections::BTreeMap;

struct Cell {
    coord: CellCoord,
    tile: i32,
    sub: u8,
    flags: u32,
    level: u8,
    overlay: i32,
    anchor: Option<CellCoord>,
}
struct Host {
    cells: Vec<Cell>,
    index: BTreeMap<i32, usize>,
    dummy: usize,
    input: Value,
    trace: Vec<Value>,
    ordinals: BTreeMap<String, usize>,
    validates: usize,
}
fn point(v: &Value) -> CellCoord {
    (v[0].as_i64().unwrap() as i16, v[1].as_i64().unwrap() as i16)
}
impl Host {
    fn new(input: &Value) -> Self {
        let mut cells: Vec<Cell> = input["cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| Cell {
                coord: point(r),
                tile: r[2].as_i64().unwrap() as i32,
                sub: r[3].as_u64().unwrap() as u8,
                flags: r[4].as_u64().unwrap() as u32,
                overlay: r[5].as_i64().unwrap_or(-1) as i32,
                anchor: (!r[7].is_null()).then(|| point(&r[7])),
                level: r[8].as_u64().unwrap() as u8,
            })
            .collect();
        let index = cells
            .iter()
            .enumerate()
            .map(|(i, c)| (i32::from(c.coord.1) * 512 + i32::from(c.coord.0), i))
            .collect();
        let dummy = cells.len();
        cells.push(Cell {
            coord: (0, 0),
            tile: 0xffff,
            sub: 0,
            flags: 0,
            level: 0,
            overlay: -1,
            anchor: None,
        });
        Self {
            cells,
            index,
            dummy,
            input: input.clone(),
            trace: Vec::new(),
            ordinals: BTreeMap::new(),
            validates: 0,
        }
    }
    fn allocated_index(&self, p: CellCoord) -> Option<usize> {
        let i = i32::from(p.1) * 512 + i32::from(p.0);
        (0..0x40000)
            .contains(&i)
            .then(|| self.index.get(&i).copied())
            .flatten()
    }
    fn get(&mut self, p: CellCoord) -> usize {
        self.allocated_index(p).unwrap_or_else(|| {
            self.cells[self.dummy].coord = p;
            self.dummy
        })
    }
    fn event(&mut self, event: Value) {
        let kind = event["kind"].as_str().unwrap().to_owned();
        self.trace.push(event);
        let ordinal = self.ordinals.entry(kind.clone()).or_default();
        *ordinal += 1;
        let writes: Vec<Value> = self.input["callbacks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| c["kind"] == kind && c["ordinal"].as_u64() == Some(*ordinal as u64))
            .flat_map(|c| c["writes"].as_array().unwrap().iter().cloned())
            .collect();
        for change in writes {
            let i = if change["target"] == "dummy" {
                self.dummy
            } else {
                self.allocated_index(point(&change["target"])).unwrap()
            };
            if !change["coord"].is_null() {
                self.cells[i].coord = point(&change["coord"]);
            }
            if let Some(tile) = change["tile"].as_i64() {
                self.cells[i].tile = tile as i32;
            }
            self.trace
                .push(json!({"kind":"callback_write","change":change}));
        }
    }

    fn callback_result(&self, kind: &str) -> Result<(), String> {
        if self.input["failure"] == kind {
            Err(format!("failed {kind}"))
        } else {
            Ok(())
        }
    }
}
impl HutCells for Host {
    type Cell = usize;
    type Error = String;
    fn lookup(&mut self, p: CellCoord) -> usize {
        self.get(p)
    }
    fn coord(&self, c: usize) -> CellCoord {
        self.cells[c].coord
    }
    fn flags(&self, c: usize) -> u32 {
        self.cells[c].flags
    }
    fn anchor(&self, c: usize) -> Result<usize, String> {
        self.cells[c]
            .anchor
            .and_then(|p| self.allocated_index(p))
            .ok_or_else(|| "unadmitted null anchor".into())
    }
}
impl RepairHost for Host {
    fn tiles(&self, family: Family) -> HighBridgeRimTiles {
        let keys = &self.input["rim_keys"];
        let get = |k: &str| keys[k].as_i64().unwrap() as i32;
        HighBridgeRimTiles {
            base: self.input[if family == Family::High {
                "bridge_base"
            } else {
                "wood_base"
            }]
            .as_i64()
            .unwrap() as i32,
            top_left: [get("BridgeTopLeft1"), get("BridgeTopLeft2")],
            bottom_right: [get("BridgeBottomRight1"), get("BridgeBottomRight2")],
            top_right: [get("BridgeTopRight1"), get("BridgeTopRight2")],
            bottom_left: [get("BridgeBottomLeft1"), get("BridgeBottomLeft2")],
            middle: [get("BridgeMiddle1"), get("BridgeMiddle2")],
        }
    }
    fn tile(&self, c: usize) -> i32 {
        self.cells[c].tile
    }
    fn subtile(&self, c: usize) -> u8 {
        self.cells[c].sub
    }
    fn level(&self, c: usize) -> u8 {
        self.cells[c].level
    }
    fn write_level(&mut self, c: usize, level: u8) -> Result<(), String> {
        self.cells[c].level = level;
        self.event(json!({"kind":"level","coord":self.cells[c].coord,"level":level}));
        self.callback_result("level")
    }
    fn overlay(&self, c: usize) -> i32 {
        self.cells[c].overlay
    }
    fn search_in_bounds(&self, p: CellCoord, family: Family) -> bool {
        let (x, y) = (i32::from(p.0), i32::from(p.1));
        if family == Family::High {
            let r = self.input["search_rect"]
                .as_array()
                .map(|r| {
                    r.iter()
                        .map(|v| v.as_i64().unwrap() as i32)
                        .collect::<Vec<_>>()
                })
                .unwrap_or(vec![0, 0, 511, 511]);
            x >= r[0] && x <= r[0] + r[2] && y >= r[1] && y <= r[1] + r[3]
        } else {
            let w = self.input["size"][0].as_i64().unwrap() as i32;
            let h = self.input["size"][1].as_i64().unwrap() as i32;
            x + y > w && x - y < w && y - x < w && x + y <= w + 2 * h
        }
    }
    fn allocated(&self, p: CellCoord) -> bool {
        self.allocated_index(p).is_some()
    }
    fn ordinary_repair(&mut self, p: CellCoord, family: Family) -> Result<(), String> {
        self.event(json!({"kind":"ordinary","family":if family==Family::High {"high"}else{"low"},"coord":p}));
        Ok(())
    }
    fn pavement_clear(&mut self, p: CellCoord) {
        self.event(json!({"kind":"pavement","coord":p,"state":0,"recursive":0}));
        bridge_pavement::set_connected(&mut Pavement(self), p, false);
    }
    fn replace(&mut self, p: CellCoord, tile: i32) -> Result<(), String> {
        self.event(json!({"kind":"replace","coord":p,"tile":tile,"level":-1,"recursive":0}));
        iso_tile_flood::replace_connected(&mut Flood(self), p, tile, -1)
    }
    fn validate(&mut self, p: CellCoord) -> Result<bool, String> {
        let result = self.input["validate_returns"][self.validates]
            .as_bool()
            .unwrap_or(false);
        self.validates += 1;
        self.event(json!({"kind":"validate","coord":p,"result":result}));
        self.callback_result("validate")?;
        Ok(result)
    }
    fn construct(&mut self, p: CellCoord, overlay: u8, frame: i32) -> Result<(), String> {
        self.event(json!({"kind":"construct","coord":p,"overlay":overlay,"frame":frame}));
        Ok(())
    }
    fn connectivity(&mut self) -> Result<(), String> {
        self.event(json!({"kind":"connectivity"}));
        self.callback_result("connectivity")
    }
    fn rebuild(&mut self, cells: &[CellCoord]) -> Result<(), String> {
        self.event(json!({"kind":"rebuild","cells":cells}));
        Ok(())
    }
    fn project(&mut self, _: CellCoord, _: i8) -> [i32; 2] {
        [0, 0]
    }
    fn dirty_screen(&mut self, _: Option<Rect>) {
        self.event(json!({"kind":"screen"}));
    }
}

#[test]
fn ramp_host_errors_retain_native_prefix_and_stop_later_callbacks() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_repair.json",
    ))
    .unwrap();
    for (name, failure) in [
        ("high_2_middle_4", "level"),
        ("high_2_distance_1", "validate"),
        ("high_2_middle_4", "connectivity"),
    ] {
        let case = corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["input"]["name"] == name)
            .unwrap();
        let mut input = case["input"].clone();
        input["failure"] = json!(failure);
        let mut host = Host::new(&input);
        let start = host.get(point(&input["start"]));
        assert_eq!(
            restore_span(&mut host, Family::High, start, 2, false),
            Err(format!("failed {failure}"))
        );
        let native: Vec<_> = case["output"]["trace"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["kind"] != "span_entry" && event["kind"] != "repair_entry")
            .cloned()
            .collect();
        let stop = native
            .iter()
            .position(|event| event["kind"] == failure)
            .unwrap()
            + 1;
        assert_eq!(
            host.trace.as_slice(),
            &native[..stop],
            "{failure}: completed native prefix retained"
        );
        if failure == "level" {
            let last = host.trace.last().unwrap();
            assert_eq!(
                host.cells[host.allocated_index(point(&last["coord"])).unwrap()].level,
                last["level"].as_u64().unwrap() as u8,
                "raw store survives its publication error"
            );
        }
    }
}
struct Flood<'a>(&'a mut Host);
impl iso_tile_flood::IsoTileFloodHost for Flood<'_> {
    type Cell = usize;
    type Error = String;
    fn lookup(&mut self, p: CellCoord) -> usize {
        self.0.get(p)
    }
    fn tile(&self, c: usize) -> i32 {
        self.0.cells[c].tile
    }
    fn write_tile(&mut self, c: usize, tile: i32) -> Result<(), String> {
        self.0.cells[c].tile = tile;
        self.0
            .event(json!({"kind":"tile","coord":self.0.cells[c].coord,"tile":tile as u32}));
        Ok(())
    }
    fn recalc(&mut self, c: usize, level: i32) -> Result<(), String> {
        self.0
            .event(json!({"kind":"recalc","coord":self.0.cells[c].coord,"level":level}));
        Ok(())
    }
    fn radar(&mut self, c: usize) {
        self.0
            .event(json!({"kind":"radar","coord":self.0.cells[c].coord}));
    }
    fn initial_screen(&mut self, _: CellCoord, _: usize) {
        self.0.event(json!({"kind":"screen"}));
    }
}
struct Pavement<'a>(&'a mut Host);
impl bridge_pavement::PavementHost for Pavement<'_> {
    type Cell = usize;
    fn lookup(&mut self, p: CellCoord) -> usize {
        self.0.get(p)
    }
    fn tile(&self, c: usize) -> i32 {
        self.0.cells[c].tile
    }
    fn flags(&self, c: usize) -> u32 {
        self.0.cells[c].flags
    }
    fn write_flags(&mut self, c: usize, flags: u32) {
        self.0.cells[c].flags = flags;
        self.0
            .event(json!({"kind":"flags","coord":self.0.cells[c].coord,"flags":flags}));
    }
    fn has_damaged_data(&mut self, c: usize) -> bool {
        self.0
            .event(json!({"kind":"gate","tile":self.0.cells[c].tile,"sub":self.0.cells[c].sub}));
        true
    }
    fn initial_screen(&mut self, _: CellCoord, _: usize) {
        self.0.event(json!({"kind":"screen"}));
    }
    fn radar(&mut self, c: usize) {
        self.0
            .event(json!({"kind":"radar","coord":self.0.cells[c].coord}));
    }
}
#[test]
fn high_and_low_ramp_repair_control_matches_original_execution() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_repair.json",
    ))
    .unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let input = &case["input"];
        let name = input["name"].as_str().unwrap();
        let mut host = Host::new(input);
        let start = point(&input["start"]);
        let family = if input["family"] == "high" {
            Family::High
        } else {
            Family::Low
        };
        if input["entry"] == "span" {
            let cell = host.get(start);
            restore_span(
                &mut host,
                family,
                cell,
                input["direction"].as_u64().unwrap() as u8,
                false,
            )
            .unwrap();
        } else {
            repair(&mut host, start, family).unwrap();
        }
        let trace: Vec<Value> = case["output"]["trace"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] != "span_entry" && e["kind"] != "repair_entry")
            .cloned()
            .collect();
        if let Some(i) = host.trace.iter().zip(&trace).position(|(a, b)| a != b) {
            panic!(
                "{name} event{i}: Rust={} native={}",
                host.trace[i], trace[i]
            );
        }
        assert_eq!(
            host.trace.len(),
            trace.len(),
            "{name} callback count: Rust={:?} native={trace:?}",
            host.trace
        );
        let final_cells: Vec<Value> = host.cells[..host.dummy]
            .iter()
            .map(|c| json!([c.coord.0, c.coord.1, c.tile, c.flags, c.level]))
            .collect();
        assert_eq!(
            json!(final_cells),
            case["output"]["final"],
            "{name} final cells"
        );
        let d = &host.cells[host.dummy];
        assert_eq!(
            json!([d.coord.0, d.coord.1, d.tile, d.flags, d.level]),
            case["output"]["dummy"],
            "{name} final dummy"
        );
    }
}
