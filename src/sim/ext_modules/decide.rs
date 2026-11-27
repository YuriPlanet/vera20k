//! Decidable events (RFC 0001 section 6f, open question 10): a module
//! trigger with a decision handler (`TriggerDef.decide_key`) is asked
//! synchronously, through `on_decide`, at the point where the engine
//! commits a result, and may replace it.
//!
//! Decision points (each checks `TriggerRuntime::module_decides` first and
//! builds nothing without a decider):
//! - `TakeDamage` → final damage: Object5F5390's port
//!   (`combat::object_health::commit`), after the Ares cull hook
//!   (`0x005F5456`) and before the packet's zero and sign tests
//!   (`0x005F5458` and `0x005F5468`) and the Strength write. Observation-only
//!   receivers ask nothing. Clamped to
//!   `0..=i32::MAX`; the native `min(Health)` follows.
//! - `ProductionAllowed` → 1 allow, 0 disallow: once per frame at a fixed
//!   step, the start of the module phase, for every house that is not
//!   defeated (array order) and every TechnoType a `ProductionAllowed`
//!   decider is bound to (ascending ID). `native` is
//!   `HouseClass::CanBuild @ 0x004F7870` as the sidebar asks it (Yes is 1).
//!   Explicit answers of 0 are kept in the trigger registry until the next
//!   step. CanBuild's port ([`crate::sim::production::can_build`]) answers No for
//!   them at its entry, beside VERA's room bans, whatever its flags. So the
//!   sidebar, a PRODUCE event, the per-frame production check, the AI's
//!   choosers and FindFactory all read the cached result.
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
//! - `CommandAllowed` → 1 allow, 0 refuse: the command receiver
//!   (`Simulation::apply_command_with_overlays`, the port of
//!   `EventClass::Execute @ 0x004C6CB0`) once the command's house may order
//!   its actor and before planning detach and the per-type dispatch, for
//!   every command with an appendix 8.3 number and an actor object
//!   (`orders::command_request`). Player commands from the network and the local
//!   queue, module orders (`run_ext_module_orders`) and the `SetMission`
//!   write all pass there, on every machine in command order. A refused
//!   command does nothing, as a receiver refusal does; a rally point keeps
//!   the producers not refused.
//! - `FatalDamage` (preview) → the death plan's bits: the entry of the
//!   receiver's death arm (`combat::death_plan`, `0x00702050`).
//! - `Fire` (preview) → a shot plan, the count of `ProjectileSpec`s in the
//!   result area: FireAt's launch, just before `BulletClass::Fire`
//!   (`combat::fire_plan`).
//! - `Launch` and `ProjectileFrame` (preview) → a `PathSpec` (its byte count
//!   in `result`) and a `ProjectileStep`: a bullet's launch and its flight
//!   visit (`ext_modules::projectiles`).
//! - `IdleMission` and `MissionStep` (preview) → a `Mission` number and a
//!   `MissionStepResult`: an object going idle and the outcome of its Guard,
//!   Area Guard, Hunt or Attack body (`ext_modules::unit_ai`).
//! - `MoveFrame` (preview) → a `MoveStep`: a bound ground mover's locomotor
//!   Process with no step in flight (`ext_modules::motion`).
//! - `PathFound` (preview) → a count of `PathCell`s: a Find_Path search's
//!   adoption (`ext_modules::paths`).
//! - `ApproachPoint` (preview) → a cell reference: the cell Approach chose,
//!   before it becomes the destination (`ext_modules::paths`).
//!
//! Result areas (`Fire`, `Launch`, `ProjectileFrame`, `MissionStep`,
//! `MoveFrame`, `PathFound`; RFC 15.4 sizes): the
//! area follows the record in the module's delivery area, smaller when that
//! area is (0 when nothing fits). Each link gets the last accepted result's
//! bytes (`previous_len`), the rest zeroed; an answer whose size does not fit
//! (`vera20k_wasm_ext::decide::result_len`) keeps the previous bytes.
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
//! - Each call may use [`DECIDE_BUDGET`] operations. Exceeding that stops
//!   the module. Calls start while their pool lasts (`Slot::decision_pool`):
//!   `ProductionAllowed` uses the frame budget B, `Defeat` B/4 of its own,
//!   the rest B/2 of their own, so module-phase work never lapses a veto.
//!   Once a pool is used up its links are skipped and pass the previous
//!   result on, with two exceptions: a skipped `ProductionAllowed` link
//!   passes the pair's last refusal on, which a later link may overrule, and
//!   a skipped `Defeat` defeats nobody this frame.
//! - No reentry (RFC 5): while a module call runs, that module's slot is
//!   out of the slot list (`Simulation::with_ext_module`), so a decision
//!   the call causes (an `action` dealing damage, a `SetMission` write)
//!   skips that module's links, and the other module's links run, as the
//!   RFC's example has it. A handler is read-only, so it causes no
//!   decision and nothing nests deeper than two calls.
//!
//! Saves: decisions complete inside a frame, except the `ProductionAllowed`
//! refusals, which the trigger registry saves and hashes. The substreams
//! start over each frame and are neither saved nor hashed. A handler's
//! memory is saved and hashed.
//!
//! Decision triggers with immediate actions answer `Unsupported` at
//! `trigger_create`. RFC 6a lets a trigger have both, but no section says
//! when its actions run or what result they see, and running stock or
//! atomic actions at a decision point would change the world inside the
//! calculation being decided (a `Damage` inside the damage commit, a
//! `SetMission` inside the command receiver), which the read-only handler
//! rule exists to prevent. The same effect is reachable with a delivered
//! trigger on the same event, or with the handler noting the request in
//! memory for `on_frame`. `Unsupported` leaves room to add them later.
//!
//! Residuals:
//! - The record's and clauses' `house` is the event's house for
//!   `ProductionAllowed`, `ProductionQueued` and `Defeat` (not the
//!   trigger's house).
//! - `ProductionAllowed`: an answer of 1 cannot lift CanBuild's own refusal
//!   (prerequisites, tech level, build limit); each request is its own
//!   `on_decide` call (`count` 1), not one batch per module; the step asks
//!   every house that is not defeated, the Neutral and Special houses too.
//! - `CommandAllowed`: commands with no 8.3 number (Select, control groups,
//!   ArchiveObject, Online toggles, MegaMissions other than Enter/Eaten/Harvest,
//!   Ares trench clicks) and commands without an actor (production,
//!   placement, superweapons, wall sales) ask nothing; the computer's own
//!   AI gives missions directly, not through commands, so only module
//!   orders reach it for computer houses.
//! - A decision nested in another module's call gets no merged rules text
//!   (`rules_get` answers `Unsupported`): the outer call holds it.

