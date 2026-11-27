//! Runtime triggers (RFC 0001 sections 6a and 7, appendix section 3):
//! [`Trigger`] builds a `TriggerDef` and calls `trigger_create` and
//! `trigger_bind`, [`ActionDef`] builds trigger actions, and the event views
//! read `EventRecord`s in place inside `on_events` ([`crate::on_events!`]).
//!
//! ```ignore
//! use vera20k_sdk::prelude::*;
//!
//! vera20k_sdk::named_actions! { ON_HIT = "OnHit" => 1 }
//!
//! fn start() {
//!     let _ = Trigger::on(Event::TAKE_DAMAGE)
//!         .bind(some_unit)
//!         .immediate(ActionDef::set_var(Var::local(3), 1))
//!         .enqueue(ON_HIT)
//!         .done();
//! }
//!
//! fn on_hit(e: TakeDamageEvent) {
//!     let _ = e.damage();
//! }
//!
//! vera20k_sdk::on_scenario_start!(start);
//! vera20k_sdk::on_events!(ON_HIT => on_hit);
//! ```

use crate::{
    Action, Cell, House, Modifier, ModuleEvent, NO_HANDLE, Op, Prop, Ref, Result, Techno, Var,
    check, sys,
};

/// Event kinds (appendix 4). Stock events keep 0..61 (4.1); new delivered
/// events are numbered from 1000 in appendix 4.3 order and the polling
/// events of 4.4 follow them; provisional (RFC open question 5). The engine
/// answers `has_kind(0, kind)` for the kinds it raises.
pub struct Event;

impl Event {
    // Stock events (appendix 4.1). `value` names a house by array index.
    pub const ENTERED_BY: i32 = 1;
    pub const SPIED_BY: i32 = 2;
    pub const THIEVED_BY: i32 = 3;
    pub const DISCOVERED_BY_PLAYER: i32 = 4;
    pub const HOUSE_DISCOVERED: i32 = 5;
    pub const ATTACKED_BY_ANY_HOUSE: i32 = 6;
    pub const DESTROYED_BY_ANY_HOUSE: i32 = 7;
    pub const ANY_EVENT: i32 = 8;
    pub const ALL_UNITS_DESTROYED: i32 = 9;
    pub const ALL_BUILDINGS_DESTROYED: i32 = 10;
    pub const ALL_DESTROYED: i32 = 11;
    pub const CREDITS_EXCEED: i32 = 12;
    pub const ELAPSED_TIME: i32 = 13;
    pub const MISSION_TIMER_EXPIRED: i32 = 14;
    pub const BUILDINGS_DESTROYED_COUNT: i32 = 15;
    pub const UNITS_DESTROYED_COUNT: i32 = 16;
    pub const NO_FACTORIES_LEFT: i32 = 17;
    pub const CIVILIANS_EVACUATED: i32 = 18;
    pub const BUILD_BUILDING_TYPE: i32 = 19;
    pub const BUILD_UNIT_TYPE: i32 = 20;
    pub const BUILD_INFANTRY_TYPE: i32 = 21;
    pub const BUILD_AIRCRAFT_TYPE: i32 = 22;
    pub const TEAM_LEAVES_MAP: i32 = 23;
    pub const ZONE_ENTRY_BY: i32 = 24;
    pub const CROSSES_HORIZONTAL_LINE: i32 = 25;
    pub const CROSSES_VERTICAL_LINE: i32 = 26;
    pub const GLOBAL_IS_SET: i32 = 27;
    pub const GLOBAL_IS_CLEAR: i32 = 28;
    pub const DESTROYED_BY_ANYTHING_NOT_INFILTRATE: i32 = 29;
    pub const LOW_POWER: i32 = 30;
    pub const BRIDGE_DESTROYED: i32 = 31;
    pub const BUILDING_EXISTS: i32 = 32;
    pub const SELECTED_BY_PLAYER: i32 = 33;
    pub const COMES_NEAR_WAYPOINT: i32 = 34;
    pub const ENEMY_IN_SPOTLIGHT: i32 = 35;
    pub const LOCAL_IS_SET: i32 = 36;
    pub const LOCAL_IS_CLEAR: i32 = 37;
    pub const FIRST_DAMAGED_COMBAT: i32 = 38;
    pub const HALF_HEALTH_COMBAT: i32 = 39;
    pub const QUARTER_HEALTH_COMBAT: i32 = 40;
    pub const FIRST_DAMAGED_ANY: i32 = 41;
    pub const HALF_HEALTH_ANY: i32 = 42;
    pub const QUARTER_HEALTH_ANY: i32 = 43;
    pub const ATTACKED_BY_HOUSE: i32 = 44;
    pub const AMBIENT_LIGHT_BELOW: i32 = 45;
    pub const AMBIENT_LIGHT_ABOVE: i32 = 46;
    pub const ELAPSED_SCENARIO_TIME: i32 = 47;
    pub const DESTROYED_BY_ANYTHING: i32 = 48;
    pub const PICKUP_CRATE: i32 = 49;
    pub const PICKUP_CRATE_ANY: i32 = 50;
    pub const RANDOM_DELAY: i32 = 51;
    pub const CREDITS_BELOW: i32 = 52;
    pub const SPY_AS_HOUSE: i32 = 53;
    pub const SPY_AS_INFANTRY: i32 = 54;
    pub const NAVAL_UNITS_DESTROYED: i32 = 55;
    pub const LAND_UNITS_DESTROYED: i32 = 56;
    pub const BUILDING_DOES_NOT_EXIST: i32 = 57;
    pub const POWER_FULL: i32 = 58;
    pub const ENTERED_OR_OVERFLOWN_BY: i32 = 59;
    pub const TECH_TYPE_EXISTS: i32 = 60;
    pub const TECH_TYPE_DOES_NOT_EXIST: i32 = 61;

