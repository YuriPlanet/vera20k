//! Decidable events (RFC 0001 section 6f, open question 10): a module
//! trigger with a decision handler (`TriggerDef.decide_key`) is asked
//! synchronously, through `on_decide`, at the point where the engine
//! commits a result, and may replace it.
//!
//! Decision points (each checks `TriggerRuntime::module_decides` first and
//! builds nothing without a decider):
//! - `TakeDamage` → final damage: Object5F5390's port
//!   (`combat::object_health::commit`), after the Ares cull hook
//!   (`0x005F5456`) and before the packet's sign dispatch (`5F546A`) and
//!   the Strength write. Observation-only receivers ask nothing. Clamped to
//!   `0..=i32::MAX`; the native `min(Health)` follows.
//! - `ProductionQueued` → 1 allow, 0 refuse: `HouseClass::Begin_Production
//!   @ 0x004FA350` for a PRODUCE event (player, module order), after the
//!   held same-type resume (`0x004FA5A8..0x004FA5C4`) and before
//!   `FactoryClass::StartProduction @ 0x004C9C70` (`enqueue_selected`);
//!   and `BuildingClass::Factory_AI`'s start (`0x004502F4..0x004503C5`)
//!   before the FactoryClass constructor (`0x004C9974`). StartNext's forced
//!   reissue of an already queued type (`0x004CA60A`) asks nothing. A
//!   refusal makes the request fail as a refused FindFactory does: nothing
//!   queued, no credits spent.
//! - `Defeat` → 1 defeat, 0 veto: `HouseClass::Update`'s defeat gate
//!   (`0x004F8E86..0x004F8F82`), once the house holds nothing, before
//!   `Blowup_All @ 0x004FC6D0` and `MPlayer_Defeated @ 0x004FC0B0`. A veto
//!   skips both this frame; the gate asks again next frame.
//!
//! The chain (RFC section 2): every enabled decider whose clauses hold is a
//! link, mod module's triggers first, then the map's, each in creation
//! order. Each gets one `DecideRecord` (`count` 1) with `native` and the
//! previous link's result; an answer the kind does not accept keeps the
//! previous result. A trigger with a decision handler is never delivered.
//!
//! Rules while a handler runs:
//! - Only the imports appendix 1.1 marks `decide` answer; a write stops the
//!   module (the sandbox refuses it), and its link keeps the previous
//!   result, as any fault does.
//! - `random` draws from a substream of (frame, subject creation serial,
//!   event kind) per module; later requests for the same subject and kind
//!   in the frame continue it. The module's main stream never moves.
//! - Each call has [`DECIDE_BUDGET`] operations; running out stops the
//!   module. A module's decisions in one frame may use up to its frame
//!   budget in all; past it, its links are skipped for the rest of the
//!   frame.
//! - No reentry (RFC 5): while any module call runs, the module slots are
//!   set aside (`Simulation::with_ext_module`), so a decision it causes
//!   (an `action` dealing damage) asks no module and keeps the native
//!   result. Residual: the RFC lets the other module's link run.
//!
//! Saves: decisions complete inside a frame, so nothing waits across a
//! save. The per-frame budget use and substreams start over each frame and
//! are neither saved nor hashed; a handler's own memory is, as always.
//!
//! Residuals:
//! - A trigger with a decision handler and actions answers `Unsupported`.
//! - The record's and clauses' `house` is the event's house for
//!   `ProductionQueued` and `Defeat` (not the trigger's house).
//! - No result areas: none of these kinds uses one.

use vera20k_wasm_ext::decide::{
    DECIDABLE_EVENTS, DECIDE_BUDGET, DECISION_ONLY_EVENTS, DecideRecord, EVENT_DEFEAT,
    EVENT_PRODUCTION_QUEUED,
};
use vera20k_wasm_ext::{EventRecord, Export, Invalid, Status, TriggerDef};

use super::triggers::RAISED_EVENTS;
use super::{ModuleEvent, ModuleSource};
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::rng::SimRng;
use crate::sim::world::{FrameEffects, Simulation};

