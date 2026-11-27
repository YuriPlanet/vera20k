//! Runtime triggers made by Wasm extension modules (RFC 0001 section 7):
//! the trigger runtime's own registry for them, beside the map's.
//!
//! Map triggers are native TriggerType/Tag structures whose events are map
//! text and whose instances hang off Tags. A module trigger instead holds an
//! `EventDef` list with filter clauses, binds to objects and types without
//! taking an object's Tag slot, runs one instance per bound object, and
//! queues module actions as `EventRecord`s. Those have no place in the native
//! structures, so this registry keeps them, and [`super::TriggerRuntime`]
//! saves, hashes and validates it with the rest of the trigger state.
//!
//! Handles are creation serials from the shared object counter
//! (`ObjectSubstrate::allocate_stable_id`), disjoint from map trigger
//! ordinals. The Simulation side that raises events and runs actions is
//! `sim::ext_modules::triggers`.

use std::collections::{BTreeMap, BTreeSet};

use vera20k_wasm_ext::EventRecord;
use vera20k_wasm_ext::decide::EVENT_DEFEAT;
use vera20k_wasm_ext::triggers::{
    ACTION_MODULE, ActionDef, EVENT_DISTANCE_POLL, EVENT_MODIFIER_COUNT_POLL, EVENT_PROPERTY_POLL,
    EVENT_TIMER, EVENT_VARIABLE_POLL, EventDef, Repeat,
};

use crate::sim::ext_modules::ModuleSource;

/// Preview limits per source. Object polls can scan the entire live world
/// before calling Wasm, so they have a smaller limit than other events.
const MAX_TRIGGERS_PER_SOURCE: usize = 256;
const MAX_OBJECT_POLLS_PER_SOURCE: usize = 16;

/// A module may deliver one frame's worth before its backlog is checked.
pub(crate) const MAX_RECORDS_PER_FRAME: usize = 1024;
pub(crate) const MAX_BACKLOG_RECORDS: usize = 16 * MAX_RECORDS_PER_FRAME;
pub(crate) const MAX_QUEUED_RECORDS: usize = MAX_BACKLOG_RECORDS + MAX_RECORDS_PER_FRAME;

fn object_polls(scope: TriggerScope, events: &[EventDef]) -> usize {
    if !matches!(scope, TriggerScope::Object { .. }) {
        return 0;
    }
    events
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                EVENT_DISTANCE_POLL
                    | EVENT_MODIFIER_COUNT_POLL
                    | EVENT_PROPERTY_POLL
                    | EVENT_TIMER
                    | EVENT_VARIABLE_POLL
            )
        })
        .count()
}

/// What a trigger's events bind to (RFC 6a): every event of one trigger
/// shares it, which `trigger_create` checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum TriggerScope {
    /// House, logic and global raised events, `Timer` alone, superweapon
    /// launches and global polls: no binding, one instance.
    Global,
    /// Object events, with or without `Timer`: object, type and tag
    /// bindings. `weapon` when every event also admits weapon, warhead and
    /// projectile type bindings (appendix 4.3 "Object + weapon").
    Object { weapon: bool },
    /// Events marked "cell binding" in appendix 4.1.
    Cell,
}

/// One module trigger's definition and switches.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct ModuleTrigger {
    /// The module that made it, whose queue its module actions fill.
    pub(crate) source: ModuleSource,
    /// The house array index of `TriggerDef.house`; `None` takes the
    /// subject's owner at firing time, or the Neutral house.
    pub(crate) house: Option<u32>,
    pub(crate) repeat: Repeat,
    /// Bit 0 Easy, 1 Normal, 2 Hard.
    pub(crate) difficulty: u8,
    /// `TriggerDef.flags`; allowWin and transferOnCapture are kept but not
    /// acted on yet.
    pub(crate) flags: i32,
    pub(crate) enabled: bool,
    /// The frame `trigger_create` ran, which `Timer` periods count from.
    pub(crate) created: u32,
    /// The frame polling intervals count from: `created`, moved to the
    /// frame of a write to a `VariablePoll` event's variable (appendix 4.4).
    pub(crate) poll_start: u32,
    pub(crate) scope: TriggerScope,
    pub(crate) events: Vec<EventDef>,
    pub(crate) actions: Vec<ActionDef>,
    /// Built from the mod's INI trigger sections
    /// (`sim::ext_modules::ini_triggers`), not by a module: it outlives a
    /// stopped module, which takes only its module actions along.
    #[serde(default)]
    pub(crate) ini: bool,
    /// `TriggerDef.decide_key`: the decision handler's dispatch key, −1
    /// for none. A trigger with one raises decision requests in place of
    /// delivery (RFC 6f, `sim::ext_modules::decide`).
    #[serde(default = "no_decide_key")]
    pub(crate) decide_key: i32,
}