    // Ares events (appendix 4.2), only when Ares plays (`has_kind` answers
    // 0 otherwise). `value` names a house by array index; `id` names the
    // TechnoType (the SuperWeaponType for 77).
    pub const UNDER_EMP: i32 = 62;
    pub const UNDER_EMP_BY_HOUSE: i32 = 63;
    pub const REMOVED_FROM_EMP: i32 = 64;
    pub const REMOVED_FROM_EMP_BY_HOUSE: i32 = 65;
    pub const IN_SPOTLIGHT: i32 = 66;
    pub const DRIVER_KILLED: i32 = 67;
    pub const DRIVER_KILLED_BY_HOUSE: i32 = 68;
    pub const VEHICLE_TAKEN: i32 = 69;
    pub const VEHICLE_TAKEN_BY_HOUSE: i32 = 70;
    pub const ABDUCTED: i32 = 71;
    pub const ABDUCTED_BY_HOUSE: i32 = 72;
    pub const ABDUCTS_SOMETHING: i32 = 73;
    pub const ABDUCTS_SOMETHING_OF_HOUSE: i32 = 74;
    pub const SUPER_WEAPON_ACTIVATED: i32 = 75;
    pub const SUPER_WEAPON_DEACTIVATED: i32 = 76;
    pub const SUPER_WEAPON_NEAR_WAYPOINT: i32 = 77;
    pub const HOUSE_REVERSE_ENGINEERED_TYPE: i32 = 78;
    pub const SOMETHING_REVERSE_ENGINEERED: i32 = 79;
    pub const TYPE_REVERSE_ENGINEERED: i32 = 80;
    pub const HOUSE_OWNS_AT_LEAST: i32 = 81;
    pub const HOUSE_OWNS_AT_MOST: i32 = 82;
    pub const ATTACKED_OR_DESTROYED_BY_ANYBODY: i32 = 83;
    pub const ATTACKED_OR_DESTROYED_BY_HOUSE: i32 = 84;
    pub const DESTROYED_BY_HOUSE: i32 = 85;
    pub const TYPE_COUNT_AT_MOST: i32 = 86;
    pub const HOUSE_NO_KEEP_ALIVE_OBJECTS: i32 = 87;
    pub const HOUSE_NO_KEEP_ALIVE_BUILDINGS: i32 = 88;

    // New delivered events (appendix 4.3).
    pub const TAKE_DAMAGE: i32 = 1000;
    pub const HIT_BY_WARHEAD: i32 = 1001;
    pub const ATTACKED_BY: i32 = 1002;
    pub const FATAL_DAMAGE: i32 = 1003;
    pub const KILLED: i32 = 1004;
    pub const FIRE: i32 = 1005;
    pub const SPAWN_FIRE: i32 = 1006;
    pub const HIT_TARGET: i32 = 1007;
    pub const TARGET_CHANGED: i32 = 1008;
    pub const PROJECTILE_INCOMING: i32 = 1009;
    pub const DETONATED: i32 = 1010;
    pub const BIRTH: i32 = 1011;
    pub const TRANSPORT_ENTERED: i32 = 1012;
    pub const TRANSPORT_EXITED: i32 = 1013;
    pub const PASSENGER_ADDED: i32 = 1014;
    pub const PASSENGER_REMOVED: i32 = 1015;
    pub const COMMANDED: i32 = 1016;
    pub const CRUSHED: i32 = 1017;
    pub const HOUSE_TRANSFER: i32 = 1018;
    pub const HOUSE_TRANSFERRED: i32 = 1019;
    pub const MODIFIER_START: i32 = 1020;
    pub const MODIFIER_EXPIRE: i32 = 1021;
    pub const MODIFIER_STACKS: i32 = 1022;
    pub const TIMER: i32 = 1023;
    pub const SUPER_WEAPON_LAUNCHED: i32 = 1024;
    pub const PRODUCTION_CANCELLED: i32 = 1025;

    // New polling events (appendix 4.4).
    pub const PROPERTY_POLL: i32 = 1026;
    pub const HOUSE_POWER_POLL: i32 = 1027;
    pub const MODIFIER_COUNT_POLL: i32 = 1028;
    pub const DISTANCE_POLL: i32 = 1029;
    pub const VARIABLE_POLL: i32 = 1030;

    // Decision-only events (appendix 4.5).
    /// Decision-only (appendix 4.5): needs [`Trigger::decide_sync`] and a
    /// type binding. Asked once per frame for each house and bound type.
    pub const PRODUCTION_ALLOWED: i32 = 1031;
    /// Decision-only (appendix 4.5): needs [`Trigger::decide_sync`].
    pub const PRODUCTION_QUEUED: i32 = 1032;
    /// Decision-only (appendix 4.5): needs [`Trigger::decide_sync`].
    pub const DEFEAT: i32 = 1033;
    /// Decision-only (appendix 4.5): needs [`Trigger::decide_sync`] and an
    /// object or type binding. Asked for each command on a bound object.
    pub const COMMAND_ALLOWED: i32 = 1034;
    /// Decision-only, preview (appendix 4.5): the incoming damage at the
    /// receiver's entry ([`crate::DamageIncomingDecision`]). The event's
    /// `id` names the armor for `value2` (-1 the subject's); bit 0 of its
    /// `value` applies the subject's armor divides first.
    pub const DAMAGE_INCOMING: i32 = 1039;

    // RFC 0001 0.0.22 (appendix 4.3 and 4.5): unit control points. The
    // decision-only ones need [`Trigger::decide_sync`] and an object or type
    // binding.
    /// Decision-only: veto a shot ([`crate::FireAllowedDecision`]).
    pub const FIRE_ALLOWED: i32 = 1044;
    /// Decision-only: retaliate against a hit's source
    /// ([`crate::RetaliateDecision`]).
    pub const RETALIATE: i32 = 1045;
    /// Decision-only: the destination a move takes
    /// ([`crate::DestinationSetDecision`]).
    pub const DESTINATION_SET: i32 = 1046;
    /// Decision-only: veto a crush, on the victim
    /// ([`crate::CrushAllowedDecision`]).
    pub const CRUSH_ALLOWED: i32 = 1047;
    /// Delivered: the current mission changed (`value` the
    /// [`crate::Mission`]). Not raised by this engine yet: `has_kind` answers
    /// 0.
    pub const MISSION_CHANGED: i32 = 1048;
    /// Decision-only: the cell a miner harvests
    /// ([`crate::HarvestCellDecision`]).
    pub const HARVEST_CELL: i32 = 1049;
    /// Decision-only: veto a deploy ([`crate::DeployAllowedDecision`]).
    pub const DEPLOY_ALLOWED: i32 = 1050;
    /// Delivered: deployed or undeployed ([`crate::DeployedEvent`]).
    pub const DEPLOYED: i32 = 1051;
    /// Delivered: promoted ([`crate::PromotedEvent`]).
    pub const PROMOTED: i32 = 1052;
    /// Delivered: a kill's experience ([`crate::ExperienceGainedEvent`]).
    pub const EXPERIENCE_GAINED: i32 = 1053;
    /// Delivered: wholly cloaked ([`crate::CloakEvent`]).
    pub const CLOAKED: i32 = 1054;
    /// Delivered: wholly visible again ([`crate::CloakEvent`]).
    pub const DECLOAKED: i32 = 1055;