use std::collections::BTreeSet;

use vera20k_wasm_ext::decide::{
    DECIDABLE_EVENTS, DECIDE_BUDGET, DECISION_ONLY_EVENTS, DecideRecord, EVENT_COMMAND_ALLOWED,
    EVENT_DEFEAT, EVENT_PRODUCTION_ALLOWED, EVENT_PRODUCTION_QUEUED, ResultArea, result_area_bytes,
    result_len,
};
use vera20k_wasm_ext::{
    EventRecord, Export, Invalid, NAME_PROJECTILE, NAME_WARHEAD, NAME_WEAPON, Status, TriggerDef,
};
use vera20k_wasm_ext::triggers::EventDef;

use super::{ModuleEvent, ModuleSource, techno_type};
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::intern::InternedId;
use crate::sim::production::{CanBuild, can_build};
use crate::sim::rng::SimRng;
use crate::sim::world::{FrameEffects, Simulation};

/// The `random` substreams drawn from within one frame (module docs).
#[derive(Default)]
pub(super) struct DecideState {
    frame: u32,
    streams: Vec<(StreamKey, SimRng)>,
}

/// What one link of a decision chain did, when it was not passed over.
enum Link {
    Answered(i64),
    /// Its decision pool was used up.
    Unasked,
}

/// A decision chain's result.
struct Decided {
    /// The last accepted answer.
    answer: Option<i64>,
    /// Some link was [`Link::Unasked`].
    unasked: bool,
}

