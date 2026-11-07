//! Replays `tools/rocket_oracle/spawn_manager.json` through the manager's
//! bodies. Each row is a script over one manager; every step runs the same
//! entry the original ran (AI over a frame range, SetTarget, PointerExpired,
//! ClearAllTargets) or changes the row's world, and must leave the manager,
//! its slots, the kamikaze timer, the owner's burst index and the children as
//! the original left them, after the same calls on the owner, the children
//! and the tracker. The host answers from the row's world as the oracle's
//! stubs do; its Push writes the tracker's child stores (+0x6CA, Ammo) as the
//! oracle's seam does.

use std::collections::VecDeque;

use serde_json::{Value, json};

use super::*;
use crate::rules::missile_spawn::MissileSpawnRules;

const OWNER: u64 = 1;
const CHILD_BASE: u64 = 100;
const TARGET_BASE: u64 = 500;

fn int(value: &Value) -> i32 {
    value.as_i64().expect("an integer") as i32
}

fn reference(value: &Value) -> Option<u64> {
    match value {
        Value::Null => None,
        Value::String(owner) if owner == "owner" => Some(OWNER),
        Value::Array(pair) => {
            let index = pair[1].as_u64().expect("an index");
            Some(match pair[0].as_str() {
                Some("child") => CHILD_BASE + index,
                Some("target") => TARGET_BASE + index,
                other => panic!("unknown reference {other:?}"),
            })
        }
        other => panic!("unknown reference {other}"),
    }
}

fn name(id: Option<u64>) -> Value {
    match id {
        None => Value::Null,
        Some(OWNER) => json!("owner"),
        Some(id) if id >= TARGET_BASE => json!(["target", id - TARGET_BASE]),
        Some(id) => json!(["child", id - CHILD_BASE]),
    }
}

fn target_name(target: Option<TargetKind>) -> Value {
    match target {
        None => Value::Null,
        Some(TargetKind::Entity(id)) => name(Some(id)),
        Some(other) => panic!("the rows target objects only: {other:?}"),
    }
}

fn slot_state(status: i32) -> SpawnSlotState {
    use SpawnSlotState::*;
    match status {
        0 => ReadyDocked,
        1 => KamikazeWait,
        2 => InFlight,
        3 => Attacking,
        4 => ComingHome,
        6 => Reloading,
        7 => Regenerating,
        other => panic!("node status {other}"),
    }
}

fn status_of(state: SpawnSlotState) -> i32 {
    use SpawnSlotState::*;
    match state {
        ReadyDocked => 0,
        KamikazeWait => 1,
        InFlight => 2,
        Attacking => 3,
        ComingHome => 4,
        Reloading => 6,
        Regenerating => 7,
    }
}

fn mode_of(status: i32) -> SpawnManagerMode {
    match status {
        0 => SpawnManagerMode::Idle,
        1 => SpawnManagerMode::Launching,
        2 => SpawnManagerMode::Returning,
        other => panic!("manager status {other}"),
    }
}

fn timer(value: &Value) -> CdTimer {
    CdTimer::from_raw(int(&value[0]), int(&value[1]))
}

struct Child {
    health: i32,
    estimated: i32,
    ammo: i32,
    spawn_owner: bool,
    tracked: bool,
    cell: (i32, i32),
    z: i32,
    other_type: bool,
}

struct Replay<'a> {
    row: &'a Value,
    frame: i32,
    manager: SpawnManagerState,
    owner: serde_json::Map<String, Value>,
    owner_burst: i32,
    children: Vec<Child>,
    spares: VecDeque<usize>,
    kamikaze_timer: (i32, i32),
    events: Vec<Value>,
}