    /// Decision-only (appendix 4.5, preview): needs [`Trigger::decide_sync`]
    /// and a weapon, warhead or projectile type binding (or the launcher's
    /// object or type). Asked once per launched bullet: answer with
    /// [`crate::LaunchDecision::set_path`].
    pub const LAUNCH: i32 = 1035;
    /// Decision-only (appendix 4.5, preview): needs [`Trigger::decide_sync`]
    /// and a projectile type binding. Asked every frame a bound bullet
    /// moves: answer with [`crate::ProjectileFrameDecision::set_step`].
    pub const PROJECTILE_FRAME: i32 = 1036;
    /// Decision-only (appendix 4.5, preview): needs [`Trigger::decide_sync`]
    /// and an object or type binding. Answer the mission the object idles
    /// into ([`crate::Mission`], 255 for none).
    pub const IDLE_MISSION: i32 = 1060;
    /// Decision-only (appendix 4.5, preview): needs [`Trigger::decide_sync`]
    /// and an object or type binding. Asked after each Guard, Area Guard,
    /// Hunt or Attack mission body: replace its target, destination, queued
    /// mission or delay ([`crate::MissionStepDecision`]).
    pub const MISSION_STEP: i32 = 1062;
    /// Decision-only (appendix 4.5, preview): needs [`Trigger::decide_sync`]
    /// and an object or type binding. Asked each frame a bound vehicle or
    /// infantryman (Drive, Ship, Walk, Mech, Hover) could start a step:
    /// answer with [`crate::MoveFrameDecision::set_step`].
    pub const MOVE_FRAME: i32 = 1063;
    /// Decision-only (appendix 4.5, preview): needs [`Trigger::decide_sync`]
    /// and an object or type binding. Asked when a path search's route is
    /// adopted: answer with [`crate::PathFoundDecision::set_cells`] or
    /// [`crate::PathFoundDecision::no_route`].
    pub const PATH_FOUND: i32 = 1064;
    /// Decision-only (appendix 4.5, preview): needs [`Trigger::decide_sync`]
    /// and an object or type binding. Asked when a unit approaching its
    /// target has chosen the cell to move to: answer another cell
    /// ([`crate::ApproachPointDecision::set_cell`]).
    pub const APPROACH_POINT: i32 = 1065;

    // Module INI (RFC 0001 section 6h).
    /// A module event ([`crate::ini::event`]): the trigger's `id` is the
    /// event ID; [`Trigger::on_module_event`] sets both.
    pub const MODULE_EVENT: i32 = 1040;
    /// Delivery only, never in a trigger: one entry of a registry
    /// ([`crate::IniEntryRecord`]).
    pub const INI_ENTRY: i32 = 1041;
    /// Delivery only, never in a trigger: one registered key
    /// ([`crate::IniKeyRecord`]).
    pub const INI_KEY: i32 = 1042;

    // RFC 0001 section 6i (appendix 4.3).
    /// Global: an order ([`crate::House::order`]) or player command the
    /// execution step did not carry out, once per unit, or once with
    /// subject 0 for a house-level order. `house` is the issuing house,
    /// `value` the [`crate::Command`], `value2` the reason (appendix 8.3
    /// `RefuseReason`), `other` and `cell` the target, `id` the type of a
    /// house-level order (otherwise -1).
    pub const ORDER_REFUSED: i32 = 1043;
}

/// What a trigger action acts on (`ActionTarget`, appendix 5.1).
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Subject = 0,
    Other = 1,
    TagHolder = 2,
    Projectile = 3,
    House = 4,
    Cell = 5,
}

impl Target {
    /// The target numbered `raw`, or `None` for another number.
    pub const fn from_raw(raw: i32) -> Option<Self> {
        Some(match raw {
            0 => Self::Subject,
            1 => Self::Other,
            2 => Self::TagHolder,
            3 => Self::Projectile,
            4 => Self::House,
            5 => Self::Cell,
            _ => return None,
        })
    }
}

/// A trigger action (`ActionDef` with its `ActionParams`), only constructed
/// here; [`Trigger::immediate`] adds it.
#[derive(Clone, Copy, Debug)]
pub struct ActionDef(pub sys::ActionDef);

impl ActionDef {
    /// Any action `kind` on `target` with `params`.
    pub const fn new(kind: i32, target: Target, params: sys::ActionParams) -> Self {
        Self(sys::ActionDef {
            kind,
            target: target as i32,
            params,
        })
    }

    /// Gives the trigger's house `amount` credits.
    pub const fn add_money(amount: i32) -> Self {
        Self::add_money_to(House(NO_HANDLE), amount)
    }

    /// Gives `house` `amount` credits.
    pub const fn add_money_to(house: House, amount: i32) -> Self {
        Self::new(
            Action::ADD_MONEY,
            Target::House,
            sys::ActionParams {
                value: amount as i64,
                house: house.0,
                ..PARAMS
            },
        )
    }

    /// Writes `value` to variable `var` (a Red Alert 2 variable ignores the
    /// target).
    pub const fn set_var(var: Var, value: i64) -> Self {
        Self::new(
            Action::SET_VAR,
            Target::Subject,
            sys::ActionParams {
                id: var.0,
                value,
                ..PARAMS
            },
        )
    }