/// (module, subject creation serial, event kind).
type StreamKey = (ModuleSource, u64, i32);

/// A link's record `value` and `value2` from its trigger's event and its
/// `previous` ([`Simulation::decide_module_event_shaped`]).
pub(crate) type LinkShape<'s> = dyn Fn(&EventDef, i64) -> (i64, i64) + 's;

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
pub(super) fn check_decide_key(
    def: &TriggerDef,
    rules: &RuleSet,
) -> Result<Option<Status>, Invalid> {
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
        return match super::events::raises(rules, event.kind) {
            true => Err(Invalid(format!(
                "event {} has no decision form",
                event.kind
            ))),
            // An event this engine does not raise at all.
            false => Ok(Some(Status::Unsupported)),
        };
    }
    // A decision trigger with immediate actions (module docs).
    Ok((!def.actions.is_empty()).then_some(Status::Unsupported))
}

impl Simulation {
    /// The decision chain for one request (module docs): returns the last
    /// accepted answer. `None` leaves the caller's native result in place.
    /// `house` is the event's house index, or `None` for the trigger's house.
    /// `type_id` is the bound TechnoType, or `None` for the subject's type.
    /// `accept` validates an answer for the kind, returning `None` to reject it.
    pub(crate) fn decide_module_event(
        &mut self,
        rules: &RuleSet,
        event: &ModuleEvent,
        house: Option<u32>,
        type_id: Option<u32>,
        native: i64,
        accept: impl Fn(i64) -> Option<i64>,
    ) -> Option<i64> {
        let mut area = ResultArea::none();
        self.decide_chain(rules, event, house, type_id, native, None, None, &mut area, accept)
            .answer
    }

    /// [`Self::decide_module_event`] whose records each link shapes with
    /// `shape` (`DamageIncoming`): its `value` and `value2` from the link's
    /// own trigger event and its `previous`.
    pub(crate) fn decide_module_event_shaped(
        &mut self,
        rules: &RuleSet,
        event: &ModuleEvent,
        native: i64,
        shape: &LinkShape<'_>,
        accept: impl Fn(i64) -> Option<i64>,
    ) -> Option<i64> {
        let mut area = ResultArea::none();
        self.decide_chain(rules, event, None, None, native, None, Some(shape), &mut area, accept)
            .answer
    }

    /// The decision chain for a kind with a result area (`Fire`, `Launch`,
    /// `ProjectileFrame`, `MissionStep`, `MoveFrame`, `PathFound`; module docs): `area` goes in holding the native
    /// result's bytes (or none) and comes out holding the last accepted
    /// link's, cut to the bytes its answer uses. `None` when no link
    /// answered; `area` is then as it went in.
    pub(crate) fn decide_module_area(
        &mut self,
        rules: &RuleSet,
        event: &ModuleEvent,
        native: i64,
        area: &mut Vec<u8>,
    ) -> Option<i64> {
        let mut result = ResultArea::of(event.kind, std::mem::take(area));
        let answer = self
            .decide_chain(rules, event, None, None, native, None, None, &mut result, Some)
            .answer;
        *area = result.bytes;
        answer
    }

