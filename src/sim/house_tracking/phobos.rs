//! Phobos HouseExt::CountOwnedPresentAndLimboed and its lifecycle hooks.
//! Source: https://github.com/Phobos-developers/Phobos/tree/661f8bcafbe82ec9592972277dbcfd57ddaddf83/src/Ext/House

use super::*;

/// TechnoExt::HasBeenPlacedOnMap, written by the Unlimbo hook at 0x006F6D85.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct PhobosPlacement {
    has_been_placed: bool,
}

/// Hooks.cpp's Enabled/IsBeingDeleted controls. Only synchronous loading and
/// UnInit change these; stable save/tick boundaries are enabled at depth zero.
#[derive(Debug, Clone)]
pub(crate) struct PhobosLimboControl {
    enabled: bool,
    deletion_depth: u32,
}

impl Default for PhobosLimboControl {
    fn default() -> Self {
        Self {
            enabled: true,
            deletion_depth: 0,
        }
    }
}

fn category_index(category: EntityCategory) -> usize {
    match category {
        EntityCategory::Unit => 0,
        EntityCategory::Infantry => 1,
        EntityCategory::Structure => 2,
        EntityCategory::Aircraft => 3,
    }
}

impl HouseTracking {
    /// HouseExt::CountOwnedPresentAndLimboed (Body.cpp): retain the native
    /// active-counter quirks, then add the distinct signed limbo count.
    pub(crate) fn count_owned_present_and_limboed(
        &self,
        category: EntityCategory,
        type_id: InternedId,
    ) -> i32 {
        self.active_count(category, type_id).wrapping_add(
            self.phobos_limbo_counts[category_index(category)]
                .get(&type_id)
                .copied()
                .unwrap_or(0),
        )
    }

    fn change_phobos_limbo_count(&mut self, entity: &GameEntity, delta: i32) {
        if entity.tracking_facts.insignificant || entity.tracking_facts.dont_score() {
            return;
        }
        let count = self.phobos_limbo_counts[category_index(entity.category())]
            .entry(entity.type_ref())
            .or_default();
        *count = count.wrapping_add(delta);
    }
}

impl Simulation {
    /// ScenarioClass_ReadINI_StartTracking @ 0x00687B18 seeds limboed Foot
    /// objects after the authored readers, ignoring discarded map Buildings.
    pub(crate) fn begin_phobos_map_objects(&mut self) {
        self.substrate.phobos_limbo_control.enabled = false;
        for house in self.houses.values_mut() {
            house.tracking_mut().phobos_limbo_counts = Default::default();
        }
    }

    pub(crate) fn finish_phobos_map_objects(&mut self) {
        for entity in self.substrate.entities.values_sorted() {
            if entity.category().is_foot()
                && entity.lifecycle.in_limbo()
                && let Some(house) = self.houses.get_mut(&entity.owner())
            {
                house.tracking_mut().change_phobos_limbo_count(entity, 1);
            }
        }
        self.substrate.phobos_limbo_control.enabled = true;
    }

