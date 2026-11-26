//! Replays `tools/spatial_oracle/aircraft_enter.json`: the original
//! `0x00419C80`, one call per row, through [`enter_visit`].

use super::*;
use serde_json::{Value, json};

fn rows() -> Vec<Value> {
    let oracle: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/aircraft_enter.json",
    ))
    .expect("aircraft_enter.json");
    oracle["mission_enter"].as_array().unwrap().clone()
}

/// The oracle's two Buildings, as NavCom, dock or the cell's first Building.
fn building(name: &Value) -> Option<u64> {
    match name.as_str() {
        None => None,
        Some("bld_a") => Some(1),
        Some("bld_b") => Some(2),
        Some(other) => panic!("{other} is not one of the oracle's Buildings"),
    }
}

fn building_name(id: Option<u64>) -> Value {
    match id {
        None => Value::Null,
        Some(1) => json!("bld_a"),
        Some(2) => json!("bld_b"),
        Some(other) => panic!("building {other} is not one of the oracle's"),
    }
}

/// Answers each call from the row and logs it as the oracle logs the native
/// call or write it stands for; keeps Mission+0xBC, `+0x6D4` and the dock.
/// GetHeight, GetCell and the locomotor's Get_Status are logged calls; the
/// fields `airport_bound`, `nav_com`, `dock`, `state` and `nothing_queued`
/// read are not.
struct Recorder {
    input: Value,
    calls: Vec<Value>,
    state: u32,
    ready: i64,
    dock: Option<u64>,
}

impl Recorder {
    fn int(&self, key: &str) -> i32 {
        self.input[key].as_i64().unwrap() as i32
    }

    fn log(&mut self, call: Value) {
        self.calls.push(call);
    }
}

impl EnterHost for Recorder {
    fn height(&mut self) -> i32 {
        self.log(json!(["height"]));
        self.int("height")
    }

    fn airport_bound(&mut self) -> bool {
        self.input["airport_bound"].as_bool().unwrap()
    }

    fn nav_com(&mut self) -> EnterNav {
        match self.input["nav"].as_str() {
            None => EnterNav::None,
            Some("cell" | "unit") => EnterNav::Other,
            Some(_) => EnterNav::Building(building(&self.input["nav"]).unwrap()),
        }
    }

    fn building_here(&mut self) -> Option<u64> {
        self.log(json!(["building_here"]));
        building(&self.input["here"])
    }

    fn dock(&mut self) -> Option<u64> {
        self.dock
    }

    fn set_dock(&mut self, dock: Option<u64>) {
        self.log(json!(["set_dock", building_name(dock)]));
        self.dock = dock;
    }

    fn over_out(&mut self) {
        self.log(json!(["over_out"]));
    }

    fn queue(&mut self, mission: MissionType, start: bool) {
        self.log(json!(["queue", mission.id(), u8::from(start)]));
    }

    fn on_ground_layer(&mut self) -> bool {
        self.log(json!(["on_ground_layer"]));
        self.int("layer") == 2
    }

    fn docking(&mut self) -> bool {
        self.log(json!(["docking"]));
        self.int("docking") == 1
    }

    fn approach_pending_entry(&mut self) -> bool {
        self.log(json!(["approach_pending_entry"]));
        self.input["pending"].as_bool().unwrap()
    }

    fn enter_idle_mode(&mut self) {
        self.log(json!(["enter_idle_mode", 0, 1]));
    }

    fn state(&mut self) -> u32 {
        self.state
    }

    fn set_state(&mut self, state: u32) {
        self.log(json!(["set_state", state as i32]));
        self.state = state;
    }

    fn set_transition_ready(&mut self, ready: bool) {
        self.log(json!(["set_transition_ready", u8::from(ready)]));
        self.ready = i64::from(ready);
    }

    fn in_radio_contact(&mut self) -> bool {
        self.log(json!(["in_radio_contact"]));
        self.input["radio"].as_bool().unwrap()
    }

    fn status(&mut self) -> i32 {
        self.log(json!(["status"]));
        self.int("status")
    }

    fn nothing_queued(&mut self) -> bool {
        self.int("queued") == -1
    }

