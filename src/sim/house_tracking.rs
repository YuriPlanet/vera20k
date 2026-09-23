//! A house's object counts, as its multiplayer defeat gate reads them.
//!
//! Native owner: `HouseClass`. Two independent sets of counters, each with its
//! own writers:
//! - Tracking (`HouseClass::Add_Tracking @ 0x004FF700`, `Remove_Tracking @
//!   0x004FF550`): added when the Techno is constructed (the class
//!   constructors and InitFromType, `0x007355EA`, `0x00517CD4`, `0x00414068`,
//!   `0x00442C62`), removed by its destructor at the pending-delete drain,
//!   moved by `TechnoClass::ChangeOwner` (`0x007015DE`, `0x007015E6`).
//!   `Insignificant=` and `DontScore=` types are never tracked. A building
//!   counts in `+0x2F0` unless it is a 1x1 undeployer (vtable `+0x80`,
//!   `0x00465D40`) or undeploys into a `ResourceGatherer=` (then `+0x2E8`,
//!   with the units); a Unit counts per type in `+0x5514`.
//! - On the map (`HouseClass::Added_To_Game @ 0x00502A80` from
//!   `TechnoClass::Unlimbo` `0x006F6D8F`, `Removed_From_Game @ 0x005025F0`
//!   from `TechnoClass::Limbo` `0x006F6BD1`, both from ChangeOwner
//!   `0x0070159D`/`0x0070178E`): per-type counters whose totals the normal
//!   game reads (`+0x5564` units, `+0x5578` infantry, `+0x558C` aircraft) and
//!   `+0x5550` per building type. `DontScore=` skips them, except that a
//!   Unit is added without the test (`0x00502CF9`) but removed with it, and
//!   an infantry survivor flagged `+0x6D9` is not added (`0x00502C3C`) but is
//!   removed.
//!
//! The gate (`HouseClass::Update @ 0x004F8E86..0x004F8F82`) reads only these:
//! a short game keeps a house alive while `+0x2F0 > 0` or its tracked
//! `BaseUnit=` types sum above zero; a normal game while `+0x2F0` plus the
//! on-map unit, infantry and aircraft totals and the on-map count of
//! `[AI] BuildRefinery=`'s third type is not zero. The other counters those
//! functions write (`+0x2E8`, `+0x2EC`, `+0x2F4`, `+0x2F8`, the owned-type
//! sets) have no reader in this mechanism and are not kept.
//!
//! Evidence: `tools/spatial_oracle/house_tracking.py` runs the original
//! Add_Tracking and Remove_Tracking (36 cases);
//! `tools/spatial_oracle/house_defeat_gate.py` runs the gate block with the
//! original counter readers (21 cases). Added_To_Game and Removed_From_Game
//! are read, not executed.
//!
//! RESIDUAL: the survivor flag `+0x6D9` is not written. SpawnSurvivors sets it
//! (`0x00443111..0x00443127`), before the survivor's Unlimbo, on a `Nominal=`
//! survivor (the Technician) of a building with Buildup art (Init_Managers sets
//! `+0x6E9` when the type has a Buildup shape, `0x00442CAA..0x00442CCF`); the
//! sale crew sets it after a successful Unlimbo (`0x0044A733..0x0044A747`;
//! VERA has no sale crew). Nothing clears it. Added_To_Game skips a flagged
//! infantry on every Unlimbo while Removed_From_Game still decrements it on
//! every Limbo, so each time a flagged Technician leaves the map (dies,
//! garrisons, boards) native `+0x5578` drops by one for good. Trigger: a
//! Technician survivor of an armed building (the 15% crew roll) leaving the
//! map in a normal (non-short) game. Effect: native's sum can reach zero while
//! objects stand (the house is defeated and they are blown up) or stay
//! negative when nothing is left (the house is never defeated); VERA does
//! neither.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::map::entities::EntityCategory;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;

/// The type facts the counters test, fixed when the Techno is constructed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrackingFacts {
    /// `ObjectType+0x232` Insignificant.
    pub insignificant: bool,
    /// A building tracked with the units: a 1x1 undeployer, or one that
    /// undeploys into a `ResourceGatherer=` (a deployed Slave Miner).
    pub unit_like_building: bool,
}

/// The counters the defeat gate reads (see the module doc).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HouseTracking {
    /// `HouseClass+0x2F0`.
    buildings: i32,
    /// `HouseClass+0x5514`, the tracked count of each UnitType.
    unit_types: BTreeMap<InternedId, i32>,
    /// `HouseClass+0x5564` total.
    active_units: i32,
    /// `HouseClass+0x5578` total.
    active_infantry: i32,
    /// `HouseClass+0x558C` total.
    active_aircraft: i32,
    /// `HouseClass+0x5550`, the on-map count of each BuildingType.
    active_building_types: BTreeMap<InternedId, i32>,
}

impl HouseTracking {
    /// `HouseClass::Add_Tracking @ 0x004FF700`.
    pub(crate) fn add_tracking(&mut self, entity: &GameEntity) {
        self.track(entity, 1);
    }

    /// `HouseClass::Remove_Tracking @ 0x004FF550`.
    pub(crate) fn remove_tracking(&mut self, entity: &GameEntity) {
        self.track(entity, -1);
    }

