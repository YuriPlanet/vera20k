//! The event kinds module triggers take beyond `TakeDamage`, `Killed`,
//! `Fire` and `Timer` (RFC 0001 appendix 4): what `trigger_create` accepts
//! of each, where each is raised and what its record holds.
//!
//! Stock events 0..61 (appendix 4.1), read through the trigger runtime's
//! own predicates (`trigger_runtime::module_events`):
//! - object events 1, 4, 6, 7, 29, 34, 35, 38..44, 48, 49, raised for bound
//!   objects at the object Tag funnel (`dispatch_object_tag_input`, the
//!   Tag6E53A0 ingress every native site uses); `other` is the raise's
//!   source or acting object;
//! - cell events 25 and 26, for cells bound with a cell reference, from the
//!   foot's per-cell arrival (Foot4D8A70..4D8B66): a bound cell on the
//!   arrival cell's row (25) or column (26);
//! - House events 5, 8..12, 15..22, 30, 32, 52, 55..58, polled every eighth
//!   frame, and logic events 13, 14, 27, 28, 36, 37, 45..47, 60, 61, every
//!   frame, at the start of the module phase.
//!
//! Stock events module triggers do not take (`Unsupported`):
//! - 0 is private and never true;
//! - 2 (spied) and 3 (thieved): no engine site raises them;
//! - 23 (team leaves map) needs a TeamType argument module IDs do not
//!   name yet; 24, 53, 54 and 59 reach only map CellTags and the Map tag
//!   list, whose binding a module trigger has no form for;
//! - 31 (bridge destroyed): its raise knows the cell's native identity, not
//!   its coordinates;
//! - 33 (selected) depends on one player's selection and is off online;
//! - 50 (any crate) reads the trigger runtime's dirty byte, cleared before
//!   the module phase;
//! - 51 (random delay) draws the native random stream for each trigger.
//!
//! New delivered events (appendix 4.3):
//! - `Birth` after a produced object's RecordLastBuilt (`0x004FB6B0`), with
//!   the factory or yard as `other` where the site knows it;
//! - `TransportEntered` (passenger) and `PassengerAdded` (transport) at the
//!   end of AddPassenger (`0x00710670`), `value` the passenger's size;
//! - `HouseTransferred` at the end of ChangeOwner (`0x007014A0`), `house`
//!   the old house;
//! - `SuperWeaponLaunched` after SuperClass::Launch (`0x006CC390`).
//!
//! Polling events (appendix 4.4): `PropertyPoll` and `VariablePoll`, checked
//! at the start of the module phase every `interval` frames counted from the
//! trigger's creation, or from the latest change of a `VariablePoll`'s
//! variable; an object on frames where that count matches its creation
//! serial modulo `interval`. Each fires when its condition becomes true and
//! only again after a check finds it false; the record's `value` is the
//! reading.
//!
//! Residuals:
//! - `HouseTransferred` carries `value` 0 also for a mind control: ChangeOwner
//!   is not told why it runs (its 42 callers would have to say), and the
//!   controller link is written after it returns.
//! - `Birth` is raised for produced objects only, not for reinforcements,
//!   spawned aircraft or crate units; `value` (production time) is −1.
//! - Garrisons raise no `TransportEntered`/`PassengerAdded`.
//! - Stock 19..22 read `value` as the class type index; the `id` form is
//!   not read.
//! - Polls on type-bound triggers scan every object each frame for that
//!   type (RFC 15.1 item 1 wants a side column).
//! - The other 4.3 and 4.4 events answer `Unsupported`.

use std::collections::BTreeSet;

use vera20k_wasm_ext::triggers::{
    EVENT_BIRTH, EVENT_FIRE, EVENT_HOUSE_TRANSFERRED, EVENT_KILLED, EVENT_PASSENGER_ADDED,
    EVENT_PROPERTY_POLL, EVENT_SUPER_WEAPON_LAUNCHED, EVENT_TAKE_DAMAGE, EVENT_TIMER,
    EVENT_TRANSPORT_ENTERED, EVENT_VARIABLE_POLL, EventDef,
};
use vera20k_wasm_ext::{Invalid, Status, VarScope, property_name};

