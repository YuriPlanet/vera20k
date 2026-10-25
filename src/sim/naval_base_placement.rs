//! Native naval branch of HouseClass AI base-placement selection.
//!
//! This module owns only the stock-active `Naval=yes` fast path. The ordinary
//! perimeter search is `sim::ai_base_site`, which dispatches here; sharing
//! either path would add gates and ordering that
//! `HouseClass__AI_FindBasePlacement` does not execute.

use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
use crate::sim::house_state::HouseState;
use crate::sim::world::Simulation;

/// Find the one naval ready-Building site selected by the native fast path.
///
/// gamemd-derived: `HouseClass__AI_FindBasePlacement @ 0x005060B0`, naval
/// branch `0x005060E8..0x00506213`; type selection delegates to
/// `HouseClass__FirstBuildableFromArray @ 0x005051E0`.
pub(crate) fn find_naval_base_placement(
    sim: &Simulation,
    rules: &RuleSet,
    owner: crate::sim::intern::InternedId,
) -> Option<(u16, u16)> {
    let house = sim.houses.get(&owner)?;

    // Native invokes the source-ordered selector twice rather than retaining
    // the first pointer. Keep the two calls independent even though ordinary
    // immutable Rules yield the same type both times.
    let width_type = first_buildable_shipyard(rules, sim, house)?;
    let width = width_type.foundation.dimensions().0;
    let height_type = first_buildable_shipyard(rules, sim, house)?;
    let height = height_type.foundation.dimensions().1;

    // The native fallback is literal `(0,0)` and reaches normal FNPC/MapClass
    // admission rather than becoming an early Rust-side failure.
    let origin = house.base_origin();

    let resolved_terrain = sim.resolved_terrain.as_ref()?;
    let candidate = sim.nearby_passable_cell(
        (i32::from(origin.0 as i16), i32::from(origin.1 as i16)),
        NearbyArgs::plain(
            PassabilityArgs {
                speed_type: SpeedType::Float,
                required_zone_id: None,
                movement_zone: MovementZone::Normal,
                bridge_aware_zone: false,
            },
            NearbyFootprint::new(
                i32::from(width).wrapping_add(2),
                i32::from(height).wrapping_add(2),
            ),
        ),
    )?;
    if candidate == (0, 0) {
        return None;
    }

    first_yard_distance_accepts(sim, rules, house, candidate, resolved_terrain).then_some(candidate)
}

/// Exact `FirstBuildableFromArray` specialization used for `[General]
/// Shipyard=`. No generic build-option eligibility is consulted.
fn first_buildable_shipyard<'a>(
    rules: &'a RuleSet,
    sim: &Simulation,
    house: &HouseState,
) -> Option<&'a ObjectType> {
    let country = house.country()?;
    let country_name = sim.interner.resolve(country);
    crate::sim::ai_buildable::first_buildable_from_array(
        rules,
        &rules.shipyard_types,
        country_name,
        house.side_index(),
        sim.session.game_options().super_weapons,
    )
}

/// Compare the chosen CellClass center against exactly the first acquired live
/// BuildConst Building. An empty vector bypasses the cap; a stale first ID is an
/// invariant failure and never silently advances to a later yard.
///
/// gamemd-derived: `HouseClass__AI_FindBasePlacement @ 0x005061BE..0x00506213`,
/// `BuildingClass::GetCoords @ 0x00447AC0`,
/// `CellClass__Get_Center_Coords @ 0x00480A30`, and
/// `CoordStruct__Distance3D @ 0x0041C380`.
fn first_yard_distance_accepts(
    sim: &Simulation,
    rules: &RuleSet,
    house: &HouseState,
    candidate: (u16, u16),
    terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
) -> bool {
    let Some(&yard_id) = house.build_const_order().first() else {
        return true;
    };
    let Some(yard) = sim.substrate.entities.get(yard_id) else {
        return false;
    };
    if yard.category() != EntityCategory::Structure
        || !yard.type_facts.build_const_eligible()
        || !yard.lifecycle.object_alive()
        || yard.lifecycle.in_limbo()
        || !yard.lifecycle.cell_marked()
    {
        return false;
    }

    let Some(cell) = terrain.cell(candidate.0, candidate.1) else {
        return false;
    };
    let Some(center) = crate::sim::cell_rect::CellRef::Real(cell).center_coordinate() else {
        return false;
    };

    let yard_coord = crate::sim::movement::ground_pose::object_get_coords(yard, Some(terrain));
    let distance = crate::util::native_x87::distance_3d_leptons(
        [center.x, center.y, center.z],
        [yard_coord.x, yard_coord.y, yard_coord.z],
    );
    let cap = rules.ai_naval_yard_adjacency.wrapping_shl(8);
    distance <= cap
}

#[cfg(test)]
mod tests {
    use super::find_naval_base_placement;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;

    /// The naval branch `0x005060E8..0x00506213` seeds FNPC (`0x00506193`)
    /// at the base origin with the plain pushes `0x0050616D..0x0050618D`
    /// (Float, zone -1, Normal) and the yard's Foundation width + 2 by
    /// height + 2 (`0x00506125`, `0x0050613B`). Each CellRect cell fails on
    /// any raw ground occupation bit (`CheckCellPassability @ 0x004834A0`,
    /// `0x00483552..0x0048357A`), so an object in the seed's 4x3 rect moves
    /// the site to a ring-1 cell whose rect misses it.
    #[test]
    fn raw_ground_occupation_in_the_yard_rect_moves_the_site() {
        let ini = IniFile::from_str(
            "[Countries]\n0=Americans\n1=Russians\n[Russians]\nParentCountry=Russians\n\
             [General]\nShipyard=GAYARD\n\
             [BuildingTypes]\n0=GAYARD\n[GAYARD]\nOwner=Russians\nNaval=yes\n\
             [Water]\nFloat=100%\n",
        );
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &ini,
            &IniFile::from_str("[GAYARD]\nFoundation=2x1\n"),
        )
        .unwrap();
        let mut sim = crate::sim::arena_fixture::rival_water_arena(&rules, 0, 32);
        let owner = sim.interner.intern("Russians");
        let house = sim.houses.get_mut(&owner).unwrap();
        house.set_country_for_test(Some(owner));
        house.set_base_center_for_test(Some((16, 16)));
        assert_eq!(
            find_naval_base_placement(&sim, &rules, owner),
            Some((16, 16))
        );

        // The far corner of the seed's 4x3 rect; a 3x4 rect would miss it.
        sim.substrate.raw_cell_occupation.mark_ground(19, 18, 0x40);
        // Ring 1's first cell, whose rect (15..=18, 15..=17) misses (19, 18),
        // is the frame-0 pick.
        assert_eq!(sim.session.binary_frame(), 0);
        assert_eq!(
            find_naval_base_placement(&sim, &rules, owner),
            Some((15, 15))
        );
    }
}
