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
}
