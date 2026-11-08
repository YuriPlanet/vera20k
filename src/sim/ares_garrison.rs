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
//! - A selected trench's occupants move into a neighbouring trench of the
//!   same `IsTrench=` kind (#666): the Enter cursor and click over it
//!   (`100171B0`, hook `0x44725F`; `10016F60`, hook `0x443414`) raise the
//!   TrenchRedirectClick event (0x60, `1006A170`), whose handler
//!   (Networking_RespondToEvent `1006A540`) moves them
//!   (`BuildingExt::ExtData::canTraverseTo @ 10011690`,
//!   `doTraverseTo @ 100130E0`).
//!
//! RESIDUAL: the occupant's TechnoExt `GarrisonedIn` (`10017240` and
//! doTraverseTo write it) is not kept; nothing in 3.0 reads it. VERA's
//! occupant carrier link (`PassengerRole::set_inside_transport`) follows
//! the occupant instead.

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

impl Simulation {
    /// `BuildingExt::ExtData::canTraverseTo @ 10011690`: the occupants of
    /// `source_id` may move into `target_id` when it is another building
    /// whose type is `CanBeOccupied=` (`+0x157B`) with fewer occupants than
    /// `MaxNumberOccupants=` (`+0x1580`), the source holds any, both types
    /// are trenches of one kind (sameTrench), and their Locations lie at
    /// most 256 leptons apart (a straight neighbour for 1x1 pieces). Ares
    /// tests no owner.
    pub fn ares_can_traverse_to(&self, source_id: u64, target_id: u64, rules: &RuleSet) -> bool {
        if source_id == target_id {
            return false;
        }
        let entities = &self.substrate.entities;
        let (Some(source), Some(target)) = (entities.get(source_id), entities.get(target_id))
        else {
            return false;
        };
        let structure = crate::map::entities::EntityCategory::Structure;
        if source.category() != structure || target.category() != structure {
            return false;
        }
        let (Some(source_type), Some(target_type)) = (
            self.object_type(source.type_ref(), rules),
            self.object_type(target.type_ref(), rules),
        ) else {
            return false;
        };
        if i64::from(target.passenger_role.occupants().count())
            >= i64::from(target_type.max_number_occupants)
            || source.passenger_role.occupants().is_empty()
            || !target_type.can_be_occupied
            || !source_type
                .building_type_ext()
                .same_trench(target_type.building_type_ext())
        {
            return false;
        }
        // CoordStruct::DistanceFrom (sqrt of the squared sum) <= 256.0: the
        // integer square compares the same.
        let terrain = self.resolved_terrain.as_ref();
        let from = crate::sim::movement::ground_pose::object_location(source, terrain);
        let to = crate::sim::movement::ground_pose::object_location(target, terrain);
        let [dx, dy, dz] = [to.x - from.x, to.y - from.y, to.z - from.z].map(i64::from);
        dx * dx + dy * dy + dz * dz <= 256 * 256
    }

    /// `BuildingExt::ExtData::doTraverseTo @ 100130E0`, past
    /// [`Self::ares_can_traverse_to`]: `min(free slots, occupants)` heads of
    /// `source_id`'s occupants move, in order, to the end of `target_id`'s;
    /// the source's firing index restarts at 0 once it reaches the count,
    /// and an emptied raided source goes back to its house (evalRaidStatus
    /// `100136C0`, inlined). Nothing is announced or refreshed: native
    /// leaves the cell threat (`0x0070F6E0`) and the target's raid state as
    /// they were.
    pub(crate) fn ares_do_traverse_to(
        &mut self,
        source_id: u64,
        target_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(target) = self.substrate.entities.get(target_id) else {
            return;
        };
        let Some(max) = self
            .object_type(target.type_ref(), rules)
            .map(|object| i64::from(object.max_number_occupants))
        else {
            return;
        };
        let free = max - i64::from(target.passenger_role.occupants().count());
        let held = self.substrate.entities.get(source_id).map_or(0, |source| {
            i64::from(source.passenger_role.occupants().count())
        });
        if free >= 0 {
            for _ in 0..free.min(held) {
                let Some(occupant) = self
                    .substrate
                    .entities
                    .get_mut(source_id)
                    .and_then(|source| source.passenger_role.occupants_mut().remove_head())
                else {
                    break;
                };
                if let Some(target) = self.substrate.entities.get_mut(target_id) {
                    target.passenger_role.occupants_mut().add(occupant);
                }
                if let Some(occupant) = self.substrate.entities.get_mut(occupant) {
                    occupant
                        .passenger_role
                        .set_inside_transport(target_id, false);
                }
            }
        }
        if let Some(source) = self.substrate.entities.get_mut(source_id) {
            let occupants = source.passenger_role.occupants_mut();
            if occupants.fire_index() >= occupants.count() {
                occupants.reset_fire_index();
            }
        }
        self.ares_eval_raid_status(source_id, rules, registry, effects);
    }

    /// The TrenchRedirectClick (0x60) arm of Networking_RespondToEvent
    /// (`1006A540`): the event's Cell (`TargetClass::As_Cell`) and source
    /// Building (`As_Building`), then the building on that Cell
    /// (`CellClass::GetBuilding`); canTraverseTo is asked again before
    /// doTraverseTo. The event's House is not consulted. Answers whether
    /// the occupants moved.
    pub(crate) fn ares_respond_to_trench_redirect(
        &mut self,
        event: &crate::sim::command::AresTrenchRedirectRecord,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        use crate::sim::command::MegaMissionTarget;
        let (MegaMissionTarget::Cell { x, y }, MegaMissionTarget::Object { id }) =
            (event.target, event.source)
        else {
            return false;
        };
        let (Ok(rx), Ok(ry), Ok(source_id)) =
            (u16::try_from(x), u16::try_from(y), u64::try_from(id))
        else {
            return false;
        };
        let Some(target_id) = self.substrate.occupancy.first_building_on_layer(
            rx,
            ry,
            crate::sim::movement::locomotor::MovementLayer::Ground,
        ) else {
            return false;
        };
        if !self.ares_can_traverse_to(source_id, target_id, rules) {
            return false;
        }
        self.ares_do_traverse_to(source_id, target_id, rules, registry, effects);
        true
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
