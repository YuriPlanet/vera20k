//! Module triggers on the simulation side (RFC 0001 sections 5, 6a and 7):
//! the `trigger_*` imports, raising events at their sites, filter clauses,
//! firing, and `on_events` delivery in the module phase. The triggers
//! themselves live in the trigger runtime
//! (`trigger_runtime::module_triggers`).
//!
//! Events raised so far, each where the engine already commits the outcome
//! (numbers in `vera20k_wasm_ext::triggers`):
//! - `TakeDamage`: the receiver's health write (`object_health::commit`,
//!   Object5F5390), beside stock events 38..43;
//! - `Killed`: the end of `Simulation::record_the_kill` (Techno702D40) for a
//!   terminal kill, after stock events 7, 48 and 29;
//! - `Fire`: the end of a FireAt that launched a bullet
//!   (`world_fire::emit_admitted_fire`, Techno6FDD50);
//! - `Timer`: the start of the module phase.
//!
//! Each site asks [`crate::sim::trigger_runtime::TriggerRuntime::module_listens`]
//! first and builds nothing when no trigger has the kind.
//!
//! Residuals:
//! - Weapon, warhead and projectile type bindings answer `Unsupported`; a
//!   `Fire` trigger binds to its shooter or the shooter's type.
//! - `Killed` carries no lethal warhead (`id` −1): `record_the_kill` is
//!   not told it.
//! - Chain depth and per-frame firing caps (open question 17) are not needed
//!   yet: no action a trigger may run here raises an event.
//! - Immediate actions are `AddMoney` and `SetVar` on Red Alert 2 variables;
//!   other kinds make `trigger_create` answer `Unsupported`.
//! - A trigger mixing `Timer` with object events answers `Unsupported`.
//! - The "flag" an event site checks is a per-kind test, then a map lookup
//!   by subject, not a side column in entity storage (RFC 15.1, item 1).

use vera20k_wasm_ext::triggers::{
    ACTION_MODULE, ACTION_SET_VAR, ActionDef, ActionTarget, EVENT_FIRE, EVENT_KILLED,
    EVENT_TAKE_DAMAGE, EVENT_TIMER, EventDef, FLAG_STARTS_DISABLED, FilterClause, FilterSubject,
    VarTarget,
};
use vera20k_wasm_ext::{
    EventRecord, Fault, Invalid, NAME_TECHNO_TYPE, SimHost, SimModule, Status, TriggerDef,
    property_id, property_name,
};

use super::{
    ACTION_ADD_MONEY, FRAME_BUDGET, GLOBAL_VARS, Host, LOCAL_VARS, ModuleSource, Ref, cell_ref,
    decode_ref, house_ref,
};
use crate::rules::ruleset::RuleSet;
use crate::sim::rng::SimRng;
use crate::sim::trigger_runtime::ModuleTrigger;
use crate::sim::world::Simulation;

/// The event kinds this engine raises for module triggers.
pub const RAISED_EVENTS: [i32; 4] = [EVENT_TAKE_DAMAGE, EVENT_KILLED, EVENT_FIRE, EVENT_TIMER];

/// Records one module receives per frame, and how many frames' worth may
/// wait before it is stopped as unable to keep up (open question 7).
pub const MAX_RECORDS_PER_FRAME: usize = 1024;
pub const MAX_BACKLOG_FRAMES: usize = 16;

/// An occurrence an event site hands over. Unset integers stay −1 and
/// unset references 0, as the record layout asks.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ModuleEvent {
    pub(crate) kind: i32,
    /// Creation serial, 0 for none.
    pub(crate) subject: u64,
    pub(crate) other: u64,
    pub(crate) projectile: u64,
    /// In cells.
    pub(crate) cell: Option<(u16, u16)>,
    pub(crate) value: i64,
    pub(crate) id: i32,
    /// Record the subject's position (`x`, `y`, `z`).
    pub(crate) at_subject: bool,
}