    fn clear_destination(&mut self) {
        self.log(json!(["clear_destination", 1]));
    }

    fn step_toward_nav_com(&mut self) {
        self.log(json!(["step_toward_nav_com"]));
    }

    fn dock_now(&mut self) -> i32 {
        self.log(json!(["dock_now"]));
        self.int("dock_now")
    }

    fn board_contact(&mut self) {
        self.log(json!(["limbo"]));
        self.log(json!(["add_passenger", "contact", "aircraft"]));
    }

    /// The rows' aircraft is no Carryall.
    fn deliver_cargo(&mut self) -> bool {
        false
    }
}

/// Drops a write that repeats the entry before it. State 6 rewrites `+0x6D4`
/// with the same byte up to three times in a row (`0x00419E52`,
/// `0x00419E90`, `0x00419E97`) where the port writes it once or twice;
/// nothing runs between those writes to tell them apart.
fn collapse_rewrites(calls: &[Value]) -> Vec<Value> {
    let mut collapsed: Vec<Value> = Vec::new();
    for call in calls {
        let write = matches!(
            call[0].as_str(),
            Some("set_state" | "set_transition_ready" | "set_dock")
        );
        if !(write && collapsed.last() == Some(call)) {
            collapsed.push(call.clone());
        }
    }
    collapsed
}

/// Every row: the call log, the returned frames and Mission+0xBC, `+0x6D4`
/// (preset 2) and the dock afterwards.
#[test]
fn mission_enter_matches_the_original() {
    let rows = rows();
    assert_eq!(rows.len(), 612);
    let mut disagreements = Vec::new();
    for row in &rows {
        let input = &row["input"];
        let mut host = Recorder {
            input: input.clone(),
            calls: Vec::new(),
            state: input["state"].as_i64().unwrap() as i32 as u32,
            ready: input["ready"].as_i64().unwrap(),
            dock: building(&input["dock"]),
        };
        let frames = enter_visit(&mut host);
        let rust = json!({
            "ret": frames,
            "calls": collapse_rewrites(&host.calls),
            "state": host.state as i32,
            "ready": host.ready,
            "dock": building_name(host.dock),
        });
        let native = json!({
            "ret": row["ret"],
            "calls": collapse_rewrites(row["calls"].as_array().unwrap()),
            "state": row["state"],
            "ready": row["ready"],
            "dock": row["dock"],
        });
        if rust != native {
            disagreements.push(format!(
                "{}:\n  native {native}\n  rust   {rust}",
                input["name"]
            ));
        }
    }
    assert!(
        disagreements.is_empty(),
        "{} of {} rows disagree:\n{}",
        disagreements.len(),
        rows.len(),
        disagreements.join("\n")
    );
}

/// The original's descending arm of state 7, which `step_toward_nav_com`
/// stands for: a Building NavCom's dock coordinate for this aircraft
/// (`vt+0xA8`), any other NavCom's coordinate (`vt+0x48`), then SetLocation
/// at most 5 leptons from the Location toward it in X and in Y, keeping Z;
/// nothing without a NavCom or outside that arm.
#[test]
fn landing_step_matches_the_original() {
    let rows = rows();
    let mut steps = 0;
    for row in &rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let stepped = row["calls"]
            .as_array()
            .unwrap()
            .contains(&json!(["step_toward_nav_com"]));
        let expected = match input["nav"].as_str() {
            Some(nav) if stepped => {
                let coord = |key: &str, n: usize| input[key][n].as_i64().unwrap();
                let toward = |n: usize| {
                    coord("location", n)
                        + (coord("nav_coord", n) - coord("location", n)).clamp(-5, 5)
                };
                let query = if nav.starts_with("bld") {
                    json!(["dock_coord", "aircraft"])
                } else {
                    json!(["center_coord"])
                };
                steps += 1;
                json!([
                    query,
                    ["set_location", [toward(0), toward(1), coord("location", 2)]]
                ])
            }
            _ => json!([]),
        };
        assert_eq!(row["step"], expected, "{name}");
    }
    assert_eq!(steps, 34);
}
