//! The event kinds module triggers take beyond `TakeDamage`, `Killed`,
//! `Fire` and `Timer` (RFC 0001 appendix 4): what `trigger_create` accepts
//! of each, where each is raised and what its record holds.
//!
//! Stock events 0..61 and, when Ares plays (`rules.ares_form()`), Ares
//! events 62..88 (appendix 4.1, 4.2), read through the trigger runtime's own
//! predicates (`trigger_runtime::module_events`; Ares' through
//! TEventExt::HasOccured 1004F7A0):
//! - object events 1, 4, 6, 7, 29, 34, 35, 38..44, 48, 49 and Ares 62..74,
//!   79, 80, 83..85, raised for bound objects at the object Tag funnel
//!   (`dispatch_object_tag_input`, the Tag6E53A0 ingress every native site
//!   uses); `other` is the raise's source or acting object;
//! - object events 2 (`SpiedBy`) and 3 (`ThievedBy`), which no native site
//!   raises on a Tag, raised for module triggers only
//!   (`raise_module_infiltration_event`): 2 on the building after
//!   BuildingClass::Infiltrate (`0x004571E0`, the PerCell spy arm
//!   `0x0051A002`), the spy as `other`; 3 on the vehicle a thief took
//!   (TryThiefVehicleCapture `0x005203F7..0x0052046D` after ChangeOwner,
//!   Ares' capture `10045B60` after its Tag events), the thief as `other`,
//!   before the thief leaves. 3's `value` is the thief's house, which the
//!   record carries;
//! - cell events 25 and 26, for cells bound with a cell reference, from the
//!   foot's per-cell arrival (Foot4D8A70..4D8B66): a bound cell on the
//!   arrival cell's row (25) or column (26);
//! - global raised events, for every global trigger: 23 (team leaves map,
//!   `id` a TeamType, beside TeamClass's event-23 Tag broadcasts 6E92D0 and
//!   6EA49F), 50 (any crate, at Pickup481A7F where the crate marks the
//!   Scenario crate byte) and Ares 75..77 (SWTypeExt::Activate 10032020 and
//!   Deactivate 100325D0, which raise them on the owner's House Tags: a
//!   trigger with a fixed house takes only its own house's);
//! - House events 5, 8..12, 15..22, 30, 32, 52, 55..58 and Ares 78, 81, 82,
//!   polled every eighth frame, and logic events 13, 14, 27, 28, 36, 37,
//!   45..47, 60, 61 and Ares 86, every frame, at the start of the module
//!   phase. Ares' `id` names the TechnoType (77: the SuperWeaponType).
//!
//! Stock events module triggers do not take (`Unsupported`):
//! - 0 is private and never true;
//! - 24, 53, 54 and 59 reach only map CellTags and the Map tag list, whose
//!   binding a module trigger has no form for;
//! - 31 (bridge destroyed): its raise knows the cell's native identity, not
//!   its coordinates;
//! - 33 (selected) depends on one player's selection and is off online;
//! - 51 (random delay) draws the native random stream for each trigger;
//! - Ares 87 and 88 read `value` as Ares' ResolveHouseParam selector
//!   (Country index or Player@X), not as a house index.
//!
//! New delivered events (appendix 4.3):
//! - `Birth` after a produced object's RecordLastBuilt (`0x004FB6B0`), with
//!   the factory or yard as `other` where the site knows it; after each
//!   member a reinforcement placed (SpawnReinforcements 65D8E0's Unlimbo
//!   loop 65E010 or its building exit, Chrono reinforcement 65EC30), the
//!   building of TActionCreateBuilding 6E4200 (after its Grand_Opening),
//!   each object HouseClass 50D440/50D320 recreates (stock actions 126 and
//!   130), and a module's `CreateObject`; for the building a unit deployed
//!   into (UnitClass::Deploy `0x007393C0`, before the unit's UnInit) and
//!   the unit a building undeployed into (Mission_Selling `0x00449CEA`,
//!   before the building's UnInit), the origin as `other`; for each
//!   survivor, crewman and pilot placed (SpawnSurvivors `0x00442D90`, a
//!   sale's crew `0x0044A5F0`, a dying unit's crew `0x007381BC`, Ares'
//!   pilots `10046540`), the building or vehicle as `other`;
//! - `TransportEntered` (passenger) and `PassengerAdded` (transport) at the
//!   end of AddPassenger (`0x00710670`), `value` the passenger's size;
//!   `TransportExited` and `PassengerRemoved` after a cargo departure placed
//!   its passenger (CargoClass::RemoveFirstPassenger 0x00473430, then the
//!   route's Unlimbo); in their garrison form (`value2` 1, `value` the
//!   occupants then inside) when an occupier enters (AddGarrisonOccupant
//!   `0x00522A18`), is ejected (SellBuilding `0x00457DE0`, after its
//!   placement bracket) or leaves inside (KillOccupants `0x004585C0`, an
//!   assault, Ares' fatal occupant hit; before the UnInit);
//! - `AttackedBy` on the target object after a FireAt that launched its
//!   bullet (Techno6FDD50), beside the shooter's `Fire`;
//! - `FatalDamage` after the Strength write that leaves 0 (Object5F5390),
//!   before Kill and Destroy, which read the live Strength again; and before
//!   the Record_The_Kill (vt+0xE0) of every death without a damage commit
//!   (`Simulation::record_the_kill`: crush, Temporal erase, a crash's live
//!   prefix, passengers and slaves dying inside, Magnetron drops, warp
//!   sinks), `value` the Strength left, `id` −1. A sale is no death;
//! - `Crushed` on the victim once a crush is decided, before its kill:
//!   Crush_Or_Prescatter_Cell (`0x007416A0`) and the Magnetron or Jumpjet
//!   landing crush (`0x0054CCDE`);
//! - `Detonated` after a fired projectile's whole detonation
//!   (`raise_module_detonated`);
//! - `TargetChanged`, `Deployed`, `Promoted`, `ExperienceGained`, `Cloaked`
//!   and `Decloaked` at the unit control points (`ext_modules::control`);
//! - `HouseTransferred` at the end of ChangeOwner (`0x007014A0`), `house`
//!   the old house, `value` 1 when CaptureManager's Capture (0x00471D40),
//!   FreeUnit (0x004720D5), PsyDom's permanent capture or Ares' permanent
//!   mind control and Dominator transfer the object;
//! - `SuperWeaponLaunched` after SuperClass::Launch (`0x006CC390`);
//! - `ModifierStart`, `ModifierStacks` and `ModifierExpire` on the modified
//!   object, from the modifier store's callers (`ext_modules::modifiers`);
//! - `Commanded` on each actor of a command the receiver
//!   (EventClass::Execute 0x004C6CB0) applied, and `OrderRefused` (global)
//!   for one it refused, player commands and module orders alike, after the
//!   receiver in the command step (`ext_modules::orders`).
//!
//! Polling events (appendix 4.4): `PropertyPoll`, `HousePowerPoll` (the
//! trigger's house's `PowerOutput` minus `PowerDrain`), `ModifierCountPoll`
//! (modifiers on the subject in group `id` that the trigger's module
//! added), `DistancePoll` (the subject's `X`, `Y` to the centre of the cell
//! or waypoint `value`, by Sqrt_Approx 0x4CAC40, compared with `id`) and
//! `VariablePoll`, checked at the start of the module phase every
//! `interval` frames counted from the trigger's creation, or from the latest
//! change of a `VariablePoll`'s variable; an object on frames where that
//! count matches its creation serial modulo `interval`. Each fires when its
//! condition becomes true and only again after a check finds it false; the
//! record's `value` is the reading.
//!
//! Residuals:
//! - `Birth` is not raised for paradropped reinforcement members (they
//!   leave their plane later, raising `TransportExited`), spawned aircraft,
//!   crate units or the map's own starting objects (placed before modules
//!   install); `value` (production time) is −1.
//! - Cargo that leaves other than by a departure (a dying or crashing
//!   transport, an uninit) raises no `TransportExited`/`PassengerRemoved`,
//!   and neither does Ares' trench traverse moving an occupant between
//!   buildings (`transfer_garrison_head`).
//! - Stock 19..22 read `value` as the class type index; the `id` form is
//!   not read. Ares 77 compares the launched type's first 24 bytes with the
//!   whole name of `id`.
//! - Polls on type-bound triggers walk the object IDs on a frame something
//!   is due and read the types of that phase's objects alone (RFC 15.1
//!   item 1 wants a side column). A type-bound trigger's `Timer` reads every
//!   object's type when due.
//! - The other 4.3 events answer `Unsupported`: `HitByWarhead` and
//!   `HitTarget` (no raise point apart from `TakeDamage` that knows the
//!   immune hit or the shooter's weapon), `SpawnFire`, `MissionChanged`
//!   (`ext_modules::control`),
//!   `ProjectileIncoming`,
//!   `HouseTransfer` (before ChangeOwner, which would have to recheck),
//!   the modifier events and `ProductionCancelled` (House Abandon 0x004FAA10
//!   also settles every delivery).

