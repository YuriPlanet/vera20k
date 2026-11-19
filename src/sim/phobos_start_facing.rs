//! Phobos `StartFacing` / `StartFacing.Random`: a building's initial facing.
//!
//! `GetBuildingStartFacing` (`Ext/Building/Hooks.cpp:1491-1504`) reads the
//! BuildingType key, else `[General] BuildingStartFacing`. Random mode draws
//! `ScenarioClass::Random.RandomRanged(0, 255)` once and caches it in
//! `BuildingExt::ConstructionStartFacing` (constructed `-1`,
//! `Ext/Building/Body.h:31,50`).

use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

impl Simulation {
    /// `GetBuildingStartFacing`: the type's `StartFacing` (else `[General]`),
    /// or the cached one-time `RandomRanged(0, 255)` when random mode is on.
    /// `None` for anything that is not a BuildingType object.
    pub(crate) fn phobos_building_start_facing(&mut self, id: u64, rules: &RuleSet) -> Option<i32> {
        let (random, facing) = {
            let entity = self.substrate.entities.get(id)?;
            let object = self.object_type(entity.type_ref(), rules)?;
            if object.category != ObjectCategory::Building {
                return None;
            }
            (
                object
                    .phobos_start_facing_random
                    .unwrap_or(rules.phobos_globals.building_start_facing_random),
                object
                    .phobos_start_facing
                    .unwrap_or(rules.phobos_globals.building_start_facing),
            )
        };
        if !random {
            return Some(facing);
        }
        let cached = self
            .substrate
            .entities
            .get(id)?
            .phobos_construction_start_facing();
        if cached >= 0 {
            return Some(cached);
        }
        let drawn = self.scenario_rng.next_range_i32_inclusive(0, 255);
        self.substrate
            .entities
            .get_mut(id)?
            .set_phobos_construction_start_facing(drawn);
        Some(drawn)
    }

    /// The `0x6F6D9E` `TechnoClass_Unlimbo` hook
    /// (`Ext/Building/Hooks.cpp:1530-1543`): a building not created from a map
    /// file and not a LaserFence starts with the configured facing byte. The
    /// caller has already excluded map creation and fetched the type.
    pub(crate) fn apply_phobos_building_start_facing(&mut self, id: u64, rules: &RuleSet) {
        let Some(facing) = self.phobos_building_start_facing(id, rules) else {
            return;
        };
        let frame = self.session.binary_frame();
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.body_facing.snap(u16::from(facing as u8) << 8, frame);
        }
    }
}

#[cfg(all(test, tests_sim_p_s))]
#[path = "phobos_start_facing_tests.rs"]
mod tests;
