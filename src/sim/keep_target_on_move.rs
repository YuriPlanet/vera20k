//! Phobos `KeepTargetOnMove.*` consumers.
//!
//! - `EventClass_Execute_MegaMission_MoveCommand @ 0x4C7462`
//!   (`Ext/Techno/Hooks.Misc.cpp:1036-1075`): a Move order to a unit whose
//!   current target is close enough to attack keeps the target, stores the
//!   order destination and skips the shared target/destination setters.
//! - `UnitExt::UpdateKeepTargetOnMove` (`Ext/Unit/Body.cpp:94-144`), called
//!   from `UnitClass_AI @ 0x736480`: the latch is re-checked each Unit AI
//!   pass; a Guard unit with `NoMorePursuit=no` queues Attack, and a target no
//!   longer within the weapon's range plus `ExtraDistance` is dropped.
//!
//! `IsCloseEnoughToAttack` / `IsCloseEnough` reuse the authoritative InRange
//! body ([`crate::sim::combat::in_range::in_range_with_weapon_extra`]) with
//! the temporary range extension native applies to the weapon's `Range`.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/world, sim/combat, rules/.

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::line_of_fire::LineOfFireInputs;
use crate::sim::combat::{TargetKind, in_range};
use crate::sim::mission::MissionId;
use crate::sim::mission::MissionType;
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::world::{FrameEffects, Simulation};

/// `TechnoClass::IsCloseEnoughToAttack` (or `IsCloseEnough(target, slot)` for
/// an explicit slot) with `KeepTargetOnMove.ExtraDistance` folded into the
/// selected weapon's range. The map path uses the shared InRange owner; a
/// map-less fixture keeps the native refusal (no cells to measure).
fn in_range_with(
    sim: &Simulation,
    id: u64,
    target: &TargetKind,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    weapon_index: i32,
    extra_distance: i32,
) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    let Some(obj) = sim.interner.object_type(entity.type_ref(), rules) else {
        return false;
    };
    let Some(terrain) = sim.resolved_terrain.as_ref() else {
        return false;
    };
    let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
    let _ = obj;
    let los = LineOfFireInputs {
        overlay_grid: sim.overlay_grid.as_ref(),
        overlay_registry: registry,
        alliances: Some(&sim.house_alliances),
        buildings: Some(crate::sim::combat::line_of_fire::CellBuildings::of(sim)),
    };
    in_range::in_range_with_weapon_extra(
        sim,
        rules,
        &sim.interner,
        id,
        target,
        weapon_index,
        extra_distance,
        &cells,
        Some(&sim.house_alliances),
        &los,
    )
}

/// `TechnoClass::IsCloseEnough` (vt+0x3A8) at one explicit weapon slot with
/// no temporary extension: `TechnoClass_SetTarget_After`'s range test before
/// resetting a turretless Unit's firing sequence.
pub(crate) fn is_close_enough_with_weapon(
    sim: &Simulation,
    id: u64,
    target: &TargetKind,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    weapon_index: i32,
) -> bool {
    in_range_with(sim, id, target, rules, registry, weapon_index, 0)
}
/// `IsCloseEnoughToAttack @ 0x6F7780` (`CanFireAtTarget`): the selected
/// weapon with no key overrides. The Move-order hook uses this plain form.
fn close_enough_to_attack(
    sim: &Simulation,
    id: u64,
    target: &TargetKind,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> bool {
    in_range_with(sim, id, target, rules, registry, -1, 0)
}

/// `IsCloseEnough` (vt+0x3A8) with `KeepTargetOnMove.Weapon` and the
/// temporary `ExtraDistance`. The Unit-AI update uses this form.
fn close_enough_with_key(
    sim: &Simulation,
    id: u64,
    target: &TargetKind,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    let Some(obj) = sim.interner.object_type(entity.type_ref(), rules) else {
        return false;
    };
    let cfg = obj.phobos_keep_target;
    in_range_with(
        sim,
        id,
        target,
        rules,
        registry,
        cfg.weapon,
        cfg.extra_distance,
    )
}

/// `EventClass_Execute_MegaMission_MoveCommand`'s keep arm: latches the unit
/// and reports whether the caller must keep the target and skip the shared
/// setters.
pub(crate) fn move_order_keeps_target(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    if entity.category() != crate::map::entities::EntityCategory::Unit {
        return false;
    }
    let Some(obj) = sim.interner.object_type(entity.type_ref(), rules) else {
        return false;
    };
    if !obj.phobos_keep_target.enabled {
        return false;
    }
    let Some(target) = entity.attack_target.target() else {
        return false;
    };
    if !close_enough_to_attack(sim, id, &target, rules, registry) {
        return false;
    }
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.set_phobos_keep_target_on_move(true);
    }
    true
}

/// `UnitExt::UpdateKeepTargetOnMove` at `0x736480`, before
/// `DepletedAmmoActions`.
pub(crate) fn update_keep_target_on_move(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: &mut FrameEffects<'_>,
) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if entity.category() != crate::map::entities::EntityCategory::Unit
        || !entity.phobos_keep_target_on_move()
    {
        return;
    }
    let Some(target) = entity.attack_target.target() else {
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.set_phobos_keep_target_on_move(false);
        }
        return;
    };
    let Some(obj) = sim.interner.object_type(entity.type_ref(), rules) else {
        return;
    };
    if !obj.phobos_keep_target.enabled {
        clear_latched_target(sim, id, rules, registry, effects);
        return;
    }
    let cfg = obj.phobos_keep_target;
    let mission = sim
        .substrate
        .entities
        .get(id)
        .map(|entity| entity.mission.current());
    let guard = MissionId::from_known(MissionType::Guard);
    let moving = MissionId::from_known(MissionType::Move);
    if mission == Some(guard) {
        if !cfg.no_more_pursuit {
            let _ = sim.mission_queue_exact(
                id,
                MissionId::from_known(MissionType::Attack),
                0,
                sim.session.binary_frame(),
                &LiveReadyInputProvider { rules },
            );
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                entity.set_phobos_keep_target_on_move(false);
            }
            return;
        }
    } else if mission != Some(moving) {
        return;
    }
    if !close_enough_with_key(sim, id, &target, rules, registry) {
        clear_latched_target(sim, id, rules, registry, effects);
    }
}

fn clear_latched_target(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: &mut FrameEffects<'_>,
) {
    let _ = sim.assign_target_represented(id, None, rules, effects);
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.set_phobos_keep_target_on_move(false);
    }
}

#[cfg(all(test, tests_sim_h_o))]
#[path = "keep_target_on_move_tests.rs"]
mod tests;