    /// The chain; `cached` is the result a link that could not be asked
    /// passes on in place of the previous one. `area` is the result area
    /// ([`ResultArea::none`] for the kinds without one): each link gets the
    /// previous accepted bytes, and an answer [`result_len`] or `accept`
    /// rejects leaves them in place.
    #[allow(clippy::too_many_arguments)]
    fn decide_chain(
        &mut self,
        rules: &RuleSet,
        event: &ModuleEvent,
        house: Option<u32>,
        type_id: Option<u32>,
        native: i64,
        cached: Option<i64>,
        shape: Option<&LinkShape<'_>>,
        area: &mut ResultArea,
        accept: impl Fn(i64) -> Option<i64>,
    ) -> Decided {
        let mut decided = Decided {
            answer: None,
            unasked: false,
        };
        // No module to ask (or every one is in a call on the stack). A
        // calling module's own links find no slot below: no reentry.
        if self.ext_modules.slots.is_empty() {
            return decided;
        }
        let triggers = self.trigger_runtime.module_triggers();
        let type_id = type_id.or_else(|| {
            triggers.has_type_bindings().then_some(())?;
            let entity = self.entities().get(event.subject)?;
            let object = self.entity_object_type(entity, rules)?;
            rules.type_handle(&object.id).map(|handle| handle.0)
        });
        // A weapon, warhead or projectile type binding reaches the kinds
        // that name them (appendix 4.3 "object + weapon").
        let weapons: Vec<(i32, u32)> = if triggers.has_weapon_bindings() {
            [
                (NAME_WEAPON, event.weapon),
                (NAME_WARHEAD, event.warhead),
                (NAME_PROJECTILE, event.projectile_type),
            ]
            .into_iter()
            .filter_map(|(kind, id)| Some((kind, u32::try_from(id).ok()?)))
            .collect()
        } else {
            Vec::new()
        };
        let mut handles = triggers.deciders(event.kind, event.subject, type_id, &weapons);
        // Mod first, then map; creation order within each (a stable sort).
        handles.sort_by_key(|handle| triggers.get(*handle).map(|t| t.source));
        for handle in handles {
            let previous = decided.answer.unwrap_or(native);
            let before = (result_area_bytes(event.kind) > 0).then(|| area.bytes.clone());
            match self.ask_decider(rules, handle, event, house, native, previous, shape, area) {
                // A shaped chain (`DamageIncoming`) takes 0 or an answer with
                // the sign of the link's `previous`.
                Some(Link::Answered(answer)) => {
                    let accepted = accept(answer).filter(|&answer| {
                        shape.is_none() || answer == 0 || answer.signum() == previous.signum()
                    });
                    match accepted.zip(result_len(event.kind, answer, area.cap)) {
                        Some((answer, len)) => {
                            area.bytes.truncate(len);
                            decided.answer = Some(answer);
                        }
                        None => {
                            if let Some(before) = before {
                                area.bytes = before;
                            }
                        }
                    }
                }
                Some(Link::Unasked) => {
                    decided.unasked = true;
                    decided.answer = cached.or(decided.answer);
                }
                None => {}
            }
        }
        decided
    }