    fn track(&mut self, entity: &GameEntity, delta: i32) {
        let facts = entity.tracking_facts;
        if facts.insignificant || entity.dont_score {
            return;
        }
        match entity.category {
            EntityCategory::Unit => {
                let count = self.unit_types.entry(entity.type_ref()).or_default();
                *count = count.wrapping_add(delta);
            }
            EntityCategory::Structure if !facts.unit_like_building => {
                self.buildings = self.buildings.wrapping_add(delta);
            }
            _ => {}
        }
    }

    /// `HouseClass::Added_To_Game @ 0x00502A80`.
    pub(crate) fn added_to_game(&mut self, entity: &GameEntity) {
        let dont_score = entity.dont_score;
        match entity.category {
            // The Unit case (increment at `0x00502CF9`) has no DontScore test.
            EntityCategory::Unit => self.active_units = self.active_units.wrapping_add(1),
            EntityCategory::Aircraft if !dont_score => {
                self.active_aircraft = self.active_aircraft.wrapping_add(1);
            }
            EntityCategory::Structure if !dont_score => {
                let count = self
                    .active_building_types
                    .entry(entity.type_ref())
                    .or_default();
                *count = count.wrapping_add(1);
            }
            EntityCategory::Infantry if !dont_score => {
                self.active_infantry = self.active_infantry.wrapping_add(1);
            }
            _ => {}
        }
    }

    /// `HouseClass::Removed_From_Game @ 0x005025F0`.
    pub(crate) fn removed_from_game(&mut self, entity: &GameEntity) {
        if entity.dont_score {
            return;
        }
        match entity.category {
            EntityCategory::Unit => self.active_units = self.active_units.wrapping_sub(1),
            EntityCategory::Aircraft => {
                self.active_aircraft = self.active_aircraft.wrapping_sub(1);
            }
            EntityCategory::Structure => {
                let count = self
                    .active_building_types
                    .entry(entity.type_ref())
                    .or_default();
                *count = count.wrapping_sub(1);
            }
            EntityCategory::Infantry => {
                self.active_infantry = self.active_infantry.wrapping_sub(1);
            }
        }
    }

    /// The short game's test (`0x004F8EC6..0x004F8F1D`): alive while
    /// `+0x2F0 > 0` or the tracked counts of `BaseUnit=` entries 1, 2 and 0
    /// sum above zero.
    pub(crate) fn short_game_alive(&self, base_units: &[Option<InternedId>; 3]) -> bool {
        let tracked = |slot: usize| {
            base_units[slot].map_or(0, |unit| self.unit_types.get(&unit).copied().unwrap_or(0))
        };
        let base = tracked(1).wrapping_add(tracked(2)).wrapping_add(tracked(0));
        self.buildings > 0 || base > 0
    }

    /// The normal game's test (`0x004F8F21..0x004F8F77`): alive while
    /// `+0x2F0` plus the on-map totals and the on-map count of `[AI]
    /// BuildRefinery=`'s third type is not zero.
    pub(crate) fn normal_game_alive(&self, build_refinery_2: Option<InternedId>) -> bool {
        let refinery = build_refinery_2.map_or(0, |refinery| {
            self.active_building_types
                .get(&refinery)
                .copied()
                .unwrap_or(0)
        });
        self.buildings
            .wrapping_add(self.active_units)
            .wrapping_add(self.active_infantry)
            .wrapping_add(self.active_aircraft)
            .wrapping_add(refinery)
            != 0
    }
}

#[cfg(test)]
impl HouseTracking {
    /// `+0x2F0`.
    pub(crate) fn buildings_for_test(&self) -> i32 {
        self.buildings
    }

    /// Stand in for tracked buildings a fixture places without the lifecycle.
    pub(crate) fn set_buildings_for_test(&mut self, buildings: i32) {
        self.buildings = buildings;
    }

    /// The tracked Units of every type.
    pub(crate) fn units_for_test(&self) -> i32 {
        self.unit_types.values().sum()
    }

    /// Stand in for on-map units a fixture places without the lifecycle.
    pub(crate) fn set_active_units_for_test(&mut self, units: i32) {
        self.active_units = units;
    }

    /// The on-map unit, infantry and aircraft totals.
    pub(crate) fn active_for_test(&self) -> (i32, i32, i32) {
        (
            self.active_units,
            self.active_infantry,
            self.active_aircraft,
        )
    }

    /// The on-map count of one building type.
    pub(crate) fn active_building_count_for_test(&self, building: InternedId) -> i32 {
        self.active_building_types
            .get(&building)
            .copied()
            .unwrap_or(0)
    }

    /// Set every counter the gate reads.
    pub(crate) fn set_for_test(
        &mut self,
        buildings: i32,
        unit_types: &[(InternedId, i32)],
        active: (i32, i32, i32),
        active_building_types: &[(InternedId, i32)],
    ) {
        self.buildings = buildings;
        self.unit_types = unit_types.iter().copied().collect();
        (
            self.active_units,
            self.active_infantry,
            self.active_aircraft,
        ) = active;
        self.active_building_types = active_building_types.iter().copied().collect();
    }
}

#[cfg(test)]
#[path = "house_tracking_tests.rs"]
mod tests;
