//! The kamikaze tracker: missiles that have left their launcher's control.
//!
//! Native owner: the global at `0x00ABC5F8` (static initializer
//! `0x0054E260`), a frame timer and a vector of nodes, each a missile and the
//! cell it flies at. Here [`KamikazeTracker`] is `Simulation::kamikaze`;
//! only this module writes it.
//!
//! - Push (`SpawnRetreat__Push @ 0x0054E3B0`): a child whose type lacks
//!   `MissileSpawn=` (`+0xD68`) takes `Crash(NULL)` (vt+0x3DC) and is not
//!   kept. Otherwise the node's cell is the target's GetCoords cell, each
//!   axis over 256 toward zero, or, with no target, the cell next to the
//!   child's occupied cell in its current facing (`((Current >> 12) + 1) >>
//!   1 & 7` through `CellClass::Adjacent_Cell @ 0x00481810`); the child gets
//!   `+0x6CA = 1` and Ammo 1, and the node is appended.
//! - Callers: SpawnManagerClass::AI's Launching pass (`0x006B7A37`) and
//!   ClearAllTargets (`0x006B7BEB`) push a state-2 child whose type sets
//!   `MissileSpawn=`, then restart the timer for 2 frames. Kill_All_Spawns
//!   (`0x006B71B7`) pushes each child out in states 2..5 without a restart.
//! - Update (`Kamikaze__Update @ 0x0054E4D0`), from LogicClass::AI right
//!   after the BombList (`0x0055B4F0`), before the object AI: once the timer
//!   runs out it restarts for 30 frames and gives each node's child, in
//!   order, Ammo 1, `Assign_Target(cell)` (vt+0x3C8) and
//!   `Queue_Mission(Attack, 0)` (vt+0x1E8). That target keeps the missile
//!   from the leave-map removal (`AircraftClass @ 0x0041B890` needs a NULL
//!   target).
//! - Remove (`0x0054E590`), from DispatchPointerExpiredCleanup after the
//!   listeners and the BombList (`0x00725972`) and from Kill_All_Spawns
//!   before it UnInits a missile still in its launch window (`0x006B7162`):
//!   the last node whose child is the pointer is dropped.
//! - Clear (`0x0054E6F0`) drops every node and restarts the timer for 1
//!   frame: at the scenario start (`Clear_Scene` `0x00685529`, after
//!   `Read_Scenario` zeroes the frame at `0x0068466B`) and before a load
//!   reads the nodes (`0x0067F6BE`, after the frame is restored). Save
//!   (`0x0054E750`) writes the nodes, not the timer.
//! - `+0x6CA` has no field here: the tracker's membership stands in for it
//!   (`spawn_manager::notify_pointer_expired`, the only active reader of a
//!   missile's; `AircraftClass::Fire_At @ 0x0041659E` and
//!   `TechnoClass::SelectWeaponAgainst @ 0x006F3658` read it for a weapon,
//!   and no `MissileSpawn=` type in retail rules has one).
//!
//! Scenario draws: none of its own.
//!
//! Evidence: `tools/rocket_oracle/kamikaze.py` runs the original Push,
//! Update, Remove and Clear; `tests::the_tracker_matches_the_native_corpus`
//! replays every step through [`push`] and [`update`].
//!
//! RESIDUALS:
//! - Remove's cell arm (`0x0054E60A..0x0054E681`): a node whose cell is the
//!   expired pointer re-targets its child at that cell (Assign_Target, then
//!   Queue_Mission(Attack)) and keeps it. VERA announces no cell expiry
//!   (native does for a destroyed wall or overlay,
//!   `CellClass::DestroyOverlay @ 0x00481019`), so nothing drops the child's
//!   cell target for the arm to restore. Trigger: a wall or overlay destroyed
//!   on a missile's tracker cell. Effect: native re-issues the pair at once;
//!   here the next Update does. Frequency: rare.
//! - A cell off the map's cell array: `MapClass::GetCellAt @ 0x005657A0`
//!   answers the one dummy cell (`0x00ABDC50`) carrying the last missed
//!   coordinate, which `AircraftClass::AI` drops as a target
//!   (`aircraft::leave_map`'s residual). Here a node keeps its own coordinate
//!   and assigns no target below zero. Trigger: a missile at the array's
//!   edge facing out with no target. Frequency: none on stock maps.