impl<'a> Replay<'a> {
    fn new(row: &'a Value) -> Self {
        let input = &row["input"];
        let family = match input["family"].as_str() {
            Some("v3") => Some(MissileFamily::V3Rocket),
            Some("dmisl") => Some(MissileFamily::DMisl),
            Some("cmisl") => Some(MissileFamily::CMisl),
            None => None,
            other => panic!("family {other:?}"),
        };
        let mut missile_rules = MissileSpawnRules::default();
        let v3 = &input["rules"]["v3_frames"];
        let dmisl = &input["rules"]["dmisl_frames"];
        (missile_rules.v3.pause_frames, missile_rules.v3.tilt_frames) = (int(&v3[0]), int(&v3[1]));
        (
            missile_rules.dmisl.pause_frames,
            missile_rules.dmisl.tilt_frames,
        ) = (int(&dmisl[0]), int(&dmisl[1]));
        let manager = &input["manager"];
        let slots = manager["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|node| SpawnSlot {
                spawn: reference(&node["unit"]),
                state: slot_state(int(&node["status"])),
                timer: timer(&node["timer"]),
                is_missile_spawn: int(&node["missile"]) == 1,
            })
            .collect();
        let target = |value: &Value| reference(value).map(TargetKind::Entity);
        let manager = SpawnManagerState {
            spawn_type: crate::sim::intern::StringInterner::default().intern("SPAWN"),
            missile_family: family,
            regen_rate: int(&manager["regen"]) as u32,
            reload_rate: int(&manager["reload"]) as u32,
            kamikaze_wait_frames: family
                .map(|family| missile_rules.kamikaze_wait_frames(family))
                .unwrap_or(0),
            slots,
            update_timer: timer(&manager["update_timer"]),
            spawn_timer: timer(&manager["spawn_timer"]),
            current_target: target(&manager["target"]),
            queued_target: target(&manager["new_target"]),
            mode: mode_of(int(&manager["status"])),
        };
        let owner = input["owner"].as_object().unwrap().clone();
        let children: Vec<Child> = input["children"]
            .as_array()
            .unwrap()
            .iter()
            .map(|child| Child {
                health: int(&child["health"]),
                estimated: int(&child["health"]),
                ammo: int(&child["ammo"]),
                spawn_owner: false,
                tracked: int(&child["tracked"]) != 0,
                cell: (int(&child["cell"][0]), int(&child["cell"][1])),
                z: int(&child["location"][2]),
                other_type: child["type"].as_str() == Some("other"),
            })
            .collect();
        let spares =
            (children.len() - input["spares"].as_u64().unwrap() as usize..children.len()).collect();
        Self {
            row,
            frame: 0,
            manager,
            owner_burst: int(&owner["burst_index"]),
            owner,
            children,
            spares,
            kamikaze_timer: (0, 0),
            events: Vec::new(),
        }
    }

    fn owner_int(&self, key: &str) -> i32 {
        match &self.owner[key] {
            Value::Bool(flag) => i32::from(*flag),
            value => int(value),
        }
    }

    fn child(&self, id: u64) -> &Child {
        &self.children[(id - CHILD_BASE) as usize]
    }

    fn child_mut(&mut self, id: u64) -> &mut Child {
        &mut self.children[(id - CHILD_BASE) as usize]
    }

    fn spawn_type(&self) -> &Value {
        &self.row["input"]["spawn_type"]
    }

    fn log(&mut self, event: Value) {
        let mut entry = vec![json!(self.frame)];
        entry.extend(event.as_array().unwrap().iter().cloned());
        self.events.push(Value::Array(entry));
    }

    fn change_world(&mut self, step: &Value) {
        for (key, value) in step["owner"].as_object().unwrap() {
            self.owner.insert(key.clone(), value.clone());
        }
        for (index, fields) in step["children"].as_object().unwrap() {
            let child = &mut self.children[index.parse::<usize>().unwrap()];
            for (key, value) in fields.as_object().unwrap() {
                match key.as_str() {
                    "health" => child.health = int(value),
                    "ammo" => child.ammo = int(value),
                    "cell" => child.cell = (int(&value[0]), int(&value[1])),
                    "location" => child.z = int(&value[2]),
                    "tracked" => child.tracked = int(value) != 0,
                    other => panic!("child field {other}"),
                }
            }
        }
    }

    fn state(&self) -> Value {
        let manager = &self.manager;
        let pair = |timer: CdTimer| json!([timer.start_frame(), timer.duration()]);
        json!({
            "status": match manager.mode {
                SpawnManagerMode::Idle => 0,
                SpawnManagerMode::Launching => 1,
                SpawnManagerMode::Returning => 2,
            },
            "target": target_name(manager.current_target),
            "new_target": target_name(manager.queued_target),
            "update_timer": pair(manager.update_timer),
            "spawn_timer": pair(manager.spawn_timer),
            "nodes": manager.slots.iter().map(|slot| json!({
                "unit": name(slot.spawn),
                "status": status_of(slot.state),
                "timer": pair(slot.timer),
                "missile": i32::from(slot.is_missile_spawn),
            })).collect::<Vec<_>>(),
            "kamikaze_timer": [self.kamikaze_timer.0, self.kamikaze_timer.1],
            "owner_burst": self.owner_burst,
            "children": self.children.iter().map(|child| json!({
                "health": child.health,
                "estimated": child.estimated,
                "ammo": child.ammo,
                "spawn_owner": if child.spawn_owner { json!("owner") } else { Value::Null },
            })).collect::<Vec<_>>(),
        })
    }
}