use super::triggers::variable_scope;
use super::{Host, ModuleEvent, cell_ref, house_ref, techno_type};
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::trigger_runtime::{
    StockClass, StockReading, TriggerScope, stock_class, stock_poll_due, value_filters_actor_house,
    value_names_house,
};
use crate::sim::world::{FrameEffects, Simulation};

/// New delivered events this engine raises (appendix 4.3).
const DELIVERED: [i32; 9] = [
    EVENT_TAKE_DAMAGE,
    EVENT_KILLED,
    EVENT_FIRE,
    EVENT_TIMER,
    EVENT_BIRTH,
    EVENT_TRANSPORT_ENTERED,
    EVENT_PASSENGER_ADDED,
    EVENT_HOUSE_TRANSFERRED,
    EVENT_SUPER_WEAPON_LAUNCHED,
];

/// What `PropertyPoll` reads on a bound object (appendix 4.4); `Money` is
/// its one global form, on the trigger's house.
const OBJECT_POLL_PROPERTIES: [&str; 9] = [
    "Health",
    "HealthPercent",
    "Ammo",
    "PassengerCount",
    "TransportSpaceLeft",
    "Storage",
    "Mission",
    "Moving",
    "LandType",
];

/// Whether this engine raises event `kind`: `has_kind(0, kind)`.
pub(super) fn raises(kind: i32) -> bool {
    stock_class(kind).is_some()
        || DELIVERED.contains(&kind)
        || matches!(kind, EVENT_PROPERTY_POLL | EVENT_VARIABLE_POLL)
}

/// Events checked in the module phase rather than raised at a site.
fn is_polled(kind: i32) -> bool {
    matches!(kind, EVENT_PROPERTY_POLL | EVENT_VARIABLE_POLL)
        || matches!(
            stock_class(kind),
            Some(StockClass::House | StockClass::Logic)
        )
}

/// Whether a polling event's check falls on `frame`: every `interval` frames
/// from `start`, an object's at its creation serial's phase (RFC 6a).
fn poll_due(interval: i32, start: u32, frame: u32, holder: u64) -> bool {
    let Some(interval) = u32::try_from(interval).ok().filter(|i| *i > 0) else {
        return false;
    };
    let phase = (holder % u64::from(interval)) as u32;
    frame > start && (frame - start) % interval == phase
}

impl Host<'_, '_> {
    /// What `trigger_create` accepts of one event, and what it binds to:
    /// `Err(status)` for a kind this engine does not raise or a missing
    /// waypoint; invalid arguments are invalid.
    pub(super) fn event_scope(
        &self,
        def: &EventDef,
    ) -> Result<Result<TriggerScope, Status>, Invalid> {
        let object = TriggerScope::Object { weapon: false };
        let invalid = |why: String| Err(Invalid(format!("event {}: {why}", def.kind)));
        let polling = matches!(
            def.kind,
            EVENT_TIMER | EVENT_PROPERTY_POLL | EVENT_VARIABLE_POLL
        );
        if polling && def.interval < 1 {
            return invalid(format!("interval {}", def.interval));
        }
        let scope = match def.kind {
            EVENT_TIMER | EVENT_SUPER_WEAPON_LAUNCHED => TriggerScope::Global,
            // Decision-only (appendix 4.5): Defeat asks about a house and
            // needs no binding; ProductionAllowed and ProductionQueued bind
            // to the type, CommandAllowed to the commanded object or its
            // type.
            vera20k_wasm_ext::decide::EVENT_DEFEAT => TriggerScope::Global,
            vera20k_wasm_ext::decide::EVENT_PRODUCTION_ALLOWED
            | vera20k_wasm_ext::decide::EVENT_PRODUCTION_QUEUED
            | vera20k_wasm_ext::decide::EVENT_COMMAND_ALLOWED => object,
            EVENT_FIRE => TriggerScope::Object { weapon: true },
            EVENT_TAKE_DAMAGE
            | EVENT_KILLED
            | EVENT_BIRTH
            | EVENT_TRANSPORT_ENTERED
            | EVENT_PASSENGER_ADDED
            | EVENT_HOUSE_TRANSFERRED => object,
            // Appendix 4.4: other properties are refused at creation.
            EVENT_PROPERTY_POLL => match property_name(def.id) {
                Some("Money") => TriggerScope::Global,
                Some(name) if OBJECT_POLL_PROPERTIES.contains(&name) => object,
                _ => return invalid(format!("PropertyPoll of property {}", def.id)),
            },
            EVENT_VARIABLE_POLL => match variable_scope(&self.sim.ext_modules.vars, def.id)? {
                VarScope::Object => object,
                VarScope::Global | VarScope::House => TriggerScope::Global,
            },
            kind => {
                let Some(class) = stock_class(kind) else {
                    return Ok(Err(Status::Unsupported));
                };
                if let Some(status) = self.check_stock_arguments(def)? {
                    return Ok(Err(status));
                }
                match class {
                    StockClass::Object => object,
                    StockClass::Cell => TriggerScope::Cell,
                    StockClass::House | StockClass::Logic => TriggerScope::Global,
                }
            }
        };
        Ok(Ok(scope))
    }

