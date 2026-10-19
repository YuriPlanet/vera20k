//! World observations for the ra2-mono target-preflight port. No mutable
//! gameplay state is duplicated here: occupancy, power, alliances and fog
//! stay with their existing VERA owners.

use crate::map::entities::EntityCategory;
use crate::map::houses::is_allied_with;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_targeting::{
    HouseMask, LaunchSite, TargetContent, TargetMask, TargetPreflightInput, TargetReject,
    evaluate_target,
};
use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::Simulation;

/// Shared optional-SW target/placement bound, using the terrain owner.
pub(super) fn on_map(sim: &Simulation, cell: (u16, u16)) -> bool {
    sim.resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(cell.0, cell.1))
        .is_some()
        && crate::sim::cell_rect::cell_is_in_playfield_height_aware_in_query(
            (i32::from(cell.0), i32::from(cell.1)),
            sim.playfield_bounds,
            sim.resolved_terrain.as_ref(),
            None,
        )
}

impl Simulation {
    /// Shared extension target gate, suitable for cursor preview and the final
    /// command transaction. Stock types retain their existing dispatch gates.
    /// A preview uses an isolated native-cell query, so even a projected fog
    /// lookup outside the grid cannot mutate the authoritative Dummy cell.
    pub fn super_weapon_target_check(
        &self,
        rules: &RuleSet,
        owner: InternedId,
        sw_type: InternedId,
        cell: (u16, u16),
        automatic: bool,
    ) -> Result<(), TargetReject> {
        let sw = rules
            .super_weapon(self.interner.resolve(sw_type))
            .ok_or(TargetReject::InstanceNotFound)?;
        check(self, rules, owner, sw, cell, automatic)
    }
}

pub(super) fn check(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
    automatic: bool,
) -> Result<(), TargetReject> {
    if sw.generic_warhead.is_none() {
        return Ok(());
    }
    let sites: Vec<_> = launch_sites(sim, rules, owner, sw)
        .into_iter()
        .map(|(_, site)| site)
        .collect();
    check_with_sites(sim, rules, owner, sw, cell, automatic, &sites)
}

/// AI prepares the launcher observation once per SW, not once per candidate.
/// This is a borrowed query input, never a persistent cache or new authority.
pub(super) fn check_with_sites(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
    automatic: bool,
    sites: &[LaunchSite],
) -> Result<(), TargetReject> {
    let Some(settings) = &sw.generic_warhead else {
        return Ok(());
    };
    if !super::generic_warhead::can_launch(sim, rules, sw, cell) {
        return Err(
            if sim
                .resolved_terrain
                .as_ref()
                .is_some_and(|terrain| terrain.cell(cell.0, cell.1).is_some())
            {
                TargetReject::InvalidPayload
            } else {
                TargetReject::TargetOutOfBounds
            },
        );
    }
    let targeting = &settings.targeting;
    let terrain = sim.resolved_terrain.as_ref().expect("validated map");
    let target = terrain.cell(cell.0, cell.1).expect("validated cell");
    let layer =
        if target.bridge_facts.raw_flags & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL != 0 {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
    let contents: Vec<_> = sim
        .substrate
        .occupancy
        .get(cell.0, cell.1)
        .into_iter()
        .flat_map(|occupancy| occupancy.iter_layer(layer))
        .filter_map(|occupant| sim.substrate.entities.get(occupant.entity_id))
        .filter(|entity| entity.is_object_alive() && !entity.lifecycle.in_limbo())
        .map(|entity| TargetContent {
            kind: match entity.category() {
                EntityCategory::Infantry => TargetMask::INFANTRY,
                EntityCategory::Structure => TargetMask::BUILDING,
                EntityCategory::Unit | EntityCategory::Aircraft => TargetMask::UNIT,
            },
            relation: if entity.owner() == owner {
                HouseMask::OWNER
            } else if is_allied_with(
                &sim.house_alliances,
                sim.interner.resolve(owner),
                sim.interner.resolve(entity.owner()),
            ) {
                HouseMask::ALLIES
            } else {
                HouseMask::ENEMIES
            },
        })
        .collect();
    let shrouded = (!targeting.fire_into_shroud).then(|| {
        crate::sim::vision::cell_is_shrouded(
            &sim.fog,
            &NativeCellQuery::isolated(terrain),
            owner,
            cell,
        )
    });
    evaluate_target(&TargetPreflightInput {
        target_cell: Some((i32::from(cell.0), i32::from(cell.1))),
        requires_target_cell: true,
        requirements: if automatic {
            &targeting.automatic
        } else {
            &targeting.manual
        },
        contents: &contents,
        target_in_bounds: true,
        terrain_is_water: Some(
            target.land_type() == crate::rules::terrain_rules::LandType::Water.as_index(),
        ),
        fire_into_shroud: targeting.fire_into_shroud,
        shrouded,
        range: targeting.range(),
        requires_launch_site: false,
        launch_sites: sites,
    })
}

/// ra2-mono ResolveSuperweaponLaunchSites adaptation: VERA's retained House
/// building order and operational-state owner replace OOP/ECS bridge lookups.
pub(super) fn launch_sites(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
) -> Vec<(u64, LaunchSite)> {
    sim.houses
        .get(&owner)
        .into_iter()
        .flat_map(|house| house.base_projection.buildings())
        .filter_map(|&id| {
            let entity = sim.substrate.entities.get(id)?;
            if !super::provided_supers(sim, rules, entity)
                .any(|name| name.eq_ignore_ascii_case(&sw.id))
            {
                return None;
            }
            let coord = crate::sim::movement::ground_pose::object_get_coords(
                entity,
                sim.resolved_terrain.as_ref(),
            );
            Some((
                id,
                LaunchSite {
                    center: (coord.x / 256, coord.y / 256),
                    eligible: entity.owner() == owner
                        && entity.is_object_alive()
                        && !entity.lifecycle.in_limbo()
                        && sim.building_operational_state(id, rules) == Some(true),
                    range_minimum_override: None,
                    range_maximum_override: None,
                },
            ))
        })
        .collect()
}
