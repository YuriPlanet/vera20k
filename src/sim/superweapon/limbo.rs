//! Phobos `SWTypeExt::ApplyLimboDelivery` and `ApplyLimboKill`
//! (`Ext/SWType/FireSuperWeapon.cpp:86-262`).
//!
//! The two arms run from the shared `FireSuperWeaponExt` tail
//! (`:13-40`) before `SW.Next`: a launch creates the type's
//! `LimboDelivery.Types` buildings for the owner, then removes delivered
//! buildings of the houses its `LimboKill.AffectsHouse` accepts whose
//! `LimboID` matches `LimboKill.IDs`, at most each `LimboKill.Counts` entry.
//!
//! `LimboCreate` (`:87-170`) is a normal `BuildingClass::CreateObject(owner)`
//! that the hook makes alive and on-map without a real Limbo (so power,
//! factories and prerequisites see it), registers as a house gain, adds to
//! `HouseExt::OwnedLimboDeliveredBuildings` and to the separate limbo
//! tracking counter, and stamps `BuildingExt::LimboID`.
//!
//! `ApplyLimboKill` (`:203-262`) walks every house, removes the selected
//! buildings from that list, then for each: `RemoveFromLimboTracking`
//! (unless Insignificant/DontScore), `Stun()`, `Limbo()`,
//! `RegisterDestruction(nullptr)`, `UnInit()`.
//!
//! Ledger: the weighted selection draws one Scenario `RandomDouble` per roll
//! (see `super::chaining::weighted_rolls`); the lifecycle calls write their
//! own timers and the kill records its destruction.
//!
//! RESIDUALS:
//! - `LimboCreate` places the building at the constructor's cell (0, 0);
//!   native's `CreateObject` retains a building whose `Place` fails, while
//!   VERA's Unlimbo admission refuses it and the delivery is skipped.
//! - The house-transfer path does not move `OwnedLimboDeliveredBuildings` to
//!   the new owner; native's list is on the owning HouseExt and is not
//!   moved either, so a captured delivered building is left in the old
//!   house's list in both.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim/house_tracking, sim/world.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, Simulation, UninitContext};

/// `ApplyLimboDelivery(pHouse)` (`:172-201`).
pub(super) fn apply_limbo_delivery(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: &mut FrameEffects<'_>,
) {
    let Some(settings) = rules.phobos_super_weapon_limbo(&sw.id) else {
        return;
    };
    if settings.types().is_empty() {
        return;
    }
    let selected = if settings.random_weights().is_empty() {
        (0..settings.types().len()).collect()
    } else {
        super::chaining::weighted_rolls(
            &settings.roll_chances(),
            settings.random_weights(),
            settings.types().len(),
            &mut sim.scenario_rng,
        )
    };
    let ids = settings.ids();
    // `int id = -1` lives across the loop: an index without an IDs entry
    // keeps the previous id.
    let mut id = -1;
    for index in selected {
        if index < ids.len() {
            id = ids[index];
        }
        let name = settings.types()[index].clone();
        limbo_create(sim, rules, owner, &name, id, overlay_registry, effects);
    }
}

/// `LimboCreate(BuildingTypeClass*, HouseClass*, int ID)` (`:87-170`).
#[allow(clippy::too_many_arguments)]
fn limbo_create(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    type_name: &str,
    limbo_id: i32,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: &mut FrameEffects<'_>,
) {
    let Some(building) = rules.object_in_category(ObjectCategory::Building, type_name) else {
        return;
    };
    // The BuildLimit check counts the plain and the UndeploysInto type.
    if building.build_limit > 0 {
        let Some(type_id) = sim.interner.get(&building.id) else {
            return;
        };
        let Some(house) = sim.houses.get(&owner) else {
            return;
        };
        let mut owned = house
            .tracking()
            .active_count(EntityCategory::Structure, type_id);
        if let Some(undeploy) = building.undeploys_into.as_deref()
            && let Some(undeploy) = rules.object_in_category(ObjectCategory::Building, undeploy)
            && let Some(undeploy) = sim.interner.get(&undeploy.id)
        {
            owned = owned.wrapping_add(
                house
                    .tracking()
                    .active_count(EntityCategory::Structure, undeploy),
            );
        }
        if owned >= building.build_limit {
            return;
        }
    }
    let owner_name = sim.interner.resolve(owner).to_owned();
    // `BuildingClass::CreateObject(owner)` at the constructor's cell (0, 0).
    let z = sim.terrain_cell_level(0, 0).unwrap_or(0);
    let Some(id) = sim.spawn_object_at_height_with_overlay_context(
        &building.id,
        &owner_name,
        0,
        0,
        0,
        z,
        rules,
        overlay_registry,
    ) else {
        log::debug!(
            "LimboDelivery of \"{}\" for \"{owner_name}\" failed to place",
            building.id
        );
        return;
    };
    let Some((category, type_id, counts)) = sim.substrate.entities.get(id).map(|entity| {
        (
            entity.category(),
            entity.type_ref(),
            entity.tracking_facts().counts_toward_limbo_tracking(),
        )
    }) else {
        return;
    };
    if let Some(house) = sim.houses.get_mut(&owner) {
        house.add_limbo_delivered(id, limbo_id, category, type_id, counts);
        // `pOwner->RecheckTechTree = true` (`:120`).
        house.request_tech_recheck();
    }
    let _ = effects;
}

/// `ApplyLimboKill(pHouse)` (`:203-262`).
pub(super) fn apply_limbo_kill(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: &mut FrameEffects<'_>,
) {
    let Some(settings) = rules.phobos_super_weapon_limbo(&sw.id) else {
        return;
    };
    let ids = settings.kill_ids();
    if ids.is_empty() {
        return;
    }
    let mut kills: Vec<u64> = Vec::new();
    let houses: Vec<InternedId> = sim.houses.order().to_vec();
    for target in houses {
        if !crate::sim::combat::can_target_house(
            settings.kill_affects_house(),
            owner,
            target,
            &sim.houses,
            &sim.house_alliances,
            &sim.interner,
        ) {
            continue;
        }
        let mut removed_id: Vec<i32> = Vec::new();
        for (index, &id) in ids.iter().enumerate() {
            if removed_id.contains(&id) {
                continue;
            }
            let max = settings
                .kill_counts()
                .get(index)
                .copied()
                .unwrap_or(i32::MAX);
            let Some(house) = sim.houses.get_mut(&target) else {
                continue;
            };
            let taken = house.take_limbo_delivered(id, max);
            kills.extend(taken);
            removed_id.push(id);
        }
    }
    for id in kills {
        let Some((owner, category, type_id, counts)) = sim.substrate.entities.get(id).map(|e| {
            (
                e.owner(),
                e.category(),
                e.type_ref(),
                e.tracking_facts().counts_toward_limbo_tracking(),
            )
        }) else {
            continue;
        };
        // `RemoveFromLimboTracking` happens here because the building is not
        // truly in limbo (`:240-243`).
        if counts && let Some(house) = sim.houses.get_mut(&owner) {
            house.remove_limbo_delivered_tracking(category, type_id);
        }
        let mut context = UninitContext::new(rules).with_effects(effects);
        sim.stun(id, context.reborrow());
        let _ = sim.techno_limbo_with_context(id, context.reborrow());
        sim.record_kill_and_uninit(id, None, context);
    }
}

#[cfg(all(test, tests_sim_superweapon))]
#[path = "limbo_tests.rs"]
mod tests;
