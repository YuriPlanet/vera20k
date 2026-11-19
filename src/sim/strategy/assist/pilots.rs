//! Which behaviour a type runs, chosen by its capabilities.
//!
//! A pilot is a name, not a state machine yet: the table answers "what is this
//! unit for" from the capability tags (`rules::capabilities`), so the layers
//! below can be written one pilot at a time instead of one type at a time.
//! Today two of them are dispatched (crush, and the engage every armed unit
//! gets); the rest are named so the next slice has a place to land rather than
//! another `if` in a pass.

use crate::rules::capabilities::{Capabilities, Tag};
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;

/// What a type does when the brain has nothing more specific for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pilot {
    /// Flatten what it drives over: a tank beside a soldier.
    Crush,
    /// Put a passenger in the turret and choose the round (the IFV).
    Gunner,
    /// Carry infantry to where they are needed.
    Transport,
    /// Walk into a building and take it (the engineer).
    Capture,
    /// Walk into a `Spyable=` building and do what the spy does there.
    Infiltrate,
    /// Enter a civilian building and hold it.
    Garrison,
    /// Take the deployed pose and fire from it.
    Deploy,
    /// Enter a `UnitRepair=` shed and come back repaired.
    Repair,
    /// Jump onto a vehicle and eat it (the terror drone).
    Parasite,
    /// Take a vehicle whole (the mind-control unit): the target is the prize.
    Subdue,
    /// Walk onto ore, and back.
    Harvest,
    /// Shoot what is in reach, and go where it is told.
    Engage,
    /// Keep its distance from what out-ranges it while reloading.
    Kite,
    /// Nothing a unit pilot does: a structure, or a type the brain has no use
    /// for yet.
    Structure,
}

/// What a pilot goes for first when several hostiles are in reach.
///
/// The one thing a capability changes about an ordinary fight: what the unit
/// is worth against. A terror drone's jump eats a vehicle; an IFV without a
/// passenger carries an anti-aircraft rocket. Both are facts about the pilot,
/// so both are answered here rather than in the target loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Preference {
    /// Anything: the ordinary soldier's answer.
    Any,
    /// Vehicles and aircraft first: what a parasite eats.
    Armour,
    /// Aircraft first: the IFV's own rocket.
    Air,
}

impl Pilot {
    /// What this pilot prefers in reach.
    pub(super) fn preference(self) -> Preference {
        match self {
            Pilot::Parasite | Pilot::Subdue => Preference::Armour,
            Pilot::Gunner => Preference::Air,
            _ => Preference::Any,
        }
    }

    /// The pilot for a type, by the tags it carries. Order is the decision:
    /// a specialist outranks the ordinary fight, and the fight outranks the
    /// march.
    pub(super) fn of(rules: &RuleSet, ty: &ObjectType) -> Self {
        let caps = Capabilities::of(rules, ty);
        if ty.category == crate::rules::object_type::ObjectCategory::Building {
            return Self::Structure;
        }
        if caps.has(Tag::Harvester) {
            return Self::Harvest;
        }
        if caps.has(Tag::MindControl) {
            return Self::Subdue;
        }
        if caps.has(Tag::Infiltrator) && caps.has(Tag::Spy) {
            return Self::Infiltrate;
        }
        if caps.has(Tag::Engineer) || caps.has(Tag::Infiltrator) {
            return Self::Capture;
        }
        if caps.has(Tag::Gunner) {
            return Self::Gunner;
        }
        if caps.has(Tag::Transport) {
            return Self::Transport;
        }
        if caps.has(Tag::Parasite) {
            return Self::Parasite;
        }
        if caps.has(Tag::Deployer) && caps.has(Tag::DeployFire) {
            return Self::Deploy;
        }
        if caps.has(Tag::Crusher) {
            return Self::Crush;
        }
        // A soldier that can hold a building still fights: garrisoning is a
        // tactic the policy orders, not the unit's whole job. Only an unarmed
        // occupier is a garrison first.
        if caps.has(Tag::Armed) {
            return Self::Engage;
        }
        if caps.has(Tag::Occupier) {
            return Self::Garrison;
        }
        Self::Structure
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pilot(name: &str) -> Option<Pilot> {
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()?;
        let ty = rules.object(name)?;
        Some(Pilot::of(&rules, ty))
    }

    /// The table is read off the keys, so a mod's tank is a Crush without a
    /// name list here and a mod's spy is an Infiltrate the same way.
    #[test]
    fn the_pilot_follows_the_capabilities() {
        assert_eq!(pilot("MTNK"), Some(Pilot::Crush));
        assert_eq!(pilot("FV"), Some(Pilot::Gunner));
        assert_eq!(pilot("CMIN"), Some(Pilot::Harvest));
        assert_eq!(pilot("SPY"), Some(Pilot::Infiltrate));
        assert_eq!(pilot("ENGINEER"), Some(Pilot::Capture));
        assert_eq!(pilot("GGI"), Some(Pilot::Deploy));
        assert_eq!(pilot("E1"), Some(Pilot::Deploy));
        assert_eq!(pilot("ADOG"), Some(Pilot::Parasite));
        // A mind-control unit is its own pilot: the target is the prize, so
        // its preference is what it can take, not the nearest thing.
        assert_eq!(pilot("YURI"), Some(Pilot::Subdue));
        assert_eq!(Pilot::Subdue.preference(), Preference::Armour);
        assert_eq!(pilot("E2"), Some(Pilot::Engage));
        assert_eq!(pilot("DRON"), Some(Pilot::Parasite));
        assert_eq!(pilot("GADEPT"), Some(Pilot::Structure));
    }
}
