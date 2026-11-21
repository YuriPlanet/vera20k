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
use vera20k_wasm_ext::triggers::{ActionDef, EVENT_TIMER, EventDef, Repeat};

use crate::sim::ext_modules::ModuleSource;

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
    pub(crate) events: Vec<EventDef>,
    pub(crate) actions: Vec<ActionDef>,
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
    /// A trigger whose events need no binding (`Timer`, `Defeat`).
    pub(crate) fn is_global(&self) -> bool {
        self.events
            .iter()
            .all(|event| matches!(event.kind, EVENT_TIMER | EVENT_DEFEAT))
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
}

/// One module's records waiting for its `on_events`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct Queue {
    records: Vec<EventRecord>,
    /// Records before this were sorted for delivery in an earlier frame.
    sorted: u32,
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
}

impl ModuleTriggers {
    pub(crate) fn is_empty(&self) -> bool {
        self.triggers.is_empty() && self.queues.values().all(|q| q.records.is_empty())
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

    pub(crate) fn get(&self, handle: u64) -> Option<&ModuleTrigger> {
        self.triggers.get(&handle)
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
        for bound in self.objects.values_mut().chain(self.types.values_mut()) {
            bound.remove(&handle);
        }
        self.objects.retain(|_, bound| !bound.is_empty());
        self.types.retain(|_, bound| !bound.is_empty());
        self.instances.retain(|(trigger, _), _| *trigger != handle);
        for queue in self.queues.values_mut() {
            queue.retain(|record| record.trigger != handle as i64);
        }
        self.rebuild_kinds();
        true
    }

    /// Removes every trigger `source` made and its queue: a disabled module
    /// keeps no triggers.
    pub(crate) fn remove_source(&mut self, source: ModuleSource) {
        let handles: Vec<u64> = self
            .triggers
            .iter()
            .filter(|(_, t)| t.source == source)
            .map(|(&handle, _)| handle)
            .collect();
        for handle in handles {
            self.remove(handle);
        }
        self.queues.remove(&source);
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

    /// An object left the game: its bindings and instances go (RFC 7).
    pub(crate) fn forget_object(&mut self, object: u64) {
        if self.triggers.is_empty() {
            return;
        }
        self.objects.remove(&object);
        self.instances.retain(|(_, holder), _| *holder != object);
    }

    /// The triggers with an event of `kind` bound to `subject` or to its
    /// TechnoType, in creation order.
    pub(crate) fn bound(&self, kind: i32, subject: u64, type_id: Option<u32>) -> Vec<u64> {
        let by_object = self.objects.get(&subject).into_iter().flatten();
        let by_type = type_id
            .and_then(|id| self.types.get(&id))
            .into_iter()
            .flatten();
        let handles: BTreeSet<u64> = by_object.chain(by_type).copied().collect();
        handles
            .into_iter()
            .filter(|handle| {
                self.triggers
                    .get(handle)
                    .is_some_and(|t| !t.decides() && t.events.iter().any(|e| e.kind == kind))
            })
            .collect()
    }

    /// The triggers with a decision handler for `kind`: every global one,
    /// else those bound to `subject` or to `type_id`, in creation order.
    pub(crate) fn deciders(&self, kind: i32, subject: u64, type_id: Option<u32>) -> Vec<u64> {
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
        let handles: BTreeSet<u64> = by_object.chain(by_type).copied().collect();
        handles
            .into_iter()
            .filter(|handle| self.triggers.get(handle).is_some_and(of_kind))
            .collect()
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
        if state == Instance::default() {
            self.instances.remove(&(handle, holder));
        } else {
            self.instances.insert((handle, holder), state);
        }
        fires
    }

    pub(crate) fn enqueue(&mut self, source: ModuleSource, record: EventRecord) {
        self.queues.entry(source).or_default().records.push(record);
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
        if ready > 0 {
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
        if queue.records.is_empty() {
            self.queues.remove(&source);
        }
        taken
    }

    pub(crate) fn queued(&self, source: ModuleSource) -> usize {
        self.queues.get(&source).map_or(0, |q| q.records.len())
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
        for record in self.queues.values().flat_map(|q| &q.records) {
            checksum.fold_value(record.key as u32);
            checksum.fold_value(record.kind as u32);
            checksum.fold_value(record.trigger as u32);
            checksum.fold_value(record.subject as u32);
        }
    }

    /// Restore checks: every binding, instance and queued record names a
    /// live trigger, and the derived kinds match the triggers.
    pub(crate) fn validate(&self) -> Result<(), String> {
        let known = |handle: &u64| self.triggers.contains_key(handle);
        let bound = self.objects.values().chain(self.types.values()).flatten();
        if !bound.into_iter().all(known)
            || !self.instances.keys().all(|(handle, _)| known(handle))
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