use std::collections::BTreeSet;

use vera20k_wasm_ext::triggers::{
    ACTION_MODULE, EVENT_ATTACKED_BY, EVENT_BIRTH, EVENT_CRUSHED, EVENT_DETONATED,
    EVENT_DISTANCE_POLL, EVENT_FATAL_DAMAGE, EVENT_FIRE, EVENT_HOUSE_POWER_POLL,
    EVENT_HOUSE_TRANSFERRED, EVENT_INI_ENTRY, EVENT_INI_KEY, EVENT_KILLED,
    EVENT_MODIFIER_COUNT_POLL, EVENT_MODIFIER_EXPIRE, EVENT_MODIFIER_STACKS, EVENT_MODIFIER_START,
    EVENT_MODULE, EVENT_PASSENGER_ADDED, EVENT_PASSENGER_REMOVED, EVENT_PROPERTY_POLL,
    EVENT_SUPER_WEAPON_LAUNCHED, EVENT_TAKE_DAMAGE, EVENT_TIMER, EVENT_TRANSPORT_ENTERED,
    EVENT_TRANSPORT_EXITED, EVENT_VARIABLE_POLL, EventDef,
};
use vera20k_wasm_ext::triggers::{
    EVENT_CLOAKED, EVENT_COMMANDED, EVENT_DECLOAKED, EVENT_DEPLOYED, EVENT_EXPERIENCE_GAINED,
    EVENT_ORDER_REFUSED, EVENT_PROMOTED, EVENT_TARGET_CHANGED,
};
use vera20k_wasm_ext::{Invalid, Status, VarScope, property_id, property_name};