impl SpawnHost for Replay<'_> {
    fn frame(&self) -> i32 {
        self.frame
    }

    fn manager(&mut self) -> &mut SpawnManagerState {
        &mut self.manager
    }

    fn manager_ref(&self) -> &SpawnManagerState {
        &self.manager
    }

    fn is_owner(&self, id: u64) -> bool {
        id == OWNER
    }

    /// The oracle points Rules' CMislType at the pool's type for a `cmisl`
    /// row only.
    fn pool_is_cmisl(&self) -> bool {
        self.row["input"]["family"].as_str() == Some("cmisl")
    }

    fn pool_is_spawn_missile(&self) -> bool {
        self.manager_ref().missile_family.is_some()
    }

    fn owner_is_moving(&mut self) -> bool {
        self.log(json!(["is_moving"]));
        self.owner_int("moving") != 0
    }

    fn owner_is_moving_now(&mut self) -> bool {
        self.log(json!(["is_moving_now"]));
        self.owner_int("moving_now") != 0
    }

    fn owner_locomotor_swap(&self) -> bool {
        self.owner_int("foot") != 0 && self.owner_int("swap_latch") != 0
    }

    fn owner_missile_spawn(&self) -> bool {
        self.owner_int("missile_spawn") != 0
    }

    fn owner_weapon_burst(&self) -> Option<i32> {
        self.owner["weapon"].get("burst").map(int)
    }

    fn owner_health(&self) -> i32 {
        self.owner_int("health")
    }

    fn owner_is_alive(&self) -> bool {
        self.owner_int("alive_byte") != 0
    }

    fn owner_cell(&self) -> (i32, i32) {
        let cell = &self.owner["cell"];
        (int(&cell[0]), int(&cell[1]))
    }

    fn can_fire_at(&mut self, target: TargetKind) -> bool {
        self.log(json!(["can_fire_at", target_name(Some(target))]));
        self.owner_int("can_fire") != 0
    }

    fn child_ammo(&self, child: u64) -> i32 {
        self.child(child).ammo
    }

    fn child_missile_spawn(&self, child: u64) -> bool {
        if self.child(child).other_type {
            self.row["input"]["other_type_missile_spawn"]
                .as_i64()
                .unwrap_or(0)
                != 0
        } else {
            int(&self.spawn_type()["missile_spawn"]) != 0
        }
    }

    fn child_health(&self, child: u64) -> i32 {
        self.child(child).health
    }

    fn child_tracked(&self, child: u64) -> bool {
        self.child(child).tracked
    }

    fn child_cell(&self, child: u64) -> (i32, i32) {
        self.child(child).cell
    }

    fn child_height_over_owner(&self, child: u64) -> i32 {
        self.child(child).z - int(&self.owner["location"][2])
    }

    fn launch(&mut self, child: u64, burst_parity: Option<i32>) {
        self.log(json!(["launch", name(Some(child)), burst_parity]));
    }

    fn reset_owner_burst(&mut self) {
        self.owner_burst = 0;
    }

    fn takeoff_anim(&mut self, _child: u64) {
        self.log(json!(["takeoff_anim"]));
    }

    fn set_destination(&mut self, child: u64, destination: SpawnDestination) {
        let destination = match destination {
            SpawnDestination::Target(target) => target_name(target),
            SpawnDestination::Adjacent(direction) => {
                let (x, y) = self.owner_cell();
                let (x, y) = adjacent_cell(x as u16, y as u16, direction);
                json!(["cell", x, y])
            }
            SpawnDestination::Owner => json!("owner"),
        };
        self.log(json!([
            "set_destination",
            name(Some(child)),
            destination,
            1
        ]));
    }

    fn queue_mission(&mut self, child: u64, mission: MissionType) {
        let mission = crate::sim::mission::MissionId::from_known(mission).raw();
        self.log(json!(["queue_mission", name(Some(child)), mission, 0]));
    }

    fn assign_target(&mut self, child: u64, target: Option<TargetKind>) {
        self.log(json!([
            "assign_target",
            name(Some(child)),
            target_name(target)
        ]));
    }

    fn limbo(&mut self, child: u64) {
        self.log(json!(["limbo", name(Some(child))]));
    }

    fn refill(&mut self, child: u64) {
        let (ammo, strength) = if self.child(child).other_type {
            (1, 1)
        } else {
            let kind = self.spawn_type();
            (int(&kind["ammo"]), int(&kind["strength"]))
        };
        let child = self.child_mut(child);
        child.ammo = ammo;
        child.health = strength;
        child.estimated = strength;
    }

    fn create_child(&mut self) -> Option<u64> {
        let index = self.spares.pop_front().expect("a spare child");
        let child = CHILD_BASE + index as u64;
        self.log(json!(["create_object", name(Some(child))]));
        Some(child)
    }

    fn adopt(&mut self, child: u64) {
        self.child_mut(child).spawn_owner = true;
    }

    fn uninit(&mut self, child: u64) {
        self.log(json!(["uninit", name(Some(child))]));
    }

    fn push(&mut self, child: u64, target: Option<TargetKind>) {
        self.log(json!(["push", name(Some(child)), target_name(target)]));
        if self.child_missile_spawn(child) {
            let child = self.child_mut(child);
            child.tracked = true;
            child.ammo = 1;
        }
    }

    fn restart_kamikaze(&mut self) {
        self.kamikaze_timer = (self.frame, 2);
    }

    fn kamikaze_remove(&mut self, child: u64) {
        self.log(json!(["kamikaze_remove", name(Some(child))]));
    }
}