    /// Queues a record with dispatch key `key` for `on_events`.
    pub const fn module(key: i32) -> Self {
        Self::new(
            Action::MODULE,
            Target::Subject,
            sys::ActionParams { id: key, ..PARAMS },
        )
    }

    /// Queues a record with dispatch key `key` whose `target` is `target`
    /// and whose `args` is `text` ([`EventRecord::args_text`]), as INI's
    /// `Wasm:name(Target,text)`; `trigger_create` copies the text.
    pub fn module_with(key: i32, target: Target, text: &'static str) -> Self {
        Self::new(
            Action::MODULE,
            target,
            sys::ActionParams {
                id: key,
                text: text.as_ptr() as i32,
                text_len: text.len() as i32,
                ..PARAMS
            },
        )
    }

    /// Raises module event `event` on `target` (any target for a global
    /// event) with `value` and `value2`, as [`Action::raise_event`] does.
    pub const fn raise_event(target: Target, event: ModuleEvent, value: i64, value2: i64) -> Self {
        Self::new(
            Action::RAISE_EVENT,
            target,
            sys::ActionParams {
                id: event.0,
                value,
                value2,
                ..PARAMS
            },
        )
    }

    /// Writes `line` to the in-game log; `trigger_create` copies the text.
    pub fn log(line: &'static str) -> Self {
        Self::new(
            Action::LOG,
            Target::Subject,
            sys::ActionParams {
                text: line.as_ptr() as i32,
                text_len: line.len() as i32,
                ..PARAMS
            },
        )
    }

    /// Restores `amount` health to `target`.
    pub const fn heal(target: Target, amount: i32) -> Self {
        Self::new(
            Action::HEAL,
            target,
            sys::ActionParams {
                value: amount as i64,
                ..PARAMS
            },
        )
    }

    /// Writes `prop` of `target`, as `set` does.
    pub const fn set_property(target: Target, prop: Prop, value: i64) -> Self {
        Self::new(
            Action::SET_PROPERTY,
            target,
            sys::ActionParams {
                id: prop.0,
                value,
                ..PARAMS
            },
        )
    }
}

const PARAMS: sys::ActionParams = sys::ActionParams {
    id: 0,
    arg: 0,
    arg2: 0,
    reserved: 0,
    value: 0,
    value2: 0,
    cell: 0,
    house: 0,
    other: 0,
    text: 0,
    text_len: 0,
    extra: 0,
    extra_len: 0,
};

/// Most events, clauses per event, actions and bindings one [`Trigger`]
/// holds; more are dropped and make [`Trigger::done`] answer `Refused`.
pub const MAX_EVENTS: usize = 4;
pub const MAX_EVENT_CLAUSES: usize = 4;
pub const MAX_ACTIONS: usize = 8;
pub const MAX_BINDS: usize = 8;

const CLAUSE: sys::Clause = sys::Clause {
    subject: 0,
    op: 0,
    scope: 0,
    reserved: 0,
    prop: 0,
    value: 0,
};

#[derive(Clone, Copy)]
struct EventSpec {
    def: sys::EventDef,
    clauses: [sys::Clause; MAX_EVENT_CLAUSES],
}

/// A trigger handle.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TriggerRef(pub i64);

impl Ref for TriggerRef {
    fn raw(self) -> i64 {
        self.0
    }
}

impl TriggerRef {
    pub fn bind(self, reference: impl Ref) -> Result<()> {
        check(unsafe { sys::trigger_bind(self.0, reference.raw()) })
    }

    pub fn unbind(self, reference: impl Ref) -> Result<()> {
        check(unsafe { sys::trigger_unbind(self.0, reference.raw()) })
    }

    /// Removes the trigger and every instance of it.
    pub fn remove(self) -> Result<()> {
        check(unsafe { sys::trigger_remove(self.0) })
    }

    /// Writes the trigger's `Enabled` property.
    pub fn set_enabled(self, enabled: bool) -> Result<()> {
        crate::set(self, Prop::named("Enabled")?, i64::from(enabled))
    }
}

/// A trigger under construction (appendix section 3, "Triggers"). Nothing
/// reaches the engine before [`Trigger::done`].
pub struct Trigger {
    header: sys::TriggerDef,
    events: [EventSpec; MAX_EVENTS],
    actions: [sys::ActionDef; MAX_ACTIONS],
    binds: [i64; MAX_BINDS],
    binds_len: usize,
    overflow: bool,
}

impl Trigger {
    /// A repeating trigger on event `kind` for every difficulty.
    pub fn on(kind: i32) -> Self {
        let trigger = Self {
            header: sys::TriggerDef {
                repeat: 2,
                difficulty: 0b111,
                decide_key: -1,
                ..Default::default()
            },
            events: [EventSpec {
                def: sys::EventDef::default(),
                clauses: [CLAUSE; MAX_EVENT_CLAUSES],
            }; MAX_EVENTS],
            actions: [sys::ActionDef::default(); MAX_ACTIONS],
            binds: [NO_HANDLE; MAX_BINDS],
            binds_len: 0,
            overflow: false,
        };
        trigger.and(kind)
    }

    /// A trigger that fires every `interval` frames (`Timer`), counted from
    /// its creation.
    pub fn every(interval: i32) -> Self {
        let mut trigger = Self::on(Event::TIMER);
        trigger.events[0].def.interval = interval;
        trigger
    }

    /// A trigger on module event `event` ([`crate::ini::event`]): an object
    /// event fires on bound objects, a global event needs no binding.
    pub fn on_module_event(event: ModuleEvent) -> Self {
        Self::on(Event::MODULE_EVENT).id(event.0)
    }

    /// A trigger on `PropertyPoll` (appendix 3 `Trigger.onProperty`): fires
    /// when `prop op value` becomes true, checked every `interval` frames.
    pub fn on_property(prop: Prop, op: Op, value: i64, interval: i32) -> Self {
        Self::on(Event::PROPERTY_POLL).poll(prop.0, op, value, interval)
    }

    /// A trigger on `VariablePoll`: fires when `var op value` becomes true,
    /// checked every `interval` frames; a change of the variable restarts
    /// the interval.
    pub fn on_variable(var: Var, op: Op, value: i64, interval: i32) -> Self {
        Self::on(Event::VARIABLE_POLL).poll(var.0, op, value, interval)
    }