impl ModuleEvent {
    pub(crate) const fn new(kind: i32, subject: u64) -> Self {
        Self {
            kind,
            subject,
            other: 0,
            projectile: 0,
            cell: None,
            value: -1,
            id: -1,
            at_subject: false,
        }
    }
}

/// A variable ID a trigger may read or write: Red Alert 2 variables have
/// global scope only. `vera.vars` IDs (from 1000) are not kept yet.
fn check_variable(var: i32, target: VarTarget) -> Result<Option<Status>, Invalid> {
    let stock = LOCAL_VARS.contains(&var) || GLOBAL_VARS.contains(&var);
    match (stock, target) {
        (true, VarTarget::Global) => Ok(None),
        (true, _) => Err(Invalid(format!("variable {var} has global scope"))),
        (false, _) if var >= 1000 => Ok(Some(Status::Unsupported)),
        (false, _) => Err(Invalid(format!("unknown variable {var}"))),
    }
}

impl Host<'_> {
    /// A house reference's array index; `None` for one past the houses.
    fn house_index(&self, reference: i64) -> Result<Option<u32>, Invalid> {
        match decode_ref(reference)? {
            Some(Ref::House(index)) if index < self.sim.houses.order().len() => {
                Ok(Some(index as u32))
            }
            Some(Ref::House(_)) => Ok(None),
            _ => Err(Invalid(format!("{reference:#x} is not a house reference"))),
        }
    }

    /// A module trigger's handle; `None` for 0 or a removed trigger. An
    /// object that is not a trigger is invalid.
    fn trigger_handle(&self, reference: i64) -> Result<Option<u64>, Invalid> {
        match decode_ref(reference)? {
            None => Ok(None),
            Some(Ref::Object(serial)) => {
                if self
                    .sim
                    .trigger_runtime
                    .module_triggers()
                    .get(serial)
                    .is_some()
                {
                    Ok(Some(serial))
                } else if self.sim.entities().get(serial).is_some() {
                    Err(Invalid(format!("{reference:#x} is not a trigger")))
                } else {
                    Ok(None)
                }
            }
            Some(_) => Err(Invalid(format!("{reference:#x} is not a trigger"))),
        }
    }

    /// `trigger_create`: checks every event, clause and action, then adds
    /// the trigger to the trigger runtime.
    pub(super) fn create_trigger(&mut self, def: &TriggerDef) -> Result<(i64, Status), Invalid> {
        // Decision handlers come with the decidable events (open question 10).
        if def.decide_key != -1 {
            return Ok((0, Status::Unsupported));
        }
        let house = match def.house {
            0 => None,
            reference => match self.house_index(reference)? {
                Some(index) => Some(index),
                None => return Ok((0, Status::Gone)),
            },
        };
        for event in &def.events {
            if let Some(status) = check_event(event)? {
                return Ok((0, status));
            }
        }
        let timers = def.events.iter().filter(|e| e.kind == EVENT_TIMER).count();
        if timers != 0 && timers != def.events.len() {
            return Ok((0, Status::Unsupported));
        }
        for action in &def.actions {
            if let Some(status) = self.check_action(action)? {
                return Ok((0, status));
            }
        }
        let handle = self.sim.substrate.allocate_stable_id();
        let trigger = ModuleTrigger {
            source: self.source,
            house,
            repeat: def.repeat,
            difficulty: def.difficulty,
            flags: def.flags,
            enabled: def.flags & FLAG_STARTS_DISABLED == 0,
            created: self.sim.session.binary_frame(),
            events: def.events.clone(),
            actions: def.actions.clone(),
        };
        self.sim
            .trigger_runtime
            .module_triggers_mut()
            .create(handle, trigger);
        Ok((handle as i64, Status::Ok))
    }

    /// What `trigger_create` accepts of an action (appendix 5.1, 5.3).
    fn check_action(&self, action: &ActionDef) -> Result<Option<Status>, Invalid> {
        let params = &action.params;
        match action.kind {
            ACTION_MODULE => Ok(None),
            ACTION_SET_VAR => check_variable(params.id, VarTarget::Global),
            ACTION_ADD_MONEY => {
                if action.target != ActionTarget::House {
                    return Err(Invalid(format!(
                        "AddMoney targets a house, not {:?}",
                        action.target
                    )));
                }
                if i32::try_from(params.value).is_err() {
                    return Err(Invalid(format!(
                        "AddMoney amount {} is out of range",
                        params.value
                    )));
                }
                match params.house {
                    0 => Ok(None),
                    house => Ok(self.house_index(house)?.is_none().then_some(Status::Gone)),
                }
            }
            _ => Ok(Some(Status::Unsupported)),
        }
    }

    pub(super) fn remove_trigger(&mut self, reference: i64) -> Result<Status, Invalid> {
        let Some(handle) = self.trigger_handle(reference)? else {
            return Ok(Status::Gone);
        };
        self.sim
            .trigger_runtime
            .module_triggers_mut()
            .remove(handle);
        Ok(Status::Ok)
    }

    /// `trigger_bind` and, with `bind` false, `trigger_unbind`.
    pub(super) fn bind_trigger(
        &mut self,
        reference: i64,
        target: i64,
        bind: bool,
    ) -> Result<Status, Invalid> {
        let Some(handle) = self.trigger_handle(reference)? else {
            return Ok(Status::Gone);
        };
        let global = self
            .sim
            .trigger_runtime
            .module_triggers()
            .get(handle)
            .is_some_and(ModuleTrigger::is_global);
        if global {
            return Err(Invalid(format!(
                "trigger {reference:#x} has only global events and takes no binding"
            )));
        }
        let found = match decode_ref(target)? {
            None => return Ok(Status::Gone),
            Some(Ref::Object(object)) => {
                if self.sim.entities().get(object).is_none() {
                    return match self.trigger_handle(target) {
                        Ok(None) => Ok(Status::Gone),
                        _ => Err(Invalid(format!("{target:#x} is not an object"))),
                    };
                }
                let triggers = self.sim.trigger_runtime.module_triggers_mut();
                if bind {
                    triggers.bind_object(handle, object);
                    true
                } else {
                    triggers.unbind_object(handle, object)
                }
            }
            Some(Ref::Type {
                kind: NAME_TECHNO_TYPE,
                id,
            }) => {
                let known = i32::try_from(id).is_ok_and(|id| self.techno_type(id).is_some());
                if !known {
                    return Err(Invalid(format!("no TechnoType {id}")));
                }
                let triggers = self.sim.trigger_runtime.module_triggers_mut();
                if bind {
                    triggers.bind_type(handle, id);
                    true
                } else {
                    triggers.unbind_type(handle, id)
                }
            }
            // Weapon, warhead and projectile type bindings (module residuals).
            Some(Ref::Type { kind: 1..=3, .. }) => return Ok(Status::Unsupported),
            // No event this engine raises allows a cell binding (appendix 4.1).
            Some(_) => {
                return Err(Invalid(format!(
                    "{target:#x} cannot hold a trigger binding"
                )));
            }
        };
        Ok(if found { Status::Ok } else { Status::NotFound })
    }

    /// A trigger's property (appendix 7.9) for `get`, or with `write` for
    /// `set`; `None` when `reference` is not a module trigger.
    pub(super) fn trigger_property(
        &mut self,
        reference: i64,
        prop: i32,
        write: Option<i64>,
    ) -> Option<(i64, Status)> {
        let Ok(Some(handle)) = self.trigger_handle(reference) else {
            return None;
        };
        let triggers = self.sim.trigger_runtime.module_triggers_mut();
        let trigger = triggers.get(handle)?;
        let name = property_name(prop).unwrap_or_default();
        Some(match (name, write) {
            ("Enabled", None) => (i64::from(trigger.enabled), Status::Ok),
            ("Enabled", Some(value)) => {
                triggers.set_enabled(handle, value != 0);
                (0, Status::Ok)
            }
            ("House", None) => (
                trigger.house.map_or(0, |index| house_ref(index as usize)),
                Status::Ok,
            ),
            ("Repeat", None) => (trigger.repeat as i64, Status::Ok),
            ("House" | "Repeat", Some(_)) => (0, Status::Refused),
            _ => (0, Status::Unsupported),
        })
    }
}

