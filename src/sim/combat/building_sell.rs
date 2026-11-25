//! Phobos sell/undeploy on impact (`Ext/WarheadType/Detonate.cpp:213-214,
//! 266-352`), the last of `DetonateOnOneUnit`'s house-changing arms: after
//! `RemoveMindControl`, `ChangeOwner` and `ApplyConvert`, a hit building is
//! sold or packed up.
//!
//! `ApplyBuildingUndeploy` reads:
//!
//! 1. Only a `BuildingClass` target proceeds.
//! 2. `BuildingSell=` outranks `BuildingUndeploy=` and returns either way: a
//!    building that `CanBeSold()` and is not strange sells
//!    (`:270-279`), unless `BuildingSell.IgnoreUnsellable=` overrides both
//!    gates.
//! 3. `BuildingUndeploy=` refuses a `Construction` or `Selling` mission and a
//!    type with no `UndeploysInto=`, or a `ConstructionYard` while the game
//!    mode forbids MCV redeploy, then sells through the undeploy order
//!    (`:284-304`).
//! 4. The building's cell becomes the archive target (`:351-353`) so the
//!    pack-up drives the resulting unit there; `BuildingUndeploy.Leave=`
//!    relocates that cell away from the threat instead (`:304-349`).
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ and sim/ only; never audio/ or ui/.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::rules::warhead_type::WarheadType;
use crate::sim::combat::attack_target::TargetKind;
use crate::sim::mission::MissionType;
use crate::sim::production::SellOrder;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

/// Sells or undeploys `target_id` when the warhead asks for it.
pub(crate) fn apply(
    world: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    warhead: &WarheadType,
    target_id: u64,
    effects: &mut FrameEffects<'_>,
) {
    let extras = &warhead.phobos_extras;
    if !(extras.building_sell || extras.building_undeploy) {
        return;
    }
    let Some(target) = world.substrate.entities.get(target_id) else {
        return;
    };
    // `abstract_cast<BuildingClass*, true>(pTarget)` (`:268`).
    if target.category() != EntityCategory::Structure {
        return;
    }
    let type_ref = target.type_ref();
    let strange = world
        .interner
        .object_type(type_ref, rules)
        .is_some_and(|object| object.is_vehicle());
    if extras.building_sell {
        // `if ((pBuilding->CanBeSold() && !pBuilding->IsStrange()) ||
        // this->BuildingSell_IgnoreUnsellable)` (`:271-273`).
        let sellable =
            crate::sim::production::can_sell_building(world, rules, target_id) && !strange;
        if extras.building_sell_ignore_unsellable || sellable {
            // `pBuilding->SetArchiveTarget(nullptr)` then `Sell(1)` (`:275-277`).
            clear_archive_target(world, target_id);
            crate::sim::production::sell_back(
                world,
                rules,
                target_id,
                SellOrder::Player,
                registry,
                effects,
            );
        }
        // `return;` (`:279`): a sell arm never undeploys.
        return;
    }
    if !extras.building_undeploy {
        return;
    }
    // `// CanBeSold() will also check this` (`:282-287`).
    let mission = target.mission.effective().known();
    if matches!(
        mission,
        Some(MissionType::Construction | MissionType::Selling)
    ) {
        return;
    }
    // `!pType->UndeploysInto || (pType->ConstructionYard && !MCVRedeploy)`
    // (`:291-292`) is [`Simulation::can_undeploy_building_runtime`].
    if !world.can_undeploy_building_runtime(target_id, false, rules) {
        return;
    }
    // `SetArchiveTarget(pCell)` then `Sell(1)` (`:351-353`). `Finish_undeploy`
    // hands that archive to the new unit as its destination
    // (`world_spawn.rs:3233-3247`), so the cell chosen here is where the
    // resulting unit walks to. `Leave=` picks it away from the threat; the
    // ordinary arm leaves the archive alone and the unit stands where the
    // building stood, matching native's `SetArchiveTarget(GetMapCoords())`
    // once the pack-up lands on that same cell.
    if extras.building_undeploy_leave
        && let Some((x, y)) = relocation_cell(world, rules, target_id)
    {
        if let Some(target) = world.substrate.entities.get_mut(target_id) {
            target.set_archive_target(Some(TargetKind::Cell(x, y)));
        }
    }
    world.undeploy_building(target_id, false, rules, registry, effects);
}

/// `pBuilding->SetArchiveTarget(nullptr)` (`:275`), so the pack-up does not
/// drive the resulting unit anywhere.
fn clear_archive_target(world: &mut Simulation, target_id: u64) {
    if let Some(target) = world.substrate.entities.get_mut(target_id) {
        target.set_archive_target(None);
    }
}

