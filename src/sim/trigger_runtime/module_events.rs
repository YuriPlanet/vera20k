//! Stock events 0..61 for module triggers (RFC 0001 appendix 4.1): which of
//! them reach module triggers, and their predicates, read through
//! HasOccurred71E940's own House, waypoint and variable owners
//! ([`Simulation::house_event_holds`], [`Simulation::near_waypoint`],
//! [`super::TriggerRuntime::evaluate_event`]).
//!
//! A module event's `value` names a house by its array index (−1 for any
//! where the table allows it), not by Country as map events do; everything
//! else reads as the map event would. Delivered (R) events reach module
//! triggers at the Tag funnels map Tags use; House (H) and Logic (L) events
//! are polled in the module phase, H every eighth frame and L every frame,
//! as appendix 4 says. Stock events keep their stock repeat behavior: a
//! repeating trigger fires on every check its condition holds.

use vera20k_wasm_ext::triggers::EventDef;

use crate::map::events::EventCondition;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

/// How a stock event reaches module triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StockClass {
    /// Delivered on a bound object through the object Tag funnel.
    Object,
    /// Delivered on a bound cell by the foot's per-cell Tag raise.
    Cell,
    /// A House condition, checked every eighth frame.
    House,
    /// A logic condition, checked every frame.
    Logic,
}

/// The stock events module triggers take, with their class. The others
/// answer `Unsupported` at `trigger_create`; the module's documentation
/// lists them and why.
pub(crate) const fn stock_class(kind: i32) -> Option<StockClass> {
    use StockClass::*;
    Some(match kind {
        1 | 4 | 6 | 7 | 29 | 34 | 35 | 38..=44 | 48 | 49 => Object,
        25 | 26 => Cell,
        5 | 8..=12 | 15..=22 | 30 | 32 | 52 | 55..=58 => House,
        13 | 14 | 27 | 28 | 36 | 37 | 45..=47 | 60 | 61 => Logic,
        _ => return None,
    })
}

/// Stock events whose `value` is a house array index, read as the house the
/// condition is about (appendix 4.1 "value house").
pub(crate) const fn value_names_house(kind: i32) -> bool {
    matches!(kind, 5 | 9 | 10 | 11 | 30 | 44 | 55 | 56 | 58)
}

/// Stock events whose `value` is a house index or −1 for any, compared with
/// the acting object's house, which the record retains (1, 25, 26).
pub(crate) const fn value_filters_actor_house(kind: i32) -> bool {
    matches!(kind, 1 | 25 | 26)
}

/// Whether a polled stock event is checked on `frame`: House Tags poll on
/// signed `Frame % 8 == 0` (HouseClass::Update `0x004F8F87`), Logic Tags
/// every frame (Logic55AFB0).
pub(crate) fn stock_poll_due(kind: i32, frame: u32) -> bool {
    match stock_class(kind) {
        Some(StockClass::House) => (frame as i32) % 8 == 0,
        Some(StockClass::Logic) => true,
        _ => false,
    }
}

/// What a stock event reads for one module trigger firing.
pub(crate) struct StockReading<'a> {
    pub(crate) def: &'a EventDef,
    /// The event's subject and other object (creation serials, 0 for none).
    pub(crate) subject: u64,
    pub(crate) other: u64,
    /// The trigger's house for this firing (array index).
    pub(crate) trigger_house: Option<u32>,
    /// The frame polling counts from (`ElapsedTime`).
    pub(crate) poll_start: u32,
    /// TechnoType `def.id`'s name, for 60 and 61.
    pub(crate) type_name: Option<&'a str>,
}

impl Simulation {
    /// Whether stock event `reading.def` holds, and the house the record
    /// carries for it (`Hs` in appendix 4.1; `None` keeps the trigger's
    /// house). The caller has matched the raised kind.
    pub(crate) fn module_stock_event_holds(
        &self,
        reading: &StockReading<'_>,
        rules: &RuleSet,
    ) -> (bool, Option<u32>) {
        let def = reading.def;
        let (kind, value) = (def.kind, i32::try_from(def.value).unwrap_or(i32::MIN));
        let house_of = |object: u64| {
            self.substrate
                .entities
                .get(object)
                .and_then(|entity| self.houses.index_of(entity.owner()))
                .map(|index| index as u32)
        };
        let house_at = |index: i32| {
            usize::try_from(index)
                .ok()
                .and_then(|index| self.houses.order().get(index).copied())
        };
        if value_filters_actor_house(kind) {
            let actor = house_of(reading.other);
            let admitted = actor.is_some() && (value == -1 || actor == u32::try_from(value).ok());
            return (admitted, actor);
        }
        match kind {
            // HasOccurred71EE57: the source Techno's House at predicate time.
            44 => (
                house_of(reading.other).is_some_and(|index| Ok(index) == u32::try_from(value)),
                None,
            ),
            34 => (self.near_waypoint(value, reading.subject), None),
            8 => (true, reading.trigger_house),
            _ if stock_class(kind) == Some(StockClass::Object) => (true, None),
            _ if stock_class(kind) == Some(StockClass::House) => {
                let index = if value_names_house(kind) {
                    u32::try_from(value).ok()
                } else {
                    reading.trigger_house
                };
                let holds = index
                    .and_then(|index| house_at(index as i32))
                    .is_some_and(|owner| self.house_event_holds(kind, value, owner, rules));
                (holds, index)
            }
            // Event 13 counts from the trigger's poll start, as a map
            // trigger's timer counts from its own start (Value seconds of
            // 15 frames, `0x0071E9E6`).
            13 => {
                let frame = self.session.binary_frame();
                let interval = value.max(0).saturating_mul(15).max(1) as u32;
                let due =
                    frame > reading.poll_start && (frame - reading.poll_start) % interval == 0;
                (due, None)
            }
            14 => (self.session.scenario_timer_elapsed(), None),
            // HasOccurred71EAE7/71EB06.
            45 => (self.session.lighting().current_ambient() <= value, None),
            46 => (self.session.lighting().current_ambient() >= value, None),
            27 | 28 | 36 | 37 | 47 | 60 | 61 => {
                let condition = EventCondition {
                    kind,
                    value,
                    type_name: reading.type_name.map(str::to_owned),
                    team_name: None,
                };
                let holds = self.trigger_runtime.evaluate_event(
                    &condition,
                    self.session.binary_frame(),
                    self,
                    rules,
                );
                (holds, None)
            }
            _ => (false, None),
        }
    }
}