    /// A trigger on `HousePowerPoll`: fires when the trigger's house's power
    /// output minus drain `op value` becomes true, checked every `interval`
    /// frames. Set the house with [`Trigger::house`].
    pub fn on_house_power(op: Op, value: i64, interval: i32) -> Self {
        Self::on(Event::HOUSE_POWER_POLL).poll(-1, op, value, interval)
    }

    /// A trigger on `ModifierCountPoll`: fires when the number of this
    /// module's modifiers in `group` on a bound object `op count` becomes
    /// true, checked every `interval` frames.
    pub fn on_modifier_count(group: i32, op: Op, count: i64, interval: i32) -> Self {
        Self::on(Event::MODIFIER_COUNT_POLL).poll(group, op, count, interval)
    }

    /// A trigger on `DistancePoll`: fires when a bound object's distance in
    /// leptons to the centre of `cell` (a [`Cell`] or [`crate::Waypoint`])
    /// `op leptons` becomes true, checked every `interval` frames.
    pub fn on_distance(cell: impl Ref, op: Op, leptons: i32, interval: i32) -> Self {
        Self::on(Event::DISTANCE_POLL).poll(leptons, op, cell.raw(), interval)
    }

    fn poll(mut self, id: i32, op: Op, value: i64, interval: i32) -> Self {
        if let Some(index) = (self.header.event_count as usize).checked_sub(1) {
            let def = &mut self.events[index].def;
            def.id = id;
            def.op = op as i32;
            def.value = value;
            def.interval = interval;
        }
        self
    }

    /// Sets the latest event's `value` argument (a stock event's house
    /// index, credits, seconds, ...; appendix 4.1).
    pub fn value(mut self, value: i64) -> Self {
        match (self.header.event_count as usize).checked_sub(1) {
            Some(index) => self.events[index].def.value = value,
            None => self.overflow = true,
        }
        self
    }

    /// Sets the latest event's `id` argument (a TechnoType ID for stock
    /// events 60 and 61, the event ID for [`Event::MODULE_EVENT`]).
    pub fn id(mut self, id: i32) -> Self {
        match (self.header.event_count as usize).checked_sub(1) {
            Some(index) => self.events[index].def.id = id,
            None => self.overflow = true,
        }
        self
    }

    /// Adds another event; all must occur before the trigger fires.
    pub fn and(mut self, kind: i32) -> Self {
        match self.events.get_mut(self.header.event_count as usize) {
            Some(event) => {
                event.def = sys::EventDef {
                    kind,
                    id: -1,
                    ..Default::default()
                };
                self.header.event_count += 1;
            }
            None => self.overflow = true,
        }
        self
    }

    /// Fires once per instance instead of every time.
    pub fn once(mut self) -> Self {
        self.header.repeat = 0;
        self
    }

    /// The trigger's house, in place of the subject's owner.
    pub fn house(mut self, house: House) -> Self {
        self.header.house = house.0;
        self
    }

    /// Starts disabled (`TriggerRef::set_enabled` turns it on).
    pub fn disabled(mut self) -> Self {
        self.header.flags |= 1;
        self
    }

    fn clause(mut self, subject: u8, scope: u8, prop: i32, op: Op, value: i64) -> Self {
        let Some(index) = (self.header.event_count as usize).checked_sub(1) else {
            self.overflow = true;
            return self;
        };
        let event = &mut self.events[index];
        match event.clauses.get_mut(event.def.clause_count as usize) {
            Some(clause) => {
                *clause = sys::Clause {
                    subject,
                    op: op as u8,
                    scope,
                    prop,
                    value,
                    ..CLAUSE
                };
                event.def.clause_count += 1;
            }
            None => self.overflow = true,
        }
        self
    }

    /// The latest event counts only when the subject's `prop` compares.
    pub fn where_prop(self, prop: Prop, op: Op, value: i64) -> Self {
        self.clause(0, 0, prop.0, op, value)
    }

    /// As [`Trigger::where_prop`], on the event's other object.
    pub fn where_other(self, prop: Prop, op: Op, value: i64) -> Self {
        self.clause(1, 0, prop.0, op, value)
    }

    /// As [`Trigger::where_prop`], on the trigger's house.
    pub fn where_house(self, prop: Prop, op: Op, value: i64) -> Self {
        self.clause(2, 0, prop.0, op, value)
    }

    /// As [`Trigger::where_prop`], on a global variable.
    pub fn where_var(self, var: Var, op: Op, value: i64) -> Self {
        self.clause(3, 0, var.0, op, value)
    }

    /// Binds the trigger to `reference` (an object, a type, a weapon,
    /// warhead or projectile type for `Fire`, or a cell for stock events 25
    /// and 26) at [`Trigger::done`].
    pub fn bind(mut self, reference: impl Ref) -> Self {
        match self.binds.get_mut(self.binds_len) {
            Some(slot) => {
                *slot = reference.raw();
                self.binds_len += 1;
            }
            None => self.overflow = true,
        }
        self
    }

    /// Runs `action` when the trigger fires.
    pub fn immediate(mut self, action: ActionDef) -> Self {
        match self.actions.get_mut(self.header.action_count as usize) {
            Some(slot) => {
                *slot = action.0;
                self.header.action_count += 1;
            }
            None => self.overflow = true,
        }
        self
    }

    /// Delivers a record with dispatch key `key` to `on_events` in the
    /// module phase.
    pub fn enqueue(self, key: i32) -> Self {
        self.immediate(ActionDef::module(key))
    }

    /// As [`Trigger::enqueue`], with the record's target and parameter text
    /// ([`ActionDef::module_with`]).
    pub fn enqueue_with(self, key: i32, target: Target, text: &'static str) -> Self {
        self.immediate(ActionDef::module_with(key, target, text))
    }

    /// Makes this a decision trigger (RFC 6f) whose requests reach the
    /// [`crate::on_decide!`] handler for `key`, and finalizes it as
    /// [`Trigger::done`] does. Its event must have a decision form; such a
    /// trigger takes no actions.
    pub fn decide_sync(mut self, key: i32) -> Result<TriggerRef> {
        self.header.decide_key = key;
        self.done()
    }