    /// A stock event's `value` and `id` (appendix 4.1): house indices in
    /// range, variables in their tables, a known TechnoType, an existing
    /// waypoint (`NotFound` otherwise).
    fn check_stock_arguments(&self, def: &EventDef) -> Result<Option<Status>, Invalid> {
        let houses = self.sim.houses.order().len() as i64;
        let in_range = |range: std::ops::Range<i64>| match range.contains(&def.value) {
            true => Ok(None),
            false => Err(Invalid(format!(
                "event {} value {} outside {range:?}",
                def.kind, def.value
            ))),
        };
        match def.kind {
            kind if value_names_house(kind) => in_range(0..houses),
            kind if value_filters_actor_house(kind) && def.value != -1 => in_range(0..houses),
            27 | 28 => in_range(0..50),
            36 | 37 => in_range(0..100),
            60 | 61 => match techno_type(self.rules, def.id) {
                Some(_) => Ok(None),
                None => Err(Invalid(format!("event {} TechnoType {}", def.kind, def.id))),
            },
            34 => Ok(i32::try_from(def.value)
                .ok()
                .and_then(|waypoint| self.sim.trigger_waypoint_location(waypoint))
                .is_none()
                .then_some(Status::NotFound)),
            _ => Ok(None),
        }
    }
}

impl Simulation {
    /// Whether event `def` (index `bit` of its trigger) takes `event`, and
    /// the house its record carries instead of the trigger's: a stock
    /// event's predicate and retained house, a polling event's own check.
    pub(super) fn module_event_admits(
        &self,
        rules: &RuleSet,
        def: &EventDef,
        bit: usize,
        event: &ModuleEvent,
        house: Option<u32>,
        poll_start: u32,
    ) -> Option<Option<u32>> {
        match def.kind {
            EVENT_PROPERTY_POLL | EVENT_VARIABLE_POLL => (event.poll == Some(bit)).then_some(None),
            kind if stock_class(kind).is_some() => {
                let type_name = matches!(kind, 60 | 61)
                    .then(|| techno_type(rules, def.id).map(|object| object.id.as_str()))
                    .flatten();
                let reading = StockReading {
                    def,
                    subject: event.subject,
                    other: event.other,
                    trigger_house: house,
                    poll_start,
                    type_name,
                };
                let (holds, carried) = self.module_stock_event_holds(&reading, rules);
                holds.then_some(carried.or(event.house))
            }
            _ => Some(None),
        }
    }

    /// Stock object event `kind` raised on `subject` (appendix 4.1, the
    /// object Tag funnel). Call it only when the trigger runtime's
    /// `module_listens(kind)`.
    pub(crate) fn raise_module_stock_object_event(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        kind: i32,
        subject: u64,
        other: Option<u64>,
    ) {
        if stock_class(kind) != Some(StockClass::Object) {
            return;
        }
        let event = ModuleEvent {
            other: other.unwrap_or(0),
            ..ModuleEvent::new(kind, subject)
        };
        self.raise_module_event(rules, frame_effects, event);
    }

