//! Foot Mission_Eaten4D4CB0..4D4DB8, inherited by Infantry and Unit.
//! Original Steam gamemd SHA2563e81a617..08600; dispatcher5B332C calls
//! vt+218 (both class tables contain4D4CB0). Arrival belongs to PerCell.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::Ability;
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
    effects: FrameEffects<'_>,
) -> i32 {
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
    //4D4CE7..4D4D33: only C4-capable or Engineer Infantry restore a
    //missing destination from TarCom. Ordinary civilians keep their NavCom.
    if let Some(building) = building_target
        && entity.navigation.nav_com().is_none()
        && entity.category() == EntityCategory::Infantry
        && sim
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| {
                object.c4
                    || object.engineer
                    || veterancy::has_weapon_ability(
                        entity.veterancy.rank_kind(),
                        object,
                        Ability::C4,
                    )
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
    sim.mission_rate_epilogue_for(rules, id, MissionType::Eaten)
}
