//! Replays `tools/spatial_oracle/aircraft_guard.json`: the original
//! `0x0041A5C0` and `0x0041A940`, one call per row, through [`guard_visit`]
//! and [`area_guard_visit`].

use super::*;
use serde_json::{Value, json};

fn rows(key: &str) -> Vec<Value> {
    let oracle: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/aircraft_guard.json",
    ))
    .expect("aircraft_guard.json");
    oracle[key].as_array().unwrap().clone()
}

/// The dock the oracle's dock search (`vt+0x528`) answers.
const DOCK: u64 = 1;

fn facts(input: &Value) -> GuardFacts {
    let int = |key: &str| input[key].as_i64().unwrap() as i32;
    let flag = |key: &str| input[key].as_bool().unwrap();
    GuardFacts {
        height: int("height"),
        flight_level: int("flight_level"),
        team: flag("team"),
        nav_com: flag("nav_com"),
        weapon: flag("weapon"),
        armed: flag("armed"),
        ammo: int("ammo"),
        type_ammo: int("type_ammo"),
        human: flag("human"),
    }
}

/// Answers each call from the row and logs it as the oracle logs the native
/// call it stands for, with the arguments that call passes; keeps `+0x6D4`.
struct Recorder {
    input: Value,
    calls: Vec<Value>,
    ready: i64,
}

impl Recorder {
    fn flag(&self, key: &str) -> bool {
        self.input[key].as_bool().unwrap()
    }

    fn int(&self, key: &str) -> i32 {
        self.input[key].as_i64().unwrap() as i32
    }

    fn log(&mut self, call: Value) {
        self.calls.push(call);
    }
}

impl GuardHost for Recorder {
    fn set_transition_ready(&mut self) {
        self.log(json!(["set_transition_ready", 1]));
        self.ready = 1;
    }

    fn queue(&mut self, mission: MissionType) {
        self.log(json!(["queue", mission.id(), 0]));
    }

    fn rate(&mut self) -> i32 {
        self.log(json!(["rate"]));
        self.int("rate")
    }

    fn assign_own_cell(&mut self) {
        self.log(json!(["assign_own_cell", 1]));
    }

    fn enter_idle_mode(&mut self) {
        self.log(json!(["enter_idle_mode", 0, 1]));
    }

    fn in_radio_contact(&mut self) -> bool {
        self.log(json!(["in_radio_contact"]));
        !self.input["contact"].is_null()
    }

    fn find_dock(&mut self) -> Option<u64> {
        self.log(json!(["find_dock", "type_dock", 0, 0]));
        self.flag("dock").then_some(DOCK)
    }

    fn assign_dock(&mut self, dock: u64) {
        assert_eq!(dock, DOCK);
        self.log(json!(["assign_dock", "dock", 1]));
    }

    fn clear_target(&mut self) {
        self.log(json!(["clear_target"]));
    }

    fn contact_reloads(&mut self) -> bool {
        self.log(json!(["contact_reloads"]));
        self.input["contact"] == "reload"
    }

    fn target(&mut self) -> bool {
        self.log(json!(["target"]));
        self.flag("target")
    }

    fn attack_bridge_unit(&mut self) {
        self.log(json!(["attack_bridge_unit"]));
        if self.flag("bridge_unit") {
            self.log(json!(["assign_target", "bridge_unit"]));
            self.log(json!(["queue", MissionType::Attack.id(), 0]));
        }
    }

    fn in_air(&mut self) -> bool {
        self.log(json!(["in_air"]));
        self.flag("in_air")
    }

    fn jitter(&mut self) -> i32 {
        self.log(json!(["jitter", 0, 2]));
        self.int("jitter")
    }

    fn foot_guard(&mut self) -> i32 {
        self.log(json!(["foot_guard"]));
        self.int("foot")
    }

    fn foot_area_guard(&mut self) -> i32 {
        self.log(json!(["foot_area_guard"]));
        self.int("foot")
    }
}

/// Every row: the call log, the returned frames, Mission+0xBC (preset 3,
/// which neither function writes) and `+0x6D4` (preset 2).
fn replay(key: &str, count: usize, visit: impl Fn(&GuardFacts, &mut Recorder) -> i32) {
    let rows = rows(key);
    assert_eq!(rows.len(), count, "{key} rows");
    let mut disagreements = Vec::new();
    for row in &rows {
        let input = &row["input"];
        let mut host = Recorder {
            input: input.clone(),
            calls: Vec::new(),
            ready: input["ready"].as_i64().unwrap(),
        };
        let frames = visit(&facts(input), &mut host);
        let rust = json!({
            "ret": frames,
            "calls": host.calls,
            "state": input["state"],
            "ready": host.ready,
        });
        let native = json!({
            "ret": row["ret"],
            "calls": row["calls"],
            "state": row["state"],
            "ready": row["ready"],
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
        "{} of {count} {key} rows disagree:\n{}",
        disagreements.len(),
        disagreements.join("\n")
    );
}

#[test]
fn mission_guard_matches_the_original() {
    replay("mission_guard", 433, guard_visit);
}

#[test]
fn mission_area_guard_matches_the_original() {
    replay("mission_area_guard", 53, |facts, host| {
        area_guard_visit(facts, host)
    });
}