    /// One link: `None` when the trigger is off, its clauses fail, its
    /// module is disabled, or the handler leaves `handled` 0 or faults
    /// (which stops the module).
    #[allow(clippy::too_many_arguments)]
    fn ask_decider(
        &mut self,
        rules: &RuleSet,
        handle: u64,
        event: &ModuleEvent,
        house: Option<u32>,
        native: i64,
        previous: i64,
        shape: Option<&LinkShape<'_>>,
        area: &mut ResultArea,
    ) -> Option<Link> {
        let difficulty = self.trigger_difficulty();
        let trigger = self.trigger_runtime.module_triggers().get(handle)?;
        if !trigger.enabled || !trigger.admits_difficulty(difficulty) {
            return None;
        }
        let (source, key, fixed_house) = (trigger.source, trigger.decide_key, trigger.house);
        let def = trigger.events.first()?.clone();
        let clauses = def.clauses.clone();
        let index = self
            .ext_modules
            .slots
            .iter()
            .position(|slot| slot.source == source)?;
        let frame = self.session.binary_frame();
        if self.ext_modules.slots[index].disabled.is_some() {
            return None;
        }
        let house = house
            .or(fixed_house)
            .or_else(|| self.module_trigger_house(rules, event.subject));
        if !self.module_clauses_hold(rules, source, &clauses, event, house) {
            return None;
        }
        let triggers = self.trigger_runtime.module_triggers();
        if triggers.is_done(handle, event.subject) {
            return None;
        }
        // After the clauses and a spent one-time trigger: only a link that
        // would be asked counts as skipped. Before the latch: a skipped link
        // has not fired.
        let (used, pool) = self.ext_modules.slots[index].decision_pool(frame, event.kind);
        if *used >= pool {
            return Some(Link::Unasked);
        }
        // One event: the instance fires each time unless one-time and done.
        if !self
            .trigger_runtime
            .module_triggers_mut()
            .latch(handle, event.subject, 1)
        {
            return None;
        }
        let mut record = DecideRecord {
            event: EventRecord {
                key,
                ..self.module_event_record(rules, source, handle, event.subject, event, house)
            },
            native,
            previous,
        };
        if let Some(shape) = shape {
            (record.event.value, record.event.value2) = shape(&def, previous);
        }
        let stream_key = (source, event.subject, event.kind);
        let seed = self.session.seed();
        let decide = &mut self.ext_modules.decide;
        decide.roll_to(frame);
        let mut stream = decide.take_stream(seed, stream_key);
        std::mem::swap(&mut self.ext_modules.slots[index].rng, &mut stream);
        let result = self.with_ext_module(
            rules,
            Some(rules.overlay_registry()),
            &mut FrameEffects::empty(),
            index,
            |module, host| {
                // Each module's delivery area bounds its own view of it.
                area.cap = result_area_bytes(event.kind);
                module.decide(&record, area, DECIDE_BUDGET, host)
            },
        );
        std::mem::swap(&mut self.ext_modules.slots[index].rng, &mut stream);
        self.ext_modules.decide.streams.push((stream_key, stream));
        match result {
            Ok((answer, spent)) => {
                *self.ext_modules.slots[index]
                    .decision_pool(frame, event.kind)
                    .0 += spent;
                answer.map(Link::Answered)
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
        self.decide_module_event(rules, &event, Some(house as u32), Some(type_id), 1, allow)
            != Some(0)
    }

    /// `Defeat` for `owner` (module docs): false when a decider vetoes or
    /// could not be asked; the gate asks again next frame.
    pub(crate) fn module_allows_defeat(&mut self, rules: &RuleSet, owner: InternedId) -> bool {
        if !self.trigger_runtime.module_decides(EVENT_DEFEAT) {
            return true;
        }
        let Some(house) = self.houses.index_of(owner) else {
            return true;
        };
        let event = ModuleEvent::new(EVENT_DEFEAT, 0);
        let decided = self.decide_chain(
            rules,
            &event,
            Some(house as u32),
            None,
            1,
            None,
            None,
            &mut ResultArea::none(),
            allow,
        );
        !decided.unasked && decided.answer != Some(0)
    }
}

impl Simulation {
    /// The `ProductionAllowed` step (module docs), at the start of the
    /// module phase: asks for every house that is not defeated and every
    /// bound type, and keeps the answers of 0 for CanBuild until the next
    /// step. Without a decider it only clears what an earlier step kept.
    pub(super) fn decide_production_allowed(&mut self, rules: &RuleSet) {
        let triggers = self.trigger_runtime.module_triggers_mut();
        if !triggers.decides(EVENT_PRODUCTION_ALLOWED) {
            if triggers.refuses_any_production() {
                triggers.set_production_refusals(BTreeSet::new());
            }
            return;
        }
        let types = triggers.decided_types(EVENT_PRODUCTION_ALLOWED);
        // `native` is CanBuild's own answer, with no module refusal kept.
        let last = triggers.take_production_refusals();
        let houses: Vec<(u32, InternedId)> = self
            .houses
            .order()
            .iter()
            .enumerate()
            .filter(|(_, owner)| self.houses.get(owner).is_some_and(|h| !h.is_defeated()))
            .filter_map(|(index, owner)| Some((u32::try_from(index).ok()?, *owner)))
            .collect();
        let objects: Vec<(u32, &ObjectType)> = types
            .iter()
            .filter_map(|&id| Some((id, techno_type(rules, i32::try_from(id).ok()?)?)))
            .collect();
        let mut refusals = BTreeSet::new();
        for (house, owner) in houses {
            for &(type_id, object) in &objects {
                let native = can_build(self, rules, owner, object, false, true) == CanBuild::Yes;
                let event = ModuleEvent {
                    id: type_id as i32,
                    ..ModuleEvent::new(EVENT_PRODUCTION_ALLOWED, 0)
                };
                let decided = self.decide_chain(
                    rules,
                    &event,
                    Some(house),
                    Some(type_id),
                    i64::from(native),
                    // A refusal does not lapse because the budget did.
                    last.contains(&(house, type_id)).then_some(0),
                    None,
                    &mut ResultArea::none(),
                    allow,
                );
                // The sidebar's native answer is the input to the decision.
                // Only a module's veto is shared with other CanBuild routes.
                if decided.answer == Some(0) {
                    refusals.insert((house, type_id));
                }
            }
        }
        self.trigger_runtime
            .module_triggers_mut()
            .set_production_refusals(refusals);
    }

    /// Whether the latest `ProductionAllowed` step disallowed `object` to
    /// `owner`: CanBuild's entry check (module docs). One branch without a
    /// refusal kept.
    pub(crate) fn module_refuses_production(
        &self,
        rules: &RuleSet,
        owner: InternedId,
        object: &ObjectType,
    ) -> bool {
        let triggers = self.trigger_runtime.module_triggers();
        if !triggers.refuses_any_production() {
            return false;
        }
        let (Some(house), Some(type_id)) = (
            self.houses
                .index_of(owner)
                .and_then(|index| u32::try_from(index).ok()),
            rules.type_handle(&object.id).map(|handle| handle.0),
        ) else {
            return false;
        };
        triggers.refuses_production(house, type_id)
    }

    /// `CommandAllowed` for `command` at the command receiver (module
    /// docs): one request per actor, each with `native` 1.
    pub(crate) fn module_admits_command(
        &mut self,
        rules: &RuleSet,
        command: &Command,
    ) -> CommandAdmission {
        if !self.trigger_runtime.module_decides(EVENT_COMMAND_ALLOWED) {
            return CommandAdmission::Allowed;
        }
        let Some(request) = super::orders::command_request(self, rules, command) else {
            return CommandAdmission::Allowed;
        };
        let mut allowed = Vec::with_capacity(request.actors.len());
        for &actor in &request.actors {
            let event = ModuleEvent {
                other: request.other,
                cell: request.cell,
                value: i64::from(request.number),
                ..ModuleEvent::new(EVENT_COMMAND_ALLOWED, actor)
            };
            if self.decide_module_event(rules, &event, None, None, 1, allow) != Some(0) {
                allowed.push(actor);
            }
        }
        match command {
            _ if allowed.len() == request.actors.len() => CommandAdmission::Allowed,
            Command::SetRally { rx, ry, .. } if !allowed.is_empty() => {
                CommandAdmission::Narrowed(Command::SetRally {
                    rx: *rx,
                    ry: *ry,
                    producer_ids: allowed,
                })
            }
            _ => CommandAdmission::Refused,
        }
    }
}

/// What `CommandAllowed` leaves of a command.
pub(crate) enum CommandAdmission {
    /// The command as sent.
    Allowed,
    /// Every actor was refused: the receiver does nothing.
    Refused,
    /// A rally point for the producers not refused.
    Narrowed(Command),
}

/// A yes/no result: 1 or 0; anything else keeps the previous result.
pub(crate) fn allow(answer: i64) -> Option<i64> {
    matches!(answer, 0 | 1).then_some(answer)
}
