//! The army, as one object: who is in it, where it stands, what it weighs.
//!
//! Every decision about what to attack and who holds the base used to build
//! its own list of fighters from the ledger — `advance` filtered the census
//! one way, `forces` another, `squads` a third — so "the army" meant three
//! slightly different things depending on who asked. Two people driving one
//! car is exactly that: the same noun, three definitions.
//!
//! This is the single definition. It holds references into the ledger, so it
//! is built per frame and thrown away; what it fixes is not the storage but
//! the answer.

use crate::rules::ruleset::RuleSet;
use crate::sim::control::observation::Observation;

use super::census::{Census, Unit};

/// The house's army this frame.
pub(super) struct Army<'a> {
    /// Every unit that can be given an order: the house's, alive, not in a
    /// building or a depot.
    pub(super) members: Vec<&'a Unit>,
    /// Where the army stands: the anchor a target is chosen from. The first
    /// member's cell, in the ledger's order, so the same frame gives the same
    /// answer twice.
    pub(super) anchor: (u16, u16),
}

impl<'a> Army<'a> {
    pub(super) fn of(census: &'a Census, obs: &Observation<'_>) -> Self {
        let members: Vec<&Unit> = census
            .combat_units()
            .filter(|unit| unit.controllable() && !unit.in_service())
            .collect();
        let anchor = members.first().map_or((0, 0), |unit| unit.cell);
        let _ = obs;
        Self { members, anchor }
    }

    /// The units that hold ground: every fighting object the brain may give a
    /// guard order to. Wider than [`Self::members`] on purpose — a spawned
    /// sub-unit or a unit the census cannot order still guards the base, and
    /// narrowing this set took the guards off it (measured: won at 23086
    /// became a timeout at 30000 when `forces` read the fist's members).
    ///
    /// Aircraft are not in it. A guard is a cell to stand on, and the
    /// `AirportBound=` aircraft has no cell to stand on: its `Guard` replaces
    /// the rearm-and-return mission the airfield owns, so the plane takes off
    /// with the racks it had, flies at the ground, turns around, and does it
    /// again — the wishlist's "飞机没有装完子弹就再次发动攻击", and the reason a
    /// house with a full airfield still lost its aircraft. What an aircraft may
    /// be given is [`super::advance`]'s `Attack`, which sends it out and lets
    /// the rearm win when it comes back.
    pub(super) fn guards(census: &'a Census) -> Vec<&'a Unit> {
        census
            .combat_units()
            .filter(|unit| !unit.hand && !unit.in_service())
            .filter(|unit| !unit.traits.aircraft)
            .collect()
    }

    pub(super) fn len(&self) -> usize {
        self.members.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}