    /// Events 25 and 26 for `actor` arriving at `cell`: every trigger bound
    /// to a cell on that row (25) or column (26). The bound cell holds the
    /// instance.
    pub(crate) fn raise_module_line_events(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        actor: u64,
        cell: (i16, i16),
    ) {
        let (Ok(x), Ok(y)) = (u16::try_from(cell.0), u16::try_from(cell.1)) else {
            return;
        };
        for kind in [25, 26] {
            if !self.trigger_runtime.module_listens(kind) {
                continue;
            }
            let bound = self
                .trigger_runtime
                .module_triggers()
                .cell_bound(kind, |(bx, by)| if kind == 25 { by == y } else { bx == x });
            let event = ModuleEvent {
                other: actor,
                ..ModuleEvent::new(kind, 0)
            };
            for (handle, (bx, by)) in bound {
                let holder = cell_ref(bx, by) as u64;
                self.offer_module_event(rules, frame_effects, handle, holder, &event);
            }
        }
    }

    /// The polling events whose check falls on this frame, from the start
    /// of the module phase: stock House and logic events on their stock
    /// cadence, `PropertyPoll` and `VariablePoll` on their interval.
    pub(super) fn poll_module_events(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        let triggers = self.trigger_runtime.module_triggers();
        if !triggers.kinds().iter().any(|&kind| is_polled(kind)) {
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
            let (global, poll_start, source) =
                (trigger.is_global(), trigger.poll_start, trigger.source);
            let fixed_house = trigger.house;
            let polls: Vec<(usize, EventDef)> = trigger
                .events
                .iter()
                .enumerate()
                .filter(|(_, def)| is_polled(def.kind))
                .map(|(bit, def)| (bit, def.clone()))
                .collect();
            let mut offered = Vec::new();
            for (bit, def) in polls {
                if stock_class(def.kind).is_some() {
                    if stock_poll_due(def.kind, frame) && !offered.contains(&def.kind) {
                        offered.push(def.kind);
                        let event = ModuleEvent::new(def.kind, 0);
                        self.offer_module_event(rules, frame_effects, handle, 0, &event);
                    }
                    continue;
                }
                let holders = if global {
                    vec![0]
                } else {
                    self.module_poll_holders(rules, handle)
                };
                let house = fixed_house.or_else(|| self.module_trigger_house(rules, 0));
                for holder in holders {
                    if !poll_due(def.interval, poll_start, frame, holder) {
                        continue;
                    }
                    let reading = self.module_poll_reading(rules, source, &def, holder, house);
                    let holds = reading.is_some_and(|reading| def.compares(reading));
                    if !self
                        .trigger_runtime
                        .module_triggers_mut()
                        .poll_edge(handle, holder, bit, holds)
                    {
                        continue;
                    }
                    let event = ModuleEvent {
                        value: reading.unwrap_or(-1),
                        poll: Some(bit),
                        ..ModuleEvent::new(def.kind, holder)
                    };
                    self.offer_module_event(rules, frame_effects, handle, holder, &event);
                }
            }
        }
    }

    /// The live objects an object poll checks: those bound to `handle`, and
    /// every object of a type bound to it, in creation order.
    fn module_poll_holders(&self, rules: &RuleSet, handle: u64) -> Vec<u64> {
        let triggers = self.trigger_runtime.module_triggers();
        let mut holders: BTreeSet<u64> = triggers.bound_objects(handle).into_iter().collect();
        let types = triggers.bound_types(handle);
        if !types.is_empty() {
            holders.extend(
                self.entities()
                    .iter_sorted()
                    .filter(|(_, entity)| {
                        self.entity_object_type(entity, rules)
                            .and_then(|object| rules.type_handle(&object.id))
                            .is_some_and(|handle| types.contains(&handle.0))
                    })
                    .map(|(id, _)| id),
            );
        }
        holders
            .into_iter()
            .filter(|id| self.entities().get(*id).is_some())
            .collect()
    }