/// Decision state within one frame: each module's operations used and the
/// `random` substreams drawn from. Not saved or hashed (module docs).
#[derive(Default)]
pub(super) struct DecideState {
    frame: u32,
    /// By [`ModuleSource`].
    used: [u64; 2],
    streams: Vec<(StreamKey, SimRng)>,
}

/// (module, subject creation serial, event kind).
type StreamKey = (ModuleSource, u64, i32);

impl DecideState {
    fn roll_to(&mut self, frame: u32) {
        if self.frame != frame {
            *self = Self {
                frame,
                ..Self::default()
            };
        }
    }

    /// The substream for `key` this frame, continued if drawn from before.
    fn take_stream(&mut self, seed: u64, key: StreamKey) -> SimRng {
        match self.streams.iter().position(|(k, _)| *k == key) {
            Some(at) => self.streams.swap_remove(at).1,
            None => substream(seed, key, self.frame),
        }
    }
}

/// A substream seed from the scenario seed, the frame, the subject's
/// creation serial, the event kind and the module (RFC section 4).
fn substream(seed: u64, (source, subject, kind): StreamKey, frame: u32) -> SimRng {
    let mut x = seed ^ 0x4445_4349_4445_0000 ^ source as u64;
    for part in [u64::from(frame), subject, u64::from(kind as u32)] {
        x = splitmix64(x ^ part);
    }
    SimRng::new(x ^ (x >> 32))
}

const fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// What `trigger_create` accepts of `decide_key` (appendix "Trigger
/// definition"): a handler needs exactly one event with a decision form
/// here, and a decision-only event (4.5) needs a handler.
pub(super) fn check_decide_key(def: &TriggerDef) -> Result<Option<Status>, Invalid> {
    if def.decide_key == -1 {
        return match def
            .events
            .iter()
            .find(|e| DECISION_ONLY_EVENTS.contains(&e.kind))
        {
            Some(event) => Err(Invalid(format!(
                "event {} needs a decision handler",
                event.kind
            ))),
            None => Ok(None),
        };
    }
    let [event] = def.events.as_slice() else {
        return Err(Invalid(format!(
            "a decision handler takes exactly one event, not {}",
            def.events.len()
        )));
    };
    if !DECIDABLE_EVENTS.contains(&event.kind) {
        return match RAISED_EVENTS.contains(&event.kind) {
            true => Err(Invalid(format!(
                "event {} has no decision form",
                event.kind
            ))),
            // An event this engine does not raise at all.
            false => Ok(Some(Status::Unsupported)),
        };
    }
    Ok((!def.actions.is_empty()).then_some(Status::Unsupported))
}

impl Simulation {
    /// The decision chain for one request (module docs): returns the last
    /// accepted answer, or `native` when no link answers. `house` (an
    /// index) is the event's house, `None` for the trigger's; `type_id`
    /// the bound TechnoType to look up, `None` for the subject's. `accept`
    /// validates an answer for the kind, `None` rejecting it.
    pub(crate) fn decide_module_event(
        &mut self,
        rules: &RuleSet,
        event: &ModuleEvent,
        house: Option<u32>,
        type_id: Option<u32>,
        native: i64,
        accept: impl Fn(i64) -> Option<i64>,
    ) -> i64 {
        // Inside a module call the slots are set aside: no reentry.
        if self.ext_modules.slots.is_empty() {
            return native;
        }
        let triggers = self.trigger_runtime.module_triggers();
        let type_id = type_id.or_else(|| {
            triggers.has_type_bindings().then_some(())?;
            let entity = self.entities().get(event.subject)?;
            let object = self.entity_object_type(entity, rules)?;
            rules.type_handle(&object.id).map(|handle| handle.0)
        });
        let mut handles = triggers.deciders(event.kind, event.subject, type_id);
        // Mod first, then map; creation order within each (a stable sort).
        handles.sort_by_key(|handle| triggers.get(*handle).map(|t| t.source));
        let mut previous = native;
        for handle in handles {
            if let Some(answer) = self.ask_decider(rules, handle, event, house, native, previous)
                && let Some(result) = accept(answer)
            {
                previous = result;
            }
        }
        previous
    }