    /// Creates the trigger, then binds it. A builder that overflowed creates
    /// nothing and answers `Refused`; a failed binding answers its status,
    /// the trigger already made.
    pub fn done(self) -> Result<TriggerRef> {
        if self.overflow {
            return Err(crate::Status::Refused);
        }
        let mut buffer = [0u8; BUFFER];
        let mut len = 0;
        let mut put = |bytes: &[u8]| {
            buffer[len..len + bytes.len()].copy_from_slice(bytes);
            len += bytes.len();
        };
        put(bytes_of(&self.header));
        for event in &self.events[..self.header.event_count as usize] {
            put(bytes_of(&event.def));
            for clause in &event.clauses[..event.def.clause_count as usize] {
                put(bytes_of(clause));
            }
        }
        for action in &self.actions[..self.header.action_count as usize] {
            put(bytes_of(action));
        }
        let mut handle = NO_HANDLE;
        check(unsafe { sys::trigger_create(buffer.as_ptr(), len as i32, &mut handle) })?;
        let trigger = TriggerRef(handle);
        for reference in &self.binds[..self.binds_len] {
            check(unsafe { sys::trigger_bind(handle, *reference) })?;
        }
        Ok(trigger)
    }
}

const BUFFER: usize = size_of::<sys::TriggerDef>()
    + MAX_EVENTS * (size_of::<sys::EventDef>() + MAX_EVENT_CLAUSES * size_of::<sys::Clause>())
    + MAX_ACTIONS * size_of::<sys::ActionDef>();

/// The bytes of a padding-free `repr(C)` layout.
fn bytes_of<T: Copy>(value: &T) -> &[u8] {
    // SAFETY: only the `sys` layouts above reach here; each is `repr(C)`
    // with no padding (their sizes are asserted), so every byte is
    // initialized.
    unsafe { core::slice::from_raw_parts((value as *const T).cast(), size_of::<T>()) }
}

// ---------------------------------------------------------------- Records

/// One delivered record (`EventRecord`), valid during its `on_events` call.
pub type EventRecord = sys::EventRecord;

impl EventRecord {
    /// The records `on_events(ptr, count)` received.
    ///
    /// # Safety
    /// `ptr` and `count` must be the arguments the engine passed to
    /// `on_events`, and the slice must not outlive that call.
    pub unsafe fn batch<'a>(ptr: i32, count: i32) -> &'a [EventRecord] {
        // SAFETY: the engine wrote `count` aligned records at `ptr`, in the
        // `vera.delivery` area (8-byte aligned, checked at load).
        unsafe { core::slice::from_raw_parts(ptr as usize as *const EventRecord, count as usize) }
    }

    pub fn trigger(&self) -> TriggerRef {
        TriggerRef(self.trigger)
    }

    pub fn subject(&self) -> Techno {
        Techno(self.subject)
    }

    pub fn other(&self) -> Techno {
        Techno(self.other)
    }

    /// The tag holder: the bound object, or the object or cell whose tag
    /// holds the trigger (raw reference; 0 for none).
    pub fn holder(&self) -> i64 {
        self.holder
    }

    pub fn house(&self) -> House {
        House(self.house)
    }

    /// The `args` string ID: a module action's parameter text, an
    /// `IniEntry` entry name, an `IniKey` value; 0 for none.
    pub fn args(&self) -> i32 {
        self.args
    }

    /// The `args` text, written into `buf` (`""` without any).
    /// `Err(Refused)` when `buf` is too small.
    pub fn args_text<'b>(&self, buf: &'b mut [u8]) -> Result<&'b str> {
        crate::text(self.args, buf)
    }

    /// A module action's target, as its trigger names it (`Subject` on
    /// other records).
    pub fn target(&self) -> Target {
        Target::from_raw(self.target).unwrap_or(Target::Subject)
    }
}

macro_rules! view {
    ($(#[$doc:meta])* $name:ident { $($(#[$fdoc:meta])* $field:ident: $ty:ty = |$r:ident| $read:expr),* $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Copy)]
        pub struct $name<'a>(pub &'a EventRecord);

        impl<'a> From<&'a EventRecord> for $name<'a> {
            fn from(record: &'a EventRecord) -> Self {
                Self(record)
            }
        }

        impl $name<'_> {
            $(
                $(#[$fdoc])*
                pub fn $field(&self) -> $ty {
                    let $r = self.0;
                    $read
                }
            )*

            /// The whole record.
            pub fn record(&self) -> &EventRecord {
                self.0
            }
        }
    };
}
pub(crate) use view;

view!(
    /// `TakeDamage` (appendix 3 and 4.3).
    TakeDamageEvent {
        victim: Techno = |r| Techno(r.subject),
        /// The source; `NO_HANDLE` without one.
        attacker: Techno = |r| Techno(r.other),
        /// Damage after armor.
        damage: i64 = |r| r.value,
        /// Warhead ID, −1 when unknown.
        warhead: i32 = |r| r.id,
        /// The victim's position in leptons.
        position: [i32; 3] = |r| [r.x, r.y, r.z],
    }
);

view!(
    /// `Killed` (appendix 4.3).
    KilledEvent {
        victim: Techno = |r| Techno(r.subject),
        killer: Techno = |r| Techno(r.other),
        position: [i32; 3] = |r| [r.x, r.y, r.z],
    }
);

view!(
    /// `Fire` (appendix 3 and 4.3).
    FireEvent {
        shooter: Techno = |r| Techno(r.subject),
        target: Techno = |r| Techno(r.other),
        target_cell: Cell = |r| Cell(r.cell),
        weapon_slot: i64 = |r| r.value,
        /// Weapon ID.
        weapon: i32 = |r| r.id,
        projectile: i64 = |r| r.projectile,
    }
);

view!(
    /// `Timer` (appendix 4.3).
    TimerEvent {
        /// The frame of the previous firing, or of the trigger's creation.
        last_frame: i64 = |r| r.value,
    }
);

view!(
    /// A stock event (appendix 4.1): subject, other object and house as the
    /// event's row lists them.
    StockEvent {
        subject: Techno = |r| Techno(r.subject),
        other: Techno = |r| Techno(r.other),
        /// The house the event is about (`Hs`), else the trigger's.
        house: House = |r| House(r.house),
        /// The bound cell for stock events 25 and 26.
        bound_cell: Cell = |r| Cell(r.holder),
    }
);

