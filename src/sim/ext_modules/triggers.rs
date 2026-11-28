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
//!   terminal kill, after stock events 7, 48 and 29, with the lethal warhead
//!   when the receiver's kill path names one;
//! - `Fire`: the end of a FireAt that launched a bullet
//!   (`world_fire::emit_admitted_fire`, Techno6FDD50);
//! - `Timer`: the start of the module phase, once for a global trigger and
//!   once per instance (bound object, object of a bound type) for a trigger
//!   whose other events are object events;
//! - stock events, the other delivered events and the polling events:
//!   [`super::events`].
//!
//! Each site asks [`crate::sim::trigger_runtime::TriggerRuntime::module_listens`]
//! first and builds nothing when no trigger has the kind.
//!
//! Chains (open question 17): events raised by a module trigger's immediate
//! actions fire module triggers again, up to the caps in [`super::chains`];
//! a firing past them waits for the next module phase.
//!
//! Weapon, warhead and projectile type bindings (name kinds 1, 2 and 3)
//! serve the "Object + weapon" events (`Fire`): the event names the shot's
//! weapon, its warhead and its projectile type.
//!
//! Residuals:
//! - Immediate actions are the kinds [`super::run_action`] implements, run
//!   through it with the trigger house and the text `trigger_create` copied;
//!   other kinds make `trigger_create` answer `Unsupported`.
//! - A trigger mixing global events (house and logic events, global polls,
//!   `Defeat`) with object or cell events answers `Unsupported`, as does
//!   `Timer` with cell events. `Timer` with object events on a trigger bound
//!   only to weapon types never fires: such a trigger has no instances.
//! - The "flag" an event site checks is a per-kind test, then a map lookup
//!   by subject, not a side column in entity storage (RFC 15.1, item 1).

use vera20k_wasm_ext::triggers::{
    ACTION_MODULE, ACTION_SET_VAR, ActionDef, ActionTarget, EVENT_TIMER, EventDef,
    FLAG_STARTS_DISABLED, FilterClause, FilterSubject, VarTarget,
};
use vera20k_wasm_ext::{
    EventRecord, Fault, Invalid, NAME_PROJECTILE, NAME_TECHNO_TYPE, NAME_WARHEAD, NAME_WEAPON,
    SimModule, Status, TriggerDef, VarScope, property_id, property_name,
};

use super::{
    ActionContext, GLOBAL_VARS, Host, LOCAL_VARS, ModuleSource, Ref, cell_ref, decode_ref,
    house_ref, run_action, techno_type, vars::VarStore,
};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::rng::SimRng;
use crate::sim::trigger_runtime::{
    MAX_BACKLOG_RECORDS, MAX_QUEUED_RECORDS, MAX_RECORDS_PER_FRAME, ModuleTrigger, StockInput,
    TriggerScope,
};
use crate::sim::world::{FrameEffects, Simulation};

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
    pub(crate) value2: i64,
    pub(crate) id: i32,
    /// Record the subject's position (`x`, `y`, `z`).
    pub(crate) at_subject: bool,
    /// A position in leptons the record carries instead (`Detonated`'s
    /// detonation point).
    pub(crate) at: Option<[i32; 3]>,
    /// The house the record carries instead of the trigger's (array index).
    pub(crate) house: Option<u32>,
    /// A polling event's index in the trigger it is checked for: only that
    /// event latches.
    pub(crate) poll: Option<usize>,
    /// Weapon, warhead and projectile type IDs whose type bindings the
    /// event reaches (−1 for none); not part of the record.
    pub(crate) weapon: i32,
    pub(crate) warhead: i32,
    pub(crate) projectile_type: i32,
    /// What a stock event's raise handed its Tag funnel, which the Ares
    /// predicates read; not part of the record.
    pub(crate) stock: StockInput,
    /// The modifier handle of a modifier event, 0 for none.
    pub(crate) modifier: u64,
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
            value2: -1,
            id: -1,
            at_subject: false,
            at: None,
            house: None,
            poll: None,
            weapon: -1,
            warhead: -1,
            projectile_type: -1,
            stock: StockInput::NONE,
            modifier: 0,
        }
    }
}