const fn no_decide_key() -> i32 {
    -1
}

impl ModuleTrigger {
    /// A trigger whose events need no binding.
    pub(crate) fn is_global(&self) -> bool {
        self.scope == TriggerScope::Global
    }

    pub(crate) const fn decides(&self) -> bool {
        self.decide_key != -1
    }

    pub(crate) fn admits_difficulty(&self, difficulty: i32) -> bool {
        u32::try_from(difficulty)
            .ok()
            .and_then(|bit| 1u8.checked_shl(bit))
            .is_none_or(|bit| self.difficulty & bit != 0)
    }
}

/// An instance's repeat state, kept only while it differs from the default
/// (RFC 15.1, item 9).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
struct Instance {
    /// Events seen since the last firing, one bit per event (AND).
    completed: u32,
    /// A one-time instance that fired.
    done: bool,
    /// Polling events whose condition held at their last check, one bit per
    /// event: they fire again only after a check finds it false (RFC 6a).
    polled: u32,
}

/// A chained firing past the chain caps, fired with its original record at
/// the start of the next module phase (RFC section 5, "Chains").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct DeferredFiring {
    pub(crate) trigger: u64,
    pub(crate) holder: u64,
    /// The record as raised; `key` and `house` are set when it fires.
    pub(crate) record: EventRecord,
}

/// One module's records waiting for its `on_events`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct Queue {
    records: Vec<EventRecord>,
    /// Records before this were sorted for delivery in an earlier frame.
    sorted: u32,
    /// A producer reached the cap before the module had a chance to drain.
    overflowed: bool,
}

/// The registry. Every map is keyed by creation serial or type ID, so
/// iteration, saves and the hash follow creation order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct ModuleTriggers {
    triggers: BTreeMap<u64, ModuleTrigger>,
    /// Bound object (creation serial) to its triggers.
    objects: BTreeMap<u64, BTreeSet<u64>>,
    /// Bound TechnoType ID to its triggers.
    types: BTreeMap<u32, BTreeSet<u64>>,
    /// Bound weapon, warhead or projectile type (name kind 1, 2 or 3, ID) to
    /// its triggers.
    weapons: BTreeMap<(i32, u32), BTreeSet<u64>>,
    /// Bound cell (x, y) to its triggers.
    cells: BTreeMap<(u16, u16), BTreeSet<u64>>,
    /// Chained firings waiting for the next module phase, in raise order.
    deferred: Vec<DeferredFiring>,
    /// (trigger, holder) where the state is not the default; holder 0 is a
    /// global trigger's one instance.
    instances: BTreeMap<(u64, u64), Instance>,
    queues: BTreeMap<ModuleSource, Queue>,
    /// Derived: the event kinds of every trigger, sorted, rebuilt on each
    /// create and remove. The flag event sites check first.
    kinds: Vec<i32>,
    /// Derived as `kinds`, from the triggers with a decision handler, which
    /// `kinds` leaves out: the flag decision points check.
    #[serde(default)]
    decide_kinds: Vec<i32>,
    /// `ProductionAllowed` answers of 0, as (house array index, TechnoType
    /// ID), from the latest once-per-frame decision step
    /// (`sim::ext_modules::decide`); `HouseClass::CanBuild`'s port reads
    /// them. Kept here, beside the triggers that decide them, so saves, the
    /// state hash and the presentation copy carry them to the readers
    /// before the next step.
    #[serde(default)]
    production_refusals: BTreeSet<(u32, u32)>,
}

impl ModuleTriggers {
    pub(crate) fn is_empty(&self) -> bool {
        self.triggers.is_empty()
            && self.deferred.is_empty()
            && self
                .queues
                .values()
                .all(|q| q.records.is_empty() && !q.overflowed)
            && self.production_refusals.is_empty()
    }