view!(
    /// `Birth` (appendix 4.3).
    BirthEvent {
        object: Techno = |r| Techno(r.subject),
        /// The factory or yard; `NO_HANDLE` when unknown.
        producer: Techno = |r| Techno(r.other),
    }
);

view!(
    /// `TransportEntered` (appendix 4.3), on the passenger.
    TransportEnteredEvent {
        passenger: Techno = |r| Techno(r.subject),
        transport: Techno = |r| Techno(r.other),
        /// The space the passenger occupies.
        space: i64 = |r| r.value,
        /// 1 for a garrison, 0 for a transport.
        garrison: i64 = |r| r.value2,
    }
);

view!(
    /// `PassengerAdded` (appendix 4.3), on the transport.
    PassengerAddedEvent {
        transport: Techno = |r| Techno(r.subject),
        passenger: Techno = |r| Techno(r.other),
        space: i64 = |r| r.value,
        garrison: i64 = |r| r.value2,
    }
);

view!(
    /// `TransportExited` (appendix 4.3), on the passenger.
    TransportExitedEvent {
        passenger: Techno = |r| Techno(r.subject),
        transport: Techno = |r| Techno(r.other),
        /// The space the passenger occupied.
        space: i64 = |r| r.value,
        /// 1 for a garrison, 0 for a transport.
        garrison: i64 = |r| r.value2,
    }
);

view!(
    /// `PassengerRemoved` (appendix 4.3), on the transport.
    PassengerRemovedEvent {
        transport: Techno = |r| Techno(r.subject),
        passenger: Techno = |r| Techno(r.other),
        space: i64 = |r| r.value,
        garrison: i64 = |r| r.value2,
    }
);

view!(
    /// `AttackedBy` (appendix 4.3), on the target after the shot.
    AttackedByEvent {
        target: Techno = |r| Techno(r.subject),
        attacker: Techno = |r| Techno(r.other),
        weapon_slot: i64 = |r| r.value,
        projectile: i64 = |r| r.projectile,
    }
);

view!(
    /// `FatalDamage` (appendix 4.3): the hit left no Strength; death
    /// processing follows.
    FatalDamageEvent {
        victim: Techno = |r| Techno(r.subject),
        /// The source; `NO_HANDLE` without one.
        attacker: Techno = |r| Techno(r.other),
        damage: i64 = |r| r.value,
        /// Warhead ID, −1 when unknown.
        warhead: i32 = |r| r.id,
        /// The victim's position in leptons.
        position: [i32; 3] = |r| [r.x, r.y, r.z],
    }
);

view!(
    /// `HouseTransferred` (appendix 4.3).
    HouseTransferredEvent {
        object: Techno = |r| Techno(r.subject),
        old_house: House = |r| House(r.house),
        /// 1 for a mind control.
        mind_control: i64 = |r| r.value,
    }
);

view!(
    /// `SuperWeaponLaunched` (appendix 4.3).
    SuperWeaponLaunchedEvent {
        /// Superweapon type ID.
        super_weapon: i32 = |r| r.id,
        house: House = |r| House(r.house),
        target_cell: Cell = |r| Cell(r.cell),
    }
);

view!(
    /// `ModifierStart` and `ModifierExpire` (appendix 4.3), on the modified
    /// object. An expired modifier reads as gone.
    ModifierEvent {
        target: Techno = |r| Techno(r.subject),
        modifier: Modifier = |r| Modifier(r.modifier),
        /// The modifier's property.
        property: Prop = |r| Prop(r.value as i32),
        /// The modifier's source object; `NO_HANDLE` without one.
        source: Techno = |r| Techno(r.other),
    }
);

view!(
    /// `ModifierStacks` (appendix 4.3), on the modified object.
    ModifierStacksEvent {
        target: Techno = |r| Techno(r.subject),
        /// The modifier just added.
        modifier: Modifier = |r| Modifier(r.modifier),
        /// How many modifiers the object now holds in the group.
        count: i64 = |r| r.value,
    }
);

view!(
    /// `Killed` (appendix 4.3) with its lethal warhead.
    KilledByWarheadEvent {
        victim: Techno = |r| Techno(r.subject),
        killer: Techno = |r| Techno(r.other),
        /// Warhead ID, −1 when the kill had none.
        warhead: i32 = |r| r.id,
    }
);

view!(
    /// `TargetChanged` (appendix 4.3): after the object's target was set or
    /// cleared.
    TargetChangedEvent {
        object: Techno = |r| Techno(r.subject),
        /// The new target object; `NO_HANDLE` for a cell target or none.
        target: Techno = |r| Techno(r.other),
        /// The new target's cell; `Cell(NO_HANDLE)` when cleared.
        target_cell: Cell = |r| Cell(r.cell),
    }
);

view!(
    /// `Detonated` (appendix 4.3): a fired projectile's detonation, on its
    /// source (`NO_HANDLE` without one).
    DetonatedEvent {
        source: Techno = |r| Techno(r.subject),
        /// The projectile, already gone.
        projectile: i64 = |r| r.projectile,
        /// Warhead ID.
        warhead: i32 = |r| r.id,
        cell: Cell = |r| Cell(r.cell),
        /// The detonation point in leptons.
        position: [i32; 3] = |r| [r.x, r.y, r.z],
    }
);

view!(
    /// `Crushed` (appendix 4.3), on the victim before its death.
    CrushedEvent {
        victim: Techno = |r| Techno(r.subject),
        crusher: Techno = |r| Techno(r.other),
        position: [i32; 3] = |r| [r.x, r.y, r.z],
    }
);

view!(
    /// `MissionChanged` (appendix 4.3).
    MissionChangedEvent {
        object: Techno = |r| Techno(r.subject),
        /// The new mission's number ([`crate::Mission`]).
        mission: i64 = |r| r.value,
    }
);

