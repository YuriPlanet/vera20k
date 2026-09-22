//! Native naval branch of HouseClass AI base-placement selection.
//!
//! This module owns only the stock-active `Naval=yes` fast path. Ordinary
//! BasePlan/perimeter placement remains in `ai`; sharing either path would add
//! gates and ordering that `HouseClass__AI_FindBasePlacement` does not execute.

use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::find_nearby_cell::{
    NearbyAnchorGate, NearbyFootprint, NearbyQuery, PassabilityArgs, find_nearby_passable_cell,
    map_owned_radius_cap,
};
use crate::sim::house_state::HouseState;
use crate::sim::pathfinding::PathGrid;
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
    path_grid: Option<&PathGrid>,
) -> Option<(u16, u16)> {
    let house = sim.houses.get(&owner)?;

    // Native invokes the source-ordered selector twice rather than retaining
    // the first pointer. Keep the two calls independent even though ordinary
    // immutable Rules yield the same type both times.
    let width_type = first_buildable_shipyard(rules, sim, house)?;
    let width = crate::rules::foundation::foundation_dimensions(&width_type.foundation).0;
    let height_type = first_buildable_shipyard(rules, sim, house)?;
    let height = crate::rules::foundation::foundation_dimensions(&height_type.foundation).1;

    let origin = if house.alternate_base_center != (0, 0) {
        house.alternate_base_center
    } else {
        // Rust's older Option projects the packed native default; the native
        // fallback itself is still literal `(0,0)` and reaches normal FNPC/
        // MapClass admission rather than becoming an early Rust-side failure.
        house.base_center.unwrap_or((0, 0))
    };

    // This is an active MapClass query. Missing PathGrid, real CellClass
    // terrain, diamond bounds, or Size-height authority cannot be replaced by
    // the compatibility projection without changing bridge pools or Z.
    let path_grid = path_grid?;
    let resolved_terrain = sim.resolved_terrain.as_ref()?;
    let playfield_bounds = sim.playfield_bounds?;
    let size_height = sim.playfield_size_height?;
    let query = naval_query(
        path_grid,
        resolved_terrain,
        sim.overlay_grid.as_ref(),
        sim.zone_grid.as_ref(),
        playfield_bounds,
        playfield_bounds.base,
        size_height,
        i32::from(width).wrapping_add(2),
        i32::from(height).wrapping_add(2),
    );
    let candidate = find_nearby_passable_cell(
        (i32::from(origin.0 as i16), i32::from(origin.1 as i16)),
        &query,
        sim.session.binary_frame,
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
    let country = house.country?;
    let country_name = sim.interner.resolve(country);
    crate::sim::ai_buildable::first_buildable_from_array(
        rules,
        &rules.shipyard_types,
        country_name,
        house.side_index,
        sim.session.game_options.super_weapons,
    )
}

#[allow(clippy::too_many_arguments)]
fn naval_query<'a>(
    path_grid: &'a PathGrid,
    resolved_terrain: &'a crate::map::resolved_terrain::ResolvedTerrainGrid,
    overlay_grid: Option<&'a crate::sim::overlay_grid::OverlayGrid>,
    zone_grid: Option<&'a crate::sim::pathfinding::zone_map::ZoneGrid>,
    playfield_bounds: crate::sim::cell_rect::PlayfieldBounds,
    size_width: i32,
    size_height: i32,
    footprint_width: i32,
    footprint_height: i32,
) -> NearbyQuery<'a> {
    NearbyQuery {
        native_cells: None,
        raw_occupation: None,
        passability: PassabilityArgs {
            speed_type: SpeedType::Float,
            required_zone_id: None,
            movement_zone: MovementZone::Normal,
            bridge_aware_zone: false,
        },
        footprint: NearbyFootprint::new(footprint_width, footprint_height),
        anchor_gate: NearbyAnchorGate::NativeHeightAware,
        allow_bridge_cells: true,
        check_height: false,
        check_occupancy: false,
        radius_cap: map_owned_radius_cap(size_width, size_height),
        target_cell: None,
        path_grid: Some(path_grid),
        resolved_terrain: Some(resolved_terrain),
        overlay_grid,
        occupancy: None,
        entities: None,
        zone_grid,
        playfield_bounds: Some(playfield_bounds),
    }
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
    let Some(&yard_id) = house.build_const_order.first() else {
        return true;
    };
    let Some(yard) = sim.substrate.entities.get(yard_id) else {
        return false;
    };
    if yard.category != EntityCategory::Structure
        || !yard.build_const_eligible
        || !yard.lifecycle.object_alive
        || yard.lifecycle.in_limbo
        || !yard.lifecycle.cell_marked
    {
        return false;
    }

    let signed_x = i32::from(candidate.0 as i16);
    let signed_y = i32::from(candidate.1 as i16);
    let cell_x = signed_x
        .wrapping_mul(crate::sim::cell_kernel::LEPTONS_PER_CELL)
        .wrapping_add(crate::sim::cell_kernel::CELL_CENTER_LEPTONS);
    let cell_y = signed_y
        .wrapping_mul(crate::sim::cell_kernel::LEPTONS_PER_CELL)
        .wrapping_add(crate::sim::cell_kernel::CELL_CENTER_LEPTONS);
    let Some(cell) = terrain.cell(candidate.0, candidate.1) else {
        return false;
    };
    let Ok(cell_z) =
        crate::util::lepton::ground_height_leptons(cell.level, cell.slope_type, cell_x, cell_y)
    else {
        return false;
    };

    let (foundation_width, foundation_height) =
        crate::rules::foundation::foundation_dimensions(&yard.foundation);
    let yard_x = i32::from(yard.position.rx as i16)
        .wrapping_mul(crate::sim::cell_kernel::LEPTONS_PER_CELL)
        .wrapping_add(yard.position.sub_x.to_num::<i32>())
        .wrapping_add(
            i32::from(foundation_width)
                .wrapping_sub(1)
                .wrapping_mul(crate::sim::cell_kernel::CELL_CENTER_LEPTONS),
        );
    let yard_y = i32::from(yard.position.ry as i16)
        .wrapping_mul(crate::sim::cell_kernel::LEPTONS_PER_CELL)
        .wrapping_add(yard.position.sub_y.to_num::<i32>())
        .wrapping_add(
            i32::from(foundation_height)
                .wrapping_sub(1)
                .wrapping_mul(crate::sim::cell_kernel::CELL_CENTER_LEPTONS),
        );
    let yard_z = crate::sim::combat::object_world_z_leptons(yard, Some(terrain));
    let distance = crate::util::native_x87::distance_3d_leptons(
        [cell_x, cell_y, cell_z],
        [yard_x, yard_y, yard_z],
    );
    let cap = rules.ai_naval_yard_adjacency.wrapping_shl(8);
    distance <= cap
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    use crate::map::bridge_facts::{
        BRIDGE_FLAG_FORWARD_SIDE, BRIDGE_FLAG_STRUCTURAL, BridgeCellFacts,
    };
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainClass};
    use crate::sim::ai::{AiPlayerState, tick_ai};
    use crate::sim::cell_rect::PlayfieldBounds;
    use crate::sim::command::Command;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;

    fn water_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
        let mut speed_costs = SpeedCostProfile::default();
        speed_costs.float = Some(100);
        ResolvedTerrainCell {
            rx,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: false,
            tileset_index: None,
            land_type: LandType::Water.as_index(),
            yr_cell_land_type: LandType::Water.as_index(),
            slope_type: 0,
            template_height: 0,
            height_in_pixels: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Water,
            speed_costs,
            is_water: true,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
            accepts_smudge: false,
            allows_tiberium: false,
            variant: 0,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: false,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: zone_class::WATER,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: LandType::Water.as_index(),
            base_yr_cell_land_type: LandType::Water.as_index(),
            base_terrain_class: TerrainClass::Water,
            base_speed_costs: speed_costs,
            build_blocked: false,
            has_bridge_deck: false,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: None,
            bridge_facts: BridgeCellFacts::default(),
            tube_index: None,
            radar_left: [0; 3],
            radar_right: [0; 3],
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        }
    }

    fn water_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        ResolvedTerrainGrid::from_cells(
            width,
            height,
            (0..height)
                .flat_map(|ry| (0..width).map(move |rx| water_cell(rx, ry)))
                .collect(),
        )
    }

    fn broad_bounds() -> PlayfieldBounds {
        PlayfieldBounds {
            base: 0,
            off_fc: -128,
            off_100: -128,
            off_104: 256,
            off_108: 256,
        }
    }

    fn selector_rules(
        shipyards: &[&str],
        sections: &str,
        build_tech: &str,
        adjacency: i32,
    ) -> RuleSet {
        let registry = shipyards
            .iter()
            .enumerate()
            .map(|(index, id)| format!("{index}={id}"))
            .collect::<Vec<_>>()
            .join("\n");
        let shipyard_list = shipyards.join(",");
        let text = format!(
            "[General]\nShipyard={shipyard_list}\nAINavalYardAdjacency={adjacency}\n\
             [AI]\nBuildTech={build_tech}\n\
             [Countries]\n0=Americans\n1=Russians\n2=YuriCountry\n\
             [Sides]\nAllied=Americans\nSoviet=Russians\nThird=YuriCountry\n\
             [Americans]\nName=AlliedAlias\nSide=Allied\n\
             [Russians]\nName=SovietAlias\nSide=Soviet\n\
             [YuriCountry]\nName=YuriAlias\nSide=Third\n\
             [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n{registry}\n{sections}\n\
             [SuperWeaponTypes]\n0=DISABLED_SW\n1=FIXED_SW\n\
             [DISABLED_SW]\nType=MultiMissile\nDisableableFromShell=yes\n\
             [FIXED_SW]\nType=MultiMissile\nDisableableFromShell=no\n"
        );
        RuleSet::from_ini(&IniFile::from_str(&text)).expect("selector rules")
    }

    fn naval_integration_rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[General]\nShipyard=GAYARD\nAINavalYardAdjacency=0\n\
             [AI]\nBuildConst=gacnst\n\
             [Countries]\n0=Americans\n[Sides]\nAllied=Americans\n[Americans]\nSide=Allied\n\
             [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GAYARD\n1=NAVAL\n2=LAND\n3=GACNST\n\
             [GAYARD]\nFoundation=1x1\nOwner=Americans\n\
             [NAVAL]\nFoundation=1x1\nNaval=yes\n\
             [LAND]\nFoundation=1x1\n\
             [GACNST]\nFoundation=1x1\nStrength=1000\nOwner=Americans\nConstructionYard=yes\n",
        ))
        .expect("naval integration rules")
    }

    fn selector_sim(
        country: &str,
        side: u8,
        super_weapons: bool,
    ) -> (Simulation, crate::sim::intern::InternedId) {
        let mut sim = Simulation::new();
        sim.session.game_options.super_weapons = super_weapons;
        let owner = sim.interner.intern("AIHouse");
        let country = sim.interner.intern(country);
        sim.houses.insert(
            owner,
            HouseState::new(owner, side, Some(country), false, 10_000, 10),
        );
        (sim, owner)
    }

    fn selected_id<'a>(
        rules: &'a RuleSet,
        sim: &Simulation,
        owner: crate::sim::intern::InternedId,
    ) -> Option<&'a str> {
        first_buildable_shipyard(rules, sim, &sim.houses[&owner]).map(|object| object.id.as_str())
    }

    #[test]
    fn shipyard_selector_exact_gate_truth_table_and_source_fallthrough() {
        let rules = selector_rules(&["YARD"], "[YARD]\nFoundation=4x4", "", 20);
        let (sim, owner) = selector_sim("Americans", 0, true);
        assert_eq!(
            selected_id(&rules, &sim, owner),
            None,
            "native zero-default Owner mask rejects"
        );

        let rules = selector_rules(&["YARD"], "[YARD]\nFoundation=4x4\nOwner=Americans", "", 20);
        let (sim, owner) = selector_sim("Americans", 0, true);
        assert_eq!(
            selected_id(&rules, &sim, owner),
            Some("YARD"),
            "an explicit matching Owner mask accepts"
        );

        let rejected = [
            ("Owner=SovietAlias", "Owner mask"),
            ("RequiredHouses=Russians", "RequiredHouses mask"),
            ("ForbiddenHouses=AlliedAlias", "ForbiddenHouses mask"),
            ("AIBasePlanningSide=1", "signed side"),
        ];
        for (gate, label) in rejected {
            let sections = format!("[YARD]\nFoundation=4x4\n{gate}");
            let rules = selector_rules(&["YARD"], &sections, "", 20);
            let (sim, owner) = selector_sim("Americans", 0, true);
            assert_eq!(selected_id(&rules, &sim, owner), None, "{label}");
        }

        let rules = selector_rules(
            &["BAD", "GOOD"],
            "[BAD]\nFoundation=2x2\nOwner=Russians\n\
             [GOOD]\nFoundation=4x4\nOwner=AlliedAlias\nRequiredHouses=Americans\nForbiddenHouses=Russians\nAIBasePlanningSide=0",
            "",
            20,
        );
        let (sim, owner) = selector_sim("Americans", 0, true);
        assert_eq!(selected_id(&rules, &sim, owner), Some("GOOD"));

        let enabled = selector_rules(
            &["YARD"],
            "[YARD]\nFoundation=4x4\nOwner=Americans\nSuperWeapon=DISABLED_SW\n\
             TechLevel=-1\nPrerequisite=MISSING\nBuildLimit=1\nCost=999999\nAIBuildThis=no",
            "",
            20,
        );
        let (sim, owner) = selector_sim("Americans", 0, true);
        assert_eq!(selected_id(&enabled, &sim, owner), Some("YARD"));

        let cases = [
            (
                "[YARD]\nFoundation=4x4\nOwner=Americans",
                "",
                Some("YARD"),
                "absent primary",
            ),
            (
                "[YARD]\nFoundation=4x4\nOwner=Americans\nSuperWeapon=DISABLED_SW",
                "YARD",
                Some("YARD"),
                "BuildTech exemption",
            ),
            (
                "[YARD]\nFoundation=4x4\nOwner=Americans\nSuperWeapon=FIXED_SW",
                "",
                Some("YARD"),
                "non-disableable primary",
            ),
            (
                "[YARD]\nFoundation=4x4\nOwner=Americans\nSuperWeapon=DISABLED_SW",
                "",
                None,
                "disableable primary",
            ),
            (
                "[YARD]\nFoundation=4x4\nOwner=Americans\nSuperWeapon2=DISABLED_SW",
                "",
                Some("YARD"),
                "ignored SuperWeapon2",
            ),
        ];
        for (section, build_tech, expected, label) in cases {
            let rules = selector_rules(&["YARD"], section, build_tech, 20);
            let (sim, owner) = selector_sim("Americans", 0, false);
            assert_eq!(selected_id(&rules, &sim, owner), expected, "{label}");
        }

        let rules = selector_rules(&["YARD"], "[YARD]\nOwner=Russians", "", 20);
        let (sim, owner) = selector_sim("Americans", 0, true);
        assert_eq!(selected_id(&rules, &sim, owner), None, "no fallback");
    }

    #[test]
    fn retail_side_order_resolves_each_four_by_four_shipyard_to_six_by_six() {
        let rules = selector_rules(
            &["GAYARD", "NAYARD", "YAYARD"],
            "[GAYARD]\nFoundation=4x4\nOwner=Americans\n\
             [NAYARD]\nFoundation=4x4\nOwner=Russians\n\
             [YAYARD]\nFoundation=4x4\nOwner=YuriCountry",
            "",
            20,
        );
        for (country, side, expected) in [
            ("Americans", 0, "GAYARD"),
            ("Russians", 1, "NAYARD"),
            ("YuriCountry", 2, "YAYARD"),
        ] {
            let (sim, owner) = selector_sim(country, side, true);
            let width_type = first_buildable_shipyard(&rules, &sim, &sim.houses[&owner]).unwrap();
            let width =
                crate::rules::foundation::foundation_dimensions(&width_type.foundation).0 + 2;
            let height_type = first_buildable_shipyard(&rules, &sim, &sim.houses[&owner]).unwrap();
            let height =
                crate::rules::foundation::foundation_dimensions(&height_type.foundation).1 + 2;
            assert_eq!(width_type.id, expected);
            assert_eq!((width, height), (6, 6));
        }
    }

    #[test]
    fn naval_query_transcript_uses_literal_native_arguments_and_frame_modulo() {
        let mut terrain = water_terrain(24, 24);
        terrain.cell_mut(12, 12).unwrap().speed_costs.float = Some(0);
        let path = PathGrid::from_resolved_terrain(&terrain);
        let query = naval_query(&path, &terrain, None, None, broad_bounds(), 24, 24, 1, 1);
        assert_eq!(query.passability.speed_type, SpeedType::Float);
        assert_eq!(query.passability.required_zone_id, None);
        assert_eq!(query.passability.movement_zone, MovementZone::Normal);
        assert!(!query.passability.bridge_aware_zone);
        assert_eq!(query.footprint, NearbyFootprint::new(1, 1));
        assert_eq!(query.anchor_gate, NearbyAnchorGate::NativeHeightAware);
        assert!(query.allow_bridge_cells);
        assert!(!query.check_height);
        assert!(!query.check_occupancy);
        assert!(query.target_cell.is_none());
        assert!(query.occupancy.is_none() && query.entities.is_none());
        assert!(query.resolved_terrain.is_some());

        let picks = (0..9)
            .map(|frame| find_nearby_passable_cell((12, 12), &query, frame).unwrap())
            .collect::<Vec<_>>();
        assert!(picks.iter().all(|&cell| cell != (12, 12)));
        assert!(picks.iter().copied().collect::<BTreeSet<_>>().len() > 1);
        assert_eq!(picks[0], picks[8], "eight ring-one entries wrap modulo");
    }

    #[test]
    fn naval_origin_fallback_and_exact_map_authority_are_independent_of_ordinary_base() {
        let rules = naval_integration_rules();
        let mut sim = Simulation::new();
        sim.session.binary_frame = 0;
        sim.session.map_width = 24;
        sim.session.map_height = 24;
        sim.playfield_bounds = Some(broad_bounds());
        sim.playfield_size_height = Some(24);
        sim.resolved_terrain = Some(water_terrain(24, 24));
        let path = PathGrid::from_resolved_terrain(sim.resolved_terrain.as_ref().unwrap());
        let owner = sim.interner.intern("AIHouse");
        let country = sim.interner.intern("Americans");
        let mut house = HouseState::new(owner, 0, Some(country), false, 10_000, 10);
        house.base_center = Some((10, 10));
        house.alternate_base_center = (15, 15);
        sim.houses.insert(owner, house);

        assert_eq!(
            find_naval_base_placement(&sim, &rules, owner, Some(&path)),
            Some((15, 15)),
            "nonzero alternate packed cell overrides primary"
        );
        sim.houses.get_mut(&owner).unwrap().alternate_base_center = (0, 0);
        assert_eq!(
            find_naval_base_placement(&sim, &rules, owner, Some(&path)),
            Some((10, 10)),
            "packed-zero alternate falls back to primary"
        );
        sim.houses.get_mut(&owner).unwrap().base_center = None;
        assert_eq!(
            find_naval_base_placement(&sim, &rules, owner, Some(&path)),
            None,
            "absent Rust primary projects native packed zero and is rejected normally"
        );
        sim.houses.get_mut(&owner).unwrap().base_center = Some((10, 10));

        assert!(find_naval_base_placement(&sim, &rules, owner, None).is_none());
        let terrain = sim.resolved_terrain.take();
        assert!(find_naval_base_placement(&sim, &rules, owner, Some(&path)).is_none());
        sim.resolved_terrain = terrain;
        let bounds = sim.playfield_bounds.take();
        assert!(find_naval_base_placement(&sim, &rules, owner, Some(&path)).is_none());
        sim.playfield_bounds = bounds;
        let size_height = sim.playfield_size_height.take();
        assert!(find_naval_base_placement(&sim, &rules, owner, Some(&path)).is_none());
        sim.playfield_size_height = size_height;
    }

    #[test]
    fn naval_caller_replays_forward_bridge_projection_before_frame_pool_selection() {
        let rules = naval_integration_rules();
        let owner_name = "AIHouse";

        let run = |terrain: ResolvedTerrainGrid| {
            let path = PathGrid::from_resolved_terrain(&terrain);
            let mut sim = Simulation::new();
            sim.session.binary_frame = 0;
            sim.playfield_bounds = Some(broad_bounds());
            sim.playfield_size_height = Some(20);
            let owner = sim.interner.intern(owner_name);
            let country = sim.interner.intern("Americans");
            let mut house = HouseState::new(owner, 0, Some(country), false, 10_000, 10);
            house.base_center = Some((5, 5));
            sim.houses.insert(owner, house);
            sim.resolved_terrain = Some(terrain);
            find_naval_base_placement(&sim, &rules, owner, Some(&path))
        };

        assert_eq!(run(water_terrain(20, 20)), Some((5, 5)));
        let mut projected = water_terrain(20, 20);
        projected.cell_mut(5, 5).unwrap().bridge_facts.raw_flags = BRIDGE_FLAG_FORWARD_SIDE;
        projected.cell_mut(6, 6).unwrap().bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        assert_eq!(
            run(projected),
            Some((4, 4)),
            "the seed becomes indirect and frame zero selects the first direct ring-one cell"
        );
    }

    fn distance_rules(adjacency: i32) -> RuleSet {
        selector_rules(&[], "", "", adjacency)
    }

    fn live_yard(sim: &mut Simulation, id: u64, cell: (u16, u16), foundation: &str) {
        let owner = sim.interner.intern("AIHouse");
        let type_ref = sim.interner.intern("CONYARD");
        let mut yard = GameEntity::new_at_frame_zero_for_test(
            id,
            cell.0,
            cell.1,
            0,
            0,
            owner,
            Health { current: 1000 },
            type_ref,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        yard.foundation = foundation.to_string();
        yard.build_const_eligible = true;
        yard.lifecycle.in_limbo = false;
        yard.lifecycle.cell_marked = true;
        sim.substrate.entities.insert(yard);
    }

    #[test]
    fn first_yard_cap_uses_order_foundation_center_cellclass_z_and_signed_threshold() {
        let mut sim = Simulation::new();
        let terrain = water_terrain(32, 32);
        let mut house = HouseState::default();
        assert!(first_yard_distance_accepts(
            &sim,
            &distance_rules(0),
            &house,
            (11, 11),
            &terrain
        ));

        live_yard(&mut sim, 20, (10, 10), "4x4");
        house.build_const_order.push(20);
        assert!(
            first_yard_distance_accepts(&sim, &distance_rules(1), &house, (11, 11), &terrain),
            "4x4 GetCoords lies between cell centers: its 181-lepton diagonal fits the one-cell cap"
        );

        live_yard(&mut sim, 10, (1, 1), "1x1");
        house.build_const_order = vec![10, 20];
        assert!(
            !first_yard_distance_accepts(&sim, &distance_rules(1), &house, (11, 11), &terrain),
            "first acquisition wins even though stable-id-later yard is at the candidate"
        );

        live_yard(&mut sim, 30, (10, 10), "1x1");
        house.build_const_order = vec![30];
        assert!(
            first_yard_distance_accepts(&sim, &distance_rules(1), &house, (11, 10), &terrain),
            "distance equal to the shifted signed cap is admitted"
        );
        assert!(
            !first_yard_distance_accepts(&sim, &distance_rules(1), &house, (12, 10), &terrain),
            "distance greater than the cap is rejected"
        );
        assert!(!first_yard_distance_accepts(
            &sim,
            &distance_rules(-1),
            &house,
            (10, 10),
            &terrain
        ));

        let mut elevated = water_terrain(32, 32);
        elevated.cell_mut(10, 10).unwrap().level = 1;
        sim.substrate
            .entities
            .get_mut(30)
            .unwrap()
            .position
            .exact_z_leptons = Some(0);
        assert!(
            !first_yard_distance_accepts(&sim, &distance_rules(0), &house, (10, 10), &elevated),
            "CellClass 104-lepton terrain Z makes this nonzero despite identical 2D coordinates"
        );
        sim.substrate
            .entities
            .get_mut(30)
            .unwrap()
            .position
            .exact_z_leptons = Some(104);
        assert!(first_yard_distance_accepts(
            &sim,
            &distance_rules(0),
            &house,
            (10, 10),
            &elevated
        ));
    }

    #[test]
    fn parsed_readai_buildconst_populates_house_and_enforces_naval_cap() {
        let rules = naval_integration_rules();
        assert_eq!(rules.build_const_types, ["gacnst"]);
        assert!(rules.object("GACNST").unwrap().build_const_eligible);

        let mut sim = Simulation::new();
        sim.session.binary_frame = 0;
        sim.playfield_bounds = Some(broad_bounds());
        sim.playfield_size_height = Some(24);
        sim.resolved_terrain = Some(water_terrain(24, 24));
        let owner = sim.interner.intern("AIHouse");
        let country = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            HouseState::new(owner, 0, Some(country), false, 10_000, 10),
        );
        let yard = sim
            .spawn_object("GACNST", "AIHouse", 10, 10, 0, &rules, &BTreeMap::new())
            .expect("parsed BuildConst Construction Yard reveals");
        assert_eq!(sim.houses[&owner].build_const_order, [yard]);
        assert!(sim.entities().get(yard).unwrap().build_const_eligible);

        sim.houses.get_mut(&owner).unwrap().base_center = Some((11, 10));
        let path = PathGrid::from_resolved_terrain(sim.resolved_terrain.as_ref().unwrap());
        assert_eq!(
            find_naval_base_placement(&sim, &rules, owner, Some(&path)),
            None,
            "the zero-cell cap rejects the adjacent result once the parsed yard reaches House state"
        );

        sim.houses
            .get_mut(&owner)
            .unwrap()
            .build_const_order
            .clear();
        assert_eq!(
            find_naval_base_placement(&sim, &rules, owner, Some(&path)),
            Some((11, 10)),
            "the same query would pass only through the native empty-vector bypass"
        );
    }

    #[test]
    fn naval_ai_uses_house_origin_without_ordinary_center_and_retains_failed_ready_item() {
        let rules = naval_integration_rules();
        let mut sim = Simulation::new();
        sim.session.binary_frame = 1;
        sim.session.map_width = 32;
        sim.session.map_height = 32;
        // Keep the entire admissible diamond inside the populated 32x32 map.
        // The broad query fixture admits missing cells too; blocking only real
        // Float rows there does not mean that every candidate is blocked.
        sim.playfield_bounds = Some(PlayfieldBounds::from_normalized_local_size(16, 2, 2, 12, 8));
        sim.playfield_size_height = Some(32);
        sim.resolved_terrain = Some(water_terrain(32, 32));
        let path = PathGrid::from_resolved_terrain(sim.resolved_terrain.as_ref().unwrap());
        let owner = sim.interner.intern("AIHouse");
        let country = sim.interner.intern("Americans");
        let mut house = HouseState::new(owner, 0, Some(country), false, 10_000, 10);
        house.base_center = Some((12, 12));
        house.alternate_base_center = (15, 15);
        sim.houses.insert(owner, house);
        let naval = sim.interner.intern("NAVAL");
        let land = sim.interner.intern("LAND");
        sim.production
            .ready_by_owner
            .entry(owner)
            .or_default()
            .extend([naval, land]);
        let mut ai = [AiPlayerState::new(owner)];
        let commands = tick_ai(&sim, &mut ai, &rules, Some(&path), &BTreeMap::new(), None);
        assert_eq!(
            commands.len(),
            1,
            "only naval placement bypasses the missing live-structure average"
        );
        assert!(
            matches!(commands[0].payload, Command::PlaceReadyBuilding { type_id, .. } if type_id == naval)
        );

        let mut blocked = sim;
        for cell in &mut blocked.resolved_terrain.as_mut().unwrap().cells {
            cell.speed_costs.float = Some(0);
        }
        let blocked_path =
            PathGrid::from_resolved_terrain(blocked.resolved_terrain.as_ref().unwrap());
        let mut ai = [AiPlayerState::new(owner)];
        let commands = tick_ai(
            &blocked,
            &mut ai,
            &rules,
            Some(&blocked_path),
            &BTreeMap::new(),
            None,
        );
        assert!(commands.is_empty());
        assert_eq!(
            blocked.production.ready_by_owner[&owner].front(),
            Some(&naval)
        );
    }
}