    /// Whether the latest `ProductionAllowed` step refused anything: the
    /// buildability check asks this before looking a pair up.
    pub(crate) fn refuses_any_production(&self) -> bool {
        !self.production_refusals.is_empty()
    }

    /// Whether the latest `ProductionAllowed` step refused `type_id` to the
    /// house at array index `house`.
    pub(crate) fn refuses_production(&self, house: u32, type_id: u32) -> bool {
        self.production_refusals.contains(&(house, type_id))
    }

    /// Takes the `ProductionAllowed` refusals out for the next step.
    pub(crate) fn take_production_refusals(&mut self) -> BTreeSet<(u32, u32)> {
        std::mem::take(&mut self.production_refusals)
    }

    /// Replaces the `ProductionAllowed` refusals with a step's answers.
    pub(crate) fn set_production_refusals(&mut self, refusals: BTreeSet<(u32, u32)>) {
        self.production_refusals = refusals;
    }

    /// The TechnoType IDs bound to a trigger with a decision handler for
    /// `kind`, ascending.
    pub(crate) fn decided_types(&self, kind: i32) -> BTreeSet<u32> {
        self.types
            .iter()
            .filter(|(_, bound)| {
                bound.iter().any(|handle| {
                    self.triggers.get(handle).is_some_and(|t| {
                        t.decides() && t.events.iter().any(|event| event.kind == kind)
                    })
                })
            })
            .map(|(&id, _)| id)
            .collect()
    }

    /// Whether any trigger has an event of `kind`. Event sites ask this
    /// before building a payload.
    pub(crate) fn listens(&self, kind: i32) -> bool {
        self.kinds.binary_search(&kind).is_ok()
    }

    /// Whether a trigger with a decision handler has an event of `kind`.
    /// Decision points ask this before building a request.
    pub(crate) fn decides(&self, kind: i32) -> bool {
        self.decide_kinds.binary_search(&kind).is_ok()
    }

    pub(crate) fn has_type_bindings(&self) -> bool {
        !self.types.is_empty()
    }

    /// The event kinds of every trigger, sorted.
    pub(crate) fn kinds(&self) -> &[i32] {
        &self.kinds
    }

    /// Every trigger handle, in creation order.
    pub(crate) fn handles(&self) -> Vec<u64> {
        self.triggers.keys().copied().collect()
    }

    pub(crate) fn get(&self, handle: u64) -> Option<&ModuleTrigger> {
        self.triggers.get(&handle)
    }

    fn source_usage(&self, source: ModuleSource) -> (usize, usize) {
        let mut definitions = 0;
        let mut polls = 0;
        for trigger in self
            .triggers
            .values()
            .filter(|trigger| trigger.source == source)
        {
            definitions += 1;
            polls += object_polls(trigger.scope, &trigger.events);
        }
        (definitions, polls)
    }

    pub(crate) fn can_create(
        &self,
        source: ModuleSource,
        scope: TriggerScope,
        events: &[EventDef],
    ) -> bool {
        let (definitions, polls) = self.source_usage(source);
        definitions < MAX_TRIGGERS_PER_SOURCE
            && polls + object_polls(scope, events) <= MAX_OBJECT_POLLS_PER_SOURCE
    }

    pub(crate) fn create(&mut self, handle: u64, trigger: ModuleTrigger) {
        self.triggers.insert(handle, trigger);
        self.rebuild_kinds();
    }

    /// Removes a trigger with its bindings, instances and queued records.
    pub(crate) fn remove(&mut self, handle: u64) -> bool {
        if self.triggers.remove(&handle).is_none() {
            return false;
        }
        for bound in self
            .objects
            .values_mut()
            .chain(self.types.values_mut())
            .chain(self.weapons.values_mut())
            .chain(self.cells.values_mut())
        {
            bound.remove(&handle);
        }
        self.objects.retain(|_, bound| !bound.is_empty());
        self.types.retain(|_, bound| !bound.is_empty());
        self.weapons.retain(|_, bound| !bound.is_empty());
        self.cells.retain(|_, bound| !bound.is_empty());
        self.instances.retain(|(trigger, _), _| *trigger != handle);
        self.deferred.retain(|firing| firing.trigger != handle);
        for queue in self.queues.values_mut() {
            queue.retain(|record| record.trigger != handle as i64);
        }
        self.rebuild_kinds();
        true
    }