use super::triggers::variable_scope;
use super::{Host, ModuleEvent, Ref, cell_ref, decode_ref, house_ref, techno_type};
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::trigger_runtime::{
    StockClass, StockInput, StockReading, TriggerScope, stock_class, stock_poll_due,
    value_filters_actor_house, value_names_house,
};
use crate::sim::world::{FrameEffects, Simulation};

/// New delivered events this engine raises (appendix 4.3, and the INI
/// module event of RFC 0001 section 6h).
const DELIVERED: [i32; 27] = [
    EVENT_TAKE_DAMAGE,
    EVENT_DETONATED,
    EVENT_CRUSHED,
    EVENT_TARGET_CHANGED,
    EVENT_DEPLOYED,
    EVENT_PROMOTED,
    EVENT_EXPERIENCE_GAINED,
    EVENT_CLOAKED,
    EVENT_DECLOAKED,
    EVENT_ATTACKED_BY,
    EVENT_FATAL_DAMAGE,
    EVENT_KILLED,
    EVENT_FIRE,
    EVENT_BIRTH,
    EVENT_TRANSPORT_ENTERED,
    EVENT_TRANSPORT_EXITED,
    EVENT_PASSENGER_ADDED,
    EVENT_PASSENGER_REMOVED,
    EVENT_HOUSE_TRANSFERRED,
    EVENT_TIMER,
    EVENT_SUPER_WEAPON_LAUNCHED,
    EVENT_MODIFIER_START,
    EVENT_MODIFIER_EXPIRE,
    EVENT_MODIFIER_STACKS,
    EVENT_MODULE,
    EVENT_COMMANDED,
    EVENT_ORDER_REFUSED,
];

/// The polling events (appendix 4.4), every one of which this engine checks.
const POLLS: [i32; 5] = [
    EVENT_PROPERTY_POLL,
    EVENT_HOUSE_POWER_POLL,
    EVENT_MODIFIER_COUNT_POLL,
    EVENT_DISTANCE_POLL,
    EVENT_VARIABLE_POLL,
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

/// The stock events a module trigger takes here: Ares' (62..88) only when
/// Ares plays (appendix 4.2).
fn takes_stock(rules: &RuleSet, kind: i32) -> bool {
    stock_class(kind).is_some() && (kind < 62 || rules.ares_form())
}

/// Whether this engine raises event `kind`: `has_kind(0, kind)`.
pub(super) fn raises(rules: &RuleSet, kind: i32) -> bool {
    takes_stock(rules, kind) || DELIVERED.contains(&kind) || POLLS.contains(&kind)
}

/// Events checked in the module phase rather than raised at a site.
fn is_polled(kind: i32) -> bool {
    POLLS.contains(&kind)
        || matches!(
            stock_class(kind),
            Some(StockClass::House | StockClass::Logic)
        )
}

/// The share of holders whose check falls on one frame: every `interval`
/// frames from the trigger's poll start, staggered by creation serial
/// (appendix 4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PollPhase {
    interval: u32,
    phase: u32,
}

impl PollPhase {
    fn of(interval: i32, start: u32, frame: u32) -> Option<Self> {
        let interval = u32::try_from(interval).ok().filter(|i| *i > 0)?;
        (frame > start).then(|| Self {
            interval,
            phase: (frame - start) % interval,
        })
    }

    fn holds(self, holder: u64) -> bool {
        (holder % u64::from(self.interval)) as u32 == self.phase
    }
}

