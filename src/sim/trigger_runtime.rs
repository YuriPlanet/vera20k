//! Minimal runtime evaluation for map-authored triggers.
//!
//! This is intentionally narrow: it executes a small, high-value subset of
//! RA2/YR trigger behavior that already fits the current engine structure.
//! Supported today:
//! - Event 47: elapsed scenario time
//! - Event 27/28 and 36/37: global/local variable set/clear
//! - Action 22: force trigger
//! - Action 28/29: set/clear global variable
//! - Action 40: change visible map area
//! - Action 53/54: enable/disable trigger
//! - Action 48/112: center camera at waypoint
//! - Action 137/138: set/clear a House's alternate base cell
//!
//! Simulation owns construction and dispatch; TriggerRuntime stores its private
//! serialized state. Live Tag instances, timers and native polling order remain
//! to replace the legacy definition queue below.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::hash::{Hash, Hasher};

use crate::map::actions::ActionEntry;
use crate::map::events::{EventCondition, EventMap};
use crate::map::trigger_graph::LinkedTrigger;
use crate::map::triggers::TriggerMap;
use crate::map::variable_names::LocalVariableMap;
use crate::sim::world::{Simulation, TriggerInputs};

const ACTION_FORCE_TRIGGER: i32 = 22;
const ACTION_SET_GLOBAL: i32 = 28;
const ACTION_CLEAR_GLOBAL: i32 = 29;
const ACTION_CHANGE_VISIBLE_MAP_AREA: i32 = 40;
const ACTION_CENTER_CAMERA: i32 = 48;
const ACTION_ENABLE_TRIGGER: i32 = 53;
const ACTION_DISABLE_TRIGGER: i32 = 54;
const ACTION_SET_LOCAL: i32 = 56;
const ACTION_CLEAR_LOCAL: i32 = 57;
const ACTION_ANNOUNCE_WIN: i32 = 67;
const ACTION_ANNOUNCE_LOSE: i32 = 68;
const ACTION_END_SCENARIO: i32 = 69;
const ACTION_JUMP_CAMERA: i32 = 112;
const ACTION_SET_ALTERNATE_BASE: i32 = 137;
const ACTION_CLEAR_ALTERNATE_BASE: i32 = 138;