    /// A polling event's reading, as the trigger's module would read it
    /// with `get` or `var_get`; `None` when the read is not `Ok`.
    fn module_poll_reading(
        &mut self,
        rules: &RuleSet,
        source: super::ModuleSource,
        def: &EventDef,
        holder: u64,
        house: Option<u32>,
    ) -> Option<i64> {
        let house = house.map_or(0, |index| house_ref(index as usize));
        self.with_module_host(rules, source, |host| {
            let read = if def.kind == EVENT_PROPERTY_POLL {
                host.read(if holder == 0 { house } else { holder as i64 }, def.id)
            } else {
                let scope = match variable_scope(&host.sim.ext_modules.vars, def.id) {
                    Ok(VarScope::Object) => holder as i64,
                    Ok(VarScope::House) => house,
                    _ => 0,
                };
                host.read_var(scope, def.id)
            };
            match read {
                Ok((value, Status::Ok)) => Some(value),
                _ => None,
            }
        })
    }

    /// `Birth` for `product`, after its RecordLastBuilt; `producer` is the
    /// factory or yard where the site knows it.
    pub(crate) fn raise_module_birth(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        product: u64,
        producer: Option<u64>,
    ) {
        if !self.trigger_runtime.module_listens(EVENT_BIRTH) {
            return;
        }
        let event = ModuleEvent {
            other: producer.unwrap_or(0),
            ..ModuleEvent::new(EVENT_BIRTH, product)
        };
        self.raise_module_event(rules, frame_effects, event);
    }

    /// `TransportEntered` on each passenger and `PassengerAdded` on
    /// `carrier`, after AddPassenger took them; `value` is the size each
    /// occupies, `value2` 0 (a transport, not a garrison).
    pub(crate) fn raise_module_passengers_added(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        carrier: u64,
        passengers: &[(u64, f64)],
    ) {
        for &(passenger, size) in passengers {
            let space = size as i64;
            for (kind, subject, other) in [
                (EVENT_TRANSPORT_ENTERED, passenger, carrier),
                (EVENT_PASSENGER_ADDED, carrier, passenger),
            ] {
                if self.trigger_runtime.module_listens(kind) {
                    let event = ModuleEvent {
                        other,
                        value: space,
                        value2: 0,
                        ..ModuleEvent::new(kind, subject)
                    };
                    self.raise_module_event(rules, frame_effects, event);
                }
            }
        }
    }

    /// `HouseTransferred` on `object` after ChangeOwner moved it from
    /// `old_owner`.
    pub(crate) fn raise_module_house_transferred(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        object: u64,
        old_owner: InternedId,
    ) {
        if !self.trigger_runtime.module_listens(EVENT_HOUSE_TRANSFERRED) {
            return;
        }
        let event = ModuleEvent {
            house: self.houses.index_of(old_owner).map(|index| index as u32),
            value: 0,
            ..ModuleEvent::new(EVENT_HOUSE_TRANSFERRED, object)
        };
        self.raise_module_event(rules, frame_effects, event);
    }

    /// `SuperWeaponLaunched` (global) after `owner`'s Super of type
    /// `sw_type` launched at `cell`.
    pub(crate) fn raise_module_super_weapon_launched(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        owner: InternedId,
        sw_type: InternedId,
        cell: (u16, u16),
    ) {
        if !self
            .trigger_runtime
            .module_listens(EVENT_SUPER_WEAPON_LAUNCHED)
        {
            return;
        }
        let id = rules
            .super_weapon_index(self.interner.resolve(sw_type))
            .and_then(|index| i32::try_from(index).ok())
            .unwrap_or(-1);
        let event = ModuleEvent {
            id,
            cell: Some(cell),
            house: self.houses.index_of(owner).map(|index| index as u32),
            ..ModuleEvent::new(EVENT_SUPER_WEAPON_LAUNCHED, 0)
        };
        for handle in self
            .trigger_runtime
            .module_triggers()
            .global(EVENT_SUPER_WEAPON_LAUNCHED)
        {
            self.offer_module_event(rules, frame_effects, handle, 0, &event);
        }
    }
}
