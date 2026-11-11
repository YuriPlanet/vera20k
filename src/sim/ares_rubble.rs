//! Optional Ares advanced rubble (`Rubble.*`, `rules::ares_building_type`).
//!
//! - A destroyed building whose type names `Rubble.Destroyed` (or sets
//!   `Rubble.Destroyed.Remove`) is switched at
//!   `BuildingClass::DestructionEffects`' hook `0x00441F12`
//!   (`BuildingClass_Destroy_RubbleYell @ 10017160`), after the NoSurvivor
//!   write and before SpawnSurvivors; once KillPassengers is done, the
//!   Foots standing on its foundation are kicked out (hook `0x00441F2C`,
//!   `BuildingClass_Destroy_KickOutOfRubble @ 10017130`).
//! - An engineer entering a friendly rubble whose type names `Rubble.Intact`
//!   (or sets `Rubble.Intact.Remove`) rebuilds it (`InfantryClass::
//!   PerCellProcess` hook `0x00519FAF`, `100257A0`); his action over it is
//!   the repair action (hook `0x0051E635`, `10025420`), in
//!   `Simulation::engineer_building_action`.
//!
//! Both directions share [`Simulation::ares_switch_rubble`]
//! (`BuildingTypeExt::SwitchRubble @ 1000F2B0`).

use crate::map::entities::EntityCategory;
use crate::rules::ares_building_type::RubbleSwitch;
use crate::rules::object_type::ObjectCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::destruction_effects::DeathAnimSpawn;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::world::{
    ConcealOutcome, FrameEffects, PlacementEvidence, Simulation, UninitContext,
};

