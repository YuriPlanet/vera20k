//! Foot Mission_Eaten4D4CB0..4D4DB8, inherited by Infantry and Unit.
//! Original Steam gamemd; dispatcher5B332C calls
//! vt+218 (both class tables contain4D4CB0). Arrival belongs to PerCell.
//! The aircraft dispatch runs this body too (`sim::aircraft::dispatch_mission`):
//! AircraftClass keeps `4D4CB0` in its own slot (the retail table at
//! `0x007E22A4` holds it at `vt+0x218`).

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{TargetKind, veterancy};
use crate::sim::components::NavTargetRef;
use crate::sim::mission::MissionType;
use crate::sim::movement::ScatterFlags;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::{FrameEffects, Simulation};

pub(crate) fn mission_eaten(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    id: u64,
    effects: &mut FrameEffects<'_>,
) -> i32 {
    let (building_target, nav_com, category, type_ref, rank) = {
        let Some(entity) = sim.substrate.entities.get(id) else {
            return 1;
        };
        let building_target = match entity.attack_target.target() {
            Some(TargetKind::Entity(target)) => sim
                .substrate
                .entities
                .get(target)
                .filter(|object| object.category() == EntityCategory::Structure)
                .map(|_| target),
            _ => None,
        };
        (
            building_target,
            entity.navigation.nav_com(),
            entity.category(),
            entity.type_ref(),
            entity.veterancy.rank_kind(),
        )
    };
    // Phobos `FootClass_Mission_Eaten_Grinding` (`0x4D4CD3`,
    // `Hooks.Grinding.cpp:52-66`): when the destination is a `Grinding=`
    // building whose type refuses this foot, clear it and take the
    // missing-destination arm (LoseDestination `0x4D4D43`) without the
    // TarCom restore below.
    let refused_destination = matches!(
        nav_com,
        Some(NavTargetRef::Object { id: building })
            if sim.substrate.entities.get(building).is_some_and(|target| {
                target.category() == EntityCategory::Structure
                    && sim
                        .object_type(target.type_ref(), rules)
                        .is_some_and(|object| object.grinding)
                    && !sim.can_grind_techno(building, id, rules)
            })
    );
    if refused_destination {
        sim.assign_null_destination(id, rules, registry, effects);
    }
    let restore_destination = !refused_destination;
    //4D4CE7..4D4D33: only C4-capable or Engineer Infantry restore a
    //missing destination from TarCom. Ordinary civilians keep their NavCom.
    if restore_destination
        && let Some(building) = building_target
        && sim
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.navigation.nav_com().is_none())
        && category == EntityCategory::Infantry
        && sim
            .object_type(type_ref, rules)
            .is_some_and(|object| veterancy::holds_c4(rank, object) || object.engineer)
    {
        let _ = sim.assign_destination_represented(
            id,
            Some(NavTargetRef::object(building)),
            rules,
            effects,
        );
    }
    if sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.navigation.nav_com().is_none())
    {
        crate::sim::world::enter_idle_mode(sim, id, rules, registry, effects);
        let on_building = sim.substrate.entities.get(id).is_some_and(|entity| {
            sim.substrate
                .occupancy
                .first_building_on_layer(
                    entity.position.rx(),
                    entity.position.ry(),
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
            log::debug!("Eaten foot {id} did not scatter off its building: {cause}");
        }
    }
    Simulation::mission_rate_epilogue_for(
        &mut sim.scenario_rng,
        &sim.substrate,
        rules,
        id,
        MissionType::Eaten,
    )
}