/// The objects of each phase, with their TechnoTypes, as far as this frame's
/// polls have needed them, so polls on one phase share one scan. An action
/// may create, remove or convert objects, so the scans are dropped whenever
/// a polled trigger with actions is offered an event.
#[derive(Default)]
struct PhaseScans(Vec<(PollPhase, Vec<(u64, u32)>)>);

impl PhaseScans {
    fn objects(&mut self, sim: &Simulation, rules: &RuleSet, due: PollPhase) -> &[(u64, u32)] {
        let at = match self.0.iter().position(|(scanned, _)| *scanned == due) {
            Some(at) => at,
            None => {
                let objects = sim
                    .entities()
                    .iter_sorted()
                    .filter(|(id, _)| due.holds(*id))
                    .filter_map(|(id, entity)| {
                        let handle = sim.interner.type_handle(entity.type_ref(), rules)?;
                        Some((id, handle.0))
                    })
                    .collect();
                self.0.push((due, objects));
                self.0.len() - 1
            }
        };
        &self.0[at].1
    }
}

/// The name of TechnoType or, for Ares 77, SuperWeaponType `def.id`, which
/// stock events 60 and 61 and Ares' typed events compare.
fn stock_type_name<'r>(rules: &'r RuleSet, def: &EventDef) -> Option<&'r str> {
    match def.kind {
        60 | 61 | 78 | 80 | 81 | 82 | 86 => techno_type(rules, def.id).map(|o| o.id.as_str()),
        77 => usize::try_from(def.id)
            .ok()
            .and_then(|index| rules.super_weapon_order.get(index))
            .map(String::as_str),
        _ => None,
    }
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
        if (def.kind == EVENT_TIMER || POLLS.contains(&def.kind)) && def.interval < 1 {
            return invalid(format!("interval {}", def.interval));
        }
        let scope = match def.kind {
            EVENT_TIMER
            | EVENT_SUPER_WEAPON_LAUNCHED
            | EVENT_HOUSE_POWER_POLL
            | EVENT_ORDER_REFUSED => TriggerScope::Global,
            // A registered module event: object or global as registered.
            EVENT_MODULE => match self.sim.ext_modules.ini.event_is_global(def.id) {
                Some(true) => TriggerScope::Global,
                Some(false) => object,
                None => return invalid(format!("no module event {}", def.id)),
            },
            // Start-of-match records only, never a trigger's.
            EVENT_INI_ENTRY | EVENT_INI_KEY => {
                return invalid("IniEntry and IniKey are not trigger events".into());
            }
            // Decision-only (appendix 4.5): Defeat asks about a house and
            // needs no binding; ProductionAllowed and ProductionQueued bind
            // to the type, CommandAllowed to the commanded object or its
            // type.
            vera20k_wasm_ext::decide::EVENT_DEFEAT => TriggerScope::Global,
            vera20k_wasm_ext::decide::EVENT_PRODUCTION_ALLOWED
            | vera20k_wasm_ext::decide::EVENT_PRODUCTION_QUEUED
            | vera20k_wasm_ext::decide::EVENT_COMMAND_ALLOWED => object,
            // The unit decisions (`decide`, `control`, `unit_ai`) bind to the
            // object or its type.
            vera20k_wasm_ext::decide::EVENT_DAMAGE_INCOMING
            | vera20k_wasm_ext::decide::EVENT_FIRE_ALLOWED
            | vera20k_wasm_ext::decide::EVENT_RETALIATE
            | vera20k_wasm_ext::decide::EVENT_DESTINATION_SET
            | vera20k_wasm_ext::decide::EVENT_CRUSH_ALLOWED
            | vera20k_wasm_ext::decide::EVENT_HARVEST_CELL
            | vera20k_wasm_ext::decide::EVENT_DEPLOY_ALLOWED
            | vera20k_wasm_ext::decide::EVENT_IDLE_MISSION
            | vera20k_wasm_ext::decide::EVENT_MISSION_STEP
            // Movement (`motion`, `paths`): the mover or its type.
            | vera20k_wasm_ext::decide::EVENT_MOVE_FRAME
            | vera20k_wasm_ext::decide::EVENT_PATH_FOUND
            | vera20k_wasm_ext::decide::EVENT_APPROACH_POINT => object,
            // `Fire`, `Launch` and `Detonated` also bind the weapon, warhead
            // and projectile type; `ProjectileFrame` binds the projectile type.
            EVENT_FIRE
            | EVENT_DETONATED
            | vera20k_wasm_ext::decide::EVENT_LAUNCH
            | vera20k_wasm_ext::decide::EVENT_PROJECTILE_FRAME => TriggerScope::Object { weapon: true },
            EVENT_TAKE_DAMAGE
            | EVENT_ATTACKED_BY
            | EVENT_CRUSHED
            | EVENT_FATAL_DAMAGE
            | EVENT_KILLED
            | EVENT_BIRTH
            | EVENT_TRANSPORT_ENTERED
            | EVENT_TRANSPORT_EXITED
            | EVENT_PASSENGER_ADDED
            | EVENT_PASSENGER_REMOVED
            | EVENT_HOUSE_TRANSFERRED
            | EVENT_MODIFIER_START
            | EVENT_MODIFIER_EXPIRE
            | EVENT_MODIFIER_STACKS
            | EVENT_COMMANDED
            | EVENT_TARGET_CHANGED
            | EVENT_DEPLOYED
            | EVENT_PROMOTED
            | EVENT_EXPERIENCE_GAINED
            | EVENT_CLOAKED
            | EVENT_DECLOAKED => object,
            // Appendix 4.4: other properties are refused at creation.
            EVENT_PROPERTY_POLL => match property_name(def.id) {
                Some("Money") => TriggerScope::Global,
                Some(name) if OBJECT_POLL_PROPERTIES.contains(&name) => object,
                _ => return invalid(format!("PropertyPoll of property {}", def.id)),
            },
            // A modifier group; 0 counts the ungrouped ones.
            EVENT_MODIFIER_COUNT_POLL if def.id < 0 => {
                return invalid(format!("modifier group {}", def.id));
            }
            EVENT_MODIFIER_COUNT_POLL => object,
            EVENT_DISTANCE_POLL => match decode_ref(def.value)? {
                Some(Ref::Cell { .. }) => object,
                Some(Ref::Waypoint(number)) if self.reader().waypoint_cell(number).is_some() => {
                    object
                }
                Some(Ref::Waypoint(_)) => return Ok(Err(Status::NotFound)),
                _ => return invalid(format!("{:#x} is not a cell", def.value)),
            },
            EVENT_VARIABLE_POLL => match variable_scope(&self.sim.ext_modules.vars, def.id)? {
                VarScope::Object => object,
                VarScope::Global | VarScope::House => TriggerScope::Global,
            },
            kind => {
                let Some(class) = stock_class(kind).filter(|_| takes_stock(self.rules, kind))
                else {
                    return Ok(Err(Status::Unsupported));
                };
                if let Some(status) = self.check_stock_arguments(def)? {
                    return Ok(Err(status));
                }
                match class {
                    StockClass::Object => object,
                    StockClass::Cell => TriggerScope::Cell,
                    StockClass::Global | StockClass::House | StockClass::Logic => {
                        TriggerScope::Global
                    }
                }
            }
        };
        Ok(Ok(scope))
    }

    /// A stock event's `value` and `id` (appendix 4.1, 4.2): house indices
    /// in range, variables in their tables, a known TechnoType, TeamType or
    /// SuperWeaponType, an existing waypoint (`NotFound` otherwise).
    fn check_stock_arguments(&self, def: &EventDef) -> Result<Option<Status>, Invalid> {
        let houses = self.sim.houses.order().len() as i64;
        let in_range = |range: std::ops::Range<i64>| match range.contains(&def.value) {
            true => Ok(None),
            false => Err(Invalid(format!(
                "event {} value {} outside {range:?}",
                def.kind, def.value
            ))),
        };
        let named = |found: bool, what: &str| match found {
            true => Ok(None),
            false => Err(Invalid(format!("event {} {what} {}", def.kind, def.id))),
        };
        let waypoint = || {
            i32::try_from(def.value)
                .ok()
                .and_then(|waypoint| self.sim.trigger_waypoint_location(waypoint))
                .is_none()
                .then_some(Status::NotFound)
        };
        match def.kind {
            kind if value_names_house(kind) => in_range(0..houses),
            // `ThievedBy`: the thief's house.
            3 => in_range(0..houses),
            kind if value_filters_actor_house(kind) && def.value != -1 => in_range(0..houses),
            27 | 28 => in_range(0..50),
            36 | 37 => in_range(0..100),
            60 | 61 | 78 | 80 | 81 | 82 | 86 => {
                named(techno_type(self.rules, def.id).is_some(), "TechnoType")
            }
            23 => named(
                usize::try_from(def.id)
                    .ok()
                    .and_then(|id| self.sim.stock_team_type_name(id))
                    .is_some(),
                "TeamType",
            ),
            34 => Ok(waypoint()),
            // Ares: the source's or the source House's array index.
            63 | 65 | 68 | 70 | 72 | 74 | 84 | 85 => in_range(0..houses),
            75 | 76 => in_range(0..self.rules.super_weapon_order.len() as i64),
            77 => match stock_type_name(self.rules, def) {
                Some(_) => Ok(i32::try_from(def.value)
                    .ok()
                    .and_then(|waypoint| self.sim.session.waypoint_cell(waypoint))
                    .is_none()
                    .then_some(Status::NotFound)),
                None => named(false, "SuperWeaponType"),
            },
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
            kind if POLLS.contains(&kind) => (event.poll == Some(bit)).then_some(None),
            // A module event trigger takes only its own event ID.
            EVENT_MODULE => (def.id == event.id).then_some(None),
            kind if stock_class(kind).is_some() => {
                let reading = StockReading {
                    def,
                    subject: event.subject,
                    other: event.other,
                    input: event.stock,
                    trigger_house: house,
                    poll_start,
                    type_name: stock_type_name(rules, def),
                };
                let (holds, carried) = self.module_stock_event_holds(&reading, rules);
                holds.then_some(carried.or(event.house))
            }
            _ => Some(None),
        }
    }

    /// Stock or Ares object event `kind` raised on `subject` (appendix 4.1,
    /// 4.2, the object Tag funnel), with what the raise passed the funnel.
    /// Call it only when the trigger runtime's `module_listens(kind)`.
    pub(crate) fn raise_module_stock_object_event(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        kind: i32,
        subject: u64,
        other: Option<u64>,
        stock: StockInput,
    ) {
        if stock_class(kind) != Some(StockClass::Object) {
            return;
        }
        let event = ModuleEvent {
            other: other.unwrap_or(0),
            stock,
            ..ModuleEvent::new(kind, subject)
        };
        self.raise_module_event(rules, frame_effects, event);
    }

    /// Stock 2 (`SpiedBy`) on an infiltrated building or 3 (`ThievedBy`) on
    /// a stolen vehicle, `actor` the spy or thief: a VERA raise for module
    /// triggers only, as the native sites raise neither on the object's Tag
    /// (`dispatch_object_tag_input` would reach map Tags as well).
    pub(crate) fn raise_module_infiltration_event(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        kind: i32,
        subject: u64,
        actor: u64,
    ) {
        if !self.trigger_runtime.module_listens(kind) {
            return;
        }
        let stock = StockInput {
            actor: Some(actor),
            source: None,
            ares_source: None,
        };
        self.raise_module_stock_object_event(
            rules,
            frame_effects,
            kind,
            subject,
            Some(actor),
            stock,
        );
    }

    /// A global raised stock event (23, 50, Ares 75..77) for every global
    /// trigger with it. `owner` is the house whose House Tags the native
    /// raise reaches, if any: the record carries it, and a trigger whose
    /// house is fixed takes only that house's raise.
    pub(crate) fn raise_module_global_stock_event(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        kind: i32,
        owner: Option<InternedId>,
        stock: StockInput,
    ) {
        if stock_class(kind) != Some(StockClass::Global)
            || !self.trigger_runtime.module_listens(kind)
        {
            return;
        }
        let house = owner
            .and_then(|owner| self.houses.index_of(owner))
            .map(|index| index as u32);
        let event = ModuleEvent {
            house,
            stock,
            ..ModuleEvent::new(kind, 0)
        };
        for handle in self.trigger_runtime.module_triggers().global(kind) {
            let fixed = self
                .trigger_runtime
                .module_triggers()
                .get(handle)
                .and_then(|trigger| trigger.house);
            if owner.is_some() && fixed.is_some_and(|fixed| Some(fixed) != house) {
                continue;
            }
            self.offer_module_event(rules, frame_effects, handle, 0, &event);
        }
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
    /// of the module phase: stock and Ares House and logic events on their
    /// stock cadence, the 4.4 polls on their interval.
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
        let mut scans = PhaseScans::default();
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
            // Only a trigger's own actions change the objects during the polls.
            let acts = trigger.actions.iter().any(|a| a.kind != ACTION_MODULE);
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
                        if acts {
                            scans.0.clear();
                        }
                    }
                    continue;
                }
                // The cadence first, so a frame with nothing due selects nothing.
                let Some(due) = PollPhase::of(def.interval, poll_start, frame) else {
                    continue;
                };
                let holders = if !global {
                    self.due_poll_holders(rules, handle, due, &mut scans)
                } else if due.holds(0) {
                    vec![0]
                } else {
                    continue;
                };
                let house = fixed_house.or_else(|| self.module_trigger_house(rules, 0));
                for holder in holders {
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
                    if acts {
                        scans.0.clear();
                    }
                }
            }
        }
    }

    /// The live objects of `due` that an object poll of `handle` checks:
    /// [`Self::module_poll_holders`] of that phase, selected without reading
    /// any other object's type.
    fn due_poll_holders(
        &self,
        rules: &RuleSet,
        handle: u64,
        due: PollPhase,
        scans: &mut PhaseScans,
    ) -> Vec<u64> {
        let triggers = self.trigger_runtime.module_triggers();
        let mut holders: BTreeSet<u64> = triggers
            .bound_objects(handle)
            .into_iter()
            .filter(|&id| due.holds(id))
            .collect();
        let types = triggers.bound_types(handle);
        if !types.is_empty() {
            holders.extend(
                scans
                    .objects(self, rules, due)
                    .iter()
                    .filter(|(_, kind)| types.contains(kind))
                    .map(|&(id, _)| id),
            );
        }
        holders
            .into_iter()
            .filter(|id| self.entities().get(*id).is_some())
            .collect()
    }

    /// The live objects an object poll or an object trigger's `Timer`
    /// checks: those bound to `handle`, and every object of a type bound to
    /// it, in creation order.
    pub(super) fn module_poll_holders(&self, rules: &RuleSet, handle: u64) -> Vec<u64> {
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
    /// with `get` or `var_get`; `None` when a read is not `Ok`.
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
            let reader = host.reader();
            let get = |reference: i64, name: &str| match property_id(name)
                .map(|prop| reader.read(reference, prop))
            {
                Some(Ok((value, Status::Ok))) => Some(value),
                _ => None,
            };
            match def.kind {
                EVENT_PROPERTY_POLL => {
                    let reference = if holder == 0 { house } else { holder as i64 };
                    match reader.read(reference, def.id) {
                        Ok((value, Status::Ok)) => Some(value),
                        _ => None,
                    }
                }
                EVENT_HOUSE_POWER_POLL => {
                    Some(get(house, "PowerOutput")? - get(house, "PowerDrain")?)
                }
                // The module's own modifiers in group `id` (appendix 4.4).
                EVENT_MODIFIER_COUNT_POLL => Some(
                    host.sim
                        .entities()
                        .get(holder)?
                        .module_modifier_count(source, def.id) as i64,
                ),
                // VERA's reading: the subject's `X`, `Y` to the cell's
                // centre, by the shared lepton distance (Sqrt_Approx).
                EVENT_DISTANCE_POLL => {
                    let (x, y) = match decode_ref(def.value).ok()?? {
                        Ref::Cell { x, y } => (x, y),
                        Ref::Waypoint(number) => reader.waypoint_cell(number)?,
                        _ => return None,
                    };
                    let centre = |cell: u16| i64::from(cell) * 256 + 128;
                    let dx = centre(x) - get(holder as i64, "X")?;
                    let dy = centre(y) - get(holder as i64, "Y")?;
                    let (dx, dy) = (i32::try_from(dx).ok()?, i32::try_from(dy).ok()?);
                    Some(i64::from(crate::sim::cell_kernel::native_xy_distance(
                        dx, dy,
                    )))
                }
                EVENT_VARIABLE_POLL => {
                    let scope = match variable_scope(&host.sim.ext_modules.vars, def.id) {
                        Ok(VarScope::Object) => holder as i64,
                        Ok(VarScope::House) => house,
                        _ => 0,
                    };
                    match reader.read_var(scope, def.id) {
                        Ok((value, Status::Ok)) => Some(value),
                        _ => None,
                    }
                }
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

    /// `TransportEntered` on `occupant` and `PassengerAdded` on `building`
    /// once an occupier garrisoned it (appendix 4.3: `value` the occupants
    /// now inside, `value2` 1 for a garrison).
    pub(crate) fn raise_module_garrison_entered(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        building: u64,
        occupant: u64,
        occupants: i32,
    ) {
        for (kind, subject, other) in [
            (EVENT_TRANSPORT_ENTERED, occupant, building),
            (EVENT_PASSENGER_ADDED, building, occupant),
        ] {
            if self.trigger_runtime.module_listens(kind) {
                let event = ModuleEvent {
                    other,
                    value: i64::from(occupants),
                    value2: 1,
                    ..ModuleEvent::new(kind, subject)
                };
                self.raise_module_event(rules, frame_effects, event);
            }
        }
    }

    /// `TransportExited` on `occupant` and `PassengerRemoved` on `building`
    /// once an occupant left the garrison: ejected onto the map (raised
    /// after the ejection loop), or removed inside it (KillOccupants, an
    /// assault, Ares' fatal occupant hit; raised before the UnInit, so the
    /// occupant is still there to read). `value` the occupants left
    /// inside, `value2` 1 for a garrison, as on entry.
    pub(crate) fn raise_module_garrison_left(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        building: u64,
        occupant: u64,
        occupants: i32,
    ) {
        for (kind, subject, other) in [
            (EVENT_TRANSPORT_EXITED, occupant, building),
            (EVENT_PASSENGER_REMOVED, building, occupant),
        ] {
            if self.trigger_runtime.module_listens(kind) {
                let event = ModuleEvent {
                    other,
                    value: i64::from(occupants),
                    value2: 1,
                    ..ModuleEvent::new(kind, subject)
                };
                self.raise_module_event(rules, frame_effects, event);
            }
        }
    }

    /// `TransportExited` on `passenger` and `PassengerRemoved` on `carrier`
    /// after a cargo departure placed the passenger; `value` is the size it
    /// occupied, `value2` 0 (a transport, not a garrison).
    pub(crate) fn raise_module_passenger_removed(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        carrier: u64,
        passenger: u64,
        size: f64,
    ) {
        for (kind, subject, other) in [
            (EVENT_TRANSPORT_EXITED, passenger, carrier),
            (EVENT_PASSENGER_REMOVED, carrier, passenger),
        ] {
            if self.trigger_runtime.module_listens(kind) {
                let event = ModuleEvent {
                    other,
                    value: size as i64,
                    value2: 0,
                    ..ModuleEvent::new(kind, subject)
                };
                self.raise_module_event(rules, frame_effects, event);
            }
        }
    }

    /// `Detonated` once a projectile's detonation, its cluster loop
    /// included, is done (`BulletClass::Detonate` 0x00469020's commit,
    /// `combat::world_receiver::commit_projectile_detonations_inline`): on
    /// the source object (0 without one, which only weapon, warhead and
    /// projectile bindings reach), `other` the source, `id` the warhead,
    /// `cell` and `x`, `y`, `z` the impact. Only a fired projectile raises
    /// it (it reached its target, fused, collided, lost its target or ended
    /// its NUKE wait); a death weapon's or an animation's bare
    /// `DetonateAtCoord`, a Phobos AttachEffect shot and a crit warhead
    /// detonation reuse the detonation path without a projectile and raise
    /// nothing.
    pub(crate) fn raise_module_detonated(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        detonation: &crate::sim::projectile::ProjectileDetonation,
    ) {
        use crate::sim::projectile::ProjectileDetonationReason as Reason;
        if !self.trigger_runtime.module_listens(EVENT_DETONATED)
            || !matches!(
                detonation.reason,
                Reason::ReachedTarget
                    | Reason::Fuse
                    | Reason::Collision
                    | Reason::TargetExpired
                    | Reason::AnimEnded
            )
        {
            return;
        }
        let payload = &detonation.payload;
        let index = |found: Option<u32>| found.map_or(-1, |index| index as i32);
        let warhead = index(rules.warhead_index(self.interner.resolve(payload.warhead)));
        let projectile_type = index(
            payload
                .projectile_type()
                .and_then(|id| rules.projectile_index(self.interner.resolve(id))),
        );
        let weapon = index(
            payload
                .installed_weapon()
                .and_then(|id| rules.weapon_index(self.interner.resolve(id))),
        );
        let impact = detonation.impact;
        let cell = u16::try_from(impact.x.div_euclid(256))
            .ok()
            .zip(u16::try_from(impact.y.div_euclid(256)).ok());
        let event = ModuleEvent {
            other: detonation.source_id,
            projectile: detonation.projectile_id,
            cell,
            id: warhead,
            at: Some([impact.x, impact.y, impact.z]),
            weapon,
            warhead,
            projectile_type,
            ..ModuleEvent::new(EVENT_DETONATED, detonation.source_id)
        };
        self.raise_module_event(rules, frame_effects, event);
    }

    /// `Crushed` on `victim`, `other` its crusher and `x`, `y`, `z` its
    /// position, once the crush is decided and before the victim's kill is
    /// recorded (so it is still there to read; `FatalDamage` and `Killed`
    /// follow): the Unit's Crush_Or_Prescatter_Cell (`0x007416A0`), a
    /// Magnetron drop's landing crush and a Jumpjet's landing crush.
    pub(crate) fn raise_module_crushed(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        victim: u64,
        crusher: u64,
    ) {
        if !self.trigger_runtime.module_listens(EVENT_CRUSHED) {
            return;
        }
        let event = ModuleEvent {
            other: crusher,
            at_subject: true,
            ..ModuleEvent::new(EVENT_CRUSHED, victim)
        };
        self.raise_module_event(rules, frame_effects, event);
    }

    /// `HouseTransferred` on `object` after ChangeOwner moved it from
    /// `old_owner`; `value` 1 for a mind control.
    pub(crate) fn raise_module_house_transferred(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        object: u64,
        old_owner: InternedId,
        mind_control: bool,
    ) {
        if !self.trigger_runtime.module_listens(EVENT_HOUSE_TRANSFERRED) {
            return;
        }
        let event = ModuleEvent {
            house: self.houses.index_of(old_owner).map(|index| index as u32),
            value: i64::from(mind_control),
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