/// `BuildingUndeploy.Leave=`'s destination cell (`Detonate.cpp:296-350`).
///
/// `cell` starts at `GetMapCoords()`, gains `(1,1)` on a foundation wider or
/// taller than 2 (`:300-301`), moves ~14 cells along the threat bin's radian
/// (`:338-348`, [`super::building_sell_leave::leave_target_cell`]) and is
/// finally snapped to a deployable cell by `MapClass::NearByLocation`
/// (`:354-355`) — `Find_Nearby_Passable_Cell` with the `UndeploysInto` unit's
/// SpeedType and MovementZone, the zone the `-1` argument resolves to at the
/// seed, and a `(width + 2, height + 2)` footprint. Every one of that call's
/// six booleans is `false` (`:354`), so no passability flag needs a tie-break.
/// Phobos keeps the raw offset when the snap finds nothing
/// (`if (newCell != CellStruct::Empty)`), which this mirrors by returning
/// `Some` either way and letting a cell the map cannot hold simply fail to
/// move the unit.
///
/// `None` only when the building itself is gone.
pub(super) fn relocation_cell(
    world: &mut Simulation,
    rules: &RuleSet,
    target_id: u64,
) -> Option<(u16, u16)> {
    use crate::sim::combat::combat_weapon::is_armed;
    use crate::sim::combat::phobos_airburst::cell_spread_technos;
    use crate::sim::find_nearby_cell::{
        NearbyArgs, NearbyFootprint, NearbySearchOptions, PassabilityArgs,
    };
    use crate::sim::movement::ground_pose::object_get_coords;
    use crate::sim::projectile::ProjectileCoord;
    use crate::util::direction_tables::facing16_between;

    let (owner, type_ref, map_cell, centre) = {
        let target = world.substrate.entities.get(target_id)?;
        (
            target.owner(),
            target.type_ref(),
            (target.position.rx(), target.position.ry()),
            object_get_coords(target, world.resolved_terrain.as_ref()),
        )
    };
    let object = world.object_type(type_ref, rules)?;
    let (width, height) = object.foundation.dimensions();
    // `if (width > 2 || height > 2) cell += CellStruct { 1, 1 }` (`:300-301`).
    // `CellStruct` is a signed short pair; the map cells fit losslessly.
    let base = (
        (map_cell.0 as i16).wrapping_add(i16::from(width > 2 || height > 2)),
        (map_cell.1 as i16).wrapping_add(i16::from(width > 2 || height > 2)),
    );

    // `Helpers::Alex::getCellSpreadItems(pBuilding->GetCoords(), 20)` and the
    // per-item `record[GetTargetDirection(pItem).GetValue<4>()] += Cost`
    // (`:307-315`), keeping armed technos the building's house does not count
    // an ally (`HouseClass::IsAlliedWith`, which is true for one's own house).
    let impact = ProjectileCoord::new(centre.x, centre.y, centre.z);
    let mut record = super::building_sell_leave::ThreatRecord::default();
    for id in cell_spread_technos(world, rules, impact, 20.0, false) {
        let Some(item) = world.substrate.entities.get(id) else {
            continue;
        };
        if world
            .house_alliances
            .is_allied(&world.interner, owner, item.owner())
        {
            continue;
        }
        let Some(item_object) = world.object_type(item.type_ref(), rules) else {
            continue;
        };
        if !is_armed(
            item,
            item_object,
            &world.substrate.entities,
            rules,
            &world.interner,
        ) {
            continue;
        }
        let [ix, iy] = crate::sim::movement::ground_pose::object_center_xy(item);
        let bin = super::building_sell_leave::threat_bin(facing16_between(
            [centre.x, centre.y],
            [ix, iy],
        ));
        // `+= pItem->GetTechnoType()->Cost` on the native `int` (`:315`).
        record[bin] = record[bin].wrapping_add(item_object.cost);
    }

    // `if (!costs) dir = RandomRanged(0, 15)` (`:337-338`).
    let dir = match super::building_sell_leave::strongest_bin(&record) {
        Some(dir) => dir,
        None => {
            let roll = world.scenario_rng.next_range_i32_inclusive(0, 15);
            // The range is already `0..=15`; keep the native unsigned look.
            usize::try_from(roll).unwrap_or_default() & 0xF
        }
    };
    let offset = super::building_sell_leave::leave_target_cell(base, dir);

    // `MapClass::NearByLocation(cell, UndeploysInto->SpeedType, -1,
    // UndeploysInto->MovementZone, false, width + 2, height + 2, false, false,
    // false, false, CellStruct::Empty, false, false)` (`:354-355`). The `-1`
    // zone argument is the seed's own `GetZoneID` without the bridge lookup,
    // the shape `phobos_unlimbo_detonate::search_landing_cell` already ports.
    let undeploy_type = rules.object(object.undeploys_into.as_deref()?)?;
    let seed = (i32::from(offset.0), i32::from(offset.1));
    let zone = world
        .zone_grid
        .as_ref()
        .zip(world.resolved_terrain.as_ref())
        .and_then(|(zones, terrain)| {
            zones.get_zone_id_native(
                terrain,
                (offset.0 as u16, offset.1 as u16),
                undeploy_type.movement_zone,
                false,
            )
        });
    let snapped = world.nearby_passable_cell(
        seed,
        NearbyArgs {
            require_burrowable: false,
            passability: PassabilityArgs {
                speed_type: undeploy_type.speed_type,
                required_zone_id: zone,
                movement_zone: undeploy_type.movement_zone,
                bridge_aware_zone: false,
            },
            footprint: NearbyFootprint::new(i32::from(width) + 2, i32::from(height) + 2),
            allow_bridge_cells: false,
            check_height: false,
            check_occupancy: false,
            target_cell: None,
            options: NearbySearchOptions::default(),
        },
    );
    // `if (newCell != CellStruct::Empty) cell = newCell` (`:356-357`): the raw
    // offset stands when the snap finds nothing.
    let (x, y) = snapped.unwrap_or((offset.0 as u16, offset.1 as u16));
    Some((x, y))
}

#[cfg(all(test, tests_sim_combat_a_c))]
#[path = "building_sell_tests.rs"]
mod tests;
