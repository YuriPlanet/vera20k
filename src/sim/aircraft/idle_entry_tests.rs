//! Replays `tools/spatial_oracle/aircraft_idle_retreat.json`'s `enter_idle`
//! rows: the original `0x004176F0` through its head, both arms and its tail.

use super::*;
use serde_json::{Value, json};

fn rows() -> Vec<Value> {
    let oracle: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/aircraft_idle_retreat.json",
    ))
    .expect("aircraft_idle_retreat.json");
    oracle["enter_idle"].as_array().unwrap().clone()
}

/// The dock the oracle's dock search answers.
const DOCK: u64 = 0xD0C;

/// The oracle's name for a dock pointer.
fn dock_name(dock: u64) -> Value {
    if dock == DOCK {
        json!("dock")
    } else {
        json!(dock)
    }
}

/// Answers from the row and logs each call as the oracle's stubs and hooks
/// do. Keeps the team, which the oracle's Remove_Member clears, and
/// Mission+0xBC and `+0x6D2` (the rows preset 7 and 1).
struct Recorder<'a> {
    input: &'a Value,
    calls: Vec<Value>,
    team: bool,
    state: i64,
    latch: i64,
}

impl Recorder<'_> {
    fn flag(&self, key: &str) -> bool {
        self.input[key].as_bool().unwrap()
    }

    fn mission(&self, key: &str) -> MissionId {
        MissionId::from_raw(self.input[key].as_i64().unwrap() as i32)
    }
}

impl IdleHost for Recorder<'_> {
    fn current(&mut self) -> MissionId {
        self.mission("current")
    }

    fn queued(&mut self) -> MissionId {
        self.mission("queued")
    }

    fn restore(&mut self) -> MissionId {
        self.calls.push(json!(["restore"]));
        // The oracle's Restore stub moves the suspended selector to current.
        self.mission("suspended")
    }

    fn restart_patrol(&mut self) {
        self.state = 0;
        self.latch = 0;
    }

    fn commence(&mut self) {
        self.calls.push(json!(["commence"]));
    }

    fn foot_enter_idle(&mut self) -> bool {
        self.calls.push(json!(["foot_enter_idle", 0, 1]));
        self.flag("foot")
    }

    fn team(&mut self) -> bool {
        self.team
    }

    fn team_entered_map(&mut self) -> bool {
        self.calls.push(json!(["has_entered_map"]));
        self.flag("entered")
    }

    fn leave_team(&mut self) {
        self.calls.push(json!(["remove_member", -1, 0]));
        self.team = false;
    }

    fn target(&mut self) -> bool {
        self.flag("target")
    }

    fn nav_com(&mut self) -> bool {
        self.flag("nav_com")
    }

    fn in_air(&mut self) -> bool {
        self.calls.push(json!(["in_air"]));
        self.flag("high")
    }

    fn clear_target(&mut self) {
        self.calls.push(json!(["assign_target", null]));
    }

    fn clear_destination(&mut self) {
        self.calls.push(json!(["assign_destination", null, 1]));
    }

    fn find_dock(&mut self) -> Option<u64> {
        self.calls.push(json!(["dock_search", "type_dock", 0, 0]));
        self.flag("dock").then_some(DOCK)
    }

    fn hello(&mut self, dock: u64) -> bool {
        self.calls.push(json!([
            "transmit_message",
            RadioMessage::Hello as u8,
            dock_name(dock)
        ]));
        self.input["hello"].as_i64() == Some(i64::from(RadioResponse::Roger.code()))
    }

    fn assign_dock(&mut self, dock: u64) {
        self.calls
            .push(json!(["assign_destination", dock_name(dock), 1]));
    }

    fn assign_airfield(&mut self) {
        self.calls.push(json!(["nearest_airfield"]));
        self.calls
            .push(json!(["assign_destination", "airfield", 1]));
    }

    fn crash(&mut self) {
        self.calls.push(json!(["crash", null]));
    }

    fn in_radio_contact(&mut self) -> bool {
        self.calls.push(json!(["in_radio_contact"]));
        self.flag("radio")
    }

    fn queue(&mut self, mission: MissionType) {
        self.calls.push(json!(["queue_mission", mission.id(), 0]));
    }

    fn ready(&mut self) -> bool {
        self.calls.push(json!(["ready"]));
        self.flag("ready")
    }
}

/// Every row: the call log, the answer, and Mission+0xBC and `+0x6D2`
/// afterwards. Collects every disagreeing row before failing.
#[test]
fn enter_idle_mode_matches_the_original() {
    let rows = rows();
    assert_eq!(rows.len(), 905);
    let mut disagreements = Vec::new();
    for row in &rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let int = |key: &str| input[key].as_i64().unwrap();
        let flag = |key: &str| input[key].as_bool().unwrap();
        let facts = IdleFacts {
            suspended: int("suspended") != -1,
            airstrike: flag("airstrike"),
            mission_only: flag("mission_only"),
            passengers: flag("passengers"),
            human: flag("human"),
            armed: flag("armed"),
            weapon: flag("weapon"),
            ammo: int("ammo") as i32,
            type_ammo: int("type_ammo") as i32,
            docks: flag("docks"),
            airport_bound: flag("airport_bound"),
            in_playfield: flag("in_playfield"),
            // `0x004177BA..0x004177FC`.
            landed: int("layer") == 2 || int("height") <= int("landing") || flag("missile_spawn"),
        };
        let mut host = Recorder {
            input,
            calls: Vec::new(),
            team: flag("team"),
            state: int("state"),
            latch: int("latch"),
        };
        let answer = enter_idle_mode(&facts, &mut host);
        let port = json!({
            "calls": host.calls,
            "ret": u8::from(answer),
            "state": host.state,
            "latch": host.latch,
        });
        let original = json!({
            "calls": row["calls"],
            "ret": row["ret"],
            "state": row["state"],
            "latch": row["latch"],
        });
        if port != original {
            disagreements.push(format!("{name}:\n  port     {port}\n  original {original}"));
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