    /// Removes every trigger `source` made and its queue: a disabled module
    /// keeps no triggers. An INI trigger addressing it loses only its module
    /// actions, and goes when no action is left.
    pub(crate) fn remove_source(&mut self, source: ModuleSource) {
        let mut handles = Vec::new();
        for (&handle, trigger) in self.triggers.iter_mut().filter(|(_, t)| t.source == source) {
            if trigger.ini {
                trigger
                    .actions
                    .retain(|action| action.kind != ACTION_MODULE);
            }
            if !trigger.ini || trigger.actions.is_empty() {
                handles.push(handle);
            }
        }
        for handle in handles {
            self.remove(handle);
        }
        self.queues.remove(&source);
    }

    /// Marks `handle` as built from INI trigger sections.
    pub(crate) fn mark_ini(&mut self, handle: u64) {
        if let Some(trigger) = self.triggers.get_mut(&handle) {
            trigger.ini = true;
        }
    }

    pub(crate) fn set_enabled(&mut self, handle: u64, enabled: bool) -> bool {
        match self.triggers.get_mut(&handle) {
            Some(trigger) => {
                trigger.enabled = enabled;
                true
            }
            None => false,
        }
    }

    pub(crate) fn bind_object(&mut self, handle: u64, object: u64) {
        self.objects.entry(object).or_default().insert(handle);
    }

    pub(crate) fn bind_type(&mut self, handle: u64, type_id: u32) {
        self.types.entry(type_id).or_default().insert(handle);
    }

    /// Removes one object binding; false when there was none.
    pub(crate) fn unbind_object(&mut self, handle: u64, object: u64) -> bool {
        let removed = self
            .objects
            .get_mut(&object)
            .is_some_and(|bound| bound.remove(&handle));
        self.objects.retain(|_, bound| !bound.is_empty());
        self.instances.remove(&(handle, object));
        removed
    }

    /// Removes one type binding; false when there was none. Instances that
    /// objects of the type ran stay until the objects go.
    pub(crate) fn unbind_type(&mut self, handle: u64, type_id: u32) -> bool {
        let removed = self
            .types
            .get_mut(&type_id)
            .is_some_and(|bound| bound.remove(&handle));
        self.types.retain(|_, bound| !bound.is_empty());
        removed
    }

    /// Binds a weapon (name kind 1), warhead (2) or projectile (3) type.
    pub(crate) fn bind_weapon(&mut self, handle: u64, kind: i32, id: u32) {
        self.weapons.entry((kind, id)).or_default().insert(handle);
    }

    /// Removes one weapon, warhead or projectile binding; false when there
    /// was none.
    pub(crate) fn unbind_weapon(&mut self, handle: u64, kind: i32, id: u32) -> bool {
        let removed = self
            .weapons
            .get_mut(&(kind, id))
            .is_some_and(|bound| bound.remove(&handle));
        self.weapons.retain(|_, bound| !bound.is_empty());
        removed
    }

    pub(crate) fn bind_cell(&mut self, handle: u64, cell: (u16, u16)) {
        self.cells.entry(cell).or_default().insert(handle);
    }

    /// Removes one cell binding and its instance; false when there was none.
    pub(crate) fn unbind_cell(&mut self, handle: u64, cell: (u16, u16)) -> bool {
        let removed = self
            .cells
            .get_mut(&cell)
            .is_some_and(|bound| bound.remove(&handle));
        self.cells.retain(|_, bound| !bound.is_empty());
        removed
    }

    /// The triggers with an event of `kind` bound to a cell `selects`
    /// accepts, with the cell, in cell then creation order.
    pub(crate) fn cell_bound(
        &self,
        kind: i32,
        selects: impl Fn((u16, u16)) -> bool,
    ) -> Vec<(u64, (u16, u16))> {
        self.cells
            .iter()
            .filter(|(cell, _)| selects(**cell))
            .flat_map(|(&cell, bound)| bound.iter().map(move |&handle| (handle, cell)))
            .filter(|(handle, _)| self.has_event(*handle, kind))
            .collect()
    }

    /// The objects bound to `handle` directly, in creation order.
    pub(crate) fn bound_objects(&self, handle: u64) -> Vec<u64> {
        self.objects
            .iter()
            .filter(|(_, bound)| bound.contains(&handle))
            .map(|(&object, _)| object)
            .collect()
    }