    /// TechnoClass_Unlimbo_RemoveTracking @ 0x006F6D85 precedes Added_To_Game.
    /// First placement never subtracts: construction alone added no limbo count.
    pub(crate) fn phobos_tracking_before_unlimbo(&mut self, id: u64) {
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            if self.substrate.phobos_limbo_control.enabled
                && entity.phobos_placement.has_been_placed
                && let Some(house) = self.houses.get_mut(&entity.owner())
            {
                house.tracking_mut().change_phobos_limbo_count(entity, -1);
            }
            entity.phobos_placement.has_been_placed = true;
        }
    }

    /// TechnoClass_Limbo_AddTracking @ 0x006F6BC9 precedes Removed_From_Game.
    /// UnInit's nested Limbo calls must not turn deleted objects into presence.
    pub(crate) fn phobos_tracking_before_limbo(&mut self, id: u64) {
        let control = &self.substrate.phobos_limbo_control;
        if control.enabled
            && control.deletion_depth == 0
            && let Some(entity) = self.substrate.entities.get(id)
            && let Some(house) = self.houses.get_mut(&entity.owner())
        {
            house.tracking_mut().change_phobos_limbo_count(entity, 1);
        }
    }

    /// TechnoClass_UnInit_Wrapper at Foot call 0x004DE60B / Building vt 0x007E3FB4.
    /// Like Phobos, decrement a limboed object even if it was never placed;
    /// signed counter history must not be rebuilt from the surviving objects.
    pub(crate) fn phobos_tracking_begin_uninit(&mut self, id: u64) {
        if self.substrate.phobos_limbo_control.enabled
            && let Some(entity) = self.substrate.entities.get(id)
            && entity.lifecycle.in_limbo()
            && let Some(house) = self.houses.get_mut(&entity.owner())
        {
            house.tracking_mut().change_phobos_limbo_count(entity, -1);
        }
        self.substrate.phobos_limbo_control.deletion_depth += 1;
    }

    pub(crate) fn phobos_tracking_end_uninit(&mut self) {
        self.substrate.phobos_limbo_control.deletion_depth -= 1;
    }

    /// TechnoClass_Captured_UpdateTracking @ 0x007015C9 transfers limbo counts
    /// while the old owner is still installed, before stock Remove/Add_Tracking.
    pub(crate) fn phobos_tracking_change_owner(&mut self, id: u64, new_owner: InternedId) {
        if self.substrate.phobos_limbo_control.enabled
            && let Some(entity) = self.substrate.entities.get(id)
            && entity.lifecycle.in_limbo()
        {
            if let Some(house) = self.houses.get_mut(&entity.owner()) {
                house.tracking_mut().change_phobos_limbo_count(entity, -1);
            }
            if let Some(house) = self.houses.get_mut(&new_owner) {
                house.tracking_mut().change_phobos_limbo_count(entity, 1);
            }
        }
        // The same 0x007015C9 hook requests both Foot houses independently
        // of limbo and Ares prerequisite-alternate membership.
        if let Some(entity) = self.substrate.entities.get(id)
            && entity.category().is_foot()
        {
            let old_owner = entity.owner();
            self.houses.request_house_tech_tree_recheck(old_owner);
            self.houses.request_house_tech_tree_recheck(new_owner);
        }
    }

    /// Phobos `HouseExt::ActiveHarvesterCount` and `TotalHarvesterCount`
    /// (`Ext/House/Body.cpp:278-303`), the two numbers the sidebar harvester
    /// counter draws. Native walks a house vector of every Techno whose type is
    /// `Harvester.Counted=`; VERA scans the store for the same pair, so spawn,
    /// death, capture and `TypeFrom=` conversion need no membership bookkeeping.
    /// Total adds `TechnoExt::HasBeenPlacedOnMap` — construction alone does not
    /// count, but a placed techno still counts from inside a transport. Active
    /// adds the types [`Self::techno_is_harvesting`] answers for.
    pub fn harvester_counts(
        &self,
        rules: &crate::rules::ruleset::RuleSet,
        owner: InternedId,
    ) -> (i32, i32) {
        let mut active = 0;
        let mut total = 0;
        for entity in self.substrate.entities.values() {
            if entity.owner() != owner || !entity.phobos_placement.has_been_placed {
                continue;
            }
            let Some(object) = rules.object(self.interner().resolve(entity.type_ref())) else {
                continue;
            };
            if !object.harvester_counted() {
                continue;
            }
            total += 1;
            if self.techno_is_harvesting(entity, object) {
                active += 1;
            }
        }
        (active, total)
    }

    /// Phobos `TechnoExt::IsHarvesting` (`Ext/Techno/Body.cpp:78-129`), with its
    /// `HasAvailableDock` (`:131-140`) and `HasRadioLinkWithDock` (`:142-153`)
    /// helpers. `object` is the entity's own type, already resolved.
    fn techno_is_harvesting(
        &self,
        entity: &GameEntity,
        object: &crate::rules::object_type::ObjectType,
    ) -> bool {
        // TechnoExt::IsActive (Body.cpp:65-76).
        if !entity.lifecycle.object_alive()
            || entity.health.current() <= 0
            || entity.lifecycle.in_limbo()
            || entity.is_warped_out()
            || entity.deactivated.is_offline()
            || entity.emp_remaining() > 0
        {
            return false;
        }
        // A slave manager that is scanning, travelling or returning is out
        // gathering, which Phobos counts as working (Body.cpp:83-86).
        if entity.slave_manager.as_ref().is_some_and(|manager| {
            manager.state() != crate::sim::slave_manager::ManagerState::Ready
        }) {
            return true;
        }
        // A counted building — the oil derrick the key documents — works while it
        // is powered online, which is `BuildingClass::IsPowerOnline`'s latch
        // (Body.cpp:88-89).
        if entity.category() == EntityCategory::Structure {
            return entity.building_online();
        }
        // HasAvailableDock: one of this type's `Dock=` buildings is tracked for
        // the owner (Body.cpp:131-140, the same `+0x5500` read as the Harvest
        // preamble at 0x0073E69B).
        let docks: Vec<InternedId> = object
            .dock
            .iter()
            .filter_map(|name| self.interner().get(name))
            .collect();
        if docks.is_empty()
            || !self
                .houses
                .get(&entity.owner())
                .is_some_and(|house| house.tracking().owns_any_building(docks.iter().copied()))
        {
            return false;
        }
        use crate::rules::mission_data::MissionType;
        match entity.mission.current().known() {
            Some(MissionType::Harvest) => {
                // A unit holding any link that is not radio-linked to a dock is
                // still sitting in its factory, and a purposeless unit idles even
                // on Harvest (Body.cpp:96-102).
                if !entity.radio_contacts.is_empty()
                    && !self.has_radio_link_with_dock(entity, object)
                {
                    return false;
                }
                !entity.is_useless()
            }
            Some(MissionType::Unload) => true,
            Some(MissionType::Enter) => {
                // Entering something that is not a building is an unload elsewhere,
                // not a dock visit (Body.cpp:107-115).
                entity
                    .tether
                    .partner()
                    .and_then(|link| self.substrate.entities.get(link))
                    .is_none_or(|link| link.category() == EntityCategory::Structure)
            }
            Some(MissionType::Guard) => {
                // Parked on Guard but waiting for a free dock: an archive target,
                // some cargo and a moving locomotor (Body.cpp:116-122).
                entity.archive_target().is_some()
                    && !entity.storage_bales().is_empty()
                    && crate::sim::movement::motion_query::is_moving(entity).unwrap_or(false)
            }
            _ => false,
        }
    }

    /// `TechnoExt::HasRadioLinkWithDock` (Body.cpp:142-153): the first link is a
    /// Building whose type is on this harvester's `Dock=` list.
    fn has_radio_link_with_dock(
        &self,
        entity: &GameEntity,
        object: &crate::rules::object_type::ObjectType,
    ) -> bool {
        entity
            .tether
            .partner()
            .and_then(|link| self.substrate.entities.get(link))
            .is_some_and(|link| {
                link.category() == EntityCategory::Structure
                    && object.dock.iter().any(|name| {
                        self.interner()
                            .get(name)
                            .is_some_and(|type_id| link.type_ref() == type_id)
                    })
            })
    }
}