    /// One link: `None` when the trigger is off, its clauses fail, its
    /// module is disabled or out of decision budget, or the handler leaves
    /// `handled` 0 or faults (which stops the module).
    fn ask_decider(
        &mut self,
        rules: &RuleSet,
        handle: u64,
        event: &ModuleEvent,
        house: Option<u32>,
        native: i64,
        previous: i64,
    ) -> Option<i64> {
        let difficulty = self.trigger_difficulty();
        let trigger = self.trigger_runtime.module_triggers().get(handle)?;
        if !trigger.enabled || !trigger.admits_difficulty(difficulty) {
            return None;
        }
        let (source, key, fixed_house) = (trigger.source, trigger.decide_key, trigger.house);
        let clauses = trigger.events.first()?.clauses.clone();
        let index = self
            .ext_modules
            .slots
            .iter()
            .position(|slot| slot.source == source)?;
        let slot = &self.ext_modules.slots[index];
        let decide = &mut self.ext_modules.decide;
        decide.roll_to(self.session.binary_frame());
        if slot.disabled.is_some() || decide.used[source as usize] >= slot.frame_budget {
            return None;
        }
        let house = house
            .or(fixed_house)
            .or_else(|| self.module_trigger_house(rules, event.subject));
        if !self.module_clauses_hold(rules, source, &clauses, event, house) {
            return None;
        }
        // One event: the instance fires each time unless one-time and done.
        if !self
            .trigger_runtime
            .module_triggers_mut()
            .latch(handle, event.subject, 1)
        {
            return None;
        }
        let record = DecideRecord {
            event: EventRecord {
                key,
                ..self.module_event_record(rules, source, handle, event.subject, event, house)
            },
            native,
            previous,
        };
        let stream_key = (source, event.subject, event.kind);
        let seed = self.session.seed();
        let mut stream = self.ext_modules.decide.take_stream(seed, stream_key);
        std::mem::swap(&mut self.ext_modules.slots[index].rng, &mut stream);
        let result = self.with_ext_module(
            rules,
            Some(rules.overlay_registry()),
            &mut FrameEffects::empty(),
            index,
            |module, host| module.decide(&record, DECIDE_BUDGET, host),
        );
        std::mem::swap(&mut self.ext_modules.slots[index].rng, &mut stream);
        let decide = &mut self.ext_modules.decide;
        decide.streams.push((stream_key, stream));
        match result {
            Ok((answer, used)) => {
                decide.used[source as usize] += used;
                answer
            }
            Err(fault) => {
                self.disable_ext_module(index, Export::Decide, fault);
                None
            }
        }
    }

    /// `ProductionQueued` for `owner` adding `object` (module docs): false
    /// when a decider refuses.
    pub(crate) fn module_allows_production(
        &mut self,
        rules: &RuleSet,
        owner: InternedId,
        object: &ObjectType,
    ) -> bool {
        if !self.trigger_runtime.module_decides(EVENT_PRODUCTION_QUEUED) {
            return true;
        }
        let (Some(type_id), Some(house)) = (
            rules.type_handle(&object.id).map(|handle| handle.0),
            self.houses.index_of(owner),
        ) else {
            return true;
        };
        let event = ModuleEvent {
            id: type_id as i32,
            ..ModuleEvent::new(EVENT_PRODUCTION_QUEUED, 0)
        };
        self.decide_module_event(rules, &event, Some(house as u32), Some(type_id), 1, allow) != 0
    }

    /// `Defeat` for `owner` (module docs): false when a decider vetoes.
    pub(crate) fn module_allows_defeat(&mut self, rules: &RuleSet, owner: InternedId) -> bool {
        if !self.trigger_runtime.module_decides(EVENT_DEFEAT) {
            return true;
        }
        let Some(house) = self.houses.index_of(owner) else {
            return true;
        };
        let event = ModuleEvent::new(EVENT_DEFEAT, 0);
        self.decide_module_event(rules, &event, Some(house as u32), None, 1, allow) != 0
    }
}

/// A yes/no result: 1 or 0; anything else keeps the previous result.
fn allow(answer: i64) -> Option<i64> {
    matches!(answer, 0 | 1).then_some(answer)
}