impl Simulation {
    /// `BuildingTypeExt::SwitchRubble @ 1000F2B0`: takes `building_id` off the
    /// map (Remove, vt+0xD4) and destroys all its anims (`DestroyNthAnim(-2)`);
    /// unless `switch.remove`, constructs the switch's type for the house
    /// `HouseExt::GetHouseKind` picks (random allowed: one Scenario
    /// `RandomRanged(0, houses - 1)` for `Random`; default the building's
    /// owner), sets its health from `switch.strength` and Puts it at the
    /// building's Location and current facing with the placement scope
    /// (`IKnowWhatImDoing`) raised; then plays `switch.anim` at the
    /// building's GetCoords (delay 0, loop 1, flags 0x600). Returns the new
    /// building; a failed Put is not handled (the DLL returns the object
    /// anyway).
    pub(crate) fn ares_switch_rubble(
        &mut self,
        building_id: u64,
        switch: &RubbleSwitch,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Option<u64> {
        let building = self.substrate.entities.get(building_id)?;
        let owner = building.owner();
        let (rx, ry, z) = (
            building.position.rx(),
            building.position.ry(),
            building.position.z(),
        );
        let location = crate::sim::movement::ground_pose::position_world_coord(&building.position);
        let coords = crate::sim::movement::ground_pose::object_get_coords(
            building,
            self.resolved_terrain.as_ref(),
        );
        // `Facing.current().value256()` (`((raw >> 7) + 1) >> 1`).
        let facing = building.body_facing_dir(self.session.binary_frame());
        let context = UninitContext::new(rules, registry).with_effects(effects);
        let _ = self.techno_limbo_with_context(building_id, context);
        self.clear_all_building_anim_slots(building_id);

        let mut new = None;
        if !switch.remove {
            let random_house = (switch.owner == crate::rules::ares_warhead::HouseSelector::Random)
                .then(|| {
                    let order = self.houses.order();
                    let last = order.len() as i32 - 1;
                    let index = self.scenario_rng.next_range_i32_inclusive(0, last);
                    order.get(index as usize).copied()
                })
                .flatten();
            // A missing named house (no Civilian side, Special or Neutral)
            // would hand CreateObject a null owner in the DLL; VERA skips the
            // successor instead.
            let house = self.house_kind(
                switch.owner,
                Some(owner),
                None,
                None,
                None,
                rules,
                random_house,
            );
            let object = switch
                .building_type
                .as_deref()
                .and_then(|name| rules.object_in_category(ObjectCategory::Building, name));
            if let (Some(house), Some(object)) = (house, object) {
                let house_name = self.interner.resolve(house).to_owned();
                new = self.construct_resolved_object_limbo_at_height(
                    object,
                    &house_name,
                    rx,
                    ry,
                    facing,
                    z,
                    rules,
                    effects,
                );
            }
            if let (Some(new_id), Some(object)) = (new, object) {
                let full = object.strength;
                let strength = switch.strength;
                if let Some(entity) = self.substrate.entities.get_mut(new_id) {
                    if (-100..=-1).contains(&strength) {
                        // A percentage of the new type's Strength, at least 1.
                        entity
                            .health
                            .set((strength.wrapping_neg().wrapping_mul(full) / 100).max(1));
                    } else if strength > 0 {
                        entity.health.set(strength.min(full));
                    }
                }
                let _ = self.with_object_placement_scope(|sim| {
                    sim.reveal_constructed_object_at_coord_with_overlay_context(
                        new_id,
                        location,
                        facing,
                        PlacementEvidence::EvaluateMark,
                        rules,
                        registry,
                        effects,
                    )
                });
            }
        }
        if let Some(anim) = switch.anim.as_deref() {
            let type_id = self.interner.intern(anim);
            let _ = self.admit_death_anim(
                rules,
                type_id,
                DeathAnimSpawn::at(
                    crate::sim::anim_class::AnimWorldCoord {
                        x: coords.x,
                        y: coords.y,
                        z: coords.z,
                    },
                    0,
                ),
            );
        }
        new
    }

    /// `BuildingClass_Destroy_RubbleYell @ 10017160` (hook `0x00441F12`): a
    /// dying building whose type has `Rubble.Destroyed` or
    /// `Rubble.Destroyed.Remove` switches with the `Rubble.Destroyed*` keys.
    pub(crate) fn ares_destroy_into_rubble(
        &mut self,
        building_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(switch) = self.rubble_destroyed_switch(building_id, rules) else {
            return;
        };
        let _ = self.ares_switch_rubble(building_id, &switch, rules, registry, effects);
    }

    /// `BuildingClass_Destroy_KickOutOfRubble @ 10017130` (hook `0x00441F2C`)
    /// for the same types, then `BuildingExt::KickOutOfRubble @ 10012970`:
    /// every Foot in each foundation cell's content (the deck list over a
    /// bridge head) of the destroyed building that Remove takes off the map
    /// is kicked out by the destroyed building (`KickOutUnit`, vt+0x100,
    /// with its Location cell); a kicked-out one is selected again if it
    /// was, any other is UnInit.
    pub(crate) fn ares_kick_out_of_rubble(
        &mut self,
        building_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        if self.rubble_destroyed_switch(building_id, rules).is_none() {
            return;
        }
        let Some(building) = self.substrate.entities.get(building_id) else {
            return;
        };
        let Some(object) = self.object_type(building.type_ref(), rules) else {
            return;
        };
        let origin = (building.position.rx(), building.position.ry());
        let context = UninitContext::new(rules, registry).with_effects(effects);
        let mut removed = Vec::new();
        for cell in
            crate::sim::crew_survival::foundation_cells(origin.0, origin.1, &object.foundation)
        {
            let feet: Vec<u64> = self
                .native_cell_content(cell)
                .filter_map(|member| match member {
                    CellObjectMember::Entity(id) => Some(id),
                    CellObjectMember::Terrain(_) => None,
                })
                .filter(|&id| {
                    self.substrate.entities.get(id).is_some_and(|entity| {
                        matches!(
                            entity.category(),
                            EntityCategory::Infantry
                                | EntityCategory::Unit
                                | EntityCategory::Aircraft
                        )
                    })
                })
                .collect();
            for foot in feet {
                let selected = self
                    .substrate
                    .entities
                    .get(foot)
                    .is_some_and(|entity| entity.selection.is_selected());
                if self.techno_limbo_with_context(foot, context) == ConcealOutcome::Concealed {
                    removed.push((foot, selected));
                }
            }
        }
        let exit_cell = (origin.0 as i16, origin.1 as i16);
        for (foot, selected) in removed {
            let exit = crate::sim::production::exit_produced_object(
                self,
                rules,
                building_id,
                foot,
                exit_cell,
                registry,
                effects,
            );
            if exit == crate::sim::ai_base_building::BuildingExit::Placed {
                if selected && let Some(entity) = self.substrate.entities.get_mut(foot) {
                    entity.selection.select();
                }
            } else {
                self.uninit_with_context(foot, context);
            }
        }
    }

    /// `InfantryClass_UpdatePosition_EngineerRepairsFriendly @ 100257A0`
    /// (hook `0x00519FAF`) for a rubble with `Rubble.Intact` or
    /// `Rubble.Intact.Remove`: the rubble switches with the `Rubble.Intact*`
    /// keys and is UnInit. Without a new building the engineer is turned away
    /// (`0x00519FB9`, `false`); otherwise he is removed and the new building
    /// kicks him out from his cell, selected again if he was, and
    /// PerCellProcess returns (`0x0051A65D`, `true`). He is not consumed.
    pub(crate) fn ares_engineer_rebuilds_rubble(
        &mut self,
        engineer_id: u64,
        building_id: u64,
        switch: &RubbleSwitch,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let new = self.ares_switch_rubble(building_id, switch, rules, registry, effects);
        let context = UninitContext::new(rules, registry).with_effects(effects);
        self.uninit_with_context(building_id, context);
        let Some(new) = new else {
            return false;
        };
        let selected = self
            .substrate
            .entities
            .get(engineer_id)
            .is_some_and(|entity| entity.selection.is_selected());
        let _ = self.techno_limbo_with_context(engineer_id, context);
        // GetMapCoords after Remove: the cell of his retained Location.
        let Some(cell) = self
            .substrate
            .entities
            .get(engineer_id)
            .map(|entity| (entity.position.rx() as i16, entity.position.ry() as i16))
        else {
            return true;
        };
        // The DLL ignores KickOutUnit's result.
        let _ = crate::sim::production::exit_produced_object(
            self,
            rules,
            new,
            engineer_id,
            cell,
            registry,
            effects,
        );
        if selected && let Some(entity) = self.substrate.entities.get_mut(engineer_id) {
            entity.selection.select();
        }
        true
    }

    /// The `Rubble.Destroyed*` switch of a building's type, when it applies.
    fn rubble_destroyed_switch(&self, building_id: u64, rules: &RuleSet) -> Option<RubbleSwitch> {
        let building = self.substrate.entities.get(building_id)?;
        if building.category() != EntityCategory::Structure {
            return None;
        }
        let switch = &self
            .object_type(building.type_ref(), rules)?
            .building_type_ext()
            .rubble_destroyed;
        switch.applies().then(|| switch.clone())
    }
}

#[cfg(test)]
#[path = "ares_rubble_tests.rs"]
mod tests;
