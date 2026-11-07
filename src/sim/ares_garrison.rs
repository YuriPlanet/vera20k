//! Optional Ares urban combat and raidable garrisons
//! (`rules::ares_building_type`).
//!
//! - `CanBeOccupiedBy` and `Bunker.Raidable` admit occupiers
//!   (`BuildingClass_CanBeOccupied_SpecificOccupiers @ 10017060`, hook
//!   `0x00457D58`, in `passenger::can_dock_occupier_garrison`).
//! - An occupier entering a building of another house raids it
//!   (`InfantryClass_GarrisonBuilding_OccupierEntered @ 10017240`, hook
//!   `0x0052297F`): the building is the occupier's until it is empty again
//!   (`BuildingExt::ExtData::evalRaidStatus @ 100136C0`, after
//!   SellBuilding's ejection `0x004581CD`, KillOccupants `0x00458729` and a
//!   pass-through hit), and cannot be sold meanwhile (`10017200`, hook
//!   `0x004494D2`, in `production::can_sell_building`).
//! - A shot that snaps to a garrisoned building can pass through to one of
//!   its occupants (`BulletExt::ExtData::DamageOccupants @ 10019AC0`, from
//!   `BulletClass_Detonate @ 100536E0`, hook `0x0046920B`; see
//!   `combat::world_receiver`).
//!
//! RESIDUAL: the occupant's TechnoExt `GarrisonedIn` (`10017240` writes
//! it) is not kept; only Ares trenches read it.

use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, Simulation};

/// A building's `OwnerBeforeRaid` (BuildingExt `+0x8`): the house it
/// belonged to before an occupier of another house raided it. Only this
/// module writes it.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct RaidState {
    owner_before_raid: Option<InternedId>,
}

impl RaidState {
    /// Whether the building is raided (`OwnerBeforeRaid` set).
    pub(crate) fn raided(self) -> bool {
        self.owner_before_raid.is_some()
    }

    /// Folded only while the building is raided.
    pub(crate) fn hash_state(&self, hasher: &mut impl std::hash::Hasher) {
        if let Some(owner) = self.owner_before_raid {
            std::hash::Hash::hash(b"ares-owner-before-raid-v1", hasher);
            std::hash::Hash::hash(&owner, hasher);
        }
    }
}

impl Simulation {
    /// `InfantryClass_GarrisonBuilding_OccupierEntered @ 10017240` (hook
    /// `0x0052297F`, after the occupier joins the list, before the threat
    /// refresh): an occupier of `occupier_owner` entering a building of
    /// another house that is not already raided raids it, unless it is a
    /// civilian building (TechLevel -1) of a `MultiplayPassive=` house, which
    /// the original garrison reconciliation takes over. The building then
    /// changes to the occupier's house with the capture announcement
    /// (`SetOwningHouse(owner, true)`).
    pub(crate) fn ares_occupier_entered(
        &mut self,
        building_id: u64,
        occupier_owner: InternedId,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(building) = self.substrate.entities.get(building_id) else {
            return;
        };
        let owner = building.owner();
        if owner == occupier_owner || building.ares_raid.raided() {
            return;
        }
        let Some(object) = self.object_type(building.type_ref(), rules) else {
            return;
        };
        let civilian = self.object_tech_level(object) == -1
            && self
                .houses
                .get(&owner)
                .is_some_and(|house| house.multiplay_passive());
        if civilian {
            return;
        }
        if let Some(building) = self.substrate.entities.get_mut(building_id) {
            building.ares_raid.owner_before_raid = Some(owner);
        }
        self.announce_engineer_capture(building_id, occupier_owner, rules);
        self.change_owner_with_rules(building_id, occupier_owner, rules, registry, effects);
    }

    /// `BuildingExt::ExtData::evalRaidStatus @ 100136C0`: an empty raided
    /// building goes back to its house without an announcement
    /// (`SetOwningHouse(owner, false)`), unless that house is defeated; it
    /// is no longer raided either way.
    pub(crate) fn ares_eval_raid_status(
        &mut self,
        building_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(building) = self.substrate.entities.get(building_id) else {
            return;
        };
        let Some(owner) = building.ares_raid.owner_before_raid else {
            return;
        };
        if !building.passenger_role.occupants().is_empty() {
            return;
        }
        if self
            .houses
            .get(&owner)
            .is_some_and(|house| !house.is_defeated())
        {
            self.change_owner_with_rules(building_id, owner, rules, registry, effects);
        }
        if let Some(building) = self.substrate.entities.get_mut(building_id) {
            building.ares_raid.owner_before_raid = None;
        }
    }
}

/// `BuildingClass_CanBeOccupied_SpecificOccupiers @ 10017060` (hook
/// `0x00457D58`), past the original's type, mission, playfield and warp
/// tests: an `Occupier=` infantryman of a type the building's
/// `CanBeOccupiedBy` admits may enter when the building is not full and he
/// is eligible (not mind-controlled, and for a civilian TechLevel -1
/// building, not at red health), and the building is his house's, of a
/// `MultiplayPassive=` house (`passive`), or `Bunker.Raidable` and empty.
pub(crate) fn occupier_admitted(
    sim: &Simulation,
    passenger: &crate::sim::game_entity::GameEntity,
    building: &crate::sim::game_entity::GameEntity,
    passenger_obj: &crate::rules::object_type::ObjectType,
    building_obj: &crate::rules::object_type::ObjectType,
    occupants: i32,
    passive: bool,
    red_health: bool,
) -> bool {
    let ext = building_obj.building_type_ext();
    if !passenger_obj.occupier || !ext.can_be_occupied_by(&passenger_obj.id) {
        return false;
    }
    let full = occupants as u32 == building_obj.max_number_occupants;
    let ineligible = (sim.object_tech_level(building_obj) == -1 && red_health)
        || passenger.mind_control.is_mind_controlled();
    if full || ineligible {
        return false;
    }
    passenger.owner() == building.owner() || passive || (ext.bunker_raidable && occupants == 0)
}

#[cfg(test)]
#[path = "ares_garrison_tests.rs"]
mod tests;