    /// The TechnoType IDs bound to `handle`.
    pub(crate) fn bound_types(&self, handle: u64) -> BTreeSet<u32> {
        self.types
            .iter()
            .filter(|(_, bound)| bound.contains(&handle))
            .map(|(&id, _)| id)
            .collect()
    }

    /// A polling event's check on one instance: answers whether its
    /// condition just became true, and remembers what it found.
    pub(crate) fn poll_edge(&mut self, handle: u64, holder: u64, bit: usize, holds: bool) -> bool {
        let Some(mask) = 1u32.checked_shl(bit as u32) else {
            return false;
        };
        // An earlier holder's action may have removed the trigger.
        if !self.triggers.contains_key(&handle) {
            return false;
        }
        let mut state = self
            .instances
            .get(&(handle, holder))
            .copied()
            .unwrap_or_default();
        let rising = holds && state.polled & mask == 0;
        if holds {
            state.polled |= mask;
        } else {
            state.polled &= !mask;
        }
        self.store_instance(handle, holder, state);
        rising
    }

    /// A write to variable `var`: the polling intervals of every trigger
    /// with a `VariablePoll` of it count from `frame` (appendix 4.4).
    pub(crate) fn reset_variable_polls(&mut self, var: i32, frame: u32) {
        use vera20k_wasm_ext::triggers::EVENT_VARIABLE_POLL;
        for trigger in self.triggers.values_mut() {
            if trigger
                .events
                .iter()
                .any(|event| event.kind == EVENT_VARIABLE_POLL && event.id == var)
            {
                trigger.poll_start = frame;
            }
        }
    }

    pub(crate) fn defer(&mut self, firing: DeferredFiring) {
        self.deferred.push(firing);
    }

    /// The deferred firings of the triggers `source` made.
    pub(crate) fn deferred_of(&self, source: ModuleSource) -> usize {
        self.deferred
            .iter()
            .filter(|f| {
                self.triggers
                    .get(&f.trigger)
                    .is_some_and(|t| t.source == source)
            })
            .count()
    }

    /// Takes every deferred firing, in raise order.
    pub(crate) fn take_deferred(&mut self) -> Vec<DeferredFiring> {
        std::mem::take(&mut self.deferred)
    }

    fn has_event(&self, handle: u64, kind: i32) -> bool {
        self.triggers
            .get(&handle)
            .is_some_and(|t| t.events.iter().any(|e| e.kind == kind))
    }

    fn store_instance(&mut self, handle: u64, holder: u64, state: Instance) {
        if state == Instance::default() {
            self.instances.remove(&(handle, holder));
        } else {
            self.instances.insert((handle, holder), state);
        }
    }

    /// An object left the game: its bindings and instances go (RFC 7).
    pub(crate) fn forget_object(&mut self, object: u64) {
        if self.triggers.is_empty() {
            return;
        }
        self.objects.remove(&object);
        // Every instance is a live trigger's, so there is no table to scan.
        if !self.instances.is_empty() {
            for &handle in self.triggers.keys() {
                self.instances.remove(&(handle, object));
            }
        }
    }

    /// The triggers with an event of `kind` bound to `subject`, to its
    /// TechnoType or to one of the weapon, warhead and projectile types in
    /// `weapons` (name kind, ID), in creation order.
    pub(crate) fn bound(
        &self,
        kind: i32,
        subject: u64,
        type_id: Option<u32>,
        weapons: &[(i32, u32)],
    ) -> Vec<u64> {
        let by_object = self.objects.get(&subject).into_iter().flatten();
        let by_type = type_id
            .and_then(|id| self.types.get(&id))
            .into_iter()
            .flatten();
        let by_weapon = weapons
            .iter()
            .filter_map(|key| self.weapons.get(key))
            .flatten();
        let handles: BTreeSet<u64> = by_object.chain(by_type).chain(by_weapon).copied().collect();
        handles
            .into_iter()
            .filter(|handle| {
                self.has_event(*handle, kind)
                    && self.triggers.get(handle).is_some_and(|t| !t.decides())
            })
            .collect()
    }