use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::timer::CdTimer;
use crate::sim::world::Simulation;
use crate::util::lepton::lepton_to_cell_packed;

/// Update's restart (`0x0054E500`).
const UPDATE_PERIOD_FRAMES: i32 = 30;
/// The Push callers' restart (`0x006B7A41`, `0x006B7BF5`).
const PUSH_RESTART_FRAMES: i32 = 2;
/// Clear's restart (`0x0054E731`).
const CLEAR_RESTART_FRAMES: i32 = 1;

/// The tracker at `0x00ABC5F8`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct KamikazeTracker {
    /// `+0x00..+0x08`. Not saved: a load restarts it ([`Self::restart_after_load`]).
    #[serde(skip)]
    timer: CdTimer,
    /// The vector at `+0x0C`, in push order.
    nodes: Vec<KamikazeNode>,
}

/// One node (`operator new(8)` at `0x0054E3E3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct KamikazeNode {
    /// `+0x0`: the missile.
    child: u64,
    /// `+0x4`: its cell, as the CellClass's `+0x24` coordinate.
    cell: (i16, i16),
}

impl Default for KamikazeTracker {
    /// Clear at the scenario start, frame 0.
    fn default() -> Self {
        Self {
            timer: CdTimer::started(0, CLEAR_RESTART_FRAMES),
            nodes: Vec::new(),
        }
    }
}

impl KamikazeTracker {
    /// The `{Frame, 2}` the Launching pass and ClearAllTargets write after
    /// their Push.
    fn restart_after_push(&mut self, frame: u32) {
        self.timer = CdTimer::started(frame as i32, PUSH_RESTART_FRAMES);
    }

    /// Remove (`0x0054E590`), child arm: from the back, the first node whose
    /// child is `child` goes.
    pub(crate) fn remove(&mut self, child: u64) {
        if let Some(index) = self.nodes.iter().rposition(|node| node.child == child) {
            self.nodes.remove(index);
        }
    }

    /// Clear (`0x0054E6F0`).
    #[cfg(test)]
    fn clear(&mut self, frame: u32) {
        self.nodes.clear();
        self.timer = CdTimer::started(frame as i32, CLEAR_RESTART_FRAMES);
    }

    /// Load (`0x0054E7B0`) follows Clear, so the saved nodes come back with
    /// the timer restarted at the restored frame.
    pub(crate) fn restart_after_load(&mut self, frame: u32) {
        self.timer = CdTimer::started(frame as i32, CLEAR_RESTART_FRAMES);
    }

    /// Whether `child` has a node: its `+0x6CA` while it is alive.
    pub(crate) fn contains(&self, child: u64) -> bool {
        self.nodes.iter().any(|node| node.child == child)
    }

    /// Folds the nodes, with the timer that phases them, into the world hash.
    pub(crate) fn fold_hash(&self, hasher: &mut impl Hasher) {
        // An empty tracker's timer phases only a later Kill_All_Spawns push,
        // whose effects the hash then sees on the missile.
        if self.nodes.is_empty() {
            return;
        }
        b"kamikaze-tracker-v1".hash(hasher);
        self.timer.hash(hasher);
        self.nodes.hash(hasher);
    }
}

