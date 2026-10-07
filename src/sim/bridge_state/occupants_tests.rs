use super::*;
use crate::map::cell_index::{NativeCellIdentity as Cell, cell_linear_index};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone)]
struct CellData {
    coord: CellCoord,
    level: u8,
    slope: u8,
    head: Option<u32>,
}

fn point(value: &Value) -> CellCoord {
    (
        value[0].as_i64().unwrap() as i16,
        value[1].as_i64().unwrap() as i16,
    )
}

fn member(value: &Value) -> Option<u32> {
    value.as_u64().map(|v| v as u32)
}

impl CellData {
    fn new(value: &Value) -> Self {
        Self {
            coord: point(&value["coord"]),
            level: value["level"].as_i64().unwrap_or(0) as u8,
            slope: value["slope"].as_u64().unwrap_or(0) as u8,
            head: member(&value["head"]),
        }
    }
}

struct Host {
    cells: Vec<CellData>,
    slots: BTreeMap<i64, Cell>,
    dummy: CellData,
    selected: Cell,
    objects: BTreeMap<u32, Value>,
    callbacks: Vec<Value>,
    ordinals: BTreeMap<(String, Option<u32>), usize>,
    trace: Vec<Value>,
    failure: Option<&'static str>,
}

impl Host {
    fn new(input: &Value) -> Self {
        let cells: Vec<_> = input["cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(CellData::new)
            .collect();
        let slots: BTreeMap<_, _> = cells
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                (
                    cell_linear_index(i32::from(cell.coord.0), i32::from(cell.coord.1)).unwrap(),
                    Cell::Real(index),
                )
            })
            .collect();
        let start = point(&input["start"]);
        let selected = cell_linear_index(i32::from(start.0), i32::from(start.1))
            .and_then(|slot| slots.get(&slot).copied())
            .unwrap_or(Cell::Dummy);
        Self {
            cells,
            slots,
            selected,
            dummy: CellData::new(&input["dummy"]),
            objects: input["objects"]
                .as_array()
                .unwrap()
                .iter()
                .map(|object| (member(&object["id"]).unwrap(), object.clone()))
                .collect(),
            callbacks: input["callbacks"].as_array().cloned().unwrap_or_default(),
            ordinals: BTreeMap::new(),
            trace: Vec::new(),
            failure: None,
        }
    }

    fn cell(&self, cell: Cell) -> &CellData {
        match cell {
            Cell::Real(index) => &self.cells[index],
            Cell::Dummy => &self.dummy,
        }
    }

    fn cell_mut(&mut self, cell: Cell) -> &mut CellData {
        match cell {
            Cell::Real(index) => &mut self.cells[index],
            Cell::Dummy => &mut self.dummy,
        }
    }

    fn event(&mut self, event: Value) -> Result<(), String> {
        let kind = event["kind"].as_str().unwrap().to_owned();
        let object = member(&event["object"]);
        self.trace.push(event);
        let ordinal = self.ordinals.entry((kind.clone(), object)).or_default();
        *ordinal += 1;
        let ordinal = *ordinal;
        for callback in self.callbacks.clone() {
            if callback["event"] != kind
                || member(&callback["object"]) != object
                || callback["ordinal"].as_u64().unwrap() as usize != ordinal
            {
                continue;
            }
            for change in callback["writes"].as_array().unwrap() {
                if let Some(id) = member(&change["object"]) {
                    for field in ["next", "health", "flags", "admission", "at", "kind"] {
                        if let Some(value) = change.get(field) {
                            self.objects.get_mut(&id).unwrap()[field] = value.clone();
                        }
                    }
                } else {
                    let target = if change["cell"] == "selected" {
                        self.selected
                    } else {
                        Cell::Dummy
                    };
                    let cell = self.cell_mut(target);
                    if let Some(coord) = change.get("coord") {
                        cell.coord = point(coord);
                    }
                    if let Some(level) = change.get("level") {
                        cell.level = level.as_i64().unwrap() as u8;
                    }
                    if let Some(head) = change.get("head") {
                        cell.head = member(head);
                    }
                }
                self.trace
                    .push(json!({"kind":"callback_write", "change":change}));
            }
        }
        if self.failure == Some(kind.as_str()) {
            Err(format!("failed {kind}"))
        } else {
            Ok(())
        }
    }
}

impl BridgeOccupantHost for Host {
    type Cell = Cell;
    type Object = u32;
    type Error = String;