    /// The triggers with a decision handler for `kind`: every global one,
    /// else those bound to `subject`, to `type_id` or to one of the weapon,
    /// warhead and projectile types in `weapons` (name kind, ID), in
    /// creation order.
    pub(crate) fn deciders(
        &self,
        kind: i32,
        subject: u64,
        type_id: Option<u32>,
        weapons: &[(i32, u32)],
    ) -> Vec<u64> {
        let of_kind = |t: &ModuleTrigger| t.decides() && t.events.iter().any(|e| e.kind == kind);
        if kind == EVENT_DEFEAT {
            return self
                .triggers
                .iter()
                .filter(|(_, t)| of_kind(t))
                .map(|(&handle, _)| handle)
                .collect();
        }
        let by_object = self.objects.get(&subject).into_iter().flatten();
        let by_type = type_id
            .and_then(|id| self.types.get(&id))
            .into_iter()
            .flatten();
        let by_weapon = weapons
            .iter()
            .filter_map(|key| self.weapons.get(key))
            .flatten();
        let handles: BTreeSet<u64> = by_object.chain(by_type).chain(by_weapon).copied().collect();
        handles
            .into_iter()
            .filter(|handle| self.triggers.get(handle).is_some_and(of_kind))
            .collect()
    }

    pub(crate) fn has_weapon_bindings(&self) -> bool {
        !self.weapons.is_empty()
    }

    /// The global triggers with an event of `kind`, in creation order.
    pub(crate) fn global(&self, kind: i32) -> Vec<u64> {
        self.triggers
            .iter()
            .filter(|(_, t)| t.is_global() && t.events.iter().any(|e| e.kind == kind))
            .map(|(&handle, _)| handle)
            .collect()
    }

    /// Records the events in `seen` (bits) for the instance of `handle` on
    /// `holder`. Answers whether every event has now occurred, which fires
    /// the instance: its events reset, and a one-time instance is done.
    pub(crate) fn latch(&mut self, handle: u64, holder: u64, seen: u32) -> bool {
        let Some(trigger) = self.triggers.get(&handle) else {
            return false;
        };
        let all = u32::MAX >> (32 - trigger.events.len().clamp(1, 32) as u32);
        let one_time = trigger.repeat != Repeat::Repeating;
        let mut state = self
            .instances
            .get(&(handle, holder))
            .copied()
            .unwrap_or_default();
        if state.done {
            return false;
        }
        state.completed |= seen & all;
        let fires = state.completed == all;
        if fires {
            state.completed = 0;
            state.done = one_time;
        }
        self.store_instance(handle, holder, state);
        fires
    }

    /// Whether a one-time trigger's instance has fired.
    pub(crate) fn is_done(&self, handle: u64, holder: u64) -> bool {
        self.instances
            .get(&(handle, holder))
            .is_some_and(|state| state.done)
    }

    pub(crate) fn enqueue(&mut self, source: ModuleSource, record: EventRecord) {
        let queue = self.queues.entry(source).or_default();
        if queue.records.len() == MAX_QUEUED_RECORDS {
            queue.overflowed = true;
        } else if !queue.overflowed {
            queue.records.push(record);
        }
    }

    /// Readies `source`'s queue for this frame's delivery and answers how
    /// many records may go out: records of a trigger that is gone or
    /// disabled are dropped (RFC section 5), and the records queued since
    /// the last delivery are sorted by subject creation serial, subject 0
    /// first, each subject's records in the order they were raised.
    pub(crate) fn prepare_delivery(&mut self, source: ModuleSource) -> usize {
        let Some(mut queue) = self.queues.remove(&source) else {
            return 0;
        };
        queue.retain(|record| {
            u64::try_from(record.trigger)
                .ok()
                .and_then(|handle| self.triggers.get(&handle))
                .is_some_and(|trigger| trigger.enabled)
        });
        let sorted = (queue.sorted as usize).min(queue.records.len());
        queue.records[sorted..].sort_by_key(|record| record.subject);
        queue.sorted = queue.records.len() as u32;
        let ready = queue.records.len();
        if ready > 0 || queue.overflowed {
            self.queues.insert(source, queue);
        }
        ready
    }

    /// Takes up to `count` records from the front of `source`'s queue.
    pub(crate) fn take(&mut self, source: ModuleSource, count: usize) -> Vec<EventRecord> {
        let Some(queue) = self.queues.get_mut(&source) else {
            return Vec::new();
        };
        let count = count.min(queue.records.len());
        queue.sorted = queue.sorted.saturating_sub(count as u32);
        let taken = queue.records.drain(..count).collect();
        if queue.records.is_empty() && !queue.overflowed {
            self.queues.remove(&source);
        }
        taken
    }