/// The calls Push and Update make on the objects.
pub(crate) trait KamikazeHost {
    fn tracker(&mut self) -> &mut KamikazeTracker;
    /// The child's type `MissileSpawn=` (`+0xD68`).
    fn missile_spawn(&self, child: u64) -> bool;
    /// vt+0x3DC `Crash(NULL)`.
    fn crash(&mut self, child: u64);
    /// The child's Location X and Y (`+0x9C`), which GetOccupiedCell
    /// (vt+0x1BC, `0x005F6960`) reads.
    fn location_xy(&self, child: u64) -> [i32; 2];
    /// `FacingClass::Current` on the child's `+0x388`.
    fn facing(&self, child: u64) -> u16;
    /// Push's `+0x6CA = 1` and Ammo (`+0x2FC`) = 1 (`0x0054E478..0x0054E483`).
    fn arm(&mut self, child: u64);
    /// Update's Ammo = 1 (`0x0054E524`).
    fn reload(&mut self, child: u64);
    /// vt+0x3C8 `Assign_Target(cell)`.
    fn assign_target(&mut self, child: u64, cell: (i16, i16));
    /// vt+0x1E8 `Queue_Mission(Attack, 0)`.
    fn queue_attack(&mut self, child: u64);
}

/// Push (`0x0054E3B0`). `target_xy` is the target's GetCoords X and Y
/// (vt+0x48), or `None` for a NULL target.
pub(crate) fn push(host: &mut impl KamikazeHost, child: u64, target_xy: Option<[i32; 2]>) {
    if !host.missile_spawn(child) {
        host.crash(child);
        return;
    }
    let cell = match target_xy {
        Some([x, y]) => (lepton_to_cell_packed(x), lepton_to_cell_packed(y)),
        None => {
            let [x, y] = host.location_xy(child);
            let direction = ((u32::from(host.facing(child)) >> 12) + 1) >> 1;
            let (dx, dy) = crate::util::direction_tables::CELL_DELTAS[(direction & 7) as usize];
            // Adjacent_Cell adds the step to the occupied cell's 16-bit coordinate.
            (
                lepton_to_cell_packed(x).wrapping_add(dx as i16),
                lepton_to_cell_packed(y).wrapping_add(dy as i16),
            )
        }
    };
    host.arm(child);
    host.tracker().nodes.push(KamikazeNode { child, cell });
}

/// Update (`0x0054E4D0`). Native reads the node count again after each node.
pub(crate) fn update(host: &mut impl KamikazeHost, frame: u32) {
    let tracker = host.tracker();
    if !tracker.timer.expired(frame as i32) {
        return;
    }
    tracker.timer = CdTimer::started(frame as i32, UPDATE_PERIOD_FRAMES);
    let mut index = 0;
    while let Some(node) = host.tracker().nodes.get(index).copied() {
        // A NULL cell would take the facing step again (`0x0054E530`); Push
        // never stores one.
        host.reload(node.child);
        host.assign_target(node.child, node.cell);
        host.queue_attack(node.child);
        index += 1;
    }
}

struct WorldHost<'a> {
    sim: &'a mut Simulation,
    rules: &'a RuleSet,
    registry: Option<&'a OverlayTypeRegistry>,
}

impl KamikazeHost for WorldHost<'_> {
    fn tracker(&mut self) -> &mut KamikazeTracker {
        &mut self.sim.kamikaze
    }

    fn missile_spawn(&self, child: u64) -> bool {
        self.sim
            .substrate
            .entities
            .get(child)
            .and_then(|entity| self.sim.object_type(entity.type_ref(), self.rules))
            .is_some_and(|kind| kind.missile_spawn)
    }

    fn crash(&mut self, child: u64) {
        self.sim.foot_crash(child, None, self.rules, self.registry);
    }

    fn location_xy(&self, child: u64) -> [i32; 2] {
        self.sim
            .substrate
            .entities
            .get(child)
            .map(|entity| crate::sim::movement::ground_pose::position_world_xy(&entity.position))
            .unwrap_or_default()
    }

    fn facing(&self, child: u64) -> u16 {
        self.sim
            .substrate
            .entities
            .get(child)
            .map(|entity| entity.body_facing_current(self.sim.session.binary_frame))
            .unwrap_or_default()
    }

    fn arm(&mut self, child: u64) {
        self.reload(child);
    }

    fn reload(&mut self, child: u64) {
        if let Some(ammo) = self
            .sim
            .substrate
            .entities
            .get_mut(child)
            .and_then(|entity| entity.aircraft_ammo.as_mut())
        {
            ammo.current = 1;
        }
    }

    fn assign_target(&mut self, child: u64, (x, y): (i16, i16)) {
        let cell = u16::try_from(x)
            .ok()
            .zip(u16::try_from(y).ok())
            .map(|(x, y)| TargetKind::Cell(x, y));
        let _ = self
            .sim
            .assign_target_represented(child, cell, Some(self.rules));
    }

    fn queue_attack(&mut self, child: u64) {
        let now = self.sim.session.binary_frame;
        let _ = self.sim.mission_queue_exact(
            child,
            MissionId::from_known(MissionType::Attack),
            0,
            now,
            &crate::sim::mission::authority::EntityReadyInputProvider,
        );
    }
}