/// What `trigger_create` accepts of an event: a kind this engine raises, a
/// positive `Timer` interval and clauses on Red Alert 2 variables.
fn check_event(event: &EventDef) -> Result<Option<Status>, Invalid> {
    if !RAISED_EVENTS.contains(&event.kind) {
        return Ok(Some(Status::Unsupported));
    }
    if event.kind == EVENT_TIMER && event.interval < 1 {
        return Err(Invalid(format!("Timer interval {}", event.interval)));
    }
    for clause in &event.clauses {
        if let FilterSubject::Var { var, target } = clause.subject
            && let Some(status) = check_variable(var, target)?
        {
            return Ok(Some(status));
        }
    }
    Ok(None)
}

impl Simulation {
    /// Runs `f` with the host module `source` sees. Inside a module call its
    /// slot is set aside, and a placeholder stream serves: clause reads and
    /// trigger actions draw nothing.
    fn with_module_host<R>(
        &mut self,
        rules: &RuleSet,
        source: ModuleSource,
        f: impl FnOnce(&mut Host<'_>) -> R,
    ) -> R {
        let mut slots = std::mem::take(&mut self.ext_modules.slots);
        let result = match slots.iter_mut().find(|slot| slot.source == source) {
            Some(slot) => f(&mut Host {
                sim: self,
                rules,
                rng: &mut slot.rng,
                source,
            }),
            None => f(&mut Host {
                sim: self,
                rules,
                rng: &mut SimRng::new(0),
                source,
                orders: &mut Vec::new(),
                rules_ini: None,
            }),
        };
        self.ext_modules.slots = slots;
        result
    }

    /// Offers an object event to the triggers bound to its subject or the
    /// subject's TechnoType. Call it only when
    /// `trigger_runtime.module_listens(event.kind)`.
    pub(crate) fn raise_module_event(&mut self, rules: &RuleSet, event: ModuleEvent) {
        let triggers = self.trigger_runtime.module_triggers();
        let type_id = if triggers.has_type_bindings() {
            self.entities()
                .get(event.subject)
                .and_then(|entity| self.entity_object_type(entity, rules))
                .and_then(|object| rules.type_handle(&object.id))
                .map(|handle| handle.0)
        } else {
            None
        };
        let handles =
            self.trigger_runtime
                .module_triggers()
                .bound(event.kind, event.subject, type_id);
        for handle in handles {
            self.offer_module_event(rules, handle, event.subject, &event);
        }
    }

    /// `Timer` triggers whose period ends this frame, from the start of the
    /// module phase. A period counts from the trigger's creation frame.
    pub(super) fn raise_module_timers(&mut self, rules: &RuleSet) {
        if !self.trigger_runtime.module_listens(EVENT_TIMER) {
            return;
        }
        let frame = self.session.binary_frame();
        for handle in self.trigger_runtime.module_triggers().global(EVENT_TIMER) {
            let Some(trigger) = self.trigger_runtime.module_triggers().get(handle) else {
                continue;
            };
            let Some(interval) = trigger
                .events
                .iter()
                .find(|event| timer_due(event, trigger.created, frame))
                .map(|event| event.interval)
            else {
                continue;
            };
            let event = ModuleEvent {
                // The frame of the previous firing, or of creation.
                value: i64::from(frame) - i64::from(interval),
                ..ModuleEvent::new(EVENT_TIMER, 0)
            };
            self.offer_module_event(rules, handle, 0, &event);
        }
    }

    /// One trigger instance meets one occurrence: its events of that kind
    /// whose clauses hold latch, and when every event has latched the
    /// trigger fires.
    fn offer_module_event(
        &mut self,
        rules: &RuleSet,
        handle: u64,
        holder: u64,
        event: &ModuleEvent,
    ) {
        let difficulty = self.trigger_difficulty();
        let frame = self.session.binary_frame();
        let Some(trigger) = self.trigger_runtime.module_triggers().get(handle) else {
            return;
        };
        if !trigger.enabled || !trigger.admits_difficulty(difficulty) {
            return;
        }
        let (source, created) = (trigger.source, trigger.created);
        let fixed_house = trigger.house;
        let events: Vec<(usize, Vec<FilterClause>)> = trigger
            .events
            .iter()
            .enumerate()
            .filter(|(_, def)| {
                def.kind == event.kind
                    && (def.kind != EVENT_TIMER || timer_due(def, created, frame))
            })
            .map(|(bit, def)| (bit, def.clauses.clone()))
            .collect();
        let house = fixed_house.or_else(|| self.module_trigger_house(rules, event.subject));
        let mut seen = 0u32;
        for (bit, clauses) in events {
            if self.module_clauses_hold(rules, source, &clauses, event, house) {
                seen |= 1 << bit;
            }
        }
        if seen != 0
            && self
                .trigger_runtime
                .module_triggers_mut()
                .latch(handle, holder, seen)
        {
            self.fire_module_trigger(rules, handle, holder, event, house);
        }
    }

    /// The trigger house when the definition names none: the subject's
    /// current owner, else the Neutral house (RFC 6a).
    fn module_trigger_house(&self, rules: &RuleSet, subject: u64) -> Option<u32> {
        let owner = self.entities().get(subject).map(|entity| entity.owner());
        let neutral = || {
            rules
                .country_index_of_name("Neutral")
                .and_then(|country| self.house_by_country_index(country, rules))
        };
        owner
            .or_else(neutral)
            .and_then(|house| self.houses.index_of(house))
            .map(|index| index as u32)
    }

    /// Whether every clause holds, each read as the trigger's module would
    /// read it with `get` or `var_get`. A reading that is not `Ok` fails.
    fn module_clauses_hold(
        &mut self,
        rules: &RuleSet,
        source: ModuleSource,
        clauses: &[FilterClause],
        event: &ModuleEvent,
        house: Option<u32>,
    ) -> bool {
        if clauses.is_empty() {
            return true;
        }
        let house = house.map_or(0, |index| house_ref(index as usize));
        self.with_module_host(rules, source, |host| {
            clauses.iter().all(|clause| {
                let reading = match clause.subject {
                    FilterSubject::Subject(prop) => host.read(event.subject as i64, prop),
                    FilterSubject::Other(prop) => host.read(event.other as i64, prop),
                    FilterSubject::House(prop) => host.read(house, prop),
                    FilterSubject::Var { var, .. } => host.read_var(0, var),
                };
                matches!(reading, Ok((value, Status::Ok)) if clause.holds(value))
            })
        })
    }

    /// Runs a firing trigger's actions in order: module actions queue a
    /// record for the module phase, the others run now through the host's
    /// own `var_set` and `action` (RFC section 5, "Action timing").
    fn fire_module_trigger(
        &mut self,
        rules: &RuleSet,
        handle: u64,
        holder: u64,
        event: &ModuleEvent,
        house: Option<u32>,
    ) {
        let Some(trigger) = self.trigger_runtime.module_triggers().get(handle) else {
            return;
        };
        let (source, actions) = (trigger.source, trigger.actions.clone());
        let house = house.map_or(0, |index| house_ref(index as usize));
        let [x, y, z] = if event.at_subject {
            self.with_module_host(rules, source, |host| {
                ["X", "Y", "Z"].map(|name| {
                    match property_id(name).map(|prop| host.read(event.subject as i64, prop)) {
                        Some(Ok((value, Status::Ok))) => i32::try_from(value).unwrap_or(-1),
                        _ => -1,
                    }
                })
            })
        } else {
            [-1; 3]
        };
        let record = EventRecord {
            key: 0,
            kind: event.kind,
            trigger: handle as i64,
            holder: holder as i64,
            subject: event.subject as i64,
            other: event.other as i64,
            house,
            projectile: event.projectile as i64,
            modifier: 0,
            cell: event.cell.map_or(0, |(x, y)| cell_ref(x, y)),
            value: event.value,
            value2: -1,
            id: event.id,
            frame: self.session.binary_frame() as i32,
            x,
            y,
            z,
        };
        for action in actions {
            if action.kind == ACTION_MODULE {
                self.trigger_runtime.module_triggers_mut().enqueue(
                    source,
                    EventRecord {
                        key: action.params.id,
                        ..record
                    },
                );
                continue;
            }
            // In a trigger, house 0 is the trigger house and the event
            // supplies the other object (appendix 5.1).
            let mut params = action.params;
            if params.house == 0 {
                params.house = house;
            }
            params.other = record.other;
            let target = match action.target {
                ActionTarget::Subject => record.subject,
                ActionTarget::Other => record.other,
                ActionTarget::TagHolder => record.holder,
                ActionTarget::Projectile => record.projectile,
                ActionTarget::House => params.house,
                ActionTarget::Cell => params.cell,
            };
            let result = self.with_module_host(rules, source, |host| match action.kind {
                ACTION_SET_VAR => host.var_set(0, params.id, params.value).map(|s| (0, s)),
                kind => host.action(kind, target, &params),
            });
            // `trigger_create` checked the action; what is left is a data
            // error at execution, logged and skipped (RFC 6b).
            match result {
                Ok((_, Status::Ok)) => {}
                Ok((_, status)) => log::info!(
                    target: "ext_module",
                    "trigger {handle} action {} answered {status:?}",
                    action.kind
                ),
                Err(Invalid(why)) => log::warn!(
                    target: "ext_module",
                    "trigger {handle} action {} refused: {why}",
                    action.kind
                ),
            }
        }
    }

    /// The module phase's delivery for module `index`: its queued records go
    /// to `on_events` in batches the delivery area holds, while the frame
    /// budget lasts and up to [`MAX_RECORDS_PER_FRAME`]; the rest wait, in
    /// order, for the next frame. Records of a disabled or removed trigger,
    /// and every record once the game has ended, are dropped. A backlog past
    /// [`MAX_BACKLOG_FRAMES`] frames stops the module.
    pub(super) fn deliver_ext_module_events(
        &mut self,
        rules: &RuleSet,
        index: usize,
    ) -> Result<(), Fault> {
        let Some(slot) = self.ext_modules.slots.get(index) else {
            return Ok(());
        };
        let (source, capacity) = (slot.source, slot.module.batch_capacity());
        let ended = self.session.outcome_request().is_some();
        let triggers = self.trigger_runtime.module_triggers_mut();
        if capacity == 0 || ended {
            triggers.clear_queue(source);
            return Ok(());
        }
        let ready = triggers.prepare_delivery(source).min(MAX_RECORDS_PER_FRAME);
        let (mut delivered, mut used) = (0, 0);
        while delivered < ready && used < FRAME_BUDGET {
            let batch = self
                .trigger_runtime
                .module_triggers_mut()
                .take(source, capacity.min(ready - delivered));
            if batch.is_empty() {
                break;
            }
            delivered += batch.len();
            used += self.with_ext_module(rules, index, |module: &mut SimModule, host| {
                module.deliver(&batch, FRAME_BUDGET, host)
            })?;
        }
        let backlog = self.trigger_runtime.module_triggers().queued(source);
        if backlog > MAX_RECORDS_PER_FRAME * MAX_BACKLOG_FRAMES {
            return Err(Fault::Backlog(backlog));
        }
        Ok(())
    }
}

/// Whether a `Timer` event's period ends on `frame`.
fn timer_due(event: &EventDef, created: u32, frame: u32) -> bool {
    let interval = u32::try_from(event.interval).unwrap_or(0);
    interval > 0 && frame > created && (frame - created) % interval == 0
}
