//! The single Foot Capture mission owner (gamemd.exe 0x004D4B20).
//!
//! Infantry, Unit and Aircraft share the body (vt+0x214); the walk itself
//! belongs to the destination the order gave, and the arrival to
//! `InfantryClass::PerCellProcess` (an engineer's capture or repair, a spy's
//! infiltration, an occupier's garrison; `sim::movement::per_cell`). Each
//! dispatch only keeps the order coherent: it heads for a building target,
//! idles once no destination is left, and sends a computer house's capturer
//! with nothing to capture hunting.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::Ability;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{TargetKind, veterancy};
use crate::sim::components::NavTargetRef;
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::ScatterFlags;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::{FrameEffects, Simulation};

/// `FootClass::Mission_Capture @ 0x004D4B20`. The MissionClass dispatcher
/// owns the returned delay timer.
///
/// - With a building target and no destination, an infantryman that is
///   `C4=`, has weapon ability 14 (C4), or is `Engineer=`, `Occupier=`,
///   `Assaulter=` or `Infiltrate=` takes the target as its destination
///   (vt+0x480(target, 1), `0x004D4B43..0x004D4BC1`).
/// - With no destination then, the object idles (vt+0x484(0, 1)) and, when
///   a building stands in the cell it then occupies, scatters
///   (vt+0x174(NULL, 1, 0); `0x004D4BC7..0x004D4C0E`).
/// - With no target, a computer house's object that is not an `Occupier=`
///   infantryman drops its target and destination and queues Hunt
///   (vt+0x3C8(0), vt+0x480(0, 1), vt+0x1E8(Hunt, 0);
///   `0x004D4C14..0x004D4C6B`).
///
/// It answers the then-current mission's `ftol(Rate × 900) +
/// RandomRanged(0, 2)` (`0x004D4C71..0x004D4C9C`).
pub(crate) fn mission_capture(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    id: u64,
    effects: FrameEffects<'_>,
) -> i32 {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 1;
    };
    let target = match entity.attack_target.as_ref().map(|attack| &attack.target) {
        Some(TargetKind::Entity(target)) => Some(*target),
        _ => None,
    };
    let building_target = target.filter(|&target| {
        sim.substrate
            .entities
            .get(target)
            .is_some_and(|object| object.category() == EntityCategory::Structure)
    });
    if let Some(building) = building_target
        && entity.navigation.nav_com.is_none()
        && entity.category() == EntityCategory::Infantry
        && sim
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| {
                object.c4
                    || veterancy::has_weapon_ability(
                        entity.veterancy.rank_kind(),
                        object,
                        Ability::C4,
                    )
                    || object.engineer
                    || object.occupier
                    || object.assaulter
                    || object.infiltrate
            })
    {
        let _ = sim.assign_destination_represented(
            id,
            Some(NavTargetRef::object(building)),
            Some(rules),
            registry,
            effects,
        );
    }

    if sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.navigation.nav_com.is_none())
    {
        crate::sim::world::enter_idle_mode(sim, id, rules, registry, effects);
        let on_building = sim.substrate.entities.get(id).is_some_and(|entity| {
            sim.substrate
                .occupancy
                .first_building_on_layer(
                    entity.position.rx,
                    entity.position.ry,
                    MovementLayer::Ground,
                )
                .is_some()
        });
        if on_building
            && let Err(cause) = sim.scatter(
                id,
                None,
                ScatterFlags::new(true, false),
                rules,
                registry,
                effects,
            )
        {
            log::debug!("capturer {id} did not scatter off its building: {cause}");
        }
    }

    let hunts = sim.substrate.entities.get(id).is_some_and(|entity| {
        entity.attack_target.is_none()
            && !sim.owner_is_human(entity.owner())
            && !(entity.category() == EntityCategory::Infantry
                && sim
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.occupier))
    });
    if hunts {
        let _ = sim.assign_target_represented(id, None, Some(rules), registry, effects);
        sim.assign_null_destination(id, Some(rules), registry, effects);
        let now = sim.session.binary_frame;
        let _ = sim.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Hunt),
            0,
            now,
            &LiveReadyInputProvider { rules },
        );
    }
    sim.mission_rate_epilogue_for(rules, id, MissionType::Capture)
}

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