    fn coord(&self, cell: Cell) -> CellCoord {
        self.cell(cell).coord
    }
    fn lookup(&mut self, coord: CellCoord) -> Cell {
        let cell = cell_linear_index(i32::from(coord.0), i32::from(coord.1))
            .and_then(|slot| self.slots.get(&slot).copied())
            .unwrap_or(Cell::Dummy);
        if cell == Cell::Dummy {
            self.dummy.coord = coord;
        }
        let name = match cell {
            Cell::Real(i) => json!(i),
            Cell::Dummy => json!("dummy"),
        };
        self.event(json!({"kind":"lookup", "requested":coord, "cell":name}))
            .unwrap();
        cell
    }
    fn ground_head(&self, cell: Cell) -> Option<u32> {
        self.cell(cell).head
    }
    fn next_object(&self, object: u32) -> Option<u32> {
        member(&self.objects[&object]["next"])
    }
    fn is_foot(&self, object: u32) -> bool {
        self.objects[&object]["flags"].as_u64().unwrap_or(4) & 4 != 0
    }
    fn is_techno(&self, object: u32) -> bool {
        self.objects[&object]["flags"].as_u64().unwrap_or(4) & 1 != 0
    }
    fn type_is_jumpjet(&mut self, object: u32) -> Result<bool, String> {
        let result = self.objects[&object]["jumpjet"].as_bool().unwrap_or(false);
        self.event(json!({"kind":"jumpjet_type", "object":object, "result":result}))?;
        Ok(result)
    }
    fn admission(&mut self, object: u32, cell: Cell) -> Result<i32, String> {
        assert_eq!(cell, self.selected);
        let result = self.objects[&object]["admission"].as_i64().unwrap_or(0) as i32;
        self.event(json!({"kind":"admit", "object":object, "result":result}))?;
        Ok(result)
    }
    fn abstract_kind(&mut self, object: u32) -> Result<i32, String> {
        let result = self.objects[&object]["kind"].as_i64().unwrap_or(6) as i32;
        self.event(json!({"kind":"abstract_kind", "object":object, "result":result}))?;
        Ok(result)
    }
    fn current_health(&self, object: u32) -> i32 {
        self.objects[&object]["health"].as_i64().unwrap_or(100) as i32
    }
    fn receive_damage(&mut self, object: u32, damage: i32) -> Result<(), String> {
        self.event(json!({"kind":"damage", "object":object, "damage":damage, "health_alias":false}))
    }
    fn ground_probe(&mut self, cell: Cell) -> Result<[i32; 3], String> {
        let cell = self.cell(cell);
        let x = i32::from(cell.coord.0) * 256 + 128;
        let y = i32::from(cell.coord.1) * 256 + 128;
        let z = crate::util::lepton::ground_height_leptons(cell.level, cell.slope, 128, 128)
            .map_err(|error| format!("ground height {error:?}"))?;
        let point = [x, y, z];
        self.event(json!({"kind":"probe", "point":point}))?;
        Ok(point)
    }
    fn is_at_coord(&mut self, object: u32, point: [i32; 3]) -> Result<bool, String> {
        let result = self.objects[&object]["at"].as_bool().unwrap_or(true);
        self.event(json!({"kind":"at_coord", "object":object, "point":point, "result":result}))?;
        Ok(result)
    }
}

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_occupants.json",
    ))
    .unwrap()
}

#[test]
fn repair_occupant_controller_matches_original_live_order_and_height() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 19);
    for case in cases {
        let mut host = Host::new(&case["input"]);
        let selected = host.selected;
        recheck_occupants(&mut host, selected, 0).unwrap();
        let health: Vec<_> = host
            .objects
            .keys()
            .map(|&id| [id, host.current_health(id) as u32])
            .collect();
        let actual = json!({"trace":host.trace, "dummy_coord":host.dummy.coord,
            "selected_coord":host.coord(selected), "health":health});
        assert_eq!(actual, case["output"], "{}", case["input"]["name"]);
    }
}

#[test]
fn damage_occupant_mode_matches_original_type_gate() {
    let corpus = corpus();
    for case in corpus["damage_cases"].as_array().unwrap() {
        let mut host = Host::new(&case["input"]);
        let selected = host.selected;
        recheck_occupants(
            &mut host,
            selected,
            case["input"]["mode"].as_u64().unwrap() as u8,
        )
        .unwrap();
        assert_eq!(
            json!(host.trace),
            case["output"]["trace"],
            "{}",
            case["input"]["name"]
        );
    }
}

#[test]
fn repair_occupant_callback_errors_keep_completed_prefix() {
    let corpus = corpus();
    for (name, failure) in [
        ("ground_next_captured_before_admission", "admit"),
        ("ground_health_read_after_aircraft_kind", "abstract_kind"),
        ("ground_next_captured_before_damage", "damage"),
        ("neighbor_next_reread_after_at_coord", "at_coord"),
    ] {
        let case = corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["input"]["name"] == name)
            .unwrap();
        let mut host = Host::new(&case["input"]);
        host.failure = Some(failure);
        let selected = host.selected;
        assert_eq!(
            recheck_occupants(&mut host, selected, 0),
            Err(format!("failed {failure}"))
        );
        let native = case["output"]["trace"].as_array().unwrap();
        let first = native
            .iter()
            .position(|event| event["kind"] == failure)
            .unwrap();
        let mut end = first + 1;
        while end < native.len() && native[end]["kind"] == "callback_write" {
            end += 1;
        }
        assert_eq!(host.trace, native[..end], "{name}");
    }
}