/// Every row of the original's runs, step by step.
#[test]
fn spawn_manager_matches_the_original_step_by_step() {
    let rows: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rocket_oracle/spawn_manager.json",
    ))
    .unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 19);
    for row in rows {
        let label = row["name"].as_str().unwrap();
        let mut replay = Replay::new(row);
        let start = &row["output"]["start"];
        assert_eq!(replay.state(), *start, "{label}: the supplied start");
        let script = row["input"]["script"].as_array().unwrap();
        let steps = row["output"]["steps"].as_array().unwrap();
        assert_eq!(script.len(), steps.len());
        for (index, (step, native)) in script.iter().zip(steps).enumerate() {
            replay.events.clear();
            match step["op"].as_str().unwrap() {
                "ai" => {
                    for frame in int(&step["frames"][0])..=int(&step["frames"][1]) {
                        replay.frame = frame;
                        ai(&mut replay);
                    }
                }
                "set_target" => {
                    replay.frame = int(&step["frame"]);
                    let target = reference(&step["target"]).map(TargetKind::Entity);
                    replay.manager.set_target(target);
                }
                "pointer_expired" => {
                    replay.frame = int(&step["frame"]);
                    pointer_expired(&mut replay, reference(&step["pointer"]).unwrap());
                }
                "clear" => {
                    replay.frame = int(&step["frame"]);
                    clear_all_targets(&mut replay);
                }
                "world" => replay.change_world(step),
                other => panic!("{label}: op {other}"),
            }
            let mut actual = replay.state();
            actual["events"] = Value::Array(replay.events.clone());
            let mut expected = native.clone();
            expected.as_object_mut().unwrap().remove("detail");
            assert_eq!(actual, expected, "{label} step {index} ({step})");
        }
    }
}