const EVENT_GLOBAL_IS_SET: i32 = 27;
const EVENT_GLOBAL_IS_CLEAR: i32 = 28;
const EVENT_LOCAL_IS_SET: i32 = 36;
const EVENT_LOCAL_IS_CLEAR: i32 = 37;
const EVENT_ELAPSED_SCENARIO_TIME: i32 = 47;
const EVENT_TECHTYPE_EXISTS: i32 = 60;
const EVENT_TECHTYPE_DOES_NOT_EXIST: i32 = 61;
const LOGICAL_FRAMES_PER_SECOND: i32 = crate::util::fixed_math::RA2_LOGIC_FRAMES_PER_SECOND as i32;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TriggerEffect {
    CenterCameraAtWaypoint { waypoint: u32, immediate: bool },
    MissionAnnouncement { text: String },
    MissionResult { title: String, detail: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum MissionAnnouncementKind {
    Victory,
    Defeat,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TriggerRuntime {
    /// `BTreeSet` (not `HashSet`) so save files have a deterministic iteration
    /// order — required for replay/lockstep correctness.
    globals_set: BTreeSet<u32>,
    locals_set: BTreeSet<u32>,
    disabled_triggers: BTreeSet<String>,
    fired_one_shot_triggers: BTreeSet<String>,
    last_announcement: Option<MissionAnnouncementKind>,
}

impl TriggerRuntime {
    /// Fold mutable trigger state into the simulation's lockstep hash.
    ///
    /// The map definitions are static input, but these latches decide which
    /// later YR LogicClass trigger actions may run after a save or replay.
    pub(crate) fn hash_state(&self, hasher: &mut impl Hasher) {
        self.globals_set.len().hash(hasher);
        for value in &self.globals_set {
            value.hash(hasher);
        }

        self.locals_set.len().hash(hasher);
        for value in &self.locals_set {
            value.hash(hasher);
        }

        self.disabled_triggers.len().hash(hasher);
        for value in &self.disabled_triggers {
            value.hash(hasher);
        }

        self.fired_one_shot_triggers.len().hash(hasher);
        for value in &self.fired_one_shot_triggers {
            value.hash(hasher);
        }

        match self.last_announcement {
            None => 0u8.hash(hasher),
            Some(MissionAnnouncementKind::Victory) => 1u8.hash(hasher),
            Some(MissionAnnouncementKind::Defeat) => 2u8.hash(hasher),
        }
    }

    fn from_map(triggers: &TriggerMap, local_variables: &LocalVariableMap) -> Self {
        let mut runtime = TriggerRuntime::default();
        for trigger in triggers.values() {
            if !trigger.enabled || !trigger.difficulty.medium {
                runtime.disabled_triggers.insert(trigger.id.clone());
            }
        }
        for local in local_variables.values() {
            if local.initially_set {
                runtime.locals_set.insert(local.index);
            }
        }
        runtime
    }

    fn is_trigger_ready(
        &self,
        linked: &LinkedTrigger,
        triggers: &TriggerMap,
        events: &EventMap,
        current_frame: u32,
        simulation: &Simulation,
    ) -> bool {
        if self.disabled_triggers.contains(&linked.trigger_id) {
            return false;
        }

        let Some(trigger) = triggers.get(&linked.trigger_id) else {
            return false;
        };
        if !trigger.repeating && self.fired_one_shot_triggers.contains(&linked.trigger_id) {
            return false;
        }

        let Some(event_id) = &linked.event_id else {
            return false;
        };
        let Some(event) = events.get(event_id) else {
            return false;
        };
        !event.conditions.is_empty()
            && event
                .conditions
                .iter()
                .all(|condition| self.evaluate_event(condition, current_frame, simulation))
    }

    fn evaluate_event(
        &self,
        condition: &EventCondition,
        current_frame: u32,
        simulation: &Simulation,
    ) -> bool {
        match condition.kind {
            EVENT_ELAPSED_SCENARIO_TIME => {
                condition.value <= (current_frame as i32) / LOGICAL_FRAMES_PER_SECOND
            }
            EVENT_GLOBAL_IS_SET
            | EVENT_GLOBAL_IS_CLEAR
            | EVENT_LOCAL_IS_SET
            | EVENT_LOCAL_IS_CLEAR => {
                let global = matches!(condition.kind, EVENT_GLOBAL_IS_SET | EVENT_GLOBAL_IS_CLEAR);
                let limit = if global { 50 } else { 100 };
                if !(0..limit).contains(&condition.value) {
                    // Native Get689760/689A00 leaves an uninitialized output
                    // byte on invalid indices. Reject malformed conditions;
                    // stack residue is not a deterministic gameplay contract.
                    return false;
                }
                let values = if global {
                    &self.globals_set
                } else {
                    &self.locals_set
                };
                values.contains(&(condition.value as u32))
                    == matches!(condition.kind, EVENT_GLOBAL_IS_SET | EVENT_LOCAL_IS_SET)
            }
            EVENT_TECHTYPE_EXISTS => {
                let sim = simulation;
                let min_count = condition.value;
                let Some(type_id) = condition.type_name.as_deref() else {
                    return false;
                };
                if type_id.is_empty() {
                    return false;
                }
                // Native71EA03 tests the signed threshold inside its scan:
                // even a nonpositive threshold requires at least one Techno.
                !sim.entities().is_empty()
                    && count_techtype(sim, type_id) as i64 >= i64::from(min_count)
            }
            EVENT_TECHTYPE_DOES_NOT_EXIST => {
                let sim = simulation;
                let Some(type_id) = condition.type_name.as_deref() else {
                    return false;
                };
                if type_id.is_empty() {
                    return false;
                }
                count_techtype(sim, type_id) == 0
            }
            _ => false,
        }
    }
}

impl Simulation {
    /// Seed map trigger state on the staged Simulation before objects are loaded.
    /// App and headless construction share this call; replacing/restoring the
    /// Simulation preserves its serialized state instead of reinitializing it.
    pub(crate) fn initialize_map_triggers(
        &mut self,
        triggers: &TriggerMap,
        local_variables: &LocalVariableMap,
    ) {
        self.trigger_runtime = TriggerRuntime::from_map(triggers, local_variables);
    }

    /// Execute map actions on the installed Simulation owner. Short runtime
    /// borrows keep nested gameplay callbacks on this same authoritative state.
    /// The live Tag/Trigger migration must replace the legacy definition queue
    /// below without ever lifting TriggerRuntime out of Simulation.
    pub(crate) fn advance_triggers(&mut self, inputs: TriggerInputs<'_>) -> Vec<TriggerEffect> {
        let TriggerInputs {
            graph,
            triggers,
            events,
            actions,
            waypoints,
            rules,
        } = inputs;
        let current_frame = self.session.binary_frame;
        let linked_by_id: BTreeMap<&str, &LinkedTrigger> = graph
            .triggers
            .iter()
            .map(|linked| (linked.trigger_id.as_str(), linked))
            .collect();

        let mut queue: VecDeque<String> = graph
            .triggers
            .iter()
            .filter(|linked| {
                self.trigger_runtime
                    .is_trigger_ready(linked, triggers, events, current_frame, self)
            })
            .map(|linked| linked.trigger_id.clone())
            .collect();
        let mut queued: BTreeSet<String> = queue.iter().cloned().collect();
        let mut effects: Vec<TriggerEffect> = Vec::new();

        while let Some(trigger_id) = queue.pop_front() {
            queued.remove(&trigger_id);
            let Some(trigger) = triggers.get(&trigger_id) else {
                continue;
            };
            let Some(linked) = linked_by_id.get(trigger_id.as_str()).copied() else {
                continue;
            };
            if !self
                .trigger_runtime
                .is_trigger_ready(linked, triggers, events, current_frame, self)
            {
                continue;
            }

            if let Some(action) = actions.get(&trigger_id) {
                for entry in &action.entries {
                    self.apply_trigger_action(
                        entry,
                        &mut effects,
                        &mut queue,
                        &mut queued,
                        triggers,
                        trigger,
                        rules,
                        waypoints,
                    );
                }
            }

            if let Some(linked_trigger_id) = &trigger.linked_trigger_id {
                if triggers.contains_key(linked_trigger_id) {
                    enqueue_trigger(&mut queue, &mut queued, linked_trigger_id.clone());
                }
            }

            if !trigger.repeating {
                self.trigger_runtime
                    .fired_one_shot_triggers
                    .insert(trigger_id);
            }
        }

        effects
    }

    fn apply_trigger_action(
        &mut self,
        action: &ActionEntry,
        effects: &mut Vec<TriggerEffect>,
        queue: &mut VecDeque<String>,
        queued: &mut BTreeSet<String>,
        triggers: &TriggerMap,
        trigger: &crate::map::triggers::MapTrigger,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        waypoints: &HashMap<u32, crate::map::waypoints::Waypoint>,
    ) {
        match action.kind {
            ACTION_FORCE_TRIGGER => {
                if let Some(target) = parse_trigger_id_param(&action.params, 0) {
                    enqueue_trigger(queue, queued, target);
                }
            }
            ACTION_SET_GLOBAL | ACTION_CLEAR_GLOBAL | ACTION_SET_LOCAL | ACTION_CLEAR_LOCAL => {
                // gamemd 6DD8B0 cases28/29/56/57 read materialized +90,
                // then Scenario setters689670/689910 reject out-of-range
                // indices. ParamType and the four auxiliary dwords are not
                // variable indices. Live timer-reset fanout is still pending
                // the Tag/Trigger instance migration.
                let global = matches!(action.kind, ACTION_SET_GLOBAL | ACTION_CLEAR_GLOBAL);
                let limit = if global { 50 } else { 100 };
                let Some(index) = action
                    .literal_value()
                    .filter(|index| (0..limit).contains(index))
                else {
                    return;
                };
                let values = if global {
                    &mut self.trigger_runtime.globals_set
                } else {
                    &mut self.trigger_runtime.locals_set
                };
                if matches!(action.kind, ACTION_SET_GLOBAL | ACTION_SET_LOCAL) {
                    values.insert(index as u32);
                } else {
                    values.remove(&(index as u32));
                }
            }
            ACTION_CHANGE_VISIBLE_MAP_AREA => {
                // `TActionClass::Read @ 0x006DD5B0` parses ActionID,
                // ParamType, Param3, then stores the four writer dwords at
                // +0x34..+0x40. ActionEntry.params is chunk[1..], so these are
                // exactly indices 2..5; params[0]/[1] must never leak in.
                if let Some(raw_local_size) = parse_visible_map_area(&action.params) {
                    let _ = self.change_visible_map_area(raw_local_size, rules);
                }
            }
            ACTION_ENABLE_TRIGGER => {
                if let Some(target) = parse_trigger_id_param(&action.params, 0) {
                    self.trigger_runtime.disabled_triggers.remove(&target);
                    if triggers.contains_key(&target) {
                        enqueue_trigger(queue, queued, target);
                    }
                }
            }
            ACTION_DISABLE_TRIGGER => {
                if let Some(target) = parse_trigger_id_param(&action.params, 0) {
                    self.trigger_runtime.disabled_triggers.insert(target);
                }
            }
            ACTION_CENTER_CAMERA => {
                if let Some(waypoint) = action.waypoint_index {
                    effects.push(TriggerEffect::CenterCameraAtWaypoint {
                        waypoint,
                        immediate: false,
                    });
                }
            }
            ACTION_JUMP_CAMERA => {
                if let Some(waypoint) = action.waypoint_index {
                    effects.push(TriggerEffect::CenterCameraAtWaypoint {
                        waypoint,
                        immediate: true,
                    });
                }
            }
            ACTION_SET_ALTERNATE_BASE => {
                let Some(house_id) = resolve_trigger_house(self, trigger.owner.as_deref(), rules)
                else {
                    return;
                };
                let Some(waypoint_index) = action.waypoint_index else {
                    return;
                };
                let Some(waypoint) = waypoints.get(&waypoint_index) else {
                    return;
                };
                if (waypoint.rx, waypoint.ry) == (0, 0) {
                    return;
                }
                // gamemd-derived: `TriggerAction__Execute @ 0x006DD8B0`
                // case 137 calls `FUN_006E44E0`, which rejects the packed-zero
                // waypoint and writes only `HouseClass+0x5494` through
                // `FUN_0050DFE0`.
                if let Some(house) = self.houses.get_mut(&house_id) {
                    house.alternate_base_center = (waypoint.rx, waypoint.ry);
                }
            }
            ACTION_CLEAR_ALTERNATE_BASE => {
                let Some(house_id) = resolve_trigger_house(self, trigger.owner.as_deref(), rules)
                else {
                    return;
                };
                // gamemd-derived: `TriggerAction__Execute @ 0x006DD8B0`
                // case 138 calls `FUN_006E4540`, which writes packed zero only
                // to `HouseClass+0x5494` through `FUN_0050DFF0`.
                if let Some(house) = self.houses.get_mut(&house_id) {
                    house.alternate_base_center = (0, 0);
                }
            }
            ACTION_ANNOUNCE_WIN => {
                self.trigger_runtime.last_announcement = Some(MissionAnnouncementKind::Victory);
                effects.push(TriggerEffect::MissionAnnouncement {
                    text: "Mission Accomplished".to_string(),
                });
            }
            ACTION_ANNOUNCE_LOSE => {
                self.trigger_runtime.last_announcement = Some(MissionAnnouncementKind::Defeat);
                effects.push(TriggerEffect::MissionAnnouncement {
                    text: "Mission Failed".to_string(),
                });
            }
            ACTION_END_SCENARIO => {
                let (title, detail) = match self.trigger_runtime.last_announcement {
                    Some(MissionAnnouncementKind::Victory) => (
                        "Mission Accomplished".to_string(),
                        "The scenario ended after a victory announcement.".to_string(),
                    ),
                    Some(MissionAnnouncementKind::Defeat) => (
                        "Mission Failed".to_string(),
                        "The scenario ended after a defeat announcement.".to_string(),
                    ),
                    None => (
                        "Scenario Ended".to_string(),
                        "A map trigger ended the scenario.".to_string(),
                    ),
                };
                effects.push(TriggerEffect::MissionResult { title, detail });
            }
            _ => {}
        }
    }
}

fn enqueue_trigger(
    queue: &mut VecDeque<String>,
    queued: &mut BTreeSet<String>,
    trigger_id: String,
) {
    if queued.insert(trigger_id.clone()) {
        queue.push_back(trigger_id);
    }
}

/// Resolve a trigger's canonical HouseType owner to the first registered House.
///
/// gamemd-derived: `TriggerTypeClass::Read` canonicalizes the owner through
/// `HouseTypeClass__FindIndexOfName @ 0x005117D0` (alias before ID in source
/// order, with `<none>` selecting index zero). `TriggerClass::Spring @
/// 0x007265C0` passes that type index to `HouseClass__Find_By_Country_Index @
/// 0x00502D30`, whose global House-array scan returns the first matching
/// registration.
fn resolve_trigger_house(
    sim: &Simulation,
    trigger_owner: Option<&str>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> Option<crate::sim::intern::InternedId> {
    let rules = rules?;
    let trigger_house_type = rules.trigger_house_type_index(trigger_owner?)?;
    sim.session.house_order.iter().copied().find(|house_id| {
        sim.houses
            .get(house_id)
            .and_then(|house| house.country)
            .and_then(|country| rules.country_index(sim.interner.resolve(country)))
            == Some(trigger_house_type)
    })
}

fn parse_visible_map_area(fields: &[String]) -> Option<[i32; 4]> {
    Some([
        crate::rules::ini_value::atoi_lenient(fields.get(2)?.trim()),
        crate::rules::ini_value::atoi_lenient(fields.get(3)?.trim()),
        crate::rules::ini_value::atoi_lenient(fields.get(4)?.trim()),
        crate::rules::ini_value::atoi_lenient(fields.get(5)?.trim()),
    ])
}

fn parse_trigger_id_param(fields: &[String], index: usize) -> Option<String> {
    let id = fields.get(index)?.trim();
    (!id.is_empty()).then(|| id.to_ascii_uppercase())
}

fn count_techtype(sim: &Simulation, type_id: &str) -> usize {
    sim.substrate
        .entities
        .values()
        .filter(|e| {
            sim.interner
                .resolve(e.type_ref())
                .eq_ignore_ascii_case(type_id)
        })
        .count()
}

#[cfg(test)]
#[path = "trigger_runtime_tests.rs"]
mod tests;