/// A variable a trigger may read or write, with the scope its holder must
/// have: Red Alert 2 variables are global, `vera.vars` ones (from 1000) have
/// their declared scope.
pub(super) fn variable_scope(vars: &VarStore, var: i32) -> Result<VarScope, Invalid> {
    if LOCAL_VARS.contains(&var) || GLOBAL_VARS.contains(&var) {
        return Ok(VarScope::Global);
    }
    vars.scope(var)
        .ok_or_else(|| Invalid(format!("unknown variable {var}")))
}

/// A clause's variable target must name a holder of the variable's scope.
fn check_clause_variable(vars: &VarStore, var: i32, target: VarTarget) -> Result<(), Invalid> {
    let scope = variable_scope(vars, var)?;
    let fits = match target {
        VarTarget::Global => scope == VarScope::Global,
        VarTarget::Subject | VarTarget::Other => scope == VarScope::Object,
        VarTarget::House => scope == VarScope::House,
    };
    match fits {
        true => Ok(()),
        false => Err(Invalid(format!(
            "variable {var} has {scope:?} scope, not {target:?}"
        ))),
    }
}

impl Host<'_, '_> {
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
        if let Some(status) = super::decide::check_decide_key(def, self.rules)? {
            return Ok((0, status));
        }
        let house = match def.house {
            0 => None,
            reference => match self.house_index(reference)? {
                Some(index) => Some(index),
                None => return Ok((0, Status::Gone)),
            },
        };
        let (mut scope, mut timer) = (None, false);
        for event in &def.events {
            for clause in &event.clauses {
                if let FilterSubject::Var { var, target } = clause.subject {
                    check_clause_variable(&self.sim.ext_modules.vars, var, target)?;
                }
            }
            let this = match self.event_scope(event)? {
                Ok(this) => this,
                Err(status) => return Ok((0, status)),
            };
            // `Timer` takes the scope of the trigger's other events: each
            // instance counts its periods (`raise_module_timers`).
            if event.kind == EVENT_TIMER {
                timer = true;
                continue;
            }
            scope = match (scope, this) {
                (None, this) => Some(this),
                (Some(a), b) if a == b => Some(a),
                // An object trigger admits weapon bindings only when every
                // event does.
                (Some(TriggerScope::Object { .. }), TriggerScope::Object { .. }) => {
                    Some(TriggerScope::Object { weapon: false })
                }
                _ => return Ok((0, Status::Unsupported)),
            };
        }
        let scope = match (scope, timer) {
            (None, true) => TriggerScope::Global,
            (Some(TriggerScope::Cell), true) | (None, false) => {
                return Ok((0, Status::Unsupported));
            }
            (Some(scope), _) => scope,
        };
        for action in &def.actions {
            if let Some(status) = self.check_action(action)? {
                return Ok((0, status));
            }
        }
        if !self
            .sim
            .trigger_runtime
            .module_triggers()
            .can_create(self.source, scope, &def.events)
        {
            return Ok((0, Status::Refused));
        }
        // A module action's parameter text becomes a string ID here, once,
        // kept in its `value` (unused by `Module`), so a firing interns
        // nothing (RFC 0001 section 6h).
        let mut actions = def.actions.clone();
        for action in actions.iter_mut().filter(|a| a.kind == ACTION_MODULE) {
            action.params.value = i64::from(self.sim.ext_modules.ini.intern(&action.text));
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
            poll_start: self.sim.session.binary_frame(),
            scope,
            events: def.events.clone(),
            actions,
            ini: false,
            decide_key: def.decide_key,
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
        // SetVar's target must fit the variable's scope; `run_action` checks
        // the other kinds' operands when the trigger fires.
        match action.kind {
            ACTION_MODULE => Ok(None),
            // Object scope writes the target object, house scope the house
            // target, global scope ignores the target (appendix 5.3).
            ACTION_SET_VAR => match variable_scope(&self.sim.ext_modules.vars, params.id)? {
                VarScope::Global => Ok(None),
                VarScope::House if action.target == ActionTarget::House => Ok(None),
                VarScope::Object
                    if !matches!(action.target, ActionTarget::House | ActionTarget::Cell) =>
                {
                    Ok(None)
                }
                scope => Err(Invalid(format!(
                    "SetVar of {scope:?} variable {} targets {:?}",
                    params.id, action.target
                ))),
            },
            kind if super::actions::has_action(kind, self.rules) => Ok(None),
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
        let scope = self
            .sim
            .trigger_runtime
            .module_triggers()
            .get(handle)
            .map(|trigger| trigger.scope);
        let refuse = |what: &str| {
            Err(Invalid(format!(
                "trigger {reference:#x} ({scope:?}) takes no {what} binding"
            )))
        };
        let target_ref = decode_ref(target)?;
        let object_scope = matches!(scope, Some(TriggerScope::Object { .. }));
        match target_ref {
            None => return Ok(Status::Gone),
            Some(Ref::Cell { .. }) if scope != Some(TriggerScope::Cell) => return refuse("cell"),
            Some(Ref::Object(_) | Ref::Type { .. }) if !object_scope => {
                return refuse("object or type");
            }
            Some(Ref::Type {
                kind: NAME_WEAPON | NAME_WARHEAD | NAME_PROJECTILE,
                ..
            }) if scope != Some(TriggerScope::Object { weapon: true }) => {
                return refuse("weapon, warhead or projectile");
            }
            _ => {}
        }
        let found = match target_ref {
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
                let known = i32::try_from(id).is_ok_and(|id| techno_type(self.rules, id).is_some());
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
            Some(Ref::Type {
                kind: kind @ (NAME_WEAPON | NAME_WARHEAD | NAME_PROJECTILE),
                id,
            }) => {
                let known = match kind {
                    NAME_WEAPON => self.rules.weapon_at(id).is_some(),
                    NAME_WARHEAD => self.rules.warhead_at(id).is_some(),
                    _ => self.rules.projectile_at(id).is_some(),
                };
                if !known {
                    return Err(Invalid(format!("no type {id} of name kind {kind}")));
                }
                let triggers = self.sim.trigger_runtime.module_triggers_mut();
                if bind {
                    triggers.bind_weapon(handle, kind, id);
                    true
                } else {
                    triggers.unbind_weapon(handle, kind, id)
                }
            }
            Some(Ref::Cell { x, y }) => {
                let triggers = self.sim.trigger_runtime.module_triggers_mut();
                if bind {
                    triggers.bind_cell(handle, (x, y));
                    true
                } else {
                    triggers.unbind_cell(handle, (x, y))
                }
            }
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

impl Simulation {
    /// Runs `f` with the host module `source` sees, for the reads of an
    /// event site (clauses, the record's position):
    /// [`Simulation::with_ext_module`] on its slot. Inside that module's own
    /// call its slot is out of the list, and a placeholder stream serves:
    /// reads draw nothing. Reads reach no lent owner, so no frame effects
    /// are lent.
    pub(super) fn with_module_host<R>(
        &mut self,
        rules: &RuleSet,
        source: ModuleSource,
        f: impl FnOnce(&mut Host<'_, '_>) -> R,
    ) -> R {
        let overlay_registry = Some(rules.overlay_registry());
        let mut effects = FrameEffects::empty();
        match self
            .ext_modules
            .slots
            .iter()
            .position(|s| s.source == source)
        {
            Some(index) => {
                self.with_ext_module(rules, overlay_registry, &mut effects, index, |_, host| {
                    f(host)
                })
            }
            None => f(&mut Host {
                sim: self,
                rules,
                overlay_registry,
                frame_effects: &mut effects,
                rng: &mut SimRng::new(0),
                source,
                orders: &mut Vec::new(),
                rules_ini: None,
            }),
        }
    }

    /// Offers an object event to the triggers bound to its subject or the
    /// subject's TechnoType. Call it only when
    /// `trigger_runtime.module_listens(event.kind)`.
    pub(crate) fn raise_module_event(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        event: ModuleEvent,
    ) {
        // Client modules' `SimEvent` ([`super::client_outbox`]).
        self.forward_client_event(rules, &event);
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
        let handles = self.trigger_runtime.module_triggers().bound(
            event.kind,
            event.subject,
            type_id,
            &weapons,
        );
        for handle in handles {
            self.offer_module_event(rules, frame_effects, handle, event.subject, &event);
        }
    }

    /// `Timer` triggers whose period ends this frame, from the start of the
    /// module phase. A period counts from the trigger's creation frame. A
    /// global trigger has one instance; a trigger with object events counts
    /// the period on each instance: every bound object and every object of
    /// a bound type, in creation order.
    pub(super) fn raise_module_timers(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        if !self.trigger_runtime.module_listens(EVENT_TIMER) {
            return;
        }
        let frame = self.session.binary_frame();
        for handle in self.trigger_runtime.module_triggers().handles() {
            let Some(trigger) = self.trigger_runtime.module_triggers().get(handle) else {
                continue;
            };
            if !trigger.enabled {
                continue;
            }
            let Some(interval) = trigger
                .events
                .iter()
                .find(|event| event.kind == EVENT_TIMER && timer_due(event, trigger.created, frame))
                .map(|event| event.interval)
            else {
                continue;
            };
            let holders = if trigger.is_global() {
                vec![0]
            } else {
                self.module_poll_holders(rules, handle)
            };
            for holder in holders {
                let event = ModuleEvent {
                    // The frame of the previous firing, or of creation.
                    value: i64::from(frame) - i64::from(interval),
                    ..ModuleEvent::new(EVENT_TIMER, holder)
                };
                self.offer_module_event(rules, frame_effects, handle, holder, &event);
            }
        }
    }

    /// One trigger instance meets one occurrence: its events of that kind
    /// whose clauses hold latch, and when every event has latched the
    /// trigger fires.
    pub(super) fn offer_module_event(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
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
        let (source, created, poll_start) = (trigger.source, trigger.created, trigger.poll_start);
        let fixed_house = trigger.house;
        let events: Vec<(usize, EventDef)> = trigger
            .events
            .iter()
            .enumerate()
            .filter(|(_, def)| {
                def.kind == event.kind
                    && (def.kind != EVENT_TIMER || timer_due(def, created, frame))
            })
            .map(|(bit, def)| (bit, def.clone()))
            .collect();
        let house = fixed_house.or_else(|| self.module_trigger_house(rules, event.subject));
        let mut seen = 0u32;
        let mut record_house = event.house;
        for (bit, def) in events {
            let Some(carried) =
                self.module_event_admits(rules, &def, bit, event, house, poll_start)
            else {
                continue;
            };
            if self.module_clauses_hold(rules, source, &def.clauses, event, house) {
                seen |= 1 << bit;
                record_house = carried.or(record_house);
            }
        }
        if seen != 0
            && self
                .trigger_runtime
                .module_triggers_mut()
                .latch(handle, holder, seen)
        {
            let event = ModuleEvent {
                house: record_house,
                ..*event
            };
            self.fire_module_trigger(rules, frame_effects, handle, holder, &event, house);
        }
    }

    /// The trigger house when the definition names none: the subject's
    /// current owner, else the Neutral house (RFC 6a).
    pub(super) fn module_trigger_house(&self, rules: &RuleSet, subject: u64) -> Option<u32> {
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
    /// read it with `get` or `var_get`, viewpoint properties for the trigger
    /// house. A reading that is not `Ok` fails.
    pub(super) fn module_clauses_hold(
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
        // Viewpoint properties read for the trigger house (appendix 7).
        let view = super::View::of_trigger_house(house);
        let house = house.map_or(0, |index| house_ref(index as usize));
        self.with_module_host(rules, source, |host| {
            let reader = host.reader();
            clauses.iter().all(|clause| {
                let reading = match clause.subject {
                    FilterSubject::Subject(prop) => {
                        reader.read_in(event.subject as i64, prop, view)
                    }
                    FilterSubject::Other(prop) => reader.read_in(event.other as i64, prop, view),
                    FilterSubject::House(prop) => reader.read_in(house, prop, view),
                    FilterSubject::Var { var, target } => {
                        let scope = match target {
                            VarTarget::Global => 0,
                            VarTarget::Subject => event.subject as i64,
                            VarTarget::Other => event.other as i64,
                            VarTarget::House => house,
                        };
                        reader.read_var(scope, var)
                    }
                };
                matches!(reading, Ok((value, Status::Ok)) if clause.holds(value))
            })
        })
    }

    /// Runs a firing trigger's actions in order: module actions queue a
    /// record for the module phase, the others run now through
    /// [`run_action`] with the trigger house (RFC section 5, "Action
    /// timing").
    fn fire_module_trigger(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        handle: u64,
        holder: u64,
        event: &ModuleEvent,
        house: Option<u32>,
    ) {
        let Some(source) = self
            .trigger_runtime
            .module_triggers()
            .get(handle)
            .map(|trigger| trigger.source)
        else {
            return;
        };
        let record = self.module_event_record(rules, source, handle, holder, event, None);
        // Past the chain caps the firing waits, with this record, for the
        // next module phase (RFC section 5, "Chains").
        let frame = self.session.binary_frame();
        if !self.ext_modules.chain.admit(frame) {
            self.defer_module_firing(handle, holder, record);
            return;
        }
        let record_house = event.house.or(house);
        self.run_module_trigger_actions(rules, frame_effects, handle, record, house, record_house);
    }

    /// A firing trigger's actions, in order, on `record`: the trigger house
    /// is `house`, the record carries `record_house`.
    pub(super) fn run_module_trigger_actions(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        handle: u64,
        record: EventRecord,
        house: Option<u32>,
        record_house: Option<u32>,
    ) {
        let Some(trigger) = self.trigger_runtime.module_triggers().get(handle) else {
            return;
        };
        let (source, actions) = (trigger.source, trigger.actions.clone());
        let trigger_house =
            house.and_then(|index| self.houses.order().get(index as usize).copied());
        let record = EventRecord {
            house: record_house.map_or(0, |index| house_ref(index as usize)),
            ..record
        };
        for action in actions {
            if action.kind == ACTION_MODULE {
                self.trigger_runtime.module_triggers_mut().enqueue(
                    source,
                    EventRecord {
                        key: action.params.id,
                        args: action.params.value as i32,
                        target: action.target as i32,
                        ..record
                    },
                );
                continue;
            }
            // In a trigger, house 0 is the trigger house and the event
            // supplies the other object (appendix 5.1).
            let mut params = action.params;
            params.other = record.other;
            let target = match action.target {
                ActionTarget::Subject => record.subject,
                ActionTarget::Other => record.other,
                ActionTarget::TagHolder => record.holder,
                ActionTarget::Projectile => record.projectile,
                // House 0 is the trigger house (RFC 6b), for the actions
                // that write any reference (`SetProperty`, `Modify`) too.
                ActionTarget::House if params.house == 0 => record.house,
                ActionTarget::House => params.house,
                ActionTarget::Cell => params.cell,
            };
            let context = ActionContext {
                rules,
                overlay_registry: Some(rules.overlay_registry()),
                frame_effects: &mut *frame_effects,
                text: &action.text,
                trigger_house,
                source,
            };
            self.ext_modules.chain.depth += 1;
            let result = run_action(self, action.kind, target, &params, context);
            self.ext_modules.chain.depth -= 1;
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

    /// The `EventRecord` of `event` for trigger `handle` on `holder`, with
    /// key 0 and the house at array index `house` (0 for none): what a
    /// firing queues for `on_events`, and what a decision request carries.
    pub(super) fn module_event_record(
        &mut self,
        rules: &RuleSet,
        source: ModuleSource,
        handle: u64,
        holder: u64,
        event: &ModuleEvent,
        house: Option<u32>,
    ) -> EventRecord {
        let house = house.map_or(0, |index| house_ref(index as usize));
        let [x, y, z] = if let Some(at) = event.at {
            at
        } else if event.at_subject {
            self.with_module_host(rules, source, |host| {
                ["X", "Y", "Z"].map(|name| {
                    match property_id(name)
                        .map(|prop| host.reader().read(event.subject as i64, prop))
                    {
                        Some(Ok((value, Status::Ok))) => i32::try_from(value).unwrap_or(-1),
                        _ => -1,
                    }
                })
            })
        } else {
            [-1; 3]
        };
        EventRecord {
            key: 0,
            kind: event.kind,
            trigger: handle as i64,
            holder: holder as i64,
            subject: event.subject as i64,
            other: event.other as i64,
            house,
            projectile: event.projectile as i64,
            modifier: event.modifier as i64,
            cell: event.cell.map_or(0, |(x, y)| cell_ref(x, y)),
            value: event.value,
            value2: event.value2,
            id: event.id,
            frame: self.session.binary_frame() as i32,
            x,
            y,
            z,
            args: 0,
            target: 0,
        }
    }

    /// The module phase's delivery for module `index`: its queued records go
    /// to `on_events` in batches of what the delivery area holds, 16 at most,
    /// each with a whole budget. The first batch always starts unless an
    /// `on_message` call already did this frame (`messaged`, RFC section 5:
    /// the first of either), so `ProductionAllowed` spending the frame's
    /// budget cannot starve delivery; later batches start while the frame's
    /// use is below the budget, up to [`MAX_RECORDS_PER_FRAME`] records.
    /// The rest wait, in order, for the next frame. Records of a disabled or removed trigger,
    /// and every record once the game has ended, are dropped. The module
    /// stops when producers hit the queue cap or delivery leaves too much backlog.
    pub(super) fn deliver_ext_module_events(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
        index: usize,
        messaged: bool,
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
        if triggers.queue_overflowed(source) {
            return Err(Fault::Backlog(MAX_QUEUED_RECORDS + 1));
        }
        let ready = triggers.prepare_delivery(source).min(MAX_RECORDS_PER_FRAME);
        let frame = self.session.binary_frame();
        let mut delivered = 0;
        while delivered < ready {
            let slot = &mut self.ext_modules.slots[index];
            if !slot.may_start_call(frame) && (delivered > 0 || messaged) {
                break;
            }
            let budget = slot.frame_budget;
            let batch = self
                .trigger_runtime
                .module_triggers_mut()
                .take(source, capacity.min(ready - delivered));
            if batch.is_empty() {
                break;
            }
            delivered += batch.len();
            let used = self.with_ext_module(
                rules,
                overlay_registry,
                frame_effects,
                index,
                |module: &mut SimModule, host| module.deliver(&batch, budget, host),
            )?;
            self.ext_modules.slots[index].charge_frame_ops(frame, used);
        }
        let triggers = self.trigger_runtime.module_triggers();
        if triggers.queue_overflowed(source) {
            return Err(Fault::Backlog(MAX_QUEUED_RECORDS + 1));
        }
        let backlog = triggers.queued(source);
        if backlog > MAX_BACKLOG_RECORDS {
            return Err(Fault::Backlog(backlog));
        }
        Ok(())
    }
}

/// Whether a `Timer` event's period ends on `frame`.
pub(super) fn timer_due(event: &EventDef, created: u32, frame: u32) -> bool {
    let interval = u32::try_from(event.interval).unwrap_or(0);
    interval > 0 && frame > created && (frame - created) % interval == 0
}