view!(
    /// `Deployed` (appendix 4.3).
    DeployedEvent {
        object: Techno = |r| Techno(r.subject),
        /// The object it became (a building, an undeployed unit);
        /// `NO_HANDLE` when it stays itself.
        became: Techno = |r| Techno(r.other),
        /// 1 deployed, 0 undeployed.
        deployed: i64 = |r| r.value,
    }
);

view!(
    /// `Promoted` (appendix 4.3).
    PromotedEvent {
        object: Techno = |r| Techno(r.subject),
        /// The new rank: 1 veteran, 2 elite.
        rank: i64 = |r| r.value,
    }
);

view!(
    /// `ExperienceGained` (appendix 4.3), on a kill's experience recipient.
    ExperienceGainedEvent {
        object: Techno = |r| Techno(r.subject),
        victim: Techno = |r| Techno(r.other),
        /// The gain in hundredths of a rank (the `Experience` property's
        /// unit).
        gain: i64 = |r| r.value,
    }
);

view!(
    /// `Cloaked` and `Decloaked` (appendix 4.3).
    CloakEvent {
        object: Techno = |r| Techno(r.subject),
    }
);

view!(
    /// The polling events (appendix 4.4).
    PollEvent {
        /// The polled object; `NO_HANDLE` for a global poll.
        subject: Techno = |r| Techno(r.subject),
        /// The reading that made the condition true.
        reading: i64 = |r| r.value,
    }
);

// ----------------------------------------------------------------- Exports

/// Exports `on_events`, calling the handler whose key matches each record,
/// in order. A handler takes `&EventRecord` or an event view. Keys are the
/// module's own: those of [`named_actions!`](crate::named_actions!),
/// [`Trigger::enqueue`] and the INI registrations of [`crate::ini`], whose
/// handlers take [`crate::IniEntryRecord`] or [`crate::IniKeyRecord`]. The
/// macro also declares the module's delivery area ([`DELIVERY_SECTION`]).
///
/// ```ignore
/// vera20k_sdk::on_events!(ON_HIT => on_hit, ON_DEATH => on_death);
/// ```
#[macro_export]
macro_rules! on_events {
    ($($key:path => $handler:path),* $(,)?) => {
        #[used]
        #[unsafe(link_section = "vera.delivery")]
        static __VERA_DELIVERY: [u8; 9] = $crate::DELIVERY_SECTION;

        #[unsafe(no_mangle)]
        pub extern "C" fn on_events(ptr: i32, count: i32) {
            // SAFETY: these are the engine's own arguments, used only here.
            let records = unsafe { $crate::EventRecord::batch(ptr, count) };
            for record in records {
                $(
                    if record.key == $key {
                        $handler(::core::convert::From::from(record));
                        continue;
                    }
                )*
            }
        }
    };
}

/// Where the SDK's delivery area lies: eight records from address 8.
///
/// A `link_section` static holds plain bytes only (rustc refuses an address
/// there), so the area cannot be a static buffer whose address the linker
/// fills in. The first KiB of memory is free in both wasm32 layouts
/// wasm-ld produces: below the 1024-byte global base without
/// `--stack-first`, and the far end of the 1 MiB stack with it (rustc's
/// default). Limitation: a module whose stack comes within a KiB of its
/// 1 MiB limit during `on_events` would overwrite records it has not read.
pub const DELIVERY_OFFSET: u32 = 8;
pub const DELIVERY_RECORDS: u32 = 8;

/// `EventRecord`'s size; a `DecideRecord` takes 160.
const RECORD_BYTES: u32 = size_of::<EventRecord>() as u32;

/// The `vera.delivery` section (appendix "Custom sections") for
/// [`DELIVERY_OFFSET`] and [`DELIVERY_RECORDS`].
pub const DELIVERY_SECTION: [u8; 9] = {
    let offset = DELIVERY_OFFSET.to_le_bytes();
    let bytes = (DELIVERY_RECORDS * RECORD_BYTES).to_le_bytes();
    [
        1, offset[0], offset[1], offset[2], offset[3], bytes[0], bytes[1], bytes[2], bytes[3],
    ]
};

/// Declares the module's named actions as one `vera.actions` section and a
/// `const` dispatch key for each. A module has exactly one such section: a
/// second `named_actions!` would append a second blob, and the engine
/// rejects the module (trailing bytes).
///
/// ```ignore
/// vera20k_sdk::named_actions! { ON_HIT = "OnHit" => 1, ON_DEATH = "OnDeath" => 2 }
/// ```
#[macro_export]
macro_rules! named_actions {
    ($($key:ident = $name:literal => $value:expr),* $(,)?) => {
        $(pub const $key: i32 = $value;)*

        #[used]
        #[unsafe(link_section = "vera.actions")]
        static __VERA_ACTIONS: [u8; $crate::actions_section_len(&[$($name),*])] =
            $crate::actions_section(&[$(($name, $value)),*]);
    };
}

/// The byte length of a `vera.actions` section naming `names`.
pub const fn actions_section_len(names: &[&str]) -> usize {
    let mut len = 1 + 4;
    let mut i = 0;
    while i < names.len() {
        len += 2 + names[i].len() + 4;
        i += 1;
    }
    len
}

/// A `vera.actions` section (appendix "Custom sections"): format 1, count,
/// then each name's `u16` length, bytes and `i32` key. Names must be ASCII
/// and unique ignoring case; the engine checks.
pub const fn actions_section<const N: usize>(entries: &[(&str, i32)]) -> [u8; N] {
    let mut out = [0u8; N];
    out[0] = 1;
    let count = (entries.len() as u32).to_le_bytes();
    let mut at = 1;
    let mut i = 0;
    while i < 4 {
        out[at + i] = count[i];
        i += 1;
    }
    at += 4;
    let mut e = 0;
    while e < entries.len() {
        let (name, key) = entries[e];
        let name = name.as_bytes();
        let len = (name.len() as u16).to_le_bytes();
        out[at] = len[0];
        out[at + 1] = len[1];
        at += 2;
        let mut b = 0;
        while b < name.len() {
            out[at + b] = name[b];
            b += 1;
        }
        at += name.len();
        let key = key.to_le_bytes();
        let mut k = 0;
        while k < 4 {
            out[at + k] = key[k];
            k += 1;
        }
        at += 4;
        e += 1;
    }
    out
}