    pub(crate) fn queued(&self, source: ModuleSource) -> usize {
        self.queues.get(&source).map_or(0, |q| q.records.len())
    }

    pub(crate) fn queue_overflowed(&self, source: ModuleSource) -> bool {
        self.queues
            .get(&source)
            .is_some_and(|queue| queue.overflowed)
    }

    pub(crate) fn clear_queue(&mut self, source: ModuleSource) {
        self.queues.remove(&source);
    }

    /// Multiplayer frame checksum terms: each trigger's handle and switch,
    /// then each queued record's key, kind, trigger and subject.
    pub(crate) fn fold_match_checksum(
        &self,
        checksum: &mut crate::sim::multiplayer_checksum::RetailChecksumAccumulator,
    ) {
        for (&handle, trigger) in &self.triggers {
            checksum.fold_value(handle as u32);
            checksum.fold_value(u32::from(trigger.enabled));
        }
        for firing in &self.deferred {
            checksum.fold_value(firing.trigger as u32);
            checksum.fold_value(firing.record.kind as u32);
        }
        for (&source, queue) in &self.queues {
            if queue.overflowed {
                checksum.fold_value(u32::MAX);
                checksum.fold_value(source as u32);
            }
        }
        for record in self.queues.values().flat_map(|q| &q.records) {
            checksum.fold_value(record.key as u32);
            checksum.fold_value(record.kind as u32);
            checksum.fold_value(record.trigger as u32);
            checksum.fold_value(record.subject as u32);
        }
        for &(house, type_id) in &self.production_refusals {
            checksum.fold_value(house);
            checksum.fold_value(type_id);
        }
    }

    /// Restore checks: every binding, instance and queued record names a
    /// live trigger, and the derived kinds match the triggers.
    pub(crate) fn validate(&self) -> Result<(), String> {
        for source in [ModuleSource::Mod, ModuleSource::Map] {
            let (definitions, polls) = self.source_usage(source);
            if definitions > MAX_TRIGGERS_PER_SOURCE || polls > MAX_OBJECT_POLLS_PER_SOURCE {
                return Err(format!("{source:?} module trigger limits exceeded"));
            }
        }
        if self
            .queues
            .values()
            .any(|queue| queue.records.len() > MAX_QUEUED_RECORDS)
        {
            return Err("a module event queue exceeds its limit".into());
        }
        let known = |handle: &u64| self.triggers.contains_key(handle);
        let bound = self
            .objects
            .values()
            .chain(self.types.values())
            .chain(self.weapons.values())
            .chain(self.cells.values())
            .flatten();
        if !bound.into_iter().all(known)
            || !self.instances.keys().all(|(handle, _)| known(handle))
            || !self.deferred.iter().all(|f| known(&f.trigger))
            || !self
                .queues
                .values()
                .flat_map(|q| &q.records)
                .all(|r| u64::try_from(r.trigger).is_ok_and(|h| known(&h)))
        {
            return Err("a module trigger binding or record names no trigger".into());
        }
        let mut rebuilt = self.clone();
        rebuilt.rebuild_kinds();
        if rebuilt.kinds != self.kinds || rebuilt.decide_kinds != self.decide_kinds {
            return Err("module trigger event kinds disagree with the triggers".into());
        }
        Ok(())
    }

    fn rebuild_kinds(&mut self) {
        let kinds = |decides: bool| {
            let kinds: BTreeSet<i32> = self
                .triggers
                .values()
                .filter(|t| t.decides() == decides)
                .flat_map(|t| t.events.iter().map(|e| e.kind))
                .collect();
            kinds.into_iter().collect()
        };
        self.kinds = kinds(false);
        self.decide_kinds = kinds(true);
    }
}

impl Queue {
    /// Keeps the records `keep` accepts, in order; `sorted` still counts the
    /// already sorted records that remain.
    fn retain(&mut self, mut keep: impl FnMut(&EventRecord) -> bool) {
        let sorted = self.sorted as usize;
        let (mut index, mut kept_sorted) = (0, 0u32);
        self.records.retain(|record| {
            let kept = keep(record);
            if kept && index < sorted {
                kept_sorted += 1;
            }
            index += 1;
            kept
        });
        self.sorted = kept_sorted;
    }
}