impl Simulation {
    /// Push (`0x0054E3B0`) with the target's GetCoords. A spawn manager
    /// drops a target at its pointer expiry, so a Push never names a missing
    /// object.
    pub(crate) fn kamikaze_push(
        &mut self,
        child: u64,
        target: Option<TargetKind>,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let target_xy = target.and_then(|target| {
            let cells = self
                .resolved_terrain
                .as_ref()
                .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
            crate::sim::movement::ground_pose::target_get_coords(
                target,
                &self.substrate.entities,
                cells.as_ref(),
            )
            .map(|coord| [coord.x, coord.y])
        });
        push(
            &mut WorldHost {
                sim: self,
                rules,
                registry,
            },
            child,
            target_xy,
        );
    }

    /// The `{Frame, 2}` restart the Launching pass and ClearAllTargets write
    /// after their Push.
    pub(crate) fn kamikaze_restart_after_push(&mut self) {
        let frame = self.session.binary_frame;
        self.kamikaze.restart_after_push(frame);
    }

    /// Update (`0x0054E4D0`), from LogicClass::AI after the BombList.
    pub(crate) fn kamikaze_update(&mut self, rules: &RuleSet) {
        let frame = self.session.binary_frame;
        update(
            &mut WorldHost {
                sim: self,
                rules,
                registry: None,
            },
            frame,
        );
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::sim::movement::facing_class::FacingClass;

    struct Child {
        missile_spawn: bool,
        location: [i32; 2],
        facing: FacingClass,
        ammo: i64,
        flag: i64,
    }

    struct FixtureHost {
        tracker: KamikazeTracker,
        children: Vec<Child>,
        frame: u32,
        events: Vec<Value>,
    }

    impl KamikazeHost for FixtureHost {
        fn tracker(&mut self) -> &mut KamikazeTracker {
            &mut self.tracker
        }
        fn missile_spawn(&self, child: u64) -> bool {
            self.children[child as usize].missile_spawn
        }
        fn crash(&mut self, child: u64) {
            self.events.push(json!(["crash", child, 0]));
        }
        fn location_xy(&self, child: u64) -> [i32; 2] {
            self.children[child as usize].location
        }
        fn facing(&self, child: u64) -> u16 {
            self.children[child as usize].facing.current(self.frame)
        }
        fn arm(&mut self, child: u64) {
            let child = &mut self.children[child as usize];
            child.ammo = 1;
            child.flag = 1;
        }
        fn reload(&mut self, child: u64) {
            self.children[child as usize].ammo = 1;
        }
        fn assign_target(&mut self, child: u64, cell: (i16, i16)) {
            self.events
                .push(json!(["assign_target", child, [cell.0, cell.1]]));
        }
        fn queue_attack(&mut self, child: u64) {
            self.events.push(json!(["queue_mission", child, 1, 0]));
        }
    }

    fn int(value: &Value) -> i64 {
        value.as_i64().expect("integer")
    }

    fn observed(host: &FixtureHost) -> Value {
        json!({
            "nodes": host.tracker.nodes.iter()
                .map(|node| json!([node.child, [node.cell.0, node.cell.1]]))
                .collect::<Vec<_>>(),
            "timer": [host.tracker.timer.start_frame(), host.tracker.timer.duration()],
            "children": host.children.iter()
                .map(|child| json!([child.ammo, child.flag]))
                .collect::<Vec<_>>(),
        })
    }

    /// Parity with `tools/rocket_oracle/kamikaze.json`: the original Push,
    /// Update, Remove and Clear, every node, timer, Ammo, `+0x6CA` and seam
    /// call after each step.
    #[test]
    fn the_tracker_matches_the_native_corpus() {
        let rows: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/rocket_oracle/kamikaze.json",
        ))
        .expect("corpus parses");
        let rows = rows.as_array().expect("row list");
        assert_eq!(rows.len(), 7);
        for row in rows {
            let name = row["name"].as_str().expect("row name");
            let input = &row["input"];
            let children = input["children"]
                .as_array()
                .expect("children")
                .iter()
                .map(|child| {
                    let frame = int(&child["facing_frame"]) as u32;
                    let mut facing = FacingClass::new(0, int(&child["rot"]) as i32);
                    facing.snap(int(&child["facing"]) as u16, frame);
                    if !child["turn_to"].is_null() {
                        facing.set(int(&child["turn_to"]) as u16, frame);
                    }
                    let location = child["location"].as_array().expect("location");
                    Child {
                        missile_spawn: child["missile_spawn"].as_bool().expect("flag"),
                        location: [int(&location[0]) as i32, int(&location[1]) as i32],
                        facing,
                        ammo: int(&child["ammo"]),
                        flag: 0,
                    }
                })
                .collect();
            let targets = input["targets"].as_array().expect("targets");
            let mut host = FixtureHost {
                tracker: KamikazeTracker::default(),
                children,
                frame: 0,
                events: Vec::new(),
            };
            let output = &row["output"];
            assert_eq!(observed(&host), output["start"], "{name}: start");
            let script = input["script"].as_array().expect("script");
            let steps = output["steps"].as_array().expect("steps");
            assert_eq!(script.len(), steps.len(), "{name}: step count");
            for (index, (step, expected)) in script.iter().zip(steps).enumerate() {
                host.frame = int(&step["frame"]) as u32;
                host.events.clear();
                match step["op"].as_str().expect("op") {
                    "push" => {
                        let target_xy = step["target"].as_i64().map(|target| {
                            let coords = targets[target as usize].as_array().expect("target");
                            [int(&coords[0]) as i32, int(&coords[1]) as i32]
                        });
                        push(&mut host, int(&step["child"]) as u64, target_xy);
                        if step["restart"].as_bool().expect("restart") {
                            host.tracker.restart_after_push(host.frame);
                        }
                    }
                    "update" => {
                        let frame = host.frame;
                        update(&mut host, frame);
                    }
                    "remove" => {
                        let pointer = step["pointer"].as_array().expect("pointer");
                        // A target is no child; its pointer matches no node.
                        if pointer[0] == "child" {
                            host.tracker.remove(int(&pointer[1]) as u64);
                        }
                    }
                    "clear" => {
                        let frame = host.frame;
                        host.tracker.clear(frame);
                    }
                    op => panic!("{name}: unknown op {op}"),
                }
                let mut actual = observed(&host);
                actual["events"] = json!(host.events);
                assert_eq!(&actual, expected, "{name}: step {index} {step}");
            }
        }
    }

    #[test]
    fn remove_drops_the_last_matching_node_only() {
        let mut tracker = KamikazeTracker::default();
        for (child, cell) in [(7, (1, 1)), (8, (2, 2)), (7, (3, 3))] {
            tracker.nodes.push(KamikazeNode { child, cell });
        }
        tracker.remove(7);
        assert_eq!(
            tracker.nodes,
            [
                KamikazeNode {
                    child: 7,
                    cell: (1, 1)
                },
                KamikazeNode {
                    child: 8,
                    cell: (2, 2)
                }
            ]
        );
        assert!(tracker.contains(7));
        tracker.remove(9);
        assert_eq!(tracker.nodes.len(), 2);
    }
}
